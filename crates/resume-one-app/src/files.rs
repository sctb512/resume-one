use std::path::PathBuf;

use gpui::{
    App, Context, IntoElement, KeyBinding, Menu, MenuItem, PathPromptOptions, PromptLevel,
    ScrollStrategy, UniformListScrollHandle, Window, actions, div, prelude::*, px, uniform_list,
};

use crate::editor_ui::{Save, SaveAs, ToggleSource};
use crate::export_ui::{ExportDocx, ExportMarkdown, ExportPdf, ExportText};
use crate::theme_manager::ManageThemes;
use crate::workspace::OpenedFile;
use crate::workspace::OpenedPath;
use crate::{ResumeOne, color};

actions!(
    resume_one,
    [
        OpenFile,
        OpenDirectory,
        NewResume,
        ToggleFiles,
        DismissFiles,
        NextFile,
        PreviousFile,
        ChooseFile,
        RefreshFolder,
        Quit
    ]
);

#[derive(Default)]
pub(super) struct FileUi {
    pub open: bool,
    pub busy: bool,
    pub selected: usize,
    pub scroll: UniformListScrollHandle,
    pub recent_scroll: UniformListScrollHandle,
    pub actions_only: bool,
    pub anchor_right: bool,
}

fn register_bindings(cx: &mut App) {
    let modifier = if cfg!(target_os = "macos") {
        "cmd"
    } else {
        "ctrl"
    };
    cx.bind_keys([
        KeyBinding::new(&format!("{modifier}-o"), OpenFile, Some("ResumeOne")),
        KeyBinding::new(&format!("{modifier}-n"), NewResume, Some("ResumeOne")),
        KeyBinding::new(
            &format!("{modifier}-shift-o"),
            OpenDirectory,
            Some("ResumeOne"),
        ),
        KeyBinding::new(&format!("{modifier}-p"), ToggleFiles, Some("ResumeOne")),
        KeyBinding::new("escape", DismissFiles, Some("FileSwitcher")),
        KeyBinding::new("down", NextFile, Some("FileSwitcher")),
        KeyBinding::new("up", PreviousFile, Some("FileSwitcher")),
        KeyBinding::new("enter", ChooseFile, Some("FileSwitcher")),
        KeyBinding::new(&format!("{modifier}-q"), Quit, None),
    ]);
}

pub(super) fn register(cx: &mut App) {
    register_bindings(cx);
    cx.set_menus(vec![
        Menu {
            name: "Resume One".into(),
            items: vec![MenuItem::action("退出 Resume One", Quit)],
        },
        Menu {
            name: "文件".into(),
            items: vec![
                MenuItem::action("打开 JSON…", OpenFile),
                MenuItem::action("打开目录…", OpenDirectory),
                MenuItem::action("新建简历", NewResume),
                MenuItem::separator(),
                MenuItem::action("保存", Save),
                MenuItem::action("另存为…", SaveAs),
                MenuItem::separator(),
                MenuItem::action("导出 PDF…", ExportPdf),
                MenuItem::action("导出 DOCX…", ExportDocx),
                MenuItem::action("导出纯文本…", ExportText),
                MenuItem::action("导出 Markdown…", ExportMarkdown),
                MenuItem::separator(),
                MenuItem::action("切换文件", ToggleFiles),
                MenuItem::action("刷新目录", RefreshFolder),
            ],
        },
        Menu {
            name: "视图".into(),
            items: vec![MenuItem::action("切换源码与预览", ToggleSource)],
        },
        Menu {
            name: "主题".into(),
            items: vec![MenuItem::action("管理主题…", ManageThemes)],
        },
    ]);
}

impl ResumeOne {
    fn scroll_to_selected_file(&self) {
        let folder_count = self
            .model
            .folder
            .as_ref()
            .map_or(0, |folder| folder.files.len());
        if self.files.selected < folder_count {
            self.files
                .scroll
                .scroll_to_item(self.files.selected, ScrollStrategy::Center);
        } else {
            self.files.recent_scroll.scroll_to_item(
                self.files.selected.saturating_sub(folder_count),
                ScrollStrategy::Center,
            );
        }
    }

    fn recent_file_candidates(&self) -> Vec<PathBuf> {
        self.theme_store
            .as_ref()
            .map(|store| {
                store
                    .recent_files()
                    .iter()
                    .filter(|path| {
                        self.model
                            .folder
                            .as_ref()
                            .is_none_or(|folder| !folder.files.contains(path))
                    })
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(super) fn remember_file(&mut self, path: &std::path::Path) {
        if let Some(store) = &mut self.theme_store
            && let Err(message) = store.remember_file(path)
        {
            store.warnings.push(format!("无法记录最近文件：{message}"));
        }
    }

    pub(super) fn new_resume(
        &mut self,
        _: &NewResume,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.theme_manager_open
            || self.files.busy
            || self.export_busy
            || self.editor.as_ref().is_some_and(|e| e.saving)
        {
            return;
        }
        if !self.ensure_editor(window, cx) || self.recovery_pending {
            return;
        }
        self.files.open = false;
        self.export_menu_open = false;
        self.close_drawer();
        let has_draft = match self.editor.as_ref().unwrap().store.load() {
            Ok(draft) => draft.is_some(),
            Err(message) => {
                self.editor_warning("无法读取恢复草稿", &message, window, cx);
                return;
            }
        };
        if has_draft || self.editor.as_ref().is_some_and(|e| e.buffer.is_dirty()) {
            self.files.busy = true;
            let choice = window.prompt(
                PromptLevel::Warning,
                "当前简历有未保存的更改",
                Some("可取消后先保存；新建不会覆盖当前磁盘文件。"),
                &["继续编辑", "丢弃更改并新建"],
                cx,
            );
            cx.spawn_in(window, async move |this, cx| {
                let answer = choice.await.ok();
                this.update_in(cx, |this, window, cx| {
                    this.files.busy = false;
                    if answer == Some(1) {
                        this.apply_new_document(window, cx);
                    } else {
                        this.focus_current(window, cx);
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach();
            cx.notify();
            return;
        }
        self.apply_new_document(window, cx);
    }

    fn discard_current_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if let Some(editor) = &mut self.editor {
            editor.generation += 1;
            if let Err(message) = editor.store.clear_if_newer(editor.generation) {
                self.editor_warning("无法丢弃草稿", &message, window, cx);
                return false;
            }
        }
        true
    }

    fn apply_new_document(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.discard_current_draft(window, cx) {
            return;
        }
        self.clear_inline();
        self.cancel_insertion();
        self.hovered_theme = None;
        self.theme_hover_generation += 1;
        self.theme_hover_candidate = None;
        self.online.cancel_interaction();
        self.preview_scroll.set_offset(gpui::point(px(0.), px(0.)));
        self.model.new_document();
        self.reset_editor_for_new_document(window, cx);
        self.source_mode = false;
        self.begin_mode_transition();
        window.set_window_title("未命名简历 — Resume One · 简里");
        self.open_inline("basics", None, window, cx);
        self.focus_inline_first(window, cx);
        cx.notify();
    }

    pub(super) fn open_file(&mut self, _: &OpenFile, window: &mut Window, cx: &mut Context<Self>) {
        self.prompt_path(false, window, cx);
    }

    pub(super) fn open_directory(
        &mut self,
        _: &OpenDirectory,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.prompt_path(true, window, cx);
    }

    fn prompt_path(&mut self, directory: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.theme_manager_open
            || self.files.busy
            || self.export_busy
            || self.editor.as_ref().is_some_and(|editor| editor.saving)
        {
            return;
        }
        self.close_drawer();
        self.files.busy = true;
        self.files.open = false;
        self.hovered_theme = None;
        self.online.cancel_interaction();
        cx.notify();
        let choice = cx.prompt_for_paths(PathPromptOptions {
            files: !directory,
            directories: directory,
            multiple: false,
            prompt: Some(
                if directory {
                    "打开目录"
                } else {
                    "打开 JSON"
                }
                .into(),
            ),
        });
        cx.spawn_in(window, async move |this, cx| {
            let choice = match choice.await {
                Ok(Ok(paths)) => Ok(paths.and_then(|paths| paths.into_iter().next())),
                Ok(Err(error)) => Err(format!("无法打开选择窗口：{error}")),
                Err(_) => Err("选择窗口已关闭，请重试。".into()),
            };
            let result = match choice {
                Ok(Some(path)) => Some(
                    cx.background_executor()
                        .spawn(async move { OpenedPath::read(&path) })
                        .await,
                ),
                Ok(None) => None,
                Err(error) => Some(Err(error)),
            };
            this.update_in(cx, |this, window, cx| this.finish_open(result, window, cx))
                .ok();
        })
        .detach();
    }

    pub(super) fn load_path(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if self.files.busy
            || self.export_busy
            || self.editor.as_ref().is_some_and(|editor| editor.saving)
        {
            return;
        }
        self.files.busy = true;
        self.files.open = false;
        self.hovered_theme = None;
        self.online.cancel_interaction();
        cx.notify();
        let task = cx
            .background_executor()
            .spawn(async move { OpenedPath::read(&path) });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            this.update_in(cx, |this, window, cx| {
                this.finish_open(Some(result), window, cx)
            })
            .ok();
        })
        .detach();
    }

    fn finish_open(
        &mut self,
        result: Option<Result<OpenedPath, String>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.files.busy = false;
        match result {
            Some(Ok(OpenedPath::File(opened))) => {
                self.offer_opened_file(*opened, window, cx);
            }
            Some(Ok(OpenedPath::Folder(folder))) => {
                if let Some(store) = &mut self.theme_store
                    && let Err(message) = store.remember_folder(&folder.path)
                {
                    store.warnings.push(format!("无法记录最近目录：{message}"));
                }
                self.model.folder = Some(folder);
                self.show_file_switcher(window, cx);
            }
            Some(Err(error)) => {
                let answer = window.prompt(
                    PromptLevel::Warning,
                    "无法打开",
                    Some(&error),
                    &["确定"],
                    cx,
                );
                cx.spawn(async move |_, _| {
                    let _ = answer.await;
                })
                .detach();
            }
            None => {}
        }
        self.focus_current(window, cx);
        cx.notify();
    }

    fn offer_opened_file(
        &mut self,
        opened: OpenedFile,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .editor
            .as_ref()
            .is_some_and(|editor| editor.buffer.is_dirty())
        {
            let choice = window.prompt(
                PromptLevel::Warning,
                "当前简历有未保存的更改",
                Some("可以先按 ⌘S 或 Ctrl+S 保存。"),
                &["继续编辑", "丢弃更改并打开"],
                cx,
            );
            cx.spawn_in(window, async move |this, cx| {
                if choice.await.ok() == Some(1) {
                    this.update_in(cx, |this, window, cx| {
                        this.apply_opened_file(opened, window, cx)
                    })
                    .ok();
                }
            })
            .detach();
        } else {
            self.apply_opened_file(opened, window, cx);
        }
    }

    fn apply_opened_file(
        &mut self,
        opened: OpenedFile,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.editor.as_ref().is_some_and(|e| e.buffer.is_dirty())
            && !self.discard_current_draft(window, cx)
        {
            return;
        }
        self.preview_scroll
            .set_offset(gpui::point(px(0.0), px(0.0)));
        let path = opened.path.clone();
        self.model.apply_opened(opened);
        self.remember_file(&path);
        self.reset_editor_for_document(window, cx);
        if !self.model.preview_ready && !self.source_mode {
            self.toggle_source(&ToggleSource, window, cx);
        }
        window.set_window_title(&format!(
            "{} — Resume One · 简里",
            self.model.document_label
        ));
        self.focus_current(window, cx);
        cx.notify();
    }

    fn show_file_switcher(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.files.open = true;
        let recent = self.recent_file_candidates();
        self.files.actions_only = self.model.folder.is_none() && recent.is_empty();
        self.files.anchor_right = false;
        let folder_selected = self.model.folder.as_ref().and_then(|folder| {
            folder
                .files
                .iter()
                .position(|path| Some(path) == self.model.document_path.as_ref())
        });
        let folder_count = self
            .model
            .folder
            .as_ref()
            .map_or(0, |folder| folder.files.len());
        self.files.selected = folder_selected
            .or_else(|| {
                recent
                    .iter()
                    .position(|path| Some(path) == self.model.document_path.as_ref())
                    .map(|index| folder_count + index)
            })
            .unwrap_or(0);
        self.scroll_to_selected_file();
        window.focus(&self.focus);
        cx.notify();
    }

    pub(super) fn toggle_files(
        &mut self,
        _: &ToggleFiles,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.files.busy {
            return;
        }
        self.close_drawer();
        if self.files.open && !self.files.anchor_right {
            self.dismiss_files(&DismissFiles, window, cx);
        } else {
            self.show_file_switcher(window, cx);
        }
    }

    pub(super) fn toggle_open_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.files.busy {
            return;
        }
        self.close_drawer();
        if self.files.open && self.files.anchor_right {
            self.files.open = false;
        } else {
            self.files.open = true;
            self.files.actions_only = true;
            self.files.anchor_right = true;
            self.files.selected = 0;
            window.focus(&self.focus);
        }
        cx.notify();
    }

    pub(super) fn dismiss_files(
        &mut self,
        _: &DismissFiles,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.files.open = false;
        cx.notify();
    }

    pub(super) fn next_file(&mut self, _: &NextFile, _: &mut Window, cx: &mut Context<Self>) {
        let count = if self.files.actions_only {
            3
        } else {
            self.model
                .folder
                .as_ref()
                .map_or(0, |folder| folder.files.len())
                + self.recent_file_candidates().len()
        };
        if self.files.open && count > 0 {
            self.files.selected = (self.files.selected + 1).min(count - 1);
            self.scroll_to_selected_file();
            cx.notify();
        }
    }

    pub(super) fn previous_file(
        &mut self,
        _: &PreviousFile,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.files.open {
            self.files.selected = self.files.selected.saturating_sub(1);
            self.scroll_to_selected_file();
            cx.notify();
        }
    }

    pub(super) fn choose_file(
        &mut self,
        _: &ChooseFile,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.files.open && self.files.actions_only {
            if self.files.selected == 2 {
                self.new_resume(&NewResume, window, cx);
            } else {
                self.prompt_path(self.files.selected == 1, window, cx);
            }
            return;
        }
        let folder_count = self
            .model
            .folder
            .as_ref()
            .map_or(0, |folder| folder.files.len());
        let path = if self.files.selected < folder_count {
            self.model
                .folder
                .as_ref()
                .and_then(|folder| folder.files.get(self.files.selected))
                .cloned()
        } else {
            self.recent_file_candidates()
                .get(self.files.selected.saturating_sub(folder_count))
                .cloned()
        };
        if self.files.open
            && let Some(path) = path
        {
            self.load_path(path, window, cx);
        }
    }

    pub(super) fn refresh_folder(
        &mut self,
        _: &RefreshFolder,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(folder) = &self.model.folder {
            self.load_path(folder.path.clone(), window, cx);
        }
    }

    pub(super) fn file_switcher(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = &self.model.selected_style().colors;
        let shortcut = if cfg!(target_os = "macos") {
            "⌘"
        } else {
            "Ctrl+"
        };
        let mut menu = div()
            .id("file-switcher")
            .debug_selector(|| "file-switcher".to_string())
            .absolute()
            .top(px(crate::chrome::TOOLBAR_HEIGHT))
            .when(self.files.anchor_right, |menu| menu.right(px(24.0)))
            .when(!self.files.anchor_right, |menu| menu.left(px(160.0)))
            .w(px(if self.files.actions_only {
                260.0
            } else {
                360.0
            }))
            .max_h(px((self.viewport_height
                - crate::chrome::TOOLBAR_HEIGHT
                - 16.0)
                .max(180.0)))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .p_2()
            .gap_1()
            .rounded_lg()
            .shadow_lg()
            .border_1()
            .border_color(color(colors.border))
            .bg(color(colors.chrome))
            .text_color(color(colors.ink))
            .text_size(px(13.0))
            .occlude()
            .on_mouse_down_out(
                cx.listener(|this, event: &gpui::MouseDownEvent, window, cx| {
                    if event.position.y >= px(crate::chrome::TOOLBAR_HEIGHT) {
                        this.dismiss_files(&DismissFiles, window, cx);
                    }
                }),
            )
            .child(
                div()
                    .id("open-json")
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .cursor_pointer()
                    .when(self.files.actions_only && self.files.selected == 0, |row| {
                        row.bg(color(colors.canvas))
                    })
                    .hover(|row| row.bg(color(colors.canvas)))
                    .flex()
                    .justify_between()
                    .child("打开 JSON…")
                    .child(format!("{shortcut}O"))
                    .on_click(
                        cx.listener(|this, _, window, cx| this.open_file(&OpenFile, window, cx)),
                    ),
            )
            .child(
                div()
                    .id("open-directory")
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .cursor_pointer()
                    .when(self.files.actions_only && self.files.selected == 1, |row| {
                        row.bg(color(colors.canvas))
                    })
                    .hover(|row| row.bg(color(colors.canvas)))
                    .flex()
                    .justify_between()
                    .child("打开目录…")
                    .child(format!("{shortcut}⇧O"))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.open_directory(&OpenDirectory, window, cx)
                    })),
            )
            .child(
                div()
                    .id("new-resume")
                    .debug_selector(|| "new-resume".into())
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .cursor_pointer()
                    .when(self.files.actions_only && self.files.selected == 2, |row| {
                        row.bg(color(colors.canvas))
                    })
                    .hover(|row| row.bg(color(colors.canvas)))
                    .flex()
                    .justify_between()
                    .child("新建简历")
                    .child(format!("{shortcut}N"))
                    .on_click(
                        cx.listener(|this, _, window, cx| this.new_resume(&NewResume, window, cx)),
                    ),
            );
        if let Some(folder) = &self.model.folder
            && !self.files.actions_only
        {
            menu = menu.child(
                div()
                    .mt_2()
                    .pt_2()
                    .px_3()
                    .border_t_1()
                    .border_color(color(colors.border))
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                folder
                                    .path
                                    .file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                                    .into_owned(),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(12.0))
                                    .text_color(color(colors.muted))
                                    .child(folder.path.to_string_lossy().into_owned()),
                            ),
                    )
                    .child(
                        div()
                            .id("refresh-folder")
                            .cursor_pointer()
                            .text_size(px(12.0))
                            .child("刷新")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.refresh_folder(&RefreshFolder, window, cx)
                            })),
                    ),
            );
            if folder.files.is_empty() {
                menu = menu.child(
                    div()
                        .px_3()
                        .py_4()
                        .text_color(color(colors.muted))
                        .child("此目录没有 JSON 文件"),
                );
            } else {
                menu = menu.child(
                    uniform_list(
                        "folder-files",
                        folder.files.len(),
                        cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                            let colors = &this.model.selected_style().colors;
                            range
                                .map(|index| {
                                    let path =
                                        this.model.folder.as_ref().unwrap().files[index].clone();
                                    let name = path
                                        .file_name()
                                        .unwrap_or_default()
                                        .to_string_lossy()
                                        .into_owned();
                                    let active = this.model.document_path.as_ref() == Some(&path);
                                    div()
                                        .id(("file", index))
                                        .w_full()
                                        .debug_selector(move || format!("file-{index}"))
                                        .h(px(36.0))
                                        .px_3()
                                        .flex()
                                        .items_center()
                                        .gap_2()
                                        .rounded_md()
                                        .cursor_pointer()
                                        .when(index == this.files.selected, |row| {
                                            row.bg(color(colors.canvas))
                                        })
                                        .hover(|row| row.bg(color(colors.canvas)))
                                        .child(div().flex_1().min_w_0().truncate().child(name))
                                        .child(if active { "✓" } else { "" })
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.load_path(path.clone(), window, cx)
                                        }))
                                })
                                .collect()
                        }),
                    )
                    .h(px((folder.files.len() as f32 * 36.0).min(252.0)))
                    .track_scroll(self.files.scroll.clone()),
                );
            }
        }
        let folder_count = self
            .model
            .folder
            .as_ref()
            .map_or(0, |folder| folder.files.len());
        let recent = self.recent_file_candidates();
        if !self.files.actions_only && !recent.is_empty() {
            menu = menu.child(
                div()
                    .mt_2()
                    .pt_2()
                    .px_3()
                    .border_t_1()
                    .border_color(color(colors.border))
                    .text_size(px(11.0))
                    .text_color(color(colors.muted))
                    .child("最近打开"),
            );
            let recent_count = recent.len();
            menu = menu.child(
                uniform_list(
                    "recent-files",
                    recent_count,
                    cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                        let colors = &this.model.selected_style().colors;
                        range
                            .map(|index| {
                                let path = recent[index].clone();
                                let name = path
                                    .file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                                    .into_owned();
                                let parent = path
                                    .parent()
                                    .map(|parent| parent.to_string_lossy().into_owned())
                                    .unwrap_or_default();
                                let selected = this.files.selected == folder_count + index;
                                let active = this.model.document_path.as_ref() == Some(&path);
                                div()
                                    .id(("recent-file", index))
                                    .debug_selector(move || format!("recent-file-{index}"))
                                    .h(px(42.0))
                                    .px_3()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .rounded_md()
                                    .cursor_pointer()
                                    .when(selected, |row| row.bg(color(colors.canvas)))
                                    .hover(|row| row.bg(color(colors.canvas)))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .flex()
                                            .flex_col()
                                            .child(div().truncate().child(name))
                                            .child(
                                                div()
                                                    .truncate()
                                                    .text_size(px(11.0))
                                                    .text_color(color(colors.muted))
                                                    .child(parent),
                                            ),
                                    )
                                    .child(if active { "✓" } else { "" })
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.load_path(path.clone(), window, cx)
                                    }))
                            })
                            .collect()
                    }),
                )
                .h(px((recent_count as f32 * 42.0).min(240.0)))
                .track_scroll(self.files.recent_scroll.clone()),
            );
        }
        menu
    }
}

#[cfg(test)]
#[path = "files_tests.rs"]
mod tests;
