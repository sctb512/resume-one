//! Native CoreText check for shared tracked grapheme offsets. No window,
//! system-font installation, settings changes or network access.
#[cfg(target_os = "macos")]
fn main() {
    use resume_one_core::{
        layout::{self, FontKind, LayoutElement},
        render::Language,
        resume::parse_resume,
        semantic::project,
    };
    use std::{borrow::Cow, collections::BTreeSet, path::Path};
    let path = std::env::args()
        .nth(1)
        .expect("absolute theme manifest path");
    let path = Path::new(&path);
    assert!(path.is_absolute());
    let root = if path.file_name().unwrap() == "theme.json" {
        path.parent().unwrap().parent().unwrap()
    } else {
        path.parent().unwrap()
    };
    let theme = resume_one_core::load_theme(root, path.strip_prefix(root).unwrap()).unwrap();
    let content =
        project(&parse_resume(include_bytes!("../../../assets/example.resume.json")).unwrap());
    let layout = layout::layout_with_theme(&content, Language::En, &theme).unwrap();
    let app = gpui::Application::headless();
    let system = app.text_system();
    system
        .add_fonts(vec![
            Cow::Borrowed(resume_one_core::fonts::sans_font_bytes()),
            Cow::Borrowed(resume_one_core::fonts::serif_font_bytes()),
        ])
        .unwrap();
    let mut loaded: BTreeSet<_> = theme.font_asset_ids().collect();
    for element in layout.pages.iter().flatten() {
        if let LayoutElement::Text(text) = element
            && let FontKind::Local(id) = text.font
        {
            loaded.insert(id);
        }
    }
    system
        .add_fonts(
            loaded
                .iter()
                .copied()
                .map(resume_one_core::fonts::bytes)
                .collect(),
        )
        .unwrap();
    let mut checked = 0;
    for element in layout.pages.iter().flatten() {
        let LayoutElement::Text(text) = element else {
            continue;
        };
        if text.tracking_pt == 0.0 {
            continue;
        };
        let mut font = gpui::font(text.font.family());
        font.weight = gpui::FontWeight(match text.font {
            FontKind::Sans => 400.,
            FontKind::Serif => 600.,
            FontKind::Local(id) => resume_one_core::fonts::get(id).weight as f32,
        });
        font.style = if text.font.is_italic() {
            gpui::FontStyle::Italic
        } else {
            gpui::FontStyle::Normal
        };
        let font_id = system.resolve_font(&font);
        let resolved = system.get_font_for_id(font_id).unwrap();
        assert_eq!(resolved.family, font.family);
        assert_eq!(resolved.weight, font.weight);
        assert_eq!(resolved.style, font.style);
        for (cluster, _) in text.paint_runs() {
            let expected = text.font.text_width_pt(cluster, text.size_pt);
            let measured: f32 = cluster
                .chars()
                .map(|c| {
                    f32::from(
                        system
                            .advance(font_id, gpui::px(text.size_pt), c)
                            .unwrap()
                            .width,
                    )
                })
                .sum();
            assert!(
                (measured - expected).abs() < 0.06,
                "native cluster advance differs: {measured} vs {expected}"
            );
            checked += 1;
        }
    }
    assert!(checked > 0, "theme must exercise tracking");
    println!(
        "PASS: {checked} native tracked clusters match shared font advances; no window or font installation"
    );
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!(
        "This CoreText acceptance check requires macOS; Windows needs independent native verification."
    );
    std::process::exit(2);
}
