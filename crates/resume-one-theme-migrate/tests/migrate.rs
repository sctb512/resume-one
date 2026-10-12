use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use resume_one_core::parse_theme;
use serde_json::Value;

const V1: &str = r##"{"formatVersion":1,"id":"quiet-ink","name":"静墨","layout":"minimal","colors":{"accent":"#456A5D","seal":"#A14F40","paper":"#FFFFFF","canvas":"#F5F3EE"},"modules":["basics","work","skills"]}"##;

fn call(args: &[&str]) -> (Output, Value) {
    let output = Command::new(env!("CARGO_BIN_EXE_resume-one-theme-migrate"))
        .args(args)
        .output()
        .unwrap();
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout.clone()).unwrap();
    assert_eq!(stdout.lines().count(), 1, "{stdout}");
    let value = serde_json::from_str(&stdout).unwrap();
    (output, value)
}

fn path(path: &Path) -> &str {
    path.to_str().unwrap()
}

#[test]
fn upgrades_v1_without_touching_source_or_claiming_review() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("original.theme.json");
    let out = root.path().join("quiet-ink.theme.json");
    fs::write(&source, V1).unwrap();
    let (status, report) = call(&[
        "--source",
        path(&source),
        "--out",
        path(&out),
        "--license",
        "MIT",
        "--upstream-url",
        "https://example.com/themes/quiet-ink",
        "--upstream-revision",
        "abc123",
    ]);
    assert!(status.status.success(), "{report}");
    assert_eq!(report["result"]["written"], true);
    assert_eq!(report["result"]["sourceVersion"], 1);
    assert_eq!(report["result"]["targetVersion"], 2);
    assert_eq!(report["result"]["licenseDeclared"], true);
    assert_eq!(report["result"]["licenseVerified"], false);
    assert_eq!(report["result"]["visualVerified"], false);
    assert_eq!(fs::read_to_string(&source).unwrap(), V1);
    let migrated = fs::read(&out).unwrap();
    let theme = parse_theme(&migrated).unwrap();
    assert_eq!(theme.source_version, 2);
    assert_eq!(theme.id, "quiet-ink");
    assert_eq!(theme.normalized["license"], "MIT");
    assert_eq!(theme.normalized["provenance"]["upstreamRevision"], "abc123");
    assert_eq!(theme.modules, ["basics", "work", "skills"]);
    assert_eq!(report["result"]["candidate"], theme.normalized);
}

#[test]
fn dry_run_and_no_clobber_are_safe() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.theme.json");
    let out = root.path().join("new.theme.json");
    fs::write(&source, V1).unwrap();
    let (status, report) = call(&["--source", path(&source), "--out", path(&out), "--dry-run"]);
    assert!(status.status.success());
    assert_eq!(report["result"]["written"], false);
    assert_eq!(report["result"]["licenseDeclared"], false);
    assert!(!out.exists());
    fs::write(&out, "do not replace").unwrap();
    let (status, error) = call(&["--source", path(&source), "--out", path(&out)]);
    assert_eq!(status.status.code(), Some(5));
    assert_eq!(error["error"]["code"], "FILE_EXISTS");
    assert_eq!(fs::read_to_string(&out).unwrap(), "do not replace");
    let (status, _) = call(&[
        "--source",
        path(&source),
        "--out",
        path(&out),
        "--overwrite",
    ]);
    assert!(status.status.success());
    assert!(parse_theme(&fs::read(&out).unwrap()).is_ok());
    assert_eq!(fs::read_to_string(&source).unwrap(), V1);
}

#[test]
fn rejects_unsafe_paths_and_invalid_metadata_without_writing() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.theme.json");
    let out = root.path().join("new.theme.json");
    fs::write(&source, V1).unwrap();
    let (status, error) = call(&["--source", "relative.json", "--out", path(&out)]);
    assert_eq!(status.status.code(), Some(2));
    assert_eq!(error["error"]["code"], "PATH_NOT_ABSOLUTE");
    let (status, error) = call(&[
        "--source",
        path(&source),
        "--out",
        path(&out),
        "--upstream-url",
        "http://insecure.example.com",
    ]);
    assert_eq!(status.status.code(), Some(3));
    assert_eq!(error["error"]["code"], "THEME_INVALID");
    assert!(!out.exists());
    let (status, error) = call(&[
        "--source",
        path(&source),
        "--out",
        path(&source),
        "--overwrite",
    ]);
    assert_eq!(status.status.code(), Some(2));
    assert_eq!(error["error"]["code"], "SAME_FILE");
    assert_eq!(fs::read_to_string(&source).unwrap(), V1);
}

#[test]
fn does_not_rewrite_already_v2_or_invalid_theme() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.theme.json");
    let out = root.path().join("new.theme.json");
    let v2 = V1
        .replace("\"formatVersion\":1", "\"formatVersion\":2")
        .replace(
            "\"layout\":",
            "\"version\":\"1.0.0\",\"license\":\"MIT\",\"layout\":",
        );
    fs::write(&source, &v2).unwrap();
    let (status, error) = call(&["--source", path(&source), "--out", path(&out)]);
    assert_eq!(status.status.code(), Some(3));
    assert_eq!(error["error"]["code"], "ALREADY_V2");
    assert!(!out.exists());
    fs::write(&source, "not json").unwrap();
    let (status, error) = call(&["--source", path(&source), "--out", path(&out)]);
    assert_eq!(status.status.code(), Some(3));
    assert_eq!(error["error"]["code"], "THEME_INVALID");
    assert!(!out.exists());
}
