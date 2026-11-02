//! ESM symbol evidence from a caller-owned, whole-file CST and original bytes.
//! No evaluation, snippet reparse, alias folding, module loading or purity proof.
//! Only direct module declarations participate in the local symbol namespace.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tree_sitter::Node;

const MAX_BYTES: usize = 8 * 1024 * 1024;
const MAX_NODES: usize = 100_000;
const MAX_DEPTH: usize = 128;
const MAX_RECORDS: usize = 8_192;

struct Local {
    initialized_at: usize,
}

struct Reference {
    name: String,
    span: Value,
    at: usize,
    snapshot: bool,
    export: Option<usize>,
}

enum ExportTarget {
    Local {
        local: Option<String>,
        snapshot: bool,
    },
    Reexport {
        module: Option<String>,
        imported: Option<String>,
    },
    Unresolved {
        module: Option<String>,
        imported: Option<String>,
    },
}

struct Symbols<'a> {
    path: &'a str,
    source: &'a str,
    imports: Vec<Value>,
    exports: Vec<Value>,
    declarations: Vec<Value>,
    top_level: Vec<Value>,
    side_effects: Vec<Value>,
    blockers: Vec<Value>,
    locals: BTreeMap<String, Local>,
    collisions: BTreeSet<String>,
    exported: BTreeSet<String>,
    references: Vec<Reference>,
    started: Instant,
}

/// `root` must be the program parsed from these exact bytes by the caller.
/// Resource/input contract violations are Err; unsupported semantics are data.
pub(crate) fn extract(path: &str, source: &[u8], root: Node<'_>) -> Result<Value, String> {
    if source.len() > MAX_BYTES || path.len() > 4_096 {
        return Err(
            "MODULE_SYMBOL_INPUT_LIMIT: source exceeds 8 MiB or path exceeds 4096 bytes".into(),
        );
    }
    let text = std::str::from_utf8(source).map_err(|_| "MODULE_SYMBOL_UTF8: invalid UTF-8")?;
    if root.kind() != "program"
        || root.parent().is_some()
        || root.start_byte() > source.len()
        || !text.is_char_boundary(root.start_byte())
        || !text[..root.start_byte()].chars().all(|ch| {
            matches!(ch,
            '\u{0009}'..='\u{000d}' | ' ' | '\u{00a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' |
            '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}')
        })
        || root.end_byte() != source.len()
    {
        return Err("MODULE_SYMBOL_CST: require the caller's whole-file program CST for these original bytes".into());
    }
    let mut symbols = Symbols {
        path,
        source: text,
        imports: Vec::new(),
        exports: Vec::new(),
        declarations: Vec::new(),
        top_level: Vec::new(),
        side_effects: Vec::new(),
        blockers: Vec::new(),
        locals: BTreeMap::new(),
        collisions: BTreeSet::new(),
        exported: BTreeSet::new(),
        references: Vec::new(),
        started: Instant::now(),
    };
    let mut syntax_valid = true;
    let mut pending = vec![(root, 0)];
    let mut count = 0;
    while let Some((node, depth)) = pending.pop() {
        count += 1;
        symbols.budget()?;
        if count > MAX_NODES || depth > MAX_DEPTH {
            return Err("MODULE_SYMBOL_AST_LIMIT: more than 100000 nodes or depth 128".into());
        }
        if node.start_byte() > node.end_byte()
            || node.end_byte() > source.len()
            || !text.is_char_boundary(node.start_byte())
            || !text.is_char_boundary(node.end_byte())
        {
            return Err("MODULE_SYMBOL_CST: CST ranges do not address original UTF-8 bytes".into());
        }
        if node.is_error() || node.is_missing() {
            symbols.issue(
                node,
                "MODULE_SYMBOL_SYNTAX",
                "error/missing CST node; symbols are not inferred from broken syntax",
            );
            syntax_valid = false;
        }
        if matches!(node.kind(), "import_statement" | "export_statement")
            && node
                .parent()
                .is_some_and(|parent| parent.kind() != "program")
        {
            symbols.issue(
                node,
                "NESTED_MODULE_STATEMENT",
                "ESM imports/exports must be direct module statements",
            );
            syntax_valid = false;
        }
        if node.kind() == "jsx_element"
            && let (Some(open), Some(close)) = (
                node.child_by_field_name("open_tag"),
                node.child_by_field_name("close_tag"),
            )
            && open.child_by_field_name("name").map(|n| symbols.raw(n))
                != close.child_by_field_name("name").map(|n| symbols.raw(n))
        {
            symbols.issue(
                close,
                "MODULE_SYMBOL_JSX_TAG",
                "opening and closing JSX names differ",
            );
            syntax_valid = false;
        }
        for index in (0..node.child_count()).rev() {
            if let Some(child) = node.child(index) {
                pending.push((child, depth + 1));
            }
        }
    }
    for (order, node) in children(root).into_iter().enumerate() {
        symbols.budget()?;
        let status = match node.kind() {
            "comment" => "none-comment",
            "function_declaration" | "generator_function_declaration" => {
                "function-body-not-analyzed"
            }
            "import_statement" => "dependency-load-not-analyzed",
            "export_statement" if node.child_by_field_name("source").is_some() => {
                "dependency-load-not-analyzed"
            }
            "export_statement" if children(node).iter().any(|n| n.kind() == "export_clause") => {
                "binding-only"
            }
            _ => "possible-unassessed",
        };
        symbols.top_level.push(
            json!({"order":order,"astKind":node.kind(),"span":symbols.span(node),
            "source":symbols.snippet(node),"sideEffectStatus":status,"evaluated":false}),
        );
        if !matches!(
            status,
            "none-comment" | "function-body-not-analyzed" | "binding-only"
        ) {
            symbols.side_effects.push(
                json!({"order":order,"astKind":node.kind(),"span":symbols.span(node),
                "status":status,"evaluated":false}),
            );
        }
        if !syntax_valid {
            continue;
        }
        match node.kind() {
            "comment" => {}
            "import_statement" => symbols.import(node, order)?,
            "export_statement" => symbols.export(node, order)?,
            "function_declaration"
            | "generator_function_declaration"
            | "lexical_declaration"
            | "variable_declaration" => {
                symbols.declare(node, node, order)?;
            }
            _ => symbols.issue(
                node,
                "UNSUPPORTED_MODULE_STATEMENT",
                "top-level statement retained as side-effect evidence, not interpreted",
            ),
        }
    }
    symbols.validate_references();
    symbols.budget()?;
    symbols.blockers.sort_by_key(|value| {
        (
            value["span"]["startByte"].as_u64().unwrap_or(0),
            value["code"].as_str().unwrap_or("").to_owned(),
        )
    });
    Ok(
        json!({"format":"resume-one-module-symbols-v1","file":path,"syntaxValid":syntax_valid,
        "symbolsResolved":syntax_valid && symbols.blockers.is_empty(),"resolved":false,
        "moduleExecutable":false,"moduleCompiled":false,"purity":"not-analyzed",
        "imports":symbols.imports,"exports":symbols.exports,"declarations":symbols.declarations,
        "topLevel":symbols.top_level,"sideEffects":symbols.side_effects,"blockers":symbols.blockers,
        "scope":"direct ESM symbol extraction only; external targets, initializer values and function bodies are not compiled"}),
    )
}

fn children(node: Node<'_>) -> Vec<Node<'_>> {
    (0..node.named_child_count())
        .filter_map(|index| node.named_child(index))
        .collect()
}

fn parts(node: Node<'_>) -> Vec<Node<'_>> {
    children(node)
        .into_iter()
        .filter(|node| node.kind() != "comment")
        .collect()
}

fn token(node: Node<'_>, kind: &str) -> bool {
    (0..node.child_count())
        .filter_map(|index| node.child(index))
        .any(|child| child.kind() == kind)
}

fn unparen(mut node: Node<'_>) -> Node<'_> {
    while node.kind() == "parenthesized_expression" {
        let values = parts(node);
        if values.len() != 1 {
            break;
        }
        node = values[0];
    }
    node
}

fn is_function(node: Node<'_>) -> bool {
    matches!(
        node.kind(),
        "function_declaration"
            | "function_expression"
            | "arrow_function"
            | "generator_function_declaration"
            | "generator_function"
    )
}

impl Symbols<'_> {
    fn budget(&self) -> Result<(), String> {
        if self.started.elapsed() > Duration::from_secs(3) {
            return Err("MODULE_SYMBOL_TIME_LIMIT: symbol extraction exceeds 3 seconds".into());
        }
        if self.imports.len()
            + self.exports.len()
            + self.declarations.len()
            + self.top_level.len()
            + self.side_effects.len()
            + self.blockers.len()
            > MAX_RECORDS
        {
            return Err(
                "MODULE_SYMBOL_RECORD_LIMIT: more than 8192 records; no complete result".into(),
            );
        }
        Ok(())
    }

    fn raw(&self, node: Node<'_>) -> &str {
        &self.source[node.byte_range()]
    }

    fn span(&self, node: Node<'_>) -> Value {
        json!({"file":self.path,"startByte":node.start_byte(),"endByte":node.end_byte(),
            "line":node.start_position().row+1,"column":node.start_position().column+1,
            "endLine":node.end_position().row+1,"endColumn":node.end_position().column+1,"columnUnit":"utf8-bytes"})
    }

    fn snippet(&self, node: Node<'_>) -> Value {
        let raw = self.raw(node);
        let mut end = raw.len().min(512);
        while !raw.is_char_boundary(end) {
            end -= 1;
        }
        json!({"text":&raw[..end],"truncated":end<raw.len()})
    }

    fn issue(&mut self, node: Node<'_>, code: &str, message: &str) {
        self.blockers.push(json!({"code":code,"message":message,"astKind":node.kind(),"span":self.span(node),"source":self.snippet(node)}));
    }

    fn name(&mut self, node: Node<'_>, allow_default: bool) -> Option<String> {
        if !(node.kind() == "identifier" || allow_default && node.kind() == "default") {
            self.issue(node,"UNSUPPORTED_SYMBOL_NAME","only identifier names and explicit default tokens are supported; string/computed specifiers are not folded");
            return None;
        }
        if self.raw(node).contains('\\') {
            self.issue(
                node,
                "ESCAPED_SYMBOL_NAME",
                "escaped identifiers/specifiers are not decoded or assumed equivalent",
            );
            return None;
        }
        Some(self.raw(node).to_owned())
    }

    fn module(&mut self, node: Node<'_>) -> Option<String> {
        let raw = self.raw(node);
        let inner = raw.get(1..raw.len().saturating_sub(1));
        if node.kind() != "string"
            || inner.is_none_or(|text| text.contains('\\') || text.chars().any(char::is_control))
        {
            self.issue(
                node,
                "ESCAPED_MODULE_SPECIFIER",
                "module specifier must be an unescaped static string",
            );
            None
        } else {
            inner.map(str::to_owned)
        }
    }

    fn register(&mut self, name: &str, node: Node<'_>, initialized_at: usize) {
        if self.locals.contains_key(name) {
            self.collisions.insert(name.to_owned());
            self.issue(node,"DUPLICATE_MODULE_LOCAL","import/declaration names collide in the module namespace; records are not overwritten");
        } else {
            self.locals
                .insert(name.to_owned(), Local { initialized_at });
        }
    }

    fn import(&mut self, node: Node<'_>, order: usize) -> Result<(), String> {
        let Some(source) = node.child_by_field_name("source") else {
            self.issue(
                node,
                "UNSUPPORTED_IMPORT",
                "import has no static module specifier",
            );
            return Ok(());
        };
        let module = self.module(source);
        let clauses = parts(node)
            .into_iter()
            .filter(|n| n.kind() == "import_clause")
            .collect::<Vec<_>>();
        let mut count = 0;
        for clause in clauses {
            for child in parts(clause) {
                let list = if child.kind() == "named_imports" {
                    parts(child)
                } else {
                    vec![child]
                };
                for specifier in list {
                    self.budget()?;
                    let (local_node, imported, kind, supported) = match specifier.kind() {
                        "identifier" => {
                            (Some(specifier), Some("default".to_owned()), "default", true)
                        }
                        "import_specifier" => {
                            let name = specifier.child_by_field_name("name");
                            (
                                specifier.child_by_field_name("alias").or(name),
                                name.and_then(|n| self.name(n, true)),
                                "named",
                                true,
                            )
                        }
                        "namespace_import" => {
                            self.issue(
                                specifier,
                                "UNSUPPORTED_NAMESPACE_IMPORT",
                                "namespace imports require enumeration and are not resolved",
                            );
                            (
                                parts(specifier).first().copied(),
                                Some("*".to_owned()),
                                "namespace",
                                false,
                            )
                        }
                        _ => {
                            self.issue(
                                specifier,
                                "UNSUPPORTED_IMPORT",
                                "unknown import binding form",
                            );
                            continue;
                        }
                    };
                    let local = local_node.and_then(|name| self.name(name, false));
                    if let (Some(local), Some(local_node)) = (&local, local_node) {
                        self.register(local, local_node, 0);
                    }
                    self.imports.push(json!({"local":local,"module":module,"imported":imported,"kind":kind,
                        "supported":supported&&module.is_some()&&local.is_some()&&imported.is_some(),"order":order,"span":self.span(specifier)}));
                    count += 1;
                }
            }
        }
        if count == 0 {
            self.imports.push(
                json!({"local":null,"module":module,"imported":null,"kind":"side-effect",
                "supported":module.is_some(),"order":order,"span":self.span(node)}),
            );
            self.issue(
                node,
                "SIDE_EFFECT_IMPORT",
                "dependency initialization is retained as evidence, not proved side-effect free",
            );
        }
        for child in parts(node)
            .into_iter()
            .filter(|n| n.kind() == "import_attribute")
        {
            self.issue(
                child,
                "UNSUPPORTED_IMPORT_ATTRIBUTE",
                "import attributes/assertions are not resolved",
            );
        }
        Ok(())
    }

    fn initializer(&mut self, node: Node<'_>, order: usize) -> Value {
        let value = unparen(node);
        let (form, status, reference) = if is_function(value) {
            ("function", "not-compiled", None)
        } else if value.kind() == "identifier" {
            let name = self.name(value, false);
            if let Some(name) = &name {
                self.references.push(Reference {
                    name: name.clone(),
                    span: self.span(value),
                    at: value.start_byte(),
                    snapshot: true,
                    export: None,
                });
            }
            ("identifier-reference", "not-resolved", name)
        } else if matches!(
            value.kind(),
            "string" | "number" | "true" | "false" | "null"
        ) {
            ("literal", "not-evaluated", None)
        } else {
            self.issue(
                value,
                "UNCOMPILED_INITIALIZER",
                "unknown/dynamic initializer retained by span; no execution or alias inference",
            );
            ("uncompiled-expression", "blocked", None)
        };
        json!({"form":form,"status":status,"referencedLocal":reference,"span":self.span(node),
            "astKind":value.kind(),"order":order,"source":self.snippet(node)})
    }

    fn declare(
        &mut self,
        node: Node<'_>,
        statement: Node<'_>,
        order: usize,
    ) -> Result<Vec<String>, String> {
        if is_function(node) {
            let Some(name_node) = node.child_by_field_name("name") else {
                self.issue(
                    node,
                    "UNNAMED_DECLARATION",
                    "only default-exported function values may receive a synthetic identity",
                );
                return Ok(Vec::new());
            };
            let Some(name) = self.name(name_node, false) else {
                return Ok(Vec::new());
            };
            self.register(&name, name_node, 0);
            let initializer = self.initializer(node, order);
            self.declarations.push(json!({"name":name,"id":format!("symbol:{}:{name}",name_node.start_byte()),"kind":"function",
                "mutable":true,"synthetic":false,"initializer":initializer,"span":self.span(node),"nameSpan":self.span(name_node),
                "statementSpan":self.span(statement),"order":order}));
            return Ok(vec![name]);
        }
        if !matches!(node.kind(), "lexical_declaration" | "variable_declaration") {
            self.issue(
                node,
                "UNSUPPORTED_DECLARATION",
                "only direct function and named lexical declarations are extracted",
            );
            return Ok(Vec::new());
        }
        let kind = if node.kind() == "variable_declaration" {
            "var"
        } else if token(node, "const") {
            "const"
        } else {
            "let"
        };
        let mut names = Vec::new();
        for item in parts(node) {
            self.budget()?;
            if item.kind() != "variable_declarator" {
                self.issue(
                    item,
                    "UNSUPPORTED_DECLARATION",
                    "unknown lexical declaration child",
                );
                continue;
            }
            let Some(name_node) = item.child_by_field_name("name") else {
                self.issue(item, "UNSUPPORTED_DECLARATION", "declarator has no name");
                continue;
            };
            if name_node.kind() != "identifier" {
                self.issue(
                    name_node,
                    "UNSUPPORTED_DESTRUCTURING",
                    "destructured module bindings are not enumerated or inferred",
                );
                continue;
            }
            let Some(name) = self.name(name_node, false) else {
                continue;
            };
            let value = item.child_by_field_name("value");
            self.register(
                &name,
                name_node,
                value.map_or(item.end_byte(), |node| node.end_byte()),
            );
            let initializer=value.map(|node|self.initializer(node,order)).unwrap_or_else(||json!({"form":"none","status":"not-resolved","referencedLocal":null,"span":null}));
            if kind != "const" {
                self.issue(
                    item,
                    "MUTABLE_MODULE_BINDING",
                    "mutable binding value cannot be assumed stable by a static component linker",
                );
            }
            if kind == "const" && value.is_none() {
                self.issue(
                    item,
                    "MISSING_CONST_INITIALIZER",
                    "const requires an explicit initializer",
                );
            }
            self.declarations.push(json!({"name":name,"id":format!("symbol:{}:{name}",name_node.start_byte()),"kind":kind,
                "mutable":kind!="const","synthetic":false,"initializer":initializer,"span":self.span(item),"nameSpan":self.span(name_node),
                "statementSpan":self.span(statement),"order":order}));
            names.push(name);
        }
        Ok(names)
    }

    fn add_export(
        &mut self,
        node: Node<'_>,
        exported: Option<String>,
        target: ExportTarget,
        mut supported: bool,
        order: usize,
    ) {
        let (kind, local, module, imported, mode) = match target {
            ExportTarget::Local { local, snapshot } => (
                "local",
                local,
                None,
                None,
                if snapshot { "snapshot" } else { "live" },
            ),
            ExportTarget::Reexport { module, imported } => {
                ("reexport", None, module, imported, "live")
            }
            ExportTarget::Unresolved { module, imported } => {
                ("unresolved", None, module, imported, "unresolved")
            }
        };
        if let Some(name) = &exported
            && name != "*"
            && !self.exported.insert(name.clone())
        {
            supported = false;
            for previous in &mut self.exports {
                if previous["exported"] == *name {
                    previous["supported"] = json!(false);
                }
            }
            self.issue(
                node,
                "DUPLICATE_MODULE_EXPORT",
                "exported public name appears more than once, including default",
            );
        }
        let index = self.exports.len();
        if kind == "local"
            && let Some(local) = &local
        {
            self.references.push(Reference {
                name: local.clone(),
                span: self.span(node),
                at: node.start_byte(),
                snapshot: mode == "snapshot",
                export: Some(index),
            });
        }
        self.exports.push(json!({"exported":exported,"kind":kind,"local":local,"module":module,"imported":imported,
            "bindingMode":mode,"supported":supported,"order":order,"span":self.span(node)}));
    }

    fn export(&mut self, node: Node<'_>, order: usize) -> Result<(), String> {
        let module_node = node.child_by_field_name("source");
        let module = module_node.and_then(|source| self.module(source));
        if token(node, "default") {
            let Some(value) = node
                .child_by_field_name("declaration")
                .or_else(|| node.child_by_field_name("value"))
            else {
                self.issue(
                    node,
                    "UNSUPPORTED_DEFAULT_EXPORT",
                    "default export has no value/declaration",
                );
                return Ok(());
            };
            let value = unparen(value);
            if matches!(
                value.kind(),
                "function_declaration" | "generator_function_declaration"
            ) {
                let names = self.declare(value, node, order)?;
                self.add_export(
                    node,
                    Some("default".into()),
                    ExportTarget::Local {
                        local: names.first().cloned(),
                        snapshot: false,
                    },
                    names.len() == 1,
                    order,
                );
            } else if is_function(value) {
                let name = format!("@default:function:{}", value.start_byte());
                self.register(&name, value, 0);
                let initializer = self.initializer(value, order);
                self.declarations.push(json!({"name":name,"id":format!("symbol:{}:{name}",value.start_byte()),"kind":"function",
                    "mutable":false,"synthetic":true,"initializer":initializer,"span":self.span(value),"nameSpan":null,
                    "statementSpan":self.span(node),"order":order,
                    "internalName":value.child_by_field_name("name").map(|n|self.raw(n))}));
                self.add_export(
                    node,
                    Some("default".into()),
                    ExportTarget::Local {
                        local: Some(name),
                        snapshot: true,
                    },
                    true,
                    order,
                );
            } else if value.kind() == "identifier" {
                let local = self.name(value, false);
                let supported = local.is_some();
                self.add_export(
                    node,
                    Some("default".into()),
                    ExportTarget::Local {
                        local,
                        snapshot: true,
                    },
                    supported,
                    order,
                );
            } else {
                self.issue(value,"UNSUPPORTED_DEFAULT_EXPRESSION","default expression is neither a lexical reference nor a directly declared function");
                self.add_export(
                    node,
                    Some("default".into()),
                    ExportTarget::Unresolved {
                        module: None,
                        imported: None,
                    },
                    false,
                    order,
                );
            }
            return Ok(());
        }
        if let Some(declaration) = node.child_by_field_name("declaration") {
            for name in self.declare(declaration, node, order)? {
                self.add_export(
                    node,
                    Some(name.clone()),
                    ExportTarget::Local {
                        local: Some(name),
                        snapshot: false,
                    },
                    true,
                    order,
                );
            }
            return Ok(());
        }
        if let Some(clause) = parts(node)
            .into_iter()
            .find(|part| part.kind() == "export_clause")
        {
            for specifier in parts(clause) {
                self.budget()?;
                let Some(name_node) = specifier.child_by_field_name("name") else {
                    self.issue(
                        specifier,
                        "UNSUPPORTED_EXPORT",
                        "export specifier lacks a name",
                    );
                    continue;
                };
                let imported = self.name(name_node, module_node.is_some());
                let alias = specifier.child_by_field_name("alias").unwrap_or(name_node);
                let exported = self.name(alias, true);
                let supported = exported.is_some()
                    && imported.is_some()
                    && (module_node.is_none() || module.is_some());
                if module_node.is_some() {
                    self.add_export(
                        specifier,
                        exported,
                        ExportTarget::Reexport {
                            module: module.clone(),
                            imported,
                        },
                        supported,
                        order,
                    );
                } else {
                    self.add_export(
                        specifier,
                        exported,
                        ExportTarget::Local {
                            local: imported,
                            snapshot: false,
                        },
                        supported,
                        order,
                    );
                }
            }
            return Ok(());
        }
        if token(node, "*")
            || parts(node)
                .iter()
                .any(|node| node.kind() == "namespace_export")
        {
            self.issue(
                node,
                "UNSUPPORTED_STAR_EXPORT",
                "star/namespace reexports require explicit external enumeration and remain blocked",
            );
            let namespace = parts(node)
                .into_iter()
                .find(|node| node.kind() == "namespace_export");
            let exported = namespace
                .and_then(|namespace| parts(namespace).first().copied())
                .and_then(|name| self.name(name, true))
                .or_else(|| Some("*".into()));
            self.add_export(
                node,
                exported,
                ExportTarget::Unresolved {
                    module,
                    imported: Some("*".into()),
                },
                false,
                order,
            );
        } else {
            self.issue(
                node,
                "UNSUPPORTED_EXPORT",
                "unknown export form; no symbol mapping inferred",
            );
        }
        Ok(())
    }

    fn validate_references(&mut self) {
        for reference in &self.references {
            let problem = if self.collisions.contains(&reference.name) {
                Some((
                    "AMBIGUOUS_LOCAL_REFERENCE",
                    "reference has conflicting module bindings",
                ))
            } else if let Some(local) = self.locals.get(&reference.name) {
                (reference.snapshot&&local.initialized_at>reference.at).then_some(("EARLY_SNAPSHOT_REFERENCE","default/const reference is evaluated before the local initializer completes; not a live alias"))
            } else {
                Some((
                    "UNRESOLVED_LOCAL_REFERENCE",
                    "export/initializer references no explicitly extracted module local",
                ))
            };
            if let Some((code, message)) = problem {
                self.blockers.push(json!({"code":code,"message":message,"local":reference.name,"span":reference.span}));
                if let Some(index) = reference.export {
                    self.exports[index]["supported"] = json!(false);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tree_sitter::Parser;

    fn parse(source: &str) -> tree_sitter::Tree {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_javascript::LANGUAGE.into())
            .unwrap();
        parser.parse(source, None).unwrap()
    }
    fn syms(source: &str) -> Value {
        let tree = parse(source);
        extract("src/index.jsx", source.as_bytes(), tree.root_node()).unwrap()
    }
    fn has(value: &Value, code: &str) -> bool {
        value["blockers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue["code"] == code)
    }
    fn exported<'a>(value: &'a Value, name: &str) -> &'a Value {
        value["exports"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["exported"] == name)
            .unwrap()
    }

    #[test]
    fn real_core_barrels_and_section_forms_keep_named_and_default_targets() {
        let core = syms(
            "export { Section, SectionTitle } from './primitives/index.jsx'; export { default as tokens, typography } from './tokens/index.js';",
        );
        assert_eq!(core["symbolsResolved"], true);
        assert_eq!(
            exported(&core, "Section")["module"],
            "./primitives/index.jsx"
        );
        let primitives = syms(
            "import React from 'react'; export { Section } from './Section.jsx'; export { default as DefaultSection } from './Section.jsx';",
        );
        assert_eq!(primitives["symbolsResolved"], true);
        assert_eq!(
            exported(&primitives, "DefaultSection")["imported"],
            "default"
        );
        // The actual Section source has a props-dependent styled initializer.
        let section = syms(
            r#"import React from 'react'; import styled from 'styled-components';
const StyledSection = styled.section`margin-bottom: ${(props) => props.theme?.spacing?.section || 'var(--resume-space-section, 2rem)'};`;
export function Section({ children, className, id, ...rest }) { return <StyledSection id={id} className={`resume-section ${className || ''}`.trim()} {...rest}>{children}</StyledSection>; }
export default Section;"#,
        );
        assert_eq!(exported(&section, "Section")["local"], "Section");
        assert_eq!(exported(&section, "default")["local"], "Section");
        assert_eq!(exported(&section, "default")["bindingMode"], "snapshot");
        assert!(has(&section, "UNCOMPILED_INITIALIZER"));
        assert_eq!(section["symbolsResolved"], false);
    }

    #[test]
    fn import_aliases_and_reexports_are_exact_not_transitively_folded() {
        let value = syms(
            "import Base, { Section as S, default as Other } from './x.jsx'; export { S as Public }; export { S as T, default as X } from './x.jsx'; export default Base;",
        );
        assert_eq!(value["symbolsResolved"], true, "{:#}", value["blockers"]);
        assert_eq!(value["imports"][0]["imported"], "default");
        assert_eq!(value["imports"][1]["local"], "S");
        assert_eq!(exported(&value, "Public")["local"], "S");
        assert_eq!(exported(&value, "T")["imported"], "S");
        assert_eq!(exported(&value, "X")["imported"], "default");
        assert_eq!(exported(&value, "default")["local"], "Base");
    }

    #[test]
    fn const_declarations_have_separate_identity_and_alias_evidence() {
        let value = syms(
            "function Section(){} export const A=Section, B=() => <p/>; export { A as Public, A as default };",
        );
        assert_eq!(value["symbolsResolved"], true);
        let declarations = value["declarations"].as_array().unwrap();
        assert_eq!(declarations.len(), 3);
        assert_ne!(declarations[0]["id"], declarations[1]["id"]);
        assert_eq!(declarations[1]["kind"], "const");
        assert_eq!(declarations[1]["mutable"], false);
        assert_eq!(declarations[1]["initializer"]["referencedLocal"], "Section");
        assert_eq!(declarations[1]["initializer"]["status"], "not-resolved");
        assert_eq!(exported(&value, "Public")["local"], "A");
    }

    #[test]
    fn anonymous_default_function_is_synthetic_but_named_expression_does_not_leak() {
        for source in [
            "export default function(){return <p/>;}",
            "export default () => <p/>;",
            "export default (function Inner(){return Inner;});",
        ] {
            let value = syms(source);
            assert_eq!(value["symbolsResolved"], true, "{:#}", value["blockers"]);
            let declaration = &value["declarations"][0];
            assert_eq!(declaration["synthetic"], true);
            assert_eq!(exported(&value, "default")["local"], declaration["name"]);
            assert!(
                declaration["name"]
                    .as_str()
                    .unwrap()
                    .starts_with("@default:function:")
            );
        }
        let value = syms("export default (function Inner(){}); export { Inner };");
        assert!(has(&value, "UNRESOLVED_LOCAL_REFERENCE"));
    }

    #[test]
    fn nested_shadowing_and_fake_source_are_not_module_symbols() {
        let value = syms(
            "// export { Fake };\nconst text = 'import Fake from bogus'; export function Section(){ const Section=1; function Hidden(){} return Section; } export default Section;",
        );
        assert_eq!(value["symbolsResolved"], true);
        assert_eq!(value["imports"].as_array().unwrap().len(), 0);
        assert_eq!(value["declarations"].as_array().unwrap().len(), 2);
        assert!(!value["exports"].to_string().contains("Fake"));
        assert!(!has(&value, "DUPLICATE_MODULE_LOCAL"));
    }

    #[test]
    fn conflicting_locals_and_duplicate_public_names_remain_blocked() {
        let value = syms(
            "import S from './x'; const S=1; export { S, S as Public }; export { S as Public }; export default S; export default S;",
        );
        assert!(has(&value, "DUPLICATE_MODULE_LOCAL"));
        assert!(has(&value, "DUPLICATE_MODULE_EXPORT"));
        assert!(has(&value, "AMBIGUOUS_LOCAL_REFERENCE"));
        assert_eq!(value["symbolsResolved"], false);
        assert_eq!(value["imports"].as_array().unwrap().len(), 1);
        assert_eq!(value["declarations"].as_array().unwrap().len(), 1);
        assert_eq!(value["exports"].as_array().unwrap().len(), 5);
    }

    #[test]
    fn unsupported_namespace_star_destructuring_and_escaped_specifiers_are_explicit() {
        let value = syms(
            r#"import * as ns from './x'; import { 'Section' as S } from './x'; import X from './\u0078'; const {a}=ns; export * from './x'; export * as All from './x'; export { S as '\u0050ublic' };"#,
        );
        for code in [
            "UNSUPPORTED_NAMESPACE_IMPORT",
            "UNSUPPORTED_SYMBOL_NAME",
            "ESCAPED_MODULE_SPECIFIER",
            "UNSUPPORTED_DESTRUCTURING",
            "UNSUPPORTED_STAR_EXPORT",
        ] {
            assert!(has(&value, code), "missing {code}: {:#}", value["blockers"]);
        }
        assert_eq!(value["symbolsResolved"], false);
        assert_eq!(value["resolved"], false);
    }

    #[test]
    fn unknown_values_and_side_effect_order_are_never_execution_or_purity_proofs() {
        let value = syms(
            "import './setup.js'; const Component=makeComponent(); run(); export default choose(Component); export { Component };",
        );
        assert!(has(&value, "UNCOMPILED_INITIALIZER"));
        assert!(has(&value, "UNSUPPORTED_MODULE_STATEMENT"));
        assert!(has(&value, "UNSUPPORTED_DEFAULT_EXPRESSION"));
        assert_eq!(value["symbolsResolved"], false);
        assert_eq!(exported(&value, "Component")["local"], "Component");
        assert_eq!(exported(&value, "default")["kind"], "unresolved");
        assert_eq!(value["topLevel"].as_array().unwrap().len(), 5);
        assert!(!value["sideEffects"].as_array().unwrap().is_empty());
        assert_eq!(value["purity"], "not-analyzed");
        assert_eq!(value["moduleExecutable"], false);
        assert_eq!(value["moduleCompiled"], false);
    }

    #[test]
    fn snapshots_distinguish_early_capture_from_live_exports_and_const_mutability() {
        let value = syms(
            "export default Later; export { Later as Live }; const Later=()=>null; let Mutable=1; export { Mutable };",
        );
        assert!(has(&value, "EARLY_SNAPSHOT_REFERENCE"));
        assert_eq!(exported(&value, "default")["supported"], false);
        assert_eq!(exported(&value, "Live")["bindingMode"], "live");
        assert_eq!(exported(&value, "Live")["supported"], true);
        assert!(has(&value, "MUTABLE_MODULE_BINDING"));
        assert_eq!(value["declarations"][1]["mutable"], true);
        let value = syms("const Self=Self; export { Self }; export { Missing };");
        assert!(has(&value, "EARLY_SNAPSHOT_REFERENCE"));
        assert!(has(&value, "UNRESOLVED_LOCAL_REFERENCE"));
    }

    #[test]
    fn spans_address_original_utf8_and_broken_or_partial_csts_are_rejected() {
        for source in [
            "  export const A=1;",
            "\n\texport const A=1;\n",
            "\u{feff}export const A=1;",
        ] {
            let value = syms(source);
            assert_eq!(value["syntaxValid"], true, "{value:#}");
            assert_eq!(value["declarations"][0]["name"], "A");
        }
        let source = "// résumé\nexport const café = 1;";
        let value = syms(source);
        let span = &value["declarations"][0]["nameSpan"];
        let start = span["startByte"].as_u64().unwrap() as usize;
        let end = span["endByte"].as_u64().unwrap() as usize;
        assert_eq!(&source[start..end], "café");
        assert_eq!(span["line"], 2);
        assert_eq!(span["columnUnit"], "utf8-bytes");
        let value = syms("export const = ;");
        assert_eq!(value["syntaxValid"], false);
        assert_eq!(value["symbolsResolved"], false);
        let value = syms("export function Bad(){return <div></p>;}");
        assert!(has(&value, "MODULE_SYMBOL_JSX_TAG"));
        let tree = parse("const A=1;");
        assert!(
            extract(
                "x.js",
                b"const A=1;",
                tree.root_node().named_child(0).unwrap()
            )
            .unwrap_err()
            .starts_with("MODULE_SYMBOL_CST")
        );
        assert!(
            extract("x.js", &[0xff], tree.root_node())
                .unwrap_err()
                .starts_with("MODULE_SYMBOL_UTF8")
        );
    }

    #[test]
    fn input_depth_node_and_record_budgets_fail_without_a_complete_report() {
        let tree = parse("");
        assert!(
            extract("x.js", &vec![b' '; MAX_BYTES + 1], tree.root_node())
                .unwrap_err()
                .starts_with("MODULE_SYMBOL_INPUT_LIMIT")
        );
        let deep = format!(
            "const A={}1{};",
            "(".repeat(MAX_DEPTH + 1),
            ")".repeat(MAX_DEPTH + 1)
        );
        let tree = parse(&deep);
        assert!(
            extract("x.js", deep.as_bytes(), tree.root_node())
                .unwrap_err()
                .starts_with("MODULE_SYMBOL_AST_LIMIT")
        );
        let records = format!(
            "const {};",
            (0..MAX_RECORDS + 1)
                .map(|i| format!("A{i}=1"))
                .collect::<Vec<_>>()
                .join(",")
        );
        let tree = parse(&records);
        assert!(
            extract("x.js", records.as_bytes(), tree.root_node())
                .unwrap_err()
                .starts_with("MODULE_SYMBOL_RECORD_LIMIT")
        );
        let nodes = "const A=1;".repeat(20_000);
        let tree = parse(&nodes);
        assert!(
            extract("x.js", nodes.as_bytes(), tree.root_node())
                .unwrap_err()
                .starts_with("MODULE_SYMBOL_AST_LIMIT")
        );
    }
}
