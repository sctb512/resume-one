//! Font-measured layout shared by PDF export and desktop preview.
//! All positions are points in a bottom-left page coordinate system.

use crate::ThemeManifest;
use crate::render::{Language, title};
use crate::semantic::{Item, ResumeContent};
use crate::theme_style::{Rgb, TextStyle, ThemeStyle};
use ttf_parser::Face;

pub const SANS_FONT_BYTES: &[u8] = include_bytes!("../../../assets/fonts/NotoSansSC-Regular.ttf");
pub const SERIF_FONT_BYTES: &[u8] =
    include_bytes!("../../../assets/fonts/NotoSerifSC-SemiBold.ttf");
pub const PT_PER_MM: f32 = 72.0 / 25.4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontKind {
    Sans,
    Serif,
}

impl FontKind {
    pub fn family(self) -> &'static str {
        match self {
            Self::Sans => "Noto Sans SC",
            Self::Serif => "Noto Serif SC",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LayoutText {
    pub text: String,
    pub x_pt: f32,
    pub baseline_y_pt: f32,
    pub size_pt: f32,
    pub leading_pt: f32,
    pub color: Rgb,
    pub font: FontKind,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LayoutRule {
    pub left_pt: f32,
    pub right_pt: f32,
    pub y_pt: f32,
    pub thickness_pt: f32,
    pub color: Rgb,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LayoutElement {
    Text(LayoutText),
    Rule(LayoutRule),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResumeLayout {
    pub width_mm: f32,
    pub height_mm: f32,
    pub paper: Rgb,
    pub pages: Vec<Vec<LayoutElement>>,
}

struct PageWriter<'a> {
    sans_face: Face<'a>,
    serif_face: Face<'a>,
    pages: Vec<Vec<LayoutElement>>,
    ops: Vec<LayoutElement>,
    y: f32,
    width: f32,
    height: f32,
    left: f32,
    right: f32,
    top: f32,
    bottom: f32,
    muted: Rgb,
}

impl<'a> PageWriter<'a> {
    fn new(sans_face: Face<'a>, serif_face: Face<'a>, style: &ThemeStyle) -> Self {
        let width = style.page.width_mm * PT_PER_MM;
        let height = style.page.height_mm * PT_PER_MM;
        let top = style.page.margins.top * PT_PER_MM;
        Self {
            sans_face,
            serif_face,
            pages: Vec::new(),
            ops: Vec::new(),
            y: height - top,
            width,
            height,
            left: style.page.margins.left * PT_PER_MM,
            right: style.page.margins.right * PT_PER_MM,
            top,
            bottom: style.page.margins.bottom * PT_PER_MM,
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
        self.ops.push(LayoutElement::Text(LayoutText {
            text: text.into(),
            x_pt: self.left + indent,
            baseline_y_pt: self.y,
            size_pt: size,
            leading_pt: leading,
            color,
            font: kind,
        }));
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

    fn finish(mut self) -> Vec<Vec<LayoutElement>> {
        self.pages.push(self.ops);
        let count = self.pages.len();
        for (index, ops) in self.pages.iter_mut().enumerate() {
            ops.push(LayoutElement::Text(LayoutText {
                text: format!("{} / {}", index + 1, count),
                x_pt: self.width - self.right - 28.0,
                baseline_y_pt: (self.bottom / 2.0).max(5.0),
                size_pt: 8.0,
                leading_pt: 8.0,
                color: self.muted,
                font: FontKind::Sans,
            }));
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
    page.ops.push(LayoutElement::Rule(LayoutRule {
        left_pt: page.left,
        right_pt: page.width - page.right,
        y_pt: page.y,
        thickness_pt: thickness,
        color,
    }));
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

pub fn layout(content: &ResumeContent, language: Language) -> Result<ResumeLayout, String> {
    layout_with_style(content, language, &ThemeStyle::built_in_fallback())
}

pub fn layout_with_theme(
    content: &ResumeContent,
    language: Language,
    theme: &ThemeManifest,
) -> Result<ResumeLayout, String> {
    let style = ThemeStyle::from_manifest(theme);
    validate_theme(theme, &style)?;
    layout_with_style(content, language, &style)
}

fn layout_with_style(
    content: &ResumeContent,
    language: Language,
    style: &ThemeStyle,
) -> Result<ResumeLayout, String> {
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
    let mut page = PageWriter::new(sans_face, serif_face, style);
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
    Ok(ResumeLayout {
        width_mm: style.page.width_mm,
        height_mm: style.page.height_mm,
        paper: style.colors.paper,
        pages: page.finish(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{resume::parse_resume, semantic::project};

    #[test]
    fn shared_layout_keeps_multilingual_content_and_pagination_in_page_coordinates() {
        let source = serde_json::json!({
            "basics": {"name": "林清和", "summary": "跨平台排版验收"},
            "work": (0..30).map(|index| serde_json::json!({
                "name": format!("Example {index}"), "position": format!("Role {index}"),
                "summary": "中英文内容需要使用同一份排版结果。 Shared measured layout across screen and PDF.",
                "highlights": ["保留内容顺序并在页边距内换页。"]
            })).collect::<Vec<_>>()
        });
        let content = project(&parse_resume(&serde_json::to_vec(&source).unwrap()).unwrap());
        let result = layout(&content, Language::Zh).unwrap();
        assert!(result.pages.len() > 1);
        assert_eq!((result.width_mm, result.height_mm), (210.0, 297.0));
        assert_eq!(result, layout(&content, Language::Zh).unwrap());
        let mut texts = Vec::new();
        for (index, page) in result.pages.iter().enumerate() {
            for element in page {
                if let LayoutElement::Text(text) = element {
                    assert!(text.x_pt >= 0.0 && text.x_pt < result.width_mm * PT_PER_MM);
                    assert!(
                        text.baseline_y_pt > 0.0
                            && text.baseline_y_pt < result.height_mm * PT_PER_MM
                    );
                    texts.push(text.text.as_str());
                }
            }
            let Some(LayoutElement::Text(footer)) = page.last() else {
                panic!("page footer missing")
            };
            assert_eq!(
                footer.text,
                format!("{} / {}", index + 1, result.pages.len())
            );
        }
        let text = texts.join("\n");
        assert!(text.contains("林清和"));
        assert!(text.contains("Role 29"));
    }

    #[test]
    fn wraps_multilingual_text_without_exceeding_content_width() {
        let face = Face::parse(SANS_FONT_BYTES, 0).unwrap();
        let serif_face = Face::parse(SERIF_FONT_BYTES, 0).unwrap();
        let writer = PageWriter::new(face, serif_face, &ThemeStyle::built_in_fallback());
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
}
