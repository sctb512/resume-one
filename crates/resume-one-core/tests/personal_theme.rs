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
