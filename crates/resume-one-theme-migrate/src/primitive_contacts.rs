//! Finite, content-only expansion of pinned @jsonresume/core ContactInfo.
//!
//! ContactInfo.jsx SHA-256:
//! 6774f1d5991f1801849b4114bac802d429509558de507cac64d770eec8a17782
//! @jsonresume/utils/src/url.js SHA-256:
//! 3fe129a7b00f161ea66e63a66e05336ebc47ce5e37b32c038d1f295a9aa783ce
//! The core security.js file re-exports the latter's safeUrl implementation.
//!
//! No JS, HTML, network, or URL parser is executed. The caller owns pinned
//! identity verification, parent className handling, and style compilation.
//! Missing map entries mean JS undefined; JSON null never means undefined.
//! Unsupported boxed basics, composite text/attribute values, non-integer
//! numeric coercions, and ambiguous URL fallbacks fail explicitly. These domain
//! restrictions are not assertions that upstream JS itself throws there.

use serde_json::{Map, Value, json};

const MAX_NODES: usize = 32_768;
const MAX_DEPTH: usize = 32;
const MAX_BYTES: usize = 1024 * 1024;
const MAX_PROFILES: usize = 1024;
const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

fn budget(props: &Map<String, Value>) -> Result<(), String> {
    let mut pending: Vec<_> = props.values().map(|value| (value, 0usize)).collect();
    let mut nodes = props.len();
    let mut bytes = props.keys().map(String::len).sum::<usize>();
    while let Some((value, depth)) = pending.pop() {
        nodes += 1;
        if nodes > MAX_NODES || pending.len() > MAX_NODES || depth > MAX_DEPTH {
            return Err("CONTACT_INPUT_LIMIT".into());
        }
        match value {
            Value::String(s) => bytes = bytes.saturating_add(s.len()),
            Value::Array(values) => pending.extend(values.iter().map(|v| (v, depth + 1))),
            Value::Object(values) => {
                bytes = bytes.saturating_add(values.keys().map(String::len).sum::<usize>());
                pending.extend(values.values().map(|v| (v, depth + 1)));
            }
            _ => bytes = bytes.saturating_add(32),
        }
        if bytes > MAX_BYTES {
            return Err("CONTACT_INPUT_LIMIT".into());
        }
    }
    Ok(())
}

fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null | Value::Bool(false)) => false,
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Number(n)) => n.as_f64().is_some_and(|n| n != 0.0),
        Some(Value::Bool(true) | Value::Array(_) | Value::Object(_)) => true,
    }
}

fn ecma_space(c: char) -> bool {
    matches!(c, '\u{0009}'..='\u{000d}' | '\u{0020}' | '\u{00a0}' | '\u{1680}'
        | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}'
        | '\u{205f}' | '\u{3000}' | '\u{feff}')
}

fn scalar_string(value: &Value, field: &str) -> Result<String, String> {
    match value {
        Value::String(s) => Ok(s.clone()),
        Value::Bool(b) => Ok(b.to_string()),
        Value::Null => Ok("null".into()),
        Value::Number(n) => {
            let number = n
                .as_f64()
                .ok_or_else(|| format!("CONTACT_NUMBER_UNSUPPORTED:{field}"))?;
            if !number.is_finite() || number.fract() != 0.0 || number.abs() > MAX_SAFE_INTEGER {
                return Err(format!("CONTACT_NUMBER_COERCION_UNSUPPORTED:{field}"));
            }
            // The finite safe-integer subset has the same decimal spelling as
            // JS Number::toString, including 1.0 -> "1" and -0 -> "0".
            Ok(if number == 0.0 {
                "0".into()
            } else {
                format!("{number:.0}")
            })
        }
        _ => Err(format!("CONTACT_COMPOSITE_TEXT_UNSUPPORTED:{field}")),
    }
}

fn text(value: Option<&Value>, field: &str) -> Result<Vec<Value>, String> {
    match value {
        None | Some(Value::Null | Value::Bool(_)) => Ok(vec![]),
        Some(value) => Ok(vec![Value::String(scalar_string(value, field)?)]),
    }
}

fn element(
    tag: &str,
    attributes: Map<String, Value>,
    children: Vec<Value>,
    role: Option<&str>,
) -> Value {
    let mut value = json!({"kind":"element","tag":tag,"attributes":attributes,"children":children});
    if let Some(role) = role {
        value["styleRole"] = json!(role);
    }
    value
}

fn item(attributes: Map<String, Value>, children: Vec<Value>) -> Value {
    element("span", attributes, children, Some("core-contact-item"))
}

fn prefix_ci(value: &str, prefix: &str) -> bool {
    value
        .get(..prefix.len())
        .is_some_and(|s| s.eq_ignore_ascii_case(prefix))
}

fn source_domain(value: &str) -> bool {
    // Exactly /^[a-z0-9][a-z0-9.-]+\.[a-z]{2,}$/i, including its requirement
    // of at least TWO characters before the final dot (a.co does not match).
    let Some((head, tld)) = value.rsplit_once('.') else {
        return false;
    };
    head.len() >= 2
        && head.as_bytes()[0].is_ascii_alphanumeric()
        && head
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-'))
        && tld.len() >= 2
        && tld.bytes().all(|b| b.is_ascii_alphabetic())
}

fn safe_url(value: &Value) -> Result<Option<String>, String> {
    let Some(original) = value.as_str().filter(|s| !s.is_empty()) else {
        // Upstream's typeof guard returns null, even for truthy objects/numbers.
        return Ok(None);
    };
    let value = original.trim_matches(ecma_space);
    if ["javascript:", "data:", "vbscript:", "file:", "about:"]
        .iter()
        .any(|prefix| prefix_ci(value, prefix))
    {
        return Ok(None);
    }
    // The source falls back to returning unrecognized strings unchanged. Do
    // not emit a potentially executable browser-normalized fallback or pretend
    // that a stronger product URL policy is this source implementation.
    if value.chars().any(|c| c.is_ascii_control() || c == '\\') {
        return Err("CONTACT_HREF_CONTROL_OR_BACKSLASH_UNSUPPORTED".into());
    }
    if ["http:", "https:", "mailto:", "tel:", "sms:", "ftp:"]
        .iter()
        .any(|prefix| prefix_ci(value, prefix))
        || value.starts_with('/')
        || value.starts_with('.')
    {
        return Ok(Some(value.into()));
    }
    if prefix_ci(value, "www.") || source_domain(value) {
        return Ok(Some(format!("https://{value}")));
    }
    if value.contains(':') {
        return Err("CONTACT_HREF_UNKNOWN_SCHEME_FALLBACK_UNSUPPORTED".into());
    }
    // No colon/control/backslash: this is the source's inert relative-text
    // fallback, including #fragments, queries, paths and the empty trimmed URL.
    Ok(Some(value.into()))
}

fn anchor(
    href: &Value,
    label: Option<&Value>,
    children: Vec<Value>,
    external: bool,
) -> Result<Value, String> {
    let mut attrs = Map::new();
    if let Some(href) = safe_url(href)? {
        attrs.insert("href".into(), json!(href));
    }
    if external {
        attrs.insert("target".into(), json!("_blank"));
        attrs.insert("rel".into(), json!("noopener noreferrer"));
    }
    if let Some(label) = label.filter(|v| !v.is_null()) {
        attrs.insert(
            "aria-label".into(),
            json!(scalar_string(label, "aria-label")?),
        );
    }
    // There is no independent styled anchor primitive: the pinned container
    // styles descendant `a`. Giving it the item role would invent span styles.
    Ok(element("a", attrs, children, None))
}

fn website_text(url: &str) -> String {
    // ContactInfo uses these two regexes, NOT utils.formatUrlForDisplay:
    // /^https?:\/\// (case-sensitive), then /\/$/ (one slash only).
    let url = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    url.strip_suffix('/').unwrap_or(url).to_owned()
}

/// Expands plain, finite JSON props into inert element/text data. `None` is
/// exactly the empty-items return-null case within this supported domain.
/// It does not establish visual equivalence or compile any CSS style role.
pub(crate) fn expand(props: &Map<String, Value>) -> Result<Option<Value>, String> {
    budget(props)?;
    let empty = Map::new();
    let basics = match props.get("basics") {
        None => &empty,
        Some(Value::Object(basics)) => basics,
        Some(Value::Null) => return Err("CONTACT_BASICS_NULL_DESTRUCTURE".into()),
        Some(_) => return Err("CONTACT_BASICS_NONOBJECT_BOXING_UNSUPPORTED".into()),
    };
    // Destructuring defaults apply to undefined only. Do not turn null into [].
    let profiles = match basics.get("profiles") {
        None => &[][..],
        Some(Value::Array(profiles)) if profiles.len() <= MAX_PROFILES => profiles.as_slice(),
        Some(Value::Array(_)) => return Err("CONTACT_PROFILE_LIMIT".into()),
        Some(Value::Null) => return Err("CONTACT_PROFILES_NULL_FOREACH".into()),
        Some(_) => return Err("CONTACT_PROFILES_FOREACH_NOT_FUNCTION".into()),
    };
    let separator = match props.get("separator") {
        None => Some("•"),
        Some(Value::String(separator)) => Some(separator.as_str()),
        Some(Value::Null) => None,
        Some(_) => return Err("CONTACT_SEPARATOR_TYPE_UNSUPPORTED".into()),
    };
    let mut items = Vec::new();
    for (field, protocol, label) in [("email", "mailto:", "Email"), ("phone", "tel:", "Phone")] {
        if let Some(value) = basics.get(field).filter(|value| truthy(Some(value))) {
            let href = json!(format!("{protocol}{}", scalar_string(value, field)?));
            let link = anchor(&href, Some(&json!(label)), text(Some(value), field)?, false)?;
            items.push(item(Map::new(), vec![link]));
        }
    }
    if let Some(location) = basics.get("location").filter(|value| truthy(Some(value))) {
        // For JSON primitives/arrays, named property reads are undefined in JS.
        // Null is excluded by the original `if (location)` truthiness guard.
        if let Some(location) = location.as_object() {
            let mut parts = Vec::new();
            for field in ["city", "region", "countryCode"] {
                if let Some(value) = location.get(field).filter(|value| truthy(Some(value))) {
                    parts.push(scalar_string(value, field)?);
                }
            }
            let location = parts.join(", ");
            if !location.is_empty() {
                let attrs = Map::from_iter([("aria-label".into(), json!("Location"))]);
                items.push(item(attrs, vec![json!(location)]));
            }
        }
    }
    if let Some(url) = basics.get("url").filter(|value| truthy(Some(value))) {
        let raw = url.as_str().ok_or("CONTACT_WEBSITE_REPLACE_NOT_FUNCTION")?;
        let link = anchor(
            url,
            Some(&json!("Website")),
            vec![json!(website_text(raw))],
            true,
        )?;
        items.push(item(Map::new(), vec![link]));
    }
    for profile in profiles {
        if profile.is_null() {
            return Err("CONTACT_PROFILE_NULL_PROPERTY_ACCESS".into());
        }
        // JSON scalar/array entries have no named `url` property. No string
        // coercion, inherited property lookup or user code is involved.
        let Some(profile) = profile.as_object() else {
            continue;
        };
        if let Some(url) = profile.get("url").filter(|value| truthy(Some(value))) {
            let network = profile.get("network");
            let label = if truthy(network) {
                network
            } else {
                profile.get("username")
            };
            let link = anchor(url, network, text(label, "profile-label")?, true)?;
            items.push(item(Map::new(), vec![link]));
        }
    }
    if items.is_empty() {
        return Ok(None);
    }
    let mut children = Vec::with_capacity(items.len() * 2 - 1);
    for (index, item) in items.into_iter().enumerate() {
        if index > 0 {
            children.push(element(
                "span",
                Map::from_iter([("aria-hidden".into(), json!("true"))]),
                separator.map(|s| vec![json!(s)]).unwrap_or_default(),
                Some("core-contact-separator"),
            ));
        }
        children.push(item);
    }
    let output = element(
        "div",
        Map::from_iter([("className".into(), json!("resume-contact"))]),
        children,
        Some("core-contact-container"),
    );
    if serde_json::to_vec(&output)
        .map_err(|_| "CONTACT_OUTPUT_ENCODING")?
        .len()
        > MAX_BYTES
    {
        return Err("CONTACT_OUTPUT_LIMIT".into());
    }
    Ok(Some(output))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(props: Value) -> Result<Option<Value>, String> {
        expand(props.as_object().unwrap())
    }
    fn contact(basics: Value) -> Value {
        run(json!({"basics":basics})).unwrap().unwrap()
    }
    fn links(node: &Value) -> Vec<&Value> {
        node["children"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|item| item["children"].as_array()?.first())
            .filter(|value| value["tag"] == "a")
            .collect()
    }

    #[test]
    fn undefined_only_defaults_and_five_flat_props_do_not_fill_basics() {
        assert_eq!(run(json!({})).unwrap(), None);
        assert_eq!(run(json!({"basics":{}})).unwrap(), None);
        assert_eq!(run(json!({"email":"e@example.com","phone":"123","url":"https://example.com",
            "location":{"city":"Paris"},"profiles":[{"network":"GitHub","url":"https://github.com/x"}]})).unwrap(), None);
        assert!(
            run(json!({"basics":null}))
                .unwrap_err()
                .contains("NULL_DESTRUCTURE")
        );
        for basics in [json!(true), json!(0), json!("x"), json!([])] {
            assert!(
                run(json!({"basics":basics}))
                    .unwrap_err()
                    .contains("BOXING_UNSUPPORTED")
            );
        }
    }

    #[test]
    fn profiles_null_or_nonarray_are_not_defaulted() {
        assert!(
            run(json!({"basics":{"profiles":null}}))
                .unwrap_err()
                .contains("NULL_FOREACH")
        );
        for profiles in [json!(false), json!(""), json!(0), json!({})] {
            assert!(
                run(json!({"basics":{"profiles":profiles}}))
                    .unwrap_err()
                    .contains("FOREACH_NOT_FUNCTION")
            );
        }
        assert_eq!(run(json!({"basics":{"profiles":[]}})).unwrap(), None);
        assert!(
            run(json!({"basics":{"profiles":[null]}}))
                .unwrap_err()
                .contains("NULL_PROPERTY_ACCESS")
        );
    }

    #[test]
    fn source_order_location_parts_and_separators() {
        let out = contact(json!({"email":"e@x.test","phone":"+12","location":{
            "city":"City","region":"Region","countryCode":"ZZ","address":"NOT SHOWN"},
            "url":"https://example.com/","profiles":[{"network":"GitHub","url":"https://github.com/u"}]}));
        let children = out["children"].as_array().unwrap();
        assert_eq!(children.len(), 9);
        assert_eq!(
            children[0]["children"][0]["attributes"]["href"],
            "mailto:e@x.test"
        );
        assert_eq!(children[2]["children"][0]["attributes"]["href"], "tel:+12");
        assert_eq!(children[4]["children"], json!(["City, Region, ZZ"]));
        assert_eq!(children[4]["attributes"]["aria-label"], "Location");
        assert_eq!(
            children[6]["children"][0]["children"],
            json!(["example.com"])
        );
        assert_eq!(children[8]["children"][0]["children"], json!(["GitHub"]));
        for index in [1, 3, 5, 7] {
            assert_eq!(children[index]["children"], json!(["•"]));
            assert_eq!(children[index]["attributes"]["aria-hidden"], "true");
            assert_eq!(children[index]["styleRole"], "core-contact-separator");
        }
    }

    #[test]
    fn truthiness_preserves_false_zero_null_empty_and_boolean_rendering() {
        for value in [Value::Null, json!(false), json!(0), json!("")] {
            assert_eq!(
                run(json!({"basics":{"email":value,"phone":value,"url":value,"location":value}}))
                    .unwrap(),
                None
            );
        }
        let out = contact(
            json!({"email":true,"phone":12.0,"location":{"city":0,"region":false,"countryCode":"ZZ"}}),
        );
        let found = links(&out);
        assert_eq!(found[0]["attributes"]["href"], "mailto:true");
        assert_eq!(found[0]["children"], json!([]));
        assert_eq!(found[1]["attributes"]["href"], "tel:12");
        assert_eq!(found[1]["children"], json!(["12"]));
        assert_eq!(out["children"][4]["children"], json!(["ZZ"]));
    }

    #[test]
    fn profile_url_gate_network_or_username_and_empty_item() {
        let out = contact(json!({"profiles":[
            {"network":"invisible","username":"nobody"},
            {"network":"","username":"fallback","url":"/u"},
            {"network":false,"username":0,"url":"/zero"},
            {"url":"/empty"},
            {"network":"Shown","username":"ignored","url":"/shown"},
            {"url":""},false,3,"scalar",[]]}));
        let found = links(&out);
        assert_eq!(found.len(), 4);
        assert_eq!(found[0]["children"], json!(["fallback"]));
        assert_eq!(found[0]["attributes"]["aria-label"], "");
        assert_eq!(found[1]["children"], json!(["0"]));
        assert_eq!(found[1]["attributes"]["aria-label"], "false");
        assert_eq!(found[2]["children"], json!([]));
        assert!(
            !found[2]["attributes"]
                .as_object()
                .unwrap()
                .contains_key("aria-label")
        );
        assert_eq!(found[3]["children"], json!(["Shown"]));
    }

    #[test]
    fn website_display_is_case_sensitive_and_removes_one_slash_only() {
        for (url, display) in [
            ("https://example.com//", "example.com/"),
            ("http://example.com/", "example.com"),
            ("HTTPS://example.com/", "HTTPS://example.com"),
            ("ftp://example.com///", "ftp://example.com//"),
            (" https://example.com/ ", " https://example.com/ "),
        ] {
            let out = contact(json!({"url":url}));
            assert_eq!(links(&out)[0]["children"], json!([display]));
        }
        for url in [json!(true), json!(1), json!([]), json!({})] {
            assert!(
                run(json!({"basics":{"url":url}}))
                    .unwrap_err()
                    .contains("REPLACE_NOT_FUNCTION")
            );
        }
    }

    #[test]
    fn known_dangerous_protocols_remove_href_not_the_item() {
        for url in [
            "javascript:alert(1)",
            " DATA:text/html,x",
            "VbScRiPt:x",
            "file:/x",
            "about:blank",
        ] {
            let out = contact(json!({"url":url}));
            let link = links(&out)[0];
            assert!(!link["attributes"].as_object().unwrap().contains_key("href"));
            assert_eq!(link["attributes"]["target"], "_blank");
            assert_eq!(link["attributes"]["rel"], "noopener noreferrer");
        }
    }

    #[test]
    fn finite_source_url_rules_are_not_the_product_strict_url_policy() {
        for (url, expected) in [
            (" HTTPS:example ", "HTTPS:example"),
            ("sms:123", "sms:123"),
            ("ftp://example.com", "ftp://example.com"),
            ("//host/path", "//host/path"),
            ("../resume", "../resume"),
            ("www.Example.com/x", "https://www.Example.com/x"),
            ("ab.co", "https://ab.co"),
            ("a.co", "a.co"),
            ("#fragment", "#fragment"),
            ("example.com/path", "example.com/path"),
            (" ", ""),
        ] {
            assert_eq!(safe_url(&json!(url)).unwrap(), Some(expected.into()));
        }
        assert_eq!(
            safe_url(&json!("\u{feff}https://example.com\u{feff}")).unwrap(),
            Some("https://example.com".into())
        );
        assert!(safe_url(&json!("\u{0085}javascript:x")).is_err());
    }

    #[test]
    fn unknown_or_browser_normalized_danger_is_explicitly_unsupported() {
        for url in [
            "custom:foo",
            "java\nscript:alert(1)",
            "java\tscript:x",
            "\\javascript:x",
            "x\u{0000}y",
        ] {
            assert!(
                run(json!({"basics":{"url":url}}))
                    .unwrap_err()
                    .starts_with("CONTACT_HREF_")
            );
        }
        // Source's known dangerous-protocol check runs before our finite-domain
        // guard, so it still faithfully returns null for this blocked scheme.
        assert_eq!(safe_url(&json!("javascript:\nalert(1)")).unwrap(), None);
    }

    #[test]
    fn nonstring_profile_url_follows_source_safe_url_type_guard() {
        let out = contact(json!({"profiles":[{"url":true,"username":"true"},
            {"url":1,"username":"number"},{"url":{},"username":"object"}]}));
        assert_eq!(links(&out).len(), 3);
        for link in links(&out) {
            assert!(!link["attributes"].as_object().unwrap().contains_key("href"));
        }
    }

    #[test]
    fn separator_null_keeps_empty_span_and_parent_owns_classname() {
        for separator in [Value::Null, json!(""), json!(" | ")] {
            let out = run(json!({"basics":{"email":"x","phone":"y"},
                "separator":separator,"className":"parent-owned"}))
            .unwrap()
            .unwrap();
            assert_eq!(out["attributes"]["className"], "resume-contact");
            let expected = if separator.is_null() {
                json!([])
            } else {
                json!([separator])
            };
            assert_eq!(out["children"][1]["children"], expected);
        }
        assert!(
            run(json!({"separator":false}))
                .unwrap_err()
                .contains("SEPARATOR_TYPE")
        );
    }

    #[test]
    fn unknown_composite_text_and_number_coercion_do_not_guess() {
        for basics in [
            json!({"email":{}}),
            json!({"phone":[]}),
            json!({"email":0.5}),
            json!({"location":{"city":[]}}),
            json!({"profiles":[{"url":"/x","network":{}}]}),
        ] {
            assert!(run(json!({"basics":basics})).is_err());
        }
        assert!(run(json!({"basics":{"phone":9007199254740992_u64}})).is_err());
    }

    #[test]
    fn output_contains_only_inert_allowed_elements_and_string_children() {
        let out = contact(json!({"email":"<script>alert(1)</script>",
            "url":"/x","profiles":[{"network":"<img onerror=x>","url":"/p"}]}));
        let mut pending = vec![&out];
        while let Some(node) = pending.pop() {
            if node.is_string() {
                continue;
            }
            assert_eq!(node["kind"], "element");
            assert!(matches!(node["tag"].as_str(), Some("div" | "span" | "a")));
            for key in node["attributes"].as_object().unwrap().keys() {
                assert!(matches!(
                    key.as_str(),
                    "href" | "target" | "rel" | "aria-label" | "aria-hidden" | "className"
                ));
            }
            pending.extend(node["children"].as_array().unwrap());
        }
        assert_eq!(
            links(&out)[0]["children"],
            json!(["<script>alert(1)</script>"])
        );
    }

    #[test]
    fn resource_limits_fail_closed_without_source_execution() {
        let many = vec![json!({"url":"/x"}); MAX_PROFILES + 1];
        assert!(
            run(json!({"basics":{"profiles":many}}))
                .unwrap_err()
                .contains("PROFILE_LIMIT")
        );
        assert!(
            run(json!({"basics":{"email":"x".repeat(MAX_BYTES + 1)}}))
                .unwrap_err()
                .contains("INPUT_LIMIT")
        );
        let mut deep = Value::Null;
        for _ in 0..MAX_DEPTH + 2 {
            deep = json!([deep]);
        }
        assert!(
            run(json!({"ignored":deep}))
                .unwrap_err()
                .contains("INPUT_LIMIT")
        );
    }
}
