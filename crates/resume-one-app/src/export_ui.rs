use std::path::PathBuf;

use gpui::{App, Context, IntoElement, KeyBinding, Window, actions, div, prelude::*, px};
use resume_one_core::ThemeManifest;
use resume_one_core::export::{write_export, write_export_unnamed};
use resume_one_core::render::{Language, markdown, plain_text};
use resume_one_core::semantic::ResumeContent;

use crate::ResumeOne;

actions!(
    resume_one,
    [
        ExportPdf,
        ExportDocx,
        ExportText,
        ExportMarkdown,
        DismissExport,
        NextExport,
        PreviousExport,
        ChooseExport,
        ExportChinese,
        ExportEnglish
    ]
);

pub(super) fn register(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("escape", DismissExport, Some("ResumeOne && ExportMenu")),
        KeyBinding::new("down", NextExport, Some("ResumeOne && ExportMenu")),
        KeyBinding::new("up", PreviousExport, Some("ResumeOne && ExportMenu")),
        KeyBinding::new("enter", ChooseExport, Some("ResumeOne && ExportMenu")),
        KeyBinding::new("left", ExportChinese, Some("ResumeOne && ExportMenu")),
        KeyBinding::new("right", ExportEnglish, Some("ResumeOne && ExportMenu")),
    ]);
}

#[derive(Clone, Copy)]
enum ExportKind {
    Pdf,
    Docx,
    Text,
    Markdown,
}

impl ExportKind {
    fn extension(self) -> &'static str {
        match self {
            Self::Pdf => "pdf",
            Self::Docx => "docx",
            Self::Text => "txt",
            Self::Markdown => "md",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Pdf => "PDF",
            Self::Docx => "DOCX",
            Self::Text => "纯文本",
            Self::Markdown => "Markdown",
        }
    }
}

struct ExportSnapshot {
    kind: ExportKind,
    language: Language,
    content: ResumeContent,
    theme: ThemeManifest,
    source_path: Option<PathBuf>,
}

fn render(snapshot: &ExportSnapshot) -> Result<Vec<u8>, String> {
    let language = snapshot.language;
    match snapshot.kind {
        ExportKind::Pdf => {
            resume_one_core::pdf::render_with_theme(&snapshot.content, language, &snapshot.theme)
        }
        ExportKind::Docx => {
            resume_one_core::docx::render_with_theme(&snapshot.content, language, &snapshot.theme)
        }
        ExportKind::Text => Ok(plain_text(&snapshot.content, language).into_bytes()),
        ExportKind::Markdown => Ok(markdown(&snapshot.content, language).into_bytes()),
    }
}

impl ResumeOne {
    pub(super) fn export_chinese(
        &mut self,
        _: &ExportChinese,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.export_language = Language::Zh;
        cx.notify();
    }

    pub(super) fn export_english(
        &mut self,
        _: &ExportEnglish,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.export_language = Language::En;
        cx.notify();
    }

    pub(super) fn dismiss_export(
        &mut self,
        _: &DismissExport,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.export_menu_open = false;
        cx.notify();
    }
    pub(super) fn next_export(&mut self, _: &NextExport, _: &mut Window, cx: &mut Context<Self>) {
        self.export_choice = (self.export_choice + 1).min(3);
        cx.notify();
    }
    pub(super) fn previous_export(
        &mut self,
        _: &PreviousExport,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.export_choice = self.export_choice.saturating_sub(1);
        cx.notify();
    }
    pub(super) fn choose_export(
        &mut self,
        _: &ChooseExport,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.export_menu_open = false;
        self.start_export(
            [
                ExportKind::Pdf,
                ExportKind::Docx,
                ExportKind::Text,
                ExportKind::Markdown,
            ][self.export_choice],
            window,
            cx,
        );
        cx.notify();
    }
    pub(super) fn export_popup(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = crate::chrome::Palette::new(self.shell_style());
        let mut menu = div()
            .id("export-popup")
            .debug_selector(|| "export-popup".into())
            .absolute()
            .top(px(crate::chrome::TOOLBAR_HEIGHT - 4.))
            .right(px(26.))
            .w(px(228.))
            .p_2()
            .rounded(px(8.))
            .border_1()
            .border_color(crate::color(p.line))
            .bg(crate::color(p.panel))
            .shadow_md()
            .occlude()
            .flex()
            .flex_col()
            .on_mouse_down_out(
                cx.listener(|this, event: &gpui::MouseDownEvent, window, cx| {
                    if event.position.y >= px(crate::chrome::TOOLBAR_HEIGHT) {
                        this.dismiss_export(&DismissExport, window, cx)
                    }
                }),
            );
        for (index, (kind, hint)) in [
            (ExportKind::Pdf, "固定版式，适合投递"),
            (ExportKind::Docx, "可在 Word 中继续编辑"),
            (ExportKind::Text, "适合粘贴到招聘系统"),
            (ExportKind::Markdown, "保留标题与列表结构"),
        ]
        .into_iter()
        .enumerate()
        {
            menu = menu.child(
                div()
                    .id(("export-format", index))
                    .debug_selector(move || format!("export-format-{index}"))
                    .px_3()
                    .py_2()
                    .rounded(px(5.))
                    .cursor_pointer()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .when(index == self.export_choice, |v| v.bg(crate::color(p.soft)))
                    .hover(|v| v.bg(crate::color(p.soft)))
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(crate::color(p.ink))
                            .child(kind.label()),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(crate::color(p.muted))
                            .child(hint),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.export_menu_open = false;
                        this.start_export(kind, window, cx);
                        cx.notify();
                    })),
            );
        }
        menu = menu.child(
            div()
                .mt_2()
                .pt_2()
                .border_t_1()
                .border_color(crate::color(p.line))
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(crate::color(p.muted))
                        .child("导出标题"),
                )
                .child(div().flex().gap_1().children(
                    [(Language::Zh, "中文"), (Language::En, "EN")].map(|(language, label)| {
                        div()
                            .id(match language {
                                Language::Zh => "export-language-zh",
                                Language::En => "export-language-en",
                            })
                            .debug_selector(move || match language {
                                Language::Zh => "export-language-zh".into(),
                                Language::En => "export-language-en".into(),
                            })
                            .px_2()
                            .py_1()
                            .rounded(px(4.))
                            .cursor_pointer()
                            .text_size(px(11.))
                            .text_color(crate::color(if self.export_language == language {
                                p.accent
                            } else {
                                p.muted
                            }))
                            .when(self.export_language == language, |v| {
                                v.bg(crate::color(p.soft))
                            })
                            .hover(|v| v.bg(crate::color(p.soft)))
                            .child(label)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.export_language = language;
                                cx.notify();
                            }))
                    }),
                )),
        );
        menu
    }

    pub(super) fn export_pdf(
        &mut self,
        _: &ExportPdf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.start_export(ExportKind::Pdf, window, cx);
    }

    pub(super) fn export_docx(
        &mut self,
        _: &ExportDocx,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.start_export(ExportKind::Docx, window, cx);
    }

    pub(super) fn export_text(
        &mut self,
        _: &ExportText,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.start_export(ExportKind::Text, window, cx);
    }

    pub(super) fn export_markdown(
        &mut self,
        _: &ExportMarkdown,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.start_export(ExportKind::Markdown, window, cx);
    }

    fn start_export(&mut self, kind: ExportKind, window: &mut Window, cx: &mut Context<Self>) {
        if self.theme_manager_open || self.export_busy || self.recovery_pending {
            return;
        }
        if let Some(issue) = self
            .editor
            .as_ref()
            .and_then(|editor| editor.buffer.issues().first())
        {
            self.editor_warning("无法导出", &issue.message, window, cx);
            return;
        }
        if !self.model.preview_ready {
            self.editor_warning("无法导出", "请先修复 JSON 和 Schema 错误", window, cx);
            return;
        }
        let snapshot = ExportSnapshot {
            kind,
            language: self.export_language,
            content: self.model.content.clone(),
            theme: self
                .model
                .appearance
                .apply(&self.model.applied_theme().manifest),
            source_path: self.model.document_path.clone(),
        };
        let directory = self
            .model
            .document_path
            .as_ref()
            .and_then(|path| path.parent().map(PathBuf::from))
            .or_else(|| self.model.folder.as_ref().map(|folder| folder.path.clone()))
            .or_else(dirs::document_dir)
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| PathBuf::from("."));
        let stem = self
            .model
            .document_path
            .as_ref()
            .and_then(|path| path.file_stem().and_then(|stem| stem.to_str()))
            .unwrap_or("resume");
        let suggested = format!("{stem}.{}", kind.extension());
        let choice = cx.prompt_for_new_path(&directory, Some(&suggested));
        self.export_busy = true;
        cx.spawn_in(window, async move |this, cx| {
            let path = match choice.await {
                Ok(Ok(path)) => path,
                Ok(Err(error)) => {
                    this.update_in(cx, |this, window, cx| {
                        this.export_busy = false;
                        this.editor_warning("无法选择导出位置", &error.to_string(), window, cx);
                    })
                    .ok();
                    return;
                }
                Err(_) => None,
            };
            this.update_in(cx, |this, window, cx| {
                if let Some(path) = path {
                    this.finish_export_path(path, snapshot, window, cx);
                } else {
                    this.export_busy = false;
                }
            })
            .ok();
        })
        .detach();
    }

    fn finish_export_path(
        &mut self,
        path: PathBuf,
        snapshot: ExportSnapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let extension = path.extension().and_then(|extension| extension.to_str());
        if !extension
            .is_some_and(|extension| extension.eq_ignore_ascii_case(snapshot.kind.extension()))
        {
            self.export_busy = false;
            self.editor_warning(
                "无法导出",
                &format!("文件名必须以 .{} 结尾", snapshot.kind.extension()),
                window,
                cx,
            );
            return;
        }
        // The native Save dialog already asks before replacing an existing file.
        // Keep the no-clobber check for a file that appears after dialog dismissal.
        let overwrite = path.exists();
        self.commit_export(path, snapshot, overwrite, window, cx);
    }

    fn commit_export(
        &mut self,
        path: PathBuf,
        snapshot: ExportSnapshot,
        overwrite: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let label = snapshot.kind.label();
        let task = cx.background_executor().spawn(async move {
            let bytes = render(&snapshot)?;
            let result = if let Some(source) = snapshot.source_path.as_deref() {
                write_export(source, &path, &bytes, overwrite)
            } else {
                write_export_unnamed(&path, &bytes, overwrite)
            };
            result.map_err(|error| error.message)
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            this.update_in(cx, |this, window, cx| {
                this.export_busy = false;
                if let Err(message) = result {
                    this.editor_warning(&format!("{label} 导出失败"), &message, window, cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

#[cfg(test)]
mod chrome_tests {
    use super::*;
    use gpui::{Modifiers, TestAppContext};
    #[gpui::test]
    fn toolbar_export_menu_is_keyboard_accessible_and_does_not_change_document(
        cx: &mut TestAppContext,
    ) {
        cx.update(gpui_component::init);
        cx.update(register);
        cx.update(crate::inline_ui::register);
        let model = crate::model::AppModel::load(None).unwrap();
        let source = model.document.source.clone();
        let (view, cx) = cx.add_window_view(|_, cx| ResumeOne::new(model, cx));
        cx.update(|window, _| window.refresh());
        let right = cx.debug_bounds("module-panel").unwrap();
        let toolbar = cx.debug_bounds("toolbar").unwrap();
        let trigger = cx.debug_bounds("export-menu").unwrap();
        cx.simulate_click(trigger.center(), Modifiers::none());
        cx.update(|window, _| window.refresh());
        cx.simulate_click(trigger.center(), Modifiers::none());
        cx.update(|_, cx| assert!(!view.read(cx).export_menu_open));
        cx.update(|window, _| window.refresh());
        cx.simulate_click(trigger.center(), Modifiers::none());
        cx.update(|window, _| window.refresh());
        for selector in [
            "export-format-0",
            "export-format-1",
            "export-format-2",
            "export-format-3",
            "export-language-zh",
            "export-language-en",
        ] {
            assert!(cx.debug_bounds(selector).is_some());
        }
        let english = cx.debug_bounds("export-language-en").unwrap();
        cx.simulate_click(english.center(), Modifiers::none());
        cx.update(|_, cx| {
            assert_eq!(view.read(cx).export_language, Language::En);
            assert!(view.read(cx).export_menu_open);
        });
        cx.simulate_keystrokes("left");
        cx.update(|_, cx| assert_eq!(view.read(cx).export_language, Language::Zh));
        cx.simulate_keystrokes("right");
        cx.update(|_, cx| assert_eq!(view.read(cx).export_language, Language::En));
        cx.simulate_keystrokes("down down up");
        cx.update(|_, cx| assert_eq!(view.read(cx).export_choice, 1));
        cx.simulate_keystrokes("escape");
        cx.update(|_, cx| {
            assert!(
                !view.read(cx).export_menu_open,
                "Escape must dismiss menu state"
            )
        });
        cx.update(|window, _| window.refresh());
        cx.update(|_, cx| assert!(!view.read(cx).export_menu_open));
        assert_eq!(right, cx.debug_bounds("module-panel").unwrap());
        assert_eq!(toolbar, cx.debug_bounds("toolbar").unwrap());
        cx.update(|_, cx| assert_eq!(view.read(cx).model.document.source, source));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::AppModel;

    #[test]
    fn all_four_formats_render_from_the_same_valid_content() {
        let model = AppModel::load(None).unwrap();
        for (kind, prefix) in [
            (ExportKind::Pdf, b"%PDF".as_slice()),
            (ExportKind::Docx, b"PK".as_slice()),
            (ExportKind::Text, "林清和".as_bytes()),
            (ExportKind::Markdown, b"# ".as_slice()),
        ] {
            let snapshot = ExportSnapshot {
                kind,
                language: Language::Zh,
                content: model.content.clone(),
                theme: model.themes[0].manifest.clone(),
                source_path: None,
            };
            let bytes = render(&snapshot).unwrap();
            assert!(bytes.starts_with(prefix), "{}", kind.extension());
        }
    }

    #[test]
    fn docx_export_uses_snapshot_theme_and_rejects_unsupported_layout() {
        let model = AppModel::load(None).unwrap();
        let manifest = resume_one_core::parse_theme(include_bytes!(
            "../../../assets/themes/xingye.theme.json"
        ))
        .unwrap();
        let mut snapshot = ExportSnapshot {
            kind: ExportKind::Docx,
            language: Language::Zh,
            content: model.content.clone(),
            theme: manifest,
            source_path: None,
        };
        let bytes = render(&snapshot).unwrap();
        // Use the core's reader through a semantic check in core/CLI tests;
        // here the theme guard demonstrates the GUI cannot take the unthemed path.
        assert!(bytes.starts_with(b"PK"));
        snapshot.theme.normalized["composition"] = serde_json::json!({"preset":"sidebar-left"});
        assert!(render(&snapshot).unwrap_err().contains("composition"));
    }

    #[test]
    fn language_changes_export_headings_without_changing_resume_content() {
        let model = AppModel::load(None).unwrap();
        for kind in [ExportKind::Text, ExportKind::Markdown] {
            let snapshot = ExportSnapshot {
                kind,
                language: Language::En,
                content: model.content.clone(),
                theme: model.themes[0].manifest.clone(),
                source_path: None,
            };
            let english = String::from_utf8(render(&snapshot).unwrap()).unwrap();
            let chinese = String::from_utf8(
                render(&ExportSnapshot {
                    language: Language::Zh,
                    ..snapshot
                })
                .unwrap(),
            )
            .unwrap();
            assert!(english.contains("Work"));
            assert!(chinese.contains("工作经历"));
            assert!(!english.contains("工作经历"));
            assert!(english.contains("林清和"));
        }
    }
}
