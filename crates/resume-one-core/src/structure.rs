//! Schema-safe structural edits used by both visual and JSON editing surfaces.
//! They change only standard section containers and preserve unrelated fields.

use serde_json::{Value, json};

use crate::resume::{ResumeDocument, parse_resume};

const SECTIONS: &[&str] = &[
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
pub enum StructureEdit {
    InsertSection { id: String, before: Option<String> },
    RemoveSection { id: String },
    InsertItem { id: String, index: usize },
    RemoveItem { id: String, index: usize },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructureError {
    pub code: &'static str,
    pub message: String,
}

fn error(code: &'static str, message: impl Into<String>) -> StructureError {
    StructureError {
        code,
        message: message.into(),
    }
}

fn supported(id: &str) -> Result<(), StructureError> {
    if SECTIONS.contains(&id) {
        Ok(())
    } else {
        Err(error("UNKNOWN_SECTION", format!("不是标准简历模块：{id}")))
    }
}

/// Apply one operation to a valid resume without touching its source file.
/// The returned document is a new, formatted revision; the caller owns undo.
pub fn apply_structure(
    document: &ResumeDocument,
    edit: &StructureEdit,
) -> Result<ResumeDocument, StructureError> {
    let mut value = document.value.clone();
    let fields = value
        .as_object_mut()
        .ok_or_else(|| error("INVALID_ROOT", "简历根节点必须是对象"))?;
    match edit {
        StructureEdit::InsertSection { id, before } => {
            supported(id)?;
            if fields.contains_key(id) {
                return Err(error("SECTION_EXISTS", format!("模块已存在：{id}")));
            }
            let index = if let Some(before) = before {
                supported(before)?;
                fields
                    .keys()
                    .position(|key| key == before)
                    .ok_or_else(|| error("ANCHOR_MISSING", format!("插入位置不存在：{before}")))?
            } else {
                fields.len()
            };
            let section = if id == "basics" { json!({}) } else { json!([]) };
            fields.shift_insert(index, id.clone(), section);
        }
        StructureEdit::RemoveSection { id } => {
            supported(id)?;
            fields
                .shift_remove(id)
                .ok_or_else(|| error("SECTION_MISSING", format!("模块不存在：{id}")))?;
        }
        StructureEdit::InsertItem { id, index } => {
            supported(id)?;
            if id == "basics" {
                return Err(error("SINGLETON_SECTION", "基本信息不是条目列表"));
            }
            let array = fields
                .get_mut(id)
                .and_then(Value::as_array_mut)
                .ok_or_else(|| error("SECTION_MISSING", format!("模块不存在：{id}")))?;
            if *index > array.len() {
                return Err(error("ITEM_INDEX", "条目插入位置超出范围"));
            }
            array.insert(*index, json!({}));
        }
        StructureEdit::RemoveItem { id, index } => {
            supported(id)?;
            if id == "basics" {
                return Err(error("SINGLETON_SECTION", "基本信息不是条目列表"));
            }
            let array = fields
                .get_mut(id)
                .and_then(Value::as_array_mut)
                .ok_or_else(|| error("SECTION_MISSING", format!("模块不存在：{id}")))?;
            if *index >= array.len() {
                return Err(error("ITEM_INDEX", "条目不存在"));
            }
            array.remove(*index);
        }
    }
    let mut source = serde_json::to_vec_pretty(&value)
        .map_err(|reason| error("SERIALIZE", reason.to_string()))?;
    source.push(b'\n');
    parse_resume(&source).map_err(|issues| {
        error(
            "RESULT_INVALID",
            format!("编辑结果不符合 JSON Resume Schema：{}", issues[0].message),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> ResumeDocument {
        parse_resume(
            br#"{"basics":{"name":"A"},"custom":{"html":"<br>"},"work":[{"name":"First"},{"name":"Second"}],"education":[{}]}"#,
        )
        .unwrap()
    }

    #[test]
    fn insert_and_remove_sections_preserve_order_and_extensions() {
        let base = base();
        let inserted = apply_structure(
            &base,
            &StructureEdit::InsertSection {
                id: "skills".into(),
                before: Some("education".into()),
            },
        )
        .unwrap();
        let keys: Vec<_> = inserted
            .value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, ["basics", "custom", "work", "skills", "education"]);
        assert_eq!(inserted.value["custom"]["html"], "<br>");
        assert_eq!(inserted.value["skills"], json!([]));
        assert_eq!(base.value["skills"], Value::Null);
        let removed = apply_structure(
            &inserted,
            &StructureEdit::RemoveSection { id: "work".into() },
        )
        .unwrap();
        let keys: Vec<_> = removed
            .value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, ["basics", "custom", "skills", "education"]);
        assert_eq!(removed.value["custom"], base.value["custom"]);
    }

    #[test]
    fn items_insert_at_exact_boundaries_without_changing_neighbors() {
        let base = base();
        let first = apply_structure(
            &base,
            &StructureEdit::InsertItem {
                id: "work".into(),
                index: 0,
            },
        )
        .unwrap();
        assert_eq!(
            first.value["work"],
            json!([{}, {"name":"First"}, {"name":"Second"}])
        );
        let end = apply_structure(
            &first,
            &StructureEdit::InsertItem {
                id: "work".into(),
                index: 3,
            },
        )
        .unwrap();
        assert_eq!(end.value["work"][3], json!({}));
        let removed = apply_structure(
            &end,
            &StructureEdit::RemoveItem {
                id: "work".into(),
                index: 1,
            },
        )
        .unwrap();
        assert_eq!(removed.value["work"], json!([{}, {"name":"Second"}, {}]));
        assert_eq!(base.value["work"][0]["name"], "First");
    }

    #[test]
    fn rejects_invalid_operations_without_mutating_input() {
        let base = base();
        let source = base.source.clone();
        let cases = [
            (
                StructureEdit::InsertSection {
                    id: "work".into(),
                    before: None,
                },
                "SECTION_EXISTS",
            ),
            (
                StructureEdit::RemoveSection {
                    id: "skills".into(),
                },
                "SECTION_MISSING",
            ),
            (
                StructureEdit::InsertSection {
                    id: "private".into(),
                    before: None,
                },
                "UNKNOWN_SECTION",
            ),
            (
                StructureEdit::InsertSection {
                    id: "skills".into(),
                    before: Some("awards".into()),
                },
                "ANCHOR_MISSING",
            ),
            (
                StructureEdit::InsertItem {
                    id: "basics".into(),
                    index: 0,
                },
                "SINGLETON_SECTION",
            ),
            (
                StructureEdit::RemoveItem {
                    id: "work".into(),
                    index: 2,
                },
                "ITEM_INDEX",
            ),
        ];
        for (edit, expected) in cases {
            assert_eq!(apply_structure(&base, &edit).unwrap_err().code, expected);
            assert_eq!(base.source, source);
        }
    }
}
