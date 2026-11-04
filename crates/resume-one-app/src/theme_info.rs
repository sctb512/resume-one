//! Quiet attribution entry shared by local and already-verified online themes.
use gpui::{Context, Window, div, prelude::*, px};
use gpui_component::{
    WindowExt as _,
    button::{ButtonCustomVariant, ButtonVariant},
    dialog::DialogButtonProps,
};
use resume_one_core::ThemeManifest;

use crate::{ResumeOne, color};

struct Attribution {
    author: String,
    license: String,
    upstream_license: String,
    url: Option<String>,
    revision: String,
    restriction: Option<&'static str>,
}

impl Attribution {
    fn from_theme(theme: &ThemeManifest) -> Self {
        let p = &theme.normalized["provenance"];
        let text = |value: &serde_json::Value, fallback: &str| {
            value
                .as_str()
                .filter(|s| !s.is_empty())
                .unwrap_or(fallback)
                .to_owned()
        };
        let license = text(&theme.normalized["license"], "未声明");
        let upstream_license = text(&p["upstreamLicense"], &license);
        let restriction = if [license.as_str(), upstream_license.as_str()]
            .iter()
            .any(|s| s.contains("CC-BY-NC-SA-4.0"))
        {
            Some(
                "此主题采用 CC BY-NC-SA 4.0：署名、非商业性使用、相同方式共享。改编主题仍保留该协议；软件的 Apache 许可不替代它。商业使用或分发前须另行确认授权。",
            )
        } else {
            None
        };
        let url = p["upstreamUrl"]
            .as_str()
            .filter(|url| url.starts_with("https://") && !url.chars().any(char::is_control))
            .map(str::to_owned);
        Self {
            author: text(&p["upstreamAuthor"], "未声明"),
            license,
            upstream_license,
            url,
            revision: text(&p["upstreamRevision"], "未声明"),
            restriction,
        }
    }
}

impl ResumeOne {
    pub(super) fn show_theme_info(
        &mut self,
        theme: ThemeManifest,
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
        self.theme_hover_candidate = None;
        self.theme_hover_generation += 1;
        self.online.cancel_interaction();
        let info = Attribution::from_theme(&theme);
        let style = self.model.selected_style();
        crate::editor_ui::apply_editor_theme(&style, cx);
        let quiet = ButtonCustomVariant::new(cx)
            .color(color(style.colors.paper).into())
            .foreground(color(style.colors.accent).into())
            .border(color(style.colors.border).into())
            .hover(color(style.colors.canvas).into())
            .active(color(style.colors.chrome).into());
        let close_owner = cx.entity().downgrade();
        let version = theme.normalized["version"]
            .as_str()
            .unwrap_or("1")
            .to_owned();
        // The popup menu is about to be destroyed. Do not let the dialog
        // remember its ephemeral focus handle as the return destination.
        self.focus_current(window, cx);
        window.open_dialog(cx, move |dialog, _, _| {
            let mut details = div()
                .id("theme-attribution")
                .debug_selector(|| "theme-attribution".into())
                .flex()
                .flex_col()
                .gap(px(14.))
                .text_size(px(13.))
                .text_color(color(style.colors.ink))
                .child(
                    div()
                        .text_color(color(style.colors.muted))
                        .child(format!("{} · {}", theme.id, version)),
                );
            let mut fields = vec![
                ("原作者", &info.author),
                ("主题协议", &info.license),
                ("原始版本", &info.revision),
            ];
            if info.license != info.upstream_license {
                fields.insert(2, ("上游协议", &info.upstream_license));
            }
            for (label, value) in fields {
                details = details.child(
                    div()
                        .flex()
                        .gap(px(16.))
                        .child(
                            div()
                                .w(px(72.))
                                .flex_none()
                                .text_color(color(style.colors.muted))
                                .child(label),
                        )
                        .child(div().flex_1().min_w_0().child(value.clone())),
                );
            }
            if let Some(url) = &info.url {
                let url = url.clone();
                let link = url.clone();
                details = details.child(
                    div()
                        .id("theme-upstream-link")
                        .debug_selector(|| "theme-upstream-link".into())
                        .cursor_pointer()
                        .text_color(color(style.colors.accent))
                        .child("查看原始项目 ↗")
                        .on_click(move |_, _, cx| cx.open_url(&link)),
                );
            }
            if let Some(restriction) = info.restriction {
                details = details.child(
                    div()
                        .id("theme-license-restriction")
                        .debug_selector(|| "theme-license-restriction".into())
                        .p_3()
                        .border_1()
                        .border_color(color(style.colors.border))
                        .bg(color(style.colors.canvas))
                        .child(restriction),
                );
            }
            details =
                details.child(div().text_color(color(style.colors.muted)).child(
                    "以主题文件与来源项目中注明的协议为准；社区内置包附完整许可与改编说明。",
                ));
            dialog
                .title(format!("{} · 主题说明", theme.name))
                .width(px(620.))
                .border_color(color(style.colors.border))
                .close_button(false)
                .button_props(
                    DialogButtonProps::default()
                        .ok_text("完成")
                        .ok_variant(ButtonVariant::Custom(quiet)),
                )
                .footer(|ok, _, window, cx| vec![ok(window, cx)])
                .child(details)
                .on_close({
                    let owner = close_owner.clone();
                    move |_, window, cx| {
                        if let Some(owner) = owner.upgrade() {
                            owner.update(cx, |owner, cx| {
                                owner.theme_manager_open = false;
                                owner.focus_current(window, cx);
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

    #[test]
    fn restricted_attribution_is_not_replaced_by_application_license() {
        let mut theme = resume_one_core::parse_theme(include_bytes!(
            "../../../assets/themes/zhibai.theme.json"
        ))
        .unwrap();
        theme.normalized["license"] = serde_json::json!("CC-BY-NC-SA-4.0");
        theme.normalized["provenance"] = serde_json::json!({"upstreamAuthor":"Rick Osborne","upstreamLicense":"CC-BY-NC-SA-4.0","upstreamUrl":"https://github.com/rickosborne/jsonresume-theme-rickosborne"});
        let info = Attribution::from_theme(&theme);
        assert_eq!(info.author, "Rick Osborne");
        assert!(info.url.unwrap().ends_with("jsonresume-theme-rickosborne"));
        assert!(info.restriction.unwrap().contains("非商业"));
        assert!(info.restriction.unwrap().contains("相同方式共享"));
        assert_eq!(info.license, "CC-BY-NC-SA-4.0");
    }

    #[gpui::test]
    fn right_click_keeps_applied_theme_and_menu_escape_restores_shortcuts(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(gpui_component::init);
        let child = std::rc::Rc::new(std::cell::RefCell::new(None));
        let output = child.clone();
        let (_, cx) = cx.add_window_view(|window, cx| {
            let owner =
                cx.new(|cx| ResumeOne::new(crate::model::AppModel::load(None).unwrap(), cx));
            *output.borrow_mut() = Some(owner.clone());
            gpui_component::Root::new(owner, window, cx)
        });
        let owner = child.borrow_mut().take().unwrap();
        cx.simulate_resize(gpui::size(px(1280.), px(820.)));
        cx.update(|window, _| window.refresh());
        let point = cx.debug_bounds("theme-1").unwrap().center();
        for down in [true, false] {
            if down {
                cx.simulate_event(gpui::MouseDownEvent {
                    button: gpui::MouseButton::Right,
                    position: point,
                    ..Default::default()
                });
            } else {
                cx.simulate_event(gpui::MouseUpEvent {
                    button: gpui::MouseButton::Right,
                    position: point,
                    ..Default::default()
                });
            }
        }
        cx.run_until_parked();
        cx.update(|_, cx| assert_eq!(owner.read(cx).model.selected_theme, 0));
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(owner.read(cx).focus.is_focused(window));
            assert_eq!(owner.read(cx).model.selected_theme, 0);
        });
        cx.simulate_event(gpui::MouseDownEvent {
            button: gpui::MouseButton::Right,
            position: point,
            ..Default::default()
        });
        cx.simulate_event(gpui::MouseUpEvent {
            button: gpui::MouseButton::Right,
            position: point,
            ..Default::default()
        });
        cx.simulate_keystrokes("down down enter");
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(window.has_active_dialog(cx));
            assert!(owner.read(cx).theme_manager_open);
            assert_eq!(owner.read(cx).model.selected_theme, 0);
        });
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(!window.has_active_dialog(cx));
            assert!(!owner.read(cx).theme_manager_open);
            assert!(owner.read(cx).focus.is_focused(window));
        });
    }

    #[gpui::test]
    fn attribution_dialog_shows_restrictions_without_changing_the_resume(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(gpui_component::init);
        let child = std::rc::Rc::new(std::cell::RefCell::new(None));
        let output = child.clone();
        let (_, cx) = cx.add_window_view(|window, cx| {
            let owner =
                cx.new(|cx| ResumeOne::new(crate::model::AppModel::load(None).unwrap(), cx));
            *output.borrow_mut() = Some(owner.clone());
            gpui_component::Root::new(owner, window, cx)
        });
        let owner = child.borrow_mut().take().unwrap();
        cx.simulate_resize(gpui::size(px(800.), px(600.)));
        let original = cx.update(|_, cx| owner.read(cx).model.document.source.clone());
        cx.update(|window, cx| owner.update(cx, |app, cx| {
            // The invoking menu disappears; do not restore its stale focus.
            let ephemeral_menu_focus = cx.focus_handle();
            window.focus(&ephemeral_menu_focus);
            let mut theme = app.model.themes[0].manifest.clone();
            theme.normalized["license"] = serde_json::json!("CC-BY-NC-SA-4.0");
            theme.normalized["provenance"] = serde_json::json!({"upstreamAuthor":"Rick Osborne","upstreamLicense":"CC-BY-NC-SA-4.0","upstreamUrl":"https://github.com/rickosborne/jsonresume-theme-rickosborne"});
            app.show_theme_info(theme, window, cx);
        }));
        cx.run_until_parked();
        let notice = cx.debug_bounds("theme-license-restriction").unwrap();
        assert!(notice.top() >= px(0.) && notice.bottom() <= px(600.));
        assert!(cx.debug_bounds("theme-upstream-link").is_some());
        cx.update(|window, cx| {
            assert!(window.has_active_dialog(cx));
            assert!(owner.read(cx).theme_manager_open);
        });
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(!window.has_active_dialog(cx));
            assert!(!owner.read(cx).theme_manager_open);
            assert_eq!(owner.read(cx).model.document.source, original);
            assert!(owner.read(cx).focus.is_focused(window));
        });
    }
}
