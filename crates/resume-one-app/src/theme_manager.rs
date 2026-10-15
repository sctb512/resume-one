use gpui::{
    App, Context, Entity, Focusable as _, IntoElement, KeyBinding, Render, ScrollHandle,
    Subscription, WeakEntity, Window, actions, div, prelude::*, px,
};
use gpui_component::{
    Sizable as _, WindowExt as _,
    button::{Button, ButtonCustomVariant, ButtonVariant, ButtonVariants as _},
    dialog::DialogButtonProps,
    input::{Input, InputState},
};
use resume_one_themes::GithubSource;

use crate::{
    ResumeOne, color,
    theme_local::{ChooseThemeDirectory, ResetThemeDirectory},
};

actions!(resume_one, [ManageThemes]);

fn quiet_button(style: &resume_one_core::theme_style::ThemeStyle, cx: &App) -> ButtonCustomVariant {
    ButtonCustomVariant::new(cx)
        .color(color(style.colors.paper).into())
        .foreground(color(style.colors.accent).into())
        .border(color(style.colors.border).into())
        .hover(color(style.colors.canvas).into())
        .active(color(style.colors.chrome).into())
}

pub(super) fn register(cx: &mut App) {
    let modifier = if cfg!(target_os = "macos") {
        "cmd"
    } else {
        "ctrl"
    };
    cx.bind_keys([KeyBinding::new(
        &format!("{modifier}-,"),
        ManageThemes,
        Some("ResumeOne"),
    )]);
}

pub(super) struct ThemeManager {
    owner: WeakEntity<ResumeOne>,
    inputs: [Entity<InputState>; 3],
    editing: Option<String>,
    error: Option<String>,
    scroll: ScrollHandle,
    _observer: Subscription,
}

impl ThemeManager {
    fn new(owner: Entity<ResumeOne>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let inputs = ["", "main", "themes/index.json"]
            .map(|value| cx.new(|cx| InputState::new(window, cx).default_value(value)));
        let observer = cx.observe(&owner, |_, _, cx| cx.notify());
        Self {
            owner: owner.downgrade(),
            inputs,
            editing: None,
            error: None,
            scroll: ScrollHandle::new(),
            _observer: observer,
        }
    }

    fn reset_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for (input, value) in self.inputs.iter().zip(["", "main", "themes/index.json"]) {
            input.update(cx, |state, cx| {
                state.set_value(value.to_owned(), window, cx)
            });
        }
        self.editing = None;
        self.error = None;
        cx.notify();
    }

    fn source_input(
        &self,
        index: usize,
        selector: String,
        style: &resume_one_core::theme_style::ThemeStyle,
        window: &Window,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let input = self.inputs[index].clone();
        let scroll = self.scroll.clone();
        let focused = input.read(cx).focus_handle(cx).is_focused(window);
        div()
            .on_children_prepainted(move |children, window, cx| {
                if !input.read(cx).focus_handle(cx).is_focused(window) {
                    return;
                }
                let Some(bounds) = children.first() else {
                    return;
                };
                let viewport = scroll.bounds();
                let mut next = scroll.offset();
                if bounds.top() < viewport.top() + px(18.0) {
                    next.y += viewport.top() + px(18.0) - bounds.top();
                } else if bounds.bottom() > viewport.bottom() - px(12.0) {
                    next.y -= bounds.bottom() - viewport.bottom() + px(12.0);
                }
                next.y = next.y.max(-scroll.max_offset().height).min(px(0.0));
                if (next.y - scroll.offset().y).abs() > px(0.5) {
                    scroll.set_offset(next);
                    window.defer(cx, |window, _| window.refresh());
                }
            })
            .id(gpui::SharedString::from(selector.clone()))
            .debug_selector(move || selector.clone())
            .child(
                Input::new(&self.inputs[index])
                    .focus_bordered(false)
                    .border_color(color(if focused {
                        style.colors.accent
                    } else {
                        style.colors.border
                    })),
            )
    }

    fn edit(&mut self, source: &GithubSource, window: &mut Window, cx: &mut Context<Self>) {
        self.editing = Some(source.key());
        self.error = None;
        for (input, value) in
            self.inputs
                .iter()
                .zip([source.repository(), source.reference(), source.index_path()])
        {
            input.update(cx, |state, cx| {
                state.set_value(value.to_owned(), window, cx)
            });
        }
        self.inputs[0].update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let values = self
            .inputs
            .each_ref()
            .map(|input| input.read(cx).value().to_string());
        let source = match GithubSource::new(values[0].trim(), values[1].trim(), values[2].trim()) {
            Ok(source) => source,
            Err(error) => {
                self.error = Some(error.message);
                cx.notify();
                return;
            }
        };
        let Some(owner) = self.owner.upgrade() else {
            return;
        };
        let editing = self.editing.clone();
        let result = owner.update(cx, |owner, cx| {
            let store = owner
                .theme_store
                .as_mut()
                .ok_or("主题设置不可用".to_string())?;
            let mut sources = store.sources.clone();
            if let Some(key) = editing {
                let position = sources
                    .iter()
                    .position(|item| item.key() == key)
                    .ok_or("来源已改变，请重新选择".to_string())?;
                sources[position] = source;
            } else {
                sources.push(source);
            }
            store.save_sources(sources)?;
            cx.notify();
            Ok::<(), String>(())
        });
        match result {
            Ok(()) => self.reset_draft(window, cx),
            Err(error) => {
                self.error = Some(error);
                cx.notify();
            }
        }
    }

    fn remove(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(owner) = self.owner.upgrade() else {
            return;
        };
        let result = owner.update(cx, |owner, cx| {
            let store = owner
                .theme_store
                .as_mut()
                .ok_or("主题设置不可用".to_string())?;
            let sources = store
                .sources
                .iter()
                .filter(|source| source.key() != key)
                .cloned()
                .collect();
            store.save_sources(sources)?;
            cx.notify();
            Ok::<(), String>(())
        });
        match result {
            Ok(()) if self.editing.as_deref() == Some(key) => self.reset_draft(window, cx),
            Ok(()) => {
                self.error = None;
                cx.notify();
            }
            Err(error) => {
                self.error = Some(error);
                cx.notify();
            }
        }
    }
}

impl Render for ThemeManager {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(owner) = self.owner.upgrade() else {
            return div().into_any_element();
        };
        let app = owner.read(cx);
        let style = app.active_style().clone();
        let quiet = quiet_button(&style, cx);
        let primary = ButtonCustomVariant::new(cx)
            .color(color(style.colors.accent).into())
            .foreground(color(style.colors.paper).into())
            .border(color(style.colors.accent).into())
            .hover(color(style.colors.accent).into())
            .active(color(style.colors.accent).into());
        let directory = app
            .theme_store
            .as_ref()
            .map(|store| store.directory.display().to_string())
            .unwrap_or_else(|| "主题目录不可用".into());
        let sources = app
            .theme_store
            .as_ref()
            .map(|store| store.sources.clone())
            .unwrap_or_default();
        let warnings = app
            .theme_store
            .as_ref()
            .map(|store| store.warnings.clone())
            .unwrap_or_default();
        let mut list = div()
            .id("managed-source-list")
            .flex()
            .flex_col()
            .gap_2()
            .max_h(px(150.0))
            .overflow_y_scroll();
        if sources.is_empty() {
            list = list.child(
                div()
                    .text_size(px(12.0))
                    .text_color(color(style.colors.muted))
                    .child("尚未添加来源"),
            );
        }
        for (index, source) in sources.into_iter().enumerate() {
            let edit = source.clone();
            let key = source.key();
            let selector = format!("managed-source-{index}");
            list =
                list.child(
                    div()
                        .id(gpui::SharedString::from(selector.clone()))
                        .debug_selector(move || selector.clone())
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_3()
                        .p_3()
                        .rounded_md()
                        .bg(color(style.colors.canvas))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .min_w_0()
                                .flex_1()
                                .child(
                                    div()
                                        .text_size(px(13.0))
                                        .truncate()
                                        .child(source.repository().to_string()),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.0))
                                        .text_color(color(style.colors.muted))
                                        .truncate()
                                        .child(format!(
                                            "{} · {}",
                                            source.reference(),
                                            source.index_path()
                                        )),
                                ),
                        )
                        .child(
                            Button::new(("edit-source", index))
                                .small()
                                .custom(quiet)
                                .label("修改")
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.edit(&edit, window, cx)
                                })),
                        )
                        .child(
                            Button::new(("remove-source", index))
                                .small()
                                .custom(quiet)
                                .label("移除")
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.remove(&key, window, cx)
                                })),
                        ),
                );
        }
        let mut form = div().flex().flex_col().gap_3();
        for (index, label) in [
            "GitHub 仓库 · owner/repository",
            "分支、标签或 commit",
            "清单路径",
        ]
        .into_iter()
        .enumerate()
        {
            let selector = format!("theme-source-input-{index}");
            form = form.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(color(style.colors.muted))
                            .child(label),
                    )
                    .child(self.source_input(index, selector, &style, window, cx)),
            );
        }
        let choose_owner = owner.downgrade();
        let reset_owner = owner.downgrade();
        div()
            .id("theme-manager")
            .debug_selector(|| "theme-manager".into())
            .flex()
            .flex_col()
            .gap_4()
            .max_h(px(
                (f32::from(window.viewport_size().height) * 0.9 - 160.0).clamp(240.0, 520.0)
            ))
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .on_action(
                cx.listener(|this, _: &gpui_component::input::Enter, window, cx| {
                    this.save(window, cx)
                }),
            )
            .text_color(color(style.colors.ink))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(div().text_size(px(13.0)).child("本地主题目录"))
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(color(style.colors.muted))
                            .child(directory),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(
                                Button::new("choose-theme-folder")
                                    .small()
                                    .custom(quiet)
                                    .label("更换目录…")
                                    .on_click(move |_, window, cx| {
                                        if let Some(owner) = choose_owner.upgrade() {
                                            owner.update(cx, |owner, cx| {
                                                owner.choose_theme_directory(
                                                    &ChooseThemeDirectory,
                                                    window,
                                                    cx,
                                                )
                                            });
                                        }
                                    }),
                            )
                            .child(
                                Button::new("reset-theme-folder")
                                    .small()
                                    .custom(quiet)
                                    .label("恢复默认目录")
                                    .on_click(move |_, window, cx| {
                                        if let Some(owner) = reset_owner.upgrade() {
                                            owner.update(cx, |owner, cx| {
                                                owner.reset_theme_directory(
                                                    &ResetThemeDirectory,
                                                    window,
                                                    cx,
                                                )
                                            });
                                        }
                                    }),
                            ),
                    ),
            )
            .child(
                div()
                    .border_t_1()
                    .border_color(color(style.colors.border))
                    .pt_4()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(div().text_size(px(13.0)).child("在线主题源"))
                    .child(list)
                    .child(form)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                Button::new("save-theme-source")
                                    .small()
                                    .custom(primary)
                                    .label(if self.editing.is_some() {
                                        "保存修改"
                                    } else {
                                        "添加来源"
                                    })
                                    .on_click(
                                        cx.listener(|this, _, window, cx| this.save(window, cx)),
                                    ),
                            )
                            .when(self.editing.is_some(), |row| {
                                row.child(
                                    Button::new("cancel-theme-source")
                                        .small()
                                        .custom(quiet)
                                        .label("取消修改")
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.reset_draft(window, cx)
                                        })),
                                )
                            }),
                    ),
            )
            .when_some(self.error.clone(), |view, error| {
                view.child(
                    div()
                        .text_size(px(12.0))
                        .text_color(color(style.colors.seal))
                        .child(error),
                )
            })
            .when(!warnings.is_empty(), |view| {
                view.child(
                    div()
                        .text_size(px(11.0))
                        .text_color(color(style.colors.seal))
                        .child(warnings.join("\n")),
                )
            })
            .into_any_element()
    }
}

impl ResumeOne {
    pub(super) fn manage_themes(
        &mut self,
        _: &ManageThemes,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.theme_manager_open
            || self.files.busy
            || self.export_busy
            || self.editor.as_ref().is_some_and(|editor| editor.saving)
        {
            return;
        }
        self.theme_manager_open = true;
        self.files.open = false;
        self.clear_inline();
        self.cancel_insertion();
        self.hovered_theme = None;
        crate::editor_ui::apply_editor_theme(self.model.selected_style(), cx);
        let owner = cx.entity();
        let manager = cx.new(|cx| ThemeManager::new(owner.clone(), window, cx));
        let close_owner = owner.downgrade();
        let quiet = quiet_button(self.model.selected_style(), cx);
        let border = color(self.model.selected_style().colors.border);
        window.open_dialog(cx, move |dialog, _, _| {
            dialog
                .title("主题管理")
                .width(px(620.0))
                .border_color(border)
                .close_button(false)
                .button_props(
                    DialogButtonProps::default()
                        .ok_text("完成")
                        .ok_variant(ButtonVariant::Custom(quiet)),
                )
                .footer(|ok, _, window, cx| vec![ok(window, cx)])
                .child(manager.clone())
                .on_close({
                    let owner = close_owner.clone();
                    move |_, _, cx| {
                        if let Some(owner) = owner.upgrade() {
                            owner.update(cx, |owner, cx| {
                                owner.theme_manager_open = false;
                                cx.notify();
                            });
                        }
                    }
                })
        });
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{model::AppModel, theme_local::ThemeStore};
    use gpui::TestAppContext;
    use std::{cell::RefCell, rc::Rc};

    #[gpui::test]
    fn manager_opens_from_sidebar_and_escape_restores_unchanged_workspace(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        cx.update(register);
        let root = tempfile::tempdir().unwrap();
        let store = ThemeStore::new(
            root.path().join("settings.json"),
            root.path().join("themes"),
        )
        .unwrap();
        let model = AppModel::load(None).unwrap();
        let source = model.document.source.clone();
        let child = Rc::new(RefCell::new(None));
        let output = child.clone();
        let (_, cx) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| {
                let mut app = ResumeOne::new(model, cx);
                app.theme_store = Some(store);
                app
            });
            *output.borrow_mut() = Some(view.clone());
            gpui_component::Root::new(view, window, cx)
        });
        let owner = child.borrow_mut().take().unwrap();
        cx.simulate_resize(gpui::size(px(800.0), px(600.0)));
        cx.update(|window, _| window.refresh());
        let right = cx.debug_bounds("module-panel").unwrap();
        let entry = cx.debug_bounds("manage-themes").unwrap();
        cx.simulate_click(entry.center(), gpui::Modifiers::none());
        cx.run_until_parked();
        let manager = cx.debug_bounds("theme-manager").unwrap();
        assert!(manager.top() >= px(0.0));
        assert!(manager.bottom() <= px(600.0));
        let repository = cx.debug_bounds("theme-source-input-0").unwrap();
        cx.simulate_click(repository.center(), gpui::Modifiers::none());
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        // Enter validates/submits the form, not the dialog's Done action.
        cx.update(|window, cx| assert!(window.has_active_dialog(cx)));
        cx.simulate_keystrokes("tab tab");
        cx.run_until_parked();
        let field = cx.debug_bounds("theme-source-input-2").unwrap();
        let viewport = cx.debug_bounds("theme-manager").unwrap();
        assert!(field.top() >= viewport.top());
        assert!(field.bottom() <= viewport.bottom());
        cx.update(|window, cx| {
            assert!(window.has_active_dialog(cx));
            owner.update(cx, |app, cx| {
                assert!(app.theme_manager_open);
                app.toggle_source(&crate::editor_ui::ToggleSource, window, cx);
                assert!(!app.source_mode);
                assert_eq!(app.model.document.source, source);
            });
        });
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(!window.has_active_dialog(cx));
            assert!(!owner.read(cx).theme_manager_open);
            assert_eq!(owner.read(cx).model.document.source, source);
        });
        assert_eq!(right, cx.debug_bounds("module-panel").unwrap());
        cx.update(|window, cx| window.focus(&owner.read(cx).focus));
        cx.simulate_keystrokes("cmd-,");
        cx.run_until_parked();
        cx.update(|window, cx| assert!(window.has_active_dialog(cx)));
        cx.simulate_keystrokes("escape");
        cx.update(|window, cx| assert!(!window.has_active_dialog(cx)));
    }

    #[gpui::test]
    fn source_forms_add_edit_remove_and_keep_failed_changes_out_of_settings(
        cx: &mut TestAppContext,
    ) {
        cx.update(gpui_component::init);
        let root = tempfile::tempdir().unwrap();
        let settings = root.path().join("settings.json");
        let defaults = root.path().join("themes");
        let store = ThemeStore::new(settings.clone(), defaults.clone()).unwrap();
        let child = Rc::new(RefCell::new(None));
        let output = child.clone();
        let (_, cx) = cx.add_window_view(|window, cx| {
            let owner = cx.new(|cx| {
                let mut app = ResumeOne::new(AppModel::load(None).unwrap(), cx);
                app.theme_store = Some(store);
                app
            });
            *output.borrow_mut() = Some(owner.clone());
            gpui_component::Root::new(owner, window, cx)
        });
        let owner = child.borrow_mut().take().unwrap();
        let source = cx.update(|_, cx| owner.read(cx).model.document.source.clone());
        let manager =
            cx.update(|window, cx| cx.new(|cx| ThemeManager::new(owner.clone(), window, cx)));
        // The form handlers are exercised against a real GPUI window and input states.
        for repository in ["Example/One", "Example/Two"] {
            cx.update(|window, cx| {
                manager.update(cx, |manager, cx| {
                    manager.inputs[0]
                        .update(cx, |state, cx| state.set_value(repository, window, cx));
                    manager.save(window, cx);
                    assert!(manager.error.is_none());
                })
            });
        }
        let before = std::fs::read(&settings).unwrap();
        cx.update(|window, cx| {
            manager.update(cx, |manager, cx| {
                manager.inputs[0]
                    .update(cx, |state, cx| state.set_value("example/one", window, cx));
                manager.save(window, cx);
                assert!(manager.error.is_some());
            })
        });
        assert_eq!(std::fs::read(&settings).unwrap(), before);
        let sources =
            cx.update(|_, cx| owner.read(cx).theme_store.as_ref().unwrap().sources.clone());
        cx.update(|window, cx| {
            manager.update(cx, |manager, cx| {
                manager.edit(&sources[0], window, cx);
                manager.inputs[1].update(cx, |state, cx| state.set_value("v2", window, cx));
                manager.save(window, cx);
                assert!(manager.error.is_none());
                manager.remove(&sources[1].key(), window, cx);
                assert!(manager.error.is_none());
            })
        });
        let reopened = ThemeStore::new(settings, defaults).unwrap();
        assert_eq!(reopened.sources.len(), 1);
        assert_eq!(reopened.sources[0].reference(), "v2");
        cx.update(|_, cx| assert_eq!(owner.read(cx).model.document.source, source));
    }
}
