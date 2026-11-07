//! Source-derived entry header candidates, never a JavaScript/HTML runtime.
//!
//! Only actual, lexically bound map item fields in an entry function's return
//! are considered. Component names do not identify data or section names.
//! Native block/inline boundaries are hints, not computed CSS layout proof.
//! Unsupported expressions, arbitrary components and branches remain rejected.

use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Write};

const MAX_INPUT_NODES: usize = 400_000;
const MAX_INPUT_BYTES: usize = 16 * 1024 * 1024;
const MAX_STEPS: usize = 200_000;
const MAX_RECORDS: usize = 2_048;
const MAX_ROWS: usize = 16;
const MAX_PARTS: usize = 16;
const MAX_TEXT: usize = 512;

struct OutputBudget(usize);

impl Write for OutputBudget {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0 = self.0.saturating_add(bytes.len());
        if self.0 > 8 * 1024 * 1024 {
            return Err(io::Error::other("ENTRY_TEMPLATE_OUTPUT_LIMIT"));
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn items<'a>(value: &'a Value, key: &str) -> impl Iterator<Item = &'a Value> {
    value[key].as_array().into_iter().flatten()
}

fn preflight(value: &Value) -> Result<(), String> {
    let mut pending = vec![(value, 0usize)];
    let (mut count, mut bytes) = (0usize, 0usize);
    while let Some((value, depth)) = pending.pop() {
        count += 1;
        if count > MAX_INPUT_NODES || depth > 128 || pending.len() > MAX_INPUT_NODES {
            return Err("ENTRY_TEMPLATE_INPUT_LIMIT".into());
        }
        match value {
            Value::String(value) => bytes = bytes.saturating_add(value.len()),
            Value::Array(values) => pending.extend(values.iter().map(|v| (v, depth + 1))),
            Value::Object(values) => {
                bytes = bytes.saturating_add(values.keys().map(String::len).sum::<usize>());
                pending.extend(values.values().map(|v| (v, depth + 1)));
            }
            _ => bytes = bytes.saturating_add(8),
        }
        if bytes > MAX_INPUT_BYTES {
            return Err("ENTRY_TEMPLATE_INPUT_LIMIT".into());
        }
    }
    Ok(())
}

fn valid_span(value: &Value, file: &str) -> bool {
    value["file"] == file
        && value["startByte"]
            .as_u64()
            .zip(value["endByte"].as_u64())
            .is_some_and(|(start, end)| start <= end && end <= 1_048_576)
}

fn is_header_field(module: &str, field: &str) -> bool {
    // dateRange is a renderer-derived field, not a direct JSON Resume member.
    // Mapping a start/end pair or a DateRange component needs its own proof.
    field != "dateRange"
        && !matches!(field, "keywords" | "roles")
        && resume_one_core::semantic::header_field_names(module).contains(&field)
}

fn supported_module(module: &str) -> bool {
    matches!(
        module,
        "work"
            | "volunteer"
            | "education"
            | "awards"
            | "certificates"
            | "publications"
            | "projects"
            | "references"
    )
}

fn body_field(field: &str) -> bool {
    matches!(
        field,
        "summary"
            | "description"
            | "highlights"
            | "courses"
            | "url"
            | "website"
            | "keywords"
            | "roles"
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Root {
    Props,
    Item { id: String, scope: String, map: u64 },
}

#[derive(Clone, Debug)]
struct Path {
    root: Root,
    fields: Vec<String>,
    bindings: Vec<String>,
}

#[derive(Clone)]
struct Component {
    name: String,
    tag: String,
    binding: Option<String>,
    attributes: Value,
}

impl Component {
    fn evidence(&self) -> Value {
        json!({"name":self.name,"nativeTag":self.tag,"binding":self.binding,"attributes":self.attributes})
    }
    fn block(&self) -> bool {
        matches!(
            self.tag.as_str(),
            "div"
                | "section"
                | "article"
                | "header"
                | "main"
                | "aside"
                | "p"
                | "h1"
                | "h2"
                | "h3"
                | "h4"
                | "h5"
                | "h6"
                | "li"
                | "ul"
                | "ol"
                | "dl"
                | "dt"
                | "dd"
                | "blockquote"
                | "tr"
        )
    }
}

#[derive(Clone)]
enum Token {
    Text {
        text: String,
        guard: Option<String>,
        span: Value,
        components: Vec<Component>,
    },
    Field {
        field: String,
        guards: BTreeSet<String>,
        source: Value,
        components: Vec<Component>,
    },
}

#[derive(Default)]
struct Entry {
    module: String,
    rows: Vec<Value>,
    current: Vec<Token>,
    rejected: Vec<Value>,
    differences: Vec<Value>,
    excluded: Vec<Value>,
    guards: BTreeSet<String>,
    components: Vec<Component>,
}

struct Extractor<'a> {
    report: &'a Value,
    template: &'a Value,
    file: &'a str,
    bindings: BTreeMap<&'a str, &'a Value>,
    steps: usize,
    entry_scope: &'a str,
    candidates: Vec<Value>,
    rejected: Vec<Value>,
}

impl<'a> Extractor<'a> {
    fn tick(&mut self, depth: usize) -> Result<(), String> {
        self.steps += 1;
        if self.steps > MAX_STEPS || depth > 96 {
            return Err("ENTRY_TEMPLATE_ANALYSIS_LIMIT".into());
        }
        Ok(())
    }

    fn issue(&self, node: &Value, code: &str) -> Value {
        json!({"code":code,"entryFile":self.file,"span":node["span"],
            "kind":node["kind"],"bindingVerified":false,"layoutVerified":false,"equivalent":false})
    }

    fn binding(&self, id: &str, scopes: &BTreeSet<String>) -> Result<&'a Value, String> {
        let binding = self
            .bindings
            .get(id)
            .copied()
            .ok_or("ENTRY_UNKNOWN_BINDING")?;
        let scope = binding["scope"].as_str().ok_or("ENTRY_BINDING_SCOPE")?;
        if scope != "module" && !scopes.contains(scope) {
            return Err("ENTRY_BINDING_OUT_OF_SCOPE".into());
        }
        Ok(binding)
    }

    fn path(
        &mut self,
        node: &Value,
        scopes: &BTreeSet<String>,
        seen: &mut BTreeSet<String>,
        depth: usize,
    ) -> Result<Path, String> {
        self.tick(depth)?;
        match node["kind"].as_str() {
            Some("group") => self.path(&node["value"], scopes, seen, depth + 1),
            Some("input") if node["name"] == "props" => Ok(Path {
                root: Root::Props,
                fields: vec![],
                bindings: vec![],
            }),
            Some("get") => {
                let field = node["property"].as_str().ok_or("ENTRY_PROPERTY_UNKNOWN")?;
                if field.is_empty()
                    || field.len() > 128
                    || matches!(field, "__proto__" | "constructor" | "prototype")
                    || !valid_span(&node["span"], self.file)
                {
                    return Err("ENTRY_PROPERTY_INVALID".into());
                }
                let mut path = self.path(&node["object"], scopes, seen, depth + 1)?;
                path.fields.push(field.to_owned());
                Ok(path)
            }
            Some("binding") => {
                let id = node["id"].as_str().ok_or("ENTRY_BINDING_ID")?;
                if !seen.insert(id.to_owned()) {
                    return Err("ENTRY_BINDING_CYCLE".into());
                }
                let binding = self.binding(id, scopes)?;
                if node.get("name").is_some() && node["name"] != binding["name"] {
                    return Err("ENTRY_REFERENCE_IDENTITY_MISMATCH".into());
                }
                let mut path = if binding["value"]["kind"] == "map-parameter" {
                    if binding["value"]["role"] != "item" {
                        return Err("ENTRY_MAP_INDEX_IS_NOT_ITEM".into());
                    }
                    Path {
                        root: Root::Item {
                            id: id.to_owned(),
                            scope: binding["scope"].as_str().unwrap_or("").to_owned(),
                            map: binding["value"]["mapStartByte"]
                                .as_u64()
                                .ok_or("ENTRY_MAP_ORIGIN")?,
                        },
                        fields: vec![],
                        bindings: vec![],
                    }
                } else {
                    self.path(&binding["value"], scopes, seen, depth + 1)?
                };
                path.bindings.push(id.to_owned());
                seen.remove(id);
                Ok(path)
            }
            // Only the empty-array undefined default used by module collections
            // is transparent. A default string/member changes field semantics.
            Some("default")
                if node["when"] == "undefined"
                    && node["fallback"]["kind"] == "literal"
                    && node["fallback"]["value"] == json!([]) =>
            {
                let path = self.path(&node["value"], scopes, seen, depth + 1)?;
                if path.root == Root::Props
                    && path.fields.len() == 2
                    && path.fields[0] == "resume"
                    && supported_module(&path.fields[1])
                {
                    Ok(path)
                } else {
                    Err("ENTRY_DEFAULT_UNSUPPORTED".into())
                }
            }
            _ => Err("ENTRY_NOT_DIRECT_DATA_PATH".into()),
        }
    }

    fn data_path(&mut self, node: &Value, scopes: &BTreeSet<String>) -> Result<Path, String> {
        self.path(node, scopes, &mut BTreeSet::new(), 0)
    }

    fn component(&self, node: &Value) -> Result<Component, String> {
        let tag = &node["tag"];
        let name = tag["name"].as_str().ok_or("ENTRY_COMPONENT_NAME")?;
        let mut attributes = BTreeMap::new();
        for attribute in items(node, "attributes") {
            let key = attribute["name"]
                .as_str()
                .ok_or("ENTRY_COMPONENT_ATTRIBUTE")?;
            if key == "key" {
                continue;
            }
            let value = &attribute["value"];
            // Equal literals on two source elements have equal contexts. A
            // dynamic prop retains its exact source identity, never a guessed
            // runtime value. The source compiler already rejects spreads and
            // ambiguous/unsafe attributes.
            let value = if value["kind"] == "literal" {
                json!({"kind":"literal","value":value["value"]})
            } else {
                value.clone()
            };
            if attributes.insert(key.to_owned(), value).is_some() {
                return Err("ENTRY_COMPONENT_ATTRIBUTE_DUPLICATE".into());
            }
        }
        let attributes = json!(attributes);
        if tag["kind"] == "intrinsic" {
            return Ok(Component {
                name: name.into(),
                tag: name.into(),
                binding: None,
                attributes,
            });
        }
        if tag["kind"] != "component-reference" {
            return Err("ENTRY_COMPONENT_UNRESOLVED".into());
        }
        let id = tag["binding"].as_str().ok_or("ENTRY_COMPONENT_BINDING")?;
        let mut matches = items(&self.template["styleGraph"], "nodes")
            .filter(|style| style["id"] == id && style["name"] == name);
        let style = matches.next().ok_or("ENTRY_COMPONENT_UNRESOLVED")?;
        if matches.next().is_some()
            || style["inheritanceLinked"] != true
            || style["terminal"]["kind"] != "intrinsic"
            || self.bindings.get(id).is_none_or(|b| {
                b["name"] != name
                    || b["scope"] != "module"
                    || b["value"]["kind"] != "styled-reference"
            })
        {
            return Err("ENTRY_COMPONENT_UNRESOLVED".into());
        }
        Ok(Component {
            name: name.into(),
            tag: style["terminal"]["name"]
                .as_str()
                .ok_or("ENTRY_COMPONENT_TAG")?
                .into(),
            binding: Some(id.into()),
            attributes,
        })
    }

    /// An explicit, lossy adapter for the default core DateRange API, not an
    /// implementation identity or formatter-equivalence certificate. Both the
    /// actual import binding and the item's original two member reads must be
    /// proven. Extra props, computed arguments and merely matching names fail.
    fn date_component(
        &mut self,
        node: &Value,
        module: &str,
        root: &Root,
        scopes: &BTreeSet<String>,
        entry: &mut Entry,
    ) -> Result<bool, String> {
        let tag = &node["tag"];
        if tag["kind"] != "component-reference"
            || tag["module"] != "@jsonresume/core"
            || tag["exported"] != "DateRange"
        {
            return Ok(false);
        }
        let name = tag["name"].as_str().ok_or("ENTRY_DATE_IMPORT_NAME")?;
        let id = tag["binding"].as_str().ok_or("ENTRY_DATE_IMPORT_BINDING")?;
        let mut imports = Vec::new();
        for dependency in items(&self.template["program"], "dependencies") {
            if dependency["kind"] != "import" {
                continue;
            }
            for specifier in items(dependency, "specifiers") {
                let local = specifier["local"].as_str().unwrap_or("");
                let start = specifier["span"]["startByte"].as_u64();
                if start.is_some_and(|start| id == format!("b:{start}:{local}")) {
                    imports.push((dependency, specifier));
                }
            }
        }
        let proof = if imports.len() == 1 {
            let (dependency, specifier) = imports[0];
            (dependency["module"] == "@jsonresume/core"
                && specifier["exported"] == "DateRange"
                && specifier["local"] == name
                && valid_span(&specifier["span"], self.file)
                && specifier["span"]["endByte"]
                    .as_u64()
                    .unwrap_or(0)
                    .checked_sub(specifier["span"]["startByte"].as_u64().unwrap_or(0))
                    == Some(name.len() as u64)
                && !self.bindings.contains_key(id))
            .then(|| {
                json!({"module":"@jsonresume/core","exported":"DateRange","local":name,
                    "binding":id,"importSpan":dependency["span"],"localSpan":specifier["span"],
                    "importDeclarationVerified":true,"implementationVerified":false})
            })
        } else {
            None
        };
        let Some(proof) = proof else {
            entry
                .rejected
                .push(self.issue(node, "ENTRY_DATE_IMPORT_IDENTITY_MISMATCH"));
            return Ok(true);
        };
        if !resume_one_core::semantic::header_field_names(module).contains(&"dateRange")
            || items(node, "children").next().is_some()
        {
            entry
                .rejected
                .push(self.issue(node, "ENTRY_DATE_COMPONENT_SHAPE_UNSUPPORTED"));
            return Ok(true);
        }
        let mut attributes = BTreeMap::new();
        for attribute in items(node, "attributes") {
            let key = attribute["name"].as_str().ok_or("ENTRY_DATE_PROP_NAME")?;
            if !matches!(key, "startDate" | "endDate")
                || attributes.insert(key, attribute).is_some()
            {
                entry
                    .rejected
                    .push(self.issue(node, "ENTRY_DATE_OPTIONS_UNSUPPORTED"));
                return Ok(true);
            }
        }
        if attributes.len() != 2 {
            entry
                .rejected
                .push(self.issue(node, "ENTRY_DATE_ARGUMENTS_UNSUPPORTED"));
            return Ok(true);
        }
        let mut arguments = Vec::new();
        for key in ["startDate", "endDate"] {
            let expression = &attributes[key]["value"];
            let Some((field, path)) = self.same_item_field(expression, root, scopes) else {
                entry
                    .rejected
                    .push(self.issue(node, "ENTRY_DATE_ARGUMENT_BINDING_UNSUPPORTED"));
                return Ok(true);
            };
            if field != key {
                entry
                    .rejected
                    .push(self.issue(node, "ENTRY_DATE_ARGUMENT_BINDING_UNSUPPORTED"));
                return Ok(true);
            }
            arguments.push(json!({"prop":key,"field":field,"expression":expression,
                "span":expression["span"],"bindingChain":path.bindings,"bindingVerified":true}));
        }
        let packages: Vec<_> = items(&self.report["dependencyBundle"], "packages")
            .filter(|package| package["name"] == "@jsonresume/core")
            .collect();
        let snapshot = if packages.len() == 1 {
            Some(packages[0])
        } else {
            None
        };
        let snapshot_matched = snapshot.is_some_and(|package| {
            package["version"]=="0.3.6" && package["sourceSnapshot"]["complete"]==true
                && items(&package["sourceSnapshot"],"files").filter(|file| {
                    file["path"]=="src/primitives/DateRange.jsx" && file["sha256"]==
                        "8e9207af957f8292c520e5f59e70da77386f7b03b1f7093c50833a68297d02ad"
                }).count()==1
        });
        entry.differences.push(json!({"code":"DATE_FORMATTER_ADAPTED","entryFile":self.file,
            "span":node["span"],"import":proof,"arguments":arguments,
            "adapter":"core-daterange-default-api-to-short-en-v1","field":"dateRange","dateFormat":"short-en",
            "expectedPackageVersion":"0.3.6","observedPackageVersion":snapshot.map(|p|&p["version"]),
            "dependencySnapshotMatched":snapshot_matched,
            "formatterImplementationVerified":false,"dateSemanticsVerified":false,"equivalent":false,
            "limitations":["uses the declarative renderer's finite short-en fallback",
                "source package initialization, arbitrary date strings, locale and CSS remain unverified"]}));
        entry.current.push(Token::Field {
            field:"dateRange".into(),guards:entry.guards.clone(),components:entry.components.clone(),
            source:json!({"field":"dateRange","span":node["span"],"accessKind":"adapted-date-component",
                "itemBinding":match root {Root::Item{id,..}=>Some(id),_=>None},
                "import":proof,"arguments":arguments,"conditions":entry.guards,
                "styleComponent":entry.components.last().map(|c|c.name.as_str()),
                "styleComponentBinding":entry.components.last().and_then(|c|c.binding.as_deref()),
                "components":entry.components.iter().map(Component::evidence).collect::<Vec<_>>(),
                "bindingVerified":true,"dateSemanticsVerified":false,"equivalent":false}),
        });
        Ok(true)
    }

    fn date_presence_condition(
        &mut self,
        node: &Value,
        root: &Root,
        scopes: &BTreeSet<String>,
    ) -> bool {
        let mut node = node;
        while node["kind"] == "group" {
            node = &node["value"];
        }
        if self
            .same_item_field(node, root, scopes)
            .is_some_and(|(field, _)| field == "startDate")
        {
            return true;
        }
        if node["kind"] != "logical" || node["operator"] != "||" {
            return false;
        }
        let left = self
            .same_item_field(&node["left"], root, scopes)
            .map(|(field, _)| field);
        let right = self
            .same_item_field(&node["right"], root, scopes)
            .map(|(field, _)| field);
        matches!(
            (left.as_deref(), right.as_deref()),
            (Some("startDate"), Some("endDate")) | (Some("endDate"), Some("startDate"))
        )
    }

    fn discover(
        &mut self,
        node: &Value,
        scopes: &BTreeSet<String>,
        ancestry: &mut Vec<Value>,
        depth: usize,
    ) -> Result<(), String> {
        self.tick(depth)?;
        match node["kind"].as_str() {
            Some("map") => {
                let collection = match self.data_path(&node["collection"], scopes) {
                    Ok(path) => path,
                    Err(code) => {
                        self.rejected.push(self.issue(node, &code));
                        return Ok(());
                    }
                };
                if collection.root == Root::Props
                    && collection.fields.len() == 2
                    && collection.fields[0] == "resume"
                    && supported_module(&collection.fields[1])
                {
                    self.map(node, &collection.fields[1], scopes, ancestry, depth + 1)?;
                }
                // A nested body list cannot become a new standard module.
            }
            Some("function") => self.discover(&node["body"], scopes, ancestry, depth + 1)?,
            Some("element" | "fragment") => {
                for child in items(node, "children") {
                    self.discover(child, scopes, ancestry, depth + 1)?;
                }
            }
            Some("group") => self.discover(&node["value"], scopes, ancestry, depth + 1)?,
            Some("logical") if node["operator"] == "&&" => {
                // The outer section visibility condition is retained verbatim;
                // deriving an item's fields does not prove that condition.
                ancestry.push(json!({"kind":"section-condition","expression":node["left"],
                    "conditionCompiled":false,"equivalent":false}));
                self.discover(&node["right"], scopes, ancestry, depth + 1)?;
                ancestry.pop();
            }
            Some("conditional") => {
                self.rejected
                    .push(self.issue(node, "ENTRY_SECTION_BRANCH_UNSUPPORTED"));
            }
            Some("binding") => {
                let id = node["id"].as_str().ok_or("ENTRY_BINDING_ID")?;
                let binding = self.binding(id, scopes)?;
                if binding["value"]["kind"] != "map-parameter" {
                    // A lexical alias for JSX is not guessed to be a component.
                    self.rejected
                        .push(self.issue(node, "ENTRY_RETURN_ALIAS_UNSUPPORTED"));
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn map(
        &mut self,
        node: &Value,
        module: &str,
        scopes: &BTreeSet<String>,
        ancestry: &[Value],
        depth: usize,
    ) -> Result<(), String> {
        self.tick(depth)?;
        let scope = node["scope"].as_str().ok_or("ENTRY_MAP_SCOPE")?;
        let start = node["span"]["startByte"].as_u64().ok_or("ENTRY_MAP_SPAN")?;
        let id = node["parameters"][0]
            .as_str()
            .ok_or("ENTRY_MAP_PARAMETER")?;
        if scope != format!("map:{start}") || !valid_span(&node["span"], self.file) {
            self.rejected
                .push(self.issue(node, "ENTRY_MAP_IDENTITY_MISMATCH"));
            return Ok(());
        }
        let mut scopes = scopes.clone();
        scopes.insert(scope.into());
        let item = self.data_path(&json!({"kind":"binding","id":id}), &scopes)?;
        let expected = Root::Item {
            id: id.into(),
            scope: scope.into(),
            map: start,
        };
        if item.root != expected || !item.fields.is_empty() {
            self.rejected
                .push(self.issue(node, "ENTRY_MAP_BINDING_MISMATCH"));
            return Ok(());
        }
        let mut entry = Entry {
            module: module.into(),
            ..Entry::default()
        };
        self.render(
            &node["body"],
            module,
            &expected,
            &scopes,
            &mut entry,
            depth + 1,
        )?;
        self.flush(&mut entry)?;
        let mut used = BTreeSet::new();
        for row in &entry.rows {
            for part in items(row, "parts") {
                if !used.insert(part["field"].as_str().unwrap_or("").to_owned()) {
                    entry
                        .rejected
                        .push(self.issue(node, "ENTRY_DUPLICATE_FIELD_UNREPRESENTABLE"));
                }
            }
        }
        if entry.rows.is_empty() && entry.rejected.is_empty() {
            return Ok(());
        }
        let header: Vec<_> = entry
            .rows
            .iter()
            .map(|row| json!({"role":row["role"],"parts":row["parts"]}))
            .collect();
        let usable = !entry.rows.is_empty() && entry.rejected.is_empty();
        let date_adapted = entry
            .differences
            .iter()
            .any(|difference| difference["code"] == "DATE_FORMATTER_ADAPTED");
        let punctuation_verified = entry.rejected.is_empty()
            && entry.differences.iter().all(|difference| {
                matches!(
                    difference["code"].as_str(),
                    Some("DATE_FORMATTER_ADAPTED" | "DATE_PRESENCE_GUARD_ADAPTED")
                )
            });
        self.candidates.push(json!({
            "module":module,"entryFile":self.file,"sourceSha256":self.template["sourceSha256"],
            "mapSpan":node["span"],"mapScope":scope,"itemBinding":id,
            "sourceConditions":ancestry,"rows":entry.rows,"entryHeader":header,
            "usable":usable,"selectionUnambiguous":true,
            "safePartial":usable && date_adapted,"requiresDateFallback":date_adapted,"dateSemanticsVerified":false,
            "rejected":entry.rejected,"differences":entry.differences,"excludedBodyFields":entry.excluded,
            "fieldBindingsVerified":true,"sourceOrderVerified":true,
            "missingFieldPunctuationVerified":punctuation_verified,
            "componentSemanticsVerified":false,"layoutCompiled":false,"layoutVerified":false,
            "moduleInitializationVerified":false,"equivalent":false,
            "scope":"bounded direct field grouping; no computed CSS, runtime branch, component, date or visual equivalence proof"
        }));
        if self.candidates.len() + self.rejected.len() > MAX_RECORDS {
            return Err("ENTRY_TEMPLATE_RECORD_LIMIT".into());
        }
        Ok(())
    }

    fn same_item_field(
        &mut self,
        node: &Value,
        root: &Root,
        scopes: &BTreeSet<String>,
    ) -> Option<(String, Path)> {
        let path = self.data_path(node, scopes).ok()?;
        (path.root == *root && path.fields.len() == 1).then(|| (path.fields[0].clone(), path))
    }

    fn has_header_access(
        &mut self,
        node: &Value,
        module: &str,
        root: &Root,
        scopes: &BTreeSet<String>,
        depth: usize,
    ) -> Result<bool, String> {
        self.tick(depth)?;
        if let Some((field, _)) = self.same_item_field(node, root, scopes) {
            return Ok(is_header_field(module, &field)
                || matches!(field.as_str(), "startDate" | "endDate"));
        }
        match node {
            Value::Array(values) => {
                for value in values {
                    if self.has_header_access(value, module, root, scopes, depth + 1)? {
                        return Ok(true);
                    }
                }
            }
            Value::Object(values) => {
                for (key, value) in values {
                    if key != "span"
                        && self.has_header_access(value, module, root, scopes, depth + 1)?
                    {
                        return Ok(true);
                    }
                }
            }
            _ => {}
        }
        Ok(false)
    }

    fn known_body_condition(
        &mut self,
        node: &Value,
        root: &Root,
        scopes: &BTreeSet<String>,
    ) -> bool {
        let node = if node["kind"] == "compare" {
            &node["left"]
        } else {
            node
        };
        self.data_path(node, scopes).is_ok_and(|path| {
            path.root == *root
                && path.fields.first().is_some_and(|field| body_field(field))
                && (path.fields.len() == 1
                    || (path.fields.len() == 2 && path.fields[1] == "length"))
        })
    }

    fn render(
        &mut self,
        node: &Value,
        module: &str,
        root: &Root,
        scopes: &BTreeSet<String>,
        entry: &mut Entry,
        depth: usize,
    ) -> Result<(), String> {
        self.tick(depth)?;
        if let Some((field, path)) = self.same_item_field(node, root, scopes) {
            if body_field(&field) {
                self.flush(entry)?;
                entry
                    .excluded
                    .push(json!({"field":field,"span":node["span"],"bindingChain":path.bindings}));
            } else if is_header_field(module, &field) {
                entry.current.push(Token::Field {
                    field: field.clone(), guards: entry.guards.clone(), components: entry.components.clone(),
                    source: json!({"field":field,"span":node["span"],"accessKind":node["kind"],
                        "bindingChain":path.bindings,"itemBinding":match root {Root::Item{id,..}=>Some(id),_=>None},
                        "conditions":entry.guards,"bindingVerified":true,
                        "styleComponent":entry.components.last().map(|c|c.name.as_str()),
                        "styleComponentBinding":entry.components.last().and_then(|c|c.binding.as_deref()),
                        "components":entry.components.iter().map(Component::evidence).collect::<Vec<_>>()}),
                });
            } else {
                entry.rejected.push(self.issue(
                    node,
                    if matches!(field.as_str(), "startDate" | "endDate") {
                        "ENTRY_DATE_PAIR_SEMANTICS_UNSUPPORTED"
                    } else {
                        "ENTRY_NON_HEADER_FIELD_UNSUPPORTED"
                    },
                ));
            }
            return Ok(());
        }
        match node["kind"].as_str() {
            Some("element") => {
                if self.date_component(node, module, root, scopes, entry)? {
                    return Ok(());
                }
                let component = match self.component(node) {
                    Ok(component) => component,
                    Err(code) => {
                        self.flush(entry)?;
                        let mut issue = self.issue(node, &code);
                        issue["component"] = node["tag"].clone();
                        entry.rejected.push(issue);
                        return Ok(());
                    }
                };
                if items(node, "attributes").any(|attribute| {
                    matches!(
                        attribute["name"].as_str(),
                        Some("startDate" | "endDate" | "format" | "locale")
                    )
                }) {
                    entry
                        .rejected
                        .push(self.issue(node, "ENTRY_COMPONENT_DATE_PROPS_UNRESOLVED"));
                    return Ok(());
                }
                if component.tag == "br" {
                    self.flush(entry)?;
                    return Ok(());
                }
                if matches!(
                    component.tag.as_str(),
                    "table" | "thead" | "tbody" | "tr" | "td" | "th"
                ) {
                    entry
                        .rejected
                        .push(self.issue(node, "ENTRY_TABLE_LAYOUT_UNSUPPORTED"));
                    return Ok(());
                }
                // Runtime display/order and source event/style expressions are
                // not ignored. Frontend blockers have already rejected those.
                let block = component.block();
                if block {
                    self.flush(entry)?;
                }
                entry.components.push(component);
                for child in items(node, "children") {
                    self.render(child, module, root, scopes, entry, depth + 1)?;
                }
                if block {
                    self.flush(entry)?;
                }
                entry.components.pop();
            }
            Some("fragment") => {
                for child in items(node, "children") {
                    self.render(child, module, root, scopes, entry, depth + 1)?;
                }
            }
            Some("group") => self.render(&node["value"], module, root, scopes, entry, depth + 1)?,
            Some("text") => self.text(
                node["value"].as_str().ok_or("ENTRY_TEXT_INVALID")?,
                node,
                entry,
            )?,
            Some("literal") => match &node["value"] {
                Value::String(text) => self.text(text, node, entry)?,
                Value::Null | Value::Bool(_) => (),
                _ => entry
                    .rejected
                    .push(self.issue(node, "ENTRY_LITERAL_UNSUPPORTED")),
            },
            Some("empty" | "undefined") => (),
            Some("logical") if node["operator"] == "&&" => {
                if self.known_body_condition(&node["left"], root, scopes)
                    && !self.has_header_access(&node["right"], module, root, scopes, depth + 1)?
                {
                    self.flush(entry)?;
                    entry
                        .excluded
                        .push(json!({"kind":"body-condition","span":node["span"]}));
                    return Ok(());
                }
                let own_field = self
                    .same_item_field(&node["left"], root, scopes)
                    .filter(|(field, _)| is_header_field(module, field))
                    .map(|(field, _)| field);
                let date_guard = own_field.is_none()
                    && self.date_presence_condition(&node["left"], root, scopes);
                let field = own_field.or_else(|| date_guard.then(|| "dateRange".into()));
                if let Some(field) = field {
                    // Only a field's own truthiness guard is representable by
                    // the renderer's empty-field suppression. A guard on a
                    // different field would hide other content and is rejected.
                    let mut probe = Entry {
                        module: module.into(),
                        ..Entry::default()
                    };
                    probe.guards = entry.guards.clone();
                    probe.guards.insert(field.clone());
                    probe.components = entry.components.clone();
                    if date_guard {
                        probe.differences.push(json!({"code":"DATE_PRESENCE_GUARD_ADAPTED","span":node["span"],
                            "sourceCondition":node["left"],"dateSemanticsVerified":false,"equivalent":false}));
                    }
                    self.render(&node["right"], module, root, scopes, &mut probe, depth + 1)?;
                    let mut guarded = probe.current.clone();
                    for row in &probe.rows {
                        for evidence in items(row, "fieldEvidence") {
                            if evidence["field"] != field {
                                probe
                                    .rejected
                                    .push(self.issue(node, "ENTRY_CROSS_FIELD_GUARD_UNSUPPORTED"));
                            }
                        }
                    }
                    if guarded
                        .iter()
                        .any(|token| matches!(token,Token::Field{field:other,..} if other!=&field))
                    {
                        probe
                            .rejected
                            .push(self.issue(node, "ENTRY_CROSS_FIELD_GUARD_UNSUPPORTED"));
                    }
                    if probe.rows.is_empty() {
                        entry.current.append(&mut guarded);
                    } else {
                        self.flush(entry)?;
                        self.flush(&mut probe)?;
                        entry.rows.append(&mut probe.rows);
                    }
                    entry.rejected.append(&mut probe.rejected);
                    entry.differences.append(&mut probe.differences);
                    entry.excluded.append(&mut probe.excluded);
                } else {
                    entry
                        .rejected
                        .push(self.issue(node, "ENTRY_CONDITION_UNSUPPORTED"));
                }
            }
            Some("conditional") => {
                // A ternary's branches are never selected speculatively, even
                // when one happens to look empty in a particular fixture.
                entry
                    .rejected
                    .push(self.issue(node, "ENTRY_BRANCH_UNSUPPORTED"));
            }
            Some("text-template") => {
                for part in items(node, "parts") {
                    if part["kind"] == "to-string" {
                        if let Some((field, _)) = self.same_item_field(&part["value"], root, scopes)
                            && entry.guards.contains(&field)
                        {
                            self.render(&part["value"], module, root, scopes, entry, depth + 1)?;
                        } else {
                            entry
                                .rejected
                                .push(self.issue(part, "ENTRY_TEMPLATE_COERCION_UNGUARDED"));
                        }
                    } else if part["kind"] == "literal" && part["value"].is_string() {
                        self.text(part["value"].as_str().unwrap(), part, entry)?;
                    } else {
                        entry
                            .rejected
                            .push(self.issue(part, "ENTRY_TEMPLATE_PART_UNSUPPORTED"));
                    }
                }
            }
            Some("map") => {
                if self
                    .data_path(&node["collection"], scopes)
                    .is_ok_and(|path| {
                        path.root == *root && path.fields.len() == 1 && body_field(&path.fields[0])
                    })
                {
                    self.flush(entry)?;
                    entry
                        .excluded
                        .push(json!({"kind":"body-map","span":node["span"]}));
                } else {
                    entry
                        .rejected
                        .push(self.issue(node, "ENTRY_NESTED_MAP_UNSUPPORTED"));
                }
            }
            _ => entry
                .rejected
                .push(self.issue(node, "ENTRY_EXPRESSION_UNSUPPORTED")),
        }
        if entry.current.len() > 128
            || entry.rows.len() > MAX_ROWS
            || entry.rejected.len() + entry.excluded.len() + entry.differences.len() > MAX_RECORDS
        {
            return Err("ENTRY_TEMPLATE_RECORD_LIMIT".into());
        }
        Ok(())
    }

    fn text(&self, text: &str, node: &Value, entry: &mut Entry) -> Result<(), String> {
        if text.is_empty() {
            return Ok(());
        }
        if text.len() > MAX_TEXT
            || text
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
        {
            return Err("ENTRY_TEMPLATE_TEXT_LIMIT".into());
        }
        // Multiple active guards cannot be represented as independent part
        // prefixes. They are retained as an explicit rejection, not collapsed.
        if entry.guards.len() > 1 {
            entry
                .rejected
                .push(self.issue(node, "ENTRY_MULTI_FIELD_GUARD_UNSUPPORTED"));
        }
        entry.current.push(Token::Text {
            text: text.into(),
            guard: entry.guards.iter().next().cloned(),
            span: node["span"].clone(),
            components: entry.components.clone(),
        });
        Ok(())
    }

    fn flush(&self, entry: &mut Entry) -> Result<(), String> {
        let tokens = std::mem::take(&mut entry.current);
        if tokens.is_empty() {
            return Ok(());
        }
        if !tokens
            .iter()
            .any(|token| matches!(token, Token::Field { .. }))
        {
            if tokens
                .iter()
                .any(|token| matches!(token,Token::Text{text,..} if !text.trim().is_empty()))
            {
                entry
                    .rejected
                    .push(json!({"code":"ENTRY_STANDALONE_TEXT_UNREPRESENTABLE",
                    "entryFile":self.file,"equivalent":false}));
            }
            return Ok(());
        }
        let mut parts: Vec<Value> = Vec::new();
        let mut evidence = Vec::new();
        let mut components = Vec::new();
        let mut pending: Vec<(String, Option<String>, Value)> = Vec::new();
        let mut first_field = String::new();
        let mut heading = false;
        let mut style_contexts = Vec::new();
        for token in tokens {
            match token {
                Token::Text {
                    text,
                    guard,
                    span,
                    components,
                } => {
                    style_contexts.push(json!({"kind":"literal","span":span,
                        "components":components.iter().map(Component::evidence).collect::<Vec<_>>()}));
                    pending.push((text, guard, span));
                }
                Token::Field {
                    field,
                    guards,
                    source,
                    components: source_components,
                } => {
                    style_contexts.push(json!({"kind":"field","field":field,"span":source["span"],
                        "components":source_components.iter().map(Component::evidence).collect::<Vec<_>>()}));
                    if first_field.is_empty() {
                        first_field = field.clone();
                    }
                    let mut prefix = String::new();
                    for (text, guard, span) in std::mem::take(&mut pending) {
                        if guard.as_deref().is_some_and(|guard| guard != field) {
                            if let Some(previous) = parts.last_mut()
                                && guard.as_deref() == previous["field"].as_str()
                            {
                                let suffix = previous["suffix"].as_str().unwrap_or("");
                                previous["suffix"] = json!(format!("{suffix}{text}"));
                                continue;
                            }
                            entry.rejected.push(json!({"code":"ENTRY_LITERAL_GUARD_UNREPRESENTABLE",
                                "entryFile":self.file,"span":span,"guard":guard,"equivalent":false}));
                        } else {
                            if guard.is_none() && !text.trim().is_empty() {
                                entry.differences.push(json!({"code":"ENTRY_UNCONDITIONAL_LITERAL_SUPPRESSED_WITH_MISSING_FIELD",
                                    "field":field,"text":text,"span":span,"equivalent":false}));
                            }
                            prefix.push_str(&text);
                        }
                    }
                    if guards.iter().any(|guard| guard != &field) {
                        entry.rejected.push(json!({"code":"ENTRY_CROSS_FIELD_GUARD_UNSUPPORTED","field":field,
                            "guards":guards,"entryFile":self.file,"span":source["span"],"equivalent":false}));
                    }
                    let mut part = json!({"field":field});
                    if field == "dateRange" {
                        part["dateFormat"] = json!("short-en");
                    }
                    if !prefix.is_empty() {
                        part["prefix"] = json!(prefix);
                    }
                    parts.push(part);
                    evidence.push(source);
                    for component in source_components {
                        heading |= matches!(
                            component.tag.as_str(),
                            "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
                        );
                        if !components.iter().any(|c: &Component| {
                            c.name == component.name && c.binding == component.binding
                        }) {
                            components.push(component);
                        }
                    }
                }
            }
        }
        let last = parts.last_mut().ok_or("ENTRY_PARTS_MISSING")?;
        for (text, guard, span) in pending {
            if guard
                .as_deref()
                .is_some_and(|guard| Some(guard) != last["field"].as_str())
            {
                entry
                    .rejected
                    .push(json!({"code":"ENTRY_LITERAL_GUARD_UNREPRESENTABLE",
                    "entryFile":self.file,"span":span,"guard":guard,"equivalent":false}));
                continue;
            }
            if guard.is_none() && !text.trim().is_empty() {
                entry.differences.push(
                    json!({"code":"ENTRY_UNCONDITIONAL_LITERAL_SUPPRESSED_WITH_MISSING_FIELD",
                    "field":last["field"],"text":text,"span":span,"equivalent":false}),
                );
            }
            let suffix = last["suffix"].as_str().unwrap_or("");
            last["suffix"] = json!(format!("{suffix}{text}"));
        }
        if parts.len() > MAX_PARTS || entry.rows.len() >= MAX_ROWS {
            return Err("ENTRY_TEMPLATE_ROW_LIMIT".into());
        }
        if parts.len() > 6 || entry.rows.len() >= 8 {
            entry.rejected.push(
                json!({"code":"ENTRY_MANIFEST_ROW_LIMIT","entryFile":self.file,
                "equivalent":false}),
            );
        }
        for part in &parts {
            for key in ["prefix", "suffix"] {
                if let Some(text) = part[key].as_str()
                    && (text.chars().count() > 32
                        || text.chars().any(|c| {
                            c.is_control() || matches!(c, '<' | '>' | '\u{2028}' | '\u{2029}')
                        }))
                {
                    entry.rejected.push(json!({"code":"ENTRY_MANIFEST_LITERAL_UNREPRESENTABLE",
                        "entryFile":self.file,"field":part["field"],"property":key,"equivalent":false}));
                }
            }
        }
        // The innermost common block is a useful source style hint. Inline
        // descendants are retained separately; mixed inline styles are not
        // claimed to fit the renderer's single-style row.
        let block = components.iter().rev().find(|c| c.block());
        let style = if parts.len() == 1 {
            components.last()
        } else {
            block.or_else(|| components.last())
        };
        // One reused leaf binding is insufficient: inherited styles can differ
        // under different inline ancestors or selector-affecting attributes.
        // Prefix/suffix text has its own context too. This is a conservative
        // source-structure comparison, never a computed CSS equality claim.
        let single_source_style = style_contexts.first().is_none_or(|first| {
            style_contexts
                .iter()
                .all(|context| context["components"] == first["components"])
        });
        let primary = match entry.module.as_str() {
            "work" | "volunteer" => "position",
            "education" => "institution",
            "awards" => "title",
            _ => "name",
        };
        let role = if heading || first_field == primary {
            "itemTitle"
        } else if matches!(first_field.as_str(), "date" | "releaseDate" | "dateRange") {
            "meta"
        } else {
            "body"
        };
        entry.rows.push(json!({
            "role":role,"parts":parts,"fieldEvidence":evidence,
            "styleComponent":style.map(|c|c.name.as_str()),
            "styleComponentBinding":style.and_then(|c|c.binding.as_deref()),
            "nativeTag":style.map(|c|c.tag.as_str()),
            "components":components.iter().map(Component::evidence).collect::<Vec<_>>(),
            "singleSourceStyle":single_source_style,"styleContexts":style_contexts,
            "fieldBindingsVerified":true,"sourceOrderVerified":true,
            "styleInheritanceVerified":false,"computedLineGroupingVerified":false,"equivalent":false
        }));
        Ok(())
    }
}

/// Extract candidates from compiler-owned IR. This is an adaptation hint stage;
/// it never turns frontend unresolved=false into a complete/verified theme.
pub(crate) fn extract(report: &Value) -> Result<Value, String> {
    preflight(report)?;
    let mut candidates = Vec::new();
    let mut rejected = Vec::new();
    let templates: Vec<_> = items(report, "templatePrograms").collect();
    if templates.len() > 128 {
        return Err("ENTRY_TEMPLATE_PROGRAM_LIMIT".into());
    }
    for template in templates {
        let program = &template["program"];
        let file = template["entryFile"]
            .as_str()
            .ok_or("ENTRY_TEMPLATE_FILE")?;
        if file.len() > 512 || file.is_empty() {
            return Err("ENTRY_TEMPLATE_FILE".into());
        }
        let allowed = [
            "UNCOMPILED_STYLE_DEPENDENCY",
            "UNCOMPILED_COMPONENT_DEPENDENCY",
            "UNVERIFIED_URL_ATTRIBUTE",
        ];
        if program["format"] != "resume-one-template-ir-v1"
            || program["syntaxValid"] != true
            || program["source"]["path"] != file
            || program["entry"]["kind"] != "function"
        {
            rejected.push(json!({"entryFile":file,"code":"ENTRY_TEMPLATE_FRONTEND_UNSUPPORTED","equivalent":false}));
            continue;
        }
        let unsupported: Vec<_> = items(program, "blockers")
            .filter(|issue| !allowed.iter().any(|code| issue["code"] == *code))
            .cloned()
            .collect();
        if !unsupported.is_empty() {
            rejected.push(json!({"entryFile":file,"code":"ENTRY_TEMPLATE_FRONTEND_BLOCKED","blockers":unsupported,"equivalent":false}));
            continue;
        }
        let scope = program["entry"]["scope"]
            .as_str()
            .ok_or("ENTRY_TEMPLATE_FUNCTION_SCOPE")?;
        let mut extractor = Extractor {
            report,
            template,
            file,
            bindings: BTreeMap::new(),
            steps: 0,
            entry_scope: scope,
            candidates: Vec::new(),
            rejected: Vec::new(),
        };
        for binding in items(program, "bindings") {
            let id = binding["id"].as_str().ok_or("ENTRY_BINDING_ID")?;
            let name = binding["name"].as_str().ok_or("ENTRY_BINDING_NAME")?;
            if !valid_span(&binding["span"], file)
                || id
                    != format!(
                        "b:{}:{name}",
                        binding["span"]["startByte"].as_u64().unwrap()
                    )
                || extractor.bindings.insert(id, binding).is_some()
            {
                return Err("ENTRY_BINDING_IDENTITY_MISMATCH".into());
            }
        }
        let scopes = BTreeSet::from([extractor.entry_scope.to_owned()]);
        extractor.discover(&program["entry"]["body"], &scopes, &mut Vec::new(), 0)?;
        candidates.append(&mut extractor.candidates);
        rejected.append(&mut extractor.rejected);
        if candidates.len() + rejected.len() > MAX_RECORDS {
            return Err("ENTRY_TEMPLATE_RECORD_LIMIT".into());
        }
    }
    // Different returns/files/maps for the same module are alternatives, not a
    // declaration that the last candidate wins. The integrating adapter must
    // not silently choose one of these variants.
    let mut counts = BTreeMap::new();
    for candidate in &candidates {
        *counts
            .entry(candidate["module"].as_str().unwrap_or("").to_owned())
            .or_insert(0usize) += 1;
    }
    for candidate in &mut candidates {
        if counts[candidate["module"].as_str().unwrap_or("")] > 1 {
            candidate["selectionUnambiguous"] = json!(false);
            candidate["usable"] = json!(false);
            candidate["safePartial"] = json!(false);
            candidate["rejected"].as_array_mut().unwrap().push(json!({
                "code":"ENTRY_MULTIPLE_MODULE_CANDIDATES","equivalent":false
            }));
        }
    }
    let output = json!({"format":"resume-one-entry-templates-v1","candidates":candidates,"rejected":rejected,
        "executedSourceCode":false,"sourceModified":false,"bindingVerified":false,
        "layoutCompiled":false,"visualVerified":false,"migrationComplete":false,"equivalent":false,
        "scope":"direct bound item fields, native source grouping, self-field truthiness and guarded scalar text templates only"});
    serde_json::to_writer(OutputBudget(0), &output)
        .map_err(|_| "ENTRY_TEMPLATE_OUTPUT_LIMIT".to_owned())?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inspection(source: &str) -> Value {
        let program = crate::template_ir::compile("Resume.jsx", source.as_bytes()).unwrap();
        let syntax = crate::syntax::analyze("Resume.jsx", source.as_bytes())
            .unwrap()
            .unwrap();
        let graph = crate::style_link::link("Resume.jsx", &syntax, &program).unwrap();
        json!({"syntaxEvidence":[syntax],"templatePrograms":[{"entryFile":"Resume.jsx","sourceSha256":"a".repeat(64),"program":program,"styleGraph":graph}]})
    }

    fn run(source: &str) -> Value {
        extract(&inspection(source)).unwrap()
    }

    fn adapted(report: &Value) -> (Value, Value) {
        let native: Value =
            serde_json::from_slice(include_bytes!("../../../assets/themes/zhibai.theme.json"))
                .unwrap();
        crate::adapt_style::adapt(report, native).unwrap()
    }

    fn has_difference(report: &Value, code: &str) -> bool {
        items(report, "differences").any(|difference| difference["code"] == code)
    }

    fn candidate<'a>(value: &'a Value, module: &str) -> &'a Value {
        items(value, "candidates")
            .find(|c| c["module"] == module)
            .unwrap()
    }

    #[test]
    fn academic_like_fields_follow_bound_source_rows_and_actual_style_components() {
        let value = run(r#"
            import styled from 'styled-components';
            const Item = styled.div`margin-bottom:20px;`;
            const Institution = styled.h3`font-size:13pt;`;
            const Degree = styled.div`font-size:12pt;`;
            const Position = styled.h3`font-size:13pt;`;
            const Company = styled.div`font-style:italic;`;
            export default function Resume({resume}) {
              const {education = [], work = []} = resume;
              return <main>
                {education.map((edu, index) => <Item key={index}>
                  <Institution>{edu.institution}</Institution>
                  <Degree>{edu.studyType} in {edu.area}{edu.score && ` • ${edu.score}`}</Degree>
                </Item>)}
                {work.map((job, index) => <Item key={index}>
                  <Position>{job.position}</Position>
                  {job.name && <Company>{job.name}</Company>}
                  {job.summary && <p>{job.summary}</p>}
                </Item>)}
              </main>;
            }
        "#);
        let education = candidate(&value, "education");
        assert_eq!(education["usable"], true, "{value:#}");
        assert_eq!(
            education["entryHeader"],
            json!([
                {"role":"itemTitle","parts":[{"field":"institution"}]},
                {"role":"body","parts":[{"field":"studyType"},{"field":"area","prefix":" in "},{"field":"score","prefix":" • "}]}
            ])
        );
        assert_eq!(education["rows"][0]["styleComponent"], "Institution");
        assert_eq!(education["rows"][1]["styleComponent"], "Degree");
        assert_eq!(education["missingFieldPunctuationVerified"], false);
        assert_eq!(
            education["rows"][1]["fieldEvidence"][2]["conditions"],
            json!(["score"])
        );
        assert_eq!(education["fieldBindingsVerified"], true);
        assert_eq!(education["equivalent"], false);
        let work = candidate(&value, "work");
        assert_eq!(
            work["entryHeader"],
            json!([
                {"role":"itemTitle","parts":[{"field":"position"}]},
                {"role":"body","parts":[{"field":"name"}]}
            ])
        );
        assert_eq!(work["rows"][1]["styleComponent"], "Company");
        assert!(!work["excludedBodyFields"].as_array().unwrap().is_empty());
    }

    #[test]
    fn shadowed_map_items_never_cross_module_binding_identity() {
        let value = run(r#"export default ({resume}) => <main>
          {resume.work.map(item => <div><h3>{item.position}</h3><span>{item.name}</span></div>)}
          {resume.education.map(item => <div><h3>{item.institution}</h3><span>{item.area}</span></div>)}
        </main>"#);
        let work = candidate(&value, "work");
        let education = candidate(&value, "education");
        assert_ne!(work["itemBinding"], education["itemBinding"]);
        assert_eq!(
            work["rows"][0]["fieldEvidence"][0]["itemBinding"],
            work["itemBinding"]
        );
        assert_eq!(
            education["rows"][0]["fieldEvidence"][0]["itemBinding"],
            education["itemBinding"]
        );
        assert_eq!(education["usable"], true);
    }

    #[test]
    fn direct_const_alias_and_destructured_item_fields_keep_binding_chains() {
        let value = run(r#"export default ({resume:r}) => <main>{r.work.map(job => {
          const {position: title, name} = job;
          return <div><h3>{title}</h3><p>{name}</p></div>;
        })}</main>"#);
        let work = candidate(&value, "work");
        assert_eq!(work["usable"], true, "{value:#}");
        assert_eq!(work["entryHeader"][0]["parts"][0]["field"], "position");
        assert_eq!(
            work["rows"][0]["fieldEvidence"][0]["bindingChain"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn mutation_unknown_helpers_and_conditional_branches_fail_closed() {
        for source in [
            "export default ({resume}) => { let work = resume.work; return <div>{work.map(job => <h3>{job.position}</h3>)}</div>; }",
            "export default ({resume}) => <div>{resume.work.map(job => <h3>{format(job.position)}</h3>)}</div>",
            "export default ({resume}) => <div>{resume.work.map(job => <div>{job.name ? <h3>{job.position}</h3> : <h3>{job.name}</h3>}</div>)}</div>",
        ] {
            let value = run(source);
            assert!(
                items(&value, "candidates").all(|c| c["usable"] == false),
                "{value:#}"
            );
            assert!(
                !value["rejected"].as_array().unwrap().is_empty()
                    || items(&value, "candidates")
                        .any(|c| !c["rejected"].as_array().unwrap().is_empty())
            );
        }
    }

    #[test]
    fn punctuation_is_attached_to_its_own_guarded_field_and_does_not_leak() {
        let value = run(
            r#"export default ({resume}) => <div>{resume.education.map(edu => <p>
          {edu.area}{edu.score && ` (${edu.score})`}
        </p>)}</div>"#,
        );
        let education = candidate(&value, "education");
        assert_eq!(education["usable"], true, "{value:#}");
        assert_eq!(
            education["entryHeader"][0]["parts"],
            json!([
                {"field":"area"},{"field":"score","prefix":" (","suffix":")"}
            ])
        );
        assert_eq!(education["missingFieldPunctuationVerified"], true);
        // Render the same bounded part rule with a missing score: no bare ().
        let fields = json!({"area":"Mathematics"});
        let mut rendered = String::new();
        for part in items(&education["entryHeader"][0], "parts") {
            if let Some(value) = fields[part["field"].as_str().unwrap()]
                .as_str()
                .filter(|s| !s.is_empty())
            {
                rendered.push_str(part["prefix"].as_str().unwrap_or(""));
                rendered.push_str(value);
                rendered.push_str(part["suffix"].as_str().unwrap_or(""));
            }
        }
        assert_eq!(rendered, "Mathematics");
    }

    #[test]
    fn guard_for_one_field_cannot_silently_control_another_field() {
        let value = run(
            r#"export default ({resume}) => <div>{resume.work.map(job => <div>
          {job.name && <h3>{job.position}</h3>}
        </div>)}</div>"#,
        );
        let work = candidate(&value, "work");
        assert_eq!(work["usable"], false);
        assert!(
            items(work, "rejected").any(|r| r["code"] == "ENTRY_CROSS_FIELD_GUARD_UNSUPPORTED")
        );
    }

    #[test]
    fn default_core_date_range_alias_is_explicitly_adapted_with_real_parent_style() {
        let value = run(r#"import styled from 'styled-components';
          import {DateRange as Period} from '@jsonresume/core';
          const DateText=styled.div`font-size:11pt;color:#6b7280;`;
          export default ({resume}) => <div>{resume.work.map(job => <div>
            <h3>{job.position}</h3><DateText><Period startDate={job.startDate} endDate={job.endDate}/></DateText>
          </div>)}</div>"#);
        let work = candidate(&value, "work");
        assert_eq!(work["usable"], true, "{value:#}");
        assert_eq!(work["safePartial"], true);
        assert_eq!(work["requiresDateFallback"], true);
        assert_eq!(work["dateSemanticsVerified"], false);
        assert_eq!(work["equivalent"], false);
        assert_eq!(
            work["entryHeader"][1],
            json!({"role":"meta","parts":[{"field":"dateRange","dateFormat":"short-en"}]})
        );
        assert_eq!(work["rows"][1]["styleComponent"], "DateText");
        let difference = items(work, "differences")
            .find(|r| r["code"] == "DATE_FORMATTER_ADAPTED")
            .unwrap();
        assert_eq!(difference["import"]["local"], "Period");
        assert_eq!(difference["import"]["exported"], "DateRange");
        assert_eq!(difference["import"]["importDeclarationVerified"], true);
        assert_eq!(difference["dateSemanticsVerified"], false);
        assert_eq!(difference["dependencySnapshotMatched"], false);
        assert_eq!(difference["arguments"][0]["field"], "startDate");
        assert_eq!(difference["arguments"][1]["field"], "endDate");
        let mut theme: Value =
            serde_json::from_slice(include_bytes!("../../../assets/themes/zhibai.theme.json"))
                .unwrap();
        theme["sections"]["work"]["date"] = json!("inline");
        theme["sections"]["work"]["entryHeader"] = work["entryHeader"].clone();
        assert!(resume_one_core::parse_theme(theme.to_string().as_bytes()).is_ok());
    }

    #[test]
    fn wrong_date_import_local_component_and_shadowed_component_fail_closed() {
        for source in [
            "import {DateRange} from 'other-package';export default ({resume}) => <div>{resume.work.map(job => <div><h3>{job.position}</h3><DateRange startDate={job.startDate} endDate={job.endDate}/></div>)}</div>",
            "function DateRange(p){return <span/>;}export default ({resume}) => <div>{resume.work.map(job => <div><h3>{job.position}</h3><DateRange startDate={job.startDate} endDate={job.endDate}/></div>)}</div>",
            "import {DateRange} from '@jsonresume/core';export default ({resume}) => <div>{resume.work.map(DateRange => <div><h3>{DateRange.position}</h3><DateRange startDate={DateRange.startDate} endDate={DateRange.endDate}/></div>)}</div>",
            "import styled from 'styled-components';const DateRange=styled.span`color:red;`;export default ({resume}) => <div>{resume.work.map(job => <div><h3>{job.position}</h3><DateRange startDate={job.startDate} endDate={job.endDate}/></div>)}</div>",
        ] {
            let value = run(source);
            assert!(
                items(&value, "candidates")
                    .all(|c| c["usable"] == false && c["safePartial"] == false),
                "{value:#}"
            );
        }
    }

    #[test]
    fn non_default_date_props_wrong_arguments_and_foreign_fields_fail_closed() {
        for props in [
            "startDate={job.startDate} endDate={job.endDate} format=\"long\"",
            "startDate={job.startDate} endDate={job.endDate} locale=\"fr\"",
            "startDate={job.startDate} endDate={job.endDate} presentLabel=\"Now\"",
            "startDate={job.endDate} endDate={job.startDate}",
            "startDate={resume.basics.name} endDate={job.endDate}",
            "startDate={job.startDate}",
        ] {
            let source = format!(
                "import {{DateRange}} from '@jsonresume/core';export default ({{resume}}) => <div>{{resume.work.map(job => <div><h3>{{job.position}}</h3><DateRange {props}/></div>)}}</div>"
            );
            let value = run(&source);
            assert!(
                items(&value, "candidates")
                    .all(|c| c["usable"] == false && c["safePartial"] == false),
                "{value:#}"
            );
        }
    }

    #[test]
    fn date_presence_guard_only_applies_to_date_row_not_other_headers() {
        let source = r#"import {DateRange} from '@jsonresume/core';export default ({resume}) => <div>
          {resume.volunteer.map(job => <div><h3>{job.position}</h3>
            {(job.startDate || job.endDate) && <p><DateRange startDate={job.startDate} endDate={job.endDate}/></p>}
          </div>)}</div>"#;
        let value = run(source);
        let volunteer = candidate(&value, "volunteer");
        assert_eq!(volunteer["usable"], true, "{value:#}");
        assert_eq!(volunteer["safePartial"], true);
        assert_eq!(
            volunteer["rows"][1]["fieldEvidence"][0]["conditions"],
            json!(["dateRange"])
        );
        for replaced in [
            source.replace("job.startDate || job.endDate", "job.endDate"),
            source.replace(
                "job.startDate || job.endDate",
                "job.startDate && job.endDate",
            ),
            source.replace("<p><DateRange", "<p>{job.organization}<DateRange"),
        ] {
            let rejected = run(&replaced);
            assert_eq!(
                candidate(&rejected, "volunteer")["usable"],
                false,
                "{rejected:#}"
            );
        }
    }

    #[test]
    fn duplicate_date_rows_remain_rejected_after_semantic_adaptation() {
        let value = run(
            r#"import {DateRange as Period} from '@jsonresume/core';export default ({resume}) => <div>
          {resume.work.map(job => <div><h3>{job.position}</h3>
            <p><Period startDate={job.startDate} endDate={job.endDate}/></p>
            <p><Period startDate={job.startDate} endDate={job.endDate}/></p>
          </div>)}</div>"#,
        );
        let work = candidate(&value, "work");
        assert_eq!(work["usable"], false);
        assert_eq!(work["safePartial"], false);
        assert!(
            items(work, "rejected").any(|r| r["code"] == "ENTRY_DUPLICATE_FIELD_UNREPRESENTABLE")
        );
    }

    #[test]
    fn unguarded_template_string_missing_semantics_stay_rejected() {
        let value = run(
            r#"export default ({resume}) => <div>{resume.education.map(edu =>
          <p>{`${edu.score}`}</p>)}</div>"#,
        );
        let education = candidate(&value, "education");
        assert_eq!(education["usable"], false);
        assert!(
            items(education, "rejected").any(|r| r["code"] == "ENTRY_TEMPLATE_COERCION_UNGUARDED")
        );
    }

    #[test]
    fn multiple_actual_maps_are_explicit_alternatives_not_last_write_wins() {
        let value = run(r#"export default ({resume}) => <main>
          {resume.work.map(job => <h3>{job.position}</h3>)}
          {resume.work.map(job => <p>{job.name}</p>)}
        </main>"#);
        assert_eq!(value["candidates"].as_array().unwrap().len(), 2);
        assert!(
            items(&value, "candidates")
                .all(|c| c["usable"] == false && c["selectionUnambiguous"] == false)
        );
    }

    #[test]
    fn report_limits_and_duplicate_binding_identity_are_rejected() {
        let huge = json!({"templatePrograms":[],"payload":"x".repeat(MAX_INPUT_BYTES+1)});
        assert!(extract(&huge).is_err());
        let program=crate::template_ir::compile("Resume.jsx",b"export default ({resume}) => <h3>{resume.work.map(job => <span>{job.position}</span>)}</h3>").unwrap();
        let mut report = json!({"templatePrograms":[{"entryFile":"Resume.jsx","program":program}]});
        let first = report["templatePrograms"][0]["program"]["bindings"][0].clone();
        report["templatePrograms"][0]["program"]["bindings"]
            .as_array_mut()
            .unwrap()
            .push(first);
        assert_eq!(
            extract(&report).unwrap_err(),
            "ENTRY_BINDING_IDENTITY_MISMATCH"
        );
    }

    #[test]
    fn usable_rows_satisfy_actual_manifest_contract_and_literal_limits() {
        let value = run(
            r#"export default ({resume}) => <main>{resume.education.map(edu => <div>
          <h3>{edu.institution}</h3><p>{edu.area}{edu.score && ` (${edu.score})`}</p>
        </div>)}</main>"#,
        );
        let education = candidate(&value, "education");
        let mut theme: Value =
            serde_json::from_slice(include_bytes!("../../../assets/themes/zhibai.theme.json"))
                .unwrap();
        theme["sections"]["education"]["date"] = json!("inline");
        theme["sections"]["education"]["entryHeader"] = education["entryHeader"].clone();
        assert!(resume_one_core::parse_theme(theme.to_string().as_bytes()).is_ok());
        let invalid = run(
            r#"export default ({resume}) => <main>{resume.education.map(edu =>
          <p>{edu.score && `<${edu.score}>`}</p>)}</main>"#,
        );
        let education = candidate(&invalid, "education");
        assert_eq!(education["usable"], false);
        assert!(
            items(education, "rejected")
                .any(|r| r["code"] == "ENTRY_MANIFEST_LITERAL_UNREPRESENTABLE")
        );
    }

    #[test]
    fn integration_academic_rows_keep_order_real_bindings_local_styles_and_non_equivalent_dates() {
        let inspected = inspection(
            r#"
            import styled from 'styled-components';
            import {DateRange as Period} from '@jsonresume/core';
            const Layout=styled.main`font-family:'Merriweather',serif;font-size:12pt;line-height:1.6;`;
            const Item=styled.div`margin-bottom:20px;`;
            const Institution=styled.h3`font-size:13pt;font-weight:700;margin-bottom:4px;`;
            const Degree=styled.div`font-size:12pt;`;
            const Position=styled.h3`font-size:13pt;font-weight:700;`;
            const Company=styled.div`font-size:12pt;font-style:italic;`;
            const DateText=styled.div`font-size:11pt;color:#555555;`;
            export default function Resume({resume}) {
              const {education=[],work=[]}=resume;
              return <Layout>
                {education.map(item => <Item>
                  <Institution>{item.institution}</Institution>
                  <Degree>{item.studyType} in {item.area}{item.score && ` • ${item.score}`}</Degree>
                  <DateText><Period startDate={item.startDate} endDate={item.endDate}/></DateText>
                </Item>)}
                {work.map(item => <Item>
                  <Position>{item.position}</Position>
                  {item.name && <Company>{item.name}</Company>}
                  <DateText><Period startDate={item.startDate} endDate={item.endDate}/></DateText>
                  {item.summary && <p>{item.summary}</p>}
                </Item>)}
              </Layout>;
            }
        "#,
        );
        let (theme, report) = adapted(&inspected);
        let proof = &report["entryTemplates"];
        let education = candidate(proof, "education");
        let work = candidate(proof, "work");
        assert_ne!(education["itemBinding"], work["itemBinding"]);
        for (module, expected) in [
            (
                "education",
                vec!["institution", "studyType", "area", "score", "dateRange"],
            ),
            ("work", vec!["position", "name", "dateRange"]),
        ] {
            let source = candidate(proof, module);
            assert_eq!(source["usable"], true, "{report:#}");
            assert_eq!(source["safePartial"], true);
            assert_eq!(source["requiresDateFallback"], true);
            assert_eq!(source["dateSemanticsVerified"], false);
            assert_eq!(source["equivalent"], false);
            assert!(items(source, "rows").all(|row| row["singleSourceStyle"] == true));
            assert!(
                items(source, "rows")
                    .flat_map(|row| items(row, "fieldEvidence"))
                    .all(|field| field["itemBinding"] == source["itemBinding"]
                        && field["bindingVerified"] == true)
            );
            let actual: Vec<_> = items(&theme["sections"], module).collect();
            assert!(actual.is_empty()); // Section styles are objects, never an alternative row list.
            let actual: Vec<_> = items(&theme["sections"][module], "entryHeader")
                .flat_map(|row| items(row, "parts"))
                .map(|part| part["field"].as_str().unwrap())
                .collect();
            assert_eq!(actual, expected);
            assert_eq!(theme["sections"][module]["date"], "inline");
            assert_eq!(
                theme["sections"][module]["entryHeader"][0]["style"]["sizePt"],
                13.0
            );
            assert_eq!(
                theme["sections"][module]["entryHeader"][1]["style"]["sizePt"],
                12.0
            );
            assert_eq!(
                theme["sections"][module]["entryHeader"][2]["style"]["sizePt"],
                11.0
            );
            assert_eq!(
                theme["sections"][module]["entryHeader"][2]["parts"][0]["dateFormat"],
                "short-en"
            );
            assert!(
                items(source, "differences")
                    .any(|difference| difference["code"] == "DATE_FORMATTER_ADAPTED"
                        && difference["dateSemanticsVerified"] == false
                        && difference["equivalent"] == false)
            );
        }
        assert_eq!(
            theme["sections"]["education"]["entryHeader"][0]["spaceAfterPt"],
            3.0
        );
        assert!(items(&report, "entryHeaderFonts").any(|font| {
            font["path"] == "/sections/work/entryHeader/1/style"
                && font["request"]["fontStyle"] == "italic"
        }));
        assert_eq!(report["fidelity"], "adapted");
        assert_eq!(report["visualVerified"], false);
        assert_eq!(report["migrationComplete"], false);
        assert_eq!(proof["executedSourceCode"], false);
    }

    #[test]
    fn integration_mixed_leaf_styles_skip_the_entire_module_instead_of_partial_rows() {
        let inspected = inspection(
            r#"import styled from 'styled-components';
            const Title=styled.h3`font-size:14pt;`;
            const Publisher=styled.span`font-style:italic;`;
            const DateText=styled.span`font-size:9pt;`;
            export default ({resume}) => <main>{resume.publications.map(item => <article>
              <Title>{item.name}</Title>
              <p><Publisher>{item.publisher}</Publisher><DateText>{item.releaseDate}</DateText></p>
            </article>)}</main>"#,
        );
        let (theme, report) = adapted(&inspected);
        let publications = candidate(&report["entryTemplates"], "publications");
        assert_eq!(publications["usable"], true);
        assert_eq!(publications["rows"][0]["singleSourceStyle"], true);
        assert_eq!(publications["rows"][1]["singleSourceStyle"], false);
        assert!(
            theme["sections"]["publications"]
                .get("entryHeader")
                .is_none()
        );
        assert!(has_difference(
            &report,
            "ENTRY_MIXED_INLINE_STYLES_UNREPRESENTABLE"
        ));
        assert!(items(&report, "entryHeaderFonts").all(|font| {
            !font["path"]
                .as_str()
                .unwrap()
                .starts_with("/sections/publications/")
        }));
    }

    #[test]
    fn integration_reused_leaf_under_different_ancestors_cannot_be_single_source_style() {
        let inspected = inspection(
            r#"import styled from 'styled-components';
            const Red=styled.span`color:#990000;`;
            const Blue=styled.span`color:#000099;`;
            const Shared=styled.span`font-size:12pt;`;
            export default ({resume}) => <main>{resume.work.map(item => <p>
              <Red><Shared>{item.position}</Shared></Red><Blue><Shared>{item.name}</Shared></Blue>
            </p>)}</main>"#,
        );
        let (theme, report) = adapted(&inspected);
        let work = candidate(&report["entryTemplates"], "work");
        let row = &work["rows"][0];
        assert_eq!(row["fieldEvidence"][0]["styleComponent"], "Shared");
        assert_eq!(row["fieldEvidence"][1]["styleComponent"], "Shared");
        assert_ne!(
            row["fieldEvidence"][0]["components"],
            row["fieldEvidence"][1]["components"]
        );
        assert_eq!(row["singleSourceStyle"], false);
        assert!(theme["sections"]["work"].get("entryHeader").is_none());
        assert!(has_difference(
            &report,
            "ENTRY_MIXED_INLINE_STYLES_UNREPRESENTABLE"
        ));
    }

    #[test]
    fn integration_selector_attributes_and_literal_text_participate_in_style_context() {
        for source in [
            r#"export default ({resume}) => <main>{resume.work.map(item => <p><span className="red">{item.position}</span><span className="blue">{item.name}</span></p>)}</main>"#,
            r#"export default ({resume}) => <main>{resume.work.map(item => <p><strong>Position: </strong>{item.position}</p>)}</main>"#,
        ] {
            let (theme, report) = adapted(&inspection(source));
            let work = candidate(&report["entryTemplates"], "work");
            assert_eq!(work["rows"][0]["singleSourceStyle"], false, "{report:#}");
            assert!(theme["sections"]["work"].get("entryHeader").is_none());
            assert!(has_difference(
                &report,
                "ENTRY_MIXED_INLINE_STYLES_UNREPRESENTABLE"
            ));
        }
        let same = run(
            r#"export default ({resume}) => <main>{resume.work.map(item => <p><span className="same">{item.position}</span><span className="same">{item.name}</span></p>)}</main>"#,
        );
        assert_eq!(
            candidate(&same, "work")["rows"][0]["singleSourceStyle"],
            true
        );
    }

    #[test]
    fn integration_invalid_item_binding_and_non_default_dates_are_not_applied() {
        let source = r#"import {DateRange} from '@jsonresume/core';
            export default ({resume}) => <main>{resume.work.map(item => <article>
              <h3>{item.position}</h3><p><DateRange startDate={item.startDate} endDate={item.endDate}/></p>
            </article>)}</main>"#;
        let mut invalid = inspection(source);
        fn corrupt(value: &mut Value) -> bool {
            if value["kind"] == "get" && value["property"] == "position" {
                value["object"]["id"] = json!("b:999999:item");
                return true;
            }
            match value {
                Value::Object(values) => values.values_mut().any(corrupt),
                Value::Array(values) => values.iter_mut().any(corrupt),
                _ => false,
            }
        }
        assert!(corrupt(
            &mut invalid["templatePrograms"][0]["program"]["entry"]
        ));
        for inspected in [
            invalid,
            inspection(&source.replace(
                "endDate={item.endDate}",
                "endDate={item.endDate} format=\"long\"",
            )),
        ] {
            let (theme, report) = adapted(&inspected);
            let work = candidate(&report["entryTemplates"], "work");
            assert_eq!(work["usable"], false);
            assert_eq!(work["safePartial"], false);
            assert!(theme["sections"]["work"].get("entryHeader").is_none());
        }
    }

    #[test]
    fn integration_missing_style_graph_is_a_component_rejection_not_an_empty_scan() {
        let mut inspected = inspection(
            r#"import styled from 'styled-components';
            const Title=styled.h3`font-size:13pt;`;
            export default ({resume}) => <main>{resume.work.map(item => <Title>{item.position}</Title>)}</main>"#,
        );
        inspected["templatePrograms"][0]
            .as_object_mut()
            .unwrap()
            .remove("styleGraph");
        let (theme, report) = adapted(&inspected);
        let proof = &report["entryTemplates"];
        assert_eq!(proof["candidates"].as_array().unwrap().len(), 1);
        let work = candidate(proof, "work");
        assert_eq!(work["usable"], false);
        assert!(items(work, "rejected").any(|issue| issue["code"] == "ENTRY_COMPONENT_UNRESOLVED"));
        assert!(theme["sections"]["work"].get("entryHeader").is_none());
    }

    #[test]
    fn integration_proven_styled_base_layers_supply_inherited_entry_style() {
        let inspected = inspection(
            r#"import styled from 'styled-components';
            const Layout=styled.main`font-size:12pt;`;
            const Base=styled.h3`font-size:17.25pt;color:#123456;`;
            const Title=styled(Base)`font-weight:700;`;
            export default ({resume}) => <Layout>{resume.work.map(item => <Title>{item.position}</Title>)}</Layout>"#,
        );
        let title = items(&inspected["templatePrograms"][0]["styleGraph"], "nodes")
            .find(|node| node["name"] == "Title")
            .unwrap();
        assert_eq!(title["inheritanceLinked"], true);
        assert_eq!(title["layers"].as_array().unwrap().len(), 2);
        let (theme, _) = adapted(&inspected);
        assert_eq!(
            theme["sections"]["work"]["entryHeader"][0]["style"]["sizePt"],
            17.25
        );
        assert_eq!(
            theme["sections"]["work"]["entryHeader"][0]["style"]["color"],
            "#123456"
        );
    }
}
