use super::*;
use crate::model::AppModel;
use gpui::TestAppContext;

fn click_center(cx: &mut gpui::VisualTestContext, selector: &'static str) {
    let bounds = cx.debug_bounds(selector).unwrap();
    cx.simulate_click(
        gpui::point(
            bounds.origin.x + bounds.size.width / 2.0,
            bounds.origin.y + bounds.size.height / 2.0,
        ),
        gpui::Modifiers::none(),
    );
}

#[gpui::test]
fn file_trigger_and_open_button_toggle_their_own_menu(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let model = AppModel::load(Some(directory.path())).unwrap();
    let (view, cx) = cx.add_window_view(|_, cx| ResumeOne::new(model, cx));
    cx.update(|window, _| window.refresh());
    click_center(cx, "current-file");
    cx.update(|_, cx| assert!(!view.read(cx).files.open));
    click_center(cx, "current-file");
    cx.update(|_, cx| {
        assert!(view.read(cx).files.open);
        assert!(!view.read(cx).files.anchor_right);
    });
    click_center(cx, "open-menu");
    cx.update(|_, cx| {
        assert!(view.read(cx).files.open);
        assert!(view.read(cx).files.anchor_right);
        assert!(view.read(cx).files.actions_only);
    });
    click_center(cx, "open-menu");
    cx.update(|_, cx| assert!(!view.read(cx).files.open));
}

#[gpui::test]
fn folder_keyboard_selection_and_failed_open_keep_document_and_theme(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let first = directory.path().join("a.json");
    let second = directory.path().join("b.json");
    let invalid = directory.path().join("c.json");
    std::fs::write(&first, br#"{"basics":{"name":"A"}}"#).unwrap();
    let second_bytes = br#"{ "basics": { "name": "B" }, "custom": {"keep": true} }"#;
    std::fs::write(&second, second_bytes).unwrap();
    std::fs::write(&invalid, b"{invalid").unwrap();
    let model = AppModel::load(None).unwrap();
    let (view, cx) = cx.add_window_view(|window, cx| {
        register_bindings(cx);
        let view = ResumeOne::new(model, cx);
        window.focus(&view.focus);
        view
    });
    cx.update(|window, cx| {
        view.update(cx, |app, cx| {
            app.model.select_theme(3);
            app.load_path(directory.path().to_path_buf(), window, cx);
        })
    });
    cx.run_until_parked();
    cx.update(|window, _| window.refresh());
    assert!(cx.debug_bounds("file-switcher").is_some());
    cx.update(|_, cx| {
        assert_eq!(view.read(cx).model.content.name, "林清和");
        assert_eq!(view.read(cx).model.folder.as_ref().unwrap().files.len(), 3);
        view.read(cx)
            .preview_scroll
            .set_offset(gpui::point(px(0.0), px(-100.0)));
    });
    cx.simulate_keystrokes("down enter");
    cx.run_until_parked();
    cx.update(|_, cx| {
        let app = view.read(cx);
        assert_eq!(app.model.content.name, "B");
        assert_eq!(app.model.document.source, second_bytes);
        assert_eq!(app.model.selected_theme, 3);
        assert!(!app.files.open);
        assert!(!app.files.busy);
        assert_eq!(app.preview_scroll.offset(), gpui::point(px(0.0), px(0.0)));
    });
    assert_eq!(std::fs::read(&second).unwrap(), second_bytes);
    cx.dispatch_action(ToggleFiles);
    cx.update(|window, _| window.refresh());
    cx.simulate_keystrokes("down enter");
    cx.run_until_parked();
    assert!(cx.has_pending_prompt());
    cx.update(|_, cx| {
        let app = view.read(cx);
        assert_eq!(app.model.content.name, "B");
        assert_eq!(app.model.document.source, second_bytes);
        assert_eq!(
            app.model.document_path,
            Some(second.canonicalize().unwrap())
        );
        assert_eq!(app.model.selected_theme, 3);
        assert!(!app.files.busy);
    });
    cx.simulate_prompt_answer("确定");
    cx.dispatch_action(ToggleFiles);
    cx.update(|window, _| window.refresh());
    cx.simulate_keystrokes("escape");
    cx.update(|_, cx| assert!(!view.read(cx).files.open));
    std::fs::write(directory.path().join("new.json"), b"{}").unwrap();
    cx.dispatch_action(RefreshFolder);
    cx.run_until_parked();
    cx.update(|_, cx| {
        let app = view.read(cx);
        assert_eq!(app.model.folder.as_ref().unwrap().files.len(), 4);
        assert_eq!(app.model.content.name, "B");
    });
}

#[gpui::test]
fn canceled_selection_and_empty_folder_keep_current_resume(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let (view, cx) = cx.add_window_view(|_, cx| ResumeOne::new(AppModel::load(None).unwrap(), cx));
    cx.update(|window, cx| {
        view.update(cx, |app, cx| {
            app.files.busy = true;
            app.finish_open(None, window, cx);
            assert!(!app.files.busy);
            assert_eq!(app.model.content.name, "林清和");
            app.load_path(directory.path().to_path_buf(), window, cx);
        })
    });
    cx.run_until_parked();
    cx.update(|window, _| window.refresh());
    assert!(cx.debug_bounds("file-switcher").is_some());
    cx.update(|_, cx| {
        let app = view.read(cx);
        assert_eq!(app.model.content.name, "林清和");
        assert!(app.model.folder.as_ref().unwrap().files.is_empty());
        assert!(app.files.open);
    });
}
