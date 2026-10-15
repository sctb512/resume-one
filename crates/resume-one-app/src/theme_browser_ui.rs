use std::sync::Arc;

use gpui::{Context, IntoElement, Window, div, prelude::*, px};
use gpui_component::menu::{ContextMenuExt, PopupMenuItem};
use resume_one_themes::{GithubClient, GithubSource, ThemeCache, VerifiedTheme};

use crate::{ResumeOne, color, model::BuiltinTheme, theme_online::Provider};

impl ResumeOne {
    pub(super) fn sync_theme_sources(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let sources = self
            .theme_store
            .as_ref()
            .map(|store| store.sources.clone())
            .unwrap_or_default();
        self.online.sync_sources(&sources);
        self.pump_online(window, cx);
        cx.notify();
    }

    fn browse_online(&mut self, online: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.hovered_theme = None;
        self.clear_inline();
        self.cancel_insertion();
        if online && self.online.provider.is_none() {
            let result = dirs::cache_dir()
                .ok_or_else(|| "无法定位主题缓存目录".to_owned())
                .and_then(|root| {
                    ThemeCache::new(&root.join("Resume One").join("theme-cache"))
                        .map_err(|error| error.to_string())
                });
            match result {
                Ok(cache) => self.online.provider = Some(Arc::new(GithubClient::new(cache))),
                Err(error) => self.online.diagnostic(error),
            }
        }
        self.online.set_online(online);
        self.theme_scroll.set_offset(gpui::point(px(0.), px(0.)));
        self.sync_theme_sources(window, cx);
    }

    pub(super) fn pump_online(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(provider) = self.online.provider.clone() else {
            return;
        };
        for job in self.online.take_jobs() {
            let provider: Arc<dyn Provider> = provider.clone();
            let work = job.clone();
            let task = cx
                .background_executor()
                .spawn(async move { work.run(provider.as_ref()) });
            cx.spawn_in(window, async move |this, cx| {
                let result = task.await;
                this.update_in(cx, |this, window, cx| {
                    let effects = this.online.complete(&job, result);
                    if let Some(error) = effects.download_error {
                        this.editor_warning("无法下载主题", &error, window, cx);
                    }
                    if let Some(theme) = effects.apply {
                        this.apply_online_theme(theme);
                    }
                    if let Some((source, theme)) = effects.download {
                        this.install_online_theme(source, theme, window, cx);
                    }
                    this.pump_online(window, cx);
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
    }

    fn apply_online_theme(&mut self, theme: BuiltinTheme) {
        self.clear_inline();
        self.cancel_insertion();
        self.model.applied_online = Some(theme);
    }

    fn install_online_theme(
        &mut self,
        source: GithubSource,
        theme: Arc<VerifiedTheme>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.online.downloading {
            return;
        }
        let Some(directory) = self
            .theme_store
            .as_ref()
            .map(|store| store.directory.clone())
        else {
            return;
        };
        self.online.downloading = true;
        let task = cx.background_executor().spawn(async move {
            theme
                .install(&source, &directory)
                .map_err(|error| error.to_string())
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            this.update_in(cx, |this, window, cx| {
                this.online.downloading = false;
                match result {
                    Ok(_) => {
                        if let Some(store) = &mut this.theme_store {
                            match store.load_themes() {
                                Ok(themes) => this.model.replace_themes(themes),
                                Err(error) => this.online.diagnostic(error),
                            }
                        }
                    }
                    Err(error) => {
                        this.online.diagnostic(error.clone());
                        this.editor_warning("无法下载主题", &error, window, cx);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn theme_navigation(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let style = self.active_style();
        let mut tabs = div().flex().gap_4().text_size(px(12.)).px_2();
        for (online, title) in [(false, "本地"), (true, "在线")] {
            let selector = if online {
                "themes-online"
            } else {
                "themes-local"
            };
            tabs = tabs.child(
                div()
                    .id(selector)
                    .debug_selector(move || selector.into())
                    .cursor_pointer()
                    .pb_2()
                    .border_b_1()
                    .border_color(color(if online == self.online.online {
                        style.colors.accent
                    } else {
                        style.colors.chrome
                    }))
                    .text_color(color(if online == self.online.online {
                        style.colors.accent
                    } else {
                        style.colors.muted
                    }))
                    .child(title)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.browse_online(online, window, cx)
                    })),
            );
        }
        let mut nav = div().flex().flex_col().gap_2().child(tabs);
        if self.online.online {
            let title = self
                .online
                .sources
                .get(self.online.source_index)
                .map(|source| source.repository().to_owned())
                .unwrap_or_else(|| "管理来源".into());
            nav = nav.child(
                div()
                    .id("theme-source-picker")
                    .debug_selector(|| "theme-source-picker".into())
                    .px_2()
                    .py_1()
                    .text_size(px(10.))
                    .text_color(color(style.colors.muted))
                    .cursor_pointer()
                    .overflow_hidden()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(div().flex_1().min_w_0().truncate().child(title))
                    .child("▾")
                    .on_click(cx.listener(|this, _, window, cx| {
                        if this.online.sources.is_empty() {
                            this.manage_themes(&crate::theme_manager::ManageThemes, window, cx);
                        } else {
                            this.online.sources_open = !this.online.sources_open;
                            cx.notify();
                        }
                    })),
            );
            if self.online.sources_open {
                let mut sources = div()
                    .id("theme-sources-menu")
                    .max_h(px(144.))
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap_1();
                for (index, source) in self.online.sources.iter().enumerate() {
                    sources = sources.child(
                        div()
                            .id(("source-choice", index))
                            .debug_selector(move || format!("source-choice-{index}"))
                            .text_size(px(10.))
                            .px_2()
                            .py_2()
                            .rounded_md()
                            .truncate()
                            .cursor_pointer()
                            .hover(|row| row.bg(color(style.colors.paper)))
                            .child(format!("{} · {}", source.repository(), source.reference()))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.online.select_source(index);
                                this.theme_scroll.set_offset(gpui::point(px(0.), px(0.)));
                                this.pump_online(window, cx);
                                cx.notify();
                            })),
                    );
                }
                nav = nav.child(sources);
            }
        }
        nav
    }

    pub(super) fn online_theme_list(&self, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let style = self.active_style();
        let mut list = div()
            .id("theme-list")
            .flex()
            .flex_col()
            .gap_1()
            .overflow_y_scroll()
            .flex_1()
            .min_h_0()
            .track_scroll(&self.theme_scroll)
            .on_scroll_wheel(cx.listener(|this, _, window, cx| {
                this.online.clear_hover();
                let owner = cx.entity().downgrade();
                window.defer(cx, move |window, cx| {
                    if let Some(owner) = owner.upgrade() {
                        owner.update(cx, |this, cx| {
                            let center = ((-f32::from(this.theme_scroll.offset().y)
                                + f32::from(this.theme_scroll.bounds().size.height) / 2.)
                                / 66.)
                                .floor()
                                .max(0.) as usize;
                            this.online.scroll_to(center);
                            this.pump_online(window, cx);
                            cx.notify();
                        });
                    }
                });
                cx.notify();
            }));
        if let Some(catalog) = &self.online.catalog {
            for (index, entry) in catalog.entries().iter().enumerate() {
                let identity = self
                    .online
                    .identity(&entry.id)
                    .expect("entry in current catalog");
                let selected = self.model.applied_theme().identity == identity;
                let id = entry.id.clone();
                let click_id = id.clone();
                let download_id = id.clone();
                let owner = cx.entity().downgrade();
                let download_disabled = self.online.download_pending() || self.files.busy;
                let row = div()
                    .id(("theme", index))
                    .debug_selector(move || format!("theme-{index}"))
                    .flex()
                    .flex_none()
                    .h(px(62.))
                    .items_center()
                    .gap_3()
                    .px_3()
                    .rounded_md()
                    .cursor_pointer()
                    .when(selected, |row| row.bg(color(style.colors.paper)))
                    .hover(|row| row.bg(color(style.colors.paper)))
                    .child(div().w(px(5.)).h(px(34.)).rounded_sm().bg(color(
                        self.online.color(&entry.id).unwrap_or(style.colors.border),
                    )))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .text_color(color(if selected {
                                        style.colors.accent
                                    } else {
                                        style.colors.ink
                                    }))
                                    .truncate()
                                    .child(entry.name.clone()),
                            )
                            .child(
                                div()
                                    .text_size(px(10.))
                                    .text_color(color(style.colors.muted))
                                    .truncate()
                                    .child(entry.id.clone()),
                            ),
                    )
                    .on_hover(cx.listener(move |this, entering: &bool, window, cx| {
                        this.online.hover(&id, *entering);
                        this.pump_online(window, cx);
                        cx.notify();
                    }))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if let Some(theme) = this.online.apply(&click_id) {
                            this.apply_online_theme(theme);
                        }
                        this.pump_online(window, cx);
                        cx.notify();
                    }));
                list = list.child(div().id(("online-theme-row", index)).flex_none().child(
                    row.context_menu(move |menu, _, _| {
                        let owner = owner.clone();
                        let id = download_id.clone();
                        let identity = identity.clone();
                        menu.item(
                            PopupMenuItem::new("下载到本地")
                                .disabled(download_disabled)
                                .on_click(move |_, window, cx| {
                                    if let Some(owner) = owner.upgrade() {
                                        owner.update(cx, |this, cx| {
                                            if this.online.identity(&id).as_ref() != Some(&identity)
                                            {
                                                return;
                                            }
                                            if let Some((source, theme)) = this.online.download(&id)
                                            {
                                                this.install_online_theme(
                                                    source, theme, window, cx,
                                                );
                                            }
                                            this.pump_online(window, cx);
                                            cx.notify();
                                        });
                                    }
                                }),
                        )
                    }),
                ));
            }
        }
        list
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        model::AppModel,
        theme_local::{ResetThemeDirectory, ThemeStore},
        theme_online::test_support::{Fixture, bytes, source},
    };
    use gpui::{Modifiers, TestAppContext};
    use std::{cell::RefCell, rc::Rc};

    #[gpui::test]
    fn online_browsing_uses_current_resume_keeps_right_panel_fixed_and_sources_independent(
        cx: &mut TestAppContext,
    ) {
        cx.update(gpui_component::init);
        let root = tempfile::tempdir().unwrap();
        let mut store = ThemeStore::new(
            root.path().join("settings.json"),
            root.path().join("themes"),
        )
        .unwrap();
        store.save_sources(vec![source("a"), source("b")]).unwrap();
        let mut model = AppModel::load(None).unwrap();
        model.replace_themes(store.load_themes().unwrap());
        let resume = model.document.source.clone();
        let child = Rc::new(RefCell::new(None));
        let output = child.clone();
        let (_, cx) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| {
                let mut app = ResumeOne::new(model, cx);
                app.theme_store = Some(store);
                app.online.provider = Some(Arc::new(Fixture));
                app
            });
            *output.borrow_mut() = Some(view.clone());
            gpui_component::Root::new(view, window, cx)
        });
        let owner = child.borrow_mut().take().unwrap();
        cx.simulate_resize(gpui::size(px(1120.), px(780.)));
        cx.update(|window, _| window.refresh());
        let right = cx.debug_bounds("module-panel").unwrap();
        let tab = cx.debug_bounds("themes-online").unwrap();
        cx.simulate_click(tab.center(), Modifiers::none());
        cx.run_until_parked();
        cx.update(|_, cx| {
            let app = owner.read(cx);
            assert_eq!(app.online.catalog.as_ref().unwrap().entries().len(), 50);
            assert!(app.model.applied_online.is_none());
        });
        cx.update(|window, _| window.refresh());
        let second = cx.debug_bounds("theme-1").unwrap();
        cx.simulate_mouse_move(second.center(), None, Modifiers::none());
        cx.run_until_parked();
        cx.update(|_, cx| {
            let app = owner.read(cx);
            assert_eq!(app.display_theme().manifest.id, "theme-1");
            assert_eq!(app.model.applied_theme().manifest.id, "zhibai");
            assert_eq!(app.model.document.source, resume);
        });
        cx.simulate_click(second.center(), Modifiers::none());
        cx.run_until_parked();
        let applied = cx.update(|_, cx| owner.read(cx).model.applied_theme().identity.clone());
        assert!(applied.contains(&source("a").key()));
        let picker = cx.debug_bounds("theme-source-picker").unwrap();
        cx.simulate_click(picker.center(), Modifiers::none());
        cx.update(|window, _| window.refresh());
        let choice = cx.debug_bounds("source-choice-1").unwrap();
        cx.simulate_click(choice.center(), Modifiers::none());
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        let second = cx.debug_bounds("theme-1").unwrap();
        cx.simulate_mouse_move(second.center(), None, Modifiers::none());
        cx.run_until_parked();
        cx.update(|_, cx| {
            let app = owner.read(cx);
            assert!(app.display_theme().identity.contains(&source("b").key()));
            assert_eq!(app.model.applied_theme().identity, applied);
            assert_eq!(app.model.document.source, resume);
        });
        assert_eq!(right, cx.debug_bounds("module-panel").unwrap());
        cx.simulate_event(gpui::MouseDownEvent {
            position: second.center(),
            button: gpui::MouseButton::Right,
            ..Default::default()
        });
        cx.run_until_parked();
        cx.simulate_keystrokes("down");
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        cx.update(|_, cx| {
            let app = owner.read(cx);
            assert_eq!(app.model.themes.len(), 21);
            let downloaded = app
                .model
                .themes
                .iter()
                .find(|theme| theme.manifest.id == "theme-1")
                .unwrap();
            assert_eq!(
                std::fs::read(downloaded.path.as_ref().unwrap()).unwrap(),
                bytes(1)
            );
            assert_eq!(app.model.applied_theme().identity, applied);
        });
        // A real wheel event must move the prefetch window, not just the visual list.
        cx.simulate_event(gpui::ScrollWheelEvent {
            position: second.center(),
            delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(-2400.))),
            ..Default::default()
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            let app = owner.read(cx);
            assert!(app.theme_scroll.offset().y < px(-1000.));
            assert!(app.online.color("theme-40").is_some());
            assert_eq!(app.model.applied_theme().identity, applied);
            assert_eq!(app.model.document.source, resume);
        });
        let local = cx.debug_bounds("themes-local").unwrap();
        cx.simulate_click(local.center(), Modifiers::none());
        cx.run_until_parked();
        cx.update(|_, cx| {
            let app = owner.read(cx);
            assert!(!app.online.online);
            assert_eq!(app.display_theme().identity, applied);
        });
    }

    #[gpui::test]
    fn gui_download_writes_verified_original_to_current_local_directory(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let root = tempfile::tempdir().unwrap();
        let mut store = ThemeStore::new(
            root.path().join("settings.json"),
            root.path().join("defaults"),
        )
        .unwrap();
        let custom = root.path().join("custom");
        std::fs::create_dir(&custom).unwrap();
        let custom = custom.canonicalize().unwrap();
        store.choose(custom.clone()).unwrap();
        let catalog = crate::theme_online::test_support::catalog(&source("a"));
        let theme =
            Arc::new(VerifiedTheme::parse(catalog.entry("theme-1").unwrap(), &bytes(1)).unwrap());
        let model = AppModel::load(None).unwrap();
        let resume = model.document.source.clone();
        let (owner, cx) = cx.add_window_view(|_, cx| {
            let mut app = ResumeOne::new(model, cx);
            app.theme_store = Some(store);
            app
        });
        cx.update(|window, cx| {
            owner.update(cx, |app, cx| {
                app.install_online_theme(source("a"), theme, window, cx);
                assert!(app.online.downloading);
                // Directory changes cannot race the installation's atomic commit.
                app.reset_theme_directory(&ResetThemeDirectory, window, cx);
                assert_eq!(app.theme_store.as_ref().unwrap().directory, custom);
            });
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            let app = owner.read(cx);
            assert!(!app.online.downloading);
            let downloaded = app
                .model
                .themes
                .iter()
                .find(|theme| theme.manifest.id == "theme-1")
                .unwrap();
            let path = downloaded.path.as_ref().unwrap();
            assert_eq!(path.parent().unwrap(), custom);
            assert_eq!(std::fs::read(path).unwrap(), bytes(1));
            assert!(app.online.diagnostics.is_empty());
            assert_eq!(app.model.document.source, resume);
            assert_eq!(app.model.themes.len(), 21);
        });
        assert_eq!(
            std::fs::read_dir(root.path().join("defaults"))
                .unwrap()
                .count(),
            20
        );
    }
}
