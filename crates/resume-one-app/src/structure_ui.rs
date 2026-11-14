use crate::i18n::tr;
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

pub(super) enum VisualEdit {
    Content {
        before: String,
        after: String,
    },
    Theme {
        before: Box<crate::model::BuiltinTheme>,
        after: Box<crate::model::BuiltinTheme>,
    },
}

impl VisualEdit {
    pub fn content_after(&self) -> Option<&str> {
        match self {
            Self::Content { after, .. } => Some(after),
            Self::Theme { .. } => None,
        }
    }
}

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
    pub(super) fn apply_theme_selection(&mut self, theme: crate::model::BuiltinTheme) {
        self.theme_restore_epoch = self.theme_restore_epoch.wrapping_add(1);
        let before = self.model.applied_theme().clone();
        self.clear_inline();
        self.cancel_insertion();
        self.drawer = None;
        self.hovered_theme = None;
        self.online.cancel_interaction();
        self.theme_hover_generation += 1;
        self.theme_hover_candidate = None;
        if before.identity == theme.identity {
            self.remember_applied_theme();
            return;
        }
        self.visual_undo.push(VisualEdit::Theme {
            before: Box::new(before),
            after: Box::new(theme.clone()),
        });
        self.visual_redo.clear();
        self.restore_theme_selection(&theme);
        self.remember_applied_theme();
    }

    pub(super) fn restore_theme_selection(&mut self, theme: &crate::model::BuiltinTheme) {
        if let Some(index) = self
            .model
            .themes
            .iter()
            .position(|t| t.identity == theme.identity)
        {
            self.model.select_theme(index);
        } else {
            // A verified snapshot also permits undo after a directory/source
            // change. No deleted file is recreated and no network is needed.
            self.model.applied_online = Some(theme.clone());
        }
        self.hovered_theme = None;
        self.theme_hover_generation += 1;
        self.theme_hover_candidate = None;
        self.online.cancel_interaction();
    }

    fn apply_history_entry(
        &mut self,
        entry: &VisualEdit,
        forward: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        match entry {
            VisualEdit::Content { before, after } => {
                let (expected, target) = if forward {
                    (before, after)
                } else {
                    (after, before)
                };
                if self.editor.as_ref().map(|e| e.buffer.source()) != Some(expected.as_str()) {
                    return false;
                }
                self.replace_editor_source(target, window, cx);
            }
            VisualEdit::Theme { before, after } => {
                let (expected, target) = if forward {
                    (before, after)
                } else {
                    (after, before)
                };
                if self.model.applied_theme().identity != expected.identity {
                    return false;
                }
                self.restore_theme_selection(target);
                self.theme_restore_epoch = self.theme_restore_epoch.wrapping_add(1);
                self.remember_applied_theme();
            }
        }
        true
    }

    pub(super) fn structure_enabled(&self) -> bool {
        !self.recovery_pending
            && self.model.preview_ready
            && self
                .editor
                .as_ref()
                .is_none_or(|e| e.buffer.issues().is_empty())
    }
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
            self.editor_warning(
                tr("无法修改结构"),
                tr("请先修复 JSON 或 Schema 错误"),
                window,
                cx,
            );
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
                self.editor_warning(tr("无法修改结构"), &error.message, window, cx);
                return;
            }
        };
        let retained_item_module = self.selected_item_module.as_ref().and_then(|id| {
            (self.source_mode && result.value.get(id).is_some()).then_some(id.clone())
        });
        let after = String::from_utf8(result.source).expect("validated JSON is UTF-8");
        if self
            .visual_undo
            .iter()
            .rev()
            .find_map(VisualEdit::content_after)
            .is_some_and(|latest| latest != before)
        {
            self.visual_undo.clear();
        }
        self.visual_undo.push(VisualEdit::Content {
            before,
            after: after.clone(),
        });
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
                this.open_pinned_inline(&section, index, window, cx);
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
        self.close_drawer();
        self.clear_inline();
        cx.notify();
        let Some(entry) = self.visual_undo.pop() else {
            return;
        };
        if !self.apply_history_entry(&entry, false, window, cx) {
            self.visual_undo.clear();
            self.visual_redo.clear();
            return;
        }
        self.visual_redo.push(entry);
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
        self.close_drawer();
        self.clear_inline();
        cx.notify();
        let Some(entry) = self.visual_redo.pop() else {
            return;
        };
        if !self.apply_history_entry(&entry, true, window, cx) {
            self.visual_undo.clear();
            self.visual_redo.clear();
            return;
        }
        self.visual_undo.push(entry);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::AppContext as _;

    #[gpui::test]
    fn theme_clicks_undo_without_an_editor_and_hover_never_becomes_history(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(register);
        let model = crate::model::AppModel::load(None).unwrap();
        let source = model.document.source.clone();
        let (view, cx) = cx.add_window_view(|_, cx| ResumeOne::new(model, cx));
        cx.update(|window, _| window.refresh());
        let row = cx.debug_bounds("theme-1").unwrap();
        cx.simulate_click(row.center(), gpui::Modifiers::none());
        cx.simulate_click(row.center(), gpui::Modifiers::none());
        let hover = cx.debug_bounds("theme-2").unwrap();
        cx.simulate_mouse_move(hover.center(), None, gpui::Modifiers::none());
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(160));
        cx.run_until_parked();
        cx.update(|_, cx| {
            let app = view.read(cx);
            assert_eq!(app.model.selected_theme, 1);
            assert_eq!(app.hovered_theme, Some(2));
            assert_eq!(app.visual_undo.len(), 1);
            assert!(app.editor.is_none());
        });
        cx.simulate_keystrokes("cmd-z");
        cx.run_until_parked();
        cx.update(|_, cx| {
            let app = view.read(cx);
            assert_eq!(app.model.selected_theme, 0);
            assert!(app.hovered_theme.is_none());
            assert_eq!(app.model.document.source, source);
            assert!(app.editor.is_none());
        });
        cx.simulate_keystrokes("cmd-shift-z");
        cx.update(|_, cx| assert_eq!(view.read(cx).model.selected_theme, 1));
        // Restoring an old verified appearance does not require its directory.
        cx.update(|window, cx| {
            view.update(cx, |app, cx| {
                app.model.replace_themes(vec![app.model.themes[1].clone()]);
                app.undo_visual_edit(&UndoVisualEdit, window, cx);
                assert_eq!(app.model.applied_theme().manifest.id, "zhibai");
                assert_eq!(app.model.document.source, source);
            })
        });
    }

    #[gpui::test]
    fn theme_and_document_history_preserve_operation_order(cx: &mut gpui::TestAppContext) {
        cx.update(gpui_component::init);
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("history.json");
        let original = br#"{"basics":{"name":"A"},"custom":{"keep":true}}"#;
        std::fs::write(&path, original).unwrap();
        let child = std::rc::Rc::new(std::cell::RefCell::new(None));
        let output = child.clone();
        let (_, cx) = cx.add_window_view(|window, cx| {
            let view =
                cx.new(|cx| ResumeOne::new(crate::model::AppModel::load(Some(&path)).unwrap(), cx));
            *output.borrow_mut() = Some(view.clone());
            gpui_component::Root::new(view, window, cx)
        });
        let view = child.borrow_mut().take().unwrap();
        cx.update(|window, cx| {
            view.update(cx, |app, cx| {
                app.apply_visual_structure(
                    StructureEdit::InsertSection {
                        id: "work".into(),
                        before: None,
                    },
                    window,
                    cx,
                )
            })
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            view.update(cx, |app, cx| {
                app.apply_theme_selection(app.model.themes[1].clone());
                app.undo_visual_edit(&UndoVisualEdit, window, cx);
                assert_eq!(app.model.selected_theme, 0);
                assert!(app.model.document.value.get("work").is_some());
            })
        });
        cx.update(|window, cx| {
            view.update(cx, |app, cx| {
                app.undo_visual_edit(&UndoVisualEdit, window, cx)
            })
        });
        cx.run_until_parked();
        cx.update(|_, cx| assert!(view.read(cx).model.document.value.get("work").is_none()));
        cx.update(|window, cx| {
            view.update(cx, |app, cx| {
                app.redo_visual_edit(&RedoVisualEdit, window, cx)
            })
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            view.update(cx, |app, cx| {
                app.redo_visual_edit(&RedoVisualEdit, window, cx);
                assert_eq!(app.model.selected_theme, 1);
                assert!(app.model.document.value.get("work").is_some());
                assert_eq!(app.model.document.value["custom"]["keep"], true);
                assert!(app.model.document.value.get("theme").is_none());
            })
        });
        assert_eq!(std::fs::read(path).unwrap(), original);
    }

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
