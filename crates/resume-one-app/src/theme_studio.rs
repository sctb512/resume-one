//! Edit a validated declarative theme while the paper remains visible.
use std::path::PathBuf;

use gpui::{
    Context, Entity, IntoElement, PromptLevel, Render, Subscription, WeakEntity, Window, div,
    prelude::*, px,
};
use gpui_component::{
    Disableable, Selectable, Sizable,
    button::{Button, ButtonVariants},
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
        let subscriptions = margins
            .iter()
            .map(|input| {
                cx.subscribe_in(input, window, |this, _, event, _, cx| {
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        this.apply_margins(cx);
                    }
                })
            })
            .collect();
        Self {
            owner,
            base: theme.manifest.clone(),
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
        !self.history.is_empty()
            || self.name.read(cx).value().as_ref() != self.default_name
            || self.editing_title
            || self.element.is_some()
            || SIDES.iter().zip(&self.margins).any(|((side, _), input)| {
                input.read(cx).value().as_ref() != self.value["page"]["marginsMm"][side].to_string()
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

    fn parsed_margins(&self, cx: &Context<Self>) -> Option<Value> {
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
        self.text.update(cx, |text, cx| {
            text.set_value(value, window, cx);
            text.focus(window, cx);
        });
        cx.notify();
    }

    fn save_text(&mut self, id: &'static str, cx: &mut Context<Self>) {
        let text = self.text.read(cx).value().trim().to_owned();
        let title = self.editing_title;
        let index = self.element;
        let language = self
            .owner
            .upgrade()
            .map(|owner| owner.read(cx).model.presentation_language);
        let locale = if language == Some(resume_one_core::render::Language::En) {
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
        if (self.editing_title || self.element.is_some())
            && let Some(id) = self.module
        {
            self.save_text(id, cx);
            if self.editing_title || self.element.is_some() {
                return;
            }
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
                                .text_xs()
                                .text_color(color(palette.muted))
                                .child(tr(SIDES[index].1)),
                        )
                        .child(edit_input(&self.margins[index], palette)),
                );
            }
            margins = margins.child(fields);
        }
        div().flex().flex_col().gap_2()
            .child(div().flex().justify_between().items_center().gap_1()
                .child(div().font_weight(gpui::FontWeight::SEMIBOLD).child(tr("页边距 · mm")))
                .child(Button::new("theme-margin-compact").small().ghost().label(tr("紧凑"))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.edit(|v| {
                            v["page"]["marginsMm"] = json!({"top":10, "bottom":10, "left":11, "right":11});
                            v["page"]["firstLinePosition"] = json!("line-box");
                        }, cx);
                        this.refresh_margins(window, cx);
                    }))))
            .child(margins)
            .child(div().flex().justify_between()
                .child(Button::new("theme-margin-reset").small().ghost().label(tr("恢复默认"))
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
                .child(Button::new("theme-margin-apply").small().outline().label(tr("应用"))
                    .on_click(cx.listener(|this, _, _, cx| { this.apply_margins(cx); }))))
            .child(div().mt_3().pt_3().border_t_1().border_color(color(palette.line)).flex().flex_col().gap_2()
                .child(Button::new("theme-fit").small().outline()
                    .label(if fit { tr("✓ 适配一页") } else { tr("适配一页") })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this.apply_margins(cx) {
                            this.edit(|v| v["page"]["fitToOnePage"] = json!(!fit), cx);
                        }
                    })))
                .when_some(pages, |view, count| view.child(div().text_xs().text_color(color(palette.muted)).child(
                    if fit && count > 1 { tr("内容较多，已保留主题分页").into() }
                    else if fit && count == 1 { tr("已排成一页").into() }
                    else if crate::i18n::is_english() { format!("{count} {}", if count == 1 {"page"} else {"pages"}) }
                    else { format!("共 {count} 页") }
                ))))
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
        let mut rows = div().flex().flex_col().gap_1().child(
            Button::new("theme-module-basics")
                .small()
                .outline()
                .label(label("basics"))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.module = Some("basics");
                    this.clear_text_edit();
                    cx.notify();
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
                .min_w_0()
                .flex()
                .items_center()
                .gap_1()
                .rounded_md()
                .bg(color(palette.soft))
                .child(
                    div().flex_1().min_w_0().child(
                        Button::new(gpui::SharedString::from(format!("theme-module-{id}")))
                            .small()
                            .ghost()
                            .w_full()
                            .label(label(id))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.module = Some(id);
                                this.clear_text_edit();
                                cx.notify();
                            })),
                    ),
                );
            for (offset, target) in adjacent.into_iter().enumerate() {
                row = row.child(
                    Button::new(gpui::SharedString::from(format!(
                        "theme-module-move-{id}-{offset}"
                    )))
                    .xsmall()
                    .ghost()
                    .label(if offset == 0 { "↑" } else { "↓" })
                    .disabled(target.is_none())
                    .tooltip(if offset == 0 {
                        tr("上移模块")
                    } else {
                        tr("下移模块")
                    })
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
            row = row.child(
                Button::new(gpui::SharedString::from(format!(
                    "theme-module-toggle-{id}"
                )))
                .xsmall()
                .ghost()
                .label(if hidden { "+" } else { "−" })
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
        let mut body = div().flex().flex_col().gap_2().child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    Button::new("theme-module-back")
                        .small()
                        .ghost()
                        .label("‹")
                        .tooltip(tr("返回模块"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.module = None;
                            this.clear_text_edit();
                            cx.notify();
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
                Button::new("theme-module-placement")
                    .small()
                    .outline()
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
                    .bg(color(palette.soft))
                    .pl_2()
                    .child(div().flex_1().min_w_0().truncate().text_xs().child(display))
                    .when_some(text, |row, text| {
                        row.child(
                            Button::new(gpui::SharedString::from(format!(
                                "theme-extra-edit-{index}"
                            )))
                            .xsmall()
                            .ghost()
                            .label(tr("编辑"))
                            .on_click(cx.listener(
                                move |this, _, window, cx| {
                                    this.clear_text_edit();
                                    this.element = Some(index);
                                    this.open_text(text.clone(), window, cx);
                                },
                            )),
                        )
                    })
                    .child(
                        Button::new(gpui::SharedString::from(format!(
                            "theme-extra-remove-{index}"
                        )))
                        .xsmall()
                        .ghost()
                        .label("−")
                        .tooltip(tr("删除元素"))
                        .on_click(cx.listener(move |this, _, _, cx| {
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
                    Button::new("theme-edit-title")
                        .small()
                        .outline()
                        .label(tr("编辑模块标题"))
                        .on_click(cx.listener(move |this, _, window, cx| {
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
                            Button::new("theme-new-text")
                                .small()
                                .ghost()
                                .label(tr("+ 文字"))
                                .disabled(extras.len() == 8)
                                .on_click(cx.listener(move |this, _, window, cx| {
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
                            Button::new("theme-new-divider")
                                .small()
                                .ghost()
                                .label(tr("+ 分隔线"))
                                .disabled(extras.len() == 8)
                                .on_click(cx.listener(move |this, _, _, cx| {
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
                                Button::new("theme-text-cancel")
                                    .small()
                                    .ghost()
                                    .label(tr("取消"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.clear_text_edit();
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("theme-text-apply")
                                    .small()
                                    .outline()
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
                    .child(tr(title)),
            )
            .child(
                Button::new(key)
                    .xsmall()
                    .outline()
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
                        Button::new(("edit-entry-field", index))
                            .xsmall()
                            .ghost()
                            .label(tr("编辑"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.clear_text_edit();
                                this.header_line = Some(index);
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new(("remove-entry-field", index))
                            .xsmall()
                            .ghost()
                            .label("−")
                            .tooltip(tr("删除元素"))
                            .on_click(cx.listener(move |this, _, _, cx| {
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
            Button::new("theme-add-entry-field")
                .small()
                .ghost()
                .label(tr("+ 字段行"))
                .disabled(lines.len() == 8)
                .on_click(cx.listener(|this, _, _, cx| {
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
                    Button::new((
                        "pick-entry-field",
                        resume_one_core::semantic::header_field_names(id)
                            .iter()
                            .position(|value| *value == field)
                            .unwrap(),
                    ))
                    .xsmall()
                    .outline()
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
                Button::new("cancel-entry-field")
                    .xsmall()
                    .ghost()
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
        let mut tabs = div().flex().gap_1();
        for (tab, title) in [(Tab::Paper, "纸面"), (Tab::Modules, "模块")] {
            tabs = tabs.child(
                Button::new(title)
                    .small()
                    .flex_1()
                    .outline()
                    .label(tr(title))
                    .selected(self.tab == tab)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this.tab == Tab::Paper && !this.apply_margins(cx) {
                            return;
                        }
                        this.tab = tab;
                        this.clear_text_edit();
                        cx.notify();
                    })),
            );
        }
        let content = match (self.tab, self.module) {
            (Tab::Paper, _) => self.paper(palette, pages, cx).into_any_element(),
            (Tab::Modules, Some(id)) => self.module_detail(id, palette, cx).into_any_element(),
            (Tab::Modules, None) => self.modules(palette, cx).into_any_element(),
        };
        div()
            .id("theme-editor")
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
                                Button::new("close-theme-editor")
                                    .xsmall()
                                    .ghost()
                                    .label("×")
                                    .tooltip(tr("关闭"))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        if let Some(owner) = this.owner.upgrade() {
                                            owner.update(cx, |app, cx| {
                                                app.request_close_theme_editor(window, cx)
                                            });
                                        }
                                    })),
                            ),
                    )
                    .child(tabs),
            )
            .child(
                div()
                    .id("theme-editor-content")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
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
                                        Button::new("theme-undo")
                                            .xsmall()
                                            .ghost()
                                            .label("↶")
                                            .tooltip(tr("撤销"))
                                            .disabled(self.history.is_empty())
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.restore(true, window, cx)
                                            })),
                                    )
                                    .child(
                                        Button::new("theme-redo")
                                            .xsmall()
                                            .ghost()
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
                        Button::new("theme-save-copy")
                            .small()
                            .primary()
                            .label(tr("保存并应用副本"))
                            .on_click(
                                cx.listener(|this, _, window, cx| this.save_copy(window, cx)),
                            ),
                    ),
            )
    }
}

fn edit_input(input: &Entity<InputState>, palette: chrome::Palette) -> Input {
    Input::new(input).small().palette(InputPalette {
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
        let Some(editor) = self
            .theme_editor
            .as_ref()
            .filter(|editor| editor.read(cx).has_changes(cx))
        else {
            self.close_theme_editor(window, cx);
            return;
        };
        let editor = editor.downgrade();
        let answer = window.prompt(
            PromptLevel::Warning,
            tr("未保存主题副本"),
            Some(tr("退出后将丢弃本次主题修改。")),
            &[tr("继续编辑"), tr("放弃修改")],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if answer.await.ok() == Some(1) {
                this.update_in(cx, |app, window, cx| {
                    if editor.upgrade().as_ref() == app.theme_editor.as_ref() {
                        app.close_theme_editor(window, cx);
                    }
                })
                .ok();
            }
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
