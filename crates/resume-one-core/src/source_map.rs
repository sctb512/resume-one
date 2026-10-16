//! Read-only UTF-8 anchors for structural controls. Never rewrites source text.
use std::collections::HashSet;

use serde_json::Value;

use crate::{resume::MAX_RESUME_BYTES, structure::StructureEdit};

pub const MAX_SOURCE_ANCHORS: usize = 50_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceAction {
    ModuleGap { before: Option<String> },
    Edit(StructureEdit),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Anchor {
    pub offset: usize,
    pub action: SourceAction,
}

impl Anchor {
    pub fn adds(&self) -> bool {
        matches!(
            self.action,
            SourceAction::ModuleGap { .. } | SourceAction::Edit(StructureEdit::InsertItem { .. })
        )
    }
}

struct Field {
    id: String,
    key: usize,
    start: usize,
    end: usize,
}

fn whitespace(bytes: &[u8], index: &mut usize) {
    while bytes.get(*index).is_some_and(u8::is_ascii_whitespace) {
        *index += 1;
    }
}

// Called only after serde_json validates syntax and bounds recursion/size.
fn string_end(bytes: &[u8], mut index: usize) -> usize {
    index += 1;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index += 2,
            b'"' => return index + 1,
            _ => index += 1,
        }
    }
    index
}

fn value_end(bytes: &[u8], mut index: usize) -> usize {
    if bytes[index] == b'"' {
        return string_end(bytes, index);
    }
    if !matches!(bytes[index], b'{' | b'[') {
        while bytes
            .get(index)
            .is_some_and(|byte| !byte.is_ascii_whitespace() && !b",]}".contains(byte))
        {
            index += 1;
        }
        return index;
    }
    let mut depth = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'"' => {
                index = string_end(bytes, index);
                continue;
            }
            b'{' | b'[' => depth += 1,
            b'}' | b']' => {
                depth -= 1;
                if depth == 0 {
                    return index + 1;
                }
            }
            _ => {}
        }
        index += 1;
    }
    index
}

/// Syntax must be valid and the root an object. Ambiguous duplicate root keys
/// are rejected, including escaped spellings of an otherwise identical key.
/// Schema validation remains the caller's responsibility.
pub fn anchors(source: &str) -> Option<Vec<Anchor>> {
    if source.len() > MAX_RESUME_BYTES {
        return None;
    }
    let value: Value = serde_json::from_str(source).ok()?;
    let object = value.as_object()?;
    let bytes = source.as_bytes();
    let mut index = 0;
    whitespace(bytes, &mut index);
    let root = index;
    index += 1;
    let mut fields = Vec::new();
    let mut keys = HashSet::new();
    loop {
        whitespace(bytes, &mut index);
        if bytes[index] == b'}' {
            break;
        }
        let key = index;
        let end = string_end(bytes, index);
        let id: String = serde_json::from_str(&source[key..end]).ok()?;
        if !keys.insert(id.clone()) {
            return None;
        }
        index = end;
        whitespace(bytes, &mut index);
        index += 1; // colon
        whitespace(bytes, &mut index);
        let start = index;
        index = value_end(bytes, index);
        fields.push(Field {
            id,
            key,
            start,
            end: index,
        });
        whitespace(bytes, &mut index);
        if bytes[index] == b',' {
            index += 1;
        }
    }
    let closing = index;
    let standard = |id: &str| {
        matches!(
            id,
            "basics"
                | "work"
                | "education"
                | "volunteer"
                | "awards"
                | "certificates"
                | "publications"
                | "skills"
                | "languages"
                | "interests"
                | "references"
                | "projects"
        )
    };
    let fields: Vec<_> = fields
        .into_iter()
        .filter(|field| standard(&field.id))
        .collect();
    let mut result = vec![Anchor {
        offset: root,
        action: SourceAction::ModuleGap {
            before: fields.first().map(|field| field.id.clone()),
        },
    }];
    for (position, field) in fields.iter().enumerate() {
        result.push(Anchor {
            offset: field.key,
            action: SourceAction::Edit(StructureEdit::RemoveSection {
                id: field.id.clone(),
            }),
        });
        if object[&field.id].is_array() {
            let mut cursor = field.start + 1;
            let mut item = 0;
            loop {
                whitespace(bytes, &mut cursor);
                if bytes[cursor] == b']' {
                    break;
                }
                result.push(Anchor {
                    offset: cursor,
                    action: SourceAction::Edit(StructureEdit::InsertItem {
                        id: field.id.clone(),
                        index: item,
                    }),
                });
                result.push(Anchor {
                    offset: cursor,
                    action: SourceAction::Edit(StructureEdit::RemoveItem {
                        id: field.id.clone(),
                        index: item,
                    }),
                });
                cursor = value_end(bytes, cursor);
                whitespace(bytes, &mut cursor);
                if bytes[cursor] == b',' {
                    cursor += 1;
                }
                item += 1;
                if result.len() > MAX_SOURCE_ANCHORS {
                    return None;
                }
            }
            result.push(Anchor {
                offset: cursor,
                action: SourceAction::Edit(StructureEdit::InsertItem {
                    id: field.id.clone(),
                    index: item,
                }),
            });
        }
        if let Some(next) = fields.get(position + 1) {
            result.push(Anchor {
                offset: field.end - 1,
                action: SourceAction::ModuleGap {
                    before: Some(next.id.clone()),
                },
            });
        }
    }
    if !fields.is_empty() {
        result.push(Anchor {
            offset: closing,
            action: SourceAction::ModuleGap { before: None },
        });
    }
    result.sort_by_key(|anchor| anchor.offset);
    if result.len() > MAX_SOURCE_ANCHORS {
        return None;
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_control_count_independently_from_source_byte_limit() {
        let source = format!(
            "{{\"work\":[{}]}}",
            vec!["{}"; MAX_SOURCE_ANCHORS / 2].join(",")
        );
        assert!(source.len() < MAX_RESUME_BYTES);
        assert!(anchors(&source).is_none());
    }

    #[test]
    fn maps_unicode_escaped_keys_nested_lists_and_crlf_without_touching_source() {
        let source = " {\r\n  \"custom\":{\"work\":[{},{}]},\r\n  \"basics\":{\"name\":\"林🙂 } \\\"\"},\r\n  \"wo\\u0072k\": [{\"highlights\":[\"[]\",\"{}\"]}, {}],\r\n  \"education\":[]\r\n} ";
        let result = anchors(source).unwrap();
        assert_eq!(
            result
                .iter()
                .filter(|anchor| matches!(
                    anchor.action,
                    SourceAction::Edit(StructureEdit::RemoveSection { .. })
                ))
                .count(),
            3
        );
        let items: Vec<_> = result
            .iter()
            .filter_map(|anchor| match &anchor.action {
                SourceAction::Edit(StructureEdit::RemoveItem { id, index }) => {
                    Some((id.as_str(), *index, source.as_bytes()[anchor.offset]))
                }
                _ => None,
            })
            .collect();
        assert_eq!(items, [("work", 0, b'{'), ("work", 1, b'{')]);
        assert!(
            result
                .iter()
                .all(|anchor| source.is_char_boundary(anchor.offset))
        );
        assert!(result.iter().any(|anchor| matches!(&anchor.action, SourceAction::Edit(StructureEdit::InsertItem{id,index:0}) if id=="education") && source.as_bytes()[anchor.offset]==b']'));
        let gaps: Vec<_> = result
            .iter()
            .filter_map(|anchor| {
                if let SourceAction::ModuleGap { before } = &anchor.action {
                    Some(before.as_deref())
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(
            gaps,
            [Some("basics"), Some("work"), Some("education"), None]
        );
    }

    #[test]
    fn handles_empty_root_minified_items_and_non_object_extensions() {
        assert_eq!(anchors("{}").unwrap().len(), 1);
        let source = r#"{"x":-2.4e+12,"y":true,"z":null,"w":"\\\"{}","work":[{},{}]}"#;
        let result = anchors(source).unwrap();
        assert_eq!(
            result
                .iter()
                .filter(|anchor| matches!(
                    anchor.action,
                    SourceAction::Edit(StructureEdit::InsertItem { .. })
                ))
                .count(),
            3
        );
    }

    #[test]
    fn rejects_invalid_or_ambiguous_sources_without_panicking() {
        for source in [
            "",
            "[{}]",
            "null",
            "{",
            "{\"work\":[}",
            r#"{"work":[],"wo\u0072k":[]}"#,
        ] {
            assert!(anchors(source).is_none(), "{source}");
        }
        assert!(anchors(&" ".repeat(MAX_RESUME_BYTES + 1)).is_none());
    }
}
