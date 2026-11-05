use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path, process::Command};

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

const STANDARD_MODULES: [&str; 12] = [
    "basics",
    "work",
    "volunteer",
    "education",
    "awards",
    "certificates",
    "publications",
    "skills",
    "languages",
    "interests",
    "references",
    "projects",
];

fn source_files(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fs::read_dir(root)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            assert!(entry.file_type().unwrap().is_file());
            (
                entry.file_name().into_string().unwrap(),
                fs::read(entry.path()).unwrap(),
            )
        })
        .collect()
}

fn adapt_static_source(files: &[(&str, &str)]) -> Value {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let upstream = root.join("upstream");
    fs::create_dir(&upstream).unwrap();
    fs::write(
        upstream.join("package.json"),
        r#"{"name":"jsonresume-theme-static-regression","version":"1.0.0","license":"MIT","main":"index.js"}"#,
    )
    .unwrap();
    // The CLI must inspect source, including its entrypoint, without running it.
    fs::write(
        upstream.join("index.js"),
        "throw new Error('upstream entrypoint must never execute');",
    )
    .unwrap();
    for (name, contents) in files {
        fs::write(upstream.join(name), contents).unwrap();
    }
    let source_before = source_files(&upstream);
    let fixture = root.join("all-modules.json");
    let original = serde_json::to_vec_pretty(&json!({
        "basics":{"name":"Ada Example","label":"Product Engineer","summary":"A complete resume fixture."},
        "work":[{"name":"Acme","position":"Engineer","summary":"Delivered a product."}],
        "volunteer":[{"organization":"Community","position":"Mentor"}],
        "education":[{"institution":"University","area":"Computing","studyType":"BSc"}],
        "awards":[{"title":"Design Award","awarder":"Design Society"}],
        "certificates":[{"name":"Engineering Certificate","issuer":"Institute"}],
        "publications":[{"name":"Testing Paper","publisher":"Journal"}],
        "skills":[{"name":"Rust","keywords":["Testing"]}],
        "languages":[{"language":"English","fluency":"Native"}],
        "interests":[{"name":"Typography","keywords":["Print"]}],
        "references":[{"name":"A Referee","reference":"Reliable colleague."}],
        "projects":[{"name":"Regression Suite","description":"Preserves all modules."}],
        "custom":{"preserved":true}
    })).unwrap();
    fs::write(&fixture, &original).unwrap();
    let out = root.join("result.theme.json");
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
    assert_eq!(source_files(&upstream), source_before);
    assert_eq!(fs::read(&fixture).unwrap(), original);
    let result = &report["result"];
    assert_eq!(result["adaptationGenerated"], true);
    assert_eq!(result["migrationComplete"], false);
    let adaptation = &result["adaptation"];
    for flag in [
        "executedSourceCode",
        "sourceModified",
        "resumeModified",
        "exactMigrationComplete",
        "visualVerified",
    ] {
        assert_eq!(adaptation[flag], false, "{flag}: {adaptation:#}");
    }
    let candidate = &result["candidate"];
    let modules = candidate["modules"].as_array().unwrap();
    assert_eq!(modules.len(), STANDARD_MODULES.len());
    for module in STANDARD_MODULES {
        assert!(modules.iter().any(|id| id == module), "missing {module}");
    }
    let checks = adaptation["validation"]["checks"].as_array().unwrap();
    assert_eq!(checks.len(), 1);
    assert_eq!(checks[0]["sourceUnchanged"], true);
    assert_eq!(checks[0]["paperBoundsChecked"], true);
    let rendered = checks[0]["sections"].as_array().unwrap();
    for module in &STANDARD_MODULES[1..] {
        assert!(
            rendered.iter().any(|id| id == *module),
            "unrendered {module}: {checks:#?}"
        );
    }
    let written = fs::read(&out).unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&written).unwrap(),
        *candidate
    );
    let parsed = resume_one_core::parse_theme(&written).unwrap();
    resume_one_core::layout::check_theme(&parsed).unwrap();
    report
}

#[test]
fn cli_print_projection_preserves_source_order_and_excludes_screen_and_conditions() {
    let report = adapt_static_source(&[(
        "style.css",
        r#"
        @page { size:A4; margin:18mm; }
        body { font-size:10pt; background:#ffffff; color:#123456; }
        h1 { font-size:21pt; color:#112233; }
        @media print {
            @page { size:Letter; margin-left:22mm; margin-right:21mm; }
            body { font-size:12pt; }
            h1 { font-size:29pt; color:#223344; }
        }
        @page { margin-right:19mm; }
        h1 { color:#345678; }
        @media screen { @page { size:A4; margin-left:31mm; } body { font-size:39pt; } h1 { font-size:41pt; color:#ff0000; } }
        @media print and (min-width:1px) { @page { size:A4; margin-right:32mm; } h1 { font-size:40pt; color:#ee0000; } }
        @supports (display:grid) { @media print { h1 { font-size:38pt; color:#dd0000; } } }
        @media screen { @media print { h1 { font-size:37pt; color:#cc0000; } } }
    "#,
    )]);
    let result = &report["result"];
    let typography = &result["candidate"]["typography"];
    // Print beats earlier ordinary rules; later ordinary rules still beat print.
    assert_eq!(typography["body"]["sizePt"], 12.0);
    assert_eq!(typography["name"]["sizePt"], 29.0);
    assert_eq!(typography["name"]["color"], "#345678");
    assert_eq!(result["candidate"]["page"]["size"], "Letter");
    for (side, expected) in [("left", 22.0), ("right", 19.0)] {
        assert!(
            (result["candidate"]["page"]["marginsMm"][side]
                .as_f64()
                .unwrap()
                - expected)
                .abs()
                < 1e-6,
            "{side}: {}",
            result["candidate"]["page"]
        );
    }
    let profile = &result["adaptation"]["printProfile"];
    assert_eq!(profile["format"], "resume-one-css-print-profile-v1");
    assert_eq!(profile["profile"], "print");
    assert_eq!(profile["executedSourceCode"], false);
    assert_eq!(profile["completeCssCascade"], false);
    assert_eq!(profile["equivalent"], false);
    let coverage = &profile["coverage"];
    assert!(coverage["positiveMediaDeclarations"].as_u64().unwrap() >= 3);
    assert!(coverage["unconditionalReplays"].as_u64().unwrap() >= 1);
    for reason in [
        "nonPrintMedia",
        "unsupportedMediaCondition",
        "unsupportedAtRule",
    ] {
        assert!(
            coverage["skipped"][reason].as_u64().unwrap() > 0,
            "{profile:#}"
        );
    }
    let evidence = profile["evidence"].as_array().unwrap();
    for kind in ["positive-print-media", "unconditional-source-order-replay"] {
        assert!(evidence.iter().any(|e| e["file"] == "style.css"
            && e["projection"]["kind"] == kind
            && e["projection"]["sourceSpanRetained"] == true
            && e["projection"]["sourceOrderPreservedWithinBlock"] == true));
    }
}

#[test]
fn cli_literal_hbs_and_html_headings_map_modules_and_professional_label() {
    for extension in ["hbs", "html"] {
        let markup = r#"
            <main>
              <header id="header"><h1>IDENTITY_NAME</h1><h2>IDENTITY_LABEL</h2></header>
              <!-- <h3>Awards</h3> -->
              <div title="<h3>References</h3>"></div>
              <script>throw new Error('never execute'); const fake = '<h3>Publications</h3>';</script>
              <section><h3>Technical Skills</h3><p>Skills content</p></section>
              <section><h3>Professional Experience</h3><p>Work content</p></section>
              <section><h3>Education</h3><p>Education content</p></section>
            </main>
        "#.replace("IDENTITY_NAME", if extension == "hbs" { "{{basics.name}}" } else { "Ada Example" })
          .replace("IDENTITY_LABEL", if extension == "hbs" { "{{basics.label}}" } else { "Product Engineer" });
        let name = format!("resume.{extension}");
        let report = adapt_static_source(&[
            (&name, &markup),
            (
                "style.css",
                "body{padding:18mm;} h1{font-size:25pt;} #header h2{font-size:18pt;color:#234567;} h3{font-size:16pt;color:#345678;}",
            ),
        ]);
        let result = &report["result"];
        let candidate = &result["candidate"];
        assert_eq!(
            candidate["typography"]["sectionTitle"]["sizePt"], 16.0,
            "{extension}"
        );
        assert_eq!(candidate["typography"]["sectionTitle"]["color"], "#345678");
        assert_eq!(candidate["typography"]["label"]["sizePt"], 18.0);
        assert_eq!(candidate["typography"]["label"]["color"], "#234567");
        assert_ne!(candidate["typography"]["itemTitle"]["sizePt"], 16.0);
        let order = json!(["skills", "work", "education"]);
        assert_eq!(candidate["composition"]["moduleOrder"], order);
        assert_eq!(
            candidate["sections"]["work"]["title"],
            "Professional Experience"
        );
        assert_eq!(candidate["sections"]["skills"]["title"], "Technical Skills");
        let hints = &result["adaptation"]["markupHints"];
        assert_eq!(hints["moduleHeadingTag"], "h3");
        assert_eq!(hints["moduleOrder"], order);
        assert_eq!(hints["orderSource"], name);
        assert_eq!(hints["bindingVerified"], false);
        assert_eq!(hints["equivalent"], false);
        let headings = hints["literalHeadings"].as_array().unwrap();
        assert_eq!(headings.len(), 3, "{hints:#}");
        for (heading, module) in headings.iter().zip(["skills", "work", "education"]) {
            assert_eq!(heading["file"], name);
            assert_eq!(heading["tag"], "h3");
            assert_eq!(heading["module"], module);
            let start = heading["startByte"].as_u64().unwrap() as usize;
            let end = heading["endByte"].as_u64().unwrap() as usize;
            assert_eq!(
                &markup[start..end],
                format!("<h3>{}</h3>", heading["title"].as_str().unwrap())
            );
        }
    }
}

#[test]
fn cli_namespaced_document_font_is_inherited_without_paragraph_style_leaks() {
    let report = adapt_static_source(&[
        (
            "resume.html",
            "<main class='pro'><header><h1 class='pro-name'>Ada Example</h1><h2 class='pro-label'>Product Engineer</h2></header><p class='description'>A paragraph</p></main>",
        ),
        (
            "style.css",
            r#"
            .pro { max-width:800px; padding:18mm; font-family:'Root Serif',serif;
                   font-size:12pt; line-height:1.5; color:#123456; }
            .description { font-family:'Paragraph Sans',sans-serif; font-size:9pt;
                           line-height:1.7; text-transform:uppercase; padding-left:30mm; color:#345678; }
            .pro-name { font-size:25pt; }
            .pro-label { font-size:14pt; }
            .pro-section-title { font-size:16pt; }
        "#,
        ),
    ]);
    let result = &report["result"];
    let typography = &result["candidate"]["typography"];
    assert_eq!(typography["body"]["sizePt"], 9.0);
    assert_eq!(typography["body"]["family"], "cjk-sans");
    assert_eq!(typography["body"]["lineHeight"], 1.7);
    assert_eq!(typography["body"]["textTransform"], "uppercase");
    assert_eq!(typography["name"]["sizePt"], 25.0);
    assert_eq!(typography["label"]["sizePt"], 14.0);
    assert_eq!(typography["sectionTitle"]["sizePt"], 16.0);
    assert_eq!(typography["summary"]["sizePt"], 12.0);
    assert!(
        (result["candidate"]["page"]["marginsMm"]["left"]
            .as_f64()
            .unwrap()
            - 18.0)
            .abs()
            < 1e-6
    );
    let requests = result["adaptation"]["styles"]["fontRequests"]
        .as_array()
        .unwrap();
    let evidence = result["adaptation"]["styles"]["evidence"]
        .as_array()
        .unwrap();
    assert_eq!(requests.len(), 8);
    for request in requests {
        let role = request["role"].as_str().unwrap();
        if role == "body" {
            assert_eq!(request["family"], "Paragraph Sans");
            assert_eq!(request["families"], json!(["Paragraph Sans", "sans-serif"]));
        } else {
            assert_eq!(request["family"], "Root Serif", "{role}");
            assert_eq!(request["families"], json!(["Root Serif", "serif"]));
            // Built-in fallback chooses a real face by weight afterwards;
            // inspect the family projection as well as the original request.
            assert!(
                evidence.iter().any(|e| {
                    e["target"] == format!("/typography/{role}/family")
                        && e["value"] == "cjk-serif"
                        && e["source"]["declaration"]["scope"] == json!([".pro"])
                        && e["equivalent"] == false
                }),
                "{role}"
            );
            assert_eq!(typography[role]["lineHeight"], 1.5, "{role}");
            assert_ne!(typography[role]["textTransform"], "uppercase", "{role}");
            assert_eq!(typography[role]["color"], "#123456", "{role}");
        }
    }
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
