//! Bounded declarative header rows, never expressions, HTML or source mutation.

use serde_json::Value;

use crate::ThemeManifest;
use crate::semantic::Item;
use crate::theme_style::{Rgb, TextStyle, ThemeStyle, inherited_text_style};

#[derive(Debug, Clone)]
pub(crate) struct HeaderPart {
    field: String,
    prefix: String,
    suffix: String,
    short_en_dates: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct HeaderLine {
    parts: Vec<HeaderPart>,
    pub style: TextStyle,
    pub role: String,
    before: f32,
    after: f32,
}

pub(crate) struct HeaderRow<'a> {
    pub text: String,
    pub style: &'a TextStyle,
    pub color: Rgb,
    pub before: f32,
    pub after: f32,
}

pub(crate) fn parse_lines(
    value: &Value,
    base: &ThemeStyle,
    theme: &ThemeManifest,
) -> Vec<HeaderLine> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .map(|line| {
            let role = line["role"].as_str().unwrap_or("itemTitle");
            let base_style = match role {
                "body" => &base.typography.body,
                "meta" => &base.typography.meta,
                "sideDate" => &base.typography.side_date,
                _ => &base.typography.item_title,
            };
            let mut style = inherited_text_style(line.get("style"), base_style);
            style.asset_font = style
                .family
                .strip_prefix("asset:")
                .and_then(|id| theme.asset_fonts.get(id))
                .copied();
            HeaderLine {
                parts: line["parts"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|part| HeaderPart {
                        field: part["field"].as_str().unwrap_or_default().into(),
                        prefix: part["prefix"].as_str().unwrap_or_default().into(),
                        suffix: part["suffix"].as_str().unwrap_or_default().into(),
                        short_en_dates: part["dateFormat"].as_str() == Some("short-en"),
                    })
                    .collect(),
                style,
                role: role.into(),
                before: line["spaceBeforePt"].as_f64().unwrap_or(0.0) as f32,
                after: line["spaceAfterPt"].as_f64().unwrap_or(0.0) as f32,
            }
        })
        .collect()
}

/// Resolve once for both rendering and measurement. Empty fields bring no
/// punctuation, and an entirely empty row contributes neither text nor gaps.
pub(crate) fn rows<'a>(item: &Item, id: &str, style: &'a ThemeStyle) -> Option<Vec<HeaderRow<'a>>> {
    let lines = style.entry_headers.get(id)?;
    let mut previous: Option<(f32, f32)> = None;
    let mut rows = Vec::new();
    for line in lines {
        let mut text = String::new();
        for part in &line.parts {
            let value = if part.short_en_dates && part.field == "dateRange" {
                Some(item.short_en_date_range.as_str())
            } else {
                item.header_fields
                    .get(part.field.as_str())
                    .map(String::as_str)
            };
            if let Some(value) = value.filter(|value| !value.is_empty()) {
                text.push_str(&part.prefix);
                text.push_str(value);
                text.push_str(&part.suffix);
            }
        }
        if text.trim().is_empty() {
            continue;
        }
        let kind = crate::layout::font_kind(&line.style).expect("validated header font");
        let (ascent, descent) = kind.vertical_metrics();
        let leading = line.style.size_pt * line.style.line_height;
        // Do not rely on summed before/after spacing: Word readers can use
        // their maximum instead. An adequate before alone clears both faces.
        let extra = previous.map_or(0.0, |(descent, leading)| {
            (descent + ascent * line.style.size_pt - leading - line.before).max(0.0)
        });
        rows.push(HeaderRow {
            text,
            style: &line.style,
            color: if matches!(line.role.as_str(), "meta" | "sideDate") {
                style.colors.muted
            } else {
                style.colors.ink
            },
            before: line.before + extra,
            after: line.after,
        });
        previous = Some((descent * line.style.size_pt, leading));
    }
    Some(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        appearance::{Appearance, FontChoice},
        layout::{FontKind, check_theme},
        resume::parse_resume,
        semantic::project,
    };
    use serde_json::json;

    fn theme(lines: Value) -> ThemeManifest {
        let mut theme = crate::parse_theme(crate::LEGACY_THEME_TEST_BYTES).unwrap();
        theme.normalized["sections"]["work"]["entryHeader"] = lines;
        theme.normalized["sections"]["work"]["date"] = json!("inline");
        crate::parse_theme(theme.normalized.to_string().as_bytes()).unwrap()
    }

    #[test]
    fn rows_preserve_independent_style_skip_missing_values_and_do_not_rewrite_source() {
        let theme = theme(json!([
            {"role":"itemTitle","parts":[{"field":"position"}],"style":{"sizePt":14,"color":"#123456"},"spaceBeforePt":2,"spaceAfterPt":3},
            {"role":"body","parts":[{"field":"name","prefix":"At ","suffix":"."}],"style":{"sizePt":11,"trackingEm":0.1,"color":"#345678"}},
            {"role":"meta","parts":[{"field":"location","prefix":"Location: "}],"spaceBeforePt":40,"spaceAfterPt":40},
            {"role":"meta","parts":[{"field":"dateRange"}]}
        ]));
        let doc = parse_resume(br#"{"work":[{"position":"Lead","name":"Example & Co","startDate":"2020","endDate":"2024","summary":"BODY","custom":"PRIVATE"}]}"#).unwrap();
        let original = doc.clone();
        let content = project(&doc);
        let style = ThemeStyle::from_manifest(&theme);
        let rows = rows(&content.sections[0].items[0], "work", &style).unwrap();
        assert_eq!(
            rows.iter().map(|r| r.text.as_str()).collect::<Vec<_>>(),
            ["Lead", "At Example & Co.", "2020 – 2024"]
        );
        assert_eq!(rows[0].style.size_pt, 14.0);
        assert_eq!(rows[0].style.color.unwrap().hex(), "#123456");
        assert_eq!(rows[0].before, 2.0);
        assert_eq!(rows[0].after, 3.0);
        assert_eq!(rows[1].style.size_pt, 11.0);
        assert_eq!(rows[1].style.tracking_em, 0.1);
        assert!(rows[2].before < 40.0);
        assert_eq!(doc.value, original.value);
        assert_eq!(doc.source, original.source);
        assert!(
            crate::render::plain_text(&content, crate::render::Language::En)
                .contains("Lead · Example & Co")
        );
        assert!(
            !crate::render::markdown(&content, crate::render::Language::En).contains("At Example")
        );
    }

    #[test]
    fn explicit_date_format_is_shared_by_preview_and_editable_word_but_not_text_exports() {
        use crate::render::Language;
        use std::io::Read;
        let document = parse_resume(br#"{"work":[{"startDate":"2020-06","endDate":"2024-12"},{"startDate":"2018"},{"endDate":"2025"}]}"#).unwrap();
        let source = document.source.clone();
        let content = project(&document);
        let original_text = crate::render::plain_text(&content, Language::En);
        let original_markdown = crate::render::markdown(&content, Language::En);
        for (format, expected) in [
            (None, vec!["2020-06 – 2024-12", "2018", "2025"]),
            (Some("raw"), vec!["2020-06 – 2024-12", "2018", "2025"]),
            (Some("short-en"), vec!["Jun 2020 - Dec 2024", "Jan 2018"]),
        ] {
            let mut part = json!({"field":"dateRange","prefix":"[","suffix":"]"});
            if let Some(format) = format {
                part["dateFormat"] = json!(format);
            }
            let theme = theme(json!([{"role":"meta","parts":[part]}]));
            let layout = crate::layout::layout_with_theme(&content, Language::En, &theme).unwrap();
            let actual: Vec<_> = layout
                .pages
                .iter()
                .flatten()
                .filter_map(|el| {
                    if let crate::layout::LayoutElement::Text(text) = el
                        && text.target.is_some_and(|t| t.index.is_some())
                    {
                        Some(text.text.as_str())
                    } else {
                        None
                    }
                })
                .collect();
            let expected: Vec<_> = expected.into_iter().map(|s| format!("[{s}]")).collect();
            assert_eq!(actual, expected);
            let bytes = crate::docx::render_with_theme(&content, Language::En, &theme).unwrap();
            let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
            let mut xml = String::new();
            zip.by_name("word/document.xml")
                .unwrap()
                .read_to_string(&mut xml)
                .unwrap();
            for value in expected {
                assert!(xml.contains(&value), "{value}");
            }
            if format == Some("short-en") {
                assert!(!xml.contains("[2025]") && !xml.contains("[2020-06"));
            }
            assert_eq!(
                crate::render::plain_text(&content, Language::En),
                original_text
            );
            assert_eq!(
                crate::render::markdown(&content, Language::En),
                original_markdown
            );
            assert_eq!(document.source, source);
        }
    }

    #[test]
    fn native_family_override_preserves_inherited_role_and_explicit_row_weights() {
        let mut theme = theme(json!([
            {"role":"itemTitle","parts":[{"field":"position"}],"style":{"color":"#123456"}},
            {"role":"body","parts":[{"field":"name"}],"style":{"weight":500}}
        ]));
        theme.normalized["typography"]["body"]["weight"] = json!(600);
        theme.normalized["typography"]["itemTitle"] = json!({"color":"#345678"});
        let mut appearance = Appearance::default();
        appearance.body_font = Some(FontChoice::Font("font:Example System Family".into()));
        let effective = appearance.apply(&theme);
        let style = ThemeStyle::from_manifest(&effective);
        assert_eq!(style.typography.item_title.weight, 600);
        assert_eq!(style.entry_headers["work"][0].style.weight, 600);
        assert_eq!(style.entry_headers["work"][1].style.weight, 500);
        assert_eq!(
            style.entry_headers["work"][0].style.family,
            "font:Example System Family"
        );
        assert!(!style.entry_headers["work"][0].style.italic);
        assert!(
            effective.normalized["sections"]["work"]["entryHeader"][0]["style"]
                .get("weight")
                .is_none()
        );
    }

    #[test]
    fn personal_fonts_and_palette_override_nested_styles_and_reset_restores_them() {
        let theme = theme(json!([
            {"role":"body","parts":[{"field":"name"}],"style":{"family":"cjk-serif","weight":600,"color":"#123456","sizePt":11}},
            {"role":"meta","parts":[{"field":"dateRange"}],"style":{"color":"#345678"}}
        ]));
        let original = theme.normalized.clone();
        let mut appearance = Appearance::default();
        appearance.body_font = Some(FontChoice::Sans);
        appearance.set_color("ink", "#224466").unwrap();
        let effective = appearance.apply(&theme);
        check_theme(&effective).unwrap();
        let style = ThemeStyle::from_manifest(&effective);
        assert!(style.entry_headers["work"][0].style.color.is_none());
        assert_eq!(
            crate::layout::font_kind(&style.entry_headers["work"][0].style).unwrap(),
            FontKind::Sans
        );
        assert_eq!(style.entry_headers["work"][0].style.size_pt, 11.0);
        assert_eq!(
            style.entry_headers["work"][1].style.color.unwrap().hex(),
            "#345678"
        );
        appearance.set_color("paper", "#222222").unwrap();
        let effective = appearance.apply(&theme);
        check_theme(&effective).unwrap();
        assert!(
            ThemeStyle::from_manifest(&effective).entry_headers["work"]
                .iter()
                .all(|line| line.style.color.is_none())
        );
        appearance.reset_colors();
        appearance.body_font = None;
        assert_eq!(appearance.apply(&theme).normalized, original);
        assert_eq!(theme.normalized, original);
    }

    #[test]
    fn adjacent_size_changes_have_clearance_and_sidebar_scaling_is_not_applied_twice() {
        let mut theme = theme(json!([
            {"role":"body","parts":[{"field":"position"}],"style":{"sizePt":7,"lineHeight":1}},
            {"role":"body","parts":[{"field":"name"}],"style":{"sizePt":30,"lineHeight":1}}
        ]));
        theme.normalized["composition"] = json!({"preset":"sidebar-left","placements":{"work":"sidebar"},"sidebarTypographyScale":0.8});
        let style = ThemeStyle::from_manifest(&theme);
        let content =
            project(&parse_resume(br#"{"work":[{"position":"Small","name":"Large"}]}"#).unwrap());
        let item = &content.sections[0].items[0];
        let result = rows(item, "work", &style).unwrap();
        assert!(result[1].before > 10.0);
        let side = style.column_style(true);
        assert_eq!(side.entry_headers["work"][0].style.size_pt, 7.0);
        assert_eq!(side.entry_headers["work"][1].style.size_pt, 24.0);
        assert_eq!(style.entry_headers["work"][1].style.size_pt, 30.0);
        assert_eq!(
            style.column_style(false).entry_headers["work"][1]
                .style
                .size_pt,
            30.0
        );
    }
}
