//! A content-only adapter for one pinned core Section implementation.
//!
//! This consumes inspection evidence, not JavaScript or source snippets. It does
//! not execute modules, resolve children, compile StyledSection, or remove any
//! frontend blocker. A lowered wrapper is not a resolved component tree.

use std::collections::BTreeSet;
use std::io::{self, Write};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

const PACKAGE: &str = "@jsonresume/core";
const VERSION: &str = "0.3.6";
const FILE: &str = "src/primitives/Section.jsx";
const FILE_SHA: &str = "51a0b42151195761af25b97113e9e838b7275054ec74432263e07e185c2c4c05";
const PROGRAM_ID: &str = "core-section-0.3.6-51a0b421-content-v1";
const MAX_BYTES: usize = 16 * 1024 * 1024;
const MAX_INPUT_NODES: usize = 250_000;
const MAX_DEPTH: usize = 128;
const MAX_WALK_NODES: usize = 50_000;
const MAX_INSTANCES: usize = 2048;
const MAX_REQUESTS: usize = 4096;

fn items<'a>(value: &'a Value, key: &str) -> impl Iterator<Item = &'a Value> {
    value[key].as_array().into_iter().flatten()
}

fn hash(value: &Value) -> bool {
    value.as_str().is_some_and(|s| {
        s.len() == 64
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}

struct SizeLimit(usize);

impl Write for SizeLimit {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0 = self
            .0
            .checked_add(bytes.len())
            .filter(|n| *n <= MAX_BYTES)
            .ok_or_else(|| io::Error::other("SECTION_ADAPTER_BYTE_LIMIT"))?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn size(value: &Value) -> Result<usize, String> {
    let mut limit = SizeLimit(0);
    serde_json::to_writer(&mut limit, value)
        .map_err(|_| "SECTION_ADAPTER_BYTE_LIMIT".to_owned())?;
    Ok(limit.0)
}

fn bounded(value: &Value) -> Result<(), String> {
    // Iterative depth checking happens before serde serialization or cloning.
    let mut pending = vec![(value, 0usize)];
    let mut count = 0usize;
    while let Some((node, depth)) = pending.pop() {
        count += 1;
        if count > MAX_INPUT_NODES || depth > MAX_DEPTH {
            return Err("SECTION_ADAPTER_INPUT_LIMIT".into());
        }
        match node {
            Value::Array(values) => pending.extend(values.iter().map(|v| (v, depth + 1))),
            Value::Object(values) => pending.extend(values.values().map(|v| (v, depth + 1))),
            _ => {}
        }
        if pending.len() > MAX_INPUT_NODES {
            return Err("SECTION_ADAPTER_INPUT_LIMIT".into());
        }
    }
    size(value)?;
    Ok(())
}

fn span(value: &Value, file: &str, start: u64, end: u64) -> bool {
    value["file"] == file
        && value["startByte"].as_u64() == Some(start)
        && value["endByte"].as_u64() == Some(end)
}

fn same_span(a: &Value, b: &Value) -> bool {
    a.is_object()
        && a["file"].is_string()
        && a["startByte"].as_u64().is_some()
        && a["endByte"].as_u64().is_some()
        && a["file"] == b["file"]
        && a["startByte"] == b["startByte"]
        && a["endByte"] == b["endByte"]
}

fn binding(import: &Value) -> Option<String> {
    let local = import["local"].as_str()?;
    let local_span = if import["localSpan"].is_object() {
        &import["localSpan"]
    } else {
        &import["span"]
    };
    let start = local_span["startByte"].as_u64()?;
    let end = local_span["endByte"].as_u64()?;
    if local.is_empty() || end.checked_sub(start)? != local.len() as u64 {
        return None;
    }
    Some(format!("b:{start}:{local}"))
}

fn ecma_space(c: char) -> bool {
    // ECMA WhiteSpace + LineTerminator, deliberately not Rust's Unicode trim.
    matches!(c, '\u{0009}'..='\u{000d}' | '\u{0020}' | '\u{00a0}' | '\u{1680}'
        | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}'
        | '\u{205f}' | '\u{3000}' | '\u{feff}')
}

fn class_name(value: &str) -> String {
    // For the supported string-only domain, empty string is the only falsy
    // string. Do not pre-trim, normalize, or deduplicate user class names.
    format!("resume-section {value}")
        .trim_matches(ecma_space)
        .to_owned()
}

fn style_dependency() -> Value {
    json!({"packageName":PACKAGE,"packageVersion":VERSION,"file":FILE,
        "fileSha256":FILE_SHA,"declaration":"StyledSection",
        "stylesCompiled":false,"layoutCompiled":false,
        "requiresOriginalStyleCompilation":true,
        "uncompiledFeatures":["theme.spacing.section fallback", "print page-break-inside"]})
}

fn component_program() -> Value {
    json!({"id":PROGRAM_ID,"kind":"pinned-component-content-subset",
        "source":{"packageName":PACKAGE,"packageVersion":VERSION,"file":FILE,
            "fileSha256":FILE_SHA,"actualDeclarationName":"Section"},
        "operations":[
            {"op":"semantic-boundary","tag":"section"},
            {"op":"forward-optional-literal-string","from":"id","to":"id"},
            {"op":"class-name","prefix":"resume-section ","fallback":"",
                "inputDomain":"literal-string-or-absent","trim":"ECMA-WhiteSpace-and-LineTerminator"},
            {"op":"forward-children-ir-verbatim","evaluate":false,"preserveOrder":true},
            {"op":"retain-logical-key","domAttribute":false,"evaluate":false}
        ],"supportedProps":["id","className","key"],
        "styleDependencies":[style_dependency()],"componentSemanticsCompiled":false,
        "childrenResolved":false,"layoutCompiled":false,"moduleEvaluationProven":false})
}

fn unique<'a>(mut values: impl Iterator<Item = &'a Value>, why: &str) -> Result<&'a Value, String> {
    let value = values.next().ok_or_else(|| why.to_owned())?;
    if values.next().is_some() {
        return Err(why.into());
    }
    Ok(value)
}

fn prove_target(report: &Value, request: &Value) -> Result<Value, String> {
    let target = &request["target"];
    let declaration = &target["declaration"];
    if request["declarationResolved"] != true
        || target["packageName"] != PACKAGE
        || target["packageVersion"] != VERSION
        || target["file"] != FILE
        || target["fileSha256"] != FILE_SHA
        || !hash(&target["sourceSha256"])
        || target["lookup"] != "local"
        || target["symbol"] != "Section"
        || target["name"] != "Section"
        || declaration["name"] != "Section"
        || declaration["id"] != "symbol:545:Section"
        || declaration["kind"] != "function"
        || declaration["synthetic"] != false
        || declaration["initializer"]["form"] != "function"
        || declaration["initializer"]["astKind"] != "function_declaration"
        || !span(&declaration["nameSpan"], FILE, 545, 552)
        || !span(&declaration["span"], FILE, 536, 764)
        || !same_span(&target["span"], &declaration["span"])
    {
        return Err("SECTION_PINNED_DECLARATION_MISMATCH".into());
    }
    // A function declaration is mutable in module-symbol evidence. We preserve
    // the linker's lack of runtime/binding-value proof instead of inventing one.
    let package = unique(
        items(&report["dependencyBundle"], "packages").filter(|p| p["name"] == PACKAGE),
        "SECTION_DEPENDENCY_PACKAGE_MISSING_OR_AMBIGUOUS",
    )?;
    if package["version"] != VERSION
        || package["package"]["name"] != PACKAGE
        || package["package"]["version"] != VERSION
        || package["sourceSnapshot"]["complete"] != true
        || package["sourceSha256"] != target["sourceSha256"]
        || package["sourceSnapshot"]["sourceSha256"] != target["sourceSha256"]
    {
        return Err("SECTION_DEPENDENCY_IDENTITY_MISMATCH".into());
    }
    let file = unique(
        items(&package["sourceSnapshot"], "files").filter(|f| f["path"] == FILE),
        "SECTION_DEPENDENCY_FILE_MISSING_OR_AMBIGUOUS",
    )?;
    if file["sha256"] != FILE_SHA || file["bytes"] != 790 || file["utf8"] != true {
        return Err("SECTION_DEPENDENCY_FILE_HASH_MISMATCH".into());
    }
    let evidence = unique(
        items(package, "syntaxEvidence").filter(|f| f["file"] == FILE),
        "SECTION_DEPENDENCY_SYNTAX_MISSING_OR_AMBIGUOUS",
    )?;
    if evidence["syntaxValid"] != true || evidence["moduleSymbols"]["syntaxValid"] != true {
        return Err("SECTION_DEPENDENCY_SYNTAX_INVALID".into());
    }
    let actual = unique(
        items(&evidence["moduleSymbols"], "declarations").filter(|d| d["name"] == "Section"),
        "SECTION_DEPENDENCY_DECLARATION_MISSING_OR_AMBIGUOUS",
    )?;
    if actual != declaration {
        return Err("SECTION_DEPENDENCY_DECLARATION_MISMATCH".into());
    }
    Ok(
        json!({"packageName":PACKAGE,"packageVersion":VERSION,"file":FILE,
        "fileSha256":FILE_SHA,"sourceSha256":target["sourceSha256"],
        "actualDeclarationName":"Section","bindingValueProven":false}),
    )
}

fn prove_import(report: &Value, template: &Value, tag: &Value) -> Result<Value, String> {
    let entry = template["entryFile"]
        .as_str()
        .ok_or("SECTION_ENTRY_FILE_MISSING")?;
    if report["moduleLinks"]["format"] != "resume-one-module-links-v1"
        || template["program"]["format"] != "resume-one-template-ir-v1"
        || template["program"]["syntaxValid"] != true
        || template["program"]["source"]["path"] != entry
        || report["sourceSnapshot"]["complete"] != true
    {
        return Err("SECTION_INPUT_EVIDENCE_UNSUPPORTED".into());
    }
    let root_file = unique(
        items(&report["sourceSnapshot"], "files").filter(|f| f["path"] == entry),
        "SECTION_ROOT_FILE_MISSING_OR_AMBIGUOUS",
    )?;
    if !hash(&template["sourceSha256"]) || root_file["sha256"] != template["sourceSha256"] {
        return Err("SECTION_ROOT_SOURCE_HASH_MISMATCH".into());
    }
    let request = unique(
        items(&report["moduleLinks"], "requests").filter(|r| {
            r["entryFile"] == entry && binding(&r["import"]).is_some_and(|b| tag["binding"] == b)
        }),
        "SECTION_IMPORT_LINK_MISSING_OR_AMBIGUOUS",
    )?;
    let import = &request["import"];
    if import["supported"] != true
        || !matches!(import["kind"].as_str(), Some("named" | "default"))
        || import["span"]["file"] != entry
        || !import["module"].is_string()
        || !import["imported"].is_string()
        || tag["name"] != import["local"]
        || tag["module"] != import["module"]
        || tag["exported"] != import["imported"]
    {
        return Err("SECTION_DIRECT_IMPORT_BINDING_MISMATCH".into());
    }
    let mut matches = 0;
    for dependency in items(&template["program"], "dependencies") {
        if dependency["kind"] != "import" || dependency["module"] != import["module"] {
            continue;
        }
        for specifier in items(dependency, "specifiers") {
            if specifier["local"] == import["local"]
                && specifier["exported"] == import["imported"]
                && same_span(
                    &specifier["span"],
                    if import["localSpan"].is_object() {
                        &import["localSpan"]
                    } else {
                        &import["span"]
                    },
                )
                && dependency["span"]["file"] == entry
                && dependency["span"]["startByte"]
                    .as_u64()
                    .is_some_and(|start| {
                        import["span"]["startByte"]
                            .as_u64()
                            .is_some_and(|s| s >= start)
                    })
                && dependency["span"]["endByte"]
                    .as_u64()
                    .is_some_and(|end| import["span"]["endByte"].as_u64().is_some_and(|e| e <= end))
            {
                matches += 1;
            }
        }
    }
    if matches != 1 {
        return Err("SECTION_PROGRAM_IMPORT_SPAN_MISMATCH".into());
    }
    // Cross-check the local module's symbol import as well as template bindings.
    let syntax = unique(
        items(report, "syntaxEvidence").filter(|f| f["file"] == entry),
        "SECTION_ROOT_SYNTAX_MISSING_OR_AMBIGUOUS",
    )?;
    if syntax["syntaxValid"] != true
        || syntax["moduleSymbols"]["syntaxValid"] != true
        || items(&syntax["moduleSymbols"], "imports")
            .filter(|actual| *actual == import)
            .count()
            != 1
    {
        return Err("SECTION_ROOT_IMPORT_EVIDENCE_MISMATCH".into());
    }
    let identity = prove_target(report, request)?;
    Ok(json!({"bindingId":tag["binding"],"import":import,"target":identity}))
}

fn candidate(report: &Value, entry: &Value, tag: &Value) -> bool {
    tag["kind"] == "component-reference"
        && (tag["name"] == "Section"
            || tag["exported"] == "Section"
            || items(&report["moduleLinks"], "requests").any(|r| {
                r["entryFile"] == *entry
                    && r["target"]["packageName"] == PACKAGE
                    && r["target"]["file"] == FILE
                    && binding(&r["import"]).is_some_and(|b| tag["binding"] == b)
            }))
}

fn lower(node: &Value) -> (Option<Value>, Vec<Value>) {
    let mut issues = Vec::new();
    let mut seen = BTreeSet::new();
    let mut id = None;
    let mut class = "";
    let mut key = Value::Null;
    let Some(attributes) = node["attributes"].as_array() else {
        return (None, vec![json!({"code":"SECTION_ATTRIBUTES_MALFORMED"})]);
    };
    if attributes.len() > 64 {
        return (None, vec![json!({"code":"SECTION_ATTRIBUTE_LIMIT"})]);
    }
    for (index, attr) in attributes.iter().enumerate() {
        let name = attr["name"].as_str().unwrap_or("");
        let code = if attr["kind"] != "attribute" || name.is_empty() {
            Some("SECTION_SPREAD_OR_UNKNOWN_ATTRIBUTE")
        } else if !seen.insert(name) {
            Some("SECTION_DUPLICATE_ATTRIBUTE")
        } else {
            match name {
                "id" | "className" => {
                    if attr["value"]["kind"] != "literal" || !attr["value"]["value"].is_string() {
                        Some("SECTION_PROP_REQUIRES_LITERAL_STRING")
                    } else {
                        let value = attr["value"]["value"].as_str().unwrap_or("");
                        if name == "id" {
                            id = Some(value);
                        } else {
                            class = value;
                        }
                        None
                    }
                }
                "key" => {
                    key = attr["value"].clone();
                    // A key expression is still evaluated by React even though
                    // it is not a DOM prop. Only inert scalar literals qualify.
                    if key["kind"] == "literal"
                        && matches!(
                            key.get("value"),
                            Some(
                                Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_)
                            )
                        )
                    {
                        None
                    } else {
                        Some("SECTION_KEY_EXPRESSION_UNRESOLVED")
                    }
                }
                _ => Some("SECTION_PROP_UNSUPPORTED"),
            }
        };
        if let Some(code) = code {
            issues.push(json!({"code":code,"attributeIndex":index,"name":name,
                "span":attr["span"],"originalAttribute":attr}));
        }
    }
    if !node["children"].is_array() {
        issues.push(json!({"code":"SECTION_CHILDREN_MALFORMED"}));
    }
    if !issues.is_empty() {
        return (None, issues);
    }
    let mut attributes = vec![json!({"name":"className","value":class_name(class)})];
    if let Some(id) = id {
        attributes.insert(0, json!({"name":"id","value":id}));
    }
    (
        Some(
            json!({"kind":"semantic-section-boundary","semanticTag":"section",
        "attributes":attributes,"logicalKey":key,"keyIsDomAttribute":false,
        "children":node["children"],"childrenRepresentation":"original-template-ir",
        "styleDependencies":[style_dependency()],"canLowerWithoutStyles":false,
        "childrenResolved":false,"layoutCompiled":false}),
        ),
        issues,
    )
}

struct Adapter<'a> {
    report: &'a Value,
    instances: Vec<Value>,
    steps: usize,
    instance_bytes: usize,
    started: Instant,
}

impl Adapter<'_> {
    fn walk(
        &mut self,
        template: &Value,
        node: &Value,
        pointer: &str,
        depth: usize,
    ) -> Result<(), String> {
        self.steps += 1;
        if self.started.elapsed() > Duration::from_secs(3) {
            return Err("SECTION_ADAPTER_TIME_LIMIT".into());
        }
        if self.steps > MAX_WALK_NODES || depth > 96 {
            return Err("SECTION_ADAPTER_WALK_LIMIT".into());
        }
        if node["kind"] == "element" && candidate(self.report, &template["entryFile"], &node["tag"])
        {
            if self.instances.len() >= MAX_INSTANCES {
                return Err("SECTION_ADAPTER_INSTANCE_LIMIT".into());
            }
            let proof = prove_import(self.report, template, &node["tag"]);
            let (boundary, issues, evidence) = match proof {
                Ok(evidence) => {
                    let (boundary, issues) = lower(node);
                    (boundary, issues, evidence)
                }
                Err(code) => (None, vec![json!({"code":code})], Value::Null),
            };
            let instance = json!({"componentProgram":PROGRAM_ID,
                "entryFile":template["entryFile"],"sourceSha256":template["sourceSha256"],
                "inputPointer":pointer,"span":node["span"],"tag":node["tag"],
                "bindingEvidence":evidence,"wrapperContentLowered":boundary.is_some(),
                "childrenResolved":false,"childrenResolution":"not-attempted-preserved-verbatim",
                "originalAttributes":node["attributes"],"originalChildren":node["children"],
                "boundary":boundary,"blockers":issues,"layoutCompiled":false,
                "componentSemanticsCompiled":false});
            self.instance_bytes = self
                .instance_bytes
                .checked_add(size(&instance)?)
                .filter(|n| *n <= MAX_BYTES)
                .ok_or("SECTION_ADAPTER_BYTE_LIMIT")?;
            self.instances.push(instance);
        }
        // Traverse only render-IR edges, never source snippets, import metadata,
        // literal JSON payloads, tags, attributes, or arbitrary unknown objects.
        let keys: &[&str] = match node["kind"].as_str() {
            Some("function") => &["body"],
            Some("element" | "fragment") => &["children"],
            Some("default") => &["value", "fallback"],
            Some("get") => &["object"],
            Some("group" | "not" | "to-string") => &["value"],
            Some("logical" | "compare") => &["left", "right"],
            Some("conditional") => &["condition", "consequence", "alternative"],
            Some("map") => &["collection", "body"],
            Some("join") => &["collection", "separator"],
            Some("text-template") => &["parts"],
            _ => &[],
        };
        for key in keys {
            if let Some(value) = node.get(*key) {
                if let Some(values) = value.as_array() {
                    for (i, child) in values.iter().enumerate() {
                        self.walk(template, child, &format!("{pointer}/{key}/{i}"), depth + 1)?;
                    }
                } else {
                    self.walk(template, value, &format!("{pointer}/{key}"), depth + 1)?;
                }
            }
        }
        Ok(())
    }
}

/// Accepts the inspection result object (not its CLI envelope). Limits fail
/// closed with Err; unsupported instances remain data with explicit blockers.
pub(crate) fn compile(report: &Value) -> Result<Value, String> {
    bounded(report)?;
    let templates = report["templatePrograms"]
        .as_array()
        .ok_or("SECTION_TEMPLATE_PROGRAMS_MISSING")?;
    if templates.len() > 64
        || items(&report["moduleLinks"], "requests").count() > MAX_REQUESTS
        || items(&report["dependencyBundle"], "packages").count() > 8
    {
        return Err("SECTION_ADAPTER_RECORD_LIMIT".into());
    }
    let mut adapter = Adapter {
        report,
        instances: Vec::new(),
        steps: 0,
        instance_bytes: 0,
        started: Instant::now(),
    };
    let mut retained = Vec::new();
    for (index, template) in templates.iter().enumerate() {
        let program = &template["program"];
        if !program.is_object() {
            return Err("SECTION_TEMPLATE_PROGRAM_MALFORMED".into());
        }
        retained.push(json!({"entryFile":template["entryFile"],
            "inputPointer":format!("/templatePrograms/{index}/program/blockers"),
            "blockers":program["blockers"]}));
        adapter.walk(
            template,
            &program["entry"],
            &format!("/templatePrograms/{index}/program/entry"),
            0,
        )?;
        for (i, value) in items(program, "bindings").enumerate() {
            adapter.walk(
                template,
                &value["value"],
                &format!("/templatePrograms/{index}/program/bindings/{i}/value"),
                0,
            )?;
        }
    }
    let lowered = adapter
        .instances
        .iter()
        .filter(|i| i["wrapperContentLowered"] == true)
        .count();
    let proven = adapter
        .instances
        .iter()
        .any(|i| i["bindingEvidence"].is_object());
    let blockers = if adapter.instances.is_empty() {
        vec![]
    } else {
        vec![
            json!({"code":"SECTION_STYLES_NOT_COMPILED","dependency":style_dependency()}),
            json!({"code":"SECTION_CHILDREN_NOT_RESOLVED"}),
            json!({"code":"SECTION_MODULE_EVALUATION_NOT_PROVEN"}),
        ]
    };
    let output = json!({"format":"resume-one-section-content-adapter-v1",
        "componentPrograms":if proven { vec![component_program()] } else { vec![] },
        "instances":adapter.instances,
        "statistics":{"instances":adapter.instances.len(),"wrappersLowered":lowered,
            "wrappersBlocked":adapter.instances.len()-lowered,"walkNodes":adapter.steps},
        "retainedBlockers":{"inspection":report["blockers"],
            "moduleLinks":report["moduleLinks"]["blockers"],"programs":retained},
        "blockers":blockers,
        "originalProgramsModified":false,"originalBlockersCleared":false,
        "componentSemanticsCompiled":false,"childrenResolved":false,"layoutCompiled":false,
        "dependencyGraphComplete":false,"moduleEvaluationProven":false,
        "migrationComplete":false,"executedSourceCode":false});
    size(&output)?;
    if adapter.started.elapsed() > Duration::from_secs(3) {
        return Err("SECTION_ADAPTER_TIME_LIMIT".into());
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Value {
        let import_span = json!({"file":"src/Resume.jsx","startByte":78,"endByte":85});
        let declaration = json!({"name":"Section","id":"symbol:545:Section","kind":"function",
            "mutable":true,"synthetic":false,"initializer":{"form":"function","astKind":"function_declaration"},
            "span":{"file":FILE,"startByte":536,"endByte":764},
            "nameSpan":{"file":FILE,"startByte":545,"endByte":552}});
        let import = json!({"local":"Section","module":PACKAGE,"imported":"Section",
            "kind":"named","supported":true,"span":import_span});
        let snapshot = "a".repeat(64);
        json!({"sourceSnapshot":{"complete":true,"files":[{"path":"src/Resume.jsx","sha256":"b".repeat(64)}]},
            "blockers":[{"code":"ROOT_STILL_BLOCKED"}],
            "syntaxEvidence":[{"file":"src/Resume.jsx","syntaxValid":true,
                "moduleSymbols":{"syntaxValid":true,"imports":[import.clone()]}}],
            "moduleLinks":{"format":"resume-one-module-links-v1","blockers":[{"code":"LINK_STILL_BLOCKED"}],
                "requests":[{"entryFile":"src/Resume.jsx","import":import,"declarationResolved":true,
                    "target":{"packageName":PACKAGE,"packageVersion":VERSION,"file":FILE,
                        "fileSha256":FILE_SHA,"sourceSha256":snapshot,"lookup":"local",
                        "symbol":"Section","name":"Section","declaration":declaration.clone(),
                        "span":declaration["span"]}}]},
            "dependencyBundle":{"packages":[{"name":PACKAGE,"version":VERSION,
                "package":{"name":PACKAGE,"version":VERSION},"sourceSha256":snapshot,
                "sourceSnapshot":{"complete":true,"sourceSha256":snapshot,
                    "files":[{"path":FILE,"sha256":FILE_SHA,"bytes":790,"utf8":true}]},
                "syntaxEvidence":[{"file":FILE,"syntaxValid":true,
                    "moduleSymbols":{"syntaxValid":true,"declarations":[declaration]}}]}]},
            "templatePrograms":[{"entryFile":"src/Resume.jsx","sourceSha256":"b".repeat(64),
                "program":{"format":"resume-one-template-ir-v1","syntaxValid":true,
                    "source":{"path":"src/Resume.jsx"},"blockers":[{"code":"CHILD_UNKNOWN"}],"bindings":[],
                    "dependencies":[{"kind":"import","module":PACKAGE,
                        "span":{"file":"src/Resume.jsx","startByte":67,"endByte":157},
                        "specifiers":[{"local":"Section","exported":"Section","span":import_span}]}],
                    "entry":{"kind":"element","tag":{"kind":"component-reference","name":"Section",
                        "binding":"b:78:Section","module":PACKAGE,"exported":"Section"},
                        "attributes":[],"children":[]}}}]})
    }

    fn entry(report: &mut Value) -> &mut Value {
        &mut report["templatePrograms"][0]["program"]["entry"]
    }

    fn attr(name: &str, value: Value) -> Value {
        json!({"kind":"attribute","name":name,"value":value})
    }

    fn literal(value: Value) -> Value {
        json!({"kind":"literal","value":value})
    }

    fn result(report: &Value) -> Value {
        compile(report).unwrap()
    }

    fn lowered(report: &Value) -> bool {
        result(report)["instances"][0]["wrapperContentLowered"] == true
    }

    #[test]
    fn pinned_direct_wrapper_and_default_class() {
        let report = fixture();
        let out = result(&report);
        assert_eq!(out["statistics"]["wrappersLowered"], 1);
        assert_eq!(out["componentPrograms"].as_array().unwrap().len(), 1);
        assert_eq!(
            out["instances"][0]["boundary"]["attributes"],
            json!([{"name":"className","value":"resume-section"}])
        );
        for key in [
            "componentSemanticsCompiled",
            "childrenResolved",
            "layoutCompiled",
            "migrationComplete",
            "executedSourceCode",
        ] {
            assert_eq!(out[key], false);
        }
        assert_eq!(
            out["instances"][0]["boundary"]["styleDependencies"][0]["fileSha256"],
            FILE_SHA
        );
    }

    #[test]
    fn exact_class_string_and_ecma_whitespace() {
        let mut report = fixture();
        entry(&mut report)["attributes"] = json!([
            attr("id", literal(json!("  work  "))),
            attr("className", literal(json!("  literary\u{feff}")))
        ]);
        let out = result(&report);
        assert_eq!(
            out["instances"][0]["boundary"]["attributes"][0]["value"],
            "  work  "
        );
        assert_eq!(
            out["instances"][0]["boundary"]["attributes"][1]["value"],
            "resume-section   literary"
        );
        assert_eq!(class_name("\u{feff}x\u{feff}"), "resume-section \u{feff}x");
        assert_eq!(class_name("x\u{0085}"), "resume-section x\u{0085}");
        assert_eq!(
            class_name("x\u{180e}\u{200b}"),
            "resume-section x\u{180e}\u{200b}"
        );
    }

    #[test]
    fn import_alias_uses_real_local_span_not_component_name() {
        let mut report = fixture();
        let mut import = report["moduleLinks"]["requests"][0]["import"].clone();
        import["local"] = json!("S");
        import["span"]["endByte"] = json!(79);
        report["moduleLinks"]["requests"][0]["import"] = import.clone();
        report["syntaxEvidence"][0]["moduleSymbols"]["imports"][0] = import.clone();
        let specifier =
            &mut report["templatePrograms"][0]["program"]["dependencies"][0]["specifiers"][0];
        specifier["local"] = json!("S");
        specifier["span"] = import["span"].clone();
        entry(&mut report)["tag"]["name"] = json!("S");
        entry(&mut report)["tag"]["binding"] = json!("b:78:S");
        assert!(lowered(&report));
        entry(&mut report)["tag"]["binding"] = json!("b:79:S");
        assert!(!lowered(&report));
    }

    #[test]
    fn alias_distinguishes_whole_specifier_span_from_local_identifier_span() {
        let mut report = fixture();
        let local_span = json!({"file":"src/Resume.jsx","startByte":89,"endByte":90});
        let import = &mut report["moduleLinks"]["requests"][0]["import"];
        import["local"] = json!("S");
        import["span"]["endByte"] = json!(90);
        import["localSpan"] = local_span.clone();
        let import = import.clone();
        report["syntaxEvidence"][0]["moduleSymbols"]["imports"][0] = import;
        let specifier =
            &mut report["templatePrograms"][0]["program"]["dependencies"][0]["specifiers"][0];
        specifier["local"] = json!("S");
        specifier["span"] = local_span;
        entry(&mut report)["tag"]["name"] = json!("S");
        entry(&mut report)["tag"]["binding"] = json!("b:89:S");
        assert!(lowered(&report));
        report["moduleLinks"]["requests"][0]["import"]["localSpan"]["startByte"] = json!(88);
        assert!(!lowered(&report));
    }

    #[test]
    fn import_span_and_export_identity_cannot_be_forged() {
        let mut report = fixture();
        report["templatePrograms"][0]["program"]["dependencies"][0]["specifiers"][0]["span"]["startByte"] =
            json!(77);
        assert!(!lowered(&report));
        let mut report = fixture();
        entry(&mut report)["tag"]["exported"] = json!("Other");
        assert!(!lowered(&report));
        let mut report = fixture();
        let duplicate = report["moduleLinks"]["requests"][0].clone();
        report["moduleLinks"]["requests"]
            .as_array_mut()
            .unwrap()
            .push(duplicate);
        assert!(!lowered(&report));
    }

    #[test]
    fn pinned_target_and_dependency_guards_fail_closed() {
        for (field, bad) in [
            ("fileSha256", json!("0".repeat(64))),
            ("packageName", json!("fake-core")),
            ("packageVersion", json!("0.3.7")),
            ("file", json!("other/Section.jsx")),
            ("name", json!("Wrapper")),
        ] {
            let mut report = fixture();
            report["moduleLinks"]["requests"][0]["target"][field] = bad;
            assert!(!lowered(&report), "{field}");
        }
        let mut report = fixture();
        report["moduleLinks"]["requests"][0]["target"]["declaration"]["name"] =
            json!("LocalSection");
        assert!(!lowered(&report));
        let mut report = fixture();
        report["dependencyBundle"]["packages"][0]["sourceSnapshot"]["files"][0]["sha256"] =
            json!("0".repeat(64));
        assert!(!lowered(&report));
        let mut report = fixture();
        report.as_object_mut().unwrap().remove("dependencyBundle");
        assert!(!lowered(&report));
    }

    #[test]
    fn local_same_name_rejected_and_styled_wrapper_not_compiled() {
        let mut report = fixture();
        entry(&mut report)["tag"] =
            json!({"kind":"component-reference","name":"Section","binding":"b:900:Section"});
        assert!(!lowered(&report));
        let mut report = fixture();
        entry(&mut report)["tag"] = json!({"kind":"component-reference","name":"StyledSection","binding":"b:900:StyledSection"});
        let out = result(&report);
        assert_eq!(out["instances"], json!([]));
        assert_eq!(out["componentPrograms"], json!([]));
    }

    #[test]
    fn unsupported_props_spreads_and_duplicates_are_blocked() {
        for name in [
            "attrs",
            "as",
            "style",
            "theme",
            "forwardedAs",
            "ref",
            "title",
            "children",
            "onClick",
        ] {
            let mut report = fixture();
            entry(&mut report)["attributes"] = json!([attr(name, literal(json!("x")))]);
            assert!(!lowered(&report), "{name}");
        }
        let mut report = fixture();
        entry(&mut report)["attributes"] =
            json!([{"kind":"blocked","code":"SPREAD_PROPS","source":"sideEffect()"}]);
        assert!(!lowered(&report));
        let mut report = fixture();
        entry(&mut report)["attributes"] = json!([
            attr("id", literal(json!("a"))),
            attr("id", literal(json!("b")))
        ]);
        assert!(!lowered(&report));
    }

    #[test]
    fn non_string_props_are_not_coerced_or_evaluated() {
        for value in [
            literal(Value::Null),
            literal(json!(0)),
            literal(json!(false)),
            json!({"kind":"binding","id":"b:20:id"}),
            json!({"kind":"blocked","source":"evil()"}),
        ] {
            for name in ["id", "className"] {
                let mut report = fixture();
                entry(&mut report)["attributes"] = json!([attr(name, value.clone())]);
                assert!(!lowered(&report));
            }
        }
    }

    #[test]
    fn children_and_every_original_program_blocker_are_preserved() {
        let mut report = fixture();
        let children = json!([null,false,0,[],"",{"kind":"literal","value":null},
            {"kind":"literal","value":false},{"kind":"literal","value":0},
            {"kind":"literal","value":[]},{"kind":"blocked","code":"UNKNOWN_CALL","source":"f()"},
            {"kind":"element","tag":{"kind":"component-reference","name":"SectionTitle"},"attributes":[],"children":[]}]);
        entry(&mut report)["children"] = children.clone();
        let original = report.clone();
        let out = result(&report);
        assert!(lowered(&report));
        assert_eq!(out["instances"][0]["boundary"]["children"], children);
        assert_eq!(out["instances"][0]["childrenResolved"], false);
        assert_eq!(
            out["retainedBlockers"]["programs"][0]["blockers"],
            report["templatePrograms"][0]["program"]["blockers"]
        );
        assert_eq!(out["retainedBlockers"]["inspection"], report["blockers"]);
        assert_eq!(report, original);
    }

    #[test]
    fn logical_key_is_not_dom_and_unknown_key_keeps_its_blocker() {
        let mut report = fixture();
        entry(&mut report)["attributes"] = json!([attr("key", literal(json!("work-1")))]);
        let out = result(&report);
        let boundary = &out["instances"][0]["boundary"];
        assert_eq!(boundary["logicalKey"], literal(json!("work-1")));
        assert!(!items(boundary, "attributes").any(|a| a["name"] == "key"));
        for key in [
            json!({"kind":"blocked","code":"SIDE_EFFECT","source":"evil()"}),
            json!({"kind":"binding","id":"unknown"}),
            json!({"kind":"call","callee":"f"}),
        ] {
            let mut report = fixture();
            entry(&mut report)["attributes"] = json!([attr("key", key.clone())]);
            let out = result(&report);
            assert_eq!(out["instances"][0]["wrapperContentLowered"], false);
            assert_eq!(out["instances"][0]["originalAttributes"][0]["value"], key);
            assert_eq!(
                out["instances"][0]["blockers"][0]["code"],
                "SECTION_KEY_EXPRESSION_UNRESOLVED"
            );
        }
    }

    #[test]
    fn binding_values_walked_but_literal_data_and_unknown_nodes_not_compiled() {
        let mut report = fixture();
        let section = entry(&mut report).clone();
        report["templatePrograms"][0]["program"]["bindings"] = json!([
            {"value":section}, {"value":{"kind":"literal","value":section}},
            {"value":{"kind":"unknown","body":section}}
        ]);
        assert_eq!(result(&report)["statistics"]["wrappersLowered"], 2);
    }

    #[test]
    fn budgets_reject_depth_records_bytes_and_instances() {
        let mut deep = Value::Null;
        for _ in 0..=MAX_DEPTH {
            deep = json!([deep]);
        }
        assert!(compile(&deep).unwrap_err().contains("INPUT_LIMIT"));
        let mut report = fixture();
        report["extra"] = json!("x".repeat(MAX_BYTES));
        assert!(compile(&report).unwrap_err().contains("BYTE_LIMIT"));
        let mut report = fixture();
        let request = report["moduleLinks"]["requests"][0].clone();
        report["moduleLinks"]["requests"] = json!(vec![request; MAX_REQUESTS + 1]);
        assert!(compile(&report).unwrap_err().contains("RECORD_LIMIT"));
        let mut report = fixture();
        let section = entry(&mut report).clone();
        entry(&mut report)["children"] = json!(vec![section; MAX_INSTANCES]);
        assert!(compile(&report).unwrap_err().contains("INSTANCE_LIMIT"));
    }

    #[test]
    #[ignore = "optional read-only integration check against the cached Academic inspection report"]
    fn cached_academic_has_eleven_direct_wrappers() {
        let path =
            std::env::var("RESUME_ONE_SECTION_INSPECTION").expect("set cached inspection path");
        let bytes = std::fs::read(path).unwrap();
        let envelope: Value = serde_json::from_slice(&bytes).unwrap();
        let original = envelope["result"].clone();
        let out = compile(&envelope["result"]).unwrap();
        assert_eq!(out["statistics"]["wrappersLowered"], 11, "{out}");
        assert_eq!(out["statistics"]["wrappersBlocked"], 0);
        assert_eq!(envelope["result"], original);
    }

    #[test]
    #[ignore = "requires RESUME_ONE_CACHED_PACKAGES_ROOT for exact upstream Section source"]
    fn real_source_aliases_and_shadowing_use_exact_whole_file_binding_evidence() {
        use std::fs;
        use std::path::PathBuf;
        let base = PathBuf::from(
            std::env::var("RESUME_ONE_CACHED_PACKAGES_ROOT").expect("set cached packages root"),
        );
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        fs::write(
            root.join("package.json"),
            r#"{"name":"section-fixture","version":"1.0.0"}"#,
        )
        .unwrap();
        fs::write(
            root.join("bridge.jsx"),
            "export {Section as default} from '@jsonresume/core';",
        )
        .unwrap();
        let dependencies = [base.join("resume-core"), base.join("utils")];
        for source in [
            "import {Section as S} from '@jsonresume/core'; export default function Resume(){return <S id=\"work\">hello</S>;}",
            "import S from './bridge.jsx'; export default function Resume(){return <S>hello</S>;}",
        ] {
            fs::write(root.join("Resume.jsx"), source).unwrap();
            let report = crate::inspection::inspect_dependencies(
                &root,
                &["Resume.jsx".into()],
                &dependencies,
            )
            .unwrap();
            assert_eq!(
                report["componentContent"]["statistics"]["wrappersLowered"], 1,
                "{:#}",
                report["componentContent"]
            );
            assert_eq!(report["migrationComplete"], false);
            assert_eq!(fs::read_to_string(root.join("Resume.jsx")).unwrap(), source);
        }
        fs::write(root.join("Resume.jsx"), "import {Section} from '@jsonresume/core'; export default function Resume({Section}){return <Section/>;}").unwrap();
        let report =
            crate::inspection::inspect_dependencies(&root, &["Resume.jsx".into()], &dependencies)
                .unwrap();
        assert_eq!(
            report["componentContent"]["statistics"]["wrappersLowered"],
            0
        );
    }
}
