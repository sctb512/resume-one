//! Explicit native-engine acceptance (not GPUI's NoopTextSystem).
//! Run on macOS with `cargo run --release -p resume-one-app --example native-theme-fonts`.
//! Does not open windows, install fonts, or modify application preferences.
#[cfg(target_os = "macos")]
fn main() {
    use std::borrow::Cow;
    let app = gpui::Application::headless();
    let system = app.text_system();
    system
        .add_fonts(vec![Cow::Borrowed(include_bytes!(
            "../../resume-one-core/tests/fixtures/theme-font.ttf"
        ))])
        .expect("native font registration");
    let wanted = gpui::font("Resume One Font Fixture");
    let id = system.resolve_font(&wanted);
    assert_eq!(
        system.get_font_for_id(id).expect("native family").family,
        wanted.family,
        "native engine must not silently fall back"
    );
    let advance = system
        .advance(id, gpui::px(12.0), 'm')
        .expect("native glyph metrics");
    assert!(advance.width > gpui::px(0.0));
    // Generated packages may rename files. Read the same validated manifests
    // as the application instead of hard-coding a previous package's paths.
    let mut packages = Vec::new();
    let root = std::env::var_os("RESUME_ONE_COMMUNITY_THEMES_DIR").map(std::path::PathBuf::from);
    if let Some(root) = &root {
        packages.extend(
            std::fs::read_dir(root)
                .expect("community theme data directory")
                .map(|p| p.unwrap().path())
                .filter(|p| p.is_dir()),
        );
        assert!(!packages.is_empty());
    }
    packages.sort();
    let mut faces = std::collections::BTreeMap::new();
    for package in packages {
        let relative = std::path::Path::new(package.file_name().unwrap()).join("theme.json");
        let theme = resume_one_core::load_theme(root.as_deref().unwrap(), &relative)
            .expect("validated local package");
        for id in theme.font_asset_ids() {
            let face = resume_one_core::fonts::get(id);
            faces
                .entry((face.family.clone(), face.weight, face.italic))
                .or_insert(face);
        }
    }
    // Register every real weight before first resolving a family: native
    // systems can cache the family's face set on first access.
    if !faces.is_empty() {
        system
            .add_fonts(
                faces
                    .values()
                    .map(|f| Cow::Owned(f.bytes.as_ref().clone()))
                    .collect(),
            )
            .expect("native migrated font registration");
    }
    for face in faces.values() {
        let mut wanted = gpui::font(face.family.clone());
        wanted.weight = gpui::FontWeight(face.weight as f32);
        wanted.style = if face.italic {
            gpui::FontStyle::Italic
        } else {
            gpui::FontStyle::Normal
        };
        let id = system.resolve_font(&wanted);
        let actual = system.get_font_for_id(id).expect("migrated native font");
        assert_eq!(actual.family, wanted.family);
        assert_eq!(actual.weight, wanted.weight);
        assert_eq!(actual.style, wanted.style);
        assert!(system.advance(id, gpui::px(12.), 'm').unwrap().width > gpui::px(0.));
    }
    println!(
        "PASS: {} distinct community faces resolved through CoreText",
        faces.len()
    );
    system
        .add_fonts(vec![Cow::Borrowed(include_bytes!(
            "../../resume-one-core/tests/fixtures/theme-italic-font.ttf"
        ))])
        .expect("real italic native registration");
    let mut italic = gpui::font("Resume One Italic Font Fixture");
    italic.style = gpui::FontStyle::Italic;
    let id = system.resolve_font(&italic);
    let actual = system.get_font_for_id(id).unwrap();
    assert_eq!(actual.family, italic.family);
    assert_eq!(actual.style, gpui::FontStyle::Italic);
    assert!(system.advance(id, gpui::px(12.), 'm').unwrap().width > gpui::px(0.));
    println!(
        "PASS: CoreText resolved and measured the theme-local font; no window or font installation"
    );
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!(
        "This native acceptance example targets macOS; Windows needs its own native acceptance run."
    );
    std::process::exit(2);
}
