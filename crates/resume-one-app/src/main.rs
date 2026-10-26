#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod chrome;
mod editor_ui;
mod export_ui;
mod files;
mod inline_ui;
mod model;
mod motion;
mod preview_ui;
mod source_gutter;
mod structure_ui;
mod theme_browser_ui;
mod theme_local;
mod theme_manager;
mod theme_online;
mod workspace;
mod workspace_ui;

use std::path::Path;
use std::time::Instant;
use std::{cell::RefCell, rc::Rc};

use gpui::{
    App, Application, Bounds, Context, FocusHandle, IntoElement, MouseButton, Render, ScrollHandle,
    Window, WindowBounds, WindowOptions, div, prelude::*, px, rgb, size,
};
use model::AppModel;
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
    shell: chrome::Shell,
    window_drag_pending: bool,
    hovered_theme: Option<usize>,
    theme_hover_generation: u64,
    theme_hover_candidate: Option<String>,
    focus: FocusHandle,
    files: files::FileUi,
    preview_scroll: ScrollHandle,
    preview_scale: f32,
    source_mode: bool,
    layout: workspace_ui::Layout,
    drawer: Option<workspace_ui::Drawer>,
    reduce_motion: bool,
    viewport_height: f32,
    sidebar_progress: f32,
    view_blend: f32,
    view_blend_from: f32,
    mode_transition: Option<motion::ModeTransition>,
    spotlight_progress: f32,
    spotlight_target: f32,
    spotlight_transition: Option<motion::ModeTransition>,
    spotlight_dimmed: Vec<&'static str>,
    editor: Option<editor_ui::EditorUi>,
    export_busy: bool,
    export_menu_open: bool,
    export_choice: usize,
    export_language: resume_one_core::render::Language,
    theme_store: Option<theme_local::ThemeStore>,
    theme_manager_open: bool,
    online: theme_online::OnlineThemes,
    theme_scroll: ScrollHandle,
    hovered_section: Option<(String, usize)>,
    hovered_item: Option<(String, usize, usize)>,
    insertion_before: Option<Option<String>>,
    selected_module: Option<String>,
    selected_item_module: Option<String>,
    visual_undo: Vec<structure_ui::VisualEdit>,
    visual_redo: Vec<structure_ui::VisualEdit>,
    recovery_pending: bool,
    inline: inline_ui::InlineUi,
    preview: preview_ui::PreviewState,
}

impl ResumeOne {
    fn new(model: AppModel, cx: &mut Context<Self>) -> Self {
        let files = files::FileUi {
            open: model.folder.is_some(),
            ..Default::default()
        };
        Self {
            shell: chrome::Shell::new(model.selected_style()),
            window_drag_pending: false,
            model,
            hovered_theme: None,
            theme_hover_generation: 0,
            theme_hover_candidate: None,
            focus: cx.focus_handle(),
            files,
            preview_scroll: ScrollHandle::new(),
            preview_scale: 1.0,
            source_mode: false,
            layout: workspace_ui::Layout::Wide,
            drawer: None,
            reduce_motion: false,
            viewport_height: 820.,
            sidebar_progress: 0.0,
            view_blend: 0.0,
            view_blend_from: 0.0,
            mode_transition: None,
            spotlight_progress: 0.0,
            spotlight_target: 0.0,
            spotlight_transition: None,
            spotlight_dimmed: Vec::new(),
            editor: None,
            export_busy: false,
            export_menu_open: false,
            export_choice: 0,
            export_language: resume_one_core::render::Language::Zh,
            theme_store: None,
            theme_manager_open: false,
            online: theme_online::OnlineThemes::default(),
            theme_scroll: ScrollHandle::new(),
            hovered_section: None,
            hovered_item: None,
            insertion_before: None,
            selected_module: None,
            selected_item_module: None,
            visual_undo: Vec::new(),
            visual_redo: Vec::new(),
            recovery_pending: false,
            inline: inline_ui::InlineUi::default(),
            preview: preview_ui::PreviewState::new(cx),
        }
    }
    fn active_style(&self) -> &ThemeStyle {
        &self.display_theme().style
    }

    fn shell_style(&self) -> &ThemeStyle {
        &self.shell.style
    }

    fn display_theme(&self) -> &model::BuiltinTheme {
        if self.online.online
            && !self.source_mode
            && !self.theme_manager_open
            && let Some(theme) = &self.online.preview
        {
            return theme;
        }
        self.hovered_theme
            .and_then(|index| self.model.themes.get(index))
            .unwrap_or_else(|| self.model.applied_theme())
    }

    fn begin_mode_transition(&mut self) {
        self.theme_hover_generation += 1;
        self.theme_hover_candidate = None;
        let target = if self.source_mode { 1.0 } else { 0.0 };
        self.drawer = None;
        if self.reduce_motion {
            self.sidebar_progress = target;
            self.view_blend = target;
            self.mode_transition = None;
            return;
        }
        self.view_blend_from = self.view_blend;
        self.mode_transition = Some(if self.layout.theme_docked() {
            motion::ModeTransition::new(self.sidebar_progress, target, Instant::now())
        } else {
            motion::ModeTransition::crossfade(self.sidebar_progress, target, Instant::now())
        });
    }

    fn advance_mode_transition(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let Some(transition) = &self.mode_transition else {
            return;
        };
        let now = Instant::now();
        let (progress, complete) = transition.sample(now);
        self.sidebar_progress = progress;
        let target = if self.source_mode { 1.0 } else { 0.0 };
        let phase = transition.handoff(now);
        self.view_blend = self.view_blend_from + (target - self.view_blend_from) * phase;
        if complete {
            self.mode_transition = None;
        }
        cx.notify();
    }

    fn quit(&mut self, _: &files::Quit, window: &mut Window, cx: &mut Context<Self>) {
        if self.confirm_close(window, cx) {
            window.remove_window();
        }
    }

    fn theme_sidebar(&self, width: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = chrome::Palette::new(self.shell_style());
        let accent = color(palette.accent);
        let mut list = div()
            .id("theme-list")
            .flex()
            .flex_col()
            .px_2()
            .overflow_y_scroll()
            .flex_1()
            .on_scroll_wheel(cx.listener(|this, _, _, cx| {
                this.theme_hover_generation += 1;
                this.theme_hover_candidate = None;
                if this.hovered_theme.take().is_some() {
                    cx.notify();
                }
            }));
        for (index, theme) in self
            .model
            .themes
            .iter()
            .enumerate()
            .filter(|_| !self.online.online)
        {
            let selected =
                self.model.applied_online.is_none() && index == self.model.selected_theme;
            let row = div()
                .id(("theme", index))
                .debug_selector(|| format!("theme-{index}"))
                .flex()
                .flex_none()
                .relative()
                .h(px(chrome::THEME_ROW_HEIGHT))
                .items_center()
                .gap_3()
                .px(px(9.))
                .py(px(10.))
                .border_b_1()
                .border_color(color(palette.line))
                .cursor_pointer()
                .when(selected, |row| row.bg(color(palette.soft)))
                .when(!selected, |row| row.hover(|row| row.border_color(accent)))
                .when(selected, |row| {
                    row.child(
                        div()
                            .absolute()
                            .left_0()
                            .top(px(11.))
                            .bottom(px(11.))
                            .w(px(3.))
                            .rounded_sm()
                            .bg(accent),
                    )
                })
                .child(self.theme_thumbnail(Some(theme)))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(7.))
                        .child(
                            div()
                                .font_family(chrome::SERIF)
                                .text_size(px(15.0))
                                .truncate()
                                .text_color(if selected { accent } else { color(palette.ink) })
                                .child(theme.manifest.name.clone()),
                        )
                        .child(
                            div()
                                .text_size(px(12.0))
                                .line_height(px(17.))
                                .line_clamp(2)
                                .text_color(color(palette.muted))
                                .child(chrome::theme_description(
                                    theme.manifest.normalized["description"]
                                        .as_str()
                                        .unwrap_or(&theme.manifest.id),
                                )),
                        ),
                )
                .on_hover(cx.listener(move |this, hovering: &bool, window, cx| {
                    this.hover_local_theme(index, *hovering, window, cx);
                }))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.apply_theme_selection(this.model.themes[index].clone());
                    window.focus(&this.focus);
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
        if self.online.online {
            list = self.online_theme_list(cx);
        }
        div()
            .id("theme-panel")
            .debug_selector(|| "theme-panel".to_string())
            .w(px(width))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .overflow_hidden()
            .bg(color(palette.panel))
            .child(
                div()
                    .id("theme-track")
                    .debug_selector(|| "theme-track".into())
                    .relative()
                    .left(px(width - chrome::THEME_WIDTH))
                    .w(px(chrome::THEME_WIDTH))
                    .h_full()
                    .flex_none()
                    .flex()
                    .flex_col()
                    .border_r_1()
                    .border_color(color(palette.line))
                    .pb_3()
                    .child(
                        div()
                            .h(px(63.))
                            .flex_none()
                            .items_center()
                            .px(px(18.))
                            .flex()
                            .justify_between()
                            .child(
                                div()
                                    .font_family(chrome::SERIF)
                                    .text_size(px(18.))
                                    .text_color(color(palette.ink))
                                    .child("主题"),
                            )
                            .child(
                                div()
                                    .id("manage-themes")
                                    .debug_selector(|| "manage-themes".into())
                                    .cursor_pointer()
                                    .text_size(px(12.))
                                    .text_color(accent)
                                    .py_2()
                                    .child("管理")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.manage_themes(&theme_manager::ManageThemes, window, cx)
                                    })),
                            ),
                    )
                    .child(self.theme_navigation(cx))
                    .child(list),
            )
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
                    this.clear_inline();
                    this.hovered_theme = None;
                    this.selected_item_module = None;
                    this.insertion_before = Some(before.clone());
                    window.focus(&this.focus);
                    cx.notify();
                }
            }))
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
            .w(px(32.))
            .h(px(32.))
            .flex()
            .items_center()
            .justify_center()
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
                        before: this.insertion_before.clone().flatten(),
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
            .w(px(32.))
            .h(px(32.))
            .flex()
            .items_center()
            .justify_center()
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
            .min_h(px(32.))
            .cursor_pointer()
            .text_size(px(12.0))
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
            .min_h(px(32.))
            .rounded_sm()
            .flex()
            .justify_between()
            .text_size(px(12.0))
            .text_color(color(chrome::Palette::new(self.active_style()).muted))
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

    fn preview_surface(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let style = self.active_style();
        div()
            .id("resume-preview")
            .debug_selector(|| "resume-preview".to_string())
            .w_full()
            .h_full()
            .overflow_scroll()
            .track_scroll(&self.preview_scroll)
            .bg(color(self.shell_style().colors.canvas))
            .when(!self.recovery_pending && !self.structure_enabled() && self.model.preview_ready, |view| {
                let p = chrome::Palette::new(style);
                view.child(div().id("stale-preview").debug_selector(|| "stale-preview".into())
                    .mx_4().mt_3().px_4().py_2().rounded_md()
                    .bg(color(p.panel)).text_size(px(12.)).text_color(color(p.ink))
                    .child("JSON 尚有错误 · 当前显示上次有效内容，只读。请按 ⌘/ 或 Ctrl+/ 返回修复。"))
            })
            .child(
                div()
                    .w_full()
                    .min_w(px(style.page.width_mm * 3.0 * self.preview_scale + 44.0))
                    .pt(px(24.))
                    .pb(px(58.))
                    .flex()
                    .justify_center()
                    .when(self.model.preview_ready, |view| {
                        view.child(self.resume_pages(cx))
                    })
                    .when(!self.model.preview_ready, |view| {
                        view.child(
                            div()
                                .w(px(style.page.width_mm * 3.0))
                                .min_h(px(style.page.height_mm * 3.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .bg(color(style.colors.paper))
                                .shadow_lg()
                                .text_size(px(14.0))
                                .text_color(color(style.colors.muted))
                                .child("简历尚未通过校验，请切换到 JSON 编辑修复。"),
                        )
                    }),
            )
    }

    fn module_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = chrome::Palette::new(self.shell_style());
        let accent = color(palette.accent);
        let enabled = self.structure_enabled();
        let mut list = div()
            .id("module-list")
            .flex()
            .flex_col()
            .px(px(17.))
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
            let offered = self
                .model
                .applied_theme()
                .manifest
                .modules
                .iter()
                .any(|module| module == id);
            if !present && !offered {
                continue;
            }
            let count = existing.map_or(0, |section| section.count);
            let icon_id = *id;
            let id = (*id).to_owned();
            let selector = format!("module-{id}");
            let selected = self.selected_module.as_deref() == Some(id.as_str())
                || self.selected_item_module.as_deref() == Some(id.as_str());
            let dimmed = self.spotlight_dimmed.contains(&id.as_str());
            let mut controls = div().flex().items_center().gap_1();
            if present {
                controls = controls.child(
                    div()
                        .text_size(px(12.0))
                        .text_color(color(palette.muted))
                        .child(count.to_string()),
                );
            }
            if self.source_mode && enabled {
                if id != "basics" || !present {
                    controls = controls.child(self.source_module_add(&id, present, count, cx));
                }
                if present {
                    controls = controls.child(self.source_module_remove(&id, cx));
                }
            } else if !present && enabled {
                controls = controls.child(div().text_size(px(17.0)).text_color(accent).child("+"));
            }
            let source_mode = self.source_mode;
            let row_id = id.clone();
            list = list.child(
                div()
                    .id(gpui::SharedString::from(selector.clone()))
                    .debug_selector(move || selector.clone())
                    .flex()
                    .min_h(px(58.))
                    .flex_none()
                    .gap(px(10.))
                    .items_center()
                    .justify_between()
                    .px(px(4.))
                    .border_t_1()
                    .border_color(color(palette.line))
                    .opacity(if dimmed {
                        1.0 - 0.76 * self.spotlight_progress
                    } else {
                        1.0
                    })
                    .when(selected, |row| row.bg(color(palette.soft)))
                    .hover(|row| row.bg(color(palette.soft)))
                    .cursor_pointer()
                    .when(!enabled, |v| v.opacity(0.5).cursor_default())
                    .text_size(px(13.0))
                    .text_color(color(palette.ink))
                    .child(chrome::icon(icon_id, palette.accent))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child((*title).to_string()),
                    )
                    .child(controls)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if !this.structure_enabled() {
                            return;
                        }
                        // Source-mode rows expand their item tools in place;
                        // keep that drawer open so these tools remain usable.
                        if !source_mode || !present {
                            this.drawer = None;
                        }
                        if source_mode {
                            if present {
                                if this.insertion_before.is_some() {
                                    return;
                                }
                                this.selected_module = None;
                                this.selected_item_module = if row_id == "basics"
                                    || this.selected_item_module.as_deref() == Some(row_id.as_str())
                                {
                                    None
                                } else {
                                    Some(row_id.clone())
                                };
                                cx.notify();
                            } else if let Some(before) = this.insertion_before.clone() {
                                this.apply_visual_structure(
                                    StructureEdit::InsertSection {
                                        id: row_id.clone(),
                                        before,
                                    },
                                    window,
                                    cx,
                                );
                            } else {
                                this.selected_module = (this.selected_module.as_deref()
                                    != Some(row_id.as_str()))
                                .then(|| row_id.clone());
                                this.selected_item_module = None;
                                window.focus(&this.focus);
                                cx.notify();
                            }
                            return;
                        }
                        if this.model.document.value.get(&row_id).is_some() {
                            if this.insertion_before.is_some() {
                                return;
                            }
                            this.clear_inline();
                            this.hovered_theme = None;
                            this.selected_item_module = (row_id != "basics"
                                && this.selected_item_module.as_deref() != Some(&row_id))
                            .then_some(row_id.clone());
                            this.selected_module = None;
                            this.insertion_before = None;
                            window.focus(&this.focus);
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
                            this.clear_inline();
                            this.hovered_theme = None;
                            this.selected_module = (this.selected_module.as_deref()
                                != Some(&row_id))
                            .then_some(row_id.clone());
                            this.selected_item_module = None;
                            window.focus(&this.focus);
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
            .w(px(chrome::MODULE_WIDTH))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .border_l_1()
            .border_color(color(palette.line))
            .bg(color(palette.panel))
            .child(
                div()
                    .h(px(63.))
                    .flex_none()
                    .px(px(18.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        div()
                            .font_family(chrome::SERIF)
                            .text_size(px(18.))
                            .text_color(color(palette.ink))
                            .child("模块"),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(px(12.))
                            .text_color(color(palette.muted))
                            .child(self.model.applied_theme().manifest.name.clone()),
                    ),
            )
            .child(list)
            .child(
                div()
                    .h(px(47.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .px_4()
                    .border_t_1()
                    .border_color(color(palette.line))
                    .text_size(px(12.))
                    .text_color(color(palette.muted))
                    .child(if self.recovery_pending {
                        "请先恢复或丢弃草稿，再编辑模块"
                    } else if !enabled {
                        "请先修复 JSON，结构操作暂不可用"
                    } else if self.insertion_before.is_some() {
                        "选择要添加的模块 · Esc 取消"
                    } else if self.selected_module.is_some() || self.selected_item_module.is_some()
                    {
                        "选择文档中的插入位置 · Esc 取消"
                    } else {
                        "点击文档中的＋，或先选模块"
                    }),
            )
    }
}

impl Render for ResumeOne {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.viewport_height = f32::from(window.viewport_size().height);
        let layout = workspace_ui::Layout::for_width(f32::from(window.viewport_size().width));
        if self.layout != layout {
            self.layout = layout;
            self.drawer = None;
            self.hovered_theme = None;
            self.online.cancel_interaction();
            self.theme_hover_generation += 1;
            self.theme_hover_candidate = None;
            self.mode_transition = None;
            self.sidebar_progress = if self.source_mode { 1. } else { 0. };
            self.view_blend = self.sidebar_progress;
        }
        self.reduce_motion = motion::system_reduces_motion();
        if self.reduce_motion {
            self.mode_transition = None;
            self.sidebar_progress = if self.source_mode { 1. } else { 0. };
            self.view_blend = self.sidebar_progress;
        }
        let shell_target = self.active_style().colors;
        if self
            .shell
            .advance(shell_target, Instant::now(), self.reduce_motion)
        {
            cx.on_next_frame(window, |_, _, cx| cx.notify());
            window.request_animation_frame();
        }
        if self.insertion_before.is_some() && !self.layout.modules_docked() {
            self.drawer = Some(workspace_ui::Drawer::Modules);
        }
        // A screen transform only: PDF geometry and line breaks stay unchanged.
        // Use the preview rails' resting widths, so mode animation never resizes glyphs.
        self.preview_scale =
            ((f32::from(window.viewport_size().width) - self.layout.reserved_width() - 44.)
                / (self.active_style().page.width_mm * 3.))
                .clamp(0.4, 1.2);
        let spotlight_target = if self.insertion_active() { 1.0 } else { 0.0 };
        if self.reduce_motion {
            self.spotlight_progress = spotlight_target;
            self.spotlight_transition = None;
            self.spotlight_target = spotlight_target;
        }
        if self.spotlight_target != spotlight_target {
            self.spotlight_target = spotlight_target;
            self.spotlight_transition = Some(motion::ModeTransition::with_duration(
                self.spotlight_progress,
                spotlight_target,
                Instant::now(),
                std::time::Duration::from_millis(140),
            ));
        }
        if let Some(transition) = &self.spotlight_transition {
            let (progress, complete) = transition.sample(Instant::now());
            self.spotlight_progress = progress;
            if complete {
                self.spotlight_transition = None;
            } else {
                cx.on_next_frame(window, |_, _, cx| cx.notify());
                window.request_animation_frame();
            }
        }
        if self.insertion_active() {
            self.spotlight_dimmed = SECTION_LABELS
                .iter()
                .filter_map(|&(id, _)| {
                    let dim = if self.insertion_before.is_some() {
                        self.model.document.value.get(id).is_some()
                    } else {
                        self.selected_module.as_deref() != Some(id)
                            && self.selected_item_module.as_deref() != Some(id)
                    };
                    dim.then_some(id)
                })
                .collect();
        } else if self.spotlight_progress == 0.0 {
            self.spotlight_dimmed.clear();
        }
        if self.mode_transition.is_some() {
            cx.on_next_frame(window, |this, window, cx| {
                this.advance_mode_transition(window, cx)
            });
            window.request_animation_frame();
        }
        let dialogs = gpui_component::Root::render_dialog_layer(window, cx);
        let progress = self.sidebar_progress;
        let preview_alpha = 1.0 - self.view_blend;
        let json_alpha = self.view_blend;
        let style = self.shell_style();
        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .track_focus(&self.focus)
            .font_family(chrome::SANS)
            .text_size(px(13.))
            .text_color(color(chrome::Palette::new(style).ink))
            .key_context(if self.export_menu_open {
                "ResumeOne ExportMenu"
            } else if self.files.open {
                "ResumeOne FileSwitcher"
            } else {
                "ResumeOne"
            })
            .on_action(cx.listener(Self::open_file))
            .on_action(cx.listener(Self::open_directory))
            .on_action(cx.listener(Self::new_resume))
            .on_action(cx.listener(Self::toggle_files))
            .on_action(cx.listener(Self::dismiss_files))
            .on_action(cx.listener(Self::next_file))
            .on_action(cx.listener(Self::previous_file))
            .on_action(cx.listener(Self::choose_file))
            .on_action(cx.listener(Self::refresh_folder))
            .on_action(cx.listener(Self::toggle_source))
            .on_action(cx.listener(Self::dismiss_inline))
            .on_action(cx.listener(Self::format_json))
            .on_action(cx.listener(Self::undo_visual_edit))
            .on_action(cx.listener(Self::redo_visual_edit))
            .on_action(cx.listener(Self::save_json))
            .on_action(cx.listener(Self::save_json_as))
            .on_action(cx.listener(Self::export_pdf))
            .on_action(cx.listener(Self::export_docx))
            .on_action(cx.listener(Self::export_text))
            .on_action(cx.listener(Self::export_markdown))
            .on_action(cx.listener(Self::dismiss_export))
            .on_action(cx.listener(Self::next_export))
            .on_action(cx.listener(Self::previous_export))
            .on_action(cx.listener(Self::choose_export))
            .on_action(cx.listener(Self::export_chinese))
            .on_action(cx.listener(Self::export_english))
            .on_action(cx.listener(Self::choose_theme_directory))
            .on_action(cx.listener(Self::reset_theme_directory))
            .on_action(cx.listener(Self::manage_themes))
            .on_action(cx.listener(Self::quit))
            .bg(color(style.colors.canvas))
            .child(self.toolbar(cx))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .when(
                        self.layout.theme_docked() && (progress < 1.0 || !self.source_mode),
                        |view| {
                            view.child(
                                self.theme_sidebar(motion::THEME_WIDTH * (1.0 - progress), cx),
                            )
                        },
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .flex()
                            .flex_col()
                            .when(self.recovery_pending, |surface| {
                                surface.child(self.recovery_notice(cx))
                            })
                            .child(
                                div()
                                    .relative()
                                    .flex_1()
                                    .min_h_0()
                                    .w_full()
                                    .overflow_hidden()
                                    .bg(color(style.colors.canvas))
                                    .when(progress < 1.0 || !self.source_mode, |surface| {
                                        surface.child(
                                            div()
                                                .absolute()
                                                .size_full()
                                                .opacity(preview_alpha)
                                                .child(self.preview_surface(cx)),
                                        )
                                    })
                                    .when(progress > 0.0 || self.source_mode, |surface| {
                                        surface.child(
                                            div()
                                                .absolute()
                                                .size_full()
                                                .flex()
                                                .opacity(json_alpha)
                                                .child(self.json_editor(cx)),
                                        )
                                    })
                                    // Invisible incoming layers must never intercept a
                                    // click while the outgoing view is still visible.
                                    .when(self.mode_transition.is_some(), |surface| {
                                        surface.child(div().absolute().size_full().occlude())
                                    }),
                            ),
                    )
                    .when(self.layout.modules_docked(), |view| {
                        view.child(self.module_sidebar(cx))
                    }),
            )
            .when(self.drawer.is_some(), |view| {
                view.child(self.drawer_overlay(cx))
            })
            .when(self.files.open, |view| view.child(self.file_switcher(cx)))
            .when(self.export_menu_open, |view| {
                view.child(self.export_popup(cx))
            })
            .children(dialogs)
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
    let restored_folder =
        path.is_none() && model.folder.is_none() && theme_store.last_folder().is_some();
    if restored_folder
        && let Some(folder) = theme_store
            .last_folder()
            .and_then(|path| workspace::LocalFolder::scan(path).ok())
    {
        model.folder = Some(folder);
    }
    if let Some(path) = model.document_path.as_ref()
        && let Err(message) = theme_store.remember_file(path)
    {
        theme_store
            .warnings
            .push(format!("无法记录最近文件：{message}"));
    }
    if path.as_deref().is_some_and(|path| Path::new(path).is_dir())
        && let Some(folder) = model.folder.as_ref()
        && let Err(message) = theme_store.remember_folder(&folder.path)
    {
        theme_store
            .warnings
            .push(format!("无法记录最近目录：{message}"));
    }
    Application::new().run(move |cx: &mut App| {
        gpui_component::init(cx);
        files::register(cx);
        export_ui::register(cx);
        editor_ui::register(cx);
        structure_ui::register(cx);
        inline_ui::register(cx);
        theme_manager::register(cx);
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
        let bounds = Bounds::centered(None, size(px(1280.0), px(820.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(720.0), px(480.0))),
                #[cfg(target_os = "macos")]
                titlebar: Some(gpui::TitlebarOptions {
                    appears_transparent: true,
                    traffic_light_position: Some(gpui::point(px(16.), px(23.))),
                    ..Default::default()
                }),
                ..Default::default()
            },
            move |window, cx| {
                window.set_window_title("Resume One · 简里");
                let needs_repair = !model.preview_ready;
                let view = cx.new(|cx| {
                    let mut app = ResumeOne::new(model, cx);
                    app.theme_store = Some(theme_store);
                    if restored_folder {
                        app.files.open = false;
                    }
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
    fn theme_hover_waits_and_does_not_apply_after_pointer_leaves(cx: &mut TestAppContext) {
        let (view, cx) =
            cx.add_window_view(|_, cx| ResumeOne::new(AppModel::load(None).unwrap(), cx));
        cx.update(|window, _| window.refresh());
        let row = cx.debug_bounds("theme-1").unwrap();
        cx.simulate_mouse_move(row.center(), None, Modifiers::none());
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(100));
        cx.run_until_parked();
        cx.update(|_, cx| assert!(view.read(cx).hovered_theme.is_none()));
        cx.simulate_mouse_move(point(px(700.), px(100.)), None, Modifiers::none());
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(100));
        cx.run_until_parked();
        cx.update(|_, cx| assert!(view.read(cx).hovered_theme.is_none()));
        cx.simulate_mouse_move(row.center(), None, Modifiers::none());
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(160));
        cx.run_until_parked();
        cx.update(|_, cx| assert_eq!(view.read(cx).hovered_theme, Some(1)));
    }

    #[gpui::test]
    fn every_mode_sample_clips_and_pushes_theme_track_without_moving_toolbar_or_right_rail(
        cx: &mut TestAppContext,
    ) {
        cx.update(gpui_component::init);
        let child = Rc::new(RefCell::new(None));
        let output = child.clone();
        let (_, cx) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| ResumeOne::new(AppModel::load(None).unwrap(), cx));
            *output.borrow_mut() = Some(view.clone());
            gpui_component::Root::new(view, window, cx)
        });
        let view = child.borrow_mut().take().unwrap();
        cx.update(|window, _| window.refresh());
        let toolbar = cx.debug_bounds("toolbar").unwrap();
        let module = cx.debug_bounds("module-panel").unwrap();
        cx.update(|window, cx| {
            view.update(cx, |app, cx| {
                app.toggle_source(&editor_ui::ToggleSource, window, cx)
            })
        });
        for direction in [true, false] {
            for frame in 0..=20 {
                let progress = if direction {
                    frame as f32 / 20.
                } else {
                    1. - frame as f32 / 20.
                };
                cx.update(|window, cx| {
                    view.update(cx, |app, cx| {
                        app.source_mode = direction;
                        app.sidebar_progress = progress;
                        app.mode_transition = None;
                        cx.notify();
                    });
                    window.refresh();
                });
                assert_eq!(toolbar, cx.debug_bounds("toolbar").unwrap());
                assert_eq!(module, cx.debug_bounds("module-panel").unwrap());
                if progress < 1. {
                    let track = cx.debug_bounds("theme-track").unwrap();
                    assert!((f32::from(track.left()) + chrome::THEME_WIDTH * progress).abs() <= 1.);
                    assert_eq!(track.size.width, px(chrome::THEME_WIDTH));
                }
            }
        }
    }

    #[gpui::test]
    fn insertion_spotlights_are_bidirectional_cancellable_and_non_mutating(
        cx: &mut TestAppContext,
    ) {
        cx.update(gpui_component::init);
        cx.update(inline_ui::register);
        let executor = cx.executor();
        let model = AppModel::load(None).unwrap();
        let source = model.document.source.clone();
        let (view, cx) = cx.add_window_view(|_, cx| ResumeOne::new(model, cx));
        cx.update(|window, _| window.refresh());
        let right = cx.debug_bounds("module-panel").unwrap();
        let add = cx.debug_bounds("add-module-before-work").unwrap();
        let add_work_item = cx.debug_bounds("add-item-work-0").unwrap();
        let add_education_item = cx.debug_bounds("add-item-education-0").unwrap();
        cx.simulate_click(add.center(), Modifiers::none());
        cx.update(|window, _| window.refresh());
        cx.update(|_, cx| {
            let app = view.read(cx);
            assert!(app.insertion_active());
            assert_eq!(app.insertion_before, Some(Some("work".into())));
            assert!(app.spotlight_dimmed.contains(&"work"));
            assert!(!app.spotlight_dimmed.contains(&"volunteer"));
            assert_eq!(app.model.document.source, source);
        });
        cx.simulate_click(add_work_item.center(), Modifiers::none());
        cx.update(|_, cx| assert_eq!(view.read(cx).model.document.source, source));
        let work = cx.debug_bounds("module-work").unwrap();
        cx.simulate_click(work.center(), Modifiers::none());
        cx.update(|_, cx| assert!(view.read(cx).insertion_before.is_some()));
        let basics = cx.debug_bounds("preview-basics").unwrap();
        cx.simulate_mouse_move(basics.center(), None, Modifiers::none());
        executor.advance_clock(std::time::Duration::from_millis(200));
        cx.run_until_parked();
        cx.update(|_, cx| assert!(view.read(cx).inline.form.is_none()));
        cx.simulate_keystrokes("escape");
        cx.update(|_, cx| assert!(!view.read(cx).insertion_active()));
        let volunteer = cx.debug_bounds("module-volunteer").unwrap();
        cx.simulate_click(volunteer.center(), Modifiers::none());
        cx.update(|window, _| window.refresh());
        cx.update(|_, cx| {
            let app = view.read(cx);
            assert_eq!(app.selected_module.as_deref(), Some("volunteer"));
            assert!(app.spotlight_dimmed.contains(&"work"));
            assert!(!app.spotlight_dimmed.contains(&"volunteer"));
        });
        cx.simulate_click(volunteer.center(), Modifiers::none());
        cx.update(|_, cx| assert!(!view.read(cx).insertion_active()));
        cx.simulate_click(work.center(), Modifiers::none());
        cx.update(|window, _| window.refresh());
        assert!(cx.debug_bounds("add-item-work-0").is_some());
        cx.simulate_click(add_education_item.center(), Modifiers::none());
        cx.update(|_, cx| assert_eq!(view.read(cx).model.document.source, source));
        cx.simulate_keystrokes("escape");
        cx.update(|_, cx| {
            let app = view.read(cx);
            assert!(!app.insertion_active());
            assert_eq!(app.model.document.source, source);
            assert!(app.visual_undo.is_empty());
        });
        assert_eq!(right, cx.debug_bounds("module-panel").unwrap());
    }

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
        let child = Rc::new(RefCell::new(None));
        let child_for_window = child.clone();
        let (_, cx) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| ResumeOne::new(model, cx));
            *child_for_window.borrow_mut() = Some(view.clone());
            gpui_component::Root::new(view, window, cx)
        });
        let view = child.borrow_mut().take().unwrap();
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
            assert!(app.inline_matches("skills", Some(0)));
        });
        assert!(cx.debug_bounds("inline-form").is_some());
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
                app.selected_module = Some("awards".into());
                app.toggle_source(&editor_ui::ToggleSource, window, cx);
                assert!(app.selected_module.is_none());
                assert!(app.insertion_before.is_none());
            });
        });
        cx.update(|window, cx| {
            view.update(cx, |app, cx| {
                app.sidebar_progress = 0.5;
                app.mode_transition = None;
                cx.notify();
            });
            window.refresh();
        });
        let middle_theme = cx.debug_bounds("theme-panel").unwrap();
        assert_eq!(middle_theme.size.width, px(chrome::THEME_WIDTH / 2.));
        assert_eq!(module, cx.debug_bounds("module-panel").unwrap());
        cx.update(|window, cx| {
            view.update(cx, |app, cx| {
                app.sidebar_progress = 1.0;
                cx.notify();
            });
            window.refresh();
        });
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
        cx.update(|window, cx| {
            view.update(cx, |app, cx| {
                app.sidebar_progress = 0.5;
                app.mode_transition = None;
                cx.notify();
            });
            window.refresh();
        });
        let middle_theme = cx.debug_bounds("theme-panel").unwrap();
        assert_eq!(middle_theme.size.width, px(chrome::THEME_WIDTH / 2.));
        assert_eq!(module, cx.debug_bounds("module-panel").unwrap());
        cx.update(|window, cx| {
            view.update(cx, |app, cx| {
                app.sidebar_progress = 0.0;
                cx.notify();
            });
            window.refresh();
        });
        cx.update(|_, cx| assert!(!view.read(cx).source_mode));
        let preview = cx.debug_bounds("resume-preview").unwrap();
        assert_eq!(preview.left(), px(chrome::THEME_WIDTH));
        assert_eq!(preview.right(), module.left());
        assert_eq!(module, cx.debug_bounds("module-panel").unwrap());
    }

    #[gpui::test]
    fn preview_window_renders_all_three_panes(cx: &mut TestAppContext) {
        let model = AppModel::load(None).unwrap();
        let original_sections = model.content.sections.len();
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
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(160));
        cx.run_until_parked();
        cx.update(|_, cx| {
            let app = view.read(cx);
            assert_eq!(app.model.themes.len(), 20);
            assert_eq!(app.model.content.name, "林清和");
            assert_eq!(app.model.content.sections.len(), original_sections);
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
                let expected = px(app.active_style().page.width_mm * 3.0 * app.preview_scale);
                assert!(f32::from(paper.size.width - expected).abs() <= 1.0);
            });
            assert_eq!(module, cx.debug_bounds("module-panel").unwrap());
        }
        for width in [800.0, 1120.0, 1440.0] {
            cx.simulate_resize(size(px(width), px(600.0)));
            cx.update(|window, _| window.refresh());
            let preview = cx.debug_bounds("resume-preview").unwrap();
            let paper = cx.debug_bounds("resume-paper").unwrap();
            if width >= 1024. {
                let modules = cx.debug_bounds("module-panel").unwrap();
                assert_eq!(modules.size.width, px(chrome::MODULE_WIDTH));
                assert_eq!(modules.right(), px(width));
            } else {
                assert!(cx.debug_bounds("show-modules").is_some());
            }
            assert!(paper.left() >= preview.left());
            assert!(paper.right() <= preview.right());
        }
    }

    #[gpui::test]
    fn module_choices_follow_applied_theme_and_preserve_existing_content(cx: &mut TestAppContext) {
        let mut model = AppModel::load(None).unwrap();
        let source = model.document.source.clone();
        model.themes[0].manifest.modules = vec!["basics".into(), "work".into()];
        let (view, cx) = cx.add_window_view(|_, cx| ResumeOne::new(model, cx));
        cx.update(|window, _| window.refresh());
        let right = cx.debug_bounds("module-panel").unwrap();
        assert!(cx.debug_bounds("module-volunteer").is_none());
        assert!(cx.debug_bounds("module-education").is_some());
        assert!(cx.debug_bounds("module-skills").is_some());
        let theme = cx.debug_bounds("theme-1").unwrap();
        let point = point(theme.left() + px(10.0), theme.top() + px(10.0));
        cx.simulate_mouse_move(point, None, Modifiers::none());
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(160));
        cx.run_until_parked();
        cx.update(|_, cx| assert_eq!(view.read(cx).hovered_theme, Some(1)));
        assert!(cx.debug_bounds("module-volunteer").is_none());
        cx.update(|_, cx| view.update(cx, |app, _| app.selected_module = Some("awards".into())));
        cx.simulate_click(point, Modifiers::none());
        cx.update(|_, cx| {
            let app = view.read(cx);
            assert_eq!(app.model.selected_theme, 1);
            assert!(app.selected_module.is_none());
            assert_eq!(app.model.document.source, source);
        });
        assert!(cx.debug_bounds("module-volunteer").is_some());
        assert_eq!(right, cx.debug_bounds("module-panel").unwrap());
    }
}
