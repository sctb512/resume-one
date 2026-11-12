use std::sync::Arc;

use crate::i18n::tr;
use gpui::{Context, IntoElement, Window, div, prelude::*, px};
use gpui_component::input::{Input, InputEvent, InputPalette, InputState};
use gpui_component::menu::{ContextMenuExt, PopupMenuItem};
use resume_one_themes::{GithubClient, GithubSource, ThemeCache, VerifiedTheme};

use crate::{ResumeOne, color, model::BuiltinTheme, theme_online::Provider};

pub(super) fn layout_label(style: &resume_one_core::theme_style::ThemeStyle) -> &'static str {
    if style.layout.contains("sidebar") {
        tr("侧栏排版")
    } else if style.split_header {
        tr("分栏页眉")
    } else if style.centered_header {
        tr("居中页眉")
    } else {
        tr("单栏排版")
    }
}

/// Keep all lightweight rows for a stable scroll range, but only measure
/// previews within two rows of the viewport. This stays below the 40-layout
/// cache even when the local library grows beyond the bundled collection.
pub(super) fn thumbnail_window(offset: f32, height: f32, total: usize) -> std::ops::Range<usize> {
    let row = crate::chrome::THEME_ROW_HEIGHT;
    // Wheel deltas can overshoot the end. GPUI clamps its handle in prepaint,
    // after this render, so culling must clamp before calculating row indices.
    let max_offset = (total as f32 * row - height.max(row)).max(0.);
    let first = ((-offset).clamp(0., max_offset) / row).floor() as usize;
    let count = (height.max(row) / row).ceil() as usize;
    first.saturating_sub(2).min(total)..first.saturating_add(count + 2).min(total)
}

impl ResumeOne {
    pub(super) fn ensure_theme_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.theme_search.is_some() || !cx.has_global::<gpui_component::Theme>() {
            return;
        }
        let search = cx.new(|cx| {
            InputState::new(window, cx).validate(|value, _| !value.contains(['\r', '\n']))
        });
        cx.subscribe_in(&search, window, |this, input, event, window, cx| {
            if matches!(event, InputEvent::Change) {
                this.theme_query = input.read(cx).value().trim().to_lowercase();
                this.theme_scroll.set_offset(gpui::point(px(0.), px(0.)));
                this.hovered_theme = None;
                this.theme_hover_generation += 1;
                this.theme_hover_candidate = None;
                this.online.clear_hover();
                this.prefetch_theme_results(window, cx);
                cx.notify();
            }
        })
        .detach();
        self.theme_search = Some(search);
    }

    pub(super) fn theme_matches(&self, labels: &[&str]) -> bool {
        self.theme_query.is_empty()
            || labels
                .iter()
                .any(|label| label.to_lowercase().contains(&self.theme_query))
    }

    pub(super) fn local_theme_results(&self) -> Vec<(usize, &BuiltinTheme)> {
        let locale = crate::i18n::theme_locale();
        self.model
            .themes
            .iter()
            .enumerate()
            .filter(|(_, theme)| {
                self.theme_matches(&[
                    &theme.manifest.id,
                    theme.manifest.localized_name(&locale),
                    theme.manifest.localized_description(&locale),
                    layout_label(&theme.style),
                ])
            })
            .collect()
    }

    pub(super) fn online_results(&self) -> Vec<(usize, &resume_one_themes::ThemeEntry)> {
        self.online
            .catalog
            .iter()
            .flat_map(|catalog| catalog.entries().iter().enumerate())
            .filter(|(_, entry)| {
                let locale = crate::i18n::theme_locale();
                let theme = (!self.theme_query.is_empty())
                    .then(|| self.online.thumbnail_theme(&entry.id))
                    .flatten();
                self.theme_matches(&[
                    &entry.id,
                    &entry.name,
                    theme
                        .as_ref()
                        .map_or("", |theme| theme.manifest.localized_name(&locale)),
                    theme
                        .as_ref()
                        .map_or("", |theme| layout_label(&theme.style)),
                ])
            })
            .collect()
    }

    fn prefetch_theme_results(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.online.online {
            return;
        }
        let row = ((-f32::from(self.theme_scroll.offset().y)
            + f32::from(self.theme_scroll.bounds().size.height) / 2.)
            / crate::chrome::THEME_ROW_HEIGHT)
            .floor()
            .max(0.) as usize;
        let results = self.online_results();
        if let Some(&(index, _)) = results.get(row.min(results.len().saturating_sub(1))) {
            self.online.scroll_to(index);
        }
        self.pump_online(window, cx);
    }

    pub(super) fn theme_list_status(&self, message: &'static str) -> impl IntoElement {
        let palette = crate::chrome::Palette::new(self.shell_style());
        div()
            .px_2()
            .py_3()
            .text_size(px(12.))
            .text_color(color(palette.muted))
            .child(message)
    }

    pub(super) fn hover_local_theme(
        &mut self,
        index: usize,
        entering: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let identity = self.model.themes[index].identity.clone();
        if !entering {
            if self.theme_hover_candidate.as_ref() == Some(&identity) {
                self.theme_hover_generation += 1;
                self.theme_hover_candidate = None;
            }
            if self.hovered_theme == Some(index) {
                self.hovered_theme = None;
                cx.notify();
            }
            return;
        }
        self.theme_hover_generation += 1;
        let generation = self.theme_hover_generation;
        self.theme_hover_candidate = Some(identity.clone());
        let timer = cx
            .background_executor()
            .timer(std::time::Duration::from_millis(160));
        cx.spawn_in(window, async move |this, cx| {
            timer.await;
            this.update_in(cx, |this, _, cx| {
                if this.theme_hover_generation == generation
                    && !this.source_mode
                    && !this.theme_manager_open
                    && !this.online.online
                    && this
                        .model
                        .themes
                        .get(index)
                        .is_some_and(|t| t.identity == identity)
                {
                    this.hovered_theme = Some(index);
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

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
                .ok_or_else(|| tr("无法定位主题缓存目录").to_owned())
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
                        this.editor_warning(tr("无法下载主题"), &error, window, cx);
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
        self.apply_theme_selection(theme);
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
                        this.editor_warning(tr("无法下载主题"), &error, window, cx);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn theme_navigation(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let style = self.shell_style();
        let p = crate::chrome::Palette::new(style);
        let mut tabs = div()
            .flex()
            .text_size(px(12.))
            .mx(px(14.))
            .mb_2()
            .border_b_1()
            .border_color(color(p.line));
        for (online, title) in [(false, tr("本地")), (true, tr("在线"))] {
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
                    .flex_1()
                    .flex()
                    .justify_center()
                    .pt_2()
                    .pb(px(11.))
                    .border_b_2()
                    .border_color(color(if online == self.online.online {
                        style.colors.accent
                    } else {
                        p.panel
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
        let mut nav = div().flex_none().flex().flex_col().gap_2().child(tabs);
        if self.online.online && !self.online.sources.is_empty() {
            let title = self
                .online
                .sources
                .get(self.online.source_index)
                .map(|source| crate::i18n::source_name(source).to_owned())
                .unwrap_or_else(|| tr("选择来源").into());
            nav = nav.child(
                div()
                    .id("theme-source-picker")
                    .debug_selector(|| "theme-source-picker".into())
                    .px_2()
                    .py_1()
                    .text_size(px(12.))
                    .text_color(color(style.colors.muted))
                    .cursor_pointer()
                    .overflow_hidden()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(div().flex_1().min_w_0().truncate().child(title))
                    .child("▾")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.online.sources_open = !this.online.sources_open;
                        cx.notify();
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
                            .text_size(px(12.))
                            .px_2()
                            .py_2()
                            .rounded_md()
                            .truncate()
                            .cursor_pointer()
                            .hover(|row| row.bg(color(style.colors.paper)))
                            .child(crate::i18n::source_name(source).to_owned())
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
        if let Some(search) = &self.theme_search {
            let background = p.panel.mix(p.ink, 0.035);
            let selection = background.mix(style.colors.accent, 0.16);
            let focus = crate::chrome::readable_on(p.accent, background);
            let foreground = crate::chrome::readable_on(p.ink, selection);
            nav = nav.child(
                div()
                    .px_2()
                    .pb_2()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(color(p.muted))
                            .child(tr("搜索主题")),
                    )
                    .child(
                        Input::new(search)
                            .w_full()
                            .rounded_md()
                            .palette(InputPalette {
                                background: color(background).into(),
                                border: color(p.line).into(),
                                focus: color(focus).into(),
                                foreground: color(foreground).into(),
                                placeholder: color(crate::chrome::readable_on(p.muted, background))
                                    .into(),
                                selection: color(selection).into(),
                            }),
                    ),
            );
        }
        nav
    }

    pub(super) fn online_theme_list(&self, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let style = self.shell_style();
        let p = crate::chrome::Palette::new(style);
        let mut list = div()
            .id("theme-list")
            .flex()
            .flex_col()
            .px_2()
            .overflow_y_scroll()
            .flex_1()
            .min_h_0()
            .track_scroll(&self.theme_scroll)
            .on_scroll_wheel(cx.listener(|this, _, window, cx| {
                this.theme_hover_generation += 1;
                this.theme_hover_candidate = None;
                this.online.clear_hover();
                let owner = cx.entity().downgrade();
                window.defer(cx, move |window, cx| {
                    if let Some(owner) = owner.upgrade() {
                        owner.update(cx, |this, cx| {
                            this.prefetch_theme_results(window, cx);
                            cx.notify();
                        });
                    }
                });
                cx.notify();
            }));
        if self.online.catalog.is_none() {
            let message = if self.online.sources.is_empty() {
                tr("暂无在线来源")
            } else if self.online.catalog_failed || self.online.provider.is_none() {
                tr("无法加载主题目录 · 重新选择来源可重试")
            } else {
                tr("正在读取主题目录…")
            };
            list = list.child(self.theme_list_status(message));
        } else {
            let locale = crate::i18n::theme_locale();
            let results = self.online_results();
            if results.is_empty() {
                list = list.child(self.theme_list_status(tr("没有匹配的主题")));
            }
            for (index, entry) in results {
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
                let thumbnail_theme = self.online.thumbnail_theme(&entry.id);
                let description = thumbnail_theme
                    .as_ref()
                    .filter(|theme| theme.original.is_none())
                    .map_or("", |theme| layout_label(&theme.style));
                let row = div()
                    .id(("theme", index))
                    .debug_selector(move || format!("theme-{index}"))
                    .flex()
                    .flex_none()
                    .relative()
                    .h(px(crate::chrome::THEME_ROW_HEIGHT))
                    .items_center()
                    .gap_3()
                    .px(px(9.))
                    .py(px(10.))
                    .border_b_1()
                    .border_color(color(p.line))
                    .cursor_pointer()
                    .when(selected, |row| row.bg(color(p.soft)))
                    .when(!selected, |row| {
                        row.hover(|row| row.border_color(color(p.accent)))
                    })
                    .when(selected, |row| {
                        row.child(
                            div()
                                .absolute()
                                .left_0()
                                .top(px(11.))
                                .bottom(px(11.))
                                .w(px(3.))
                                .rounded_sm()
                                .bg(color(style.colors.accent)),
                        )
                    })
                    .child(self.theme_thumbnail(thumbnail_theme.as_ref()))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(7.))
                            .child(
                                div()
                                    .font_family(crate::chrome::SERIF)
                                    .text_size(px(15.))
                                    .text_color(color(if selected { p.accent } else { p.ink }))
                                    .line_clamp(2)
                                    .child(thumbnail_theme.as_ref().map_or_else(
                                        || entry.name.clone(),
                                        |theme| theme.manifest.localized_name(&locale).to_owned(),
                                    )),
                            )
                            .when(!description.is_empty(), |label| {
                                label.child(
                                    div()
                                        .text_size(px(12.))
                                        .line_height(px(17.))
                                        .line_clamp(2)
                                        .text_color(color(p.muted))
                                        .child(description),
                                )
                            }),
                    )
                    .on_hover(cx.listener(move |this, entering: &bool, window, cx| {
                        let identity = this.online.identity(&id);
                        if !entering && this.theme_hover_candidate != identity {
                            return;
                        }
                        this.theme_hover_generation += 1;
                        let generation = this.theme_hover_generation;
                        if !entering {
                            this.theme_hover_candidate = None;
                            this.online.hover(&id, false);
                        } else {
                            this.theme_hover_candidate = identity.clone();
                            let id = id.clone();
                            let timer = cx
                                .background_executor()
                                .timer(std::time::Duration::from_millis(160));
                            cx.spawn_in(window, async move |this, cx| {
                                timer.await;
                                this.update_in(cx, |this, window, cx| {
                                    if generation == this.theme_hover_generation
                                        && !this.source_mode
                                        && !this.theme_manager_open
                                        && identity == this.online.identity(&id)
                                        && this.online.online
                                    {
                                        this.online.hover(&id, true);
                                        this.pump_online(window, cx);
                                        cx.notify();
                                    }
                                })
                                .ok();
                            })
                            .detach();
                        }
                        cx.notify();
                    }))
                    .on_click(
                        cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                            if !event.standard_click() {
                                return;
                            }
                            if let Some(theme) = this.online.apply(&click_id) {
                                this.drawer = None;
                                this.apply_online_theme(theme);
                            }
                            this.pump_online(window, cx);
                            window.focus(&this.focus);
                            cx.notify();
                        }),
                    );
                list = list.child(div().id(("online-theme-row", index)).flex_none().child(
                    row.context_menu(move |menu, _, _| {
                        let owner = owner.clone();
                        let id = download_id.clone();
                        let identity = identity.clone();
                        menu.item(
                            PopupMenuItem::new(tr("下载到本地"))
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

    #[test]
    fn thumbnail_culling_survives_both_end_overscroll_and_empty_lists() {
        assert_eq!(thumbnail_window(-100_000., 600., 21), 12..21);
        assert_eq!(thumbnail_window(100_000., 600., 21), 0..9);
        assert_eq!(thumbnail_window(-100_000., 600., 0), 0..0);
        assert_eq!(thumbnail_window(-100_000., 600., 1), 0..1);
    }
    use crate::{
        model::AppModel,
        theme_local::{ResetThemeDirectory, ThemeStore},
        theme_online::test_support::{Fixture, bytes, source},
    };
    use gpui::{Modifiers, TestAppContext};
    use std::{cell::RefCell, rc::Rc};

    #[gpui::test]
    fn empty_online_list_has_no_duplicate_management_entry(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let (view, cx) =
            cx.add_window_view(|_, cx| ResumeOne::new(AppModel::load(None).unwrap(), cx));
        cx.update(|_, cx| {
            view.update(cx, |app, cx| {
                app.online.set_online(true);
                cx.notify();
            })
        });
        assert!(cx.debug_bounds("manage-themes").is_some());
        assert!(cx.debug_bounds("theme-source-picker").is_none());
        let right = cx.debug_bounds("module-panel").unwrap();
        cx.update(|_, cx| {
            view.update(cx, |app, cx| {
                app.online.sync_sources(&[source("a")]);
                cx.notify();
            })
        });
        assert!(cx.debug_bounds("theme-source-picker").is_some());
        assert_eq!(right, cx.debug_bounds("module-panel").unwrap());
    }

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
        let toggle = cx.debug_bounds("show-themes").unwrap();
        cx.simulate_click(toggle.center(), Modifiers::none());
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
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(160));
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
        let toggle = cx.debug_bounds("show-themes").unwrap();
        cx.simulate_click(toggle.center(), Modifiers::none());
        cx.update(|window, _| window.refresh());
        let picker = cx.debug_bounds("theme-source-picker").unwrap();
        cx.simulate_click(picker.center(), Modifiers::none());
        cx.update(|window, _| window.refresh());
        let choice = cx.debug_bounds("source-choice-1").unwrap();
        cx.simulate_click(choice.center(), Modifiers::none());
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        let second = cx.debug_bounds("theme-1").unwrap();
        cx.simulate_mouse_move(second.center(), None, Modifiers::none());
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(160));
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
            assert_eq!(
                app.model.themes.len(),
                crate::model::BUILTIN_THEMES.len() + 1
            );
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
            let center = ((-f32::from(app.theme_scroll.offset().y)
                + f32::from(app.theme_scroll.bounds().size.height) / 2.)
                / crate::chrome::THEME_ROW_HEIGHT)
                .floor() as usize;
            assert!(
                app.online
                    .thumbnail_theme(&format!("theme-{center}"))
                    .is_some()
            );
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
            assert_eq!(
                app.model.themes.len(),
                crate::model::BUILTIN_THEMES.len() + 1
            );
        });
        assert_eq!(
            std::fs::read_dir(root.path().join("defaults"))
                .unwrap()
                .filter(|entry| {
                    entry
                        .as_ref()
                        .unwrap()
                        .path()
                        .to_string_lossy()
                        .ends_with(".theme.json")
                })
                .count(),
            crate::model::BUILTIN_THEMES.len()
        );
    }
}
