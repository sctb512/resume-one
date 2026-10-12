//! PDF export with embedded Unicode fonts, measured wrapping and automatic pagination.

use printpdf::{
    Color, Line, LinePoint, Mm, Op, PaintMode, ParsedFont, PdfDocument, PdfFontHandle, PdfPage,
    PdfSaveOptions, Point, Polygon, PolygonRing, Pt, Rgb as PdfRgb, TextItem, WindingOrder,
};
use ttf_parser::Face;

use crate::ThemeManifest;
use crate::render::{Language, title};
use crate::semantic::{Item, ResumeContent};
use crate::theme_style::{Rgb, TextStyle, ThemeStyle};

const SANS_FONT_BYTES: &[u8] = include_bytes!("../../../assets/fonts/NotoSansSC-Regular.ttf");
const SERIF_FONT_BYTES: &[u8] = include_bytes!("../../../assets/fonts/NotoSerifSC-SemiBold.ttf");
const PT_PER_MM: f32 = 72.0 / 25.4;

#[derive(Clone, Copy)]
enum FontKind {
    Sans,
    Serif,
}

fn pdf_color(color: Rgb) -> Color {
    Color::Rgb(PdfRgb::new(
        color.r as f32 / 255.0,
        color.g as f32 / 255.0,
        color.b as f32 / 255.0,
        None,
    ))
}

struct PageWriter<'a> {
    sans_font: PdfFontHandle,
    serif_font: PdfFontHandle,
    sans_face: Face<'a>,
    serif_face: Face<'a>,
    pages: Vec<Vec<Op>>,
    ops: Vec<Op>,
    y: f32,
    width: f32,
    height: f32,
    left: f32,
    right: f32,
    top: f32,
    bottom: f32,
    paper: Rgb,
    muted: Rgb,
}

impl<'a> PageWriter<'a> {
    fn new(
        sans_font: PdfFontHandle,
        serif_font: PdfFontHandle,
        sans_face: Face<'a>,
        serif_face: Face<'a>,
        style: &ThemeStyle,
    ) -> Self {
        let width = style.page.width_mm * PT_PER_MM;
        let height = style.page.height_mm * PT_PER_MM;
        let left = style.page.margins.left * PT_PER_MM;
        let right = style.page.margins.right * PT_PER_MM;
        let top = style.page.margins.top * PT_PER_MM;
        let bottom = style.page.margins.bottom * PT_PER_MM;
        Self {
            sans_font,
            serif_font,
            sans_face,
            serif_face,
            pages: Vec::new(),
            ops: Vec::new(),
            y: height - top,
            width,
            height,
            left,
            right,
            top,
            bottom,
            paper: style.colors.paper,
            muted: style.colors.muted,
        }
    }

    fn content_width(&self) -> f32 {
        self.width - self.left - self.right
    }

    fn face(&self, kind: FontKind) -> &Face<'a> {
        match kind {
            FontKind::Sans => &self.sans_face,
            FontKind::Serif => &self.serif_face,
        }
    }

    fn font(&self, kind: FontKind) -> PdfFontHandle {
        match kind {
            FontKind::Sans => self.sans_font.clone(),
            FontKind::Serif => self.serif_font.clone(),
        }
    }

    fn ensure(&mut self, height: f32) {
        let height = height.min(self.height - self.top - self.bottom);
        if self.y - height < self.bottom {
            self.pages.push(std::mem::take(&mut self.ops));
            self.y = self.height - self.top;
        }
    }

    fn gap(&mut self, height: f32) {
        self.ensure(height);
        self.y -= height;
    }

    fn line(
        &mut self,
        text: &str,
        size: f32,
        color: Rgb,
        kind: FontKind,
        indent: f32,
        leading: f32,
    ) {
        self.ensure(leading);
        self.ops.extend([
            Op::StartTextSection,
            Op::SetTextCursor {
                pos: Point {
                    x: Pt(self.left + indent),
                    y: Pt(self.y),
                },
            },
            Op::SetFont {
                font: self.font(kind),
                size: Pt(size),
            },
            Op::SetFillColor {
                col: pdf_color(color),
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
        color: Rgb,
        kind: FontKind,
        indent: f32,
        leading: f32,
    ) {
        for input_line in text.lines() {
            for line in self.wrap(input_line, size, self.content_width() - indent, kind) {
                self.line(&line, size, color, kind, indent, leading);
            }
        }
    }

    fn text_width(&self, text: &str, size: f32, kind: FontKind) -> f32 {
        let face = self.face(kind);
        let units = face.units_per_em() as f32;
        text.chars()
            .map(|character| {
                face.glyph_index(character)
                    .and_then(|glyph| face.glyph_hor_advance(glyph))
                    .unwrap_or(face.units_per_em()) as f32
                    * size
                    / units
            })
            .sum()
    }

    fn wrap(&self, input: &str, size: f32, maximum: f32, kind: FontKind) -> Vec<String> {
        let input = input.trim();
        if input.is_empty() {
            return Vec::new();
        }
        let mut lines = Vec::new();
        let mut current = String::new();
        for character in input.chars() {
            current.push(character);
            if self.text_width(&current, size, kind) <= maximum {
                continue;
            }
            current.pop();
            if current.is_empty() {
                current.push(character);
                continue;
            }
            if "，。、；：！？）》」』".contains(character)
                && current.chars().count() > 1
                && let Some(previous) = current.pop()
            {
                lines.push(current.trim_end().to_owned());
                current.clear();
                current.push(previous);
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
            if self.paper
                != (Rgb {
                    r: 255,
                    g: 255,
                    b: 255,
                })
            {
                ops.splice(
                    0..0,
                    [
                        Op::SetFillColor {
                            col: pdf_color(self.paper),
                        },
                        Op::DrawPolygon {
                            polygon: Polygon {
                                rings: vec![PolygonRing {
                                    points: vec![
                                        LinePoint {
                                            p: Point {
                                                x: Pt(0.0),
                                                y: Pt(0.0),
                                            },
                                            bezier: false,
                                        },
                                        LinePoint {
                                            p: Point {
                                                x: Pt(self.width),
                                                y: Pt(0.0),
                                            },
                                            bezier: false,
                                        },
                                        LinePoint {
                                            p: Point {
                                                x: Pt(self.width),
                                                y: Pt(self.height),
                                            },
                                            bezier: false,
                                        },
                                        LinePoint {
                                            p: Point {
                                                x: Pt(0.0),
                                                y: Pt(self.height),
                                            },
                                            bezier: false,
                                        },
                                    ],
                                }],
                                mode: PaintMode::Fill,
                                winding_order: WindingOrder::NonZero,
                            },
                        },
                    ],
                );
            }
            let label = format!("{} / {}", index + 1, count);
            ops.extend([
                Op::StartTextSection,
                Op::SetTextCursor {
                    pos: Point {
                        x: Pt(self.width - self.right - 28.0),
                        y: Pt((self.bottom / 2.0).max(5.0)),
                    },
                },
                Op::SetFont {
                    font: self.sans_font.clone(),
                    size: Pt(8.0),
                },
                Op::SetFillColor {
                    col: pdf_color(self.muted),
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

fn font_kind(style: &TextStyle) -> Result<FontKind, String> {
    if style.tracking_em != 0.0 {
        return Err("PDF 暂不支持主题 typography.trackingEm；请设为 0".into());
    }
    match (style.family.as_str(), style.weight) {
        ("cjk-sans" | "system-sans", 400) => Ok(FontKind::Sans),
        ("cjk-serif" | "system-serif", 600) => Ok(FontKind::Serif),
        _ => Err(format!(
            "PDF 暂不支持字体组合 {} / {}；当前支持 cjk-sans 400 和 cjk-serif 600",
            style.family, style.weight
        )),
    }
}

fn validate_theme(theme: &ThemeManifest, style: &ThemeStyle) -> Result<(), String> {
    let source = &theme.normalized;
    let composition = &source["composition"];
    if style.composition_preset != "single-column"
        || composition["header"]
            .as_str()
            .is_some_and(|value| value != "stacked")
        || composition["contact"]
            .as_str()
            .is_some_and(|value| value != "inline")
        || composition["sidebarWidthPct"].is_number()
        || composition["gapMm"].is_number()
        || composition["placements"]
            .as_object()
            .is_some_and(|value| !value.is_empty())
    {
        return Err("PDF 暂不支持该 composition 布局；仅支持单栏、堆叠页眉与行内联系方式".into());
    }
    if source["sections"]
        .as_object()
        .is_some_and(|value| !value.is_empty())
    {
        return Err("PDF 暂不支持 sections 自定义模块构件".into());
    }
    if source["decorations"]
        .as_object()
        .is_some_and(|value| !value.is_empty())
    {
        return Err("PDF 暂不支持 decorations 自定义装饰".into());
    }
    if source["assets"]["fonts"]
        .as_array()
        .is_some_and(|value| !value.is_empty())
    {
        return Err("PDF 暂不支持主题自带字体 assets.fonts".into());
    }
    for text_style in [
        &style.typography.body,
        &style.typography.name,
        &style.typography.section_title,
        &style.typography.meta,
    ] {
        font_kind(text_style)?;
    }
    Ok(())
}

fn add_rule(page: &mut PageWriter<'_>, color: Rgb, thickness: f32) {
    page.ops.push(Op::SetOutlineColor {
        col: pdf_color(color),
    });
    page.ops.push(Op::SetOutlineThickness { pt: Pt(thickness) });
    page.ops.push(Op::DrawLine {
        line: Line {
            points: vec![
                LinePoint {
                    p: Point {
                        x: Pt(page.left),
                        y: Pt(page.y),
                    },
                    bezier: false,
                },
                LinePoint {
                    p: Point {
                        x: Pt(page.width - page.right),
                        y: Pt(page.y),
                    },
                    bezier: false,
                },
            ],
            is_closed: false,
        },
    });
}

fn wrapped_height(
    page: &PageWriter<'_>,
    text: &str,
    size: f32,
    font: FontKind,
    indent: f32,
    leading: f32,
) -> f32 {
    text.lines()
        .map(|line| {
            page.wrap(line, size, page.content_width() - indent, font)
                .len() as f32
                * leading
        })
        .sum()
}

fn item_height(page: &PageWriter<'_>, item: &Item, section_id: &str, style: &ThemeStyle) -> f32 {
    let body = &style.typography.body;
    let meta = &style.typography.meta;
    let body_font = font_kind(body).expect("theme checked");
    let meta_font = font_kind(meta).expect("theme checked");
    let mut height = 4.0;
    if !item.heading.is_empty() {
        height += wrapped_height(
            page,
            &item.heading,
            body.size_pt * 1.06,
            body_font,
            0.0,
            body.size_pt * body.line_height * 1.06,
        );
    }
    if !item.details.is_empty() {
        height += wrapped_height(
            page,
            &item.details.join("   ·   "),
            meta.size_pt,
            meta_font,
            0.0,
            meta.size_pt * meta.line_height,
        );
    }
    for paragraph in &item.paragraphs {
        height += 3.0
            + wrapped_height(
                page,
                paragraph,
                body.size_pt,
                body_font,
                0.0,
                body.size_pt * body.line_height,
            );
    }
    if (section_id == "skills" || section_id == "interests") && !item.bullets.is_empty() {
        height += wrapped_height(
            page,
            &item.bullets.join("   ·   "),
            meta.size_pt,
            meta_font,
            0.0,
            meta.size_pt * meta.line_height,
        );
    } else {
        for bullet in &item.bullets {
            height += wrapped_height(
                page,
                &format!("•  {bullet}"),
                body.size_pt,
                body_font,
                11.0,
                body.size_pt * body.line_height,
            );
        }
    }
    if let Some(url) = &item.url {
        height += wrapped_height(
            page,
            url,
            meta.size_pt,
            meta_font,
            0.0,
            meta.size_pt * meta.line_height,
        );
    }
    height
}

pub fn render(content: &ResumeContent, language: Language) -> Result<Vec<u8>, String> {
    render_with_style(content, language, &ThemeStyle::built_in_fallback())
}

pub fn render_with_theme(
    content: &ResumeContent,
    language: Language,
    theme: &ThemeManifest,
) -> Result<Vec<u8>, String> {
    let style = ThemeStyle::from_manifest(theme);
    validate_theme(theme, &style)?;
    render_with_style(content, language, &style)
}

fn render_with_style(
    content: &ResumeContent,
    language: Language,
    style: &ThemeStyle,
) -> Result<Vec<u8>, String> {
    let mut font_warnings = Vec::new();
    let sans = ParsedFont::from_bytes(SANS_FONT_BYTES, 0, &mut font_warnings)
        .ok_or_else(|| "内置无衬线字体无法解析".to_owned())?;
    let serif = ParsedFont::from_bytes(SERIF_FONT_BYTES, 0, &mut font_warnings)
        .ok_or_else(|| "内置衬线字体无法解析".to_owned())?;
    let sans_face = Face::parse(SANS_FONT_BYTES, 0).map_err(|error| error.to_string())?;
    let serif_face = Face::parse(SERIF_FONT_BYTES, 0).map_err(|error| error.to_string())?;
    if let Some(character) =
        crate::render::plain_text(content, language)
            .chars()
            .find(|character| {
                !character.is_whitespace()
                    && (sans_face.glyph_index(*character).is_none()
                        || serif_face.glyph_index(*character).is_none())
            })
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
    let sans_id = document.add_font(&sans);
    let serif_id = document.add_font(&serif);
    let mut page = PageWriter::new(
        PdfFontHandle::External(sans_id),
        PdfFontHandle::External(serif_id),
        sans_face,
        serif_face,
        style,
    );
    let body = &style.typography.body;
    let name = &style.typography.name;
    let section = &style.typography.section_title;
    let meta = &style.typography.meta;
    let ink = style.colors.ink;
    let accent = style.colors.accent;
    let muted = style.colors.muted;
    let section_gap = style.page.section_gap_mm * PT_PER_MM;
    if !content.name.is_empty() {
        page.wrapped(
            &content.name,
            name.size_pt,
            ink,
            font_kind(name)?,
            0.0,
            name.size_pt * name.line_height,
        );
    }
    if !content.label.is_empty() {
        page.wrapped(
            &content.label,
            body.size_pt,
            accent,
            font_kind(body)?,
            0.0,
            body.size_pt * body.line_height,
        );
    }
    if !content.contacts.is_empty() {
        page.wrapped(
            &content.contacts.join("   ·   "),
            meta.size_pt,
            muted,
            font_kind(meta)?,
            0.0,
            meta.size_pt * meta.line_height,
        );
    }
    page.gap(section_gap.max(4.0));
    if style.layout != "minimal" {
        let (rule_color, thickness) = match style.layout.as_str() {
            "rules" => (accent, 1.3),
            "modern" => (accent, 2.0),
            _ => (style.colors.border, 0.7),
        };
        add_rule(&mut page, rule_color, thickness);
        page.gap(body.size_pt * 1.2 + 4.0);
    }
    if !content.summary.is_empty() {
        page.wrapped(
            &content.summary,
            body.size_pt,
            ink,
            font_kind(body)?,
            0.0,
            body.size_pt * body.line_height,
        );
        page.gap(section_gap / 2.0);
    }
    for section_content in &content.sections {
        if style.page.keep_heading_with_first_item {
            let first_item_height = section_content.items.first().map_or(0.0, |item| {
                item_height(&page, item, section_content.id, style)
            });
            page.ensure(
                section_gap + section.size_pt * section.line_height + 4.0 + first_item_height,
            );
        }
        page.gap(section_gap);
        page.line(
            title(section_content.id, language),
            section.size_pt,
            accent,
            font_kind(section)?,
            0.0,
            section.size_pt * section.line_height + 4.0,
        );
        for item in &section_content.items {
            if style.page.avoid_split_items {
                let height = item_height(&page, item, section_content.id, style);
                page.ensure(height);
            }
            page.gap(4.0);
            if !item.heading.is_empty() {
                page.wrapped(
                    &item.heading,
                    body.size_pt * 1.06,
                    ink,
                    font_kind(body)?,
                    0.0,
                    body.size_pt * body.line_height * 1.06,
                );
            }
            if !item.details.is_empty() {
                page.wrapped(
                    &item.details.join("   ·   "),
                    meta.size_pt,
                    muted,
                    font_kind(meta)?,
                    0.0,
                    meta.size_pt * meta.line_height,
                );
            }
            for paragraph in &item.paragraphs {
                page.gap(3.0);
                page.wrapped(
                    paragraph,
                    body.size_pt,
                    ink,
                    font_kind(body)?,
                    0.0,
                    body.size_pt * body.line_height,
                );
            }
            if (section_content.id == "skills" || section_content.id == "interests")
                && !item.bullets.is_empty()
            {
                page.wrapped(
                    &item.bullets.join("   ·   "),
                    meta.size_pt,
                    muted,
                    font_kind(meta)?,
                    0.0,
                    meta.size_pt * meta.line_height,
                );
            } else {
                for bullet in &item.bullets {
                    page.wrapped(
                        &format!("•  {bullet}"),
                        body.size_pt,
                        ink,
                        font_kind(body)?,
                        11.0,
                        body.size_pt * body.line_height,
                    );
                }
            }
            if let Some(url) = &item.url {
                page.wrapped(
                    url,
                    meta.size_pt,
                    accent,
                    font_kind(meta)?,
                    0.0,
                    meta.size_pt * meta.line_height,
                );
            }
        }
    }
    let pages = page
        .finish()
        .into_iter()
        .map(|ops| PdfPage::new(Mm(style.page.width_mm), Mm(style.page.height_mm), ops))
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
        let face = Face::parse(SANS_FONT_BYTES, 0).unwrap();
        let serif_face = Face::parse(SERIF_FONT_BYTES, 0).unwrap();
        let writer = PageWriter::new(
            PdfFontHandle::Builtin(printpdf::BuiltinFont::Helvetica),
            PdfFontHandle::Builtin(printpdf::BuiltinFont::TimesRoman),
            face,
            serif_face,
            &ThemeStyle::built_in_fallback(),
        );
        let lines = writer.wrap(
            "这是一段很长的中文内容 with several English words and more content",
            10.0,
            120.0,
            FontKind::Sans,
        );
        assert!(lines.len() > 1);
        assert!(
            lines
                .iter()
                .all(|line| writer.text_width(line, 10.0, FontKind::Sans) <= 120.0)
        );
        let cjk_lines = writer.wrap("汉汉汉汉汉汉。继续", 10.0, 60.0, FontKind::Sans);
        assert!(cjk_lines.iter().all(|line| !line.starts_with('。')));
        assert!(
            cjk_lines
                .iter()
                .all(|line| writer.text_width(line, 10.0, FontKind::Sans) <= 60.0)
        );
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
