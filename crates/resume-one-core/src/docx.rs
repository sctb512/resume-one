//! Editable Word export from the shared semantic content model.

use std::io::Cursor;

use docx_rs::{Docx, LineSpacing, PageMargin, Paragraph, Run, RunFonts};

use crate::render::{Language, title};
use crate::semantic::ResumeContent;

const INK: &str = "263737";
const MUTED: &str = "657571";
const ACCENT: &str = "57766C";

fn font() -> RunFonts {
    RunFonts::new()
        .ascii("Noto Sans SC")
        .hi_ansi("Noto Sans SC")
        .east_asia("Noto Sans SC")
}

fn run(text: impl Into<String>, size: usize, color: &str, bold: bool) -> Run {
    let run = Run::new()
        .add_text(text)
        .size(size)
        .color(color)
        .fonts(font());
    if bold { run.bold() } else { run }
}

fn paragraph(
    text: impl Into<String>,
    size: usize,
    color: &str,
    bold: bool,
    after: u32,
) -> Paragraph {
    Paragraph::new()
        .add_run(run(text, size, color, bold))
        .line_spacing(LineSpacing::new().after(after))
}

pub fn render(content: &ResumeContent, language: Language) -> Result<Vec<u8>, String> {
    let mut document = Docx::new().page_size(11906, 16838).page_margin(PageMargin {
        top: 1150,
        left: 1250,
        bottom: 1150,
        right: 1250,
        header: 500,
        footer: 500,
        gutter: 0,
    });
    if !content.name.is_empty() {
        document =
            document.add_paragraph(paragraph(&content.name, 40, INK, true, 90).keep_next(true));
    }
    if !content.label.is_empty() {
        document = document.add_paragraph(paragraph(&content.label, 22, ACCENT, false, 130));
    }
    if !content.contacts.is_empty() {
        document = document.add_paragraph(paragraph(
            content.contacts.join("  ·  "),
            18,
            MUTED,
            false,
            170,
        ));
    }
    if !content.summary.is_empty() {
        for line in content
            .summary
            .lines()
            .filter(|line| !line.trim().is_empty())
        {
            document = document.add_paragraph(paragraph(line, 20, INK, false, 100));
        }
    }
    for section in &content.sections {
        let mut paragraphs = vec![
            paragraph(title(section.id, language), 24, ACCENT, true, 110)
                .line_spacing(LineSpacing::new().before(230).after(110))
                .keep_next(true),
        ];
        for item in &section.items {
            if !item.heading.is_empty() {
                paragraphs.push(paragraph(&item.heading, 21, INK, true, 30).keep_next(true));
            }
            if !item.details.is_empty() {
                paragraphs.push(
                    paragraph(item.details.join("  ·  "), 18, MUTED, false, 90).keep_next(
                        !item.paragraphs.is_empty()
                            || !item.bullets.is_empty()
                            || item.url.is_some(),
                    ),
                );
            }
            for text in &item.paragraphs {
                for line in text.lines().filter(|line| !line.trim().is_empty()) {
                    paragraphs.push(paragraph(line, 20, INK, false, 70));
                }
            }
            if (section.id == "skills" || section.id == "interests") && !item.bullets.is_empty() {
                paragraphs.push(paragraph(item.bullets.join("  ·  "), 18, MUTED, false, 95));
            } else {
                for (index, bullet) in item.bullets.iter().enumerate() {
                    paragraphs.push(
                        paragraph(format!("•  {bullet}"), 19, INK, false, 45)
                            .indent(Some(240), None, None, None)
                            .keep_next(index + 1 < item.bullets.len() || item.url.is_some()),
                    );
                }
            }
            if let Some(url) = &item.url {
                paragraphs.push(paragraph(url, 18, ACCENT, false, 120));
            }
        }
        let compact = paragraphs.len() <= 10;
        let count = paragraphs.len();
        for (index, paragraph) in paragraphs.into_iter().enumerate() {
            document = document.add_paragraph(if compact && index + 1 < count {
                paragraph.keep_next(true)
            } else {
                paragraph
            });
        }
    }
    let mut bytes = Cursor::new(Vec::new());
    document
        .pack(&mut bytes)
        .map_err(|error| error.to_string())?;
    Ok(bytes.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{resume::parse_resume, semantic::project};

    #[test]
    fn creates_readable_editable_docx_with_unicode() {
        let source = br#"{"basics":{"name":"\u674e\u660e","summary":"\u7b80\u5386\u6982\u8ff0"},"work":[{"name":"Acme","position":"Lead","highlights":["Shipped"]}],"custom":{"secret":"HIDDEN"}}"#;
        let content = project(&parse_resume(source).unwrap());
        let bytes = render(&content, Language::Zh).unwrap();
        assert!(bytes.starts_with(b"PK"));
        let parsed = docx_rs::read_docx(&bytes).unwrap();
        let body = parsed.json();
        assert!(body.contains("李明"));
        assert!(body.contains("工作经历"));
        assert!(body.contains("Shipped"));
        assert!(!body.contains("HIDDEN"));
    }
}
