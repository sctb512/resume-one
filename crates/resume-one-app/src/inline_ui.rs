use std::time::Duration;

use gpui::{
    Animation, AnimationExt, App, Context, Entity, Focusable as _, IntoElement, KeyBinding,
    ScrollHandle, Subscription, Window, actions, anchored, deferred, div, prelude::*, px,
};
use gpui_component::input::{Input, InputEvent, InputState};
use resume_one_core::content_edit::{
    ProfileChange, StringListChange, StringListEdit, TextEdit, list_fields, prepare_list_edit,
    prepare_profile_edit, prepare_text_edit, text_fields,
};

use crate::{ResumeOne, color, label};

actions!(resume_one, [OpenInline, DismissInline]);

#[derive(Default)]
pub(super) struct InlineUi {
    pub form: Option<InlineForm>,
    generation: u64,
    pub paper_hovered: bool,
    form_hovered: bool,
    scroll: ScrollHandle,
}

pub(super) struct InlineForm {
    pub section: String,
    pub index: Option<usize>,
    pub page: Option<usize>,
    fields: Vec<Field>,
    list_keys: &'static [&'static str],
    profile_count: usize,
    history_before: Option<String>,
    history_slot: usize,
    error: Option<String>,
}

struct Field {
    key: &'static str,
    list_index: Option<usize>,
    input: Entity<InputState>,
    _subscription: Subscription,
}

const PROFILE_FORM_FIELDS: &[&str] = &["profiles.network", "profiles.username", "profiles.url"];

pub(super) fn register(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("enter", OpenInline, Some("PreviewItem")),
        KeyBinding::new("escape", DismissInline, Some("ResumeOne")),
    ]);
}

fn field_label(key: &str) -> &str {
    match key {
        "name" => "名称",
        "label" => "职业标题",
        "image" => "头像地址",
        "email" => "邮箱",
        "phone" => "电话",
        "url" => "链接",
        "summary" => "概述",
        "location" => "地点",
        "description" => "描述",
        "position" => "职位",
        "startDate" => "开始日期",
        "endDate" => "结束日期",
        "organization" => "机构",
        "institution" => "学校",
        "area" => "专业",
        "studyType" => "学位",
        "score" => "成绩",
        "title" => "标题",
        "date" => "日期",
        "awarder" => "颁发机构",
        "issuer" => "发证机构",
        "publisher" => "出版方",
        "releaseDate" => "发表日期",
        "level" => "熟练程度",
        "language" => "语言",
        "fluency" => "掌握程度",
        "reference" => "推荐内容",
        "entity" => "所属机构",
        "type" => "类型",
        "highlights" => "成果要点",
        "keywords" => "关键词",
        "courses" => "课程",
        "roles" => "项目角色",
        "location.address" => "详细地址",
        "location.postalCode" => "邮政编码",
        "location.city" => "城市",
        "location.countryCode" => "国家代码",
        "location.region" => "省份 / 地区",
        "profiles.network" => "平台",
        "profiles.username" => "账号",
        "profiles.url" => "链接",
        _ => key,
    }
}

impl ResumeOne {
    pub(super) fn inline_matches(&self, section: &str, index: Option<usize>) -> bool {
        self.inline
            .form
            .as_ref()
            .is_some_and(|form| form.section == section && form.index == index)
    }

    pub(super) fn focus_inline_first(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(field) = self
            .inline
            .form
            .as_ref()
            .and_then(|form| form.fields.first())
        {
            field.input.update(cx, |state, cx| state.focus(window, cx));
        }
    }

    fn inline_focused(&self, window: &Window, cx: &App) -> bool {
        self.inline.form.as_ref().is_some_and(|form| {
            form.fields
                .iter()
                .any(|field| field.input.read(cx).focus_handle(cx).is_focused(window))
        })
    }

    pub(super) fn schedule_inline(
        &mut self,
        section: &str,
        index: Option<usize>,
        page: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.inline_matches(section, index) || self.inline_focused(window, cx) {
            return;
        }
        self.inline.generation += 1;
        let generation = self.inline.generation;
        let section = section.to_owned();
        let timer = cx.background_executor().timer(Duration::from_millis(180));
        cx.spawn_in(window, async move |this, cx| {
            timer.await;
            this.update_in(cx, |this, window, cx| {
                let still_hovered = match index {
                    Some(index) => this.hovered_item.as_ref() == Some(&(section.clone(), index)),
                    None => this.hovered_section.as_deref() == Some(section.as_str()),
                };
                if generation == this.inline.generation
                    && still_hovered
                    && !this.inline_focused(window, cx)
                {
                    this.open_inline(&section, index, window, cx);
                    if let Some(form) = &mut this.inline.form {
                        form.page = Some(page);
                    }
                }
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn open_inline(
        &mut self,
        section: &str,
        index: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.source_mode
            || !self.structure_enabled()
            || self.inline_matches(section, index)
            || !self.ensure_editor(window, cx)
            || self.recovery_pending
        {
            return;
        }
        // Preview-first users may never have toggled the JSON editor, where
        // the component palette used to be installed as an accidental side effect.
        crate::editor_ui::apply_editor_theme(self.model.selected_style(), cx);
        let editor = self.editor.as_ref().unwrap();
        let initial_error = editor
            .buffer
            .issues()
            .first()
            .map(|issue| format!("{} · {}", issue.path, issue.message));
        let Ok(value) = serde_json::from_str::<serde_json::Value>(editor.buffer.source()) else {
            return;
        };
        let Some(item) = value
            .get(section)
            .and_then(|value| index.map_or(Some(value), |index| value.get(index)))
        else {
            return;
        };
        if !item.is_object() {
            return;
        }
        self.inline.generation += 1;
        self.inline.scroll = ScrollHandle::new();
        let mut fields: Vec<_> = text_fields(section)
            .iter()
            .map(|&key| {
                let value = item
                    .pointer(&format!("/{}", key.replace('.', "/")))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
                    .to_owned();
                Self::make_inline_field(key, None, value, window, cx)
            })
            .collect();
        for &key in list_fields(section) {
            if let Some(values) = item.get(key).and_then(serde_json::Value::as_array) {
                for (list_index, value) in values.iter().enumerate() {
                    if let Some(value) = value.as_str() {
                        fields.push(Self::make_inline_field(
                            key,
                            Some(list_index),
                            value.into(),
                            window,
                            cx,
                        ));
                    }
                }
            }
        }
        let profiles = (section == "basics")
            .then(|| item.get("profiles").and_then(serde_json::Value::as_array))
            .flatten();
        for (profile_index, profile) in profiles.into_iter().flatten().enumerate() {
            for &key in PROFILE_FORM_FIELDS {
                let value = profile
                    .get(key.strip_prefix("profiles.").unwrap())
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("");
                fields.push(Self::make_inline_field(
                    key,
                    Some(profile_index),
                    value.into(),
                    window,
                    cx,
                ));
            }
        }
        self.inline.form = Some(InlineForm {
            section: section.into(),
            index,
            page: None,
            fields,
            list_keys: list_fields(section),
            profile_count: profiles.map_or(0, Vec::len),
            history_before: None,
            history_slot: self.visual_undo.len(),
            error: initial_error,
        });
        cx.notify();
    }

    fn make_inline_field(
        key: &'static str,
        list_index: Option<usize>,
        value: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Field {
        let input = cx.new(|cx| {
            let state = InputState::new(window, cx).default_value(value);
            if matches!(key, "summary" | "description" | "reference" | "highlights") {
                state.auto_grow(2, 5).soft_wrap(true)
            } else {
                state
            }
        });
        let subscription = cx.subscribe_in(
            &input,
            window,
            move |this, input, event, window, cx| match event {
                InputEvent::Change => this.inline_field_changed(key, input, window, cx),
                InputEvent::Blur => this.defer_inline_close(window, cx),
                _ => {}
            },
        );
        Field {
            key,
            list_index,
            input,
            _subscription: subscription,
        }
    }

    fn inline_field_changed(
        &mut self,
        field: &str,
        input: &Entity<InputState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(form) = &self.inline.form else {
            return;
        };
        let Some(candidate) = form
            .fields
            .iter()
            .find(|candidate| candidate.key == field && candidate.input == *input)
        else {
            return;
        };
        let before = self.editor.as_ref().unwrap().buffer.source().to_owned();
        let result = match candidate.list_index {
            Some(profile_index) if field.starts_with("profiles.") => prepare_profile_edit(
                before.as_bytes(),
                &ProfileChange::SetText {
                    index: profile_index,
                    field: field.strip_prefix("profiles.").unwrap().into(),
                    value: input.read(cx).value().to_string(),
                },
            ),
            Some(list_index) => prepare_list_edit(
                before.as_bytes(),
                &StringListEdit {
                    section: form.section.clone(),
                    index: form.index.expect("list in item"),
                    field: field.into(),
                    change: StringListChange::Set {
                        index: list_index,
                        value: input.read(cx).value().to_string(),
                    },
                },
            ),
            None => prepare_text_edit(
                before.as_bytes(),
                &TextEdit {
                    section: form.section.clone(),
                    index: form.index,
                    field: field.into(),
                    value: input.read(cx).value().to_string(),
                },
            ),
        };
        let after = match result {
            Ok(source) => String::from_utf8(source).expect("JSON is UTF-8"),
            Err(error) => {
                self.inline.form.as_mut().unwrap().error = Some(error.message);
                cx.notify();
                return;
            }
        };
        if before == after {
            return;
        }
        let form = self.inline.form.as_mut().unwrap();
        if form.history_before.is_none() {
            if self
                .visual_undo
                .last()
                .is_some_and(|(_, latest)| latest != &before)
            {
                self.visual_undo.clear();
            }
            form.history_before = Some(before.clone());
            form.history_slot = self.visual_undo.len();
            self.visual_undo.push((before, after.clone()));
        } else if let Some(entry) = self.visual_undo.get_mut(form.history_slot) {
            entry.1.clone_from(&after);
        }
        self.visual_redo.clear();
        self.replace_editor_source(&after, window, cx);
        self.sync_inline_error();
        cx.notify();
    }

    fn inline_list_structure(
        &mut self,
        key: &'static str,
        change: StringListChange,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(form) = &self.inline.form else {
            return;
        };
        let section = form.section.clone();
        let Some(index) = form.index else { return };
        let before = self.editor.as_ref().unwrap().buffer.source().to_owned();
        let focus_index = match change {
            StringListChange::Insert { index } => Some(index),
            _ => None,
        };
        let edit = StringListEdit {
            section: section.clone(),
            index,
            field: key.into(),
            change,
        };
        let after = match prepare_list_edit(before.as_bytes(), &edit) {
            Ok(source) => String::from_utf8(source).expect("JSON is UTF-8"),
            Err(error) => {
                self.inline.form.as_mut().unwrap().error = Some(error.message);
                cx.notify();
                return;
            }
        };
        self.record_inline_structure(
            before,
            after,
            focus_index.map(|index| (key, index)),
            window,
            cx,
        );
    }

    fn inline_profile_structure(
        &mut self,
        change: ProfileChange,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.inline_matches("basics", None) {
            return;
        }
        let before = self.editor.as_ref().unwrap().buffer.source().to_owned();
        let focus = match change {
            ProfileChange::Insert { index } => Some(("profiles.network", index)),
            _ => None,
        };
        let after = match prepare_profile_edit(before.as_bytes(), &change) {
            Ok(source) => String::from_utf8(source).expect("JSON is UTF-8"),
            Err(error) => {
                self.inline.form.as_mut().unwrap().error = Some(error.message);
                cx.notify();
                return;
            }
        };
        self.record_inline_structure(before, after, focus, window, cx);
    }

    fn record_inline_structure(
        &mut self,
        before: String,
        after: String,
        focus: Option<(&'static str, usize)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(form) = &self.inline.form else {
            return;
        };
        let section = form.section.clone();
        let index = form.index;
        let page = form.page;
        if self
            .visual_undo
            .last()
            .is_some_and(|(_, latest)| latest != &before)
        {
            self.visual_undo.clear();
        }
        self.visual_undo.push((before, after.clone()));
        self.visual_redo.clear();
        self.replace_editor_source(&after, window, cx);
        let owner = cx.entity().downgrade();
        let retained_hover = self.inline.form_hovered;
        let retained_offset = self.inline.scroll.offset();
        window.defer(cx, move |window, cx| {
            if let Some(owner) = owner.upgrade() {
                owner.update(cx, |this, cx| {
                    if !this.inline_matches(&section, index)
                        || this
                            .editor
                            .as_ref()
                            .is_none_or(|editor| editor.buffer.source() != after)
                    {
                        return;
                    }
                    this.clear_inline();
                    this.open_inline(&section, index, window, cx);
                    if let Some(form) = &mut this.inline.form {
                        form.page = page;
                    }
                    this.inline.scroll.set_offset(retained_offset);
                    this.inline.form_hovered = retained_hover;
                    if let Some((key, list_index)) = focus {
                        let input = this
                            .inline
                            .form
                            .as_ref()
                            .and_then(|form| {
                                form.fields.iter().find(|field| {
                                    field.key == key && field.list_index == Some(list_index)
                                })
                            })
                            .map(|field| field.input.clone());
                        if let Some(input) = input {
                            input.update(cx, |state, cx| state.focus(window, cx));
                        }
                    } else {
                        window.focus(&this.focus);
                    }
                });
            }
        });
        cx.notify();
    }

    pub(super) fn sync_inline_error(&mut self) {
        let error = self
            .editor
            .as_ref()
            .and_then(|editor| editor.buffer.issues().first())
            .map(|issue| format!("{} · {}", issue.path, issue.message));
        if let Some(form) = &mut self.inline.form {
            form.error = error;
        }
    }

    pub(super) fn close_inline_outside(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.inline.paper_hovered
            && !self.inline.form_hovered
            && !self.inline_focused(window, cx)
        {
            self.clear_inline();
            cx.notify();
        }
    }

    pub(super) fn defer_inline_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let owner = cx.entity().downgrade();
        window.defer(cx, move |window, cx| {
            if let Some(owner) = owner.upgrade() {
                owner.update(cx, |this, cx| this.close_inline_outside(window, cx));
            }
        });
    }

    pub(super) fn clear_inline(&mut self) {
        self.inline.form = None;
        self.inline.form_hovered = false;
        self.inline.generation += 1;
    }

    pub(super) fn dismiss_inline(
        &mut self,
        _: &DismissInline,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Escape is also bound at the workspace scope. Dismiss transient menus
        // first, regardless of registration order, before touching edit state.
        if self.export_menu_open {
            self.export_menu_open = false;
            cx.notify();
            return;
        }
        if self.files.open {
            self.files.open = false;
            cx.notify();
            return;
        }
        if self.drawer.take().is_some() {
            self.hovered_theme = None;
            self.online.cancel_interaction();
            self.theme_hover_generation += 1;
            self.theme_hover_candidate = None;
            self.cancel_insertion();
            cx.notify();
            return;
        }
        let insertion = self.cancel_insertion();
        if self.inline.form.take().is_some() || insertion {
            self.clear_inline();
            self.focus_current(window, cx);
            cx.notify();
        }
    }

    pub(super) fn inline_form(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let form = self.inline.form.as_ref().expect("active inline form");
        let style = self.active_style();
        let reduce_motion = self.reduce_motion;
        let width = (style.page.width_mm - style.page.margins.left - style.page.margins.right)
            * 3.0
            * self.preview_scale;
        let mut fields = div()
            .id("inline-fields")
            .debug_selector(|| "inline-fields".into())
            .flex()
            .flex_col()
            .gap(px(16.))
            .max_h(px((self.viewport_height - 220.).clamp(160., 440.)))
            .overflow_y_scroll()
            .track_scroll(&self.inline.scroll)
            .px(px(20.))
            .py(px(18.));
        let mut text_fields = div()
            .grid()
            .grid_cols(if width >= 440. { 2 } else { 1 })
            .gap(px(16.));
        for field in form
            .fields
            .iter()
            .filter(|field| field.list_index.is_none())
        {
            let selector = format!("inline-field-{}", field.key);
            text_fields = text_fields.child(
                div()
                    .min_w_0()
                    .when(
                        width >= 440.
                            && matches!(
                                field.key,
                                "summary" | "description" | "reference" | "image" | "url"
                            ),
                        |v| v.col_span(2),
                    )
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(color(style.colors.muted))
                            .child(field_label(field.key).to_owned()),
                    )
                    .child(self.inline_input(field, selector)),
            );
        }
        fields = fields.child(text_fields);
        for &key in form.list_keys {
            let rows: Vec<_> = form
                .fields
                .iter()
                .filter(|field| field.key == key && field.list_index.is_some())
                .collect();
            let mut list = div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(color(style.colors.muted))
                        .child(field_label(key).to_owned()),
                )
                .child(self.inline_list_add(key, 0, cx));
            for field in rows {
                let index = field.list_index.unwrap();
                let selector = format!("inline-list-input-{key}-{index}");
                let remove_selector = format!("inline-list-remove-{key}-{index}");
                list = list.child(
                    div()
                        .flex()
                        .items_start()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(self.inline_input(field, selector)),
                        )
                        .child(
                            div()
                                .id(gpui::SharedString::from(remove_selector.clone()))
                                .debug_selector(move || remove_selector.clone())
                                .cursor_pointer()
                                .px_2()
                                .py_1()
                                .text_color(color(style.colors.seal))
                                .child("−")
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    cx.stop_propagation();
                                    this.inline_list_structure(
                                        key,
                                        StringListChange::Remove { index },
                                        window,
                                        cx,
                                    );
                                })),
                        ),
                );
                list = list.child(self.inline_list_add(key, index + 1, cx));
            }
            fields = fields.child(list);
        }
        if form.section == "basics" {
            let mut profiles = div()
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(color(style.colors.muted))
                        .child("社交账号"),
                )
                .child(self.inline_profile_add(0, cx));
            for index in 0..form.profile_count {
                let selector = format!("inline-profile-remove-{index}");
                let mut profile = div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .rounded_md()
                    .border_1()
                    .border_color(color(style.colors.border))
                    .p_3()
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .items_center()
                            .text_size(px(12.0))
                            .text_color(color(style.colors.muted))
                            .child(format!("账号 {}", index + 1))
                            .child(
                                div()
                                    .id(gpui::SharedString::from(selector.clone()))
                                    .debug_selector(move || selector.clone())
                                    .cursor_pointer()
                                    .px_2()
                                    .py_1()
                                    .text_color(color(style.colors.seal))
                                    .child("−")
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        cx.stop_propagation();
                                        this.inline_profile_structure(
                                            ProfileChange::Remove { index },
                                            window,
                                            cx,
                                        );
                                    })),
                            ),
                    );
                for field in form.fields.iter().filter(|field| {
                    field.key.starts_with("profiles.") && field.list_index == Some(index)
                }) {
                    let selector = format!("inline-profile-input-{}-{index}", field.key);
                    profile = profile.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(px(12.0))
                                    .text_color(color(style.colors.muted))
                                    .child(field_label(field.key).to_owned()),
                            )
                            .child(self.inline_input(field, selector)),
                    );
                }
                profiles = profiles
                    .child(profile)
                    .child(self.inline_profile_add(index + 1, cx));
            }
            fields = fields.child(profiles);
        }
        deferred(
            anchored().snap_to_window_with_margin(px(64.0)).child(
                div()
                    .id("inline-form")
                    .debug_selector(|| "inline-form".into())
                    .occlude()
                    .w(px(width))
                    .flex()
                    .flex_col()
                    .rounded_md()
                    .shadow_md()
                    .border_1()
                    .border_color(color(crate::chrome::Palette::new(style).line))
                    .border_l(px(3.))
                    .border_color(color(style.colors.accent.mix(style.colors.paper, 0.55)))
                    .bg(color(crate::chrome::Palette::new(style).panel))
                    .on_hover(cx.listener(|this, hovering: &bool, window, cx| {
                        this.inline.form_hovered = *hovering;
                        if !hovering {
                            this.defer_inline_close(window, cx);
                        }
                    }))
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .items_center()
                            .px_4()
                            .py_3()
                            .border_b_1()
                            .border_color(color(style.colors.border))
                            .font_family(crate::chrome::SERIF)
                            .text_size(px(17.0))
                            .text_color(color(style.colors.accent))
                            .child(label(&form.section).to_owned())
                            .child(
                                div()
                                    .id("close-inline")
                                    .cursor_pointer()
                                    .px_2()
                                    .child("×")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.dismiss_inline(&DismissInline, window, cx)
                                    })),
                            ),
                    )
                    .child(fields)
                    .when_some(form.error.clone(), |view, message| {
                        view.child(
                            div()
                                .px_4()
                                .pb_3()
                                .text_size(px(12.0))
                                .text_color(color(style.colors.seal))
                                .child(message),
                        )
                    })
                    .with_animation(
                        gpui::SharedString::from(format!(
                            "inline-arrival-{}-{:?}",
                            form.section, form.index
                        )),
                        Animation::new(Duration::from_millis(160)),
                        move |v, t| {
                            let t = if reduce_motion {
                                1.
                            } else {
                                t * t * (3. - 2. * t)
                            };
                            v.relative().top(px(4. * (1. - t))).opacity(t)
                        },
                    ),
            ),
        )
        .priority(1)
    }

    fn inline_list_add(
        &self,
        key: &'static str,
        index: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let selector = format!("inline-list-add-{key}-{index}");
        div()
            .id(gpui::SharedString::from(selector.clone()))
            .debug_selector(move || selector.clone())
            .h(px(32.0))
            .w_full()
            .flex()
            .justify_center()
            .items_center()
            .cursor_pointer()
            .text_size(px(13.0))
            .text_color(color(self.active_style().colors.accent))
            .child("+")
            .on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.inline_list_structure(key, StringListChange::Insert { index }, window, cx);
            }))
    }

    fn inline_input(&self, field: &Field, selector: String) -> impl IntoElement {
        let input = field.input.clone();
        let scroll = self.inline.scroll.clone();
        div()
            .on_children_prepainted(move |children, window, cx| {
                if !input.read(cx).focus_handle(cx).is_focused(window) {
                    return;
                }
                let Some(bounds) = children.first().cloned() else {
                    return;
                };
                let viewport = scroll.bounds();
                let mut next = scroll.offset();
                if bounds.top() < viewport.top() + px(12.0) {
                    next.y += viewport.top() + px(12.0) - bounds.top();
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
            .w_full()
            .min_w_0()
            .child(Input::new(&field.input).w_full())
    }

    fn inline_profile_add(&self, index: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let selector = format!("inline-profile-add-{index}");
        div()
            .id(gpui::SharedString::from(selector.clone()))
            .debug_selector(move || selector.clone())
            .h(px(24.0))
            .flex()
            .justify_center()
            .items_center()
            .cursor_pointer()
            .text_size(px(13.0))
            .text_color(color(self.active_style().colors.accent))
            .child("+")
            .on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.inline_profile_structure(ProfileChange::Insert { index }, window, cx);
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{editor_ui, model::AppModel, structure_ui};
    use gpui::{EntityInputHandler as _, Modifiers, TestAppContext, point};
    use std::{cell::RefCell, rc::Rc};

    #[gpui::test]
    fn preview_first_forms_sync_palette_without_source_mode_or_content_changes(
        cx: &mut TestAppContext,
    ) {
        cx.update(gpui_component::init);
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("resume.json");
        let source = br#"{"work":[{"name":"Original","position":"Designer"}]}"#;
        std::fs::write(&path, source).unwrap();
        let mut model = AppModel::load(Some(&path)).unwrap();
        let mut dark = model.themes[0].clone();
        dark.manifest.normalized["colors"] = serde_json::json!({"accent":"#C9AC77","seal":"#A14F40","paper":"#172426","canvas":"#101A1B"});
        dark.style = resume_one_core::theme_style::ThemeStyle::from_manifest(&dark.manifest);
        dark.identity = "test:dark-form".into();
        model.themes.push(dark);
        let count = model.themes.len();
        let (view, cx) = cx.add_window_view(|_, cx| ResumeOne::new(model, cx));
        for index in 0..count {
            cx.update(|window, cx| {
                view.update(cx, |app, cx| {
                    app.clear_inline();
                    app.model.select_theme(index);
                    gpui_component::Theme::global_mut(cx).colors.background =
                        gpui::rgb(0x000000).into();
                    app.open_inline("work", Some(0), window, cx);
                    assert!(app.inline.form.is_some());
                    assert!(!app.source_mode);
                    let style = app.model.selected_style();
                    let colors = &gpui_component::Theme::global(cx).colors;
                    assert_eq!(
                        colors.background,
                        gpui::Hsla::from(color(style.colors.paper))
                    );
                    assert_eq!(colors.foreground, gpui::Hsla::from(color(style.colors.ink)));
                    assert_eq!(colors.input, gpui::Hsla::from(color(style.colors.border)));
                    assert_eq!(colors.caret, gpui::Hsla::from(color(style.colors.accent)));
                    assert!(!app.editor.as_ref().unwrap().buffer.is_dirty());
                })
            });
        }
        assert_eq!(std::fs::read(&path).unwrap(), source);
    }

    #[gpui::test]
    fn basics_nested_fields_and_profiles_keep_focus_visible_and_roundtrip_undo(
        cx: &mut TestAppContext,
    ) {
        cx.update(gpui_component::init);
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("basics.json");
        let disk = br#"{"basics":{"name":"A","location":{"city":"Old","extra":true},"profiles":[{"network":"GitHub","custom":9}]}}"#;
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
                app.inline.paper_hovered = true;
                app.open_inline("basics", None, window, cx);
            })
        });
        cx.run_until_parked();
        let city = cx.update(|_, cx| {
            view.read(cx)
                .inline
                .form
                .as_ref()
                .unwrap()
                .fields
                .iter()
                .find(|field| field.key == "location.city")
                .unwrap()
                .input
                .clone()
        });
        cx.update(|window, cx| {
            city.update(cx, |state, cx| {
                state.focus(window, cx);
                let length = state.value().chars().map(char::len_utf16).sum();
                state.replace_text_in_range(Some(0..length), "杭州", window, cx);
            })
        });
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
        let viewport = cx.debug_bounds("inline-fields").unwrap();
        let field = cx.debug_bounds("inline-field-location.city").unwrap();
        assert!(
            field.top() >= viewport.top() && field.bottom() <= viewport.bottom(),
            "focused city should scroll into the form viewport: field={field:?}, viewport={viewport:?}, offset={:?}",
            cx.update(|_, cx| view.read(cx).inline.scroll.offset())
        );
        cx.update(|window, cx| {
            view.update(cx, |app, cx| {
                assert_eq!(
                    app.model.document.value["basics"]["location"]["city"],
                    "杭州"
                );
                app.inline_profile_structure(ProfileChange::Insert { index: 1 }, window, cx);
            })
        });
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
        let viewport = cx.debug_bounds("inline-fields").unwrap();
        let field = cx
            .debug_bounds("inline-profile-input-profiles.network-1")
            .unwrap();
        assert!(
            field.top() >= viewport.top() && field.bottom() <= viewport.bottom(),
            "new profile input should be visible"
        );
        cx.simulate_input("GitLab");
        cx.run_until_parked();
        cx.update(|window, cx| {
            view.update(cx, |app, cx| {
                assert_eq!(
                    app.model.document.value["basics"]["profiles"][1]["network"],
                    "GitLab"
                );
                assert_eq!(
                    app.model.document.value["basics"]["profiles"][0]["custom"],
                    9
                );
                assert_eq!(
                    app.model.document.value["basics"]["location"]["extra"],
                    true
                );
                app.dismiss_inline(&DismissInline, window, cx);
            })
        });
        for _ in 0..3 {
            cx.update(|window, cx| {
                view.update(cx, |app, cx| {
                    app.undo_visual_edit(&structure_ui::UndoVisualEdit, window, cx)
                })
            });
            cx.run_until_parked();
        }
        cx.update(|_, cx| assert!(!view.read(cx).editor.as_ref().unwrap().buffer.is_dirty()));
        assert_eq!(std::fs::read(path).unwrap(), disk);
    }

    #[gpui::test]
    fn list_controls_keep_each_structural_edit_undoable(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        cx.update(structure_ui::register);
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("lists.json");
        let disk = br#"{"skills":[{"name":"Tools","keywords":["Rust"],"extra":true}]}"#;
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
                app.inline.paper_hovered = true;
                app.open_inline("skills", Some(0), window, cx);
            })
        });
        cx.run_until_parked();
        let right = cx.debug_bounds("module-panel").unwrap();
        let button = cx.debug_bounds("inline-list-add-keywords-1").unwrap();
        cx.simulate_click(
            point(button.left() + px(10.0), button.top() + px(10.0)),
            Modifiers::none(),
        );
        cx.run_until_parked();
        cx.update(|window, cx| {
            let app = view.read(cx);
            assert_eq!(
                app.model.document.value["skills"][0]["keywords"],
                serde_json::json!(["Rust", ""])
            );
            let input = &app
                .inline
                .form
                .as_ref()
                .unwrap()
                .fields
                .iter()
                .find(|field| field.key == "keywords" && field.list_index == Some(1))
                .unwrap()
                .input;
            assert!(input.read(cx).focus_handle(cx).is_focused(window));
        });
        cx.simulate_input("GPUI");
        cx.run_until_parked();
        cx.update(|window, cx| {
            view.update(cx, |app, cx| {
                assert_eq!(
                    app.model.document.value["skills"][0]["keywords"],
                    serde_json::json!(["Rust", "GPUI"])
                );
                assert_eq!(app.model.document.value["skills"][0]["extra"], true);
                app.dismiss_inline(&DismissInline, window, cx);
                app.undo_visual_edit(&structure_ui::UndoVisualEdit, window, cx);
            })
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            view.update(cx, |app, cx| {
                assert_eq!(
                    app.model.document.value["skills"][0]["keywords"],
                    serde_json::json!(["Rust", ""])
                );
                app.undo_visual_edit(&structure_ui::UndoVisualEdit, window, cx);
            })
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            view.update(cx, |app, cx| {
                assert!(!app.editor.as_ref().unwrap().buffer.is_dirty());
                app.redo_visual_edit(&structure_ui::RedoVisualEdit, window, cx);
            })
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            view.update(cx, |app, cx| {
                app.redo_visual_edit(&structure_ui::RedoVisualEdit, window, cx);
            })
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            view.update(cx, |app, cx| app.open_inline("skills", Some(0), window, cx))
        });
        cx.run_until_parked();
        let remove = cx.debug_bounds("inline-list-remove-keywords-0").unwrap();
        cx.simulate_click(
            point(remove.left() + px(4.0), remove.top() + px(4.0)),
            Modifiers::none(),
        );
        cx.run_until_parked();
        cx.update(|window, cx| {
            view.update(cx, |app, cx| {
                assert_eq!(
                    app.model.document.value["skills"][0]["keywords"],
                    serde_json::json!(["GPUI"])
                );
                assert!(app.focus.is_focused(window));
                cx.notify();
            })
        });
        cx.simulate_keystrokes("cmd-z");
        cx.run_until_parked();
        cx.update(|_, cx| {
            assert_eq!(
                view.read(cx).model.document.value["skills"][0]["keywords"],
                serde_json::json!(["Rust", "GPUI"])
            )
        });
        assert_eq!(right, cx.debug_bounds("module-panel").unwrap());
        assert_eq!(std::fs::read(path).unwrap(), disk);
    }

    #[gpui::test]
    fn inline_edits_keep_invalid_input_sync_source_and_undo_as_a_session(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        cx.update(register);
        let executor = cx.executor();
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("inline.json");
        let disk = br#"{"basics":{"name":"A","email":"a@example.com"},"work":[{"name":"B"}],"custom":{"keep":true}}"#;
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
        cx.update(|window, _| window.refresh());
        let paper_before = cx.debug_bounds("resume-paper").unwrap();
        let right_before = cx.debug_bounds("module-panel").unwrap();
        let header = cx.debug_bounds("preview-basics").unwrap();
        cx.simulate_mouse_move(
            point(header.left() + px(10.0), header.top() + px(10.0)),
            None,
            Modifiers::none(),
        );
        executor.advance_clock(Duration::from_millis(200));
        cx.run_until_parked();
        let form = cx.debug_bounds("inline-form").unwrap();
        cx.simulate_mouse_move(
            point(form.left() + px(15.0), form.top() + px(30.0)),
            None,
            Modifiers::none(),
        );
        cx.run_until_parked();
        cx.update(|_, cx| assert!(view.read(cx).inline.form.is_some()));
        cx.simulate_mouse_move(point(px(20.0), px(100.0)), None, Modifiers::none());
        cx.run_until_parked();
        cx.update(|_, cx| assert!(view.read(cx).inline.form.is_none()));
        cx.update(|window, cx| window.focus(&view.read(cx).focus));
        cx.simulate_keystrokes("tab tab tab tab enter");
        cx.update(|window, cx| assert!(view.read(cx).inline_focused(window, cx)));
        cx.simulate_keystrokes("escape");
        cx.update(|_, cx| assert!(view.read(cx).inline.form.is_none()));
        cx.simulate_click(
            point(header.left() + px(10.0), header.top() + px(10.0)),
            Modifiers::none(),
        );
        cx.run_until_parked();
        assert!(cx.debug_bounds("inline-form").is_some());
        assert_eq!(paper_before, cx.debug_bounds("resume-paper").unwrap());
        assert_eq!(right_before, cx.debug_bounds("module-panel").unwrap());
        let input = cx.update(|_, cx| {
            view.read(cx)
                .inline
                .form
                .as_ref()
                .unwrap()
                .fields
                .iter()
                .find(|field| field.key == "email")
                .unwrap()
                .input
                .clone()
        });
        cx.update(|window, cx| {
            input.update(cx, |state, cx| {
                state.focus(window, cx);
                let length = state.value().chars().map(char::len_utf16).sum();
                state.replace_text_in_range(Some(0..length), "unfinished", window, cx);
            })
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            view.update(cx, |app, cx| {
                assert!(
                    app.editor
                        .as_ref()
                        .unwrap()
                        .buffer
                        .source()
                        .contains("unfinished")
                );
                assert!(!app.editor.as_ref().unwrap().buffer.issues().is_empty());
                assert!(app.inline.form.as_ref().unwrap().error.is_some());
                assert_eq!(app.model.document.value["basics"]["email"], "a@example.com");
                app.inline.paper_hovered = false;
                app.close_inline_outside(window, cx);
                assert!(app.inline.form.is_some());
            });
        });
        cx.update(|window, cx| {
            input.update(cx, |state, cx| {
                let length = state.value().chars().map(char::len_utf16).sum();
                state.replace_text_in_range(Some(0..length), "new@example.com", window, cx);
            })
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            view.update(cx, |app, cx| {
                assert!(app.editor.as_ref().unwrap().buffer.issues().is_empty());
                assert_eq!(
                    app.model.document.value["basics"]["email"],
                    "new@example.com"
                );
                assert_eq!(app.model.document.value["custom"]["keep"], true);
                assert_eq!(app.visual_undo.len(), 1);
                app.dismiss_inline(&DismissInline, window, cx);
                app.undo_visual_edit(&structure_ui::UndoVisualEdit, window, cx);
            })
        });
        cx.run_until_parked();
        cx.update(|_, cx| assert!(!view.read(cx).editor.as_ref().unwrap().buffer.is_dirty()));
        cx.update(|window, cx| {
            view.update(cx, |app, cx| {
                app.redo_visual_edit(&structure_ui::RedoVisualEdit, window, cx);
                app.toggle_source(&editor_ui::ToggleSource, window, cx);
            })
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            let app = view.read(cx);
            assert!(
                app.editor
                    .as_ref()
                    .unwrap()
                    .input
                    .read(cx)
                    .value()
                    .contains("new@example.com")
            );
        });
        assert_eq!(std::fs::read(path).unwrap(), disk);
    }
}
