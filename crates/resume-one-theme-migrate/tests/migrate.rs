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
fn compiles_an_explicit_snapshot_locked_template_entry_without_claiming_theme_migration() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    fs::create_dir(root.join("src")).unwrap();
    let text = "export default function Resume({resume}) {const {basics={},work=[]}=resume;return <main><h1>{basics?.name}</h1>{work.length > 0 && work.map((job,index)=><section key={index}><h2>{job.position}</h2></section>)}</main>;}";
    fs::write(root.join("src/Resume.jsx"), text).unwrap();
    let (status, report) = call(&[
        "--source",
        path(&root),
        "--inspect",
        "--template-entry",
        "src/Resume.jsx",
    ]);
    assert!(status.status.success(), "{report}");
    let result = &report["result"];
    let program = &result["templatePrograms"][0];
    assert_eq!(program["entryFile"], "src/Resume.jsx");
    assert_eq!(program["sourceSha256"].as_str().unwrap().len(), 64);
    assert_eq!(program["program"]["format"], "resume-one-template-ir-v1");
    assert_eq!(program["program"]["resolved"], true);
    assert_eq!(program["program"]["syntaxValid"], true);
    assert!(!program["program"]["entry"].is_null());
    assert_eq!(result["migrationComplete"], false);
    assert_eq!(result["visualVerified"], false);
    assert_eq!(result["executedSourceCode"], false);
    assert_eq!(
        fs::read_to_string(root.join("src/Resume.jsx")).unwrap(),
        text
    );
    let (status, report) = call(&[
        "--source",
        path(&root),
        "--inspect",
        "--template-entry",
        "src/Resume.jsx",
        "--require-complete",
    ]);
    assert_eq!(status.status.code(), Some(3));
    assert_eq!(report["error"]["code"], "MIGRATION_INCOMPLETE");
    assert_eq!(
        report["error"]["inspection"]["templatePrograms"][0]["program"]["resolved"],
        true
    );
}

#[test]
fn template_entry_errors_and_unknown_semantics_are_explicit_and_never_read_outside_snapshot() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    fs::write(
        root.join("Resume.jsx"),
        "export default ({resume})=><p>{run(resume)}</p>;",
    )
    .unwrap();
    let (status, report) = call(&[
        "--source",
        path(&root),
        "--inspect",
        "--template-entry",
        "Resume.jsx",
    ]);
    assert!(status.status.success());
    assert_eq!(
        report["result"]["templatePrograms"][0]["program"]["resolved"],
        false
    );
    assert!(
        report["result"]["blockers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|b| b["code"] == "UNSUPPORTED_CALL" && b["stage"] == "template-ir")
    );
    for entry in [
        "../private.jsx",
        "/absolute.jsx",
        "src/./Resume.jsx",
        "src\\Resume.jsx",
        "C:/private.jsx",
    ] {
        let (status, report) = call(&[
            "--source",
            path(&root),
            "--inspect",
            "--template-entry",
            entry,
        ]);
        assert_eq!(status.status.code(), Some(2), "{report}");
        assert_eq!(report["error"]["code"], "TEMPLATE_ENTRY_PATH");
    }
    let (status, report) = call(&[
        "--source",
        path(&root),
        "--inspect",
        "--template-entry",
        "absent.jsx",
    ]);
    assert_eq!(status.status.code(), Some(3));
    assert_eq!(report["error"]["code"], "TEMPLATE_ENTRY_NOT_FOUND");
    let (status, report) = call(&[
        "--source",
        path(&root),
        "--inspect",
        "--template-entry",
        "Resume.jsx",
        "--template-entry",
        "Resume.jsx",
    ]);
    assert_eq!(status.status.code(), Some(2));
    assert_eq!(report["error"]["code"], "TEMPLATE_ENTRY_PATH");
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

#[test]
fn inspection_records_full_source_and_real_syntax_without_executing_or_claiming_migration() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let source = root.join("source");
    fs::create_dir_all(source.join("src/deep/components")).unwrap();
    fs::create_dir(source.join("fonts")).unwrap();
    fs::write(
        source.join("package.json"),
        r#"{"name":"jsonresume-theme-example","version":"1.0.0","license":"MIT"}"#,
    )
    .unwrap();
    fs::write(source.join("LICENSE.txt"), "MIT test fixture license").unwrap();
    fs::write(source.join("fonts/face.ttf"), [0, 1, 255]).unwrap();
    fs::write(
        source.join("src/deep/components/Resume.jsx"),
        r#"import styled from 'styled-components';
import {DateRange} from '@jsonresume/core';
const Name=styled.h1`font-size: 28pt; @media print {margin: 0;} &:hover {color: #123456;}`;
const fake='const Wrong=styled.div`color:red;`';
function Resume({basics}) {return <Name>{basics.name}</Name>;}
require('fs').writeFileSync('SHOULD_NOT_EXECUTE','bad');"#,
    )
    .unwrap();
    let (status, response) = call(&["--inspect", "--source", path(&source)]);
    assert!(status.status.success(), "{response}");
    let report = &response["result"];
    assert_eq!(report["statistics"]["sourceFiles"], 4);
    assert_eq!(report["statistics"]["styledComponents"], 1);
    assert_eq!(report["statistics"]["cssDeclarations"], 3);
    assert_eq!(report["sourceSnapshot"]["complete"], true);
    assert_eq!(report["migrationComplete"], false);
    assert_eq!(report["visualVerified"], false);
    assert_eq!(report["executedSourceCode"], false);
    assert_eq!(report["licenseVerified"], false);
    assert!(
        report["syntaxEvidence"][0]["styledComponents"][0]["css"]["declarations"][1]["scope"]
            == serde_json::json!(["@media print"])
    );
    assert!(
        report["blockers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|b| b["code"] == "EXTERNAL_DEPENDENCIES_UNRESOLVED")
    );
    assert!(!source.join("SHOULD_NOT_EXECUTE").exists());
    assert!(!root.join("SHOULD_NOT_EXECUTE").exists());
    assert_eq!(
        fs::read(source.join("fonts/face.ttf")).unwrap(),
        [0, 1, 255]
    );
    let original = report["sourceSnapshot"]["sourceSha256"].clone();
    fs::write(source.join("fonts/face.ttf"), [0, 2, 255]).unwrap();
    let (_, second) = call(&["--inspect", "--source", path(&source)]);
    assert_ne!(second["result"]["sourceSnapshot"]["sourceSha256"], original);
}

#[test]
fn strict_migration_never_writes_or_overwrites_an_incomplete_result() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let source = root.join("source");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("style.css"), "p {color: #123456;}").unwrap();
    let out = root.join("candidate.theme.json");
    let (status, response) = call(&["--inspect", "--source", path(&source), "--require-complete"]);
    assert_eq!(status.status.code(), Some(3));
    assert_eq!(response["error"]["code"], "MIGRATION_INCOMPLETE");
    assert_eq!(response["error"]["inspection"]["migrationComplete"], false);
    assert!(!out.exists());
    fs::write(&out, "EXISTING").unwrap();
    let (status, response) = call(&[
        "--source",
        path(&source),
        "--out",
        path(&out),
        "--overwrite",
        "--require-complete",
    ]);
    assert_eq!(status.status.code(), Some(3));
    assert_eq!(response["error"]["code"], "MIGRATION_INCOMPLETE");
    assert_eq!(fs::read_to_string(&out).unwrap(), "EXISTING");
    let (status, response) = call(&["--inspect", "--source", path(&source), "--out", path(&out)]);
    assert_eq!(status.status.code(), Some(2));
    assert_eq!(response["error"]["code"], "ARGUMENT_ERROR");
}

#[test]
fn inspection_exposes_missing_local_dependencies_dynamic_styles_and_unsupported_templates() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().canonicalize().unwrap();
    fs::write(source.join("Resume.jsx"),r#"import styled from 'styled-components'; import './missing.css'; const A=styled.div`color:${props=>props.color};`;"#).unwrap();
    fs::write(source.join("layout.hbs"), "{{#each work}}{{name}}{{/each}}").unwrap();
    let (status, response) = call(&["--inspect", "--source", path(&source)]);
    assert!(status.status.success(), "{response}");
    let codes: Vec<_> = response["result"]["blockers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["code"].as_str().unwrap())
        .collect();
    for expected in [
        "DYNAMIC_STYLE_UNRESOLVED",
        "LOCAL_DEPENDENCY_UNRESOLVED",
        "FORMAT_ADAPTER_REQUIRED",
        "DECLARATIVE_COMPILER_REQUIRED",
        "VISUAL_ACCEPTANCE_REQUIRED",
    ] {
        assert!(codes.contains(&expected), "{response}");
    }
}

#[cfg(unix)]
#[test]
fn inspection_rejects_a_symlink_root_instead_of_canonicalizing_it_away() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let source = root.join("source");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("style.css"), "p {color:red;}").unwrap();
    let link = root.join("linked");
    std::os::unix::fs::symlink(&source, &link).unwrap();
    let (status, response) = call(&["--inspect", "--source", path(&link)]);
    assert_eq!(status.status.code(), Some(3));
    assert_eq!(response["error"]["code"], "SOURCE_SYMLINK");
}

#[test]
fn extracts_third_party_package_without_executing_its_code() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("external-theme");
    fs::create_dir(&source).unwrap();
    fs::create_dir(source.join("src")).unwrap();
    fs::write(
        source.join("package.json"),
        r#"{"name":"@people/jsonresume-theme-moss","version":"2.3.4","license":"MIT"}"#,
    )
    .unwrap();
    fs::write(
        source.join("src/style.css"),
        "body { background: #fff; color: rgb(69, 106, 93); font-family: 'Noto Serif SC', serif } h1 { color: #456A5D } a { color: #123456 } .resume { display: grid; grid-template-columns: 1fr 2fr }",
    )
    .unwrap();
    fs::write(
        source.join("index.js"),
        "require('fs').writeFileSync('EXECUTED', 'bad'); const color = '#ABC';",
    )
    .unwrap();
    let out = root.path().join("moss.theme.json");
    let (status, report) = call(&["--source", path(&source), "--out", path(&out)]);
    assert!(status.status.success(), "{report}");
    assert_eq!(report["result"]["sourceKind"], "third-party-static");
    assert!(report["result"]["sourceVersion"].is_null());
    assert_eq!(report["result"]["licenseDeclared"], true);
    assert_eq!(report["result"]["licenseVerified"], false);
    assert_eq!(report["result"]["visualVerified"], false);
    assert_eq!(report["result"]["extraction"]["confidence"], "low");
    assert_eq!(report["result"]["extraction"]["executedSourceCode"], false);
    assert_eq!(
        report["result"]["extraction"]["executableSourcePresent"],
        true
    );
    assert_eq!(report["result"]["extraction"]["selectedAccent"], "#456A5D");
    assert_eq!(
        report["result"]["extraction"]["colorsFound"][0]["occurrences"],
        2
    );
    assert_eq!(
        report["result"]["extraction"]["fontFamiliesFound"][0]["declaration"],
        "'Noto Serif SC', serif"
    );
    assert_eq!(
        report["result"]["extraction"]["layoutSignals"],
        serde_json::json!(["grid", "grid-columns"])
    );
    assert!(report["result"]["candidate"]["typography"].is_null());
    assert!(report["result"]["candidate"]["composition"].is_null());
    assert!(!source.join("EXECUTED").exists());
    assert!(!root.path().join("EXECUTED").exists());
    let theme = parse_theme(&fs::read(&out).unwrap()).unwrap();
    assert_eq!(theme.id, "moss");
    assert_eq!(theme.normalized["version"], "2.3.4");
    assert_eq!(theme.normalized["license"], "MIT");
    assert_eq!(theme.normalized["provenance"]["fidelity"], "inspired");
    assert_eq!(theme.normalized["provenance"]["upstreamLicense"], "MIT");
}

#[test]
fn static_extraction_skips_untrusted_links_and_oversized_files() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("base.css"), "h1 { color: #235E6A; }").unwrap();
    fs::write(source.join("huge.css"), vec![b'x'; 524_289]).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(root.path().join("outside.css"), source.join("escape.css")).unwrap();
    fs::write(root.path().join("outside.css"), "body { color: #FF0000; }").unwrap();
    let out = root.path().join("safe.theme.json");
    let (status, report) = call(&["--source", path(&source), "--out", path(&out), "--dry-run"]);
    assert!(status.status.success(), "{report}");
    assert_eq!(report["result"]["written"], false);
    assert!(!out.exists());
    assert_eq!(report["result"]["extraction"]["selectedAccent"], "#235E6A");
    assert_eq!(
        report["result"]["extraction"]["filesScanned"],
        serde_json::json!(["base.css"])
    );
    assert!(
        !report["result"]["extraction"]["filesSkipped"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(report["result"]["candidate"]["license"], "NOASSERTION");
}

#[test]
fn static_extraction_requires_real_style_files_and_valid_package_json() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    fs::create_dir(&source).unwrap();
    let out = root.path().join("candidate.theme.json");
    let (status, error) = call(&["--source", path(&source), "--out", path(&out)]);
    assert_eq!(status.status.code(), Some(3));
    assert_eq!(error["error"]["code"], "NO_STYLE_FILES");
    fs::write(source.join("theme.css"), "h1 { color: #123456 }").unwrap();
    fs::write(source.join("package.json"), "not json").unwrap();
    let (status, error) = call(&["--source", path(&source), "--out", path(&out)]);
    assert_eq!(status.status.code(), Some(3));
    assert_eq!(error["error"]["code"], "PACKAGE_JSON");
    assert!(!out.exists());
}
