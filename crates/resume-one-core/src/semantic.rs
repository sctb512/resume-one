//! Theme-independent semantic projection of the standard JSON Resume sections.
//! Unknown extension fields stay in the source document but are not exported implicitly.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::resume::ResumeDocument;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumeContent {
    pub name: String,
    pub label: String,
    pub contacts: Vec<String>,
    pub summary: String,
    pub sections: Vec<Section>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub id: &'static str,
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub heading: String,
    /// Explicit standard header fields, separate from the legacy combined heading/details.
    pub header_fields: BTreeMap<&'static str, String>,
    /// Alternate visual date text; only an explicit theme option uses it.
    /// Legacy details and text/Markdown retain the original period projection.
    pub short_en_date_range: String,
    pub details: Vec<String>,
    /// Position of the date inside `details`; inline output keeps its original order.
    pub date_detail_index: Option<usize>,
    pub paragraphs: Vec<String>,
    pub bullets: Vec<String>,
    pub url: Option<String>,
}

const IDS: [&str; 11] = [
    "work",
    "volunteer",
    "education",
    "awards",
    "certificates",
    "publications",
    "skills",
    "languages",
    "interests",
    "references",
    "projects",
];

/// Allowed declarative entry-header fields for a standard section.
/// Paragraphs, bullets, URLs and extension fields are never implicit header fields.
pub fn header_field_names(id: &str) -> &'static [&'static str] {
    match id {
        "work" => &["position", "name", "dateRange", "location"],
        "volunteer" => &["position", "organization", "dateRange"],
        "education" => &["institution", "studyType", "area", "score", "dateRange"],
        "awards" => &["title", "awarder", "date"],
        "certificates" => &["name", "issuer", "date"],
        "publications" => &["name", "publisher", "releaseDate"],
        "skills" => &["name", "level"],
        "languages" => &["language", "fluency"],
        "interests" | "references" => &["name"],
        "projects" => &["name", "dateRange", "entity", "type", "roles", "keywords"],
        _ => &[],
    }
}

fn string<'a>(item: &'a Value, key: &str) -> &'a str {
    item.get(key).and_then(Value::as_str).unwrap_or("").trim()
}

fn strings(item: &Value, key: &str) -> Vec<String> {
    item.get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flat_map(|items| items.iter())
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect()
}

fn joined(item: &Value, fields: &[&str], separator: &str) -> String {
    fields
        .iter()
        .map(|key| string(item, key))
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join(separator)
}

fn maybe_push(items: &mut Vec<String>, value: impl Into<String>) {
    let value = value.into();
    if !value.is_empty() {
        items.push(value);
    }
}

fn push_date(details: &mut Vec<String>, date: String, index: &mut Option<usize>) {
    if !date.is_empty() {
        *index = Some(details.len());
        details.push(date);
    }
}

impl Item {
    pub fn date(&self) -> Option<&str> {
        self.date_detail_index
            .and_then(|index| self.details.get(index))
            .map(String::as_str)
    }

    pub fn other_details(&self) -> Vec<&str> {
        self.details
            .iter()
            .enumerate()
            .filter_map(|(index, detail)| {
                (Some(index) != self.date_detail_index).then_some(detail.as_str())
            })
            .collect()
    }
}

fn period(item: &Value, start: &str, end: &str) -> String {
    match (string(item, start), string(item, end)) {
        ("", "") => String::new(),
        (start, "") => start.to_owned(),
        ("", end) => end.to_owned(),
        (start, end) => format!("{start} – {end}"),
    }
}

fn header_fields(id: &str, source: &Value) -> BTreeMap<&'static str, String> {
    header_field_names(id)
        .iter()
        .filter_map(|&field| {
            let value = match field {
                "dateRange" => period(source, "startDate", "endDate"),
                "roles" | "keywords" => strings(source, field).join(", "),
                _ => string(source, field).to_owned(),
            };
            (!value.is_empty()).then_some((field, value))
        })
        .collect()
}

fn map_item(id: &str, source: &Value) -> Item {
    let heading = match id {
        "work" => joined(source, &["position", "name"], " · "),
        "volunteer" => joined(source, &["position", "organization"], " · "),
        "education" => joined(source, &["studyType", "area", "institution"], " · "),
        "awards" => string(source, "title").to_owned(),
        "certificates" | "publications" | "skills" | "interests" | "references" | "projects" => {
            string(source, "name").to_owned()
        }
        "languages" => string(source, "language").to_owned(),
        _ => String::new(),
    };
    let mut details = Vec::new();
    let mut date_detail_index = None;
    let mut paragraphs = Vec::new();
    let mut bullets = Vec::new();
    match id {
        "work" => {
            push_date(
                &mut details,
                period(source, "startDate", "endDate"),
                &mut date_detail_index,
            );
            maybe_push(&mut details, string(source, "location"));
            maybe_push(&mut paragraphs, string(source, "description"));
            maybe_push(&mut paragraphs, string(source, "summary"));
            bullets = strings(source, "highlights");
        }
        "volunteer" => {
            push_date(
                &mut details,
                period(source, "startDate", "endDate"),
                &mut date_detail_index,
            );
            maybe_push(&mut paragraphs, string(source, "summary"));
            bullets = strings(source, "highlights");
        }
        "education" => {
            push_date(
                &mut details,
                period(source, "startDate", "endDate"),
                &mut date_detail_index,
            );
            maybe_push(&mut details, string(source, "score"));
            bullets = strings(source, "courses");
        }
        "awards" => {
            maybe_push(&mut details, string(source, "awarder"));
            push_date(
                &mut details,
                string(source, "date").to_owned(),
                &mut date_detail_index,
            );
            maybe_push(&mut paragraphs, string(source, "summary"));
        }
        "certificates" => {
            maybe_push(&mut details, string(source, "issuer"));
            push_date(
                &mut details,
                string(source, "date").to_owned(),
                &mut date_detail_index,
            );
        }
        "publications" => {
            maybe_push(&mut details, string(source, "publisher"));
            push_date(
                &mut details,
                string(source, "releaseDate").to_owned(),
                &mut date_detail_index,
            );
            maybe_push(&mut paragraphs, string(source, "summary"));
        }
        "skills" => {
            maybe_push(&mut details, string(source, "level"));
            bullets = strings(source, "keywords");
        }
        "languages" => maybe_push(&mut details, string(source, "fluency")),
        "interests" => bullets = strings(source, "keywords"),
        "references" => maybe_push(&mut paragraphs, string(source, "reference")),
        "projects" => {
            push_date(
                &mut details,
                period(source, "startDate", "endDate"),
                &mut date_detail_index,
            );
            maybe_push(&mut details, string(source, "entity"));
            maybe_push(&mut details, string(source, "type"));
            maybe_push(&mut details, strings(source, "roles").join(", "));
            maybe_push(&mut paragraphs, string(source, "description"));
            bullets = strings(source, "highlights");
            maybe_push(&mut details, strings(source, "keywords").join(", "));
        }
        _ => {}
    }
    let url = string(source, "url");
    Item {
        heading,
        header_fields: header_fields(id, source),
        short_en_date_range: if header_field_names(id).contains(&"dateRange") {
            use crate::date_range::{RangeEnd, short_en_range};
            let end = match source.get("endDate") {
                None => RangeEnd::Missing,
                Some(Value::Null) => RangeEnd::Present,
                Some(Value::String(value)) => RangeEnd::Value(value),
                // Invalid typed values must not become invented date strings.
                _ => RangeEnd::Missing,
            };
            short_en_range(source["startDate"].as_str().unwrap_or_default(), end)
        } else {
            String::new()
        },
        details,
        date_detail_index,
        paragraphs,
        bullets,
        url: (!url.is_empty()).then(|| url.to_owned()),
    }
}

pub fn project(document: &ResumeDocument) -> ResumeContent {
    let value = &document.value;
    let basics = value.get("basics").unwrap_or(&Value::Null);
    let mut contacts = Vec::new();
    for field in ["email", "phone", "url"] {
        maybe_push(&mut contacts, string(basics, field));
    }
    let location = basics.get("location").unwrap_or(&Value::Null);
    maybe_push(
        &mut contacts,
        joined(
            location,
            &["address", "city", "region", "postalCode", "countryCode"],
            ", ",
        ),
    );
    if let Some(profiles) = basics.get("profiles").and_then(Value::as_array) {
        for profile in profiles {
            let label = joined(profile, &["network", "username"], " · ");
            let url = string(profile, "url");
            maybe_push(
                &mut contacts,
                match (label.is_empty(), url.is_empty()) {
                    (true, true) => String::new(),
                    (true, false) => url.to_owned(),
                    (false, true) => label,
                    (false, false) => format!("{label}: {url}"),
                },
            );
        }
    }
    let sections = value
        .as_object()
        .into_iter()
        .flat_map(|fields| fields.iter())
        .filter_map(|(key, section)| {
            let id = IDS.into_iter().find(|id| *id == key)?;
            let items = section.as_array()?;
            if items.is_empty() {
                return None;
            }
            Some(Section {
                id,
                items: items.iter().map(|item| map_item(id, item)).collect(),
            })
        })
        .collect();
    ResumeContent {
        name: string(basics, "name").to_owned(),
        label: string(basics, "label").to_owned(),
        contacts,
        summary: string(basics, "summary").to_owned(),
        sections,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resume::parse_resume;

    #[test]
    fn visual_short_dates_preserve_presence_without_changing_legacy_periods_or_source() {
        for id in ["work", "volunteer", "education", "projects"] {
            for (fields, expected, legacy) in [
                (
                    serde_json::json!({"startDate":"2020-06-01"}),
                    "Jun 2020",
                    "2020-06-01",
                ),
                (
                    serde_json::json!({"startDate":"2020-06","endDate":"2024-12"}),
                    "Jun 2020 - Dec 2024",
                    "2020-06 – 2024-12",
                ),
                (
                    serde_json::json!({"startDate":"2020","endDate":null}),
                    "Jan 2020 - Present",
                    "2020",
                ),
                (
                    serde_json::json!({"startDate":"2020","endDate":""}),
                    "Jan 2020 - Present",
                    "2020",
                ),
                (serde_json::json!({"endDate":"2024"}), "", "2024"),
                (
                    serde_json::json!({"startDate":"2024-02-31","endDate":"2024-03"}),
                    "2024-02-31 - Mar 2024",
                    "2024-02-31 – 2024-03",
                ),
                (
                    serde_json::json!({"startDate":"2020","endDate":{}}),
                    "Jan 2020",
                    "2020",
                ),
            ] {
                let original = fields.clone();
                let item = map_item(id, &fields);
                assert_eq!(item.short_en_date_range, expected, "{id}/{fields}");
                assert_eq!(item.date().unwrap_or_default(), legacy);
                assert_eq!(fields, original);
            }
        }
        // The visual helper is defensive; it does not relax the official
        // document schema to accept null/empty/incorrectly typed date input.
        assert!(parse_resume(br#"{"work":[{"startDate":"2020","endDate":null}]}"#).is_err());
        for id in [
            "awards",
            "certificates",
            "publications",
            "skills",
            "references",
        ] {
            let item = map_item(
                id,
                &serde_json::json!({"startDate":"2020","date":"2024-03"}),
            );
            assert!(item.short_en_date_range.is_empty());
        }
    }

    #[test]
    fn header_fields_map_every_standard_module_without_changing_source_or_text_exports() {
        let source = br#"{
          "work":[{"position":" Lead ","name":"Work Org","startDate":"2020-01","endDate":"2022-12","location":" Remote ","description":"Work description","summary":"Work summary","highlights":["Work bullet"],"url":"https://example.com/work","customHeader":"private work extension"}],
          "volunteer":[{"position":"Mentor","organization":"Volunteer Org","startDate":"2021-01","endDate":"2023-12","summary":"Volunteer summary","highlights":["Volunteer bullet"],"url":"https://example.com/volunteer"}],
          "education":[{"institution":"University","studyType":"MSc","area":"Computing","score":"4.0","startDate":"2015-09","endDate":"2019-06","courses":["Algorithms"],"url":"https://example.com/education"}],
          "awards":[{"title":"Prize","awarder":"Award Org","date":"2024-01","summary":"Award summary"}],
          "certificates":[{"name":"Certificate","issuer":"Issuer","date":"2024-02","url":"https://example.com/certificate"}],
          "publications":[{"name":"Article","publisher":"Publisher","releaseDate":"2024-03","summary":"Publication summary","url":"https://example.com/publication"}],
          "skills":[{"name":"Rust","level":"Advanced","keywords":["Ownership","Testing"]}],
          "languages":[{"language":"English","fluency":"Native"}],
          "interests":[{"name":"Design","keywords":["Typography"]}],
          "references":[{"name":"Reviewer","reference":"Reference paragraph"}],
          "projects":[{"name":"Project","startDate":"2022-01","endDate":"2024-12","entity":"Project Org","type":"Library","roles":[" Design ","","Delivery","  "],"keywords":[" Rust ","","CLI","  "],"description":"Project description","highlights":["Project bullet"],"url":"https://example.com/project","customHeader":"private project extension"}],
          "custom":{"name":"Private module"}
        }"#;
        let doc = parse_resume(source).unwrap();
        let original_value = doc.value.clone();
        let original_revision = doc.revision.clone();
        let content = project(&doc);
        let expected: &[(&str, &[(&str, &str)])] = &[
            (
                "work",
                &[
                    ("position", "Lead"),
                    ("name", "Work Org"),
                    ("dateRange", "2020-01 – 2022-12"),
                    ("location", "Remote"),
                ],
            ),
            (
                "volunteer",
                &[
                    ("position", "Mentor"),
                    ("organization", "Volunteer Org"),
                    ("dateRange", "2021-01 – 2023-12"),
                ],
            ),
            (
                "education",
                &[
                    ("institution", "University"),
                    ("studyType", "MSc"),
                    ("area", "Computing"),
                    ("score", "4.0"),
                    ("dateRange", "2015-09 – 2019-06"),
                ],
            ),
            (
                "awards",
                &[
                    ("title", "Prize"),
                    ("awarder", "Award Org"),
                    ("date", "2024-01"),
                ],
            ),
            (
                "certificates",
                &[
                    ("name", "Certificate"),
                    ("issuer", "Issuer"),
                    ("date", "2024-02"),
                ],
            ),
            (
                "publications",
                &[
                    ("name", "Article"),
                    ("publisher", "Publisher"),
                    ("releaseDate", "2024-03"),
                ],
            ),
            ("skills", &[("name", "Rust"), ("level", "Advanced")]),
            (
                "languages",
                &[("language", "English"), ("fluency", "Native")],
            ),
            ("interests", &[("name", "Design")]),
            ("references", &[("name", "Reviewer")]),
            (
                "projects",
                &[
                    ("name", "Project"),
                    ("dateRange", "2022-01 – 2024-12"),
                    ("entity", "Project Org"),
                    ("type", "Library"),
                    ("roles", "Design, Delivery"),
                    ("keywords", "Rust, CLI"),
                ],
            ),
        ];
        assert_eq!(content.sections.len(), expected.len());
        for (section, (id, fields)) in content.sections.iter().zip(expected) {
            assert_eq!(section.id, *id);
            assert_eq!(section.items.len(), 1);
            assert_eq!(
                header_field_names(id),
                fields.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
                "{id} validation whitelist"
            );
            let item = &section.items[0];
            let expected_map: BTreeMap<_, _> = fields
                .iter()
                .map(|(key, value)| (*key, value.to_string()))
                .collect();
            assert_eq!(item.header_fields, expected_map, "{id}");
            let date = fields.iter().find_map(|(key, value)| {
                matches!(*key, "dateRange" | "date" | "releaseDate").then_some(*value)
            });
            assert_eq!(item.date(), date, "{id} keeps the existing date projection");
        }
        // The new map is additional presentation input, not replacement text.
        assert_eq!(content.sections[0].items[0].heading, "Lead · Work Org");
        assert_eq!(content.sections[0].items[0].other_details(), ["Remote"]);
        assert_eq!(
            content.sections[0].items[0].paragraphs,
            ["Work description", "Work summary"]
        );
        assert_eq!(content.sections[0].items[0].bullets, ["Work bullet"]);
        assert_eq!(
            content.sections[0].items[0].url.as_deref(),
            Some("https://example.com/work")
        );
        assert_eq!(
            content.sections[2].items[0].heading,
            "MSc · Computing · University"
        );
        assert_eq!(
            content.sections[10].items[0].details,
            [
                "2022-01 – 2024-12",
                "Project Org",
                "Library",
                "Design, Delivery",
                "Rust, CLI"
            ]
        );
        let mut legacy = content.clone();
        for section in &mut legacy.sections {
            for item in &mut section.items {
                item.header_fields.clear();
            }
        }
        for language in [crate::render::Language::En, crate::render::Language::Zh] {
            assert_eq!(
                crate::render::plain_text(&content, language),
                crate::render::plain_text(&legacy, language)
            );
            assert_eq!(
                crate::render::markdown(&content, language),
                crate::render::markdown(&legacy, language)
            );
        }
        assert_eq!(doc.source, source);
        assert_eq!(doc.value, original_value);
        assert_eq!(doc.revision, original_revision);
    }

    #[test]
    fn header_fields_omit_empty_and_non_string_values_and_never_copy_extensions() {
        for id in IDS {
            for empty in [
                serde_json::json!(""),
                serde_json::json!(" \t\n "),
                Value::Null,
                serde_json::json!(false),
                serde_json::json!(42),
                serde_json::json!({"nested": "not a scalar header"}),
                serde_json::json!(["not a scalar header"]),
            ] {
                let mut source = serde_json::json!({
                    "summary":"Summary is not a header",
                    "description":"Description is not a header",
                    "reference":"Reference is not a header",
                    "highlights":["Bullet is not a header"],
                    "courses":["Course is not a header"],
                    "url":"https://example.com/not-a-header",
                    "customHeader":"Unknown extension is not a header",
                    "header_fields":{"name":"Do not trust a supplied map"},
                    "dateRange":"Do not trust a supplied computed field",
                    "roles":["", "  ", null, false, 42],
                    "keywords":["", "  ", null, false, 42]
                });
                for field in header_field_names(id) {
                    match *field {
                        "dateRange" => {
                            source["startDate"] = empty.clone();
                            source["endDate"] = empty.clone();
                        }
                        "roles" | "keywords" => {}
                        _ => source[*field] = empty.clone(),
                    }
                }
                let original = source.clone();
                assert!(
                    map_item(id, &source).header_fields.is_empty(),
                    "{id}: {empty}"
                );
                assert_eq!(source, original);
            }
        }
        for id in ["", "basics", "custom", "Work", "work.summary"] {
            assert!(header_field_names(id).is_empty(), "{id}");
            assert!(
                map_item(
                    id,
                    &serde_json::json!({"name":"Private", "title":"Private"})
                )
                .header_fields
                .is_empty()
            );
        }
    }

    #[test]
    fn header_date_ranges_reuse_period_and_project_arrays_reuse_existing_joining() {
        for id in ["work", "volunteer", "education", "projects"] {
            for (start, end, expected) in [
                ("", "", None),
                (" 2020-01 ", "", Some("2020-01")),
                ("", " 2022-12 ", Some("2022-12")),
                (" 2020-01 ", " 2022-12 ", Some("2020-01 – 2022-12")),
            ] {
                let source = serde_json::json!({
                    "startDate":start, "endDate":end, "dateRange":"Ignored extension"
                });
                let item = map_item(id, &source);
                assert_eq!(
                    item.header_fields.get("dateRange").map(String::as_str),
                    expected
                );
                assert_eq!(item.date(), expected);
                assert!(!item.header_fields.contains_key("startDate"));
                assert!(!item.header_fields.contains_key("endDate"));
            }
        }
        let source = serde_json::json!({
            "roles":[" Lead ", "", null, 12, " Reviewer ", "  "],
            "keywords":[" Rust ", false, "", " CLI "]
        });
        let item = map_item("projects", &source);
        assert_eq!(item.header_fields["roles"], "Lead, Reviewer");
        assert_eq!(item.header_fields["keywords"], "Rust, CLI");
        assert_eq!(item.details, ["Lead, Reviewer", "Rust, CLI"]);
        for id in IDS.into_iter().filter(|id| *id != "projects") {
            let item = map_item(id, &source);
            assert!(!item.header_fields.contains_key("roles"), "{id}");
            assert!(!item.header_fields.contains_key("keywords"), "{id}");
        }
    }

    #[test]
    fn projects_every_standard_section_and_preserves_source() {
        let source = br#"{"basics":{"name":"Ada","profiles":[{"network":"GitHub","username":"ada","url":"https://example.com"}]},"work":[{"name":"Org","position":"Engineer","highlights":["Built it"]}],"volunteer":[{}],"education":[{}],"awards":[{}],"certificates":[{}],"publications":[{}],"skills":[{}],"languages":[{}],"interests":[{}],"references":[{}],"projects":[{}],"custom":{"private":true}}"#;
        let doc = parse_resume(source).unwrap();
        let content = project(&doc);
        assert_eq!(content.sections.len(), 11);
        assert_eq!(content.sections[0].items[0].heading, "Engineer · Org");
        assert_eq!(content.sections[0].items[0].bullets, ["Built it"]);
        assert!(content.contacts[0].contains("GitHub"));
        assert_eq!(doc.source, source);
    }

    #[test]
    fn preserves_resume_module_order_for_all_exports() {
        let source = br#"{"basics":{},"projects":[{"name":"One"}],"education":[{"institution":"Two"}],"work":[{"name":"Three"}]}"#;
        let content = project(&parse_resume(source).unwrap());
        assert_eq!(
            content
                .sections
                .iter()
                .map(|section| section.id)
                .collect::<Vec<_>>(),
            ["projects", "education", "work"]
        );
    }

    #[test]
    fn marks_date_without_reordering_other_details_or_text_exports() {
        let source = r#"{"work":[{"name":"Acme","startDate":"2020-01","endDate":"2022-12","location":"北京"}],"awards":[{"title":"Prize","awarder":"2024-01","date":"2024-02"}],"certificates":[{"name":"Cert","date":"2025-01"}],"skills":[{"name":"Rust","level":"Advanced"}]}"#;
        let content = project(&parse_resume(source.as_bytes()).unwrap());
        let work = &content.sections[0].items[0];
        assert_eq!(work.date(), Some("2020-01 – 2022-12"));
        assert_eq!(work.other_details(), ["北京"]);
        let award = &content.sections[1].items[0];
        assert_eq!(award.date_detail_index, Some(1));
        assert_eq!(award.other_details(), ["2024-01"]);
        assert_eq!(content.sections[2].items[0].date_detail_index, Some(0));
        assert_eq!(content.sections[3].items[0].date(), None);
        let plain = crate::render::plain_text(&content, crate::render::Language::En);
        assert!(plain.contains("2020-01 – 2022-12"));
        assert!(plain.contains("2024-01\n2024-02"));
    }
}
