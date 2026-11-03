//! Finite date text for the cached core DateRange / utils formatDateRange.
//!
//! Source contract: resume-core/src/primitives/DateRange.jsx and
//! utils/src/dates.js in the academic-upstream-baseline-20261030 cache, read in
//! full. Only canonical Gregorian YYYY[-MM[-DD]], default short/en-US, and
//! UTC date-only semantics are implemented. No JS, Intl, locale database,
//! timezone, invalid-date fallback or Date normalization is approximated.
//!
//! This is a formatter, NOT a complete component/props validator. It consumes
//! startDate/endDate/format/locale/numberingSystem/presentLabel only; className,
//! key, rest props, styling and component identity are the parent's concern.
//! Omitted keys represent JS undefined; callers must not replace them with null.

use serde_json::{Map, Value};

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// Configuration is checked first to enforce this finite supported domain,
/// even for empty output. Missing defaults are supported; explicit null is not
/// an omitted format/locale/option. After that check a falsy start returns None
/// without examining endDate, matching the source's early return.
pub(crate) fn format(props: &Map<String, Value>) -> Result<Option<String>, String> {
    default_option(props, "format", "short")?;
    default_option(props, "locale", "en-US")?;
    for option in ["numberingSystem", "presentLabel"] {
        if props.contains_key(option) {
            return Err(format!(
                "DATERANGE_UNSUPPORTED_OPTION: explicit {option} is outside the finite formatter"
            ));
        }
    }

    let Some(start) = props.get("startDate").filter(|value| !falsy(value)) else {
        return Ok(None);
    };
    let start = date(start, "startDate")?;
    let Some(end) = props.get("endDate") else {
        return Ok(Some(start));
    };
    let end = if falsy(end) {
        "Present".to_owned()
    } else {
        date(end, "endDate")?
    };
    Ok(Some(format!("{start} - {end}")))
}

fn default_option(props: &Map<String, Value>, key: &str, expected: &str) -> Result<(), String> {
    match props.get(key) {
        None => Ok(()),
        Some(Value::String(value)) if value == expected => Ok(()),
        Some(_) => Err(format!(
            "DATERANGE_UNSUPPORTED_OPTION: {key} must be omitted or the explicit string {expected}"
        )),
    }
}

fn falsy(value: &Value) -> bool {
    match value {
        Value::Null | Value::Bool(false) => true,
        Value::Number(number) => number.as_f64() == Some(0.0),
        Value::String(string) => string.is_empty(),
        _ => false,
    }
}

fn date(value: &Value, field: &str) -> Result<String, String> {
    let Value::String(text) = value else {
        return Err(format!(
            "DATERANGE_DATE_TYPE: truthy {field} must be a canonical date-only string"
        ));
    };
    let bytes = text.as_bytes();
    if !matches!(bytes.len(), 4 | 7 | 10)
        || bytes.iter().enumerate().any(|(index, byte)| {
            if matches!(index, 4 | 7) {
                *byte != b'-'
            } else {
                !byte.is_ascii_digit()
            }
        })
    {
        return Err(format!(
            "DATERANGE_NON_CANONICAL_DATE: {field} requires ASCII YYYY, YYYY-MM or YYYY-MM-DD; no timestamps or coercion"
        ));
    }

    // Fixed ASCII widths were checked above; no slicing or numeric conversion
    // depends on user-supplied length, and years stay in 0000..9999.
    let number = |digits: &[u8]| -> u16 {
        digits
            .iter()
            .fold(0, |value, digit| value * 10 + u16::from(digit - b'0'))
    };
    let year = number(&bytes[..4]);
    let month = if bytes.len() >= 7 {
        number(&bytes[5..7])
    } else {
        1
    };
    let day = if bytes.len() == 10 {
        number(&bytes[8..10])
    } else {
        1
    };
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let last_day = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => 0,
    };
    if day == 0 || day > last_day {
        return Err(format!(
            "DATERANGE_INVALID_GREGORIAN_DATE: {field} would require an invalid date or normalization"
        ));
    }
    // Proleptic Gregorian year 0000 is 1 BCE. Intl's numeric year without an
    // era renders 1 (not 0); validate leap days using year 0 before this mapping.
    let display_year = if year == 0 { 1 } else { year };
    Ok(format!("{} {display_year}", MONTHS[usize::from(month - 1)]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn run(value: Value) -> Result<Option<String>, String> {
        format(value.as_object().unwrap())
    }

    fn output(value: Value) -> String {
        run(value).unwrap().unwrap()
    }

    #[test]
    fn missing_start_is_none_and_does_not_examine_end_date() {
        assert_eq!(run(json!({})).unwrap(), None);
        assert_eq!(run(json!({"endDate":true})).unwrap(), None);
        assert_eq!(run(json!({"endDate":"not a date"})).unwrap(), None);
    }

    #[test]
    fn every_supported_falsy_start_is_none_without_coercion() {
        for value in [
            Value::Null,
            json!(false),
            json!(0),
            json!(0.0),
            json!(-0.0),
            json!(""),
        ] {
            assert_eq!(run(json!({"startDate":value,"endDate":{}})).unwrap(), None);
        }
    }

    #[test]
    fn absent_end_is_a_single_date_but_explicit_null_is_present() {
        assert_eq!(output(json!({"startDate":"2020-01-15"})), "Jan 2020");
        assert_eq!(
            output(json!({"startDate":"2020-01-15","endDate":null})),
            "Jan 2020 - Present"
        );
    }

    #[test]
    fn every_supported_falsy_end_means_present_not_absent() {
        for value in [
            Value::Null,
            json!(false),
            json!(0),
            json!(0.0),
            json!(-0.0),
            json!(""),
        ] {
            assert_eq!(
                output(json!({"startDate":"2020","endDate":value})),
                "Jan 2020 - Present"
            );
        }
    }

    #[test]
    fn canonical_precisions_use_the_input_month_and_short_year() {
        for (value, expected) in [
            ("2020", "Jan 2020"),
            ("2020-06", "Jun 2020"),
            ("2020-06-30", "Jun 2020"),
        ] {
            assert_eq!(output(json!({"startDate":value})), expected);
        }
        assert_eq!(
            output(json!({"startDate":"2018-06","endDate":"2020-03-01"})),
            "Jun 2018 - Mar 2020"
        );
        // No range reordering: the upstream formatter formats, not sorts.
        assert_eq!(
            output(json!({"startDate":"2020","endDate":"2018"})),
            "Jan 2020 - Jan 2018"
        );
    }

    #[test]
    fn all_en_us_short_month_names_are_exact() {
        for (month, expected) in [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ]
        .iter()
        .enumerate()
        {
            assert_eq!(
                output(json!({"startDate":format!("2024-{:02}-01",month+1)})),
                format!("{expected} 2024")
            );
        }
    }

    #[test]
    fn year_zero_uses_intl_year_one_but_gregorian_year_zero_for_leaps() {
        for (value, expected) in [
            ("0000", "Jan 1"),
            ("0000-02-29", "Feb 1"),
            ("0001-01-01", "Jan 1"),
            ("0099-12", "Dec 99"),
            ("0999", "Jan 999"),
            ("9999-12-31", "Dec 9999"),
        ] {
            assert_eq!(output(json!({"startDate":value})), expected);
        }
        assert_eq!(
            output(json!({"startDate":"0000-02-29","endDate":"0001-03"})),
            "Feb 1 - Mar 1"
        );
    }

    #[test]
    fn gregorian_leap_centuries_and_invalid_normalizations_are_checked() {
        for year in [0, 400, 1600, 2000, 2024, 2400] {
            assert!(run(json!({"startDate":format!("{year:04}-02-29")})).is_ok());
        }
        for value in [
            "0001-02-29",
            "1900-02-29",
            "2100-02-29",
            "2023-02-29",
            "2024-02-30",
            "2024-04-31",
            "2024-00",
            "2024-13",
            "2024-01-00",
            "2024-01-32",
        ] {
            for field in ["startDate", "endDate"] {
                let mut props = json!({"startDate":"2020"});
                props[field] = json!(value);
                assert!(
                    run(props)
                        .unwrap_err()
                        .starts_with("DATERANGE_INVALID_GREGORIAN_DATE"),
                    "{field}: {value}"
                );
            }
        }
    }

    #[test]
    fn noncanonical_date_strings_are_never_guessed_or_returned_raw() {
        for value in [
            "0",
            "20",
            "20201",
            "02020",
            "2020-1",
            "2020-01-1",
            "2020/01/01",
            " 2020",
            "2020 ",
            "2020\n",
            "２０２０",
            "2020-01-01Z",
            "January 2020",
            "invalid",
        ] {
            assert!(
                run(json!({"startDate":value}))
                    .unwrap_err()
                    .starts_with("DATERANGE_NON_CANONICAL_DATE"),
                "{value:?}"
            );
        }
    }

    #[test]
    fn iso_timestamps_timezone_offsets_and_extended_years_are_unsupported() {
        for value in [
            "2020-01-01T00:00:00Z",
            "2020-01-01T00:00:00-08:00",
            "2020-01-01T01:02:03.456+08:00",
            "+002020-01-01",
            "-000001-01-01",
        ] {
            assert!(run(json!({"startDate":value})).is_err());
            assert!(run(json!({"startDate":"2020","endDate":value})).is_err());
        }
    }

    #[test]
    fn truthy_nonstring_dates_fail_instead_of_coercing() {
        for value in [
            json!(true),
            json!(1),
            json!(-1),
            json!(1.5),
            json!([]),
            json!(["2020"]),
            json!({}),
            json!({"year":2020}),
        ] {
            for field in ["startDate", "endDate"] {
                let mut props = json!({"startDate":"2020"});
                props[field] = value.clone();
                assert!(run(props).unwrap_err().starts_with("DATERANGE_DATE_TYPE"));
            }
        }
    }

    #[test]
    fn explicit_defaults_work_but_null_and_all_other_options_fail_closed() {
        assert_eq!(
            output(json!({"startDate":"2020","format":"short","locale":"en-US"})),
            "Jan 2020"
        );
        for (key, values) in [
            (
                "format",
                vec![
                    Value::Null,
                    json!(false),
                    json!(0),
                    json!(""),
                    json!("long"),
                    json!("numeric"),
                    json!("Short"),
                ],
            ),
            (
                "locale",
                vec![
                    Value::Null,
                    json!(false),
                    json!(0),
                    json!(""),
                    json!("en"),
                    json!("en-us"),
                    json!("fr-FR"),
                    json!(["en-US"]),
                ],
            ),
            (
                "numberingSystem",
                vec![
                    Value::Null,
                    json!(false),
                    json!(""),
                    json!("latn"),
                    json!("arab"),
                ],
            ),
            (
                "presentLabel",
                vec![
                    Value::Null,
                    json!(false),
                    json!(""),
                    json!("Present"),
                    json!("Now"),
                ],
            ),
        ] {
            for value in values {
                let mut props = json!({"startDate":"2020","endDate":null});
                props[key] = value.clone();
                assert!(
                    run(props)
                        .unwrap_err()
                        .starts_with("DATERANGE_UNSUPPORTED_OPTION"),
                    "{key}={value}"
                );
                // Unsupported options never enter the finite domain just because
                // the original component would have returned no text early.
                let mut props = json!({});
                props[key] = value;
                assert!(
                    run(props)
                        .unwrap_err()
                        .starts_with("DATERANGE_UNSUPPORTED_OPTION")
                );
            }
        }
    }

    #[test]
    fn formatter_leaves_component_props_and_input_untouched() {
        let value = json!({"startDate":"2024-09-01","className":"custom","key":"logical","unknownProp":{"not":"validated here"}});
        let original = value.clone();
        assert_eq!(
            format(value.as_object().unwrap()).unwrap(),
            Some("Sep 2024".into())
        );
        assert_eq!(value, original);
    }
}
