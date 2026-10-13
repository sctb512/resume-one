//! Versioned, read-only JSON Resume document handling.
//!
//! The official JSON Resume schema is embedded; validation never fetches a remote schema.

use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::OnceLock;

use jsonschema::Validator;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::Diagnostic;

pub const JSON_RESUME_SCHEMA_VERSION: &str = "1.3.1";
pub const JSON_RESUME_SCHEMA: &str = include_str!("../../../spec/json-resume-1.3.1.schema.json");
pub const MAX_RESUME_BYTES: usize = 16 * 1024 * 1024;
static RESUME_VALIDATOR: OnceLock<Validator> = OnceLock::new();

const SECTION_IDS: [&str; 12] = [
    "basics",
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionSummary {
    pub id: String,
    pub count: usize,
}

#[derive(Debug, Clone)]
pub struct ResumeDocument {
    /// The user's complete parsed JSON; unknown fields and top-level order are retained.
    pub value: Value,
    /// SHA-256 of the exact input bytes, including whitespace and line endings.
    pub revision: String,
    /// Exact original bytes until the document is explicitly edited.
    pub source: Vec<u8>,
}

fn validator() -> &'static Validator {
    RESUME_VALIDATOR.get_or_init(|| {
        jsonschema::draft7::options()
            .should_validate_formats(true)
            .build(&serde_json::from_str(JSON_RESUME_SCHEMA).expect("bundled resume schema JSON"))
            .expect("bundled resume schema validity")
    })
}

/// Parse and validate a JSON Resume without rewriting the source.
pub fn parse_resume(bytes: &[u8]) -> Result<ResumeDocument, Vec<Diagnostic>> {
    if bytes.len() > MAX_RESUME_BYTES {
        return Err(vec![issue(
            "RESUME_TOO_LARGE",
            "",
            "简历文件超过 16 MiB 上限",
        )]);
    }
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|error| vec![issue("INVALID_JSON", "", format!("JSON 语法无效：{error}"))])?;
    let errors: Vec<_> = validator()
        .iter_errors(&value)
        .map(|error| {
            issue(
                "RESUME_SCHEMA",
                error.instance_path().to_string(),
                format!("字段不符合 Schema 的 {} 规则", error.kind().keyword()),
            )
        })
        .collect();
    if !errors.is_empty() {
        return Err(errors);
    }
    Ok(ResumeDocument {
        value,
        revision: revision(bytes),
        source: bytes.to_vec(),
    })
}

pub fn load_resume(path: &Path) -> Result<ResumeDocument, Vec<Diagnostic>> {
    let mut source = Vec::new();
    File::open(path)
        .and_then(|file| {
            file.take(MAX_RESUME_BYTES as u64 + 1)
                .read_to_end(&mut source)
        })
        .map_err(|error| vec![issue("RESUME_IO", "", error.to_string())])?;
    parse_resume(&source)
}

/// The caller must explicitly opt into reading sensitive resume content.
pub fn read_pointer<'a>(document: &'a ResumeDocument, pointer: &str) -> Option<&'a Value> {
    document.value.pointer(pointer)
}

pub fn revision(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

impl ResumeDocument {
    pub fn sections(&self) -> Vec<SectionSummary> {
        self.value
            .as_object()
            .into_iter()
            .flat_map(|fields| fields.iter())
            .filter_map(|(key, value)| {
                if !SECTION_IDS.contains(&key.as_str()) {
                    return None;
                }
                let count = if key == "basics" {
                    1
                } else {
                    value.as_array().map_or(0, Vec::len)
                };
                Some(SectionSummary {
                    id: key.clone(),
                    count,
                })
            })
            .collect()
    }
}

fn issue(code: &'static str, path: impl Into<String>, message: impl Into<String>) -> Diagnostic {
    Diagnostic {
        code,
        path: path.into(),
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;

    #[test]
    fn validates_content_and_preserves_unknown_fields_and_order() {
        let source =
            r#"{"basics":{"name":"李明"},"custom":{"note":"keep"},"work":[{"name":"示例公司"}]}"#
                .as_bytes();
        let document = parse_resume(source).unwrap();
        assert_eq!(document.source, source);
        assert_eq!(
            read_pointer(&document, "/custom/note"),
            Some(&json!("keep"))
        );
        assert_eq!(
            document.sections(),
            vec![
                SectionSummary {
                    id: "basics".into(),
                    count: 1
                },
                SectionSummary {
                    id: "work".into(),
                    count: 1
                },
            ]
        );
        assert_eq!(
            document
                .value
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["basics", "custom", "work"]
        );
    }

    #[test]
    fn whitespace_and_line_endings_change_the_revision() {
        let compact = parse_resume(br#"{"basics":{}}"#).unwrap();
        let spaced = parse_resume(b"{ \"basics\": {} }\r\n").unwrap();
        assert_ne!(compact.revision, spaced.revision);
    }

    #[test]
    fn rejects_schema_errors_with_field_location() {
        let invalid = parse_resume(br#"{"work":[{"name":7}]}"#).unwrap_err();
        assert!(
            invalid
                .iter()
                .any(|item| item.code == "RESUME_SCHEMA" && item.path == "/work/0/name")
        );
    }

    #[test]
    fn rejects_invalid_dates_and_emails() {
        let date = parse_resume(br#"{"work":[{"startDate":"yesterday"}]}"#).unwrap_err();
        assert!(date.iter().any(|item| item.path == "/work/0/startDate"));
        let email = parse_resume(br#"{"basics":{"email":"not-an-email"}}"#).unwrap_err();
        assert!(email.iter().any(|item| item.path == "/basics/email"));
    }

    #[test]
    fn does_not_accept_invalid_or_oversized_documents() {
        assert_eq!(parse_resume(b"{").unwrap_err()[0].code, "INVALID_JSON");
        assert_eq!(
            parse_resume(&vec![b' '; MAX_RESUME_BYTES + 1]).unwrap_err()[0].code,
            "RESUME_TOO_LARGE"
        );
    }

    #[test]
    fn loads_a_local_file_without_rewriting_it() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("履历.json");
        let source = b"{\n  \"basics\": {\"name\": \"Example\"}\n}\n";
        fs::write(&path, source).unwrap();
        let document = load_resume(&path).unwrap();
        assert_eq!(document.source, source);
        assert_eq!(fs::read(&path).unwrap(), source);
        assert_eq!(document.revision, revision(source));
    }
}
