use gpui::{App, Context, KeyBinding, Window, actions};
use resume_one_core::structure::{StructureEdit, apply_structure};

use crate::ResumeOne;

/// A single UTF-16 replacement, trimmed at complete Unicode scalar boundaries.
/// Keeping unaffected text out of history also keeps undo near the actual edit.
fn changed_range<'a>(before: &str, after: &'a str) -> Option<(std::ops::Range<usize>, &'a str)> {
    if before == after {
        return None;
    }
    let prefix = before
        .chars()
        .zip(after.chars())
        .take_while(|(a, b)| a == b)
        .map(|(a, _)| a.len_utf8())
        .sum::<usize>();
    let suffix = before[prefix..]
        .chars()
        .rev()
        .zip(after[prefix..].chars().rev())
        .take_while(|(a, b)| a == b)
        .map(|(a, _)| a.len_utf8())
        .sum::<usize>();
    let start = before[..prefix].encode_utf16().count();
    let end = start + before[prefix..before.len() - suffix].encode_utf16().count();
    Some((start..end, &after[prefix..after.len() - suffix]))
}

actions!(resume_one, [UndoVisualEdit, RedoVisualEdit]);

pub(super) fn register(cx: &mut App) {
    let modifier = if cfg!(target_os = "macos") {
        "cmd"
    } else {
        "ctrl"
    };
    cx.bind_keys([
        KeyBinding::new(&format!("{modifier}-z"), UndoVisualEdit, Some("ResumeOne")),
        KeyBinding::new(
            &format!("{modifier}-shift-z"),
            RedoVisualEdit,
            Some("ResumeOne"),
        ),
    ]);
}

impl ResumeOne {
    pub(super) fn insertion_active(&self) -> bool {
        self.insertion_before.is_some()
            || self.selected_module.is_some()
            || (!self.source_mode && self.selected_item_module.is_some())
    }

    pub(super) fn cancel_insertion(&mut self) -> bool {
        let active = self.insertion_before.take().is_some();
        let module = self.selected_module.take().is_some();
        let item = self.selected_item_module.take().is_some();
        active || module || item
    }

    pub(super) fn apply_visual_structure(
        &mut self,
        edit: StructureEdit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.model.preview_ready || !self.ensure_editor(window, cx) {
            return;
        }
        if self.recovery_pending {
            return;
        }
        let Some(editor) = &self.editor else { return };
        if editor.input.read(cx).is_composing() {
            return;
        }
        if !editor.buffer.issues().is_empty() {
            self.editor_warning("无法修改结构", "请先修复 JSON 或 Schema 错误", window, cx);
            return;
        }
        let before = editor.buffer.source().to_owned();
        let open_after = if self.source_mode {
            None
        } else {
            match &edit {
                StructureEdit::InsertItem { id, index } => Some((id.clone(), Some(*index))),
                StructureEdit::InsertSection { id, .. } if id == "basics" => {
                    Some((id.clone(), None))
                }
                _ => None,
            }
        };
        self.clear_inline();
        let result = match apply_structure(&self.model.document, &edit) {
            Ok(document) => document,
            Err(error) => {
                self.editor_warning("无法修改结构", &error.message, window, cx);
                return;
            }
        };
        let retained_item_module = self.selected_item_module.as_ref().and_then(|id| {
            (self.source_mode && result.value.get(id).is_some()).then_some(id.clone())
        });
        let after = String::from_utf8(result.source).expect("validated JSON is UTF-8");
        if self
            .visual_undo
            .last()
            .is_some_and(|(_, latest)| latest != &before)
        {
            self.visual_undo.clear();
        }
        self.visual_undo.push((before, after.clone()));
        self.visual_redo.clear();
        self.replace_editor_source(&after, window, cx);
        self.insertion_before = None;
        self.selected_module = None;
        self.selected_item_module = retained_item_module;
        if self.source_mode
            && let Some(editor) = &self.editor
        {
            editor.input.update(cx, |input, cx| input.focus(window, cx));
        }
        if let Some((section, index)) = open_after {
            cx.defer_in(window, move |this, window, cx| {
                this.open_inline(&section, index, window, cx);
                this.focus_inline_first(window, cx);
                cx.notify();
            });
        }
        cx.notify();
    }

    pub(super) fn replace_editor_source(
        &mut self,
        source: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = &self.editor else { return };
        let input = editor.input.clone();
        let display_source = editor
            .display_alias
            .as_ref()
            .and_then(|(display, original)| {
                (!self.source_mode && source == original).then_some(display.as_str())
            })
            .unwrap_or(source);
        let preserve_cursor = self.source_mode;
        input.update(cx, |state, cx| {
            let cursor = preserve_cursor.then(|| state.cursor_position());
            if let Some((range, text)) = changed_range(&state.value(), display_source) {
                state.replace_text_transaction(range, text, window, cx);
            }
            if let Some(cursor) = cursor {
                state.set_cursor_position(cursor, window, cx);
            }
        });
        // InputEvent::Change updates the shared buffer, preview and recovery draft.
    }

    pub(super) fn undo_visual_edit(
        &mut self,
        _: &UndoVisualEdit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.source_mode {
            return;
        }
        self.cancel_insertion();
        self.clear_inline();
        cx.notify();
        let Some((before, after)) = self.visual_undo.pop() else {
            return;
        };
        if self.editor.as_ref().map(|editor| editor.buffer.source()) != Some(after.as_str()) {
            self.visual_undo.clear();
            self.visual_redo.clear();
            return;
        }
        self.replace_editor_source(&before, window, cx);
        self.visual_redo.push((before, after));
        cx.notify();
    }

    pub(super) fn redo_visual_edit(
        &mut self,
        _: &RedoVisualEdit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.source_mode {
            return;
        }
        self.cancel_insertion();
        self.clear_inline();
        cx.notify();
        let Some((before, after)) = self.visual_redo.pop() else {
            return;
        };
        if self.editor.as_ref().map(|editor| editor.buffer.source()) != Some(before.as_str()) {
            self.visual_undo.clear();
            self.visual_redo.clear();
            return;
        }
        self.replace_editor_source(&after, window, cx);
        self.visual_undo.push((before, after));
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimal_transactions_respect_unicode_crlf_and_disjoint_changes() {
        assert_eq!(
            changed_range("林🙂甲\r\n尾", "林🙂乙\r\n尾"),
            Some((3..4, "乙"))
        );
        assert_eq!(changed_range("🙂x", "🙂中x"), Some((2..2, "中")));
        assert_eq!(changed_range("🙂中x", "🙂x"), Some((2..3, "")));
        assert_eq!(changed_range("{}", "{}"), None);
        assert_eq!(changed_range("ab🙂cd", "az🙂yd"), Some((1..5, "z🙂y")));
        assert_eq!(changed_range("", "🙂"), Some((0..0, "🙂")));
    }
}
