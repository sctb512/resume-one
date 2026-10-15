use gpui::{App, Context, EntityInputHandler as _, KeyBinding, Window, actions};
use resume_one_core::structure::{StructureEdit, apply_structure};

use crate::ResumeOne;

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
        if !editor.buffer.issues().is_empty() {
            self.editor_warning("无法修改结构", "请先修复 JSON 或 Schema 错误", window, cx);
            return;
        }
        let before = editor.buffer.source().to_owned();
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
            let length = state.value().chars().map(char::len_utf16).sum();
            state.replace_text_in_range(Some(0..length), display_source, window, cx);
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
        self.clear_inline();
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
        self.clear_inline();
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
