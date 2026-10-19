use std::path::PathBuf;
use std::sync::Arc;

use gpui::{
    App, Context, Entity, IntoElement, KeyBinding, PromptLevel, Subscription, Window, actions, div,
    prelude::*, px,
};
use gpui_component::{
    Theme,
    highlighter::{HighlightTheme, SyntaxColors},
    input::{Input, InputEvent, InputState},
};
use resume_one_core::draft::{DraftStore, RecoveryDraft};
use resume_one_core::editor::ResumeBuffer;
use resume_one_core::semantic::project;
use resume_one_core::storage::{create_resume, replace_resume};
use resume_one_core::theme_style::{Rgb, ThemeStyle};
use serde_json::json;

use crate::{ResumeOne, color};

actions!(resume_one, [ToggleSource, FormatJson, Save, SaveAs]);

pub(super) struct EditorUi {
    pub input: Entity<InputState>,
    pub buffer: ResumeBuffer,
    last_display: String,
    pub display_alias: Option<(String, String)>,
    pub store: DraftStore,
    pub generation: u64,
    pub saving: bool,
    pub journal_error: Option<String>,
    pub source_anchors: Arc<Vec<resume_one_core::source_map::Anchor>>,
    pub source_generation: u64,
    _subscription: Subscription,
}

pub(super) fn register(cx: &mut App) {
    let modifier = if cfg!(target_os = "macos") {
        "cmd"
    } else {
        "ctrl"
    };
    cx.bind_keys([
        KeyBinding::new(&format!("{modifier}-/"), ToggleSource, Some("ResumeOne")),
        KeyBinding::new(&format!("{modifier}-s"), Save, Some("ResumeOne")),
        KeyBinding::new(&format!("{modifier}-shift-s"), SaveAs, Some("ResumeOne")),
        KeyBinding::new("ctrl-f", FormatJson, Some("Input")),
        KeyBinding::new("cmd-f", FormatJson, Some("Input")),
        KeyBinding::new(
            &format!("shift-{modifier}-f"),
            gpui_component::input::Search,
            Some("Input"),
        ),
    ]);
}

fn role(style: &ThemeStyle, tint: Rgb, weight: f32) -> String {
    let candidate = style.colors.ink.mix(tint, weight);
    if candidate.contrast(style.colors.paper) >= 4.5 {
        candidate.hex()
    } else {
        style.colors.ink.hex()
    }
}

pub(super) fn apply_editor_theme(style: &ThemeStyle, cx: &mut App) {
    let mut highlight = (*HighlightTheme::default_light()).clone();
    highlight.style.editor_background = Some(color(style.colors.paper).into());
    highlight.style.editor_foreground = Some(color(style.colors.ink).into());
    highlight.style.editor_line_number = Some(color(style.colors.muted).into());
    highlight.style.editor_active_line_number = Some(color(style.colors.accent).into());
    highlight.style.syntax = serde_json::from_value::<SyntaxColors>(json!({
        "property": {"color": role(style, style.colors.accent, 0.75)},
        "string": {"color": role(style, style.colors.seal, 0.64)},
        "number": {"color": role(style, style.colors.accent, 0.62)},
        "boolean": {"color": role(style, style.colors.seal, 0.78)},
        "constant": {"color": role(style, style.colors.muted, 0.75)},
        "punctuation": {"color": style.colors.muted.hex()},
        "string.escape": {"color": style.colors.accent.hex()}
    }))
    .expect("constant syntax colors");
    let theme = Theme::global_mut(cx);
    theme.highlight_theme = Arc::new(highlight);
    theme.colors.background = color(style.colors.paper).into();
    theme.colors.foreground = color(style.colors.ink).into();
    theme.colors.accent = color(style.colors.accent).into();
    theme.colors.accent_foreground = color(style.colors.paper).into();
    theme.colors.popover = color(style.colors.paper).into();
    theme.colors.popover_foreground = color(style.colors.ink).into();
    theme.colors.border = color(style.colors.border).into();
    theme.colors.input = color(style.colors.border).into();
    theme.colors.caret = color(style.colors.accent).into();
    theme.colors.muted = color(style.colors.paper.mix(style.colors.ink, 0.03)).into();
    let selection = style.colors.paper.mix(style.colors.accent, 0.12);
    theme.colors.selection = color(if style.colors.ink.contrast(selection) >= 4.5 {
        selection
    } else {
        style.colors.paper
    })
    .into();
    theme.colors.muted_foreground = color(style.colors.muted).into();
    theme.colors.ring = color(style.colors.accent).into();
}

fn drafts_root() -> Result<PathBuf, String> {
    dirs::data_local_dir()
        .map(|directory| directory.join("Resume One").join("drafts"))
        .ok_or_else(|| "无法定位本地应用数据目录，不能安全保存编辑草稿".into())
}

fn inside_json_string(prefix: &str) -> bool {
    let mut quoted = false;
    let mut escaped = false;
    for byte in prefix.bytes() {
        if escaped {
            escaped = false;
        } else if byte == b'\\' && quoted {
            escaped = true;
        } else if byte == b'"' {
            quoted = !quoted;
        }
    }
    quoted
}

/// Pair only a single newly inserted opener. Bulk paste, undo, IME replacement
/// and programmatic full-text changes must not acquire extra punctuation.
fn automatic_pair(before: &str, after: &str, cursor: usize) -> Option<char> {
    if cursor == 0 || after.len() != before.len() + 1 {
        return None;
    }
    let opener = *after.as_bytes().get(cursor - 1)?;
    let closer = match opener {
        b'{' => '}',
        b'[' => ']',
        b'"' => '"',
        _ => return None,
    };
    if before.get(..cursor - 1)? != after.get(..cursor - 1)?
        || before.get(cursor - 1..)? != after.get(cursor..)?
        || after.get(cursor..)?.starts_with(closer)
        || inside_json_string(after.get(..cursor - 1)?)
    {
        return None;
    }
    Some(closer)
}

impl ResumeOne {
    pub(super) fn focus_current(&self, window: &mut Window, cx: &mut Context<Self>) {
        if self.source_mode
            && !self.files.open
            && let Some(editor) = &self.editor
        {
            editor.input.update(cx, |input, cx| input.focus(window, cx));
            return;
        }
        window.focus(&self.focus);
    }

    pub(super) fn reset_editor_for_document(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.rebuild_editor_for_document(true, window, cx);
    }

    pub(super) fn reset_editor_for_new_document(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.rebuild_editor_for_document(false, window, cx);
    }

    fn rebuild_editor_for_document(
        &mut self,
        offer_recovery: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.visual_undo.clear();
        self.visual_redo.clear();
        self.insertion_before = None;
        self.selected_module = None;
        self.selected_item_module = None;
        self.recovery_pending = false;
        self.clear_inline();
        let Some(editor) = &mut self.editor else {
            return;
        };
        let Ok(buffer) = ResumeBuffer::new(self.model.opened_source.clone()) else {
            return;
        };
        let display = buffer
            .formatted()
            .unwrap_or_else(|_| buffer.source().into());
        editor.display_alias =
            (display != buffer.source()).then(|| (display.clone(), buffer.source().into()));
        editor.last_display = display.clone();
        editor.source_anchors =
            Arc::new(resume_one_core::source_map::anchors(&display).unwrap_or_default());
        editor.source_generation = editor.source_generation.wrapping_add(1);
        editor.buffer = buffer;
        editor.store = DraftStore::new(
            drafts_root().expect("draft root already checked"),
            self.model.document_path.clone(),
        );
        editor.generation = 0;
        editor.journal_error = None;
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .code_editor("json")
                .soft_wrap(true)
                .default_value(display)
        });
        let subscription = cx.subscribe_in(&input, window, |this, input, event, window, cx| {
            if matches!(event, InputEvent::Change) {
                this.editor_changed(input, window, cx);
            }
        });
        editor.input = input;
        editor._subscription = subscription;
        if offer_recovery {
            self.prompt_recovery(window, cx);
        }
    }

    pub(super) fn confirm_close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.export_busy {
            self.editor_warning("导出中", "请等待导出完成再关闭窗口", window, cx);
            return false;
        }
        let Some(editor) = &self.editor else {
            return true;
        };
        if editor.saving {
            self.editor_warning("保存中", "请等待保存完成再关闭窗口", window, cx);
            return false;
        }
        if !editor.buffer.is_dirty() {
            return true;
        }
        let choice = window.prompt(
            PromptLevel::Warning,
            "简历有未保存的更改",
            Some("可以先按 ⌘S 或 Ctrl+S 保存。"),
            &["继续编辑", "丢弃更改并关闭"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if choice.await.ok() == Some(1) {
                this.update_in(cx, |this, window, cx| {
                    if let Some(editor) = &mut this.editor {
                        editor.generation += 1;
                        match editor.store.clear_if_newer(editor.generation) {
                            Ok(_) => {
                                editor.buffer = ResumeBuffer::new(this.model.opened_source.clone())
                                    .expect("opened source is UTF-8");
                                window.remove_window();
                            }
                            Err(message) => {
                                this.editor_warning("无法丢弃草稿", &message, window, cx)
                            }
                        }
                    }
                })
                .ok();
            }
        })
        .detach();
        false
    }

    pub(super) fn flush_draft_on_quit(&mut self) {
        let Some(editor) = &mut self.editor else {
            return;
        };
        if editor.buffer.is_dirty() {
            editor.generation += 1;
            let draft = RecoveryDraft {
                source: editor.buffer.source().into(),
                if_match: editor.buffer.baseline_revision().into(),
            };
            if let Err(message) = editor.store.write_if_newer(editor.generation, &draft) {
                editor.journal_error = Some(message);
            }
        }
    }
    pub(super) fn format_json(
        &mut self,
        _: &FormatJson,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.source_mode {
            return;
        }
        let Some(editor) = &mut self.editor else {
            return;
        };
        if editor.input.read(cx).is_composing() {
            return;
        }
        let formatted = match editor.buffer.formatted() {
            Ok(value) => value,
            Err(issue) => {
                self.editor_warning("无法格式化", &issue.message, window, cx);
                return;
            }
        };
        editor.display_alias = None;
        let input = editor.input.clone();
        input.update(cx, |state, cx| {
            let length = state.value().chars().map(char::len_utf16).sum();
            state.replace_text_transaction(0..length, &formatted, window, cx);
        });
        // Entering the editor renders a formatted copy without touching the
        // original bytes. An explicit Format command makes that copy editable.
        if let Some(editor) = &mut self.editor
            && editor.buffer.set_source(formatted)
        {
            self.persist_editor_draft(cx);
        }
        cx.notify();
    }

    pub(super) fn save_json(&mut self, _: &Save, window: &mut Window, cx: &mut Context<Self>) {
        if self.theme_manager_open
            || self.inline_composing(cx)
            || self
                .editor
                .as_ref()
                .is_some_and(|e| e.input.read(cx).is_composing())
        {
            return;
        }
        let Some(editor) = &self.editor else { return };
        if editor.saving {
            return;
        }
        let snapshot = match editor.buffer.save_snapshot() {
            Ok(snapshot) => snapshot,
            Err(issues) => {
                self.editor_warning("无法保存", &issues[0].message, window, cx);
                return;
            }
        };
        if let Some(path) = self.model.document_path.clone() {
            self.commit_save(path, false, snapshot, window, cx);
        } else {
            self.save_json_as(&SaveAs, window, cx);
        }
    }

    pub(super) fn save_json_as(&mut self, _: &SaveAs, window: &mut Window, cx: &mut Context<Self>) {
        if self.theme_manager_open {
            return;
        }
        let Some(editor) = &self.editor else { return };
        if editor.saving {
            return;
        }
        let snapshot = match editor.buffer.save_snapshot() {
            Ok(snapshot) => snapshot,
            Err(issues) => {
                self.editor_warning("无法另存为", &issues[0].message, window, cx);
                return;
            }
        };
        let directory = self
            .model
            .folder
            .as_ref()
            .map(|folder| folder.path.clone())
            .or_else(|| {
                self.model
                    .document_path
                    .as_ref()
                    .and_then(|path| path.parent().map(PathBuf::from))
            })
            .or_else(dirs::document_dir)
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| PathBuf::from("."));
        let suggested = self
            .model
            .document_path
            .as_ref()
            .and_then(|path| path.file_name().and_then(|name| name.to_str()))
            .unwrap_or("resume.json");
        let choice = cx.prompt_for_new_path(&directory, Some(suggested));
        if let Some(editor) = &mut self.editor {
            editor.saving = true;
        }
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let path = match choice.await {
                Ok(Ok(path)) => path,
                Ok(Err(error)) => {
                    this.update_in(cx, |this, window, cx| {
                        if let Some(editor) = &mut this.editor {
                            editor.saving = false;
                        }
                        this.editor_warning("无法另存为", &error.to_string(), window, cx);
                        cx.notify();
                    })
                    .ok();
                    return;
                }
                Err(_) => None,
            };
            this.update_in(cx, |this, window, cx| {
                if let Some(editor) = &mut this.editor {
                    editor.saving = false;
                }
                if let Some(path) = path {
                    if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                        this.editor_warning("无法另存为", "请选择 .json 文件名", window, cx);
                    } else {
                        this.commit_save(path, true, snapshot, window, cx);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn commit_save(
        &mut self,
        path: PathBuf,
        create: bool,
        snapshot: resume_one_core::editor::SaveSnapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(editor) = &mut self.editor {
            editor.saving = true;
        }
        cx.notify();
        let task = cx.background_executor().spawn({
            let path = path.clone();
            let snapshot = snapshot.clone();
            async move {
                if create {
                    create_resume(&path, &snapshot.source)
                } else {
                    replace_resume(&path, &snapshot.source, &snapshot.if_match)
                }
            }
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            this.update_in(cx, |this, window, cx| {
                this.finish_save(path, create, snapshot, result, window, cx)
            })
            .ok();
        })
        .detach();
    }

    fn finish_save(
        &mut self,
        path: PathBuf,
        create: bool,
        snapshot: resume_one_core::editor::SaveSnapshot,
        result: Result<String, resume_one_core::storage::StorageError>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(editor) = &mut self.editor {
            editor.saving = false;
        }
        match result {
            Ok(_) => {
                self.model.is_example = false;
                self.model.opened_source = snapshot.source.clone();
                self.model.preview_ready = true;
                if create {
                    let old = self
                        .editor
                        .as_ref()
                        .map(|editor| (editor.store.clone(), editor.generation + 1));
                    if let Some((store, generation)) = old
                        && let Err(message) = store.clear_if_newer(generation)
                    {
                        self.editor_warning("草稿整理失败", &message, window, cx);
                    }
                    let path = path.canonicalize().unwrap_or(path);
                    self.model.document_label = path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into();
                    self.model.document_path = Some(path.clone());
                    if let Some(editor) = &mut self.editor {
                        editor.store = DraftStore::new(
                            drafts_root().expect("draft root already checked"),
                            Some(path),
                        );
                        editor.generation = 0;
                    }
                }
                if let Some(editor) = &mut self.editor {
                    editor.buffer.acknowledge_save(&snapshot);
                    editor.journal_error = None;
                }
                self.persist_editor_draft(cx);
                if let Some(path) = self.model.document_path.clone() {
                    self.remember_file(&path);
                }
                window.set_window_title(&format!(
                    "{} — Resume One · 简里",
                    self.model.document_label
                ));
            }
            Err(error) => self.editor_warning("保存失败", &error.message, window, cx),
        }
        cx.notify();
    }
    pub(super) fn ensure_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.editor.is_none() {
            let root = match drafts_root() {
                Ok(root) => root,
                Err(message) => {
                    self.editor_warning("无法进入编辑", &message, window, cx);
                    return false;
                }
            };
            let buffer = match ResumeBuffer::new(self.model.opened_source.clone()) {
                Ok(buffer) => buffer,
                Err(issue) => {
                    self.editor_warning("无法进入编辑", &issue.message, window, cx);
                    return false;
                }
            };
            let display = buffer
                .formatted()
                .unwrap_or_else(|_| buffer.source().into());
            let display_alias =
                (display != buffer.source()).then(|| (display.clone(), buffer.source().into()));
            let initial_display = display.clone();
            let input = cx.new(|cx| {
                InputState::new(window, cx)
                    .code_editor("json")
                    .soft_wrap(true)
                    .default_value(initial_display)
            });
            let subscription = cx.subscribe_in(&input, window, |this, input, event, window, cx| {
                if matches!(event, InputEvent::Change) {
                    this.editor_changed(input, window, cx);
                }
            });
            let store = DraftStore::new(root, self.model.document_path.clone());
            self.editor = Some(EditorUi {
                source_anchors: Arc::new(
                    resume_one_core::source_map::anchors(&display).unwrap_or_default(),
                ),
                source_generation: 0,
                input,
                buffer,
                last_display: display,
                display_alias,
                store,
                generation: 0,
                saving: false,
                journal_error: None,
                _subscription: subscription,
            });
            self.prompt_recovery(window, cx);
        }
        true
    }

    pub(super) fn toggle_source(
        &mut self,
        _: &ToggleSource,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.theme_manager_open
            || self.inline_composing(cx)
            || self
                .editor
                .as_ref()
                .is_some_and(|e| e.input.read(cx).is_composing())
        {
            return;
        }
        self.files.open = false;
        self.clear_inline();
        self.cancel_insertion();
        self.hovered_theme = None;
        self.online.cancel_interaction();
        if self.source_mode {
            self.source_mode = false;
            self.begin_mode_transition();
            window.focus(&self.focus);
            cx.notify();
            return;
        }
        if !self.ensure_editor(window, cx) {
            return;
        }
        apply_editor_theme(self.model.selected_style(), cx);
        self.source_mode = true;
        self.begin_mode_transition();
        if let Some(editor) = &self.editor {
            editor.input.update(cx, |state, cx| state.focus(window, cx));
        }
        cx.notify();
    }

    fn prompt_recovery(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = &self.editor else { return };
        let input = editor.input.clone();
        let generation = editor.generation;
        match editor.store.load() {
            Ok(Some(draft)) => {
                self.recovery_pending = true;
                let choice = window.prompt(
                    PromptLevel::Info,
                    "发现未保存的编辑草稿",
                    Some("恢复后会保留原文件的版本检查，避免覆盖外部修改。"),
                    &["恢复草稿", "暂不恢复"],
                    cx,
                );
                cx.spawn_in(window, async move |this, cx| {
                    let answer = choice.await.ok();
                    this.update_in(cx, |this, window, cx| {
                        if this.editor.as_ref().is_some_and(|editor| {
                            editor.input == input && editor.generation == generation
                        }) {
                            this.recovery_pending = false;
                            if answer == Some(0) {
                                this.restore_draft(draft, window, cx);
                            }
                        }
                    })
                    .ok();
                })
                .detach();
            }
            Ok(None) => {}
            Err(message) => self.editor_warning("无法读取恢复草稿", &message, window, cx),
        }
    }

    fn restore_draft(&mut self, draft: RecoveryDraft, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = &mut self.editor else {
            return;
        };
        match ResumeBuffer::restore(self.model.opened_source.clone(), &draft) {
            Ok(buffer) => {
                editor.buffer = buffer;
                editor.display_alias = None;
                editor.last_display = draft.source.clone();
                editor.source_anchors = Arc::new(
                    resume_one_core::source_map::anchors(&draft.source).unwrap_or_default(),
                );
                editor.source_generation = editor.source_generation.wrapping_add(1);
                editor
                    .input
                    .update(cx, |state, cx| state.set_value(draft.source, window, cx));
                self.refresh_preview_from_buffer();
                cx.notify();
            }
            Err(issue) => self.editor_warning("无法恢复草稿", &issue.message, window, cx),
        }
    }

    fn editor_changed(
        &mut self,
        input: &Entity<InputState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.source_mode {
            self.cancel_insertion();
        }
        let Some(editor) = &mut self.editor else {
            return;
        };
        let state = input.read(cx);
        let source = state.value().to_string();
        editor.source_anchors =
            Arc::new(resume_one_core::source_map::anchors(&source).unwrap_or_default());
        editor.source_generation = editor.source_generation.wrapping_add(1);
        let cursor = state.cursor();
        let closer = automatic_pair(&editor.last_display, &source, cursor);
        editor.last_display.clone_from(&source);
        if let Some(closer) = closer {
            let input = input.clone();
            let expected = source.clone();
            let owner = cx.entity().downgrade();
            window.defer(cx, move |window, cx| {
                let active = owner.upgrade().is_some_and(|owner| {
                    owner
                        .read(cx)
                        .editor
                        .as_ref()
                        .is_some_and(|editor| editor.input == input)
                });
                if !active {
                    return;
                }
                input.update(cx, |state, cx| {
                    if state.value().as_str() != expected || state.cursor() != cursor {
                        return;
                    }
                    let position = state.cursor_position();
                    state.insert(closer.to_string(), window, cx);
                    state.set_cursor_position(position, window, cx);
                });
            });
        }
        let stored_source = editor
            .display_alias
            .as_ref()
            .and_then(|(display, original)| (source == *display).then(|| original.clone()))
            .unwrap_or(source);
        if !editor.buffer.set_source(stored_source) {
            return;
        }
        self.sync_inline_error();
        self.refresh_preview_from_buffer();
        self.persist_editor_draft(cx);
        cx.notify();
    }

    fn refresh_preview_from_buffer(&mut self) {
        if let Some(document) = self
            .editor
            .as_ref()
            .and_then(|editor| {
                editor
                    .buffer
                    .issues()
                    .is_empty()
                    .then(|| editor.buffer.last_valid())
                    .flatten()
            })
            .cloned()
        {
            self.model.content = project(&document);
            self.model.document = document;
            self.model.preview_ready = true;
        }
    }

    fn persist_editor_draft(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = &mut self.editor else {
            return;
        };
        editor.generation += 1;
        let generation = editor.generation;
        let store = editor.store.clone();
        let input = editor.input.clone();
        let dirty = editor.buffer.is_dirty();
        let draft = RecoveryDraft {
            source: editor.buffer.source().into(),
            if_match: editor.buffer.baseline_revision().into(),
        };
        let task = cx.background_executor().spawn(async move {
            if dirty {
                store.write_if_newer(generation, &draft)
            } else {
                store.clear_if_newer(generation)
            }
        });
        cx.spawn(async move |this, cx| {
            if let Err(message) = task.await {
                this.update(cx, |this, cx| {
                    if let Some(editor) = &mut this.editor
                        && editor.input == input
                        && editor.generation == generation
                    {
                        editor.journal_error = Some(message);
                    }
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    pub(super) fn editor_warning(
        &self,
        title: &str,
        message: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let answer = window.prompt(PromptLevel::Warning, title, Some(message), &["确定"], cx);
        cx.spawn(async move |_, _| {
            let _ = answer.await;
        })
        .detach();
    }

    pub(super) fn json_editor(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let style = self.active_style();
        let editor = self.editor.as_ref().expect("source mode has editor");
        let status = if let Some(error) = &editor.journal_error {
            format!("草稿备份失败：{error}")
        } else if let Some(issue) = editor.buffer.issues().first() {
            if issue.path.is_empty() {
                issue.message.clone()
            } else {
                format!("{} · {}", issue.path, issue.message)
            }
        } else if editor.saving {
            "保存中…".into()
        } else if editor.buffer.is_dirty() {
            "未保存".into()
        } else {
            "".into()
        };
        div()
            .id("resume-json-editor")
            .debug_selector(|| "resume-json-editor".into())
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .bg(color(style.colors.canvas))
            .px_5()
            .pt_5()
            .pb_4()
            .items_center()
            .child(
                div()
                    .relative()
                    .w_full()
                    .max_w(px(930.))
                    .flex_1()
                    .min_h_0()
                    .rounded(px(8.))
                    .border_1()
                    .border_color(color(crate::chrome::Palette::new(style).line))
                    .bg(color(style.colors.paper))
                    .child(
                        Input::new(&editor.input)
                            .text_size(px(13.))
                            .line_height(px(21.))
                            .opacity(1.0 - 0.65 * self.spotlight_progress)
                            .appearance(false)
                            .bordered(false)
                            .focus_bordered(false)
                            .h_full()
                            .w_full()
                            .pl(px(60.0))
                            .pt_3()
                            .pr_5()
                            .pb_2(),
                    )
                    .child(self.source_gutter(cx)),
            )
            .when(!status.is_empty(), |view| {
                view.child(
                    div()
                        .h(px(24.0))
                        .flex_none()
                        .px_2()
                        .text_size(px(12.0))
                        .text_color(if editor.buffer.issues().is_empty() {
                            color(style.colors.muted)
                        } else {
                            color(style.colors.seal)
                        })
                        .child(status),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::automatic_pair;

    #[gpui::test]
    fn source_menus_follow_each_resume_palette(cx: &mut gpui::TestAppContext) {
        cx.update(gpui_component::init);
        let model = crate::model::AppModel::load(None).unwrap();
        for theme in model.themes {
            cx.update(|cx| {
                super::apply_editor_theme(&theme.style, cx);
                let colors = &gpui_component::Theme::global(cx).colors;
                assert_eq!(
                    colors.popover,
                    gpui::Hsla::from(crate::color(theme.style.colors.paper))
                );
                assert_eq!(
                    colors.popover_foreground,
                    gpui::Hsla::from(crate::color(theme.style.colors.ink))
                );
                assert_eq!(
                    colors.accent_foreground,
                    gpui::Hsla::from(crate::color(theme.style.colors.paper))
                );
            });
        }
    }

    #[test]
    fn pairs_only_single_openers_outside_strings() {
        assert_eq!(automatic_pair("", "{", 1), Some('}'));
        assert_eq!(automatic_pair("🙂", "🙂[", 5), Some(']'));
        assert_eq!(automatic_pair("", "\"", 1), Some('"'));
        assert_eq!(automatic_pair("}", "{}", 1), None);
        assert_eq!(automatic_pair("", "{}", 1), None);
        assert_eq!(automatic_pair(r#"{"x":"a"#, r#"{"x":"a{"#, 8), None);
        assert_eq!(automatic_pair(r#"{"x":"a"#, r#"{"x":"a""#, 8), None);
        assert_eq!(automatic_pair("中", "中{", 1), None);
    }
}
