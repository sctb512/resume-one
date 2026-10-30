//! Bounded, deterministic `short` / `en-US` JSON Resume date-range formatting.
//!
//! Behavior reference: jsonresume/jsonresume.org, revision
//! `542cd5315fc9995fca3c401d0c818767530bb6bd`, `packages/utils/src/dates.js`
//! (`formatDateRange`, MIT); source SHA-256:
//! `4fb77d24f9a3ea8e2a2855a92d4d9a9c8599e9a6f81aa23bf75da862265287fd`.
//!
//! Only exact ASCII `YYYY`, `YYYY-MM`, and `YYYY-MM-DD` are parsed. Omitted
//! month/day mean January/1; days are validated and then omitted from output.
//! Month names, numeric years, `Present`, and the ASCII ` - ` separator match
//! the source's default English date-only path without a clock or timezone.
//! Year zero is the ISO astronomical year (Gregorian 1 BC); the source asks
//! Intl for a numeric year without an era, so both `0000` and `0001` show `1`.
//!
//! Deliberate bounds/differences from JavaScript Date / Intl:
//! - Strict proleptic Gregorian validation preserves impossible dates verbatim
//!   instead of accepting JavaScript engines' possible overflow normalization.
//! - Timestamps, timezone offsets, expanded/signed years, nonstandard date
//!   strings and surrounding whitespace are preserved verbatim, not parsed.
//!   The source can parse some of these and format them in a local timezone.
//! - There is no locale, numbering-system, long/numeric format, custom present
//!   label, Date-object input, or other Intl support. No JSON normalization or
//!   field-presence inference is performed here; the semantic projection owns that.

/// Preserve the source's distinction between omitted and explicitly ongoing ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RangeEnd<'a> {
    /// An absent/undefined end produces only the formatted start.
    Missing,
    /// An explicit null/empty end produces `start - Present`.
    Present,
    /// A supplied end is formatted; an empty value also means `Present`.
    Value(&'a str),
}

/// Format a bounded English date range without trimming or reordering inputs.
///
/// An empty start always suppresses the whole range, even with a supplied end.
/// Invalid or unsupported strings are retained exactly, including whitespace.
pub(crate) fn short_en_range(start: &str, end: RangeEnd<'_>) -> String {
    if start.is_empty() {
        return String::new();
    }

    let start = short_en_date(start);
    match end {
        RangeEnd::Missing => start,
        RangeEnd::Present | RangeEnd::Value("") => format!("{start} - Present"),
        RangeEnd::Value(end) => format!("{start} - {}", short_en_date(end)),
    }
}

const SHORT_MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

fn short_en_date(value: &str) -> String {
    let Some((year, month)) = date_only(value) else {
        return value.to_owned();
    };

    // Intl's Gregorian numeric year is the year of era, not a zero-padded ISO year.
    let year_of_era = year.max(1);
    format!("{} {year_of_era}", SHORT_MONTHS[usize::from(month - 1)])
}

/// Validate the complete date, even though the caller displays only month/year.
fn date_only(value: &str) -> Option<(u16, u16)> {
    let bytes = value.as_bytes();
    if !matches!(bytes.len(), 4 | 7 | 10) {
        return None;
    }

    let year = decimal(&bytes[..4])?;
    let month = if bytes.len() >= 7 {
        if bytes[4] != b'-' {
            return None;
        }
        decimal(&bytes[5..7])?
    } else {
        1
    };
    let day = if bytes.len() == 10 {
        if bytes[7] != b'-' {
            return None;
        }
        decimal(&bytes[8..10])?
    } else {
        1
    };

    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days_in_month = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return None,
    };
    if day == 0 || day > days_in_month {
        return None;
    }

    Some((year, month))
}

fn decimal(digits: &[u8]) -> Option<u16> {
    digits.iter().try_fold(0u16, |value, &digit| {
        if !digit.is_ascii_digit() {
            return None;
        }
        value.checked_mul(10)?.checked_add(u16::from(digit - b'0'))
    })
}

#[cfg(test)]
mod tests {
    use super::{RangeEnd, short_en_range};

    #[test]
    fn formats_all_short_english_months() {
        for (input, expected) in [
            ("2024-01", "Jan 2024"),
            ("2024-02", "Feb 2024"),
            ("2024-03", "Mar 2024"),
            ("2024-04", "Apr 2024"),
            ("2024-05", "May 2024"),
            ("2024-06", "Jun 2024"),
            ("2024-07", "Jul 2024"),
            ("2024-08", "Aug 2024"),
            ("2024-09", "Sep 2024"),
            ("2024-10", "Oct 2024"),
            ("2024-11", "Nov 2024"),
            ("2024-12", "Dec 2024"),
        ] {
            assert_eq!(
                short_en_range(input, RangeEnd::Missing),
                expected,
                "{input}"
            );
        }
    }

    #[test]
    fn year_only_means_january_and_numeric_year_is_not_zero_padded() {
        for (input, expected) in [
            ("2020", "Jan 2020"),
            ("0001", "Jan 1"),
            ("0099", "Jan 99"),
            ("0100", "Jan 100"),
            ("0999", "Jan 999"),
            ("9999", "Jan 9999"),
        ] {
            assert_eq!(
                short_en_range(input, RangeEnd::Missing),
                expected,
                "{input}"
            );
        }
    }

    #[test]
    fn astronomical_year_zero_has_numeric_year_of_era_one() {
        assert_eq!(short_en_range("0000", RangeEnd::Missing), "Jan 1");
        assert_eq!(short_en_range("0000-02-29", RangeEnd::Missing), "Feb 1");
        assert_eq!(
            short_en_range("0001-02-29", RangeEnd::Missing),
            "0001-02-29"
        );
    }

    #[test]
    fn missing_end_is_a_single_date_not_present() {
        assert_eq!(short_en_range("2020-06-01", RangeEnd::Missing), "Jun 2020");
        assert_eq!(
            short_en_range("unavailable", RangeEnd::Missing),
            "unavailable"
        );
    }

    #[test]
    fn explicit_present_and_empty_end_are_ongoing() {
        assert_eq!(
            short_en_range("2020-06", RangeEnd::Present),
            "Jun 2020 - Present"
        );
        assert_eq!(
            short_en_range("2020-06", RangeEnd::Value("")),
            "Jun 2020 - Present"
        );
        assert_eq!(
            short_en_range("unknown", RangeEnd::Present),
            "unknown - Present"
        );
    }

    #[test]
    fn empty_start_suppresses_every_end() {
        for end in [
            RangeEnd::Missing,
            RangeEnd::Present,
            RangeEnd::Value("2024-12-31"),
            RangeEnd::Value(""),
            RangeEnd::Value("unknown"),
        ] {
            assert_eq!(short_en_range("", end), "");
        }
    }

    #[test]
    fn valid_days_are_checked_then_omitted_without_month_shift() {
        for (input, expected) in [
            ("2024-01-01", "Jan 2024"),
            ("2024-12-31", "Dec 2024"),
            ("2024-04-30", "Apr 2024"),
            ("2024-06-30", "Jun 2024"),
            ("2024-09-30", "Sep 2024"),
            ("2024-11-30", "Nov 2024"),
            ("2023-02-28", "Feb 2023"),
            ("2024-02-29", "Feb 2024"),
            ("1900-02-28", "Feb 1900"),
            ("2000-02-29", "Feb 2000"),
            ("2100-02-28", "Feb 2100"),
            ("2400-02-29", "Feb 2400"),
        ] {
            assert_eq!(
                short_en_range(input, RangeEnd::Missing),
                expected,
                "{input}"
            );
        }
    }

    #[test]
    fn impossible_gregorian_dates_are_retained_not_normalized() {
        for input in [
            "2024-00",
            "2024-13",
            "2024-99",
            "2024-00-01",
            "2024-13-01",
            "2024-01-00",
            "2024-01-32",
            "2024-12-99",
            "2024-04-31",
            "2024-06-31",
            "2024-09-31",
            "2024-11-31",
            "2023-02-29",
            "1900-02-29",
            "2100-02-29",
            "2024-02-30",
            "2024-02-31",
        ] {
            assert_eq!(short_en_range(input, RangeEnd::Missing), input, "{input}");
        }
    }

    #[test]
    fn invalid_end_is_retained_independently_of_valid_start() {
        assert_eq!(
            short_en_range("2024-02", RangeEnd::Value("2023-02-29")),
            "Feb 2024 - 2023-02-29"
        );
        assert_eq!(
            short_en_range("bad start", RangeEnd::Value("2024-12-31")),
            "bad start - Dec 2024"
        );
        assert_eq!(
            short_en_range("bad start", RangeEnd::Value("bad end")),
            "bad start - bad end"
        );
    }

    #[test]
    fn unsupported_strings_are_retained_exactly_without_trimming() {
        for input in [
            "2020-01-01T00:00:00Z",
            "2020-01-01T00:30:00+14:00",
            "2020/01/01",
            "Jan 2020",
            "2020-1",
            "2020-01-1",
            "+002020-01-01",
            "10000-01-01",
            " 2020 ",
            "2020\n",
            " ",
            "NaN",
            "null",
            "Present",
            "２０２４",
            "2024-０１",
            "2024-01-0é",
        ] {
            assert_eq!(short_en_range(input, RangeEnd::Missing), input, "{input:?}");
            assert_eq!(
                short_en_range("2024", RangeEnd::Value(input)),
                format!("Jan 2024 - {input}"),
                "{input:?}"
            );
        }
    }

    #[test]
    fn separator_is_ascii_and_range_order_is_not_changed() {
        assert_eq!(
            short_en_range("2020-06-01", RangeEnd::Value("2022-02-28")),
            "Jun 2020 - Feb 2022"
        );
        assert_eq!(
            short_en_range("2024-12", RangeEnd::Value("2020-01")),
            "Dec 2024 - Jan 2020"
        );
        assert_eq!(
            short_en_range("2024-03-31", RangeEnd::Value("2024-03-01")),
            "Mar 2024 - Mar 2024"
        );
    }
}
