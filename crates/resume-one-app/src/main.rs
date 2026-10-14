#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod editor_ui;
mod export_ui;
mod files;
mod model;
mod structure_ui;
mod theme_local;
mod workspace;

use std::path::Path;
use std::{cell::RefCell, rc::Rc};

use gpui::{
    App, Application, Bounds, Context, FocusHandle, FontWeight, IntoElement, MouseButton, Render,
    ScrollHandle, Window, WindowBounds, WindowOptions, div, prelude::*, px, rgb, size,
};
use model::AppModel;
use resume_one_core::semantic::{Item, Section};
use resume_one_core::structure::StructureEdit;
use resume_one_core::theme_style::{Rgb, ThemeStyle};

const SECTION_LABELS: &[(&str, &str)] = &[
    ("basics", "基本信息"),
    ("work", "工作经历"),
    ("volunteer", "志愿经历"),
    ("education", "教育经历"),
    ("awards", "荣誉奖项"),
    ("certificates", "证书"),
    ("publications", "出版物"),
    ("skills", "技能"),
    ("languages", "语言"),
    ("interests", "兴趣"),
    ("references", "推荐人"),
    ("projects", "项目经历"),
];

fn color(value: Rgb) -> gpui::Rgba {
    rgb(u32::from(value.r) << 16 | u32::from(value.g) << 8 | u32::from(value.b))
}

fn label(id: &str) -> &str {
    SECTION_LABELS
        .iter()
        .find(|(key, _)| *key == id)
        .map_or(id, |(_, label)| label)
}

struct ResumeOne {
    model: AppModel,
    hovered_theme: Option<usize>,
    focus: FocusHandle,
    files: files::FileUi,
    preview_scroll: ScrollHandle,
    source_mode: bool,
    editor: Option<editor_ui::EditorUi>,
    export_busy: bool,
    theme_store: Option<theme_local::ThemeStore>,
    hovered_section: Option<String>,
    hovered_item: Option<(String, usize)>,
    insertion_before: Option<Option<String>>,
    selected_module: Option<String>,
    selected_item_module: Option<String>,
    visual_undo: Vec<(String, String)>,
    visual_redo: Vec<(String, String)>,
    recovery_pending: bool,
}

impl ResumeOne {
    fn new(model: AppModel, cx: &mut Context<Self>) -> Self {
        let files = files::FileUi {
            open: model.folder.is_some(),
            ..Default::default()
        };
        Self {
            model,
            hovered_theme: None,
            focus: cx.focus_handle(),
            files,
            preview_scroll: ScrollHandle::new(),
            source_mode: false,
            editor: None,
            export_busy: false,
            theme_store: None,
            hovered_section: None,
            hovered_item: None,
            insertion_before: None,
            selected_module: None,
            selected_item_module: None,
            visual_undo: Vec::new(),
            visual_redo: Vec::new(),
            recovery_pending: false,
        }
    }
    fn active_style(&self) -> &ThemeStyle {
        self.hovered_theme
            .and_then(|index| self.model.themes.get(index))
            .map_or_else(|| self.model.selected_style(), |theme| &theme.style)
    }

    fn quit(&mut self, _: &files::Quit, window: &mut Window, cx: &mut Context<Self>) {
        if self.confirm_close(window, cx) {
            window.remove_window();
        }
    }

    fn theme_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let accent = color(self.active_style().colors.accent);
        let warnings = self
            .theme_store
            .as_ref()
            .map_or_else(Vec::new, |store| store.warnings.clone());
        let mut list = div()
            .id("theme-list")
            .flex()
            .flex_col()
            .gap_1()
            .overflow_y_scroll()
            .flex_1()
            .on_scroll_wheel(cx.listener(|this, _, _, cx| {
                if this.hovered_theme.take().is_some() {
                    cx.notify();
                }
            }));
        for (index, theme) in self.model.themes.iter().enumerate() {
            let selected = index == self.model.selected_theme;
            let row = div()
                .id(("theme", index))
                .debug_selector(|| format!("theme-{index}"))
                .flex()
                .items_center()
                .gap_3()
                .px_3()
                .py_3()
                .rounded_md()
                .cursor_pointer()
                .when(selected, |row| row.bg(rgb(0xffffff)))
                .hover(|row| row.bg(rgb(0xffffff)))
                .child(
                    div()
                        .w(px(5.0))
                        .h(px(34.0))
                        .rounded_sm()
                        .bg(color(theme.style.colors.accent)),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .text_size(px(13.0))
                                .text_color(if selected { accent } else { rgb(0x333933) })
                                .child(theme.manifest.name.clone()),
                        )
                        .child(
                            div()
                                .text_size(px(10.0))
                                .text_color(rgb(0x8e958e))
                                .child(theme.manifest.id.clone()),
                        ),
                )
                .on_hover(cx.listener(move |this, hovering: &bool, _, cx| {
                    let next = if *hovering {
                        Some(index)
                    } else if this.hovered_theme == Some(index) {
                        None
                    } else {
                        this.hovered_theme
                    };
                    if this.hovered_theme != next {
                        this.hovered_theme = next;
                        cx.notify();
                    }
                }))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.model.select_theme(index);
                    cx.notify();
                }))
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, _, window, cx| {
                        this.reveal_theme(index, window, cx);
                    }),
                );
            list = list.child(row);
        }
        div()
            .id("theme-panel")
            .debug_selector(|| "theme-panel".to_string())
            .w(px(206.0))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(rgb(0xe4e7e1))
            .bg(rgb(0xf6f7f3))
            .px_3()
            .pt_5()
            .pb_4()
            .gap_4()
            .child(
                div()
                    .px_2()
                    .flex()
                    .justify_between()
                    .text_size(px(11.0))
                    .text_color(rgb(0x758078))
                    .child("主题")
                    .child(format!("本地 · {}", self.model.themes.len())),
            )
            .when(!warnings.is_empty(), |panel| {
                panel.child(
                    div()
                        .id("theme-warnings")
                        .px_2()
                        .text_size(px(10.0))
                        .text_color(rgb(0x9b6045))
                        .cursor_pointer()
                        .child(format!("跳过 {} 个无效主题 · 查看", warnings.len()))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.editor_warning("主题加载提示", &warnings.join("\n"), window, cx);
                        })),
                )
            })
            .child(list)
    }

    fn item(
        &self,
        item: &Item,
        section_id: &str,
        index: usize,
        style: &ThemeStyle,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let hover_id = section_id.to_owned();
        let id = section_id.to_owned();
        let mut view = div()
            .id(gpui::SharedString::from(format!(
                "preview-item-{section_id}-{index}"
            )))
            .relative()
            .flex()
            .flex_col()
            .gap_1()
            .mb_2()
            .pr_5()
            .on_hover(cx.listener(move |this, hovering: &bool, _, cx| {
                let target = (hover_id.clone(), index);
                if *hovering {
                    this.hovered_item = Some(target);
                } else if this.hovered_item.as_ref() == Some(&target) {
                    this.hovered_item = None;
                }
                cx.notify();
            }))
            .child(
                div()
                    .id(gpui::SharedString::from(format!(
                        "remove-item-{section_id}-{index}"
                    )))
                    .absolute()
                    .right_0()
                    .top_0()
                    .px_2()
                    .rounded_sm()
                    .cursor_pointer()
                    .opacity(
                        if self.hovered_item.as_ref() == Some(&(id.clone(), index)) {
                            1.0
                        } else {
                            0.0
                        },
                    )
                    .text_color(color(style.colors.seal))
                    .child("−")
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.apply_visual_structure(
                            StructureEdit::RemoveItem {
                                id: id.clone(),
                                index,
                            },
                            window,
                            cx,
                        );
                    })),
            );
        if !item.heading.is_empty() {
            view = view.child(
                div()
                    .text_size(px(style.typography.body.size_pt * 1.3))
                    .text_color(color(style.colors.ink))
                    .child(item.heading.clone()),
            );
        }
        for detail in &item.details {
            view = view.child(
                div()
                    .text_size(px(style.typography.meta.size_pt * 1.3))
                    .text_color(color(style.colors.muted))
                    .child(detail.clone()),
            );
        }
        for paragraph in &item.paragraphs {
            view = view.child(
                div()
                    .text_size(px(style.typography.body.size_pt * 1.3))
                    .line_height(px(style.typography.body.size_pt
                        * 1.3
                        * style.typography.body.line_height))
                    .text_color(color(style.colors.ink))
                    .child(paragraph.clone()),
            );
        }
        for bullet in &item.bullets {
            view = view.child(
                div()
                    .text_size(px(style.typography.body.size_pt * 1.3))
                    .text_color(color(style.colors.ink))
                    .child(format!("· {bullet}")),
            );
        }
        if item.heading.is_empty()
            && item.details.is_empty()
            && item.paragraphs.is_empty()
            && item.bullets.is_empty()
        {
            view = view.child(
                div()
                    .text_size(px(11.0))
                    .text_color(color(style.colors.muted))
                    .child("新条目 · 可在 JSON 中填写内容"),
            );
        }
        view
    }

    fn item_insert_control(
        &self,
        id: &str,
        index: usize,
        style: &ThemeStyle,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let id = id.to_owned();
        let selector = format!("add-item-{id}-{index}");
        div()
            .id(gpui::SharedString::from(selector.clone()))
            .debug_selector(move || selector.clone())
            .w_full()
            .h(px(10.0))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .text_size(px(14.0))
            .text_color(color(style.colors.accent))
            .opacity(
                if self.selected_item_module.as_deref() == Some(id.as_str()) {
                    1.0
                } else {
                    0.0
                },
            )
            .hover(|control| control.opacity(1.0))
            .child("+")
            .on_click(cx.listener(move |this, _, window, cx| {
                this.apply_visual_structure(
                    StructureEdit::InsertItem {
                        id: id.clone(),
                        index,
                    },
                    window,
                    cx,
                );
            }))
    }

    fn module_insert_control(
        &self,
        before: Option<String>,
        style: &ThemeStyle,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let selector = format!("add-module-before-{}", before.as_deref().unwrap_or("end"));
        div()
            .id(gpui::SharedString::from(selector.clone()))
            .debug_selector(move || selector.clone())
            .w_full()
            .h(px(12.0))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .text_size(px(17.0))
            .text_color(color(style.colors.accent))
            .opacity(
                if self.selected_module.is_some() || self.insertion_before.as_ref() == Some(&before)
                {
                    1.0
                } else {
                    0.0
                },
            )
            .hover(|control| control.opacity(1.0))
            .child("+")
            .on_click(cx.listener(move |this, _, window, cx| {
                if let Some(id) = this.selected_module.clone() {
                    this.apply_visual_structure(
                        StructureEdit::InsertSection {
                            id,
                            before: before.clone(),
                        },
                        window,
                        cx,
                    );
                } else {
                    this.insertion_before = Some(before.clone());
                    cx.notify();
                }
            }))
    }

    fn section(
        &self,
        section: &Section,
        style: &ThemeStyle,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let hover_id = section.id.to_owned();
        let id = section.id.to_owned();
        let mut view = div().flex().flex_col().mt_5().gap_3().child(
            div()
                .id(gpui::SharedString::from(format!(
                    "section-title-{}",
                    section.id
                )))
                .relative()
                .pb_2()
                .border_b_1()
                .border_color(color(style.colors.border))
                .text_size(px(style.typography.section_title.size_pt * 1.3))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(color(style.colors.accent))
                .on_hover(cx.listener(move |this, hovering: &bool, _, cx| {
                    if *hovering {
                        this.hovered_section = Some(hover_id.clone());
                    } else if this.hovered_section.as_deref() == Some(hover_id.as_str()) {
                        this.hovered_section = None;
                    }
                    cx.notify();
                }))
                .child(label(section.id).to_string())
                .child(
                    div()
                        .id(gpui::SharedString::from(format!(
                            "remove-module-{}",
                            section.id
                        )))
                        .absolute()
                        .right_0()
                        .top_0()
                        .cursor_pointer()
                        .text_size(px(13.0))
                        .text_color(color(style.colors.seal))
                        .opacity(if self.hovered_section.as_deref() == Some(section.id) {
                            1.0
                        } else {
                            0.0
                        })
                        .child("−")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.apply_visual_structure(
                                StructureEdit::RemoveSection { id: id.clone() },
                                window,
                                cx,
                            );
                        })),
                ),
        );
        view = view.child(self.item_insert_control(section.id, 0, style, cx));
        for (index, item) in section.items.iter().enumerate() {
            view = view.child(self.item(item, section.id, index, style, cx));
            view = view.child(self.item_insert_control(section.id, index + 1, style, cx));
        }
        view
    }

    fn resume_page(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let style = self.active_style();
        let content = &self.model.content;
        let mm = 3.0;
        let mut page = div()
            .id("resume-paper")
            .debug_selector(|| "resume-paper".to_string())
            .w(px(style.page.width_mm * mm))
            .min_h(px(style.page.height_mm * mm))
            .flex_none()
            .flex()
            .flex_col()
            .bg(color(style.colors.paper))
            .shadow_lg()
            .pt(px(style.page.margins.top * mm))
            .pr(px(style.page.margins.right * mm))
            .pb(px(style.page.margins.bottom * mm))
            .pl(px(style.page.margins.left * mm))
            .child(
                div()
                    .text_size(px(style.typography.name.size_pt * 1.3))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(color(style.colors.accent))
                    .child(content.name.clone()),
            )
            .child(
                div()
                    .mt_2()
                    .text_size(px(style.typography.body.size_pt * 1.3))
                    .text_color(color(style.colors.muted))
                    .child(content.label.clone()),
            );
        if !content.contacts.is_empty() {
            page = page.child(
                div()
                    .mt_3()
                    .text_size(px(style.typography.meta.size_pt * 1.3))
                    .text_color(color(style.colors.muted))
                    .child(content.contacts.join("  ·  ")),
            );
        }
        if !content.summary.is_empty() {
            page = page.child(
                div()
                    .mt_6()
                    .text_size(px(style.typography.body.size_pt * 1.3))
                    .line_height(px(style.typography.body.size_pt
                        * 1.3
                        * style.typography.body.line_height))
                    .text_color(color(style.colors.ink))
                    .child(content.summary.clone()),
            );
        }
        let sections = self.model.document.sections();
        for summary in &sections {
            if summary.id == "basics" {
                continue;
            }
            page = page.child(self.module_insert_control(Some(summary.id.clone()), style, cx));
            if let Some(section) = content
                .sections
                .iter()
                .find(|section| section.id == summary.id)
            {
                page = page.child(self.section(section, style, cx));
            } else {
                let hover_id = summary.id.clone();
                let id = summary.id.clone();
                page = page.child(
                    div()
                        .id(gpui::SharedString::from(format!(
                            "empty-title-{}",
                            summary.id
                        )))
                        .relative()
                        .mt_5()
                        .text_size(px(style.typography.section_title.size_pt * 1.3))
                        .text_color(color(style.colors.accent))
                        .on_hover(cx.listener(move |this, hovering: &bool, _, cx| {
                            if *hovering {
                                this.hovered_section = Some(hover_id.clone());
                            } else if this.hovered_section.as_deref() == Some(hover_id.as_str()) {
                                this.hovered_section = None;
                            }
                            cx.notify();
                        }))
                        .child(label(&summary.id).to_owned())
                        .child(
                            div()
                                .id(gpui::SharedString::from(format!(
                                    "remove-empty-module-{id}"
                                )))
                                .absolute()
                                .right_0()
                                .top_0()
                                .cursor_pointer()
                                .text_size(px(12.0))
                                .text_color(color(style.colors.seal))
                                .opacity(
                                    if self.hovered_section.as_deref() == Some(summary.id.as_str())
                                    {
                                        1.0
                                    } else {
                                        0.0
                                    },
                                )
                                .child("−")
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.apply_visual_structure(
                                        StructureEdit::RemoveSection { id: id.clone() },
                                        window,
                                        cx,
                                    );
                                })),
                        ),
                );
                page = page.child(self.item_insert_control(&summary.id, 0, style, cx));
            }
        }
        page = page.child(self.module_insert_control(None, style, cx));
        page
    }

    fn source_module_add(
        &self,
        id: &str,
        present: bool,
        count: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let id = id.to_owned();
        let selector = format!("source-add-module-{id}");
        div()
            .id(gpui::SharedString::from(selector.clone()))
            .debug_selector(move || selector.clone())
            .cursor_pointer()
            .px_1()
            .text_size(px(15.0))
            .text_color(color(self.active_style().colors.accent))
            .child("+")
            .on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                let edit = if present {
                    StructureEdit::InsertItem {
                        id: id.clone(),
                        index: count,
                    }
                } else {
                    StructureEdit::InsertSection {
                        id: id.clone(),
                        before: None,
                    }
                };
                this.apply_visual_structure(edit, window, cx);
            }))
    }

    fn source_module_remove(&self, id: &str, cx: &mut Context<Self>) -> impl IntoElement {
        let id = id.to_owned();
        let selector = format!("source-remove-module-{id}");
        div()
            .id(gpui::SharedString::from(selector.clone()))
            .debug_selector(move || selector.clone())
            .cursor_pointer()
            .px_1()
            .text_size(px(15.0))
            .text_color(color(self.active_style().colors.seal))
            .child("−")
            .on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.apply_visual_structure(
                    StructureEdit::RemoveSection { id: id.clone() },
                    window,
                    cx,
                );
            }))
    }

    fn source_item_insert(
        &self,
        id: &str,
        index: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let id = id.to_owned();
        let selector = format!("source-add-item-{id}-{index}");
        div()
            .id(gpui::SharedString::from(selector.clone()))
            .debug_selector(move || selector.clone())
            .ml_4()
            .px_2()
            .py_1()
            .cursor_pointer()
            .text_size(px(10.0))
            .text_color(color(self.active_style().colors.accent))
            .child("+ 在此插入条目")
            .on_click(cx.listener(move |this, _, window, cx| {
                this.apply_visual_structure(
                    StructureEdit::InsertItem {
                        id: id.clone(),
                        index,
                    },
                    window,
                    cx,
                );
            }))
    }

    fn source_item_remove(
        &self,
        id: &str,
        index: usize,
        heading: &str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let id = id.to_owned();
        let selector = format!("source-remove-item-{id}-{index}");
        let title = if heading.is_empty() {
            format!("条目 {}", index + 1)
        } else {
            heading.to_owned()
        };
        div()
            .id(gpui::SharedString::from(selector.clone()))
            .debug_selector(move || selector.clone())
            .ml_4()
            .px_2()
            .py_1()
            .rounded_sm()
            .flex()
            .justify_between()
            .text_size(px(11.0))
            .text_color(rgb(0x68716a))
            .child(div().min_w_0().truncate().child(title))
            .child("−")
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, window, cx| {
                this.apply_visual_structure(
                    StructureEdit::RemoveItem {
                        id: id.clone(),
                        index,
                    },
                    window,
                    cx,
                );
            }))
    }

    fn module_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let accent = color(self.active_style().colors.accent);
        let mut list = div()
            .id("module-list")
            .flex()
            .flex_col()
            .gap_2()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll();
        let sections = if self.model.preview_ready {
            self.model.document.sections()
        } else {
            Vec::new()
        };
        for (id, title) in SECTION_LABELS {
            let existing = sections.iter().find(|section| section.id == *id);
            let present = existing.is_some();
            let count = existing.map_or(0, |section| section.count);
            let id = (*id).to_owned();
            let selector = format!("module-{id}");
            let selected = self.selected_module.as_deref() == Some(id.as_str())
                || self.selected_item_module.as_deref() == Some(id.as_str());
            let mut controls = div().flex().items_center().gap_2();
            if present {
                controls = controls.child(
                    div()
                        .text_size(px(10.0))
                        .text_color(accent)
                        .child(count.to_string()),
                );
            }
            if self.source_mode {
                if id != "basics" || !present {
                    controls = controls.child(self.source_module_add(&id, present, count, cx));
                }
                if present {
                    controls = controls.child(self.source_module_remove(&id, cx));
                }
            } else if !present {
                controls = controls.child(div().text_size(px(10.0)).text_color(accent).child("+"));
            }
            let source_mode = self.source_mode;
            let row_id = id.clone();
            list = list.child(
                div()
                    .id(gpui::SharedString::from(selector.clone()))
                    .debug_selector(move || selector.clone())
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .bg(if selected {
                        color(self.active_style().colors.canvas)
                    } else {
                        rgb(0xffffff)
                    })
                    .cursor_pointer()
                    .text_size(px(12.0))
                    .text_color(rgb(0x464e47))
                    .child((*title).to_string())
                    .child(controls)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if source_mode {
                            if present {
                                this.selected_item_module = if row_id == "basics"
                                    || this.selected_item_module.as_deref() == Some(row_id.as_str())
                                {
                                    None
                                } else {
                                    Some(row_id.clone())
                                };
                                cx.notify();
                            } else {
                                this.apply_visual_structure(
                                    StructureEdit::InsertSection {
                                        id: row_id.clone(),
                                        before: None,
                                    },
                                    window,
                                    cx,
                                );
                            }
                            return;
                        }
                        if this.model.document.value.get(&row_id).is_some() {
                            this.selected_item_module =
                                (row_id != "basics").then_some(row_id.clone());
                            this.selected_module = None;
                            this.insertion_before = None;
                            cx.notify();
                            return;
                        }
                        if let Some(before) = this.insertion_before.clone() {
                            this.apply_visual_structure(
                                StructureEdit::InsertSection {
                                    id: row_id.clone(),
                                    before,
                                },
                                window,
                                cx,
                            );
                        } else {
                            this.selected_module = Some(row_id.clone());
                            this.selected_item_module = None;
                            cx.notify();
                        }
                    })),
            );
            if self.source_mode
                && present
                && id != "basics"
                && self.selected_item_module.as_deref() == Some(id.as_str())
            {
                for index in 0..=count {
                    list = list.child(self.source_item_insert(&id, index, cx));
                    if index < count {
                        let heading = self
                            .model
                            .content
                            .sections
                            .iter()
                            .find(|section| section.id == id)
                            .and_then(|section| section.items.get(index))
                            .map_or("", |item| item.heading.as_str());
                        list = list.child(self.source_item_remove(&id, index, heading, cx));
                    }
                }
            }
        }
        div()
            .id("module-panel")
            .debug_selector(|| "module-panel".to_string())
            .w(px(188.0))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .border_l_1()
            .border_color(rgb(0xe4e7e1))
            .bg(rgb(0xf7f8f5))
            .px_3()
            .pt_5()
            .pb_4()
            .gap_4()
            .child(
                div()
                    .px_2()
                    .text_size(px(11.0))
                    .text_color(rgb(0x758078))
                    .child("内容模块"),
            )
            .child(list)
    }
}

impl Render for ResumeOne {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let style = self.active_style();
        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .track_focus(&self.focus)
            .key_context(if self.files.open {
                "ResumeOne FileSwitcher"
            } else {
                "ResumeOne"
            })
            .on_action(cx.listener(Self::open_file))
            .on_action(cx.listener(Self::open_directory))
            .on_action(cx.listener(Self::toggle_files))
            .on_action(cx.listener(Self::dismiss_files))
            .on_action(cx.listener(Self::next_file))
            .on_action(cx.listener(Self::previous_file))
            .on_action(cx.listener(Self::choose_file))
            .on_action(cx.listener(Self::refresh_folder))
            .on_action(cx.listener(Self::toggle_source))
            .on_action(cx.listener(Self::format_json))
            .on_action(cx.listener(Self::undo_visual_edit))
            .on_action(cx.listener(Self::redo_visual_edit))
            .on_action(cx.listener(Self::save_json))
            .on_action(cx.listener(Self::save_json_as))
            .on_action(cx.listener(Self::export_pdf))
            .on_action(cx.listener(Self::export_docx))
            .on_action(cx.listener(Self::export_text))
            .on_action(cx.listener(Self::export_markdown))
            .on_action(cx.listener(Self::choose_theme_directory))
            .on_action(cx.listener(Self::reset_theme_directory))
            .on_action(cx.listener(Self::quit))
            .bg(rgb(0xf5f5f1))
            .child(
                div()
                    .h(px(55.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_6()
                    .border_b_1()
                    .border_color(rgb(0xe4e7e1))
                    .bg(color(style.colors.chrome))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            this.dismiss_files(&files::DismissFiles, window, cx);
                        }),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_1()
                            .min_w_0()
                            .items_center()
                            .gap_4()
                            .child(
                                div()
                                    .text_size(px(15.0))
                                    .text_color(color(style.colors.accent))
                                    .child("简里  resume one"),
                            )
                            .child(
                                div()
                                    .id("current-file")
                                    .debug_selector(|| "current-file".into())
                                    .min_w_0()
                                    .truncate()
                                    .cursor_pointer()
                                    .px_2()
                                    .py_2()
                                    .rounded_md()
                                    .hover(|label| label.bg(color(style.colors.canvas)))
                                    .text_size(px(12.0))
                                    .text_color(color(style.colors.muted))
                                    .child(self.model.document_label.clone())
                                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                        cx.stop_propagation()
                                    })
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.toggle_files(&files::ToggleFiles, window, cx);
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .id("open-menu")
                            .debug_selector(|| "open-menu".into())
                            .flex_none()
                            .px_3()
                            .py_2()
                            .rounded_md()
                            .cursor_pointer()
                            .text_size(px(13.0))
                            .text_color(color(style.colors.accent))
                            .hover(|button| button.bg(color(style.colors.canvas)))
                            .child(if self.files.busy {
                                "打开中…"
                            } else {
                                "打开"
                            })
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.toggle_open_menu(window, cx);
                            })),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .when(!self.source_mode, |view| view.child(self.theme_sidebar(cx)))
                    .when(self.source_mode, |view| view.child(self.json_editor()))
                    .when(!self.source_mode, |view| {
                        view.child(
                            div()
                                .id("resume-preview")
                                .debug_selector(|| "resume-preview".to_string())
                                .flex_1()
                                .min_w_0()
                                .h_full()
                                .overflow_scroll()
                                .track_scroll(&self.preview_scroll)
                                .bg(color(style.colors.canvas))
                                .child(
                                    div()
                                        .w_full()
                                        .min_w(px(style.page.width_mm * 3.0 + 64.0))
                                        .py_8()
                                        .flex()
                                        .justify_center()
                                        .when(self.model.preview_ready, |view| {
                                            view.child(self.resume_page(cx))
                                        })
                                        .when(!self.model.preview_ready, |view| {
                                            view.child(
                                        div().w(px(style.page.width_mm * 3.0))
                                            .min_h(px(style.page.height_mm * 3.0))
                                            .flex().items_center().justify_center()
                                            .bg(color(style.colors.paper)).shadow_lg()
                                            .text_size(px(14.0))
                                            .text_color(color(style.colors.muted))
                                            .child("简历尚未通过校验，请切换到 JSON 编辑修复。"),
                                    )
                                        }),
                                ),
                        )
                    })
                    .child(self.module_sidebar(cx)),
            )
            .when(self.files.open, |view| view.child(self.file_switcher(cx)))
    }
}

fn main() {
    let path = std::env::args_os().nth(1);
    let mut model = match AppModel::load(path.as_deref().map(Path::new)) {
        Ok(model) => model,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(1);
        }
    };
    let mut theme_store = match theme_local::ThemeStore::system() {
        Ok(store) => store,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(1);
        }
    };
    match theme_store.load_themes() {
        Ok(themes) => model.replace_themes(themes),
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(1);
        }
    }
    Application::new().run(move |cx: &mut App| {
        gpui_component::init(cx);
        files::register(cx);
        editor_ui::register(cx);
        structure_ui::register(cx);
        let quit_target = Rc::new(RefCell::new(None::<gpui::WeakEntity<ResumeOne>>));
        let quit_flush_target = quit_target.clone();
        cx.on_app_quit(move |cx| {
            if let Some(view) = quit_flush_target
                .borrow()
                .as_ref()
                .and_then(|view| view.upgrade())
            {
                view.update(cx, |app, _| app.flush_draft_on_quit());
            }
            async {}
        })
        .detach();
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        let bounds = Bounds::centered(None, size(px(1120.0), px(780.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(800.0), px(600.0))),
                ..Default::default()
            },
            move |window, cx| {
                window.set_window_title("Resume One · 简里");
                let needs_repair = !model.preview_ready;
                let view = cx.new(|cx| {
                    let mut app = ResumeOne::new(model, cx);
                    app.theme_store = Some(theme_store);
                    app
                });
                window.focus(&view.read(cx).focus);
                *quit_target.borrow_mut() = Some(view.downgrade());
                let close_view = view.clone();
                window.on_window_should_close(cx, move |window, cx| {
                    close_view.update(cx, |app, cx| app.confirm_close(window, cx))
                });
                let root = cx.new(|cx| gpui_component::Root::new(view.clone(), window, cx));
                if needs_repair {
                    window.defer(cx, move |window, cx| {
                        view.update(cx, |app, cx| {
                            app.toggle_source(&editor_ui::ToggleSource, window, cx)
                        });
                    });
                }
                root
            },
        )
        .expect("failed to open Resume One window");
        cx.activate(true);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{
        EntityInputHandler as _, Modifiers, ScrollDelta, ScrollWheelEvent, TestAppContext, point,
    };
    use gpui_component::RopeExt;

    #[gpui::test]
    fn visual_structure_edits_share_json_draft_and_undo_without_touching_disk(
        cx: &mut TestAppContext,
    ) {
        cx.update(gpui_component::init);
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("structure.json");
        let disk = br#"{"basics":{"name":"A"},"work":[{"name":"B"}],"custom":{"keep":true}}"#;
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
        cx.update(|window, cx| {
            view.update(cx, |app, cx| {
                app.apply_visual_structure(
                    StructureEdit::InsertSection {
                        id: "skills".into(),
                        before: Some("work".into()),
                    },
                    window,
                    cx,
                );
            });
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            let app = view.read(cx);
            assert!(!app.source_mode);
            assert!(app.editor.as_ref().unwrap().buffer.is_dirty());
            assert_eq!(app.model.document.value["skills"], serde_json::json!([]));
            assert_eq!(app.model.document.value["custom"]["keep"], true);
            assert!(app.editor.as_ref().unwrap().store.load().unwrap().is_some());
        });
        assert_eq!(std::fs::read(&path).unwrap(), disk);
        cx.update(|window, cx| {
            view.update(cx, |app, cx| {
                app.undo_visual_edit(&structure_ui::UndoVisualEdit, window, cx)
            });
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            let app = view.read(cx);
            assert!(app.model.document.value.get("skills").is_none());
            assert!(!app.editor.as_ref().unwrap().buffer.is_dirty());
        });
        cx.update(|window, cx| {
            view.update(cx, |app, cx| {
                app.redo_visual_edit(&structure_ui::RedoVisualEdit, window, cx)
            });
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            view.update(cx, |app, cx| {
                app.toggle_source(&editor_ui::ToggleSource, window, cx)
            });
        });
        cx.update(|_, cx| {
            let app = view.read(cx);
            assert!(app.source_mode);
            let input = app.editor.as_ref().unwrap().input.read(cx);
            assert!(input.value().contains("\"skills\": []"));
        });
        cx.update(|window, cx| {
            let input = view.read(cx).editor.as_ref().unwrap().input.clone();
            input.update(cx, |state, cx| {
                let source = state
                    .value()
                    .replace("\"name\": \"A\"", "\"name\": \"Edited\"");
                let length = state.value().chars().map(char::len_utf16).sum();
                state.replace_text_in_range(Some(0..length), &source, window, cx);
            });
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            view.update(cx, |app, cx| {
                app.toggle_source(&editor_ui::ToggleSource, window, cx);
                app.undo_visual_edit(&structure_ui::UndoVisualEdit, window, cx);
            });
        });
        cx.update(|_, cx| {
            let app = view.read(cx);
            assert_eq!(app.model.content.name, "Edited");
            assert_eq!(app.model.document.value["skills"], serde_json::json!([]));
        });
    }

    #[gpui::test]
    fn sidebar_and_paper_insertions_keep_right_pane_fixed(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("structure-clicks.json");
        std::fs::write(&path, br#"{"basics":{"name":"A"},"work":[{"name":"B"}]}"#).unwrap();
        let model = AppModel::load(Some(&path)).unwrap();
        let (view, cx) = cx.add_window_view(|_, cx| ResumeOne::new(model, cx));
        cx.update(|window, _| window.refresh());
        let sidebar = cx.debug_bounds("module-panel").unwrap();
        let skill = cx.debug_bounds("module-skills").unwrap();
        cx.simulate_click(
            point(
                skill.origin.x + skill.size.width / 2.0,
                skill.origin.y + skill.size.height / 2.0,
            ),
            Modifiers::none(),
        );
        cx.update(|window, _| window.refresh());
        cx.update(|_, cx| assert_eq!(view.read(cx).selected_module.as_deref(), Some("skills")));
        let target = cx.debug_bounds("add-module-before-work").unwrap();
        cx.simulate_click(
            point(
                target.origin.x + target.size.width / 2.0,
                target.origin.y + target.size.height / 2.0,
            ),
            Modifiers::none(),
        );
        cx.run_until_parked();
        cx.update(|_, cx| {
            let app = view.read(cx);
            assert_eq!(app.model.document.value["skills"], serde_json::json!([]));
            let ids: Vec<_> = app
                .model
                .document
                .sections()
                .into_iter()
                .map(|s| s.id)
                .collect();
            assert_eq!(ids, ["basics", "skills", "work"]);
        });
        assert_eq!(sidebar, cx.debug_bounds("module-panel").unwrap());
        cx.update(|window, _| window.refresh());
        let skill = cx.debug_bounds("module-skills").unwrap();
        cx.simulate_click(
            point(
                skill.origin.x + skill.size.width / 2.0,
                skill.origin.y + skill.size.height / 2.0,
            ),
            Modifiers::none(),
        );
        cx.update(|window, _| window.refresh());
        let target = cx.debug_bounds("add-item-skills-0").unwrap();
        cx.simulate_click(
            point(
                target.origin.x + target.size.width / 2.0,
                target.origin.y + target.size.height / 2.0,
            ),
            Modifiers::none(),
        );
        cx.run_until_parked();
        cx.update(|_, cx| {
            let app = view.read(cx);
            assert_eq!(
                app.model.document.value["skills"].as_array().unwrap().len(),
                1
            );
        });
        assert_eq!(sidebar, cx.debug_bounds("module-panel").unwrap());
    }

    #[gpui::test]
    fn source_sidebar_edits_are_undoable_in_json_editor(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("source-structure.json");
        let disk = br#"{"basics":{"name":"\u6797\u6e05"},"work":[{"name":"B"}]}"#;
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
        cx.update(|window, cx| {
            view.update(cx, |app, cx| {
                app.toggle_source(&editor_ui::ToggleSource, window, cx)
            });
            window.refresh();
        });
        let right = cx.debug_bounds("module-panel").unwrap();
        let add = cx.debug_bounds("source-add-module-skills").unwrap();
        cx.simulate_click(
            point(
                add.origin.x + add.size.width / 2.0,
                add.origin.y + add.size.height / 2.0,
            ),
            Modifiers::none(),
        );
        cx.run_until_parked();
        cx.update(|_, cx| {
            let app = view.read(cx);
            assert_eq!(app.model.document.value["skills"], serde_json::json!([]));
            let input = app.editor.as_ref().unwrap().input.read(cx);
            assert!(input.value().contains("\"skills\""));
            assert_eq!(input.cursor_position().line, 0);
        });
        assert_eq!(right, cx.debug_bounds("module-panel").unwrap());
        cx.simulate_keystrokes("cmd-z");
        cx.run_until_parked();
        cx.update(|_, cx| {
            let app = view.read(cx);
            assert!(app.model.document.value.get("skills").is_none());
            assert!(!app.editor.as_ref().unwrap().buffer.is_dirty());
            let display = app
                .editor
                .as_ref()
                .unwrap()
                .input
                .read(cx)
                .value()
                .to_string();
            assert!(display.contains("林清"));
            assert!(!display.contains("\\u6797"));
        });
        cx.update(|window, _| window.refresh());
        let work = cx.debug_bounds("module-work").unwrap();
        cx.simulate_click(
            point(
                work.origin.x + work.size.width / 3.0,
                work.origin.y + work.size.height / 2.0,
            ),
            Modifiers::none(),
        );
        cx.update(|window, _| window.refresh());
        let remove = cx.debug_bounds("source-remove-item-work-0").unwrap();
        cx.simulate_click(
            point(
                remove.origin.x + remove.size.width / 2.0,
                remove.origin.y + remove.size.height / 2.0,
            ),
            Modifiers::none(),
        );
        cx.run_until_parked();
        cx.update(|_, cx| {
            assert_eq!(
                view.read(cx).model.document.value["work"],
                serde_json::json!([])
            )
        });
        cx.simulate_keystrokes("cmd-z");
        cx.run_until_parked();
        cx.update(|_, cx| {
            let app = view.read(cx);
            assert_eq!(app.model.document.value["work"][0]["name"], "B");
        });
        assert_eq!(std::fs::read(&path).unwrap(), disk);
        assert_eq!(right, cx.debug_bounds("module-panel").unwrap());
    }

    #[gpui::test]
    fn json_openers_pair_and_undo_as_one_edit(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("pair-test.json");
        let original_disk = br#"{"basics":{"name":"Pair Test"}}"#;
        std::fs::write(&path, original_disk).unwrap();
        let model = AppModel::load(Some(&path)).unwrap();
        let child = Rc::new(RefCell::new(None));
        let child_for_window = child.clone();
        let (_, cx) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| ResumeOne::new(model, cx));
            *child_for_window.borrow_mut() = Some(view.clone());
            gpui_component::Root::new(view, window, cx)
        });
        let view = child.borrow_mut().take().unwrap();
        cx.update(|window, cx| {
            view.update(cx, |app, cx| {
                app.toggle_source(&editor_ui::ToggleSource, window, cx)
            });
        });
        let input = cx.update(|_, cx| view.read(cx).editor.as_ref().unwrap().input.clone());
        let original = cx.update(|_, cx| input.read(cx).value().to_string());
        cx.update(|window, cx| {
            input.update(cx, |state, cx| {
                let end = state.text().offset_to_position(state.value().len());
                state.set_cursor_position(end, window, cx);
                state.insert("{", window, cx);
            });
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            let state = input.read(cx);
            assert_eq!(state.value().as_str(), format!("{original}{{}}").as_str());
            assert_eq!(state.cursor(), original.len() + 1);
            assert_eq!(
                view.read(cx).editor.as_ref().unwrap().buffer.source(),
                state.value().as_str()
            );
        });
        cx.simulate_keystrokes("cmd-z");
        cx.run_until_parked();
        cx.update(|_, cx| assert_eq!(input.read(cx).value().as_str(), original));
        cx.update(|window, cx| {
            input.update(cx, |state, cx| {
                state.set_value(
                    String::from_utf8(original_disk.to_vec()).unwrap(),
                    window,
                    cx,
                )
            });
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            let editor = view.read(cx).editor.as_ref().unwrap();
            assert!(!editor.buffer.is_dirty());
            assert!(editor.store.load().unwrap().is_none());
        });
    }

    #[gpui::test]
    fn source_mode_reclaims_left_pane_without_moving_right_pane(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let model = AppModel::load(None).unwrap();
        let child = Rc::new(RefCell::new(None));
        let child_for_window = child.clone();
        let (_, cx) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| ResumeOne::new(model, cx));
            *child_for_window.borrow_mut() = Some(view.clone());
            gpui_component::Root::new(view, window, cx)
        });
        let view = child.borrow_mut().take().unwrap();
        cx.update(|window, _| window.refresh());
        let module = cx.debug_bounds("module-panel").unwrap();
        assert!(cx.debug_bounds("theme-panel").is_some());

        cx.update(|window, cx| {
            view.update(cx, |app, cx| {
                app.toggle_source(&editor_ui::ToggleSource, window, cx)
            });
        });
        cx.update(|window, _| window.refresh());
        cx.update(|_, cx| assert!(view.read(cx).source_mode));
        let source = cx.debug_bounds("resume-json-editor").unwrap();
        assert_eq!(source.left(), px(0.0));
        assert_eq!(source.right(), module.left());
        assert_eq!(module, cx.debug_bounds("module-panel").unwrap());

        cx.update(|window, cx| {
            view.update(cx, |app, cx| {
                app.toggle_source(&editor_ui::ToggleSource, window, cx)
            });
        });
        cx.update(|window, _| window.refresh());
        cx.update(|_, cx| assert!(!view.read(cx).source_mode));
        let preview = cx.debug_bounds("resume-preview").unwrap();
        assert_eq!(preview.left(), px(206.0));
        assert_eq!(preview.right(), module.left());
        assert_eq!(module, cx.debug_bounds("module-panel").unwrap());
    }

    #[gpui::test]
    fn preview_window_renders_all_three_panes(cx: &mut TestAppContext) {
        let model = AppModel::load(None).unwrap();
        let (view, cx) = cx.add_window_view(|_, cx| ResumeOne::new(model, cx));
        cx.update(|window, _| window.refresh());
        let theme = cx.debug_bounds("theme-panel").unwrap();
        let preview = cx.debug_bounds("resume-preview").unwrap();
        let module = cx.debug_bounds("module-panel").unwrap();
        assert!(theme.right() <= preview.left());
        assert!(preview.right() <= module.left());
        let second = cx.debug_bounds("theme-1").unwrap();
        let location = point(
            second.origin.x + second.size.width / 2.0,
            second.origin.y + second.size.height / 2.0,
        );
        cx.simulate_mouse_move(location, None, Modifiers::none());
        cx.update(|_, cx| {
            let app = view.read(cx);
            assert_eq!(app.model.themes.len(), 20);
            assert_eq!(app.model.content.name, "林清和");
            assert_eq!(app.model.content.sections.len(), 3);
            assert_eq!(app.hovered_theme, Some(1));
            assert_eq!(app.model.selected_theme, 0);
        });
        let module_after_hover = cx.debug_bounds("module-panel").unwrap();
        assert_eq!(module, module_after_hover);
        cx.simulate_click(location, Modifiers::none());
        cx.update(|_, cx| {
            let app = view.read(cx);
            assert_eq!(app.model.selected_theme, 1);
        });
        let module_after_select = cx.debug_bounds("module-panel").unwrap();
        assert_eq!(module, module_after_select);
        cx.simulate_event(ScrollWheelEvent {
            position: location,
            delta: ScrollDelta::Pixels(point(px(0.0), px(-80.0))),
            ..Default::default()
        });
        cx.update(|_, cx| {
            assert_eq!(view.read(cx).hovered_theme, None);
            assert_eq!(view.read(cx).model.selected_theme, 1);
        });
        cx.simulate_mouse_move(
            point(
                preview.origin.x + preview.size.width / 2.0,
                preview.origin.y + preview.size.height / 2.0,
            ),
            None,
            Modifiers::none(),
        );
        cx.update(|_, cx| {
            let app = view.read(cx);
            assert_eq!(app.hovered_theme, None);
            assert_eq!(app.model.selected_theme, 1);
        });
        for index in 0..20 {
            view.update(cx, |app, cx| {
                app.model.select_theme(index);
                cx.notify();
            });
            cx.update(|window, _| window.refresh());
            let paper = cx.debug_bounds("resume-paper").unwrap();
            cx.update(|_, cx| {
                let app = view.read(cx);
                let expected = px(app.active_style().page.width_mm * 3.0);
                assert!(f32::from(paper.size.width - expected).abs() <= 1.0);
            });
            assert_eq!(module, cx.debug_bounds("module-panel").unwrap());
        }
        for width in [800.0, 1120.0, 1440.0] {
            cx.simulate_resize(size(px(width), px(600.0)));
            cx.update(|window, _| window.refresh());
            let preview = cx.debug_bounds("resume-preview").unwrap();
            let paper = cx.debug_bounds("resume-paper").unwrap();
            let modules = cx.debug_bounds("module-panel").unwrap();
            assert_eq!(modules.size.width, px(188.0));
            assert_eq!(modules.right(), px(width));
            assert!(paper.left() >= preview.left());
        }
    }
}
