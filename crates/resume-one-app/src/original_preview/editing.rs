use gpui::{Context, Window};
use resume_one_core::{
    content_edit::{
        PROFILE_FIELDS, ProfileChange, StringListChange, StringListEdit, TextEdit, list_fields,
        prepare_list_edit, prepare_profile_edit, prepare_text_edit, text_fields,
    },
    resume::{parse_resume, revision},
};
use serde_json::{Value, json};

use crate::{ResumeOne, inline_ui, structure_ui::VisualEdit};

const MAX_TARGETS: usize = 384;
const MAX_FIELD_BYTES: usize = 48 * 1024;

fn field(section: &str, key: &str, value: &Value, index: Option<usize>) -> Value {
    let value = value.as_str().unwrap_or_default();
    json!({
        "key": key, "label": inline_ui::form_field_label(section, key),
        "value": if value.len() <= MAX_FIELD_BYTES { value } else { "" },
        "readOnly": value.len() > MAX_FIELD_BYTES, "index": index,
        "long": matches!(key, "summary" | "description" | "reference" | "highlights"),
    })
}

pub(super) fn targets(resume: &Value) -> Value {
    let mut targets = Vec::new();
    let mut remaining_bytes: usize = 1024 * 1024;
    for &(section, _) in crate::SECTION_LABELS {
        let title = crate::label(section);
        let item = &resume[section];
        let items: &[Value] = if section == "basics" && item.is_object() {
            std::slice::from_ref(item)
        } else {
            item.as_array().map(Vec::as_slice).unwrap_or_default()
        };
        for (index, item) in items.iter().enumerate().take(MAX_TARGETS - targets.len()) {
            let mut fields = text_fields(section).to_vec();
            fields.sort_by_key(|key| inline_ui::field_order(section, key));
            let mut fields: Vec<_> = fields
                .into_iter()
                .map(|key| {
                    field(
                        section,
                        key,
                        item.pointer(&format!("/{}", key.replace('.', "/")))
                            .unwrap_or(&Value::Null),
                        None,
                    )
                })
                .collect();
            for &key in list_fields(section) {
                for (index, value) in item[key]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .enumerate()
                    .take(24)
                {
                    fields.push(field(section, key, value, Some(index)));
                }
            }
            if section == "basics" {
                for (index, profile) in item["profiles"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .enumerate()
                    .take(16)
                {
                    for key in PROFILE_FIELDS {
                        fields.push(field(
                            section,
                            &format!("profiles.{key}"),
                            &profile[key],
                            Some(index),
                        ));
                    }
                }
            }
            let anchors = [
                "name",
                "position",
                "organization",
                "institution",
                "title",
                "language",
                "area",
                "email",
                "summary",
            ]
            .into_iter()
            .filter_map(|key| item[key].as_str())
            .filter(|value| (1..=200).contains(&value.chars().count()))
            .take(6)
            .collect::<Vec<_>>();
            let target = json!({
                "section": section, "title": title, "anchors": anchors, "fields": fields,
                "index": (section != "basics").then_some(index)
            });
            if let Some(remaining) = remaining_bytes.checked_sub(target.to_string().len()) {
                remaining_bytes = remaining;
                targets.push(target);
            }
        }
    }
    Value::Array(targets)
}

fn draft(before: &str, message: &Value) -> Option<Result<String, String>> {
    let section = message["section"].as_str()?;
    let item = message["item"]
        .as_u64()
        .and_then(|i| usize::try_from(i).ok());
    let field = message["field"].as_str()?;
    let index = message["index"]
        .as_u64()
        .and_then(|i| usize::try_from(i).ok());
    let value = message["value"].as_str()?.to_owned();
    if value.len() > MAX_FIELD_BYTES {
        return Some(Err(
            crate::i18n::tr("字段过长，请在 JSON 视图中编辑。").into()
        ));
    }
    let bytes = before.as_bytes();
    let result = match (section, item, index) {
        ("basics", None, Some(index)) => prepare_profile_edit(
            bytes,
            &ProfileChange::SetText {
                index,
                field: field.strip_prefix("profiles.")?.into(),
                value,
            },
        ),
        (_, Some(item), Some(index)) => prepare_list_edit(
            bytes,
            &StringListEdit {
                section: section.into(),
                index: item,
                field: field.into(),
                change: StringListChange::Set { index, value },
            },
        ),
        (_, item, None) => prepare_text_edit(
            bytes,
            &TextEdit {
                section: section.into(),
                index: item,
                field: field.into(),
                value,
            },
        ),
        _ => return None,
    };
    Some(result.map_err(|error| error.message).and_then(|source| {
        parse_resume(&source).map_err(|issues| {
            issues
                .first()
                .map(|issue| format!("{} · {}", issue.path, issue.message))
                .unwrap_or_else(|| crate::i18n::tr("请检查字段值").into())
        })?;
        Ok(String::from_utf8(source).expect("JSON is UTF-8"))
    }))
}

impl ResumeOne {
    pub(super) fn edit_original_field(
        &mut self,
        message: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Ok(message) = serde_json::from_str::<Value>(message) else {
            return;
        };
        let Some(session) = message["session"].as_u64() else {
            return;
        };
        if message["kind"] != "edit"
            || self.source_mode
            || !self.structure_enabled()
            || self.insertion_active()
            || self.model.applied_theme().identity != self.display_theme().identity
        {
            return;
        }
        let state = &self.original_preview;
        let (Some(key), Some(token)) = (&state.painted, state.painted_token) else {
            return;
        };
        if message["token"].as_u64() != Some(token)
            || key.theme != self.display_theme().identity
            || (key.resume_revision != self.model.document.revision
                && state.edit_revision.as_ref() != Some(&self.model.document.revision))
        {
            return;
        }
        if !self.ensure_editor(window, cx) {
            return;
        }
        let before = self
            .editor
            .as_ref()
            .expect("editor")
            .buffer
            .source()
            .to_owned();
        let Some(result) = draft(&before, &message) else {
            return;
        };
        let result = match result {
            Ok(after) => {
                if before != after {
                    let state = &mut self.original_preview;
                    let merge = state
                        .edit_session
                        .and_then(|(previous, slot)| {
                            (previous == session && slot + 1 == self.visual_undo.len())
                                .then_some(slot)
                        })
                        .filter(|&slot| {
                            self.visual_undo[slot].content_after() == Some(before.as_str())
                        });
                    if let Some(slot) = merge {
                        if let VisualEdit::Content { after: prior, .. } =
                            &mut self.visual_undo[slot]
                        {
                            prior.clone_from(&after);
                        }
                    } else {
                        if self
                            .visual_undo
                            .iter()
                            .rev()
                            .find_map(VisualEdit::content_after)
                            .is_some_and(|last| last != before)
                        {
                            self.visual_undo.clear();
                        }
                        state.edit_session = Some((session, self.visual_undo.len()));
                        self.visual_undo.push(VisualEdit::Content {
                            before,
                            after: after.clone(),
                        });
                    }
                    state.edit_revision = Some(revision(after.as_bytes()));
                    self.visual_redo.clear();
                    self.replace_editor_source(&after, window, cx);
                    cx.notify();
                }
                None
            }
            Err(error) => Some(error),
        };
        if let Some(desktop) = &self.original_preview.desktop {
            let result = serde_json::to_string(&result).expect("result");
            let _ = desktop.evaluate(&format!("jianliEditResult({session}, {result})"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bridge_changes_one_existing_field_and_rejects_invalid_or_unlisted_targets() {
        let original = r#"{"basics":{"name":"林清和"},"work":[{"name":"Old","highlights":["A"]}]}"#;
        let message =
            json!({"section":"work","item":0,"field":"highlights","index":0,"value":"新成果"});
        let after: Value =
            serde_json::from_str(&draft(original, &message).unwrap().unwrap()).unwrap();
        assert_eq!(after["work"][0]["highlights"][0], "新成果");
        assert_eq!(after["basics"]["name"], "林清和");
        for message in [
            json!({"section":"work","item":8,"field":"name","value":"Other"}),
            json!({"section":"basics","field":"admin","value":"Other"}),
            json!({"section":"basics","field":"email","value":"not-an-email"}),
        ] {
            assert!(draft(original, &message).unwrap().is_err());
        }
    }
}
