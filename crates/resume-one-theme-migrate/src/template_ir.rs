//! A bounded, non-executing JSX template front end, not a theme or JS runtime.
//!
//! Only a default-exported function's actual return is an entry point. Import
//! and lexical declaration evidence is retained separately from template nodes.
//! Bindings have lexical identities; defaults mean undefined-only fallback,
//! optional access and short-circuit operators are not rewritten to empty text.
//! Input values must be JSON data, not objects with getters/prototype overrides.
//! Styled/external components remain unresolved dependencies for a later stage.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tree_sitter::Node;

const MAX_SOURCE_BYTES: usize = 1_048_576;
const MAX_AST_NODES: usize = 50_000;
const MAX_AST_DEPTH: usize = 96;
const MAX_RECORDS: usize = 8_192;
const ANALYSIS_BUDGET: Duration = Duration::from_secs(2);

type ResultValue = Result<Value, String>;

#[derive(Clone)]
enum SymbolKind {
    Data,
    Function,
    Import { module: String, exported: String },
    Styled,
    Invalid,
}

#[derive(Clone)]
struct Symbol {
    id: String,
    kind: SymbolKind,
    ready: bool,
}

struct Compiler<'a> {
    path: &'a str,
    source: &'a str,
    line_starts: Vec<usize>,
    scopes: Vec<BTreeMap<String, Symbol>>,
    scope_ids: Vec<String>,
    bindings: Vec<Value>,
    dependencies: Vec<Value>,
    blockers: Vec<Value>,
    issues: BTreeSet<(usize, usize, &'static str)>,
    started: Instant,
    steps: usize,
}

/// Compile pure data only. Resource/encoding failures are Err, unsupported
/// language/runtime features are structured blockers with resolved=false.
pub(crate) fn compile(path: &str, source: &[u8]) -> ResultValue {
    if source.len() > MAX_SOURCE_BYTES {
        return Err("TEMPLATE_SOURCE_LIMIT: source exceeds 1 MiB".into());
    }
    let source_text =
        std::str::from_utf8(source).map_err(|_| "TEMPLATE_UTF8: source is not UTF-8".to_owned())?;
    let (tree, compatibility) = crate::syntax::parse_javascript(path, source)
        .map_err(|error| format!("{}: {}", error.code, error.message))?;
    let mut compiler = Compiler {
        path,
        source: source_text,
        line_starts: std::iter::once(0)
            .chain(
                source
                    .iter()
                    .enumerate()
                    .filter_map(|(i, b)| (*b == b'\n').then_some(i + 1)),
            )
            .collect(),
        scopes: vec![BTreeMap::new()],
        scope_ids: vec!["module".into()],
        bindings: Vec::new(),
        dependencies: Vec::new(),
        blockers: Vec::new(),
        issues: BTreeSet::new(),
        started: Instant::now(),
        steps: 0,
    };
    let all = compiler.preflight(tree.root_node())?;
    for node in all.iter().filter(|node| node.kind() == "jsx_element") {
        if let (Some(open), Some(close)) = (
            node.child_by_field_name("open_tag"),
            node.child_by_field_name("close_tag"),
        ) {
            let opening = open
                .child_by_field_name("name")
                .map(|name| compiler.raw(name));
            let closing = close
                .child_by_field_name("name")
                .map(|name| compiler.raw(name));
            if opening != closing {
                compiler.issue(
                    close,
                    "MISMATCHED_JSX_TAG",
                    "opening and closing component names differ",
                )?;
            }
        }
    }
    let invalid = all.iter().any(|node| node.is_error() || node.is_missing());
    let entry = if invalid {
        for node in all
            .iter()
            .filter(|node| node.is_error() || node.is_missing())
        {
            compiler.issue(
                *node,
                "TEMPLATE_SYNTAX",
                "invalid or missing AST node; no runtime entry inferred",
            )?;
        }
        for node in children(tree.root_node()) {
            compiler
                .dependencies
                .push(compiler.evidence(node, "unparsed-top-level"));
        }
        Value::Null
    } else {
        compiler.module(tree.root_node())?
    };
    compiler.check_budget()?;
    compiler.blockers.sort_by_key(|issue| {
        (
            issue["span"]["startByte"].as_u64().unwrap_or(0),
            issue["span"]["endByte"].as_u64().unwrap_or(0),
            issue["code"].as_str().unwrap_or("").to_owned(),
        )
    });
    Ok(json!({
        "format": "resume-one-template-ir-v1",
        "source": {"path":path,"bytes":source.len()},
        "syntaxValid": !invalid && !compiler.blockers.iter().any(|issue|issue["code"]=="MISMATCHED_JSX_TAG"),
        "resolved": compiler.blockers.is_empty(),
        "entry": entry,
        "bindings": compiler.bindings,
        "dependencies": compiler.dependencies,
        "blockers": compiler.blockers,
        "parserCompatibility": compatibility,
        "evaluationContract": {
            "input":"JSON data only; no getters, proxies or overridden prototypes",
            "missing":"undefined is distinct from null; never substitute an empty string",
            "defaults":"fallback only when undefined, not for null/false/zero/empty string",
            "operators":"JavaScript short-circuit and strict comparison semantics",
            "optionalChain":"get/map/join chainContinuation propagates a nullish short-circuit sentinel, not ordinary undefined; group and to-string terminate the chain",
            "map":"dense JSON arrays only; non-array receivers are errors; preserve optional access/call",
            "textTemplate":"untagged templates concatenate cooked literal fragments and explicit to-string IR; raw fragments retained, no JSX whitespace folding",
            "toString":"ECMAScript scalar ToString, not JSON serialization: string unchanged; finite number in ECMAScript radix-10 notation (-0 becomes 0); boolean true/false; null becomes null text; undefined becomes undefined text; arrays/objects rejected, never coerced via hooks",
            "join":"intrinsic dense JSON-array join only; static string separator (default comma); null/undefined elements become empty text by join semantics, other scalar elements use the same to-string contract; nested arrays/objects rejected; bad/missing receivers are errors unless the original optional chain short-circuits",
            "text":"JSX whitespace normalization after adjacent text/entity coalescing",
            "execution":"none; no JS, imports, CSS, URL or callback is executed",
            "scope":"template IR only, not a full compiler or visual acceptance"
        }
    }))
}

fn children(node: Node<'_>) -> Vec<Node<'_>> {
    (0..node.named_child_count())
        .filter_map(|index| node.named_child(index))
        .filter(|child| child.kind() != "comment")
        .collect()
}

fn field<'t>(node: Node<'t>, name: &str) -> Result<Node<'t>, String> {
    node.child_by_field_name(name)
        .ok_or_else(|| format!("TEMPLATE_AST: {} lacks {name}", node.kind()))
}

fn unparen(mut node: Node<'_>) -> Node<'_> {
    while node.kind() == "parenthesized_expression" {
        let parts = children(node);
        if parts.len() != 1 {
            break;
        }
        node = parts[0];
    }
    node
}

fn function(node: Node<'_>) -> bool {
    matches!(
        node.kind(),
        "function_declaration" | "function_expression" | "arrow_function"
    )
}

fn unsafe_key(name: &str) -> bool {
    matches!(name, "__proto__" | "prototype" | "constructor")
}

// Parentheses terminate optional-chain propagation: (x?.a).b is not x?.a.b.
fn active_chain(node: Node<'_>) -> bool {
    match node.kind() {
        "member_expression" => {
            node.child_by_field_name("optional_chain").is_some()
                || node.child_by_field_name("object").is_some_and(active_chain)
        }
        "call_expression" => {
            node.child_by_field_name("optional_chain").is_some()
                || node
                    .child_by_field_name("function")
                    .is_some_and(active_chain)
        }
        _ => false,
    }
}

impl Compiler<'_> {
    fn check_budget(&mut self) -> Result<(), String> {
        self.steps += 1;
        if self.started.elapsed() > ANALYSIS_BUDGET || self.steps > 200_000 {
            return Err(
                "TEMPLATE_ANALYSIS_LIMIT: analysis budget exceeded; no complete IR returned".into(),
            );
        }
        if self.bindings.len() + self.dependencies.len() + self.blockers.len() > MAX_RECORDS {
            return Err("TEMPLATE_RECORD_LIMIT: more than 8192 evidence/IR records".into());
        }
        Ok(())
    }

    fn preflight<'t>(&mut self, root: Node<'t>) -> Result<Vec<Node<'t>>, String> {
        let mut pending = vec![(root, 0)];
        let mut all = Vec::new();
        while let Some((node, depth)) = pending.pop() {
            self.check_budget()?;
            if depth > MAX_AST_DEPTH || all.len() >= MAX_AST_NODES {
                return Err("TEMPLATE_AST_LIMIT: AST exceeds depth 96 or 50000 nodes".into());
            }
            all.push(node);
            for index in (0..node.child_count()).rev() {
                if let Some(child) = node.child(index) {
                    pending.push((child, depth + 1));
                }
            }
        }
        Ok(all)
    }

    fn raw(&self, node: Node<'_>) -> &str {
        &self.source[node.byte_range()]
    }

    fn span(&self, node: Node<'_>) -> Value {
        json!({"file":self.path,"startByte":node.start_byte(),"endByte":node.end_byte(),
            "line":node.start_position().row+1,"column":node.start_position().column+1,
            "endLine":node.end_position().row+1,"endColumn":node.end_position().column+1,
            "columnUnit":"utf8-bytes"})
    }

    fn evidence(&self, node: Node<'_>, kind: &str) -> Value {
        json!({"kind":kind,"astKind":node.kind(),"source":self.raw(node),"span":self.span(node)})
    }

    fn range_span(&self, start: usize, end: usize) -> Value {
        let a = self
            .line_starts
            .partition_point(|p| *p <= start)
            .saturating_sub(1);
        let b = self
            .line_starts
            .partition_point(|p| *p <= end)
            .saturating_sub(1);
        json!({"file":self.path,"startByte":start,"endByte":end,"line":a+1,"column":start-self.line_starts[a]+1,
            "endLine":b+1,"endColumn":end-self.line_starts[b]+1,"columnUnit":"utf8-bytes"})
    }

    fn issue(&mut self, node: Node<'_>, code: &'static str, message: &str) -> Result<(), String> {
        self.check_budget()?;
        if self
            .issues
            .insert((node.start_byte(), node.end_byte(), code))
        {
            let raw = self.raw(node);
            let mut end = raw.len().min(256);
            while !raw.is_char_boundary(end) {
                end -= 1;
            }
            self.blockers
                .push(json!({"code":code,"message":message,"astKind":node.kind(),
                "span":self.span(node),"source":&raw[..end],"sourceTruncated":end<raw.len()}));
        }
        Ok(())
    }

    /// Diagnose the entire rejected subtree, not just its first unknown call.
    fn reject(&mut self, node: Node<'_>, code: &'static str, message: &str) -> ResultValue {
        self.issue(node, code, message)?;
        let mut pending = children(node);
        while let Some(child) = pending.pop() {
            self.check_budget()?;
            if child.named_child_count() > 0
                || matches!(child.kind(), "spread_element" | "this" | "super")
            {
                self.issue(
                    child,
                    "UNSUPPORTED_SUBTREE_NODE",
                    "node inside an unsupported construct; retained as evidence, not lowered",
                )?;
            }
            pending.extend(children(child));
        }
        Ok(json!({"kind":"blocked","astKind":node.kind(),"span":self.span(node)}))
    }

    fn lookup(&self, name: &str) -> Option<Symbol> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).cloned())
    }

    fn reserve(&mut self, node: Node<'_>, kind: SymbolKind, ready: bool) -> Result<(), String> {
        let name = self.raw(node).to_owned();
        if self.scopes.last().unwrap().contains_key(&name) {
            self.issue(
                node,
                "DUPLICATE_BINDING",
                "binding collision in the same lexical scope",
            )?;
            self.scopes.last_mut().unwrap().insert(
                name.clone(),
                Symbol {
                    id: format!("b:{}:{name}", node.start_byte()),
                    kind: SymbolKind::Invalid,
                    ready: true,
                },
            );
        } else {
            self.scopes.last_mut().unwrap().insert(
                name.clone(),
                Symbol {
                    id: format!("b:{}:{name}", node.start_byte()),
                    kind,
                    ready,
                },
            );
        }
        Ok(())
    }

    fn reserve_pattern(&mut self, pattern: Node<'_>) -> Result<(), String> {
        match pattern.kind() {
            "identifier" | "shorthand_property_identifier_pattern" => {
                self.reserve(pattern, SymbolKind::Data, false)
            }
            "object_pattern" => {
                for part in children(pattern) {
                    self.reserve_pattern(part)?;
                }
                Ok(())
            }
            "assignment_pattern" | "object_assignment_pattern" => {
                self.reserve_pattern(field(pattern, "left")?)
            }
            "pair_pattern" => self.reserve_pattern(field(pattern, "value")?),
            _ => {
                self.reject(pattern,"UNSUPPORTED_BINDING_PATTERN","only explicit scalar/object bindings are supported; no rest or computed bindings")?;
                Ok(())
            }
        }
    }

    fn define(&mut self, node: Node<'_>, value: Value, kind: SymbolKind) -> Result<(), String> {
        self.check_budget()?;
        let name = self.raw(node).to_owned();
        let id = format!("b:{}:{name}", node.start_byte());
        let existing = self.scopes.last().unwrap().get(&name).cloned();
        if existing
            .as_ref()
            .is_some_and(|symbol| symbol.id != id || symbol.ready)
        {
            self.issue(
                node,
                "DUPLICATE_BINDING",
                "cannot replace an existing lexical binding",
            )?;
            return Ok(());
        }
        self.scopes.last_mut().unwrap().insert(
            name.clone(),
            Symbol {
                id: id.clone(),
                kind,
                ready: true,
            },
        );
        self.bindings
            .push(json!({"id":id,"name":name,"value":value,"scope":self.scope_ids.last(),"declarationOrder":self.bindings.len(),"span":self.span(node)}));
        Ok(())
    }

    fn bind_pattern(&mut self, pattern: Node<'_>, value: Value) -> Result<(), String> {
        self.check_budget()?;
        match pattern.kind() {
            "identifier" | "shorthand_property_identifier_pattern" => {
                self.define(pattern, value, SymbolKind::Data)
            }
            "assignment_pattern" | "object_assignment_pattern" => {
                let fallback = self.expr(field(pattern, "right")?)?;
                self.bind_pattern(field(pattern,"left")?,json!({"kind":"default","when":"undefined","value":value,"fallback":fallback,"span":self.span(pattern)}))
            }
            "object_pattern" => {
                if children(pattern).is_empty() {
                    self.reject(pattern,"UNSUPPORTED_EMPTY_DESTRUCTURING","empty destructuring still requires a nullish coercion check; not silently discarded")?;
                }
                for part in children(pattern) {
                    let (key, target) = match part.kind() {
                        "shorthand_property_identifier_pattern" => {
                            (self.raw(part).to_owned(), part)
                        }
                        "object_assignment_pattern" => {
                            (self.raw(field(part, "left")?).to_owned(), part)
                        }
                        "pair_pattern" => {
                            let key = field(part, "key")?;
                            if key.kind() != "property_identifier" {
                                self.reject(
                                    part,
                                    "UNSUPPORTED_BINDING_KEY",
                                    "only explicit identifier property names are supported",
                                )?;
                                continue;
                            }
                            (self.raw(key).to_owned(), field(part, "value")?)
                        }
                        _ => {
                            self.reject(
                                part,
                                "UNSUPPORTED_BINDING_PATTERN",
                                "no rest/computed destructuring",
                            )?;
                            continue;
                        }
                    };
                    if unsafe_key(&key) {
                        self.reject(
                            part,
                            "UNSAFE_PROPERTY",
                            "prototype-related keys are not JSON template fields",
                        )?;
                        continue;
                    }
                    self.bind_pattern(target,json!({"kind":"get","object":value,"property":key,"optional":false,"span":self.span(part)}))?;
                }
                Ok(())
            }
            _ => {
                self.reject(
                    pattern,
                    "UNSUPPORTED_BINDING_PATTERN",
                    "cannot resolve binding scope",
                )?;
                Ok(())
            }
        }
    }

    fn string(&self, node: Node<'_>) -> Option<String> {
        let raw = self.raw(node);
        if raw.starts_with('"') {
            serde_json::from_str::<String>(raw).ok()
        } else {
            let inner = raw.strip_prefix('\'')?.strip_suffix('\'')?;
            (!inner.contains(['\\', '\n', '\r'])).then(|| inner.to_owned())
        }
    }

    fn import(&mut self, node: Node<'_>) -> Result<(), String> {
        let module_node = field(node, "source")?;
        let module = self.string(module_node);
        let mut evidence = self.evidence(node, "import");
        evidence["module"] = json!(module);
        let mut specifiers = Vec::new();
        for clause in children(node)
            .into_iter()
            .filter(|child| child.kind() == "import_clause")
        {
            for part in children(clause) {
                let parts = if part.kind() == "named_imports" {
                    children(part)
                } else {
                    vec![part]
                };
                for item in parts {
                    let pair = match item.kind() {
                        "identifier" => Some((item, "default".to_owned())),
                        "namespace_import" => children(item)
                            .first()
                            .copied()
                            .map(|name| (name, "*".to_owned())),
                        "import_specifier" => {
                            let name = field(item, "name")?;
                            Some((
                                item.child_by_field_name("alias").unwrap_or(name),
                                self.raw(name).to_owned(),
                            ))
                        }
                        _ => None,
                    };
                    if let Some((local, exported)) = pair {
                        let kind = module.as_ref().map_or(SymbolKind::Invalid, |module| {
                            SymbolKind::Import {
                                module: module.clone(),
                                exported: exported.clone(),
                            }
                        });
                        self.reserve(local, kind, true)?;
                        specifiers.push(json!({"local":self.raw(local),"exported":exported,"span":self.span(local)}));
                    } else {
                        self.reject(item, "UNSUPPORTED_IMPORT", "cannot resolve import binding")?;
                    }
                }
            }
        }
        if module.is_none() {
            self.reject(
                module_node,
                "UNSUPPORTED_IMPORT_SPECIFIER",
                "module string escapes are not supported",
            )?;
        }
        if specifiers.is_empty() {
            self.issue(
                node,
                "SIDE_EFFECT_IMPORT",
                "side-effect import semantics are not compiled or executed",
            )?;
        }
        for part in children(node)
            .into_iter()
            .filter(|child| child.kind() == "import_attribute")
        {
            self.reject(
                part,
                "UNSUPPORTED_IMPORT_ATTRIBUTE",
                "import attributes are outside this frontend",
            )?;
        }
        evidence["specifiers"] = json!(specifiers);
        self.dependencies.push(evidence);
        Ok(())
    }

    fn styled(&self, node: Node<'_>) -> bool {
        if node.kind() != "call_expression"
            || node
                .child_by_field_name("arguments")
                .is_none_or(|args| args.kind() != "template_string")
        {
            return false;
        }
        let Some(tag) = node.child_by_field_name("function") else {
            return false;
        };
        let root = if tag.kind() == "member_expression" {
            tag.child_by_field_name("object")
        } else if tag.kind() == "call_expression" {
            tag.child_by_field_name("function")
        } else {
            None
        };
        root.filter(|root|root.kind()=="identifier").and_then(|root|self.lookup(self.raw(root)))
            .is_some_and(|symbol|matches!(symbol.kind,SymbolKind::Import {module,exported} if module=="styled-components" && matches!(exported.as_str(),"default"|"styled")))
    }

    fn declarations<'t>(
        &mut self,
        declaration: Node<'t>,
        functions: &mut BTreeMap<String, Node<'t>>,
    ) -> Result<(), String> {
        for item in children(declaration) {
            if item.kind() != "variable_declarator" {
                self.reject(
                    item,
                    "UNSUPPORTED_DECLARATION",
                    "unsupported lexical declaration child",
                )?;
                continue;
            }
            let name = field(item, "name")?;
            self.reserve_pattern(name)?;
            if let Some(value) = item.child_by_field_name("value") {
                let value = unparen(value);
                if name.kind() == "identifier" && function(value) {
                    functions.insert(self.raw(name).to_owned(), value);
                }
            }
        }
        Ok(())
    }

    fn declaration_values(&mut self, node: Node<'_>, entry: Option<usize>) -> Result<(), String> {
        let is_const = (0..node.child_count())
            .filter_map(|i| node.child(i))
            .any(|child| child.kind() == "const");
        self.dependencies
            .push(self.evidence(node, "lexical-declaration"));
        for item in children(node) {
            if item.kind() != "variable_declarator" {
                continue;
            }
            let pattern = field(item, "name")?;
            let Some(raw_value) = item.child_by_field_name("value") else {
                self.reject(
                    item,
                    "MISSING_INITIALIZER",
                    "lexical bindings require an explicit initializer",
                )?;
                continue;
            };
            let value = unparen(raw_value);
            if !is_const {
                self.reject(
                    item,
                    "MUTABLE_BINDING",
                    "let/var bindings are not safely lowered by this frontend",
                )?;
                continue;
            }
            if function(value) && pattern.kind() == "identifier" {
                if entry != Some(value.start_byte()) {
                    self.reject(
                        value,
                        "UNCOMPILED_FUNCTION_DEPENDENCY",
                        "helper functions are evidence, not inlined/executed",
                    )?;
                }
                self.define(
                    pattern,
                    self.evidence(value, "function-reference"),
                    SymbolKind::Function,
                )?;
            } else if self.styled(value) && pattern.kind() == "identifier" {
                self.issue(
                    value,
                    "UNCOMPILED_STYLE_DEPENDENCY",
                    "tagged styled declaration requires separate static CSS/dependency resolution",
                )?;
                self.define(
                    pattern,
                    self.evidence(value, "styled-reference"),
                    SymbolKind::Styled,
                )?;
            } else {
                let ir = self.expr(value)?;
                self.bind_pattern(pattern, ir)?;
            }
        }
        Ok(())
    }

    fn module(&mut self, root: Node<'_>) -> ResultValue {
        let top = children(root);
        let mut functions = BTreeMap::new();
        let mut defaults = Vec::new();
        let mut declarations = Vec::new();
        for node in &top {
            self.check_budget()?;
            if node.kind() == "import_statement" {
                self.import(*node)?;
                continue;
            }
            let declaration = if node.kind() == "export_statement" {
                self.dependencies.push(self.evidence(*node, "export"));
                let default = (0..node.child_count())
                    .filter_map(|i| node.child(i))
                    .any(|child| child.kind() == "default");
                if default {
                    if let Some(value) = node
                        .child_by_field_name("value")
                        .or_else(|| node.child_by_field_name("declaration"))
                    {
                        defaults.push(unparen(value));
                        if !function(unparen(value)) && unparen(value).kind() != "identifier" {
                            self.reject(value,"UNSUPPORTED_ENTRYPOINT_EXPRESSION","default export is not a directly declared function or lexical reference; no factory/expression is evaluated")?;
                        }
                    }
                } else if !node
                    .child_by_field_name("declaration")
                    .is_some_and(|declaration| {
                        matches!(
                            declaration.kind(),
                            "function_declaration" | "lexical_declaration"
                        )
                    })
                {
                    self.reject(
                        *node,
                        "UNSUPPORTED_EXPORT",
                        "named export must declare a function or lexical binding; re-exports and export clauses need separate module linking",
                    )?;
                }
                node.child_by_field_name("declaration")
            } else {
                Some(*node)
            };
            let Some(declaration) = declaration else {
                continue;
            };
            match declaration.kind() {
                "function_declaration" => {
                    let name = field(declaration, "name")?;
                    self.reserve(name, SymbolKind::Function, true)?;
                    functions.insert(self.raw(name).to_owned(), declaration);
                    declarations.push(declaration);
                }
                "lexical_declaration" | "variable_declaration" => {
                    self.declarations(declaration, &mut functions)?;
                    declarations.push(declaration);
                }
                _ if defaults.iter().any(|entry| entry.id() == declaration.id())
                    && function(declaration) => {}
                _ => {
                    self.dependencies
                        .push(self.evidence(declaration, "unsupported-top-level"));
                    self.reject(declaration,"UNSUPPORTED_TOP_LEVEL","top-level executable statements are not executed or treated as template semantics")?;
                }
            }
        }
        let entry = if defaults.len() == 1 {
            let value = defaults[0];
            if value.kind() == "identifier" {
                match self.lookup(self.raw(value)) {
                    Some(Symbol {
                        kind: SymbolKind::Invalid,
                        ..
                    })
                    | None => None,
                    _ => functions.get(self.raw(value)).copied(),
                }
            } else if function(value) {
                Some(value)
            } else {
                None
            }
        } else {
            None
        };
        for declaration in declarations {
            if declaration.kind() == "function_declaration" {
                self.dependencies
                    .push(self.evidence(declaration, "function-declaration"));
                if entry.is_none_or(|entry| entry.id() != declaration.id()) {
                    self.reject(
                        declaration,
                        "UNCOMPILED_FUNCTION_DEPENDENCY",
                        "non-entry function retained without inferring execution",
                    )?;
                }
            } else {
                self.declaration_values(declaration, entry.map(|node| node.start_byte()))?;
            }
        }
        let Some(entry) = entry else {
            self.issue(root,"UNRESOLVED_ENTRYPOINT","requires exactly one default export bound to a directly declared function; strings/JSX fragments elsewhere are not entries")?;
            return Ok(Value::Null);
        };
        self.scopes.push(BTreeMap::new());
        let scope_id = format!("function:{}", entry.start_byte());
        self.scope_ids.push(scope_id.clone());
        if self.has_token(entry, "async") {
            self.issue(
                entry,
                "ASYNC_ENTRYPOINT",
                "async function return semantics are unsupported",
            )?;
        }
        let parameters = self.parameters(entry);
        if parameters.len() > 1 {
            self.reject(
                entry,
                "UNSUPPORTED_PARAMETERS",
                "entry accepts at most one JSON props argument",
            )?;
        }
        for parameter in parameters.iter().take(1) {
            self.reserve_pattern(*parameter)?;
            self.bind_pattern(
                *parameter,
                json!({"kind":"input","name":"props","span":self.span(*parameter)}),
            )?;
        }
        let body = self.function_body(entry)?;
        self.scopes.pop();
        self.scope_ids.pop();
        if !contains_jsx(&body) {
            self.issue(
                entry,
                "ENTRYPOINT_HAS_NO_JSX",
                "actual function return contains no supported JSX element/fragment",
            )?;
        }
        Ok(json!({"kind":"function","scope":scope_id,"span":self.span(entry),"body":body}))
    }

    fn has_token(&self, node: Node<'_>, token: &str) -> bool {
        (0..node.child_count())
            .filter_map(|i| node.child(i))
            .any(|child| child.kind() == token)
    }

    fn parameters<'t>(&self, node: Node<'t>) -> Vec<Node<'t>> {
        node.child_by_field_name("parameters")
            .map(children)
            .or_else(|| {
                node.child_by_field_name("parameter")
                    .map(|parameter| vec![parameter])
            })
            .unwrap_or_default()
    }

    fn function_body(&mut self, node: Node<'_>) -> ResultValue {
        let body = field(node, "body")?;
        if body.kind() != "statement_block" {
            return self.expr(body);
        }
        let statements = children(body);
        let mut unused = BTreeMap::new();
        for statement in &statements {
            if matches!(
                statement.kind(),
                "lexical_declaration" | "variable_declaration"
            ) {
                self.declarations(*statement, &mut unused)?;
            }
        }
        let mut returned = None;
        for statement in statements {
            if returned.is_some() {
                self.reject(
                    statement,
                    "UNREACHABLE_STATEMENT",
                    "statement after return retained as evidence, not executed",
                )?;
                continue;
            }
            match statement.kind() {
                "lexical_declaration" | "variable_declaration" => {
                    self.declaration_values(statement, None)?
                }
                "return_statement" => {
                    let parts = children(statement);
                    returned = Some(if parts.len() == 1 {
                        self.expr(parts[0])?
                    } else {
                        json!({"kind":"undefined","span":self.span(statement)})
                    });
                }
                _ => {
                    self.reject(
                        statement,
                        "UNSUPPORTED_FUNCTION_STATEMENT",
                        "only const bindings followed by a direct return can be lowered",
                    )?;
                }
            }
        }
        if let Some(value) = returned {
            Ok(value)
        } else {
            self.reject(
                body,
                "MISSING_DIRECT_RETURN",
                "cannot infer a template from nested/conditional statements",
            )
        }
    }

    fn expr(&mut self, raw_node: Node<'_>) -> ResultValue {
        self.check_budget()?;
        if raw_node.kind() == "parenthesized_expression" {
            let parts = children(raw_node);
            if parts.len() != 1 {
                return self.reject(
                    raw_node,
                    "UNSUPPORTED_GROUP",
                    "parenthesized expression must contain one value",
                );
            }
            let value = self.expr(parts[0])?;
            return Ok(
                json!({"kind":"group","value":value,"optionalChainBoundary":true,"span":self.span(raw_node)}),
            );
        }
        let node = unparen(raw_node);
        let span = self.span(node);
        match node.kind() {
            "jsx_element" | "jsx_self_closing_element" => self.jsx(node),
            "jsx_expression" => {
                let parts = children(node);
                if parts.is_empty() {
                    Ok(json!({"kind":"empty","span":span}))
                } else if parts.len() == 1 {
                    self.expr(parts[0])
                } else {
                    self.reject(
                        node,
                        "UNSUPPORTED_JSX_EXPRESSION",
                        "JSX expression has more than one value",
                    )
                }
            }
            "identifier" => {
                let name = self.raw(node);
                match self.lookup(name) {
                    Some(Symbol { ready: false, .. }) => self.reject(
                        node,
                        "UNINITIALIZED_BINDING",
                        "lexical binding is read before initialization",
                    ),
                    Some(Symbol {
                        kind: SymbolKind::Data,
                        id,
                        ..
                    }) => Ok(json!({"kind":"binding","id":id,"name":name,"span":span})),
                    Some(_) => self.reject(
                        node,
                        "NON_DATA_BINDING",
                        "import/function/component cannot be used as a JSON field value",
                    ),
                    None if name == "undefined" => Ok(json!({"kind":"undefined","span":span})),
                    None => self.reject(
                        node,
                        "UNRESOLVED_BINDING",
                        "identifier has no resolvable lexical data binding",
                    ),
                }
            }
            "string" => {
                if let Some(value) = self.string(node) {
                    Ok(json!({"kind":"literal","value":value,"span":span}))
                } else {
                    self.reject(
                        node,
                        "UNSUPPORTED_STRING_ESCAPE",
                        "string escape syntax is not in the finite literal decoder",
                    )
                }
            }
            "template_string" => self.text_template(node),
            "number" => {
                let raw = self.raw(node);
                let number = serde_json::from_str::<Value>(raw)
                    .ok()
                    .filter(Value::is_number)
                    .and_then(|_| raw.parse::<f64>().ok())
                    .and_then(serde_json::Number::from_f64);
                if let Some(value) = number {
                    Ok(json!({"kind":"literal","value":value,"span":span}))
                } else {
                    self.reject(
                        node,
                        "UNSUPPORTED_NUMBER",
                        "only finite JSON decimal number syntax is supported",
                    )
                }
            }
            "true" | "false" | "null" => Ok(
                json!({"kind":"literal","value":match node.kind(){"true"=>json!(true),"false"=>json!(false),_=>Value::Null},"span":span}),
            ),
            "object" | "array" if children(node).is_empty() && !self.has_token(node, ",") => Ok(
                json!({"kind":"literal","value":if node.kind()=="array"{json!([])}else{json!({})},"span":span}),
            ),
            "member_expression" => {
                let property = field(node, "property")?;
                let name = self.raw(property).to_owned();
                if property.kind() != "property_identifier" || unsafe_key(&name) {
                    return self.reject(
                        node,
                        "UNSAFE_PROPERTY",
                        "only explicit non-prototype field access is supported",
                    );
                }
                let object_node = field(node, "object")?;
                let continuation = active_chain(object_node);
                let object = self.expr(object_node)?;
                Ok(
                    json!({"kind":"get","object":object,"property":name,"optional":node.child_by_field_name("optional_chain").is_some(),"chainContinuation":continuation,"span":span}),
                )
            }
            "binary_expression" => {
                let operator = self.raw(field(node, "operator")?).to_owned();
                let allowed = matches!(
                    operator.as_str(),
                    "&&" | "||" | "??" | ">" | ">=" | "<" | "<=" | "===" | "!=="
                );
                if !allowed {
                    return self.reject(
                        node,
                        "UNSUPPORTED_OPERATOR",
                        "operator is outside the pure template whitelist",
                    );
                }
                let left = self.expr(field(node, "left")?)?;
                let right = self.expr(field(node, "right")?)?;
                Ok(
                    json!({"kind":if matches!(operator.as_str(),"&&"|"||"|"??"){"logical"}else{"compare"},"operator":operator,"left":left,"right":right,"span":span}),
                )
            }
            "unary_expression" if self.raw(field(node, "operator")?) == "!" => {
                let value = self.expr(field(node, "argument")?)?;
                Ok(json!({"kind":"not","value":value,"span":span}))
            }
            "ternary_expression" => {
                let condition = self.expr(field(node, "condition")?)?;
                let consequence = self.expr(field(node, "consequence")?)?;
                let alternative = self.expr(field(node, "alternative")?)?;
                Ok(
                    json!({"kind":"conditional","condition":condition,"consequence":consequence,"alternative":alternative,"span":span}),
                )
            }
            "call_expression" => {
                let callee = field(node, "function")?;
                if callee.kind() == "member_expression"
                    && self.raw(field(callee, "property")?) == "join"
                {
                    self.join(node)
                } else {
                    self.map(node)
                }
            }
            _ => self.reject(
                node,
                "UNSUPPORTED_EXPRESSION",
                "expression is outside the finite, data-only template whitelist",
            ),
        }
    }

    fn map(&mut self, node: Node<'_>) -> ResultValue {
        let function_node = field(node, "function")?;
        let args = field(node, "arguments")?;
        if function_node.kind() != "member_expression"
            || args.kind() != "arguments"
            || self.raw(field(function_node, "property")?) != "map"
        {
            return self.reject(
                node,
                "UNSUPPORTED_CALL",
                "only explicit JSON-array map/join descriptors are lowered; no calls are executed",
            );
        }
        let args = children(args);
        if args.len() != 1 || args[0].kind() != "arrow_function" || self.has_token(args[0], "async")
        {
            return self.reject(node,"UNSAFE_MAP_CALLBACK","map requires exactly one synchronous lexical arrow callback, without thisArg/spread");
        }
        let receiver = field(function_node, "object")?;
        if !matches!(unparen(receiver).kind(), "identifier" | "member_expression") {
            return self.reject(
                node,
                "UNSAFE_MAP_RECEIVER",
                "map receiver must be an explicit lexical JSON data path",
            );
        }
        let collection = self.expr(receiver)?;
        let callback = args[0];
        let parameters = self.parameters(callback);
        if parameters.is_empty()
            || parameters.len() > 2
            || parameters
                .iter()
                .any(|parameter| parameter.kind() != "identifier")
        {
            return self.reject(callback,"UNSUPPORTED_MAP_BINDING","map accepts item and optional index identifiers only, no rest/default/destructuring");
        }
        self.scopes.push(BTreeMap::new());
        let scope_id = format!("map:{}", node.start_byte());
        self.scope_ids.push(scope_id.clone());
        let mut bindings = Vec::new();
        for (index, parameter) in parameters.into_iter().enumerate() {
            self.reserve(parameter, SymbolKind::Data, false)?;
            self.define(parameter,json!({"kind":"map-parameter","mapStartByte":node.start_byte(),"role":if index==0{"item"}else{"index"},"span":self.span(parameter)}),SymbolKind::Data)?;
            bindings.push(self.lookup(self.raw(parameter)).unwrap().id);
        }
        let body = self.function_body(callback)?;
        self.scopes.pop();
        self.scope_ids.pop();
        Ok(
            json!({"kind":"map","scope":scope_id,"collection":collection,"parameters":bindings,"body":body,
            "optionalMember":function_node.child_by_field_name("optional_chain").is_some(),
            "optionalCall":node.child_by_field_name("optional_chain").is_some(),
            "chainContinuation":active_chain(function_node),
            "receiverContract":"dense-json-array; otherwise error, not an implicit empty list","span":self.span(node)}),
        )
    }

    fn text_template(&mut self, node: Node<'_>) -> ResultValue {
        let mut parts = Vec::new();
        let mut cursor = node.start_byte() + 1;
        for substitution in children(node)
            .into_iter()
            .filter(|n| n.kind() == "template_substitution")
        {
            self.template_literal(&mut parts, cursor, substitution.start_byte(), node)?;
            let expressions = children(substitution);
            if expressions.len() != 1 {
                parts.push(self.reject(
                    substitution,
                    "UNSUPPORTED_TEMPLATE_SUBSTITUTION",
                    "template substitution requires exactly one pure expression",
                )?);
            } else {
                let expression = expressions[0];
                if matches!(
                    unparen(expression).kind(),
                    "object" | "array" | "jsx_element" | "jsx_self_closing_element"
                ) {
                    parts.push(self.reject(
                        expression,
                        "UNSUPPORTED_TEXT_COERCION",
                        "composite values cannot use the finite scalar ToString operation",
                    )?);
                } else {
                    let value = self.expr(expression)?;
                    if value["kind"] == "map" {
                        self.issue(
                            expression,
                            "UNSUPPORTED_TEXT_COERCION",
                            "map results are arrays, not scalar text",
                        )?;
                    }
                    parts.push(json!({"kind":"to-string","value":value,"contract":"ecmascript-json-scalar","optionalChainBoundary":true,"span":self.span(substitution)}));
                }
            }
            cursor = substitution.end_byte();
        }
        self.template_literal(&mut parts, cursor, node.end_byte() - 1, node)?;
        Ok(
            json!({"kind":"text-template","parts":parts,"inputConstraint":"scalar substitutions only; composite values must be rejected, never serialized/coerced via hooks","span":self.span(node)}),
        )
    }

    fn template_literal(
        &mut self,
        parts: &mut Vec<Value>,
        start: usize,
        end: usize,
        parent: Node<'_>,
    ) -> Result<(), String> {
        self.check_budget()?;
        if start == end {
            return Ok(());
        }
        let raw = &self.source[start..end];
        if raw.contains(['\\', '\0']) {
            let blocked=self.reject(parent,"UNSUPPORTED_TEMPLATE_ESCAPE","cooked template escapes require an exact finite decoder; not rewritten or treated as raw")?;
            parts.push(blocked);
        } else {
            parts.push(json!({"kind":"literal","value":raw.replace("\r\n","\n").replace('\r',"\n"),"raw":raw,"span":self.range_span(start,end)}));
        }
        Ok(())
    }

    fn join(&mut self, node: Node<'_>) -> ResultValue {
        let function_node = field(node, "function")?;
        let arguments = field(node, "arguments")?;
        let args = children(arguments);
        if arguments.kind() != "arguments"
            || args.len() > 1
            || args.first().is_some_and(|n| n.kind() != "string")
        {
            return self.reject(node,"UNSUPPORTED_JOIN_SEPARATOR","join accepts no separator or one static string, not dynamic arguments/spread/tagged calls");
        }
        let separator = if let Some(argument) = args.first() {
            match self.string(*argument) {
                Some(s) => s,
                None => {
                    return self.reject(
                        *argument,
                        "UNSUPPORTED_STRING_ESCAPE",
                        "join separator needs an exact finite literal decoder",
                    );
                }
            }
        } else {
            ",".into()
        };
        let receiver = field(function_node, "object")?;
        if !matches!(unparen(receiver).kind(), "identifier" | "member_expression") {
            return self.reject(
                node,
                "UNSAFE_JOIN_RECEIVER",
                "join receiver must be an explicit lexical JSON-array data path",
            );
        }
        let collection = self.expr(receiver)?;
        Ok(
            json!({"kind":"join","collection":collection,"separator":separator,"optionalMember":function_node.child_by_field_name("optional_chain").is_some(),
            "optionalCall":node.child_by_field_name("optional_chain").is_some(),"chainContinuation":active_chain(function_node),
            "elementContract":"JSON scalar elements only; null/undefined elements are empty text as defined by join; reject nested arrays/objects",
            "receiverContract":"dense-json-array; otherwise error unless optional-chain short-circuits","span":self.span(node)}),
        )
    }

    fn tag(&mut self, node: Node<'_>) -> ResultValue {
        let name = self.raw(node).to_owned();
        if node.kind() != "identifier" {
            return self.reject(
                node,
                "DYNAMIC_COMPONENT",
                "member/namespaced/dynamic component selection is not lowered",
            );
        }
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
        ];
        if TAGS.contains(&name.as_str()) {
            return Ok(json!({"kind":"intrinsic","name":name}));
        }
        match self.lookup(&name) {
            Some(Symbol {
                kind: SymbolKind::Import { module, exported },
                id,
                ..
            }) => {
                if module == "react" && exported == "Fragment" {
                    return Ok(json!({"kind":"fragment","binding":id}));
                }
                self.issue(node,"UNCOMPILED_COMPONENT_DEPENDENCY","external component implementation is not available to this template-only stage")?;
                Ok(
                    json!({"kind":"component-reference","binding":id,"name":name,"module":module,"exported":exported}),
                )
            }
            Some(Symbol {
                kind: SymbolKind::Styled,
                id,
                ..
            }) => {
                self.issue(
                    node,
                    "UNCOMPILED_COMPONENT_DEPENDENCY",
                    "styled component requires the separate static style resolver",
                )?;
                Ok(json!({"kind":"component-reference","binding":id,"name":name}))
            }
            _ => self.reject(
                node,
                "UNRESOLVED_COMPONENT",
                "component is neither a whitelisted intrinsic nor a resolved import/styled binding",
            ),
        }
    }

    fn attribute(&mut self, node: Node<'_>) -> ResultValue {
        if node.kind() != "jsx_attribute" {
            return self.reject(
                node,
                "UNSAFE_ATTRIBUTE_SPREAD",
                "JSX spread cannot be statically enumerated",
            );
        }
        let parts = children(node);
        let Some(name) = parts.first().copied() else {
            return self.reject(node, "UNSUPPORTED_ATTRIBUTE", "attribute lacks a name");
        };
        let key = self.raw(name).to_owned();
        const ATTRS: &[&str] = &[
            "key",
            "id",
            "className",
            "title",
            "role",
            "lang",
            "dir",
            "href",
            "target",
            "rel",
            "aria-label",
            "aria-hidden",
            "data-testid",
            "startDate",
            "endDate",
            "basics",
        ];
        if name.kind() != "property_identifier" || !ATTRS.contains(&key.as_str()) {
            self.issue(node,"UNSAFE_OR_UNSUPPORTED_ATTRIBUTE","attribute is not in the finite whitelist; handlers, raw HTML, style, polymorphic as, refs and resource loads are unsupported")?;
        }
        let value = if parts.len() == 1 {
            json!({"kind":"literal","value":true,"span":self.span(node)})
        } else if parts.len() == 2 && parts[1].kind() == "string" {
            let raw = self.raw(parts[1]);
            let inner = raw.get(1..raw.len().saturating_sub(1)).unwrap_or("");
            match decode_entities(inner) {
                Some(value) => json!({"kind":"literal","value":value,"span":self.span(parts[1])}),
                None => self.reject(
                    parts[1],
                    "UNSUPPORTED_ENTITY",
                    "attribute entity cannot be decoded without guessing",
                )?,
            }
        } else if parts.len() == 2 {
            self.expr(parts[1])?
        } else {
            self.reject(
                node,
                "UNSUPPORTED_ATTRIBUTE",
                "attribute has unexpected AST children",
            )?
        };
        if key == "href" {
            if let Some(url) = value["value"].as_str() {
                let safe = !url.chars().any(char::is_control)
                    && url.split_once(':').is_none_or(|(scheme, _)| {
                        matches!(
                            scheme.to_ascii_lowercase().as_str(),
                            "https" | "http" | "mailto" | "tel"
                        )
                    });
                if !safe {
                    self.issue(
                        node,
                        "UNSAFE_URL_ATTRIBUTE",
                        "URL scheme/control characters are outside the static link whitelist",
                    )?;
                }
            } else {
                self.issue(
                    node,
                    "UNVERIFIED_URL_ATTRIBUTE",
                    "dynamic URL requires separate validation; not silently assumed safe",
                )?;
            }
        }
        Ok(json!({"kind":"attribute","name":key,"value":value,"span":self.span(node)}))
    }

    fn jsx(&mut self, node: Node<'_>) -> ResultValue {
        let open = if node.kind() == "jsx_self_closing_element" {
            node
        } else {
            field(node, "open_tag")?
        };
        let tag = if let Some(name) = open.child_by_field_name("name") {
            self.tag(name)?
        } else {
            json!({"kind":"fragment"})
        };
        if let Some(close) = node.child_by_field_name("close_tag") {
            let opening = open.child_by_field_name("name").map(|name| self.raw(name));
            let closing = close.child_by_field_name("name").map(|name| self.raw(name));
            if opening != closing {
                self.issue(
                    close,
                    "MISMATCHED_JSX_TAG",
                    "opening and closing component names differ",
                )?;
            }
        }
        let name_id = open.child_by_field_name("name").map(|name| name.id());
        let mut attributes = Vec::new();
        let mut keys = BTreeSet::new();
        for part in children(open)
            .into_iter()
            .filter(|part| Some(part.id()) != name_id)
        {
            let attribute = self.attribute(part)?;
            if let Some(name) = attribute["name"].as_str()
                && !keys.insert(name.to_owned())
            {
                self.issue(
                    part,
                    "DUPLICATE_ATTRIBUTE",
                    "duplicate attributes retained in source order, not silently merged",
                )?;
            }
            attributes.push(attribute);
        }
        let parts = if node.kind() == "jsx_self_closing_element" {
            Vec::new()
        } else {
            children(node)
                .into_iter()
                .filter(|part| {
                    !matches!(part.kind(), "jsx_opening_element" | "jsx_closing_element")
                })
                .collect::<Vec<_>>()
        };
        let mut nodes = Vec::new();
        let mut index = 0;
        while index < parts.len() {
            let part = parts[index];
            if matches!(part.kind(), "jsx_text" | "html_character_reference") {
                let start = index;
                let mut decoded = String::new();
                let mut segments = Vec::new();
                while index < parts.len()
                    && matches!(parts[index].kind(), "jsx_text" | "html_character_reference")
                {
                    let text = parts[index];
                    let raw = self.raw(text).to_owned();
                    if text.kind() == "jsx_text" {
                        decoded.push_str(&raw);
                    } else if let Some(value) = decode_entities(&raw) {
                        decoded.push_str(&value);
                    } else {
                        self.issue(
                            text,
                            "UNSUPPORTED_ENTITY",
                            "entity is retained but cannot be normalized without guessing",
                        )?;
                        decoded.push_str(&raw);
                    }
                    segments.push(self.evidence(text, "text-segment"));
                    index += 1;
                }
                let first = parts[start];
                let last = parts[index - 1];
                let mut span = self.span(first);
                span["endByte"] = json!(last.end_byte());
                span["endLine"] = json!(last.end_position().row + 1);
                span["endColumn"] = json!(last.end_position().column + 1);
                nodes.push(json!({"kind":"text","raw":&self.source[first.start_byte()..last.end_byte()],"value":jsx_whitespace(&decoded),"segments":segments,"span":span}));
            } else {
                nodes.push(self.expr(part)?);
                index += 1;
            }
        }
        Ok(
            json!({"kind":if tag["kind"]=="fragment"{"fragment"}else{"element"},"tag":tag,"attributes":attributes,"children":nodes,"span":self.span(node)}),
        )
    }
}

fn contains_jsx(value: &Value) -> bool {
    let mut pending = vec![value];
    while let Some(value) = pending.pop() {
        match value {
            Value::Object(object) => {
                if matches!(
                    object.get("kind").and_then(Value::as_str),
                    Some("element" | "fragment")
                ) {
                    return true;
                }
                pending.extend(object.values());
            }
            Value::Array(array) => pending.extend(array),
            _ => {}
        }
    }
    false
}

fn decode_entities(raw: &str) -> Option<String> {
    let mut output = String::new();
    let mut remaining = raw;
    while let Some(start) = remaining.find('&') {
        output.push_str(&remaining[..start]);
        let after = &remaining[start + 1..];
        let Some(end) = after
            .find(';')
            .filter(|end| *end <= 16 && !after[..*end].contains([' ', '\n', '\r', '\t', '&']))
        else {
            output.push('&');
            remaining = after;
            continue;
        };
        let entity = &after[..end];
        let decoded = match entity {
            "amp" => '&',
            "lt" => '<',
            "gt" => '>',
            "quot" => '"',
            "apos" => '\'',
            "nbsp" => '\u{a0}',
            _ if entity.starts_with("#x") || entity.starts_with("#X") => {
                char::from_u32(u32::from_str_radix(&entity[2..], 16).ok()?)?
            }
            _ if entity.starts_with('#') => char::from_u32(entity[1..].parse::<u32>().ok()?)?,
            _ => return None,
        };
        if decoded == '\0' {
            return None;
        }
        output.push(decoded);
        remaining = &after[end + 1..];
    }
    output.push_str(remaining);
    Some(output)
}

/// Babel/React-style line folding, performed once per adjacent text/entity run.
fn jsx_whitespace(raw: &str) -> String {
    let normalized = raw
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\t', " ");
    let lines = normalized.split('\n').collect::<Vec<_>>();
    let last_nonempty = lines
        .iter()
        .rposition(|line| line.chars().any(|ch| ch != ' '))
        .unwrap_or(0);
    let mut output = String::new();
    for (index, line) in lines.iter().enumerate() {
        let line = if index > 0 {
            line.trim_start_matches(' ')
        } else {
            line
        };
        let line = if index + 1 < lines.len() {
            line.trim_end_matches(' ')
        } else {
            line
        };
        if !line.is_empty() {
            output.push_str(line);
            if index != last_nonempty {
                output.push(' ');
            }
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ir(source: &str) -> Value {
        compile("src/Resume.jsx", source.as_bytes()).unwrap()
    }
    fn codes(value: &Value) -> Vec<&str> {
        value["blockers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|issue| issue["code"].as_str().unwrap())
            .collect()
    }
    fn nodes<'a>(value: &'a Value, kind: &str) -> Vec<&'a Value> {
        let mut pending = vec![value];
        let mut result = Vec::new();
        while let Some(value) = pending.pop() {
            match value {
                Value::Object(object) => {
                    if object.get("kind").and_then(Value::as_str) == Some(kind) {
                        result.push(value);
                    }
                    pending.extend(object.values());
                }
                Value::Array(array) => pending.extend(array),
                _ => {}
            }
        }
        result
    }

    #[test]
    fn only_actual_exported_function_return_is_entry_and_source_is_never_executed() {
        let fake = ir("const fake = '<div>fake JSX</div>'; export default fake;");
        assert_eq!(fake["entry"], Value::Null);
        assert!(codes(&fake).contains(&"UNRESOLVED_ENTRYPOINT"));
        let value = ir(
            "globalThis.__TEMPLATE_SIDE_EFFECT__ = fetch('https://never.invalid'); function Resume(){ return <p>real</p>; } export default Resume;",
        );
        assert_eq!(value["resolved"], false);
        assert!(codes(&value).contains(&"UNSUPPORTED_TOP_LEVEL"));
        assert_eq!(nodes(&value["entry"], "text")[0]["value"], "real");
        assert!(value["dependencies"].to_string().contains("never.invalid"));
    }

    #[test]
    fn named_declaration_and_default_alias_share_one_actual_entry() {
        let value = ir(
            "export function Section({children}) { return <section>{children}</section>; } export default Section;",
        );
        assert_eq!(value["resolved"], true, "{:#}", value["blockers"]);
        assert_eq!(nodes(&value["entry"], "intrinsic")[0]["name"], "section");
        let constant = ir(
            "export const title = 'Hello'; export default function Resume(){return <h1>{title}</h1>;}",
        );
        assert_eq!(constant["resolved"], true, "{:#}", constant["blockers"]);
        let helper = ir(
            "export function helper(){return <b/>;} export default function Resume(){return <p/>;}",
        );
        assert!(codes(&helper).contains(&"UNCOMPILED_FUNCTION_DEPENDENCY"));
        let reexport = ir(
            "export {Section} from './Section.jsx'; export default function Resume(){return <p/>;}",
        );
        assert!(codes(&reexport).contains(&"UNSUPPORTED_EXPORT"));
        let mutable = ir(
            "export let title = 'Hello'; export default function Resume(){return <h1>{title}</h1>;}",
        );
        assert_ne!(mutable["resolved"], true);
    }

    #[test]
    fn binds_resume_defaults_optional_fields_nested_map_and_conditional_without_empty_substitution()
    {
        let value = ir(
            "export default function Resume({resume}) { const {basics = {}, work = []} = resume; return <><h1>{basics?.name}</h1>{work?.length > 0 && work.map((job,index) => <section key={index}>{job.name ? <b>{job.name}</b> : null}{job.highlights?.map((line,i) => <p key={i}>{line}</p>)}</section>)}</>; }",
        );
        assert_eq!(value["resolved"], true, "{:#}", value["blockers"]);
        assert_eq!(nodes(&value["entry"], "map").len(), 2);
        assert_eq!(nodes(&value["entry"], "conditional").len(), 1);
        assert!(
            nodes(&value["entry"], "get")
                .iter()
                .any(|node| node["optional"] == true)
        );
        assert_eq!(nodes(&value["bindings"], "default").len(), 2);
        for node in nodes(&value["bindings"], "default") {
            assert_eq!(node["when"], "undefined");
        }
        let nested = nodes(&value["entry"], "map");
        assert_ne!(nested[0]["parameters"][0], nested[1]["parameters"][0]);
    }

    #[test]
    fn reports_all_unknown_expressions_and_unresolved_bindings() {
        let value = ir(
            "export default ({resume}) => <div>{one(two())}{resume.name.toUpperCase()}{missing}<Unknown />{resume['x']}</div>;",
        );
        assert_eq!(value["resolved"], false);
        let codes = codes(&value);
        assert!(codes.contains(&"UNSUPPORTED_CALL"));
        assert!(codes.contains(&"UNSUPPORTED_SUBTREE_NODE"));
        assert!(codes.contains(&"UNRESOLVED_BINDING"));
        assert!(codes.contains(&"UNRESOLVED_COMPONENT"));
        assert!(codes.contains(&"UNSUPPORTED_EXPRESSION"));
        assert!(
            value["blockers"]
                .as_array()
                .unwrap()
                .iter()
                .all(|issue| issue["span"]["file"] == "src/Resume.jsx")
        );
    }

    #[test]
    fn unsafe_attributes_spread_urls_and_dynamic_components_are_not_silently_dropped() {
        let value = ir(
            "export default ({resume}) => <div onClick={run()} dangerouslySetInnerHTML={resume.html} {...resume}><a href='javascript:alert(1)'>link</a><UI.Card /></div>;",
        );
        assert_eq!(value["resolved"], false);
        let codes = codes(&value);
        for expected in [
            "UNSAFE_OR_UNSUPPORTED_ATTRIBUTE",
            "UNSAFE_ATTRIBUTE_SPREAD",
            "UNSAFE_URL_ATTRIBUTE",
            "DYNAMIC_COMPONENT",
            "UNSUPPORTED_CALL",
        ] {
            assert!(codes.contains(&expected), "missing {expected}: {codes:?}");
        }
        assert_eq!(nodes(&value["entry"], "attribute").len(), 3);
    }

    #[test]
    fn preserves_import_styled_and_lexical_dependency_evidence_without_claiming_component_resolution()
     {
        let source = "import React from 'react'; import styled from 'styled-components'; import {Section as S} from '@jsonresume/core'; const Box=styled.div`color: red;`; function Resume({resume}) {return <Box><S>{resume.basics.name}</S></Box>;} export default Resume;";
        let value = ir(source);
        assert_eq!(value["resolved"], false);
        assert!(codes(&value).contains(&"UNCOMPILED_STYLE_DEPENDENCY"));
        assert!(codes(&value).contains(&"UNCOMPILED_COMPONENT_DEPENDENCY"));
        assert_eq!(nodes(&value["entry"], "component-reference").len(), 2);
        assert!(
            value["dependencies"]
                .to_string()
                .contains("@jsonresume/core")
        );
        assert!(value["dependencies"].to_string().contains("color: red;"));
    }

    #[test]
    fn lexical_shadowing_and_map_bindings_do_not_leak_and_forward_reads_are_blocked() {
        let value = ir(
            "export default ({resume}) => <>{resume.work.map((item) => <p>{item.name}</p>)}{item}</>;",
        );
        assert!(codes(&value).contains(&"UNRESOLVED_BINDING"));
        let value = ir(
            "const outer='global'; export default function Resume(){ const a=outer; const outer='local'; return <p>{a}</p>; }",
        );
        assert!(codes(&value).contains(&"UNINITIALIZED_BINDING"));
        let value = ir(
            "export default ({resume}) => <>{resume.work.map((item) => <p>{item.name}</p>)}{resume.work.map((item) => <b>{item.name}</b>)}</>;",
        );
        assert_eq!(value["resolved"], true, "{:#}", value["blockers"]);
        let maps = nodes(&value["entry"], "map");
        assert_ne!(maps[0]["parameters"][0], maps[1]["parameters"][0]);
    }

    #[test]
    fn text_entities_bare_ampersands_and_fragment_whitespace_keep_original_offsets() {
        let source = "export default () => <>\n  Awards & Honors &amp; Research\n  &lt; work &#x2022; résumé\n</>;";
        let value = ir(source);
        assert_eq!(value["resolved"], true, "{:#}", value["blockers"]);
        let text = nodes(&value["entry"], "text");
        assert_eq!(text.len(), 1);
        assert_eq!(
            text[0]["value"],
            "Awards & Honors & Research < work • résumé"
        );
        let start = text[0]["span"]["startByte"].as_u64().unwrap() as usize;
        let end = text[0]["span"]["endByte"].as_u64().unwrap() as usize;
        assert_eq!(text[0]["raw"], &source[start..end]);
        assert!(text[0]["segments"].as_array().unwrap().len() > 1);
    }

    #[test]
    fn optional_chain_groups_keep_distinct_access_and_map_boundaries() {
        let value = ir(
            "export default ({obj}) => <div>{obj?.a.b}{(obj?.a).b}{obj?.items.map(item => <p>{item}</p>)}{(obj?.items).map(item => <p>{item}</p>)}</div>;",
        );
        assert_eq!(value["resolved"], true, "{:#}", value["blockers"]);
        let access = nodes(&value["entry"], "get")
            .into_iter()
            .filter(|node| node["property"] == "b")
            .collect::<Vec<_>>();
        assert_eq!(access.len(), 2);
        assert!(
            access
                .iter()
                .any(|node| node["chainContinuation"] == true && node["object"]["kind"] == "get")
        );
        assert!(access.iter().any(|node|node["chainContinuation"]==false && node["object"]["kind"]=="group"));
        let maps = nodes(&value["entry"], "map");
        assert_eq!(maps.len(), 2);
        assert!(maps.iter().any(|node| node["chainContinuation"] == true));
        assert!(maps.iter().any(
            |node| node["chainContinuation"] == false && node["collection"]["kind"] == "group"
        ));
        for binding in value["bindings"].as_array().unwrap() {
            assert!(binding["scope"].is_string());
        }
    }

    #[test]
    fn text_templates_and_join_keep_explicit_scalar_semantics_original_text_and_spans() {
        let source = "export default ({resume}) => <p>{` • ${resume.score}\r\n<span> ${resume.active} &amp;`}{resume.keywords?.join(', ')}</p>;";
        let value = ir(source);
        assert_eq!(value["resolved"], true, "{}", value["blockers"]);
        let templates = nodes(&value["entry"], "text-template");
        assert_eq!(templates.len(), 1);
        let parts = templates[0]["parts"].as_array().unwrap();
        assert_eq!(parts[0]["value"], " • ");
        assert_eq!(parts[1]["kind"], "to-string");
        assert_eq!(parts[2]["value"], "\n<span> ");
        assert_eq!(parts[2]["raw"], "\r\n<span> ");
        assert_eq!(parts[4]["value"], " &amp;");
        for part in parts.iter().filter(|p| p["kind"] == "literal") {
            let start = part["span"]["startByte"].as_u64().unwrap() as usize;
            let end = part["span"]["endByte"].as_u64().unwrap() as usize;
            assert_eq!(part["raw"], &source[start..end]);
        }
        let join = nodes(&value["entry"], "join")[0];
        assert_eq!(join["separator"], ", ");
        assert_eq!(join["optionalMember"], true);
        assert!(
            value["evaluationContract"]["toString"]
                .as_str()
                .unwrap()
                .contains("not JSON serialization")
        );
    }

    #[test]
    fn join_defaults_and_optional_chain_parentheses_remain_distinct() {
        let value = ir(
            "export default ({resume}) => <p>{resume?.keywords.join()}{(resume?.keywords).join('')}</p>;",
        );
        assert_eq!(value["resolved"], true, "{}", value["blockers"]);
        let joins = nodes(&value["entry"], "join");
        assert_eq!(joins.len(), 2);
        assert!(
            joins
                .iter()
                .any(|j| j["separator"] == "," && j["chainContinuation"] == true)
        );
        assert!(joins.iter().any(|j| j["separator"] == ""
            && j["chainContinuation"] == false
            && j["collection"]["kind"] == "group"));
    }

    #[test]
    fn unknown_join_coercions_template_escapes_and_calls_cannot_be_silently_accepted() {
        for expression in [
            "resume.items.join(separator)",
            "resume.items.join(run())",
            "resume.items.join(...resume.args)",
            "resume.items.join(',', 'extra')",
            "load().join(',')",
            "resume.items.join`not a call`",
            "`escaped\\n${resume.name}`",
            "`object ${{}}`",
            "`array ${resume.items.map(x=><p>{x}</p>)}`",
        ] {
            let value = ir(&format!(
                "export default ({{resume}}) => <p>{{{expression}}}</p>;"
            ));
            assert_eq!(
                value["resolved"], false,
                "unexpectedly accepted {expression}"
            );
            assert!(!value["blockers"].as_array().unwrap().is_empty());
        }
    }

    #[test]
    fn mismatched_tags_and_nonfunction_default_exports_are_explicit_blockers() {
        let value = ir("export default () => <div></p>;");
        assert_eq!(value["resolved"], false);
        assert_eq!(value["syntaxValid"], false);
        assert!(codes(&value).contains(&"MISMATCHED_JSX_TAG"));
        let value = ir("export default create(nested());");
        assert!(codes(&value).contains(&"UNSUPPORTED_ENTRYPOINT_EXPRESSION"));
        assert!(codes(&value).contains(&"UNSUPPORTED_SUBTREE_NODE"));
        let value =
            ir("export default function Resume({resume}) { const {}=resume; return <p/>; }");
        assert!(codes(&value).contains(&"UNSUPPORTED_EMPTY_DESTRUCTURING"));
    }

    #[test]
    fn syntax_unknown_control_flow_and_limits_never_report_resolved() {
        let value = ir(
            "export default function Resume({resume}) {if (resume.ok) return <p>A</p>; return <p>B</p>;}",
        );
        assert_eq!(value["resolved"], false);
        assert!(codes(&value).contains(&"UNSUPPORTED_FUNCTION_STATEMENT"));
        let value = ir("export default () => <p>{bad(}</p>");
        assert_eq!(value["syntaxValid"], false);
        assert_eq!(value["resolved"], false);
        assert!(
            compile("Resume.jsx", &[0xff])
                .unwrap_err()
                .starts_with("TEMPLATE_UTF8")
        );
        assert!(
            compile("Resume.jsx", &vec![b' '; MAX_SOURCE_BYTES + 1])
                .unwrap_err()
                .starts_with("TEMPLATE_SOURCE_LIMIT")
        );
        let deep = format!(
            "export default()=>{}<p/>{};",
            "(".repeat(MAX_AST_DEPTH + 1),
            ")".repeat(MAX_AST_DEPTH + 1)
        );
        assert!(
            compile("Resume.jsx", deep.as_bytes())
                .unwrap_err()
                .starts_with("TEMPLATE_AST_LIMIT")
        );
    }
}
