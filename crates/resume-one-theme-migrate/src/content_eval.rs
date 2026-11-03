//! Bounded data operations over compiler-owned IR, never a JS interpreter.
//! Produces an unstyled content tree, not a theme or visual acceptance.

use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Write};
use std::time::{Duration, Instant};

#[derive(Clone)]
enum Data {
    Missing,
    Shorted,
    Json(Value),
    List(Vec<Data>),
    Fragment(Vec<Data>),
    Element {
        tag: Value,
        props: BTreeMap<String, Data>,
        children: Vec<Data>,
    },
}

impl Data {
    fn nullish(&self) -> bool {
        matches!(
            self,
            Self::Missing | Self::Shorted | Self::Json(Value::Null)
        )
    }
    fn truth(&self) -> bool {
        match self {
            Self::Missing | Self::Shorted | Self::Json(Value::Null) => false,
            Self::Json(Value::Bool(b)) => *b,
            Self::Json(Value::Number(n)) => n.as_f64().is_some_and(|n| n != 0.0),
            Self::Json(Value::String(s)) => !s.is_empty(),
            _ => true,
        }
    }
    fn text(&self) -> Result<String, String> {
        match self {
            Self::Missing | Self::Shorted => Ok("undefined".into()),
            Self::Json(Value::Null) => Ok("null".into()),
            Self::Json(Value::Bool(v)) => Ok(v.to_string()),
            Self::Json(Value::String(v)) => Ok(v.clone()),
            Self::Json(Value::Number(v)) => {
                let value = v
                    .as_f64()
                    .filter(|v| v.is_finite())
                    .ok_or("CONTENT_NUMBER_RANGE")?;
                Ok(ryu_js::Buffer::new().format_finite(value).to_owned())
            }
            _ => Err("CONTENT_COMPOSITE_COERCION_UNSUPPORTED".into()),
        }
    }
    fn cost(&self) -> usize {
        match self {
            Self::Missing | Self::Shorted => 1,
            Self::Json(v) => json_cost(v),
            Self::List(v) | Self::Fragment(v) => {
                v.iter().map(Self::cost).sum::<usize>()
                    + v.len() * std::mem::size_of::<Data>()
                    + std::mem::size_of::<Data>()
            }
            Self::Element {
                tag,
                props,
                children,
            } => {
                json_cost(tag)
                    + props
                        .iter()
                        .map(|(k, v)| k.len() + v.cost() + 128)
                        .sum::<usize>()
                    + children.iter().map(Self::cost).sum::<usize>()
                    + children.len() * std::mem::size_of::<Data>()
                    + std::mem::size_of::<Data>()
            }
        }
    }
}

fn values<'a>(v: &'a Value, key: &str) -> impl Iterator<Item = &'a Value> {
    v[key].as_array().into_iter().flatten()
}
fn json_cost(v: &Value) -> usize {
    match v {
        Value::String(v) => v.len() + std::mem::size_of::<Value>(),
        Value::Array(v) => {
            v.iter().map(json_cost).sum::<usize>() + v.len() * std::mem::size_of::<Value>() + 32
        }
        Value::Object(v) => {
            v.iter()
                .map(|(k, v)| k.len() + json_cost(v) + 128)
                .sum::<usize>()
                + 32
        }
        _ => 32,
    }
}
fn preflight(v: &Value, max_nodes: usize) -> Result<(), String> {
    let mut pending = vec![(v, 0)];
    let mut count = 0;
    while let Some((v, depth)) = pending.pop() {
        count += 1;
        if depth > 96 || count > max_nodes {
            return Err("CONTENT_INPUT_LIMIT".into());
        }
        match v {
            Value::Array(v) => pending.extend(v.iter().map(|v| (v, depth + 1))),
            Value::Object(v) => pending.extend(v.values().map(|v| (v, depth + 1))),
            _ => {}
        }
    }
    Ok(())
}
fn name<'a>(v: &'a Value, key: &str) -> Result<&'a str, String> {
    v[key]
        .as_str()
        .ok_or_else(|| format!("CONTENT_FIELD_REQUIRED:{key}"))
}

struct Eval<'a> {
    report: &'a Value,
    template: &'a Value,
    bindings: BTreeMap<String, &'a Value>,
    env: BTreeMap<String, Data>,
    props: Value,
    steps: usize,
    bytes: usize,
    started: Instant,
}

impl Eval<'_> {
    fn tick(&mut self, depth: usize) -> Result<(), String> {
        self.steps += 1;
        if depth > 96 || self.steps > 200_000 || self.started.elapsed() > Duration::from_secs(3) {
            return Err("CONTENT_EVALUATION_LIMIT".into());
        }
        Ok(())
    }
    fn charge(&mut self, cost: usize) -> Result<(), String> {
        self.bytes = self.bytes.checked_add(cost).ok_or("CONTENT_BYTE_LIMIT")?;
        if self.bytes > 32 * 1024 * 1024 {
            return Err("CONTENT_BYTE_LIMIT".into());
        }
        Ok(())
    }
    fn json(&mut self, data: &Value) -> Result<Data, String> {
        self.charge(json_cost(data))?;
        Ok(Data::Json(data.clone()))
    }

    fn prepare(&mut self, scope: &str, depth: usize) -> Result<(), String> {
        // Eager declaration order matters even when a binding is never read.
        let bindings = values(&self.template["program"], "bindings")
            .filter(|b| b["scope"] == scope)
            .collect::<Vec<_>>();
        for b in bindings {
            if matches!(
                b["value"]["kind"].as_str(),
                Some("styled-reference" | "map-parameter")
            ) {
                continue;
            }
            let id = name(b, "id")?.to_owned();
            let value = self.expr(&b["value"], depth + 1)?;
            self.env.insert(id, value);
        }
        Ok(())
    }

    fn expr(&mut self, node: &Value, depth: usize) -> Result<Data, String> {
        self.tick(depth)?;
        match name(node, "kind")? {
            "input" => {
                let props = &self.props;
                let cost = json_cost(props);
                self.charge(cost)?;
                Ok(Data::Json(self.props.clone()))
            }
            "undefined" | "empty" => Ok(Data::Missing),
            "literal" => self.json(node.get("value").ok_or("CONTENT_LITERAL_MISSING")?),
            "text" => self.json(&node["value"]),
            "binding" => {
                let id = name(node, "id")?;
                let cost = self
                    .env
                    .get(id)
                    .ok_or("CONTENT_BINDING_NOT_INITIALIZED")?
                    .cost();
                self.charge(cost)?;
                Ok(self
                    .env
                    .get(id)
                    .ok_or("CONTENT_BINDING_NOT_INITIALIZED")?
                    .clone())
            }
            "default" => {
                let v = self.expr(&node["value"], depth + 1)?;
                if matches!(v, Data::Missing | Data::Shorted) {
                    self.expr(&node["fallback"], depth + 1)
                } else {
                    Ok(v)
                }
            }
            "group" => {
                let v = self.expr(&node["value"], depth + 1)?;
                Ok(if matches!(v, Data::Shorted) {
                    Data::Missing
                } else {
                    v
                })
            }
            "get" => {
                let object = self.expr(&node["object"], depth + 1)?;
                if matches!(object, Data::Shorted) && node["chainContinuation"] == true {
                    return Ok(Data::Shorted);
                }
                if object.nullish() {
                    return if node["optional"] == true {
                        Ok(Data::Shorted)
                    } else {
                        Err("CONTENT_NULLISH_PROPERTY_ACCESS".into())
                    };
                }
                let key = name(node, "property")?;
                if matches!(
                    key,
                    "__proto__"
                        | "prototype"
                        | "constructor"
                        | "toString"
                        | "valueOf"
                        | "toLocaleString"
                        | "hasOwnProperty"
                        | "isPrototypeOf"
                        | "propertyIsEnumerable"
                        | "__defineGetter__"
                        | "__defineSetter__"
                        | "__lookupGetter__"
                        | "__lookupSetter__"
                ) {
                    return Err("CONTENT_INHERITED_PROPERTY_UNSUPPORTED".into());
                }
                match object {
                    Data::Json(Value::Object(v)) => match v.get(key) {
                        Some(v) => self.json(v),
                        None => Ok(Data::Missing),
                    },
                    Data::Json(Value::Array(v)) if key == "length" => {
                        Ok(Data::Json(json!(v.len())))
                    }
                    Data::List(v) if key == "length" => Ok(Data::Json(json!(v.len()))),
                    Data::Fragment(_) | Data::Element { .. } if key == "length" => {
                        Ok(Data::Missing)
                    }
                    Data::Json(Value::String(v)) if key == "length" => {
                        Ok(Data::Json(json!(v.encode_utf16().count())))
                    }
                    _ => Err("CONTENT_PROPERTY_DOMAIN_UNSUPPORTED".into()),
                }
            }
            "logical" => {
                let left = self.expr(&node["left"], depth + 1)?;
                let right = match name(node, "operator")? {
                    "&&" => left.truth(),
                    "||" => !left.truth(),
                    "??" => left.nullish(),
                    _ => return Err("CONTENT_OPERATOR_UNSUPPORTED".into()),
                };
                if right {
                    self.expr(&node["right"], depth + 1)
                } else {
                    Ok(if matches!(left, Data::Shorted) {
                        Data::Missing
                    } else {
                        left
                    })
                }
            }
            "not" => {
                let v = self.expr(&node["value"], depth + 1)?;
                Ok(Data::Json(json!(!v.truth())))
            }
            "compare" => {
                let a = self.expr(&node["left"], depth + 1)?;
                let b = self.expr(&node["right"], depth + 1)?;
                Ok(Data::Json(json!(compare(&a, &b, name(node, "operator")?)?)))
            }
            "conditional" => {
                let cond = self.expr(&node["condition"], depth + 1)?;
                self.expr(
                    &node[if cond.truth() {
                        "consequence"
                    } else {
                        "alternative"
                    }],
                    depth + 1,
                )
            }
            "to-string" => {
                let v = self.expr(&node["value"], depth + 1)?;
                let text = v.text()?;
                self.charge(text.len())?;
                Ok(Data::Json(json!(text)))
            }
            "text-template" => {
                let mut out = String::new();
                for part in values(node, "parts") {
                    let value = self.expr(part, depth + 1)?.text()?;
                    self.charge(value.len())?;
                    out.push_str(&value);
                }
                Ok(Data::Json(json!(out)))
            }
            "join" | "map" => self.array_op(node, depth + 1),
            "fragment" => {
                let mut out = Vec::new();
                for child in values(node, "children") {
                    out.push(self.expr(child, depth + 1)?);
                }
                let value = Data::Fragment(out);
                self.charge(value.cost())?;
                Ok(value)
            }
            "element" => {
                let mut props = BTreeMap::new();
                for attr in values(node, "attributes") {
                    if attr["kind"] != "attribute" {
                        return Err("CONTENT_ATTRIBUTE_UNSUPPORTED".into());
                    }
                    let key = name(attr, "name")?.to_owned();
                    let value = self.expr(&attr["value"], depth + 1)?;
                    if key == "key"
                        && !matches!(
                            value,
                            Data::Missing
                                | Data::Shorted
                                | Data::Json(
                                    Value::Null
                                        | Value::Bool(_)
                                        | Value::Number(_)
                                        | Value::String(_)
                                )
                        )
                    {
                        return Err("CONTENT_KEY_COERCION_UNSUPPORTED".into());
                    }
                    if props.insert(key, value).is_some() {
                        return Err("CONTENT_DUPLICATE_ATTRIBUTE".into());
                    }
                }
                let mut children = Vec::new();
                for child in values(node, "children") {
                    children.push(self.expr(child, depth + 1)?);
                }
                self.charge(json_cost(&node["tag"]))?;
                let value = Data::Element {
                    tag: node["tag"].clone(),
                    props,
                    children,
                };
                self.charge(value.cost())?;
                Ok(value)
            }
            _ => Err(format!("CONTENT_NODE_UNSUPPORTED:{}", node["kind"])),
        }
    }

    fn array_op(&mut self, node: &Value, depth: usize) -> Result<Data, String> {
        let collection = self.expr(&node["collection"], depth + 1)?;
        if matches!(collection, Data::Shorted) && node["chainContinuation"] == true {
            return Ok(Data::Shorted);
        }
        if collection.nullish() {
            // Optional call cannot rescue a nullish receiver: a.map?.() still
            // reads map from a first. Plain JSON arrays always have map/join.
            return if node["optionalMember"] == true {
                Ok(Data::Shorted)
            } else {
                Err("CONTENT_NULLISH_ARRAY_RECEIVER".into())
            };
        }
        let array = match collection {
            Data::Json(Value::Array(v)) => v.into_iter().map(Data::Json).collect::<Vec<_>>(),
            Data::List(v) => v,
            _ => return Err("CONTENT_ARRAY_RECEIVER_REQUIRED".into()),
        };
        if array.len() > 10_000 {
            return Err("CONTENT_ARRAY_LIMIT".into());
        }
        self.charge(array.len() * std::mem::size_of::<Data>())?;
        if node["kind"] == "join" {
            let separator = name(node, "separator")?;
            let mut out = String::new();
            for (i, v) in array.iter().enumerate() {
                self.tick(depth)?;
                let text = if v.nullish() {
                    String::new()
                } else {
                    v.text()?
                };
                self.charge(text.len() + if i > 0 { separator.len() } else { 0 })?;
                if i > 0 {
                    out.push_str(separator);
                }
                out.push_str(&text);
            }
            return Ok(Data::Json(json!(out)));
        }
        let scope = name(node, "scope")?;
        let params = values(node, "parameters")
            .map(|v| v.as_str().ok_or("CONTENT_MAP_PARAMETER"))
            .collect::<Result<Vec<_>, _>>()?;
        if params.is_empty() || params.len() > 2 {
            return Err("CONTENT_MAP_PARAMETER".into());
        }
        let scope_ids = values(&self.template["program"], "bindings")
            .filter(|b| b["scope"] == scope)
            .map(|b| name(b, "id").map(str::to_owned))
            .collect::<Result<Vec<_>, _>>()?;
        let old = scope_ids
            .iter()
            .map(|id| (id.clone(), self.env.remove(id)))
            .collect::<Vec<_>>();
        let mut result = Vec::new();
        for (i, item) in array.into_iter().enumerate() {
            self.env.insert(params[0].to_owned(), item);
            if params.len() == 2 {
                self.env.insert(params[1].to_owned(), Data::Json(json!(i)));
            }
            self.prepare(scope, depth + 1)?;
            result.push(self.expr(&node["body"], depth + 1)?);
        }
        for (id, value) in old {
            if let Some(value) = value {
                self.env.insert(id, value);
            } else {
                self.env.remove(&id);
            }
        }
        Ok(Data::List(result))
    }

    fn imported(&self, id: &str) -> Result<&Value, String> {
        let imports = values(&self.template["program"], "dependencies")
            .filter(|d| d["kind"] == "import")
            .flat_map(|d| values(d, "specifiers").map(move |s| (d, s)))
            .filter(|(_, s)| {
                s["local"].as_str().is_some_and(|local| {
                    s["span"]["startByte"]
                        .as_u64()
                        .is_some_and(|start| id == format!("b:{start}:{local}"))
                })
            })
            .collect::<Vec<_>>();
        if imports.len() != 1 {
            return Err("CONTENT_IMPORT_BINDING_UNRESOLVED".into());
        }
        let (dep, spec) = imports[0];
        let requests = values(&self.report["moduleLinks"], "requests")
            .filter(|r| {
                r["entryFile"] == self.template["entryFile"]
                    && r["import"]["local"] == spec["local"]
                    && r["import"]["module"] == dep["module"]
                    && r["import"]["imported"] == spec["exported"]
            })
            .collect::<Vec<_>>();
        if requests.len() != 1 || requests[0]["declarationResolved"] != true {
            return Err("CONTENT_IMPORT_DECLARATION_UNRESOLVED".into());
        }
        Ok(&requests[0]["target"])
    }

    fn render(&mut self, data: Data, out: &mut Vec<Value>, depth: usize) -> Result<(), String> {
        self.tick(depth)?;
        match data {
            Data::Missing | Data::Shorted | Data::Json(Value::Null | Value::Bool(_)) => {}
            Data::Json(Value::String(s)) => {
                self.charge(s.len())?;
                if !s.is_empty() {
                    out.push(json!(s));
                }
            }
            Data::Json(Value::Number(n)) => {
                out.push(json!(Data::Json(Value::Number(n)).text()?));
            }
            Data::Json(Value::Array(v)) => {
                for v in v {
                    self.render(Data::Json(v), out, depth + 1)?;
                }
            }
            Data::Json(Value::Object(_)) => return Err("CONTENT_OBJECT_CHILD".into()),
            Data::List(v) | Data::Fragment(v) => {
                for v in v {
                    self.render(v, out, depth + 1)?;
                }
            }
            Data::Element {
                tag,
                props,
                children,
            } => {
                let mut styles = Vec::new();
                let mut external_layers = Vec::new();
                let mut resolved = tag.clone();
                if tag["kind"] == "component-reference" {
                    let id = name(&tag, "binding")?;
                    if self
                        .bindings
                        .get(id)
                        .is_some_and(|b| b["value"]["kind"] == "styled-reference")
                    {
                        let nodes = values(&self.template["styleGraph"], "nodes")
                            .filter(|n| n["id"] == id)
                            .collect::<Vec<_>>();
                        if nodes.len() != 1 {
                            return Err("CONTENT_STYLED_BINDING_UNRESOLVED".into());
                        }
                        let node = nodes[0];
                        styles = values(node, "layers").cloned().collect();
                        if node["terminal"]["kind"] == "intrinsic" {
                            resolved = node["terminal"].clone();
                        } else if node["terminal"]["kind"] == "import" {
                            let local = name(&node["terminal"], "local")?;
                            let imports = values(&self.template["program"], "dependencies")
                                .filter(|d| d["kind"] == "import")
                                .flat_map(|d| values(d, "specifiers"))
                                .filter(|s| s["local"] == local)
                                .collect::<Vec<_>>();
                            if imports.len() != 1 {
                                return Err("CONTENT_STYLE_IMPORT_UNRESOLVED".into());
                            }
                            let binding = format!(
                                "b:{}:{local}",
                                imports[0]["span"]["startByte"]
                                    .as_u64()
                                    .ok_or("CONTENT_STYLE_IMPORT_UNRESOLVED")?
                            );
                            let chain = crate::external_styles::resolve(
                                self.report,
                                self.imported(&binding)?,
                            )?;
                            external_layers = values(&chain, "layers").cloned().collect();
                            resolved = chain["terminal"].clone();
                        } else {
                            return Err("CONTENT_STYLE_TERMINAL_UNRESOLVED".into());
                        }
                    } else {
                        let chain =
                            crate::external_styles::resolve(self.report, self.imported(id)?)?;
                        external_layers = values(&chain, "layers").cloned().collect();
                        resolved = chain["terminal"].clone();
                    }
                }
                let ignore_children = resolved["kind"] == "core"
                    && matches!(
                        resolved["target"]["name"].as_str(),
                        Some("DateRange" | "ContactInfo")
                    );
                if resolved["kind"] == "intrinsic"
                    && matches!(resolved["name"].as_str(), Some("br" | "hr"))
                    && !(children.is_empty() || (children.len() == 1 && children[0].nullish()))
                {
                    return Err("CONTENT_VOID_CHILDREN".into());
                }
                let mut rendered = Vec::new();
                if !ignore_children {
                    for child in children {
                        self.render(child, &mut rendered, depth + 1)?;
                    }
                }
                let mut attrs = Map::new();
                for (key, value) in props {
                    match value {
                        Data::Missing | Data::Shorted => {}
                        Data::Json(value) => {
                            attrs.insert(key, value);
                        }
                        _ => return Err("CONTENT_COMPOSITE_ATTRIBUTE".into()),
                    }
                }
                let mut element = if resolved["kind"] == "core" {
                    self.primitive(&resolved["target"], attrs, rendered)?
                } else if resolved["kind"] == "intrinsic" {
                    Some(native(name(&resolved, "name")?, attrs, rendered)?)
                } else {
                    return Err("CONTENT_TAG_UNSUPPORTED".into());
                };
                if let Some(element) = &mut element {
                    if !styles.is_empty() {
                        element["styleBindings"] = json!(styles);
                    }
                    if !external_layers.is_empty() {
                        element["externalStyleLayers"] = json!(external_layers);
                    }
                    element["sourceTag"] = tag;
                }
                if let Some(element) = element {
                    self.charge(json_cost(&element))?;
                    out.push(element);
                }
            }
        }
        Ok(())
    }

    fn primitive(
        &mut self,
        target: &Value,
        mut props: Map<String, Value>,
        children: Vec<Value>,
    ) -> Result<Option<Value>, String> {
        let kind = pinned(self.report, target)?;
        // key has already been evaluated. React never forwards it to DOM props.
        props.remove("key");
        match kind {
            "Section" | "SectionTitle" => {
                let allowed = if kind == "Section" {
                    vec!["id", "className"]
                } else {
                    vec!["id", "className", "level", "icon"]
                };
                if props.keys().any(|k| !allowed.contains(&k.as_str())) {
                    return Err("CONTENT_CORE_PROP_UNSUPPORTED".into());
                }
                let prefix = if kind == "Section" {
                    "resume-section"
                } else {
                    "resume-section-title"
                };
                add_class(&mut props, prefix)?;
                let tag = if kind == "Section" {
                    "section".to_owned()
                } else {
                    if props.contains_key("icon") {
                        return Err("CONTENT_TITLE_ICON_UNSUPPORTED".into());
                    }
                    let level = props.remove("level").unwrap_or(json!(2));
                    let level = level
                        .as_u64()
                        .filter(|n| (1..=6).contains(n))
                        .ok_or("CONTENT_TITLE_LEVEL_UNSUPPORTED")?;
                    format!("h{level}")
                };
                let mut node = native(&tag, props, children)?;
                node["styleRole"] = json!(if kind == "Section" {
                    "core-section"
                } else {
                    "core-section-title"
                });
                Ok(Some(node))
            }
            "DateRange" => {
                let text = crate::primitive_dates::format(&props)?;
                let Some(text) = text else {
                    return Ok(None);
                };
                let allowed = [
                    "startDate",
                    "endDate",
                    "format",
                    "locale",
                    "numberingSystem",
                    "presentLabel",
                    "className",
                    "id",
                ];
                if props.keys().any(|k| !allowed.contains(&k.as_str())) {
                    return Err("CONTENT_DATE_PROP_UNSUPPORTED".into());
                }
                props.retain(|k, _| matches!(k.as_str(), "className" | "id"));
                add_class(&mut props, "resume-date-range")?;
                let mut node = native("span", props, vec![json!(text)])?;
                node["styleRole"] = json!("core-date-range");
                Ok(Some(node))
            }
            "ContactInfo" => {
                let mut out = crate::primitive_contacts::expand(&props)?;
                if let Some(out) = &mut out {
                    let mut class = Map::new();
                    if let Some(value) = props.get("className") {
                        class.insert("className".into(), value.clone());
                    }
                    add_class(&mut class, "resume-contact")?;
                    out["attributes"]["className"] = class["className"].clone();
                }
                Ok(out)
            }
            _ => Err("CONTENT_CORE_COMPONENT_UNSUPPORTED".into()),
        }
    }
}

fn compare(a: &Data, b: &Data, op: &str) -> Result<bool, String> {
    if matches!(op, "===" | "!==") {
        let equal = match (a, b) {
            (Data::Missing | Data::Shorted, Data::Missing | Data::Shorted) => true,
            (Data::Json(Value::Number(a)), Data::Json(Value::Number(b))) => {
                a.as_f64() == b.as_f64()
            }
            (Data::Json(Value::String(a)), Data::Json(Value::String(b))) => a == b,
            (Data::Json(Value::Bool(a)), Data::Json(Value::Bool(b))) => a == b,
            (Data::Json(Value::Null), Data::Json(Value::Null)) => true,
            (
                Data::Json(Value::Array(_) | Value::Object(_))
                | Data::Element { .. }
                | Data::List(_)
                | Data::Fragment(_),
                _,
            )
            | (
                _,
                Data::Json(Value::Array(_) | Value::Object(_))
                | Data::Element { .. }
                | Data::List(_)
                | Data::Fragment(_),
            ) => return Err("CONTENT_IDENTITY_COMPARISON_UNSUPPORTED".into()),
            _ => false,
        };
        return Ok(if op == "===" { equal } else { !equal });
    }
    let number = |v: &Data| -> Option<f64> {
        match v {
            Data::Missing | Data::Shorted => Some(f64::NAN),
            Data::Json(Value::Null) => Some(0.0),
            Data::Json(Value::Bool(v)) => Some(if *v { 1.0 } else { 0.0 }),
            Data::Json(Value::Number(n)) => n.as_f64(),
            _ => None,
        }
    };
    let ordering = match (a, b) {
        (Data::Json(Value::Number(a)), Data::Json(Value::Number(b))) => a
            .as_f64()
            .zip(b.as_f64())
            .and_then(|(a, b)| a.partial_cmp(&b)),
        (Data::Json(Value::String(a)), Data::Json(Value::String(b))) => {
            Some(a.encode_utf16().cmp(b.encode_utf16()))
        }
        _ => {
            if let Some((a, b)) = number(a).zip(number(b)) {
                a.partial_cmp(&b)
            } else {
                return Err("CONTENT_RELATIONAL_COERCION_UNSUPPORTED".into());
            }
        }
    };
    let Some(ordering) = ordering else {
        return Ok(false);
    };
    Ok(match op {
        ">" => ordering.is_gt(),
        ">=" => ordering.is_ge(),
        "<" => ordering.is_lt(),
        "<=" => ordering.is_le(),
        _ => return Err("CONTENT_OPERATOR_UNSUPPORTED".into()),
    })
}

fn ecma_space(c: char) -> bool {
    matches!(c,'\u{0009}'..='\u{000d}'|' '| '\u{00a0}'|'\u{1680}'|'\u{2000}'..='\u{200a}'|'\u{2028}'|'\u{2029}'|'\u{202f}'|'\u{205f}'|'\u{3000}'|'\u{feff}')
}
fn add_class(props: &mut Map<String, Value>, prefix: &str) -> Result<(), String> {
    let class = match props.remove("className") {
        None | Some(Value::Null) | Some(Value::Bool(false)) => String::new(),
        Some(Value::String(s)) => s,
        _ => return Err("CONTENT_CLASS_TYPE_UNSUPPORTED".into()),
    };
    props.insert(
        "className".into(),
        json!(format!("{prefix} {class}").trim_matches(ecma_space)),
    );
    Ok(())
}
fn native(tag: &str, mut attrs: Map<String, Value>, children: Vec<Value>) -> Result<Value, String> {
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
    if !TAGS.contains(&tag) {
        return Err("CONTENT_NATIVE_TAG_UNSUPPORTED".into());
    }
    if matches!(tag, "br" | "hr") && !children.is_empty() {
        return Err("CONTENT_VOID_CHILDREN".into());
    }
    attrs.remove("key");
    const ATTRS: &[&str] = &[
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
    ];
    for (key, value) in &attrs {
        if !ATTRS.contains(&key.as_str()) || value.is_array() || value.is_object() {
            return Err("CONTENT_NATIVE_ATTRIBUTE_UNSUPPORTED".into());
        }
        if key == "href" && !value.is_null() {
            let url = value.as_str().ok_or("CONTENT_URL_TYPE_UNSUPPORTED")?;
            if url.chars().any(char::is_control)
                || url.split_once(':').is_some_and(|(scheme, _)| {
                    !matches!(
                        scheme.to_ascii_lowercase().as_str(),
                        "https" | "http" | "mailto" | "tel" | "sms" | "ftp"
                    )
                })
            {
                return Err("CONTENT_UNSAFE_URL".into());
            }
        }
    }
    let mut normalized = Map::new();
    for (key, value) in attrs {
        if value.is_null() {
            continue;
        }
        if value.is_boolean() && !key.starts_with("aria-") && !key.starts_with("data-") {
            continue;
        }
        normalized.insert(key, Value::String(Data::Json(value).text()?));
    }
    Ok(json!({"kind":"element","tag":tag,"attributes":normalized,"children":children}))
}

fn pinned(report: &Value, target: &Value) -> Result<&'static str, String> {
    const PINS: [(&str, &str); 4] = [
        (
            "Section",
            "51a0b42151195761af25b97113e9e838b7275054ec74432263e07e185c2c4c05",
        ),
        (
            "SectionTitle",
            "dff130120826d0681f567bf6e0122aa1303dd6bb5e9f6f50dc626e4d57e5f8e5",
        ),
        (
            "DateRange",
            "8e9207af957f8292c520e5f59e70da77386f7b03b1f7093c50833a68297d02ad",
        ),
        (
            "ContactInfo",
            "6774f1d5991f1801849b4114bac802d429509558de507cac64d770eec8a17782",
        ),
    ];
    let Some((kind, sha)) = PINS.into_iter().find(|(kind, _)| target["name"] == *kind) else {
        return Err("CONTENT_UNSUPPORTED_IMPORT_TARGET".into());
    };
    let file = format!("src/primitives/{kind}.jsx");
    if target["packageName"] != "@jsonresume/core"
        || target["packageVersion"] != "0.3.6"
        || target["file"] != file
        || target["fileSha256"] != sha
        || target["declaration"]["name"] != kind
        || target["declaration"]["kind"] != "function"
    {
        return Err("CONTENT_PRIMITIVE_IDENTITY_MISMATCH".into());
    }
    let packages = values(&report["dependencyBundle"], "packages")
        .filter(|p| {
            p["name"] == "@jsonresume/core"
                && p["version"] == "0.3.6"
                && p["sourceSha256"] == target["sourceSha256"]
        })
        .collect::<Vec<_>>();
    if packages.len() != 1
        || values(&packages[0]["sourceSnapshot"], "files")
            .filter(|f| f["path"] == file && f["sha256"] == sha)
            .count()
            != 1
    {
        return Err("CONTENT_PRIMITIVE_SNAPSHOT_MISMATCH".into());
    }
    if kind == "ContactInfo"
        && values(&packages[0]["sourceSnapshot"], "files")
            .filter(|f| {
                f["path"] == "src/utils/security.js"
                    && f["sha256"]
                        == "26834b437953536845f9cb31671d75d672067a32f56d9eb8d9b3faec17ea17a1"
            })
            .count()
            != 1
    {
        return Err("CONTENT_PRIMITIVE_SECURITY_BRIDGE_MISMATCH".into());
    }
    let utils = values(&report["dependencyBundle"], "packages")
        .filter(|p| p["name"] == "@jsonresume/utils" && p["version"] == "0.4.0")
        .collect::<Vec<_>>();
    let helper = match kind {
        "DateRange" => Some((
            "src/dates.js",
            "4fb77d24f9a3ea8e2a2855a92d4d9a9c8599e9a6f81aa23bf75da862265287fd",
        )),
        "ContactInfo" => Some((
            "src/url.js",
            "3fe129a7b00f161ea66e63a66e05336ebc47ce5e37b32c038d1f295a9aa783ce",
        )),
        _ => None,
    };
    if let Some((file, sha)) = helper
        && (utils.len() != 1
            || values(&utils[0]["sourceSnapshot"], "files")
                .filter(|f| f["path"] == file && f["sha256"] == sha)
                .count()
                != 1
            || values(utils[0], "exportsEntries")
                .filter(|e| {
                    e["subpath"]
                        == if kind == "DateRange" {
                            "./dates"
                        } else {
                            "./url"
                        }
                        && e["status"] == "file-present"
                        && e["resolvedFile"] == file
                        && e["sha256"] == sha
                })
                .count()
                != 1)
    {
        return Err("CONTENT_PRIMITIVE_HELPER_MISMATCH".into());
    }
    Ok(kind)
}

pub(crate) fn evaluate(report: &Value, fixture: &Value) -> Result<Value, String> {
    preflight(report, 400_000)?;
    preflight(fixture, 50_000)?;
    if !fixture.is_object() || json_cost(fixture) > 1024 * 1024 {
        return Err("CONTENT_FIXTURE_LIMIT".into());
    }
    let mut results = Vec::new();
    for template in values(report, "templatePrograms") {
        let result: Result<Value, String> = (|| {
            let program = &template["program"];
            if program["syntaxValid"] != true || program["format"] != "resume-one-template-ir-v1" {
                return Err("CONTENT_PROGRAM_SYNTAX_UNSUPPORTED".into());
            }
            let allowed = [
                "UNCOMPILED_STYLE_DEPENDENCY",
                "UNCOMPILED_COMPONENT_DEPENDENCY",
                "UNVERIFIED_URL_ATTRIBUTE",
            ];
            if values(program, "blockers").any(|b| !allowed.iter().any(|a| b["code"] == *a)) {
                return Err("CONTENT_PROGRAM_BLOCKED".into());
            }
            let mut bindings = BTreeMap::new();
            let mut names = BTreeSet::new();
            for b in values(program, "bindings") {
                let id = name(b, "id")?.to_owned();
                if !names.insert(id.clone()) {
                    return Err("CONTENT_DUPLICATE_BINDING".into());
                }
                bindings.insert(id, b);
            }
            let mut eval = Eval {
                report,
                template,
                bindings,
                env: BTreeMap::new(),
                props: json!({"resume":fixture}),
                steps: 0,
                bytes: 0,
                started: Instant::now(),
            };
            eval.prepare("module", 0)?;
            let entry = &program["entry"];
            if entry["kind"] != "function" {
                return Err("CONTENT_ENTRY_UNSUPPORTED".into());
            }
            eval.prepare(name(entry, "scope")?, 0)?;
            let value = eval.expr(&entry["body"], 0)?;
            let mut tree = Vec::new();
            eval.render(value, &mut tree, 0)?;
            eval.tick(0)?;
            Ok(
                json!({"contentEvaluated":true,"tree":tree,"steps":eval.steps,"stylesCompiled":false,"layoutCompiled":false,"moduleInitializationVerified":false}),
            )
        })();
        results.push(match result {
            Ok(value)=>json!({"entryFile":template["entryFile"],"sourceSha256":template["sourceSha256"],"result":value}),
            Err(code)=>json!({"entryFile":template["entryFile"],"sourceSha256":template["sourceSha256"],"contentEvaluated":false,"error":{"code":code}}),
        });
    }
    let out = json!({"format":"resume-one-content-evaluation-v1","entries":results,"executedSourceCode":false,"migrationComplete":false,"visualVerified":false});
    if json_cost(&out) > 8 * 1024 * 1024 {
        return Err("CONTENT_OUTPUT_LIMIT".into());
    }
    struct OutputBudget(usize);
    impl Write for OutputBudget {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0 += bytes.len();
            if self.0 > 8 * 1024 * 1024 {
                return Err(io::Error::other("CONTENT_OUTPUT_LIMIT"));
            }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    serde_json::to_writer(&mut OutputBudget(0), &out).map_err(|_| "CONTENT_OUTPUT_LIMIT")?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(source: &str, fixture: Value) -> Value {
        let program = crate::template_ir::compile("Resume.jsx", source.as_bytes()).unwrap();
        let syntax = crate::syntax::analyze("Resume.jsx", source.as_bytes())
            .unwrap_or_else(|e| panic!("{}", e.message))
            .unwrap();
        let style = crate::style_link::link("Resume.jsx", &syntax, &program).unwrap();
        let report = json!({"templatePrograms":[{"entryFile":"Resume.jsx","program":program,"styleGraph":style}]});
        evaluate(&report, &fixture).unwrap()["entries"][0].clone()
    }
    fn tree(value: &Value) -> &Value {
        assert_eq!(value["result"]["contentEvaluated"], true, "{value:#}");
        &value["result"]["tree"]
    }
    fn error(value: &Value) -> &str {
        assert_eq!(value["contentEvaluated"], false, "{value:#}");
        value["error"]["code"].as_str().unwrap()
    }

    #[test]
    fn missing_defaults_differ_from_null_false_zero_and_empty_string() {
        let source = "export default function Resume({resume}){const {value='default'}=resume;return <p>{value}</p>;}";
        assert_eq!(
            tree(&run(source, json!({})))[0]["children"],
            json!(["default"])
        );
        for value in [Value::Null, json!(false), json!("")] {
            assert_eq!(
                tree(&run(source, json!({"value":value})))[0]["children"],
                json!([])
            );
        }
        assert_eq!(
            tree(&run(source, json!({"value":0})))[0]["children"],
            json!(["0"])
        );
    }

    #[test]
    fn conditions_and_short_circuit_never_evaluate_the_other_branch() {
        let value = run(
            "export default ({resume})=><p>{false && resume.missing.bad}{true || resume.missing.bad}{resume.name ?? 'fallback'}{true ? 'yes' : resume.missing.bad}</p>;",
            json!({}),
        );
        assert_eq!(tree(&value)[0]["children"], json!(["fallback", "yes"]));
        let zero = run(
            "export default ({resume})=><p>{resume.items.length && <b>has items</b>}</p>;",
            json!({"items":[]}),
        );
        assert_eq!(tree(&zero)[0]["children"], json!(["0"]));
    }

    #[test]
    fn optional_chains_preserve_group_boundaries_and_plain_missing_properties() {
        let safe = run(
            "export default ({resume})=><p>{resume.x?.a.b}{resume.x?.map(x=><b>{x}</b>)}</p>;",
            json!({}),
        );
        assert_eq!(tree(&safe)[0]["children"], json!([]));
        let grouped = run(
            "export default ({resume})=><p>{(resume.x?.a).b}</p>;",
            json!({}),
        );
        assert_eq!(error(&grouped), "CONTENT_NULLISH_PROPERTY_ACCESS");
        let plain = run(
            "export default ({resume})=><p>{resume.x?.a.b}</p>;",
            json!({"x":{}}),
        );
        assert_eq!(error(&plain), "CONTENT_NULLISH_PROPERTY_ACCESS");
        let call = run(
            "export default ({resume})=><p>{resume.x.map?.(x=><b>{x}</b>)}</p>;",
            json!({}),
        );
        assert_eq!(error(&call), "CONTENT_NULLISH_ARRAY_RECEIVER");
    }

    #[test]
    fn nested_maps_bind_item_index_and_callback_constants_per_iteration() {
        let value = run(
            "export default ({resume})=><div>{resume.groups.map((group,index)=>{const label=group.name;return <section key={index}><h2>{label}</h2>{group.items.map((line,index)=><p key={index}>{index}{line}</p>)}</section>;})}</div>;",
            json!({"groups":[{"name":"one","items":["a","b"]},{"name":"two","items":["c"]}]}),
        );
        let groups = tree(&value)[0]["children"].as_array().unwrap();
        assert_eq!(groups[0]["children"][0]["children"], json!(["one"]));
        assert_eq!(groups[0]["children"][2]["children"], json!(["1", "b"]));
        assert_eq!(groups[1]["children"][0]["children"], json!(["two"]));
        assert_eq!(groups[1]["children"][1]["children"], json!(["0", "c"]));
        assert_eq!(groups[0]["attributes"], json!({}));
    }

    #[test]
    fn unused_initializers_are_still_eager_but_unused_elements_are_not_rendered() {
        let bad = run(
            "export default ({resume})=>{const unused=resume.x.y;return <p>ok</p>;};",
            json!({}),
        );
        assert_eq!(error(&bad), "CONTENT_NULLISH_PROPERTY_ACCESS");
        let unused = run(
            "export default ()=>{const unused=<p>{{}}</p>;return <b>ok</b>;};",
            json!({}),
        );
        assert_eq!(tree(&unused)[0]["children"], json!(["ok"]));
    }

    #[test]
    fn map_and_join_validate_actual_array_and_scalar_domains() {
        let source = "export default ({resume})=><p>{resume.items.join(' | ')}</p>;";
        assert_eq!(
            tree(&run(source, json!({"items":["a",null,0,false]})))[0]["children"],
            json!(["a |  | 0 | false"])
        );
        assert_eq!(
            error(&run(source, json!({"items":[[]]}))),
            "CONTENT_COMPOSITE_COERCION_UNSUPPORTED"
        );
        assert_eq!(
            error(&run(source, json!({"items":"abc"}))),
            "CONTENT_ARRAY_RECEIVER_REQUIRED"
        );
        assert_eq!(
            error(&run(
                "export default ({resume})=><p>{resume.items.map(x=><b>{x}</b>)}</p>;",
                json!({"items":{}})
            )),
            "CONTENT_ARRAY_RECEIVER_REQUIRED"
        );
    }

    #[test]
    fn scalar_to_string_uses_ecmascript_number_spelling_not_json_serialization() {
        let source = "export default ({resume})=><p>{`${resume.value}`}</p>;";
        for (value, expected) in [
            (json!(-0.0), "0"),
            (json!(1.0), "1"),
            (json!(1e-7), "1e-7"),
            (json!(1e21), "1e+21"),
            (json!(9007199254740993_u64), "9007199254740992"),
            (Value::Null, "null"),
            (json!(false), "false"),
        ] {
            assert_eq!(
                tree(&run(source, json!({"value":value})))[0]["children"],
                json!([expected])
            );
        }
        assert_eq!(
            tree(&run(source, json!({})))[0]["children"],
            json!(["undefined"])
        );
        assert_eq!(
            error(&run(source, json!({"value":{}}))),
            "CONTENT_COMPOSITE_COERCION_UNSUPPORTED"
        );
    }

    #[test]
    fn strict_and_utf16_relational_comparison_do_not_use_json_deep_equality() {
        let source =
            "export default ({resume})=><p>{resume.a === resume.b ? 'same':'different'}</p>;";
        assert_eq!(
            tree(&run(source, json!({"a":0,"b":0.0})))[0]["children"],
            json!(["same"])
        );
        assert_eq!(
            tree(&run(source, json!({"a":null})))[0]["children"],
            json!(["different"])
        );
        assert_eq!(
            error(&run(source, json!({"a":[],"b":[]}))),
            "CONTENT_IDENTITY_COMPARISON_UNSUPPORTED"
        );
        let unicode = run(
            "export default ({resume})=><p>{resume.a < resume.b ? 'before':'after'}</p>;",
            json!({"a":"\u{10000}","b":"\u{e000}"}),
        );
        assert_eq!(tree(&unicode)[0]["children"], json!(["before"]));
    }

    #[test]
    fn strings_have_utf16_length_and_no_inherited_methods_are_synthesized() {
        let value = run(
            "export default ({resume})=><p>{resume.name.length}</p>;",
            json!({"name":"a😀"}),
        );
        assert_eq!(tree(&value)[0]["children"], json!(["3"]));
        let inherited = run(
            "export default ({resume})=><p>{resume.toString}</p>;",
            json!({}),
        );
        assert_eq!(error(&inherited), "CONTENT_INHERITED_PROPERTY_UNSUPPORTED");
    }

    #[test]
    fn styled_content_keeps_style_binding_without_claiming_css_completion() {
        let value = run(
            "import styled from 'styled-components';const Card=styled.section`color:red;`;export default()=> <Card>ok</Card>;",
            json!({}),
        );
        assert_eq!(tree(&value)[0]["tag"], "section");
        assert_eq!(tree(&value)[0]["children"], json!(["ok"]));
        assert_eq!(
            tree(&value)[0]["styleBindings"].as_array().unwrap().len(),
            1
        );
        assert_eq!(value["result"]["stylesCompiled"], false);
    }

    #[test]
    fn json_objects_are_not_children_and_void_tags_cannot_have_content() {
        assert_eq!(
            error(&run(
                "export default ({resume})=><p>{resume.value}</p>;",
                json!({"value":{}})
            )),
            "CONTENT_OBJECT_CHILD"
        );
        assert_eq!(
            error(&run("export default ()=> <br>wrong</br>;", json!({}))),
            "CONTENT_VOID_CHILDREN"
        );
        for source in [
            "export default ()=> <br>{false}</br>;",
            "export default ()=> <hr>{[]}</hr>;",
            "export default ()=> <hr>{null}{null}</hr>;",
        ] {
            assert_eq!(error(&run(source, json!({}))), "CONTENT_VOID_CHILDREN");
        }
        assert_eq!(
            tree(&run("export default ()=> <br>{null}</br>;", json!({})))[0]["tag"],
            "br"
        );
    }

    #[test]
    fn urls_attributes_and_unknown_source_calls_fail_without_partial_tree() {
        let value = run(
            "export default ({resume})=><div><p>prefix</p><a href={resume.url}>link</a></div>;",
            json!({"url":"javascript:alert(1)"}),
        );
        assert_eq!(error(&value), "CONTENT_UNSAFE_URL");
        assert!(value["result"].is_null());
        let blocked = run(
            "export default ({resume})=><p>{globalThis.run(resume)}</p>;",
            json!({}),
        );
        assert_eq!(error(&blocked), "CONTENT_PROGRAM_BLOCKED");
        let attrs = run(
            "export default ()=> <p title={false} aria-hidden={false}>text</p>;",
            json!({}),
        );
        assert_eq!(
            tree(&attrs)[0]["attributes"],
            json!({"aria-hidden":"false"})
        );
    }

    #[test]
    fn empty_arrays_remain_truthy_and_scalar_arrays_preserve_order() {
        let value = run(
            "export default ({resume})=><p>{resume.values && 'yes'}{resume.values}</p>;",
            json!({"values":[]}),
        );
        assert_eq!(tree(&value)[0]["children"], json!(["yes"]));
        let value = run(
            "export default ({resume})=><p>{resume.values}</p>;",
            json!({"values":["a",0,null,true,"b"]}),
        );
        assert_eq!(tree(&value)[0]["children"], json!(["a", "0", "b"]));
    }

    #[test]
    fn explicit_resource_limits_reject_data_before_unsafe_recursion() {
        let mut deep = Value::Null;
        for _ in 0..100 {
            deep = json!([deep]);
        }
        assert!(
            evaluate(&json!({}), &deep)
                .unwrap_err()
                .contains("INPUT_LIMIT")
        );
        let source = "export default ({resume})=><p>{resume.items.map(x=><span>{x}</span>)}</p>;";
        let value = run(source, json!({"items":vec![0;10_001]}));
        assert_eq!(error(&value), "CONTENT_ARRAY_LIMIT");
    }

    #[test]
    fn fragments_are_elements_not_arrays_and_keys_are_checked_at_creation() {
        let length = run(
            "export default()=>{const x=<><p>A</p></>;return <div>{x.length}</div>;};",
            json!({}),
        );
        assert_eq!(tree(&length)[0]["children"], json!([]));
        let map = run(
            "export default()=>{const x=<><p>A</p></>;return <div>{x.map(a=><b>{a}</b>)}</div>;};",
            json!({}),
        );
        assert_eq!(error(&map), "CONTENT_ARRAY_RECEIVER_REQUIRED");
        let key = run(
            "export default ({resume})=>{const unused=<p key={resume.k}/>;return <b>ok</b>;};",
            json!({"k":{"toString":null}}),
        );
        assert_eq!(error(&key), "CONTENT_KEY_COERCION_UNSUPPORTED");
    }

    #[test]
    fn undefined_optional_lengths_compare_as_nan_not_as_missing_zero() {
        for operator in [">", ">=", "<", "<="] {
            let source = format!(
                "export default ({{resume}})=><p>{{resume.items?.length {operator} 0 ? 'yes':'no'}}</p>;"
            );
            assert_eq!(tree(&run(&source, json!({})))[0]["children"], json!(["no"]));
        }
        let value = run(
            "export default ({resume})=><p>{resume.value >= 0 ? 'yes':'no'}</p>;",
            json!({"value":null}),
        );
        assert_eq!(tree(&value)[0]["children"], json!(["yes"]));
    }

    #[test]
    fn exponentially_referenced_empty_fragments_hit_memory_budget_before_cloning() {
        let mut source = "export default()=>{const a0=<></>;".to_owned();
        for i in 1..24 {
            source.push_str(&format!("const a{i}=<>{{a{}}}{{a{}}}</>;", i - 1, i - 1));
        }
        source.push_str("return <div>{a23}</div>;};");
        let value = run(&source, json!({}));
        assert_eq!(error(&value), "CONTENT_BYTE_LIMIT");
    }
}
