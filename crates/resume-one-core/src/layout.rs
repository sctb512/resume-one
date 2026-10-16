//! Font-measured layout shared by PDF export and desktop preview.
//! All positions are points in a bottom-left page coordinate system.

use crate::ThemeManifest;
use crate::render::{Language, title};
use crate::semantic::{Item, ResumeContent};
use crate::theme_style::{Rgb, SectionHeading, TextStyle, ThemeStyle};
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

    /// Ascender and positive descender in em units for hit-region construction.
    pub fn vertical_metrics(self) -> (f32, f32) {
        let bytes = match self {
            Self::Sans => SANS_FONT_BYTES,
            Self::Serif => SERIF_FONT_BYTES,
        };
        let face = Face::parse(bytes, 0).expect("validated bundled font");
        (
            face.ascender() as f32 / face.units_per_em() as f32,
            -(face.descender() as f32) / face.units_per_em() as f32,
        )
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
    pub target: Option<LayoutTarget>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayoutTarget {
    pub section: &'static str,
    pub index: Option<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LayoutAnchor {
    pub target: LayoutTarget,
    pub y_pt: f32,
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
pub struct LayoutFill {
    pub left_pt: f32,
    pub bottom_pt: f32,
    pub width_pt: f32,
    pub height_pt: f32,
    pub color: Rgb,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LayoutElement {
    Text(LayoutText),
    Rule(LayoutRule),
    Fill(LayoutFill),
    Anchor(LayoutAnchor),
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
    target: Option<LayoutTarget>,
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
            target: Some(LayoutTarget {
                section: "basics",
                index: None,
            }),
        }
    }
    fn content_width(&self) -> f32 {
        self.width - self.left - self.right
    }

    fn mark(&mut self, target: LayoutTarget) {
        self.target = Some(target);
        self.ops.push(LayoutElement::Anchor(LayoutAnchor {
            target,
            y_pt: self.y,
        }));
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
            target: self.target,
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

    fn header_text(&mut self, text: &str, style: &TextStyle, color: Rgb, centered: bool) {
        let kind = font_kind(style).expect("theme checked");
        for input_line in text.lines() {
            for line in self.wrap(input_line, style.size_pt, self.content_width(), kind) {
                let indent = if centered {
                    ((self.content_width() - self.text_width(&line, style.size_pt, kind)) / 2.0)
                        .max(0.0)
                } else {
                    0.0
                };
                self.line(
                    &line,
                    style.size_pt,
                    color,
                    kind,
                    indent,
                    style.size_pt * style.line_height,
                );
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
                target: None,
            }));
        }
        self.pages
    }
}
fn font_kind(style: &TextStyle) -> Result<FontKind, String> {
    if style.tracking_em != 0.0 {
        return Err("当前排版引擎暂不支持 typography.trackingEm；请设为 0".into());
    }
    match (style.family.as_str(), style.weight) {
        ("cjk-sans" | "system-sans", 400) => Ok(FontKind::Sans),
        ("cjk-serif" | "system-serif", 600) => Ok(FontKind::Serif),
        _ => Err(format!(
            "当前排版引擎暂不支持字体组合 {} / {}；当前支持 cjk-sans 400 和 cjk-serif 600",
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
            .is_some_and(|value| !matches!(value, "stacked" | "centered"))
        || composition["contact"]
            .as_str()
            .is_some_and(|value| !matches!(value, "inline" | "stacked"))
        || composition["sidebarWidthPct"].is_number()
        || composition["gapMm"].is_number()
        || composition["placements"]
            .as_object()
            .is_some_and(|value| !value.is_empty())
    {
        return Err(
            "当前排版引擎暂不支持该 composition 布局；仅支持单栏、堆叠/居中页眉与行内/逐行联系方式"
                .into(),
        );
    }
    if let Some(sections) = source["sections"].as_object() {
        for (id, value) in sections {
            if value["items"]
                .as_str()
                .is_some_and(|items| !matches!(items, "stack" | "compact"))
            {
                return Err(format!(
                    "当前排版引擎暂不支持 sections.{id}.items；目前支持 stack / compact"
                ));
            }
            if value["date"].as_str().is_some_and(|date| date != "inline") {
                return Err(format!(
                    "当前排版引擎暂不支持 sections.{id}.date；目前支持 inline"
                ));
            }
        }
    }
    if source["decorations"]
        .as_object()
        .is_some_and(|value| !value.is_empty())
    {
        return Err("当前排版引擎暂不支持 decorations 自定义装饰".into());
    }
    if source["assets"]["fonts"]
        .as_array()
        .is_some_and(|value| !value.is_empty())
    {
        return Err("当前排版引擎暂不支持主题自带字体 assets.fonts".into());
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

fn heading_height(style: &ThemeStyle, id: &str) -> f32 {
    let text = &style.typography.section_title;
    let leading = text.size_pt * text.line_height;
    let (ascent, descent) = font_kind(text).expect("theme checked").vertical_metrics();
    // The next item is baseline-positioned by the legacy stack renderer. Its
    // ascender must also be reserved below a filled box or underline.
    let following_ascent = (font_kind(&style.typography.body)
        .expect("theme checked")
        .vertical_metrics()
        .0
        * style.typography.body.size_pt
        * 1.06)
        .max(
            font_kind(&style.typography.meta)
                .expect("theme checked")
                .vertical_metrics()
                .0
                * style.typography.meta.size_pt,
        );
    if style.section(id).heading == SectionHeading::Plain {
        return (leading + 4.0).max(descent * text.size_pt + following_ascent + 4.0);
    }
    leading.max((ascent + descent) * text.size_pt)
        + following_ascent
        + if style.section(id).heading == SectionHeading::Filled {
            12.0
        } else {
            10.0
        }
}

fn section_heading(
    page: &mut PageWriter<'_>,
    id: &'static str,
    language: Language,
    style: &ThemeStyle,
) {
    let text = &style.typography.section_title;
    let kind = font_kind(text).expect("theme checked");
    let heading = style.section(id).heading;
    let height = heading_height(style, id);
    page.ensure(height);
    page.mark(LayoutTarget {
        section: id,
        index: None,
    });
    if heading == SectionHeading::Plain {
        page.line(
            title(id, language),
            text.size_pt,
            style.colors.accent,
            kind,
            0.0,
            height,
        );
        return;
    }
    let (ascent, descent) = kind.vertical_metrics();
    let leading = (text.size_pt * text.line_height).max((ascent + descent) * text.size_pt);
    let padding = if heading == SectionHeading::Filled {
        4.0
    } else {
        0.0
    };
    let top = page.y;
    if heading == SectionHeading::Filled {
        page.ops.push(LayoutElement::Fill(LayoutFill {
            left_pt: page.left,
            bottom_pt: top - leading - 2.0 * padding,
            width_pt: page.content_width(),
            height_pt: leading + 2.0 * padding,
            color: style.colors.accent,
        }));
    }
    page.ops.push(LayoutElement::Text(LayoutText {
        text: title(id, language).into(),
        x_pt: page.left
            + if heading == SectionHeading::Filled {
                6.0
            } else {
                0.0
            },
        baseline_y_pt: top
            - padding
            - (leading - (ascent + descent) * text.size_pt) / 2.0
            - ascent * text.size_pt,
        size_pt: text.size_pt,
        leading_pt: leading,
        color: if heading == SectionHeading::Filled {
            style.colors.paper
        } else {
            style.colors.accent
        },
        font: kind,
        target: page.target,
    }));
    if matches!(heading, SectionHeading::Line | SectionHeading::AccentLine) {
        page.ops.push(LayoutElement::Rule(LayoutRule {
            left_pt: page.left,
            right_pt: page.width - page.right,
            y_pt: top - leading - 4.0,
            thickness_pt: if heading == SectionHeading::Line {
                0.6
            } else {
                1.2
            },
            color: if heading == SectionHeading::Line {
                style.colors.border
            } else {
                style.colors.accent
            },
        }));
    }
    page.y = top - height;
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
    let section = style.section(section_id);
    let mut height = section.item_gap_pt();
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
        height += section.paragraph_gap_pt()
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

/// Check renderer capability without loading any resume content.
pub fn check_theme(theme: &ThemeManifest) -> Result<(), String> {
    validate_theme(theme, &ThemeStyle::from_manifest(theme))
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
            "内置字体缺少 U+{:04X} 字形，无法保证内容完整",
            character as u32
        ));
    }
    let mut page = PageWriter::new(sans_face, serif_face, style);
    page.mark(LayoutTarget {
        section: "basics",
        index: None,
    });
    let body = &style.typography.body;
    let name = &style.typography.name;
    let meta = &style.typography.meta;
    let ink = style.colors.ink;
    let accent = style.colors.accent;
    let muted = style.colors.muted;
    let section_gap = style.page.section_gap_mm * PT_PER_MM;
    if !content.name.is_empty() {
        page.header_text(&content.name, name, ink, style.centered_header);
    }
    if !content.label.is_empty() {
        page.header_text(&content.label, body, accent, style.centered_header);
    }
    if !content.contacts.is_empty() {
        page.header_text(
            &content.contacts.join(if style.stacked_contacts {
                "\n"
            } else {
                "   ·   "
            }),
            meta,
            muted,
            style.centered_header,
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
        let section = style.section(section_content.id);
        let heading_height = heading_height(style, section_content.id);
        if style.page.keep_heading_with_first_item {
            let first_item_height = section_content.items.first().map_or(0.0, |item| {
                item_height(&page, item, section_content.id, style)
            });
            page.ensure(section_gap + heading_height + first_item_height);
        }
        page.ensure(section_gap + heading_height);
        page.gap(section_gap);
        section_heading(&mut page, section_content.id, language, style);
        for (index, item) in section_content.items.iter().enumerate() {
            if style.page.avoid_split_items {
                let height = item_height(&page, item, section_content.id, style);
                page.ensure(height);
            }
            page.mark(LayoutTarget {
                section: section_content.id,
                index: Some(index),
            });
            page.gap(section.item_gap_pt());
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
                page.gap(section.paragraph_gap_pt());
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
    fn centered_header_centers_each_wrapped_line_only_and_preserves_contact_order() {
        let mut theme = crate::parse_theme(include_bytes!(
            "../../resume-one-cli/tests/fixtures/centered-header.theme.json"
        ))
        .unwrap();
        let mut content = project(&parse_resume(br#"{"basics":{"name":"Name","label":"Designer","summary":"Summary"},"work":[{"name":"Company"}]}"#).unwrap());
        content.name = "Long bilingual name 中文姓名 wrapped safely ".repeat(4);
        content.contacts = vec![
            "first@example.com".into(),
            "https://example.com/long/".repeat(8),
            "Last contact".into(),
        ];
        for paper in ["A4", "Letter"] {
            // Unequal margins catch centering around the page rather than its content area.
            theme.normalized["page"] = serde_json::json!({"size":paper,"marginsMm":{"top":16,"right":11,"bottom":16,"left":23}});
            let style = ThemeStyle::from_manifest(&theme);
            let result = layout_with_theme(&content, Language::En, &theme).unwrap();
            let measure = PageWriter::new(
                Face::parse(SANS_FONT_BYTES, 0).unwrap(),
                Face::parse(SERIF_FONT_BYTES, 0).unwrap(),
                &style,
            );
            let header: Vec<_> = result
                .pages
                .iter()
                .flatten()
                .filter_map(|e| match e {
                    LayoutElement::Text(t)
                        if t.target.is_some_and(|target| target.section == "basics")
                            && t.text != "Summary" =>
                    {
                        Some(t)
                    }
                    _ => None,
                })
                .collect();
            assert!(header.len() > 8, "long name and URL must wrap");
            let center = measure.left + measure.content_width() / 2.0;
            for text in &header {
                let width = measure.text_width(&text.text, text.size_pt, text.font);
                assert!((text.x_pt + width / 2.0 - center).abs() < 0.01);
                assert!(text.x_pt >= measure.left);
                assert!(text.x_pt + width <= measure.width - measure.right + 0.01);
            }
            let contact_start = header
                .iter()
                .position(|t| t.text == "first@example.com")
                .unwrap();
            assert!(header[contact_start + 1].text.starts_with("https://"));
            assert_eq!(header.last().unwrap().text, "Last contact");
            assert!(
                header
                    .windows(2)
                    .all(|pair| pair[0].baseline_y_pt > pair[1].baseline_y_pt)
            );
            for e in result.pages.iter().flatten() {
                if let LayoutElement::Text(t) = e
                    && ["Summary", "Work", "Company"].contains(&t.text.as_str())
                {
                    assert_eq!(t.x_pt, measure.left);
                }
            }
        }
    }

    #[test]
    fn header_defaults_are_legacy_identical_and_unsupported_variants_still_fail() {
        let mut theme = sections_theme();
        let mut content = project(
            &parse_resume(br#"{"basics":{"name":"Name","label":"Designer","summary":"Summary"}}"#)
                .unwrap(),
        );
        content.contacts = vec!["First".into(), "Second".into()];
        let legacy = layout_with_theme(&content, Language::En, &theme).unwrap();
        theme.normalized["composition"] =
            serde_json::json!({"preset":"single-column","header":"stacked","contact":"inline"});
        assert_eq!(
            legacy,
            layout_with_theme(&content, Language::En, &theme).unwrap()
        );
        for header in ["stacked", "centered"] {
            for contact in ["inline", "stacked"] {
                theme.normalized["composition"] =
                    serde_json::json!({"header":header,"contact":contact});
                assert!(layout_with_theme(&content, Language::En, &theme).is_ok());
            }
        }
        for composition in [
            serde_json::json!({"header":"split"}),
            serde_json::json!({"contact":"icons"}),
            serde_json::json!({"preset":"split-header"}),
        ] {
            theme.normalized["composition"] = composition;
            assert!(check_theme(&theme).unwrap_err().contains("composition"));
        }
    }

    fn sections_theme() -> ThemeManifest {
        crate::parse_theme(include_bytes!(
            "../../resume-one-cli/tests/fixtures/section-styles.theme.json"
        ))
        .unwrap()
    }

    #[test]
    fn heading_shapes_enclose_glyphs_and_compact_changes_only_spacing() {
        let theme = sections_theme();
        let style = ThemeStyle::from_manifest(&theme);
        let content = project(&parse_resume(br#"{"work":[{"name":"One","summary":"First"},{"name":"Two","summary":"Second"}],"projects":[{"name":"Project"}],"education":[{"institution":"School"}],"skills":[{"name":"Skill"}]}"#).unwrap());
        let result = layout_with_theme(&content, Language::En, &theme).unwrap();
        let elements = &result.pages[0];
        let fill = elements
            .iter()
            .find_map(|e| {
                if let LayoutElement::Fill(f) = e {
                    Some(f)
                } else {
                    None
                }
            })
            .unwrap();
        let heading = elements
            .iter()
            .find_map(|e| {
                if let LayoutElement::Text(t) = e {
                    (t.text == "Work").then_some(t)
                } else {
                    None
                }
            })
            .unwrap();
        let (ascent, descent) = heading.font.vertical_metrics();
        assert_eq!(heading.color, style.colors.paper);
        assert!(heading.color.contrast(fill.color) >= 4.5);
        assert!(heading.baseline_y_pt + ascent * heading.size_pt < fill.bottom_pt + fill.height_pt);
        assert!(heading.baseline_y_pt - descent * heading.size_pt > fill.bottom_pt);
        assert_eq!(heading.x_pt, fill.left_pt + 6.0);
        let first_item = elements
            .iter()
            .find_map(|e| {
                if let LayoutElement::Text(t) = e {
                    (t.target
                        == Some(LayoutTarget {
                            section: "work",
                            index: Some(0),
                        }))
                    .then_some(t)
                } else {
                    None
                }
            })
            .unwrap();
        assert!(
            first_item.baseline_y_pt + first_item.font.vertical_metrics().0 * first_item.size_pt
                <= fill.bottom_pt - 2.0
        );
        let rules: Vec<_> = elements
            .iter()
            .filter_map(|e| {
                if let LayoutElement::Rule(r) = e {
                    Some(r)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(rules.len(), 2);
        assert_eq!(
            (rules[0].color, rules[0].thickness_pt),
            (style.colors.accent, 1.2)
        );
        assert_eq!(
            (rules[1].color, rules[1].thickness_pt),
            (style.colors.border, 0.6)
        );
        let mut stacked = theme.clone();
        stacked.normalized["sections"]["work"]["items"] = serde_json::json!("stack");
        let stacked = layout_with_theme(&content, Language::En, &stacked).unwrap();
        let texts = |result: &ResumeLayout| {
            result
                .pages
                .iter()
                .flatten()
                .filter_map(|e| {
                    if let LayoutElement::Text(t) = e {
                        t.target.map(|_| t.text.clone())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(texts(&result), texts(&stacked));
        let second_y = |result: &ResumeLayout| {
            result
                .pages
                .iter()
                .flatten()
                .find_map(|e| {
                    if let LayoutElement::Text(t) = e {
                        (t.text == "Two").then_some(t.baseline_y_pt)
                    } else {
                        None
                    }
                })
                .unwrap()
        };
        assert!((second_y(&result) - second_y(&stacked) - 6.0).abs() < 0.01);
    }

    #[test]
    fn decorated_headers_stay_on_page_and_follow_first_items_across_pagination() {
        let mut theme = sections_theme();
        for heading in ["plain", "line", "accent-line", "filled"] {
            for paper in ["A4", "Letter"] {
                theme.normalized["page"] = serde_json::json!({"size":paper,"marginsMm":{"top":25,"bottom":25},"sectionGapMm":10});
                for id in [
                    "work",
                    "projects",
                    "education",
                    "skills",
                    "languages",
                    "awards",
                ] {
                    theme.normalized["sections"][id] =
                        serde_json::json!({"heading":heading,"items":"compact"});
                }
                let mut content = project(
                    &parse_resume(include_bytes!(
                        "../../resume-one-cli/tests/fixtures/visual-qa.json"
                    ))
                    .unwrap(),
                );
                content.sections[0].items = content.sections[0]
                    .items
                    .iter()
                    .cycle()
                    .take(16)
                    .cloned()
                    .collect();
                let result = layout_with_theme(&content, Language::Zh, &theme).unwrap();
                assert!(result.pages.len() > 2);
                for elements in &result.pages {
                    for (index, element) in elements.iter().enumerate() {
                        match element {
                            LayoutElement::Fill(f) => {
                                assert!(f.bottom_pt >= 25.0 * PT_PER_MM);
                                assert!(
                                    f.bottom_pt + f.height_pt
                                        <= (result.height_mm - 25.0) * PT_PER_MM + 0.01
                                );
                                assert!(matches!(
                                    elements.get(index + 1),
                                    Some(LayoutElement::Text(_))
                                ));
                            }
                            LayoutElement::Text(t)
                                if t.target.is_some_and(|target| {
                                    target.index.is_none() && target.section != "basics"
                                }) =>
                            {
                                let id = t.target.unwrap().section;
                                assert!(elements.iter().any(|e| matches!(e, LayoutElement::Anchor(a) if a.target.section == id && a.target.index == Some(0))), "orphan heading {id}");
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn explicit_default_sections_are_legacy_identical_but_future_constructs_are_rejected() {
        let mut theme = sections_theme();
        theme.normalized["sections"] = serde_json::json!({});
        let content = project(&parse_resume(br#"{"work":[{"name":"One"}]}"#).unwrap());
        let legacy = layout_with_theme(&content, Language::Zh, &theme).unwrap();
        theme.normalized["sections"] =
            serde_json::json!({"work":{"heading":"plain","items":"stack","date":"inline"}});
        assert_eq!(
            legacy,
            layout_with_theme(&content, Language::Zh, &theme).unwrap()
        );
        for value in [
            serde_json::json!({"items":"timeline"}),
            serde_json::json!({"items":"cards"}),
            serde_json::json!({"date":"right"}),
        ] {
            theme.normalized["sections"]["work"] = value;
            assert!(check_theme(&theme).unwrap_err().contains("sections.work"));
        }
        theme.normalized["sections"] = serde_json::json!({"skills":{"items":"chips"}});
        assert!(
            check_theme(&theme)
                .unwrap_err()
                .contains("sections.skills.items")
        );
    }

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
