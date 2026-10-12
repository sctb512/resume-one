//! Theme-independent semantic projection of the standard JSON Resume sections.
//! Unknown extension fields stay in the source document but are not exported implicitly.

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
    pub details: Vec<String>,
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

fn period(item: &Value, start: &str, end: &str) -> String {
    match (string(item, start), string(item, end)) {
        ("", "") => String::new(),
        (start, "") => start.to_owned(),
        ("", end) => end.to_owned(),
        (start, end) => format!("{start} – {end}"),
    }
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
    let mut paragraphs = Vec::new();
    let mut bullets = Vec::new();
    match id {
        "work" => {
            maybe_push(&mut details, period(source, "startDate", "endDate"));
            maybe_push(&mut details, string(source, "location"));
            maybe_push(&mut paragraphs, string(source, "description"));
            maybe_push(&mut paragraphs, string(source, "summary"));
            bullets = strings(source, "highlights");
        }
        "volunteer" => {
            maybe_push(&mut details, period(source, "startDate", "endDate"));
            maybe_push(&mut paragraphs, string(source, "summary"));
            bullets = strings(source, "highlights");
        }
        "education" => {
            maybe_push(&mut details, period(source, "startDate", "endDate"));
            maybe_push(&mut details, string(source, "score"));
            bullets = strings(source, "courses");
        }
        "awards" => {
            maybe_push(&mut details, string(source, "awarder"));
            maybe_push(&mut details, string(source, "date"));
            maybe_push(&mut paragraphs, string(source, "summary"));
        }
        "certificates" => {
            maybe_push(&mut details, string(source, "issuer"));
            maybe_push(&mut details, string(source, "date"));
        }
        "publications" => {
            maybe_push(&mut details, string(source, "publisher"));
            maybe_push(&mut details, string(source, "releaseDate"));
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
            maybe_push(&mut details, period(source, "startDate", "endDate"));
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
        details,
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
}
