use gpui::{
    AnyElement, App, AvailableSpace, Bounds, Context, Entity, IntoElement, Pixels, Window, canvas,
    div, prelude::*, px, size,
};
use gpui_component::{
    RopeExt as _, Sizable as _,
    button::{Button, ButtonCustomVariant, ButtonVariants as _},
    input::InputState,
    menu::{DropdownMenu, PopupMenuItem},
};
use resume_one_core::{
    source_map::{Anchor, SourceAction},
    structure::StructureEdit,
};

use crate::{ResumeOne, color, label};

fn action_label(action: &SourceAction) -> String {
    match action {
        SourceAction::ModuleGap { .. } => "新增模块…".into(),
        SourceAction::Edit(StructureEdit::RemoveSection { id }) => format!("移除{}模块", label(id)),
        SourceAction::Edit(StructureEdit::InsertItem { id, index }) => {
            format!("{} · 在第 {} 项前插入", label(id), index + 1)
        }
        SourceAction::Edit(StructureEdit::RemoveItem { id, index }) => {
            format!("{} · 移除第 {} 项", label(id), index + 1)
        }
        SourceAction::Edit(StructureEdit::InsertSection { id, .. }) => {
            format!("新增{}模块", label(id))
        }
    }
}

fn selector(anchor: &Anchor) -> String {
    match &anchor.action {
        SourceAction::ModuleGap { before } => {
            format!("json-gap-{}", before.as_deref().unwrap_or("end"))
        }
        SourceAction::Edit(StructureEdit::RemoveSection { id }) => {
            format!("json-remove-module-{id}")
        }
        SourceAction::Edit(StructureEdit::InsertItem { id, index }) => {
            format!("json-add-item-{id}-{index}")
        }
        SourceAction::Edit(StructureEdit::RemoveItem { id, index }) => {
            format!("json-remove-item-{id}-{index}")
        }
        SourceAction::Edit(StructureEdit::InsertSection { id, .. }) => {
            format!("json-add-module-{id}")
        }
    }
}

impl ResumeOne {
    fn gutter_rows(&self, cx: &App) -> Vec<(Pixels, Pixels, Vec<Anchor>)> {
        let mut rows: Vec<(Pixels, Pixels, Vec<Anchor>)> = Vec::new();
        let Some(editor) = &self.editor else {
            return rows;
        };
        if !self.source_mode
            || !editor.buffer.issues().is_empty()
            || self.recovery_pending
            || self.theme_manager_open
        {
            return rows;
        }
        let state = editor.input.read(cx);
        let Some(visible) = state.decoration_range() else {
            return rows;
        };
        for anchor in editor
            .source_anchors
            .iter()
            .filter(|anchor| visible.contains(&anchor.offset))
        {
            if self.insertion_active() && !matches!(anchor.action, SourceAction::ModuleGap { .. }) {
                continue;
            }
            let Some(position) = state.decoration_bounds(anchor.offset) else {
                continue;
            };
            if let Some(row) = rows
                .iter_mut()
                .find(|row| (row.0 - position.top()).abs() < px(0.5))
            {
                row.2.push(anchor.clone());
            } else {
                rows.push((position.top(), position.size.height, vec![anchor.clone()]));
            }
        }
        rows
    }
    fn gutter_action(
        &mut self,
        action: SourceAction,
        input: &Entity<InputState>,
        generation: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let valid = self.source_mode
            && !self.theme_manager_open
            && !self.recovery_pending
            && self.editor.as_ref().is_some_and(|editor| {
                editor.input == *input
                    && editor.source_generation == generation
                    && editor.buffer.issues().is_empty()
                    && input.read(cx).decoration_range().is_some()
            });
        if !valid {
            return;
        }
        let Some(offset) = self.editor.as_ref().and_then(|editor| {
            editor
                .source_anchors
                .iter()
                .find(|anchor| anchor.action == action)
                .map(|anchor| anchor.offset)
        }) else {
            return;
        };
        input.update(cx, |state, cx| {
            let position = state.text().offset_to_position(offset);
            state.set_cursor_position(position, window, cx);
        });
        match action {
            SourceAction::Edit(edit) => self.apply_visual_structure(edit, window, cx),
            SourceAction::ModuleGap { before } => {
                if let Some(id) = self.selected_module.clone() {
                    self.apply_visual_structure(
                        StructureEdit::InsertSection { id, before },
                        window,
                        cx,
                    );
                } else {
                    let target = Some(before);
                    self.insertion_before = if self.insertion_before == target {
                        None
                    } else {
                        target
                    };
                    self.selected_item_module = None;
                    window.focus(&self.focus);
                    cx.notify();
                }
            }
        }
    }

    fn gutter_controls(&self, bounds: Bounds<Pixels>, cx: &mut Context<Self>) -> AnyElement {
        let mut gutter = div()
            .id("json-structure-gutter")
            .debug_selector(|| "json-structure-gutter".into())
            .size_full();
        let Some(editor) = &self.editor else {
            return gutter.into_any_element();
        };
        if !self.source_mode
            || !editor.buffer.issues().is_empty()
            || self.recovery_pending
            || self.theme_manager_open
        {
            return gutter.into_any_element();
        }
        let input = editor.input.clone();
        let generation = editor.source_generation;
        let scroll_input = input.clone();
        gutter = gutter.child(
            div()
                .absolute()
                .left_0()
                .top_0()
                .w(px(54.))
                .h_full()
                .on_scroll_wheel(move |event, window, cx| {
                    scroll_input
                        .update(cx, |state, cx| state.scroll_decorations(event, window, cx));
                    cx.stop_propagation();
                }),
        );
        // Group exact visual rows. Minified JSON can put several logical targets
        // on one row; a labelled menu prevents stacked or ambiguous buttons.
        let rows = self.gutter_rows(cx);
        let style = self.active_style();
        for (y, height, anchors) in rows {
            for adds in [true, false] {
                let choices: Vec<_> = anchors
                    .iter()
                    .filter(|anchor| anchor.adds() == adds)
                    .cloned()
                    .collect();
                if choices.is_empty() {
                    continue;
                }
                let symbol = if adds { "+" } else { "−" };
                let foreground = color(if adds {
                    style.colors.accent
                } else {
                    style.colors.seal
                });
                let key = if choices.len() == 1 {
                    selector(&choices[0])
                } else {
                    format!(
                        "json-{}-choices-{}",
                        if adds { "add" } else { "remove" },
                        choices[0].offset
                    )
                };
                let mut control = div()
                    .id(gpui::SharedString::from(key.clone()))
                    .debug_selector(move || key.clone())
                    .absolute()
                    .left(px(if adds { 8. } else { 30. }))
                    .top(y - bounds.top())
                    .w(px(20.))
                    .h(height.min(px(24.)))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_sm()
                    .text_size(px(14.))
                    .text_color(foreground)
                    .cursor_pointer()
                    .on_scroll_wheel({
                        let input = input.clone();
                        move |event, window, cx| {
                            input.update(cx, |state, cx| {
                                state.scroll_decorations(event, window, cx)
                            });
                            cx.stop_propagation();
                        }
                    });
                if choices.len() == 1 {
                    let action = choices[0].action.clone();
                    let input = input.clone();
                    control = control
                        .child(symbol)
                        .hover(|control| control.bg(color(style.colors.canvas)))
                        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.gutter_action(action.clone(), &input, generation, window, cx);
                        }));
                    gutter = gutter.child(control);
                } else {
                    let owner = cx.entity().downgrade();
                    let input = input.clone();
                    let variant = ButtonCustomVariant::new(cx)
                        .color(color(style.colors.paper).into())
                        .foreground(foreground.into())
                        .border(color(style.colors.paper).into())
                        .hover(color(style.colors.canvas).into());
                    gutter = gutter.child(
                        control.child(
                            Button::new("gutter-choices")
                                .small()
                                .custom(variant)
                                .label(symbol)
                                .w(px(20.))
                                .h(height.min(px(24.)))
                                .dropdown_menu(move |mut menu, _, _| {
                                    for anchor in &choices {
                                        let owner = owner.clone();
                                        let input = input.clone();
                                        let action = anchor.action.clone();
                                        menu = menu.item(
                                            PopupMenuItem::new(action_label(&action)).on_click(
                                                move |_, window, cx| {
                                                    if let Some(owner) = owner.upgrade() {
                                                        owner.update(cx, |this, cx| {
                                                            this.gutter_action(
                                                                action.clone(),
                                                                &input,
                                                                generation,
                                                                window,
                                                                cx,
                                                            )
                                                        });
                                                    }
                                                },
                                            ),
                                        );
                                    }
                                    menu
                                }),
                        ),
                    );
                }
            }
        }
        gutter.into_any_element()
    }

    pub(super) fn source_gutter(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let owner = cx.entity().downgrade();
        canvas(
            move |bounds, window, cx| {
                let mut element = owner
                    .upgrade()
                    .map(|owner| owner.update(cx, |this, cx| this.gutter_controls(bounds, cx)))
                    .unwrap_or_else(|| div().into_any_element());
                element.layout_as_root(
                    size(
                        AvailableSpace::Definite(bounds.size.width),
                        AvailableSpace::Definite(bounds.size.height),
                    ),
                    window,
                    cx,
                );
                element.prepaint_at(bounds.origin, window, cx);
                element
            },
            |_, mut element, window, cx| element.paint(window, cx),
        )
        .absolute()
        .size_full()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{editor_ui, inline_ui, model::AppModel};
    use gpui::{EntityInputHandler as _, Modifiers, TestAppContext, VisualTestContext, point};
    use gpui_component::RopeExt;
    use std::{cell::RefCell, rc::Rc};

    fn setup<'a>(
        cx: &'a mut TestAppContext,
        source: &str,
    ) -> (
        tempfile::TempDir,
        Entity<ResumeOne>,
        &'a mut VisualTestContext,
    ) {
        cx.update(gpui_component::init);
        cx.update(editor_ui::register);
        cx.update(inline_ui::register);
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("resume.json");
        std::fs::write(&path, source).unwrap();
        let model = AppModel::load(Some(&path)).unwrap();
        let holder = Rc::new(RefCell::new(None));
        let output = holder.clone();
        let (_, cx) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| ResumeOne::new(model, cx));
            *output.borrow_mut() = Some(view.clone());
            gpui_component::Root::new(view, window, cx)
        });
        let view = holder.borrow_mut().take().unwrap();
        cx.simulate_resize(size(px(1120.), px(780.)));
        cx.update(|window, cx| {
            view.update(cx, |app, cx| {
                app.toggle_source(&editor_ui::ToggleSource, window, cx);
                app.editor.as_mut().unwrap().store = resume_one_core::draft::DraftStore::new(
                    root.path().join("drafts"),
                    app.model.document_path.clone(),
                );
                app.sidebar_progress = 1.;
                app.mode_transition = None;
            });
            window.refresh();
        });
        cx.run_until_parked();
        (root, view, cx)
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        let bounds = cx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("missing {selector}"));
        cx.simulate_click(bounds.center(), Modifiers::none());
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
    }

    const BASE: &str = r#"{"basics":{"name":"林🙂"},"work":[{"name":"First"},{"name":"Second"}],"custom":{"keep":123456789012345678901234567890}}"#;

    #[gpui::test]
    fn empty_document_and_empty_array_remain_editable_with_stepwise_undo(cx: &mut TestAppContext) {
        let (_root, view, cx) = setup(cx, "{}");
        click(cx, "json-gap-end");
        click(cx, "module-work");
        cx.update(|_, cx| {
            assert_eq!(
                view.read(cx).model.document.value["work"],
                serde_json::json!([])
            )
        });
        click(cx, "json-add-item-work-0");
        cx.update(|_, cx| {
            assert_eq!(
                view.read(cx).model.document.value["work"],
                serde_json::json!([{}])
            )
        });
        click(cx, "json-remove-module-work");
        cx.update(|_, cx| assert_eq!(view.read(cx).model.document.value, serde_json::json!({})));
        for expected in [
            serde_json::json!({"work":[{}]}),
            serde_json::json!({"work":[]}),
            serde_json::json!({}),
        ] {
            cx.simulate_keystrokes("cmd-z");
            cx.run_until_parked();
            cx.update(|_, cx| assert_eq!(view.read(cx).model.document.value, expected));
        }
        cx.update(|_, cx| assert!(!view.read(cx).editor.as_ref().unwrap().buffer.is_dirty()));
    }

    #[gpui::test]
    fn minified_json_combines_actions_in_named_menus(cx: &mut TestAppContext) {
        let (_root, view, cx) = setup(cx, BASE);
        const COMPACT: &str = r#"{"basics":{"name":"林🙂"},"work":[{},{}]}"#;
        let input = cx.update(|_, cx| view.read(cx).editor.as_ref().unwrap().input.clone());
        cx.update(|window, cx| {
            input.update(cx, |state, cx| {
                let len = state.value().encode_utf16().count();
                state.replace_text_transaction(0..len, COMPACT, window, cx);
                state.set_cursor_position(gpui_component::input::Position::new(0, 0), window, cx);
            })
        });
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.update(|_, cx| {
            let rows = view.read(cx).gutter_rows(cx);
            assert_eq!(rows.len(), 1);
            assert!(rows[0].2.iter().filter(|anchor| anchor.adds()).count() > 1);
        });
        click(cx, "json-add-choices-0");
        cx.simulate_keystrokes("down");
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        cx.update(|_, cx| assert_eq!(view.read(cx).insertion_before, Some(Some("basics".into()))));
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        click(cx, "json-remove-choices-1");
        cx.simulate_keystrokes("down");
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        cx.update(|_, cx| {
            let app = view.read(cx);
            assert!(app.model.document.value.get("basics").is_none());
            assert_eq!(
                app.model.document.value["work"].as_array().unwrap().len(),
                2
            );
        });
        cx.simulate_keystrokes("cmd-z");
        cx.run_until_parked();
        cx.update(|_, cx| assert_eq!(view.read(cx).model.document.value["basics"]["name"], "林🙂"));
    }

    #[gpui::test]
    fn gutter_actions_are_separate_undo_transactions_and_preserve_disk_and_extensions(
        cx: &mut TestAppContext,
    ) {
        let (root, view, cx) = setup(cx, BASE);
        let right = cx.debug_bounds("module-panel").unwrap();
        click(cx, "json-remove-item-work-1");
        click(cx, "json-remove-item-work-0");
        cx.update(|_, cx| {
            assert_eq!(
                view.read(cx).model.document.value["work"],
                serde_json::json!([])
            )
        });
        cx.simulate_keystrokes("cmd-z");
        cx.run_until_parked();
        cx.update(|_, cx| {
            assert_eq!(
                view.read(cx).model.document.value["work"],
                serde_json::json!([{"name":"First"}])
            )
        });
        cx.simulate_keystrokes("cmd-z");
        cx.run_until_parked();
        cx.update(|_, cx| {
            let app = view.read(cx);
            assert_eq!(
                app.model.document.value["work"].as_array().unwrap().len(),
                2
            );
            assert!(!app.editor.as_ref().unwrap().buffer.is_dirty());
        });
        cx.simulate_keystrokes("cmd-shift-z");
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        click(cx, "json-add-item-work-0");
        cx.update(|_, cx| {
            let app = view.read(cx);
            assert_eq!(app.model.document.value["work"][0], serde_json::json!({}));
            assert_eq!(
                app.model.document.value["custom"]["keep"].to_string(),
                "123456789012345678901234567890"
            );
        });
        cx.simulate_keystrokes("cmd-shift-z");
        cx.run_until_parked();
        cx.update(|_, cx| {
            assert_eq!(
                view.read(cx).model.document.value["work"]
                    .as_array()
                    .unwrap()
                    .len(),
                2
            )
        });
        assert_eq!(
            std::fs::read_to_string(root.path().join("resume.json")).unwrap(),
            BASE
        );
        assert_eq!(right, cx.debug_bounds("module-panel").unwrap());
    }

    #[gpui::test]
    fn module_gaps_and_right_panel_form_a_bidirectional_cancellable_flow(cx: &mut TestAppContext) {
        let (_root, view, cx) = setup(cx, BASE);
        let original = cx.update(|_, cx| view.read(cx).model.document.source.clone());
        let right = cx.debug_bounds("module-panel").unwrap();
        click(cx, "json-gap-work");
        cx.update(|_, cx| {
            let app = view.read(cx);
            assert_eq!(app.insertion_before, Some(Some("work".into())));
            assert!(app.insertion_active());
            assert_eq!(app.model.document.source, original);
        });
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        cx.update(|_, cx| assert!(!view.read(cx).insertion_active()));
        click(cx, "module-skills");
        cx.update(|_, cx| assert_eq!(view.read(cx).selected_module.as_deref(), Some("skills")));
        click(cx, "json-gap-work");
        cx.update(|_, cx| {
            let app = view.read(cx);
            let keys: Vec<_> = app
                .model
                .document
                .value
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect();
            assert_eq!(keys, ["basics", "skills", "work", "custom"]);
            assert!(!app.insertion_active());
        });
        cx.simulate_keystrokes("cmd-z");
        cx.run_until_parked();
        click(cx, "json-gap-work");
        click(cx, "module-education");
        cx.update(|_, cx| {
            let app = view.read(cx);
            assert!(app.model.document.value["education"].is_array());
            assert!(!app.insertion_active());
        });
        assert_eq!(right, cx.debug_bounds("module-panel").unwrap());
    }

    #[gpui::test]
    fn real_geometry_tracks_wrapping_scrolling_and_resizing_without_text_overlap(
        cx: &mut TestAppContext,
    ) {
        let mut value: serde_json::Value = serde_json::from_str(BASE).unwrap();
        value["work"][0]["summary"] =
            serde_json::json!("中英文 mixed 🙂 soft wrapping ".repeat(180));
        value["work"][0]["highlights"] = serde_json::json!(
            (0..100)
                .map(|index| format!("第 {index} 条成果"))
                .collect::<Vec<_>>()
        );
        let source = serde_json::to_string(&value).unwrap();
        let (_root, view, cx) = setup(cx, &source);
        let input = cx.update(|_, cx| view.read(cx).editor.as_ref().unwrap().input.clone());
        cx.update(|_, cx| {
            let app = view.read(cx);
            assert!(
                !app.gutter_rows(cx)
                    .iter()
                    .flat_map(|row| &row.2)
                    .any(|anchor| selector(anchor) == "json-remove-item-work-1"),
                "rows={:?}; input-length={}",
                app.gutter_rows(cx)
                    .iter()
                    .map(|row| (row.0, row.1, row.2.iter().map(selector).collect::<Vec<_>>()))
                    .collect::<Vec<_>>(),
                input.read(cx).value().len()
            )
        });
        for width in [800., 1120., 1440.] {
            cx.simulate_resize(size(px(width), px(600.)));
            cx.update(|window, _| window.refresh());
            let control = cx.debug_bounds("json-remove-item-work-0").unwrap();
            cx.update(|_, cx| {
                let app = view.read(cx);
                let editor = app.editor.as_ref().unwrap();
                let anchor = editor
                    .source_anchors
                    .iter()
                    .find(|anchor| selector(anchor) == "json-remove-item-work-0")
                    .unwrap();
                let actual = input.read(cx).decoration_bounds(anchor.offset).unwrap();
                assert!((control.top() - actual.top()).abs() < px(1.));
                assert!(control.right() < actual.left());
            });
        }
        // Keyboard navigation scrolls to the true second item beyond wrapped text.
        cx.update(|window, cx| {
            input.update(cx, |state, cx| {
                let source = state.value();
                let byte = source.find("\"Second\"").unwrap();
                let position = state.text().offset_to_position(byte);
                state.set_cursor_position(position, window, cx);
            })
        });
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.update(|_, cx| {
            assert!(
                !view
                    .read(cx)
                    .gutter_rows(cx)
                    .iter()
                    .flat_map(|row| &row.2)
                    .any(|anchor| selector(anchor) == "json-remove-item-work-0")
            )
        });
        let second = cx.debug_bounds("json-remove-item-work-1").unwrap();
        let area = cx.debug_bounds("resume-json-editor").unwrap();
        assert!(second.top() > area.top() && second.bottom() < area.bottom());
        cx.simulate_event(gpui::ScrollWheelEvent {
            position: second.center(),
            delta: gpui::ScrollDelta::Pixels(point(px(0.), px(260.))),
            ..Default::default()
        });
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.update(|_, cx| assert_eq!(view.read(cx).model.document.source, source.as_bytes()));
    }

    #[gpui::test]
    fn stale_actions_invalid_drafts_and_ime_composition_cannot_edit_structure(
        cx: &mut TestAppContext,
    ) {
        let (_root, view, cx) = setup(cx, BASE);
        let (input, generation) = cx.update(|_, cx| {
            let editor = view.read(cx).editor.as_ref().unwrap();
            (editor.input.clone(), editor.source_generation)
        });
        cx.update(|window, cx| {
            input.update(cx, |state, cx| {
                let source = state.value();
                let byte = source.find("林").unwrap();
                let offset = source[..byte].encode_utf16().count();
                state.replace_and_mark_text_in_range(
                    Some(offset..offset),
                    "中",
                    Some(1..1),
                    window,
                    cx,
                );
            })
        });
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.update(|_, cx| assert!(view.read(cx).gutter_rows(cx).is_empty()));
        cx.update(|window, cx| {
            view.update(cx, |app, cx| {
                app.format_json(&editor_ui::FormatJson, window, cx)
            });
            view.update(cx, |app, cx| {
                app.gutter_action(
                    SourceAction::Edit(StructureEdit::RemoveSection {
                        id: "basics".into(),
                    }),
                    &input,
                    generation,
                    window,
                    cx,
                )
            })
        });
        cx.update(|_, cx| assert!(view.read(cx).model.document.value.get("basics").is_some()));
        cx.update(|window, cx| input.update(cx, |state, cx| state.unmark_text(window, cx)));
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.update(|_, cx| {
            assert_eq!(
                view.read(cx).model.document.value["basics"]["name"],
                "中林🙂"
            )
        });
        let after = cx.update(|_, cx| view.read(cx).model.document.source.clone());
        cx.update(|window, cx| {
            view.update(cx, |app, cx| {
                app.gutter_action(
                    SourceAction::Edit(StructureEdit::RemoveSection {
                        id: "basics".into(),
                    }),
                    &input,
                    generation,
                    window,
                    cx,
                )
            })
        });
        cx.update(|_, cx| assert_eq!(view.read(cx).model.document.source, after));
        cx.update(|window, cx| {
            input.update(cx, |state, cx| {
                let len = state.value().encode_utf16().count();
                state.replace_text_transaction(0..len, "{\"work\":42}", window, cx);
            })
        });
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.update(|_, cx| assert!(view.read(cx).gutter_rows(cx).is_empty()));
        cx.update(|_, cx| {
            assert!(
                !view
                    .read(cx)
                    .editor
                    .as_ref()
                    .unwrap()
                    .buffer
                    .issues()
                    .is_empty()
            )
        });
    }
}
