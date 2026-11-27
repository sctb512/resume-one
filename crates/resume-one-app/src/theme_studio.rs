//! Edit a validated declarative theme while the paper remains visible.
use std::path::PathBuf;

use gpui::{
    Animation, AnimationExt, Context, Entity, IntoElement, PromptLevel, Render, ScrollHandle,
    Subscription, WeakEntity, Window, div, prelude::*, px,
};
use gpui_component::{
    Disableable, Selectable, Sizable,
    button::{Button, ButtonCustomVariant, ButtonVariants},
    input::{Input, InputEvent, InputPalette, InputState},
};
use resume_one_core::{ThemeManifest, theme_style::ThemeStyle};
use resume_one_themes::{GithubClient, GithubSource, ThemeCache};
use serde_json::{Value, json};

use crate::{ResumeOne, SECTION_LABELS, chrome, color, i18n::tr, label, model::BuiltinTheme};

const SIDES: [(&str, &str); 4] = [
    ("top", "上"),
    ("bottom", "下"),
    ("left", "左"),
    ("right", "右"),
];
const ELEMENTS: [(&str, &str); 5] = [
    ("heading", "模块标题"),
    ("entryHeader", "条目标题与日期"),
    ("description", "描述"),
    ("highlights", "要点"),
    ("link", "链接"),
];
const BASICS_ELEMENTS: [(&str, &str); 4] = [
    ("name", "姓名"),
    ("label", "职业"),
    ("contacts", "联系方式"),
    ("summary", "个人简介"),
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Paper,
    Modules,
}

pub(super) struct ThemeEditor {
    owner: WeakEntity<ResumeOne>,
    base: ThemeManifest,
    value: Value,
    initial: Value,
    name: Entity<InputState>,
    text: Entity<InputState>,
    margins: [Entity<InputState>; 4],
    _subscriptions: Vec<Subscription>,
    tab: Tab,
    module: Option<&'static str>,
    element: Option<usize>,
    header_line: Option<usize>,
    editing_title: bool,
    history: Vec<Value>,
    redo: Vec<Value>,
    message: Option<String>,
    from_original: bool,
    default_name: String,
    text_initial: String,
    text_language: resume_one_core::render::Language,
    scroll: [ScrollHandle; 2],
    module_scroll: std::collections::BTreeMap<&'static str, ScrollHandle>,
    motion: usize,
}

#[derive(Clone)]
struct DragModule {
    editor: gpui::EntityId,
    id: &'static str,
    palette: chrome::Palette,
}

impl Render for DragModule {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_3()
            .py_2()
            .rounded_md()
            .bg(color(self.palette.panel))
            .text_color(color(self.palette.ink))
            .shadow_sm()
            .child(label(self.id))
    }
}

fn array(value: &mut Value) -> &mut Vec<Value> {
    if value.is_null() {
        *value = json!([]);
    }
    value.as_array_mut().expect("validated array")
}

fn set_hidden(value: &mut Value, id: &str, hidden: bool) {
    let ids = array(value);
    ids.retain(|value| value != id);
    if hidden {
        ids.push(json!(id));
    }
}

fn copy_name(name: &str) -> String {
    format!(
        "{} {}",
        name.chars().take(65).collect::<String>(),
        tr("副本")
    )
}

fn initial_value(
    theme: &ThemeManifest,
    document_order: impl Iterator<Item = &'static str>,
) -> Value {
    let mut value = theme.normalized.clone();
    if value["composition"].is_null() {
        value["composition"] = json!({"preset":"single-column"});
    }
    let order = array(&mut value["composition"]["moduleOrder"]);
    for id in document_order.chain(SECTION_LABELS.iter().map(|(id, _)| *id)) {
        if id != "basics" && !order.iter().any(|value| value == id) {
            order.push(json!(id));
        }
    }
    let margins = ThemeStyle::from_manifest(theme).page.margins;
    value["page"]["marginsMm"] = json!({
        "top": margins.top, "bottom": margins.bottom, "left": margins.left, "right": margins.right,
    });
    value
}

impl ThemeEditor {
    fn new(
        owner: WeakEntity<ResumeOne>,
        theme: &BuiltinTheme,
        value: Value,
        from_original: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let name = copy_name(theme.manifest.localized_name(&crate::i18n::theme_locale()));
        let default_name = name.clone();
        let name = cx.new(|cx| InputState::new(window, cx).default_value(name));
        let text = cx.new(|cx| InputState::new(window, cx).placeholder(tr("纯文本")));
        let margins = SIDES.map(|(side, _)| {
            cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(value["page"]["marginsMm"][side].to_string())
            })
        });
        let mut subscriptions: Vec<_> = margins
            .iter()
            .map(|input| {
                cx.subscribe_in(input, window, |this, _, event, _, cx| {
                    if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                        this.apply_margins(cx);
                    }
                    cx.notify();
                })
            })
            .collect();
        for input in [&name, &text] {
            subscriptions
                .push(cx.subscribe_in(input, window, |_, _, _: &InputEvent, _, cx| cx.notify()));
        }
        subscriptions.push(cx.subscribe_in(&text, window, |this, _, event, _, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.finish_text(cx);
            }
        }));
        Self {
            owner,
            base: theme.manifest.clone(),
            initial: value.clone(),
            value,
            name,
            text,
            margins,
            _subscriptions: subscriptions,
            tab: Tab::Paper,
            module: None,
            element: None,
            header_line: None,
            editing_title: false,
            history: Vec::new(),
            redo: Vec::new(),
            message: None,
            from_original,
            default_name,
            text_initial: String::new(),
            text_language: crate::i18n::language(),
            scroll: std::array::from_fn(|_| ScrollHandle::new()),
            module_scroll: SECTION_LABELS
                .iter()
                .map(|(id, _)| (*id, ScrollHandle::new()))
                .collect(),
            motion: 0,
        }
    }

    fn publish_draft(&self, theme: ThemeManifest, cx: &mut Context<Self>) {
        if let Some(owner) = self.owner.upgrade() {
            owner.update(cx, |app, cx| app.render_theme_draft(theme, cx));
        }
    }

    fn edit(&mut self, change: impl FnOnce(&mut Value), cx: &mut Context<Self>) {
        let mut next = self.value.clone();
        change(&mut next);
        if next == self.value {
            self.message = None;
            cx.notify();
            return;
        }
        match self.base.edited_copy(&next) {
            Ok(theme) => {
                if self.history.len() == 60 {
                    self.history.remove(0);
                }
                self.history.push(std::mem::replace(&mut self.value, next));
                self.redo.clear();
                self.message = None;
                self.publish_draft(theme, cx);
            }
            Err(error) => self.message = Some(error),
        }
        cx.notify();
    }

    pub(super) fn restore(&mut self, undo: bool, window: &mut Window, cx: &mut Context<Self>) {
        let previous = if undo {
            self.history.pop()
        } else {
            self.redo.pop()
        };
        let Some(previous) = previous else { return };
        let Ok(theme) = self.base.edited_copy(&previous) else {
            return;
        };
        let current = std::mem::replace(&mut self.value, previous);
        if undo {
            self.redo.push(current);
        } else {
            self.history.push(current);
        }
        self.clear_text_edit();
        self.message = None;
        self.refresh_margins(window, cx);
        self.publish_draft(theme, cx);
        cx.notify();
    }

    fn clear_text_edit(&mut self) {
        self.element = None;
        self.editing_title = false;
        self.header_line = None;
    }

    fn has_changes(&self, cx: &gpui::App) -> bool {
        self.value != self.initial
            || self.name.read(cx).value().as_ref() != self.default_name
            || self.name.read(cx).is_composing()
            || ((self.editing_title || self.element.is_some())
                && (self.text.read(cx).value().as_ref() != self.text_initial
                    || self.text.read(cx).is_composing()))
            || self.parsed_margins(cx).is_none_or(|margins| {
                SIDES.iter().any(|(side, _)| {
                    margins[side].as_f64() != self.value["page"]["marginsMm"][side].as_f64()
                })
            })
    }

    fn refresh_margins(&self, window: &mut Window, cx: &mut Context<Self>) {
        for ((side, _), input) in SIDES.iter().zip(&self.margins) {
            input.update(cx, |input, cx| {
                input.set_value(
                    self.value["page"]["marginsMm"][side].to_string(),
                    window,
                    cx,
                )
            });
        }
    }

    fn parsed_margins(&self, cx: &gpui::App) -> Option<Value> {
        let mut values = serde_json::Map::new();
        for ((side, _), input) in SIDES.iter().zip(&self.margins) {
            let value = input
                .read(cx)
                .value()
                .trim()
                .replace(',', ".")
                .parse::<f64>()
                .ok()?;
            if !(3.0..=35.0).contains(&value) {
                return None;
            }
            values.insert((*side).into(), json!((value * 10.0).round() / 10.0));
        }
        Some(Value::Object(values))
    }

    fn apply_margins(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(margins) = self.parsed_margins(cx) else {
            self.message = Some(tr("页边距须在 3–35 mm 之间").into());
            cx.notify();
            return false;
        };
        if SIDES.iter().any(|(side, _)| {
            margins[side].as_f64() != self.value["page"]["marginsMm"][side].as_f64()
        }) {
            self.edit(
                |value| {
                    value["page"]["marginsMm"] = margins;
                    value["page"]["firstLinePosition"] = json!("line-box");
                },
                cx,
            );
        }
        true
    }

    fn open_text(&mut self, value: String, window: &mut Window, cx: &mut Context<Self>) {
        self.text_initial = value.clone();
        self.text_language = self
            .owner
            .upgrade()
            .map(|owner| owner.read(cx).model.presentation_language)
            .unwrap_or_else(crate::i18n::language);
        self.text.update(cx, |text, cx| {
            text.set_value(value, window, cx);
            text.focus(window, cx);
        });
        cx.notify();
    }

    fn finish_text(&mut self, cx: &mut Context<Self>) -> bool {
        if !(self.editing_title || self.element.is_some()) {
            return true;
        }
        if self.text.read(cx).is_composing() {
            self.message = Some(tr("请先完成文字输入").into());
            cx.notify();
            return false;
        }
        if self.text.read(cx).value().as_ref() == self.text_initial {
            self.clear_text_edit();
            return true;
        }
        if let Some(id) = self.module {
            self.save_text(id, cx);
        }
        !self.editing_title && self.element.is_none()
    }

    fn navigate(&mut self, tab: Tab, module: Option<&'static str>, cx: &mut Context<Self>) {
        if !self.finish_text(cx) || (self.tab == Tab::Paper && !self.apply_margins(cx)) {
            return;
        }
        if self.tab != tab || self.module != module {
            self.motion = self.motion.wrapping_add(1);
        }
        self.tab = tab;
        self.module = module;
        self.clear_text_edit();
        cx.notify();
    }

    fn move_module(&mut self, id: &str, target: &str, sidebar: bool, cx: &mut Context<Self>) {
        if id == target || !self.finish_text(cx) {
            return;
        }
        self.edit(
            |value| {
                let order = array(&mut value["composition"]["moduleOrder"]);
                let (Some(from), Some(to)) = (
                    order.iter().position(|value| value == id),
                    order.iter().position(|value| value == target),
                ) else {
                    return;
                };
                let module = order.remove(from);
                order.insert(to, module);
                let preset = value["composition"]["preset"].as_str().unwrap_or_default();
                if matches!(preset, "sidebar-left" | "sidebar-right") {
                    value["composition"]["placements"][id] =
                        json!(if sidebar { "sidebar" } else { "main" });
                }
            },
            cx,
        );
    }

    fn save_text(&mut self, id: &'static str, cx: &mut Context<Self>) {
        let text = self.text.read(cx).value().trim().to_owned();
        let title = self.editing_title;
        let index = self.element;
        let locale = if self.text_language == resume_one_core::render::Language::En {
            "en"
        } else {
            "zh-cn"
        };
        self.edit(
            |value| {
                if title {
                    value["localizations"][locale]["sections"][id] = json!(text);
                } else {
                    let parts = array(&mut value["sections"][id]["elements"]);
                    let item = json!({"kind":"text", "text":text});
                    if let Some(index) = index.filter(|index| *index < parts.len()) {
                        parts[index] = item;
                    } else {
                        parts.push(item);
                    }
                }
            },
            cx,
        );
        if self.message.is_none() {
            self.clear_text_edit();
        }
    }

    fn save_copy(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.finish_text(cx) {
            return;
        }
        if !self.apply_margins(cx) {
            return;
        }
        let Some(owner) = self.owner.upgrade() else {
            return;
        };
        let name = self.name.read(cx).value().to_string();
        let result = owner.update(cx, |app, cx| {
            let (themes, path) = app
                .theme_store
                .as_mut()
                .ok_or_else(|| tr("主题目录不可用").to_owned())?
                .save_personal_theme(&self.base, &self.value, &name)?;
            app.install_personal_theme(themes, path, window, cx)
        });
        if let Err(error) = result {
            self.message = Some(error);
            cx.notify();
        }
    }

    fn paper(
        &self,
        palette: chrome::Palette,
        pages: Option<usize>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let fit = self.value["page"]["fitToOnePage"] == true;
        let mut margins = div().flex().flex_col().gap_2();
        for row in [0..2, 2..4] {
            let mut fields = div().flex().gap_2();
            for index in row {
                fields = fields.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .text_xs()
                                .text_color(color(palette.muted))
                                .child(tr(SIDES[index].1))
                                .child(
                                    editor_button(("reset-theme-margin", index), palette, cx)
                                        .xsmall()
                                        .label("↺")
                                        .tooltip(tr("恢复此项默认值"))
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            let margins =
                                                ThemeStyle::from_manifest(&this.base).page.margins;
                                            let defaults = [
                                                margins.top,
                                                margins.bottom,
                                                margins.left,
                                                margins.right,
                                            ];
                                            this.margins[index].update(cx, |input, cx| {
                                                input.set_value(
                                                    defaults[index].to_string(),
                                                    window,
                                                    cx,
                                                );
                                            });
                                            this.apply_margins(cx);
                                        })),
                                ),
                        )
                        .child(edit_input(&self.margins[index], palette)),
                );
            }
            margins = margins.child(fields);
        }
        div().w_full().min_w_0().flex().flex_col().gap_3()
            .child(div().flex().justify_between().items_center().gap_1()
                .child(div().font_weight(gpui::FontWeight::SEMIBOLD).child(tr("页边距 · mm")))
                .child(editor_button("theme-margin-compact", palette, cx).small().label(tr("紧凑"))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.edit(|v| {
                            v["page"]["marginsMm"] = json!({"top":10, "bottom":10, "left":11, "right":11});
                            v["page"]["firstLinePosition"] = json!("line-box");
                        }, cx);
                        this.refresh_margins(window, cx);
                    }))))
            .child(margins)
            .child(div().flex().justify_between()
                .child(editor_button("theme-margin-reset", palette, cx).small().label(tr("恢复默认"))
                    .on_click(cx.listener(|this, _, window, cx| {
                        let margins = ThemeStyle::from_manifest(&this.base).page.margins;
                        let position = this.base.normalized["page"]["firstLinePosition"].clone();
                        this.edit(|v| {
                            v["page"]["marginsMm"] = json!({"top":margins.top,"bottom":margins.bottom,"left":margins.left,"right":margins.right});
                            if position.is_null() {
                                v["page"].as_object_mut().expect("page settings").remove("firstLinePosition");
                            } else {
                                v["page"]["firstLinePosition"] = position;
                            }
                        }, cx);
                        this.refresh_margins(window, cx);
                    })))
                .child(outlined_editor_button("theme-margin-apply", palette, cx)
                    .debug_selector(|| "theme-margin-apply".into()).small().label(tr("应用"))
                    .on_click(cx.listener(|this, _, _, cx| { this.apply_margins(cx); }))))
            .child(div().mt_3().pt_3().border_t_1().border_color(color(palette.line)).flex().flex_col().gap_2()
                .child(outlined_editor_button("theme-fit", palette, cx).small()
                    .label(if fit { tr("✓ 适配一页") } else { tr("适配一页") })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this.apply_margins(cx) {
                            let original = this.initial["page"]["fitToOnePage"].clone();
                            this.edit(|v| {
                                if fit && original.is_null() {
                                    v["page"].as_object_mut().unwrap().remove("fitToOnePage");
                                } else {
                                    v["page"]["fitToOnePage"] = json!(!fit);
                                }
                            }, cx);
                        }
                    })))
                .when_some(pages, |view, count| view.child(div().text_xs().text_color(color(palette.muted)).child(
                    if fit && count > 1 { tr("内容较多，已保留主题分页").into() }
                    else if fit && count == 1 { tr("已排成一页").into() }
                    else if crate::i18n::is_english() { format!("{count} {}", if count == 1 {"page"} else {"pages"}) }
                    else { format!("共 {count} 页") }
                ))))
            .when(fit, |view| {
                let explanation = self.owner.upgrade()
                    .map(|owner| owner.read(cx).theme_draft_fit_description())
                    .unwrap_or_default();
                view.child(div().text_xs().whitespace_normal().text_color(color(palette.muted)).child(explanation))
                    .child(editor_button("theme-fit-restore", palette, cx).small().label(tr("恢复主题排版"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            let original = this.initial["page"]["fitToOnePage"].clone();
                            this.edit(|value| {
                                if original.is_null() {
                                    value["page"].as_object_mut().unwrap().remove("fitToOnePage");
                                } else { value["page"]["fitToOnePage"] = json!(false); }
                            }, cx);
                        })))
            })
    }

    fn modules(&self, palette: chrome::Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let order = self.value["composition"]["moduleOrder"]
            .as_array()
            .expect("draft module order");
        let style = self
            .owner
            .upgrade()
            .and_then(|owner| {
                owner
                    .read(cx)
                    .theme_draft
                    .as_ref()
                    .map(|theme| theme.style.clone())
            })
            .unwrap_or_else(|| ThemeStyle::from_manifest(&self.base));
        let mut rows = div().w_full().min_w_0().flex().flex_col().gap_1().child(
            outlined_editor_button("theme-module-basics", palette, cx)
                .debug_selector(|| "theme-module-basics".into())
                .small()
                .label(label("basics"))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.navigate(Tab::Modules, Some("basics"), cx);
                })),
        );
        let mut entries: Vec<_> = order.iter().enumerate().collect();
        let mut lane = None;
        entries.sort_by_key(|(_, module)| style.in_sidebar(module.as_str().unwrap()));
        for (index, module) in entries {
            let id = SECTION_LABELS
                .iter()
                .map(|(id, _)| *id)
                .find(|id| module == *id)
                .expect("standard module");
            let sidebar = style.in_sidebar(id);
            if style.has_sidebar() && lane != Some(sidebar) {
                lane = Some(sidebar);
                rows = rows.child(
                    div()
                        .px_1()
                        .pt_2()
                        .text_xs()
                        .text_color(color(palette.muted))
                        .child(if sidebar { tr("侧栏") } else { tr("主栏") }),
                );
            }
            let same_lane = |i: usize| style.in_sidebar(order[i].as_str().unwrap()) == sidebar;
            let adjacent = [
                (0..index).rev().find(|&i| same_lane(i)),
                (index + 1..order.len()).find(|&i| same_lane(i)),
            ];
            let hidden = self.value["composition"]["hiddenModules"]
                .as_array()
                .is_some_and(|values| values.contains(module));
            let mut row = div()
                .id(("theme-module-row", index))
                .debug_selector(move || format!("theme-module-target-{id}"))
                .min_w_0()
                .w_full()
                .flex()
                .flex_col()
                .gap_1()
                .py_2()
                .border_b_1()
                .border_color(color(palette.line))
                .drag_over::<DragModule>(move |row, _, _, _| row.bg(color(editor_surface(palette))))
                .on_drop(cx.listener(move |this, drag: &DragModule, _, cx| {
                    if drag.editor == cx.entity_id() {
                        this.move_module(drag.id, id, sidebar, cx);
                    }
                }))
                .child(
                    div()
                        .w_full()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(
                            div()
                                .id(("theme-module-grip", index))
                                .debug_selector(move || format!("theme-module-grip-{id}"))
                                .px_1()
                                .flex_none()
                                .cursor_grab()
                                .text_color(color(palette.muted))
                                .child("⠿")
                                .on_drag(
                                    DragModule {
                                        editor: cx.entity_id(),
                                        id,
                                        palette,
                                    },
                                    |drag, _, _, cx| {
                                        cx.stop_propagation();
                                        cx.new(|_| drag.clone())
                                    },
                                ),
                        )
                        .child(
                            div().flex_1().min_w_0().child(
                                editor_button(
                                    gpui::SharedString::from(format!("theme-module-{id}")),
                                    palette,
                                    cx,
                                )
                                .debug_selector(move || format!("theme-module-{id}"))
                                .small()
                                .w_full()
                                .px_1()
                                .justify_start()
                                .tooltip(label(id))
                                .child(
                                    div()
                                        .min_w_0()
                                        .truncate()
                                        .when(hidden, |view| view.text_color(color(palette.muted)))
                                        .child(label(id)),
                                )
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.navigate(Tab::Modules, Some(id), cx);
                                    },
                                )),
                            ),
                        ),
                );
            let mut actions = div().w_full().flex().items_center().justify_end().gap_1();
            for (offset, target) in adjacent.into_iter().enumerate() {
                actions = actions.child(
                    editor_button(
                        gpui::SharedString::from(format!("theme-module-move-{id}-{offset}")),
                        palette,
                        cx,
                    )
                    .xsmall()
                    .w(px(24.))
                    .debug_selector(move || format!("theme-module-move-{id}-{offset}"))
                    .label(if offset == 0 { "↑" } else { "↓" })
                    .disabled(target.is_none())
                    .tooltip(format!(
                        "{} · {}",
                        label(id),
                        if offset == 0 {
                            tr("上移模块")
                        } else {
                            tr("下移模块")
                        }
                    ))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(target) = target {
                            this.edit(
                                |v| array(&mut v["composition"]["moduleOrder"]).swap(index, target),
                                cx,
                            );
                        }
                    })),
                );
            }
            actions = actions.child(
                editor_button(
                    gpui::SharedString::from(format!("theme-module-toggle-{id}")),
                    palette,
                    cx,
                )
                .xsmall()
                .debug_selector(move || format!("theme-module-toggle-{id}"))
                .label(if hidden {
                    tr("显示模块")
                } else {
                    tr("隐藏模块")
                })
                .tooltip(if hidden {
                    tr("显示模块")
                } else {
                    tr("隐藏模块")
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.edit(
                        |v| set_hidden(&mut v["composition"]["hiddenModules"], id, !hidden),
                        cx,
                    )
                })),
            );
            row = row.child(actions);
            rows = rows.child(row);
        }
        rows
    }

    fn module_detail(
        &self,
        id: &'static str,
        palette: chrome::Palette,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let section = &self.value["sections"][id];
        let mut body = div().w_full().min_w_0().flex().flex_col().gap_3().child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    editor_button("theme-module-back", palette, cx)
                        .debug_selector(|| "theme-module-back".into())
                        .small()
                        .label("‹")
                        .tooltip(tr("返回模块"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.navigate(Tab::Modules, None, cx);
                        })),
                )
                .child(
                    div()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(label(id)),
                ),
        );
        if id == "basics" {
            for (key, title) in BASICS_ELEMENTS {
                let hidden = self.value["composition"]["hiddenBasics"]
                    .as_array()
                    .is_some_and(|values| values.iter().any(|value| value == key));
                body = body.child(self.visibility_row(
                    key,
                    title,
                    hidden,
                    palette,
                    move |v| set_hidden(&mut v["composition"]["hiddenBasics"], key, !hidden),
                    cx,
                ));
            }
            return body.into_any_element();
        }
        if ThemeStyle::from_manifest(&self.base).has_sidebar() {
            let placement = &self.value["composition"]["placements"];
            let sidebar = placement[id] == "sidebar";
            let only_sidebar_module = sidebar
                && placement.as_object().is_some_and(|values| {
                    values.values().filter(|value| *value == "sidebar").count() == 1
                });
            let has_sidebar_header = ["headerRegion", "summaryRegion", "contactRegion"]
                .iter()
                .any(|key| self.value["composition"][key] == "sidebar");
            body = body.child(
                outlined_editor_button("theme-module-placement", palette, cx)
                    .small()
                    .label(if sidebar {
                        tr("移到主栏")
                    } else {
                        tr("移到侧栏")
                    })
                    .disabled(only_sidebar_module && !has_sidebar_header)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.edit(
                            |v| {
                                v["composition"]["placements"][id] =
                                    json!(if sidebar { "main" } else { "sidebar" })
                            },
                            cx,
                        )
                    })),
            );
        }
        for (key, title) in ELEMENTS {
            let hidden = section["hiddenElements"]
                .as_array()
                .is_some_and(|v| v.iter().any(|e| e == key));
            body = body.child(self.visibility_row(
                key,
                title,
                hidden,
                palette,
                move |v| set_hidden(&mut v["sections"][id]["hiddenElements"], key, !hidden),
                cx,
            ));
        }
        if !matches!(id, "skills" | "languages" | "interests") {
            body = body.child(self.entry_fields(id, section, palette, cx));
        }
        let extras = section["elements"]
            .as_array()
            .map_or(&[][..], Vec::as_slice);
        for (index, element) in extras.iter().enumerate() {
            let text = element["text"].as_str().map(str::to_owned);
            let display = text.as_deref().unwrap_or(tr("分隔线")).to_owned();
            body = body.child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .rounded_md()
                    .bg(color(editor_surface(palette)))
                    .pl_2()
                    .child(div().flex_1().min_w_0().truncate().text_xs().child(display))
                    .when_some(text, |row, text| {
                        row.child(
                            editor_button(
                                gpui::SharedString::from(format!("theme-extra-edit-{index}")),
                                palette,
                                cx,
                            )
                            .xsmall()
                            .label(tr("编辑"))
                            .on_click(cx.listener(
                                move |this, _, window, cx| {
                                    if !this.finish_text(cx) {
                                        return;
                                    }
                                    this.clear_text_edit();
                                    this.element = Some(index);
                                    this.open_text(text.clone(), window, cx);
                                },
                            )),
                        )
                    })
                    .child(
                        editor_button(
                            gpui::SharedString::from(format!("theme-extra-remove-{index}")),
                            palette,
                            cx,
                        )
                        .xsmall()
                        .label("−")
                        .tooltip(tr("删除元素"))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if !this.finish_text(cx) {
                                return;
                            }
                            this.clear_text_edit();
                            this.edit(
                                |v| {
                                    array(&mut v["sections"][id]["elements"]).remove(index);
                                },
                                cx,
                            )
                        })),
                    ),
            );
        }
        let editing = self.editing_title || self.element.is_some();
        body.child(
            div()
                .mt_1()
                .pt_2()
                .border_t_1()
                .border_color(color(palette.line))
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    outlined_editor_button("theme-edit-title", palette, cx)
                        .debug_selector(|| "theme-edit-title".into())
                        .small()
                        .label(tr("编辑模块标题"))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            if !this.finish_text(cx) {
                                return;
                            }
                            this.clear_text_edit();
                            this.editing_title = true;
                            let language = this
                                .owner
                                .upgrade()
                                .map(|owner| owner.read(cx).model.presentation_language)
                                .unwrap_or_else(crate::i18n::language);
                            let title = this
                                .base
                                .edited_copy(&this.value)
                                .map(|theme| {
                                    ThemeStyle::from_manifest(&theme)
                                        .section_title(id, language)
                                        .to_owned()
                                })
                                .unwrap_or_else(|_| label(id).into());
                            this.open_text(title, window, cx);
                        })),
                )
                .child(
                    div()
                        .flex()
                        .gap_1()
                        .child(
                            editor_button("theme-new-text", palette, cx)
                                .debug_selector(|| "theme-new-text".into())
                                .small()
                                .label(tr("+ 文字"))
                                .disabled(extras.len() == 8)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    if !this.finish_text(cx) {
                                        return;
                                    }
                                    this.clear_text_edit();
                                    this.element = Some(
                                        this.value["sections"][id]["elements"]
                                            .as_array()
                                            .map_or(0, Vec::len),
                                    );
                                    this.open_text(String::new(), window, cx);
                                })),
                        )
                        .child(
                            editor_button("theme-new-divider", palette, cx)
                                .debug_selector(|| "theme-new-divider".into())
                                .small()
                                .label(tr("+ 分隔线"))
                                .disabled(extras.len() == 8)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if !this.finish_text(cx) {
                                        return;
                                    }
                                    this.clear_text_edit();
                                    this.edit(
                                        |v| {
                                            array(&mut v["sections"][id]["elements"])
                                                .push(json!({"kind":"divider"}))
                                        },
                                        cx,
                                    )
                                })),
                        ),
                )
                .when(editing, |view| {
                    view.child(edit_input(&self.text, palette)).child(
                        div()
                            .flex()
                            .justify_end()
                            .gap_1()
                            .child(
                                editor_button("theme-text-cancel", palette, cx)
                                    .small()
                                    .label(tr("取消"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.clear_text_edit();
                                        cx.notify();
                                    })),
                            )
                            .child(
                                editor_button("theme-text-apply", palette, cx)
                                    .small()
                                    .custom(
                                        editor_colors(palette, cx)
                                            .border(color(palette.line).into()),
                                    )
                                    .label(tr("应用"))
                                    .on_click(
                                        cx.listener(move |this, _, _, cx| this.save_text(id, cx)),
                                    ),
                            ),
                    )
                }),
        )
        .into_any_element()
    }

    fn visibility_row(
        &self,
        key: &'static str,
        title: &'static str,
        hidden: bool,
        palette: chrome::Palette,
        change: impl Fn(&mut Value) + 'static,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .flex()
            .justify_between()
            .items_center()
            .px_1()
            .gap_1()
            .child(
                div()
                    .text_xs()
                    .text_color(color(palette.muted))
                    .flex_1()
                    .min_w_0()
                    .whitespace_normal()
                    .child(tr(title)),
            )
            .child(
                outlined_editor_button(key, palette, cx)
                    .xsmall()
                    .label(if hidden { tr("隐藏") } else { tr("显示") })
                    .on_click(cx.listener(move |this, _, _, cx| this.edit(&change, cx))),
            )
    }

    fn entry_fields(
        &self,
        id: &'static str,
        section: &Value,
        palette: chrome::Palette,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let lines = section["entryHeader"]
            .as_array()
            .map_or(&[][..], Vec::as_slice);
        let mut fields = div()
            .flex()
            .flex_col()
            .gap_1()
            .pt_2()
            .mt_1()
            .border_t_1()
            .border_color(color(palette.line))
            .child(
                div()
                    .text_xs()
                    .text_color(color(palette.muted))
                    .child(tr("条目字段行")),
            );
        for (index, line) in lines.iter().enumerate() {
            let names = line["parts"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|part| part["field"].as_str())
                .map(field_name)
                .collect::<Vec<_>>()
                .join(" · ");
            fields = fields.child(
                div()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .px_1()
                    .child(div().flex_1().min_w_0().truncate().text_xs().child(names))
                    .child(
                        editor_button(("edit-entry-field", index), palette, cx)
                            .xsmall()
                            .label(tr("编辑"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if !this.finish_text(cx) {
                                    return;
                                }
                                this.clear_text_edit();
                                this.header_line = Some(index);
                                cx.notify();
                            })),
                    )
                    .child(
                        editor_button(("remove-entry-field", index), palette, cx)
                            .xsmall()
                            .label("−")
                            .tooltip(tr("删除元素"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if !this.finish_text(cx) {
                                    return;
                                }
                                this.clear_text_edit();
                                this.edit(
                                    |v| {
                                        let section = v["sections"][id]
                                            .as_object_mut()
                                            .expect("existing section");
                                        let lines = array(
                                            section.get_mut("entryHeader").expect("existing lines"),
                                        );
                                        lines.remove(index);
                                        if lines.is_empty() {
                                            section.remove("entryHeader");
                                            section.remove("entryHeaderMode");
                                        }
                                    },
                                    cx,
                                );
                            })),
                    ),
            );
        }
        fields = fields.child(
            editor_button("theme-add-entry-field", palette, cx)
                .small()
                .label(tr("+ 字段行"))
                .disabled(lines.len() == 8)
                .on_click(cx.listener(|this, _, _, cx| {
                    if !this.finish_text(cx) {
                        return;
                    }
                    this.clear_text_edit();
                    this.header_line = Some(usize::MAX);
                    cx.notify();
                })),
        );
        if let Some(index) = self.header_line {
            let adding = index == usize::MAX;
            let mut options = div().flex().flex_wrap().gap_1();
            for &field in resume_one_core::semantic::header_field_names(id) {
                let already_used = lines.iter().any(|line| {
                    line["parts"]
                        .as_array()
                        .is_some_and(|parts| parts.iter().any(|part| part["field"] == field))
                });
                options = options.child(
                    editor_button(
                        (
                            "pick-entry-field",
                            resume_one_core::semantic::header_field_names(id)
                                .iter()
                                .position(|value| *value == field)
                                .unwrap(),
                        ),
                        palette,
                        cx,
                    )
                    .xsmall()
                    .custom(editor_colors(palette, cx).border(color(palette.line).into()))
                    .label(field_name(field))
                    .disabled(already_used)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.edit(
                            |v| {
                                let section = &mut v["sections"][id];
                                if adding && section["entryHeader"].is_null() {
                                    section["entryHeaderMode"] = json!("append");
                                }
                                let lines = array(&mut section["entryHeader"]);
                                if adding {
                                    lines.push(json!({"role":"meta","parts":[{"field":field}]}));
                                } else {
                                    lines[index]["parts"] = json!([{"field":field}]);
                                }
                            },
                            cx,
                        );
                        if this.message.is_none() {
                            this.header_line = None;
                        }
                    })),
                );
            }
            fields = fields.child(options).child(
                editor_button("cancel-entry-field", palette, cx)
                    .xsmall()
                    .label(tr("取消"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.header_line = None;
                        cx.notify();
                    })),
            );
        }
        fields
    }
}

fn field_name(id: &str) -> &str {
    match id {
        "position" => tr("职位"),
        "name" => tr("名称"),
        "organization" => tr("组织"),
        "institution" => tr("院校"),
        "studyType" => tr("学位"),
        "area" => tr("专业"),
        "score" => tr("成绩"),
        "dateRange" => tr("日期范围"),
        "date" | "releaseDate" => tr("日期"),
        "location" => tr("地点"),
        "title" => tr("标题"),
        "awarder" => tr("授奖方"),
        "issuer" => tr("颁发机构"),
        "publisher" => tr("出版方"),
        "entity" => tr("机构"),
        "type" => tr("类型"),
        "roles" => tr("角色"),
        "keywords" => tr("关键词"),
        _ => id,
    }
}

impl Render for ThemeEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (palette, pages) = self
            .owner
            .upgrade()
            .map(|owner| {
                let app = owner.read(cx);
                (
                    chrome::Palette::new(app.shell_style()),
                    app.theme_draft_page_count(),
                )
            })
            .unwrap_or_else(|| {
                (
                    chrome::Palette::new(&ThemeStyle::from_manifest(&self.base)),
                    None,
                )
            });
        let mut tabs = div()
            .w_full()
            .flex()
            .gap_1()
            .p_1()
            .rounded_md()
            .bg(color(editor_surface(palette)));
        for (tab, title) in [(Tab::Paper, "纸面"), (Tab::Modules, "模块")] {
            tabs = tabs.child(
                editor_button(title, palette, cx)
                    .debug_selector(move || {
                        if tab == Tab::Paper {
                            "theme-tab-paper".into()
                        } else {
                            "theme-tab-modules".into()
                        }
                    })
                    .small()
                    .flex_1()
                    .h(px(28.))
                    .custom(
                        editor_colors(palette, cx)
                            .color(color(editor_surface(palette)).into())
                            .active(color(palette.panel).into()),
                    )
                    .label(tr(title))
                    .selected(self.tab == tab)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.navigate(tab, this.module, cx);
                    })),
            );
        }
        let content = match (self.tab, self.module) {
            (Tab::Paper, _) => self.paper(palette, pages, cx).into_any_element(),
            (Tab::Modules, Some(id)) => self.module_detail(id, palette, cx).into_any_element(),
            (Tab::Modules, None) => self.modules(palette, cx).into_any_element(),
        };
        let reduce_motion = self
            .owner
            .upgrade()
            .is_none_or(|owner| owner.read(cx).reduce_motion);
        let content = div().w_full().min_w_0().flex_none().child(content);
        let content = if reduce_motion {
            content.into_any_element()
        } else {
            content
                .with_animation(
                    ("theme-editor-transition", self.motion),
                    Animation::new(std::time::Duration::from_millis(140)),
                    |view, t| view.opacity(0.35 + 0.65 * t * t * (3.0 - 2.0 * t)),
                )
                .into_any_element()
        };
        let scroll = match (self.tab, self.module) {
            (Tab::Paper, _) => &self.scroll[0],
            (Tab::Modules, None) => &self.scroll[1],
            (Tab::Modules, Some(id)) => self.module_scroll.get(id).expect("standard module"),
        };
        div()
            .id("theme-editor")
            .bg(color(palette.panel))
            .text_color(color(palette.ink))
            .text_sm()
            .debug_selector(|| "theme-editor".into())
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_none()
                    .p_3()
                    .border_b_1()
                    .border_color(color(palette.line))
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(px(17.))
                                    .font_family(chrome::SERIF)
                                    .child(tr("编辑主题")),
                            )
                            .child(
                                editor_button("close-theme-editor", palette, cx)
                                    .debug_selector(|| "close-theme-editor".into())
                                    .xsmall()
                                    .label(tr("关闭"))
                                    .tooltip(tr("关闭"))
                                    .on_click({
                                        let owner = self.owner.clone();
                                        // Closing reads this editor, so avoid holding its update lease.
                                        move |_, window, cx| {
                                            if let Some(owner) = owner.upgrade() {
                                                owner.update(cx, |app, cx| {
                                                    app.request_close_theme_editor(window, cx)
                                                });
                                            }
                                        }
                                    }),
                            ),
                    )
                    .child(div().text_xs().text_color(color(palette.muted)).child(tr(
                        if self.has_changes(cx) {
                            "未保存副本"
                        } else {
                            "尚未修改"
                        },
                    )))
                    .child(tabs),
            )
            .child(
                div()
                    .id("theme-editor-content")
                    .debug_selector(|| "theme-editor-content".into())
                    .min_w_0()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(scroll)
                    .p_3()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .when(self.from_original, |view| {
                        view.child(
                            div()
                                .mb_1()
                                .text_xs()
                                .whitespace_normal()
                                .text_color(color(palette.muted))
                                .child(tr("正在编辑原版主题的声明式迁移版。")),
                        )
                    })
                    .child(content),
            )
            .child(
                div()
                    .flex_none()
                    .p_3()
                    .border_t_1()
                    .border_color(color(palette.line))
                    .flex()
                    .flex_col()
                    .gap_2()
                    .when_some(self.message.clone(), |view, message| {
                        view.child(
                            div()
                                .id("theme-edit-message")
                                .max_h(px(68.))
                                .overflow_y_scroll()
                                .text_xs()
                                .whitespace_normal()
                                .text_color(color(palette.accent))
                                .child(message),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(color(palette.muted))
                                    .child(tr("副本名称")),
                            )
                            .child(
                                div()
                                    .flex()
                                    .gap_1()
                                    .child(
                                        editor_button("theme-undo", palette, cx)
                                            .xsmall()
                                            .label("↶")
                                            .tooltip(tr("撤销"))
                                            .disabled(self.history.is_empty())
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.restore(true, window, cx)
                                            })),
                                    )
                                    .child(
                                        editor_button("theme-redo", palette, cx)
                                            .xsmall()
                                            .label("↷")
                                            .tooltip(tr("重做"))
                                            .disabled(self.redo.is_empty())
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.restore(false, window, cx)
                                            })),
                                    ),
                            ),
                    )
                    .child(edit_input(&self.name, palette))
                    .child(
                        editor_button("theme-save-copy", palette, cx)
                            .debug_selector(|| "theme-save-copy".into())
                            .small()
                            .w_full()
                            .h(px(30.))
                            .custom(primary_colors(palette, cx))
                            .label(tr("保存并应用副本"))
                            .on_click(
                                cx.listener(|this, _, window, cx| this.save_copy(window, cx)),
                            ),
                    ),
            )
    }
}

// Keep the same legible foreground on both resting and hovered surfaces.
fn editor_surface(palette: chrome::Palette) -> resume_one_core::theme_style::Rgb {
    for step in (0..=20).rev() {
        let surface = palette.panel.mix(palette.soft, step as f32 / 20.0);
        if palette.ink.contrast(surface) >= 4.5 {
            return surface;
        }
    }
    palette.panel
}

// The résumé controls the inspector palette; system appearance can differ.
fn editor_colors(palette: chrome::Palette, cx: &gpui::App) -> ButtonCustomVariant {
    ButtonCustomVariant::new(cx)
        .color(gpui::Hsla::from(color(palette.panel)).opacity(0.0))
        .foreground(color(palette.ink).into())
        .border(gpui::Hsla::from(color(palette.panel)).opacity(0.0))
        .hover(color(editor_surface(palette)).into())
        .active(color(editor_surface(palette)).into())
        .shadow(false)
}

fn primary_colors(palette: chrome::Palette, cx: &gpui::App) -> ButtonCustomVariant {
    ButtonCustomVariant::new(cx)
        .color(color(palette.accent).into())
        .foreground(color(chrome::readable_on(palette.panel, palette.accent)).into())
        .border(color(palette.accent).into())
        .hover(color(palette.accent).into())
        .active(color(palette.accent).into())
        .shadow(false)
}

fn editor_button(
    id: impl Into<gpui::ElementId>,
    palette: chrome::Palette,
    cx: &gpui::App,
) -> Button {
    Button::new(id)
        .custom(editor_colors(palette, cx))
        .min_w_0()
        .rounded(px(5.))
}

fn outlined_editor_button(
    id: impl Into<gpui::ElementId>,
    palette: chrome::Palette,
    cx: &gpui::App,
) -> Button {
    editor_button(id, palette, cx)
        .custom(editor_colors(palette, cx).border(color(palette.line).into()))
}

fn edit_input(input: &Entity<InputState>, palette: chrome::Palette) -> Input {
    Input::new(input)
        .small()
        .w_full()
        .min_w_0()
        .palette(InputPalette {
            background: color(palette.panel).into(),
            border: color(palette.line).into(),
            focus: color(palette.accent).into(),
            foreground: color(palette.ink).into(),
            placeholder: color(palette.muted).into(),
            selection: color(palette.soft).into(),
        })
}

impl ResumeOne {
    pub(super) fn theme_edit_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = chrome::Palette::new(self.shell_style());
        div()
            .id("show-theme-editor")
            .debug_selector(|| "show-theme-editor".into())
            .tab_index(0)
            .tab_stop(true)
            .cursor_pointer()
            .h(px(30.))
            .px_1()
            .flex()
            .items_center()
            .rounded(px(5.))
            .text_size(px(11.))
            .text_color(color(palette.muted))
            .hover(|view| view.bg(color(palette.soft)))
            .focus(|view| view.bg(color(palette.soft)))
            .child(tr(if self.theme_editor_loading {
                "载入主题中…"
            } else {
                "编辑主题"
            }))
            .on_click(cx.listener(|this, _, window, cx| this.show_theme_editor(window, cx)))
            .on_key_down(cx.listener(|this, event, window, cx| {
                if chrome::activation_key(event) {
                    cx.stop_propagation();
                    this.show_theme_editor(window, cx);
                }
            }))
    }

    pub(super) fn show_theme_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.source_mode
            || self.theme_manager_open
            || self.theme_editor.is_some()
            || self.theme_editor_loading
        {
            return;
        }
        self.inline_window_left(window, cx);
        self.clear_inline();
        self.cancel_insertion();
        self.files.open = false;
        self.export_menu_open = false;
        self.hovered_theme = None;
        self.online.cancel_interaction();
        let theme = self.model.applied_theme().clone();
        if theme.original.is_none() {
            self.open_theme_editor(theme, false, window, cx);
            return;
        }
        let id = format!(
            "community-{}",
            theme.manifest.id.trim_start_matches("original-")
        );
        if let Some(theme) = self
            .model
            .themes
            .iter()
            .find(|theme| theme.original.is_none() && theme.manifest.id == id)
            .cloned()
        {
            self.open_theme_editor(theme, true, window, cx);
            return;
        }
        self.theme_editor_loading = true;
        self.theme_editor_generation = self.theme_editor_generation.wrapping_add(1);
        let generation = self.theme_editor_generation;
        let theme_identity = theme.identity;
        let task = cx.background_executor().spawn(async move {
            let cache_dir = dirs::cache_dir().ok_or_else(|| "无法定位主题缓存目录".to_string())?;
            let cache = ThemeCache::new(&cache_dir.join("Resume One").join("theme-cache"))
                .map_err(|e| e.to_string())?;
            let client = GithubClient::new(cache);
            let [source, _] = GithubSource::defaults();
            let catalog = client
                .catalog(&source, true)
                .or_else(|_| client.catalog(&source, false))
                .map_err(|error| error.to_string())?
                .catalog;
            let variant = client
                .theme(&catalog, &id, false)
                .map_err(|e| e.to_string())?;
            BuiltinTheme::from_verified(&variant, format!("migration:{id}"))
                .filter(|theme| theme.original.is_none())
                .ok_or_else(|| "主题缺少有效的声明式迁移版".to_owned())
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            this.update_in(cx, |this, window, cx| {
                if this.theme_editor_generation != generation {
                    return;
                }
                this.theme_editor_loading = false;
                if this.model.applied_theme().identity == theme_identity
                    && !this.source_mode
                    && !this.theme_manager_open
                {
                    match result {
                        Ok(theme) => this.open_theme_editor(theme, true, window, cx),
                        Err(error) => {
                            this.editor_warning(tr("无法载入可编辑主题"), &error, window, cx)
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn open_theme_editor(
        &mut self,
        theme: BuiltinTheme,
        from_original: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let document_order: Vec<_> = self.model.content.sections.iter().map(|s| s.id).collect();
        let value = initial_value(&theme.manifest, document_order.into_iter());
        let Ok(draft) = theme.manifest.edited_copy(&value) else {
            self.editor_warning(
                tr("无法载入可编辑主题"),
                tr("主题清单未通过校验"),
                window,
                cx,
            );
            return;
        };
        self.close_drawer();
        self.appearance_editor = None;
        self.model.preview_appearance(None);
        let owner = cx.entity().downgrade();
        self.theme_editor =
            Some(cx.new(|cx| ThemeEditor::new(owner, &theme, value, from_original, window, cx)));
        self.render_theme_draft(draft, cx);
        window.focus(&self.focus);
    }

    pub(super) fn close_theme_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.theme_editor_generation = self.theme_editor_generation.wrapping_add(1);
        self.theme_editor_loading = false;
        self.theme_exit_pending = false;
        self.theme_editor = None;
        if self.theme_draft.take().is_some() {
            self.model.theme_epoch = self.model.theme_epoch.wrapping_add(1);
        }
        self.focus_current(window, cx);
        cx.notify();
    }

    pub(super) fn request_close_theme_editor(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.after_theme_guard(window, cx, |_, _, _| {});
    }

    pub(super) fn after_theme_guard(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        action: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) {
        if self.theme_exit_pending {
            return;
        }
        let Some(editor) = self
            .theme_editor
            .as_ref()
            .filter(|editor| editor.read(cx).has_changes(cx))
        else {
            if self.theme_editor.is_some() || self.theme_editor_loading {
                self.close_theme_editor(window, cx);
            }
            action(self, window, cx);
            return;
        };
        let editor = editor.downgrade();
        self.theme_exit_pending = true;
        let answer = window.prompt(
            PromptLevel::Warning,
            tr("未保存主题副本"),
            Some(tr("保存主题副本后继续，或放弃本次修改。")),
            &[tr("继续编辑"), tr("放弃修改"), tr("保存副本并继续")],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let answer = answer.await.ok();
            this.update_in(cx, |app, window, cx| {
                app.theme_exit_pending = false;
                if editor.upgrade().as_ref() != app.theme_editor.as_ref() {
                    return;
                }
                match answer {
                    Some(1) => {
                        app.close_theme_editor(window, cx);
                        action(app, window, cx);
                    }
                    Some(2) => {
                        let owner = cx.entity().downgrade();
                        // Saving the child updates its owner. Release the owner first.
                        window.defer(cx, move |window, cx| {
                            let (Some(owner), Some(editor)) = (owner.upgrade(), editor.upgrade())
                            else {
                                return;
                            };
                            if owner.read(cx).theme_editor.as_ref() != Some(&editor) {
                                return;
                            }
                            editor.update(cx, |editor, cx| editor.save_copy(window, cx));
                            if owner.read(cx).theme_editor.is_none() {
                                owner.update(cx, |app, cx| action(app, window, cx));
                            }
                        });
                    }
                    _ => {}
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn render_theme_draft(&mut self, manifest: ThemeManifest, cx: &mut Context<Self>) {
        self.theme_draft = Some(BuiltinTheme {
            style: ThemeStyle::from_manifest(&manifest),
            identity: format!("draft:{}", manifest.id),
            manifest,
            original: None,
            source: None,
            path: None,
        });
        self.model.theme_epoch = self.model.theme_epoch.wrapping_add(1);
        cx.notify();
    }

    fn install_personal_theme(
        &mut self,
        themes: Vec<BuiltinTheme>,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let theme = themes
            .iter()
            .find(|theme| theme.path.as_ref() == Some(&path))
            .cloned()
            .ok_or_else(|| tr("主题已保存，但未能重新加载").to_owned())?;
        self.model.replace_themes(themes);
        self.close_theme_editor(window, cx);
        self.apply_theme_selection(theme);
        self.online.set_online(false);
        self.theme_scroll.set_offset(gpui::point(px(0.), px(0.)));
        cx.notify();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    #[test]
    fn hover_surfaces_preserve_readable_text_even_at_midtones() {
        use resume_one_core::theme_style::Rgb;
        for gray in 0..=255 {
            let panel = Rgb {
                r: gray,
                g: gray,
                b: gray,
            };
            let ink = chrome::readable_on(
                Rgb {
                    r: 32,
                    g: 45,
                    b: 42,
                },
                panel,
            );
            for tint in [
                Rgb { r: 0, g: 0, b: 0 },
                Rgb {
                    r: 255,
                    g: 255,
                    b: 255,
                },
            ] {
                let palette = chrome::Palette {
                    panel,
                    ink,
                    line: panel,
                    muted: ink,
                    accent: ink,
                    soft: panel.mix(tint, 0.09),
                };
                assert!(ink.contrast(editor_surface(palette)) >= 4.5);
            }
        }
    }

    #[gpui::test]
    fn narrow_inspector_keeps_module_labels_and_actions_on_separate_rows(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let (view, cx) = cx.add_window_view(|_, cx| {
            ResumeOne::new(crate::model::AppModel::load(None).unwrap(), cx)
        });
        cx.simulate_resize(gpui::size(px(1080.0), px(720.0)));
        for preference in [
            crate::i18n::Preference::Chinese,
            crate::i18n::Preference::English,
        ] {
            cx.update(|window, cx| {
                preference.apply();
                gpui_component::Theme::change(gpui_component::ThemeMode::Dark, Some(window), cx);
                view.update(cx, |app, cx| app.show_theme_editor(window, cx));
                let editor = view.read(cx).theme_editor.clone().unwrap();
                editor.update(cx, |editor, cx| editor.navigate(Tab::Modules, None, cx));
            });
            cx.run_until_parked();
            let panel = cx.debug_bounds("theme-editor").unwrap();
            let name = cx.debug_bounds("theme-module-work").unwrap();
            let action = cx.debug_bounds("theme-module-toggle-work").unwrap();
            let save = cx.debug_bounds("theme-save-copy").unwrap();
            assert!(name.origin.x >= panel.origin.x && name.right() <= panel.right());
            assert!(
                name.size.width >= px(170.0),
                "module label needs its own line"
            );
            assert!(
                name.bottom() <= action.origin.y,
                "actions must not crowd the label"
            );
            assert!(action.right() <= panel.right());
            assert!(save.origin.x >= panel.origin.x && save.right() <= panel.right());
            assert!(save.bottom() <= panel.bottom());
            cx.update(|window, cx| {
                let editor = view.read(cx).theme_editor.clone().unwrap();
                editor.update(cx, |editor, cx| editor.navigate(Tab::Paper, None, cx));
                window.refresh();
            });
            cx.run_until_parked();
            let apply = cx.debug_bounds("theme-margin-apply").unwrap();
            let content = cx.debug_bounds("theme-editor-content").unwrap();
            assert!(apply.origin.x >= content.origin.x && apply.right() <= content.right());
            assert!(apply.bottom() < save.origin.y);
            cx.update(|window, cx| {
                let editor = view.read(cx).theme_editor.clone().unwrap();
                editor.update(cx, |editor, cx| {
                    editor.navigate(Tab::Modules, Some("work"), cx)
                });
                window.refresh();
            });
            cx.run_until_parked();
            for control in ["theme-edit-title", "theme-new-text", "theme-new-divider"] {
                let bounds = cx.debug_bounds(control).expect(control);
                assert!(
                    bounds.origin.x >= content.origin.x && bounds.right() <= content.right(),
                    "{control} must stay inside the inspector"
                );
            }
            cx.update(|window, cx| view.update(cx, |app, cx| app.close_theme_editor(window, cx)));
        }
        crate::i18n::Preference::Chinese.apply();
    }

    #[gpui::test]
    fn dirty_state_survives_history_truncation_and_recognizes_restored_values(
        cx: &mut TestAppContext,
    ) {
        cx.update(gpui_component::init);
        let (view, cx) = cx.add_window_view(|_, cx| {
            ResumeOne::new(crate::model::AppModel::load(None).unwrap(), cx)
        });
        cx.update(|window, cx| {
            view.update(cx, |app, cx| app.show_theme_editor(window, cx));
            let editor = view.read(cx).theme_editor.clone().unwrap();
            editor.update(cx, |editor, cx| {
                let initial = editor.initial.clone();
                let top = initial["page"]["marginsMm"]["top"].as_f64().unwrap();
                editor.edit(
                    |value| value["page"]["marginsMm"]["top"] = json!(top + 1.0),
                    cx,
                );
                editor.edit(|value| *value = initial.clone(), cx);
                editor.refresh_margins(window, cx);
                assert!(!editor.has_changes(cx));
                for step in 1..=61 {
                    editor.edit(
                        |value| value["page"]["marginsMm"]["top"] = json!(top + step as f64 / 10.0),
                        cx,
                    );
                }
                for _ in 0..60 {
                    editor.restore(true, window, cx);
                }
                assert_ne!(editor.value, initial);
                assert!(editor.has_changes(cx));
            });
        });
    }

    #[gpui::test]
    fn navigation_commits_text_but_keeps_invalid_input_and_scroll_position(
        cx: &mut TestAppContext,
    ) {
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
            view.update(cx, |app, cx| app.show_theme_editor(window, cx));
            let editor = view.read(cx).theme_editor.clone().unwrap();
            editor.update(cx, |editor, cx| {
                editor.navigate(Tab::Modules, Some("work"), cx);
                editor.editing_title = true;
                editor.open_text("Work".into(), window, cx);
                let language = editor.text_language;
                editor.text.update(cx, |input, cx| {
                    input.set_value("Selected experience", window, cx)
                });
                editor.module_scroll["work"].set_offset(gpui::point(px(0.0), px(-72.0)));
                editor.navigate(Tab::Paper, Some("work"), cx);
                let locale = if language == resume_one_core::render::Language::En {
                    "en"
                } else {
                    "zh-cn"
                };
                assert_eq!(
                    editor.value["localizations"][locale]["sections"]["work"],
                    "Selected experience"
                );
                editor.navigate(Tab::Modules, Some("work"), cx);
                assert_eq!(editor.module_scroll["work"].offset().y, px(-72.0));
                editor.editing_title = true;
                editor.open_text("Selected experience".into(), window, cx);
                editor
                    .text
                    .update(cx, |input, cx| input.set_value("x".repeat(500), window, cx));
                editor.navigate(Tab::Paper, Some("work"), cx);
                assert!(editor.tab == Tab::Modules && editor.editing_title);
                assert!(editor.message.is_some());
                assert_eq!(editor.text.read(cx).value().len(), 500);
            });
        });
    }

    #[gpui::test]
    fn guarded_new_document_can_cancel_and_save_and_failure_keeps_draft(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let temp = tempfile::tempdir().unwrap();
        let store = crate::theme_local::ThemeStore::new(
            temp.path().join("settings.json"),
            temp.path().join("themes"),
        )
        .unwrap();
        let child = std::rc::Rc::new(std::cell::RefCell::new(None));
        let output = child.clone();
        let (_, cx) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| {
                let mut app = ResumeOne::new(crate::model::AppModel::load(None).unwrap(), cx);
                app.theme_store = Some(store);
                app
            });
            *output.borrow_mut() = Some(view.clone());
            gpui_component::Root::new(view, window, cx)
        });
        let view = child.borrow_mut().take().unwrap();
        let source = cx.update(|_, cx| view.read(cx).model.document.source.clone());
        cx.update(|window, cx| {
            view.update(cx, |app, cx| app.show_theme_editor(window, cx));
            let editor = view.read(cx).theme_editor.clone().unwrap();
            editor.update(cx, |editor, cx| {
                editor.edit(|value| value["page"]["marginsMm"]["top"] = json!(10), cx);
                editor.refresh_margins(window, cx);
            });
            view.update(cx, |app, cx| assert!(!app.confirm_close(window, cx)));
        });
        assert!(cx.has_pending_prompt());
        cx.simulate_prompt_answer(tr("继续编辑"));
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(view.read(cx).theme_editor.is_some());
            view.update(cx, |app, cx| {
                app.new_resume(&crate::files::NewResume, window, cx)
            });
        });
        assert!(cx.has_pending_prompt());
        cx.simulate_prompt_answer(tr("继续编辑"));
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(view.read(cx).theme_editor.is_some());
            assert_eq!(view.read(cx).model.document.source, source);
            let editor = view.read(cx).theme_editor.clone().unwrap();
            editor.update(cx, |editor, cx| {
                editor
                    .name
                    .update(cx, |name, cx| name.set_value("", window, cx))
            });
            view.update(cx, |app, cx| {
                app.new_resume(&crate::files::NewResume, window, cx)
            });
        });
        cx.simulate_prompt_answer(tr("保存副本并继续"));
        cx.run_until_parked();
        cx.update(|window, cx| {
            let editor = view
                .read(cx)
                .theme_editor
                .clone()
                .expect("failed save preserves the draft");
            assert_eq!(view.read(cx).model.document.source, source);
            editor.update(cx, |editor, cx| {
                editor.name.update(cx, |name, cx| {
                    name.set_value("Saved before new", window, cx)
                })
            });
            view.update(cx, |app, cx| {
                app.new_resume(&crate::files::NewResume, window, cx)
            });
        });
        cx.simulate_prompt_answer(tr("保存副本并继续"));
        cx.run_until_parked();
        cx.update(|_, cx| {
            let app = view.read(cx);
            assert!(app.theme_editor.is_none());
            assert_eq!(app.model.applied_theme().manifest.name, "Saved before new");
            assert_eq!(app.model.document.value, json!({"basics":{}}));
        });
    }

    #[gpui::test]
    fn dragging_modules_reorders_without_losing_content_and_can_be_undone(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let (view, cx) = cx.add_window_view(|_, cx| {
            ResumeOne::new(crate::model::AppModel::load(None).unwrap(), cx)
        });
        cx.simulate_resize(gpui::size(px(1340.0), px(820.0)));
        cx.update(|window, cx| {
            view.update(cx, |app, cx| app.show_theme_editor(window, cx));
            let editor = view.read(cx).theme_editor.clone().unwrap();
            editor.update(cx, |editor, cx| editor.navigate(Tab::Modules, None, cx));
            window.refresh();
        });
        let before = cx.update(|_, cx| view.read(cx).theme_editor.as_ref().unwrap().read(cx).value["composition"]["moduleOrder"].clone());
        let source = cx.debug_bounds("theme-module-grip-work").unwrap().center();
        let target = cx
            .debug_bounds("theme-module-target-education")
            .unwrap()
            .center();
        cx.simulate_mouse_down(source, gpui::MouseButton::Left, gpui::Modifiers::none());
        cx.simulate_mouse_move(target, gpui::MouseButton::Left, gpui::Modifiers::none());
        cx.simulate_mouse_up(target, gpui::MouseButton::Left, gpui::Modifiers::none());
        cx.run_until_parked();
        cx.update(|window, cx| {
            let editor = view.read(cx).theme_editor.clone().unwrap();
            editor.update(cx, |editor, cx| {
                let after = &editor.value["composition"]["moduleOrder"];
                assert_ne!(after, &before);
                let mut sorted_before = before.as_array().unwrap().clone();
                let mut sorted_after = after.as_array().unwrap().clone();
                sorted_before.sort_by_key(|value| value.as_str().unwrap().to_owned());
                sorted_after.sort_by_key(|value| value.as_str().unwrap().to_owned());
                assert_eq!(sorted_before, sorted_after);
                editor.restore(true, window, cx);
                assert_eq!(editor.value["composition"]["moduleOrder"], before);
            });
        });
    }

    #[gpui::test]
    fn close_button_releases_an_unchanged_theme_editor(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let model = crate::model::AppModel::load(None).unwrap();
        let original = model.applied_theme().identity.clone();
        let source = model.document.source.clone();
        let (view, cx) = cx.add_window_view(|_, cx| ResumeOne::new(model, cx));
        cx.simulate_resize(gpui::size(px(1340.0), px(820.0)));
        cx.update(|window, cx| {
            view.update(cx, |app, cx| app.show_theme_editor(window, cx));
        });
        cx.run_until_parked();
        let close = cx.debug_bounds("close-theme-editor").expect("close button");
        cx.simulate_click(close.center(), gpui::Modifiers::none());
        cx.run_until_parked();
        assert!(!cx.has_pending_prompt());
        cx.update(|window, _| window.refresh());
        assert!(cx.debug_bounds("show-theme-editor").is_some());
        cx.update(|_, cx| {
            let app = view.read(cx);
            assert!(app.theme_editor.is_none());
            assert!(app.theme_draft.is_none());
            assert_eq!(app.model.applied_theme().identity, original);
            assert_eq!(app.model.document.source, source);
        });
    }

    #[gpui::test]
    fn close_button_preserves_changes_until_discard_is_confirmed(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let model = crate::model::AppModel::load(None).unwrap();
        let original = model.applied_theme().clone();
        let source = model.document.source.clone();
        let (view, cx) = cx.add_window_view(|_, cx| ResumeOne::new(model, cx));
        cx.simulate_resize(gpui::size(px(1340.0), px(820.0)));
        cx.update(|window, cx| {
            view.update(cx, |app, cx| app.show_theme_editor(window, cx));
            let editor = view.read(cx).theme_editor.clone().unwrap();
            editor.update(cx, |editor, cx| {
                editor.edit(|value| value["page"]["marginsMm"]["top"] = json!(10), cx);
                editor.refresh_margins(window, cx);
            });
        });
        cx.run_until_parked();
        let close = cx.debug_bounds("close-theme-editor").expect("close button");
        cx.simulate_click(close.center(), gpui::Modifiers::none());
        cx.run_until_parked();
        assert!(cx.has_pending_prompt());
        cx.simulate_prompt_answer(tr("继续编辑"));
        cx.run_until_parked();
        cx.update(|_, cx| {
            assert!(view.read(cx).theme_editor.is_some());
            assert_eq!(view.read(cx).display_theme().style.page.margins.top, 10.0);
        });
        cx.simulate_click(close.center(), gpui::Modifiers::none());
        cx.run_until_parked();
        cx.simulate_prompt_answer(tr("放弃修改"));
        cx.run_until_parked();
        cx.update(|_, cx| {
            let app = view.read(cx);
            assert!(app.theme_editor.is_none());
            assert!(app.theme_draft.is_none());
            assert_eq!(app.model.applied_theme().identity, original.identity);
            assert_eq!(
                app.display_theme().style.page.margins.top,
                original.style.page.margins.top
            );
            assert_eq!(app.model.document.source, source);
        });
        assert!(!cx.has_pending_prompt());
    }

    #[gpui::test]
    fn theme_draft_can_be_undone_and_saved_beside_the_source(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("themes");
        let mut store = crate::theme_local::ThemeStore::new(
            temp.path().join("settings.json"),
            directory.clone(),
        )
        .unwrap();
        let mut model = crate::model::AppModel::load(None).unwrap();
        model.replace_themes(store.load_themes().unwrap());
        let original = model.applied_theme().clone();
        let original_bytes = std::fs::read(original.path.as_ref().unwrap()).unwrap();
        let resume_bytes = model.document.source.clone();
        let (view, cx) = cx.add_window_view(|_, cx| {
            let mut app = ResumeOne::new(model, cx);
            app.theme_store = Some(store);
            app
        });
        cx.simulate_resize(gpui::size(px(1340.0), px(820.0)));
        cx.update(|window, _| window.refresh());
        let entry = cx.debug_bounds("show-theme-editor").expect("edit button");
        cx.simulate_click(entry.center(), gpui::Modifiers::none());
        cx.run_until_parked();
        let panel = cx.debug_bounds("theme-editor").expect("theme editor");
        let paper = cx
            .debug_bounds("resume-paper")
            .expect("paper remains visible");
        assert!(paper.right() <= panel.left());
        let editor = cx.update(|_, cx| view.read(cx).theme_editor.clone().unwrap());
        let directory = directory.canonicalize().unwrap();
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.edit(
                    |v| {
                        v["page"]["fitToOnePage"] = json!(true);
                        v["page"]["marginsMm"]["top"] = json!(10);
                    },
                    cx,
                );
                editor.refresh_margins(window, cx);
            });
            assert_eq!(view.read(cx).display_theme().style.page.margins.top, 10.0);
            assert_eq!(
                view.read(cx).model.applied_theme().identity,
                original.identity
            );
            editor.update(cx, |editor, cx| editor.restore(true, window, cx));
            assert_eq!(
                view.read(cx).display_theme().style.page.margins.top,
                original.style.page.margins.top
            );
            editor.update(cx, |editor, cx| editor.restore(false, window, cx));
            assert!(view.read(cx).display_theme().style.page.fit_to_one_page);
            editor.update(cx, |editor, cx| {
                editor
                    .name
                    .update(cx, |input, cx| input.set_value("Draft Copy", window, cx));
                editor.save_copy(window, cx);
            });
            let app = view.read(cx);
            assert!(app.theme_editor.is_none());
            assert_eq!(app.model.document.source, resume_bytes);
            assert_eq!(app.model.applied_theme().manifest.name, "Draft Copy");
            assert!(
                app.model
                    .applied_theme()
                    .path
                    .as_ref()
                    .unwrap()
                    .starts_with(directory)
            );
            assert_eq!(app.model.applied_theme().style.page.margins.top, 10.0);
        });
        assert_eq!(
            std::fs::read(original.path.as_ref().unwrap()).unwrap(),
            original_bytes
        );
    }
}
