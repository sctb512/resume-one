//! A4 PDF export with embedded Unicode font, measured wrapping and automatic pagination.

use printpdf::{
    Color, Line, LinePoint, Mm, Op, ParsedFont, PdfDocument, PdfFontHandle, PdfPage,
    PdfSaveOptions, Point, Pt, Rgb, TextItem,
};
use ttf_parser::Face;

use crate::render::{Language, title};
use crate::semantic::ResumeContent;

const FONT_BYTES: &[u8] = include_bytes!("../../../assets/fonts/NotoSansSC-Regular.ttf");
const PAGE_HEIGHT: f32 = 841.89;
const LEFT: f32 = 57.0;
const RIGHT: f32 = 57.0;
const TOP: f32 = 57.0;
const BOTTOM: f32 = 52.0;
const CONTENT_WIDTH: f32 = 595.28 - LEFT - RIGHT;
const INK: (f32, f32, f32) = (0.15, 0.22, 0.22);
const MUTED: (f32, f32, f32) = (0.37, 0.47, 0.45);
const ACCENT: (f32, f32, f32) = (0.30, 0.46, 0.42);

struct PageWriter<'a> {
    font: PdfFontHandle,
    face: Face<'a>,
    pages: Vec<Vec<Op>>,
    ops: Vec<Op>,
    y: f32,
}

impl<'a> PageWriter<'a> {
    fn new(font: PdfFontHandle, face: Face<'a>) -> Self {
        Self {
            font,
            face,
            pages: Vec::new(),
            ops: Vec::new(),
            y: PAGE_HEIGHT - TOP,
        }
    }

    fn ensure(&mut self, height: f32) {
        if self.y - height < BOTTOM {
            self.pages.push(std::mem::take(&mut self.ops));
            self.y = PAGE_HEIGHT - TOP;
        }
    }

    fn gap(&mut self, height: f32) {
        self.ensure(height);
        self.y -= height;
    }

    fn line(&mut self, text: &str, size: f32, color: (f32, f32, f32), indent: f32, leading: f32) {
        self.ensure(leading);
        self.ops.extend([
            Op::StartTextSection,
            Op::SetTextCursor {
                pos: Point {
                    x: Pt(LEFT + indent),
                    y: Pt(self.y),
                },
            },
            Op::SetFont {
                font: self.font.clone(),
                size: Pt(size),
            },
            Op::SetFillColor {
                col: Color::Rgb(Rgb::new(color.0, color.1, color.2, None)),
            },
            Op::ShowText {
                items: vec![TextItem::Text(text.to_owned())],
            },
            Op::EndTextSection,
        ]);
        self.y -= leading;
    }

    fn wrapped(
        &mut self,
        text: &str,
        size: f32,
        color: (f32, f32, f32),
        indent: f32,
        leading: f32,
    ) {
        for input_line in text.lines() {
            for line in self.wrap(input_line, size, CONTENT_WIDTH - indent) {
                self.line(&line, size, color, indent, leading);
            }
        }
    }

    fn width(&self, text: &str, size: f32) -> f32 {
        let units = self.face.units_per_em() as f32;
        text.chars()
            .map(|character| {
                self.face
                    .glyph_index(character)
                    .and_then(|glyph| self.face.glyph_hor_advance(glyph))
                    .unwrap_or(self.face.units_per_em()) as f32
                    * size
                    / units
            })
            .sum()
    }

    fn wrap(&self, input: &str, size: f32, maximum: f32) -> Vec<String> {
        let input = input.trim();
        if input.is_empty() {
            return Vec::new();
        }
        let mut lines = Vec::new();
        let mut current = String::new();
        for character in input.chars() {
            current.push(character);
            if self.width(&current, size) <= maximum {
                continue;
            }
            current.pop();
            if current.is_empty() {
                current.push(character);
                continue;
            }
            // Prefer a recent word boundary for Latin text; CJK falls back to a character break.
            if let Some(boundary) = current.rfind(char::is_whitespace)
                && boundary > current.len() / 2
            {
                let tail = current[boundary..].trim_start().to_owned();
                let head = current[..boundary].trim_end().to_owned();
                lines.push(head);
                current = tail;
                current.push(character);
                continue;
            }
            lines.push(current.trim_end().to_owned());
            current.clear();
            if !character.is_whitespace() {
                current.push(character);
            }
        }
        if !current.trim().is_empty() {
            lines.push(current.trim_end().to_owned());
        }
        lines
    }

    fn finish(mut self) -> Vec<Vec<Op>> {
        self.pages.push(self.ops);
        let count = self.pages.len();
        for (index, ops) in self.pages.iter_mut().enumerate() {
            let label = format!("{} / {}", index + 1, count);
            ops.extend([
                Op::StartTextSection,
                Op::SetTextCursor {
                    pos: Point {
                        x: Pt(LEFT + CONTENT_WIDTH - 28.0),
                        y: Pt(28.0),
                    },
                },
                Op::SetFont {
                    font: self.font.clone(),
                    size: Pt(8.0),
                },
                Op::SetFillColor {
                    col: Color::Rgb(Rgb::new(MUTED.0, MUTED.1, MUTED.2, None)),
                },
                Op::ShowText {
                    items: vec![TextItem::Text(label)],
                },
                Op::EndTextSection,
            ]);
        }
        self.pages
    }
}

pub fn render(content: &ResumeContent, language: Language) -> Result<Vec<u8>, String> {
    let mut font_warnings = Vec::new();
    let font = ParsedFont::from_bytes(FONT_BYTES, 0, &mut font_warnings)
        .ok_or_else(|| "内置字体无法解析".to_owned())?;
    let face = Face::parse(FONT_BYTES, 0).map_err(|error| error.to_string())?;
    if let Some(character) = crate::render::plain_text(content, language)
        .chars()
        .find(|character| !character.is_whitespace() && face.glyph_index(*character).is_none())
    {
        return Err(format!(
            "内置 PDF 字体缺少 U+{:04X} 字形，无法保证内容完整",
            character as u32
        ));
    }
    let mut document = PdfDocument::new(if content.name.is_empty() {
        "Resume One"
    } else {
        &content.name
    });
    let font_id = document.add_font(&font);
    let mut page = PageWriter::new(PdfFontHandle::External(font_id), face);
    if !content.name.is_empty() {
        page.wrapped(&content.name, 23.0, INK, 0.0, 31.0);
    }
    if !content.label.is_empty() {
        page.wrapped(&content.label, 10.8, ACCENT, 0.0, 18.0);
    }
    if !content.contacts.is_empty() {
        page.wrapped(&content.contacts.join("   ·   "), 8.4, MUTED, 0.0, 13.0);
    }
    page.gap(9.0);
    page.ops.push(Op::SetOutlineColor {
        col: Color::Rgb(Rgb::new(0.78, 0.84, 0.81, None)),
    });
    page.ops.push(Op::SetOutlineThickness { pt: Pt(0.7) });
    page.ops.push(Op::DrawLine {
        line: Line {
            points: vec![
                LinePoint {
                    p: Point {
                        x: Pt(LEFT),
                        y: Pt(page.y),
                    },
                    bezier: false,
                },
                LinePoint {
                    p: Point {
                        x: Pt(LEFT + CONTENT_WIDTH),
                        y: Pt(page.y),
                    },
                    bezier: false,
                },
            ],
            is_closed: false,
        },
    });
    page.gap(14.0);
    if !content.summary.is_empty() {
        page.wrapped(&content.summary, 9.4, INK, 0.0, 15.0);
        page.gap(6.0);
    }
    for section in &content.sections {
        page.ensure(80.0);
        page.gap(12.0);
        page.line(title(section.id, language), 12.2, ACCENT, 0.0, 23.0);
        for item in &section.items {
            page.ensure(43.0);
            page.gap(4.0);
            if !item.heading.is_empty() {
                page.wrapped(&item.heading, 10.3, INK, 0.0, 16.0);
            }
            if !item.details.is_empty() {
                page.wrapped(&item.details.join("   ·   "), 8.5, MUTED, 0.0, 13.5);
            }
            for paragraph in &item.paragraphs {
                page.gap(3.0);
                page.wrapped(paragraph, 9.2, INK, 0.0, 14.0);
            }
            if (section.id == "skills" || section.id == "interests") && !item.bullets.is_empty() {
                page.wrapped(&item.bullets.join("   ·   "), 8.7, MUTED, 0.0, 13.5);
            } else {
                for bullet in &item.bullets {
                    page.wrapped(&format!("•  {bullet}"), 9.0, INK, 11.0, 14.0);
                }
            }
            if let Some(url) = &item.url {
                page.wrapped(url, 8.3, ACCENT, 0.0, 13.0);
            }
        }
    }
    let pages = page
        .finish()
        .into_iter()
        .map(|ops| PdfPage::new(Mm(210.0), Mm(297.0), ops))
        .collect();
    let options = PdfSaveOptions {
        subset_fonts: true,
        ..Default::default()
    };
    let mut save_warnings = Vec::new();
    let bytes = document
        .with_pages(pages)
        .save(&options, &mut save_warnings);
    if !bytes.starts_with(b"%PDF-") {
        return Err("PDF 生成失败".into());
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{resume::parse_resume, semantic::project};

    #[test]
    fn wraps_multilingual_text_without_exceeding_content_width() {
        let face = Face::parse(FONT_BYTES, 0).unwrap();
        let writer = PageWriter::new(
            PdfFontHandle::Builtin(printpdf::BuiltinFont::Helvetica),
            face,
        );
        let lines = writer.wrap(
            "这是一段很长的中文内容 with several English words and more content",
            10.0,
            120.0,
        );
        assert!(lines.len() > 1);
        assert!(lines.iter().all(|line| writer.width(line, 10.0) <= 120.0));
    }

    #[test]
    fn generates_pdf_with_embedded_cjk_font_and_pages() {
        let source = br#"{"basics":{"name":"\u674e\u660e","summary":"\u4e2d\u6587\u7b80\u5386"},"work":[{"position":"Engineer","name":"Example","highlights":["Completed the project"]}]}"#;
        let content = project(&parse_resume(source).unwrap());
        let bytes = render(&content, Language::Zh).unwrap();
        assert!(bytes.starts_with(b"%PDF-"));
        assert!(bytes.len() > 10_000);
    }

    #[test]
    fn refuses_to_silently_omit_unsupported_glyphs() {
        let source = br#"{"basics":{"name":"Ada \ud83d\ude80"}}"#;
        let content = project(&parse_resume(source).unwrap());
        assert!(
            render(&content, Language::En)
                .unwrap_err()
                .contains("U+1F680")
        );
    }
}
