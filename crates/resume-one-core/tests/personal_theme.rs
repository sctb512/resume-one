use resume_one_core::{
    docx,
    layout::{LayoutElement, LayoutTarget, layout_with_theme},
    parse_theme, pdf,
    render::{Language, markdown},
    resume::parse_resume,
    semantic::project,
    theme_style::ThemeStyle,
};
use serde_json::{Value, json};

const BASE: &str = include_str!("../../../assets/themes/zhibai.theme.json");

fn theme_value() -> Value {
    serde_json::from_str(BASE).unwrap()
}

fn manifest(value: &Value) -> resume_one_core::ThemeManifest {
    parse_theme(&serde_json::to_vec(value).unwrap()).unwrap()
}

fn printed(layout: &resume_one_core::layout::ResumeLayout) -> Vec<&str> {
    layout
        .pages
        .iter()
        .flatten()
        .filter_map(|element| match element {
            LayoutElement::Text(text) if text.target.is_some() => Some(text.text.as_str()),
            _ => None,
        })
        .collect()
}

fn word_xml(bytes: &[u8]) -> String {
    use std::io::{Cursor, Read};
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    archive
        .by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    xml
}

#[test]
fn personal_theme_controls_print_without_rewriting_the_resume() {
    let original = br#"{"basics":{"name":"Ada"},"work":[{"position":"WORKMARK","location":"PLACEONLY","summary":"REMOVEFROMPAPER","highlights":["KEEPTAIL"]}],"education":[{"institution":"EDUMARK"}],"skills":[{"name":"SKILLMARK"}]}"#;
    let document = parse_resume(original).unwrap();
    let content = project(&document);
    let mut theme = theme_value();
    theme["composition"]["moduleOrder"] = json!(["education", "work"]);
    theme["composition"]["hiddenModules"] = json!(["skills"]);
    theme["composition"]["hiddenBasics"] = json!(["name"]);
    theme["page"]["marginsMm"] = json!({"top":12,"left":11,"right":14,"bottom":10});
    theme["sections"]["work"]["hiddenElements"] = json!(["heading", "description"]);
    theme["sections"]["work"]["elements"] =
        json!([{"kind":"text","text":"PAPER ONLY"}, {"kind":"divider"}]);
    theme["sections"]["work"]["entryHeaderMode"] = json!("append");
    theme["sections"]["work"]["entryHeader"] =
        json!([{"role":"meta","parts":[{"field":"location"}]}]);

    let theme = manifest(&theme);
    let layout = layout_with_theme(&content, Language::En, &theme).unwrap();
    let preview = printed(&layout);
    let start = preview.iter().position(|s| s.contains("EDUMARK")).unwrap();
    assert!(start < preview.iter().position(|s| s.contains("WORKMARK")).unwrap());
    for text in ["WORKMARK", "PLACEONLY", "KEEPTAIL", "PAPER ONLY"] {
        assert!(preview.iter().any(|line| line.contains(text)), "{text}");
    }
    for text in ["REMOVEFROMPAPER", "SKILLMARK", "Work", "Ada"] {
        assert!(!preview.iter().any(|line| line.contains(text)), "{text}");
    }
    assert!(layout.pages.iter().flatten().any(|element| matches!(
        element, LayoutElement::Anchor(anchor) if anchor.target == LayoutTarget {section: "work", index: Some(0)}
    )));
    let word = word_xml(&docx::render_with_theme(&content, Language::En, &theme).unwrap());
    assert!(word.find("EDUMARK").unwrap() < word.find("WORKMARK").unwrap());
    for text in [
        "PLACEONLY",
        "KEEPTAIL",
        "PAPER ONLY",
        "w:left=\"624\"",
        "w:right=\"794\"",
    ] {
        assert!(word.contains(text), "{text}");
    }
    assert!(!word.contains("SKILLMARK"));
    assert!(!word.contains("REMOVEFROMPAPER"));
    assert!(!word.contains("Ada"));
    assert!(markdown(&content, Language::En).contains("REMOVEFROMPAPER"));
    assert_eq!(document.source, original);
}

#[test]
fn one_page_fit_compacts_a_borderline_resume_and_keeps_long_content() {
    let make_content = |count| {
        let work = (0..count).map(|i| json!({
            "position":format!("Position {i:03}"),
            "name":"Studio",
            "summary":"Designed and delivered accessible products with engineering and research teams.",
            "highlights":[format!("Verified measurable outcomes for project {i:03}.")],
        })).collect::<Vec<_>>();
        project(
            &parse_resume(
                &serde_json::to_vec(&json!({"basics":{"name":"Ada"},"work":work})).unwrap(),
            )
            .unwrap(),
        )
    };
    let mut value = theme_value();
    let natural_theme = manifest(&value);
    value["page"]["fitToOnePage"] = json!(true);
    let fit_theme = manifest(&value);
    let (content, fitted) = (7..=18)
        .find_map(|count| {
            let content = make_content(count);
            let natural = layout_with_theme(&content, Language::En, &natural_theme).unwrap();
            if natural.pages.len() <= 1 {
                return None;
            }
            let fitted = layout_with_theme(&content, Language::En, &fit_theme).unwrap();
            (fitted.pages.len() == 1).then_some((content, fitted))
        })
        .expect("a near-threshold resume should fit");
    for item in &content.sections[0].items {
        assert!(
            printed(&fitted)
                .iter()
                .any(|text| text.contains(&item.heading)),
            "{}",
            item.heading
        );
    }
    assert!(fitted.pages[0].iter().all(|op| !matches!(op,
        LayoutElement::Text(text) if text.target.is_some() && text.size_pt < 8.0)));
    let pdf = pdf::render_with_theme(&content, Language::En, &fit_theme).unwrap();
    assert!(pdf.starts_with(b"%PDF-"));
    let word = word_xml(&docx::render_with_theme(&content, Language::En, &fit_theme).unwrap());
    assert!(word.contains(content.sections[0].items.last().unwrap().bullets[0].as_str()));

    let long = make_content(90);
    let natural = layout_with_theme(&long, Language::En, &natural_theme).unwrap();
    let too_long = layout_with_theme(&long, Language::En, &fit_theme).unwrap();
    assert_eq!(too_long, natural);
    assert!(too_long.pages.len() > 1);
    assert!(
        printed(&too_long)
            .iter()
            .any(|text| text.contains("Position 089"))
    );
    assert!(ThemeStyle::from_manifest(&fit_theme).page.fit_to_one_page);
}

#[test]
fn one_page_uses_shared_date_rows_before_tightening_type_or_margins() {
    use resume_one_core::{layout::measured_theme_layout, theme_style::DatePlacement};

    let mut theme = theme_value();
    theme["sections"]["work"]["date"] = json!("inline");
    theme["page"]["marginsMm"] = json!({"top":19,"right":17,"bottom":15,"left":21});
    let default = manifest(&theme);
    let authored = ThemeStyle::from_manifest(&default);
    theme["page"]["fitToOnePage"] = json!(true);
    let opted_in = manifest(&theme);
    let content_for = |count| {
        let work = (0..count)
            .map(|index| {
                json!({
                    "position":format!("Designer {index:02}"), "name":"Studio",
                    "startDate":"2021-06", "endDate":"2024-10",
                    "summary":"Worked with teams across research, product and engineering.",
                })
            })
            .collect::<Vec<_>>();
        project(
            &parse_resume(
                &serde_json::to_vec(&json!({"basics":{"name":"Ada Lin"},"work":work})).unwrap(),
            )
            .unwrap(),
        )
    };
    let short = content_for(2);
    assert_eq!(
        layout_with_theme(&short, Language::En, &default).unwrap(),
        layout_with_theme(&short, Language::En, &opted_in).unwrap()
    );

    let (style, result) = (11..=19)
        .find_map(|count| {
            let content = content_for(count);
            let base = layout_with_theme(&content, Language::En, &default).unwrap();
            if base.pages.len() == 1 {
                return None;
            }
            let (style, layout) = measured_theme_layout(&content, Language::En, &opted_in).unwrap();
            (layout.pages.len() == 1).then_some((style, layout))
        })
        .expect("aligning dates with titles should recover a full page");
    assert_eq!(style.section("work").date, DatePlacement::Right);
    assert_eq!(style.page.section_gap_mm, authored.page.section_gap_mm);
    assert_eq!(
        style.typography.body.size_pt,
        authored.typography.body.size_pt
    );
    assert_eq!(
        style.typography.body.line_height,
        authored.typography.body.line_height
    );
    assert_eq!(style.page.margins.left, 21.0);
    assert_eq!(style.page.margins.right, 17.0);
    assert_eq!(style.page.margins.top, 19.0);
    assert_eq!(style.page.margins.bottom, 15.0);
    let entry = LayoutTarget {
        section: "work",
        index: Some(0),
    };
    let runs: Vec<_> = result.pages[0]
        .iter()
        .filter_map(|node| match node {
            LayoutElement::Text(text) if text.target == Some(entry) => Some(text),
            _ => None,
        })
        .collect();
    let title = runs
        .iter()
        .find(|text| text.text.contains("Designer 00"))
        .unwrap();
    let dates = runs
        .iter()
        .find(|text| text.text.contains("2021-06") && text.text.contains("2024-10"))
        .unwrap();
    assert_eq!(title.baseline_y_pt, dates.baseline_y_pt);
    assert!(title.x_pt + title.width_pt() < dates.x_pt);
}

#[test]
fn a_crowded_sidebar_gets_width_before_type_is_reduced() {
    use resume_one_core::layout::measured_theme_layout;

    let mut theme = theme_value();
    theme["composition"] = json!({
        "preset":"sidebar-left", "sidebarWidthPct":25,
        "placements":{"work":"sidebar"},
    });
    let default = manifest(&theme);
    let authored = ThemeStyle::from_manifest(&default);
    theme["page"]["fitToOnePage"] = json!(true);
    let opted_in = manifest(&theme);
    let (style, layout) = (7..=14)
        .find_map(|count| {
            let work = (0..count)
                .map(|index| {
                    json!({
                        "position":format!("設計師 {index:02}"),
                        "summary":"與產品及工程團隊合作完成研究、設計、交付及後續驗證。",
                    })
                })
                .collect::<Vec<_>>();
            let content = project(
                &parse_resume(
                    &serde_json::to_vec(&json!({
                        "basics":{"name":"林嘉禾"},
                        "work":work, "education":[{"institution":"研究學院","area":"服務設計"}],
                    }))
                    .unwrap(),
                )
                .unwrap(),
            );
            if layout_with_theme(&content, Language::Zh, &default)
                .unwrap()
                .pages
                .len()
                == 1
            {
                return None;
            }
            let (style, layout) = measured_theme_layout(&content, Language::Zh, &opted_in).unwrap();
            (layout.pages.len() == 1).then_some((style, layout))
        })
        .expect("giving wrapping text more room should fit");
    assert!(style.sidebar_width_pct > authored.sidebar_width_pct);
    assert!(style.in_sidebar("work"));
    assert!(!style.in_sidebar("education"));
    assert_eq!(
        style.typography.body.size_pt,
        authored.typography.body.size_pt
    );
    assert_eq!(style.page.section_gap_mm, authored.page.section_gap_mm);
    assert_eq!(style.page.margins.left, authored.page.margins.left);
    assert!(
        printed(&layout)
            .iter()
            .any(|line| line.contains("後續驗證"))
    );
}
