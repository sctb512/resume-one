//! Bounded constant folding over the original JavaScript AST; never a JS runtime.
//!
//! Only unique, simple, top-level `const` bindings are candidates. Any same-name
//! binding anywhere (including parameters/imports/destructuring), or any write
//! anywhere, disqualifies that name, even in dead code or after the use. This is
//! deliberately more conservative than scope/control-flow analysis. Dynamic
//! scope, escaped identifiers and explicit global/eval/Function access reject
//! the whole table. Other unknown calls are NOT certified safe: a folded value
//! says nothing about module execution, CSS safety or migration completeness.
//!
//! The caller must supply a program and its original, unchanged parser buffer.
//! Nodes passed to `evaluate` must belong to that tree, not a re-parsed snippet.
//! Strings are returned as data, without quotes; escapes/control characters are
//! unsupported. Arithmetic is numeric-only: no concatenation or JS coercion.
//! Decimal IEEE-754 operations are bounded; unusually precise final decimals
//! are rejected rather than claiming full ECMAScript Number-to-string support.

use std::collections::{BTreeMap, BTreeSet};

use tree_sitter::Node;

const MAX_SOURCE_BYTES: usize = 8 * 1024 * 1024;
const MAX_AST_NODES: usize = 100_000;
const MAX_AST_DEPTH: usize = 256;
const MAX_BINDINGS: usize = 4_096;
const MAX_IDENTIFIER_BYTES: usize = 256;
const MAX_TEXT_BYTES: usize = 4_096;
const MAX_EVAL_DEPTH: usize = 64;
const MAX_EVAL_STEPS: usize = 1_024;
const MAX_NUMBER: f64 = 1_000_000_000_000.0;
const MIN_NONZERO_NUMBER: f64 = 0.000_001;

#[derive(Debug)]
struct Binding<'tree> {
    initializer: Node<'tree>,
    initialized_at: usize,
}

/// Borrows both the original tree and its source. No AST/snippet re-parsing.
#[derive(Debug)]
pub(crate) struct Constants<'tree, 'source> {
    root: Node<'tree>,
    source: &'source [u8],
    bindings: BTreeMap<String, Binding<'tree>>,
    declarations: BTreeMap<String, usize>,
    writes: BTreeSet<String>,
    rejection: Option<String>,
}

#[derive(Debug)]
enum Value {
    Text(String),
    Number(f64),
}

fn text<'source>(node: Node<'_>, source: &'source [u8]) -> Result<&'source str, String> {
    let bytes = source
        .get(node.byte_range())
        .ok_or("CONST_SOURCE: AST range is outside the original source")?;
    std::str::from_utf8(bytes).map_err(|_| "CONST_SOURCE: source is not UTF-8".into())
}

fn ascii_identifier(name: &str) -> bool {
    let mut bytes = name.bytes();
    matches!(bytes.next(), Some(b'a'..=b'z' | b'A'..=b'Z' | b'_' | b'$'))
        && bytes.all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'$'))
}

fn identifier(node: Node<'_>, source: &[u8]) -> Result<String, String> {
    let name = text(node, source)?;
    if name.len() > MAX_IDENTIFIER_BYTES || !ascii_identifier(name) {
        return Err("CONST_IDENTIFIER: only bounded unescaped ASCII names are supported".into());
    }
    Ok(name.into())
}

fn nodes(root: Node<'_>) -> Result<Vec<Node<'_>>, String> {
    let mut result = Vec::new();
    let mut pending = vec![(root, 0)];
    while let Some((node, depth)) = pending.pop() {
        if depth > MAX_AST_DEPTH || result.len() >= MAX_AST_NODES {
            return Err("CONST_AST_LIMIT: program node/depth budget exceeded".into());
        }
        if node.is_error() || node.is_missing() {
            return Err("CONST_SYNTAX: program contains an error or missing node".into());
        }
        result.push(node);
        if result.len() + pending.len() + node.named_child_count() > MAX_AST_NODES {
            return Err("CONST_AST_LIMIT: program node budget exceeded".into());
        }
        for index in (0..node.named_child_count()).rev() {
            if let Some(child) = node.named_child(index) {
                pending.push((child, depth + 1));
            }
        }
    }
    Ok(result)
}

/// Pattern positions only: object keys/default RHS values are not bindings.
/// In write mode a member's receiver is conservatively tainted, not its index.
fn pattern_names(root: Node<'_>, write: bool) -> Result<Vec<Node<'_>>, String> {
    let mut result = Vec::new();
    let mut pending = vec![root];
    let mut steps = 0;
    while let Some(node) = pending.pop() {
        steps += 1;
        if steps > MAX_AST_NODES {
            return Err("CONST_AST_LIMIT: binding pattern budget exceeded".into());
        }
        match node.kind() {
            "identifier" | "shorthand_property_identifier_pattern" => result.push(node),
            "comment" => {}
            "assignment_pattern" | "object_assignment_pattern" => {
                pending.push(required(node, "left")?);
            }
            "pair_pattern" => pending.push(required(node, "value")?),
            "member_expression" | "subscript_expression" if write => {
                let object = required(node, "object")?;
                // A call result cannot mutate a primitive const binding. Its
                // call/scope effects remain for the separate module analyzer.
                if matches!(
                    object.kind(),
                    "identifier"
                        | "member_expression"
                        | "subscript_expression"
                        | "parenthesized_expression"
                ) {
                    pending.push(object);
                }
            }
            "array_pattern"
            | "object_pattern"
            | "formal_parameters"
            | "rest_pattern"
            | "parenthesized_expression" => {
                for index in 0..node.named_child_count() {
                    if let Some(child) = node.named_child(index) {
                        pending.push(child);
                    }
                }
            }
            other => {
                return Err(format!(
                    "CONST_PATTERN: unsupported binding/write pattern {other}"
                ));
            }
        }
    }
    Ok(result)
}

fn required<'tree>(node: Node<'tree>, field: &str) -> Result<Node<'tree>, String> {
    node.child_by_field_name(field)
        .ok_or_else(|| format!("CONST_AST: {} lacks {field}", node.kind()))
}

fn top_const(declarator: Node<'_>) -> bool {
    let Some(declaration) = declarator.parent() else {
        return false;
    };
    if declaration.kind() != "lexical_declaration"
        || !declaration
            .child_by_field_name("kind")
            .is_some_and(|n| n.kind() == "const")
    {
        return false;
    }
    declaration.parent().is_some_and(|parent| {
        parent.kind() == "program"
            || parent.kind() == "export_statement"
                && parent.parent().is_some_and(|p| p.kind() == "program")
    })
}

impl<'tree, 'source> Constants<'tree, 'source> {
    /// A failed scan stays fail-closed: every evaluation returns its reason.
    pub(crate) fn from_program(root: Node<'tree>, source: &'source [u8]) -> Self {
        let mut result = Self {
            root,
            source,
            bindings: BTreeMap::new(),
            declarations: BTreeMap::new(),
            writes: BTreeSet::new(),
            rejection: None,
        };
        if let Err(reason) = result.scan() {
            result.bindings.clear();
            result.rejection = Some(reason);
        }
        result
    }

    fn declare(&mut self, pattern: Node<'tree>) -> Result<(), String> {
        for node in pattern_names(pattern, false)? {
            let name = identifier(node, self.source)?;
            *self.declarations.entry(name).or_default() += 1;
            if self.declarations.len() > MAX_BINDINGS {
                return Err("CONST_BINDING_LIMIT: too many distinct bindings".into());
            }
        }
        Ok(())
    }

    fn mark_write(&mut self, target: Node<'tree>) -> Result<(), String> {
        for node in pattern_names(target, true)? {
            self.writes.insert(identifier(node, self.source)?);
            if self.writes.len() > MAX_BINDINGS {
                return Err("CONST_BINDING_LIMIT: too many distinct write targets".into());
            }
        }
        Ok(())
    }

    fn scan(&mut self) -> Result<(), String> {
        if self.source.len() > MAX_SOURCE_BYTES || std::str::from_utf8(self.source).is_err() {
            return Err("CONST_SOURCE: source exceeds 8 MiB or is not UTF-8".into());
        }
        if self.root.kind() != "program" || self.root.parent().is_some() || self.root.has_error() {
            return Err("CONST_SYNTAX: expected a complete error-free program root".into());
        }
        for node in nodes(self.root)? {
            let kind = node.kind();
            if matches!(kind, "with_statement" | "this") {
                return Err(
                    "CONST_GLOBAL_SCOPE: dynamic/global scope requires separate analysis".into(),
                );
            }
            if matches!(
                kind,
                "identifier"
                    | "property_identifier"
                    | "shorthand_property_identifier"
                    | "shorthand_property_identifier_pattern"
            ) {
                let name = text(node, self.source)?;
                if name.contains('\\') {
                    return Err(
                        "CONST_IDENTIFIER: escaped identifiers can conceal bindings/eval".into(),
                    );
                }
                if matches!(
                    name,
                    "eval" | "Function" | "globalThis" | "window" | "self" | "global"
                ) {
                    return Err("CONST_GLOBAL_SCOPE: eval/Function/global access requires separate analysis".into());
                }
            }
            match kind {
                "variable_declarator" => {
                    let name = required(node, "name")?;
                    self.declare(name)?;
                    if name.kind() == "identifier"
                        && top_const(node)
                        && let Some(initializer) = node.child_by_field_name("value")
                    {
                        self.bindings
                            .entry(identifier(name, self.source)?)
                            .or_insert(Binding {
                                initializer,
                                initialized_at: node.end_byte(),
                            });
                    }
                }
                "formal_parameters" => self.declare(node)?,
                "function_declaration"
                | "generator_function_declaration"
                | "function_expression"
                | "generator_function"
                | "class_declaration"
                | "class" => {
                    if let Some(name) = node.child_by_field_name("name") {
                        self.declare(name)?;
                    }
                }
                "arrow_function" | "catch_clause" => {
                    if let Some(parameter) = node.child_by_field_name("parameter") {
                        self.declare(parameter)?;
                    }
                }
                "import_clause" | "namespace_import" => {
                    for index in 0..node.named_child_count() {
                        if let Some(name) =
                            node.named_child(index).filter(|n| n.kind() == "identifier")
                        {
                            self.declare(name)?;
                        }
                    }
                }
                "import_specifier" => {
                    let local = node
                        .child_by_field_name("alias")
                        .or_else(|| node.child_by_field_name("name"));
                    if let Some(local) = local {
                        self.declare(local)?;
                    }
                }
                "assignment_expression" | "augmented_assignment_expression" => {
                    self.mark_write(required(node, "left")?)?;
                }
                "update_expression" => self.mark_write(required(node, "argument")?)?,
                "unary_expression"
                    if node
                        .child_by_field_name("operator")
                        .is_some_and(|n| n.kind() == "delete") =>
                {
                    self.mark_write(required(node, "argument")?)?;
                }
                "for_in_statement" => {
                    let left = required(node, "left")?;
                    let declaration = (0..node.child_count())
                        .filter_map(|i| node.child(i))
                        .any(|n| matches!(n.kind(), "const" | "let" | "var" | "using"));
                    if declaration {
                        self.declare(left)?;
                    } else {
                        self.mark_write(left)?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// `before` is normally the interpolation expression's start byte (or an
    /// earlier template boundary). A later offset cannot bypass lexical TDZ.
    pub(crate) fn evaluate(&self, node: Node<'tree>, before: usize) -> Result<String, String> {
        if let Some(reason) = &self.rejection {
            return Err(reason.clone());
        }
        if before > node.start_byte() || before > self.source.len() {
            return Err("CONST_BEFORE: cutoff must not be after the expression start".into());
        }
        let mut ancestor = Some(node);
        let mut belongs = false;
        for _ in 0..=MAX_AST_DEPTH {
            let Some(current) = ancestor else { break };
            if current == self.root {
                belongs = true;
                break;
            }
            ancestor = current.parent();
        }
        if !belongs {
            return Err("CONST_AST: expression must belong to the original program tree".into());
        }
        let mut steps = MAX_EVAL_STEPS;
        match self.value(node, before, 0, &mut steps)? {
            Value::Text(value) => Ok(value),
            Value::Number(value) => number_text(value),
        }
    }

    fn value(
        &self,
        node: Node<'tree>,
        before: usize,
        depth: usize,
        steps: &mut usize,
    ) -> Result<Value, String> {
        if depth >= MAX_EVAL_DEPTH || *steps == 0 {
            return Err(
                "CONST_EVAL_LIMIT: expression/reference depth or work budget exceeded".into(),
            );
        }
        *steps -= 1;
        match node.kind() {
            "string" => string_value(node, self.source).map(Value::Text),
            "number" => decimal(text(node, self.source)?).map(Value::Number),
            "identifier" => {
                let name = identifier(node, self.source)?;
                let binding = self.bindings.get(&name).ok_or_else(|| {
                    format!("CONST_BINDING: {name} is not a simple top-level const")
                })?;
                if self.declarations.get(&name) != Some(&1) {
                    return Err(format!(
                        "CONST_SHADOW: {name} has another declaration/parameter/import"
                    ));
                }
                if self.writes.contains(&name) {
                    return Err(format!(
                        "CONST_MUTATION: {name} has an assignment/update/write"
                    ));
                }
                if binding.initialized_at > before {
                    return Err(format!(
                        "CONST_TDZ: {name} is not initialized before byte {before}"
                    ));
                }
                // An alias is evaluated where it was initialized, not at the
                // eventual interpolation use. This also rejects forward cycles.
                self.value(
                    binding.initializer,
                    binding.initializer.start_byte(),
                    depth + 1,
                    steps,
                )
            }
            "parenthesized_expression" => {
                let children: Vec<_> = (0..node.named_child_count())
                    .filter_map(|i| node.named_child(i))
                    .filter(|n| n.kind() != "comment")
                    .collect();
                if children.len() != 1 {
                    return Err("CONST_AST: expected one parenthesized expression".into());
                }
                self.value(children[0], before, depth + 1, steps)
            }
            "unary_expression" => {
                let operator = text(required(node, "operator")?, self.source)?;
                if !matches!(operator, "+" | "-") {
                    return Err(format!("CONST_OPERATOR: unsupported unary {operator}"));
                }
                let argument = self.value(required(node, "argument")?, before, depth + 1, steps)?;
                let Value::Number(value) = argument else {
                    return Err("CONST_TYPE: unary arithmetic never coerces strings".into());
                };
                bounded(if operator == "-" { -value } else { value }).map(Value::Number)
            }
            "binary_expression" => {
                let operator = text(required(node, "operator")?, self.source)?;
                if !matches!(operator, "+" | "-" | "*" | "/") {
                    return Err(format!("CONST_OPERATOR: unsupported binary {operator}"));
                }
                let left = self.value(required(node, "left")?, before, depth + 1, steps)?;
                let right = self.value(required(node, "right")?, before, depth + 1, steps)?;
                let (Value::Number(left), Value::Number(right)) = (left, right) else {
                    return Err(
                        "CONST_TYPE: arithmetic requires two numbers; no concatenation/coercion"
                            .into(),
                    );
                };
                if operator == "/" && right == 0.0 {
                    return Err("CONST_DIV_ZERO: division by zero is unsupported".into());
                }
                bounded(match operator {
                    "+" => left + right,
                    "-" => left - right,
                    "*" => left * right,
                    "/" => left / right,
                    _ => unreachable!(),
                })
                .map(Value::Number)
            }
            other => Err(format!(
                "CONST_UNSUPPORTED: {other}; functions/props/members/calls/templates are not evaluated"
            )),
        }
    }
}

fn string_value(node: Node<'_>, source: &[u8]) -> Result<String, String> {
    let raw = text(node, source)?;
    let quote = raw.as_bytes().first().copied();
    if raw.len() < 2
        || !matches!(quote, Some(b'\'' | b'"'))
        || raw.as_bytes().last().copied() != quote
    {
        return Err("CONST_STRING: invalid quoted AST literal".into());
    }
    let inner = &raw[1..raw.len() - 1];
    if inner.len() > MAX_TEXT_BYTES {
        return Err("CONST_STRING_LIMIT: literal exceeds 4096 bytes".into());
    }
    if inner.contains('\\')
        || inner
            .chars()
            .any(|c| c.is_control() || matches!(c, '\u{2028}' | '\u{2029}'))
    {
        return Err("CONST_STRING_ESCAPE: escaped/control-bearing strings are unsupported".into());
    }
    Ok(inner.into())
}

/// Decimal grammar only: no legacy octal, base prefixes, separators or BigInt.
fn decimal(raw: &str) -> Result<f64, String> {
    let bytes = raw.as_bytes();
    if bytes.is_empty() || bytes.len() > 64 {
        return Err("CONST_NUMBER: empty/oversized numeric literal".into());
    }
    let mut at = 0;
    while bytes.get(at).is_some_and(u8::is_ascii_digit) {
        at += 1;
    }
    let integer_digits = at;
    if integer_digits > 1 && bytes[0] == b'0' {
        return Err("CONST_NUMBER: leading-zero/legacy octal literals are unsupported".into());
    }
    if bytes.get(at) == Some(&b'.') {
        at += 1;
        while bytes.get(at).is_some_and(u8::is_ascii_digit) {
            at += 1;
        }
    }
    if at == 0 || integer_digits == 0 && at == 1 {
        return Err("CONST_NUMBER: expected decimal digits".into());
    }
    if matches!(bytes.get(at), Some(b'e' | b'E')) {
        at += 1;
        if matches!(bytes.get(at), Some(b'+' | b'-')) {
            at += 1;
        }
        let exponent_start = at;
        while bytes.get(at).is_some_and(u8::is_ascii_digit) {
            at += 1;
        }
        if at == exponent_start {
            return Err("CONST_NUMBER: missing exponent digits".into());
        }
    }
    if at != bytes.len() {
        return Err("CONST_NUMBER: only unseparated decimal Number literals are supported".into());
    }
    let number: f64 = raw
        .parse()
        .map_err(|_| "CONST_NUMBER: invalid decimal literal")?;
    // parse() could underflow a nonzero literal to zero; reject that loss.
    let mantissa = raw.split(['e', 'E']).next().unwrap_or_default();
    if number == 0.0 && mantissa.bytes().any(|b| matches!(b, b'1'..=b'9')) {
        return Err("CONST_NUMBER_RANGE: nonzero literal underflowed to zero".into());
    }
    bounded(number)
}

fn bounded(number: f64) -> Result<f64, String> {
    if !number.is_finite()
        || number.abs() > MAX_NUMBER
        || number != 0.0 && number.abs() < MIN_NONZERO_NUMBER
    {
        return Err("CONST_NUMBER_RANGE: require finite zero or magnitude 1e-6..1e12".into());
    }
    Ok(number)
}

fn number_text(number: f64) -> Result<String, String> {
    bounded(number)?;
    // ECMAScript String(-0) is "0". The bounded exponent range uses fixed
    // decimal notation in JS. Require <=12 significant decimal digits so a
    // short decimal is unique well within binary64's precision; reject long
    // rounding tails instead of approximating JS's complete formatting rules.
    if number == 0.0 {
        return Ok("0".into());
    }
    let value = number.to_string();
    let digits: String = value.chars().filter(char::is_ascii_digit).collect();
    if value.contains(['e', 'E']) || digits.trim_matches('0').len() > 12 {
        return Err(
            "CONST_NUMBER_PRECISION: final Number formatting exceeds supported precision".into(),
        );
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tree_sitter::{Parser, Tree};

    fn parse(source: &str) -> Tree {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_javascript::LANGUAGE.into())
            .unwrap();
        parser.parse(source, None).unwrap()
    }

    fn substitutions(tree: &Tree) -> Vec<Node<'_>> {
        nodes(tree.root_node())
            .unwrap()
            .into_iter()
            .filter(|n| n.kind() == "template_substitution")
            .map(|n| n.named_child(0).unwrap())
            .collect()
    }

    fn is_styled_substitution(node: Node<'_>, source: &[u8]) -> bool {
        let Some(template) = node.parent().and_then(|n| n.parent()) else {
            return false;
        };
        if template.kind() != "template_string" {
            return false;
        }
        let Some(function) = template
            .parent()
            .and_then(|n| n.child_by_field_name("function"))
        else {
            return false;
        };
        let base = match function.kind() {
            "member_expression" => function.child_by_field_name("object"),
            "call_expression" => function.child_by_field_name("function"),
            _ => None,
        };
        base.is_some_and(|n| n.kind() == "identifier" && text(n, source) == Ok("styled"))
    }

    fn result(source: &str) -> Result<String, String> {
        let tree = parse(source);
        let table = Constants::from_program(tree.root_node(), source.as_bytes());
        let node = substitutions(&tree)[0];
        table.evaluate(node, node.start_byte())
    }

    fn refuses(source: &str, reason: &str) {
        let error = result(source).unwrap_err();
        assert!(
            error.contains(reason),
            "expected {reason}, got {error}; {source}"
        );
    }

    #[test]
    fn literal_colors_fonts_and_exported_aliases() {
        assert_eq!(
            result("const INK='#1f2421'; const C=styled.div`color:${INK};`;"),
            Ok("#1f2421".into())
        );
        assert_eq!(
            result(
                "export const FONT=\"'Spectral', Georgia, serif\"; const ALIAS=FONT; const C=s.div`font-family:${ALIAS}`;"
            ),
            Ok("'Spectral', Georgia, serif".into())
        );
        assert_eq!(
            result("const C=s.div`color:${'#c9a24b'}`;"),
            Ok("#c9a24b".into())
        );
    }

    #[test]
    fn berlin_grid_multiplication_and_numeric_precedence() {
        assert_eq!(
            result("const GRID_SIZE=8; const C=s.div`padding:${GRID_SIZE * 10}px`;"),
            Ok("80".into())
        );
        assert_eq!(
            result("const A=8, B=A*6; const C=s.div`margin:${(B + 4) / 2 - -1}px`;"),
            Ok("27".into())
        );
        assert_eq!(
            result("const C=s.div`opacity:${+.5 * 2 / 4}`;"),
            Ok("0.25".into())
        );
        assert_eq!(result("const C=s.div`x:${-0}`;"), Ok("0".into()));
        assert_eq!(result("const C=s.div`x:${1e3}`;"), Ok("1000".into()));
    }

    #[test]
    fn lexical_tdz_forward_aliases_self_reference_and_cutoff() {
        refuses("const C=s.div`x:${A}`; const A=8;", "CONST_TDZ");
        refuses("const A=B; const B=8; const C=s.div`x:${A}`;", "CONST_TDZ");
        refuses("const A=A; const C=s.div`x:${A}`;", "CONST_TDZ");
        let source = "const A=8; const C=s.div`x:${A}`;";
        let tree = parse(source);
        let table = Constants::from_program(tree.root_node(), source.as_bytes());
        let node = substitutions(&tree)[0];
        assert!(table.evaluate(node, 0).unwrap_err().contains("CONST_TDZ"));
        assert!(
            table
                .evaluate(node, source.len())
                .unwrap_err()
                .contains("CONST_BEFORE")
        );
    }

    #[test]
    fn assignments_updates_destructuring_and_for_writes_taint_all_uses() {
        for write in [
            "A=9;",
            "A+=1;",
            "A++;",
            "--A;",
            "({A}=value);",
            "[A]=value;",
            "for(A of values){}",
            "function later(){A=2;}",
        ] {
            refuses(
                &format!("const A=8; const C=s.div`x:${{A}}`; {write}"),
                "CONST_MUTATION",
            );
        }
    }

    #[test]
    fn same_name_bindings_parameters_imports_and_shadowing_are_rejected() {
        for other in [
            "{const A=9;}",
            "function f(A){}",
            "const f=A=>A;",
            "try{}catch(A){}",
            "function A(){}",
            "class A{}",
            "const {x:A}=value;",
            "function f({A=9}){}",
            "for(const A of values){}",
            "import {x as A} from 'x';",
            "import A from 'x';",
            "import * as A from 'x';",
        ] {
            refuses(
                &format!("const A=8; {other} const C=s.div`x:${{A}}`;"),
                "CONST_SHADOW",
            );
        }
        refuses("let A=8; const C=s.div`x:${A}`;", "CONST_BINDING");
        refuses("{const A=8;} const C=s.div`x:${A}`;", "CONST_BINDING");
        refuses("const {A}=value; const C=s.div`x:${A}`;", "CONST_BINDING");
        // A property key/import's remote name is not a local A binding.
        assert_eq!(
            result(
                "const A=8; const {A:B}=value; import {A as D} from 'x'; const C=s.div`x:${A}`;"
            ),
            Ok("8".into())
        );
    }

    #[test]
    fn eval_global_scope_and_escaped_identifiers_fail_closed() {
        for effect in [
            "eval('var A=9');",
            "(0,eval)('A=9');",
            "globalThis.eval('A=9');",
            "window['eval']('A=9');",
            "const run=Function;",
            "with(object){}",
        ] {
            refuses(
                &format!("const A=8; {effect} const C=s.div`x:${{A}}`;"),
                "CONST_GLOBAL_SCOPE",
            );
        }
        refuses(
            r"const A=8; function f(\u0041){} const C=s.div`x:${A}`;",
            "CONST_IDENTIFIER",
        );
    }

    #[test]
    fn fake_declarations_and_mutations_in_strings_or_comments_are_not_code() {
        refuses(
            "const fake=\"const A=99;\"; /* const A=2 */ const C=s.div`x:${A}`;",
            "CONST_BINDING",
        );
        assert_eq!(
            result("const A=8; const fake=\"A++; eval('oops')\"; // A=2\nconst C=s.div`x:${A}`;"),
            Ok("8".into())
        );
        assert_eq!(
            result("const A='${process.exit()}'; const C=s.div`content:${A}`;"),
            Ok("${process.exit()}".into())
        );
    }

    #[test]
    fn functions_calls_props_getters_templates_and_injection_remain_unsupported() {
        for expr in [
            "props=>props.color",
            "function(){return 8}",
            "props.color",
            "({get x(){throw 1}}).x",
            "process.exit()",
            "import('x')",
            "calc(8*2)",
            "`red`",
            "(A,8)",
            "A?1:2",
            "8**2",
        ] {
            let source = format!("const A=8; const C=s.div`x:${{{expr}}}`;");
            assert!(result(&source).is_err(), "unexpectedly accepted {expr}");
        }
    }

    #[test]
    fn strings_are_not_coerced_or_concatenated() {
        for expr in [
            "'8'+2", "2+'8'", "'a'+'b'", "'8'*2", "+'8'", "true+1", "null+1",
        ] {
            assert!(result(&format!("const C=s.div`x:${{{expr}}}`;")).is_err());
        }
        refuses(
            r"const A='\x23fff'; const C=s.div`x:${A}`;",
            "CONST_STRING_ESCAPE",
        );
    }

    #[test]
    fn division_nonfinite_huge_tiny_and_precision_losses_are_rejected() {
        refuses("const C=s.div`x:${8/0}`;", "CONST_DIV_ZERO");
        refuses("const C=s.div`x:${0/-0}`;", "CONST_DIV_ZERO");
        for expr in ["1e309", "1e13", "1e12*2", "1e-7", "1e-9999"] {
            refuses(
                &format!("const C=s.div`x:${{{expr}}}`;"),
                "CONST_NUMBER_RANGE",
            );
        }
        for expr in ["0xff", "0o10", "0b10", "010", "1_000", "8n"] {
            refuses(&format!("const C=s.div`x:${{{expr}}}`;"), "CONST_NUMBER");
        }
        refuses("const C=s.div`x:${1/3}`;", "CONST_NUMBER_PRECISION");
        refuses("const C=s.div`x:${0.1+0.2}`;", "CONST_NUMBER_PRECISION");
    }

    #[test]
    fn nested_expressions_and_exponential_alias_expansion_are_bounded() {
        let expression = format!("{}1{}", "(".repeat(70), ")".repeat(70));
        refuses(
            &format!("const C=s.div`x:${{{expression}}}`;"),
            "CONST_EVAL_LIMIT",
        );
        let mut source = String::from("const A0=1;");
        for i in 1..20 {
            source.push_str(&format!("const A{i}=A{}+A{};", i - 1, i - 1));
        }
        source.push_str("const C=s.div`x:${A19}`;");
        refuses(&source, "CONST_EVAL_LIMIT");
        refuses(
            &format!("const A='{}'; const C=s.div`x:${{A}}`;", "x".repeat(4097)),
            "CONST_STRING_LIMIT",
        );
    }

    #[test]
    fn syntax_errors_foreign_trees_and_partial_roots_are_not_salvaged() {
        let source = "const A=8; const C=s.div`x:${A}`;";
        let tree = parse(source);
        let other = parse(source);
        let table = Constants::from_program(tree.root_node(), source.as_bytes());
        let node = substitutions(&other)[0];
        assert!(
            table
                .evaluate(node, node.start_byte())
                .unwrap_err()
                .contains("CONST_AST")
        );
        let partial =
            Constants::from_program(tree.root_node().named_child(0).unwrap(), source.as_bytes());
        assert!(
            partial
                .evaluate(substitutions(&tree)[0], 0)
                .unwrap_err()
                .contains("CONST_SYNTAX")
        );
        let broken = "const A=8; const C=s.div`x:${A}`; const = ;";
        let bad_tree = parse(broken);
        let bad_table = Constants::from_program(bad_tree.root_node(), broken.as_bytes());
        assert!(
            bad_table
                .evaluate(bad_tree.root_node(), 0)
                .unwrap_err()
                .contains("CONST_SYNTAX")
        );
    }

    #[test]
    fn unknown_calls_do_not_turn_constant_folding_into_a_module_safety_claim() {
        assert_eq!(
            result("const A=8; untrusted(); const C=s.div`x:${A}`;"),
            Ok("8".into())
        );
    }

    /// Optional static integration check; never executes these source files.
    #[test]
    #[ignore = "requires RESUME_ONE_PINNED_SOURCE_ROOT pointing to the catalog cache"]
    fn pinned_berlin_field_and_art_deco_interpolations() {
        let root = std::path::PathBuf::from(
            std::env::var("RESUME_ONE_PINNED_SOURCE_ROOT").expect("set pinned source root"),
        );
        for (slug, expected_ok, expected_unsupported) in [
            ("berlin-grid", 46, 0),
            ("field-researcher", 51, 0),
            ("art-deco", 33, 2),
        ] {
            let source = std::fs::read_to_string(root.join(slug).join("src/Resume.jsx")).unwrap();
            let tree = parse(&source);
            let table = Constants::from_program(tree.root_node(), source.as_bytes());
            let mut ok = 0;
            let mut unsupported = 0;
            for node in substitutions(&tree)
                .into_iter()
                .filter(|n| is_styled_substitution(*n, source.as_bytes()))
            {
                match table.evaluate(node, node.start_byte()) {
                    Ok(value) => {
                        assert!(!value.is_empty());
                        ok += 1;
                    }
                    Err(reason) if reason.starts_with("CONST_UNSUPPORTED: arrow_function") => {
                        unsupported += 1
                    }
                    Err(reason) => panic!(
                        "{slug}: {}: {reason}",
                        text(node, source.as_bytes()).unwrap()
                    ),
                }
            }
            assert_eq!(
                (ok, unsupported),
                (expected_ok, expected_unsupported),
                "{slug}"
            );
            println!(
                "{slug}: {ok} static styled values, {unsupported} unsupported props functions"
            );
        }
    }
}
