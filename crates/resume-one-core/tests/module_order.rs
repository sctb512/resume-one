use std::io::{Cursor, Read};

use resume_one_core::{
    ThemeManifest, docx,
    layout::{self, LayoutElement},
    parse_theme, pdf,
    render::{self, Language},
    resume::parse_resume,
    semantic::{ResumeContent, project},
    theme_style::ThemeStyle,
};
use serde_json::{Value, json};

const RESUME: &[u8] = br#"{
  "projects": [{"name":"PROJECTMARKER"}],
  "skills": [{"name":"SKILLMARKER"}],
  "basics": {"name":"NAMEMARKER","summary":"SUMMARYMARKER"},
  "education": [{"institution":"EDUMARKER"}],
  "awards": [],
  "work": [{"name":"WORKMARKER"},{"name":"SECONDMARKER"}],
  "interests": [{"name":"INTERESTMARKER"}],
  "languages": [{"language":"LANGMARKER"}],
  "custom": {"kept":"extension content"}
}"#;

const INPUT_ORDER: [&str; 6] = [
    "PROJECTMARKER",
    "SKILLMARKER",
    "EDUMARKER",
    "WORKMARKER",
    "INTERESTMARKER",
    "LANGMARKER",
];
const THEME_ORDER: [&str; 6] = [
    "WORKMARKER",
    "EDUMARKER",
    "SKILLMARKER",
    "LANGMARKER",
    "PROJECTMARKER",
    "INTERESTMARKER",
];

fn theme_source() -> Value {
    serde_json::from_slice(include_bytes!("../../../assets/themes/zhibai.theme.json")).unwrap()
}

fn theme(preset: &str, order: Option<Value>) -> ThemeManifest {
    let mut source = theme_source();
    source["composition"] = json!({"preset":preset});
    if preset.starts_with("sidebar") {
        source["composition"]["placements"] =
            json!({"skills":"sidebar","languages":"sidebar","interests":"sidebar"});
    }
    if let Some(order) = order {
        source["composition"]["moduleOrder"] = order;
    }
    parse_theme(&serde_json::to_vec(&source).unwrap()).unwrap()
}

fn docx_xml(content: &ResumeContent, theme: &ThemeManifest) -> String {
    let bytes = docx::render_with_theme(content, Language::En, theme).unwrap();
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    zip.by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    xml
}

fn assert_order(text: &str, expected: &[&str]) {
    let positions: Vec<_> = expected
        .iter()
        .map(|&marker| {
            assert_eq!(text.matches(marker).count(), 1, "{marker}: {text}");
            text.find(marker).unwrap()
        })
        .collect();
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
}

#[test]
fn module_order_is_optional_partial_or_empty_and_does_not_rewrite_the_theme() {
    let source = theme_source();
    let manifest = parse_theme(&serde_json::to_vec(&source).unwrap()).unwrap();
    assert_eq!(manifest.normalized, source);
    assert!(ThemeStyle::from_manifest(&manifest).module_order.is_empty());
    assert!(manifest.normalized.get("composition").is_none());

    for order in [
        json!([]),
        json!(["education", "work"]),
        json!([
            "projects",
            "references",
            "interests",
            "languages",
            "skills",
            "publications",
            "certificates",
            "awards",
            "education",
            "volunteer",
            "work"
        ]),
    ] {
        let mut source = theme_source();
        source["composition"] = json!({"preset":"single-column","moduleOrder":order});
        let bytes = serde_json::to_vec(&source).unwrap();
        let original = bytes.clone();
        let manifest = parse_theme(&bytes).unwrap();
        let style = ThemeStyle::from_manifest(&manifest);
        assert_eq!(json!(style.module_order), order);
        assert_eq!(manifest.normalized, source);
        assert_eq!(bytes, original);
    }
}

#[test]
fn module_order_rejects_unknown_duplicate_basics_or_missing_names_and_wrong_types() {
    for order in [
        Value::Null,
        json!("work"),
        json!({"work":0}),
        json!(["basics"]),
        json!(["unknown"]),
        json!(["Work"]),
        json!([" work"]),
        json!(["work", "work"]),
        json!([""]),
        json!(["work", null]),
        json!([{}]),
        json!([1]),
    ] {
        let mut source = theme_source();
        source["composition"] = json!({"preset":"single-column","moduleOrder":order});
        let errors = parse_theme(&serde_json::to_vec(&source).unwrap()).unwrap_err();
        assert!(
            errors.iter().any(|error| {
                error.code == "THEME_SCHEMA" && error.path.starts_with("/composition/moduleOrder")
            }),
            "{order}: {errors:?}"
        );
    }
}

#[test]
fn preview_pdf_layout_and_docx_share_order_with_stable_region_placement_and_no_data_loss() {
    let document = parse_resume(RESUME).unwrap();
    let original = document.value.clone();
    let content = project(&document);
    let original_content = content.clone();
    let plain = render::plain_text(&content, Language::En);
    let markdown = render::markdown(&content, Language::En);
    assert_order(&plain, &INPUT_ORDER);
    assert_order(&markdown, &INPUT_ORDER);

    for preset in [
        "single-column",
        "split-header",
        "sidebar-left",
        "sidebar-right",
    ] {
        let mut baseline = None;
        for order in [
            None,
            Some(json!([])),
            // Empty awards and absent volunteer produce no sections. Unlisted
            // projects and interests still follow in their original order.
            Some(json!([
                "work",
                "awards",
                "volunteer",
                "education",
                "skills",
                "languages"
            ])),
        ] {
            let reordered = order.as_ref().is_some_and(|order| order != &json!([]));
            let theme = theme(preset, order);
            let original_theme = theme.normalized.clone();
            layout::check_theme(&theme).unwrap();
            let layout = layout::layout_with_theme(&content, Language::En, &theme).unwrap();
            let text = layout
                .pages
                .iter()
                .flatten()
                .filter_map(|element| match element {
                    LayoutElement::Text(text) => Some(text.text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            let xml = docx_xml(&content, &theme);
            let expected = if reordered {
                &THEME_ORDER
            } else {
                &INPUT_ORDER
            };
            for output in [&text, &xml] {
                assert_order(output, &["NAMEMARKER", "SUMMARYMARKER"]);
                assert_order(output, &["WORKMARKER", "SECONDMARKER"]);
                assert!(output.find("SUMMARYMARKER").unwrap() < output.find(expected[0]).unwrap());
                if preset.starts_with("sidebar") {
                    for sidebar in [false, true] {
                        let lane: Vec<_> = expected
                            .iter()
                            .copied()
                            .filter(|marker| {
                                ["SKILLMARKER", "LANGMARKER", "INTERESTMARKER"].contains(marker)
                                    == sidebar
                            })
                            .collect();
                        assert_order(output, &lane);
                    }
                } else {
                    assert_order(output, expected);
                }
            }
            if !reordered {
                if let Some((previous_layout, previous_xml)) = &baseline {
                    assert_eq!(&layout, previous_layout);
                    assert_eq!(&xml, previous_xml);
                } else {
                    baseline = Some((layout, xml));
                }
            }
            assert_eq!(theme.normalized, original_theme);
            assert_eq!(content, original_content);
            assert_eq!(document.value, original);
            assert_eq!(document.source, RESUME);
            assert_eq!(render::plain_text(&content, Language::En), plain);
            assert_eq!(render::markdown(&content, Language::En), markdown);
        }
    }
}

#[test]
fn pdf_bytes_contain_the_theme_order_and_all_unlisted_content() {
    let content = project(&parse_resume(RESUME).unwrap());
    let theme = theme(
        "single-column",
        Some(json!(["work", "education", "skills", "languages"])),
    );
    let bytes = pdf::render_with_theme(&content, Language::En, &theme).unwrap();
    let pdf = printpdf::PdfDocument::parse(&bytes, &Default::default(), &mut Vec::new()).unwrap();
    let text = pdf
        .extract_text()
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("\n");
    assert_order(&text, &THEME_ORDER);
    assert_order(&text, &["WORKMARKER", "SECONDMARKER"]);
}
