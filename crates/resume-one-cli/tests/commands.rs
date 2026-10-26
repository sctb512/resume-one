use std::fs;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use serde_json::{Value, json};

fn call(args: &[&str]) -> (Output, Value) {
    let output = Command::new(env!("CARGO_BIN_EXE_resume-one"))
        .args(args)
        .output()
        .unwrap();
    assert!(output.stderr.is_empty(), "stderr: {:?}", output.stderr);
    let stdout = String::from_utf8(output.stdout.clone()).unwrap();
    assert_eq!(stdout.lines().count(), 1, "stdout: {stdout}");
    let value = serde_json::from_str(&stdout).unwrap();
    (output, value)
}

fn path(path: &Path) -> &str {
    path.to_str().unwrap()
}

#[test]
fn locked_document_returns_retryable_io_error_without_waiting() {
    use std::sync::mpsc;
    use std::time::Duration;
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("busy.json");
    let patch = root.path().join("patch.json");
    let original = br#"{"basics":{"name":"Original"}}"#;
    fs::write(&file, original).unwrap();
    fs::write(
        &patch,
        br#"[{"op":"replace","path":"/basics/name","value":"Changed"}]"#,
    )
    .unwrap();
    let revision = resume_one_core::resume::revision(original);
    let guard = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.path().join(".busy.json.resume-one.lock"))
        .unwrap();
    guard.lock().unwrap();
    let (sender, receiver) = mpsc::channel();
    let worker = {
        let file = file.clone();
        let revision = revision.clone();
        std::thread::spawn(move || {
            sender
                .send(call(&[
                    "patch",
                    "--file",
                    path(&file),
                    "--patch",
                    path(&patch),
                    "--if-match",
                    &revision,
                    "--json",
                ]))
                .unwrap();
        })
    };
    let result = receiver.recv_timeout(Duration::from_secs(3));
    drop(guard);
    worker.join().unwrap();
    let (output, value) = result.expect("CLI must finish while another process holds the lock");
    assert_eq!(output.status.code(), Some(5));
    assert_eq!(value["error"]["code"], "RESUME_BUSY");
    assert_eq!(fs::read(file).unwrap(), original);
}

#[test]
fn init_inspect_read_and_schema_are_agent_safe() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("简历 one.json");
    let (created, init) = call(&["init", "--file", path(&file), "--json"]);
    assert!(created.status.success());
    assert_eq!(init["result"]["valid"], true);
    let original = fs::read(&file).unwrap();
    let (again, existing) = call(&["init", "--file", path(&file), "--json"]);
    assert_eq!(again.status.code(), Some(5));
    assert_eq!(existing["error"]["code"], "FILE_EXISTS");
    assert_eq!(fs::read(&file).unwrap(), original);

    fs::write(
        &file,
        r#"{"basics":{"name":"Sensitive Name"},"custom":{"a/b":"secret"},"work":[{}]}"#,
    )
    .unwrap();
    let (_, inspect) = call(&["inspect", "--file", path(&file), "--json"]);
    assert_eq!(inspect["result"]["sections"]["work"], 1);
    assert_eq!(inspect["result"]["sections"]["basics"], 1);
    assert!(!inspect.to_string().contains("Sensitive Name"));
    assert!(!inspect.to_string().contains("secret"));
    let (_, selected) = call(&[
        "read",
        "--file",
        path(&file),
        "--pointer",
        "/custom/a~1b",
        "--json",
    ]);
    assert_eq!(selected["result"]["value"], "secret");
    let (_, schema) = call(&["schema", "--json"]);
    assert_eq!(schema["result"]["schemaVersion"], "1.3.1");
    assert!(schema["result"]["schema"].is_object());
}

#[test]
fn list_does_not_read_files_or_recurse() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("a.json"), "not JSON").unwrap();
    fs::write(root.path().join("b.txt"), "x").unwrap();
    fs::create_dir(root.path().join("nested")).unwrap();
    fs::write(root.path().join("nested/c.json"), "x").unwrap();
    let (_, value) = call(&["list", "--dir", path(root.path()), "--json"]);
    assert_eq!(value["result"]["files"].as_array().unwrap().len(), 1);
    assert_eq!(value["result"]["files"][0]["name"], "a.json");
}

#[test]
fn patch_dry_run_commit_conflict_and_rollback() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("履历.json");
    let patch = root.path().join("change.json");
    fs::write(&file, r#"{"basics":{"name":"甲"},"custom":{"keep":true}}"#).unwrap();
    fs::write(
        &patch,
        r#"[{"op":"replace","path":"/basics/name","value":"乙"}]"#,
    )
    .unwrap();
    let (_, inspect) = call(&["inspect", "--file", path(&file), "--json"]);
    let revision = inspect["result"]["revision"].as_str().unwrap();
    let old = fs::read(&file).unwrap();
    let (_, preview) = call(&[
        "patch",
        "--file",
        path(&file),
        "--patch",
        path(&patch),
        "--if-match",
        revision,
        "--dry-run",
        "--json",
    ]);
    assert_eq!(preview["result"]["dryRun"], true);
    assert_eq!(preview["result"]["changedPaths"], json!(["/basics/name"]));
    assert!(!preview.to_string().contains("乙"));
    assert_eq!(fs::read(&file).unwrap(), old);
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 2);
    let (_, committed) = call(&[
        "patch",
        "--file",
        path(&file),
        "--patch",
        path(&patch),
        "--if-match",
        revision,
        "--json",
    ]);
    assert_eq!(
        committed["result"]["revision"],
        preview["result"]["revision"]
    );
    let parsed: Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    assert_eq!(parsed["basics"]["name"], "乙");
    assert_eq!(parsed["custom"]["keep"], true);

    let (conflict, value) = call(&[
        "patch",
        "--file",
        path(&file),
        "--patch",
        path(&patch),
        "--if-match",
        revision,
        "--json",
    ]);
    assert_eq!(conflict.status.code(), Some(4));
    assert_eq!(value["error"]["code"], "REVISION_CONFLICT");
    assert_eq!(value["revision"], committed["result"]["revision"]);

    let after = fs::read(&file).unwrap();
    fs::write(
        &patch,
        r#"[{"op":"replace","path":"/basics/name","value":42}]"#,
    )
    .unwrap();
    let current = committed["result"]["revision"].as_str().unwrap();
    let (invalid, value) = call(&[
        "patch",
        "--file",
        path(&file),
        "--patch",
        path(&patch),
        "--if-match",
        current,
        "--json",
    ]);
    assert_eq!(invalid.status.code(), Some(3));
    assert_eq!(value["error"]["code"], "RESUME_INVALID");
    assert_eq!(fs::read(&file).unwrap(), after);
}

#[test]
fn structure_commands_are_revision_guarded_and_preserve_unknown_fields() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("resume.json");
    fs::write(
        &file,
        r#"{"basics":{"name":"Private"},"custom":{"html":"<br>"},"work":[{"name":"A"}],"education":[{}]}"#,
    )
    .unwrap();
    let (_, inspected) = call(&["inspect", "--file", path(&file), "--json"]);
    let revision = inspected["result"]["revision"].as_str().unwrap();
    let original = fs::read(&file).unwrap();
    let (_, preview) = call(&[
        "structure",
        "--file",
        path(&file),
        "--if-match",
        revision,
        "--dry-run",
        "add-module",
        "skills",
        "--before",
        "education",
        "--json",
    ]);
    assert_eq!(preview["result"]["changedPaths"], json!(["/skills"]));
    assert_eq!(preview["result"]["dryRun"], true);
    assert!(!preview.to_string().contains("Private"));
    assert_eq!(fs::read(&file).unwrap(), original);
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    let (_, added) = call(&[
        "structure",
        "--file",
        path(&file),
        "--if-match",
        revision,
        "add-module",
        "skills",
        "--before",
        "education",
        "--json",
    ]);
    assert_eq!(added["result"]["revision"], preview["result"]["revision"]);
    let after: Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    assert_eq!(after["custom"]["html"], "<br>");
    let keys: Vec<_> = after
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(keys, ["basics", "custom", "work", "skills", "education"]);

    let (conflict, error) = call(&[
        "structure",
        "--file",
        path(&file),
        "--if-match",
        revision,
        "add-item",
        "work",
        "--index",
        "1",
        "--json",
    ]);
    assert_eq!(conflict.status.code(), Some(4));
    assert_eq!(error["error"]["code"], "REVISION_CONFLICT");
    let next_revision = added["result"]["revision"].as_str().unwrap();
    let (_, item) = call(&[
        "structure",
        "--file",
        path(&file),
        "--if-match",
        next_revision,
        "add-item",
        "work",
        "--index",
        "1",
        "--json",
    ]);
    assert_eq!(item["result"]["changedPaths"], json!(["/work/1"]));
    let with_item: Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    assert_eq!(with_item["work"], json!([{"name":"A"}, {}]));
    let unchanged = fs::read(&file).unwrap();
    let current = item["result"]["revision"].as_str().unwrap();
    let (bad, error) = call(&[
        "structure",
        "--file",
        path(&file),
        "--if-match",
        current,
        "remove-item",
        "work",
        "--index",
        "9",
        "--json",
    ]);
    assert_eq!(bad.status.code(), Some(3));
    assert_eq!(error["error"]["code"], "ITEM_INDEX");
    assert_eq!(fs::read(&file).unwrap(), unchanged);
}

#[test]
fn set_field_previews_validates_and_commits_without_leaking_content() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("resume.json");
    fs::write(
        &file,
        r#"{"basics":{"name":"Private"},"work":[{"name":"Old","custom":{"html":"<br>"}}]}"#,
    )
    .unwrap();
    let (_, inspected) = call(&["inspect", "--file", path(&file), "--json"]);
    let revision = inspected["result"]["revision"].as_str().unwrap();
    let original = fs::read(&file).unwrap();
    let args = [
        "set-field",
        "--file",
        path(&file),
        "--if-match",
        revision,
        "--section",
        "work",
        "--index",
        "0",
        "--field",
        "name",
        "--value",
        "New",
        "--dry-run",
        "--json",
    ];
    let (_, preview) = call(&args);
    assert_eq!(preview["result"]["changedPaths"], json!(["/work/0/name"]));
    assert!(!preview.to_string().contains("Private"));
    assert!(!preview.to_string().contains("New"));
    assert_eq!(fs::read(&file).unwrap(), original);
    let (_, committed) = call(&[
        "set-field",
        "--file",
        path(&file),
        "--if-match",
        revision,
        "--section",
        "work",
        "--index",
        "0",
        "--field",
        "name",
        "--value",
        "New",
        "--json",
    ]);
    assert_eq!(
        committed["result"]["revision"],
        preview["result"]["revision"]
    );
    let parsed: Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    assert_eq!(parsed["work"][0]["name"], "New");
    assert_eq!(parsed["work"][0]["custom"]["html"], "<br>");
    let after = fs::read(&file).unwrap();
    let current = committed["result"]["revision"].as_str().unwrap();
    let (invalid, error) = call(&[
        "set-field",
        "--file",
        path(&file),
        "--if-match",
        current,
        "--section",
        "basics",
        "--field",
        "email",
        "--value",
        "not-an-email",
        "--json",
    ]);
    assert_eq!(invalid.status.code(), Some(3));
    assert_eq!(error["error"]["code"], "RESULT_INVALID");
    assert_eq!(fs::read(&file).unwrap(), after);
    let (conflict, error) = call(&[
        "set-field",
        "--file",
        path(&file),
        "--if-match",
        revision,
        "--section",
        "work",
        "--index",
        "0",
        "--field",
        "name",
        "--value",
        "Other",
        "--json",
    ]);
    assert_eq!(conflict.status.code(), Some(4));
    assert_eq!(error["error"]["code"], "REVISION_CONFLICT");
    let (updated, nested) = call(&[
        "set-field",
        "--file",
        path(&file),
        "--if-match",
        current,
        "--section",
        "basics",
        "--field",
        "location.city",
        "--value",
        "杭州",
        "--json",
    ]);
    assert!(updated.status.success());
    assert_eq!(
        nested["result"]["changedPaths"],
        json!(["/basics/location/city"])
    );
    let parsed: Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    assert_eq!(parsed["basics"]["location"]["city"], "杭州");
}

#[test]
fn validates_errors_and_json_arguments_without_leaking_body() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("invalid.json");
    fs::write(&file, r#"{"basics":{"email":"private but invalid"}}"#).unwrap();
    let (invalid, value) = call(&["validate", "--file", path(&file), "--json"]);
    assert_eq!(invalid.status.code(), Some(3));
    assert_eq!(value["error"]["code"], "RESUME_INVALID");
    assert_eq!(value["error"]["diagnostics"][0]["path"], "/basics/email");
    assert!(!value.to_string().contains("private but invalid"));
    let (missing, value) = call(&["inspect", "--json"]);
    assert_eq!(missing.status.code(), Some(2));
    assert_eq!(value["error"]["code"], "ARGUMENT_ERROR");
    let (relative, value) = call(&["inspect", "--file", "resume.json", "--json"]);
    assert_eq!(relative.status.code(), Some(2));
    assert_eq!(value["error"]["code"], "PATH_NOT_ABSOLUTE");
}

#[test]
fn patch_from_stdin_is_supported() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("resume.json");
    fs::write(&file, r#"{"basics":{"name":"A"}}"#).unwrap();
    let (_, inspect) = call(&["inspect", "--file", path(&file), "--json"]);
    let revision = inspect["result"]["revision"].as_str().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_resume-one"))
        .args([
            "patch",
            "--file",
            path(&file),
            "--patch",
            "-",
            "--if-match",
            revision,
            "--json",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(br#"[{"op":"replace","path":"/basics/name","value":"B"}]"#)
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["result"]["changedPaths"], json!(["/basics/name"]));
}

#[test]
fn two_cli_processes_cannot_commit_the_same_revision() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("resume.json");
    let first_patch = root.path().join("first.json");
    let second_patch = root.path().join("second.json");
    fs::write(&file, r#"{"basics":{"name":"Before"}}"#).unwrap();
    fs::write(
        &first_patch,
        r#"[{"op":"replace","path":"/basics/name","value":"First"}]"#,
    )
    .unwrap();
    fs::write(
        &second_patch,
        r#"[{"op":"replace","path":"/basics/name","value":"Second"}]"#,
    )
    .unwrap();
    let (_, inspect) = call(&["inspect", "--file", path(&file), "--json"]);
    let revision = inspect["result"]["revision"].as_str().unwrap();
    let children: Vec<_> = [&first_patch, &second_patch]
        .into_iter()
        .map(|patch| {
            Command::new(env!("CARGO_BIN_EXE_resume-one"))
                .args([
                    "patch",
                    "--file",
                    path(&file),
                    "--patch",
                    path(patch),
                    "--if-match",
                    revision,
                    "--json",
                ])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap()
        })
        .collect();
    let outcomes: Vec<_> = children
        .into_iter()
        .map(|child| child.wait_with_output().unwrap())
        .collect();
    assert_eq!(
        outcomes
            .iter()
            .filter(|output| output.status.success())
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|output| {
                let value: Value = serde_json::from_slice(&output.stdout).unwrap();
                matches!(
                    (output.status.code(), value["error"]["code"].as_str()),
                    (Some(4), Some("REVISION_CONFLICT")) | (Some(5), Some("RESUME_BUSY"))
                )
            })
            .count(),
        1
    );
    let final_name = serde_json::from_slice::<Value>(&fs::read(&file).unwrap()).unwrap()["basics"]
        ["name"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(final_name == "First" || final_name == "Second");
}

#[test]
fn exports_text_and_markdown_without_changing_json_or_overwriting_by_default() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("简历.json");
    let txt = root.path().join("简历.txt");
    let md = root.path().join("resume.md");
    let source = br#"{"basics":{"name":"Li Ming","summary":"Design lead"},"work":[{"position":"Designer","name":"Studio","highlights":["Improved UX"]}],"custom":{"private":"SECRET"}}"#;
    fs::write(&file, source).unwrap();
    let (output, result) = call(&[
        "export",
        "--file",
        path(&file),
        "--format",
        "txt",
        "--out",
        path(&txt),
        "--json",
    ]);
    assert!(output.status.success());
    assert_eq!(result["result"]["format"], "txt");
    let text = fs::read_to_string(&txt).unwrap();
    assert!(text.contains("工作经历"));
    assert!(text.contains("Improved UX"));
    assert!(!text.contains("SECRET"));
    assert_eq!(fs::read(&file).unwrap(), source);
    let (exists, response) = call(&[
        "export",
        "--file",
        path(&file),
        "--format",
        "txt",
        "--out",
        path(&txt),
        "--json",
    ]);
    assert_eq!(exists.status.code(), Some(6));
    assert_eq!(response["error"]["code"], "FILE_EXISTS");
    assert_eq!(fs::read_to_string(&txt).unwrap(), text);
    let (same, response) = call(&[
        "export",
        "--file",
        path(&file),
        "--format",
        "md",
        "--out",
        path(&file),
        "--overwrite",
        "--json",
    ]);
    assert_eq!(same.status.code(), Some(6));
    assert_eq!(response["error"]["code"], "SAME_FILE");
    assert_eq!(fs::read(&file).unwrap(), source);
    let (output, _) = call(&[
        "export",
        "--file",
        path(&file),
        "--format",
        "md",
        "--out",
        path(&md),
        "--lang",
        "en",
        "--json",
    ]);
    assert!(output.status.success());
    let markdown = fs::read_to_string(&md).unwrap();
    assert!(markdown.contains("# Li Ming"));
    assert!(markdown.contains("## Work"));
    assert!(markdown.contains("- Improved UX"));
    assert!(!markdown.contains("SECRET"));
}

#[test]
fn exports_editable_docx_with_chinese_content() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("resume.json");
    let out = root.path().join("简历.docx");
    let source = r#"{"basics":{"name":"李明"},"work":[{"name":"示例公司","position":"设计师"}]}"#;
    fs::write(&file, source).unwrap();
    let (output, result) = call(&[
        "export",
        "--file",
        path(&file),
        "--format",
        "docx",
        "--out",
        path(&out),
        "--json",
    ]);
    assert!(output.status.success());
    assert_eq!(result["result"]["format"], "docx");
    let bytes = fs::read(&out).unwrap();
    assert!(bytes.starts_with(b"PK"));
    let document = docx_rs::read_docx(&bytes).unwrap();
    assert!(document.json().contains("李明"));
    assert_eq!(fs::read_to_string(&file).unwrap(), source);
}

#[test]
fn exports_themed_docx_and_preserves_existing_output_on_failure() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("resume.json");
    let out = root.path().join("resume.docx");
    let theme = root.path().join("chosen.theme.json");
    let source = r#"{"basics":{"name":"李明"},"work":[{"name":"Company"}],"extension":{"keep":"DO_NOT_EXPORT_EXTENSION"}}"#;
    fs::write(&file, source).unwrap();
    fs::write(
        &theme,
        include_bytes!("../../../assets/themes/xingye.theme.json"),
    )
    .unwrap();
    let args = [
        "export",
        "--file",
        path(&file),
        "--format",
        "docx",
        "--out",
        path(&out),
        "--theme",
        path(&theme),
        "--json",
    ];
    let (status, response) = call(&args);
    assert!(status.status.success(), "{response}");
    let old = fs::read(&out).unwrap();
    let parsed = docx_rs::read_docx(&old).unwrap().json();
    assert!(parsed.contains("93523B"), "selected theme was ignored");
    assert!(parsed.contains("李明"));
    assert!(!parsed.contains("DO_NOT_EXPORT_EXTENSION"));
    let (status, response) = call(&args);
    assert_eq!(status.status.code(), Some(6));
    assert_eq!(response["error"]["code"], "FILE_EXISTS");
    assert_eq!(fs::read(&out).unwrap(), old);
    let mut value: Value = serde_json::from_slice(&fs::read(&theme).unwrap()).unwrap();
    value["composition"] =
        serde_json::json!({"preset":"sidebar-left","placements":{"work":"sidebar"}});
    fs::write(&theme, serde_json::to_vec(&value).unwrap()).unwrap();
    let mut overwrite = args.to_vec();
    overwrite.push("--overwrite");
    let (status, response) = call(&overwrite);
    assert_eq!(status.status.code(), Some(6));
    assert_eq!(response["error"]["code"], "EXPORT_RENDER");
    assert_eq!(fs::read(&out).unwrap(), old);
    assert_eq!(fs::read_to_string(&file).unwrap(), source);
}

#[test]
fn centered_header_exports_both_formats_and_rejects_unsupported_contact_without_overwrite() {
    let root = tempfile::tempdir().unwrap();
    let resume = root.path().join("resume.json");
    let theme = root.path().join("header.theme.json");
    let source = include_bytes!("fixtures/visual-qa.json");
    fs::write(&resume, source).unwrap();
    let mut manifest: Value =
        serde_json::from_slice(include_bytes!("fixtures/centered-header.theme.json")).unwrap();
    fs::write(&theme, serde_json::to_vec(&manifest).unwrap()).unwrap();
    for format in ["pdf", "docx"] {
        let out = root.path().join(format!("resume.{format}"));
        let args = [
            "export",
            "--file",
            path(&resume),
            "--format",
            format,
            "--out",
            path(&out),
            "--theme",
            path(&theme),
            "--json",
        ];
        let (status, response) = call(&args);
        assert!(status.status.success(), "{response}");
        let bytes = fs::read(&out).unwrap();
        assert!(bytes.starts_with(if format == "pdf" { b"%PDF" } else { b"PK" }));
        manifest["composition"]["contact"] = json!("icons");
        fs::write(&theme, serde_json::to_vec(&manifest).unwrap()).unwrap();
        let mut overwrite = args.to_vec();
        overwrite.push("--overwrite");
        let (status, response) = call(&overwrite);
        assert_eq!(status.status.code(), Some(6));
        assert_eq!(response["error"]["code"], "EXPORT_RENDER");
        assert_eq!(fs::read(&out).unwrap(), bytes);
        manifest["composition"]["contact"] = json!("stacked");
        fs::write(&theme, serde_json::to_vec(&manifest).unwrap()).unwrap();
    }
    assert_eq!(fs::read(&resume).unwrap(), source);
}

#[test]
fn section_styles_export_in_both_formats_without_changing_resume() {
    let root = tempfile::tempdir().unwrap();
    let resume = root.path().join("resume.json");
    let theme = root.path().join("sections.theme.json");
    let source = include_bytes!("fixtures/visual-qa.json");
    fs::write(&resume, source).unwrap();
    fs::write(&theme, include_bytes!("fixtures/section-styles.theme.json")).unwrap();
    for format in ["pdf", "docx"] {
        let out = root.path().join(format!("resume.{format}"));
        let (status, response) = call(&[
            "export",
            "--file",
            path(&resume),
            "--format",
            format,
            "--out",
            path(&out),
            "--theme",
            path(&theme),
            "--json",
        ]);
        assert!(status.status.success(), "{response}");
        let bytes = fs::read(&out).unwrap();
        assert!(bytes.starts_with(if format == "pdf" { b"%PDF" } else { b"PK" }));
        if format == "docx" {
            let parsed = docx_rs::read_docx(&bytes).unwrap().json();
            assert!(parsed.contains("52756B") && parsed.contains("工作经历"));
        }
    }
    assert_eq!(fs::read(&resume).unwrap(), source);
    let mut value: Value =
        serde_json::from_slice(include_bytes!("fixtures/section-styles.theme.json")).unwrap();
    value["sections"]["work"]["date"] = serde_json::json!("right");
    fs::write(&theme, serde_json::to_vec(&value).unwrap()).unwrap();
    for format in ["pdf", "docx"] {
        let out = root.path().join(format!("resume.{format}"));
        let previous = fs::read(&out).unwrap();
        let args = [
            "export",
            "--file",
            path(&resume),
            "--format",
            format,
            "--out",
            path(&out),
            "--theme",
            path(&theme),
            "--overwrite",
            "--json",
        ];
        let (status, response) = call(&args);
        assert!(status.status.success(), "{response}");
        let right_date = fs::read(&out).unwrap();
        assert_ne!(right_date, previous);
        value["sections"]["work"]["date"] = serde_json::json!("left");
        fs::write(&theme, serde_json::to_vec(&value).unwrap()).unwrap();
        let (status, response) = call(&args);
        assert!(status.status.success(), "{response}");
        let left_date = fs::read(&out).unwrap();
        assert_ne!(left_date, right_date);
        value["sections"]["work"]["items"] = serde_json::json!("cards");
        fs::write(&theme, serde_json::to_vec(&value).unwrap()).unwrap();
        let (status, response) = call(&args);
        assert_eq!(status.status.code(), Some(6));
        assert_eq!(response["error"]["code"], "EXPORT_RENDER");
        assert_eq!(fs::read(&out).unwrap(), left_date);
        value["sections"]["work"]["items"] = serde_json::json!("compact");
        value["sections"]["work"]["date"] = serde_json::json!("right");
        fs::write(&theme, serde_json::to_vec(&value).unwrap()).unwrap();
    }
    assert_eq!(fs::read(&resume).unwrap(), source);
}

#[test]
fn exports_pdf_with_unicode_and_keeps_source_unchanged() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("resume.json");
    let out = root.path().join("简历.pdf");
    let source = r#"{"basics":{"name":"李明","summary":"跨平台设计"},"work":[{"name":"示例公司","position":"设计师"}]}"#;
    fs::write(&file, source).unwrap();
    let (output, result) = call(&[
        "export",
        "--file",
        path(&file),
        "--format",
        "pdf",
        "--out",
        path(&out),
        "--json",
    ]);
    assert!(output.status.success());
    assert_eq!(result["result"]["format"], "pdf");
    let bytes = fs::read(&out).unwrap();
    assert!(bytes.starts_with(b"%PDF-"));
    assert_eq!(fs::read_to_string(&file).unwrap(), source);
}

#[test]
fn exports_pdf_with_validated_theme_and_rejects_unsupported_layout() {
    let root = tempfile::tempdir().unwrap();
    let resume = root.path().join("resume.json");
    let theme = root.path().join("night.theme.json");
    let pdf = root.path().join("resume.pdf");
    let untouched = r#"{"basics":{"name":"李明","summary":"跨平台产品设计"},"work":[{"position":"设计师","name":"示例公司"}]}"#;
    fs::write(&resume, untouched).unwrap();
    fs::write(&theme, r##"{"formatVersion":2,"id":"night","name":"夜纸","version":"1.0.0","license":"MIT","layout":"minimal","colors":{"accent":"#C9AC77","seal":"#A14F40","paper":"#172426","canvas":"#101A1B"},"modules":["basics","work"],"page":{"size":"Letter","marginsMm":{"top":14,"right":17,"bottom":14,"left":17}},"typography":{"name":{"sizePt":30},"body":{"sizePt":11}}}"##).unwrap();
    let (status, response) = call(&[
        "export",
        "--file",
        path(&resume),
        "--format",
        "pdf",
        "--out",
        path(&pdf),
        "--theme",
        path(&theme),
        "--json",
    ]);
    assert!(status.status.success(), "{response}");
    assert!(fs::read(&pdf).unwrap().starts_with(b"%PDF-"));
    assert_eq!(fs::read_to_string(&resume).unwrap(), untouched);

    let unsupported = root.path().join("sidebar.theme.json");
    fs::write(&unsupported, r##"{"formatVersion":2,"id":"sidebar","name":"Side","version":"1.0.0","license":"MIT","layout":"classic","colors":{"accent":"#345678","seal":"#AA5544","paper":"#FFFFFF","canvas":"#F7F7F7"},"modules":["basics","work"],"composition":{"preset":"sidebar-left","placements":{"work":"sidebar"}}}"##).unwrap();
    let old_pdf = fs::read(&pdf).unwrap();
    let (status, response) = call(&[
        "export",
        "--file",
        path(&resume),
        "--format",
        "pdf",
        "--out",
        path(&pdf),
        "--theme",
        path(&unsupported),
        "--overwrite",
        "--json",
    ]);
    assert_eq!(status.status.code(), Some(6));
    assert_eq!(response["error"]["code"], "EXPORT_RENDER");
    assert_eq!(fs::read(&pdf).unwrap(), old_pdf);

    let (status, response) = call(&[
        "export",
        "--file",
        path(&resume),
        "--format",
        "md",
        "--out",
        path(&pdf),
        "--theme",
        path(&theme),
        "--json",
    ]);
    assert_eq!(status.status.code(), Some(2));
    assert_eq!(response["error"]["code"], "THEME_FORMAT_UNSUPPORTED");
    assert_eq!(fs::read(&pdf).unwrap(), old_pdf);
}
