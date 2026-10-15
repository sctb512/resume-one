//! Schema-aware edits for plain-text fields. This is shared by agent commands
//! and inline preview forms; it never generates presentation markup.

use serde_json::json;

use crate::resume::{MAX_RESUME_BYTES, ResumeDocument, parse_resume};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEdit {
    pub section: String,
    pub index: Option<usize>,
    pub field: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEditError {
    pub code: &'static str,
    pub message: String,
}

fn error(code: &'static str, message: impl Into<String>) -> TextEditError {
    TextEditError {
        code,
        message: message.into(),
    }
}

pub fn text_fields(section: &str) -> &'static [&'static str] {
    match section {
        "basics" => &["name", "label", "image", "email", "phone", "url", "summary"],
        "work" => &[
            "name",
            "location",
            "description",
            "position",
            "url",
            "startDate",
            "endDate",
            "summary",
        ],
        "volunteer" => &[
            "organization",
            "position",
            "url",
            "startDate",
            "endDate",
            "summary",
        ],
        "education" => &[
            "institution",
            "url",
            "area",
            "studyType",
            "startDate",
            "endDate",
            "score",
        ],
        "awards" => &["title", "date", "awarder", "summary"],
        "certificates" => &["name", "date", "url", "issuer"],
        "publications" => &["name", "publisher", "releaseDate", "url", "summary"],
        "skills" => &["name", "level"],
        "languages" => &["language", "fluency"],
        "interests" => &["name"],
        "references" => &["name", "reference"],
        "projects" => &[
            "name",
            "description",
            "startDate",
            "endDate",
            "url",
            "entity",
            "type",
        ],
        _ => &[],
    }
}

impl TextEdit {
    pub fn pointer(&self) -> String {
        match self.index {
            Some(index) => format!("/{}/{index}/{}", self.section, self.field),
            None => format!("/{}/{}", self.section, self.field),
        }
    }
}

/// Set or clear a standard string field in a valid document. An unchanged edit
/// returns the original document, preserving its exact source bytes and revision.
pub fn apply_text_edit(
    document: &ResumeDocument,
    edit: &TextEdit,
) -> Result<ResumeDocument, TextEditError> {
    let source = prepare_text_edit(&document.source, edit)?;
    if source == document.source {
        return Ok(document.clone());
    }
    parse_resume(&source).map_err(|issues| {
        error(
            "RESULT_INVALID",
            format!("{}：{}", edit.pointer(), issues[0].message),
        )
    })
}

/// Prepare an editable draft, including intermediate email/date values. The UI
/// must validate this draft before saving; CLI writes use `apply_text_edit`.
pub fn prepare_text_edit(source: &[u8], edit: &TextEdit) -> Result<Vec<u8>, TextEditError> {
    if source.len() > MAX_RESUME_BYTES || edit.value.len() > MAX_RESUME_BYTES {
        return Err(error("RESUME_TOO_LARGE", "简历文件超过 16 MiB 上限"));
    }
    if !text_fields(&edit.section).contains(&edit.field.as_str()) {
        return Err(error("UNKNOWN_FIELD", "不是支持的标准文本字段"));
    }
    if (edit.section == "basics") != edit.index.is_none() {
        return Err(error(
            "ITEM_INDEX",
            "基本信息不使用条目序号；其他模块必须指定序号",
        ));
    }
    let mut value: serde_json::Value = serde_json::from_slice(source)
        .map_err(|_| error("INVALID_JSON", "请先修复 JSON 语法错误"))?;
    let section = value
        .get_mut(&edit.section)
        .ok_or_else(|| error("SECTION_MISSING", "模块不存在"))?;
    let item = match edit.index {
        None => section,
        Some(index) => section
            .as_array_mut()
            .and_then(|items| items.get_mut(index))
            .ok_or_else(|| error("ITEM_INDEX", "条目不存在"))?,
    };
    let fields = item
        .as_object_mut()
        .ok_or_else(|| error("ITEM_INVALID", "模块或条目不是对象"))?;
    let next = if edit.value.is_empty() {
        None
    } else {
        Some(json!(edit.value))
    };
    if fields.get(&edit.field) == next.as_ref() {
        return Ok(source.to_vec());
    }
    match next {
        Some(value) => {
            fields.insert(edit.field.clone(), value);
        }
        None => {
            fields.shift_remove(&edit.field);
        }
    }
    let mut source = serde_json::to_vec_pretty(&value)
        .map_err(|reason| error("SERIALIZE", reason.to_string()))?;
    source.push(b'\n');
    if source.len() > MAX_RESUME_BYTES {
        return Err(error("RESUME_TOO_LARGE", "简历文件超过 16 MiB 上限"));
    }
    Ok(source)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> ResumeDocument {
        parse_resume(br#"{"basics":{"name":"A","email":"a@example.com"},"work":[{"name":"Old","x-extra":{"html":"<br>"}}],"x-root":9}"#).unwrap()
    }

    #[test]
    fn changes_only_plain_text_and_preserves_extensions() {
        let old = base();
        let edit = TextEdit {
            section: "work".into(),
            index: Some(0),
            field: "name".into(),
            value: "新工作室".into(),
        };
        let next = apply_text_edit(&old, &edit).unwrap();
        assert_eq!(next.value["work"][0]["name"], "新工作室");
        assert_eq!(next.value["work"][0]["x-extra"]["html"], "<br>");
        assert_eq!(next.value["x-root"], 9);
        assert_eq!(apply_text_edit(&next, &edit).unwrap().source, next.source);
        assert_eq!(old.value["work"][0]["name"], "Old");
    }

    #[test]
    fn clearing_field_removes_key_without_changing_neighbors() {
        let old = base();
        let next = apply_text_edit(
            &old,
            &TextEdit {
                section: "basics".into(),
                index: None,
                field: "email".into(),
                value: String::new(),
            },
        )
        .unwrap();
        assert!(next.value["basics"].get("email").is_none());
        assert_eq!(next.value["basics"]["name"], "A");
    }

    #[test]
    fn rejects_wrong_field_index_and_invalid_schema_value() {
        let old = base();
        let mut edit = TextEdit {
            section: "basics".into(),
            index: None,
            field: "email".into(),
            value: "broken".into(),
        };
        assert_eq!(
            apply_text_edit(&old, &edit).unwrap_err().code,
            "RESULT_INVALID"
        );
        edit.field = "customHtml".into();
        assert_eq!(
            apply_text_edit(&old, &edit).unwrap_err().code,
            "UNKNOWN_FIELD"
        );
        edit.field = "name".into();
        edit.index = Some(0);
        assert_eq!(apply_text_edit(&old, &edit).unwrap_err().code, "ITEM_INDEX");
        edit.section = "work".into();
        edit.index = Some(2);
        assert_eq!(apply_text_edit(&old, &edit).unwrap_err().code, "ITEM_INDEX");
        assert_eq!(old.source, base().source);
    }

    #[test]
    fn editable_draft_retains_intermediate_values_until_repaired() {
        let old = base();
        let mut edit = TextEdit {
            section: "basics".into(),
            index: None,
            field: "email".into(),
            value: "unfinished".into(),
        };
        let draft = prepare_text_edit(&old.source, &edit).unwrap();
        assert!(parse_resume(&draft).is_err());
        edit.field = "name".into();
        edit.value = "新名字".into();
        let draft = prepare_text_edit(&draft, &edit).unwrap();
        edit.field = "email".into();
        edit.value = "new@example.com".into();
        let repaired = parse_resume(&prepare_text_edit(&draft, &edit).unwrap()).unwrap();
        assert_eq!(repaired.value["basics"]["name"], "新名字");
        assert_eq!(repaired.value["basics"]["email"], "new@example.com");
    }
}
