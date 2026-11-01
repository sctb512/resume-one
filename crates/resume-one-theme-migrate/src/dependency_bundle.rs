//! Read-only inventory of explicitly supplied dependency packages.
//!
//! This is NOT Node's resolver, a module linker, or a dependency downloader.
//! Only string exports and explicit subpath-to-string maps are supported.
//! Conditions (including default/import), patterns, arrays, dynamic imports and
//! loader/URL semantics remain blockers. All lookups after snapshotting use the
//! immutable file table, never filesystem probes or reads outside a package.
//! Snapshot completeness is distinct from dependency-graph/rendering fidelity.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::{source_bundle, syntax};

const MAX_PACKAGES: usize = 8;
const MAX_TOTAL_BYTES: u64 = 64 * 1024 * 1024;
const MAX_MANIFEST_BYTES: usize = 256 * 1024;
const MAX_JSON_KEYS: usize = 4_096;
const MAX_REPORT_BYTES: usize = 16 * 1024 * 1024;
const MAX_BLOCKERS: usize = 4_096;
const MAX_RECORDS: usize = 16_384;
const MAX_SPECIFIER_BYTES: usize = 1_024;
const UNSUPPORTED_FORMATS: &[&str] = &[
    "ts",
    "tsx",
    "scss",
    "sass",
    "less",
    "styl",
    "html",
    "htm",
    "hbs",
    "handlebars",
    "mustache",
    "template",
    "pug",
    "jade",
    "ejs",
    "njk",
    "twig",
    "vue",
    "svelte",
    "mdx",
    "wasm",
    "node",
];

fn issue(
    issues: &mut Vec<Value>,
    code: &str,
    file: Option<&str>,
    detail: Value,
) -> Result<(), String> {
    if issues.len() >= MAX_BLOCKERS {
        return Err("DEPENDENCY_REPORT_LIMIT: more than 4096 blockers".into());
    }
    let mut value = json!({"code":code,"file":file});
    if let Some(detail) = detail.as_object() {
        value
            .as_object_mut()
            .expect("object")
            .extend(detail.clone());
    }
    issues.push(value);
    Ok(())
}

fn charge_bytes(total: &mut u64, bytes: u64) -> Result<(), String> {
    let next = total
        .checked_add(bytes)
        .filter(|value| *value <= MAX_TOTAL_BYTES)
        .ok_or("DEPENDENCY_TOTAL_LIMIT: all dependency snapshots together exceed 64 MiB")?;
    *total = next;
    Ok(())
}

fn report_size(value: &Value) -> Result<usize, String> {
    let bytes = serde_json::to_vec(value)
        .map_err(|e| format!("DEPENDENCY_JSON: {e}"))?
        .len();
    if bytes > MAX_REPORT_BYTES {
        return Err(
            "DEPENDENCY_REPORT_LIMIT: structured dependency evidence exceeds 16 MiB".into(),
        );
    }
    Ok(bytes)
}

fn time_limit(start: Instant) -> Result<(), String> {
    if start.elapsed() > Duration::from_secs(30) {
        return Err("DEPENDENCY_TIME_LIMIT: dependency inspection exceeds 30 seconds".into());
    }
    Ok(())
}

// A bounded conservative subset of npm identities, not permissive range/tag
// parsing. Different package names may share a version; a name may occur once.
fn valid_name(name: &str) -> bool {
    fn part(value: &str) -> bool {
        value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
            && value.bytes().all(|b| {
                b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_' | b'.')
            })
            && !matches!(value, "node_modules" | "favicon.ico")
    }
    if name.len() > 214 {
        return false;
    }
    if let Some(scoped) = name.strip_prefix('@') {
        let Some((scope, package)) = scoped.split_once('/') else {
            return false;
        };
        part(scope) && part(package)
    } else {
        part(name)
    }
}

fn valid_version(version: &str) -> bool {
    fn numeric(value: &str) -> bool {
        !value.is_empty()
            && value.bytes().all(|b| b.is_ascii_digit())
            && (value.len() == 1 || !value.starts_with('0'))
            && value.parse::<u64>().is_ok()
    }
    fn identifiers(value: &str, prerelease: bool) -> bool {
        !value.is_empty()
            && value.split('.').all(|part| {
                !part.is_empty()
                    && part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                    && (!prerelease || !part.bytes().all(|b| b.is_ascii_digit()) || numeric(part))
            })
    }
    if version.is_empty() || version.len() > 256 {
        return false;
    }
    let (version, build) = version
        .split_once('+')
        .map_or((version, None), |(a, b)| (a, Some(b)));
    if build.is_some_and(|v| !identifiers(v, false)) {
        return false;
    }
    let (core, pre) = version
        .split_once('-')
        .map_or((version, None), |(a, b)| (a, Some(b)));
    if pre.is_some_and(|v| !identifiers(v, true)) {
        return false;
    }
    let parts = core.split('.').collect::<Vec<_>>();
    parts.len() == 3 && parts.into_iter().all(numeric)
}

/// Serde validates JSON first. This second lexical pass ONLY rejects duplicate
/// object keys (including escaped spellings); it does not interpret JS/source.
fn unique_json_keys(bytes: &[u8]) -> Result<(), String> {
    let mut objects: Vec<Option<(bool, BTreeSet<String>)>> = Vec::new();
    let mut at = 0;
    let mut keys = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'{' => objects.push(Some((true, BTreeSet::new()))),
            b'[' => objects.push(None),
            b'}' | b']' => {
                objects.pop();
            }
            b',' => {
                if let Some(Some((key, _))) = objects.last_mut() {
                    *key = true;
                }
            }
            b':' => {
                if let Some(Some((key, _))) = objects.last_mut() {
                    *key = false;
                }
            }
            b'"' => {
                let start = at;
                at += 1;
                while at < bytes.len() {
                    if bytes[at] == b'\\' {
                        at += 2;
                        continue;
                    }
                    if bytes[at] == b'"' {
                        break;
                    }
                    at += 1;
                }
                if let Some(Some((true, seen))) = objects.last_mut() {
                    keys += 1;
                    if keys > MAX_JSON_KEYS {
                        return Err("DEPENDENCY_PACKAGE_LIMIT: too many manifest keys".into());
                    }
                    let key: String = serde_json::from_slice(&bytes[start..=at])
                        .map_err(|e| format!("DEPENDENCY_PACKAGE_JSON: {e}"))?;
                    if !seen.insert(key) {
                        return Err("DEPENDENCY_PACKAGE_JSON: duplicate object key".into());
                    }
                }
            }
            _ => {}
        }
        at += 1;
    }
    Ok(())
}

fn package_json(snapshot: &source_bundle::Snapshot) -> Result<(Value, &str), String> {
    let file = snapshot
        .files
        .iter()
        .find(|f| f.relative_path == "package.json")
        .ok_or("DEPENDENCY_PACKAGE_JSON: snapshot has no root package.json")?;
    if file.bytes.len() > MAX_MANIFEST_BYTES {
        return Err("DEPENDENCY_PACKAGE_LIMIT: package.json exceeds 256 KiB".into());
    }
    let package: Value =
        serde_json::from_slice(&file.bytes).map_err(|e| format!("DEPENDENCY_PACKAGE_JSON: {e}"))?;
    if !package.is_object() {
        return Err("DEPENDENCY_PACKAGE_JSON: expected an object".into());
    }
    unique_json_keys(&file.bytes)?;
    if !package["name"].as_str().is_some_and(valid_name) {
        return Err("DEPENDENCY_PACKAGE_NAME: require a bounded lowercase npm package name".into());
    }
    if !package["version"].as_str().is_some_and(valid_version) {
        return Err(
            "DEPENDENCY_PACKAGE_VERSION: require an exact bounded SemVer version, not a range/tag"
                .into(),
        );
    }
    Ok((package, &file.sha256))
}

/// URL decoding, aliases, query/hash loaders, backslashes and dot segments are
/// intentionally not normalized. Only exact portable snapshot paths qualify.
fn export_path(path: &str) -> Option<&str> {
    let path = path.strip_prefix("./")?;
    if path.len() > MAX_SPECIFIER_BYTES
        || path.is_empty()
        || path
            .bytes()
            .any(|b| !(0x20..=0x7e).contains(&b) || b"\\:%?#*<>\"|".contains(&b))
        || path.split('/').any(|p| {
            p.is_empty() || matches!(p, "." | ".." | "node_modules") || p.ends_with([' ', '.'])
        })
    {
        return None;
    }
    Some(path)
}

fn export_entry(
    subpath: &str,
    target: &Value,
    files: &BTreeMap<&str, &source_bundle::SourceFile>,
    issues: &mut Vec<Value>,
) -> Result<Value, String> {
    let mut entry =
        json!({"subpath":subpath,"target":target,"status":"unsupported","resolvedFile":null});
    let code = if subpath.contains('*') || target.as_str().is_some_and(|s| s.contains('*')) {
        Some("EXPORT_WILDCARD_UNSUPPORTED")
    } else if subpath != "." && export_path(subpath).is_none() {
        Some("EXPORT_SUBPATH_UNSUPPORTED")
    } else if target.is_object() {
        Some("EXPORT_CONDITION_UNSUPPORTED")
    } else if target.is_array() {
        Some("EXPORT_ARRAY_UNSUPPORTED")
    } else if !target.is_string() {
        Some("EXPORT_TARGET_UNSUPPORTED")
    } else {
        None
    };
    if let Some(code) = code {
        issue(
            issues,
            code,
            Some("package.json"),
            json!({"subpath":subpath,"message":"Only explicit subpath-to-string exports are interpreted; no branch/pattern selection"}),
        )?;
        return Ok(entry);
    }
    let Some(path) = export_path(target.as_str().expect("string")) else {
        issue(
            issues,
            "EXPORT_TARGET_PATH_UNSUPPORTED",
            Some("package.json"),
            json!({"subpath":subpath,"message":"Export must be an exact ./ file path without traversal, encoded/URL/loader syntax"}),
        )?;
        return Ok(entry);
    };
    entry["resolvedFile"] = json!(path);
    if let Some(file) = files.get(path) {
        entry["status"] = json!("file-present");
        entry["sha256"] = json!(file.sha256);
    } else {
        entry["status"] = json!("missing");
        issue(
            issues,
            "EXPORT_TARGET_MISSING",
            Some("package.json"),
            json!({"subpath":subpath,"target":path,"message":"Exact export file is not in the snapshot; no extension/directory fallback"}),
        )?;
    }
    Ok(entry)
}

fn exports(
    package: &Value,
    files: &BTreeMap<&str, &source_bundle::SourceFile>,
    issues: &mut Vec<Value>,
) -> Result<Vec<Value>, String> {
    let Some(value) = package.get("exports") else {
        issue(
            issues,
            "PACKAGE_EXPORTS_REQUIRED",
            Some("package.json"),
            json!({"message":"No explicit exports; legacy main/module and Node fallback are not implemented"}),
        )?;
        return Ok(Vec::new());
    };
    let Some(map) = value.as_object() else {
        return Ok(vec![export_entry(".", value, files, issues)?]);
    };
    if map.is_empty() {
        issue(
            issues,
            "EXPORT_MAP_EMPTY",
            Some("package.json"),
            json!({"message":"Empty exports is not a supported module entry"}),
        )?;
        return Ok(Vec::new());
    }
    if map.keys().any(|key| key != "." && !key.starts_with("./")) {
        // Do not partially accept a mixed condition/subpath object.
        issue(
            issues,
            "EXPORT_CONDITION_UNSUPPORTED",
            Some("package.json"),
            json!({"message":"Root condition or mixed condition/subpath map is unsupported"}),
        )?;
        return Ok(vec![
            json!({"subpath":".","target":value,"status":"unsupported","resolvedFile":null}),
        ]);
    }
    if map.len() > MAX_RECORDS {
        return Err("DEPENDENCY_REPORT_LIMIT: too many export records".into());
    }
    map.iter()
        .map(|(key, value)| export_entry(key, value, files, issues))
        .collect()
}

fn local_path(current: &str, module: &str) -> Result<String, &'static str> {
    if module.len() > MAX_SPECIFIER_BYTES
        || !(module.starts_with("./") || module.starts_with("../"))
        || module
            .bytes()
            .any(|b| !(0x20..=0x7e).contains(&b) || b"\\:%?#*<>\"|".contains(&b))
    {
        return Err("LOCAL_IMPORT_PATH_UNSUPPORTED");
    }
    let mut parts = current.split('/').collect::<Vec<_>>();
    parts.pop();
    for part in module.split('/') {
        match part {
            "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err("LOCAL_IMPORT_OUTSIDE_PACKAGE");
                }
            }
            "" => return Err("LOCAL_IMPORT_PATH_UNSUPPORTED"),
            _ => parts.push(part),
        }
    }
    if parts.is_empty() {
        return Err("LOCAL_IMPORT_PATH_UNSUPPORTED");
    }
    Ok(parts.join("/"))
}

fn imports(
    analysis: &Value,
    path: &str,
    files: &BTreeMap<&str, &source_bundle::SourceFile>,
    issues: &mut Vec<Value>,
    external: &mut BTreeSet<String>,
) -> Result<Vec<Value>, String> {
    let mut result = Vec::new();
    for original in analysis["imports"].as_array().into_iter().flatten() {
        let mut record = original.clone();
        record["file"] = json!(path);
        record["resolution"] = json!("unresolved");
        if matches!(
            original["kind"].as_str(),
            Some("dynamic-import" | "require-call")
        ) {
            issue(
                issues,
                "DYNAMIC_IMPORT_SEMANTICS_UNSUPPORTED",
                Some(path),
                json!({"kind":original["kind"],"span":original["span"],"message":"Call-based imports/require are syntax evidence, not evaluated or certified unshadowed"}),
            )?;
        }
        if let Some(module) = original["module"].as_str() {
            if module.starts_with('.') {
                match local_path(path, module) {
                    Err(code) => issue(
                        issues,
                        code,
                        Some(path),
                        json!({"module":module,"span":original["span"],"message":"Local path cannot be interpreted within this package snapshot"}),
                    )?,
                    Ok(base) => {
                        let candidates = if files.contains_key(base.as_str()) {
                            vec![base.clone()]
                        } else {
                            [
                                ".js",
                                ".jsx",
                                ".mjs",
                                ".cjs",
                                ".css",
                                ".json",
                                "/index.js",
                                "/index.jsx",
                            ]
                            .iter()
                            .map(|suffix| format!("{base}{suffix}"))
                            .filter(|candidate| files.contains_key(candidate.as_str()))
                            .collect::<Vec<_>>()
                        };
                        record["snapshotCandidates"] = json!(candidates);
                        let code = match candidates.as_slice() {
                            [] => Some("LOCAL_DEPENDENCY_MISSING"),
                            [one] if one == &base => {
                                record["resolution"] = json!("exact-file-present-only");
                                let extension = one.rsplit('.').next().unwrap_or("");
                                if !matches!(extension, "js" | "jsx" | "mjs" | "cjs" | "css") {
                                    Some("LOCAL_MODULE_FORMAT_UNSUPPORTED")
                                } else {
                                    None
                                }
                            }
                            [_] => Some("LOCAL_IMPORT_RESOLUTION_REQUIRED"),
                            _ => Some("LOCAL_IMPORT_AMBIGUOUS"),
                        };
                        if let Some(code) = code {
                            issue(
                                issues,
                                code,
                                Some(path),
                                json!({"module":module,"snapshotCandidates":candidates,"span":original["span"],"message":"Snapshot-only lexical candidates are not Node resolution; none were read outside the snapshot"}),
                            )?;
                        }
                    }
                }
            } else {
                external.insert(module.to_owned());
                record["resolution"] = json!("external-not-linked");
                issue(
                    issues,
                    "EXTERNAL_DEPENDENCY_UNRESOLVED",
                    Some(path),
                    json!({"module":module,"span":original["span"],"message":"External modules remain unresolved even if another explicit dependency has a matching name"}),
                )?;
                if module.is_empty()
                    || module.len() > MAX_SPECIFIER_BYTES
                    || module.starts_with('/')
                    || module
                        .bytes()
                        .any(|b| b.is_ascii_control() || b"\\:%?#*".contains(&b))
                {
                    issue(
                        issues,
                        "IMPORT_SPECIFIER_UNSUPPORTED",
                        Some(path),
                        json!({"module":module,"span":original["span"],"message":"Absolute, URL, wildcard, loader or oversized import is not interpreted or fetched"}),
                    )?;
                }
            }
        } else {
            issue(
                issues,
                "IMPORT_EXPRESSION_UNRESOLVED",
                Some(path),
                json!({"span":original["span"],"message":"Dynamic/escaped import has no supported literal module path"}),
            )?;
        }
        if result.len() >= MAX_RECORDS {
            return Err("DEPENDENCY_REPORT_LIMIT: too many import records".into());
        }
        result.push(record);
    }
    Ok(result)
}

fn css_issues(css: &Value, path: &str, issues: &mut Vec<Value>) -> Result<(), String> {
    if css["syntaxValid"] != true {
        issue(
            issues,
            "CSS_SYNTAX",
            Some(path),
            json!({"message":"CSS syntax evidence is incomplete"}),
        )?;
    }
    for directive in css["directives"].as_array().into_iter().flatten() {
        if directive["kind"] == "import_statement" {
            issue(
                issues,
                "CSS_IMPORT_UNRESOLVED",
                Some(path),
                json!({"span":directive["span"],"message":"CSS import remains unlinked; no URL/path is inferred from a truncated display snippet"}),
            )?;
        }
    }
    Ok(())
}

fn inspect_package(
    root: &str,
    snapshot: &source_bundle::Snapshot,
    package: &Value,
    manifest_sha: &str,
    started: Instant,
) -> Result<Value, String> {
    let files = snapshot
        .files
        .iter()
        .map(|f| (f.relative_path.as_str(), f))
        .collect::<BTreeMap<_, _>>();
    let mut issues = Vec::new();
    let export_entries = exports(package, &files, &mut issues)?;
    let mut evidence = Vec::new();
    let mut import_records = Vec::new();
    let mut external = BTreeSet::new();
    let mut unsupported = Vec::new();
    let mut evidence_bytes = 0usize;
    for file in &snapshot.files {
        time_limit(started)?;
        let path = &file.relative_path;
        let analysis = syntax::analyze(path, &file.bytes)
            .map_err(|e| format!("{}: {path}: {}", e.code, e.message))?;
        let Some(analysis) = analysis else {
            let extension = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
            if UNSUPPORTED_FORMATS.contains(&extension.as_str()) {
                unsupported.push(path.clone());
                issue(
                    &mut issues,
                    "FORMAT_ADAPTER_REQUIRED",
                    Some(path),
                    json!({"message":"This cached source format has no supported syntax adapter; not assumed unused"}),
                )?;
            }
            continue;
        };
        time_limit(started)?;
        evidence_bytes = evidence_bytes
            .checked_add(report_size(&analysis)?)
            .filter(|size| *size <= MAX_REPORT_BYTES)
            .ok_or("DEPENDENCY_REPORT_LIMIT: cumulative syntax evidence exceeds 16 MiB")?;
        if analysis["format"] == "css" {
            css_issues(&analysis["styles"], path, &mut issues)?;
        } else {
            if analysis["syntaxValid"] != true {
                issue(
                    &mut issues,
                    "JAVASCRIPT_SYNTAX",
                    Some(path),
                    json!({"message":"AST has syntax/JSX errors; import evidence is not a complete graph"}),
                )?;
            }
            import_records.extend(imports(
                &analysis,
                path,
                &files,
                &mut issues,
                &mut external,
            )?);
            if import_records.len() > MAX_RECORDS {
                return Err("DEPENDENCY_REPORT_LIMIT: cumulative imports exceed 16384".into());
            }
            for style in analysis["styledComponents"]
                .as_array()
                .into_iter()
                .flatten()
            {
                if !style["css"].is_null() {
                    css_issues(&style["css"], path, &mut issues)?;
                }
                if style["dynamicExpressions"]
                    .as_array()
                    .is_some_and(|v| !v.is_empty())
                    || style["templateEscapesRequireDecoding"] == true
                {
                    issue(
                        &mut issues,
                        "STYLE_DEPENDENCIES_UNRESOLVED",
                        Some(path),
                        json!({"span":style["span"],"message":"Unresolved/escaped style content may contain resource dependencies"}),
                    )?;
                }
            }
        }
        evidence.push(analysis);
    }
    let name = package["name"].as_str().expect("validated name");
    let result = json!({
        "name":name,"version":package["version"],"root":root,
        "packageJsonSha256":manifest_sha,"sourceSha256":snapshot.source_sha256,
        "sourceSnapshot":snapshot.report(),
        "package":{"name":name,"version":package["version"],"exports":package["exports"],
            "main":package["main"],"module":package["module"],"type":package["type"],
            "licenseDeclaration":package["license"],"dependencies":package["dependencies"],
            "peerDependencies":package["peerDependencies"],"optionalDependencies":package["optionalDependencies"]},
        "exportsEntries":export_entries,"imports":import_records,"externalImports":external,
        "unsupportedFormats":unsupported,"syntaxEvidence":evidence,"blockers":issues,
        "dependencyGraphComplete":false,"originalRenderingVerified":false,
        "resourceDependencyAnalysisComplete":false,"executedSourceCode":false,"sourceModified":false
    });
    report_size(&result)?;
    Ok(result)
}

/// At most eight explicit absolute roots. Snapshot, identity, budget or parser
/// failures return Err, never a truncated/partially trusted bundle. Per-package
/// snapshots retain source_bundle's 16-level/64-MiB guards. One snapshot's bytes
/// are retained at a time; accepted aggregate contents are capped at 64 MiB.
pub(crate) fn inspect(roots: &[PathBuf]) -> Result<Value, String> {
    if roots.len() > MAX_PACKAGES {
        return Err("DEPENDENCY_ROOT_LIMIT: at most 8 explicit package directories".into());
    }
    let mut unique_roots = BTreeSet::new();
    for root in roots {
        if !root.is_absolute() || root.to_str().is_none() {
            return Err(
                "DEPENDENCY_ROOT_PATH: require an explicit absolute UTF-8 directory".into(),
            );
        }
        if !unique_roots.insert(root) {
            return Err("DEPENDENCY_ROOT_DUPLICATE: repeated dependency directory".into());
        }
    }
    let started = Instant::now();
    let mut packages = Vec::new();
    let mut names = BTreeSet::new();
    let mut total_bytes = 0;
    let mut total_report = 0;
    for root in roots {
        time_limit(started)?;
        let snapshot = source_bundle::snapshot(root).map_err(|e| e.to_string())?;
        for file in &snapshot.files {
            charge_bytes(&mut total_bytes, file.bytes.len() as u64)?;
        }
        let (package, manifest_sha) = package_json(&snapshot)?;
        let name = package["name"].as_str().expect("validated");
        if !names.insert(name.to_owned()) {
            return Err(format!(
                "DEPENDENCY_PACKAGE_DUPLICATE: {name} supplied more than once (including different versions)"
            ));
        }
        let report = inspect_package(
            root.to_str().expect("validated"),
            &snapshot,
            &package,
            manifest_sha,
            started,
        )?;
        total_report += report_size(&report)?;
        if total_report > MAX_REPORT_BYTES {
            return Err("DEPENDENCY_REPORT_LIMIT: all dependency reports exceed 16 MiB".into());
        }
        packages.push(report);
    }
    packages.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    let mut external = BTreeSet::new();
    let mut blockers = Vec::new();
    let mut fingerprints = Vec::new();
    for package in &packages {
        fingerprints.push(json!({"name":package["name"],"version":package["version"],"root":package["root"],"sourceSha256":package["sourceSha256"],"packageJsonSha256":package["packageJsonSha256"]}));
        external.extend(
            package["externalImports"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_owned),
        );
        for problem in package["blockers"].as_array().into_iter().flatten() {
            if blockers.len() >= MAX_BLOCKERS {
                return Err("DEPENDENCY_REPORT_LIMIT: more than 4096 aggregate blockers".into());
            }
            let mut problem = problem.clone();
            problem["packageName"] = package["name"].clone();
            blockers.push(problem);
        }
    }
    issue(
        &mut blockers,
        "DEPENDENCY_GRAPH_NOT_LINKED",
        None,
        json!({"message":"Explicit package snapshots/exports do not complete transitive dependencies or resolve root external imports"}),
    )?;
    issue(
        &mut blockers,
        "ORIGINAL_RENDERING_REQUIRED",
        None,
        json!({"message":"No source executed, original rendering or high-fidelity acceptance performed"}),
    )?;
    let result = json!({
        "dependencyBundleVersion":1,"scope":"explicit-local-package-snapshots-only",
        "limits":{"packages":MAX_PACKAGES,"perSnapshotDepth":16,"perSnapshotBytes":MAX_TOTAL_BYTES,"aggregateBytes":MAX_TOTAL_BYTES,"reportBytes":MAX_REPORT_BYTES},
        "packageCount":packages.len(),"totalBytes":total_bytes,
        "fingerprints":fingerprints,"packages":packages,"externalImports":external,"blockers":blockers,
        "dependencyGraphComplete":false,"rootExternalImportsResolved":false,
        "originalRenderingVerified":false,"migrationComplete":false,"visualVerified":false,
        "licenseVerified":false,"executedSourceCode":false,"sourceModified":false,"networkAccessed":false,
        "resolver":"exact exports strings/subpaths plus snapshot-only local candidates; not Node resolution"
    });
    report_size(&result)?;
    time_limit(started)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Fixture(PathBuf);
    impl Fixture {
        fn new(name: &str, exports: Value) -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let parent = std::env::temp_dir().canonicalize().unwrap();
            let path = loop {
                let path = parent.join(format!(
                    "r1-deps-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
                match fs::create_dir(&path) {
                    Ok(()) => break path,
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(e) => panic!("fixture directory: {e}"),
                }
            };
            let fixture = Self(path);
            fixture.write(
                "package.json",
                serde_json::to_string(&json!({"name":name,"version":"1.0.0","exports":exports}))
                    .unwrap()
                    .as_bytes(),
            );
            fixture
        }
        fn write(&self, path: &str, bytes: &[u8]) {
            let path = self.0.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, bytes).unwrap();
        }
        fn inspect(&self) -> Value {
            inspect(std::slice::from_ref(&self.0)).unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn has(value: &Value, code: &str) -> bool {
        value["blockers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["code"] == code)
    }

    #[test]
    fn explicit_core_utils_style_exports_preserve_files_hashes_and_imports() {
        let core = Fixture::new(
            "@jsonresume/core",
            json!({".":"./src/index.js","./ssr":"./src/ssr/index.js"}),
        );
        core.write(
            "src/index.js",
            b"export {safeUrl} from '@jsonresume/utils';",
        );
        core.write(
            "src/ssr/index.js",
            b"export const renderer = 'not executed';",
        );
        let utils = Fixture::new("@jsonresume/utils", json!("./src/index.js"));
        utils.write("src/index.js", b"export const safeUrl = value => value;");
        let report = inspect(&[utils.0.clone(), core.0.clone()]).unwrap();
        assert_eq!(report["packageCount"], 2);
        assert_eq!(report["packages"][0]["name"], "@jsonresume/core");
        assert_eq!(
            report["packages"][0]["exportsEntries"][1]["resolvedFile"],
            "src/ssr/index.js"
        );
        assert_eq!(
            report["packages"][0]["exportsEntries"][0]["status"],
            "file-present"
        );
        assert_eq!(report["externalImports"], json!(["@jsonresume/utils"]));
        assert!(has(&report, "EXTERNAL_DEPENDENCY_UNRESOLVED"));
        assert_eq!(report["dependencyGraphComplete"], false);
        assert_eq!(report["rootExternalImportsResolved"], false);
        assert_eq!(report["originalRenderingVerified"], false);
        assert_eq!(report["packages"][0]["sourceSnapshot"]["complete"], true);
        let total = [core.0.as_path(), utils.0.as_path()]
            .iter()
            .map(|root| {
                source_bundle::snapshot(root)
                    .unwrap()
                    .files
                    .iter()
                    .map(|f| f.bytes.len() as u64)
                    .sum::<u64>()
            })
            .sum::<u64>();
        assert_eq!(report["totalBytes"], total);
    }

    #[test]
    fn duplicate_package_names_even_with_different_versions_are_errors() {
        let a = Fixture::new("example", json!("./index.js"));
        let b = Fixture::new("example", json!("./index.js"));
        b.write(
            "package.json",
            br#"{"name":"example","version":"2.0.0","exports":"./index.js"}"#,
        );
        assert!(
            inspect(&[a.0.clone(), b.0.clone()])
                .unwrap_err()
                .contains("DEPENDENCY_PACKAGE_DUPLICATE")
        );
        assert!(
            inspect(&[a.0.clone(), a.0.clone()])
                .unwrap_err()
                .contains("DEPENDENCY_ROOT_DUPLICATE")
        );
    }

    #[test]
    fn exports_cannot_escape_or_use_encoding_urls_aliases_and_directories() {
        for target in [
            "../outside.js",
            "/outside.js",
            "./src/../outside.js",
            "./%2e%2e/outside.js",
            "file:///outside.js",
            "./src/index.js?raw",
            "./src//index.js",
            "./node_modules/x.js",
        ] {
            let fixture = Fixture::new("example", json!(target));
            fixture.write("src/index.js", b"export const x=1;");
            let report = fixture.inspect();
            assert!(
                has(&report, "EXPORT_TARGET_PATH_UNSUPPORTED"),
                "{target}: {report}"
            );
            assert_eq!(
                report["packages"][0]["exportsEntries"][0]["status"],
                "unsupported"
            );
        }
        let fixture = Fixture::new("example", json!("./src"));
        fixture.write("src/index.js", b"export const x=1;");
        assert!(has(&fixture.inspect(), "EXPORT_TARGET_MISSING"));
    }

    #[test]
    fn external_and_local_missing_paths_and_unsupported_sources_remain_blockers() {
        let fixture = Fixture::new("example", json!("./src/index.js"));
        fixture.write("src/index.js",b"import x from 'not-provided'; import './missing.js'; import '../../outside.js'; const fake=\"import 'fake';\";");
        fixture.write("src/Widget.tsx", b"export const x:number=1;");
        fixture.write(
            "src/style.css",
            b"@import url('https://example.invalid/font.css'); p { color:red; }",
        );
        let report = fixture.inspect();
        for code in [
            "EXTERNAL_DEPENDENCY_UNRESOLVED",
            "LOCAL_DEPENDENCY_MISSING",
            "LOCAL_IMPORT_OUTSIDE_PACKAGE",
            "FORMAT_ADAPTER_REQUIRED",
            "CSS_IMPORT_UNRESOLVED",
        ] {
            assert!(has(&report, code), "{code}");
        }
        assert_eq!(report["externalImports"], json!(["not-provided"]));
        assert_eq!(
            report["packages"][0]["unsupportedFormats"],
            json!(["src/Widget.tsx"])
        );
        assert_eq!(report["networkAccessed"], false);
    }

    #[test]
    fn conditions_wildcards_arrays_and_dynamic_imports_are_explicitly_unsupported() {
        for exports in [
            json!({"import":"./index.js","default":"./index.js"}),
            json!({".":{"default":"./index.js"}}),
        ] {
            let fixture = Fixture::new("example", exports);
            fixture.write("index.js", b"export const x=1;");
            assert!(has(&fixture.inspect(), "EXPORT_CONDITION_UNSUPPORTED"));
        }
        let fixture = Fixture::new("example", json!({"./*":"./src/*.js",".":["./index.js"]}));
        fixture.write(
            "index.js",
            b"import('./x.js'); import(variable); require('./x.js');",
        );
        fixture.write("x.js", b"export const x=1;");
        let report = fixture.inspect();
        for code in [
            "EXPORT_WILDCARD_UNSUPPORTED",
            "EXPORT_ARRAY_UNSUPPORTED",
            "DYNAMIC_IMPORT_SEMANTICS_UNSUPPORTED",
            "IMPORT_EXPRESSION_UNRESOLVED",
        ] {
            assert!(has(&report, code), "{code}");
        }
    }

    #[test]
    fn fingerprints_are_read_only_content_sensitive_and_previously_returned_values_immutable() {
        let fixture = Fixture::new("example", json!("./index.js"));
        fixture.write("index.js", b"export const x=1;");
        fixture.write("fonts/font.bin", b"\0\xffFONT");
        fixture.write("LICENSE", b"unchanged full license");
        let before = fixture.inspect();
        let saved = before.clone();
        assert_eq!(before, fixture.inspect());
        assert_eq!(
            fs::read(fixture.0.join("index.js")).unwrap(),
            b"export const x=1;"
        );
        fixture.write("index.js", b"export const x=2;");
        let after = fixture.inspect();
        assert_ne!(
            before["fingerprints"][0]["sourceSha256"],
            after["fingerprints"][0]["sourceSha256"]
        );
        assert_eq!(before, saved);
        assert_eq!(before["sourceModified"], false);
        assert_eq!(
            fs::read(fixture.0.join("LICENSE")).unwrap(),
            b"unchanged full license"
        );
    }

    #[test]
    fn identity_validation_and_duplicate_json_keys_fail_closed() {
        for name in [
            "",
            "UPPER",
            "../escape",
            "@scope",
            "@scope/../x",
            "name with space",
        ] {
            assert!(!valid_name(name));
        }
        for version in [
            "",
            "1",
            "v1.0.0",
            "^1.0.0",
            "workspace:*",
            "01.0.0",
            "1.0.0-01",
            "1.0.0+",
            "1.0.0+x+y",
        ] {
            assert!(!valid_version(version), "{version}");
        }
        assert!(valid_version("1.2.3-alpha.1+build.001"));
        let fixture = Fixture::new("example", json!("./index.js"));
        fixture.write(
            "package.json",
            br#"{"name":"one","\u006eame":"two","version":"1.0.0"}"#,
        );
        assert!(
            inspect(std::slice::from_ref(&fixture.0))
                .unwrap_err()
                .contains("duplicate object key")
        );
        fixture.write("package.json", br#"{"name":"example","version":"latest"}"#);
        assert!(
            inspect(std::slice::from_ref(&fixture.0))
                .unwrap_err()
                .contains("DEPENDENCY_PACKAGE_VERSION")
        );
    }

    #[test]
    fn aggregate_budget_and_root_count_are_bounded_without_partial_success() {
        let mut total = MAX_TOTAL_BYTES - 1;
        charge_bytes(&mut total, 1).unwrap();
        assert_eq!(total, MAX_TOTAL_BYTES);
        assert!(
            charge_bytes(&mut total, 1)
                .unwrap_err()
                .contains("DEPENDENCY_TOTAL_LIMIT")
        );
        assert_eq!(total, MAX_TOTAL_BYTES);
        assert!(charge_bytes(&mut total, u64::MAX).is_err());
        assert!(
            inspect(&vec![PathBuf::from("relative"); 9])
                .unwrap_err()
                .contains("DEPENDENCY_ROOT_LIMIT")
        );
        assert!(
            inspect(&[PathBuf::from("relative")])
                .unwrap_err()
                .contains("DEPENDENCY_ROOT_PATH")
        );
        let empty = inspect(&[]).unwrap();
        assert_eq!(empty["dependencyGraphComplete"], false);
        assert!(has(&empty, "DEPENDENCY_GRAPH_NOT_LINKED"));
    }

    #[test]
    fn lexical_candidates_do_not_pretend_to_choose_node_resolution() {
        let fixture = Fixture::new("example", json!("./index.js"));
        fixture.write(
            "index.js",
            b"import './both'; import './only'; import './ok.js'; import './ok.js?inline';",
        );
        for path in ["both.js", "both.jsx", "only.js", "ok.js"] {
            fixture.write(path, b"export const x=1;");
        }
        let report = fixture.inspect();
        for code in [
            "LOCAL_IMPORT_AMBIGUOUS",
            "LOCAL_IMPORT_RESOLUTION_REQUIRED",
            "LOCAL_IMPORT_PATH_UNSUPPORTED",
        ] {
            assert!(has(&report, code), "{code}");
        }
        assert_eq!(
            report["packages"][0]["imports"][2]["resolution"],
            "exact-file-present-only"
        );
    }

    #[cfg(unix)]
    #[test]
    fn snapshot_symlink_guards_cannot_be_bypassed_by_exports() {
        let fixture = Fixture::new("example", json!("./linked.js"));
        let outside = Fixture::new("other", json!("./index.js"));
        outside.write("index.js", b"must not be read through link");
        std::os::unix::fs::symlink(outside.0.join("index.js"), fixture.0.join("linked.js"))
            .unwrap();
        assert!(
            inspect(std::slice::from_ref(&fixture.0))
                .unwrap_err()
                .contains("SOURCE_SYMLINK")
        );
    }

    #[test]
    #[ignore = "requires RESUME_ONE_CACHED_PACKAGES_ROOT for optional real core/utils snapshots"]
    fn cached_core_utils_patterns_keep_missing_exports_and_externals_visible() {
        let base = PathBuf::from(
            std::env::var("RESUME_ONE_CACHED_PACKAGES_ROOT").expect("set cached packages root"),
        );
        let report = inspect(&[base.join("resume-core"), base.join("utils")]).unwrap();
        assert_eq!(report["packages"][0]["name"], "@jsonresume/core");
        assert_eq!(report["packages"][1]["name"], "@jsonresume/utils");
        assert_eq!(
            report["packages"][0]["exportsEntries"]
                .as_array()
                .unwrap()
                .len(),
            7
        );
        assert_eq!(
            report["packages"][1]["exportsEntries"]
                .as_array()
                .unwrap()
                .len(),
            4
        );
        assert!(has(&report, "EXPORT_TARGET_MISSING"));
        assert!(has(&report, "EXTERNAL_DEPENDENCY_UNRESOLVED"));
        println!(
            "{} packages, {} bytes, {} blockers; graphComplete={}, originalRenderingVerified={}",
            report["packageCount"],
            report["totalBytes"],
            report["blockers"].as_array().unwrap().len(),
            report["dependencyGraphComplete"],
            report["originalRenderingVerified"]
        );
    }
}
