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
