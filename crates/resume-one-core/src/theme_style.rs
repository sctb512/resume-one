//! Resolved, bounded presentation tokens shared by print and desktop renderers.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::ThemeManifest;
use crate::render::{Language, title};
use crate::semantic::Section;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    fn parse(hex: &str) -> Self {
        let bytes = hex.as_bytes();
        Self {
            r: u8::from_str_radix(std::str::from_utf8(&bytes[1..3]).unwrap(), 16).unwrap(),
            g: u8::from_str_radix(std::str::from_utf8(&bytes[3..5]).unwrap(), 16).unwrap(),
            b: u8::from_str_radix(std::str::from_utf8(&bytes[5..7]).unwrap(), 16).unwrap(),
        }
    }

    pub fn hex(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }

    pub fn mix(self, other: Self, weight: f32) -> Self {
        let mix = |a: u8, b: u8| (a as f32 * (1.0 - weight) + b as f32 * weight).round() as u8;
        Self {
            r: mix(self.r, other.r),
            g: mix(self.g, other.g),
            b: mix(self.b, other.b),
        }
    }

    pub fn contrast(self, other: Self) -> f32 {
        fn channel(value: u8) -> f32 {
            let value = value as f32 / 255.0;
            if value <= 0.04045 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        }
        let luminance = |color: Self| {
            0.2126 * channel(color.r) + 0.7152 * channel(color.g) + 0.0722 * channel(color.b)
        };
        let a = luminance(self);
        let b = luminance(other);
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    pub(crate) fn readable(self, paper: Self, fallback: Self, minimum: f32) -> Self {
        if self.contrast(paper) >= minimum {
            return self;
        }
        for step in 1..=20 {
            let candidate = self.mix(fallback, step as f32 / 20.0);
            if candidate.contrast(paper) >= minimum {
                return candidate;
            }
        }
        let black = Self { r: 0, g: 0, b: 0 };
        let white = Self {
            r: 255,
            g: 255,
            b: 255,
        };
        if black.contrast(paper) >= white.contrast(paper) {
            black
        } else {
            white
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Colors {
    pub accent: Rgb,
    pub seal: Rgb,
    pub paper: Rgb,
    pub canvas: Rgb,
    pub ink: Rgb,
    pub muted: Rgb,
    pub border: Rgb,
    pub sidebar: Rgb,
    pub chrome: Rgb,
}

#[derive(Debug, Clone, Copy)]
pub struct MarginsMm {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirstLinePosition {
    /// Legacy themes position the first text baseline at the top margin.
    Baseline,
    /// The top margin is the top of the first CSS line box.
    LineBox,
}

#[derive(Debug, Clone, Copy)]
pub struct PageStyle {
    pub width_mm: f32,
    pub height_mm: f32,
    pub margins: MarginsMm,
    pub section_gap_mm: f32,
    pub avoid_split_items: bool,
    pub keep_heading_with_first_item: bool,
    pub first_line_position: FirstLinePosition,
}

#[derive(Debug, Clone)]
pub struct TextStyle {
    pub family: String,
    pub size_pt: f32,
    pub weight: u16,
    pub italic: bool,
    /// An author's exact document color, independent of readable app chrome.
    pub color: Option<Rgb>,
    pub line_height: f32,
    pub tracking_em: f32,
    pub text_transform: TextTransform,
    pub(crate) asset_font: Option<u16>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TextTransform {
    #[default]
    None,
    Uppercase,
    Lowercase,
}

impl TextStyle {
    pub fn color_or(&self, fallback: Rgb) -> Rgb {
        self.color.unwrap_or(fallback)
    }

    /// Presentation only. Never changes the resume or its semantic projection.
    pub fn display_text<'a>(&self, text: &'a str) -> std::borrow::Cow<'a, str> {
        match self.text_transform {
            TextTransform::None => std::borrow::Cow::Borrowed(text),
            TextTransform::Uppercase => std::borrow::Cow::Owned(text.to_uppercase()),
            TextTransform::Lowercase => std::borrow::Cow::Owned(text.to_lowercase()),
        }
    }

    pub fn tracking_pt(&self) -> f32 {
        self.tracking_em * self.size_pt
    }
}

#[derive(Debug, Clone)]
pub struct Typography {
    pub body: TextStyle,
    pub item_title: TextStyle,
    pub name: TextStyle,
    pub section_title: TextStyle,
    pub meta: TextStyle,
    pub label: TextStyle,
    pub summary: TextStyle,
    pub side_date: TextStyle,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SectionHeading {
    #[default]
    Plain,
    Line,
    AccentLine,
    Filled,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ItemLayout {
    #[default]
    Stack,
    Compact,
    Timeline,
    Cards,
    Chips,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DatePlacement {
    #[default]
    Inline,
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum HeadingAlignment {
    #[default]
    Left,
    Center,
    Right,
}

impl HeadingAlignment {
    pub(crate) fn inset(self, available: f32, used: f32) -> f32 {
        let free = (available - used).max(0.0);
        match self {
            Self::Left => 0.0,
            Self::Center => free / 2.0,
            Self::Right => free,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SectionStyle {
    pub heading: SectionHeading,
    pub heading_alignment: HeadingAlignment,
    pub items: ItemLayout,
    pub date: DatePlacement,
}

impl SectionStyle {
    pub fn item_gap_pt(self) -> f32 {
        if self.items == ItemLayout::Compact {
            2.0
        } else {
            4.0
        }
    }

    pub fn paragraph_gap_pt(self) -> f32 {
        if self.items == ItemLayout::Compact {
            1.0
        } else {
            3.0
        }
    }
}

#[derive(Debug, Clone)]
pub struct ThemeStyle {
    pub id: String,
    pub layout: String,
    pub colors: Colors,
    pub page: PageStyle,
    pub typography: Typography,
    pub composition_preset: String,
    /// Optional presentation order. Unlisted sections retain their input order.
    pub module_order: Vec<String>,
    pub centered_header: bool,
    pub split_header: bool,
    pub stacked_contacts: bool,
    pub header_region: String,
    pub summary_region: String,
    pub contact_region: String,
    pub sidebar_typography_scale: f32,
    pub sidebar_width_pct: f32,
    pub column_gap_mm: f32,
    pub sidebar_modules: Vec<String>,
    pub section_titles: BTreeMap<String, String>,
    pub(crate) entry_headers: BTreeMap<String, Vec<crate::entry_header::HeaderLine>>,
    pub sections: BTreeMap<String, SectionStyle>,
}

fn optional_color(value: &Value, key: &str) -> Option<Rgb> {
    value.get(key).and_then(Value::as_str).map(Rgb::parse)
}

fn number(value: &Value, key: &str, default: f32) -> f32 {
    value
        .get(key)
        .and_then(Value::as_f64)
        .map_or(default, |value| value as f32)
}

fn text_style(
    value: Option<&Value>,
    family: &str,
    size_pt: f32,
    weight: u16,
    line_height: f32,
) -> TextStyle {
    let value = value.unwrap_or(&Value::Null);
    TextStyle {
        family: value
            .get("family")
            .and_then(Value::as_str)
            .unwrap_or(family)
            .to_owned(),
        size_pt: number(value, "sizePt", size_pt),
        weight: value
            .get("weight")
            .and_then(Value::as_u64)
            .map_or(weight, |value| value as u16),
        italic: value["fontStyle"].as_str() == Some("italic"),
        color: value["color"].as_str().map(Rgb::parse),
        line_height: number(value, "lineHeight", line_height),
        tracking_em: number(value, "trackingEm", 0.0),
        text_transform: match value["textTransform"].as_str() {
            Some("uppercase") => TextTransform::Uppercase,
            Some("lowercase") => TextTransform::Lowercase,
            _ => TextTransform::None,
        },
        asset_font: None,
    }
}

pub(crate) fn inherited_text_style(value: Option<&Value>, base: &TextStyle) -> TextStyle {
    let Some(value) = value else {
        return base.clone();
    };
    let mut role = text_style(
        Some(value),
        &base.family,
        base.size_pt,
        base.weight,
        base.line_height,
    );
    role.tracking_em = number(value, "trackingEm", base.tracking_em);
    if value.get("color").is_none() {
        role.color = base.color;
    }
    if value.get("fontStyle").is_none() {
        role.italic = base.italic;
    }
    if value.get("textTransform").is_none() {
        role.text_transform = base.text_transform;
    }
    role
}

impl ThemeStyle {
    pub fn all_text_styles(&self) -> impl Iterator<Item = &TextStyle> {
        [
            &self.typography.body,
            &self.typography.item_title,
            &self.typography.name,
            &self.typography.section_title,
            &self.typography.meta,
            &self.typography.label,
            &self.typography.summary,
            &self.typography.side_date,
        ]
        .into_iter()
        .chain(
            self.entry_headers
                .values()
                .flatten()
                .map(|line| &line.style),
        )
    }

    pub fn card_paper(&self) -> Rgb {
        let tinted = self.colors.paper.mix(self.colors.accent, 0.035);
        if [self.colors.ink, self.colors.muted, self.colors.accent]
            .iter()
            .all(|ink| ink.contrast(tinted) >= 4.5)
        {
            tinted
        } else {
            self.colors.paper
        }
    }
    pub fn has_sidebar(&self) -> bool {
        matches!(
            self.composition_preset.as_str(),
            "sidebar-left" | "sidebar-right"
        )
    }

    pub fn in_sidebar(&self, id: &str) -> bool {
        self.has_sidebar() && self.sidebar_modules.iter().any(|module| module == id)
    }

    /// Shared by preview/PDF and editable Word output; never mutates source content.
    pub(crate) fn ordered_sections<'a>(&self, sections: &'a [Section]) -> Vec<&'a Section> {
        let mut ordered: Vec<_> = sections.iter().collect();
        if !self.module_order.is_empty() {
            // Stable sorting appends every unlisted section in its original order.
            ordered.sort_by_key(|section| {
                self.module_order
                    .iter()
                    .position(|id| id == section.id)
                    .unwrap_or(usize::MAX)
            });
        }
        ordered
    }

    /// Author-supplied plain display text never changes a section's identity.
    pub fn section_title(&self, id: &str, language: Language) -> &str {
        self.section_titles
            .get(id)
            .map(String::as_str)
            .unwrap_or_else(|| title(id, language))
    }

    /// Content lanes include a 6 pt inset on each side of the tinted lane.
    /// Region-local styles never affect application chrome or source data.
    pub fn column_style(&self, sidebar: bool) -> Self {
        let mut style = self.clone();
        if !self.has_sidebar() {
            return style;
        }
        let content_mm = self.page.width_mm - self.page.margins.left - self.page.margins.right;
        let sidebar_mm = (content_mm - self.column_gap_mm) * self.sidebar_width_pct / 100.0;
        let main_mm = content_mm - self.column_gap_mm - sidebar_mm;
        let sidebar_left = self.composition_preset == "sidebar-left";
        let (offset, width) = match (sidebar, sidebar_left) {
            (true, true) => (0.0, sidebar_mm),
            (true, false) => (main_mm + self.column_gap_mm, sidebar_mm),
            (false, true) => (sidebar_mm + self.column_gap_mm, main_mm),
            (false, false) => (0.0, main_mm),
        };
        let inset = if sidebar {
            6.0 / crate::layout::PT_PER_MM
        } else {
            0.0
        };
        style.page.margins.left = self.page.margins.left + offset + inset;
        style.page.margins.right =
            self.page.width_mm - self.page.margins.left - offset - width + inset;
        if sidebar {
            for line in style.entry_headers.values_mut().flatten() {
                line.style.size_pt = (line.style.size_pt * self.sidebar_typography_scale).max(7.0);
            }
            for role in [
                &mut style.typography.body,
                &mut style.typography.item_title,
                &mut style.typography.name,
                &mut style.typography.section_title,
                &mut style.typography.meta,
                &mut style.typography.label,
                &mut style.typography.summary,
                &mut style.typography.side_date,
            ] {
                role.size_pt = (role.size_pt * self.sidebar_typography_scale).max(7.0);
            }
            style.colors.paper = self.colors.sidebar;
            style.colors.ink = style
                .colors
                .ink
                .readable(style.colors.paper, style.colors.ink, 4.5);
            style.colors.muted =
                style
                    .colors
                    .muted
                    .readable(style.colors.paper, style.colors.ink, 4.5);
            style.colors.accent =
                style
                    .colors
                    .accent
                    .readable(style.colors.paper, style.colors.ink, 4.5);
        }
        style
    }

    pub fn section(&self, id: &str) -> SectionStyle {
        self.sections.get(id).copied().unwrap_or_default()
    }

    pub fn from_manifest(theme: &ThemeManifest) -> Self {
        let manifest = &theme.normalized;
        let colors = &manifest["colors"];
        let paper = Rgb::parse(colors["paper"].as_str().expect("validated paper"));
        let canvas = Rgb::parse(colors["canvas"].as_str().expect("validated canvas"));
        let fallback = if paper.contrast(Rgb {
            r: 24,
            g: 35,
            b: 34,
        }) >= 4.5
        {
            Rgb {
                r: 24,
                g: 35,
                b: 34,
            }
        } else {
            Rgb {
                r: 246,
                g: 249,
                b: 247,
            }
        };
        let ink = optional_color(colors, "ink")
            .unwrap_or(fallback)
            .readable(paper, fallback, 4.5);
        let accent = Rgb::parse(colors["accent"].as_str().expect("validated accent"))
            .readable(paper, ink, 4.5);
        let muted = optional_color(colors, "muted")
            .unwrap_or(ink.mix(paper, 0.25))
            .readable(paper, ink, 4.5);
        let border = optional_color(colors, "border").unwrap_or(accent.mix(paper, 0.65));
        let sidebar = optional_color(colors, "sidebar").unwrap_or(canvas.mix(paper, 0.5));
        let chrome = canvas.mix(paper, 0.28);
        let page = manifest.get("page").unwrap_or(&Value::Null);
        let margins = page.get("marginsMm").unwrap_or(&Value::Null);
        let typography = manifest.get("typography").unwrap_or(&Value::Null);
        let body = text_style(typography.get("body"), "cjk-sans", 10.0, 400, 1.4);
        let meta = text_style(typography.get("meta"), "cjk-sans", 8.5, 400, 1.3);
        let label = inherited_text_style(typography.get("label"), &body);
        let summary = inherited_text_style(typography.get("summary"), &body);
        let side_date = inherited_text_style(typography.get("sideDate"), &meta);
        // Absent itemTitle preserves the legacy body-derived geometry exactly.
        let mut item_title = text_style(
            typography.get("itemTitle"),
            &body.family,
            body.size_pt * 1.06,
            body.weight,
            body.line_height,
        );
        if typography["itemTitle"].get("fontStyle").is_none() {
            item_title.italic = body.italic;
        }
        if typography["itemTitle"].get("color").is_none() {
            item_title.color = body.color;
        }
        let size = page.get("size").and_then(Value::as_str).unwrap_or("A4");
        let mut style = Self {
            id: theme.id.clone(),
            layout: manifest["layout"]
                .as_str()
                .expect("validated layout")
                .to_owned(),
            colors: Colors {
                accent,
                seal: Rgb::parse(colors["seal"].as_str().expect("validated seal")),
                paper,
                canvas,
                ink,
                muted,
                border,
                sidebar,
                chrome,
            },
            page: PageStyle {
                width_mm: if size == "Letter" { 215.9 } else { 210.0 },
                height_mm: if size == "Letter" { 279.4 } else { 297.0 },
                margins: MarginsMm {
                    top: number(margins, "top", 16.0),
                    right: number(margins, "right", 16.0),
                    bottom: number(margins, "bottom", 16.0),
                    left: number(margins, "left", 16.0),
                },
                section_gap_mm: number(page, "sectionGapMm", 6.0),
                avoid_split_items: page
                    .get("avoidSplitItems")
                    .and_then(Value::as_bool)
                    .unwrap_or(true),
                keep_heading_with_first_item: page
                    .get("keepHeadingWithFirstItem")
                    .and_then(Value::as_bool)
                    .unwrap_or(true),
                first_line_position: match page["firstLinePosition"].as_str() {
                    Some("line-box") => FirstLinePosition::LineBox,
                    _ => FirstLinePosition::Baseline,
                },
            },
            typography: Typography {
                body,
                item_title,
                name: text_style(typography.get("name"), "cjk-serif", 26.0, 600, 1.2),
                section_title: text_style(
                    typography.get("sectionTitle"),
                    "cjk-serif",
                    13.0,
                    600,
                    1.3,
                ),
                meta,
                label,
                summary,
                side_date,
            },
            composition_preset: manifest
                .get("composition")
                .and_then(|value| value.get("preset"))
                .and_then(Value::as_str)
                .unwrap_or("single-column")
                .to_owned(),
            module_order: manifest["composition"]["moduleOrder"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|id| id.as_str().expect("schema checked module order").to_owned())
                .collect(),
            centered_header: manifest["composition"]["header"] == "centered",
            split_header: manifest["composition"]["header"] == "split"
                || manifest["composition"]["preset"] == "split-header",
            stacked_contacts: manifest["composition"]["contact"] == "stacked",
            header_region: manifest["composition"]["headerRegion"]
                .as_str()
                .unwrap_or("page")
                .to_owned(),
            summary_region: manifest["composition"]["summaryRegion"]
                .as_str()
                .unwrap_or("page")
                .to_owned(),
            contact_region: manifest["composition"]["contactRegion"]
                .as_str()
                .unwrap_or("header")
                .to_owned(),
            sidebar_typography_scale: number(
                &manifest["composition"],
                "sidebarTypographyScale",
                1.0,
            ),
            sidebar_width_pct: number(&manifest["composition"], "sidebarWidthPct", 30.0),
            column_gap_mm: number(&manifest["composition"], "gapMm", 6.0),
            sidebar_modules: manifest["composition"]["placements"]
                .as_object()
                .into_iter()
                .flatten()
                .filter(|(_, region)| region.as_str() == Some("sidebar"))
                .map(|(id, _)| id.clone())
                .collect(),
            entry_headers: BTreeMap::new(),
            section_titles: manifest
                .get("sections")
                .and_then(Value::as_object)
                .into_iter()
                .flatten()
                .filter_map(|(id, value)| {
                    value["title"]
                        .as_str()
                        .map(|text| (id.clone(), text.to_owned()))
                })
                .collect(),
            sections: manifest
                .get("sections")
                .and_then(Value::as_object)
                .into_iter()
                .flatten()
                .map(|(id, value)| {
                    (
                        id.clone(),
                        SectionStyle {
                            heading_alignment: match value["headingAlign"].as_str() {
                                Some("center") => HeadingAlignment::Center,
                                Some("right") => HeadingAlignment::Right,
                                _ => HeadingAlignment::Left,
                            },
                            heading: match value["heading"].as_str() {
                                Some("line") => SectionHeading::Line,
                                Some("accent-line") => SectionHeading::AccentLine,
                                Some("filled") => SectionHeading::Filled,
                                _ => SectionHeading::Plain,
                            },
                            items: match value["items"].as_str() {
                                Some("compact") => ItemLayout::Compact,
                                Some("timeline") => ItemLayout::Timeline,
                                Some("cards") => ItemLayout::Cards,
                                Some("chips") => ItemLayout::Chips,
                                _ => ItemLayout::Stack,
                            },
                            date: match value["date"].as_str() {
                                Some("left") => DatePlacement::Left,
                                Some("right") => DatePlacement::Right,
                                _ => DatePlacement::Inline,
                            },
                        },
                    )
                })
                .collect(),
        };
        for role in [
            &mut style.typography.body,
            &mut style.typography.item_title,
            &mut style.typography.name,
            &mut style.typography.section_title,
            &mut style.typography.meta,
            &mut style.typography.label,
            &mut style.typography.summary,
            &mut style.typography.side_date,
        ] {
            role.asset_font = role
                .family
                .strip_prefix("asset:")
                .and_then(|id| theme.asset_fonts.get(id))
                .copied();
        }
        style.entry_headers = manifest["sections"]
            .as_object()
            .into_iter()
            .flatten()
            .filter_map(|(id, section)| {
                section.get("entryHeader").map(|value| {
                    (
                        id.clone(),
                        crate::entry_header::parse_lines(value, &style, theme),
                    )
                })
            })
            .collect();
        style
    }

    pub fn built_in_fallback() -> Self {
        let theme = crate::parse_theme(r##"{"formatVersion":2,"id":"resume-one-default","name":"简里","version":"1.0.0","license":"Apache-2.0","layout":"classic","colors":{"accent":"#57766C","seal":"#A14F40","paper":"#FFFFFF","canvas":"#F5F3EE"},"modules":["basics","work","volunteer","education","awards","certificates","publications","skills","languages","interests","references","projects"]}"##.as_bytes())
            .expect("bundled fallback theme must validate");
        Self::from_manifest(&theme)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_line_position_is_strict_opt_in_and_survives_lane_styles() {
        let mut theme =
            crate::parse_theme(include_bytes!("../../../assets/themes/zhibai.theme.json")).unwrap();
        assert_eq!(
            ThemeStyle::from_manifest(&theme).page.first_line_position,
            FirstLinePosition::Baseline
        );
        for (value, expected) in [
            ("baseline", FirstLinePosition::Baseline),
            ("line-box", FirstLinePosition::LineBox),
        ] {
            theme.normalized["page"]["firstLinePosition"] = serde_json::json!(value);
            let validated = crate::parse_theme(theme.normalized.to_string().as_bytes()).unwrap();
            let style = ThemeStyle::from_manifest(&validated);
            assert_eq!(style.page.first_line_position, expected);
            for lane in [false, true] {
                assert_eq!(style.column_style(lane).page.first_line_position, expected);
            }
        }
        for invalid in [
            serde_json::json!("auto"),
            serde_json::json!(false),
            serde_json::json!({"code":"run()"}),
        ] {
            theme.normalized["page"]["firstLinePosition"] = invalid;
            assert!(crate::parse_theme(theme.normalized.to_string().as_bytes()).is_err());
        }
    }

    #[test]
    fn authored_display_title_is_preserved_in_both_languages_and_column_styles() {
        let mut theme =
            crate::parse_theme(include_bytes!("../../../assets/themes/zhibai.theme.json")).unwrap();
        let original = ThemeStyle::from_manifest(&theme);
        assert_eq!(original.section_title("work", Language::En), "Work");
        assert_eq!(original.section_title("work", Language::Zh), "工作经历");
        theme.normalized["sections"]["work"]["title"] =
            serde_json::json!("Professional Experience");
        let style = ThemeStyle::from_manifest(&theme);
        for lang in [Language::En, Language::Zh] {
            assert_eq!(style.section_title("work", lang), "Professional Experience");
            assert_eq!(
                style.column_style(true).section_title("work", lang),
                "Professional Experience"
            );
        }
        assert_eq!(style.section_title("skills", Language::En), "Skills");
    }

    #[test]
    fn exact_role_colors_inherit_without_recoloring_document_or_application_palette() {
        let mut theme =
            crate::parse_theme(include_bytes!("../../../assets/themes/zhibai.theme.json")).unwrap();
        let before = ThemeStyle::from_manifest(&theme);
        theme.normalized["typography"]["body"]["color"] = serde_json::json!("#DADADA");
        theme.normalized["typography"]["meta"]["color"] = serde_json::json!("#445566");
        theme.normalized["typography"]["label"] = serde_json::json!({"color":"#123456"});
        let style = ThemeStyle::from_manifest(&theme);
        assert_eq!(style.typography.body.color.unwrap().hex(), "#DADADA");
        assert_eq!(
            style.typography.item_title.color,
            style.typography.body.color
        );
        assert_eq!(style.typography.summary.color, style.typography.body.color);
        assert_eq!(
            style.typography.side_date.color,
            style.typography.meta.color
        );
        assert_eq!(style.typography.label.color.unwrap().hex(), "#123456");
        assert!(
            style.typography.name.color.is_none() && style.typography.section_title.color.is_none()
        );
        assert_eq!(style.colors.ink, before.colors.ink);
        assert_eq!(style.colors.accent, before.colors.accent);
        assert_eq!(style.colors.chrome, before.colors.chrome);
    }

    #[test]
    fn real_slant_is_inherited_only_by_dependent_roles() {
        let mut theme =
            crate::parse_theme(include_bytes!("../../../assets/themes/zhibai.theme.json")).unwrap();
        theme.normalized["typography"]["body"]["fontStyle"] = serde_json::json!("italic");
        theme.normalized["typography"]["meta"]["fontStyle"] = serde_json::json!("italic");
        let style = ThemeStyle::from_manifest(&theme);
        for role in [
            &style.typography.body,
            &style.typography.item_title,
            &style.typography.label,
            &style.typography.summary,
            &style.typography.side_date,
        ] {
            assert!(role.italic);
        }
        assert!(!style.typography.name.italic);
        assert!(!style.typography.section_title.italic);
        for role in ["itemTitle", "label", "summary", "sideDate"] {
            theme.normalized["typography"][role] = serde_json::json!({"fontStyle":"normal"});
        }
        let explicit = ThemeStyle::from_manifest(&theme);
        for role in [
            &explicit.typography.item_title,
            &explicit.typography.label,
            &explicit.typography.summary,
            &explicit.typography.side_date,
        ] {
            assert!(!role.italic);
        }
        assert!(explicit.typography.body.italic);
        assert!(explicit.typography.meta.italic);
    }

    #[test]
    fn dependent_roles_inherit_all_base_properties_and_override_independently() {
        let mut theme =
            crate::parse_theme(include_bytes!("../../../assets/themes/zhibai.theme.json")).unwrap();
        theme.normalized["typography"]["body"]["trackingEm"] = serde_json::json!(0.03);
        theme.normalized["typography"]["body"]["textTransform"] = serde_json::json!("uppercase");
        let inherited = ThemeStyle::from_manifest(&theme);
        for role in [&inherited.typography.label, &inherited.typography.summary] {
            assert_eq!(role.size_pt, inherited.typography.body.size_pt);
            assert_eq!(role.family, inherited.typography.body.family);
            assert_eq!(role.tracking_em, 0.03);
            assert_eq!(role.text_transform, TextTransform::Uppercase);
        }
        assert_eq!(
            inherited.typography.side_date.size_pt,
            inherited.typography.meta.size_pt
        );
        theme.normalized["typography"]["label"] =
            serde_json::json!({"sizePt":14,"textTransform":"lowercase"});
        theme.normalized["typography"]["summary"] = serde_json::json!({"lineHeight":1.8});
        theme.normalized["typography"]["sideDate"] =
            serde_json::json!({"family":"cjk-serif","weight":600,"sizePt":11});
        let explicit = ThemeStyle::from_manifest(&theme);
        assert_eq!(explicit.typography.label.size_pt, 14.0);
        assert_eq!(explicit.typography.label.tracking_em, 0.03);
        assert_eq!(
            explicit.typography.label.text_transform,
            TextTransform::Lowercase
        );
        assert_eq!(explicit.typography.summary.line_height, 1.8);
        assert_eq!(
            explicit.typography.body.line_height,
            inherited.typography.body.line_height
        );
        assert_eq!(explicit.typography.side_date.weight, 600);
        assert_eq!(explicit.typography.meta.weight, 400);
    }

    #[test]
    fn header_and_contact_defaults_are_independent_partial_overrides() {
        let mut theme =
            crate::parse_theme(include_bytes!("../../../assets/themes/zhibai.theme.json")).unwrap();
        for (composition, expected) in [
            (serde_json::json!({}), (false, false, false)),
            (
                serde_json::json!({"header":"centered"}),
                (true, false, false),
            ),
            (
                serde_json::json!({"contact":"stacked"}),
                (false, false, true),
            ),
            (
                serde_json::json!({"header":"centered","contact":"stacked"}),
                (true, false, true),
            ),
            (serde_json::json!({"header":"split"}), (false, true, false)),
            (
                serde_json::json!({"preset":"split-header"}),
                (false, true, false),
            ),
        ] {
            theme.normalized["composition"] = composition;
            let style = ThemeStyle::from_manifest(&theme);
            assert_eq!(
                (
                    style.centered_header,
                    style.split_header,
                    style.stacked_contacts
                ),
                expected
            );
        }
    }

    #[test]
    fn section_styles_are_partial_scoped_and_keep_unmentioned_defaults() {
        let mut theme = crate::parse_theme(include_bytes!(
            "../../resume-one-cli/tests/fixtures/section-styles.theme.json"
        ))
        .unwrap();
        let style = ThemeStyle::from_manifest(&theme);
        assert_eq!(
            style.section("work"),
            SectionStyle {
                heading: SectionHeading::Filled,
                heading_alignment: HeadingAlignment::Left,
                items: ItemLayout::Compact,
                date: DatePlacement::Inline,
            }
        );
        assert_eq!(
            style.section("projects"),
            SectionStyle {
                heading: SectionHeading::AccentLine,
                heading_alignment: HeadingAlignment::Left,
                items: ItemLayout::Stack,
                date: DatePlacement::Inline,
            }
        );
        assert_eq!(style.section("languages"), SectionStyle::default());
        assert_eq!(style.section("work").item_gap_pt(), 2.0);
        assert_eq!(style.section("work").paragraph_gap_pt(), 1.0);
        assert_eq!(style.section("languages").item_gap_pt(), 4.0);
        theme.normalized["sections"]["work"]["date"] = serde_json::json!("right");
        assert_eq!(
            ThemeStyle::from_manifest(&theme).section("work").date,
            DatePlacement::Right
        );
        theme.normalized["sections"]["work"]["date"] = serde_json::json!("left");
        assert_eq!(
            ThemeStyle::from_manifest(&theme).section("work").date,
            DatePlacement::Left
        );
    }

    #[test]
    fn derives_readable_colors_and_schema_defaults() {
        let source = br##"{"formatVersion":2,"id":"pale","name":"Pale","version":"1.0.0","license":"MIT","layout":"classic","colors":{"accent":"#FFFFCC","seal":"#AA5544","paper":"#FFFFFF","canvas":"#F7F7F7"},"modules":["basics","work"]}"##;
        let theme = crate::parse_theme(source).unwrap();
        let style = ThemeStyle::from_manifest(&theme);
        assert!(style.colors.accent.contrast(style.colors.paper) >= 4.5);
        assert!(style.colors.ink.contrast(style.colors.paper) >= 4.5);
        assert!(style.colors.muted.contrast(style.colors.paper) >= 4.5);
        assert_eq!(style.page.margins.top, 16.0);
        assert_eq!(style.typography.body.size_pt, 10.0);
    }

    #[test]
    fn reads_letter_and_explicit_typography() {
        let source = br##"{"formatVersion":2,"id":"letter","name":"Letter","version":"1.0.0","license":"MIT","layout":"rules","colors":{"accent":"#112233","seal":"#AA5544","paper":"#FDFDFD","canvas":"#EEEEEE"},"modules":["basics"],"page":{"size":"Letter","marginsMm":{"top":12,"right":13,"bottom":14,"left":15}},"typography":{"body":{"sizePt":11,"lineHeight":1.5}}}"##;
        let theme = crate::parse_theme(source).unwrap();
        let style = ThemeStyle::from_manifest(&theme);
        assert_eq!(style.page.height_mm, 279.4);
        assert_eq!(style.page.margins.left, 15.0);
        assert_eq!(style.typography.body.size_pt, 11.0);
        assert_eq!(style.typography.body.line_height, 1.5);
    }

    #[test]
    fn item_title_inherits_body_unless_explicitly_overridden() {
        let mut theme =
            crate::parse_theme(include_bytes!("../../../assets/themes/zhibai.theme.json")).unwrap();
        let inherited = ThemeStyle::from_manifest(&theme);
        assert_eq!(
            inherited.typography.item_title.family,
            inherited.typography.body.family
        );
        assert_eq!(
            inherited.typography.item_title.size_pt,
            inherited.typography.body.size_pt * 1.06
        );
        theme.normalized["typography"]["itemTitle"] =
            serde_json::json!({"family":"cjk-serif","sizePt":12,"weight":600,"lineHeight":1.25});
        let explicit = ThemeStyle::from_manifest(&theme);
        assert_eq!(explicit.typography.item_title.family, "cjk-serif");
        assert_eq!(explicit.typography.item_title.size_pt, 12.0);
        assert_eq!(explicit.typography.item_title.weight, 600);
        assert_eq!(
            explicit.typography.body.size_pt,
            inherited.typography.body.size_pt
        );
        crate::layout::check_theme(&theme).unwrap();
    }

    #[test]
    fn keeps_dark_paper_text_readable() {
        let source = br##"{"formatVersion":2,"id":"night","name":"Night","version":"1.0.0","license":"MIT","layout":"minimal","colors":{"accent":"#203020","seal":"#AA5544","paper":"#152123","canvas":"#101719","ink":"#888888","muted":"#777777"},"modules":["basics"]}"##;
        let theme = crate::parse_theme(source).unwrap();
        let style = ThemeStyle::from_manifest(&theme);
        assert!(style.colors.ink.contrast(style.colors.paper) >= 4.5);
        assert!(style.colors.muted.contrast(style.colors.paper) >= 4.5);
        assert!(style.colors.accent.contrast(style.colors.paper) >= 4.5);
    }
}
