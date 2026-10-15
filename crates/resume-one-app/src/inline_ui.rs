use std::time::Duration;

use gpui::{
    App, Context, Entity, Focusable as _, IntoElement, KeyBinding, Subscription, Window, actions,
    anchored, deferred, div, prelude::*, px,
};
use gpui_component::input::{Input, InputEvent, InputState};
use resume_one_core::content_edit::{TextEdit, prepare_text_edit, text_fields};

use crate::{ResumeOne, color, label};

actions!(resume_one, [OpenInline, DismissInline]);

#[derive(Default)]
pub(super) struct InlineUi {
    pub form: Option<InlineForm>,
    generation: u64,
    pub paper_hovered: bool,
    form_hovered: bool,
}

pub(super) struct InlineForm {
    pub section: String,
    pub index: Option<usize>,
    fields: Vec<Field>,
    history_before: Option<String>,
    history_slot: usize,
    error: Option<String>,
}

struct Field {
    key: &'static str,
    input: Entity<InputState>,
    _subscription: Subscription,
}

pub(super) fn register(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("enter", OpenInline, Some("PreviewItem")),
        KeyBinding::new("escape", DismissInline, Some("ResumeOne")),
    ]);
}

fn field_label(key: &str) -> &str {
    match key {
        "name" => "名称",
        "label" => "职业标题",
        "image" => "头像地址",
        "email" => "邮箱",
        "phone" => "电话",
        "url" => "链接",
        "summary" => "概述",
        "location" => "地点",
        "description" => "描述",
        "position" => "职位",
        "startDate" => "开始日期",
        "endDate" => "结束日期",
        "organization" => "机构",
        "institution" => "学校",
        "area" => "专业",
        "studyType" => "学位",
        "score" => "成绩",
        "title" => "标题",
        "date" => "日期",
        "awarder" => "颁发机构",
        "issuer" => "发证机构",
        "publisher" => "出版方",
        "releaseDate" => "发表日期",
        "level" => "熟练程度",
        "language" => "语言",
        "fluency" => "掌握程度",
        "reference" => "推荐内容",
        "entity" => "所属机构",
        "type" => "类型",
        _ => key,
    }
}

impl ResumeOne {
    pub(super) fn inline_matches(&self, section: &str, index: Option<usize>) -> bool {
        self.inline
            .form
            .as_ref()
            .is_some_and(|form| form.section == section && form.index == index)
    }

    fn inline_focused(&self, window: &Window, cx: &App) -> bool {
        self.inline.form.as_ref().is_some_and(|form| {
            form.fields
                .iter()
                .any(|field| field.input.read(cx).focus_handle(cx).is_focused(window))
        })
    }

    pub(super) fn schedule_inline(
        &mut self,
        section: &str,
        index: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.inline_matches(section, index) || self.inline_focused(window, cx) {
            return;
        }
        self.inline.generation += 1;
        let generation = self.inline.generation;
        let section = section.to_owned();
        let timer = cx.background_executor().timer(Duration::from_millis(180));
        cx.spawn_in(window, async move |this, cx| {
            timer.await;
            this.update_in(cx, |this, window, cx| {
                let still_hovered = match index {
                    Some(index) => this.hovered_item.as_ref() == Some(&(section.clone(), index)),
                    None => this.hovered_section.as_deref() == Some(section.as_str()),
                };
                if generation == this.inline.generation
                    && still_hovered
                    && !this.inline_focused(window, cx)
                {
                    this.open_inline(&section, index, window, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn open_inline(
        &mut self,
        section: &str,
        index: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.source_mode
            || self.inline_matches(section, index)
            || !self.ensure_editor(window, cx)
            || self.recovery_pending
        {
            return;
        }
        let editor = self.editor.as_ref().unwrap();
        let initial_error = editor
            .buffer
            .issues()
            .first()
            .map(|issue| format!("{} · {}", issue.path, issue.message));
        let Ok(value) = serde_json::from_str::<serde_json::Value>(editor.buffer.source()) else {
            return;
        };
        let Some(item) = value
            .get(section)
            .and_then(|value| index.map_or(Some(value), |index| value.get(index)))
        else {
            return;
        };
        if !item.is_object() {
            return;
        }
        let fields = text_fields(section)
            .iter()
            .map(|&key| {
                let value = item
                    .get(key)
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
                    .to_owned();
                let input = cx.new(|cx| {
                    let state = InputState::new(window, cx).default_value(value);
                    if matches!(key, "summary" | "description" | "reference") {
                        state.auto_grow(2, 5).soft_wrap(true)
                    } else {
                        state
                    }
                });
                let subscription =
                    cx.subscribe_in(&input, window, move |this, input, event, window, cx| {
                        match event {
                            InputEvent::Change => this.inline_field_changed(key, input, window, cx),
                            InputEvent::Blur => {
                                let owner = cx.entity().downgrade();
                                window.defer(cx, move |window, cx| {
                                    if let Some(owner) = owner.upgrade() {
                                        owner.update(cx, |this, cx| {
                                            this.close_inline_outside(window, cx)
                                        });
                                    }
                                });
                            }
                            _ => {}
                        }
                    });
                Field {
                    key,
                    input,
                    _subscription: subscription,
                }
            })
            .collect();
        self.inline.form = Some(InlineForm {
            section: section.into(),
            index,
            fields,
            history_before: None,
            history_slot: self.visual_undo.len(),
            error: initial_error,
        });
        cx.notify();
    }

    fn inline_field_changed(
        &mut self,
        field: &str,
        input: &Entity<InputState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(form) = &self.inline.form else {
            return;
        };
        if !form
            .fields
            .iter()
            .any(|candidate| candidate.key == field && candidate.input == *input)
        {
            return;
        }
        let before = self.editor.as_ref().unwrap().buffer.source().to_owned();
        let edit = TextEdit {
            section: form.section.clone(),
            index: form.index,
            field: field.into(),
            value: input.read(cx).value().to_string(),
        };
        let after = match prepare_text_edit(before.as_bytes(), &edit) {
            Ok(source) => String::from_utf8(source).expect("JSON is UTF-8"),
            Err(error) => {
                self.inline.form.as_mut().unwrap().error = Some(error.message);
                cx.notify();
                return;
            }
        };
        if before == after {
            return;
        }
        let form = self.inline.form.as_mut().unwrap();
        if form.history_before.is_none() {
            if self
                .visual_undo
                .last()
                .is_some_and(|(_, latest)| latest != &before)
            {
                self.visual_undo.clear();
            }
            form.history_before = Some(before.clone());
            form.history_slot = self.visual_undo.len();
            self.visual_undo.push((before, after.clone()));
        } else if let Some(entry) = self.visual_undo.get_mut(form.history_slot) {
            entry.1.clone_from(&after);
        }
        self.visual_redo.clear();
        self.replace_editor_source(&after, window, cx);
        self.sync_inline_error();
        cx.notify();
    }

    pub(super) fn sync_inline_error(&mut self) {
        let error = self
            .editor
            .as_ref()
            .and_then(|editor| editor.buffer.issues().first())
            .map(|issue| format!("{} · {}", issue.path, issue.message));
        if let Some(form) = &mut self.inline.form {
            form.error = error;
        }
    }

    pub(super) fn close_inline_outside(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.inline.paper_hovered
            && !self.inline.form_hovered
            && !self.inline_focused(window, cx)
        {
            self.clear_inline();
            cx.notify();
        }
    }

    pub(super) fn defer_inline_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let owner = cx.entity().downgrade();
        window.defer(cx, move |window, cx| {
            if let Some(owner) = owner.upgrade() {
                owner.update(cx, |this, cx| this.close_inline_outside(window, cx));
            }
        });
    }

    pub(super) fn clear_inline(&mut self) {
        self.inline.form = None;
        self.inline.form_hovered = false;
        self.inline.generation += 1;
    }

    pub(super) fn dismiss_inline(
        &mut self,
        _: &DismissInline,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.inline.form.take().is_some() {
            self.clear_inline();
            window.focus(&self.focus);
            cx.notify();
        }
    }

    pub(super) fn inline_form(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let form = self.inline.form.as_ref().expect("active inline form");
        let style = self.active_style();
        let width =
            (style.page.width_mm - style.page.margins.left - style.page.margins.right) * 3.0;
        let mut fields = div()
            .id("inline-fields")
            .flex()
            .flex_col()
            .gap_3()
            .max_h(px(340.0))
            .overflow_y_scroll()
            .p_4();
        for field in &form.fields {
            let selector = format!("inline-field-{}", field.key);
            fields = fields.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(color(style.colors.muted))
                            .child(field_label(field.key).to_owned()),
                    )
                    .child(
                        div()
                            .id(gpui::SharedString::from(selector.clone()))
                            .debug_selector(move || selector.clone())
                            .child(Input::new(&field.input).w_full()),
                    ),
            );
        }
        deferred(
            anchored().snap_to_window_with_margin(px(64.0)).child(
                div()
                    .id("inline-form")
                    .debug_selector(|| "inline-form".into())
                    .occlude()
                    .w(px(width))
                    .flex()
                    .flex_col()
                    .rounded_md()
                    .shadow_lg()
                    .border_1()
                    .border_color(color(style.colors.accent))
                    .bg(color(style.colors.paper))
                    .on_hover(cx.listener(|this, hovering: &bool, window, cx| {
                        this.inline.form_hovered = *hovering;
                        if !hovering {
                            this.defer_inline_close(window, cx);
                        }
                    }))
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .items_center()
                            .px_4()
                            .py_3()
                            .border_b_1()
                            .border_color(color(style.colors.border))
                            .text_size(px(12.0))
                            .text_color(color(style.colors.accent))
                            .child(label(&form.section).to_owned())
                            .child(
                                div()
                                    .id("close-inline")
                                    .cursor_pointer()
                                    .px_2()
                                    .child("×")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.dismiss_inline(&DismissInline, window, cx)
                                    })),
                            ),
                    )
                    .child(fields)
                    .when_some(form.error.clone(), |view, message| {
                        view.child(
                            div()
                                .px_4()
                                .pb_3()
                                .text_size(px(11.0))
                                .text_color(color(style.colors.seal))
                                .child(message),
                        )
                    }),
            ),
        )
        .priority(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{editor_ui, model::AppModel, structure_ui};
    use gpui::{EntityInputHandler as _, Modifiers, TestAppContext, point};
    use std::{cell::RefCell, rc::Rc};

    #[gpui::test]
    fn inline_edits_keep_invalid_input_sync_source_and_undo_as_a_session(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        cx.update(register);
        let executor = cx.executor();
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("inline.json");
        let disk = br#"{"basics":{"name":"A","email":"a@example.com"},"work":[{"name":"B"}],"custom":{"keep":true}}"#;
        std::fs::write(&path, disk).unwrap();
        let model = AppModel::load(Some(&path)).unwrap();
        let child = Rc::new(RefCell::new(None));
        let child_for_window = child.clone();
        let (_, cx) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| ResumeOne::new(model, cx));
            *child_for_window.borrow_mut() = Some(view.clone());
            gpui_component::Root::new(view, window, cx)
        });
        let view = child.borrow_mut().take().unwrap();
        cx.update(|window, _| window.refresh());
        let paper_before = cx.debug_bounds("resume-paper").unwrap();
        let right_before = cx.debug_bounds("module-panel").unwrap();
        let header = cx.debug_bounds("preview-basics").unwrap();
        cx.simulate_mouse_move(
            point(header.left() + px(10.0), header.top() + px(10.0)),
            None,
            Modifiers::none(),
        );
        executor.advance_clock(Duration::from_millis(200));
        cx.run_until_parked();
        let form = cx.debug_bounds("inline-form").unwrap();
        cx.simulate_mouse_move(
            point(form.left() + px(15.0), form.top() + px(30.0)),
            None,
            Modifiers::none(),
        );
        cx.run_until_parked();
        cx.update(|_, cx| assert!(view.read(cx).inline.form.is_some()));
        cx.simulate_mouse_move(point(px(20.0), px(100.0)), None, Modifiers::none());
        cx.run_until_parked();
        cx.update(|_, cx| assert!(view.read(cx).inline.form.is_none()));
        cx.update(|window, cx| window.focus(&view.read(cx).focus));
        cx.simulate_keystrokes("tab enter");
        cx.update(|_, cx| assert!(view.read(cx).inline.form.is_some()));
        cx.simulate_keystrokes("escape");
        cx.update(|_, cx| assert!(view.read(cx).inline.form.is_none()));
        cx.simulate_click(
            point(header.left() + px(10.0), header.top() + px(10.0)),
            Modifiers::none(),
        );
        cx.run_until_parked();
        assert!(cx.debug_bounds("inline-form").is_some());
        assert_eq!(paper_before, cx.debug_bounds("resume-paper").unwrap());
        assert_eq!(right_before, cx.debug_bounds("module-panel").unwrap());
        let input = cx.update(|_, cx| {
            view.read(cx)
                .inline
                .form
                .as_ref()
                .unwrap()
                .fields
                .iter()
                .find(|field| field.key == "email")
                .unwrap()
                .input
                .clone()
        });
        cx.update(|window, cx| {
            input.update(cx, |state, cx| {
                state.focus(window, cx);
                let length = state.value().chars().map(char::len_utf16).sum();
                state.replace_text_in_range(Some(0..length), "unfinished", window, cx);
            })
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            view.update(cx, |app, cx| {
                assert!(
                    app.editor
                        .as_ref()
                        .unwrap()
                        .buffer
                        .source()
                        .contains("unfinished")
                );
                assert!(!app.editor.as_ref().unwrap().buffer.issues().is_empty());
                assert!(app.inline.form.as_ref().unwrap().error.is_some());
                assert_eq!(app.model.document.value["basics"]["email"], "a@example.com");
                app.inline.paper_hovered = false;
                app.close_inline_outside(window, cx);
                assert!(app.inline.form.is_some());
            });
        });
        cx.update(|window, cx| {
            input.update(cx, |state, cx| {
                let length = state.value().chars().map(char::len_utf16).sum();
                state.replace_text_in_range(Some(0..length), "new@example.com", window, cx);
            })
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            view.update(cx, |app, cx| {
                assert!(app.editor.as_ref().unwrap().buffer.issues().is_empty());
                assert_eq!(
                    app.model.document.value["basics"]["email"],
                    "new@example.com"
                );
                assert_eq!(app.model.document.value["custom"]["keep"], true);
                assert_eq!(app.visual_undo.len(), 1);
                app.dismiss_inline(&DismissInline, window, cx);
                app.undo_visual_edit(&structure_ui::UndoVisualEdit, window, cx);
            })
        });
        cx.run_until_parked();
        cx.update(|_, cx| assert!(!view.read(cx).editor.as_ref().unwrap().buffer.is_dirty()));
        cx.update(|window, cx| {
            view.update(cx, |app, cx| {
                app.redo_visual_edit(&structure_ui::RedoVisualEdit, window, cx);
                app.toggle_source(&editor_ui::ToggleSource, window, cx);
            })
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            let app = view.read(cx);
            assert!(
                app.editor
                    .as_ref()
                    .unwrap()
                    .input
                    .read(cx)
                    .value()
                    .contains("new@example.com")
            );
        });
        assert_eq!(std::fs::read(path).unwrap(), disk);
    }
}
