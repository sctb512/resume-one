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
fn cancel_read_keeps_workspace_responsive_and_ignores_stale_completion(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let root = tempfile::tempdir().unwrap();
    let first = root.path().join("first.json");
    let second = root.path().join("second.json");
    std::fs::write(&first, br#"{"basics":{"name":"First"}}"#).unwrap();
    std::fs::write(&second, br#"{"basics":{"name":"Second"}}"#).unwrap();
    let child = std::rc::Rc::new(std::cell::RefCell::new(None));
    let output = child.clone();
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| ResumeOne::new(AppModel::load(Some(&first)).unwrap(), cx));
        *output.borrow_mut() = Some(view.clone());
        gpui_component::Root::new(view, window, cx)
    });
    let view = child.borrow_mut().take().unwrap();
    cx.update(|window, _| window.refresh());
    let right = cx.debug_bounds("module-panel").unwrap();
    let stale = cx.update(|_, cx| view.update(cx, |app, cx| app.begin_read(cx)));
    assert!(cx.debug_bounds("file-loading-notice").is_some());
    cx.update(|window, cx| {
        view.update(cx, |app, cx| {
            app.reduce_motion = true;
            app.toggle_source(&ToggleSource, window, cx);
        })
    });
    cx.update(|_, cx| assert!(view.read(cx).source_mode));
    assert_eq!(right, cx.debug_bounds("module-panel").unwrap());
    click_center(cx, "cancel-file-loading");
    cx.update(|_, cx| {
        let app = view.read(cx);
        assert!(!app.files.busy);
        assert!(!app.files.loading);
        assert_eq!(app.files.reads_in_flight, 1);
        assert_eq!(app.model.content.name, "First");
    });
    let current = cx.update(|_, cx| view.update(cx, |app, cx| app.begin_read(cx)));
    cx.update(|window, cx| {
        view.update(cx, |app, cx| {
            app.finish_read(stale, false, Err("stale failure".into()), window, cx);
            assert!(app.files.busy && app.files.loading);
            assert_eq!(app.files.reads_in_flight, 1);
            assert_eq!(app.model.content.name, "First");
            app.finish_read(current, false, OpenedPath::read(&second), window, cx);
            assert!(!app.files.busy && !app.files.loading);
            assert_eq!(app.files.reads_in_flight, 0);
            assert_eq!(app.model.content.name, "Second");
        })
    });
    assert!(!cx.has_pending_prompt());
    assert_eq!(right, cx.debug_bounds("module-panel").unwrap());
    assert_eq!(
        std::fs::read(first).unwrap(),
        br#"{"basics":{"name":"First"}}"#
    );
}

#[gpui::test]
fn canceled_system_reads_are_bounded_and_late_success_never_replaces_document(
    cx: &mut TestAppContext,
) {
    cx.update(gpui_component::init);
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("late.json");
    std::fs::write(&file, br#"{"basics":{"name":"Late"}}"#).unwrap();
    let (view, cx) = cx.add_window_view(|_, cx| ResumeOne::new(AppModel::load(None).unwrap(), cx));
    let original = cx.update(|_, cx| view.read(cx).model.opened_source.clone());
    cx.update(|window, cx| {
        view.update(cx, |app, cx| {
            let first = app.begin_read(cx);
            app.cancel_read(cx);
            let second = app.begin_read(cx);
            app.cancel_read(cx);
            app.load_path(file.clone(), window, cx);
            assert_eq!(app.files.reads_in_flight, 2);
            assert!(!app.files.loading && !app.files.busy);
            app.finish_read(first, false, OpenedPath::read(&file), window, cx);
            app.finish_read(second, false, OpenedPath::read(&file), window, cx);
            assert_eq!(app.files.reads_in_flight, 0);
            assert_eq!(app.model.opened_source, original);
        })
    });
    assert!(cx.has_pending_prompt());
    cx.simulate_prompt_answer("确定");
    cx.run_until_parked();
    cx.update(|window, cx| view.update(cx, |app, cx| app.load_path(file.clone(), window, cx)));
    cx.run_until_parked();
    cx.update(|_, cx| {
        let app = view.read(cx);
        assert_eq!(app.model.content.name, "Late");
        assert!(!app.files.loading && !app.files.busy);
    });
}

#[gpui::test]
fn restoring_folder_scope_never_reads_resume_contents_or_opens_chooser(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("broken.json"), [0xff, 0xfe]).unwrap();
    let (view, cx) = cx.add_window_view(|_, cx| ResumeOne::new(AppModel::load(None).unwrap(), cx));
    let original = cx.update(|_, cx| view.read(cx).model.opened_source.clone());
    cx.update(|window, cx| {
        view.update(cx, |app, cx| {
            app.restore_folder(root.path().to_path_buf(), window, cx)
        })
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        let app = view.read(cx);
        assert_eq!(app.model.opened_source, original);
        assert_eq!(app.model.folder.as_ref().unwrap().files.len(), 1);
        assert!(!app.files.loading && !app.files.busy && !app.files.open);
    });
    assert!(!cx.has_pending_prompt());
}

#[gpui::test]
fn pending_read_rejects_save_and_guards_edits_made_before_completion(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    cx.update(crate::inline_ui::register);
    let root = tempfile::tempdir().unwrap();
    let first = root.path().join("first.json");
    let second = root.path().join("second.json");
    let original = br#"{"basics":{"name":"Original"}}"#;
    let edited = r#"{"basics":{"name":"Edited during read"}}"#;
    std::fs::write(&first, original).unwrap();
    std::fs::write(&second, br#"{"basics":{"name":"Second"}}"#).unwrap();
    let child = std::rc::Rc::new(std::cell::RefCell::new(None));
    let output = child.clone();
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| ResumeOne::new(AppModel::load(Some(&first)).unwrap(), cx));
        *output.borrow_mut() = Some(view.clone());
        gpui_component::Root::new(view, window, cx)
    });
    let view = child.borrow_mut().take().unwrap();
    cx.update(|window, cx| {
        view.update(cx, |app, cx| {
            app.reduce_motion = true;
            app.toggle_source(&ToggleSource, window, cx);
            app.begin_read(cx);
            app.editor
                .as_ref()
                .unwrap()
                .input
                .clone()
                .update(cx, |state, cx| {
                    state.set_value(edited, window, cx);
                });
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        view.update(cx, |app, cx| {
            app.save_json(&Save, window, cx);
            app.save_json_as(&SaveAs, window, cx);
            assert!(!app.editor.as_ref().unwrap().saving);
            app.dismiss_inline(&crate::inline_ui::DismissInline, window, cx);
            assert!(!app.files.loading && !app.files.busy);
            let generation = app.begin_read(cx);
            app.finish_read(generation, false, OpenedPath::read(&second), window, cx);
            assert_eq!(app.model.document_path, Some(first.canonicalize().unwrap()));
            assert_eq!(app.editor.as_ref().unwrap().buffer.source(), edited);
        })
    });
    assert!(cx.has_pending_prompt());
    cx.simulate_prompt_answer("继续编辑");
    cx.run_until_parked();
    assert_eq!(std::fs::read(first).unwrap(), original);
    cx.update(|_, cx| assert_eq!(view.read(cx).model.content.name, "Edited during read"));
}

#[gpui::test]
fn recent_file_switcher_reopens_a_file_without_a_folder(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    cx.update(register_bindings);
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("recent.json");
    let original = br#"{"basics":{"name":"Recent"},"private":{"keep":true}}"#;
    std::fs::write(&path, original).unwrap();
    let mut store = crate::theme_local::ThemeStore::new(
        root.path().join("settings.json"),
        root.path().join("themes"),
    )
    .unwrap();
    store.remember_file(&path).unwrap();
    let child = std::rc::Rc::new(std::cell::RefCell::new(None));
    let output = child.clone();
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| {
            let mut app = ResumeOne::new(AppModel::load(None).unwrap(), cx);
            app.theme_store = Some(store);
            app
        });
        window.focus(&view.read(cx).focus);
        *output.borrow_mut() = Some(view.clone());
        gpui_component::Root::new(view, window, cx)
    });
    let view = child.borrow_mut().take().unwrap();
    cx.update(|window, _| window.refresh());
    click_center(cx, "current-file");
    cx.update(|window, _| window.refresh());
    assert!(cx.debug_bounds("recent-file-0").is_some());
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    cx.update(|_, cx| {
        let app = view.read(cx);
        assert_eq!(app.model.content.name, "Recent");
        assert_eq!(app.model.document.source, original);
        assert!(app.model.folder.is_none());
        assert!(!app.files.open);
    });
    assert_eq!(std::fs::read(path).unwrap(), original);
}

#[gpui::test]
fn folder_candidates_precede_other_recent_files_without_duplicates(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    cx.update(register_bindings);
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("folder");
    std::fs::create_dir(&folder).unwrap();
    let in_folder = folder.join("a.json");
    let other = root.path().join("other.json");
    std::fs::write(&in_folder, br#"{"basics":{"name":"Folder"}}"#).unwrap();
    std::fs::write(&other, br#"{"basics":{"name":"Other"}}"#).unwrap();
    let mut store = crate::theme_local::ThemeStore::new(
        root.path().join("settings.json"),
        root.path().join("themes"),
    )
    .unwrap();
    store.remember_file(&in_folder).unwrap();
    store.remember_file(&other).unwrap();
    let mut model = AppModel::load(None).unwrap();
    model.folder = Some(crate::workspace::LocalFolder::scan(&folder).unwrap());
    let child = std::rc::Rc::new(std::cell::RefCell::new(None));
    let output = child.clone();
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| {
            let mut app = ResumeOne::new(model, cx);
            app.theme_store = Some(store);
            app.files.open = false;
            app
        });
        window.focus(&view.read(cx).focus);
        *output.borrow_mut() = Some(view.clone());
        gpui_component::Root::new(view, window, cx)
    });
    let view = child.borrow_mut().take().unwrap();
    cx.update(|window, _| window.refresh());
    click_center(cx, "current-file");
    cx.update(|window, _| window.refresh());
    assert!(cx.debug_bounds("file-0").is_some());
    assert!(cx.debug_bounds("recent-file-0").is_some());
    assert!(cx.debug_bounds("recent-file-1").is_none());
    cx.simulate_keystrokes("down enter");
    cx.run_until_parked();
    cx.update(|_, cx| {
        let app = view.read(cx);
        assert_eq!(app.model.content.name, "Other");
        assert_eq!(app.model.folder.as_ref().unwrap().files.len(), 1);
        assert_eq!(
            app.theme_store.as_ref().unwrap().recent_files()[0],
            other.canonicalize().unwrap()
        );
    });
}

#[gpui::test]
fn new_resume_is_a_clean_preview_draft_and_never_overwrites_the_open_file(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    cx.update(register_bindings);
    cx.update(crate::inline_ui::register);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("original.json");
    let original = br#"{"basics":{"name":"Original"},"work":[{"name":"Keep"}]}"#;
    std::fs::write(&path, original).unwrap();
    let mut model = AppModel::load(Some(&path)).unwrap();
    model.folder = Some(crate::workspace::LocalFolder::scan(directory.path()).unwrap());
    model.select_theme(3);
    let theme = model.applied_theme().identity.clone();
    let child = std::rc::Rc::new(std::cell::RefCell::new(None));
    let output = child.clone();
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| ResumeOne::new(model, cx));
        *output.borrow_mut() = Some(view.clone());
        gpui_component::Root::new(view, window, cx)
    });
    let view = child.borrow_mut().take().unwrap();
    cx.update(|window, _| window.refresh());
    click_center(cx, "open-menu");
    cx.simulate_keystrokes("down down enter");
    cx.run_until_parked();
    cx.update(|_, cx| {
        view.update(cx, |app, _| {
            assert!(!app.source_mode);
            assert!(!app.model.is_example);
            assert_eq!(app.model.document_label, "未命名简历");
            assert!(app.model.document_path.is_none());
            assert_eq!(app.model.document.value, serde_json::json!({"basics":{}}));
            assert_eq!(app.model.applied_theme().identity, theme);
            assert_eq!(
                app.model.folder.as_ref().unwrap().path,
                directory.path().canonicalize().unwrap()
            );
            assert!(app.inline_matches("basics", None));
            app.editor.as_mut().unwrap().store =
                resume_one_core::draft::DraftStore::new(directory.path().join("drafts"), None);
        })
    });
    assert!(cx.debug_bounds("inline-form").is_some());
    assert_eq!(std::fs::read(&path).unwrap(), original);

    // An unsaved new draft must get the same discard guard as an opened file.
    let edited = r#"{"basics":{"name":"Unsaved"},"custom":{"keep":true}}"#;
    cx.update(|window, cx| {
        let input = view.read(cx).editor.as_ref().unwrap().input.clone();
        input.update(cx, |state, cx| state.set_value(edited, window, cx));
    });
    cx.run_until_parked();
    cx.update(|window, cx| view.update(cx, |app, cx| app.new_resume(&NewResume, window, cx)));
    assert!(cx.has_pending_prompt());
    cx.simulate_prompt_answer("继续编辑");
    cx.run_until_parked();
    cx.update(|_, cx| {
        let app = view.read(cx);
        assert_eq!(app.editor.as_ref().unwrap().buffer.source(), edited);
        assert!(app.editor.as_ref().unwrap().store.load().unwrap().is_some());
        assert!(!app.files.busy);
    });
    cx.update(|window, cx| view.update(cx, |app, cx| app.new_resume(&NewResume, window, cx)));
    cx.simulate_prompt_answer("丢弃更改并新建");
    cx.run_until_parked();
    cx.update(|_, cx| {
        let app = view.read(cx);
        assert_eq!(app.model.document.value, serde_json::json!({"basics":{}}));
        assert!(app.visual_undo.is_empty());
        assert!(app.editor.as_ref().unwrap().buffer.issues().is_empty());
    });
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert!(!directory.path().join("resume.json").exists());
}

#[gpui::test]
fn invalid_file_opens_for_repair_and_saves_only_after_becoming_valid(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("broken.json");
    let broken = b"{\"basics\": ";
    std::fs::write(&path, broken).unwrap();
    let child = std::rc::Rc::new(std::cell::RefCell::new(None));
    let child_for_window = child.clone();
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| ResumeOne::new(AppModel::load(None).unwrap(), cx));
        *child_for_window.borrow_mut() = Some(view.clone());
        gpui_component::Root::new(view, window, cx)
    });
    let view = child.borrow_mut().take().unwrap();
    cx.update(|window, cx| view.update(cx, |app, cx| app.load_path(path.clone(), window, cx)));
    cx.run_until_parked();
    cx.update(|_, cx| {
        let app = view.read(cx);
        assert!(app.source_mode);
        assert!(!app.model.preview_ready);
        assert_eq!(
            app.editor.as_ref().unwrap().buffer.issues()[0].code,
            "INVALID_JSON"
        );
        assert_eq!(
            app.editor.as_ref().unwrap().buffer.source().as_bytes(),
            broken
        );
    });
    assert_eq!(std::fs::read(&path).unwrap(), broken);

    let valid = r#"{"basics":{"name":"已修复"}}"#;
    cx.update(|window, cx| {
        let input = view.read(cx).editor.as_ref().unwrap().input.clone();
        input.update(cx, |state, cx| state.set_value(valid, window, cx));
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        let app = view.read(cx);
        assert!(app.model.preview_ready);
        assert_eq!(app.model.content.name, "已修复");
        assert!(app.editor.as_ref().unwrap().buffer.is_dirty());
    });
    cx.update(|window, cx| {
        view.update(cx, |app, cx| {
            app.save_json(&crate::editor_ui::Save, window, cx)
        });
    });
    cx.run_until_parked();
    assert_eq!(std::fs::read(&path).unwrap(), valid.as_bytes());
    cx.update(|_, cx| assert!(!view.read(cx).editor.as_ref().unwrap().buffer.is_dirty()));
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
    cx.update(gpui_component::init);
    let directory = tempfile::tempdir().unwrap();
    let first = directory.path().join("a.json");
    let second = directory.path().join("b.json");
    let invalid = directory.path().join("c.json");
    std::fs::write(&first, br#"{"basics":{"name":"A"}}"#).unwrap();
    let second_bytes = br#"{ "basics": { "name": "B" }, "custom": {"keep": true} }"#;
    std::fs::write(&second, second_bytes).unwrap();
    std::fs::write(&invalid, b"{invalid").unwrap();
    let model = AppModel::load(None).unwrap();
    let child = std::rc::Rc::new(std::cell::RefCell::new(None));
    let child_for_window = child.clone();
    let (_, cx) = cx.add_window_view(|window, cx| {
        register_bindings(cx);
        let view = cx.new(|cx| ResumeOne::new(model, cx));
        window.focus(&view.read(cx).focus);
        *child_for_window.borrow_mut() = Some(view.clone());
        gpui_component::Root::new(view, window, cx)
    });
    let view = child.borrow_mut().take().unwrap();
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
    assert!(!cx.has_pending_prompt());
    cx.update(|_, cx| {
        let app = view.read(cx);
        assert_eq!(app.model.content.name, "B");
        assert!(!app.model.preview_ready);
        assert!(app.source_mode);
        assert_eq!(app.model.opened_source, b"{invalid");
        assert_eq!(
            app.editor.as_ref().unwrap().buffer.issues()[0].code,
            "INVALID_JSON"
        );
        assert_eq!(
            app.model.document_path,
            Some(invalid.canonicalize().unwrap())
        );
        assert_eq!(app.model.selected_theme, 3);
        assert!(!app.files.busy);
    });
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
