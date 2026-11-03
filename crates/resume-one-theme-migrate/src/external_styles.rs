//! Declaration-only styled inheritance across frozen module evidence.
//! CSS/blockers stay in the source report; no initializer or module is executed.

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

const MAX_DEPTH: usize = 64;
const MAX_STEPS: usize = 1024;
const TAGS: &[&str] = &[
    "div",
    "span",
    "p",
    "main",
    "section",
    "aside",
    "header",
    "footer",
    "article",
    "address",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "ul",
    "ol",
    "li",
    "strong",
    "em",
    "b",
    "i",
    "a",
    "br",
    "hr",
    "small",
    "time",
    "blockquote",
    "dl",
    "dt",
    "dd",
    "table",
    "thead",
    "tbody",
    "tr",
    "td",
    "th",
    "img",
];

struct Budget {
    started: Instant,
    steps: usize,
}

impl Budget {
    fn tick(&mut self) -> Result<(), String> {
        self.steps += 1;
        if self.steps > MAX_STEPS || self.started.elapsed() > Duration::from_secs(3) {
            return Err("EXTERNAL_STYLE_LIMIT".into());
        }
        Ok(())
    }
}

fn items<'a>(value: &'a Value, key: &str) -> impl Iterator<Item = &'a Value> {
    value[key].as_array().into_iter().flatten()
}

fn one<'a>(mut values: impl Iterator<Item = &'a Value>, code: &str) -> Result<&'a Value, String> {
    let value = values.next().ok_or_else(|| code.to_owned())?;
    if values.next().is_some() {
        return Err(code.into());
    }
    Ok(value)
}

fn name<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value[key]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 4096)
        .ok_or_else(|| format!("EXTERNAL_STYLE_IDENTITY:{key}"))
}

fn hash(value: &Value) -> bool {
    value.as_str().is_some_and(|s| {
        s.len() == 64
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}

fn position(value: &Value, file: &str) -> Result<(u64, u64), String> {
    let start = value["startByte"].as_u64().ok_or("EXTERNAL_STYLE_SPAN")?;
    let end = value["endByte"].as_u64().ok_or("EXTERNAL_STYLE_SPAN")?;
    if value["file"] != file || end <= start {
        return Err("EXTERNAL_STYLE_SPAN".into());
    }
    Ok((start, end))
}

fn same_span(a: &Value, b: &Value, file: &str) -> bool {
    position(a, file)
        .ok()
        .zip(position(b, file).ok())
        .is_some_and(|(a, b)| a == b)
}

fn package_name(package: &Value) -> &Value {
    if package["name"].is_string() {
        &package["name"]
    } else {
        &package["package"]["name"]
    }
}

fn package_version(package: &Value) -> &Value {
    if package["version"].is_string() {
        &package["version"]
    } else {
        &package["package"]["version"]
    }
}

struct View<'a> {
    analysis: &'a Value,
    declaration: &'a Value,
}

fn view<'a>(report: &'a Value, target: &Value, budget: &mut Budget) -> Result<View<'a>, String> {
    budget.tick()?;
    let file = name(target, "file")?;
    let local = name(target, "name")?;
    name(target, "packageName")?;
    if target["lookup"] != "local"
        || target["symbol"] != local
        || !hash(&target["sourceSha256"])
        || !hash(&target["fileSha256"])
    {
        return Err("EXTERNAL_STYLE_IDENTITY".into());
    }
    let package = one(
        std::iter::once(report)
            .chain(items(&report["dependencyBundle"], "packages"))
            .filter(|p| {
                *package_name(p) == target["packageName"]
                    && p["sourceSnapshot"]["sourceSha256"] == target["sourceSha256"]
            }),
        "EXTERNAL_STYLE_PACKAGE_AMBIGUOUS_OR_MISSING",
    )?;
    if *package_version(package) != target["packageVersion"]
        || package["sourceSnapshot"]["complete"] != true
        || (package.get("sourceSha256").is_some()
            && package["sourceSha256"] != target["sourceSha256"])
    {
        return Err("EXTERNAL_STYLE_SNAPSHOT_MISMATCH".into());
    }
    let source = one(
        items(&package["sourceSnapshot"], "files").filter(|f| f["path"] == file),
        "EXTERNAL_STYLE_FILE_AMBIGUOUS_OR_MISSING",
    )?;
    if source["sha256"] != target["fileSha256"] || source["utf8"] != true {
        return Err("EXTERNAL_STYLE_FILE_HASH_MISMATCH".into());
    }
    let analysis = one(
        items(package, "syntaxEvidence").filter(|a| a["file"] == file),
        "EXTERNAL_STYLE_SYNTAX_AMBIGUOUS_OR_MISSING",
    )?;
    let symbols = &analysis["moduleSymbols"];
    if analysis["syntaxValid"] != true || symbols["syntaxValid"] != true {
        return Err("EXTERNAL_STYLE_SYNTAX_INVALID".into());
    }
    if items(symbols, "blockers").any(|b| {
        matches!(
            b["code"].as_str(),
            Some(
                "DUPLICATE_MODULE_LOCAL"
                    | "UNSUPPORTED_DESTRUCTURING"
                    | "ESCAPED_SYMBOL_NAME"
                    | "UNSUPPORTED_DECLARATION"
                    | "UNSUPPORTED_MODULE_STATEMENT"
                    | "UNSUPPORTED_IMPORT_ATTRIBUTE"
            )
        )
    }) {
        return Err("EXTERNAL_STYLE_MODULE_SCOPE_UNPROVEN".into());
    }
    let declaration = one(
        items(symbols, "declarations").filter(|d| d["name"] == local),
        "EXTERNAL_STYLE_DECLARATION_AMBIGUOUS_OR_MISSING",
    )?;
    if items(symbols, "imports").any(|i| i["local"] == local) {
        return Err("EXTERNAL_STYLE_LOCAL_COLLISION".into());
    }
    let (_, end) = position(&declaration["span"], file)?;
    if declaration != &target["declaration"]
        || !same_span(&target["span"], &declaration["span"], file)
        || source["bytes"].as_u64().is_none_or(|bytes| end > bytes)
    {
        return Err("EXTERNAL_STYLE_DECLARATION_MISMATCH".into());
    }
    budget.tick()?;
    Ok(View {
        analysis,
        declaration,
    })
}

fn local_target(identity: &Value, declaration: &Value) -> Value {
    json!({"packageName":identity["packageName"],"packageVersion":identity["packageVersion"],
        "sourceSha256":identity["sourceSha256"],"file":identity["file"],"fileSha256":identity["fileSha256"],
        "lookup":"local","symbol":declaration["name"],"name":declaration["name"],
        "span":declaration["span"],"declaration":declaration,"bindingValueProven":false})
}

fn initialized_before(target: &Value, derived: &Value, start: u64) -> Result<(), String> {
    if target["packageName"] == derived["packageName"]
        && target["sourceSha256"] == derived["sourceSha256"]
        && target["file"] == derived["file"]
    {
        let file = name(target, "file")?;
        let (_, end) = position(&target["declaration"]["initializer"]["span"], file)?;
        if end > start {
            return Err("EXTERNAL_STYLE_BASE_TDZ".into());
        }
    }
    Ok(())
}

/// Resolves declaration layers only, in base-to-derived order. A core function
/// terminal must still pass the parent's pinned primitive validation.
pub(crate) fn resolve(report: &Value, target: &Value) -> Result<Value, String> {
    let mut budget = Budget {
        started: Instant::now(),
        steps: 0,
    };
    let mut current = target.clone();
    let mut layers = Vec::new();
    let mut visited = BTreeSet::new();
    let terminal = loop {
        budget.tick()?;
        let file = name(&current, "file")?.to_owned();
        let local = name(&current, "name")?.to_owned();
        let key = (
            name(&current, "packageName")?.to_owned(),
            name(&current, "sourceSha256")?.to_owned(),
            file.clone(),
            local.clone(),
        );
        if !visited.insert(key) {
            return Err("EXTERNAL_STYLE_CYCLE".into());
        }
        let View {
            analysis,
            declaration,
        } = view(report, &current, &mut budget)?;
        if declaration["kind"] == "function" {
            if current["packageName"] != "@jsonresume/core" {
                return Err("EXTERNAL_STYLE_FUNCTION_UNSUPPORTED".into());
            }
            break json!({"kind":"core","target":current});
        }
        if layers.len() >= MAX_DEPTH {
            return Err("EXTERNAL_STYLE_DEPTH_LIMIT".into());
        }
        if declaration["kind"] != "const"
            || declaration["mutable"] != false
            || declaration["synthetic"] != false
        {
            return Err("EXTERNAL_STYLE_REQUIRES_CONST".into());
        }
        let style = one(
            items(analysis, "styledComponents").filter(|s| s["name"] == local),
            "EXTERNAL_STYLE_DEFINITION_AMBIGUOUS_OR_MISSING",
        )?;
        let (start, end) = position(&style["span"], &file)?;
        let (decl_start, decl_end) = position(&declaration["span"], &file)?;
        if style["topLevelBinding"] != true
            || !same_span(&declaration["initializer"]["span"], &style["span"], &file)
            || declaration["initializer"]["astKind"] != "call_expression"
            || start < decl_start
            || end > decl_end
        {
            return Err("EXTERNAL_STYLE_INITIALIZER_SPAN_MISMATCH".into());
        }
        layers.push(
            json!({"packageName":current["packageName"],"sourceSha256":current["sourceSha256"],
            "file":file,"fileSha256":current["fileSha256"],"name":local,"span":style["span"]}),
        );
        let base = &style["base"];
        match base["kind"].as_str() {
            Some("element") => {
                let tag = name(base, "name")?;
                if !TAGS.contains(&tag) {
                    return Err("EXTERNAL_STYLE_INTRINSIC_UNSUPPORTED".into());
                }
                break json!({"kind":"intrinsic","name":tag});
            }
            Some("component") => {
                let next = name(base, "name")?;
                let symbols = &analysis["moduleSymbols"];
                let declarations = items(symbols, "declarations")
                    .filter(|d| d["name"] == next)
                    .collect::<Vec<_>>();
                let imports = items(symbols, "imports")
                    .filter(|i| i["local"] == next)
                    .collect::<Vec<_>>();
                if declarations.len() + imports.len() != 1 {
                    return Err("EXTERNAL_STYLE_BASE_AMBIGUOUS_OR_MISSING".into());
                }
                let target = if let Some(declaration) = declarations.first() {
                    if declaration["kind"] != "const" || declaration["mutable"] != false {
                        return Err("EXTERNAL_STYLE_BASE_REQUIRES_CONST".into());
                    }
                    local_target(&current, declaration)
                } else {
                    let import = imports[0];
                    if import["supported"] != true
                        || !matches!(import["kind"].as_str(), Some("named" | "default"))
                    {
                        return Err("EXTERNAL_STYLE_IMPORT_UNSUPPORTED".into());
                    }
                    budget.tick()?;
                    let target = crate::symbol_link::imported_target(
                        report,
                        &current,
                        &file,
                        name(import, "module")?,
                        name(import, "imported")?,
                    )?;
                    budget.tick()?;
                    target
                };
                initialized_before(&target, &current, start)?;
                current = target;
            }
            _ => return Err("EXTERNAL_STYLE_HELPER_OR_BASE_UNSUPPORTED".into()),
        }
    };
    budget.tick()?;
    layers.reverse();
    Ok(
        json!({"format":"resume-one-external-style-chain-v1","layers":layers,"layerOrder":"base-to-derived",
        "terminal":terminal,"declarationChainResolved":true,"bindingValueProven":false,
        "stylesCompiled":false,"layoutCompiled":false,"moduleEvaluationProven":false,
        "originalStyleBlockersCleared":false,"executedSourceCode":false}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (Value, Value) {
        let span = json!({"file":"styles.jsx","startByte":10,"endByte":30});
        let declaration = json!({"name":"Box","id":"symbol:6:Box","kind":"const","mutable":false,"synthetic":false,
            "span":{"file":"styles.jsx","startByte":6,"endByte":30},
            "initializer":{"astKind":"call_expression","span":span}});
        let sha = "a".repeat(64);
        let file_sha = "b".repeat(64);
        let target = json!({"packageName":"theme","packageVersion":"1.0.0","sourceSha256":sha,"file":"styles.jsx",
            "fileSha256":file_sha,"lookup":"local","symbol":"Box","name":"Box","span":declaration["span"],"declaration":declaration});
        let report = json!({"package":{"name":"theme","version":"1.0.0"},
            "sourceSnapshot":{"complete":true,"sourceSha256":sha,"files":[{"path":"styles.jsx","sha256":file_sha,"bytes":1000,"utf8":true}]},
            "syntaxEvidence":[{"file":"styles.jsx","syntaxValid":true,"moduleSymbols":{"syntaxValid":true,
                "declarations":[target["declaration"]],"imports":[],"blockers":[]},
                "styledComponents":[{"name":"Box","topLevelBinding":true,"span":span,"base":{"kind":"element","name":"div"},"css":null}]}]});
        (report, target)
    }

    #[test]
    fn intrinsic_layer_keeps_identity_and_original_uncompiled_css() {
        let (report, target) = fixture();
        let original = report.clone();
        let out = resolve(&report, &target).unwrap();
        assert_eq!(out["terminal"], json!({"kind":"intrinsic","name":"div"}));
        assert_eq!(
            out["layers"][0]["span"],
            target["declaration"]["initializer"]["span"]
        );
        assert_eq!(out["layoutCompiled"], false);
        assert_eq!(report, original);
    }

    #[test]
    fn mutable_missing_helpers_and_span_mismatch_are_rejected() {
        for variant in 0..4 {
            let (mut report, mut target) = fixture();
            match variant {
                0 => {
                    target["declaration"]["mutable"] = json!(true);
                    report["syntaxEvidence"][0]["moduleSymbols"]["declarations"][0] =
                        target["declaration"].clone();
                }
                1 => report["syntaxEvidence"][0]["styledComponents"] = json!([]),
                2 => {
                    report["syntaxEvidence"][0]["styledComponents"][0]["base"] =
                        json!({"kind":"fragment"})
                }
                _ => {
                    report["syntaxEvidence"][0]["styledComponents"][0]["span"]["endByte"] =
                        json!(29)
                }
            }
            assert!(resolve(&report, &target).is_err());
        }
    }

    #[test]
    fn collisions_and_file_identity_mismatch_fail_closed() {
        let (mut report, target) = fixture();
        report["syntaxEvidence"][0]["moduleSymbols"]["imports"] = json!([{"local":"Box"}]);
        assert!(resolve(&report, &target).unwrap_err().contains("COLLISION"));
        let (mut report, target) = fixture();
        report["sourceSnapshot"]["files"][0]["sha256"] = json!("c".repeat(64));
        assert!(
            resolve(&report, &target)
                .unwrap_err()
                .contains("HASH_MISMATCH")
        );
    }

    #[test]
    fn local_base_requires_prior_complete_initialization_and_preserves_order() {
        let (mut report, mut target) = fixture();
        let base = report["syntaxEvidence"][0]["moduleSymbols"]["declarations"][0].clone();
        let base_style = report["syntaxEvidence"][0]["styledComponents"][0].clone();
        target["name"] = json!("Child");
        target["symbol"] = json!("Child");
        target["declaration"]["name"] = json!("Child");
        target["declaration"]["span"] = json!({"file":"styles.jsx","startByte":40,"endByte":70});
        target["declaration"]["initializer"]["span"] =
            json!({"file":"styles.jsx","startByte":50,"endByte":70});
        target["span"] = target["declaration"]["span"].clone();
        let child = json!({"name":"Child","topLevelBinding":true,"span":target["declaration"]["initializer"]["span"],"base":{"kind":"component","name":"Box"}});
        report["syntaxEvidence"][0]["moduleSymbols"]["declarations"] =
            json!([base, target["declaration"]]);
        report["syntaxEvidence"][0]["styledComponents"] = json!([base_style, child]);
        let out = resolve(&report, &target).unwrap();
        assert_eq!(out["layers"][0]["name"], "Box");
        assert_eq!(out["layers"][1]["name"], "Child");
        report["syntaxEvidence"][0]["moduleSymbols"]["declarations"][0]["initializer"]["span"]["endByte"] =
            json!(55);
        assert_eq!(
            resolve(&report, &target).unwrap_err(),
            "EXTERNAL_STYLE_BASE_TDZ"
        );
    }

    #[test]
    fn self_cycles_and_unresolved_imports_are_not_intrinsic_fallbacks() {
        let (mut report, target) = fixture();
        report["syntaxEvidence"][0]["styledComponents"][0]["base"] =
            json!({"kind":"component","name":"Box"});
        assert!(resolve(&report, &target).unwrap_err().contains("TDZ"));
        let (mut report, target) = fixture();
        report["syntaxEvidence"][0]["styledComponents"][0]["base"] =
            json!({"kind":"component","name":"External"});
        report["syntaxEvidence"][0]["moduleSymbols"]["imports"] = json!([{"local":"External","module":"not-provided","imported":"Thing","kind":"named","supported":true}]);
        assert!(resolve(&report, &target).is_err());
    }
}
