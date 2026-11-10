use std::{
    cell::Cell,
    collections::HashMap,
    rc::Rc,
    time::{Duration, Instant},
};

use gpui::{
    App, Bounds, Context, Entity, EntityInputHandler as _, Focusable as _, IntoElement, KeyBinding,
    Pixels, Point, ScrollHandle, Subscription, Window, actions, anchored, deferred, div,
    prelude::*, px,
};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::scroll::{Scrollbar, ScrollbarShow};
use resume_one_core::content_edit::{
    ProfileChange, StringListChange, StringListEdit, TextEdit, list_fields, prepare_list_edit,
    prepare_profile_edit, prepare_text_edit, text_fields,
};

use crate::motion::Visibility;
use crate::{ResumeOne, SECTION_LABELS, color, label};

actions!(resume_one, [OpenInline, DismissInline]);

#[derive(Default)]
pub(super) struct InlineUi {
    pub form: Option<InlineForm>,
    generation: u64,
    form_epoch: u64,
    pub paper_hovered: Option<usize>,
    form_hovered: bool,
    closing: bool,
    closing_explicit: bool,
    suppressed: Option<(String, Option<usize>)>,
    bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    visibility: Visibility,
    retiring: Option<RetiringForm>,
    candidate: Option<HoverCandidate>,
    scroll_until: Option<Instant>,
    controls: HashMap<String, ControlVisibility>,
    scroll: ScrollHandle,
}

const INLINE_HOVER_DELAY: Duration = Duration::from_millis(140);
const INLINE_SWITCH_DELAY: Duration = Duration::from_millis(85);
const INLINE_FADE_IN: Duration = Duration::from_millis(160);
const INLINE_FADE_OUT: Duration = Duration::from_millis(120);
const INLINE_HANDOFF: Duration = Duration::from_millis(90);

struct HoverCandidate {
    section: String,
    index: Option<usize>,
    page: usize,
    started: Instant,
    delay: Duration,
}

struct RetiringForm {
    form: InlineForm,
    scroll: ScrollHandle,
    visibility: Visibility,
}

#[derive(Default)]
struct ControlVisibility {
    hovered: bool,
    visibility: Visibility,
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

#[derive(Clone, Copy)]
struct FormPalette {
    surface: resume_one_core::theme_style::Rgb,
    field: resume_one_core::theme_style::Rgb,
    focus: resume_one_core::theme_style::Rgb,
    line: resume_one_core::theme_style::Rgb,
    ink: resume_one_core::theme_style::Rgb,
    title: resume_one_core::theme_style::Rgb,
    muted: resume_one_core::theme_style::Rgb,
    accent: resume_one_core::theme_style::Rgb,
}

impl FormPalette {
    fn new(style: &resume_one_core::theme_style::ThemeStyle) -> Self {
        let c = style.colors;
        let surface = c.paper.mix(c.canvas, 0.08);
        let field = surface.mix(c.ink, 0.025);
        let focus = surface.mix(c.accent, 0.035);
        let ink = crate::chrome::readable_on(c.ink, field);
        Self {
            surface,
            field,
            focus,
            line: surface.mix(c.ink, 0.13),
            ink,
            title: crate::chrome::readable_on(c.ink, surface),
            muted: crate::chrome::readable_on(c.muted, surface),
            accent: crate::chrome::readable_on(c.accent, focus),
        }
    }
}

pub(super) fn field_order(section: &str, key: &str) -> usize {
    let order: &[&str] = match section {
        "basics" => &[
            "name",
            "label",
            "email",
            "phone",
            "url",
            "summary",
            "location.address",
            "location.city",
            "location.region",
            "location.postalCode",
            "location.countryCode",
            "image",
        ],
        "work" => &[
            "name",
            "position",
            "startDate",
            "endDate",
            "location",
            "url",
            "summary",
            "description",
        ],
        "education" => &[
            "institution",
            "area",
            "studyType",
            "startDate",
            "endDate",
            "score",
            "url",
        ],
        "volunteer" => &[
            "organization",
            "position",
            "startDate",
            "endDate",
            "url",
            "summary",
        ],
        "projects" => &[
            "name",
            "entity",
            "type",
            "startDate",
            "endDate",
            "url",
            "description",
        ],
        "certificates" => &["name", "issuer", "date", "url"],
        "awards" => &["title", "awarder", "date", "summary"],
        _ => text_fields(section),
    };
    order
        .iter()
        .position(|field| *field == key)
        .unwrap_or(usize::MAX)
}

fn full_width_field(section: &str, key: &str) -> bool {
    matches!(
        key,
        "summary"
            | "description"
            | "reference"
            | "image"
            | "url"
            | "location"
            | "location.address"
            | "score"
            | "institution"
            | "title"
    ) || key == "name"
        && matches!(
            section,
            "projects" | "certificates" | "publications" | "references" | "interests"
        )
}

pub(super) fn form_field_label(section: &str, key: &str) -> &'static str {
    match (section, key) {
        ("basics", "name") => "姓名",
        ("work", "name") => "公司 / 团队",
        ("education", "institution") => "学校",
        _ => field_label(key),
    }
}

const PROFILE_FORM_FIELDS: &[&str] = &["profiles.network", "profiles.username", "profiles.url"];

pub(super) fn register(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("enter", OpenInline, Some("PreviewItem")),
        KeyBinding::new("escape", DismissInline, Some("ResumeOne")),
    ]);
}

fn field_label(key: &str) -> &'static str {
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
        _ => "内容",
    }
}

impl ResumeOne {
    pub(super) fn inline_form_geometry(&self) -> (f32, f32) {
        let style = self.active_style();
        let paper = style.page.width_mm * 3. * self.preview_scale;
        let left = style.page.margins.left * 3. * self.preview_scale;
        let content = (style.page.width_mm - style.page.margins.left - style.page.margins.right)
            * 3.
            * self.preview_scale;
        let width = (content + 64.).min(paper - 28.);
        let inset = (left - (width - content) / 2.).clamp(14., (paper - width - 14.).max(14.));
        (width, inset)
    }

    pub(super) fn cancel_inline_candidate(&mut self) {
        self.inline.candidate = None;
        self.inline.generation += 1;
    }

    pub(super) fn inline_region_left(&mut self, section: &str, index: Option<usize>, page: usize) {
        if self
            .inline
            .suppressed
            .as_ref()
            .is_some_and(|target| target.0 == section && target.1 == index)
        {
            self.inline.suppressed = None;
        }
        if self.inline.candidate.as_ref().is_some_and(|candidate| {
            candidate.section == section && candidate.index == index && candidate.page == page
        }) {
            self.cancel_inline_candidate();
        }
    }

    pub(super) fn preview_scrolled(&mut self, cx: &mut Context<Self>) {
        self.cancel_inline_candidate();
        self.hovered_item = None;
        self.hovered_section = None;
        self.inline.controls.clear();
        self.inline.scroll_until =
            Some(cx.background_executor().now() + Duration::from_millis(100));
        cx.notify();
    }

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

    pub(super) fn inline_composing(&self, cx: &App) -> bool {
        self.inline.form.as_ref().is_some_and(|form| {
            form.fields
                .iter()
                .any(|field| field.input.read(cx).is_composing())
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
        let now = cx.background_executor().now();
        if self
            .inline
            .suppressed
            .as_ref()
            .is_some_and(|target| target.0 == section && target.1 == index)
        {
            return;
        }
        self.inline.suppressed = None;
        if self.inline_focused(window, cx)
            || self.inline.form_hovered
            || self.inline.scroll_until.is_some_and(|until| now < until)
        {
            return;
        }
        if self.inline_matches(section, index) {
            self.cancel_inline_candidate();
            self.resume_inline(cx);
            if let Some(form) = &mut self.inline.form
                && form.page != Some(page)
            {
                form.page = Some(page);
                cx.notify();
            }
            return;
        }
        if let Some(candidate) = &self.inline.candidate
            && candidate.section == section
            && candidate.index == index
            && candidate.page == page
        {
            if now.saturating_duration_since(candidate.started) >= candidate.delay {
                self.open_inline(section, index, window, cx);
                if let Some(form) = &mut self.inline.form {
                    form.page = Some(page);
                }
            }
            return;
        }
        self.inline.generation += 1;
        let generation = self.inline.generation;
        let section = section.to_owned();
        let delay = if self.inline.form.is_some() {
            INLINE_SWITCH_DELAY
        } else {
            INLINE_HOVER_DELAY
        };
        self.inline.candidate = Some(HoverCandidate {
            section: section.clone(),
            index,
            page,
            started: now,
            delay,
        });
        let timer = cx.background_executor().timer(delay);
        cx.spawn_in(window, async move |this, cx| {
            timer.await;
            this.update_in(cx, |this, window, cx| {
                let still_hovered = match index {
                    Some(index) => {
                        this.hovered_item.as_ref() == Some(&(section.clone(), index, page))
                    }
                    None => this.hovered_section.as_ref() == Some(&(section.clone(), page)),
                };
                if generation == this.inline.generation
                    && still_hovered
                    && !this.inline.form_hovered
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
        if self.source_mode || !self.structure_enabled() || self.recovery_pending {
            return;
        }
        self.inline.suppressed = None;
        self.inline.closing_explicit = false;
        if self.inline_matches(section, index) {
            self.cancel_inline_candidate();
            self.resume_inline(cx);
            return;
        }
        if !self.ensure_editor(window, cx) || self.recovery_pending {
            return;
        }
        // Preview-first users may never have toggled the JSON editor, where
        // the component palette used to be installed as an accidental side effect.
        crate::editor_ui::apply_editor_theme(&self.model.selected_style(), cx);
        let editor = self.editor.as_ref().unwrap();
        let initial_error = editor
            .buffer
            .issues()
            .first()
            .map(|issue| format!("{} · {}", issue.path, issue.message));
        let Some(item) = editor
            .buffer
            .last_valid()
            .and_then(|document| document.value.get(section))
            .and_then(|value| index.map_or(Some(value), |index| value.get(index)))
            .cloned()
        else {
            return;
        };
        if !item.is_object() {
            return;
        }
        self.cancel_inline_candidate();
        let now = cx.background_executor().now();
        let replacing = self.inline.form.is_some();
        self.inline.retiring =
            self.inline
                .form
                .take()
                .filter(|_| !self.reduce_motion)
                .map(|form| {
                    let mut visibility = self.inline.visibility.clone();
                    visibility.retarget(0., now, INLINE_HANDOFF, false);
                    RetiringForm {
                        form,
                        scroll: self.inline.scroll.clone(),
                        visibility,
                    }
                });
        self.inline.form_epoch += 1;
        self.inline.form_hovered = false;
        self.inline.closing = false;
        self.inline.bounds.set(None);
        self.inline.visibility.snap(0.);
        self.inline.visibility.retarget(
            1.,
            now,
            if replacing {
                INLINE_HANDOFF
            } else {
                INLINE_FADE_IN
            },
            self.reduce_motion,
        );
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
        let subscription =
            cx.subscribe_in(&input, window, move |this, input, event, window, cx| {
                if let InputEvent::Change = event {
                    this.inline_field_changed(key, input, window, cx);
                }
            });
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
                .iter()
                .rev()
                .find_map(crate::structure_ui::VisualEdit::content_after)
                .is_some_and(|latest| latest != before)
            {
                self.visual_undo.clear();
            }
            form.history_before = Some(before.clone());
            form.history_slot = self.visual_undo.len();
            self.visual_undo
                .push(crate::structure_ui::VisualEdit::Content {
                    before,
                    after: after.clone(),
                });
        } else if let Some(crate::structure_ui::VisualEdit::Content { after: latest, .. }) =
            self.visual_undo.get_mut(form.history_slot)
        {
            latest.clone_from(&after);
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
            .iter()
            .rev()
            .find_map(crate::structure_ui::VisualEdit::content_after)
            .is_some_and(|latest| latest != before)
        {
            self.visual_undo.clear();
        }
        self.visual_undo
            .push(crate::structure_ui::VisualEdit::Content {
                before,
                after: after.clone(),
            });
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
                    // Structural edits rebuild fields but are not a new hover.
                    this.inline.visibility.snap(1.);
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

    pub(super) fn paper_hover_changed(
        &mut self,
        page: usize,
        hovering: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        if hovering {
            self.inline.paper_hovered = Some(page);
        } else if self.inline.paper_hovered == Some(page) {
            self.inline.paper_hovered = None;
            self.cancel_inline_candidate();
            self.inline.suppressed = None;
        }
    }

    pub(super) fn form_hover_changed(
        &mut self,
        form_epoch: u64,
        hovering: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if form_epoch != self.inline.form_epoch {
            return;
        }
        self.inline.form_hovered = hovering;
        if hovering {
            self.cancel_inline_candidate();
            self.inline.suppressed = None;
            self.resume_inline(cx);
        } else {
            self.close_inline_outside(window, cx);
        }
    }

    pub(super) fn inline_pointer_events(&self, cx: &Context<Self>) -> impl IntoElement {
        let view = cx.entity().downgrade();
        // Global pointer listeners belong to paint, not Render/request_layout.
        // This transparent canvas inserts no hitbox and consumes no layout space.
        gpui::canvas(
            |_, _, _| (),
            move |_, (), window, _| {
                let exit_view = view.clone();
                window.on_mouse_event(move |event: &gpui::MouseMoveEvent, phase, window, cx| {
                    if phase == gpui::DispatchPhase::Capture {
                        view.update(cx, |app, cx| {
                            app.inline_pointer_moved(event.position, window, cx)
                        })
                        .ok();
                    }
                });
                window.on_mouse_event(move |_: &gpui::MouseExitEvent, phase, window, cx| {
                    if phase == gpui::DispatchPhase::Capture {
                        exit_view
                            .update(cx, |app, cx| app.inline_window_left(window, cx))
                            .ok();
                    }
                });
            },
        )
        .absolute()
        .size_full()
    }

    pub(super) fn inline_pointer_moved(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(bounds) = self.inline.bounds.get() else {
            return;
        };
        if self.inline.form.is_none() {
            return;
        }
        let inside = bounds.contains(&position);
        if inside && self.inline.closing_explicit {
            return;
        }
        if self.inline.form_hovered != inside || (!inside && !self.inline.closing) {
            self.form_hover_changed(self.inline.form_epoch, inside, window, cx);
        }
    }

    pub(super) fn inline_window_left(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.cancel_inline_candidate();
        self.inline.form_hovered = false;
        self.close_inline_outside(window, cx);
    }

    pub(super) fn close_inline_outside(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.inline.form.is_some() && !self.inline.form_hovered && !self.inline.closing {
            self.cancel_inline_candidate();
            let form = self.inline.form.as_ref().unwrap();
            self.inline.suppressed = Some((form.section.clone(), form.index));
            // Preserve the visible text before dropping focus, including an IME
            // commit that has not yet reached the normal Change subscription.
            let pending: Vec<_> = form
                .fields
                .iter()
                .filter(|field| {
                    field.input.read(cx).is_composing()
                        || field.input.read(cx).focus_handle(cx).is_focused(window)
                })
                .map(|field| (field.key, field.input.clone()))
                .collect();
            for (key, input) in pending {
                input.update(cx, |state, cx| state.unmark_text(window, cx));
                self.inline_field_changed(key, &input, window, cx);
            }
            if self.inline_focused(window, cx) {
                window.focus(&self.focus);
            }
            if self.reduce_motion {
                let suppressed = self.inline.suppressed.clone();
                self.clear_inline();
                self.inline.suppressed = suppressed;
            } else {
                self.inline.closing = true;
                self.inline.visibility.retarget(
                    0.,
                    cx.background_executor().now(),
                    INLINE_FADE_OUT,
                    false,
                );
            }
            cx.notify();
        }
    }

    fn resume_inline(&mut self, cx: &mut Context<Self>) {
        if self.inline.form.is_some() && self.inline.closing && !self.inline.closing_explicit {
            self.inline.closing = false;
            self.inline.visibility.retarget(
                1.,
                cx.background_executor().now(),
                INLINE_FADE_IN,
                self.reduce_motion,
            );
            cx.notify();
        }
    }

    pub(super) fn advance_inline_motion(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let now = cx.background_executor().now();
        if self.reduce_motion {
            self.inline
                .visibility
                .snap(if self.inline.closing { 0. } else { 1. });
            self.inline.retiring = None;
            for control in self.inline.controls.values_mut() {
                control
                    .visibility
                    .snap(if control.hovered { 1. } else { 0. });
            }
        }
        let (alpha, done) = self.inline.visibility.sample(now);
        if self.inline.closing && done && alpha == 0. {
            let suppressed = self.inline.suppressed.clone();
            self.clear_inline();
            self.inline.suppressed = suppressed;
        }
        if self
            .inline
            .retiring
            .as_ref()
            .is_some_and(|old| old.visibility.sample(now).1)
        {
            self.inline.retiring = None;
        }
        self.inline
            .controls
            .retain(|_, control| control.hovered || !control.visibility.sample(now).1);
        let animating = self.inline.form.is_some() && !self.inline.visibility.sample(now).1
            || self.inline.retiring.is_some()
            || self
                .inline
                .controls
                .values()
                .any(|control| !control.visibility.sample(now).1);
        if animating {
            cx.on_next_frame(window, |_, _, cx| cx.notify());
            window.request_animation_frame();
        }
    }

    pub(super) fn control_hover(&mut self, key: &str, hovering: bool, cx: &mut Context<Self>) {
        let control = self.inline.controls.entry(key.into()).or_default();
        control.hovered = hovering;
        control.visibility.retarget(
            if hovering { 1. } else { 0. },
            cx.background_executor().now(),
            Duration::from_millis(if hovering { 65 } else { 85 }),
            self.reduce_motion,
        );
        cx.notify();
    }

    pub(super) fn control_alpha(&self, key: &str, cx: &App) -> f32 {
        self.inline.controls.get(key).map_or(0., |control| {
            control.visibility.sample(cx.background_executor().now()).0
        })
    }

    pub(super) fn control_hovered(&self, key: &str) -> bool {
        self.inline
            .controls
            .get(key)
            .is_some_and(|control| control.hovered)
    }

    pub(super) fn clear_inline(&mut self) {
        self.inline.form = None;
        self.inline.retiring = None;
        self.inline.form_hovered = false;
        self.inline.closing = false;
        self.inline.closing_explicit = false;
        self.inline.suppressed = None;
        self.inline.bounds.set(None);
        self.inline.visibility.snap(0.);
        self.inline.candidate = None;
        self.inline.controls.clear();
        self.inline.generation += 1;
        self.inline.form_epoch += 1;
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
        if self.files.loading {
            self.cancel_read(cx);
            self.focus_current(window, cx);
            return;
        }
        let insertion = self.cancel_insertion();
        if self.inline.form.is_some() {
            self.inline.form_hovered = false;
            self.inline.closing_explicit = true;
            self.close_inline_outside(window, cx);
            self.focus_current(window, cx);
            cx.notify();
        } else if insertion {
            cx.notify();
        } else if let Some(editor) = self.appearance_editor.clone() {
            if editor.update(cx, |editor, cx| editor.dismiss_transient(window, cx)) {
                return;
            }
            self.finish_appearance(window, cx);
        }
    }

    pub(super) fn inline_layer_target(&self, retiring: bool) -> Option<&InlineForm> {
        if retiring {
            self.inline.retiring.as_ref().map(|old| &old.form)
        } else {
            self.inline.form.as_ref()
        }
    }

    pub(super) fn inline_form(&self, retiring: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let form = self
            .inline_layer_target(retiring)
            .expect("inline layer exists");
        let style = self.active_style();
        let palette = FormPalette::new(&style);
        let bounds_slot = self.inline.bounds.clone();
        let subtitle = form
            .fields
            .iter()
            .find(|field| {
                matches!(
                    field.key,
                    "name" | "institution" | "organization" | "title" | "language"
                )
            })
            .map(|field| {
                field
                    .input
                    .read(cx)
                    .value()
                    .chars()
                    .take(42)
                    .collect::<String>()
            })
            .filter(|text| !text.trim().is_empty())
            .unwrap_or_else(|| {
                form.index
                    .map(|index| format!("第 {} 项", index + 1))
                    .unwrap_or_default()
            });
        let (scroll, alpha) = if retiring {
            let old = self.inline.retiring.as_ref().unwrap();
            (
                old.scroll.clone(),
                old.visibility.sample(cx.background_executor().now()).0,
            )
        } else {
            let alpha = self
                .inline
                .visibility
                .sample(cx.background_executor().now())
                .0;
            (self.inline.scroll.clone(), alpha)
        };
        let selector_for = |name: String| {
            if retiring {
                format!("retiring-{name}")
            } else {
                name
            }
        };
        let (width, _) = self.inline_form_geometry();
        let mut fields = div()
            .id(if retiring {
                "retiring-inline-fields"
            } else {
                "inline-fields"
            })
            .debug_selector(move || {
                if retiring {
                    "retiring-inline-fields".into()
                } else {
                    "inline-fields".into()
                }
            })
            .flex()
            .flex_col()
            .gap(px(18.))
            .max_h(px((self.viewport_height - 240.).clamp(120., 360.)))
            .overflow_y_scroll()
            .track_scroll(&scroll)
            .px(px(20.))
            .py(px(18.));
        let mut text_fields = div()
            .grid()
            .grid_cols(if width >= 440. { 2 } else { 1 })
            .gap_x(px(18.))
            .gap_y(px(14.));
        let mut ordered: Vec<_> = form
            .fields
            .iter()
            .filter(|field| field.list_index.is_none())
            .collect();
        ordered.sort_by_key(|field| field_order(&form.section, field.key));
        for field in ordered {
            let selector = selector_for(format!("inline-field-{}", field.key));
            text_fields = text_fields.child(
                div()
                    .min_w_0()
                    .when(
                        width >= 440. && full_width_field(&form.section, field.key),
                        |v| v.col_span(2),
                    )
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(color(palette.muted))
                            .child(form_field_label(&form.section, field.key).to_owned()),
                    )
                    .child(self.inline_input(field, selector, scroll.clone(), retiring, cx)),
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
                .gap(px(6.))
                .border_t_1()
                .border_color(color(palette.line))
                .pt_4()
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(color(palette.muted))
                        .child(field_label(key).to_owned()),
                )
                .child(self.inline_list_add(key, 0, retiring, cx));
            for field in rows {
                let index = field.list_index.unwrap();
                let selector = selector_for(format!("inline-list-input-{key}-{index}"));
                let remove_selector = selector_for(format!("inline-list-remove-{key}-{index}"));
                list = list.child(
                    div()
                        .flex()
                        .items_start()
                        .gap_2()
                        .child(div().flex_1().min_w_0().child(self.inline_input(
                            field,
                            selector,
                            scroll.clone(),
                            retiring,
                            cx,
                        )))
                        .child(
                            div()
                                .id(gpui::SharedString::from(remove_selector.clone()))
                                .debug_selector(move || remove_selector.clone())
                                .cursor_pointer()
                                .size(px(28.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(6.))
                                .hover(|v| v.bg(color(palette.focus)))
                                .text_color(color(palette.muted))
                                .child("−")
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    cx.stop_propagation();
                                    if retiring {
                                        return;
                                    }
                                    this.inline_list_structure(
                                        key,
                                        StringListChange::Remove { index },
                                        window,
                                        cx,
                                    );
                                })),
                        ),
                );
                list = list.child(self.inline_list_add(key, index + 1, retiring, cx));
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
                        .text_color(color(palette.muted))
                        .child("社交账号"),
                )
                .child(self.inline_profile_add(0, retiring, cx));
            for index in 0..form.profile_count {
                let selector = selector_for(format!("inline-profile-remove-{index}"));
                let mut profile = div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(color(palette.line))
                    .p_3()
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .items_center()
                            .text_size(px(12.0))
                            .text_color(color(palette.muted))
                            .child(format!("账号 {}", index + 1))
                            .child(
                                div()
                                    .id(gpui::SharedString::from(selector.clone()))
                                    .debug_selector(move || selector.clone())
                                    .cursor_pointer()
                                    .size(px(28.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(6.))
                                    .hover(|v| v.bg(color(palette.focus)))
                                    .text_color(color(palette.muted))
                                    .child("−")
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        cx.stop_propagation();
                                        if retiring {
                                            return;
                                        }
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
                    let selector =
                        selector_for(format!("inline-profile-input-{}-{index}", field.key));
                    profile = profile.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(px(12.0))
                                    .text_color(color(palette.muted))
                                    .child(field_label(field.key).to_owned()),
                            )
                            .child(self.inline_input(
                                field,
                                selector,
                                scroll.clone(),
                                retiring,
                                cx,
                            )),
                    );
                }
                profiles =
                    profiles
                        .child(profile)
                        .child(self.inline_profile_add(index + 1, retiring, cx));
            }
            fields = fields.child(profiles);
        }
        deferred(
            anchored().snap_to_window_with_margin(px(64.0)).child(
                div()
                    .on_children_prepainted(move |children, _, _| {
                        if !retiring {
                            bounds_slot.set(children.first().copied());
                        }
                    })
                    .id(if retiring {
                        "retiring-inline-boundary"
                    } else {
                        "inline-boundary"
                    })
                    .w(px(width))
                    .child(
                        div()
                            .id(if retiring {
                                "retiring-inline-form"
                            } else {
                                "inline-form"
                            })
                            .debug_selector(move || {
                                if retiring {
                                    "retiring-inline-form".into()
                                } else {
                                    "inline-form".into()
                                }
                            })
                            .relative()
                            .opacity(alpha)
                            .occlude()
                            .w(px(width))
                            .flex()
                            .flex_col()
                            .rounded(px(10.))
                            .shadow(vec![gpui::BoxShadow {
                                color: gpui::hsla(0., 0., 0., 0.12),
                                offset: gpui::point(px(0.), px(4.)),
                                blur_radius: px(18.),
                                spread_radius: px(-3.),
                            }])
                            .border_1()
                            .border_color(color(palette.line))
                            .bg(color(palette.surface))
                            .on_click(|_, _, cx| cx.stop_propagation())
                            .child(
                                div()
                                    .flex()
                                    .justify_between()
                                    .items_center()
                                    .px(px(20.))
                                    .py(px(14.))
                                    .border_b_1()
                                    .border_color(color(palette.line))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_3()
                                            .min_w_0()
                                            .child(
                                                div()
                                                    .size(px(30.))
                                                    .flex_none()
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .rounded(px(7.))
                                                    .bg(color(palette.focus))
                                                    .child(crate::chrome::icon(
                                                        SECTION_LABELS
                                                            .iter()
                                                            .find(|(id, _)| *id == form.section)
                                                            .map_or("basics", |(id, _)| *id),
                                                        palette.accent,
                                                    )),
                                            )
                                            .child(
                                                div()
                                                    .flex()
                                                    .flex_col()
                                                    .min_w_0()
                                                    .gap(px(3.))
                                                    .child(
                                                        div()
                                                            .font_family(crate::chrome::SERIF)
                                                            .text_size(px(16.))
                                                            .text_color(color(palette.title))
                                                            .child(label(&form.section).to_owned()),
                                                    )
                                                    .when(!subtitle.is_empty(), |v| {
                                                        v.child(
                                                            div()
                                                                .text_size(px(12.))
                                                                .text_color(color(palette.muted))
                                                                .truncate()
                                                                .child(subtitle),
                                                        )
                                                    }),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .id("close-inline")
                                            .tab_index(0)
                                            .tab_stop(!retiring)
                                            .cursor_pointer()
                                            .size(px(28.))
                                            .flex_none()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .rounded(px(6.))
                                            .text_size(px(17.))
                                            .text_color(color(palette.muted))
                                            .hover(|v| {
                                                v.bg(color(palette.focus))
                                                    .text_color(color(palette.accent))
                                            })
                                            .focus(|v| v.bg(color(palette.focus)))
                                            .child("×")
                                            .on_key_down(cx.listener(
                                                move |this, event, window, cx| {
                                                    if !retiring
                                                        && crate::chrome::activation_key(event)
                                                    {
                                                        cx.stop_propagation();
                                                        this.dismiss_inline(
                                                            &DismissInline,
                                                            window,
                                                            cx,
                                                        );
                                                    }
                                                },
                                            ))
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                if retiring {
                                                    return;
                                                }
                                                this.dismiss_inline(&DismissInline, window, cx)
                                            })),
                                    ),
                            )
                            .child(
                                div().relative().min_h_0().child(fields).child(
                                    div()
                                        .debug_selector(move || {
                                            if retiring {
                                                "retiring-inline-scrollbar-viewport".into()
                                            } else {
                                                "inline-scrollbar-viewport".into()
                                            }
                                        })
                                        .absolute()
                                        .inset_0()
                                        .child(
                                            Scrollbar::vertical(&scroll)
                                                .scrollbar_show(ScrollbarShow::Always),
                                        ),
                                ),
                            )
                            .when_some(form.error.clone(), |view, message| {
                                view.child(
                                    div()
                                        .px(px(20.))
                                        .pb_3()
                                        .text_size(px(12.0))
                                        .text_color(color(crate::chrome::readable_on(
                                            style.colors.seal,
                                            palette.surface,
                                        )))
                                        .child(message),
                                )
                            })
                            .when(retiring, |view| {
                                view.child(
                                    div()
                                        .id("retiring-inline-shield")
                                        .absolute()
                                        .inset_0()
                                        .occlude()
                                        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                            cx.stop_propagation()
                                        })
                                        .on_click(|_, _, cx| cx.stop_propagation()),
                                )
                            }),
                    ),
            ),
        )
        .priority(if retiring { 1 } else { 2 })
    }

    fn inline_list_add(
        &self,
        key: &'static str,
        index: usize,
        retiring: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let selector = format!(
            "{}inline-list-add-{key}-{index}",
            if retiring { "retiring-" } else { "" }
        );
        div()
            .id(gpui::SharedString::from(selector.clone()))
            .debug_selector(move || selector.clone())
            .h(px(24.0))
            .rounded(px(6.))
            .hover(|v| v.bg(color(FormPalette::new(&self.active_style()).focus)))
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
                if retiring {
                    return;
                }
                this.inline_list_structure(key, StringListChange::Insert { index }, window, cx);
            }))
    }

    fn inline_input(
        &self,
        field: &Field,
        selector: String,
        scroll: ScrollHandle,
        retiring: bool,
        cx: &App,
    ) -> impl IntoElement {
        let input = field.input.clone();
        let palette = FormPalette::new(&self.active_style());
        let focus = input.read(cx).focus_handle(cx);
        div()
            .on_children_prepainted(move |children, window, cx| {
                if retiring || !input.read(cx).focus_handle(cx).is_focused(window) {
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
            .rounded(px(6.))
            .border_1()
            .border_color(color(palette.line))
            .bg(color(palette.field))
            .text_color(color(palette.ink))
            .track_focus(&focus)
            .tab_stop(false)
            .focus(|v| {
                v.border_color(color(palette.accent))
                    .bg(color(palette.field))
            })
            .child(
                Input::new(&field.input)
                    .w_full()
                    .appearance(false)
                    .focus_bordered(false)
                    .text_size(px(13.))
                    .px(px(10.))
                    .py(px(6.))
                    .disabled(retiring)
                    .tab_index(if retiring { -1 } else { 0 }),
            )
    }

    fn inline_profile_add(
        &self,
        index: usize,
        retiring: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let selector = format!(
            "{}inline-profile-add-{index}",
            if retiring { "retiring-" } else { "" }
        );
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
                if retiring {
                    return;
                }
                this.inline_profile_structure(ProfileChange::Insert { index }, window, cx);
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{editor_ui, model::AppModel, structure_ui};
    use gpui::{Modifiers, TestAppContext, point};
    use std::{cell::RefCell, rc::Rc};

    #[gpui::test]
    fn opening_appearance_from_a_composing_form_preserves_visible_text(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("resume.json");
        let original = br#"{"basics":{"name":"Original"}}"#;
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
            owner.update(cx, |app, cx| app.open_inline("basics", None, window, cx))
        });
        let input = cx.update(|_, cx| {
            owner
                .read(cx)
                .inline
                .form
                .as_ref()
                .unwrap()
                .fields
                .iter()
                .find(|field| field.key == "name")
                .unwrap()
                .input
                .clone()
        });
        cx.update(|window, cx| {
            input.update(cx, |input, cx| {
                input.focus(window, cx);
                input.replace_and_mark_text_in_range(Some(0..8), "简里", None, window, cx);
            })
        });
        cx.update(|window, cx| owner.update(cx, |app, cx| app.show_appearance(window, cx)));
        cx.run_until_parked();
        cx.update(|_, cx| {
            let app = owner.read(cx);
            assert!(app.inline.form.is_none());
            assert!(app.appearance_editor.is_some());
            assert!(
                app.editor
                    .as_ref()
                    .unwrap()
                    .buffer
                    .source()
                    .contains("简里")
            );
        });
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }

    #[test]
    fn form_palette_keeps_all_builtin_and_custom_surfaces_readable() {
        let model = AppModel::load(None).unwrap();
        let mut styles: Vec<_> = model
            .themes
            .iter()
            .map(|theme| theme.style.clone())
            .collect();
        for paper in ["#172426", "#808080", "#FDF9F0"] {
            let mut manifest = model.themes[0].manifest.clone();
            manifest.normalized["colors"] = serde_json::json!({
                "paper": paper, "canvas": paper, "ink": "#888888",
                "muted": "#999999", "accent": "#B29566", "seal": "#A14F40"
            });
            styles.push(resume_one_core::theme_style::ThemeStyle::from_manifest(
                &manifest,
            ));
        }
        for style in styles {
            let p = FormPalette::new(&style);
            assert!(p.title.contrast(p.surface) >= 4.5);
            assert!(p.muted.contrast(p.surface) >= 4.5);
            assert!(p.ink.contrast(p.field) >= 4.5);
            assert!(p.accent.contrast(p.focus) >= 4.5);
        }
        assert!(field_order("work", "name") < field_order("work", "position"));
        assert!(field_order("work", "startDate") < field_order("work", "endDate"));
        assert!(full_width_field("work", "summary"));
        assert!(!full_width_field("work", "position"));
    }

    #[gpui::test]
    fn semantic_field_grid_and_dragging_keep_form_and_rails_stable(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let executor = cx.executor();
        let (view, cx) =
            cx.add_window_view(|_, cx| ResumeOne::new(AppModel::load(None).unwrap(), cx));
        cx.update(|window, _| window.refresh());
        let right = cx.debug_bounds("module-panel").unwrap();
        let paper = cx.debug_bounds("resume-paper").unwrap();
        cx.update(|window, cx| {
            view.update(cx, |app, cx| app.open_inline("work", Some(0), window, cx));
        });
        executor.advance_clock(INLINE_FADE_IN);
        cx.update(|window, _| window.refresh());
        let form = cx.debug_bounds("inline-form").unwrap();
        let company = cx.debug_bounds("inline-field-name").unwrap();
        let content_width = cx.update(|_, cx| {
            let app = view.read(cx);
            let style = app.active_style();
            (style.page.width_mm - style.page.margins.left - style.page.margins.right)
                * 3.
                * app.preview_scale
        });
        assert!(f32::from(form.size.width) > content_width + 60.);
        assert!(form.left() >= paper.left() + px(13.));
        assert!(form.right() <= paper.right() - px(13.));
        let position = cx.debug_bounds("inline-field-position").unwrap();
        let start = cx.debug_bounds("inline-field-startDate").unwrap();
        let end = cx.debug_bounds("inline-field-endDate").unwrap();
        let summary = cx.debug_bounds("inline-field-summary").unwrap();
        assert_eq!(company.top(), position.top());
        assert_eq!(start.top(), end.top());
        assert!(start.top() > company.top());
        assert!(summary.size.width > company.size.width * 1.9);
        assert!(company.left() - form.left() >= px(20.));
        assert!(form.right() - position.right() >= px(20.));
        cx.update(|_, cx| assert_eq!(view.read(cx).inline.bounds.get(), Some(form)));
        // Long fields can continue below the clipped scroll viewport. Only
        // visible fields are valid pointer targets until the user scrolls.
        for bounds in [company, position, start, end] {
            assert!(form.contains(&bounds.center()));
            cx.simulate_mouse_move(
                bounds.center(),
                Some(gpui::MouseButton::Left),
                Modifiers::none(),
            );
            cx.update(|_, cx| assert!(!view.read(cx).inline.closing));
        }
        let outside = point(paper.left() + px(3.), form.top() + px(20.));
        cx.simulate_mouse_move(outside, Some(gpui::MouseButton::Left), Modifiers::none());
        cx.update(|_, cx| assert!(view.read(cx).inline.closing));
        assert_eq!(right, cx.debug_bounds("module-panel").unwrap());
        assert_eq!(paper, cx.debug_bounds("resume-paper").unwrap());
    }

    #[gpui::test]
    fn wider_form_keeps_paper_insets_and_adapts_its_grid_across_all_themes(
        cx: &mut TestAppContext,
    ) {
        cx.update(gpui_component::init);
        let (view, cx) =
            cx.add_window_view(|_, cx| ResumeOne::new(AppModel::load(None).unwrap(), cx));
        let mut saw_single_column = false;
        let mut saw_two_columns = false;
        for (width, height, inspector_open) in [
            (1440., 900., false),
            (1280., 820., false),
            (1024., 640., false),
            (720., 480., false),
            (720., 480., true),
        ] {
            cx.simulate_resize(gpui::size(px(width), px(height)));
            cx.update(|window, cx| {
                view.update(cx, |app, cx| {
                    if inspector_open {
                        app.show_appearance(window, cx);
                    } else {
                        app.finish_appearance(window, cx);
                    }
                })
            });
            for index in 0..crate::model::BUILTIN_THEMES.len() {
                cx.update(|window, cx| {
                    view.update(cx, |app, cx| {
                        app.clear_inline();
                        app.model.select_theme(index);
                        cx.notify();
                    });
                    window.refresh();
                });
                cx.update(|window, cx| {
                    view.update(cx, |app, cx| app.open_inline("work", Some(0), window, cx))
                });
                cx.executor().advance_clock(INLINE_FADE_IN);
                cx.update(|window, _| window.refresh());
                let paper = cx.debug_bounds("resume-paper").unwrap();
                let form = cx.debug_bounds("inline-form").unwrap();
                let company = cx.debug_bounds("inline-field-name").unwrap();
                let position = cx.debug_bounds("inline-field-position").unwrap();
                assert!(
                    form.left() >= paper.left() + px(13.),
                    "{width}, theme {index}"
                );
                assert!(
                    form.right() <= paper.right() - px(13.),
                    "{width}, theme {index}"
                );
                assert!(company.left() >= form.left() + px(20.));
                assert!(position.right() <= form.right() - px(20.));
                if form.size.width >= px(440.) {
                    saw_two_columns = true;
                    assert_eq!(company.top(), position.top());
                    assert!(company.right() < position.left());
                } else {
                    saw_single_column = true;
                    assert!(position.top() > company.bottom());
                    assert_eq!(company.left(), position.left());
                }
            }
        }
        assert!(saw_single_column && saw_two_columns);
    }

    #[gpui::test]
    fn leaving_a_focused_form_commits_visible_ime_text_without_saving(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let executor = cx.executor();
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("ime.json");
        let disk = br#"{"basics":{"name":"Original"}}"#;
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
        for immediate in [false, true] {
            cx.update(|window, cx| {
                view.update(cx, |app, cx| app.open_inline("basics", None, window, cx));
            });
            executor.advance_clock(INLINE_FADE_IN);
            cx.update(|window, _| window.refresh());
            let form = cx.debug_bounds("inline-form").unwrap();
            let paper = cx.debug_bounds("resume-paper").unwrap();
            cx.simulate_mouse_move(form.center(), None, Modifiers::none());
            let input = cx.update(|_, cx| {
                view.read(cx)
                    .inline
                    .form
                    .as_ref()
                    .unwrap()
                    .fields
                    .iter()
                    .find(|field| field.key == "name")
                    .unwrap()
                    .input
                    .clone()
            });
            cx.update(|window, cx| {
                input.update(cx, |state, cx| {
                    state.focus(window, cx);
                    let length = state.value().chars().map(char::len_utf16).sum();
                    state.replace_and_mark_text_in_range(Some(0..length), "简里", None, window, cx);
                    assert!(state.is_composing());
                });
            });
            let outside = point(paper.left() + px(3.), form.top() + px(20.));
            assert!(!form.contains(&outside) && paper.contains(&outside));
            if immediate {
                // A render re-reads the real system preference. Exercise this
                // branch directly without changing the host's preference.
                cx.update(|window, cx| {
                    view.update(cx, |app, cx| {
                        app.reduce_motion = true;
                        app.inline_pointer_moved(outside, window, cx);
                    });
                });
            } else {
                cx.simulate_mouse_move(outside, None, Modifiers::none());
            }
            cx.update(|window, cx| {
                let app = view.read(cx);
                assert!(!input.read(cx).is_composing());
                assert!(!input.read(cx).focus_handle(cx).is_focused(window));
                assert_eq!(app.model.document.value["basics"]["name"], "简里");
                assert!(
                    app.editor
                        .as_ref()
                        .unwrap()
                        .buffer
                        .source()
                        .contains("简里")
                );
                assert!(app.editor.as_ref().unwrap().buffer.is_dirty());
                if immediate {
                    assert!(app.inline.form.is_none());
                } else {
                    assert!(app.inline.closing);
                    assert_eq!(app.inline.visibility.target(), 0.);
                }
            });
            executor.advance_clock(INLINE_FADE_OUT);
            cx.update(|window, _| window.refresh());
            cx.run_until_parked();
            cx.update(|window, cx| {
                view.update(cx, |app, cx| {
                    assert!(app.inline.form.is_none());
                    app.undo_visual_edit(&structure_ui::UndoVisualEdit, window, cx);
                });
            });
            cx.run_until_parked();
            cx.update(|_, cx| {
                assert_eq!(
                    view.read(cx).model.document.value["basics"]["name"],
                    "Original"
                );
            });
            assert_eq!(std::fs::read(&path).unwrap(), disk);
        }
    }

    #[gpui::test]
    fn exit_does_not_reopen_the_same_target_or_reverse_an_explicit_dismissal(
        cx: &mut TestAppContext,
    ) {
        cx.update(gpui_component::init);
        let executor = cx.executor();
        let (view, cx) =
            cx.add_window_view(|_, cx| ResumeOne::new(AppModel::load(None).unwrap(), cx));
        cx.update(|window, cx| {
            view.update(cx, |app, cx| {
                app.open_inline("work", Some(0), window, cx);
                app.paper_hover_changed(0, true, window, cx);
            })
        });
        executor.advance_clock(INLINE_FADE_IN);
        cx.update(|window, cx| {
            view.update(cx, |app, cx| {
                app.form_hover_changed(app.inline.form_epoch, false, window, cx);
            })
        });
        executor.advance_clock(INLINE_FADE_OUT);
        cx.update(|window, _| window.refresh());
        cx.update(|window, cx| {
            view.update(cx, |app, cx| {
                assert!(app.inline.form.is_none());
                app.hovered_item = Some(("work".into(), 0, 0));
                app.schedule_inline("work", Some(0), 0, window, cx);
                assert!(app.inline.candidate.is_none());
                app.inline_region_left("work", Some(0), 0);
                app.schedule_inline("work", Some(0), 0, window, cx);
                assert!(app.inline.candidate.is_some());
            })
        });
        executor.advance_clock(INLINE_HOVER_DELAY);
        cx.run_until_parked();
        cx.update(|window, cx| {
            view.update(cx, |app, cx| {
                assert!(app.inline_matches("work", Some(0)));
                app.dismiss_inline(&DismissInline, window, cx);
                let inside = app.inline.bounds.get().unwrap().center();
                app.inline_pointer_moved(inside, window, cx);
                assert!(app.inline.closing && app.inline.closing_explicit);
                // Explicit click/keyboard activation still opens the same target.
                app.open_inline("work", Some(0), window, cx);
                assert!(!app.inline.closing && !app.inline.closing_explicit);
            })
        });
    }

    #[gpui::test]
    fn module_movement_keeps_one_dwell_but_leaving_or_scrolling_cancels_it(
        cx: &mut TestAppContext,
    ) {
        cx.update(gpui_component::init);
        let executor = cx.executor();
        let (view, cx) =
            cx.add_window_view(|_, cx| ResumeOne::new(AppModel::load(None).unwrap(), cx));
        let enter = |window: &mut Window, cx: &mut App| {
            view.update(cx, |app, cx| {
                app.hovered_item = Some(("work".into(), 0, 0));
                app.schedule_inline("work", Some(0), 0, window, cx);
            });
        };
        cx.update(enter);
        let generation = cx.update(|_, cx| view.read(cx).inline.generation);
        executor.advance_clock(Duration::from_millis(100));
        cx.update(enter);
        executor.advance_clock(Duration::from_millis(25));
        cx.update(enter);
        cx.update(|_, cx| assert_eq!(generation, view.read(cx).inline.generation));
        executor.advance_clock(Duration::from_millis(14));
        cx.run_until_parked();
        cx.update(|_, cx| assert!(view.read(cx).inline.form.is_none()));
        executor.advance_clock(Duration::from_millis(1));
        cx.run_until_parked();
        cx.update(|_, cx| {
            assert!(
                view.read(cx).inline_matches("work", Some(0)),
                "moving within a module must not restart the 140ms dwell"
            )
        });

        cx.update(|window, cx| {
            view.update(cx, |app, cx| {
                app.clear_inline();
                app.hovered_item = Some(("work".into(), 0, 0));
                app.schedule_inline("work", Some(0), 0, window, cx);
            })
        });
        executor.advance_clock(Duration::from_millis(110));
        cx.update(|_, cx| {
            view.update(cx, |app, _| {
                app.hovered_item = None;
                app.inline_region_left("work", Some(0), 0);
            })
        });
        executor.advance_clock(Duration::from_millis(35));
        cx.run_until_parked();
        cx.update(|_, cx| {
            assert!(
                view.read(cx).inline.form.is_none(),
                "a cancelled dwell cannot open after leaving the module"
            )
        });
        cx.update(|_, cx| assert!(view.read(cx).inline.candidate.is_none()));

        cx.update(enter);
        executor.advance_clock(Duration::from_millis(50));
        cx.update(|_, cx| view.update(cx, |app, cx| app.preview_scrolled(cx)));
        executor.advance_clock(INLINE_HOVER_DELAY);
        cx.run_until_parked();
        cx.update(|_, cx| assert!(view.read(cx).inline.form.is_none()));
        cx.update(enter);
        executor.advance_clock(INLINE_HOVER_DELAY);
        cx.run_until_parked();
        cx.update(|_, cx| assert!(view.read(cx).inline_matches("work", Some(0))));
    }

    #[gpui::test]
    fn interrupted_fade_keeps_progress_and_only_unmounts_after_a_hidden_frame(
        cx: &mut TestAppContext,
    ) {
        cx.update(gpui_component::init);
        let executor = cx.executor();
        let (view, cx) =
            cx.add_window_view(|_, cx| ResumeOne::new(AppModel::load(None).unwrap(), cx));
        cx.update(|window, cx| {
            view.update(cx, |app, cx| {
                app.open_inline("work", Some(0), window, cx);
            })
        });
        cx.update(|window, _| window.refresh());
        let form = cx.debug_bounds("inline-form").unwrap();
        let paper = cx.debug_bounds("resume-paper").unwrap();
        let outside = point(paper.left() + px(5.), form.top() + px(20.));
        assert!(paper.contains(&outside));
        assert!(!form.contains(&outside));
        for _ in 0..10 {
            cx.simulate_mouse_move(form.center(), None, Modifiers::none());
            executor.advance_clock(INLINE_FADE_IN);
            cx.update(|window, _| window.refresh());
            cx.simulate_mouse_move(outside, None, Modifiers::none());
            cx.update(|_, cx| assert!(view.read(cx).inline.closing));
            executor.advance_clock(Duration::from_millis(30));
            cx.update(|window, _| window.refresh());
            let before = cx.update(|_, cx| {
                view.read(cx)
                    .inline
                    .visibility
                    .sample(cx.background_executor().now())
                    .0
            });
            assert!(before > 0. && before < 1.);
            cx.simulate_mouse_move(form.center(), None, Modifiers::none());
            cx.update(|_, cx| {
                let app = view.read(cx);
                assert_eq!(
                    before,
                    app.inline
                        .visibility
                        .sample(cx.background_executor().now())
                        .0
                );
                assert!(!app.inline.closing);
            });
        }
        executor.advance_clock(INLINE_FADE_IN);
        cx.update(|window, _| window.refresh());
        cx.simulate_mouse_move(outside, None, Modifiers::none());
        executor.advance_clock(INLINE_FADE_OUT - Duration::from_millis(1));
        cx.update(|window, _| window.refresh());
        cx.update(|_, cx| assert!(view.read(cx).inline.form.is_some()));
        executor.advance_clock(Duration::from_millis(1));
        cx.update(|window, _| window.refresh());
        cx.update(|_, cx| assert!(view.read(cx).inline.form.is_none()));
        cx.update(|window, cx| {
            view.update(cx, |app, cx| app.open_inline("work", Some(0), window, cx));
        });
        executor.advance_clock(INLINE_FADE_IN);
        cx.update(|window, _| window.refresh());
        cx.simulate_event(gpui::MouseExitEvent::default());
        cx.update(|_, cx| assert!(view.read(cx).inline.closing));
    }

    #[gpui::test]
    fn form_handoff_crossfades_without_blank_frame_or_sidebar_reflow(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let executor = cx.executor();
        let (view, cx) =
            cx.add_window_view(|_, cx| ResumeOne::new(AppModel::load(None).unwrap(), cx));
        cx.update(|window, _| window.refresh());
        let right = cx.debug_bounds("module-panel").unwrap();
        let paper = cx.debug_bounds("resume-paper").unwrap();
        let source = cx.update(|_, cx| view.read(cx).model.document.source.clone());
        cx.update(|window, cx| {
            view.update(cx, |app, cx| {
                app.open_inline("work", Some(0), window, cx);
                app.inline.form.as_mut().unwrap().page = Some(0);
            })
        });
        executor.advance_clock(INLINE_FADE_IN);
        cx.update(|window, _| window.refresh());
        let old_input = cx.update(|_, cx| {
            view.read(cx).inline.form.as_ref().unwrap().fields[0]
                .input
                .clone()
        });
        cx.update(|window, cx| {
            view.update(cx, |app, cx| {
                app.open_inline("education", Some(0), window, cx);
                app.inline.form.as_mut().unwrap().page = Some(0);
                assert_eq!(
                    app.inline
                        .visibility
                        .sample(cx.background_executor().now())
                        .0,
                    0.
                );
                assert_eq!(
                    app.inline
                        .retiring
                        .as_ref()
                        .unwrap()
                        .visibility
                        .sample(cx.background_executor().now())
                        .0,
                    1.
                );
            })
        });
        for millis in [0, 15, 15, 30] {
            executor.advance_clock(Duration::from_millis(millis));
            cx.update(|window, _| window.refresh());
            cx.update(|_, cx| {
                let app = view.read(cx);
                let now = cx.background_executor().now();
                let combined = app.inline.visibility.sample(now).0
                    + app
                        .inline
                        .retiring
                        .as_ref()
                        .unwrap()
                        .visibility
                        .sample(now)
                        .0;
                assert!((combined - 1.).abs() < 0.001);
            });
            assert_eq!(right, cx.debug_bounds("module-panel").unwrap());
            assert_eq!(paper, cx.debug_bounds("resume-paper").unwrap());
        }
        cx.update(|window, cx| {
            old_input.update(cx, |input, cx| input.set_value("stale edit", window, cx))
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            let app = view.read(cx);
            assert_eq!(app.model.document.source, source);
            assert_eq!(
                app.editor.as_ref().unwrap().buffer.source().as_bytes(),
                source
            );
        });
        executor.advance_clock(INLINE_HANDOFF);
        cx.update(|window, _| window.refresh());
        cx.update(|_, cx| {
            assert!(view.read(cx).inline.retiring.is_none());
            assert!(view.read(cx).inline_matches("education", Some(0)));
        });
        cx.update(|_, cx| {
            view.update(cx, |app, cx| {
                // Reflow may remove the old page. The live form must fall back to
                // the item's new page instead of vanishing while the user types.
                app.inline.form.as_mut().unwrap().page = Some(999);
                cx.notify();
            })
        });
        assert!(cx.debug_bounds("inline-form").is_some());
    }

    #[gpui::test]
    fn hover_exit_uses_the_form_not_the_paper_and_ignores_stale_events(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let executor = cx.executor();
        let (view, cx) =
            cx.add_window_view(|_, cx| ResumeOne::new(AppModel::load(None).unwrap(), cx));
        cx.update(|window, cx| {
            view.update(cx, |app, cx| {
                app.open_inline("work", Some(0), window, cx);
                let epoch = app.inline.form_epoch;
                app.form_hover_changed(epoch, true, window, cx);
                app.paper_hover_changed(0, true, window, cx);
                app.paper_hover_changed(1, true, window, cx);
                app.paper_hover_changed(0, false, window, cx);
                assert_eq!(app.inline.paper_hovered, Some(1));
                assert!(!app.inline.closing);
            })
        });
        executor.advance_clock(INLINE_FADE_IN);
        cx.update(|window, cx| {
            view.update(cx, |app, cx| {
                let epoch = app.inline.form_epoch;
                app.form_hover_changed(epoch, false, window, cx);
                assert!(
                    app.inline.closing,
                    "leaving the editor must start fading immediately, even on the paper"
                );
                app.paper_hover_changed(0, true, window, cx);
                assert!(
                    app.inline.closing,
                    "paper re-entry must not keep an editor alive"
                );
                app.form_hover_changed(epoch - 1, true, window, cx);
                assert!(
                    app.inline.closing,
                    "an old form cannot reverse the current fade"
                );
            })
        });
        executor.advance_clock(INLINE_FADE_OUT / 2);
        cx.update(|window, cx| {
            view.update(cx, |app, cx| {
                let now = cx.background_executor().now();
                let before = app.inline.visibility.sample(now).0;
                app.form_hover_changed(app.inline.form_epoch, true, window, cx);
                assert_eq!(app.inline.visibility.sample(now).0, before);
                assert!(!app.inline.closing);
            })
        });
        executor.advance_clock(INLINE_FADE_IN);
        cx.update(|window, cx| {
            view.update(cx, |app, cx| {
                app.form_hover_changed(app.inline.form_epoch, false, window, cx);
            })
        });
        executor.advance_clock(INLINE_FADE_OUT);
        cx.update(|window, _| window.refresh());
        cx.update(|_, cx| assert!(view.read(cx).inline.form.is_none()));
        cx.update(|window, cx| {
            view.update(cx, |app, cx| {
                app.open_inline("work", Some(0), window, cx);
                app.reduce_motion = true;
                app.form_hover_changed(app.inline.form_epoch, true, window, cx);
                app.form_hover_changed(app.inline.form_epoch, false, window, cx);
                assert!(app.inline.form.is_none());
            })
        });
    }

    #[gpui::test]
    fn a_spanning_item_moves_its_unfocused_form_to_the_hovered_page(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("spanning.json");
        let highlights = vec![
            "A long achievement with enough words to wrap across the page and preserve its editable item identity.";
            35
        ];
        let resume = serde_json::json!({
            "basics": {"name": "A"},
            "work": [{"name": "B", "highlights": highlights}]
        });
        std::fs::write(&path, serde_json::to_vec(&resume).unwrap()).unwrap();
        let model = AppModel::load(Some(&path)).unwrap();
        let layout = resume_one_core::layout::layout_with_theme(
            &model.content,
            resume_one_core::render::Language::Zh,
            &model.applied_theme().manifest,
        )
        .unwrap();
        let pages = layout
            .pages
            .iter()
            .enumerate()
            .filter_map(|(page, elements)| {
                elements
                    .iter()
                    .any(|element| matches!(element,
                        resume_one_core::layout::LayoutElement::Text(text)
                            if text.target.is_some_and(|target| target.section == "work" && target.index == Some(0))
                    ))
                    .then_some(page)
            })
            .collect::<Vec<_>>();
        assert!(pages.len() > 1, "test item must actually span pages");
        let first = pages[0];
        let next = pages[1];
        let (view, cx) = cx.add_window_view(|_, cx| ResumeOne::new(model, cx));
        cx.update(|window, cx| {
            view.update(cx, |app, cx| {
                app.open_inline("work", Some(0), window, cx);
                app.inline.form.as_mut().unwrap().page = Some(first);
                let epoch = app.inline.form_epoch;
                app.schedule_inline("work", Some(0), next, window, cx);
                assert_eq!(app.inline.form.as_ref().unwrap().page, Some(next));
                assert_eq!(
                    app.inline.form_epoch, epoch,
                    "same item must not rebuild its fields"
                );
            });
        });
    }

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
                app.inline.paper_hovered = Some(0);
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
        assert_eq!(
            viewport,
            cx.debug_bounds("inline-scrollbar-viewport").unwrap(),
            "scrollbar must cover the field viewport, not its static position below it"
        );
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
                app.inline.paper_hovered = Some(0);
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
        cx.update(|_, cx| view.update(cx, |app, _| app.reduce_motion = false));
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
        executor.advance_clock(INLINE_FADE_IN);
        cx.update(|window, _| window.refresh());
        cx.simulate_mouse_move(point(px(20.0), px(100.0)), None, Modifiers::none());
        cx.run_until_parked();
        cx.update(|_, cx| assert!(view.read(cx).inline.form.is_some()));
        cx.update(|_, cx| assert!(view.read(cx).inline.closing));
        executor.advance_clock(INLINE_FADE_OUT);
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.update(|_, cx| assert!(view.read(cx).inline.form.is_none()));
        cx.update(|window, cx| window.focus(&view.read(cx).focus));
        cx.simulate_keystrokes("tab tab tab tab enter");
        cx.update(|window, cx| assert!(view.read(cx).inline_focused(window, cx)));
        cx.simulate_keystrokes("escape");
        executor.advance_clock(INLINE_FADE_OUT);
        cx.update(|window, _| window.refresh());
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
        cx.update(|_, cx| {
            view.update(cx, |app, _| {
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
