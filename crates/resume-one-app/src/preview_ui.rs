use std::{borrow::Cow, cell::RefCell, collections::VecDeque, sync::Arc};

use gpui::{
    App, Bounds, Context, Font, FontWeight, IntoElement, Pixels, Point, Rgba, ShapedLine, TextRun,
    canvas, div, fill, point, prelude::*, px, size,
};
use resume_one_core::{
    layout::{self, FontKind, LayoutElement, LayoutTarget, PT_PER_MM, ResumeLayout},
    render::Language,
    structure::StructureEdit,
};

use crate::{
    ResumeOne, SECTION_LABELS, color, inline_ui,
    model::{AppModel, BuiltinTheme},
};

const PX_PER_MM: f32 = 3.0;
const PX_PER_PT: f32 = PX_PER_MM / PT_PER_MM;

pub(super) struct PreviewState {
    cache: RefCell<VecDeque<Cache>>,
    font_error: Option<String>,
    text_system: Arc<gpui::TextSystem>,
    registered_fonts: RefCell<std::collections::BTreeSet<u16>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Modifiers, TestAppContext, point, px};
    use resume_one_core::{resume::parse_resume, semantic::project};

    #[gpui::test]
    fn sidebar_hits_and_insert_controls_are_lane_local_while_inspector_stays_fixed(
        cx: &mut TestAppContext,
    ) {
        cx.update(gpui_component::init);
        let mut model = AppModel::load(None).unwrap();
        let manifest = resume_one_core::parse_theme(include_bytes!(
            "../../resume-one-cli/tests/fixtures/sidebar.theme.json"
        ))
        .unwrap();
        model.document = parse_resume(br#"{"basics":{"name":"Ada"},"work":[{"name":"MainCompany"}],"skills":[{"name":"SideSkill"}]}"#).unwrap();
        model.content = project(&model.document);
        model.replace_themes(vec![BuiltinTheme {
            original: None,
            style: resume_one_core::theme_style::ThemeStyle::from_manifest(&manifest),
            manifest,
            path: None,
            identity: "fixture:sidebar".into(),
        }]);
        let (view, cx) = cx.add_window_view(|_, cx| ResumeOne::new(model, cx));
        cx.simulate_resize(gpui::size(px(1440.0), px(1000.0)));
        cx.update(|window, _| window.refresh());
        let panel = cx.debug_bounds("module-panel").unwrap();
        let side = cx.debug_bounds("preview-hit-skills-Some(0)-0").unwrap();
        let main = cx.debug_bounds("preview-hit-work-Some(0)-0").unwrap();
        assert!(side.right() < main.left());
        assert!(side.size.width < main.size.width);
        let add_side = cx.debug_bounds("add-module-before-skills").unwrap();
        assert_eq!(add_side.left(), side.left());
        assert_eq!(add_side.size.width, side.size.width);
        cx.simulate_mouse_move(side.center(), None, Modifiers::none());
        cx.update(|_, cx| assert_eq!(view.read(cx).hovered_item, Some(("skills".into(), 0, 0))));
        cx.simulate_mouse_move(main.center(), None, Modifiers::none());
        cx.update(|_, cx| assert_eq!(view.read(cx).hovered_item, Some(("work".into(), 0, 0))));
        assert_eq!(cx.debug_bounds("module-panel").unwrap(), panel);
    }

    #[gpui::test]
    fn theme_font_asset_is_forwarded_to_preview_without_substitution(cx: &mut TestAppContext) {
        let root = tempfile::tempdir().unwrap();
        let data = include_bytes!("../../resume-one-core/tests/fixtures/theme-font.ttf");
        std::fs::write(root.path().join("fixture.ttf"), data).unwrap();
        let mut value: serde_json::Value =
            serde_json::from_slice(include_bytes!("../../../assets/themes/zhibai.theme.json"))
                .unwrap();
        value["assets"] = serde_json::json!({"fonts":[{"id":"fixture", "path":"fixture.ttf", "sha256":"0a8f72c42d7bff2bb6d67b78e603d46e2a285de93bbeb164d10fb2299f25945f", "license":"OFL-1.1"}]});
        value["typography"]["body"]["family"] = serde_json::json!("asset:fixture");
        value["typography"]["body"]["weight"] = serde_json::json!(400);
        std::fs::write(root.path().join("fixture.theme.json"), value.to_string()).unwrap();
        let manifest =
            resume_one_core::load_theme(root.path(), std::path::Path::new("fixture.theme.json"))
                .unwrap();
        let mut model = AppModel::load(None).unwrap();
        model.document = parse_resume(br#"{"basics":{"name":"Ada"}}"#).unwrap();
        model.content = project(&model.document);
        model.replace_themes(vec![BuiltinTheme {
            original: None,
            style: resume_one_core::theme_style::ThemeStyle::from_manifest(&manifest),
            manifest,
            path: Some(root.path().join("fixture.theme.json")),
            identity: "fixture:font-asset".into(),
        }]);
        let (view, cx) = cx.add_window_view(|_, cx| ResumeOne::new(model, cx));
        cx.update(|_, cx| {
            let app = view.read(cx);
            let preview = app
                .preview
                .resolve(&app.model, app.model.applied_theme())
                .unwrap();
            // Body has not appeared yet: preload the declared asset before
            // native family selection, not only the current layout's faces.
            assert!(!preview.layout.pages.iter().flatten().any(
                |op| matches!(op, LayoutElement::Text(t) if matches!(t.font, FontKind::Local(_)))
            ));
            // TestAppContext uses NoopTextSystem: every requested family maps
            // to the same FontId. Check our hand-off here, not native loading.
            // examples/native-theme-fonts.rs separately exercises CoreText.
            let fonts = app.preview.registered_fonts.borrow();
            assert_eq!(fonts.len(), 1);
            let font = resume_one_core::fonts::get(*fonts.first().unwrap());
            assert_eq!(font.family, "Resume One Font Fixture");
            assert_eq!(&*font.bytes, data);
        });
        cx.update(|_, cx| {
            view.update(cx, |app, _| {
                app.model.document = parse_resume(br#"{"basics":{"name":"Ada", "summary":"Ada m"}}"#).unwrap();
                app.model.content = project(&app.model.document);
                let preview = app.preview.resolve(&app.model, app.model.applied_theme()).unwrap();
                assert!(preview.layout.pages.iter().flatten().any(
                    |op| matches!(op, LayoutElement::Text(t) if matches!(t.font, FontKind::Local(_)))
                ));
                assert_eq!(app.preview.registered_fonts.borrow().len(),1);
            });
        });
    }

    #[gpui::test]
    fn hover_appearance_updates_layout_and_restores_without_affecting_export_preferences(
        cx: &mut TestAppContext,
    ) {
        let model = AppModel::load(None).unwrap();
        let (view, cx) = cx.add_window_view(|_, cx| ResumeOne::new(model, cx));
        cx.update(|_, cx| {
            view.update(cx, |app, _| {
                let before = app
                    .preview
                    .resolve(&app.model, app.model.applied_theme())
                    .unwrap();
                let committed = app.model.selected_style();
                let mut trial = app.model.appearance.clone();
                trial.set_color("paper", "#172426").unwrap();
                trial.body_font = Some(resume_one_core::appearance::FontChoice::Serif);
                app.model.preview_appearance(Some(trial));
                let after = app
                    .preview
                    .resolve(&app.model, app.model.applied_theme())
                    .unwrap();
                assert_eq!(after.layout.paper.hex(), "#172426");
                assert_ne!(after.layout, before.layout);
                assert_eq!(
                    app.model.selected_style().colors.paper,
                    committed.colors.paper
                );
                assert_eq!(
                    app.model.selected_style().typography.body.family,
                    committed.typography.body.family
                );
                assert_eq!(app.model.appearance, Default::default());
                let epoch = app.model.theme_epoch;
                app.model
                    .preview_appearance(Some(app.model.display_appearance().clone()));
                assert_eq!(
                    app.model.theme_epoch, epoch,
                    "unchanged hover must retain the cache"
                );
                assert!(Arc::ptr_eq(
                    &after,
                    &app.preview
                        .resolve(&app.model, app.model.applied_theme())
                        .unwrap()
                ));
                app.model.preview_appearance(None);
                let restored = app
                    .preview
                    .resolve(&app.model, app.model.applied_theme())
                    .unwrap();
                assert_eq!(restored.layout, before.layout);
            })
        });
    }

    #[gpui::test]
    fn independent_appearance_invalidates_preview_and_thumbnail_cache_without_mutating_theme(
        cx: &mut TestAppContext,
    ) {
        let model = AppModel::load(None).unwrap();
        let (view, cx) = cx.add_window_view(|_, cx| ResumeOne::new(model, cx));
        cx.update(|_, cx| {
            view.update(cx, |app, cx| {
                let original = app.model.themes[0].manifest.normalized.clone();
                let source = app.model.document.source.clone();
                let before = app
                    .preview
                    .resolve(&app.model, &app.model.themes[0])
                    .unwrap();
                let cached = app
                    .preview
                    .resolve(&app.model, &app.model.themes[0])
                    .unwrap();
                assert!(Arc::ptr_eq(&before, &cached));
                let mut appearance = resume_one_core::appearance::Appearance::default();
                appearance.set_color("paper", "#172426").unwrap();
                appearance.body_font = Some(resume_one_core::appearance::FontChoice::Serif);
                app.model.set_appearance(appearance);
                let after = app
                    .preview
                    .resolve(&app.model, &app.model.themes[0])
                    .unwrap();
                assert!(!Arc::ptr_eq(&before, &after));
                assert_eq!(after.layout.paper.hex(), "#172426");
                assert_eq!(app.model.themes[0].manifest.normalized, original);
                assert_eq!(app.model.document.source, source);
                let effective = app.model.appearance.apply(&app.model.themes[0].manifest);
                assert_eq!(
                    after.layout,
                    layout::layout_with_theme(&app.model.content, Language::Zh, &effective)
                        .unwrap()
                );
                app.model.select_theme(1);
                assert_eq!(app.active_style().colors.paper.hex(), "#172426");
                app.model.set_appearance(Default::default());
                let reset = app
                    .preview
                    .resolve(&app.model, &app.model.themes[0])
                    .unwrap();
                assert_eq!(reset.layout, before.layout);
                cx.notify();
            })
        });
    }

    #[gpui::test]
    fn screen_fit_preserves_layout_and_hit_coordinates_and_thumbnail_cache(
        cx: &mut TestAppContext,
    ) {
        let model = AppModel::load(None).unwrap();
        let (view, cx) = cx.add_window_view(|_, cx| ResumeOne::new(model, cx));
        let original = cx.update(|_, cx| {
            let app = view.read(cx);
            app.preview
                .resolve(&app.model, app.model.applied_theme())
                .unwrap()
        });
        for width in [800., 900., 1120., 1440.] {
            cx.simulate_resize(gpui::size(px(width), px(700.)));
            cx.update(|window, _| window.refresh());
            let paper = cx.debug_bounds("resume-paper").unwrap();
            let preview = cx.debug_bounds("resume-preview").unwrap();
            let hit = cx.debug_bounds("preview-basics").unwrap();
            cx.update(|_, cx| {
                let app = view.read(cx);
                let cached = app
                    .preview
                    .resolve(&app.model, app.model.applied_theme())
                    .unwrap();
                assert!(
                    Arc::ptr_eq(&original, &cached),
                    "screen resizing must not reflow the resume"
                );
                let scale = PX_PER_PT * app.preview_scale;
                let region = original
                    .regions
                    .iter()
                    .find(|r| r.target.section == "basics")
                    .unwrap();
                assert!(
                    (f32::from(hit.top() - paper.top()) - region.top * scale).abs() <= 1.0,
                    "hit={hit:?}, paper={paper:?}, expected_y={}",
                    region.top * scale
                );
                assert!(app.preview.cache.borrow().len() <= 40);
            });
            assert!(paper.left() >= preview.left());
            assert!(paper.right() <= preview.right());
        }
    }

    #[gpui::test]
    fn large_local_theme_library_only_measures_nearby_thumbnails(cx: &mut TestAppContext) {
        let mut model = AppModel::load(None).unwrap();
        let base = model.themes[0].clone();
        model.replace_themes(
            (0..100)
                .map(|index| {
                    let mut theme = base.clone();
                    theme.identity = format!("fixture:theme-{index}");
                    theme.manifest.id = format!("theme-{index}");
                    theme
                })
                .collect(),
        );
        let (view, cx) = cx.add_window_view(|_, cx| ResumeOne::new(model, cx));
        cx.simulate_resize(gpui::size(px(1280.), px(820.)));
        cx.update(|window, _| window.refresh());
        cx.update(|_, cx| {
            let app = view.read(cx);
            assert!(app.preview.cache.borrow().len() < 20);
            assert!(
                !app.preview
                    .cache
                    .borrow()
                    .iter()
                    .any(|entry| entry.theme_identity == "fixture:theme-60")
            );
        });
        let list = cx.debug_bounds("theme-panel").unwrap();
        cx.simulate_event(gpui::ScrollWheelEvent {
            position: list.center(),
            delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(-100_000.))),
            ..Default::default()
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            let app = view.read(cx);
            assert!(app.theme_scroll.offset().y < px(-5000.));
            assert!(app.preview.cache.borrow().len() < 30);
            assert!(
                app.preview
                    .cache
                    .borrow()
                    .iter()
                    .any(|entry| entry.theme_identity == "fixture:theme-99")
            );
        });
        let entries = cx.update(|_, cx| view.read(cx).preview.cache.borrow().len());
        for _ in 0..3 {
            cx.update(|window, _| window.refresh());
        }
        assert_eq!(
            entries,
            cx.update(|_, cx| view.read(cx).preview.cache.borrow().len())
        );
    }

    #[gpui::test]
    fn remove_control_only_exists_for_hovered_region_and_survives_pointer_handoff(
        cx: &mut TestAppContext,
    ) {
        let model = AppModel::load(None).unwrap();
        let (view, cx) = cx.add_window_view(|_, cx| ResumeOne::new(model, cx));
        cx.update(|window, _| window.refresh());
        let item = cx.debug_bounds("preview-hit-work-Some(0)-0").unwrap();
        assert!(cx.debug_bounds("remove-preview-work-Some(0)-0").is_none());

        cx.simulate_mouse_move(item.center(), None, Modifiers::none());
        cx.run_until_parked();
        cx.update(|_, cx| {
            assert_eq!(
                view.read(cx).hovered_item,
                Some(("work".into(), 0, 0)),
                "item={item:?}"
            );
        });
        let remove = cx.debug_bounds("remove-preview-work-Some(0)-0").unwrap();
        cx.simulate_mouse_move(remove.center(), None, Modifiers::none());
        cx.run_until_parked();
        assert!(cx.debug_bounds("remove-preview-work-Some(0)-0").is_some());
        cx.update(|_, cx| {
            assert_eq!(view.read(cx).hovered_item, Some(("work".into(), 0, 0)));
        });

        cx.simulate_mouse_move(point(px(20.), px(100.)), None, Modifiers::none());
        cx.run_until_parked();
        cx.update(|_, cx| assert!(view.read(cx).hovered_item.is_none()));
    }

    #[gpui::test]
    fn hover_controls_fade_without_hidden_clicks_or_geometry_changes(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let executor = cx.executor();
        let (view, cx) =
            cx.add_window_view(|_, cx| ResumeOne::new(AppModel::load(None).unwrap(), cx));
        cx.update(|window, _| window.refresh());
        let paper = cx.debug_bounds("resume-paper").unwrap();
        let right = cx.debug_bounds("module-panel").unwrap();
        let add = cx.debug_bounds("add-module-before-work").unwrap();
        cx.simulate_click(add.center(), Modifiers::none());
        cx.update(|_, cx| {
            assert!(
                !view.read(cx).insertion_active(),
                "a fully hidden control cannot act on a synthetic click without hover"
            )
        });
        cx.simulate_mouse_move(add.center(), None, Modifiers::none());
        executor.advance_clock(std::time::Duration::from_millis(25));
        cx.update(|window, _| window.refresh());
        let alpha = cx.update(|_, cx| view.read(cx).control_alpha("add-module-before-work", cx));
        assert!(alpha > 0. && alpha < 1.);
        cx.simulate_mouse_move(point(px(20.), px(100.)), None, Modifiers::none());
        cx.update(|_, cx| {
            assert_eq!(
                view.read(cx).control_alpha("add-module-before-work", cx),
                alpha
            )
        });
        executor.advance_clock(std::time::Duration::from_millis(100));
        cx.update(|window, _| window.refresh());
        cx.update(|_, cx| {
            assert_eq!(
                view.read(cx).control_alpha("add-module-before-work", cx),
                0.
            )
        });
        assert_eq!(paper, cx.debug_bounds("resume-paper").unwrap());
        assert_eq!(right, cx.debug_bounds("module-panel").unwrap());
        cx.simulate_mouse_move(add.center(), None, Modifiers::none());
        cx.simulate_click(add.center(), Modifiers::none());
        cx.update(|_, cx| {
            assert!(
                view.read(cx).insertion_active(),
                "a quick intentional click must not wait for the fade"
            )
        });
    }

    #[gpui::test]
    fn a_small_move_after_scrolling_rearms_the_same_module_blank_without_leaving_it(
        cx: &mut TestAppContext,
    ) {
        cx.update(gpui_component::init);
        let executor = cx.executor();
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("scroll-hover.json");
        std::fs::write(
            &path,
            br#"{"work":[{"name":"A sufficiently wide target"}]}"#,
        )
        .unwrap();
        let (view, cx) =
            cx.add_window_view(|_, cx| ResumeOne::new(AppModel::load(Some(&path)).unwrap(), cx));
        cx.update(|window, _| window.refresh());
        let ink = cx.debug_bounds("preview-ink-work-Some(0)-0-0").unwrap();
        let row = cx.debug_bounds("preview-hit-work-Some(0)-0").unwrap();
        let blank = point(row.right() - px(40.), ink.center().y);
        assert!(!ink.contains(&blank));
        cx.simulate_mouse_move(blank, None, Modifiers::none());
        executor.advance_clock(std::time::Duration::from_millis(50));
        cx.update(|_, cx| view.update(cx, |app, cx| app.preview_scrolled(cx)));
        executor.advance_clock(std::time::Duration::from_millis(120));
        cx.run_until_parked();
        cx.update(|_, cx| assert!(view.read(cx).inline.form.is_none()));
        cx.simulate_mouse_move(blank + point(px(1.), px(0.)), None, Modifiers::none());
        executor.advance_clock(std::time::Duration::from_millis(140));
        cx.run_until_parked();
        cx.update(|_, cx| assert!(view.read(cx).inline_matches("work", Some(0))));
    }

    #[gpui::test]
    fn rich_item_padding_remains_an_editable_module_hover_target(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let executor = cx.executor();
        for kind in ["cards", "timeline"] {
            let mut model = AppModel::load(None).unwrap();
            model.document =
                parse_resume(br#"{"basics":{"name":"Example"},"work":[{"name":"Short"}]}"#)
                    .unwrap();
            model.content = project(&model.document);
            let mut value = model.themes[0].manifest.normalized.clone();
            value["sections"] = serde_json::json!({"work":{"items":kind}});
            let manifest = resume_one_core::parse_theme(value.to_string().as_bytes()).unwrap();
            model.replace_themes(vec![BuiltinTheme {
                original: None,
                style: resume_one_core::theme_style::ThemeStyle::from_manifest(&manifest),
                manifest,
                path: None,
                identity: format!("fixture:{kind}"),
            }]);
            let (view, cx) = cx.add_window_view(|_, cx| ResumeOne::new(model, cx));
            cx.update(|window, _| window.refresh());
            let block = cx.debug_bounds("preview-hit-work-Some(0)-0").unwrap();
            let ink = cx.debug_bounds("preview-ink-work-Some(0)-0-0").unwrap();
            let blank = point(block.left() + px(2.), block.center().y);
            assert!(!ink.contains(&blank));
            cx.simulate_mouse_move(blank, None, Modifiers::none());
            executor.advance_clock(std::time::Duration::from_millis(140));
            cx.run_until_parked();
            cx.update(|_, cx| assert!(view.read(cx).inline_matches("work", Some(0)), "{kind}"));
        }
    }

    #[gpui::test]
    fn module_blank_space_opens_after_one_dwell_without_modifying_content(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let executor = cx.executor();
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("module-hover.json");
        let source = br#"{"basics":{"name":"A"},"work":[{"name":"Short"}]}"#;
        std::fs::write(&path, source).unwrap();
        let (view, cx) =
            cx.add_window_view(|_, cx| ResumeOne::new(AppModel::load(Some(&path)).unwrap(), cx));
        cx.update(|window, _| window.refresh());
        let row = cx.debug_bounds("preview-hit-work-Some(0)-0").unwrap();
        let ink = cx.debug_bounds("preview-ink-work-Some(0)-0-0").unwrap();
        let blank = point(row.right() - px(40.), row.center().y);
        assert!(!ink.contains(&blank));
        let paper = cx.debug_bounds("resume-paper").unwrap();
        let panel = cx.debug_bounds("module-panel").unwrap();
        cx.simulate_mouse_move(blank, None, Modifiers::none());
        executor.advance_clock(std::time::Duration::from_millis(139));
        cx.run_until_parked();
        cx.update(|_, cx| assert!(view.read(cx).inline.form.is_none()));
        executor.advance_clock(std::time::Duration::from_millis(1));
        cx.run_until_parked();
        cx.update(|_, cx| assert!(view.read(cx).inline_matches("work", Some(0))));
        assert_eq!(paper, cx.debug_bounds("resume-paper").unwrap());
        assert_eq!(panel, cx.debug_bounds("module-panel").unwrap());
        executor.advance_clock(std::time::Duration::from_millis(160));
        cx.update(|window, _| window.refresh());
        let form = cx.debug_bounds("inline-form").unwrap();
        cx.simulate_mouse_move(form.center(), None, Modifiers::none());
        let outside = point(paper.left() + px(3.), form.top() + px(20.));
        assert!(!form.contains(&outside));
        cx.simulate_mouse_move(outside, None, Modifiers::none());
        executor.advance_clock(std::time::Duration::from_millis(60));
        cx.update(|window, _| window.refresh());
        cx.update(|_, cx| {
            assert!(
                view.read(cx).inline.form.is_some(),
                "the form fades instead of abruptly unmounting"
            )
        });
        executor.advance_clock(std::time::Duration::from_millis(60));
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
        cx.update(|_, cx| assert!(view.read(cx).inline.form.is_none()));
        cx.update(|_, cx| assert_eq!(view.read(cx).model.document.source, source));
        assert_eq!(std::fs::read(path).unwrap(), source);
    }

    #[gpui::test]
    fn moving_between_module_text_and_blank_keeps_the_original_dwell(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let executor = cx.executor();
        let (view, cx) =
            cx.add_window_view(|_, cx| ResumeOne::new(AppModel::load(None).unwrap(), cx));
        cx.update(|window, _| window.refresh());
        let row = cx.debug_bounds("preview-hit-work-Some(0)-0").unwrap();
        let ink = cx.debug_bounds("preview-ink-work-Some(0)-0-0").unwrap();
        let blank = point(row.right() - px(40.), ink.center().y);
        assert!(!ink.contains(&blank));
        cx.simulate_mouse_move(ink.center(), None, Modifiers::none());
        executor.advance_clock(std::time::Duration::from_millis(80));
        cx.simulate_mouse_move(blank, None, Modifiers::none());
        executor.advance_clock(std::time::Duration::from_millis(59));
        cx.run_until_parked();
        cx.update(|_, cx| assert!(view.read(cx).inline.form.is_none()));
        executor.advance_clock(std::time::Duration::from_millis(1));
        cx.run_until_parked();
        cx.update(|_, cx| assert!(view.read(cx).inline_matches("work", Some(0))));
    }

    #[gpui::test]
    fn leaving_module_blank_before_dwell_cancels_and_paper_whitespace_stays_quiet(
        cx: &mut TestAppContext,
    ) {
        cx.update(gpui_component::init);
        let executor = cx.executor();
        let (view, cx) =
            cx.add_window_view(|_, cx| ResumeOne::new(AppModel::load(None).unwrap(), cx));
        cx.update(|window, _| window.refresh());
        let row = cx.debug_bounds("preview-hit-work-Some(0)-0").unwrap();
        let paper = cx.debug_bounds("resume-paper").unwrap();
        let blank = point(row.right() - px(40.), row.top() + px(4.));
        cx.simulate_mouse_move(blank, None, Modifiers::none());
        executor.advance_clock(std::time::Duration::from_millis(80));
        cx.simulate_mouse_move(
            point(paper.left() + px(3.), row.top()),
            None,
            Modifiers::none(),
        );
        executor.advance_clock(std::time::Duration::from_millis(200));
        cx.run_until_parked();
        cx.update(|_, cx| assert!(view.read(cx).inline.form.is_none()));
    }

    #[gpui::test]
    fn basic_info_empty_items_and_both_sidebar_lanes_accept_blank_hover(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let executor = cx.executor();
        for (source, selector, section, index, preset) in [
            (
                r#"{"basics":{"name":"Ada"}}"#,
                "preview-basics",
                "basics",
                None,
                None,
            ),
            (r#"{"basics":{}}"#, "preview-basics", "basics", None, None),
            (
                r#"{"work":[{}]}"#,
                "preview-hit-work-Some(0)-0",
                "work",
                Some(0),
                None,
            ),
            (
                r#"{"work":[{"name":"Main"}],"skills":[{"name":"Side"}]}"#,
                "preview-hit-skills-Some(0)-0",
                "skills",
                Some(0),
                Some("sidebar-left"),
            ),
            (
                r#"{"work":[{"name":"Main"}],"skills":[{"name":"Side"}]}"#,
                "preview-hit-skills-Some(0)-0",
                "skills",
                Some(0),
                Some("sidebar-right"),
            ),
        ] {
            let mut model = AppModel::load(None).unwrap();
            model.document = parse_resume(source.as_bytes()).unwrap();
            model.content = project(&model.document);
            if let Some(preset) = preset {
                let mut manifest = resume_one_core::parse_theme(include_bytes!(
                    "../../resume-one-cli/tests/fixtures/sidebar.theme.json"
                ))
                .unwrap();
                manifest.normalized["composition"]["preset"] = serde_json::json!(preset);
                model.replace_themes(vec![BuiltinTheme {
                    original: None,
                    style: resume_one_core::theme_style::ThemeStyle::from_manifest(&manifest),
                    manifest,
                    path: None,
                    identity: format!("fixture:{preset}"),
                }]);
            }
            let (view, cx) = cx.add_window_view(|_, cx| ResumeOne::new(model, cx));
            cx.simulate_resize(gpui::size(px(1440.), px(1000.)));
            cx.update(|window, _| window.refresh());
            let block = cx.debug_bounds(selector).unwrap();
            let panel = cx.debug_bounds("module-panel").unwrap();
            cx.simulate_mouse_move(
                point(block.right() - px(40.), block.center().y),
                None,
                Modifiers::none(),
            );
            executor.advance_clock(std::time::Duration::from_millis(140));
            cx.run_until_parked();
            cx.update(|_, cx| {
                assert!(
                    view.read(cx).inline_matches(section, index),
                    "{selector} {preset:?}"
                );
                assert_eq!(view.read(cx).model.document.source, source.as_bytes());
            });
            assert_eq!(panel, cx.debug_bounds("module-panel").unwrap());
        }
    }

    #[gpui::test]
    fn hovering_remove_control_does_not_open_a_form_or_block_deletion(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let executor = cx.executor();
        let (view, cx) =
            cx.add_window_view(|_, cx| ResumeOne::new(AppModel::load(None).unwrap(), cx));
        cx.update(|window, _| window.refresh());
        let block = cx.debug_bounds("preview-hit-work-Some(0)-0").unwrap();
        cx.simulate_mouse_move(
            point(block.right() - px(40.), block.top() + px(4.)),
            None,
            Modifiers::none(),
        );
        executor.advance_clock(std::time::Duration::from_millis(65));
        cx.update(|window, _| window.refresh());
        let remove = cx.debug_bounds("remove-preview-work-Some(0)-0").unwrap();
        cx.simulate_mouse_move(remove.center(), None, Modifiers::none());
        executor.advance_clock(std::time::Duration::from_millis(200));
        cx.run_until_parked();
        cx.update(|_, cx| assert!(view.read(cx).inline.form.is_none()));
        let before = cx.update(|_, cx| {
            view.read(cx).model.document.value["work"]
                .as_array()
                .unwrap()
                .len()
        });
        cx.simulate_click(remove.center(), Modifiers::none());
        cx.update(|_, cx| {
            assert_eq!(
                view.read(cx).model.document.value["work"]
                    .as_array()
                    .unwrap()
                    .len(),
                before - 1
            )
        });
    }

    #[test]
    fn hit_regions_preserve_empty_item_indices_and_exclude_page_footers() {
        let document =
            parse_resume(br#"{"basics":{"name":"A"},"work":[{}, {"name":"B"}]}"#).unwrap();
        let result = layout::layout(&project(&document), Language::Zh).unwrap();
        let regions = regions(&result);
        for index in 0..2 {
            assert!(regions.iter().any(|region| region.target
                == LayoutTarget {
                    section: "work",
                    index: Some(index),
                }));
        }
        let empty = regions
            .iter()
            .find(|region| region.target.index == Some(0))
            .unwrap();
        assert!(!empty.text);
        assert!(empty.bottom > empty.top);
        let bottom = result.height_mm * PT_PER_MM;
        assert!(
            regions
                .iter()
                .all(|region| region.top >= 0.0 && region.bottom < bottom - 20.0)
        );
        assert!(result.pages.iter().all(
            |page| matches!(page.last(), Some(LayoutElement::Text(text)) if text.target.is_none())
        ));
    }

    #[test]
    fn basics_hit_regions_do_not_merge_identity_and_contacts_across_lanes() {
        let document = parse_resume(br#"{"basics":{"name":"Identity","summary":"Summary","email":"contact@example.com"},"work":[{"name":"MainCompany"}],"skills":[{"name":"SideSkill"}]}"#).unwrap();
        let mut theme = resume_one_core::parse_theme(include_bytes!(
            "../../resume-one-cli/tests/fixtures/sidebar.theme.json"
        ))
        .unwrap();
        for direction in ["sidebar-left", "sidebar-right"] {
            theme.normalized["composition"]["preset"] = serde_json::json!(direction);
            theme.normalized["composition"]["headerRegion"] = serde_json::json!("main");
            theme.normalized["composition"]["summaryRegion"] = serde_json::json!("main");
            theme.normalized["composition"]["contactRegion"] = serde_json::json!("sidebar");
            let result =
                layout::layout_with_theme(&project(&document), Language::En, &theme).unwrap();
            let regions = regions(&result);
            let basics: Vec<_> = regions
                .iter()
                .filter(|r| r.page == 0 && r.target.section == "basics")
                .collect();
            assert_eq!(basics.len(), 2);
            assert!(basics[0].right < basics[1].left || basics[1].right < basics[0].left);
            for region in basics {
                assert!(region.text);
                assert!(region.ink.iter().all(|ink| ink.left >= region.left - 0.1
                    && ink.left + ink.width <= region.right + 0.1));
            }
        }
    }

    #[test]
    fn continuing_full_width_basics_do_not_inherit_a_narrow_contact_lane() {
        let document = parse_resume(br#"{"basics":{"name":"Identity","email":"contact@example.com"},"skills":[{"name":"Skill"}]}"#).unwrap();
        let mut content = project(&document);
        content.name = "Long multilingual identity 中文姓名 ".repeat(150);
        let mut theme = resume_one_core::parse_theme(include_bytes!(
            "../../resume-one-cli/tests/fixtures/sidebar.theme.json"
        ))
        .unwrap();
        theme.normalized["composition"]["header"] = serde_json::json!("stacked");
        theme.normalized["composition"]["contactRegion"] = serde_json::json!("main");
        let result = layout::layout_with_theme(&content, Language::En, &theme).unwrap();
        assert!(result.pages.len() > 1);
        let hits = regions(&result);
        for (page, elements) in result.pages.iter().enumerate() {
            for text in elements.iter().filter_map(|element| match element {
                LayoutElement::Text(t) if t.target.is_some() => Some(t),
                _ => None,
            }) {
                let lane = text.target_lane.unwrap();
                let hit = hits
                    .iter()
                    .find(|region| {
                        region.page == page
                            && Some(region.target) == text.target
                            && region.left == lane.0
                            && region.right == lane.1
                    })
                    .unwrap();
                assert!(text.x_pt >= hit.left - 0.1);
                assert!(text.x_pt + text.width_pt() <= hit.right + 0.1);
            }
        }
    }

    #[gpui::test]
    fn centered_header_keeps_basics_editable_and_uses_shared_layout(cx: &mut TestAppContext) {
        let mut model = AppModel::load(None).unwrap();
        let manifest = resume_one_core::parse_theme(include_bytes!(
            "../../resume-one-cli/tests/fixtures/centered-header.theme.json"
        ))
        .unwrap();
        model.themes = vec![BuiltinTheme {
            original: None,
            style: resume_one_core::theme_style::ThemeStyle::from_manifest(&manifest),
            manifest,
            identity: "test:centered-header".into(),
            path: None,
        }];
        let preview = cx.update(PreviewState::new);
        let data = preview.resolve(&model, &model.themes[0]).unwrap();
        let expected =
            layout::layout_with_theme(&model.content, Language::Zh, &model.themes[0].manifest)
                .unwrap();
        assert_eq!(data.layout, expected);
        let basics: Vec<_> = data
            .regions
            .iter()
            .filter(|r| r.target.section == "basics")
            .collect();
        assert!(!basics.is_empty());
        assert!(basics.iter().any(|r| r.text && r.bottom > r.top));
        let (view, cx) = cx.add_window_view(|_, cx| ResumeOne::new(model, cx));
        cx.update(|window, _| window.refresh());
        let paper = cx.debug_bounds("resume-paper").unwrap();
        let scale = cx.update(|_, cx| view.read(cx).preview_scale);
        assert_eq!(paper.size.width, px(expected.width_mm * PX_PER_MM * scale));
    }

    #[gpui::test]
    fn split_header_preview_uses_print_geometry_and_keeps_basics_editable(cx: &mut TestAppContext) {
        let mut model = AppModel::load(None).unwrap();
        let manifest = resume_one_core::parse_theme(include_bytes!(
            "../../resume-one-cli/tests/fixtures/split-header.theme.json"
        ))
        .unwrap();
        model.themes = vec![BuiltinTheme {
            original: None,
            style: resume_one_core::theme_style::ThemeStyle::from_manifest(&manifest),
            manifest,
            identity: "test:split-header".into(),
            path: None,
        }];
        let preview = cx.update(PreviewState::new);
        let data = preview.resolve(&model, &model.themes[0]).unwrap();
        let expected =
            layout::layout_with_theme(&model.content, Language::Zh, &model.themes[0].manifest)
                .unwrap();
        assert_eq!(data.layout, expected);
        assert!(
            data.regions
                .iter()
                .any(|region| region.target.section == "basics" && region.text)
        );
    }

    #[gpui::test]
    fn decorated_headings_keep_item_hit_targets_and_shared_page_geometry(cx: &mut TestAppContext) {
        let mut model = AppModel::load(None).unwrap();
        let manifest = resume_one_core::parse_theme(include_bytes!(
            "../../resume-one-cli/tests/fixtures/section-styles.theme.json"
        ))
        .unwrap();
        let theme = BuiltinTheme {
            original: None,
            style: resume_one_core::theme_style::ThemeStyle::from_manifest(&manifest),
            manifest,
            identity: "test:section-styles".into(),
            path: None,
        };
        model.document =
            parse_resume(br#"{"work":[{}, {"name":"Editable"}],"projects":[{"name":"Project"}]}"#)
                .unwrap();
        model.content = project(&model.document);
        model.themes = vec![theme];
        let preview = cx.update(PreviewState::new);
        let data = preview.resolve(&model, &model.themes[0]).unwrap();
        assert!(
            data.layout.pages[0]
                .iter()
                .any(|e| matches!(e, LayoutElement::Fill(_)))
        );
        for index in 0..2 {
            assert!(data.regions.iter().any(|r| r.target
                == LayoutTarget {
                    section: "work",
                    index: Some(index)
                }));
        }
        let (view, cx) = cx.add_window_view(|_, cx| ResumeOne::new(model, cx));
        cx.update(|window, _| window.refresh());
        let paper = cx.debug_bounds("resume-paper").unwrap();
        let scale = cx.update(|_, cx| view.read(cx).preview_scale);
        assert_eq!(
            paper.size.width,
            px(data.layout.width_mm * PX_PER_MM * scale)
        );
        assert!(
            f32::from(paper.size.height - px(data.layout.height_mm * PX_PER_MM * scale)).abs()
                <= 1.
        );
    }

    #[gpui::test]
    fn preview_caches_layout_and_invalidates_document_and_theme_reloads(cx: &mut TestAppContext) {
        let mut model = AppModel::load(None).unwrap();
        let preview = cx.update(PreviewState::new);
        let first = preview.resolve(&model, &model.themes[0]).unwrap();
        assert!(Arc::ptr_eq(
            &first,
            &preview.resolve(&model, &model.themes[0]).unwrap()
        ));
        let hovered = preview.resolve(&model, &model.themes[1]).unwrap();
        assert!(!Arc::ptr_eq(&first, &hovered));
        model.theme_epoch += 1;
        let reloaded = preview.resolve(&model, &model.themes[1]).unwrap();
        assert!(!Arc::ptr_eq(&hovered, &reloaded));
        model.document = parse_resume(br#"{"basics":{"name":"Changed"}}"#).unwrap();
        model.content = project(&model.document);
        let changed = preview.resolve(&model, &model.themes[1]).unwrap();
        assert!(!Arc::ptr_eq(&reloaded, &changed));
        assert!(
            changed.layout.pages.iter().flatten().any(
                |element| matches!(element, LayoutElement::Text(text) if text.text == "Changed")
            )
        );
    }

    #[gpui::test]
    fn paginated_preview_uses_shared_physical_page_dimensions(cx: &mut TestAppContext) {
        let mut model = AppModel::load(None).unwrap();
        model.document = parse_resume(include_bytes!(
            "../../resume-one-cli/tests/fixtures/visual-qa.json"
        ))
        .unwrap();
        model.content = project(&model.document);
        let expected =
            layout::layout_with_theme(&model.content, Language::Zh, &model.themes[0].manifest)
                .unwrap();
        assert_eq!(expected.pages.len(), 2);
        let (view, cx) = cx.add_window_view(|_, cx| ResumeOne::new(model, cx));
        cx.update(|window, _| window.refresh());
        for index in 0..expected.pages.len() {
            let scale = cx.update(|_, cx| view.read(cx).preview_scale);
            let selector = ["resume-paper", "resume-paper-1"][index];
            let bounds = cx.debug_bounds(selector).unwrap();
            assert_eq!(bounds.size.width, px(expected.width_mm * PX_PER_MM * scale));
            assert!(
                f32::from(bounds.size.height - px(expected.height_mm * PX_PER_MM * scale)).abs()
                    <= 1.
            );
            if index > 0 {
                let previous = cx.debug_bounds("resume-paper").unwrap();
                assert!(bounds.top() >= previous.bottom());
            }
        }
    }
}

struct Cache {
    revision: String,
    theme_epoch: u64,
    theme_identity: String,
    result: Result<Arc<PreviewData>, String>,
}

struct PreviewData {
    layout: ResumeLayout,
    regions: Vec<Region>,
}

#[derive(Clone)]
struct Region {
    page: usize,
    target: LayoutTarget,
    left: f32,
    right: f32,
    top: f32,
    bottom: f32,
    text: bool,
    ink: Vec<Ink>,
}

#[derive(Clone)]
struct Ink {
    left: f32,
    top: f32,
    width: f32,
    height: f32,
}

impl PreviewState {
    pub fn new(cx: &mut App) -> Self {
        let font_error = cx
            .text_system()
            .add_fonts(vec![
                Cow::Borrowed(layout::SANS_FONT_BYTES),
                Cow::Borrowed(layout::SERIF_FONT_BYTES),
            ])
            .err()
            .map(|error| format!("无法载入简历字体：{error}"));
        Self {
            cache: RefCell::new(VecDeque::new()),
            font_error,
            text_system: cx.text_system().clone(),
            registered_fonts: RefCell::new(Default::default()),
        }
    }

    fn resolve(&self, model: &AppModel, theme: &BuiltinTheme) -> Result<Arc<PreviewData>, String> {
        if let Some(error) = &self.font_error {
            return Err(error.clone());
        }
        let mut cache = self.cache.borrow_mut();
        cache.retain(|item| {
            item.revision == model.document.revision && item.theme_epoch == model.theme_epoch
        });
        if let Some(cache) = cache
            .iter()
            .find(|item| item.theme_identity == theme.identity)
        {
            return cache.result.clone();
        }
        let effective = model.display_appearance().apply(&theme.manifest);
        let result = layout::layout_with_theme(&model.content, Language::Zh, &effective).and_then(
            |layout| {
                let mut pending: std::collections::BTreeSet<_> =
                    effective.font_asset_ids().collect();
                for element in layout.pages.iter().flatten() {
                    if let LayoutElement::Text(text) = element
                        && let FontKind::Local(id) = text.font
                    {
                        pending.insert(id);
                    }
                }
                pending.retain(|id| !self.registered_fonts.borrow().contains(id));
                if !pending.is_empty() {
                    self.text_system
                        .add_fonts(
                            pending
                                .iter()
                                .copied()
                                .map(resume_one_core::fonts::bytes)
                                .collect(),
                        )
                        .map_err(|e| format!("无法载入所选字体：{e}"))?;
                    self.registered_fonts.borrow_mut().extend(pending);
                }
                let regions = regions(&layout);
                Ok(Arc::new(PreviewData { layout, regions }))
            },
        );
        // Up to 32 online manifests plus the applied/hovered theme must fit
        // together; otherwise the sidebar and main page evict each other every frame.
        if cache.len() >= 40 {
            cache.pop_front();
        }
        cache.push_back(Cache {
            revision: model.document.revision.clone(),
            theme_epoch: model.theme_epoch,
            theme_identity: theme.identity.clone(),
            result: result.clone(),
        });
        result
    }
}

fn regions(layout: &ResumeLayout) -> Vec<Region> {
    let height = layout.height_mm * PT_PER_MM;
    let sans = FontKind::Sans.vertical_metrics();
    let serif = FontKind::Serif.vertical_metrics();
    let mut regions: Vec<Region> = Vec::new();
    for (page, elements) in layout.pages.iter().enumerate() {
        for element in elements {
            let (target, top, bottom, text, lane) = match element {
                LayoutElement::Anchor(anchor) => (
                    anchor.target,
                    height - anchor.y_pt,
                    height - anchor.y_pt + 4.0,
                    false,
                    (anchor.left_pt, anchor.right_pt),
                ),
                LayoutElement::Text(text) => {
                    let Some(target) = text.target else { continue };
                    let (ascent, descent) = match text.font {
                        FontKind::Sans => sans,
                        FontKind::Serif => serif,
                        FontKind::Local(_) => text.font.vertical_metrics(),
                    };
                    (
                        target,
                        height - text.baseline_y_pt - ascent * text.size_pt,
                        height - text.baseline_y_pt + descent * text.size_pt,
                        true,
                        text.target_lane
                            .expect("targeted layout text has an outer interaction lane"),
                    )
                }
                LayoutElement::Rule(_) | LayoutElement::Fill(_) => continue,
            };
            let ink = if let LayoutElement::Text(line) = element {
                Some(Ink {
                    left: line.x_pt,
                    top,
                    width: line.width_pt(),
                    height: bottom - top,
                })
            } else {
                None
            };
            if let Some(region) = regions.iter_mut().find(|region| {
                region.page == page
                    && region.target == target
                    && region.left == lane.0
                    && region.right == lane.1
            }) {
                if text && !region.text {
                    region.top = top;
                    region.bottom = bottom;
                } else if text {
                    region.top = region.top.min(top);
                    region.bottom = region.bottom.max(bottom);
                }
                region.text |= text;
                region.ink.extend(ink);
            } else {
                regions.push(Region {
                    page,
                    target,
                    left: lane.0,
                    right: lane.1,
                    top,
                    bottom,
                    text,
                    ink: ink.into_iter().collect(),
                });
            }
        }
    }
    regions
}

enum Painted {
    Text(Vec<(ShapedLine, Point<Pixels>, Pixels)>),
    Rule(Bounds<Pixels>, Rgba),
}

fn page_canvas(data: Arc<PreviewData>, page: usize, px_per_pt: f32) -> impl IntoElement {
    canvas(
        move |bounds, window, _| {
            let height = data.layout.height_mm * PT_PER_MM;
            data.layout.pages[page]
                .iter()
                .filter_map(|element| match element {
                    LayoutElement::Text(text) => {
                        let font = Font {
                            family: text.font.family().into(),
                            weight: match text.font {
                                FontKind::Sans => FontWeight::NORMAL,
                                FontKind::Serif => FontWeight::SEMIBOLD,
                                FontKind::Local(id) => {
                                    FontWeight(resume_one_core::fonts::get(id).weight as f32)
                                }
                            },
                            style: if text.font.is_italic() {
                                gpui::FontStyle::Italic
                            } else {
                                gpui::FontStyle::Normal
                            },
                            ..gpui::font(text.font.family())
                        };
                        Some(Painted::Text(
                            text.paint_runs()
                                .into_iter()
                                .map(|(value, offset)| {
                                    let run = TextRun {
                                        len: value.len(),
                                        font: font.clone(),
                                        color: color(text.color).into(),
                                        background_color: None,
                                        underline: None,
                                        strikethrough: None,
                                    };
                                    let line = window.text_system().shape_line(
                                        value.to_owned().into(),
                                        px(text.size_pt * px_per_pt),
                                        &[run],
                                        None,
                                    );
                                    let leading = px(text.leading_pt * px_per_pt)
                                        .max(line.ascent + line.descent);
                                    let baseline =
                                        (leading - line.ascent - line.descent) / 2.0 + line.ascent;
                                    let origin = bounds.origin
                                        + point(
                                            px((text.x_pt + offset) * px_per_pt),
                                            px((height - text.baseline_y_pt) * px_per_pt)
                                                - baseline,
                                        );
                                    (line, origin, leading)
                                })
                                .collect(),
                        ))
                    }
                    LayoutElement::Rule(rule) => Some(Painted::Rule(
                        Bounds::new(
                            bounds.origin
                                + point(
                                    px(rule.left_pt * px_per_pt),
                                    px((height - rule.y_pt - rule.thickness_pt / 2.0) * px_per_pt),
                                ),
                            size(
                                px((rule.right_pt - rule.left_pt) * px_per_pt),
                                px(rule.thickness_pt * px_per_pt),
                            ),
                        ),
                        color(rule.color),
                    )),
                    LayoutElement::Fill(fill) => Some(Painted::Rule(
                        Bounds::new(
                            bounds.origin
                                + point(
                                    px(fill.left_pt * px_per_pt),
                                    px((height - fill.bottom_pt - fill.height_pt) * px_per_pt),
                                ),
                            size(
                                px(fill.width_pt * px_per_pt),
                                px(fill.height_pt * px_per_pt),
                            ),
                        ),
                        color(fill.color),
                    )),
                    LayoutElement::Anchor(_) => None,
                })
                .collect::<Vec<_>>()
        },
        |_, elements, window, cx| {
            for element in elements {
                match element {
                    Painted::Text(runs) => {
                        for (line, origin, leading) in runs {
                            let _ = line.paint(origin, leading, window, cx);
                        }
                    }
                    Painted::Rule(bounds, color) => window.paint_quad(fill(bounds, color)),
                }
            }
        },
    )
    .absolute()
    .size_full()
}

impl ResumeOne {
    pub(super) fn theme_thumbnail(&self, theme: Option<&BuiltinTheme>) -> gpui::AnyElement {
        let style = theme
            .map(|t| self.model.effective_style(t))
            .unwrap_or_else(|| self.active_style());
        let mut thumb = div()
            .relative()
            .w(px(48.))
            .h(px(68.))
            .flex_none()
            .overflow_hidden()
            .border_1()
            .border_color(color(style.colors.paper.mix(style.colors.ink, 0.12)))
            .bg(color(style.colors.paper))
            .shadow_sm();
        if let Some(theme) = theme {
            if theme.original.is_some() {
                thumb = thumb
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(crate::chrome::icon("file", style.colors.accent));
            } else if let Ok(data) = self.preview.resolve(&self.model, theme) {
                let scale = 48. / (data.layout.width_mm * PT_PER_MM);
                thumb = thumb.child(page_canvas(data, 0, scale));
            }
        }
        thumb.into_any_element()
    }

    pub(super) fn resume_pages(
        &self,
        theme: &crate::model::BuiltinTheme,
        interactive: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let data = match self.preview.resolve(&self.model, theme) {
            Ok(data) => data,
            Err(message) => {
                return div()
                    .w(px(560.0))
                    .p_8()
                    .text_size(px(13.0))
                    .text_color(color(self.active_style().colors.seal))
                    .child(message)
                    .into_any_element();
            }
        };
        let style = self.model.effective_style(theme);
        let spotlight = self.insertion_active();
        let px_per_mm = PX_PER_MM * self.preview_scale;
        let px_per_pt = PX_PER_PT * self.preview_scale;
        let valid = self
            .editor
            .as_ref()
            .is_none_or(|editor| editor.buffer.issues().is_empty());
        let left = style.page.margins.left * px_per_mm;
        let content_width =
            (style.page.width_mm - style.page.margins.left - style.page.margins.right) * px_per_mm;
        let mut pages = div().flex().flex_col().gap_6();
        for page_index in 0..data.layout.pages.len() {
            let selector = if page_index == 0 {
                "resume-paper".into()
            } else {
                format!("resume-paper-{page_index}")
            };
            let mut page = div()
                .id(gpui::SharedString::from(selector.clone()))
                .debug_selector(move || selector.clone())
                .relative()
                .w(px(data.layout.width_mm * px_per_mm))
                .h(px(data.layout.height_mm * px_per_mm))
                .flex_none()
                .bg(color(data.layout.paper))
                .shadow_md()
                .when(interactive, |page| {
                    page.on_hover(cx.listener(move |this, hovering: &bool, window, cx| {
                        this.paper_hover_changed(page_index, *hovering, window, cx);
                    }))
                })
                .child(
                    div()
                        .absolute()
                        .size_full()
                        .opacity(1.0 - 0.22 * self.spotlight_progress)
                        .child(page_canvas(data.clone(), page_index, px_per_pt)),
                );
            if !interactive {
                pages = pages.child(page);
                continue;
            }
            for region in data
                .regions
                .iter()
                .filter(|region| region.page == page_index)
            {
                if self
                    .model
                    .document
                    .value
                    .get(region.target.section)
                    .is_none()
                {
                    continue;
                }
                page = page.child(self.preview_hit(region.clone(), valid && !spotlight, cx));
                if !valid {
                    continue;
                }
                let target = region.target;
                let region_left = region.left * px_per_pt;
                let region_width = (region.right - region.left) * px_per_pt;
                if target.index.is_none() && target.section != "basics" {
                    if spotlight && self.selected_item_module.is_some() {
                        continue;
                    }
                    page = page.child(
                        div()
                            .absolute()
                            .left(px(region_left))
                            .top(px(region.top * px_per_pt - 14.0))
                            .w(px(region_width))
                            .child(self.module_insert_control(
                                Some(target.section.into()),
                                &style,
                                cx,
                            )),
                    );
                } else if let Some(index) = target.index {
                    if spotlight && self.selected_item_module.as_deref() != Some(target.section) {
                        continue;
                    }
                    let first = data
                        .regions
                        .iter()
                        .find(|candidate| candidate.target == target)
                        .unwrap();
                    let last = data
                        .regions
                        .iter()
                        .rev()
                        .find(|candidate| candidate.target == target)
                        .unwrap();
                    if index == 0 && first.page == page_index {
                        page = page.child(
                            div()
                                .absolute()
                                .left(px(region_left))
                                .top(px(region.top * px_per_pt - 10.0))
                                .w(px(region_width))
                                .child(self.item_insert_control(target.section, 0, &style, cx)),
                        );
                    }
                    if last.page == page_index {
                        page = page.child(
                            div()
                                .absolute()
                                .left(px(region_left))
                                .top(px(region.bottom * px_per_pt + 1.0))
                                .w(px(region_width))
                                .child(self.item_insert_control(
                                    target.section,
                                    index + 1,
                                    &style,
                                    cx,
                                )),
                        );
                    }
                }
            }
            if valid {
                page = self.empty_module_controls(page, &data, page_index, cx);
                if page_index + 1 == data.layout.pages.len()
                    && (!spotlight || self.selected_item_module.is_none())
                {
                    let bottom = data
                        .regions
                        .iter()
                        .filter(|region| region.page == page_index)
                        .map(|region| {
                            if region.target.section == "basics" && !region.text {
                                region.bottom.max(region.top + 32. / px_per_pt)
                            } else {
                                region.bottom
                            }
                        })
                        .fold(0.0, f32::max);
                    let position =
                        (bottom * px_per_pt + 18.0).min(data.layout.height_mm * px_per_mm - 30.0);
                    page = page.child(
                        div()
                            .absolute()
                            .left(px(left))
                            .top(px(position))
                            .w(px(content_width))
                            .child(self.module_insert_control(None, &style, cx)),
                    );
                }
            }
            // Stable page-level overlay slots: changing the edited item does
            // not change the active panel's parent identity or move the paper.
            for retiring in [true, false] {
                let Some(form) = self.inline_layer_target(retiring) else {
                    continue;
                };
                let matches_target = |region: &&Region| {
                    region.target.section == form.section && region.target.index == form.index
                };
                let Some(anchor) = data
                    .regions
                    .iter()
                    .filter(matches_target)
                    .find(|region| form.page.is_none_or(|p| region.page == p))
                    .or_else(|| data.regions.iter().find(matches_target))
                else {
                    continue;
                };
                if anchor.page == page_index {
                    let (form_width, form_left) = self.inline_form_geometry();
                    page = page.child(
                        div()
                            .id(if retiring {
                                "inline-overlay-retiring"
                            } else {
                                "inline-overlay-active"
                            })
                            .absolute()
                            .left(px(form_left))
                            .top(px(anchor.top * px_per_pt))
                            .w(px(form_width))
                            .child(self.inline_form(retiring, cx)),
                    );
                }
            }
            pages = pages.child(page);
        }
        pages.into_any_element()
    }

    fn preview_hit(&self, region: Region, valid: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let target = region.target;
        let original = self.active_style();
        let style = original.column_style(original.in_sidebar(target.section));
        let px_per_pt = PX_PER_PT * self.preview_scale;
        let id = target.section.to_owned();
        let hover_id = id.clone();
        let move_id = id.clone();
        let click_id = id.clone();
        let key_id = id.clone();
        let selector = if target.section == "basics" {
            "preview-basics".into()
        } else {
            format!(
                "preview-hit-{}-{:?}-{}",
                target.section, target.index, region.page
            )
        };
        let editable = target.index.is_some() || target.section == "basics";
        let hovered = match target.index {
            Some(index) => self.hovered_item.as_ref() == Some(&(id.clone(), index, region.page)),
            None => self.hovered_section.as_ref() == Some(&(id.clone(), region.page)),
        };
        let mut hit = div()
            .id(gpui::SharedString::from(format!(
                "hit-{}-{:?}-{}",
                id, target.index, region.page
            )))
            .debug_selector(move || selector.clone())
            .absolute()
            .left(px(region.left * px_per_pt))
            .top(px(region.top * px_per_pt))
            .w(px((region.right - region.left) * px_per_pt))
            .h(px(((region.bottom - region.top) * px_per_pt).max(4.0)))
            .when(!region.text && target.section == "basics" && valid, |hit| {
                hit.min_h(px(32.)).child(
                    div()
                        .text_size(px(13.))
                        .text_color(color(style.colors.accent))
                        .child("填写基本信息"),
                )
            })
            .when(valid && editable, |hit| {
                hit.focusable().tab_index(0).key_context("PreviewItem")
            })
            .on_hover(cx.listener(move |this, hovering: &bool, window, cx| {
                if let Some(index) = target.index {
                    if *hovering {
                        this.hovered_item = Some((hover_id.clone(), index, region.page));
                    } else if this.hovered_item.as_ref()
                        == Some(&(hover_id.clone(), index, region.page))
                    {
                        this.hovered_item = None;
                    }
                } else if *hovering {
                    this.hovered_section = Some((hover_id.clone(), region.page));
                } else if this.hovered_section.as_ref() == Some(&(hover_id.clone(), region.page)) {
                    this.hovered_section = None;
                }
                if *hovering && editable && valid {
                    this.schedule_inline(&hover_id, target.index, region.page, window, cx);
                }
                if !*hovering {
                    this.inline_region_left(&hover_id, target.index, region.page);
                }
                let remove_key = format!(
                    "remove-preview-{}-{:?}-{}",
                    hover_id, target.index, region.page
                );
                this.control_hover(&remove_key, *hovering, cx);
                cx.notify();
            }))
            .on_mouse_move(
                cx.listener(move |this, _: &gpui::MouseMoveEvent, window, cx| {
                    // Scrolling may clear affordances without changing this hit
                    // element's identity. A small move inside it must re-arm them.
                    let key = format!(
                        "remove-preview-{}-{:?}-{}",
                        move_id, target.index, region.page
                    );
                    if !this.control_hovered(&key) {
                        if let Some(index) = target.index {
                            this.hovered_item = Some((move_id.clone(), index, region.page));
                        } else {
                            this.hovered_section = Some((move_id.clone(), region.page));
                        }
                        this.control_hover(&key, true, cx);
                    }
                    if editable && valid {
                        // The whole block is a trigger, including text-free space
                        // and line gaps. Re-arm after scrolling even when there is
                        // no new hover-enter event; same-target movement preserves
                        // the original dwell rather than starting another timer.
                        this.schedule_inline(&move_id, target.index, region.page, window, cx);
                    }
                }),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                if editable && valid {
                    this.open_inline(&click_id, target.index, window, cx);
                    if let Some(form) = &mut this.inline.form {
                        form.page = Some(region.page);
                    }
                }
            }))
            .on_action(
                cx.listener(move |this, _: &inline_ui::OpenInline, window, cx| {
                    if editable && valid {
                        this.open_inline(&key_id, target.index, window, cx);
                        if let Some(form) = &mut this.inline.form {
                            form.page = Some(region.page);
                        }
                        this.focus_inline_first(window, cx);
                    }
                }),
            );
        if valid && editable {
            for (segment, ink) in region.ink.iter().enumerate() {
                let selector = format!(
                    "preview-ink-{}-{:?}-{}-{segment}",
                    target.section, target.index, region.page
                );
                hit = hit.child(
                    div()
                        .id(gpui::SharedString::from(selector.clone()))
                        .debug_selector(move || selector.clone())
                        .absolute()
                        .left(px((ink.left - region.left) * px_per_pt - 3.))
                        .top(px((ink.top - region.top) * px_per_pt))
                        .w(px((ink.width * px_per_pt + 6.).max(8.)))
                        .h(px((ink.height * px_per_pt).max(8.)))
                        .cursor_text(),
                );
            }
        }
        let remove_selector = format!("remove-preview-{}-{:?}-{}", id, target.index, region.page);
        let remove_alpha = self.control_alpha(&remove_selector, cx);
        if valid && (hovered || remove_alpha > 0.) {
            let remove_key = remove_selector.clone();
            hit = hit.child(
                div()
                    .id(gpui::SharedString::from(remove_selector.clone()))
                    .debug_selector(move || remove_selector.clone())
                    .absolute()
                    .right_0()
                    .top_0()
                    .opacity(remove_alpha)
                    .px_2()
                    .cursor_pointer()
                    .text_color(color(style.colors.seal))
                    .child("−")
                    .on_hover(cx.listener(|this, hovering: &bool, _, _| {
                        if *hovering {
                            this.cancel_inline_candidate();
                        }
                    }))
                    .on_mouse_move(cx.listener(|this, _: &gpui::MouseMoveEvent, _, cx| {
                        this.cancel_inline_candidate();
                        cx.stop_propagation();
                    }))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        if !this.control_hovered(&remove_key) {
                            return;
                        }
                        let edit = match target.index {
                            Some(index) => StructureEdit::RemoveItem {
                                id: id.clone(),
                                index,
                            },
                            None => StructureEdit::RemoveSection { id: id.clone() },
                        };
                        this.apply_visual_structure(edit, window, cx);
                    })),
            );
        }
        hit
    }

    fn empty_module_controls(
        &self,
        mut page: gpui::Stateful<gpui::Div>,
        data: &PreviewData,
        page_index: usize,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let summaries = self.model.document.sections();
        let style = self.active_style();
        let px_per_mm = PX_PER_MM * self.preview_scale;
        let px_per_pt = PX_PER_PT * self.preview_scale;
        for (index, summary) in summaries.iter().enumerate() {
            if summary.id == "basics"
                || summary.count != 0
                || self.selected_item_module.as_deref() != Some(&summary.id)
            {
                continue;
            }
            let sidebar = style.in_sidebar(&summary.id);
            let column = style.column_style(sidebar);
            let next = summaries.iter().skip(index + 1).find_map(|summary| {
                if style.in_sidebar(&summary.id) != sidebar {
                    return None;
                }
                data.regions.iter().find(|region| {
                    region.target.section == summary.id && region.target.index.is_none()
                })
            });
            let (target_page, y) = next.map_or_else(
                || {
                    let last = data
                        .regions
                        .iter()
                        .filter(|region| {
                            region.target.section == "basics"
                                || style.in_sidebar(region.target.section) == sidebar
                        })
                        .max_by(|a, b| {
                            a.page
                                .cmp(&b.page)
                                .then_with(|| a.bottom.total_cmp(&b.bottom))
                        });
                    last.map_or((0, 16.0), |region| {
                        (region.page, region.bottom * px_per_pt + 16.0)
                    })
                },
                |region| (region.page, region.top * px_per_pt - 14.0),
            );
            if target_page != page_index {
                continue;
            }
            if let Some(&(id, _)) = SECTION_LABELS.iter().find(|(id, _)| *id == summary.id) {
                page = page.child(
                    div()
                        .absolute()
                        .left(px(column.page.margins.left * px_per_mm))
                        .top(px(y))
                        .w(px((column.page.width_mm
                            - column.page.margins.left
                            - column.page.margins.right)
                            * px_per_mm))
                        .child(self.item_insert_control(id, 0, &style, cx)),
                );
            }
        }
        page
    }
}
