//! Font-measured layout shared by PDF export and desktop preview.
//! All positions are points in a bottom-left page coordinate system.

use crate::ThemeManifest;
use crate::render::Language;
use crate::semantic::{Item, ResumeContent};
use crate::theme_style::{DatePlacement, Rgb, SectionHeading, TextStyle, ThemeStyle};
use ttf_parser::Face;
use unicode_segmentation::UnicodeSegmentation;
mod sections;

pub const SANS_FONT_BYTES: &[u8] = include_bytes!("../../../assets/fonts/NotoSansSC-Regular.ttf");
pub const SERIF_FONT_BYTES: &[u8] =
    include_bytes!("../../../assets/fonts/NotoSerifSC-SemiBold.ttf");
pub const PT_PER_MM: f32 = 72.0 / 25.4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FontKind {
    Sans,
    Serif,
    Local(u16),
}

impl FontKind {
    pub fn is_italic(self) -> bool {
        matches!(self, Self::Local(id) if crate::fonts::get(id).italic)
    }

    pub fn family(self) -> String {
        match self {
            Self::Sans => "Noto Sans SC".into(),
            Self::Serif => "Noto Serif SC".into(),
            Self::Local(id) => crate::fonts::get(id).family,
        }
    }

    /// Ascender and positive descender in em units for hit-region construction.
    pub fn vertical_metrics(self) -> (f32, f32) {
        if let Self::Local(id) = self {
            let font = crate::fonts::get(id);
            let face = Face::parse(&font.bytes, 0).expect("validated local font");
            return (
                face.ascender() as f32 / face.units_per_em() as f32,
                -(face.descender() as f32) / face.units_per_em() as f32,
            );
        }
        let bytes = match self {
            Self::Sans => SANS_FONT_BYTES,
            Self::Serif => SERIF_FONT_BYTES,
            Self::Local(_) => unreachable!(),
        };
        let face = Face::parse(bytes, 0).expect("validated bundled font");
        (
            face.ascender() as f32 / face.units_per_em() as f32,
            -(face.descender() as f32) / face.units_per_em() as f32,
        )
    }

    /// The same bundled-font advances used for print layout, in points.
    pub fn text_width_pt(self, text: &str, size: f32) -> f32 {
        if let Self::Local(id) = self {
            let font = crate::fonts::get(id);
            let face = Face::parse(&font.bytes, 0).expect("validated local font");
            let fallback = Face::parse(SANS_FONT_BYTES, 0).expect("bundled font");
            return text
                .graphemes(true)
                .map(|cluster| {
                    let face = if cluster
                        .chars()
                        .all(|c| c.is_whitespace() || face.glyph_index(c).is_some())
                    {
                        &face
                    } else {
                        &fallback
                    };
                    measured_width(face, cluster, size)
                })
                .sum();
        }
        let bytes = match self {
            Self::Sans => SANS_FONT_BYTES,
            Self::Serif => SERIF_FONT_BYTES,
            Self::Local(_) => unreachable!(),
        };
        measured_width(
            &Face::parse(bytes, 0).expect("validated bundled font"),
            text,
            size,
        )
    }
}

fn measured_width(face: &Face<'_>, text: &str, size: f32) -> f32 {
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

#[derive(Debug, Clone, PartialEq)]
pub struct LayoutText {
    pub text: String,
    pub x_pt: f32,
    pub baseline_y_pt: f32,
    pub size_pt: f32,
    pub leading_pt: f32,
    /// Extra advance between extended grapheme clusters, not after the last.
    pub tracking_pt: f32,
    pub color: Rgb,
    pub font: FontKind,
    pub target: Option<LayoutTarget>,
    /// Outer interaction lane captured at mark time (before card/rail insets).
    /// Travels with continuation text; never inferred from glyph positions.
    pub target_lane: Option<(f32, f32)>,
}

impl LayoutText {
    pub fn width_pt(&self) -> f32 {
        tracked_width(self.font, &self.text, self.size_pt, self.tracking_pt)
    }

    /// Shared print/preview cluster offsets. Zero tracking retains the complete
    /// shaped line; combining marks never gain an internal tracking gap.
    pub fn paint_runs(&self) -> Vec<(&str, f32)> {
        if self.tracking_pt == 0.0 {
            return vec![(&self.text, 0.0)];
        }
        let mut x = 0.0;
        self.text
            .graphemes(true)
            .map(|cluster| {
                let at = x;
                x += self.font.text_width_pt(cluster, self.size_pt) + self.tracking_pt;
                (cluster, at)
            })
            .collect()
    }
}

fn tracked_width(font: FontKind, text: &str, size: f32, tracking: f32) -> f32 {
    font.text_width_pt(text, size) + tracking_extra(text, tracking)
}

fn tracking_extra(text: &str, tracking: f32) -> f32 {
    if tracking == 0.0 {
        0.0
    } else {
        text.graphemes(true).count().saturating_sub(1) as f32 * tracking
    }
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
    pub left_pt: f32,
    pub right_pt: f32,
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
    target_lane: Option<(f32, f32)>,
}

pub(crate) struct SideDateLines {
    pub heading: Vec<String>,
    pub date: Vec<String>,
    pub same_line: bool,
    pub date_width_pt: f32,
}

struct HeaderLine {
    text: String,
    size: f32,
    leading: f32,
    tracking: f32,
    color: Rgb,
    font: FontKind,
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
            target_lane: Some((
                style.page.margins.left * PT_PER_MM,
                width - style.page.margins.right * PT_PER_MM,
            )),
        }
    }
    fn content_width(&self) -> f32 {
        self.width - self.left - self.right
    }

    fn mark(&mut self, target: LayoutTarget) {
        self.target = Some(target);
        self.target_lane = Some((self.left, self.width - self.right));
        self.ops.push(LayoutElement::Anchor(LayoutAnchor {
            target,
            y_pt: self.y,
            left_pt: self.left,
            right_pt: self.width - self.right,
        }));
    }

    fn face(&self, kind: FontKind) -> Option<&Face<'a>> {
        match kind {
            FontKind::Sans => Some(&self.sans_face),
            FontKind::Serif => Some(&self.serif_face),
            FontKind::Local(_) => None,
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

    fn line(&mut self, text: &str, style: &TextStyle, color: Rgb, indent: f32, leading: f32) {
        self.ensure(leading);
        self.ops.push(LayoutElement::Text(LayoutText {
            text: style.display_text(text).into_owned(),
            x_pt: self.left + indent,
            baseline_y_pt: self.y,
            size_pt: style.size_pt,
            leading_pt: leading,
            tracking_pt: style.tracking_pt(),
            color: style.color_or(color),
            font: font_kind(style).expect("theme checked"),
            target: self.target,
            target_lane: self.target_lane,
        }));
        self.y -= leading;
    }
    fn wrapped(&mut self, text: &str, style: &TextStyle, color: Rgb, indent: f32) {
        for input_line in text.lines() {
            for line in self.wrap_style(input_line, style, self.content_width() - indent) {
                self.line(
                    &line,
                    style,
                    color,
                    indent,
                    style.size_pt * style.line_height,
                );
            }
        }
    }

    fn header_text(&mut self, text: &str, style: &TextStyle, color: Rgb, centered: bool) {
        for input_line in text.lines() {
            for line in self.wrap_style(input_line, style, self.content_width()) {
                let indent = if centered {
                    ((self.content_width() - self.styled_width(&line, style)) / 2.0).max(0.0)
                } else {
                    0.0
                };
                self.line(
                    &line,
                    style,
                    color,
                    indent,
                    style.size_pt * style.line_height,
                );
            }
        }
    }

    fn split_header(&mut self, content: &ResumeContent, style: &ThemeStyle) {
        // Fixed two-column header: the body remains full width. The right cell
        // has an inner gutter, matching the editable DOCX table.
        let split = self.content_width() * 0.58;
        let gutter = 14.0;
        let right_width = self.content_width() - split - gutter;
        let mut left_lines = Vec::new();
        for (value, text_style, color) in [
            (&content.name, &style.typography.name, style.colors.ink),
            (&content.label, &style.typography.label, style.colors.accent),
        ] {
            let font = font_kind(text_style).expect("theme checked");
            for input in value.lines() {
                for text in self.wrap_style(input, text_style, split) {
                    left_lines.push(HeaderLine {
                        text,
                        size: text_style.size_pt,
                        leading: text_style.size_pt * text_style.line_height,
                        tracking: text_style.tracking_pt(),
                        color: text_style.color_or(color),
                        font,
                    });
                }
            }
        }
        let meta = &style.typography.meta;
        let font = font_kind(meta).expect("theme checked");
        let contacts = content.contacts.join(if style.stacked_contacts {
            "\n"
        } else {
            "   ·   "
        });
        let mut contact_lines = Vec::new();
        if style.stacked_contacts {
            for contact in &content.contacts {
                for input in contact.lines() {
                    contact_lines.extend(self.wrap_style(input, meta, right_width));
                }
            }
        } else {
            let mut current = String::new();
            for contact in &content.contacts {
                for input in contact
                    .lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty())
                {
                    let candidate = if current.is_empty() {
                        input.to_owned()
                    } else {
                        format!("{current}   ·   {input}")
                    };
                    if self.styled_width(&candidate, meta) <= right_width {
                        current = meta.display_text(&candidate).into_owned();
                        continue;
                    }
                    if !current.is_empty() {
                        contact_lines.push(std::mem::take(&mut current));
                    }
                    let wrapped = self.wrap_style(input, meta, right_width);
                    if wrapped.len() == 1 {
                        current = wrapped.into_iter().next().unwrap();
                    } else {
                        contact_lines.extend(wrapped);
                    }
                }
            }
            if !current.is_empty() {
                contact_lines.push(current);
            }
        }
        let right_lines: Vec<_> = contact_lines
            .into_iter()
            .map(|text| HeaderLine {
                text,
                size: meta.size_pt,
                leading: meta.size_pt * meta.line_height,
                tracking: meta.tracking_pt(),
                color: meta.color_or(style.colors.muted),
                font,
            })
            .collect();
        let left_height: f32 = left_lines.iter().map(|line| line.leading).sum();
        let right_height: f32 = right_lines.iter().map(|line| line.leading).sum();
        let first_ascent = |lines: &[HeaderLine]| {
            lines
                .first()
                .map_or(0.0, |line| line.font.vertical_metrics().0 * line.size)
        };
        let height = first_ascent(&left_lines).max(first_ascent(&right_lines))
            + left_height.max(right_height);
        if height > self.height - self.top - self.bottom {
            // A pathological header must flow rather than escape the page.
            self.header_text(
                &content.name,
                &style.typography.name,
                style.colors.ink,
                false,
            );
            self.header_text(
                &content.label,
                &style.typography.label,
                style.colors.accent,
                false,
            );
            self.header_text(&contacts, meta, style.colors.muted, false);
            return;
        }
        self.ensure(height);
        let visible_top = self.y;
        for (lines, right_aligned) in [(&left_lines, false), (&right_lines, true)] {
            let mut baseline = visible_top - first_ascent(lines);
            for line in lines {
                let width = tracked_width(line.font, &line.text, line.size, line.tracking);
                self.ops.push(LayoutElement::Text(LayoutText {
                    text: line.text.clone(),
                    x_pt: if right_aligned {
                        self.left + split + gutter + right_width - width
                    } else {
                        self.left
                    },
                    baseline_y_pt: baseline,
                    size_pt: line.size,
                    leading_pt: line.leading,
                    tracking_pt: line.tracking,
                    color: line.color,
                    font: line.font,
                    target: self.target,
                    target_lane: self.target_lane,
                }));
                baseline -= line.leading;
            }
        }
        self.y -= height;
    }

    fn text_width(&self, text: &str, size: f32, kind: FontKind) -> f32 {
        self.face(kind).map_or_else(
            || kind.text_width_pt(text, size),
            |face| measured_width(face, text, size),
        )
    }

    fn styled_width(&self, text: &str, style: &TextStyle) -> f32 {
        let display = style.display_text(text);
        tracked_width(
            font_kind(style).expect("theme checked"),
            &display,
            style.size_pt,
            style.tracking_pt(),
        )
    }

    fn wrap_style(&self, input: &str, style: &TextStyle, maximum: f32) -> Vec<String> {
        self.wrap_tracked(
            &style.display_text(input),
            style.size_pt,
            maximum,
            font_kind(style).expect("theme checked"),
            style.tracking_pt(),
        )
    }

    #[cfg(test)]
    fn wrap(&self, input: &str, size: f32, maximum: f32, kind: FontKind) -> Vec<String> {
        self.wrap_tracked(input, size, maximum, kind, 0.0)
    }

    fn wrap_tracked(
        &self,
        input: &str,
        size: f32,
        maximum: f32,
        kind: FontKind,
        tracking: f32,
    ) -> Vec<String> {
        let input = input.trim();
        if input.is_empty() {
            return Vec::new();
        }
        let mut lines = Vec::new();
        let mut current = String::new();
        for cluster in input.graphemes(true) {
            current.push_str(cluster);
            if self.text_width(&current, size, kind) + tracking_extra(&current, tracking) <= maximum
            {
                continue;
            }
            current.truncate(current.len() - cluster.len());
            if current.is_empty() {
                current.push_str(cluster);
                continue;
            }
            if "，。、；：！？）》」』".contains(cluster)
                && current.graphemes(true).count() > 1
                && let Some(previous) = current.graphemes(true).next_back().map(str::to_owned)
            {
                current.truncate(current.len() - previous.len());
                lines.push(current.trim_end().to_owned());
                current.clear();
                current.push_str(&previous);
                current.push_str(cluster);
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
                current.push_str(cluster);
                continue;
            }
            lines.push(current.trim_end().to_owned());
            current.clear();
            if !cluster.chars().all(char::is_whitespace) {
                current.push_str(cluster);
            }
        }
        if !current.trim().is_empty() {
            lines.push(current.trim_end().to_owned());
        }
        lines
    }

    fn side_date_lines(
        &self,
        heading: &str,
        date: &str,
        heading_style: &TextStyle,
        meta: &TextStyle,
    ) -> SideDateLines {
        let heading_size = heading_style.size_pt;
        let date_width = self.styled_width(date, meta);
        let same_line = date_width <= self.content_width() * 0.42;
        let heading_width = if same_line {
            (self.content_width() - date_width - 12.0).max(heading_size)
        } else {
            self.content_width()
        };
        SideDateLines {
            heading: self.wrap_style(heading, heading_style, heading_width),
            date: self.wrap_style(date, meta, self.content_width()),
            same_line,
            date_width_pt: date_width,
        }
    }

    fn side_dated_heading(
        &mut self,
        heading: &str,
        date: &str,
        placement: DatePlacement,
        style: &ThemeStyle,
    ) {
        let body = &style.typography.item_title;
        let meta = &style.typography.side_date;
        let ink = style.colors.ink;
        let muted = style.colors.muted;
        let lines = self.side_date_lines(heading, date, body, meta);
        let date = meta.display_text(date);
        let heading_size = body.size_pt;
        let heading_leading = body.size_pt * body.line_height;
        let date_leading = meta.size_pt * meta.line_height;
        let body_font = font_kind(body).expect("theme checked");
        let meta_font = font_kind(meta).expect("theme checked");
        if lines.same_line {
            let first_height = if lines.heading.is_empty() {
                date_leading
            } else {
                heading_leading.max(date_leading)
            };
            self.ensure(first_height);
            if let Some(first) = lines.heading.first() {
                self.ops.push(LayoutElement::Text(LayoutText {
                    text: first.clone(),
                    x_pt: self.left
                        + if placement == DatePlacement::Left {
                            lines.date_width_pt + 12.0
                        } else {
                            0.0
                        },
                    baseline_y_pt: self.y,
                    size_pt: heading_size,
                    leading_pt: first_height,
                    tracking_pt: body.tracking_pt(),
                    color: body.color_or(ink),
                    font: body_font,
                    target: self.target,
                    target_lane: self.target_lane,
                }));
            }
            self.ops.push(LayoutElement::Text(LayoutText {
                text: date.to_string(),
                x_pt: if placement == DatePlacement::Left {
                    self.left
                } else {
                    self.width - self.right - lines.date_width_pt
                },
                baseline_y_pt: self.y,
                size_pt: meta.size_pt,
                leading_pt: first_height,
                tracking_pt: meta.tracking_pt(),
                color: meta.color_or(muted),
                font: meta_font,
                target: self.target,
                target_lane: self.target_lane,
            }));
            self.y -= first_height;
            let continuation_indent = if placement == DatePlacement::Left {
                lines.date_width_pt + 12.0
            } else {
                0.0
            };
            for line in lines.heading.iter().skip(1) {
                self.line(line, body, ink, continuation_indent, heading_leading);
            }
        } else {
            if placement == DatePlacement::Left {
                for line in &lines.date {
                    self.line(line, meta, muted, 0.0, date_leading);
                }
                for line in &lines.heading {
                    self.line(line, body, ink, 0.0, heading_leading);
                }
            } else {
                for line in &lines.heading {
                    self.line(line, body, ink, 0.0, heading_leading);
                }
                for line in &lines.date {
                    let indent = (self.content_width() - self.styled_width(line, meta)).max(0.0);
                    self.line(line, meta, muted, indent, date_leading);
                }
            }
        }
    }

    fn finish_raw(mut self) -> Vec<Vec<LayoutElement>> {
        self.pages.push(self.ops);
        self.pages
    }

    fn finish(self) -> Vec<Vec<LayoutElement>> {
        let (width, right, bottom, muted) = (self.width, self.right, self.bottom, self.muted);
        let mut pages = self.finish_raw();
        append_footers(&mut pages, width, right, bottom, muted);
        pages
    }
}

fn append_footers(
    pages: &mut [Vec<LayoutElement>],
    width: f32,
    right: f32,
    bottom: f32,
    muted: Rgb,
) {
    let count = pages.len();
    for (index, ops) in pages.iter_mut().enumerate() {
        ops.push(LayoutElement::Text(LayoutText {
            text: format!("{} / {}", index + 1, count),
            x_pt: width - right - 28.0,
            baseline_y_pt: (bottom / 2.0).max(5.0),
            size_pt: 8.0,
            leading_pt: 8.0,
            tracking_pt: 0.0,
            color: muted,
            font: FontKind::Sans,
            target: None,
            target_lane: None,
        }));
    }
}
pub(crate) fn font_kind(style: &TextStyle) -> Result<FontKind, String> {
    match (style.family.as_str(), style.weight) {
        ("cjk-sans" | "system-sans", 400) if !style.italic => Ok(FontKind::Sans),
        ("cjk-serif" | "system-serif", 600) if !style.italic => Ok(FontKind::Serif),
        (family, weight) if family.starts_with("font:") => {
            crate::fonts::resolve_style(&family[5..], weight, style.italic).map(FontKind::Local)
        }
        (family, weight) if family.starts_with("asset:") => {
            let id = style
                .asset_font
                .ok_or_else(|| format!("主题字体资源 {family} 尚未加载"))?;
            let actual = crate::fonts::get(id);
            if actual.weight != weight {
                return Err(format!(
                    "主题字体 {family} 的实际字重为 {}，与声明的 {weight} 不符",
                    actual.weight
                ));
            }
            if actual.italic != style.italic {
                return Err(format!(
                    "主题字体 {family} 的实际正斜体与 fontStyle 声明不符，未使用合成倾斜"
                ));
            }
            Ok(FontKind::Local(id))
        }
        _ if style.italic => Err(format!(
            "字体组合 {} / {} 没有已加载的真实斜体字面；请选用带有斜体的本机或主题字体，不能合成倾斜",
            style.family, style.weight
        )),
        _ => Err(format!(
            "当前排版引擎暂不支持字体组合 {} / {}；当前支持 cjk-sans 400 和 cjk-serif 600",
            style.family, style.weight
        )),
    }
}

fn validate_theme(theme: &ThemeManifest, style: &ThemeStyle) -> Result<(), String> {
    let source = &theme.normalized;
    let composition = &source["composition"];
    if !matches!(
        style.composition_preset.as_str(),
        "single-column" | "split-header" | "sidebar-left" | "sidebar-right"
    ) || (style.has_sidebar()
        && style.sidebar_modules.is_empty()
        && ![
            &style.header_region,
            &style.summary_region,
            &style.contact_region,
        ]
        .into_iter()
        .any(|region| region == "sidebar"))
        || composition["header"]
            .as_str()
            .is_some_and(|value| !matches!(value, "stacked" | "centered" | "split"))
        || (style.composition_preset == "split-header"
            && composition["header"]
                .as_str()
                .is_some_and(|value| value != "split"))
        || composition["contact"]
            .as_str()
            .is_some_and(|value| !matches!(value, "inline" | "stacked"))
        || (!style.has_sidebar()
            && (composition["sidebarWidthPct"].is_number()
                || composition["gapMm"].is_number()
                || composition["placements"]
                    .as_object()
                    .is_some_and(|value| !value.is_empty())))
    {
        return Err("当前排版引擎暂不支持该 composition 布局或非侧栏布局中的栏位参数".into());
    }
    if let Some(sections) = source["sections"].as_object() {
        for (id, value) in sections {
            if value["headingAlign"]
                .as_str()
                .is_some_and(|align| !matches!(align, "left" | "center" | "right"))
            {
                return Err(format!("当前排版引擎不支持 sections.{id}.headingAlign"));
            }
            if value["items"].as_str().is_some_and(|items| {
                !matches!(items, "stack" | "compact" | "timeline" | "cards" | "chips")
            }) {
                return Err(format!("当前排版引擎不支持 sections.{id}.items"));
            }
            if value["date"]
                .as_str()
                .is_some_and(|date| !matches!(date, "inline" | "left" | "right"))
            {
                return Err(format!(
                    "当前排版引擎暂不支持 sections.{id}.date；目前支持 inline / left / right"
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
    if source["assets"]["fonts"].as_array().is_some_and(|value| {
        value
            .iter()
            .any(|font| !theme.asset_fonts.contains_key(font["id"].as_str().unwrap()))
    }) {
        return Err(
            "主题字体资源尚未验证加载；须从主题目录加载，不能只解析清单后静默替换字体".into(),
        );
    }
    for text_style in style.all_text_styles() {
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

fn heading_lines(
    page: &PageWriter<'_>,
    style: &ThemeStyle,
    id: &str,
    language: Language,
) -> Vec<String> {
    let text = &style.typography.section_title;
    let padding = if style.section(id).heading == SectionHeading::Filled {
        12.0
    } else {
        0.0
    };
    page.wrap_style(
        style.section_title(id, language),
        text,
        (page.content_width() - padding).max(1.0),
    )
}

fn heading_height(page: &PageWriter<'_>, style: &ThemeStyle, id: &str, language: Language) -> f32 {
    let text = &style.typography.section_title;
    let (ascent, descent) = font_kind(text).expect("theme checked").vertical_metrics();
    let leading = if style.has_sidebar() {
        (text.size_pt * text.line_height).max((ascent + descent) * text.size_pt)
    } else {
        text.size_pt * text.line_height
    };
    let extra_lines = heading_lines(page, style, id, language)
        .len()
        .saturating_sub(1) as f32;
    // The next item is baseline-positioned by the legacy stack renderer. Its
    // ascender must also be reserved below a filled box or underline.
    let following_ascent = (font_kind(&style.typography.item_title)
        .expect("theme checked")
        .vertical_metrics()
        .0
        * style.typography.item_title.size_pt)
        .max(
            font_kind(&style.typography.meta)
                .expect("theme checked")
                .vertical_metrics()
                .0
                * style.typography.meta.size_pt,
        )
        .max(if style.section(id).date != DatePlacement::Inline {
            font_kind(&style.typography.side_date)
                .expect("theme checked")
                .vertical_metrics()
                .0
                * style.typography.side_date.size_pt
        } else {
            0.0
        })
        .max(
            style
                .entry_headers
                .get(id)
                .into_iter()
                .flatten()
                .map(|line| {
                    font_kind(&line.style)
                        .expect("theme checked")
                        .vertical_metrics()
                        .0
                        * line.style.size_pt
                })
                .fold(0.0, f32::max),
        );
    if style.section(id).heading == SectionHeading::Plain {
        if style.has_sidebar() {
            return (extra_lines + 1.0) * leading + following_ascent + 6.0;
        }
        return extra_lines * leading
            + (leading + 4.0).max(descent * text.size_pt + following_ascent + 4.0);
    }
    (extra_lines + 1.0) * leading.max((ascent + descent) * text.size_pt)
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
    let height = heading_height(page, style, id, language);
    let lines = heading_lines(page, style, id, language);
    page.ensure(height);
    page.mark(LayoutTarget {
        section: id,
        index: None,
    });
    if heading == SectionHeading::Plain {
        let start = page.y;
        let (ascent, descent) = kind.vertical_metrics();
        let leading = if style.has_sidebar() {
            (text.size_pt * text.line_height).max((ascent + descent) * text.size_pt)
        } else {
            text.size_pt * text.line_height
        };
        if style.has_sidebar() {
            // Align visible heading tops across the two lanes even when one
            // lane uses an underline and the other uses a plain heading.
            page.y -= (leading - (ascent + descent) * text.size_pt) / 2.0 + ascent * text.size_pt;
        }
        for line in &lines {
            let inset = style
                .section(id)
                .heading_alignment
                .inset(page.content_width(), page.styled_width(line, text));
            page.line(line, text, style.colors.accent, inset, leading);
        }
        page.y = start - height;
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
            bottom_pt: top - leading * lines.len() as f32 - 2.0 * padding,
            width_pt: page.content_width(),
            height_pt: leading * lines.len() as f32 + 2.0 * padding,
            color: style.colors.accent,
        }));
    }
    for (line_index, line) in lines.iter().enumerate() {
        let horizontal_padding = if heading == SectionHeading::Filled {
            6.0
        } else {
            0.0
        };
        let aligned_inset = style.section(id).heading_alignment.inset(
            page.content_width() - horizontal_padding * 2.0,
            page.styled_width(line, text),
        );
        page.ops.push(LayoutElement::Text(LayoutText {
            text: line.clone(),
            x_pt: page.left + horizontal_padding + aligned_inset,
            baseline_y_pt: top
                - line_index as f32 * leading
                - padding
                - (leading - (ascent + descent) * text.size_pt) / 2.0
                - ascent * text.size_pt,
            size_pt: text.size_pt,
            leading_pt: leading,
            tracking_pt: text.tracking_pt(),
            color: text.color_or(if heading == SectionHeading::Filled {
                style.colors.paper
            } else {
                style.colors.accent
            }),
            font: kind,
            target: page.target,
            target_lane: page.target_lane,
        }));
    }
    if matches!(heading, SectionHeading::Line | SectionHeading::AccentLine) {
        page.ops.push(LayoutElement::Rule(LayoutRule {
            left_pt: page.left,
            right_pt: page.width - page.right,
            y_pt: top - leading * lines.len() as f32 - 4.0,
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
fn wrapped_height(page: &PageWriter<'_>, text: &str, style: &TextStyle, indent: f32) -> f32 {
    text.lines()
        .map(|line| {
            page.wrap_style(line, style, page.content_width() - indent)
                .len() as f32
                * style.size_pt
                * style.line_height
        })
        .sum()
}

/// The first body/bullet/URL must clear the last independently sized header.
/// Use a sufficient paragraph-before gap even when a Word reader takes the
/// maximum of adjacent paragraph spacing instead of adding both values.
pub(crate) fn entry_tail_gap(item: &Item, id: &str, style: &ThemeStyle) -> f32 {
    let base = if item.paragraphs.is_empty() {
        0.0
    } else {
        style.section(id).paragraph_gap_pt()
    };
    let next = if !item.paragraphs.is_empty() || !item.bullets.is_empty() {
        &style.typography.body
    } else if item.url.is_some() {
        &style.typography.meta
    } else {
        return 0.0;
    };
    let Some(rows) = crate::entry_header::rows(item, id, style) else {
        return base;
    };
    let Some(last) = rows.last() else {
        return base;
    };
    let descent = font_kind(last.style)
        .expect("checked header font")
        .vertical_metrics()
        .1
        * last.style.size_pt;
    let ascent = font_kind(next)
        .expect("checked continuation font")
        .vertical_metrics()
        .0
        * next.size_pt;
    base.max(descent + ascent - last.style.size_pt * last.style.line_height)
}

fn item_height(page: &PageWriter<'_>, item: &Item, section_id: &str, style: &ThemeStyle) -> f32 {
    let body = &style.typography.body;
    let heading = &style.typography.item_title;
    let meta = &style.typography.meta;
    let section = style.section(section_id);
    let mut height = section.item_gap_pt();
    height += entry_tail_gap(item, section_id, style)
        - if item.paragraphs.is_empty() {
            0.0
        } else {
            section.paragraph_gap_pt()
        };
    if let Some(rows) = crate::entry_header::rows(item, section_id, style) {
        for row in rows {
            height += row.before + wrapped_height(page, &row.text, row.style, 0.0) + row.after;
        }
    } else if section.date != DatePlacement::Inline
        && let Some(date) = item.date()
    {
        let date_style = &style.typography.side_date;
        let lines = page.side_date_lines(&item.heading, date, heading, date_style);
        let heading_leading = heading.size_pt * heading.line_height;
        let date_leading = date_style.size_pt * date_style.line_height;
        height += if lines.same_line {
            (if lines.heading.is_empty() {
                date_leading
            } else {
                heading_leading.max(date_leading)
            }) + lines.heading.len().saturating_sub(1) as f32 * heading_leading
        } else {
            lines.heading.len() as f32 * heading_leading + lines.date.len() as f32 * date_leading
        };
    } else if !item.heading.is_empty() {
        height += wrapped_height(page, &item.heading, heading, 0.0);
    }
    let details = if style.entry_headers.contains_key(section_id) {
        String::new()
    } else if section.date != DatePlacement::Inline && item.date().is_some() {
        item.other_details().join("   ·   ")
    } else {
        item.details.join("   ·   ")
    };
    if !details.is_empty() {
        height += wrapped_height(page, &details, meta, 0.0);
    }
    for paragraph in &item.paragraphs {
        height += section.paragraph_gap_pt() + wrapped_height(page, paragraph, body, 0.0);
    }
    if (section_id == "skills" || section_id == "interests") && !item.bullets.is_empty() {
        height += wrapped_height(page, &item.bullets.join("   ·   "), meta, 0.0);
    } else {
        for bullet in &item.bullets {
            height += wrapped_height(page, &format!("•  {bullet}"), body, 11.0);
        }
    }
    if let Some(url) = &item.url {
        height += wrapped_height(page, url, meta, 0.0);
    }
    height
}

/// Height through the first visible line, not the whole potentially multi-page
/// item. Blank anchors do not count as text for keep-with-heading decisions.
fn item_start_height(
    page: &PageWriter<'_>,
    item: &Item,
    id: &str,
    style: &ThemeStyle,
) -> Option<f32> {
    let section = style.section(id);
    let body = &style.typography.body;
    let heading = &style.typography.item_title;
    let meta = &style.typography.meta;
    let leading = |s: &TextStyle| s.size_pt * s.line_height;
    let visible = |s: &str| !s.trim().is_empty();
    let custom = crate::entry_header::rows(item, id, style);
    let first = if let Some(row) = custom.as_ref().and_then(|rows| rows.first()) {
        row.before + leading(row.style)
    } else if custom.is_none()
        && section.date != DatePlacement::Inline
        && let Some(date) = item.date()
    {
        let date_style = &style.typography.side_date;
        let lines = page.side_date_lines(&item.heading, date, heading, date_style);
        if lines.same_line && !lines.heading.is_empty() {
            leading(heading).max(leading(date_style))
        } else if !lines.heading.is_empty() && section.date == DatePlacement::Right {
            leading(heading)
        } else {
            leading(date_style)
        }
    } else if custom.is_none() && visible(&item.heading) {
        if section.items == crate::theme_style::ItemLayout::Chips && id == "languages" {
            leading(meta) + 10.0
        } else {
            leading(heading)
        }
    } else if custom.is_none() && item.details.iter().any(|s| visible(s)) {
        leading(meta)
    } else if item.paragraphs.iter().any(|s| visible(s)) {
        section.paragraph_gap_pt() + leading(body)
    } else if item.bullets.iter().any(|s| visible(s)) {
        if matches!(id, "skills" | "interests") {
            leading(meta)
                + if section.items == crate::theme_style::ItemLayout::Chips {
                    10.0
                } else {
                    0.0
                }
        } else {
            leading(body)
        }
    } else if item.url.as_deref().is_some_and(visible) {
        leading(meta)
    } else {
        return None;
    };
    Some(section.item_gap_pt() + first)
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

/// Share the measured first-line split with editable Word output.
pub(crate) fn side_date_lines_for_style(
    style: &ThemeStyle,
    heading: &str,
    date: &str,
) -> SideDateLines {
    let sans_face = Face::parse(SANS_FONT_BYTES, 0).expect("validated bundled font");
    let serif_face = Face::parse(SERIF_FONT_BYTES, 0).expect("validated bundled font");
    PageWriter::new(sans_face, serif_face, style).side_date_lines(
        heading,
        date,
        &style.typography.item_title,
        &style.typography.side_date,
    )
}

fn layout_with_style(
    content: &ResumeContent,
    language: Language,
    style: &ThemeStyle,
) -> Result<ResumeLayout, String> {
    let sans_face = Face::parse(SANS_FONT_BYTES, 0).map_err(|error| error.to_string())?;
    let serif_face = Face::parse(SERIF_FONT_BYTES, 0).map_err(|error| error.to_string())?;
    let mut page = PageWriter::new(sans_face, serif_face, style);
    layout_basics(&mut page, content, style, "page")?;
    if style.has_sidebar() {
        return sidebar_layout(page, content, language, style);
    }
    layout_sections(&mut page, content.sections.iter(), language, style)?;
    Ok(ResumeLayout {
        width_mm: style.page.width_mm,
        height_mm: style.page.height_mm,
        paper: style.colors.paper,
        pages: fallback_runs(page.finish())?,
    })
}

fn layout_basics(
    page: &mut PageWriter<'_>,
    content: &ResumeContent,
    style: &ThemeStyle,
    region: &str,
) -> Result<(), String> {
    if style.header_region != region
        && (style.summary_region != region || content.summary.is_empty())
        && (style.contact_region != region || content.contacts.is_empty())
    {
        return Ok(());
    }
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
    if style.header_region == region {
        if style.split_header
            && style.contact_region == "header"
            && (!content.name.is_empty() || !content.label.is_empty())
            && !content.contacts.is_empty()
        {
            page.split_header(content, style);
        } else {
            if !content.name.is_empty() {
                page.header_text(&content.name, name, ink, style.centered_header);
            }
            if !content.label.is_empty() {
                page.header_text(
                    &content.label,
                    &style.typography.label,
                    accent,
                    style.centered_header,
                );
            }
            if style.contact_region == "header" && !content.contacts.is_empty() {
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
        }
        if style.summary_region == "header" && !content.summary.is_empty() {
            page.gap(section_gap / 2.0);
            layout_summary(page, content, style)?;
        }
        page.gap(section_gap.max(4.0));
        if style.layout != "minimal" {
            let (rule_color, thickness) = match style.layout.as_str() {
                "rules" => (accent, 1.3),
                "modern" => (accent, 2.0),
                _ => (style.colors.border, 0.7),
            };
            add_rule(page, rule_color, thickness);
            page.gap(body.size_pt * 1.2 + 4.0);
        }
    }
    if style.contact_region == region && !content.contacts.is_empty() {
        page.header_text(
            &content.contacts.join(if style.stacked_contacts {
                "\n"
            } else {
                "   ·   "
            }),
            meta,
            muted,
            false,
        );
        page.gap(section_gap / 2.0);
    }
    if style.summary_region == region && !content.summary.is_empty() {
        layout_summary(page, content, style)?;
    }
    Ok(())
}

fn layout_summary(
    page: &mut PageWriter<'_>,
    content: &ResumeContent,
    style: &ThemeStyle,
) -> Result<(), String> {
    page.wrapped(
        &content.summary,
        &style.typography.summary,
        style.colors.ink,
        0.0,
    );
    page.gap(style.page.section_gap_mm * PT_PER_MM / 2.0);
    Ok(())
}

fn layout_sections<'a>(
    page: &mut PageWriter<'_>,
    sections: impl Iterator<Item = &'a crate::semantic::Section>,
    language: Language,
    style: &ThemeStyle,
) -> Result<(), String> {
    let body = &style.typography.body;
    let item_title = &style.typography.item_title;
    let meta = &style.typography.meta;
    let (ink, muted, accent) = (style.colors.ink, style.colors.muted, style.colors.accent);
    let section_gap = style.page.section_gap_mm * PT_PER_MM;
    for section_content in sections {
        let section = style.section(section_content.id);
        let heading_height = heading_height(page, style, section_content.id, language);
        let mut first_visible = None;
        if style.page.keep_heading_with_first_item {
            let mut prefix_height = 0.0;
            for (index, item) in section_content.items.iter().enumerate() {
                let measured = sections::measure(page, item, section_content.id, style);
                if measured.start.is_some() {
                    first_visible = Some(index);
                    page.ensure(sections::heading_reservation(
                        section_gap + heading_height + prefix_height,
                        &measured,
                        page.height - page.top - page.bottom,
                        style.page.avoid_split_items,
                    ));
                    break;
                }
                prefix_height += measured.full
                    + if matches!(
                        section.items,
                        crate::theme_style::ItemLayout::Cards
                            | crate::theme_style::ItemLayout::Timeline
                    ) {
                        7.0
                    } else {
                        0.0
                    };
            }
        }
        page.ensure(section_gap + heading_height);
        page.gap(section_gap);
        section_heading(page, section_content.id, language, style);
        for (index, item) in section_content.items.iter().enumerate() {
            let frame = sections::ItemFrame::begin(
                page,
                item,
                section_content.id,
                index,
                style,
                first_visible.is_some_and(|first| index <= first),
            );
            if let Some(rows) = crate::entry_header::rows(item, section_content.id, style) {
                for row in rows {
                    page.gap(row.before);
                    page.wrapped(&row.text, row.style, row.color, 0.0);
                    page.gap(row.after);
                }
            } else if section.date != DatePlacement::Inline
                && let Some(date) = item.date()
            {
                page.side_dated_heading(&item.heading, date, section.date, style);
            } else if section.items == crate::theme_style::ItemLayout::Chips
                && section_content.id == "languages"
                && !item.heading.is_empty()
            {
                sections::chips(page, std::slice::from_ref(&item.heading), style);
            } else if !item.heading.is_empty() {
                page.wrapped(&item.heading, item_title, ink, 0.0);
            }
            let details = if style.entry_headers.contains_key(section_content.id) {
                String::new()
            } else if section.date != DatePlacement::Inline && item.date().is_some() {
                item.other_details().join("   ·   ")
            } else {
                item.details.join("   ·   ")
            };
            if !details.is_empty() {
                page.wrapped(&details, meta, muted, 0.0);
            }
            let tail_gap = entry_tail_gap(item, section_content.id, style);
            if item.paragraphs.is_empty() && tail_gap > 0.0 {
                page.gap(tail_gap);
            }
            for (index, paragraph) in item.paragraphs.iter().enumerate() {
                page.gap(if index == 0 {
                    tail_gap
                } else {
                    section.paragraph_gap_pt()
                });
                page.wrapped(paragraph, body, ink, 0.0);
            }
            if (section_content.id == "skills" || section_content.id == "interests")
                && !item.bullets.is_empty()
            {
                if section.items == crate::theme_style::ItemLayout::Chips {
                    sections::chips(page, &item.bullets, style);
                } else {
                    page.wrapped(&item.bullets.join("   ·   "), meta, muted, 0.0);
                }
            } else {
                for bullet in &item.bullets {
                    page.wrapped(&format!("•  {bullet}"), body, ink, 11.0);
                }
            }
            if let Some(url) = &item.url {
                page.wrapped(url, meta, accent, 0.0);
            }
            frame.finish(page, style);
        }
    }
    Ok(())
}

fn sidebar_layout(
    mut main: PageWriter<'_>,
    content: &ResumeContent,
    language: Language,
    style: &ThemeStyle,
) -> Result<ResumeLayout, String> {
    let main_style = style.column_style(false);
    let side_style = style.column_style(true);
    let start_page = main.pages.len();
    let first_top = main.y;
    main.left = main_style.page.margins.left * PT_PER_MM;
    main.right = main_style.page.margins.right * PT_PER_MM;
    let mut side = PageWriter::new(
        Face::parse(SANS_FONT_BYTES, 0).unwrap(),
        Face::parse(SERIF_FONT_BYTES, 0).unwrap(),
        &side_style,
    );
    // Side content starts below the complete full-width header/summary even
    // if that header already spans pages. Later pages use normal top margins.
    side.y = first_top;
    // Legacy flow positions text by baseline. Reserve real ascenders on
    // continuation pages so light glyphs cannot escape a dark lane's tint.
    let continuation_inset = [
        &style.typography.body,
        &style.typography.item_title,
        &style.typography.meta,
        &style.typography.section_title,
        &style.typography.label,
        &style.typography.summary,
        &style.typography.side_date,
    ]
    .into_iter()
    .chain(
        style
            .entry_headers
            .values()
            .flatten()
            .map(|line| &line.style),
    )
    .map(|role| {
        font_kind(role)
            .expect("theme checked")
            .vertical_metrics()
            .0
            .max(FontKind::Sans.vertical_metrics().0)
            * role.size_pt
            * 1.06
    })
    .fold(0.0, f32::max);
    main.top += continuation_inset;
    side.top += continuation_inset;
    // Lane-local identity/contact starts below its real ascender, inside the
    // lane background. Legacy full-page headers retain their exact geometry.
    for (writer, local, region) in [
        (&mut main, &main_style, "main"),
        (&mut side, &side_style, "sidebar"),
    ] {
        let first_role = if style.header_region == region && !content.name.is_empty() {
            Some(&local.typography.name)
        } else if style.header_region == region && !content.label.is_empty() {
            Some(&local.typography.label)
        } else if style.summary_region == region && !content.summary.is_empty() {
            Some(&local.typography.summary)
        } else if style.contact_region == region && !content.contacts.is_empty() {
            Some(&local.typography.meta)
        } else {
            None
        };
        if let Some(role) = first_role {
            writer.gap(
                font_kind(role)?
                    .vertical_metrics()
                    .0
                    .max(FontKind::Sans.vertical_metrics().0)
                    * role.size_pt
                    * 1.06,
            );
        }
        layout_basics(writer, content, local, region)?;
    }
    layout_sections(
        &mut main,
        content.sections.iter().filter(|s| !style.in_sidebar(s.id)),
        language,
        &main_style,
    )?;
    layout_sections(
        &mut side,
        content.sections.iter().filter(|s| style.in_sidebar(s.id)),
        language,
        &side_style,
    )?;
    let mut pages = main.finish_raw();
    let side_pages = side.finish_raw();
    pages.resize_with(pages.len().max(start_page + side_pages.len()), Vec::new);
    let inset = 6.0;
    for (index, elements) in pages.iter_mut().enumerate().skip(start_page) {
        let top = if index == start_page {
            first_top
        } else {
            style.page.height_mm * PT_PER_MM - style.page.margins.top * PT_PER_MM
        };
        let bottom = style.page.margins.bottom * PT_PER_MM;
        elements.insert(
            0,
            LayoutElement::Fill(LayoutFill {
                left_pt: side_style.page.margins.left * PT_PER_MM - inset,
                bottom_pt: bottom,
                width_pt: (style.page.width_mm
                    - side_style.page.margins.left
                    - side_style.page.margins.right)
                    * PT_PER_MM
                    + inset * 2.0,
                height_pt: (top - bottom).max(0.0),
                color: style.colors.sidebar,
            }),
        );
    }
    for (index, side_ops) in side_pages.into_iter().enumerate() {
        pages[start_page + index].extend(side_ops);
    }
    append_footers(
        &mut pages,
        style.page.width_mm * PT_PER_MM,
        style.page.margins.right * PT_PER_MM,
        style.page.margins.bottom * PT_PER_MM,
        style.colors.muted,
    );
    Ok(ResumeLayout {
        width_mm: style.page.width_mm,
        height_mm: style.page.height_mm,
        paper: style.colors.paper,
        pages: fallback_runs(pages)?,
    })
}

/// Fallback is explicit in shared layout, so preview and PDF use identical
/// faces/advances; a Latin-only selected font never drops Chinese glyphs.
fn fallback_runs(pages: Vec<Vec<LayoutElement>>) -> Result<Vec<Vec<LayoutElement>>, String> {
    let fallback = Face::parse(SANS_FONT_BYTES, 0).expect("bundled font");
    pages
        .into_iter()
        .map(|elements| {
            let mut result = Vec::new();
            for element in elements {
                let LayoutElement::Text(text) = element else {
                    result.push(element);
                    continue;
                };
                let local = if let FontKind::Local(id) = text.font {
                    Some(crate::fonts::get(id))
                } else {
                    None
                };
                let data = match text.font {
                    FontKind::Sans => SANS_FONT_BYTES,
                    FontKind::Serif => SERIF_FONT_BYTES,
                    FontKind::Local(_) => local.as_ref().unwrap().bytes.as_slice(),
                };
                let face = Face::parse(data, 0).map_err(|_| "排版字体无法解析")?;
                let mut current = text.font;
                let mut run = String::new();
                let mut x = text.x_pt;
                for cluster in text.text.graphemes(true) {
                    let covered = |face: &Face<'_>| {
                        cluster
                            .chars()
                            .all(|c| c.is_whitespace() || face.glyph_index(c).is_some())
                    };
                    let kind = if covered(&face) {
                        text.font
                    } else if covered(&fallback) {
                        FontKind::Sans
                    } else {
                        let character = cluster
                            .chars()
                            .find(|&c| !c.is_whitespace() && fallback.glyph_index(c).is_none())
                            .unwrap_or_else(|| cluster.chars().next().unwrap());
                        return Err(format!(
                            "所选及备用字体缺少 U+{:04X} 字形，无法保证内容完整",
                            character as u32
                        ));
                    };
                    if kind != current && !run.is_empty() {
                        let width = tracked_width(current, &run, text.size_pt, text.tracking_pt)
                            + text.tracking_pt;
                        result.push(LayoutElement::Text(LayoutText {
                            text: std::mem::take(&mut run),
                            x_pt: x,
                            font: current,
                            ..text.clone()
                        }));
                        x += width;
                    }
                    current = kind;
                    run.push_str(cluster);
                }
                if !run.is_empty() || text.text.is_empty() {
                    result.push(LayoutElement::Text(LayoutText {
                        text: run,
                        x_pt: x,
                        font: current,
                        ..text
                    }));
                }
            }
            Ok(result)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{resume::parse_resume, semantic::project};

    #[test]
    fn header_row_clearance_does_not_rely_on_word_summing_adjacent_spacing() {
        let content =
            project(&parse_resume(br#"{"work":[{"position":"SMALL","name":"LARGE"}]}"#).unwrap());
        let mut theme =
            crate::parse_theme(include_bytes!("../../../assets/themes/zhibai.theme.json")).unwrap();
        theme.normalized["sections"]["work"] = serde_json::json!({"date":"inline","entryHeader":[
            {"role":"body","parts":[{"field":"position"}],"style":{"sizePt":7,"lineHeight":1},"spaceAfterPt":5},
            {"role":"body","parts":[{"field":"name"}],"style":{"sizePt":42,"lineHeight":1},"spaceBeforePt":5}
        ]});
        let style = ThemeStyle::from_manifest(&theme);
        let rows =
            crate::entry_header::rows(&content.sections[0].items[0], "work", &style).unwrap();
        let required = font_kind(rows[0].style).unwrap().vertical_metrics().1 * 7.0
            + font_kind(rows[1].style).unwrap().vertical_metrics().0 * 42.0;
        let smallest_word_baseline_gap = 7.0 + rows[0].after.max(rows[1].before);
        assert!(smallest_word_baseline_gap + 0.01 >= required);
    }

    #[test]
    fn entry_headers_clear_large_body_bullet_and_url_continuations() {
        for (tail, marker) in [
            (serde_json::json!({"summary":"BIGNEXT"}), "BIGNEXT"),
            (serde_json::json!({"highlights":["BIGNEXT"]}), "•  BIGNEXT"),
            (
                serde_json::json!({"url":"https://x.test"}),
                "https://x.test",
            ),
        ] {
            let mut item = tail;
            item["position"] = serde_json::json!("SMALL");
            let content = project(
                &parse_resume(serde_json::json!({"work":[item]}).to_string().as_bytes()).unwrap(),
            );
            let mut theme =
                crate::parse_theme(include_bytes!("../../../assets/themes/zhibai.theme.json"))
                    .unwrap();
            for role in ["body", "meta"] {
                theme.normalized["typography"][role] = serde_json::json!({"family":"cjk-sans","weight":400,"sizePt":42,"lineHeight":1});
            }
            theme.normalized["sections"]["work"] = serde_json::json!({"date":"inline","entryHeader":[{"role":"body","parts":[{"field":"position"}],"style":{"sizePt":7,"lineHeight":1}}]});
            let layout = layout_with_theme(&content, Language::En, &theme).unwrap();
            let find = |marker: &str| {
                layout
                    .pages
                    .iter()
                    .flatten()
                    .find_map(|el| {
                        if let LayoutElement::Text(t) = el
                            && t.text == marker
                        {
                            Some(t)
                        } else {
                            None
                        }
                    })
                    .unwrap()
            };
            let first = find("SMALL");
            let next = find(marker);
            let required = first.font.vertical_metrics().1 * first.size_pt
                + next.font.vertical_metrics().0 * next.size_pt;
            assert!(
                first.baseline_y_pt - next.baseline_y_pt + 0.01 >= required,
                "{marker}"
            );
            // Total-height reservation must include the added clearance too.
            let style = ThemeStyle::from_manifest(&theme);
            assert!(entry_tail_gap(&content.sections[0].items[0], "work", &style) > 20.0);
        }
    }

    #[test]
    fn entry_headers_have_independent_lines_styles_and_stable_item_targets_in_every_lane() {
        let doc = parse_resume(br#"{"work":[{"position":"POSITION","name":"ORGANIZATION","startDate":"2020","location":"HIDDEN_LOCATION","summary":"BODY","highlights":["BULLET"],"url":"https://example.com"}]}"#).unwrap();
        let source = doc.value.clone();
        let content = project(&doc);
        for preset in [
            "single-column",
            "split-header",
            "sidebar-left",
            "sidebar-right",
        ] {
            for frame in ["stack", "cards", "timeline"] {
                let mut theme =
                    crate::parse_theme(include_bytes!("../../../assets/themes/zhibai.theme.json"))
                        .unwrap();
                theme.normalized["composition"] = serde_json::json!({"preset":preset});
                if preset.starts_with("sidebar") {
                    theme.normalized["composition"]["placements"] =
                        serde_json::json!({"work":"sidebar"});
                }
                theme.normalized["sections"]["work"] = serde_json::json!({"items":frame,"date":"inline","entryHeader":[
                    {"role":"itemTitle","parts":[{"field":"position"}],"style":{"sizePt":14,"lineHeight":1.4,"color":"#123456"},"spaceAfterPt":3},
                    {"role":"body","parts":[{"field":"name"}],"style":{"family":"cjk-serif","weight":600,"sizePt":10,"color":"#345678"},"spaceBeforePt":2,"spaceAfterPt":4},
                    {"role":"meta","parts":[{"field":"dateRange"}],"style":{"color":"#56789A"}}
                ]});
                let theme = crate::parse_theme(theme.normalized.to_string().as_bytes()).unwrap();
                let result = layout_with_theme(&content, Language::En, &theme).unwrap();
                let text = |marker: &str| {
                    result
                        .pages
                        .iter()
                        .flatten()
                        .find_map(|element| {
                            if let LayoutElement::Text(text) = element
                                && text.text == marker
                            {
                                Some(text)
                            } else {
                                None
                            }
                        })
                        .unwrap()
                };
                let first = text("POSITION");
                let second = text("ORGANIZATION");
                assert_eq!(first.color.hex(), "#123456");
                assert_eq!(second.color.hex(), "#345678");
                assert_eq!(second.font, FontKind::Serif);
                assert!(
                    (first.baseline_y_pt - second.baseline_y_pt - (14.0 * 1.4 + 3.0 + 2.0)).abs()
                        < 0.01
                );
                for marker in [
                    "POSITION",
                    "ORGANIZATION",
                    "2020",
                    "BODY",
                    "https://example.com",
                ] {
                    assert_eq!(
                        text(marker).target,
                        Some(LayoutTarget {
                            section: "work",
                            index: Some(0)
                        })
                    );
                    assert!(text(marker).target_lane.is_some());
                }
                assert!(!result.pages.iter().flatten().any(|el| matches!(el,LayoutElement::Text(t) if t.text.contains("HIDDEN_LOCATION") || t.text.contains("POSITION ·"))));
                assert_eq!(doc.value, source);
            }
        }
    }

    #[test]
    fn oversized_entry_header_keeps_first_visible_line_with_heading_and_preserves_empty_indices() {
        let position = format!("POSITION_START {}", "long position words ".repeat(120));
        let source = serde_json::json!({"work":[{}, {"position":position,"name":"ORGANIZATION_END","summary":"BODY_END"}]});
        let doc = parse_resume(source.to_string().as_bytes()).unwrap();
        let content = project(&doc);
        let mut theme =
            crate::parse_theme(include_bytes!("../../../assets/themes/zhibai.theme.json")).unwrap();
        theme.normalized["sections"]["work"] = serde_json::json!({"date":"inline","entryHeader":[
            {"role":"itemTitle","parts":[{"field":"position"}],"style":{"color":"#123456"}},
            {"role":"body","parts":[{"field":"name"}]}
        ]});
        let mut style = ThemeStyle::from_manifest(&theme);
        style.page.height_mm = 95.0;
        style.page.margins.top = 6.0;
        style.page.margins.bottom = 6.0;
        let result = layout_with_style(&content, Language::En, &style).unwrap();
        assert!(result.pages.len() > 2);
        let first = &result.pages[0];
        assert!(
            first
                .iter()
                .any(|el| matches!(el,LayoutElement::Text(t) if t.text=="Work"))
        );
        assert!(first.iter().any(|el| matches!(el,LayoutElement::Text(t) if t.text.contains("POSITION_START") && t.target==Some(LayoutTarget{section:"work",index:Some(1)}))));
        assert!(first.iter().any(|el| matches!(el,LayoutElement::Anchor(a) if a.target==LayoutTarget{section:"work",index:Some(0)})));
        let joined: String = result
            .pages
            .iter()
            .flatten()
            .filter_map(|el| {
                if let LayoutElement::Text(t) = el
                    && t.color.hex() == "#123456"
                {
                    Some(t.text.as_str())
                } else {
                    None
                }
            })
            .collect();
        let compact = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
        assert_eq!(compact(&joined), compact(&position));
        for marker in ["ORGANIZATION_END", "BODY_END"] {
            assert!(result.pages.iter().flatten().any(|el| matches!(el,LayoutElement::Text(t) if t.text==marker && t.target==Some(LayoutTarget{section:"work",index:Some(1)}))))
        }
    }

    #[test]
    fn authored_titles_wrap_in_all_lanes_without_changing_semantic_ids_or_source() {
        let document = parse_resume(
            br#"{"work":[{"name":"BODY_MARKER"}],"skills":[{"name":"SKILL_MARKER"}]}"#,
        )
        .unwrap();
        let original = document.value.clone();
        let content = project(&document);
        let plain = crate::render::plain_text(&content, Language::En);
        let markdown = crate::render::markdown(&content, Language::En);
        let custom = "Professional Experience & Research Leadership — 专业经历与研究";
        for preset in [
            "single-column",
            "split-header",
            "sidebar-left",
            "sidebar-right",
        ] {
            for heading in ["plain", "line", "accent-line", "filled"] {
                let mut theme =
                    crate::parse_theme(include_bytes!("../../../assets/themes/zhibai.theme.json"))
                        .unwrap();
                theme.normalized["composition"] = serde_json::json!({"preset":preset});
                if preset.starts_with("sidebar") {
                    theme.normalized["composition"]["placements"] =
                        serde_json::json!({"work":"sidebar"});
                }
                theme.normalized["sections"]["work"] = serde_json::json!({"heading":heading});
                let default = layout_with_theme(&content, Language::En, &theme).unwrap();
                theme.normalized["sections"]["work"]["title"] = serde_json::json!("Work");
                assert_eq!(
                    layout_with_theme(&content, Language::En, &theme).unwrap(),
                    default
                );
                theme.normalized["sections"]["work"]["title"] = serde_json::json!(custom);
                let result = layout_with_theme(&content, Language::En, &theme).unwrap();
                let display: String = result
                    .pages
                    .iter()
                    .flatten()
                    .filter_map(|element| {
                        if let LayoutElement::Text(text) = element
                            && text.target
                                == Some(LayoutTarget {
                                    section: "work",
                                    index: None,
                                })
                        {
                            Some(text.text.as_str())
                        } else {
                            None
                        }
                    })
                    .collect();
                let without_spaces = |value: &str| {
                    value
                        .chars()
                        .filter(|c| !c.is_whitespace())
                        .collect::<String>()
                };
                assert_eq!(
                    without_spaces(&display),
                    without_spaces(custom),
                    "{preset}/{heading}"
                );
                assert!(result.pages.iter().flatten().any(|element| matches!(element, LayoutElement::Text(text) if text.text == "BODY_MARKER" && text.target == Some(LayoutTarget {section:"work",index:Some(0)}))));
                assert_eq!(document.value, original);
                assert_eq!(crate::render::plain_text(&content, Language::En), plain);
                assert_eq!(crate::render::markdown(&content, Language::En), markdown);
            }
        }
    }

    #[test]
    fn exact_role_colors_reach_every_layout_path_without_changing_geometry() {
        let source=br#"{"basics":{"name":"ROLE_NAME","label":"ROLE_LABEL","email":"ROLE_META@example.com","summary":"ROLE_SUMMARY"},"work":[{"name":"ROLE_ITEM","location":"ROLE_DETAIL","startDate":"2020-01","summary":"ROLE_BODY"}],"skills":[{"name":"ROLE_SKILL","keywords":["ROLE_CHIP"]}]}"#;
        let document = parse_resume(source).unwrap();
        let original = document.value.clone();
        let content = project(&document);
        let colors = [
            ("body", "#112233"),
            ("name", "#223344"),
            ("itemTitle", "#334455"),
            ("sectionTitle", "#445566"),
            ("meta", "#556677"),
            ("label", "#667788"),
            ("summary", "#778899"),
            ("sideDate", "#8899AA"),
        ];
        for preset in [
            "single-column",
            "split-header",
            "sidebar-left",
            "sidebar-right",
        ] {
            for heading in ["plain", "line", "accent-line", "filled"] {
                for date in ["inline", "left", "right"] {
                    let mut theme = crate::parse_theme(include_bytes!(
                        "../../../assets/themes/zhibai.theme.json"
                    ))
                    .unwrap();
                    theme.normalized["composition"] = serde_json::json!({"preset":preset});
                    if preset.starts_with("sidebar") {
                        theme.normalized["composition"]["placements"] =
                            serde_json::json!({"skills":"sidebar"});
                        theme.normalized["composition"]["headerRegion"] =
                            serde_json::json!("sidebar");
                        theme.normalized["composition"]["summaryRegion"] =
                            serde_json::json!("main");
                        theme.normalized["composition"]["contactRegion"] =
                            serde_json::json!("sidebar");
                    }
                    theme.normalized["sections"] = serde_json::json!({"work":{"heading":heading,"date":date},"skills":{"heading":heading,"items":"chips"}});
                    let baseline = layout_with_theme(&content, Language::En, &theme).unwrap();
                    for (role, color) in colors {
                        theme.normalized["typography"][role]["color"] = serde_json::json!(color);
                    }
                    let result = layout_with_theme(&content, Language::En, &theme).unwrap();
                    for (marker, role) in [
                        ("ROLE_NAME", "name"),
                        ("ROLE_LABEL", "label"),
                        ("ROLE_SUMMARY", "summary"),
                        ("ROLE_ITEM", "itemTitle"),
                        ("ROLE_BODY", "body"),
                        ("ROLE_DETAIL", "meta"),
                        ("ROLE_CHIP", "meta"),
                        ("Work", "sectionTitle"),
                        (
                            "2020-01",
                            if date == "inline" { "meta" } else { "sideDate" },
                        ),
                    ] {
                        let expected = colors.iter().find(|(id, _)| *id == role).unwrap().1;
                        let colored_text: String = result
                            .pages
                            .iter()
                            .flatten()
                            .filter_map(|element| {
                                if let LayoutElement::Text(text) = element
                                    && text.color.hex() == expected
                                {
                                    Some(text.text.as_str())
                                } else {
                                    None
                                }
                            })
                            .collect();
                        assert!(
                            colored_text.contains(marker),
                            "{preset}/{heading}/{date}/{marker}"
                        );
                    }
                    let mut neutral = result.clone();
                    assert_eq!(neutral.pages.len(), baseline.pages.len());
                    for (page, old_page) in neutral.pages.iter_mut().zip(&baseline.pages) {
                        assert_eq!(page.len(), old_page.len());
                        for (element, old) in page.iter_mut().zip(old_page) {
                            if let (LayoutElement::Text(text), LayoutElement::Text(previous)) =
                                (element, old)
                            {
                                text.color = previous.color;
                            }
                        }
                    }
                    assert_eq!(neutral, baseline);
                    assert_eq!(document.value, original);
                }
            }
        }
    }

    #[test]
    fn heading_alignment_moves_only_text_inside_each_lane_and_preserves_pagination_and_targets() {
        let content = project(
            &parse_resume(
                br#"{"work":[{"name":"WORK_ITEM"}],"publications":[{"name":"PUBLICATION_ITEM"}]}"#,
            )
            .unwrap(),
        );
        for preset in ["single-column", "sidebar-left", "sidebar-right"] {
            for heading in ["plain", "line", "accent-line", "filled"] {
                let mut theme =
                    crate::parse_theme(include_bytes!("../../../assets/themes/zhibai.theme.json"))
                        .unwrap();
                theme.normalized["composition"] = serde_json::json!({"preset":preset});
                if preset != "single-column" {
                    theme.normalized["composition"]["placements"] =
                        serde_json::json!({"publications":"sidebar"});
                    theme.normalized["composition"]["sidebarWidthPct"] = serde_json::json!(20);
                }
                theme.normalized["typography"]["sectionTitle"]["sizePt"] = serde_json::json!(32);
                theme.normalized["typography"]["sectionTitle"]["trackingEm"] =
                    serde_json::json!(0.1);
                for id in ["work", "publications"] {
                    theme.normalized["sections"][id] = serde_json::json!({"heading":heading});
                }
                let left = layout_with_theme(&content, Language::En, &theme).unwrap();
                for align in ["left", "center", "right"] {
                    for id in ["work", "publications"] {
                        theme.normalized["sections"][id]["headingAlign"] = serde_json::json!(align);
                    }
                    let result = layout_with_theme(&content, Language::En, &theme).unwrap();
                    let style = ThemeStyle::from_manifest(&theme);
                    assert_eq!(result.pages.len(), left.pages.len());
                    for (actual, old) in result
                        .pages
                        .iter()
                        .flatten()
                        .zip(left.pages.iter().flatten())
                    {
                        match (actual, old) {
                            (LayoutElement::Text(text), LayoutElement::Text(previous))
                                if text.target.is_some_and(|target| target.index.is_none()) =>
                            {
                                let target = text.target.unwrap();
                                let local = style.column_style(style.in_sidebar(target.section));
                                let padding = if heading == "filled" { 6.0 } else { 0.0 };
                                let inner_left = local.page.margins.left * PT_PER_MM + padding;
                                let width = (local.page.width_mm
                                    - local.page.margins.left
                                    - local.page.margins.right)
                                    * PT_PER_MM
                                    - padding * 2.0;
                                let expected = inner_left
                                    + local
                                        .section(target.section)
                                        .heading_alignment
                                        .inset(width, text.width_pt());
                                assert!(
                                    (text.x_pt - expected).abs() < 0.01,
                                    "{preset}/{heading}/{align}"
                                );
                                let mut at_left = text.clone();
                                at_left.x_pt = previous.x_pt;
                                assert_eq!(&at_left, previous);
                            }
                            _ => assert_eq!(actual, old),
                        }
                    }
                    assert_eq!(
                        result.pages.iter().flatten().count(),
                        left.pages.iter().flatten().count()
                    );
                }
            }
        }
    }

    #[test]
    fn real_italic_is_measured_and_missing_cjk_stays_upright_without_json_mutation() {
        let document =
            parse_resume("{\"basics\":{\"label\":\"Ada é e\\u0301 简里 Z\"}}".as_bytes()).unwrap();
        let original = document.value.clone();
        let mut theme =
            crate::parse_theme(include_bytes!("../../../assets/themes/zhibai.theme.json")).unwrap();
        let id = crate::fonts::register_asset(
            include_bytes!("../tests/fixtures/theme-italic-font.ttf").to_vec(),
        )
        .unwrap();
        theme.asset_fonts.insert("italic-fixture".into(), id);
        theme.normalized["typography"]["label"] = serde_json::json!({"family":"asset:italic-fixture","weight":400,"fontStyle":"italic","trackingEm":0.03});
        let result = layout_with_theme(&project(&document), Language::En, &theme).unwrap();
        let texts: Vec<_> = result
            .pages
            .iter()
            .flatten()
            .filter_map(|element| {
                if let LayoutElement::Text(text) = element {
                    Some(text)
                } else {
                    None
                }
            })
            .collect();
        assert!(texts.iter().any(|text| text.font == FontKind::Local(id)
            && text.font.is_italic()
            && text.text.contains("Ada")));
        assert!(texts.iter().any(|text| text.font == FontKind::Sans
            && !text.font.is_italic()
            && text.text.contains("简里")));
        assert_eq!(document.value, original);
        theme.normalized["typography"]["label"]["fontStyle"] = serde_json::json!("normal");
        assert!(check_theme(&theme).unwrap_err().contains("fontStyle"));
        theme.normalized["typography"]["label"]["family"] = serde_json::json!("cjk-sans");
        theme.normalized["typography"]["label"]["fontStyle"] = serde_json::json!("italic");
        assert!(check_theme(&theme).is_err());
    }

    #[test]
    fn independent_basics_and_side_date_roles_keep_defaults_and_work_in_all_regions() {
        let document=parse_resume(br#"{"basics":{"name":"Ada","label":"ROLE_LABEL","summary":"ROLE_SUMMARY","email":"contact@example.com"},"work":[{"name":"ROLE_ITEM","location":"ROLE_DETAIL","startDate":"2020-01","summary":"ROLE_BODY"}],"skills":[{"name":"ROLE_SKILL"}]}"#).unwrap();
        let content = project(&document);
        let mut theme =
            crate::parse_theme(include_bytes!("../../../assets/themes/zhibai.theme.json")).unwrap();
        let before = layout_with_theme(&content, Language::En, &theme).unwrap();
        for role in ["label", "summary", "sideDate"] {
            theme.normalized["typography"][role] = serde_json::json!({});
        }
        assert_eq!(
            layout_with_theme(&content, Language::En, &theme).unwrap(),
            before
        );
        theme.normalized["typography"]["body"] = serde_json::json!({"sizePt":9});
        theme.normalized["typography"]["meta"] = serde_json::json!({"sizePt":8});
        theme.normalized["typography"]["label"] = serde_json::json!({"sizePt":14,"family":"cjk-serif","weight":600,"textTransform":"lowercase"});
        theme.normalized["typography"]["summary"] =
            serde_json::json!({"sizePt":12,"lineHeight":1.8});
        theme.normalized["typography"]["sideDate"] =
            serde_json::json!({"sizePt":11,"family":"cjk-serif","weight":600,"trackingEm":0.02});
        for preset in [
            "single-column",
            "split-header",
            "sidebar-left",
            "sidebar-right",
        ] {
            theme.normalized["composition"] = if preset.starts_with("sidebar") {
                serde_json::json!({"preset":preset,"headerRegion":"main","summaryRegion":"main","contactRegion":"sidebar","placements":{"skills":"sidebar"}})
            } else {
                serde_json::json!({"preset":preset})
            };
            let result = layout_with_theme(&content, Language::En, &theme).unwrap();
            let texts: Vec<_> = result
                .pages
                .iter()
                .flatten()
                .filter_map(|op| {
                    if let LayoutElement::Text(t) = op {
                        Some(t)
                    } else {
                        None
                    }
                })
                .collect();
            for (value, size, font) in [
                ("role_label", 14., FontKind::Serif),
                ("ROLE_SUMMARY", 12., FontKind::Sans),
                ("2020-01", 11., FontKind::Serif),
                ("ROLE_BODY", 9., FontKind::Sans),
                ("ROLE_DETAIL", 8., FontKind::Sans),
            ] {
                let text = texts.iter().find(|t| t.text == value).unwrap();
                assert_eq!(text.size_pt, size, "{preset}/{value}");
                assert_eq!(text.font, font, "{preset}/{value}");
                assert!(text.x_pt + text.width_pt() <= result.width_mm * PT_PER_MM + 0.02);
            }
        }
        assert_eq!(document.value["basics"]["label"], "ROLE_LABEL");
    }

    #[test]
    fn heading_keep_budget_only_reserves_a_whole_item_when_the_group_can_fit() {
        for (remaining, full, avoid, wanted) in [
            (180.0, 100.0, true, 200.0),
            (180.0, 100.0, false, 120.0),
            (180.0, 700.0, true, 120.0),
            (110.0, 700.0, true, 120.0),
            (600.0, 550.0, true, 120.0),
            (600.0, 500.0, true, 600.0),
            (600.0, 500.25, true, 120.0),
            (120.0, 700.0, true, 120.0),
            (119.75, 700.0, true, 120.0),
        ] {
            let item = sections::ItemMeasure {
                full,
                start: Some(20.0),
            };
            let reserved = sections::heading_reservation(100.0, &item, 600.0, avoid);
            assert_eq!(reserved, wanted);
            let style = ThemeStyle::built_in_fallback();
            let mut page = PageWriter::new(
                Face::parse(SANS_FONT_BYTES, 0).unwrap(),
                Face::parse(SERIF_FONT_BYTES, 0).unwrap(),
                &style,
            );
            page.height = 600.0;
            page.top = 0.0;
            page.bottom = 0.0;
            page.y = remaining;
            page.ensure(reserved);
            assert_eq!(page.pages.len(), usize::from(remaining < reserved));
        }
    }

    #[test]
    fn oversized_frames_keep_section_with_real_text_and_preserve_all_rows_once() {
        for paper in ["A4", "Letter"] {
            for kind in ["cards", "timeline"] {
                for keep in [false, true] {
                    for avoid in [false, true] {
                        for header in ["title", "date-only", "body-only", "empty-prefix"] {
                            let rows: Vec<_> = (0..100).map(|n| format!("ROW_{n:03}")).collect();
                            let mut item = serde_json::json!({"summary":rows.join("\n"),"highlights":["ITEM_END"]});
                            if matches!(header, "title" | "empty-prefix") {
                                item["name"] = serde_json::json!("ITEM_START");
                            }
                            if header == "date-only" {
                                item["startDate"] = serde_json::json!("2020-01");
                            }
                            let items = if header == "empty-prefix" {
                                serde_json::json!([{}, item])
                            } else {
                                serde_json::json!([item])
                            };
                            let content = project(&parse_resume(&serde_json::to_vec(&serde_json::json!({"basics":{"name":"Identity"},"work":items})).unwrap()).unwrap());
                            let mut theme = sections_theme();
                            theme.normalized["page"] = serde_json::json!({"size":paper,"keepHeadingWithFirstItem":keep,"avoidSplitItems":avoid,"sectionGapMm":10});
                            theme.normalized["sections"]["work"] =
                                serde_json::json!({"heading":"line","items":kind,"date":"right"});
                            let result = layout_with_theme(&content, Language::En, &theme).unwrap();
                            assert!(result.pages.len() > 1);
                            let first_index = usize::from(header == "empty-prefix");
                            let mut output = Vec::new();
                            for elements in &result.pages {
                                let texts: Vec<_> = elements
                                    .iter()
                                    .filter_map(|op| {
                                        if let LayoutElement::Text(t) = op {
                                            Some(t)
                                        } else {
                                            None
                                        }
                                    })
                                    .collect();
                                if keep
                                    && texts.iter().any(|t| {
                                        t.target
                                            == Some(LayoutTarget {
                                                section: "work",
                                                index: None,
                                            })
                                    })
                                {
                                    assert!(
                                        texts.iter().any(|t| t.target
                                            == Some(LayoutTarget {
                                                section: "work",
                                                index: Some(first_index)
                                            })),
                                        "orphan: {paper}/{kind}/{header}/{avoid}"
                                    );
                                }
                                for text in texts {
                                    if text.text.starts_with("ROW_")
                                        || text.text.contains("ITEM_END")
                                    {
                                        output.push(text.text.clone());
                                    }
                                    assert!(
                                        text.x_pt + text.width_pt()
                                            <= result.width_mm * PT_PER_MM + 0.05
                                    );
                                    assert!(text.baseline_y_pt >= 0.0);
                                }
                            }
                            for row in rows {
                                assert_eq!(
                                    output.iter().filter(|text| *text == &row).count(),
                                    1,
                                    "missing or repeated {row}"
                                );
                            }
                            assert_eq!(
                                output
                                    .iter()
                                    .filter(|text| text.contains("ITEM_END"))
                                    .count(),
                                1
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn tracking_and_case_transform_measure_before_wrapping_without_changing_content() {
        let source = br#"{"basics":{"name":"Ada Lovelace","label":"Engineer","email":"ada@example.com"},"work":[{"name":"MixedCase","position":"Principal","startDate":"2020-01"}],"skills":[{"name":"Design","keywords":["Alpha","Beta"]}]}"#;
        let document = parse_resume(source).unwrap();
        let content = project(&document);
        let mut raw: serde_json::Value =
            serde_json::from_slice(include_bytes!("../../../assets/themes/zhibai.theme.json"))
                .unwrap();
        raw["typography"]["name"]["trackingEm"] = serde_json::json!(0.25);
        raw["typography"]["name"]["textTransform"] = serde_json::json!("uppercase");
        raw["typography"]["sectionTitle"]["trackingEm"] = serde_json::json!(0.25);
        raw["typography"]["sectionTitle"]["textTransform"] = serde_json::json!("uppercase");
        raw["typography"]["itemTitle"] = serde_json::json!({"family":"cjk-sans","sizePt":10,"weight":400,"trackingEm":0.1,"textTransform":"lowercase"});
        raw["typography"]["meta"]["trackingEm"] = serde_json::json!(-0.02);
        raw["sections"] = serde_json::json!({"work":{"date":"right"},"skills":{"items":"chips"}});
        raw["composition"] = serde_json::json!({"preset":"single-column","header":"centered"});
        let theme = crate::parse_theme(&serde_json::to_vec(&raw).unwrap()).unwrap();
        let layout = layout_with_theme(&content, Language::En, &theme).unwrap();
        let texts: Vec<_> = layout
            .pages
            .iter()
            .flatten()
            .filter_map(|op| {
                if let LayoutElement::Text(t) = op {
                    Some(t)
                } else {
                    None
                }
            })
            .collect();
        let name = texts.iter().find(|t| t.text == "ADA LOVELACE").unwrap();
        let style = ThemeStyle::from_manifest(&theme);
        assert!((name.tracking_pt - style.typography.name.size_pt * 0.25).abs() < 0.001);
        assert!(
            (name.x_pt + name.width_pt() / 2.0 - layout.width_mm * PT_PER_MM / 2.0).abs() < 0.01
        );
        assert!(texts.iter().any(|t| t.text == "WORK"));
        assert!(texts.iter().any(|t| t.text.contains("mixedcase")));
        for text in texts {
            assert!(
                text.x_pt + text.width_pt()
                    <= layout.width_mm * PT_PER_MM - style.page.margins.right * PT_PER_MM + 0.05
            );
            let runs = text.paint_runs();
            if let Some((last, x)) = runs.last() {
                assert!(
                    (x + text.font.text_width_pt(last, text.size_pt) - text.width_pt()).abs()
                        < 0.01
                );
            }
        }
        assert_eq!(document.value["basics"]["name"], "Ada Lovelace");
        assert_eq!(content.name, "Ada Lovelace");
    }

    #[test]
    fn tracked_wrap_keeps_combining_clusters_and_zero_tracking_keeps_legacy_layout() {
        let style = ThemeStyle::built_in_fallback();
        let writer = PageWriter::new(
            Face::parse(SANS_FONT_BYTES, 0).unwrap(),
            Face::parse(SERIF_FONT_BYTES, 0).unwrap(),
            &style,
        );
        let mut role = style.typography.body.clone();
        role.tracking_em = 0.5;
        let source = "A\u{301}B\u{301}C\u{301}D\u{301}";
        let lines = writer.wrap_style(source, &role, 24.0);
        assert!(lines.len() > 1);
        assert_eq!(lines.concat(), source);
        assert!(
            lines.iter().all(
                |line| !line.starts_with('\u{301}') && writer.styled_width(line, &role) <= 24.0
            )
        );
        role.text_transform = crate::theme_style::TextTransform::Uppercase;
        assert_eq!(writer.wrap_style("Straße", &role, 500.0), vec!["STRASSE"]);
        let content = project(
            &parse_resume(br#"{"basics":{"name":"Ada"},"work":[{"name":"Company"}]}"#).unwrap(),
        );
        let mut theme =
            crate::parse_theme(include_bytes!("../../../assets/themes/zhibai.theme.json")).unwrap();
        let original = layout_with_theme(&content, Language::En, &theme).unwrap();
        for name in ["body", "name", "sectionTitle", "meta"] {
            theme.normalized["typography"][name]["trackingEm"] = serde_json::json!(0);
            theme.normalized["typography"][name]["textTransform"] = serde_json::json!("none");
        }
        assert_eq!(
            layout_with_theme(&content, Language::En, &theme).unwrap(),
            original
        );
    }

    #[test]
    fn header_summary_stays_left_aligned_and_precedes_the_masthead_rule() {
        let content = project(&parse_resume(br#"{"basics":{"name":"Identity","summary":"Summary body","email":"contact@example.com"},"work":[{"name":"Company"}]}"#).unwrap());
        for header in ["stacked", "centered", "split"] {
            let mut source: serde_json::Value =
                serde_json::from_slice(include_bytes!("../../../assets/themes/zhibai.theme.json"))
                    .unwrap();
            source["composition"] = serde_json::json!({"preset":"single-column", "header":header, "summaryRegion":"header"});
            source["layout"] = serde_json::json!("classic");
            let theme = crate::parse_theme(&serde_json::to_vec(&source).unwrap()).unwrap();
            let style = ThemeStyle::from_manifest(&theme);
            let result = layout_with_theme(&content, Language::En, &theme).unwrap();
            let summary = result.pages[0]
                .iter()
                .position(|op| matches!(op, LayoutElement::Text(t) if t.text == "Summary body"))
                .unwrap();
            let rule = result.pages[0]
                .iter()
                .position(|op| matches!(op, LayoutElement::Rule(_)))
                .unwrap();
            assert!(summary < rule);
            let LayoutElement::Text(text) = &result.pages[0][summary] else {
                unreachable!()
            };
            assert_eq!(text.x_pt, style.page.margins.left * PT_PER_MM);
            assert_eq!(
                result
                    .pages
                    .iter()
                    .flatten()
                    .filter(|op| matches!(op, LayoutElement::Text(t) if t.text == "Summary body"))
                    .count(),
                1
            );
        }
    }

    #[test]
    fn region_local_basics_preserve_content_and_stay_inside_both_lanes() {
        let content = project(&parse_resume(br#"{"basics":{"name":"Identity","label":"Designer","summary":"Summary","email":"contact@example.com"},"work":[{"name":"MainCompany"}],"skills":[{"name":"SideSkill"}]}"#).unwrap());
        for direction in ["sidebar-left", "sidebar-right"] {
            for (identity, summary, contacts) in [
                ("main", "main", "sidebar"),
                ("sidebar", "main", "header"),
                ("page", "sidebar", "main"),
            ] {
                let mut source: serde_json::Value = serde_json::from_slice(include_bytes!(
                    "../../resume-one-cli/tests/fixtures/sidebar.theme.json"
                ))
                .unwrap();
                source["composition"]["preset"] = serde_json::json!(direction);
                source["composition"]["header"] = serde_json::json!("stacked");
                source["composition"]["headerRegion"] = serde_json::json!(identity);
                source["composition"]["summaryRegion"] = serde_json::json!(summary);
                source["composition"]["contactRegion"] = serde_json::json!(contacts);
                source["composition"]["sidebarTypographyScale"] = serde_json::json!(0.9);
                let theme = crate::parse_theme(&serde_json::to_vec(&source).unwrap()).unwrap();
                let style = ThemeStyle::from_manifest(&theme);
                let result = layout_with_theme(&content, Language::En, &theme).unwrap();
                let texts: Vec<_> = result
                    .pages
                    .iter()
                    .flatten()
                    .filter_map(|op| {
                        if let LayoutElement::Text(t) = op {
                            Some(t)
                        } else {
                            None
                        }
                    })
                    .collect();
                for (value, region) in [
                    ("Identity", identity),
                    ("Summary", summary),
                    (
                        "contact@example.com",
                        if contacts == "header" {
                            identity
                        } else {
                            contacts
                        },
                    ),
                ] {
                    let matches: Vec<_> = texts.iter().filter(|t| t.text == value).collect();
                    assert_eq!(matches.len(), 1, "{direction} {identity} {value}");
                    let text = matches[0];
                    let lane = if region == "page" {
                        style.clone()
                    } else {
                        style.column_style(region == "sidebar")
                    };
                    assert!(text.x_pt >= lane.page.margins.left * PT_PER_MM - 0.02);
                    assert!(
                        text.x_pt + text.font.text_width_pt(&text.text, text.size_pt)
                            <= (lane.page.width_mm - lane.page.margins.right) * PT_PER_MM + 0.02
                    );
                    if region != "page" {
                        assert!(
                            text.baseline_y_pt + text.font.vertical_metrics().0 * text.size_pt
                                <= style.page.height_mm * PT_PER_MM
                                    - style.page.margins.top * PT_PER_MM
                                    + 0.02
                        );
                    }
                }
                assert_eq!(theme.normalized, source);
            }
        }
    }

    #[test]
    fn region_local_long_contacts_and_summary_continue_without_loss() {
        let mut content = project(&parse_resume(br#"{"basics":{"name":"Identity"},"work":[{"name":"MainTail"}],"skills":[{"name":"SideTail"}]}"#).unwrap());
        content.contacts = (0..160).map(|i| format!("Contact-{i:03}")).collect();
        content.summary = "Long summary with 中文 content ".repeat(200);
        let mut theme = crate::parse_theme(include_bytes!(
            "../../resume-one-cli/tests/fixtures/sidebar.theme.json"
        ))
        .unwrap();
        theme.normalized["composition"]["headerRegion"] = serde_json::json!("main");
        theme.normalized["composition"]["summaryRegion"] = serde_json::json!("main");
        theme.normalized["composition"]["contactRegion"] = serde_json::json!("sidebar");
        theme.normalized["composition"]["contact"] = serde_json::json!("stacked");
        let result = layout_with_theme(&content, Language::En, &theme).unwrap();
        assert!(result.pages.len() > 2);
        let all = result
            .pages
            .iter()
            .flatten()
            .filter_map(|op| {
                if let LayoutElement::Text(t) = op {
                    Some(t.text.as_str())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
        for tail in ["Contact-159", "MainTail", "SideTail"] {
            assert!(all.contains(tail));
        }
        assert_eq!(
            result
                .pages
                .iter()
                .flatten()
                .filter(|op| matches!(op, LayoutElement::Text(t) if t.target.is_none()))
                .count(),
            result.pages.len()
        );
    }

    #[test]
    fn sidebar_lanes_wrap_titles_preserve_all_items_and_paginate_independently() {
        let original = include_bytes!("../../resume-one-cli/tests/fixtures/sidebar.theme.json");
        let mut theme = crate::parse_theme(original).unwrap();
        let source = include_bytes!("../../../assets/example.resume.json");
        let mut content = project(&parse_resume(source).unwrap());
        // Force the narrow lane to continue beyond the main lane.
        let skills = content
            .sections
            .iter_mut()
            .find(|s| s.id == "skills")
            .unwrap();
        let mut item = skills.items[0].clone();
        item.heading = "SideTail".into();
        item.paragraphs = vec!["中英混排 reliable layout ".repeat(120)];
        skills.items.push(item);
        for side in ["sidebar-left", "sidebar-right"] {
            for paper in ["A4", "Letter"] {
                for width in [20, 30, 40] {
                    theme.normalized["composition"]["preset"] = serde_json::json!(side);
                    theme.normalized["composition"]["sidebarWidthPct"] = serde_json::json!(width);
                    theme.normalized["page"]["size"] = serde_json::json!(paper);
                    let style = ThemeStyle::from_manifest(&theme);
                    let before = theme.normalized.clone();
                    let result = layout_with_theme(&content, Language::En, &theme).unwrap();
                    assert!(result.pages.len() > 1);
                    let mut indices = std::collections::BTreeSet::new();
                    let mut footers = 0;
                    for op in result.pages.iter().flatten() {
                        if let LayoutElement::Anchor(a) = op
                            && let Some(index) = a.target.index
                        {
                            indices.insert((a.target.section, index));
                        }
                        let LayoutElement::Text(text) = op else {
                            continue;
                        };
                        let Some(target) = text.target else {
                            footers += 1;
                            continue;
                        };
                        let lane = if target.section == "basics" {
                            style.clone()
                        } else {
                            style.column_style(style.in_sidebar(target.section))
                        };
                        let right = (lane.page.width_mm - lane.page.margins.right) * PT_PER_MM;
                        assert!(
                            text.x_pt >= lane.page.margins.left * PT_PER_MM - 0.01,
                            "{side} {:?}",
                            text.text
                        );
                        assert!(
                            text.x_pt + text.font.text_width_pt(&text.text, text.size_pt)
                                <= right + 0.02,
                            "{side} {} {:?}",
                            width,
                            text.text
                        );
                        assert!(text.baseline_y_pt >= style.page.margins.bottom * PT_PER_MM - 0.01);
                        assert!(
                            text.baseline_y_pt + text.font.vertical_metrics().0 * text.size_pt
                                <= style.page.height_mm * PT_PER_MM
                                    - style.page.margins.top * PT_PER_MM
                                    + 0.02,
                            "ascender escapes the lane at {:?}",
                            text.text
                        );
                    }
                    for section in &content.sections {
                        for index in 0..section.items.len() {
                            assert!(indices.contains(&(section.id, index)));
                        }
                    }
                    assert_eq!(footers, result.pages.len());
                    assert!(result.pages.iter().flatten().any(
                        |op| matches!(op, LayoutElement::Text(t) if t.text.contains("SideTail"))
                    ));
                    assert_eq!(theme.normalized, before);
                }
            }
        }
    }

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
    fn split_header_keeps_identity_and_contacts_in_separate_bounded_columns() {
        let mut theme = sections_theme();
        theme.normalized["composition"] =
            serde_json::json!({"preset":"split-header","header":"split","contact":"stacked"});
        theme.normalized["page"]["marginsMm"] =
            serde_json::json!({"top":16,"right":11,"bottom":16,"left":23});
        let mut content = project(
            &parse_resume(br#"{"basics":{"name":"Name","label":"Designer","summary":"Summary"},"work":[{"name":"Company"}]}"#).unwrap(),
        );
        content.name = "中文姓名 Long Name ".repeat(5);
        content.contacts = vec![
            "first@example.com".into(),
            "https://example.com/a/very/long/contact".repeat(4),
            "Last contact".into(),
        ];
        let style = ThemeStyle::from_manifest(&theme);
        let result = layout_with_theme(&content, Language::En, &theme).unwrap();
        let measure = PageWriter::new(
            Face::parse(SANS_FONT_BYTES, 0).unwrap(),
            Face::parse(SERIF_FONT_BYTES, 0).unwrap(),
            &style,
        );
        let mut left_count = 0;
        let mut right_count = 0;
        for element in result.pages.iter().flatten() {
            if let LayoutElement::Text(text) = element
                && text.target.is_some_and(|target| target.section == "basics")
                && text.text != "Summary"
            {
                let width = measure.text_width(&text.text, text.size_pt, text.font);
                if text.size_pt == style.typography.meta.size_pt {
                    right_count += 1;
                    assert!(text.x_pt >= measure.left + measure.content_width() * 0.58 + 13.9);
                    assert!((text.x_pt + width - (measure.width - measure.right)).abs() < 0.02);
                } else {
                    left_count += 1;
                    assert!((text.x_pt - measure.left).abs() < 0.02);
                    assert!(
                        text.x_pt + width <= measure.left + measure.content_width() * 0.58 + 0.02
                    );
                }
                assert!(text.baseline_y_pt > measure.bottom);
                assert!(text.baseline_y_pt < measure.height);
            }
        }
        assert!(left_count > 5);
        assert!(right_count > 5);
        assert!(result.pages.iter().flatten().any(
            |element| matches!(element, LayoutElement::Text(text) if text.text == "Company" && text.x_pt == measure.left)
        ));
        theme.normalized["composition"] = serde_json::json!({"header":"split"});
        assert!(check_theme(&theme).is_ok());
        let inline = layout_with_theme(&content, Language::En, &theme).unwrap();
        let inline_contacts: Vec<_> = inline
            .pages
            .iter()
            .flatten()
            .filter_map(|element| match element {
                LayoutElement::Text(text)
                    if text.target.is_some_and(|target| target.section == "basics")
                        && text.size_pt == style.typography.meta.size_pt =>
                {
                    Some(text.text.as_str())
                }
                _ => None,
            })
            .collect();
        assert!(inline_contacts.len() > 2);
        assert!(
            inline_contacts
                .iter()
                .all(|line| !line.trim_start().starts_with('·'))
        );
        assert!(inline_contacts.join(" ").contains("Last contact"));

        content.name = "中文姓名 Long Name ".repeat(90);
        let long_result = layout_with_theme(&content, Language::En, &theme).unwrap();
        assert!(long_result.pages.len() > 1);
        let names: String = long_result
            .pages
            .iter()
            .flatten()
            .filter_map(|element| match element {
                LayoutElement::Text(text) if text.font == FontKind::Serif => {
                    Some(text.text.as_str())
                }
                _ => None,
            })
            .collect();
        assert_eq!(names.matches("中文姓名").count(), 90);
        assert!(
            long_result
                .pages
                .iter()
                .flatten()
                .all(|element| match element {
                    LayoutElement::Text(text) if text.target.is_some() => {
                        text.baseline_y_pt >= measure.bottom - 0.01
                    }
                    _ => true,
                })
        );
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
            serde_json::json!({"contact":"icons"}),
            serde_json::json!({"preset":"split-header","header":"centered"}),
            serde_json::json!({"preset":"sidebar-left"}),
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
    fn rich_section_constructs_keep_text_and_bounds_across_pages() {
        let source = serde_json::json!({
            "basics":{"name":"Community Theme 简里"},
            "work":[{"position":"Engineer", "name":"Example", "startDate":"2020", "summary":"中英 mixed long content ".repeat(500),"highlights":["Final work marker"]}],
            "skills":[{"name":"Languages", "keywords":["Rust", "中文", "Long keyword which must wrap naturally instead of being clipped or dropped", "Final skill marker"]}]
        });
        let document = parse_resume(source.to_string().as_bytes()).unwrap();
        let content = project(&document);
        for items in ["timeline", "cards"] {
            let mut theme = sections_theme();
            theme.normalized["sections"] = serde_json::json!({"work":{"items":items,"date":"right"},"skills":{"items":"chips"}});
            let result = layout_with_theme(&content, Language::En, &theme).unwrap();
            assert!(result.pages.len() > 1);
            let texts: Vec<_> = result
                .pages
                .iter()
                .flatten()
                .filter_map(|op| {
                    if let LayoutElement::Text(t) = op {
                        Some(t)
                    } else {
                        None
                    }
                })
                .collect();
            assert!(texts.iter().any(|t| t.text.contains("Final work marker")));
            assert!(texts.iter().any(|t| t.text.contains("Final skill marker")));
            for text in texts {
                assert!(text.x_pt >= 0.0);
                assert!(
                    text.x_pt + text.font.text_width_pt(&text.text, text.size_pt)
                        <= result.width_mm * PT_PER_MM + 0.1,
                    "{}",
                    text.text
                );
            }
            assert!(
                result
                    .pages
                    .iter()
                    .flatten()
                    .any(|op| matches!(op, LayoutElement::Fill(_)))
            );
        }
        assert_eq!(document.value, source);
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
            serde_json::json!({"items":"unknown"}),
            serde_json::json!({"date":"sideways"}),
        ] {
            theme.normalized["sections"]["work"] = value;
            assert!(check_theme(&theme).unwrap_err().contains("sections.work"));
        }
        theme.normalized["sections"] = serde_json::json!({"skills":{"items":"chips"}});
        assert!(check_theme(&theme).is_ok());
    }

    #[test]
    fn right_dates_align_to_content_edge_without_overlap_or_loss() {
        let mut theme = sections_theme();
        theme.normalized["sections"]["work"]["date"] = serde_json::json!("right");
        theme.normalized["sections"]["awards"] = serde_json::json!({"date":"right"});
        check_theme(&theme).unwrap();
        let source = serde_json::json!({"basics":{"name":"Ada"},
            "work":[{"position":"Very long cross-platform engineering position with many words to wrap safely",
                "name":"Example Organization", "startDate":"2020-01-01", "endDate":"2022-12-31",
                "location":"北京", "summary":"Delivered reliable work"}],
            "awards":[{"title":"Prize", "awarder":"Acme", "date":"2023-05-01"}]});
        let content = project(&parse_resume(&serde_json::to_vec(&source).unwrap()).unwrap());
        let style = ThemeStyle::from_manifest(&theme);
        let layout = layout_with_theme(&content, Language::En, &theme).unwrap();
        let measure = PageWriter::new(
            Face::parse(SANS_FONT_BYTES, 0).unwrap(),
            Face::parse(SERIF_FONT_BYTES, 0).unwrap(),
            &style,
        );
        let texts: Vec<_> = layout
            .pages
            .iter()
            .flatten()
            .filter_map(|element| {
                if let LayoutElement::Text(text) = element {
                    Some(text)
                } else {
                    None
                }
            })
            .collect();
        let date = texts
            .iter()
            .find(|text| text.text == "2020-01-01 – 2022-12-31")
            .unwrap();
        let first_heading = texts
            .iter()
            .find(|text| text.target == date.target && text.color == style.colors.ink)
            .unwrap();
        assert_eq!(first_heading.baseline_y_pt, date.baseline_y_pt);
        assert!(
            first_heading.x_pt
                + measure.text_width(
                    &first_heading.text,
                    first_heading.size_pt,
                    first_heading.font
                )
                + 12.0
                <= date.x_pt + 0.01
        );
        assert!(
            (date.x_pt + measure.text_width(&date.text, date.size_pt, date.font)
                - (measure.width - measure.right))
                .abs()
                < 0.01
        );
        for text in &texts {
            assert!(
                text.x_pt + measure.text_width(&text.text, text.size_pt, text.font)
                    <= measure.width - measure.right + 0.05,
                "{}",
                text.text
            );
        }
        assert_eq!(
            texts
                .iter()
                .filter(|text| text.text == "2020-01-01 – 2022-12-31")
                .count(),
            1
        );
        assert!(texts.iter().any(|text| text.text == "北京"));
        assert!(texts.iter().any(|text| text.text == "Acme"));
        assert!(texts.iter().any(|text| text.text == "2023-05-01"));

        let mut long_date_content = content.clone();
        long_date_content.sections[0].items[0].details[0] = "2020-01-01 – 2022-12-31 ".repeat(6);
        let long = layout_with_theme(&long_date_content, Language::En, &theme).unwrap();
        let long_texts: Vec<_> = long
            .pages
            .iter()
            .flatten()
            .filter_map(|element| {
                if let LayoutElement::Text(text) = element {
                    Some(text)
                } else {
                    None
                }
            })
            .collect();
        assert!(
            long_texts
                .iter()
                .filter(|text| text.target == date.target && text.color == style.colors.muted)
                .count()
                > 1
        );
        for text in long_texts {
            assert!(
                text.x_pt + measure.text_width(&text.text, text.size_pt, text.font)
                    <= measure.width - measure.right + 0.05,
                "{}",
                text.text
            );
        }
    }

    #[test]
    fn left_dates_align_heading_and_continuations_without_losing_details() {
        let mut theme = sections_theme();
        theme.normalized["sections"]["work"]["date"] = serde_json::json!("left");
        check_theme(&theme).unwrap();
        let source = serde_json::json!({"work":[{
            "position":"Very long cross-platform engineering position with many words to wrap safely",
            "name":"Example Organization", "startDate":"2020", "endDate":"2022",
            "location":"Berlin", "summary":"Delivered reliable work"}]});
        let content = project(&parse_resume(&serde_json::to_vec(&source).unwrap()).unwrap());
        let style = ThemeStyle::from_manifest(&theme);
        let measure = PageWriter::new(
            Face::parse(SANS_FONT_BYTES, 0).unwrap(),
            Face::parse(SERIF_FONT_BYTES, 0).unwrap(),
            &style,
        );
        let layout = layout_with_theme(&content, Language::En, &theme).unwrap();
        let texts: Vec<_> = layout
            .pages
            .iter()
            .flatten()
            .filter_map(|element| {
                if let LayoutElement::Text(text) = element {
                    Some(text)
                } else {
                    None
                }
            })
            .collect();
        let date = texts
            .iter()
            .find(|text| text.text == "2020 – 2022")
            .unwrap();
        let headings: Vec<_> = texts
            .iter()
            .filter(|text| {
                text.target == date.target
                    && text.color == style.colors.ink
                    && (text.size_pt - style.typography.body.size_pt * 1.06).abs() < 0.01
            })
            .collect();
        assert!(headings.len() > 1);
        assert_eq!(date.x_pt, measure.left);
        assert_eq!(date.baseline_y_pt, headings[0].baseline_y_pt);
        let heading_x =
            measure.left + measure.text_width(&date.text, date.size_pt, date.font) + 12.0;
        for heading in headings {
            assert!((heading.x_pt - heading_x).abs() < 0.01);
            assert!(
                heading.x_pt + measure.text_width(&heading.text, heading.size_pt, heading.font)
                    <= measure.width - measure.right + 0.05
            );
        }
        assert_eq!(
            texts
                .iter()
                .filter(|text| text.text == "2020 – 2022")
                .count(),
            1
        );
        assert!(texts.iter().any(|text| text.text == "Berlin"));

        let mut long_date = content.clone();
        let item = &mut long_date.sections[0].items[0];
        item.details[item.date_detail_index.unwrap()] = "2020 – 2022 ".repeat(12);
        let layout = layout_with_theme(&long_date, Language::En, &theme).unwrap();
        let dated: Vec<_> = layout
            .pages
            .iter()
            .flatten()
            .filter_map(|element| {
                if let LayoutElement::Text(text) = element {
                    (text.target == date.target).then_some(text)
                } else {
                    None
                }
            })
            .collect();
        assert!(
            dated
                .iter()
                .any(|text| text.text.starts_with("2020 – 2022"))
        );
        assert_eq!(dated.first().unwrap().x_pt, measure.left);
        assert!(dated.iter().any(|text| text.color == style.colors.ink));
        for text in dated {
            assert!(
                text.x_pt + measure.text_width(&text.text, text.size_pt, text.font)
                    <= measure.width - measure.right + 0.05
            );
        }
    }

    #[test]
    fn bundled_theme_visual_sample_stays_inside_every_page() {
        let content = project(
            &parse_resume(include_bytes!(
                "../../resume-one-cli/tests/fixtures/visual-qa.json"
            ))
            .unwrap(),
        );
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/themes");
        for entry in std::fs::read_dir(root).unwrap() {
            let path = entry.unwrap().path();
            let theme = crate::parse_theme(&std::fs::read(&path).unwrap()).unwrap();
            let style = ThemeStyle::from_manifest(&theme);
            let measure = PageWriter::new(
                Face::parse(SANS_FONT_BYTES, 0).unwrap(),
                Face::parse(SERIF_FONT_BYTES, 0).unwrap(),
                &style,
            );
            let layout = layout_with_theme(&content, Language::Zh, &theme).unwrap();
            assert!(!layout.pages.is_empty(), "{}", theme.id);
            let mut has_award = false;
            for page in &layout.pages {
                for element in page {
                    let LayoutElement::Text(text) = element else {
                        continue;
                    };
                    if text.target.is_none() {
                        continue;
                    }
                    has_award |= text.text.contains("年度体验设计奖");
                    assert!(
                        text.x_pt >= measure.left - 0.1
                            && text.x_pt + measure.text_width(&text.text, text.size_pt, text.font)
                                <= measure.width - measure.right + 0.1,
                        "{}: horizontal overflow: {}",
                        theme.id,
                        text.text
                    );
                    assert!(
                        text.baseline_y_pt >= measure.bottom - 0.1
                            && text.baseline_y_pt <= measure.height - measure.top + 0.1,
                        "{}: vertical overflow: {}",
                        theme.id,
                        text.text
                    );
                }
            }
            assert!(has_award, "{} lost trailing content", theme.id);
        }
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
