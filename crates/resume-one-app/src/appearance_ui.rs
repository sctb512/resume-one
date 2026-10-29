//! A docked appearance inspector; the document is never covered by these controls.
use crate::{ResumeOne, chrome, color};
use gpui::{
    Context, Entity, Focusable as _, IntoElement, PathPromptOptions, Render, ScrollHandle,
    Subscription, UniformListScrollHandle, WeakEntity, Window, actions, div, prelude::*, px,
    uniform_list,
};
use gpui_component::{
    Sizable as _,
    input::{Input, InputEvent, InputState},
    scroll::{Scrollbar, ScrollbarShow},
};
use resume_one_core::appearance::{Appearance, COLOR_KEYS, FontChoice};

mod palette;
mod trial;

actions!(resume_one, [ShowAppearance]);

pub(super) struct AppearanceEditor {
    owner: WeakEntity<ResumeOne>,
    inputs: Vec<Entity<InputState>>,
    scroll: ScrollHandle,
    error: Option<String>,
    _subscriptions: Vec<Subscription>,
    _observer: Subscription,
    default_colors: [resume_one_core::theme_style::Rgb; 7],
    font_names: Vec<String>,
    font_search: Entity<InputState>,
    font_scroll: UniformListScrollHandle,
    font_target: Option<bool>,
    font_busy: bool,
    font_notice: Option<String>,
    font_generation: [u64; 2],
    font_resolving: bool,
    pending_font: [Option<Option<FontChoice>>; 2],
    font_highlight: usize,
    font_focus: [gpui::FocusHandle; 2],
    palette: palette::PaletteState,
    trial: trial::TrialState,
}

fn color_label(key: &str) -> &'static str {
    match key {
        "accent" => "强调色",
        "paper" => "纸张",
        "canvas" => "工作区",
        "ink" => "正文",
        "muted" => "辅助文字",
        "border" => "分隔线",
        "seal" => "印色",
        _ => "颜色",
    }
}

impl AppearanceEditor {
    fn new(
        owner: Entity<ResumeOne>,
        appearance: &Appearance,
        defaults: resume_one_core::theme_style::Colors,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let colors = [
            defaults.accent,
            defaults.paper,
            defaults.canvas,
            defaults.ink,
            defaults.muted,
            defaults.border,
            defaults.seal,
        ];
        let inputs: Vec<_> = COLOR_KEYS
            .iter()
            .zip(colors)
            .map(|(key, value)| {
                cx.new(|cx| {
                    InputState::new(window, cx)
                        .default_value(appearance.color(key).unwrap_or("").to_owned())
                        .placeholder(value.hex())
                })
            })
            .collect();
        let mut subscriptions: Vec<_> = inputs
            .iter()
            .enumerate()
            .map(|(index, input)| {
                cx.subscribe_in(input, window, move |this, input, event, window, cx| {
                    let value = input.read(cx).value().to_string();
                    match event {
                        InputEvent::Change => this.edit_color(index, &value, false, window, cx),
                        InputEvent::Blur => this.edit_color(index, &value, true, window, cx),
                        _ => {}
                    }
                })
            })
            .collect();
        let font_search = cx.new(|cx| InputState::new(window, cx).placeholder("搜索字体名称"));
        subscriptions.push(cx.subscribe(&font_search, |this, _, event, cx| {
            if matches!(event, InputEvent::Change) {
                this.clear_trial(cx);
                this.font_highlight = 0;
                cx.notify();
            }
        }));
        let task = cx
            .background_executor()
            .spawn(async { resume_one_core::fonts::available_families() });
        cx.spawn_in(window, async move |this, cx| {
            let names = task.await;
            this.update_in(cx, |this, _, cx| {
                match names {
                    Ok(names) => this.font_names = names,
                    Err(error) => this.error = Some(error),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        let observer = cx.observe(&owner, |_, _, cx| cx.notify());
        Self {
            owner: owner.downgrade(),
            inputs,
            scroll: ScrollHandle::new(),
            error: None,
            _subscriptions: subscriptions,
            _observer: observer,
            default_colors: colors,
            font_names: Vec::new(),
            font_search,
            font_scroll: UniformListScrollHandle::new(),
            font_target: None,
            font_busy: false,
            font_notice: None,
            font_generation: [0; 2],
            font_resolving: false,
            pending_font: [None, None],
            font_highlight: 0,
            font_focus: [cx.focus_handle(), cx.focus_handle()],
            palette: palette::PaletteState::new(cx),
            trial: trial::TrialState::default(),
        }
    }

    fn apply(&mut self, appearance: Appearance, window: &mut Window, cx: &mut Context<Self>) {
        self.clear_trial(cx);
        let Some(owner) = self.owner.upgrade() else {
            return;
        };
        self.error = owner
            .update(cx, |app, cx| app.change_appearance(appearance, window, cx))
            .err();
        cx.notify();
    }

    fn edit_color(
        &mut self,
        index: usize,
        value: &str,
        show_error: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(owner) = self.owner.upgrade() else {
            return;
        };
        let mut appearance = owner.read(cx).model.appearance.clone();
        if let Err(error) = appearance.set_color(COLOR_KEYS[index], value) {
            self.error = show_error.then(|| format!("{}：{error}", color_label(COLOR_KEYS[index])));
            cx.notify();
            return;
        }
        self.apply(appearance, window, cx);
        if self.error.is_none() {
            self.sync_palette_color(index, value);
        }
    }

    fn change_font(
        &mut self,
        heading: bool,
        choice: Option<FontChoice>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(owner) = self.owner.upgrade() else {
            return;
        };
        let mut appearance = owner.read(cx).model.appearance.clone();
        if heading {
            appearance.heading_font = choice;
        } else {
            appearance.body_font = choice;
        }
        self.apply(appearance, window, cx);
    }

    fn reset_colors(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.clear_trial(cx);
        self.palette.target = None;
        let Some(owner) = self.owner.upgrade() else {
            return;
        };
        let mut appearance = owner.read(cx).model.appearance.clone();
        appearance.reset_colors();
        self.apply(appearance, window, cx);
        if self.error.is_none() {
            for input in &self.inputs {
                input.update(cx, |input, cx| input.set_value("", window, cx));
            }
        }
    }

    fn font_choices(&self, cx: &gpui::App) -> Vec<(String, Option<FontChoice>)> {
        let query = self.font_search.read(cx).value().trim().to_lowercase();
        let mut choices = vec![
            ("跟随主题".into(), None),
            ("Noto Sans SC · 内置".into(), Some(FontChoice::Sans)),
            ("Noto Serif SC · 内置".into(), Some(FontChoice::Serif)),
        ];
        choices.extend(
            self.font_names
                .iter()
                .filter(|name| !matches!(name.as_str(), "Noto Sans SC" | "Noto Serif SC"))
                .map(|name| (name.clone(), Some(FontChoice::Font(format!("font:{name}"))))),
        );
        if !query.is_empty() {
            choices.retain(|(name, _)| name.to_lowercase().contains(&query));
        }
        choices
    }
    fn close_font_browser(&mut self, heading: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.clear_trial(cx);
        self.font_target = None;
        window.focus(&self.font_focus[usize::from(heading)]);
        cx.notify();
    }
    fn move_font_highlight(&mut self, down: bool, cx: &mut Context<Self>) {
        let count = self.font_choices(cx).len();
        if count == 0 {
            return;
        }
        self.font_highlight = if down {
            (self.font_highlight + 1).min(count - 1)
        } else {
            self.font_highlight.saturating_sub(1)
        };
        self.font_scroll
            .scroll_to_item(self.font_highlight, gpui::ScrollStrategy::Center);
        cx.notify();
    }

    fn select_font(
        &mut self,
        heading: bool,
        choice: Option<FontChoice>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.clear_trial(cx);
        let target = usize::from(heading);
        self.font_generation[target] = self.font_generation[target].wrapping_add(1);
        let generation = self.font_generation[target];
        let Some(FontChoice::Font(token)) = &choice else {
            self.pending_font[target] = None;
            self.font_busy = self.font_resolving;
            self.change_font(heading, choice, window, cx);
            return;
        };
        let family = token[5..].to_owned();
        if self.font_resolving {
            self.pending_font[target] = Some(choice);
            self.font_busy = true;
            cx.notify();
            return;
        }
        self.font_resolving = true;
        self.font_busy = true;
        self.error = None;
        cx.notify();
        let task = cx
            .background_executor()
            .spawn(async move { resume_one_core::fonts::resolve(&family, 400) });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            this.update_in(cx, |this, window, cx| {
                this.font_resolving = false;
                this.font_busy = false;
                if generation == this.font_generation[target] {
                    match result {
                        Ok(_) => this.change_font(heading, choice, window, cx),
                        Err(error) => this.error = Some(error),
                    }
                }
                if let Some(target) = this.pending_font.iter().position(Option::is_some) {
                    let choice = this.pending_font[target].take().unwrap();
                    this.select_font(target == 1, choice, window, cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn dismiss_transient(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.palette.is_sampling() {
            // The native sampler owns Escape until it reports cancellation.
            return true;
        }
        if let Some(heading) = self.font_target {
            self.close_font_browser(heading, window, cx);
            return true;
        }
        if self.palette.is_open() {
            self.palette_escape(window, cx);
            return true;
        }
        false
    }

    fn install_fonts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.font_busy {
            return;
        }
        self.clear_trial(cx);
        let chooser = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("安装字体到简里".into()),
        });
        self.font_busy = true;
        self.error = None;
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let selected = chooser.await;
            let result = match selected {
                Ok(Ok(Some(paths))) if paths.len() <= 16 => {
                    let task = cx.background_executor().spawn(async move {
                        let root = resume_one_core::fonts::library_dir()?;
                        let mut errors = Vec::new();
                        let mut installed = Vec::new();
                        for path in paths {
                            match resume_one_core::fonts::install(&path, &root) {
                                Ok(names) => installed.extend(names),
                                Err(error) => errors.push(format!(
                                    "{}：{error}",
                                    path.file_name().unwrap_or_default().to_string_lossy()
                                )),
                            }
                        }
                        installed.sort();
                        installed.dedup();
                        let names = resume_one_core::fonts::available_families()?;
                        let error = (!errors.is_empty()).then(|| {
                            let mut message = errors
                                .iter()
                                .take(3)
                                .cloned()
                                .collect::<Vec<_>>()
                                .join("\n");
                            if errors.len() > 3 {
                                message.push_str(&format!("\n另有 {} 项未安装", errors.len() - 3));
                            }
                            message
                        });
                        Ok((names, error, installed))
                    });
                    task.await
                }
                Ok(Ok(None)) => Ok((Vec::new(), None, Vec::new())),
                Ok(Ok(Some(_))) => Err("一次最多安装 16 个字体文件".into()),
                _ => Err("无法读取字体选择结果，请重试".into()),
            };
            this.update_in(cx, |this, window, cx| {
                this.font_busy = false;
                match result {
                    Ok((names, error, installed)) => {
                        if !names.is_empty() {
                            this.font_names = names;
                        }
                        this.error = error;
                        this.font_notice = (!installed.is_empty())
                            .then(|| format!("已安装 {} 个字族 · 仅供简里使用", installed.len()));
                        if let Some(name) = installed.first() {
                            this.font_search
                                .update(cx, |input, cx| input.set_value(name.clone(), window, cx));
                        }
                    }
                    Err(error) => this.error = Some(error),
                }
                if this.font_target.is_some() {
                    window.focus(&this.font_search.read(cx).focus_handle(cx));
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn font_browser(
        &self,
        heading: bool,
        p: chrome::Palette,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let choices = self.font_choices(cx);
        let count = choices.len();
        let current = self.owner.upgrade().and_then(|owner| {
            let app = owner.read(cx);
            if heading {
                app.model.appearance.heading_font.clone()
            } else {
                app.model.appearance.body_font.clone()
            }
        });
        div()
            .id("appearance-font-browser")
            .child(self.pointer_events(cx))
            .debug_selector(|| "appearance-font-browser".into())
            .size_full()
            .flex()
            .flex_col()
            .text_color(color(p.ink))
            .on_hover(cx.listener(|this, hovering: &bool, _, cx| {
                if !*hovering {
                    this.leave_font_list(cx);
                }
            }))
            .on_scroll_wheel(cx.listener(|this, _: &gpui::ScrollWheelEvent, _, cx| {
                this.leave_font_list(cx);
            }))
            .capture_action(cx.listener(
                move |this, _: &crate::inline_ui::DismissInline, window, cx| {
                    this.close_font_browser(heading, window, cx);
                    cx.stop_propagation();
                },
            ))
            .capture_action(cx.listener(
                move |this, _: &gpui_component::input::Escape, window, cx| {
                    if !this.font_search.read(cx).is_composing() {
                        this.close_font_browser(heading, window, cx);
                        cx.stop_propagation();
                    }
                },
            ))
            .capture_action(
                cx.listener(|this, _: &gpui_component::input::MoveDown, _, cx| {
                    if !this.font_search.read(cx).is_composing() {
                        this.move_font_highlight(true, cx);
                        cx.stop_propagation();
                    }
                }),
            )
            .capture_action(
                cx.listener(|this, _: &gpui_component::input::MoveUp, _, cx| {
                    if !this.font_search.read(cx).is_composing() {
                        this.move_font_highlight(false, cx);
                        cx.stop_propagation();
                    }
                }),
            )
            .capture_action(cx.listener(
                move |this, _: &gpui_component::input::Enter, window, cx| {
                    if !this.font_search.read(cx).is_composing() {
                        let choice = this.font_choices(cx).get(this.font_highlight).cloned();
                        if let Some((_, choice)) = choice {
                            this.select_font(heading, choice, window, cx);
                        }
                        cx.stop_propagation();
                    }
                },
            ))
            .capture_key_down(
                cx.listener(move |this, event: &gpui::KeyDownEvent, window, cx| {
                    if event.keystroke.modifiers.platform
                        || event.keystroke.modifiers.control
                        || event.keystroke.modifiers.alt
                    {
                        return;
                    }
                    let choices = this.font_choices(cx);
                    match event.keystroke.key.as_str() {
                        "down" | "up" if !choices.is_empty() => {
                            let max = choices.len() - 1;
                            this.font_highlight = if event.keystroke.key == "down" {
                                (this.font_highlight + 1).min(max)
                            } else {
                                this.font_highlight.saturating_sub(1)
                            };
                            this.font_scroll
                                .scroll_to_item(this.font_highlight, gpui::ScrollStrategy::Center);
                        }
                        "enter"
                            if this
                                .font_search
                                .read(cx)
                                .focus_handle(cx)
                                .is_focused(window) =>
                        {
                            if let Some((_, choice)) = choices.get(this.font_highlight) {
                                this.select_font(heading, choice.clone(), window, cx);
                            }
                        }
                        "escape" => this.close_font_browser(heading, window, cx),
                        _ => return,
                    }
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .child(
                div()
                    .h(px(63.))
                    .flex_none()
                    .px(px(18.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .font_family(chrome::SERIF)
                            .text_size(px(18.))
                            .child(if heading {
                                "标题字体"
                            } else {
                                "正文字体"
                            }),
                    )
                    .child(
                        div()
                            .id("font-browser-back")
                            .debug_selector(|| "font-browser-back".into())
                            .tab_index(0)
                            .tab_stop(true)
                            .cursor_pointer()
                            .min_w(px(40.))
                            .h(px(30.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(5.))
                            .hover(|v| v.bg(color(p.soft)))
                            .text_size(px(12.))
                            .child("返回")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.close_font_browser(heading, window, cx);
                            }))
                            .on_key_down(cx.listener(move |this, event, window, cx| {
                                if chrome::activation_key(event) {
                                    cx.stop_propagation();
                                    this.close_font_browser(heading, window, cx);
                                }
                            })),
                    ),
            )
            .child(
                div()
                    .px_4()
                    .pb_3()
                    .child(Input::new(&self.font_search).small()),
            )
            .child(
                uniform_list(
                    "appearance-font-list",
                    count,
                    cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                        let choices = this.font_choices(cx);
                        range
                            .filter_map(|index| {
                                choices.get(index).cloned().map(|(name, choice)| {
                                    let selected = choice == current;
                                    let keyboard_choice = choice.clone();
                                    let hover_choice = choice.clone();
                                    let move_choice = choice.clone();
                                    div()
                                        .id(("font-choice", index))
                                        .debug_selector(move || {
                                            format!("appearance-font-choice-{index}")
                                        })
                                        .w_full()
                                        .h(px(36.))
                                        .px_4()
                                        .flex()
                                        .items_center()
                                        .min_w_0()
                                        .text_size(px(12.))
                                        .cursor_pointer()
                                        .tab_index(0)
                                        .tab_stop(true)
                                        .bg(color(if selected { p.soft } else { p.panel }))
                                        .when(index == this.font_highlight, |v| v.bg(color(p.soft)))
                                        .hover(|v| v.bg(color(p.soft)))
                                        .focus(|v| v.bg(color(p.soft)))
                                        .child(div().flex_1().min_w_0().truncate().child(name))
                                        .when(selected, |v| v.child("✓"))
                                        .on_hover(cx.listener(
                                            move |this, hovering: &bool, window, cx| {
                                                this.hover_font(
                                                    heading,
                                                    hover_choice.clone(),
                                                    *hovering,
                                                    window,
                                                    cx,
                                                );
                                            },
                                        ))
                                        .on_mouse_move(cx.listener(
                                            move |this, _: &gpui::MouseMoveEvent, window, cx| {
                                                this.hover_font(
                                                    heading,
                                                    move_choice.clone(),
                                                    true,
                                                    window,
                                                    cx,
                                                );
                                            },
                                        ))
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.select_font(heading, choice.clone(), window, cx)
                                        }))
                                        .on_key_down(cx.listener(move |this, event, window, cx| {
                                            if chrome::activation_key(event) {
                                                cx.stop_propagation();
                                                this.select_font(
                                                    heading,
                                                    keyboard_choice.clone(),
                                                    window,
                                                    cx,
                                                );
                                            }
                                        }))
                                })
                            })
                            .collect::<Vec<_>>()
                    }),
                )
                .flex_1()
                .min_h_0()
                .track_scroll(self.font_scroll.clone()),
            )
            .when(count == 0, |v| {
                v.child(
                    div()
                        .px_4()
                        .py_3()
                        .text_size(px(12.))
                        .child("未找到匹配字体"),
                )
            })
            .when(self.font_busy, |v| {
                v.child(
                    div()
                        .px_4()
                        .py_2()
                        .text_size(px(12.))
                        .child("正在读取字体…"),
                )
            })
            .when_some(
                self.error.clone().or(self.trial.error.clone()),
                |v, error| {
                    v.child(
                        div()
                            .id("font-browser-error")
                            .max_h(px(96.))
                            .overflow_y_scroll()
                            .px_4()
                            .py_2()
                            .text_size(px(12.))
                            .child(error),
                    )
                },
            )
            .when_some(self.font_notice.clone(), |v, notice| {
                v.child(
                    div()
                        .px_4()
                        .py_2()
                        .text_size(px(11.))
                        .text_color(color(p.muted))
                        .child(notice),
                )
            })
            .child(
                div().border_t_1().border_color(color(p.line)).p_3().child(
                    div()
                        .id("install-appearance-fonts")
                        .debug_selector(|| "install-appearance-fonts".into())
                        .h(px(32.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(5.))
                        .cursor_pointer()
                        .tab_index(0)
                        .tab_stop(true)
                        .hover(|v| v.bg(color(p.soft)))
                        .text_size(px(12.))
                        .child("安装字体…")
                        .on_click(cx.listener(|this, _, window, cx| this.install_fonts(window, cx)))
                        .on_key_down(cx.listener(|this, event, window, cx| {
                            if chrome::activation_key(event) {
                                cx.stop_propagation();
                                this.install_fonts(window, cx);
                            }
                        })),
                ),
            )
            .into_any_element()
    }
}

impl Render for AppearanceEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(owner) = self.owner.upgrade() else {
            return div().into_any_element();
        };
        let app = owner.read(cx);
        let defaults = app.model.applied_theme().style.colors;
        let defaults = [
            defaults.accent,
            defaults.paper,
            defaults.canvas,
            defaults.ink,
            defaults.muted,
            defaults.border,
            defaults.seal,
        ];
        let style = app.active_style();
        let p = chrome::Palette::new(app.shell_style());
        let appearance = app.model.appearance.clone();
        if let Some(heading) = self.font_target {
            return self.font_browser(heading, p, cx);
        }
        if defaults != self.default_colors {
            self.default_colors = defaults;
            for (input, color) in self.inputs.iter().zip(defaults) {
                input.update(cx, |input, cx| {
                    input.set_placeholder(color.hex(), window, cx)
                });
            }
        }
        let mut content = div()
            .id("appearance-scroll")
            .debug_selector(|| "appearance-scroll".into())
            .flex()
            .flex_col()
            .gap_4()
            .px(px(18.))
            .py_4()
            .pr(px(23.))
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.scroll);
        for (heading, title, current) in [
            (false, "正文与辅助文字", appearance.body_font.clone()),
            (true, "姓名与模块标题", appearance.heading_font.clone()),
        ] {
            let selector = format!(
                "appearance-font-{}",
                if heading { "heading" } else { "body" }
            );
            let name = match current {
                None => "跟随主题".to_owned(),
                Some(FontChoice::Sans) => "Noto Sans SC".into(),
                Some(FontChoice::Serif) => "Noto Serif SC".into(),
                Some(FontChoice::Font(name)) => name.trim_start_matches("font:").to_owned(),
            };
            let choices = div()
                .id(gpui::SharedString::from(selector.clone()))
                .debug_selector(move || selector.clone())
                .tab_index(0)
                .tab_stop(true)
                .track_focus(&self.font_focus[usize::from(heading)])
                .cursor_pointer()
                .flex()
                .items_center()
                .justify_between()
                .px_3()
                .h(px(34.))
                .rounded(px(5.))
                .border_1()
                .border_color(color(p.line))
                .bg(color(p.panel))
                .text_color(color(p.ink))
                .text_size(px(12.))
                .focus(|view| view.border_color(color(p.accent)))
                .child(div().min_w_0().truncate().child(name))
                .child("⌄")
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.font_target = Some(heading);
                    this.font_search.update(cx, |input, cx| {
                        input.set_value("", window, cx);
                        input.focus(window, cx);
                    });
                    cx.notify();
                }))
                .on_key_down(cx.listener(move |this, event, window, cx| {
                    if chrome::activation_key(event) {
                        cx.stop_propagation();
                        this.font_target = Some(heading);
                        this.font_search.update(cx, |input, cx| {
                            input.set_value("", window, cx);
                            input.focus(window, cx);
                        });
                        cx.notify();
                    }
                }));
            let font_focus = self.font_focus[usize::from(heading)].clone();
            let font_scroll = self.scroll.clone();
            content = content.child(
                div()
                    .on_children_prepainted(move |children, window, cx| {
                        if font_focus.is_focused(window)
                            && let Some(bounds) = children.last()
                        {
                            palette::reveal(*bounds, &font_scroll, window, cx);
                        }
                    })
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(color(p.muted))
                            .child(title),
                    )
                    .child(choices),
            );
        }
        content = content.child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .border_t_1()
                .border_color(color(p.line))
                .pt_4()
                .child(
                    div()
                        .font_family(chrome::SERIF)
                        .text_size(px(15.))
                        .child("配色"),
                )
                .child(
                    div()
                        .id("reset-appearance-colors")
                        .debug_selector(|| "reset-appearance-colors".into())
                        .tab_index(0)
                        .tab_stop(true)
                        .h(px(30.))
                        .flex()
                        .items_center()
                        .text_size(px(12.))
                        .text_color(color(p.accent))
                        .cursor_pointer()
                        .child("跟随主题")
                        .on_click(cx.listener(|this, _, window, cx| this.reset_colors(window, cx)))
                        .on_key_down(cx.listener(|this, event, window, cx| {
                            if chrome::activation_key(event) {
                                cx.stop_propagation();
                                this.reset_colors(window, cx);
                            }
                        })),
                ),
        );
        let c = style.colors;
        for (index, (key, value)) in COLOR_KEYS
            .iter()
            .zip([
                c.accent, c.paper, c.canvas, c.ink, c.muted, c.border, c.seal,
            ])
            .enumerate()
        {
            let _ = key;
            content = content.child(self.color_row(index, value, p, cx));
        }
        div()
            .id("appearance-panel")
            .child(self.pointer_events(cx))
            .debug_selector(|| "appearance-panel".into())
            .size_full()
            .flex()
            .flex_col()
            .text_color(color(p.ink))
            .capture_action(
                cx.listener(|this, _: &crate::inline_ui::DismissInline, window, cx| {
                    if this.palette.target.is_some() {
                        this.palette_escape(window, cx);
                        cx.stop_propagation();
                    }
                }),
            )
            .capture_action(
                cx.listener(|this, _: &gpui_component::input::Escape, window, cx| {
                    if this.palette.target.is_some() {
                        this.palette_escape(window, cx);
                        cx.stop_propagation();
                    }
                }),
            )
            .capture_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" && this.palette.target.is_some() {
                    this.palette_escape(window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(
                div()
                    .h(px(63.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(px(18.))
                    .child(
                        div()
                            .font_family(chrome::SERIF)
                            .text_size(px(18.))
                            .child("外观"),
                    )
                    .child(
                        div()
                            .id("finish-appearance")
                            .debug_selector(|| "finish-appearance".into())
                            .tab_index(0)
                            .tab_stop(true)
                            .cursor_pointer()
                            .min_w(px(40.))
                            .h(px(30.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(5.))
                            .hover(|view| view.bg(color(p.soft)))
                            .focus(|view| view.bg(color(p.soft)))
                            .text_size(px(12.))
                            .text_color(color(p.accent))
                            .child("完成")
                            .on_click(cx.listener(|this, _, window, cx| {
                                if let Some(owner) = this.owner.upgrade() {
                                    owner.update(cx, |app, cx| app.finish_appearance(window, cx));
                                }
                            }))
                            .on_key_down(cx.listener(|this, event, window, cx| {
                                if chrome::activation_key(event) {
                                    cx.stop_propagation();
                                    if let Some(owner) = this.owner.upgrade() {
                                        owner.update(cx, |app, cx| {
                                            app.finish_appearance(window, cx)
                                        });
                                    }
                                }
                            })),
                    ),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(content)
                    .child(div().absolute().inset_0().child(
                        Scrollbar::vertical(&self.scroll).scrollbar_show(ScrollbarShow::Always),
                    )),
            )
            .when_some(self.error.clone(), |view, error| {
                view.child(
                    div()
                        .px_4()
                        .py_2()
                        .text_size(px(12.))
                        .text_color(color(chrome::readable_on(c.seal, p.panel)))
                        .child(error),
                )
            })
            .child(
                div()
                    .flex_none()
                    .border_t_1()
                    .border_color(color(p.line))
                    .px_4()
                    .py_3()
                    .text_size(px(12.))
                    .text_color(color(p.muted))
                    .child("悬停试用 · 点击确认\n字体与配色仅保存在本机"),
            )
            .into_any_element()
    }
}

impl ResumeOne {
    pub(super) fn show_appearance(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.appearance_editor.is_some() || self.theme_manager_open {
            return;
        }
        self.inline_window_left(window, cx);
        self.clear_inline();
        self.cancel_insertion();
        self.close_drawer();
        self.files.open = false;
        self.export_menu_open = false;
        self.hovered_theme = None;
        self.online.cancel_interaction();
        let owner = cx.entity();
        let appearance = self.model.appearance.clone();
        let defaults = self.model.applied_theme().style.colors;
        self.appearance_editor =
            Some(cx.new(|cx| AppearanceEditor::new(owner, &appearance, defaults, window, cx)));
        window.focus(&self.appearance_editor.as_ref().unwrap().read(cx).font_focus[0]);
        cx.notify();
    }

    pub(super) fn open_appearance(
        &mut self,
        _: &ShowAppearance,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_appearance(window, cx);
    }

    pub(super) fn finish_appearance(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Err(error) = self.change_appearance(self.model.appearance.clone(), window, cx) {
            self.editor_warning("外观设置未保存", &error, window, cx);
        }
        self.appearance_editor = None;
        self.focus_current(window, cx);
        cx.notify();
    }

    pub(super) fn change_appearance(
        &mut self,
        appearance: Appearance,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        self.show_appearance_trial(None, cx);
        if self.model.appearance == appearance
            && self
                .theme_store
                .as_ref()
                .is_none_or(|store| store.appearance == appearance)
        {
            return Ok(());
        }
        resume_one_core::layout::check_theme(
            &appearance.apply(&self.model.applied_theme().manifest),
        )?;
        let store = self.theme_store.as_mut().ok_or("本地外观设置不可用")?;
        let result = if store.appearance == appearance {
            Ok(())
        } else {
            store.save_appearance(appearance.clone())
        };
        if let Err(error) = result {
            if let Some(store) = &self.theme_store {
                self.model.set_appearance(store.appearance.clone());
            }
            crate::editor_ui::apply_editor_theme(&self.model.selected_style(), cx);
            cx.notify();
            return Err(error);
        }
        self.model.set_appearance(appearance);
        self.clear_inline();
        crate::editor_ui::apply_editor_theme(&self.model.selected_style(), cx);
        cx.notify();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{model::AppModel, theme_local::ThemeStore};
    use gpui::{Modifiers, TestAppContext};
    use gpui_component::WindowExt as _;
    use std::{cell::RefCell, rc::Rc};

    #[gpui::test]
    fn font_and_color_hover_preview_without_saving_and_click_confirms(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let root = tempfile::tempdir().unwrap();
        let settings = root.path().join("settings.json");
        let themes = root.path().join("themes");
        let mut store = ThemeStore::new(settings.clone(), themes.clone()).unwrap();
        store.save_appearance(Appearance::default()).unwrap();
        let saved = std::fs::read(&settings).unwrap();
        let theme_bytes = std::fs::read(themes.join("zhibai.theme.json")).unwrap();
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
        cx.update(|window, cx| owner.update(cx, |app, cx| app.show_appearance(window, cx)));
        let paper = cx.debug_bounds("resume-paper").unwrap();
        let panel = cx.debug_bounds("module-panel").unwrap();
        let source = cx.update(|_, cx| owner.read(cx).model.document.source.clone());
        let font = cx.debug_bounds("appearance-font-body").unwrap().center();
        cx.simulate_click(font, Modifiers::none());
        let serif = cx
            .debug_bounds("appearance-font-choice-2")
            .unwrap()
            .center();
        cx.simulate_mouse_move(serif, None, Modifiers::none());
        cx.update(|_, cx| {
            let app = owner.read(cx);
            assert_eq!(app.active_style().typography.body.family, "cjk-serif");
            assert_eq!(app.model.appearance, Appearance::default());
        });
        assert_eq!(std::fs::read(&settings).unwrap(), saved);
        cx.simulate_mouse_move(gpui::point(px(500.), px(30.)), None, Modifiers::none());
        cx.update(|_, cx| {
            assert_eq!(
                owner.read(cx).active_style().typography.body.family,
                "cjk-sans"
            )
        });
        cx.simulate_click(serif, Modifiers::none());
        let back = cx.debug_bounds("font-browser-back").unwrap().center();
        cx.simulate_click(back, Modifiers::none());
        cx.update(|_, cx| {
            assert_eq!(
                owner.read(cx).model.appearance.body_font,
                Some(FontChoice::Serif)
            )
        });
        let font_saved = std::fs::read(&settings).unwrap();
        let accent = cx.debug_bounds("appearance-color-accent").unwrap().center();
        cx.simulate_click(accent, Modifiers::none());
        let blue = cx
            .debug_bounds("appearance-color-preset-5")
            .unwrap()
            .center();
        cx.simulate_mouse_move(blue, None, Modifiers::none());
        cx.update(|_, cx| {
            let app = owner.read(cx);
            assert_eq!(
                app.model.display_appearance().color("accent"),
                Some("#587B9E")
            );
            let mut trial = app.model.appearance.clone();
            trial.set_color("accent", "#587B9E").unwrap();
            let expected = resume_one_core::theme_style::ThemeStyle::from_manifest(
                &trial.apply(&app.model.applied_theme().manifest),
            );
            assert_eq!(app.active_style().colors.accent, expected.colors.accent);
            assert!(app.model.appearance.color("accent").is_none());
            assert_eq!(app.model.appearance.body_font, Some(FontChoice::Serif));
        });
        assert_eq!(std::fs::read(&settings).unwrap(), font_saved);
        cx.update(|window, cx| {
            owner.update(cx, |app, cx| {
                app.toggle_source(&crate::editor_ui::ToggleSource, window, cx);
                let expected: gpui::Hsla = color(app.active_style().colors.accent).into();
                assert_eq!(gpui_component::Theme::global(cx).colors.accent, expected);
                assert!(app.appearance_editor.is_some());
                app.toggle_source(&crate::editor_ui::ToggleSource, window, cx);
            })
        });
        cx.simulate_mouse_move(gpui::point(px(500.), px(30.)), None, Modifiers::none());
        cx.update(|_, cx| {
            assert!(
                owner
                    .read(cx)
                    .model
                    .display_appearance()
                    .color("accent")
                    .is_none()
            )
        });
        cx.simulate_click(blue, Modifiers::none());
        cx.run_until_parked();
        cx.update(|_, cx| {
            let app = owner.read(cx);
            assert_eq!(app.model.appearance.color("accent"), Some("#587B9E"));
            assert_eq!(app.model.document.source, source);
            assert!(app.visual_undo.is_empty());
        });
        let reopened = ThemeStore::new(settings.clone(), themes.clone()).unwrap();
        assert_eq!(reopened.appearance.body_font, Some(FontChoice::Serif));
        assert_eq!(reopened.appearance.color("accent"), Some("#587B9E"));
        assert_eq!(
            std::fs::read(themes.join("zhibai.theme.json")).unwrap(),
            theme_bytes
        );
        assert_eq!(paper, cx.debug_bounds("resume-paper").unwrap());
        assert_eq!(panel, cx.debug_bounds("module-panel").unwrap());
        // Hover must also work when the preference file cannot be saved. A
        // failed click must restore the confirmed value, never persist a trial.
        std::fs::write(&settings, b"broken settings").unwrap();
        let red = cx
            .debug_bounds("appearance-color-preset-4")
            .unwrap()
            .center();
        cx.simulate_mouse_move(red, None, Modifiers::none());
        cx.update(|_, cx| {
            assert_eq!(
                owner.read(cx).model.display_appearance().color("accent"),
                Some("#B84A3C")
            )
        });
        cx.simulate_click(red, Modifiers::none());
        cx.update(|_, cx| {
            let app = owner.read(cx);
            assert_eq!(app.model.appearance.color("accent"), Some("#587B9E"));
            assert_eq!(app.model.display_appearance(), &app.model.appearance);
        });
        assert_eq!(std::fs::read(settings).unwrap(), b"broken settings");
    }

    #[gpui::test]
    fn formatting_shortcut_in_font_search_does_not_modify_the_resume(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        cx.update(crate::editor_ui::register);
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("resume.json");
        let original = br#"{"basics":{"name":"Ada"}}"#;
        std::fs::write(&path, original).unwrap();
        let child = Rc::new(RefCell::new(None));
        let output = child.clone();
        let (_, cx) = cx.add_window_view(|window, cx| {
            let owner = cx.new(|cx| ResumeOne::new(AppModel::load(Some(&path)).unwrap(), cx));
            *output.borrow_mut() = Some(owner.clone());
            gpui_component::Root::new(owner, window, cx)
        });
        let owner = child.borrow_mut().take().unwrap();
        cx.update(|window, cx| {
            owner.update(cx, |app, cx| {
                app.toggle_source(&crate::editor_ui::ToggleSource, window, cx);
                app.show_appearance(window, cx);
            })
        });
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        cx.simulate_keystrokes("ctrl-f");
        cx.run_until_parked();
        cx.update(|_, cx| {
            let editor = owner.read(cx).editor.as_ref().unwrap();
            assert_eq!(editor.buffer.source().as_bytes(), original);
            assert!(!editor.buffer.is_dirty());
        });
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }

    #[gpui::test]
    fn docked_inspector_is_live_and_keeps_document_and_theme_unchanged(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let root = tempfile::tempdir().unwrap();
        let settings = root.path().join("settings.json");
        let defaults = root.path().join("themes");
        let store = ThemeStore::new(settings.clone(), defaults.clone()).unwrap();
        let original_theme = std::fs::read(defaults.join("zhibai.theme.json")).unwrap();
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
        cx.update(|window, _| window.refresh());
        let right = cx.debug_bounds("module-panel").unwrap();
        let paper = cx.debug_bounds("resume-paper").unwrap();
        let source = cx.update(|_, cx| owner.read(cx).model.document.source.clone());
        let show = cx.debug_bounds("show-appearance").unwrap().center();
        cx.simulate_click(show, Modifiers::none());
        cx.update(|window, cx| assert!(!window.has_active_dialog(cx)));
        assert!(cx.debug_bounds("appearance-panel").is_some());
        assert_eq!(right, cx.debug_bounds("module-panel").unwrap());
        assert_eq!(paper, cx.debug_bounds("resume-paper").unwrap());
        let font = cx.debug_bounds("appearance-font-body").unwrap().center();
        cx.simulate_click(font, Modifiers::none());
        let serif = cx
            .debug_bounds("appearance-font-choice-2")
            .unwrap()
            .center();
        cx.simulate_click(serif, Modifiers::none());
        let back = cx.debug_bounds("font-browser-back").unwrap().center();
        cx.simulate_click(back, Modifiers::none());
        cx.run_until_parked();
        let editor = cx.update(|_, cx| owner.read(cx).appearance_editor.as_ref().unwrap().clone());
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.inputs[0].update(cx, |input, cx| input.set_value("#315A71", window, cx))
            })
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            let app = owner.read(cx);
            assert_eq!(app.model.appearance.body_font, Some(FontChoice::Serif));
            assert_eq!(app.model.appearance.color("accent"), Some("#315A71"));
            assert_eq!(app.model.document.source, source);
            assert!(
                app.editor
                    .as_ref()
                    .is_none_or(|editor| !editor.buffer.is_dirty())
            );
        });
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.edit_color(0, "unfinished", false, window, cx);
                assert!(editor.error.is_none());
                editor.change_font(true, Some(FontChoice::Sans), window, cx);
            })
        });
        cx.update(|_, cx| {
            assert_eq!(
                owner.read(cx).model.appearance.color("accent"),
                Some("#315A71")
            )
        });
        cx.update(|window, cx| editor.update(cx, |editor, cx| editor.reset_colors(window, cx)));
        cx.run_until_parked();
        let reopened = ThemeStore::new(settings, defaults.clone()).unwrap();
        assert!(reopened.appearance.color("accent").is_none());
        assert_eq!(reopened.appearance.body_font, Some(FontChoice::Serif));
        assert_eq!(reopened.appearance.heading_font, Some(FontChoice::Sans));
        assert_eq!(
            std::fs::read(defaults.join("zhibai.theme.json")).unwrap(),
            original_theme
        );
        assert_eq!(right, cx.debug_bounds("module-panel").unwrap());
        let finish = cx.debug_bounds("finish-appearance").unwrap().center();
        cx.simulate_click(finish, Modifiers::none());
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.update(|_, cx| assert!(owner.read(cx).appearance_editor.is_none()));
        // A retained Entity can keep its last debug bounds after unmounting.
        // The owner state and the newly rendered module list are authoritative.
        assert!(cx.debug_bounds("module-basics").is_some());
    }

    #[gpui::test]
    fn compact_inspector_docks_without_a_mask_and_keeps_color_focus_visible(
        cx: &mut TestAppContext,
    ) {
        cx.update(gpui_component::init);
        let child = Rc::new(RefCell::new(None));
        let output = child.clone();
        let (_, cx) = cx.add_window_view(|window, cx| {
            let owner = cx.new(|cx| ResumeOne::new(AppModel::load(None).unwrap(), cx));
            *output.borrow_mut() = Some(owner.clone());
            gpui_component::Root::new(owner, window, cx)
        });
        let owner = child.borrow_mut().take().unwrap();
        cx.simulate_resize(gpui::size(px(720.), px(480.)));
        cx.update(|window, cx| owner.update(cx, |app, cx| app.show_appearance(window, cx)));
        cx.update(|window, _| window.refresh());
        let right = cx.debug_bounds("module-panel").unwrap();
        let paper = cx.debug_bounds("resume-paper").unwrap();
        assert!(paper.right() <= right.left());
        assert!(right.right() <= px(720.));
        assert!(cx.debug_bounds("drawer-overlay").is_none());
        let editor = cx.update(|_, cx| owner.read(cx).appearance_editor.as_ref().unwrap().clone());
        cx.update(|window, cx| {
            window.focus(&editor.read(cx).palette.focus[6]);
        });
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        let field = cx.debug_bounds("appearance-color-seal").unwrap();
        let viewport = cx.debug_bounds("appearance-scroll").unwrap();
        assert!(field.top() >= viewport.top() && field.bottom() <= viewport.bottom());
        cx.update(|window, cx| {
            owner.update(cx, |app, cx| {
                app.toggle_source(&crate::editor_ui::ToggleSource, window, cx)
            })
        });
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(100));
        cx.update(|window, _| window.refresh());
        assert_eq!(right, cx.debug_bounds("module-panel").unwrap());
        assert!(cx.debug_bounds("appearance-panel").is_some());
    }

    #[gpui::test]
    fn searchable_font_list_supports_arrow_enter_escape_and_restores_focus(
        cx: &mut TestAppContext,
    ) {
        cx.update(gpui_component::init);
        cx.update(crate::inline_ui::register);
        let root = tempfile::tempdir().unwrap();
        let store = ThemeStore::new(
            root.path().join("settings.json"),
            root.path().join("themes"),
        )
        .unwrap();
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
        cx.update(|window, cx| owner.update(cx, |app, cx| app.show_appearance(window, cx)));
        let editor = cx.update(|_, cx| owner.read(cx).appearance_editor.as_ref().unwrap().clone());
        cx.update(|window, cx| assert!(editor.read(cx).font_focus[0].is_focused(window)));
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        assert!(cx.debug_bounds("appearance-font-browser").is_some());
        let browser = cx.debug_bounds("appearance-font-browser").unwrap();
        let first_row = cx.debug_bounds("appearance-font-choice-0").unwrap();
        assert_eq!(first_row.left(), browser.left());
        assert_eq!(first_row.right(), browser.right());
        cx.update(|window, cx| {
            editor
                .read(cx)
                .font_search
                .clone()
                .update(cx, |input, cx| input.set_value("Noto", window, cx));
        });
        cx.run_until_parked();
        cx.simulate_keystrokes("down enter");
        cx.run_until_parked();
        cx.update(|_, cx| {
            assert_eq!(
                owner.read(cx).model.appearance.body_font,
                Some(FontChoice::Serif)
            )
        });
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(editor.read(cx).font_target.is_none());
            assert!(editor.read(cx).font_focus[0].is_focused(window));
            assert!(owner.read(cx).appearance_editor.is_some());
            editor.update(cx, |editor, cx| {
                // Hold the resolver deterministically: fast changes in one
                // role must not discard the pending choice for the other.
                editor.font_resolving = true;
                editor.select_font(
                    false,
                    Some(FontChoice::Font("font:Arial".into())),
                    window,
                    cx,
                );
                editor.select_font(
                    true,
                    Some(FontChoice::Font("font:Menlo".into())),
                    window,
                    cx,
                );
                let latest = Some(FontChoice::Font("font:Helvetica".into()));
                editor.select_font(false, latest.clone(), window, cx);
                assert_eq!(editor.pending_font[0], Some(latest.clone()));
                assert_eq!(
                    editor.pending_font[1],
                    Some(Some(FontChoice::Font("font:Menlo".into())))
                );
                editor.select_font(true, None, window, cx);
                assert_eq!(editor.pending_font[0], Some(latest));
                assert!(editor.pending_font[1].is_none());
                editor.pending_font = [None, None];
                editor.font_resolving = false;
                editor.font_busy = false;
            });
        });
        cx.update(|window, cx| {
            editor.update(cx, |editor, _| editor.font_target = Some(false));
            window.focus(&owner.read(cx).focus);
        });
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        cx.update(|_, cx| {
            assert!(editor.read(cx).font_target.is_none());
            assert!(owner.read(cx).appearance_editor.is_some());
        });
        let right = cx.debug_bounds("module-panel").unwrap();
        let paper = cx.debug_bounds("resume-paper").unwrap();
        assert!(paper.right() <= right.left());
    }

    #[gpui::test]
    fn native_appearance_action_reaches_the_same_inspector_from_focused_json(
        cx: &mut TestAppContext,
    ) {
        cx.update(gpui_component::init);
        let child = Rc::new(RefCell::new(None));
        let output = child.clone();
        let (_, cx) = cx.add_window_view(|window, cx| {
            let owner = cx.new(|cx| ResumeOne::new(AppModel::load(None).unwrap(), cx));
            *output.borrow_mut() = Some(owner.clone());
            gpui_component::Root::new(owner, window, cx)
        });
        let owner = child.borrow_mut().take().unwrap();
        cx.update(|window, cx| {
            owner.update(cx, |app, cx| {
                app.toggle_source(&crate::editor_ui::ToggleSource, window, cx);
                app.sidebar_progress = 1.;
                app.view_blend = 1.;
                app.mode_transition = None;
                assert!(
                    app.editor
                        .as_ref()
                        .unwrap()
                        .input
                        .read(cx)
                        .focus_handle(cx)
                        .is_focused(window)
                );
            });
            window.refresh();
        });
        cx.dispatch_action(ShowAppearance);
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(owner.read(cx).appearance_editor.is_some());
            assert!(!window.has_active_dialog(cx));
        });
        let right = cx.debug_bounds("module-panel").unwrap();
        let source = cx.debug_bounds("resume-json-editor").unwrap();
        assert_eq!(source.right(), right.left());
        cx.update(|window, cx| {
            owner.update(cx, |app, cx| {
                app.finish_appearance(window, cx);
                app.theme_manager_open = true;
            });
        });
        cx.dispatch_action(ShowAppearance);
        cx.run_until_parked();
        cx.update(|_, cx| assert!(owner.read(cx).appearance_editor.is_none()));
    }

    #[gpui::test]
    fn inspector_never_covers_document_during_resize_and_mode_handoff(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let child = Rc::new(RefCell::new(None));
        let output = child.clone();
        let (_, cx) = cx.add_window_view(|window, cx| {
            let owner = cx.new(|cx| ResumeOne::new(AppModel::load(None).unwrap(), cx));
            *output.borrow_mut() = Some(owner.clone());
            gpui_component::Root::new(owner, window, cx)
        });
        let owner = child.borrow_mut().take().unwrap();
        cx.update(|window, cx| {
            owner.update(cx, |app, cx| {
                assert!(app.ensure_editor(window, cx));
                app.show_appearance(window, cx);
            })
        });
        for (width, height) in [
            (1440., 900.),
            (1280., 820.),
            (1024., 640.),
            (900., 600.),
            (720., 480.),
        ] {
            cx.simulate_resize(gpui::size(px(width), px(height)));
            cx.update(|window, _| window.refresh());
            let right = cx.debug_bounds("module-panel").unwrap();
            assert_eq!(right.right(), px(width));
            assert_eq!(right.size.width, px(chrome::MODULE_WIDTH));
            for source_mode in [true, false] {
                for step in 0..=20 {
                    let progress = if source_mode { step } else { 20 - step } as f32 / 20.;
                    cx.update(|window, cx| {
                        owner.update(cx, |app, cx| {
                            app.source_mode = source_mode;
                            app.sidebar_progress = progress;
                            app.view_blend = progress;
                            app.mode_transition = None;
                            cx.notify();
                        });
                        window.refresh();
                    });
                    assert_eq!(right, cx.debug_bounds("module-panel").unwrap());
                    let inspector = cx.debug_bounds("appearance-panel").unwrap();
                    assert!(inspector.left() >= right.left());
                    assert!(inspector.right() <= right.right());
                    if progress < 1. || !source_mode {
                        let preview = cx.debug_bounds("resume-preview").unwrap();
                        let paper = cx.debug_bounds("resume-paper").unwrap();
                        assert_eq!(preview.right(), right.left());
                        assert!(paper.left() >= preview.left());
                        assert!(paper.right() <= right.left());
                    }
                    if progress > 0. || source_mode {
                        let source = cx.debug_bounds("resume-json-editor").unwrap();
                        assert_eq!(source.right(), right.left());
                    }
                    cx.update(|window, cx| assert!(!window.has_active_dialog(cx)));
                    assert!(cx.debug_bounds("drawer-overlay").is_none());
                }
            }
        }
    }

    #[gpui::test]
    fn every_theme_keeps_live_inspector_controls_inside_the_right_rail(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let child = Rc::new(RefCell::new(None));
        let output = child.clone();
        let (_, cx) = cx.add_window_view(|window, cx| {
            let owner = cx.new(|cx| ResumeOne::new(AppModel::load(None).unwrap(), cx));
            *output.borrow_mut() = Some(owner.clone());
            gpui_component::Root::new(owner, window, cx)
        });
        let owner = child.borrow_mut().take().unwrap();
        cx.simulate_resize(gpui::size(px(1280.), px(820.)));
        cx.update(|window, cx| owner.update(cx, |app, cx| app.show_appearance(window, cx)));
        let original_source = cx.update(|_, cx| owner.read(cx).model.document.source.clone());
        for index in 0..crate::model::BUILTIN_THEMES.len() {
            cx.update(|window, cx| {
                owner.update(cx, |app, cx| {
                    app.model.select_theme(index);
                    let appearance = Appearance::from_json(&serde_json::json!({
                        "formatVersion": 1,
                        "bodyFont": if index % 2 == 0 { "cjk-sans" } else { "cjk-serif" },
                        "headingFont": if index % 2 == 0 { "cjk-serif" } else { "cjk-sans" },
                        "colors": if index % 2 == 0 {
                            serde_json::json!({"paper":"#FFFEFA","ink":"#26372F","accent":"#52756B"})
                        } else {
                            serde_json::json!({"paper":"#1E2825","ink":"#F5F0E5","accent":"#A5C4B2"})
                        }
                    })).unwrap();
                    // Rendering coverage only: persistence and input dispatch are
                    // independently exercised by the live-inspector test above.
                    app.model.set_appearance(appearance);
                    cx.notify();
                });
                window.refresh();
            });
            let right = cx.debug_bounds("module-panel").unwrap();
            let paper = cx.debug_bounds("resume-paper").unwrap();
            assert!(paper.right() <= right.left());
            for selector in [
                "appearance-font-body",
                "appearance-font-heading",
                "appearance-color-accent",
                "appearance-color-paper",
                "appearance-color-canvas",
                "appearance-color-ink",
                "appearance-color-muted",
                "appearance-color-border",
                "appearance-color-seal",
                "finish-appearance",
            ] {
                let control = cx.debug_bounds(selector).unwrap();
                assert!(control.left() >= right.left(), "{index}: {selector}");
                assert!(control.right() <= right.right(), "{index}: {selector}");
                assert!(control.size.width > px(26.), "{index}: {selector}");
            }
            cx.update(|_, cx| assert_eq!(owner.read(cx).model.document.source, original_source));
        }
    }
}
