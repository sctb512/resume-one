use std::path::PathBuf;

use gpui::{Context, Window, actions};
use resume_one_core::ThemeManifest;
use resume_one_core::export::{write_export, write_export_unnamed};
use resume_one_core::render::{Language, markdown, plain_text};
use resume_one_core::semantic::ResumeContent;

use crate::ResumeOne;

actions!(
    resume_one,
    [ExportPdf, ExportDocx, ExportText, ExportMarkdown]
);

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
    content: ResumeContent,
    theme: ThemeManifest,
    source_path: Option<PathBuf>,
}

fn render(snapshot: &ExportSnapshot) -> Result<Vec<u8>, String> {
    let language = Language::Zh;
    match snapshot.kind {
        ExportKind::Pdf => {
            resume_one_core::pdf::render_with_theme(&snapshot.content, language, &snapshot.theme)
        }
        ExportKind::Docx => resume_one_core::docx::render(&snapshot.content, language),
        ExportKind::Text => Ok(plain_text(&snapshot.content, language).into_bytes()),
        ExportKind::Markdown => Ok(markdown(&snapshot.content, language).into_bytes()),
    }
}

impl ResumeOne {
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
        if self.theme_manager_open || self.export_busy {
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
            content: self.model.content.clone(),
            theme: self.model.applied_theme().manifest.clone(),
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
                content: model.content.clone(),
                theme: model.themes[0].manifest.clone(),
                source_path: None,
            };
            let bytes = render(&snapshot).unwrap();
            assert!(bytes.starts_with(prefix), "{}", kind.extension());
        }
    }
}
