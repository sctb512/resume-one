//! Explicit best-effort adaptation, distinct from exact content compilation.
//! Output contains only existing bounded renderer constructs; no upstream code runs.
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, Path};

use resume_one_core::{layout, parse_theme, render::Language, resume, semantic};
use serde_json::{Value, json};

use crate::{AppError, Cli, absolute, adapt_fonts, adapt_style, inspection, sha256, source_bundle};

const MODULES: &[&str] = &[
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
const MAX_CATALOG: u64 = 4 * 1024 * 1024;

pub(crate) fn safe_path(path: &Path) -> Result<(), AppError> {
    absolute(path)?;
    if path
        .components()
        .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
    {
        return Err(AppError::new(2, "ADAPT_PATH", "路径不能包含 . 或 .."));
    }
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(AppError::new(
                    3,
                    "ADAPT_SYMLINK",
                    "来源、目标和父路径不能是符号链接",
                ));
            }
            Ok(_) => (),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(AppError::new(5, "ADAPT_IO", e.to_string())),
        }
    }
    Ok(())
}

fn read(path: &Path, max: u64) -> Result<Vec<u8>, AppError> {
    safe_path(path)?;
    let before = fs::metadata(path).map_err(|e| AppError::new(5, "ADAPT_IO", e.to_string()))?;
    if !before.is_file() || before.len() > max {
        return Err(AppError::new(
            3,
            "ADAPT_LIMIT",
            "文件不是普通文件或超过读取上限",
        ));
    }
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|f| f.take(max + 1).read_to_end(&mut bytes))
        .map_err(|e| AppError::new(5, "ADAPT_IO", e.to_string()))?;
    let after = fs::metadata(path).map_err(|e| AppError::new(5, "ADAPT_IO", e.to_string()))?;
    if bytes.len() as u64 > max
        || before.len() != after.len()
        || before.modified().ok() != after.modified().ok()
    {
        return Err(AppError::new(
            3,
            "ADAPT_CHANGED",
            "读取期间文件改变，未采用不完整输入",
        ));
    }
    Ok(bytes)
}

fn portable_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 53
        && id.as_bytes()[0].is_ascii_alphanumeric()
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

struct Plan {
    manifest: Value,
    report: Value,
    files: Vec<(String, Vec<u8>)>,
}

fn public_url(value: &str) -> Option<String> {
    let value = value.trim().strip_prefix("git+").unwrap_or(value.trim());
    (value.len() <= 2048
        && (value.starts_with("https://") || value.starts_with("http://"))
        && !value.chars().any(|c| c.is_whitespace() || c.is_control()))
    .then(|| value.to_owned())
}

fn author_name(package: &Value) -> Option<String> {
    let author = &package["authorDeclaration"];
    let name = author
        .as_str()
        .map(|s| s.split('<').next().unwrap_or(""))
        .or_else(|| author["name"].as_str())?;
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() || name.chars().count() > 100 || name.chars().any(char::is_control) {
        return None;
    }
    if let Some(url) = author["url"].as_str().and_then(public_url) {
        let full = format!("{name} ({url})");
        if full.chars().count() <= 100 {
            return Some(full);
        }
    }
    Some(name)
}

fn repository_url(package: &Value) -> Option<String> {
    let repo = &package["repositoryDeclaration"];
    repo.as_str()
        .or_else(|| repo["url"].as_str())
        .and_then(public_url)
}

fn plan(
    source: &Path,
    slug: Option<&str>,
    license: Option<&str>,
    url: Option<&str>,
    revision: Option<&str>,
    font_library: Option<&Path>,
) -> Result<Plan, AppError> {
    safe_path(source)?;
    let mut report = inspection::inspect_templates(source, &[])?;
    crate::embedded_css::augment(&mut report, source)?;
    crate::css_print::project(&mut report)
        .map_err(|e| AppError::new(3, "ADAPT_PRINT_PROFILE", e))?;
    let package_name = report["package"]["name"]
        .as_str()
        .unwrap_or("migrated-theme");
    let inferred = package_name
        .rsplit('/')
        .next()
        .unwrap_or(package_name)
        .strip_prefix("jsonresume-theme-")
        .unwrap_or(package_name);
    let id = slug.unwrap_or(inferred);
    if !portable_id(id) {
        return Err(AppError::new(
            3,
            "ADAPT_ID",
            "主题标识必须为不超过 53 字符的小写 ASCII 名称",
        ));
    }
    let title = id
        .split('-')
        .map(|s| {
            let mut c = s.chars();
            c.next()
                .map(|a| a.to_ascii_uppercase().to_string() + c.as_str())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ");
    let license = license
        .or_else(|| report["package"]["licenseDeclaration"].as_str())
        .unwrap_or("NOASSERTION");
    let candidate = json!({"formatVersion":2,"id":format!("community-{id}"),"name":title,"description":"静态迁移适配版；已支持的视觉规则尽量保留，差异见同目录 MIGRATION.json。","version":"1.0.0","license":license,"layout":"classic","colors":{"accent":"#334155","seal":"#334155","paper":"#FFFFFF","canvas":"#F5F3EE","ink":"#1F2937","muted":"#64748B","border":"#CBD5E1"},"modules":MODULES});
    let (mut manifest, style_report) =
        adapt_style::adapt(&report, candidate).map_err(|e| AppError::new(3, "ADAPT_STYLE", e))?;
    // Never use module omission as a shortcut to matching the source picture.
    manifest["modules"] = json!(MODULES);
    let mut provenance = json!({"migratedBy":"resume-one-theme-migrate --adapt","fidelity":"adapted","upstreamLicense":license});
    if let Some(author) = author_name(&report["package"]) {
        provenance["upstreamAuthor"] = json!(author);
    }
    if let Some(url) = url {
        provenance["upstreamUrl"] = json!(url);
    }
    if let Some(revision) = revision {
        provenance["upstreamRevision"] = json!(revision);
    }
    manifest["provenance"] = provenance;
    let (font_report, files) = if let Some(library) = font_library {
        safe_path(library)?;
        adapt_fonts::apply(&mut manifest, &style_report["fontRequests"], library)
            .map_err(|e| AppError::new(3, "ADAPT_FONTS", e))?
    } else {
        (
            json!({"fontMappings":[],"differences":[{"code":"ORIGINAL_FONTS_NOT_BUNDLED","message":"未提供静态字体资料库，使用明确的内置字面回退；字体与换行可能不同"}]}),
            Vec::new(),
        )
    };
    let entry_fonts = adapt_fonts::apply_entry_rows(
        &mut manifest,
        &style_report["entryHeaderFonts"],
        &font_report,
    )
    .map_err(|e| AppError::new(3, "ADAPT_ENTRY_FONTS", e))?;
    parse_theme(&serde_json::to_vec(&manifest).expect("JSON value"))
        .map_err(AppError::from_theme)?;
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    for issue in report["blockers"].as_array().into_iter().flatten() {
        *counts
            .entry(issue["code"].as_str().unwrap_or("UNKNOWN").into())
            .or_default() += 1;
    }
    let adaptation = json!({"format":"resume-one-adaptation-v1","policy":"best-effort-explicit-differences","source":source,"sourceSnapshot":report["sourceSnapshot"],"sourcePackage":report["package"],"executedSourceCode":false,"sourceModified":false,"resumeModified":false,"exactMigrationComplete":false,"visualVerified":false,"licenseVerified":false,"requiresReview":true,"sourceBlockerCounts":counts,"sourceBlockers":report["blockers"],"styles":style_report,"fonts":font_report,"contentPolicy":"all standard JSON Resume modules preserved using shared renderer; original custom helpers, field suppression and unsupported visual constructs are not executed","differences":[{"code":"SEMANTIC_RENDERER_ADAPTATION","message":"内容由安全标准模块渲染器呈现；不声称自定义模板逻辑、字段呈现或原版分页完全相同"},{"code":"VISUAL_COMPARISON_NOT_PERFORMED","message":"自动布局与导出检查不代替原版逐主题视觉对照"}]});
    let mut adaptation = adaptation;
    adaptation["embeddedStyles"] = report["embeddedStyles"].clone();
    adaptation["utilityStyles"] = report["utilityStyles"].clone();
    adaptation["markupHints"] = report["markupHints"].clone();
    adaptation["printProfile"] = report["printProfile"].clone();
    adaptation["entryFonts"] = entry_fonts;
    adaptation["templateInspection"] = report["adaptTemplateInspection"].clone();
    Ok(Plan {
        manifest,
        report: adaptation,
        files,
    })
}

fn verify(
    manifest: &resume_one_core::ThemeManifest,
    fixtures: &[std::path::PathBuf],
    exports: bool,
) -> Result<Value, AppError> {
    layout::check_theme(manifest).map_err(|e| AppError::new(3, "ADAPT_RENDERER", e))?;
    if fixtures.len() > 8 {
        return Err(AppError::new(
            2,
            "ADAPT_FIXTURE_LIMIT",
            "每次最多验证 8 份明确样本",
        ));
    }
    let mut checks = Vec::new();
    for path in fixtures {
        let bytes = read(path, 1024 * 1024)?;
        let document = resume::parse_resume(&bytes).map_err(|issues| {
            AppError::new(3, "ADAPT_FIXTURE", format!("样本校验失败：{issues:?}"))
        })?;
        let content = semantic::project(&document);
        let lang = if std::str::from_utf8(&bytes)
            .unwrap_or("")
            .chars()
            .any(|c| ('\u{3400}'..='\u{9fff}').contains(&c))
        {
            Language::Zh
        } else {
            Language::En
        };
        let rendered = layout::layout_with_theme(&content, lang, manifest)
            .map_err(|e| AppError::new(3, "ADAPT_LAYOUT", e))?;
        let mut texts = 0;
        let mut anchors = BTreeSet::new();
        let width = rendered.width_mm * 72.0 / 25.4;
        let height = rendered.height_mm * 72.0 / 25.4;
        for element in rendered.pages.iter().flatten() {
            match element {
                layout::LayoutElement::Text(line) => {
                    texts += 1;
                    let (ascent, descent) = line.font.vertical_metrics();
                    if !line.x_pt.is_finite()
                        || !line.baseline_y_pt.is_finite()
                        || !line.width_pt().is_finite()
                        || line.x_pt < -1.0
                        || line.x_pt + line.width_pt() > width + 1.0
                        || line.baseline_y_pt - descent * line.size_pt < -1.0
                        || line.baseline_y_pt + ascent * line.size_pt > height + 1.0
                    {
                        return Err(AppError::new(
                            3,
                            "ADAPT_OVERFLOW",
                            format!("{}：文字超出纸面或坐标无效", path.display()),
                        ));
                    }
                    if let Some(target) = line.target {
                        anchors.insert(target.section.to_string());
                    }
                }
                layout::LayoutElement::Anchor(a) => {
                    anchors.insert(a.target.section.to_string());
                }
                _ => (),
            }
        }
        for section in &content.sections {
            if !section.items.is_empty() && !anchors.contains(section.id) {
                return Err(AppError::new(
                    3,
                    "ADAPT_CONTENT_MISSING",
                    format!("样本中的 {} 模块未产生渲染目标", section.id),
                ));
            }
        }
        let mut check = json!({"fixture":path,"sha256":sha256(&bytes),"schemaValid":true,"pages":rendered.pages.len(),"textLines":texts,"sections":anchors,"paperBoundsChecked":true,"sourceUnchanged":read(path,1024*1024)?==bytes,"pdfSerialized":false,"docxSerialized":false});
        if exports {
            let pdf = resume_one_core::pdf::render_with_theme(&content, lang, manifest)
                .map_err(|e| AppError::new(3, "ADAPT_PDF", e))?;
            let docx = resume_one_core::docx::render_with_theme(&content, lang, manifest)
                .map_err(|e| AppError::new(3, "ADAPT_DOCX", e))?;
            if !pdf.starts_with(b"%PDF-") || !docx.starts_with(b"PK") {
                return Err(AppError::new(3, "ADAPT_EXPORT", "序列化格式不符合预期"));
            }
            check["pdfSerialized"] = json!(true);
            check["docxSerialized"] = json!(true);
            check["pdfBytes"] = json!(pdf.len());
            check["docxBytes"] = json!(docx.len());
        }
        checks.push(check);
    }
    Ok(
        json!({"rendererSupported":true,"fixtureCount":checks.len(),"checks":checks,"visualVerified":false,"nativeWordVerified":false}),
    )
}

pub(crate) fn single(cli: &Cli) -> Result<Value, AppError> {
    let out = cli
        .out
        .as_ref()
        .ok_or_else(|| AppError::new(2, "ARGUMENT_ERROR", "需要 --out"))?;
    safe_path(out)?;
    if !out
        .file_name()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.ends_with(".theme.json") || s == "theme.json")
    {
        return Err(AppError::new(
            2,
            "OUTPUT_NAME",
            "目标须为 .theme.json 或 theme.json",
        ));
    }
    // A single-file output cannot safely materialize font resources beside an existing file.
    if cli.font_library.is_some() {
        return Err(AppError::new(
            2,
            "ADAPT_FONT_PACKAGE",
            "带字体资源的适配请使用 --batch 生成完整目录包",
        ));
    }
    let mut planned = plan(
        &cli.source,
        None,
        cli.license.as_deref(),
        cli.upstream_url.as_deref(),
        cli.upstream_revision.as_deref(),
        None,
    )?;
    let parsed = parse_theme(&serde_json::to_vec(&planned.manifest).unwrap())
        .map_err(AppError::from_theme)?;
    planned.report["validation"] = verify(&parsed, &cli.verify_fixture, cli.verify_exports)?;
    let after =
        source_bundle::snapshot(&cli.source).map_err(|e| AppError::new(3, e.code, e.message))?;
    if after.source_sha256 != planned.report["sourceSnapshot"]["sourceSha256"] {
        return Err(AppError::new(3, "ADAPT_CHANGED", "迁移期间来源发生变化"));
    }
    let bytes = serde_json::to_vec_pretty(&planned.manifest).unwrap();
    if !cli.dry_run {
        resume_one_core::export::write_export(&cli.source, out, &bytes, cli.overwrite)
            .map_err(|e| AppError::new(5, e.code, e.message))?;
    }
    Ok(
        json!({"adaptation":planned.report,"candidate":planned.manifest,"candidateSha256":sha256(&bytes),"out":out,"written":!cli.dry_run,"migrationComplete":false,"adaptationGenerated":true,"visualVerified":false}),
    )
}

pub(crate) fn batch(cli: &Cli) -> Result<Value, AppError> {
    let out = cli
        .out
        .as_ref()
        .ok_or_else(|| AppError::new(2, "ARGUMENT_ERROR", "需要 --out 目录"))?;
    safe_path(out)?;
    let bytes = read(&cli.source, MAX_CATALOG)?;
    let catalog: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::new(3, "ADAPT_CATALOG", e.to_string()))?;
    let items = catalog["items"]
        .as_array()
        .filter(|a| !a.is_empty() && a.len() <= 256)
        .ok_or_else(|| AppError::new(3, "ADAPT_CATALOG", "清单需含 1 至 256 个 items"))?;
    if out.exists() {
        return Err(AppError::new(
            5,
            "FILE_EXISTS",
            "批量目标目录必须尚不存在，避免覆盖本地主题",
        ));
    }
    let mut ids = BTreeSet::new();
    for item in items {
        let id = item["slug"]
            .as_str()
            .filter(|s| portable_id(s))
            .ok_or_else(|| AppError::new(3, "ADAPT_CATALOG", "无效 slug"))?;
        if !ids.insert(id) {
            return Err(AppError::new(3, "ADAPT_CATALOG", "清单主题标识重复"));
        }
        let source = Path::new(
            item["sourceDirectory"]
                .as_str()
                .ok_or_else(|| AppError::new(3, "ADAPT_CATALOG", "缺少来源目录"))?,
        );
        safe_path(source)?;
        if out.starts_with(source) || source.starts_with(out) {
            return Err(AppError::new(
                2,
                "ADAPT_PATH",
                "批量目标与来源目录不能相互包含",
            ));
        }
    }
    for fixture in &cli.verify_fixture {
        resume::parse_resume(&read(fixture, 1024 * 1024)?).map_err(AppError::from_theme)?;
    }
    fs::create_dir(out).map_err(|e| AppError::new(5, "ADAPT_IO", e.to_string()))?;
    let run_item = |item: &Value| {
        let id = item["slug"].as_str().unwrap();
        let directory = out.join(format!("community-{id}"));
        let source = Path::new(item["sourceDirectory"].as_str().unwrap());
        let result = (|| -> Result<Value, AppError> {
            let mut planned = plan(
                source,
                Some(id),
                item["license"].as_str(),
                item["upstreamUrl"].as_str(),
                item["upstreamRevision"].as_str(),
                cli.font_library.as_deref(),
            )?;
            let final_source =
                source_bundle::snapshot(source).map_err(|e| AppError::new(3, e.code, e.message))?;
            if final_source.source_sha256 != planned.report["sourceSnapshot"]["sourceSha256"] {
                return Err(AppError::new(3, "ADAPT_CHANGED", "迁移期间来源改变"));
            }
            fs::create_dir(&directory).map_err(|e| AppError::new(5, "ADAPT_IO", e.to_string()))?;
            for (name, data) in &planned.files {
                if Path::new(name).components().count() != 1 || name.starts_with('.') {
                    return Err(AppError::new(
                        3,
                        "ADAPT_RESOURCE_PATH",
                        "资源名不能含目录或隐藏文件",
                    ));
                }
                resume_one_core::export::write_export_unnamed(&directory.join(name), data, false)
                    .map_err(|e| AppError::new(5, e.code, e.message))?;
            }
            if let Some(license_path) = item["licenseFile"].as_str() {
                let license_bytes = read(Path::new(license_path), 1024 * 1024)?;
                std::str::from_utf8(&license_bytes)
                    .map_err(|_| AppError::new(3, "ADAPT_LICENSE", "许可正文须为 UTF-8"))?;
                planned.report["licenseEvidence"] =
                    json!({"path":license_path,"sha256":sha256(&license_bytes),"verified":false});
                resume_one_core::export::write_export_unnamed(
                    &directory.join("LICENSE.txt"),
                    &license_bytes,
                    false,
                )
                .map_err(|e| AppError::new(5, e.code, e.message))?;
            }
            let theme = serde_json::to_vec_pretty(&planned.manifest).unwrap();
            // Validate resources using the exact final relative package path before publication.
            let theme_path = directory.join("theme.json");
            let staging = directory.join(".candidate.theme.json");
            resume_one_core::export::write_export_unnamed(&staging, &theme, false)
                .map_err(|e| AppError::new(5, e.code, e.message))?;
            let loaded =
                resume_one_core::load_theme(&directory, Path::new(".candidate.theme.json"))
                    .map_err(AppError::from_theme)?;
            planned.report["validation"] =
                verify(&loaded, &cli.verify_fixture, cli.verify_exports)?;
            planned.report["candidateSha256"] = json!(sha256(&theme));
            let report_bytes = serde_json::to_vec_pretty(&planned.report).unwrap();
            resume_one_core::export::write_export_unnamed(
                &directory.join("MIGRATION.json"),
                &report_bytes,
                false,
            )
            .map_err(|e| AppError::new(5, e.code, e.message))?;
            let mut notice = format!(
                "{} — Resume One best-effort declarative adaptation\nSource: {}\nRevision: {}\nLicense declaration: {}\nNo upstream endorsement. License declaration is not legal clearance.\nNo source code executed; resume JSON not modified.\nNot pixel-identical. See MIGRATION.json for mapped rules, substitutions and verification limits.\n",
                planned.manifest["name"].as_str().unwrap_or(id),
                item["upstreamUrl"].as_str().unwrap_or("not supplied"),
                item["upstreamRevision"].as_str().unwrap_or("not supplied"),
                planned.manifest["license"]
                    .as_str()
                    .unwrap_or("NOASSERTION")
            );
            if let Some(author) = planned.manifest["provenance"]["upstreamAuthor"].as_str() {
                notice.push_str(&format!("Upstream author declaration: {author}\n"));
            }
            if let Some(url) = repository_url(&planned.report["sourcePackage"]) {
                notice.push_str(&format!("Original project declaration: {url}\n"));
            }
            for font in planned.report["fonts"]["notices"]
                .as_array()
                .into_iter()
                .flatten()
            {
                notice.push_str(&format!(
                    "\nFont {} ({}):\n{}\n",
                    font["assetId"].as_str().unwrap_or(""),
                    font["sha256"].as_str().unwrap_or(""),
                    font["text"].as_str().unwrap_or("")
                ));
            }
            resume_one_core::export::write_export_unnamed(
                &directory.join("NOTICE.txt"),
                notice.as_bytes(),
                false,
            )
            .map_err(|e| AppError::new(5, e.code, e.message))?;
            fs::rename(&staging, &theme_path)
                .map_err(|e| AppError::new(5, "ADAPT_IO", e.to_string()))?;
            Ok(
                json!({"slug":id,"status":"adapted-needs-visual-review","theme":theme_path,"candidateSha256":sha256(&theme),"sourceSha256":final_source.source_sha256,"validation":planned.report["validation"],"visualVerified":false}),
            )
        })();
        match result {
            Ok(row) => row,
            Err(error) => json!({"slug":id,"status":"failed","error":error.json()["error"]}),
        }
    };
    let results = std::thread::scope(|scope| -> Result<Vec<Value>, AppError> {
        let chunk = items.len().div_ceil(usize::from(cli.jobs));
        let handles: Vec<_> = items
            .chunks(chunk)
            .map(|group| {
                let run = &run_item;
                scope.spawn(move || group.iter().map(run).collect::<Vec<_>>())
            })
            .collect();
        let mut results = Vec::new();
        for handle in handles {
            results.extend(handle.join().map_err(|_| {
                AppError::new(3, "ADAPT_WORKER", "批量工作线程意外终止；未宣称全量完成")
            })?);
        }
        Ok(results)
    })?;
    let failed = results.iter().filter(|r| r["status"] == "failed").count();
    let report = json!({"format":"resume-one-adaptation-batch-v1","catalogSha256":sha256(&bytes),"requested":items.len(),"adapted":items.len()-failed,"failed":failed,"allGeneratedAndValidated":failed==0,"visualVerified":false,"exactMigrationComplete":false,"entries":results});
    resume_one_core::export::write_export_unnamed(
        &out.join("migration-report.json"),
        &serde_json::to_vec_pretty(&report).unwrap(),
        false,
    )
    .map_err(|e| AppError::new(5, e.code, e.message))?;
    if failed > 0 {
        let mut error = AppError::new(
            3,
            "ADAPT_BATCH_PARTIAL",
            format!("{failed} 套主题失败，未计为已迁移；详见批量报告"),
        );
        error.inspection = Some(Box::new(report));
        return Err(error);
    }
    Ok(report)
}
