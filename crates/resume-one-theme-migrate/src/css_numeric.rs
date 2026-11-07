//! Bounded numeric projection for a deliberately small, static CSS subset.
//!
//! This is not a CSS cascade, layout engine, custom-property resolver, or code
//! evaluator. Percentages use the supplied `em` reference; callers must only use
//! that convention where it describes the property they are projecting.

const MAX_INPUT_BYTES: usize = 8_192;
const MAX_DEPTH: usize = 32;
const MAX_NODES: usize = 512;
const MAX_FUNCTION_ARGS: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Dimension {
    Scalar,
    Length,
}

#[derive(Clone, Copy, Debug)]
struct Quantity {
    value: f64,
    dimension: Dimension,
}

impl Quantity {
    fn new(value: f64, dimension: Dimension) -> Option<Self> {
        value.is_finite().then_some(Self { value, dimension })
    }

    fn add(self, other: Self, subtract: bool) -> Option<Self> {
        if self.dimension != other.dimension {
            return None;
        }
        Self::new(
            if subtract {
                self.value - other.value
            } else {
                self.value + other.value
            },
            self.dimension,
        )
    }

    fn multiply(self, other: Self) -> Option<Self> {
        let dimension = match (self.dimension, other.dimension) {
            (Dimension::Scalar, Dimension::Scalar) => Dimension::Scalar,
            (Dimension::Scalar, Dimension::Length) | (Dimension::Length, Dimension::Scalar) => {
                Dimension::Length
            }
            (Dimension::Length, Dimension::Length) => return None,
        };
        Self::new(self.value * other.value, dimension)
    }

    fn divide(self, other: Self) -> Option<Self> {
        if other.dimension != Dimension::Scalar || other.value == 0.0 {
            return None;
        }
        Self::new(self.value / other.value, self.dimension)
    }

    fn compare(self, other: Self, minimum: bool) -> Option<Self> {
        if self.dimension != other.dimension {
            return None;
        }
        Self::new(
            if minimum {
                self.value.min(other.value)
            } else {
                self.value.max(other.value)
            },
            self.dimension,
        )
    }
}

struct Parser<'a> {
    input: &'a str,
    at: usize,
    nodes: usize,
    em: f64,
    rem: f64,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.input.as_bytes().get(self.at).copied()
    }

    fn whitespace(&mut self) -> bool {
        let start = self.at;
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r' | 0x0c)) {
            self.at += 1;
        }
        self.at > start
    }

    fn node(&mut self) -> Option<()> {
        self.nodes += 1;
        (self.nodes <= MAX_NODES).then_some(())
    }

    fn expect(&mut self, expected: u8) -> Option<()> {
        self.whitespace();
        if self.peek()? != expected {
            return None;
        }
        self.at += 1;
        Some(())
    }

    fn sum(&mut self, depth: usize) -> Option<Quantity> {
        let mut value = self.product(depth)?;
        loop {
            let separated = self.whitespace();
            let operator = match self.peek() {
                Some(operator @ (b'+' | b'-')) => operator,
                _ => return Some(value),
            };
            // CSS binary + and - require whitespace on both sides. A sign
            // attached to a number is instead consumed by `number` below.
            if !separated {
                return None;
            }
            self.at += 1;
            if !self.whitespace() {
                return None;
            }
            self.node()?;
            value = value.add(self.product(depth)?, operator == b'-')?;
        }
    }

    fn product(&mut self, depth: usize) -> Option<Quantity> {
        let mut value = self.primary(depth, true)?;
        loop {
            let checkpoint = self.at;
            self.whitespace();
            let operator = match self.peek() {
                Some(operator @ (b'*' | b'/')) => operator,
                _ => {
                    // Preserve the whitespace needed by a following sum.
                    self.at = checkpoint;
                    return Some(value);
                }
            };
            self.at += 1;
            self.whitespace();
            self.node()?;
            let other = self.primary(depth, true)?;
            value = if operator == b'*' {
                value.multiply(other)?
            } else {
                value.divide(other)?
            };
        }
    }

    fn primary(&mut self, depth: usize, allow_group: bool) -> Option<Quantity> {
        if depth > MAX_DEPTH {
            return None;
        }
        self.node()?;
        self.whitespace();
        match self.peek()? {
            b'(' if allow_group => {
                self.at += 1;
                let value = self.sum(depth + 1)?;
                self.expect(b')')?;
                Some(value)
            }
            b'a'..=b'z' | b'A'..=b'Z' => self.function(depth),
            b'0'..=b'9' | b'.' | b'+' | b'-' => self.number(),
            _ => None,
        }
    }

    fn function(&mut self, depth: usize) -> Option<Quantity> {
        let start = self.at;
        while self.peek().is_some_and(|b| b.is_ascii_alphabetic()) {
            self.at += 1;
        }
        let name = &self.input[start..self.at];
        // Whitespace between a name and '(' is not a CSS function token.
        if self.peek()? != b'(' {
            return None;
        }
        self.at += 1;
        if name.eq_ignore_ascii_case("calc") {
            let value = self.sum(depth + 1)?;
            self.expect(b')')?;
            Some(value)
        } else if name.eq_ignore_ascii_case("min") || name.eq_ignore_ascii_case("max") {
            let minimum = name.eq_ignore_ascii_case("min");
            let mut value = self.sum(depth + 1)?;
            let mut arguments = 1;
            loop {
                self.whitespace();
                match self.peek()? {
                    b')' => {
                        self.at += 1;
                        return Some(value);
                    }
                    b',' if arguments < MAX_FUNCTION_ARGS => {
                        self.at += 1;
                        arguments += 1;
                        value = value.compare(self.sum(depth + 1)?, minimum)?;
                    }
                    _ => return None,
                }
            }
        } else if name.eq_ignore_ascii_case("clamp") {
            let lower = self.sum(depth + 1)?;
            self.expect(b',')?;
            let preferred = self.sum(depth + 1)?;
            self.expect(b',')?;
            let upper = self.sum(depth + 1)?;
            self.expect(b')')?;
            // CSS clamp gives the minimum priority when its bounds cross.
            lower.compare(preferred.compare(upper, true)?, false)
        } else {
            None
        }
    }

    fn number(&mut self) -> Option<Quantity> {
        let start = self.at;
        if matches!(self.peek(), Some(b'+' | b'-')) {
            self.at += 1;
        }
        let digits = self.at;
        while self.peek().is_some_and(|b| b.is_ascii_digit()) {
            self.at += 1;
        }
        let mut has_digits = self.at > digits;
        if self.peek() == Some(b'.') {
            self.at += 1;
            let fraction = self.at;
            while self.peek().is_some_and(|b| b.is_ascii_digit()) {
                self.at += 1;
            }
            // A decimal point requires a following digit in a CSS number.
            if self.at == fraction {
                return None;
            }
            has_digits = true;
        }
        if !has_digits {
            return None;
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            let mut exponent = self.at + 1;
            if matches!(self.input.as_bytes().get(exponent), Some(b'+' | b'-')) {
                exponent += 1;
            }
            // The 'e' in 'em' is a unit, not an exponent. Only consume an
            // exponent if its first digit exists, as CSS tokenization does.
            if self
                .input
                .as_bytes()
                .get(exponent)
                .is_some_and(|b| b.is_ascii_digit())
            {
                self.at = exponent + 1;
                while self.peek().is_some_and(|b| b.is_ascii_digit()) {
                    self.at += 1;
                }
            }
        }
        let value: f64 = self.input[start..self.at].parse().ok()?;
        let unit_start = self.at;
        if self.peek() == Some(b'%') {
            self.at += 1;
        } else {
            while self.peek().is_some_and(|b| b.is_ascii_alphabetic()) {
                self.at += 1;
            }
        }
        let unit = &self.input[unit_start..self.at];
        if unit.is_empty() {
            return Quantity::new(value, Dimension::Scalar);
        }
        let scale = if unit.eq_ignore_ascii_case("rem") {
            self.rem
        } else if unit.eq_ignore_ascii_case("em") {
            self.em
        } else if unit.eq_ignore_ascii_case("pt") {
            1.0
        } else if unit.eq_ignore_ascii_case("px") {
            0.75
        } else if unit.eq_ignore_ascii_case("mm") {
            72.0 / 25.4
        } else if unit.eq_ignore_ascii_case("cm") {
            720.0 / 25.4
        } else if unit.eq_ignore_ascii_case("in") {
            72.0
        } else if unit.eq_ignore_ascii_case("pc") {
            12.0
        } else if unit == "%" {
            self.em / 100.0
        } else {
            return None;
        };
        // Reject a non-finite literal before multiplication as well: a zero
        // reference must never make an overflowing source value acceptable.
        if !value.is_finite() {
            return None;
        }
        Quantity::new(value * scale, Dimension::Length)
    }
}

/// Project a static CSS length expression to points without executing code.
///
/// Accepts pt/px/mm/cm/in/pc/em/rem/%, unitless zero, and nested calc/min/max/
/// clamp with dimension-aware arithmetic and CSS whitespace. Finite `em` and
/// `rem` references are required; `%` uses `em`, not viewport/container width.
/// Nonzero final scalars, unsupported units/functions, variables, comments,
/// strings, escapes, non-finite arithmetic, and excessive input are rejected.
/// Negative lengths are retained for the caller's property-specific validation.
/// This projects numbers only; it does not certify a declaration's CSS validity.
pub(crate) fn length_pt(input: &str, em: f64, rem: f64) -> Option<f64> {
    if input.len() > MAX_INPUT_BYTES || !input.is_ascii() || !em.is_finite() || !rem.is_finite() {
        return None;
    }
    let mut parser = Parser {
        input,
        at: 0,
        nodes: 0,
        em,
        rem,
    };
    let quantity = parser.primary(0, false)?;
    parser.whitespace();
    if parser.at != input.len()
        || (quantity.dimension == Dimension::Scalar && quantity.value != 0.0)
    {
        return None;
    }
    Some(quantity.value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(input: &str, em: f64, rem: f64, expected: f64) {
        let actual = length_pt(input, em, rem)
            .unwrap_or_else(|| panic!("expected a supported expression: {input:?}"));
        let tolerance = 1e-10 * expected.abs().max(1.0);
        assert!(
            (actual - expected).abs() <= tolerance,
            "{input:?}: expected {expected}, got {actual}"
        );
    }

    #[test]
    fn plain_units_keep_existing_point_conversions() {
        for (input, expected) in [
            ("12pt", 12.0),
            ("16px", 12.0),
            ("25.4mm", 72.0),
            ("2.54cm", 72.0),
            ("1in", 72.0),
            ("6pc", 72.0),
            ("1.5em", 15.0),
            ("1.5rem", 18.0),
            ("150%", 15.0),
            ("0", 0.0),
            ("-0", -0.0),
            ("+0.0", 0.0),
            ("-2px", -1.5),
            (".5PT", 0.5),
            ("  +2REM\t", 24.0),
        ] {
            close(input, 10.0, 12.0, expected);
        }
    }

    #[test]
    fn signed_decimals_and_scientific_notation_follow_css_number_tokens() {
        for (input, expected) in [
            ("1e2px", 75.0),
            ("+1E+2pt", 100.0),
            ("-2.5e-1in", -18.0),
            (".5e1em", 50.0),
            ("1e-2rem", 0.12),
            ("calc(2.5e1% + .5em)", 7.5),
        ] {
            close(input, 10.0, 12.0, expected);
        }
        for input in ["1.px", ".pt", "-.em", "1e+pt", "1e-pt", "--1pt", "+ 1px"] {
            assert_eq!(length_pt(input, 10.0, 12.0), None, "{input}");
        }
    }

    #[test]
    fn mixed_units_use_the_supplied_font_and_percentage_references() {
        close("calc(125% + 0.5em + 0.25rem - 2px)", 16.0, 12.0, 29.5);
        close("calc(125% + 0.5em + 0.25rem - 2px)", 20.0, 12.0, 36.5);
        close("calc(2rem - 1em)", 9.0, 14.0, 19.0);
        close("calc(50% + 2pt)", 0.0, 12.0, 2.0);
    }

    #[test]
    fn nested_arithmetic_preserves_precedence_and_left_associativity() {
        close("calc(2 * (1rem + calc(4px / 2)) - 1pt)", 10.0, 12.0, 26.0);
        close("calc(20px / 2 * 3)", 10.0, 12.0, 22.5);
        close("calc(10pt - 3pt - 2pt)", 10.0, 12.0, 5.0);
        close("calc((8 / 2) * 1pt + 2pt * (1 + 2))", 10.0, 12.0, 10.0);
        close("calc(1pt - -2pt + +3pt)", 10.0, 12.0, 6.0);
        close("calc(1pt / -2)", 10.0, 12.0, -0.5);
        close("calc(0 * 1rem)", 10.0, 12.0, 0.0);
    }

    #[test]
    fn comparison_functions_accept_nested_same_dimension_expressions() {
        close("min(20pt, 16px, 2rem)", 10.0, 12.0, 12.0);
        close(
            "max(2px, calc(1rem - 1pt), min(12pt, 1em))",
            10.0,
            12.0,
            11.0,
        );
        close("clamp(8pt, 125%, 20pt)", 12.0, 16.0, 15.0);
        close("clamp(8pt, 2pt, 20pt)", 12.0, 16.0, 8.0);
        close("clamp(8pt, 30pt, 20pt)", 12.0, 16.0, 20.0);
        close("clamp(20pt, 8pt, 10pt)", 12.0, 16.0, 20.0);
        close("calc(min(2, 3) * 4pt)", 10.0, 12.0, 8.0);
        close("calc(clamp(1, 3, 2) * max(1pt, 2pt))", 10.0, 12.0, 4.0);
        close("MIN(MAX(1PT, 2PT))", 10.0, 12.0, 2.0);
    }

    #[test]
    fn incompatible_dimensions_are_never_coerced_even_when_zero() {
        for input in [
            "calc(1pt * 2px)",
            "calc(0pt * 2px)",
            "calc(2pt / 1px)",
            "calc(1 / 2pt)",
            "calc(2pt / 0pt)",
            "calc(1pt + 2)",
            "calc(0 + 1pt)",
            "calc(0pt + 0)",
            "min(0, 1pt)",
            "max(1pt, 2)",
            "clamp(0, 1pt, 2pt)",
            "clamp(0pt, 1, 2pt)",
            "clamp(0pt, 1pt, 2)",
            "calc(2 * 3)",
            "min(1, 2)",
            "2",
        ] {
            assert_eq!(length_pt(input, 10.0, 12.0), None, "{input}");
        }
        close("calc(2 - 2)", 10.0, 12.0, 0.0);
    }

    #[test]
    fn css_whitespace_is_accepted_and_binary_sum_separators_are_required() {
        close("\t\r\n\x0ccalc(1pt\t+\n2pt\x0c-\r1pt) ", 10.0, 12.0, 2.0);
        close("calc(2pt*3/2)", 10.0, 12.0, 3.0);
        close("min(1pt,2pt)", 10.0, 12.0, 1.0);
        for input in [
            "calc(1pt+2pt)",
            "calc(1pt+ 2pt)",
            "calc(1pt +2pt)",
            "calc(1pt-2pt)",
            "calc(1pt- 2pt)",
            "calc(1pt -2pt)",
            "calc(1pt + + 2pt)",
            "calc (1pt)",
            "1 pt",
            "1 p x",
            "1pt 2pt",
            "(1pt)",
            "1pt + 2pt",
            "1pt\x0b",
            "1pt\u{a0}",
        ] {
            assert_eq!(length_pt(input, 10.0, 12.0), None, "{input:?}");
        }
    }

    #[test]
    fn arity_delimiters_and_complete_input_are_checked() {
        for input in [
            "",
            " ",
            "calc()",
            "calc(1pt, 2pt)",
            "min()",
            "min(,1pt)",
            "max(1pt,)",
            "min(1pt 2pt)",
            "clamp(1pt, 2pt)",
            "clamp(1pt, 2pt, 3pt, 4pt)",
            "calc((1pt)",
            "calc(1pt))",
            "calc(1pt) 2pt",
            "calc(1pt);",
            "1pt!important",
            "calc(*1pt)",
            "calc(1pt /)",
            "calc(- (1pt))",
        ] {
            assert_eq!(length_pt(input, 10.0, 12.0), None, "{input}");
        }
    }

    #[test]
    fn unsupported_context_and_code_like_input_fail_closed() {
        for input in [
            "1vw",
            "1vh",
            "1ch",
            "1fr",
            "1q",
            "var(--size)",
            "env(safe-area-inset-top)",
            "attr(size px)",
            "calc(1pt + var(--size, 2pt))",
            "round(1pt, 2pt)",
            "expression(alert(1))",
            "url(https://example.invalid/a)",
            "calc(1pt /*comment*/ + 2pt)",
            "calc(1pt/**/+/**/2pt)",
            "'1pt'",
            "\"1pt\"",
            "c\\61lc(1pt)",
            "calc(1pt + ${run()})",
            "calc(1pt + `2pt`)",
            "inherit",
            "auto",
        ] {
            assert_eq!(length_pt(input, 10.0, 12.0), None, "{input}");
        }
    }

    #[test]
    fn nonfinite_literals_references_and_intermediate_results_are_rejected() {
        for input in [
            "NaNpt",
            "infpt",
            "infinity",
            "calc(NaN * 1pt)",
            "1e309pt",
            "1e308in",
            "calc(1e308pt + 1e308pt)",
            "calc(1e308pt * 2)",
            "calc(1pt / 0)",
            "calc(1pt / -0)",
            "calc(1pt / (2 - 2))",
            "calc(1pt / 1e-999)",
            "calc(1e308pt / 1e-308)",
            "min(1pt, calc(1pt / 0))",
            "clamp(1pt, 2pt, 1e309pt)",
        ] {
            assert_eq!(length_pt(input, 10.0, 12.0), None, "{input}");
        }
        assert_eq!(length_pt("1e309em", 0.0, 12.0), None);
        for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(length_pt("1pt", invalid, 12.0), None);
            assert_eq!(length_pt("1pt", 10.0, invalid), None);
        }
        assert_eq!(length_pt("2em", f64::MAX, 12.0), None);
        close("calc(1e300pt / 1e300)", 10.0, 12.0, 1.0);
    }

    #[test]
    fn input_bytes_are_bounded_before_scanning() {
        let at_limit = format!("{}1pt", " ".repeat(MAX_INPUT_BYTES - 3));
        close(&at_limit, 10.0, 12.0, 1.0);
        let beyond_limit = format!(" {at_limit}");
        assert_eq!(length_pt(&beyond_limit, 10.0, 12.0), None);
    }

    #[test]
    fn nested_functions_and_groups_have_an_explicit_recursion_limit() {
        let at_limit = format!("{}1pt{}", "calc(".repeat(MAX_DEPTH), ")".repeat(MAX_DEPTH));
        close(&at_limit, 10.0, 12.0, 1.0);
        let too_deep = format!("calc({at_limit})");
        assert_eq!(length_pt(&too_deep, 10.0, 12.0), None);
        let groups = format!(
            "calc({}1pt{})",
            "(".repeat(MAX_DEPTH),
            ")".repeat(MAX_DEPTH)
        );
        assert_eq!(length_pt(&groups, 10.0, 12.0), None);
    }

    #[test]
    fn node_and_function_argument_budgets_are_enforced_separately() {
        // One function node, 256 literal nodes, and 255 operator nodes.
        let at_limit = format!("calc({})", vec!["1pt"; MAX_NODES / 2].join(" + "));
        close(&at_limit, 10.0, 12.0, 256.0);
        let too_many_nodes = format!("calc({})", vec!["1pt"; MAX_NODES / 2 + 1].join(" + "));
        assert_eq!(length_pt(&too_many_nodes, 10.0, 12.0), None);
        let at_arg_limit = format!("max({})", vec!["1pt"; MAX_FUNCTION_ARGS].join(","));
        close(&at_arg_limit, 10.0, 12.0, 1.0);
        let too_many_args = format!("max({})", vec!["1pt"; MAX_FUNCTION_ARGS + 1].join(","));
        assert_eq!(length_pt(&too_many_args, 10.0, 12.0), None);
    }
}
