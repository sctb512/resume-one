use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};

fn call(args: &[&str]) -> (i32, Value) {
    let output = Command::new(env!("CARGO_BIN_EXE_resume-one-theme-migrate"))
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    (
        output.status.code().unwrap(),
        serde_json::from_slice(&output.stdout).unwrap(),
    )
}
fn s(path: &Path) -> &str {
    path.to_str().unwrap()
}
fn source(root: &Path) {
    fs::create_dir(root).unwrap();
    fs::write(
        root.join("package.json"),
        r#"{"name":"jsonresume-theme-example","version":"1.0.0","license":"MIT"}"#,
    )
    .unwrap();
    fs::write(root.join("Resume.jsx"),"import styled from 'styled-components'; const Layout=styled.main`font-size:12pt;line-height:1.5;background:#ffffff;color:#223344;`; const Name=styled.h1`font-size:28pt;color:#224466;`; export default ({resume})=><Layout><Name>{resume.basics.name}</Name></Layout>;").unwrap();
}

#[test]
fn explicit_adaptation_preserves_data_and_never_claims_exact_migration() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let upstream = root.join("upstream");
    source(&upstream);
    let fixture = root.join("fixture.json");
    let original=br#"{"basics":{"name":"Ada"},"work":[{"name":"A","position":"Engineer"}],"education":[{"institution":"University"}],"custom":{"preserved":true}}"#;
    fs::write(&fixture, original).unwrap();
    let out = root.join("result.theme.json");
    let source_bytes = fs::read(upstream.join("Resume.jsx")).unwrap();
    let (code, report) = call(&[
        "--source",
        s(&upstream),
        "--adapt",
        "--out",
        s(&out),
        "--verify-fixture",
        s(&fixture),
    ]);
    assert_eq!(code, 0, "{report:#}");
    assert_eq!(report["result"]["adaptationGenerated"], true);
    assert_eq!(report["result"]["migrationComplete"], false);
    assert_eq!(report["result"]["visualVerified"], false);
    assert_eq!(
        report["result"]["candidate"]["modules"]
            .as_array()
            .unwrap()
            .len(),
        12
    );
    assert_eq!(
        report["result"]["adaptation"]["validation"]["checks"][0]["sourceUnchanged"],
        true
    );
    assert_eq!(fs::read(&fixture).unwrap(), original);
    assert_eq!(fs::read(upstream.join("Resume.jsx")).unwrap(), source_bytes);
    let parsed = resume_one_core::parse_theme(&fs::read(&out).unwrap()).unwrap();
    resume_one_core::layout::check_theme(&parsed).unwrap();
    let (code, report) = call(&[
        "--source",
        s(&upstream),
        "--adapt",
        "--require-complete",
        "--out",
        s(&out),
    ]);
    assert_eq!(code, 2);
    assert_eq!(report["error"]["code"], "ARGUMENT_ERROR");
    let (code, report) = call(&["--source", s(&upstream), "--adapt", "--out", s(&out)]);
    assert_eq!(code, 5);
    assert_eq!(report["error"]["code"], "FILE_EXISTS");
}

#[test]
fn batch_writes_complete_packages_and_rejects_duplicates_before_writing() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let upstream = root.join("upstream");
    source(&upstream);
    let catalog = root.join("catalog.json");
    let out = root.join("output");
    let fixture = root.join("fixture.json");
    fs::write(&fixture, b"{}").unwrap();
    let item = json!({"slug":"example","sourceDirectory":upstream,"license":"MIT","upstreamUrl":"https://example.com/source","upstreamRevision":"test-pin"});
    fs::write(
        &catalog,
        serde_json::to_vec(&json!({"items":[item.clone(),item.clone()]})).unwrap(),
    )
    .unwrap();
    let (code, report) = call(&[
        "--source",
        s(&catalog),
        "--out",
        s(&out),
        "--adapt",
        "--batch",
    ]);
    assert_eq!(code, 3);
    assert_eq!(report["error"]["code"], "ADAPT_CATALOG");
    assert!(!out.exists());
    fs::write(
        &catalog,
        serde_json::to_vec(&json!({"items":[item]})).unwrap(),
    )
    .unwrap();
    let (code, report) = call(&[
        "--source",
        s(&catalog),
        "--out",
        s(&out),
        "--adapt",
        "--batch",
        "--verify-fixture",
        s(&fixture),
    ]);
    assert_eq!(code, 0, "{report:#}");
    assert_eq!(report["result"]["adapted"], 1);
    let dir = out.join("community-example");
    assert!(dir.join("theme.json").is_file());
    assert!(dir.join("NOTICE.txt").is_file());
    assert!(dir.join("MIGRATION.json").is_file());
    assert!(!dir.join(".candidate.theme.json").exists());
    let loaded =
        resume_one_core::load_theme(&out, Path::new("community-example/theme.json")).unwrap();
    assert_eq!(loaded.id, "community-example");
    let meta: Value =
        serde_json::from_slice(&fs::read(dir.join("MIGRATION.json")).unwrap()).unwrap();
    assert_eq!(meta["exactMigrationComplete"], false);
    assert_eq!(meta["executedSourceCode"], false);
    let (code, _) = call(&[
        "--source",
        s(&catalog),
        "--out",
        s(&out),
        "--adapt",
        "--batch",
    ]);
    assert_eq!(code, 5);
}

#[cfg(unix)]
#[test]
fn adaptation_rejects_symlink_sources_and_does_not_follow_them() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let upstream = root.join("upstream");
    source(&upstream);
    let link = root.join("link");
    std::os::unix::fs::symlink(&upstream, &link).unwrap();
    let out = root.join("result.theme.json");
    let (code, report) = call(&["--source", s(&link), "--adapt", "--out", s(&out)]);
    assert_eq!(code, 3);
    assert_eq!(report["error"]["code"], "ADAPT_SYMLINK");
    assert!(!out.exists());
}

#[test]
fn bounded_parallel_batch_retains_catalog_order_and_reports_partial_failure() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let upstream = root.join("upstream");
    source(&upstream);
    let catalog = root.join("catalog.json");
    let out = root.join("output");
    fs::write(
        &catalog,
        serde_json::to_vec(&json!({"items":[
            {"slug":"second","sourceDirectory":upstream},
            {"slug":"first","sourceDirectory":root.join("missing-source")},
            {"slug":"third","sourceDirectory":upstream}
        ]}))
        .unwrap(),
    )
    .unwrap();
    let (code, report) = call(&[
        "--source",
        s(&catalog),
        "--out",
        s(&out),
        "--adapt",
        "--batch",
        "--jobs",
        "2",
    ]);
    assert_eq!(code, 3);
    assert_eq!(report["error"]["code"], "ADAPT_BATCH_PARTIAL");
    let summary = &report["error"]["inspection"];
    assert_eq!(summary["adapted"], 2);
    assert_eq!(summary["failed"], 1);
    assert_eq!(
        summary["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["slug"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["second", "first", "third"]
    );
    assert!(!out.join("community-first/theme.json").exists());
    assert!(out.join("community-second/theme.json").exists());
    assert!(out.join("community-third/theme.json").exists());
}
