//! Screen-space layout only; drawers never change the print layout.
use gpui::{
    Animation, AnimationExt, Context, IntoElement, MouseButton, Window, div, prelude::*, px,
};

use crate::{ResumeOne, chrome, color};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Layout {
    Wide,
    Medium,
    Compact,
}

impl Layout {
    pub fn for_width(width: f32) -> Self {
        if width >= 1280. {
            Self::Wide
        } else if width >= 1024. {
            Self::Medium
        } else {
            Self::Compact
        }
    }

    pub fn theme_docked(self) -> bool {
        self == Self::Wide
    }

    pub fn modules_docked(self) -> bool {
        self != Self::Compact
    }

    pub fn reserved_width(self) -> f32 {
        (if self.theme_docked() {
            chrome::THEME_WIDTH
        } else {
            0.
        }) + if self.modules_docked() {
            chrome::MODULE_WIDTH
        } else {
            0.
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Drawer {
    Themes,
    Modules,
}

impl ResumeOne {
    pub(super) fn close_drawer(&mut self) {
        if self.drawer.take().is_some() {
            self.hovered_theme = None;
            self.online.cancel_interaction();
            self.theme_hover_generation += 1;
            self.theme_hover_candidate = None;
            self.cancel_insertion();
        }
    }

    pub(super) fn toggle_drawer(
        &mut self,
        drawer: Drawer,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if drawer == Drawer::Modules {
            self.finish_appearance(window, cx);
        }
        self.drawer = if self.drawer == Some(drawer) {
            None
        } else {
            Some(drawer)
        };
        self.clear_inline();
        self.cancel_insertion();
        self.files.open = false;
        self.export_menu_open = false;
        self.hovered_theme = None;
        self.online.cancel_interaction();
        self.theme_hover_generation += 1;
        self.theme_hover_candidate = None;
        window.focus(&self.focus);
        cx.notify();
    }

    pub(super) fn drawer_button(&self, drawer: Drawer, cx: &mut Context<Self>) -> impl IntoElement {
        let p = chrome::Palette::new(self.shell_style());
        let (id, title) = match drawer {
            Drawer::Themes => ("show-themes", "主题"),
            Drawer::Modules => ("show-modules", "模块"),
        };
        div()
            .id(id)
            .tab_index(0)
            .focus(|v| v.border_color(color(p.accent)).bg(color(p.soft)))
            .on_key_down(cx.listener(move |this, event, window, cx| {
                if chrome::activation_key(event) {
                    this.toggle_drawer(drawer, window, cx);
                    cx.stop_propagation();
                }
            }))
            .debug_selector(move || id.into())
            .h(px(36.))
            .px(px(10.))
            .flex_none()
            .flex()
            .items_center()
            .rounded(px(6.))
            .border_1()
            .border_color(color(p.line))
            .text_color(color(p.ink))
            .cursor_pointer()
            .when(self.drawer == Some(drawer), |v| v.bg(color(p.soft)))
            .hover(|v| v.bg(color(p.soft)))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(
                cx.listener(move |this, _, window, cx| this.toggle_drawer(drawer, window, cx)),
            )
            .child(title)
    }

    pub(super) fn drawer_overlay(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let drawer = self.drawer.expect("open drawer");
        let colors = self.shell_style().colors;
        let reduce_motion = self.reduce_motion;
        div()
            .id("drawer-overlay")
            .debug_selector(|| "drawer-overlay".into())
            .absolute()
            .top(px(chrome::TOOLBAR_HEIGHT))
            .bottom_0()
            .left_0()
            // The medium layout's right rail is never masked or moved.
            .right(px(
                if self.layout.modules_docked() || self.appearance_editor.is_some() {
                    chrome::MODULE_WIDTH
                } else {
                    0.
                },
            ))
            .occlude()
            .child(
                div()
                    .id("drawer-dismiss")
                    .debug_selector(|| "drawer-dismiss".into())
                    .absolute()
                    .size_full()
                    .bg(color(colors.ink))
                    .opacity(0.18)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.drawer = None;
                        this.hovered_theme = None;
                        this.online.cancel_interaction();
                        this.theme_hover_generation += 1;
                        this.theme_hover_candidate = None;
                        this.cancel_insertion();
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .when(drawer == Drawer::Themes, |v| v.left_0())
                    .when(drawer == Drawer::Modules, |v| v.right_0())
                    .shadow_md()
                    .occlude()
                    .when(drawer == Drawer::Themes, |v| {
                        v.child(self.theme_sidebar(chrome::THEME_WIDTH, cx))
                    })
                    .when(drawer == Drawer::Modules, |v| {
                        v.child(self.module_sidebar(cx))
                    })
                    .with_animation(
                        if drawer == Drawer::Themes {
                            "theme-drawer-arrival"
                        } else {
                            "module-drawer-arrival"
                        },
                        Animation::new(std::time::Duration::from_millis(160)),
                        move |v, t| {
                            let t = if reduce_motion {
                                1.
                            } else {
                                t * t * (3. - 2. * t)
                            };
                            let offset = px(-16. * (1. - t));
                            v.opacity(t)
                                .when(drawer == Drawer::Themes, |v| v.left(offset))
                                .when(drawer == Drawer::Modules, |v| v.right(offset))
                        },
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn breakpoints_reserve_only_docked_rails() {
        assert_eq!(Layout::for_width(1280.), Layout::Wide);
        assert_eq!(Layout::for_width(1279.), Layout::Medium);
        assert_eq!(Layout::for_width(1024.), Layout::Medium);
        assert_eq!(Layout::for_width(1023.), Layout::Compact);
        assert_eq!(Layout::Wide.reserved_width(), 450.);
        assert_eq!(Layout::Medium.reserved_width(), 230.);
        assert_eq!(Layout::Compact.reserved_width(), 0.);
    }

    #[gpui::test]
    fn compact_drawers_are_exclusive_and_both_insert_paths_cancel_without_edits(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(gpui_component::init);
        cx.update(crate::inline_ui::register);
        let model = crate::model::AppModel::load(None).unwrap();
        let source = model.document.source.clone();
        let (view, cx) = cx.add_window_view(|_, cx| ResumeOne::new(model, cx));
        cx.simulate_resize(gpui::size(px(900.), px(560.)));
        cx.update(|window, _| window.refresh());
        let paper = cx.debug_bounds("resume-paper").unwrap();
        let button = cx.debug_bounds("show-themes").unwrap();
        cx.simulate_click(button.center(), gpui::Modifiers::none());
        cx.update(|_, cx| assert_eq!(view.read(cx).drawer, Some(Drawer::Themes)));
        let button = cx.debug_bounds("show-modules").unwrap();
        cx.simulate_click(button.center(), gpui::Modifiers::none());
        cx.update(|window, _| window.refresh());
        cx.update(|_, cx| assert_eq!(view.read(cx).drawer, Some(Drawer::Modules)));
        assert_eq!(paper, cx.debug_bounds("resume-paper").unwrap());
        // Drive state through the actual row, not a separate insertion model.
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(180));
        cx.run_until_parked();
        let row = cx.debug_bounds("module-volunteer").unwrap();
        cx.simulate_click(row.center(), gpui::Modifiers::none());
        cx.update(|_, cx| {
            let app = view.read(cx);
            assert_eq!(app.drawer, None);
            assert_eq!(app.selected_module.as_deref(), Some("volunteer"));
        });
        cx.simulate_keystrokes("escape");
        cx.update(|window, _| window.refresh());
        let add = cx.debug_bounds("add-module-before-work").unwrap();
        cx.simulate_mouse_move(add.center(), None, gpui::Modifiers::none());
        cx.simulate_click(add.center(), gpui::Modifiers::none());
        cx.update(|window, _| window.refresh());
        cx.update(|_, cx| {
            let app = view.read(cx);
            assert_eq!(app.drawer, Some(Drawer::Modules));
            assert!(app.insertion_before.is_some());
        });
        cx.simulate_keystrokes("escape");
        cx.update(|_, cx| {
            let app = view.read(cx);
            assert_eq!(app.drawer, None);
            assert!(!app.insertion_active());
            assert_eq!(app.model.document.source, source);
            assert!(app.visual_undo.is_empty());
        });
    }

    #[gpui::test]
    fn reduced_motion_skips_translation_and_finishes_at_requested_view(
        cx: &mut gpui::TestAppContext,
    ) {
        let (view, cx) = cx.add_window_view(|_, cx| {
            ResumeOne::new(crate::model::AppModel::load(None).unwrap(), cx)
        });
        cx.update(|_, cx| {
            view.update(cx, |app, _| {
                app.reduce_motion = true;
                for source in [true, false, true, false] {
                    app.source_mode = source;
                    app.begin_mode_transition();
                    assert_eq!(app.sidebar_progress, if source { 1. } else { 0. });
                    assert_eq!(app.view_blend, app.sidebar_progress);
                    assert!(app.mode_transition.is_none());
                }
            })
        });
    }

    #[gpui::test]
    fn invalid_json_preserves_last_preview_but_disables_structure(cx: &mut gpui::TestAppContext) {
        cx.update(gpui_component::init);
        let child = std::rc::Rc::new(std::cell::RefCell::new(None));
        let output = child.clone();
        let (_, cx) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| ResumeOne::new(crate::model::AppModel::load(None).unwrap(), cx));
            *output.borrow_mut() = Some(view.clone());
            gpui_component::Root::new(view, window, cx)
        });
        let view = child.borrow_mut().take().unwrap();
        cx.update(|window, cx| {
            view.update(cx, |app, cx| {
                assert!(app.ensure_editor(window, cx));
                app.editor
                    .as_mut()
                    .unwrap()
                    .buffer
                    .set_source("{\"work\":42}".into());
                cx.notify();
            })
        });
        cx.update(|window, _| window.refresh());
        assert!(cx.debug_bounds("stale-preview").is_some());
        assert!(cx.debug_bounds("resume-paper").is_some());
        let row = cx.debug_bounds("module-volunteer").unwrap();
        cx.simulate_click(row.center(), gpui::Modifiers::none());
        cx.update(|_, cx| {
            let app = view.read(cx);
            assert!(!app.structure_enabled());
            assert!(!app.insertion_active());
            assert!(app.inline.form.is_none());
            assert_eq!(
                app.editor.as_ref().unwrap().buffer.source(),
                "{\"work\":42}"
            );
        });
    }
}
