//! Links declarations, not executable modules or rendered components. All input
//! evidence comes from the same frozen snapshots as syntax analysis.

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

const MAX_REQUESTS: usize = 4096;
const MAX_STEPS: usize = 16_384;
const MAX_DEPTH: usize = 64;

fn array<'a>(value: &'a Value, key: &str) -> impl Iterator<Item = &'a Value> {
    value[key].as_array().into_iter().flatten()
}

fn exact_local(current: &str, module: &str) -> Result<String, String> {
    if module.len() > 512
        || !(module.starts_with("./") || module.starts_with("../"))
        || module
            .bytes()
            .any(|b| !(0x21..=0x7e).contains(&b) || b"\\:%?#*<>\"|".contains(&b))
    {
        return Err("SYMBOL_IMPORT_PATH_UNSUPPORTED".into());
    }
    let mut parts = current.split('/').collect::<Vec<_>>();
    parts.pop();
    for part in module.split('/') {
        match part {
            "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err("SYMBOL_IMPORT_OUTSIDE_PACKAGE".into());
                }
            }
            "" | "node_modules" => return Err("SYMBOL_IMPORT_PATH_UNSUPPORTED".into()),
            _ if part.ends_with([' ', '.']) => return Err("SYMBOL_IMPORT_PATH_UNSUPPORTED".into()),
            _ => parts.push(part),
        }
    }
    if parts.is_empty() {
        return Err("SYMBOL_IMPORT_PATH_UNSUPPORTED".into());
    }
    Ok(parts.join("/"))
}

struct Linker<'a> {
    root: &'a Value,
    packages: Vec<&'a Value>,
    steps: usize,
    started: Instant,
}

impl<'a> Linker<'a> {
    fn package(&self, index: usize) -> &'a Value {
        if index == 0 {
            self.root
        } else {
            self.packages[index - 1]
        }
    }

    fn identity(&self, index: usize, file: &str) -> Result<Value, String> {
        let package = self.package(index);
        let matches = array(&package["sourceSnapshot"], "files")
            .filter(|f| f["path"] == file)
            .collect::<Vec<_>>();
        if matches.len() != 1 || matches[0]["sha256"].as_str().is_none_or(|s| s.len() != 64) {
            return Err("SYMBOL_FILE_MISSING".into());
        }
        Ok(
            json!({"packageName":if index == 0 { &package["package"]["name"] } else { &package["name"] },
            "packageVersion":if index == 0 { &package["package"]["version"] } else { &package["version"] },
            "sourceSha256":package["sourceSnapshot"]["sourceSha256"],"file":file,"fileSha256":matches[0]["sha256"]}),
        )
    }

    fn symbols(&self, index: usize, file: &str) -> Result<&'a Value, String> {
        self.identity(index, file)?;
        let matches = array(self.package(index), "syntaxEvidence")
            .filter(|f| f["file"] == file)
            .collect::<Vec<_>>();
        if matches.len() != 1 || matches[0]["syntaxValid"] != true {
            return Err("SYMBOL_MODULE_SYNTAX_UNSUPPORTED".into());
        }
        let symbols = &matches[0]["moduleSymbols"];
        if !symbols.is_object() {
            return Err("SYMBOL_MODULE_EVIDENCE_MISSING".into());
        }
        if symbols["syntaxValid"] != true {
            return Err("SYMBOL_MODULE_SYNTAX_UNSUPPORTED".into());
        }
        if array(symbols, "blockers").any(|issue| {
            matches!(
                issue["code"].as_str(),
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
            return Err("SYMBOL_MODULE_SCOPE_UNPROVEN".into());
        }
        Ok(symbols)
    }

    fn target(&self, index: usize, file: &str, module: &str) -> Result<(usize, String), String> {
        if module.starts_with('.') {
            let path = exact_local(file, module)?;
            self.identity(index, &path)?;
            return Ok((index, path));
        }
        if module.len() > 512 || module.contains(['\\', ':', '%', '?', '#', '*']) {
            return Err("SYMBOL_PACKAGE_SPECIFIER_UNSUPPORTED".into());
        }
        let parts = module.split('/').collect::<Vec<_>>();
        let count = if module.starts_with('@') { 2 } else { 1 };
        if parts.len() < count
            || parts
                .iter()
                .any(|p| p.is_empty() || matches!(*p, "." | ".."))
        {
            return Err("SYMBOL_PACKAGE_SPECIFIER_UNSUPPORTED".into());
        }
        let name = parts[..count].join("/");
        let subpath = if parts.len() == count {
            ".".to_owned()
        } else {
            format!("./{}", parts[count..].join("/"))
        };
        let matches = self
            .packages
            .iter()
            .enumerate()
            .filter(|(_, p)| p["name"] == name)
            .collect::<Vec<_>>();
        if matches.len() != 1 {
            return Err("SYMBOL_PACKAGE_NOT_PROVIDED".into());
        }
        let (at, package) = matches[0];
        let entries = array(package, "exportsEntries")
            .filter(|e| e["subpath"] == subpath)
            .collect::<Vec<_>>();
        if entries.len() != 1 || entries[0]["status"] != "file-present" {
            return Err("SYMBOL_PACKAGE_EXPORT_UNRESOLVED".into());
        }
        let path = entries[0]["resolvedFile"]
            .as_str()
            .ok_or("SYMBOL_PACKAGE_EXPORT_UNRESOLVED")?;
        let identity = self.identity(at + 1, path)?;
        if identity["fileSha256"] != entries[0]["sha256"] {
            return Err("SYMBOL_EXPORT_HASH_MISMATCH".into());
        }
        Ok((at + 1, path.to_owned()))
    }

    fn budget(&mut self, depth: usize) -> Result<(), String> {
        self.steps += 1;
        if depth >= MAX_DEPTH
            || self.steps > MAX_STEPS
            || self.started.elapsed() > Duration::from_secs(10)
        {
            return Err("SYMBOL_LINK_LIMIT".into());
        }
        Ok(())
    }

    fn resolve(
        &mut self,
        mut index: usize,
        file: &str,
        exported: &str,
        trace: &mut Vec<Value>,
    ) -> Result<Value, String> {
        let mut file = file.to_owned();
        let mut symbol = exported.to_owned();
        let mut mode = "export";
        let mut visited = BTreeSet::new();
        loop {
            self.budget(trace.len())?;
            if symbol == "*" {
                return Err("SYMBOL_NAMESPACE_UNSUPPORTED".into());
            }
            if !visited.insert((index, file.clone(), symbol.clone(), mode)) {
                return Err("SYMBOL_EXPORT_CYCLE".into());
            }
            let symbols = self.symbols(index, &file)?;
            let mut step = self.identity(index, &file)?;
            step["lookup"] = json!(mode);
            step["symbol"] = json!(symbol);
            if mode == "export" {
                let entries = array(symbols, "exports")
                    .filter(|e| e["exported"] == symbol)
                    .collect::<Vec<_>>();
                if entries.len() != 1 {
                    return Err(if entries.is_empty() {
                        "SYMBOL_EXPORT_NOT_FOUND"
                    } else {
                        "SYMBOL_EXPORT_AMBIGUOUS"
                    }
                    .into());
                }
                let entry = entries[0];
                if entry["supported"] != true {
                    return Err("SYMBOL_EXPORT_UNSUPPORTED".into());
                }
                step["span"] = entry["span"].clone();
                step["bindingMode"] = entry["bindingMode"].clone();
                trace.push(step);
                match entry["kind"].as_str() {
                    Some("local") => {
                        symbol = entry["local"]
                            .as_str()
                            .ok_or("SYMBOL_LOCAL_UNSUPPORTED")?
                            .to_owned();
                        mode = "local";
                    }
                    Some("reexport") => {
                        let module = entry["module"]
                            .as_str()
                            .ok_or("SYMBOL_REEXPORT_UNSUPPORTED")?;
                        let next = self.target(index, &file, module)?;
                        index = next.0;
                        file = next.1;
                        symbol = entry["imported"]
                            .as_str()
                            .ok_or("SYMBOL_REEXPORT_UNSUPPORTED")?
                            .to_owned();
                    }
                    _ => return Err("SYMBOL_EXPORT_UNSUPPORTED".into()),
                }
            } else {
                let declarations = array(symbols, "declarations")
                    .filter(|d| d["name"] == symbol)
                    .collect::<Vec<_>>();
                let imports = array(symbols, "imports")
                    .filter(|i| i["local"] == symbol)
                    .collect::<Vec<_>>();
                if declarations.len() + imports.len() != 1 {
                    return Err("SYMBOL_LOCAL_AMBIGUOUS_OR_MISSING".into());
                }
                if let Some(declaration) = declarations.first() {
                    if declaration["mutable"] != false && declaration["kind"] != "function" {
                        return Err("SYMBOL_MUTABLE_DECLARATION".into());
                    }
                    step["span"] = declaration["span"].clone();
                    trace.push(step.clone());
                    step["name"] = json!(symbol);
                    step["declaration"] = (*declaration).clone();
                    step["bindingValueProven"] = json!(false);
                    return Ok(step);
                }
                let import = imports[0];
                if import["supported"] != true {
                    return Err("SYMBOL_IMPORT_UNSUPPORTED".into());
                }
                step["span"] = import["span"].clone();
                trace.push(step);
                let next = self.target(
                    index,
                    &file,
                    import["module"]
                        .as_str()
                        .ok_or("SYMBOL_IMPORT_UNSUPPORTED")?,
                )?;
                index = next.0;
                file = next.1;
                symbol = import["imported"]
                    .as_str()
                    .ok_or("SYMBOL_IMPORT_UNSUPPORTED")?
                    .to_owned();
                mode = "export";
            }
        }
    }
}

pub(crate) fn link(root: &Value, bundle: &Value) -> Result<Value, String> {
    let mut linker = Linker {
        root,
        packages: array(bundle, "packages").collect(),
        steps: 0,
        started: Instant::now(),
    };
    let mut requests = Vec::new();
    let mut blockers = Vec::new();
    for template in array(root, "templatePrograms") {
        let file = template["entryFile"]
            .as_str()
            .ok_or("SYMBOL_ENTRY_FILE_MISSING")?;
        let symbols = match linker.symbols(0, file) {
            Ok(symbols) => symbols,
            Err(code) => {
                blockers.push(json!({"code":code,"stage":"symbol-link","file":file,"message":"Entry module cannot establish an unambiguous declaration namespace"}));
                continue;
            }
        };
        for import in array(symbols, "imports") {
            linker.budget(0)?;
            if requests.len() >= MAX_REQUESTS {
                return Err("SYMBOL_REQUEST_LIMIT".into());
            }
            let mut request = json!({"entryFile":file,"import":import,"declarationResolved":false,"semanticsCompiled":false});
            let mut trace = Vec::new();
            let result = (|| {
                if import["imported"] == "*" {
                    return Err("SYMBOL_NAMESPACE_UNSUPPORTED".into());
                }
                if import["supported"] != true {
                    return Err("SYMBOL_IMPORT_UNSUPPORTED".into());
                }
                let module = import["module"]
                    .as_str()
                    .ok_or("SYMBOL_IMPORT_UNSUPPORTED")?;
                let exported = import["imported"]
                    .as_str()
                    .ok_or("SYMBOL_IMPORT_UNSUPPORTED")?;
                let (index, target) = linker.target(0, file, module)?;
                linker.resolve(index, &target, exported, &mut trace)
            })();
            request["trace"] = json!(trace);
            match result {
                Ok(declaration) => {
                    request["declarationResolved"] = json!(true);
                    request["target"] = declaration;
                }
                Err(code) if code == "SYMBOL_LINK_LIMIT" => return Err(code),
                Err(code) => {
                    request["blocker"] = json!(code);
                    blockers.push(json!({"code":code,"stage":"symbol-link","file":file,"span":import["span"],"module":import["module"],"local":import["local"],"message":"Requested import has no unique, exact supported declaration chain in the supplied snapshots"}));
                }
            }
            requests.push(request);
        }
    }
    linker.budget(0)?;
    let resolved = requests
        .iter()
        .filter(|r| r["declarationResolved"] == true)
        .count();
    Ok(
        json!({"format":"resume-one-module-links-v1","requests":requests,"statistics":{"requested":requests.len(),"declarationResolved":resolved},
        "allRequestedDeclarationsResolved":!requests.is_empty() && blockers.is_empty(),"blockers":blockers,
        "moduleEvaluationProven":false,"componentSemanticsCompiled":false,"dependencyGraphComplete":false,"migrationComplete":false,"executedSourceCode":false}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn fixture(import: &str, files: &[(&str, &str)]) -> Value {
        let directory = tempfile::tempdir().unwrap();
        let theme = directory.path().join("theme");
        let core = directory.path().join("core");
        fs::create_dir_all(&theme).unwrap();
        fs::create_dir_all(&core).unwrap();
        fs::write(
            theme.join("package.json"),
            r#"{"name":"theme","version":"1.0.0"}"#,
        )
        .unwrap();
        fs::write(
            theme.join("Resume.jsx"),
            format!("{import} export default function Resume() {{return <p/>;}}"),
        )
        .unwrap();
        fs::write(
            core.join("package.json"),
            r#"{"name":"@jsonresume/core","version":"0.3.6","exports":"./index.js"}"#,
        )
        .unwrap();
        for (path, source) in files {
            let target = core.join(path);
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::write(target, source).unwrap();
        }
        let root = crate::inspection::inspect_templates(
            &theme.canonicalize().unwrap(),
            &["Resume.jsx".into()],
        )
        .unwrap();
        let bundle = crate::dependency_bundle::inspect(&[core.canonicalize().unwrap()]).unwrap();
        link(&root, &bundle).unwrap()
    }

    #[test]
    fn follows_real_named_barrels_and_preserves_each_snapshot_identity() {
        let value = fixture(
            "import {Section as Local} from '@jsonresume/core';",
            &[
                (
                    "index.js",
                    "export {Section} from './primitives/index.jsx'; export {Missing} from './missing.js';",
                ),
                (
                    "primitives/index.jsx",
                    "export {Section} from './Section.jsx';",
                ),
                (
                    "primitives/Section.jsx",
                    "export function Section({children}) {return <section>{children}</section>;} export default Section;",
                ),
            ],
        );
        let request = &value["requests"][0];
        assert_eq!(request["declarationResolved"], true, "{value:#}");
        assert_eq!(request["target"]["name"], "Section");
        assert_eq!(request["target"]["file"], "primitives/Section.jsx");
        assert_eq!(request["target"]["fileSha256"].as_str().unwrap().len(), 64);
        assert_eq!(request["trace"].as_array().unwrap().len(), 4);
        assert_eq!(value["componentSemanticsCompiled"], false);
        assert_eq!(value["migrationComplete"], false);
    }

    #[test]
    fn follows_local_import_alias_and_default_export_without_name_guessing() {
        let value = fixture(
            "import {Public} from '@jsonresume/core';",
            &[
                (
                    "index.js",
                    "import Local from './component.jsx'; export {Local as Public};",
                ),
                (
                    "component.jsx",
                    "function Real(){return <section/>;} export default Real;",
                ),
            ],
        );
        assert_eq!(
            value["requests"][0]["declarationResolved"], true,
            "{value:#}"
        );
        assert_eq!(value["requests"][0]["target"]["name"], "Real");
    }

    #[test]
    fn absent_export_does_not_link_a_same_named_private_declaration() {
        let value = fixture(
            "import {Section} from '@jsonresume/core';",
            &[("index.js", "function Section(){return <section/>;}")],
        );
        assert_eq!(value["requests"][0]["blocker"], "SYMBOL_EXPORT_NOT_FOUND");
    }

    #[test]
    fn exact_file_resolution_does_not_guess_extensions_or_escape_package() {
        for (specifier, code) in [
            ("./Section", "SYMBOL_FILE_MISSING"),
            ("../Section.jsx", "SYMBOL_IMPORT_OUTSIDE_PACKAGE"),
        ] {
            let value = fixture(
                "import {Section} from '@jsonresume/core';",
                &[
                    (
                        "index.js",
                        &format!("export {{Section}} from '{specifier}';"),
                    ),
                    (
                        "Section.jsx",
                        "export function Section(){return <section/>;}",
                    ),
                ],
            );
            assert_eq!(value["requests"][0]["blocker"], code, "{value:#}");
        }
    }

    #[test]
    fn cycles_terminate_with_a_retained_trace() {
        let value = fixture(
            "import {Section} from '@jsonresume/core';",
            &[
                ("index.js", "export {Section} from './other.js';"),
                ("other.js", "export {Section} from './index.js';"),
            ],
        );
        assert_eq!(value["requests"][0]["blocker"], "SYMBOL_EXPORT_CYCLE");
        assert_eq!(value["requests"][0]["trace"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn namespaces_and_unprovided_packages_remain_blocked() {
        let value = fixture(
            "import * as core from '@jsonresume/core'; import React from 'react';",
            &[("index.js", "export const Section = 1;")],
        );
        assert_eq!(
            value["requests"][0]["blocker"],
            "SYMBOL_NAMESPACE_UNSUPPORTED"
        );
        assert_eq!(
            value["requests"][1]["blocker"],
            "SYMBOL_PACKAGE_NOT_PROVIDED"
        );
        assert_eq!(value["allRequestedDeclarationsResolved"], false);
    }

    #[test]
    fn malformed_and_mutable_declarations_are_not_certified() {
        let mutable = fixture(
            "import {Section} from '@jsonresume/core';",
            &[("index.js", "export let Section = 1;")],
        );
        assert_eq!(
            mutable["requests"][0]["blocker"],
            "SYMBOL_MUTABLE_DECLARATION"
        );
        let broken = fixture(
            "import {Section} from '@jsonresume/core';",
            &[("index.js", "export const Section = ;")],
        );
        assert_eq!(
            broken["requests"][0]["blocker"],
            "SYMBOL_MODULE_SYNTAX_UNSUPPORTED"
        );
    }

    #[test]
    fn no_requests_is_not_success_and_path_guards_are_explicit() {
        let value = fixture("", &[("index.js", "export const Section = 1;")]);
        assert_eq!(value["allRequestedDeclarationsResolved"], false);
        for path in [
            "./%2e/Section.js",
            "./node_modules/Section.js",
            "./s.js?raw",
            "./a//b",
            "./a\\b",
        ] {
            assert!(exact_local("index.js", path).is_err(), "{path}");
        }
    }

    #[test]
    fn time_step_and_depth_budgets_fail_even_without_a_resolved_target() {
        let root = json!({});
        let mut linker = Linker {
            root: &root,
            packages: vec![],
            steps: 0,
            started: Instant::now(),
        };
        assert!(linker.budget(MAX_DEPTH).is_err());
        linker.steps = MAX_STEPS;
        assert!(linker.budget(0).is_err());
        linker.steps = 0;
        linker.started = Instant::now() - Duration::from_secs(11);
        assert!(linker.budget(0).is_err());
    }
}
