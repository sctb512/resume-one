//! Bounded projection of inspection CSS AST evidence into v2 tokens.
//! No source is read or executed here. Role matching is an inference, not a DOM
//! matcher, stylesheet loader, computed-style engine, or visual certificate.

use std::collections::{BTreeMap, BTreeSet};

use resume_one_core::{layout, parse_theme, theme_style::Rgb};
use serde_json::{Value, json};

const MODULES: [&str; 12] = [
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
const ROLES: [&str; 8] = [
    "body",
    "name",
    "label",
    "summary",
    "sectionTitle",
    "itemTitle",
    "meta",
    "sideDate",
];
const LIMIT: usize = 48_000;

#[derive(Clone, Debug)]
struct Declaration {
    role: &'static str,
    section: Option<&'static str>,
    sidebar: bool,
    container: bool,
    root: bool,
    body_container: bool,
    selector: String,
    property: String,
    value: String,
    // Importance, semantic relevance, simple-selector specificity, source order.
    // This is deliberately a projection policy, NOT the CSS cascade.
    rank: (bool, i32, usize, usize),
    source: Value,
}

#[derive(Default)]
struct Audit {
    evidence: Vec<Value>,
    differences: Vec<Value>,
    seen: usize,
    styled: usize,
    css_files: usize,
    matched: BTreeSet<String>,
}

impl Audit {
    fn difference(&mut self, code: &str, source: &Value, message: &str) {
        self.differences
            .push(json!({"code":code,"source":source,"message":message}));
    }

    fn evidence(&mut self, target: &str, value: &Value, source: &Value, reason: &str) {
        self.evidence
            .push(json!({"target":target,"value":value,"source":source,
            "classification":"adapted","reason":reason,"equivalent":false}));
    }
}

fn normalized(s: &str) -> String {
    s.chars()
        .filter(char::is_ascii_alphanumeric)
        .flat_map(char::to_lowercase)
        .collect()
}

fn section(s: &str) -> Option<&'static str> {
    let s = normalized(s);
    [
        ("volunteer", "volunteer"),
        ("education", "education"),
        ("publication", "publications"),
        ("certificate", "certificates"),
        ("certification", "certificates"),
        ("project", "projects"),
        ("language", "languages"),
        ("interest", "interests"),
        ("reference", "references"),
        ("award", "awards"),
        ("skill", "skills"),
        ("experience", "work"),
        ("employment", "work"),
        ("work", "work"),
    ]
    .into_iter()
    .find_map(|(needle, id)| s.contains(needle).then_some(id))
}

fn bare_role(s: &str) -> Option<&'static str> {
    let n = normalized(s);
    let n = n.strip_prefix("styled").unwrap_or(&n);
    if matches!(
        n,
        "layout"
            | "body"
            | "resume"
            | "resumecontainer"
            | "container"
            | "page"
            | "wrapper"
            | "html"
            | "root"
    ) {
        return Some("body");
    }
    if matches!(n, "main" | "maincontent" | "content" | "maincolumn") {
        return Some("main");
    }
    if matches!(
        n,
        "cardheader" | "entryheader" | "itemheader" | "sectionheader"
    ) {
        return Some("item");
    }
    if matches!(
        n,
        "sidebar" | "leftsidebar" | "rightsidebar" | "aside" | "sidecolumn"
    ) {
        return Some("sidebar");
    }
    if matches!(
        n,
        "header" | "headercontainer" | "masthead" | "resumeheader" | "sidebarheader"
    ) {
        return Some("header");
    }
    if matches!(
        n,
        "name" | "fullname" | "resumename" | "h1" | "title" | "sidebarname"
    ) {
        return Some("name");
    }
    if matches!(
        n,
        "label" | "tagline" | "jobtitle" | "profession" | "subtitle"
    ) {
        return Some("label");
    }
    if n.contains("sectiontitle")
        || n.contains("sectionheading")
        || matches!(n, "h2" | "sidebartitle")
    {
        return Some("sectionTitle");
    }
    if n.contains("date") || n == "time" {
        return Some("sideDate");
    }
    if matches!(
        n,
        "summary" | "summarytext" | "summarysection" | "profiletext" | "about" | "objective"
    ) {
        return Some("summary");
    }
    if n.contains("contact")
        || n.contains("meta")
        || matches!(
            n,
            "company"
                | "workcompany"
                | "degree"
                | "itemsubtitle"
                | "entrysub"
                | "location"
                | "awardawarder"
                | "languagefluency"
        )
    {
        return Some("meta");
    }
    if matches!(n, "position" | "institution" | "h3" | "h4" | "heading")
        || [
            "itemtitle",
            "entrytitle",
            "cardtitle",
            "cardname",
            "entryname",
            "itemname",
            "sectionsubtitle",
            "worktitle",
            "projecttitle",
            "projectname",
            "awardtitle",
            "publicationtitle",
            "skillname",
            "skillcategory",
            "languagename",
        ]
        .contains(&n)
    {
        return Some("itemTitle");
    }
    if n.ends_with("item") || matches!(n, "entry" | "simplecard" | "skillcard" | "card") {
        return Some("item");
    }
    if n.ends_with("section") || n == "section" {
        return Some("section");
    }
    if n.contains("summary")
        || n.contains("description")
        || matches!(
            n,
            "p" | "para" | "paragraph" | "text" | "highlight" | "highlights" | "bullets" | "li"
        )
    {
        return Some("body");
    }
    if MODULES[1..].contains(&n) {
        return Some("section");
    }
    None
}

fn role(s: &str) -> Option<&'static str> {
    if let Some(role) = bare_role(s) {
        return Some(role);
    }
    // A bounded prefix strip supports CSS namespaces such as .pro-name or
    // .resume-label without treating every word ending in "name" as identity.
    for (at, _) in s.match_indices(['-', '_']).take(8) {
        if let Some(role) = bare_role(&s[at + 1..]) {
            return Some(role);
        }
    }
    None
}

fn container_role(s: &str) -> bool {
    let n = normalized(s);
    matches!(
        n.strip_prefix("styled").unwrap_or(&n),
        "html"
            | "body"
            | "root"
            | "layout"
            | "resume"
            | "resumecontainer"
            | "container"
            | "wrapper"
            | "page"
            | "main"
            | "maincontent"
            | "content"
            | "maincolumn"
    )
}

fn container_selectors(css: &Value) -> BTreeSet<String> {
    let mut properties = BTreeMap::<String, BTreeSet<&str>>::new();
    for declaration in css["declarations"].as_array().into_iter().flatten() {
        let Some(scopes) = declaration["scope"].as_array().filter(|s| s.len() == 1) else {
            continue;
        };
        let Some(selector) = scopes[0].as_str() else {
            continue;
        };
        if !selector.starts_with(['.', '#'])
            || selector[1..].contains(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_')
            || role(&selector[1..]).is_some()
        {
            continue;
        }
        if let Some(property) = declaration["property"].as_str() {
            properties
                .entry(selector.into())
                .or_default()
                .insert(property);
        }
    }
    properties
        .into_iter()
        .filter_map(|(selector, properties)| {
            (properties.contains("font-family")
                && properties.contains("font-size")
                && (properties.contains("max-width") || properties.contains("width"))
                && properties.iter().any(|p| p.starts_with("padding")))
            .then_some(selector)
        })
        .collect()
}

fn side_column_selectors(css: &Value) -> BTreeSet<String> {
    let mut candidates = BTreeMap::<String, BTreeMap<String, (bool, String)>>::new();
    for d in css["declarations"].as_array().into_iter().flatten() {
        let Some(scope) = d["scope"].as_array().filter(|s| s.len() == 1) else {
            continue;
        };
        let Some(selector) = scope[0].as_str() else {
            continue;
        };
        let Some(value) = css_value(d) else { continue };
        let Some(property) = d["property"].as_str() else {
            continue;
        };
        for selector in selector.split(',').map(str::trim) {
            if !selector.starts_with(['.', '#'])
                || selector[1..]
                    .contains(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_')
            {
                continue;
            }
            let entry = candidates.entry(selector.into()).or_default();
            let important = d["important"] == true;
            if entry
                .get(property)
                .is_none_or(|(old, _)| important || !*old)
            {
                entry.insert(property.into(), (important, value.clone()));
            }
        }
    }
    candidates
        .into_iter()
        .filter_map(|(s, values)| {
            let area = values
                .get("grid-area")
                .is_some_and(|(_, v)| matches!(v.as_str(), "left" | "right" | "sidebar"));
            let width = values
                .get("width")
                .or_else(|| values.get("flex-basis"))
                .or_else(|| values.get("min-width"))
                .and_then(|(_, v)| v.strip_suffix('%'))
                .and_then(number)
                .is_some_and(|n| (20.0..=40.0).contains(&n));
            (area && width).then_some(s)
        })
        .collect()
}

/// Only simple static selector chains are projected. Pseudo classes/elements,
/// attributes and sibling combinators never leak into their ancestor's role.
fn selector_role(selector: &str) -> Option<(&'static str, usize)> {
    if selector.contains([':', '[', ']', '+', '~', '\\', '(', ')', '*']) {
        return None;
    }
    let last = selector
        .split([' ', '\t', '\n', '>'])
        .rfind(|s| !s.is_empty())?;
    if last == "&" {
        return None;
    }
    let tokens = last
        .split(['.', '#', '&'])
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();
    let found = tokens.iter().rev().find_map(|s| role(s))?;
    let specificity = selector.bytes().filter(|b| *b == b'#').count() * 100
        + selector.bytes().filter(|b| *b == b'.').count() * 10
        + tokens.len();
    Some((found, specificity))
}

fn snippet(value: &Value) -> Option<&str> {
    if value["truncated"] == true {
        None
    } else {
        value.as_str().or_else(|| value["text"].as_str())
    }
}

fn without_comments(s: &str) -> Option<String> {
    let mut output = String::new();
    let mut chars = s.chars().peekable();
    let mut quote = None;
    while let Some(c) = chars.next() {
        if let Some(q) = quote {
            output.push(c);
            if c == '\\' {
                output.push(chars.next()?);
            } else if c == q {
                quote = None;
            }
        } else if c == '\'' || c == '"' {
            quote = Some(c);
            output.push(c);
        } else if c == '/' && chars.peek() == Some(&'*') {
            chars.next();
            loop {
                let c = chars.next()?;
                if c == '*' && chars.peek() == Some(&'/') {
                    chars.next();
                    break;
                }
            }
            // Keep token boundaries: `12/**/pt` must not become `12pt`.
            output.push(' ');
        } else {
            output.push(c);
        }
    }
    quote.is_none().then_some(output)
}

fn css_value(declaration: &Value) -> Option<String> {
    let raw = without_comments(snippet(&declaration["value"])?)?;
    let raw = raw.trim();
    let raw = if declaration["important"] == true {
        let at = raw.rfind('!')?;
        if raw[at + 1..].trim().eq_ignore_ascii_case("important") {
            &raw[..at]
        } else {
            return None;
        }
    } else {
        raw
    };
    Some(raw.trim().to_owned())
}

fn root_variables(file: &Value) -> BTreeMap<String, String> {
    let mut variables = BTreeMap::<String, (bool, String)>::new();
    let css_blocks = std::iter::once(&file["styles"]).chain(
        file["styledComponents"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|style| {
                style["base"]["kind"] == "global"
                    && style["topLevelBinding"] == true
                    && style["templateEscapesRequireDecoding"] != true
                    && style["dynamicExpressions"]
                        .as_array()
                        .is_some_and(Vec::is_empty)
            })
            .map(|style| &style["css"]),
    );
    for d in css_blocks.flat_map(|css| css["declarations"].as_array().into_iter().flatten()) {
        let scopes = d["scope"].as_array();
        if scopes.is_some_and(|s| s.len() == 1 && matches!(s[0].as_str(), Some(":root" | "html")))
            && let Some(p) = d["property"].as_str().filter(|p| p.starts_with("--"))
            && let Some(value) = css_value(d)
        {
            let important = d["important"] == true;
            if variables.get(p).is_none_or(|old| important || !old.0) {
                variables.insert(p.into(), (important, value));
            }
        }
    }
    variables.into_iter().map(|(k, (_, v))| (k, v)).collect()
}

/// Literal :root variables in the same CSS/JS file only. No CSS/JS expression
/// interpreter, imports, scoped custom-property inheritance, or calc evaluator.
fn resolve_vars(input: &str, vars: &BTreeMap<String, String>, depth: usize) -> Option<String> {
    if depth > 16 || input.len() > 8192 {
        return None;
    }
    let Some(start) = input.find("var(") else {
        return Some(input.to_owned());
    };
    let tail = &input[start + 4..];
    let mut nesting = 1;
    let mut comma = None;
    let mut end = None;
    for (at, c) in tail.char_indices() {
        match c {
            '(' => nesting += 1,
            ')' => {
                nesting -= 1;
                if nesting == 0 {
                    end = Some(at);
                    break;
                }
            }
            ',' if nesting == 1 && comma.is_none() => comma = Some(at),
            _ => (),
        }
    }
    let end = end?;
    let key = tail[..comma.unwrap_or(end)].trim();
    let replacement = if let Some(value) = vars.get(key) {
        value.as_str()
    } else {
        tail.get(comma? + 1..end)?.trim()
    };
    let replacement = resolve_vars(replacement, vars, depth + 1)?;
    resolve_vars(
        &format!("{}{}{}", &input[..start], replacement, &tail[end + 1..]),
        vars,
        depth + 1,
    )
}

fn collect_css(
    css: &Value,
    file: &str,
    component: Option<&str>,
    base: Option<&str>,
    vars: &BTreeMap<String, String>,
    declarations: &mut Vec<Declaration>,
    audit: &mut Audit,
) -> Result<(), String> {
    let containers = container_selectors(css);
    let side_columns = side_column_selectors(css);
    let partial = css["syntaxValid"] != true;
    if partial {
        audit.difference(
            "CSS_SYNTAX_UNSUPPORTED",
            &json!({"file":file,"component":component,"errors":css["errors"]}),
            "样式块含解析器未支持语法；仅考虑可独立重新解析的声明，原作用域与完整级联不作等价证明",
        );
    }
    for directive in css["directives"].as_array().into_iter().flatten() {
        audit.difference(
            "CSS_DIRECTIVE_UNSUPPORTED",
            directive,
            "指令和外部样式未加载或执行",
        );
    }
    for d in css["declarations"].as_array().into_iter().flatten() {
        audit.seen += 1;
        if audit.seen > LIMIT {
            return Err("STYLE_ADAPT_LIMIT: more than 48000 declarations".into());
        }
        let source = json!({"file":file,"component":component,"declaration":d});
        let Some(property) = d["property"].as_str() else {
            audit.difference("CSS_DECLARATION_INVALID", &source, "缺少 AST property");
            continue;
        };
        let Some(value) = css_value(d) else {
            audit.difference("CSS_VALUE_INCOMPLETE", &source, "值被截断或无法静态读取");
            continue;
        };
        if partial {
            let isolated = format!("a{{{property}:{value};}}");
            let Ok(Some(parsed)) =
                crate::syntax::analyze("<partial-declaration>.css", isolated.as_bytes())
            else {
                continue;
            };
            if parsed["styles"]["syntaxValid"] != true {
                audit.difference(
                    "CSS_PARTIAL_DECLARATION_REJECTED",
                    &source,
                    "独立声明也不能完整解析，未采用",
                );
                continue;
            }
        }
        let Some(scopes) = d["scope"].as_array() else {
            audit.difference("CSS_SCOPE_INVALID", &source, "缺少 AST scope，未假定为全局");
            continue;
        };
        if scopes.iter().any(|s| s.as_str().is_none()) {
            audit.difference("CSS_SCOPE_INVALID", &source, "非字符串作用域");
            continue;
        }
        let scope = scopes.iter().filter_map(Value::as_str).collect::<Vec<_>>();
        let page_rule = component.is_none() && scope.as_slice() == ["@page"];
        if !page_rule && scope.iter().any(|s| s.starts_with('@')) {
            audit.difference("CONDITIONAL_RULE_UNSUPPORTED", &source, "media/supports/layer/keyframes 等规则不合并到无条件样式；包含 print 在内的条件差异须审核");
            continue;
        }
        if property.starts_with("--") {
            audit.evidence(
                "same-file-root-variable",
                &json!(value),
                &source,
                "仅同文件 :root/html 的字面变量可用于 var()；局部变量不会提升为全局",
            );
            if component.is_some() || !(scope.len() == 1 && matches!(scope[0], ":root" | "html")) {
                audit.difference(
                    "SCOPED_VARIABLE_UNSUPPORTED",
                    &source,
                    "未实现局部自定义属性的级联与继承",
                );
            }
            continue;
        }
        let resolved = resolve_vars(&value, vars, 0);
        let Some(value_resolved) = resolved else {
            audit.difference(
                "CSS_VARIABLE_UNRESOLVED",
                &source,
                "变量未提供、循环、超限或需要局部级联",
            );
            continue;
        };
        if value_resolved != value {
            audit.evidence(
                "resolved-css-value",
                &json!(value_resolved),
                &source,
                "采用同文件无条件根变量；不证明运行时自定义属性等价",
            );
        }
        let selectors = if scope.is_empty() {
            vec![""]
        } else {
            scope.last().unwrap().split(',').map(str::trim).collect()
        };
        for selector in selectors {
            let full = scope.join(" ");
            let inferred_container = containers.contains(selector);
            let matched = if page_rule {
                Some(("page", 0))
            } else if selector == ":root" {
                Some(("body", 1))
            } else if inferred_container {
                Some(("body", 10))
            } else if side_columns.contains(selector) {
                Some(("sidebar", 100))
            } else if scope.is_empty() || selector == "&" {
                component
                    .and_then(role)
                    .or_else(|| base.and_then(role))
                    .map(|r| (r, 20))
                    .or_else(|| component.map(|_| ("entrySource", 20)))
            } else {
                selector_role(selector)
            };
            let Some((mut role, specificity)) = matched else {
                audit.difference(
                    "SELECTOR_ROLE_UNRESOLVED",
                    &source,
                    "选择器或组件没有可用的静态角色映射，未把子节点样式应用于父角色",
                );
                continue;
            };
            let last = selector.split_whitespace().last().unwrap_or(selector);
            let ancestor_header = scope.iter().any(|s| {
                s.split([' ', '>', '.', '#']).any(|token| {
                    matches!(
                        normalized(token).as_str(),
                        "header" | "masthead" | "resumeheader"
                    )
                })
            });
            if last == "h2" && ancestor_header {
                role = "label";
            }
            let module_ancestor = scope
                .iter()
                .flat_map(|s| s.split([' ', '>', '.', '#']))
                .any(|token| {
                    matches!(normalized(token).as_str(), "section" | "list")
                        || MODULES[1..].contains(&normalized(token).as_str())
                });
            let in_side_column = scope
                .iter()
                .any(|s| s.split([' ', '>']).any(|s| side_columns.contains(s)));
            if matches!(last, "h1" | "h2")
                && (module_ancestor || in_side_column)
                && !ancestor_header
            {
                role = "sectionTitle";
            }
            if role == "header" && module_ancestor {
                role = "item";
            }
            if let Some(level) = css["resumeOneModuleHeadingTag"].as_str() {
                if last == level && !ancestor_header {
                    role = "sectionTitle";
                }
            }
            // Check every ancestor too: e.g. :hover h1 and @media were not made unconditional.
            if !page_rule
                && scope
                    .iter()
                    .take(scope.len().saturating_sub(1))
                    .any(|s| s.contains([':', '[', '+', '~', '*', '\\']))
            {
                audit.difference(
                    "SELECTOR_CONTEXT_UNSUPPORTED",
                    &source,
                    "祖先选择器依赖状态、属性或复杂匹配",
                );
                continue;
            }
            let label = format!("{} {}", component.unwrap_or(""), full);
            let sidebar =
                normalized(&label).contains("sidebar") || role == "sidebar" || in_side_column;
            if side_columns.contains(selector) {
                audit.evidence("side-column-inferred", &json!(selector), &source, "单一容器同时声明侧栏网格区名与 20–40% 列宽；仅凭 left/right 名称或对齐不推断整页侧栏");
            }
            let section = section(&label);
            if section.is_some() && role == "name" && normalized(selector) != "h1" {
                role = "itemTitle";
            }
            let relevance = if sidebar {
                0
            } else if section.is_some() {
                10
            } else {
                20
            };
            audit.matched.insert(role.to_owned());
            let container = page_rule
                || selector == ":root"
                || inferred_container
                || if scope.is_empty() || selector == "&" {
                    component.is_some_and(container_role)
                } else {
                    selector
                        .split([' ', '>', '.', '#'])
                        .filter(|s| !s.is_empty())
                        .last()
                        .is_some_and(container_role)
                };
            if inferred_container {
                audit.evidence("document-container-inferred", &json!(selector), &source, "单一选择器同时声明字体、字号、宽度与页面内距，推断为文档容器；未把子段落内距当成纸面边距");
            }
            let mut decl = Declaration {
                role,
                section,
                sidebar,
                container,
                root: matches!(selector, "html" | ":root")
                    || (scope.is_empty()
                        && component
                            .is_some_and(|c| matches!(normalized(c).as_str(), "html" | "root"))),
                body_container: selector == "body"
                    || (scope.is_empty() && component.is_some_and(|c| normalized(c) == "body")),
                selector: selector.to_owned(),
                property: property.to_ascii_lowercase(),
                value: value_resolved.clone(),
                rank: (d["important"] == true, relevance, specificity, audit.seen),
                source: source.clone(),
            };
            // Expand only the bounded box shorthand; longhands then resolve in source order.
            if matches!(decl.property.as_str(), "padding" | "margin") {
                if let Some(parts) =
                    css_words(&decl.value, false).filter(|p| (1..=4).contains(&p.len()))
                {
                    let four = match parts.as_slice() {
                        [a] => [*a, *a, *a, *a],
                        [a, b] => [*a, *b, *a, *b],
                        [a, b, c] => [*a, *b, *c, *b],
                        [a, b, c, d] => [*a, *b, *c, *d],
                        _ => unreachable!(),
                    };
                    let prefix = decl.property.clone();
                    for (side, value) in ["top", "right", "bottom", "left"].into_iter().zip(four) {
                        let mut part = decl.clone();
                        part.property = format!("{prefix}-{side}");
                        part.value = value.into();
                        declarations.push(part);
                    }
                    continue;
                }
            }
            if decl.property == "font" {
                if let Some(parts) = font_shorthand(&decl.value) {
                    for (property, value) in parts {
                        let mut part = decl.clone();
                        part.property = property;
                        part.value = value;
                        declarations.push(part);
                    }
                    audit.evidence(
                        "font-longhands",
                        &json!(decl.value),
                        &source,
                        "受限字面 font 简写展开；未模拟系统字体或所有 reset 属性",
                    );
                    continue;
                }
                decl.property = "unsupported-font-shorthand".into();
            }
            declarations.push(decl);
        }
    }
    Ok(())
}

fn font_shorthand(input: &str) -> Option<Vec<(String, String)>> {
    let parts = css_words(input, true)?;
    let at = parts
        .iter()
        .position(|p| length_pt(p, 12.0, 12.0).is_some() && p.chars().any(char::is_alphabetic))?;
    let mut result = vec![
        ("font-style".into(), "normal".into()),
        ("font-weight".into(), "400".into()),
    ];
    for p in &parts[..at] {
        let key = if matches!(*p, "italic" | "normal") {
            "font-style"
        } else if *p == "bold" || p.parse::<u16>().is_ok() {
            "font-weight"
        } else {
            return None;
        };
        result.push((key.into(), (*p).into()));
    }
    result.push(("font-size".into(), parts[at].into()));
    let mut family_at = at + 1;
    if parts.get(family_at) == Some(&"/") {
        result.push(("line-height".into(), (*parts.get(family_at + 1)?).into()));
        family_at += 2;
    } else {
        result.push(("line-height".into(), "normal".into()));
    }
    if family_at >= parts.len() {
        return None;
    }
    result.push(("font-family".into(), parts[family_at..].join(" ")));
    Some(result)
}

/// Token boundaries outside literal functions/quotes only; never split a calc
/// expression into unrelated box sides or alter quoted font-family contents.
fn css_words(input: &str, slash: bool) -> Option<Vec<&str>> {
    if input.len() > 8192 {
        return None;
    }
    let mut result = Vec::new();
    let mut start = None;
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    for (at, c) in input.char_indices() {
        if let Some(q) = quote {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == q {
                quote = None;
            }
            continue;
        }
        if matches!(c, '\'' | '"') {
            quote = Some(c);
            start.get_or_insert(at);
            continue;
        }
        if c == '(' {
            depth += 1;
            if depth > 32 {
                return None;
            }
        }
        if c == ')' {
            depth = depth.checked_sub(1)?;
        }
        if depth == 0 && (c.is_ascii_whitespace() || (slash && c == '/')) {
            if let Some(begin) = start.take() {
                result.push(&input[begin..at]);
            }
            if c == '/' {
                result.push(&input[at..at + 1]);
            }
            if result.len() > 32 {
                return None;
            }
        } else {
            start.get_or_insert(at);
        }
    }
    if quote.is_some() || depth != 0 {
        return None;
    }
    if let Some(begin) = start {
        result.push(&input[begin..]);
    }
    (result.len() <= 32).then_some(result)
}

fn number(s: &str) -> Option<f64> {
    s.trim().parse::<f64>().ok().filter(|n| n.is_finite())
}

fn font_families(input: &str) -> Option<Vec<String>> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut quote = None;
    for (at, c) in input.char_indices() {
        if c == '\\' {
            return None;
        }
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
        } else if matches!(c, '\'' | '"') {
            quote = Some(c);
        } else if c == ',' {
            parts.push(&input[start..at]);
            start = at + 1;
        }
    }
    if quote.is_some() {
        return None;
    }
    parts.push(&input[start..]);
    parts
        .into_iter()
        .map(|p| {
            let p = p.trim();
            if p.starts_with(['\'', '"']) {
                let first = p.chars().next()?;
                if !p.ends_with(first) || p.len() < 2 {
                    return None;
                }
                let inner = &p[1..p.len() - 1];
                if inner.contains(first) {
                    return None;
                }
                Some(inner.to_owned())
            } else if p.is_empty() || p.contains(['\'', '"', '(', ')', ';']) {
                None
            } else {
                Some(p.to_owned())
            }
        })
        .collect()
}

fn length_pt(s: &str, em: f64, rem: f64) -> Option<f64> {
    crate::css_numeric::length_pt(s, em, rem)
}

fn color(s: &str) -> Option<String> {
    let s = s.trim().to_ascii_lowercase();
    if let Some(hex) = s.strip_prefix('#') {
        if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        return match hex.len() {
            3 => Some(
                format!("#{}", hex.chars().flat_map(|c| [c, c]).collect::<String>())
                    .to_ascii_uppercase(),
            ),
            4 if &hex[3..] == "f" => color(&format!("#{}", &hex[..3])),
            6 => Some(format!("#{hex}").to_ascii_uppercase()),
            8 if &hex[6..] == "ff" => color(&format!("#{}", &hex[..6])),
            _ => None,
        };
    }
    let named = match s.as_str() {
        "black" => "#000000",
        "white" => "#FFFFFF",
        "gray" | "grey" => "#808080",
        "silver" => "#C0C0C0",
        "red" => "#FF0000",
        "maroon" => "#800000",
        "navy" => "#000080",
        "blue" => "#0000FF",
        "teal" => "#008080",
        "green" => "#008000",
        "lime" => "#00FF00",
        "olive" => "#808000",
        "purple" => "#800080",
        "fuchsia" => "#FF00FF",
        "aqua" => "#00FFFF",
        "yellow" => "#FFFF00",
        "orange" => "#FFA500",
        "rebeccapurple" => "#663399",
        "darkgray" | "darkgrey" => "#A9A9A9",
        "lightgray" | "lightgrey" => "#D3D3D3",
        "whitesmoke" => "#F5F5F5",
        "transparent" | "currentcolor" => return None,
        _ => "",
    };
    if !named.is_empty() {
        return Some(named.into());
    }
    let body = s
        .strip_prefix("rgb(")
        .or_else(|| s.strip_prefix("rgba("))?
        .strip_suffix(')')?;
    let parts = body
        .split([',', '/', ' ', '\t', '\n'])
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();
    if !(3..=4).contains(&parts.len()) {
        return None;
    }
    if parts.len() == 4 {
        let a = parts[3];
        if a.strip_suffix('%')
            .and_then(number)
            .map(|v| v / 100.0)
            .or_else(|| number(a))?
            != 1.0
        {
            return None;
        }
    }
    let mut rgb = [0u8; 3];
    for (target, p) in rgb.iter_mut().zip(parts.iter()) {
        let v = if let Some(p) = p.strip_suffix('%') {
            let n = number(p)?;
            if !(0.0..=100.0).contains(&n) {
                return None;
            }
            n * 255.0 / 100.0
        } else {
            let n = number(p)?;
            if !(0.0..=255.0).contains(&n) {
                return None;
            }
            n
        };
        *target = v.round() as u8;
    }
    Some(format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2]))
}

fn rgb(hex: &str) -> Rgb {
    Rgb {
        r: u8::from_str_radix(&hex[1..3], 16).unwrap(),
        g: u8::from_str_radix(&hex[3..5], 16).unwrap(),
        b: u8::from_str_radix(&hex[5..7], 16).unwrap(),
    }
}

fn put(value: &mut Value, path: &str, new: Value) {
    let mut current = value;
    let parts = path.trim_start_matches('/').split('/').collect::<Vec<_>>();
    for key in &parts[..parts.len() - 1] {
        if !current[*key].is_object() {
            current[*key] = json!({});
        }
        current = current.get_mut(*key).expect("created object");
    }
    current[parts[parts.len() - 1]] = new;
}

fn write(
    theme: &mut Value,
    audit: &mut Audit,
    path: &str,
    value: Value,
    d: &Declaration,
    reason: &str,
) {
    put(theme, path, value.clone());
    audit.evidence(path, &value, &d.source, reason);
}

#[allow(clippy::too_many_arguments)] // Kept explicit at each schema-bound property projection.
fn bounded(
    theme: &mut Value,
    audit: &mut Audit,
    path: &str,
    value: Option<f64>,
    min: f64,
    max: f64,
    safe: f64,
    d: &Declaration,
    reason: &str,
) {
    let value = match value {
        Some(v) if v.is_finite() && (min..=max).contains(&v) => v,
        _ => {
            audit.difference(
                "NUMERIC_VALUE_UNREPRESENTABLE",
                &d.source,
                &format!("{path} 需要 {min}..={max}，采用安全值 {safe}；没有 clamp 来源数值"),
            );
            safe
        }
    };
    write(theme, audit, path, json!(value), d, reason);
}

fn choose(
    declarations: &[Declaration],
    audit: &mut Audit,
) -> BTreeMap<(String, String), Declaration> {
    let mut selected = BTreeMap::<(String, String), Declaration>::new();
    for d in declarations {
        if d.body_container {
            let key = ("body-container".to_owned(), d.property.clone());
            if selected.get(&key).is_none_or(|old| d.rank >= old.rank) {
                selected.insert(key, d.clone());
            }
        }
        if d.root {
            let key = ("root".to_owned(), d.property.clone());
            if selected.get(&key).is_none_or(|old| d.rank >= old.rank) {
                selected.insert(key, d.clone());
            }
        }
        if d.role == "body" && d.container {
            let key = ("document".to_owned(), d.property.clone());
            let depth = |v: &Declaration| {
                if v.root {
                    0
                } else if v.body_container {
                    1
                } else {
                    2
                }
            };
            if selected
                .get(&key)
                .is_none_or(|old| (depth(d), d.rank) >= (depth(old), old.rank))
            {
                selected.insert(key, d.clone());
            }
        }
        let key = (d.role.to_owned(), d.property.clone());
        if let Some(old) = selected.get(&key) {
            if d.role == "body" && d.root != old.root {
                if d.root {
                    continue;
                }
                selected.insert(key, d.clone());
                continue;
            }
            if old.value != d.value {
                audit.difference("COLLAPSED_ROLE_CONFLICT", &json!({"previous":old.source,"next":d.source,
                    "selected":if d.rank >= old.rank {&d.source} else {&old.source}}),
                    "多个声明投影到同一 v2 角色；按 important、主栏/通用角色、简单选择器、检查顺序择一；不是完整 CSS 级联");
            }
            if d.rank < old.rank {
                continue;
            }
        }
        selected.insert(key, d.clone());
    }
    selected
}

fn font_context(selected: &BTreeMap<(String, String), Declaration>) -> (f64, f64, f64) {
    let root = selected
        .get(&("root".into(), "font-size".into()))
        .and_then(|d| length_pt(&d.value, 12.0, 12.0))
        .filter(|v| (0.1..=96.0).contains(v))
        .unwrap_or(12.0);
    let body = selected
        .get(&("body-container".into(), "font-size".into()))
        .and_then(|d| length_pt(&d.value, root, root))
        .filter(|v| (0.1..=96.0).contains(v))
        .unwrap_or(root);
    let document = selected
        .get(&("document".into(), "font-size".into()))
        .and_then(|d| {
            if d.root {
                Some(root)
            } else if d.body_container {
                Some(body)
            } else {
                length_pt(&d.value, body, root)
            }
        })
        .filter(|v| (0.1..=96.0).contains(v))
        .unwrap_or(body);
    (root, body, document)
}

fn text_styles(
    theme: &mut Value,
    selected: &BTreeMap<(String, String), Declaration>,
    audit: &mut Audit,
) {
    let (root_pt, body_root_pt, document_pt) = font_context(selected);
    if let Some(d) = selected.get(&("root".into(), "font-size".into()))
        && !length_pt(&d.value, 12.0, 12.0).is_some_and(|v| (0.1..=96.0).contains(&v))
    {
        audit.difference(
            "ROOT_FONT_SIZE_CONTEXT_UNRESOLVED",
            &d.source,
            "根字号超出受支持的静态上下文范围，采用 12pt 初始根字号；相对尺寸不标为精确",
        );
    }
    audit.evidence(
        "font-size-context",
        &json!({"rootSizePt":root_pt,"documentSizePt":document_pt,"initialRootAssumptionPt":12}),
        &Value::Null,
        "根字号独立于局部段落字号；相对字号使用对应根/推断文档父字号，尚不计算任意 DOM 祖先",
    );
    for role in ROLES {
        let own = |p: &str| selected.get(&(role.into(), p.into()));
        let size_path = format!("/typography/{role}/sizePt");
        let old_size = theme
            .pointer(&size_path)
            .and_then(Value::as_f64)
            .unwrap_or(10.0);
        if own("font-size").is_none()
            && matches!(role, "label" | "summary")
            && let Some(d) = selected.get(&("document".into(), "font-size".into()))
        {
            bounded(
                theme,
                audit,
                &size_path,
                Some(document_pt),
                7.0,
                42.0,
                old_size,
                d,
                "从 Layout 正文字号推断摘要/副标题的字号继承",
            );
        }
        if let Some(d) = own("font-size") {
            bounded(
                theme,
                audit,
                &size_path,
                if d.root {
                    Some(root_pt)
                } else if d.body_container {
                    Some(body_root_pt)
                } else if d.container {
                    Some(document_pt)
                } else {
                    length_pt(&d.value, document_pt, root_pt)
                },
                7.0,
                42.0,
                old_size,
                d,
                "CSS px=0.75pt；相对字号分别使用根字号和推断的文档父字号，局部段落不改变其它角色的父字号",
            );
        }
        let size = theme
            .pointer(&size_path)
            .and_then(Value::as_f64)
            .unwrap_or(old_size);
        for property in [
            "font-family",
            "font-weight",
            "font-style",
            "line-height",
            "letter-spacing",
            "text-transform",
            "color",
        ] {
            let inherited = own(property).is_none() && role != "body";
            let Some(d) =
                own(property).or_else(|| selected.get(&("document".into(), property.into())))
            else {
                continue;
            };
            let prefix = format!("/typography/{role}/");
            let reason = if inherited {
                "从正文角色推断继承；没有计算 DOM 祖先及 UA 样式"
            } else {
                "将静态声明映射到 v2 文字角色"
            };
            match property {
                "font-family" => {
                    let families = font_families(&d.value)
                        .unwrap_or_default()
                        .into_iter()
                        .map(|s| s.to_ascii_lowercase())
                        .collect::<Vec<_>>();
                    let serif = families.iter().any(|s| {
                        matches!(
                            s.as_str(),
                            "serif"
                                | "georgia"
                                | "times new roman"
                                | "times"
                                | "merriweather"
                                | "palatino"
                                | "cjk-serif"
                        )
                    });
                    let family = if serif { "cjk-serif" } else { "cjk-sans" };
                    write(
                        theme,
                        audit,
                        &format!("{prefix}family"),
                        json!(family),
                        d,
                        reason,
                    );
                    audit.difference(
                        "FONT_FAMILY_FALLBACK",
                        &d.source,
                        &format!(
                            "原 font-family: {}；显式采用 {family}，未声称字形、字宽或字体资产等价",
                            d.value
                        ),
                    );
                }
                "font-weight" => {
                    let n = match d.value.as_str() {
                        "normal" => Some(400),
                        "bold" => Some(700),
                        _ => d.value.parse::<u64>().ok(),
                    };
                    let n = match n {
                        Some(n) if (100..=900).contains(&n) && n % 100 == 0 => n,
                        _ => {
                            audit.difference(
                                "FONT_WEIGHT_UNREPRESENTABLE",
                                &d.source,
                                "v2 仅支持 100 到 900 的整百字重，采用 400",
                            );
                            400
                        }
                    };
                    write(
                        theme,
                        audit,
                        &format!("{prefix}weight"),
                        json!(n),
                        d,
                        reason,
                    );
                }
                "font-style" => {
                    if matches!(d.value.as_str(), "normal" | "italic") {
                        write(
                            theme,
                            audit,
                            &format!("{prefix}fontStyle"),
                            json!(d.value),
                            d,
                            reason,
                        );
                    } else {
                        audit.difference(
                            "FONT_STYLE_UNSUPPORTED",
                            &d.source,
                            "未将 oblique 等斜体参数当作精确 italic",
                        );
                    }
                }
                "line-height" => {
                    let value = number(&d.value)
                        .or_else(|| {
                            d.value
                                .strip_suffix('%')
                                .and_then(number)
                                .map(|n| n / 100.0)
                        })
                        .or_else(|| {
                            length_pt(
                                &d.value,
                                if inherited { document_pt } else { size },
                                root_pt,
                            )
                            .map(|v| v / size)
                        });
                    bounded(
                        theme,
                        audit,
                        &format!("{prefix}lineHeight"),
                        value,
                        1.0,
                        2.0,
                        1.4,
                        d,
                        reason,
                    );
                }
                "letter-spacing" => {
                    let value = if d.value == "normal" {
                        Some(0.0)
                    } else {
                        length_pt(
                            &d.value,
                            if inherited { document_pt } else { size },
                            root_pt,
                        )
                        .map(|n| n / size)
                    };
                    bounded(
                        theme,
                        audit,
                        &format!("{prefix}trackingEm"),
                        value,
                        -0.05,
                        0.5,
                        0.0,
                        d,
                        reason,
                    );
                }
                "text-transform" => {
                    if matches!(d.value.as_str(), "uppercase" | "lowercase" | "none") {
                        write(
                            theme,
                            audit,
                            &format!("{prefix}textTransform"),
                            json!(d.value),
                            d,
                            reason,
                        );
                    } else {
                        audit.difference(
                            "TEXT_TRANSFORM_UNSUPPORTED",
                            &d.source,
                            "capitalize/语言相关大小写没有 v2 等价项",
                        );
                    }
                }
                "color" => {
                    if let Some(c) = color(&d.value) {
                        write(theme, audit, &format!("{prefix}color"), json!(c), d, reason);
                    } else {
                        audit.difference(
                            "COLOR_UNSUPPORTED",
                            &d.source,
                            "颜色需要混合背景、变量、渐变或未支持的颜色语法；保留安全候选色",
                        );
                    }
                }
                _ => unreachable!(),
            }
        }
    }
}

fn section_paths(d: &Declaration, field: &str) -> Vec<String> {
    d.section
        .into_iter()
        .chain(MODULES[1..].iter().copied().filter(|_| d.section.is_none()))
        .map(|id| format!("/sections/{id}/{field}"))
        .collect()
}

fn border_color(s: &str) -> Option<String> {
    color(s).or_else(|| css_words(s, false)?.into_iter().rev().find_map(color))
}

fn apply_structure(
    theme: &mut Value,
    declarations: &[Declaration],
    selected: &BTreeMap<(String, String), Declaration>,
    audit: &mut Audit,
) {
    let (root_pt, _, document_pt) = font_context(selected);
    // Resolve per output target, so section-specific decoration is not flattened.
    let mut changes = BTreeMap::<String, (Value, Declaration, &str)>::new();
    for d in declarations {
        let mut proposals = Vec::<(String, Value, &str)>::new();
        let v = d.value.trim();
        let mut handled = false;
        match d.property.as_str() {
            "background" | "background-color" if matches!(d.role, "body" | "main" | "sidebar") => {
                handled = true;
                if let Some(c) = color(v) {
                    proposals.push((
                        format!(
                            "/colors/{}",
                            if d.role == "sidebar" {
                                "sidebar"
                            } else {
                                "paper"
                            }
                        ),
                        json!(c),
                        "静态区域背景投影到纸面或侧栏；未模拟背景叠加",
                    ));
                } else {
                    audit.difference(
                        "BACKGROUND_UNSUPPORTED",
                        &d.source,
                        "渐变、透明度、图片或复杂背景没有等价颜色 token",
                    );
                }
            }
            "color" if d.role == "body" => {
                handled = true;
                if let Some(c) = color(v) {
                    proposals.push(("/colors/ink".into(), json!(c), "正文前景色"));
                }
            }
            "color" if d.role == "sectionTitle" && !d.sidebar => {
                handled = true;
                if let Some(c) = color(v) {
                    proposals.push(("/colors/accent".into(), json!(c), "标题色推断为强调色"));
                }
            }
            "color" if d.role == "meta" && !d.sidebar => {
                handled = true;
                if let Some(c) = color(v) {
                    proposals.push(("/colors/muted".into(), json!(c), "辅助文字色"));
                }
            }
            "text-align" if matches!(d.role, "header" | "name" | "label") => {
                handled = true;
                if matches!(v, "center" | "left") {
                    proposals.push((
                        "/composition/header".into(),
                        json!(if v == "center" { "centered" } else { "stacked" }),
                        "头部文字对齐；未模拟逐节点对齐",
                    ));
                } else {
                    audit.difference(
                        "HEADER_ALIGNMENT_UNSUPPORTED",
                        &d.source,
                        "v2 无右对齐头部或 justify 头部",
                    );
                }
            }
            "text-align"
                if d.role == "sectionTitle" && matches!(v, "left" | "center" | "right") =>
            {
                handled = true;
                for path in section_paths(d, "headingAlign") {
                    proposals.push((path, json!(v), "标题对齐投影"));
                }
            }
            "border"
            | "border-bottom"
            | "border-left"
            | "border-top"
            | "border-color"
            | "border-bottom-color"
            | "border-block-end"
            | "border-block-start"
            | "border-inline-start"
                if matches!(d.role, "sectionTitle" | "header" | "item") =>
            {
                handled = true;
                let none = matches!(v, "none" | "0" | "0px" | "hidden");
                if let Some(c) = border_color(v) {
                    proposals.push((
                        "/colors/border".into(),
                        json!(c),
                        "多个边框共用 v2 border 色",
                    ));
                }
                if d.role == "header" {
                    proposals.push((
                        "/layout".into(),
                        json!(if none { "minimal" } else { "classic" }),
                        "头部边框近似为内置分隔线",
                    ));
                } else if d.role == "sectionTitle" {
                    let heading = if none {
                        "plain"
                    } else if matches!(d.property.as_str(), "border-left" | "border-inline-start") {
                        "accent-line"
                    } else {
                        "line"
                    };
                    for path in section_paths(d, "heading") {
                        proposals.push((path, json!(heading), "边框方向近似为内置标题装饰"));
                    }
                } else if !none {
                    for path in section_paths(d, "items") {
                        proposals.push((
                            path.clone(),
                            json!(if matches!(
                                d.property.as_str(),
                                "border-left" | "border-inline-start"
                            ) && ["work", "volunteer", "education", "projects"]
                                .iter()
                                .any(|id| path == format!("/sections/{id}/items"))
                            {
                                "timeline"
                            } else if matches!(
                                d.property.as_str(),
                                "border-left" | "border-inline-start"
                            ) {
                                "stack"
                            } else {
                                "cards"
                            }),
                            "条目边框近似为 timeline/cards",
                        ));
                    }
                }
                audit.difference(
                    "BORDER_GEOMETRY_APPROXIMATED",
                    &d.source,
                    "边框粗细、线型、精确位置及局部颜色未完整表达",
                );
            }
            "background" | "background-color" if d.role == "sectionTitle" => {
                handled = true;
                if let Some(c) = color(v) {
                    proposals.push(("/colors/accent".into(), json!(c), "标题填充共用 accent 色"));
                    for path in section_paths(d, "heading") {
                        proposals.push((path, json!("filled"), "标题底色近似为 filled 样式"));
                    }
                    audit.difference(
                        "HEADING_FILL_APPROXIMATED",
                        &d.source,
                        "filled 的几何与内置可读文字色不保证与原背景一致",
                    );
                } else {
                    audit.difference(
                        "BACKGROUND_UNSUPPORTED",
                        &d.source,
                        "标题背景不能表示为不透明单色",
                    );
                }
            }
            "float" | "text-align" if d.role == "sideDate" && matches!(v, "left" | "right") => {
                handled = true;
                for path in section_paths(d, "date") {
                    proposals.push((path, json!(v), "日期浮动/对齐推断为日期栏位置"));
                }
            }
            "break-inside" | "page-break-inside"
                if matches!(d.role, "item" | "section" | "body") =>
            {
                handled = true;
                if matches!(v, "avoid" | "avoid-page" | "auto") {
                    proposals.push((
                        "/page/avoidSplitItems".into(),
                        json!(v != "auto"),
                        "条目断页偏好投影到全局页面策略",
                    ));
                } else {
                    audit.difference(
                        "PAGE_BREAK_UNSUPPORTED",
                        &d.source,
                        "该断页值没有等价 v2 策略",
                    );
                }
            }
            "break-after" | "page-break-after" if d.role == "sectionTitle" => {
                handled = true;
                if matches!(v, "avoid" | "avoid-page" | "auto") {
                    proposals.push((
                        "/page/keepHeadingWithFirstItem".into(),
                        json!(v != "auto"),
                        "标题断页偏好投影",
                    ));
                }
            }
            "size" if d.role == "page" => {
                handled = true;
                match v.to_ascii_lowercase().as_str() {
                    "a4" | "a4 portrait" => {
                        proposals.push(("/page/size".into(), json!("A4"), "@page 纸型"))
                    }
                    "letter" | "letter portrait" => {
                        proposals.push(("/page/size".into(), json!("Letter"), "@page 纸型"))
                    }
                    _ => audit.difference(
                        "PAGE_SIZE_UNSUPPORTED",
                        &d.source,
                        "仅 A4/Letter 纵向可表达",
                    ),
                }
            }
            "display" if v == "none" => {
                handled = true;
                audit.difference(
                    "SOURCE_VISIBILITY_NOT_APPLIED",
                    &d.source,
                    "不从模块列表删除任何内容；原模板隐藏的数据仍可显示",
                );
            }
            _ => (),
        }
        if let Some((kind, edge)) = d.property.split_once('-')
            && (kind == "padding" && matches!(d.role, "body" | "main" | "page") && d.container
                || kind == "margin" && d.role == "page")
            && let Some(side) = match edge {
                "top" | "right" | "bottom" | "left" => Some(edge),
                "block-start" => Some("top"),
                "block-end" => Some("bottom"),
                "inline-start" => Some("left"),
                "inline-end" => Some("right"),
                _ => None,
            }
        {
            handled = true;
            if edge != side {
                audit.difference(
                    "LOGICAL_PAGE_EDGE_APPROXIMATED",
                    &d.source,
                    "按当前横排从左到右的文档方向投影到物理页边；不模拟 CSS writing-mode 或 RTL",
                );
            }
            // Page margins and nested document padding are independent boxes;
            // page_box resolves their separate cascades and composes them.
        }
        if matches!(
            d.property.as_str(),
            "margin-bottom" | "margin-top" | "margin-block-start" | "margin-block-end"
        ) && d.role == "section"
        {
            handled = true;
            if d.section.is_some() {
                audit.difference("MODULE_MARGIN_NOT_GLOBALIZED", &d.source, "单一模块的 margin 不提升为所有模块的间距，避免 publication 等局部规则压缩整个文档");
                continue;
            }
            let val = (!v.contains('%'))
                .then(|| length_pt(v, document_pt, root_pt))
                .flatten()
                .map(|n| n * 25.4 / 72.0);
            let val = match val {
                Some(n) if (3.0..=20.0).contains(&n) => n,
                Some(n) if (0.0..3.0).contains(&n) => {
                    audit.difference("COMPACT_SECTION_GAP_APPROXIMATED", &d.source, "原零/极小外边距依赖未迁移的盒模型；共享基线排版采用 4mm 安全间距，避免模块标题覆盖上一模块");
                    4.0
                }
                _ => {
                    audit.difference(
                        "NUMERIC_VALUE_UNREPRESENTABLE",
                        &d.source,
                        "sectionGapMm 超范围或非静态长度，采用 6mm",
                    );
                    6.0
                }
            };
            proposals.push((
                "/page/sectionGapMm".into(),
                json!(val),
                "模块纵向外边距近似统一模块间距；横排方向，未模拟 margin collapse、writing-mode 或上下边距求和",
            ));
        }
        let text_property = [
            "font-family",
            "font-size",
            "font-weight",
            "font-style",
            "line-height",
            "letter-spacing",
            "text-transform",
            "color",
        ]
        .contains(&d.property.as_str());
        if text_property && ROLES.contains(&d.role) {
            handled = true;
        }
        let composition_property = matches!(
            d.property.as_str(),
            "grid-template-columns"
                | "column-gap"
                | "gap"
                | "width"
                | "float"
                | "order"
                | "flex-direction"
        ) && matches!(d.role, "body" | "sidebar" | "header" | "meta");
        if composition_property {
            handled = true;
        }
        if !handled {
            audit.difference(
                "CSS_PROPERTY_UNSUPPORTED",
                &d.source,
                "该静态属性或该属性的角色作用域尚无 v2 映射；原声明保留在差异中",
            );
        }
        for (path, value, reason) in proposals {
            if let Some((old, previous, _)) = changes.get(&path) {
                if old != &value {
                    audit.difference(
                        "TOKEN_CONFLICT",
                        &json!({"target":path,"previous":previous.source,"next":d.source}),
                        "多个样式共用 v2 token，按声明投影优先级选择，未宣称 CSS 级联等价",
                    );
                }
                if d.rank < previous.rank {
                    continue;
                }
            }
            changes.insert(path, (value, d.clone(), reason));
        }
    }
    for (path, (value, d, reason)) in changes {
        write(theme, audit, &path, value, &d, reason);
    }
}

fn page_box(
    theme: &mut Value,
    declarations: &[Declaration],
    selected: &BTreeMap<(String, String), Declaration>,
    audit: &mut Audit,
) {
    let (root_pt, body_pt, document_pt) = font_context(selected);
    let mut groups = BTreeMap::<(u8, String), BTreeMap<String, Declaration>>::new();
    for d in declarations {
        let Some((kind, edge)) = d.property.split_once('-') else {
            continue;
        };
        let Some(side) = (match edge {
            "top" | "right" | "bottom" | "left" => Some(edge),
            "block-start" => Some("top"),
            "block-end" => Some("bottom"),
            "inline-start" => Some("left"),
            "inline-end" => Some("right"),
            _ => None,
        }) else {
            continue;
        };
        let group = if d.role == "page" && kind == "margin" {
            (0, "@page".into())
        } else if kind == "padding" && d.container && matches!(d.role, "body" | "main") {
            if d.root {
                (1, "html".into())
            } else if d.body_container {
                (2, "body".into())
            } else {
                (
                    3,
                    if let Some(component) = d.source["component"].as_str() {
                        format!("{}:{component}", d.source["file"].as_str().unwrap_or(""))
                    } else {
                        d.selector.clone()
                    },
                )
            }
        } else {
            continue;
        };
        let fields = groups.entry(group).or_default();
        if fields.get(side).is_none_or(|old| d.rank >= old.rank) {
            fields.insert(side.into(), d.clone());
        }
    }
    let document = groups
        .iter()
        .filter(|((kind, _), _)| *kind == 3)
        .max_by_key(|(_, fields)| fields.values().map(|d| d.rank).max())
        .map(|(key, _)| key.clone());
    if groups.keys().filter(|(kind, _)| *kind == 3).count() > 1 {
        audit.difference("DOCUMENT_PADDING_CONTAINER_INFERRED",&json!({"selected":document}),"多个候选文档容器未证明父子关系，选择单一容器的边距集合，不把所有备选布局或子节点内距相加");
    }
    for side in ["top", "right", "bottom", "left"] {
        let mut total = 0.0;
        let mut inputs = Vec::new();
        let mut source = None;
        let mut invalid = false;
        for ((kind, id), fields) in &groups {
            if *kind == 3 && document.as_ref() != Some(&(*kind, id.clone())) {
                continue;
            }
            let Some(d) = fields.get(side) else { continue };
            let em = match kind {
                1 => root_pt,
                2 => body_pt,
                _ => document_pt,
            };
            // Percent box insets depend on containing-block width, never em.
            let value = if d.value.contains('%') {
                None
            } else {
                length_pt(&d.value, em, root_pt).map(|n| n * 25.4 / 72.0)
            };
            let valid = value.filter(|n| (0.0..=35.0).contains(n));
            if let Some(value) = valid {
                total += value;
            } else {
                invalid = true;
            }
            inputs.push(json!({"box":id,"kind":kind,"valueMm":valid,"source":d.source}));
            source = Some(d);
        }
        let Some(source) = source else { continue };
        let value = if !invalid && (3.0..=35.0).contains(&total) {
            total
        } else {
            audit.difference("NUMERIC_VALUE_UNREPRESENTABLE",&json!({"side":side,"inputs":inputs,"sumMm":total}),"纸面边距与容器内距分别解析并相加；结果须为 3..35mm，负值、百分比或超范围采用 16mm，不猜测容器宽度");
            16.0
        };
        let path = format!("/page/marginsMm/{side}");
        write(
            theme,
            audit,
            &path,
            json!(value),
            source,
            "组合独立的 @page 纸面边距、html/body 内距与单一文档容器内距，不再相互覆盖；片段分页及完整 CSS 盒模型仍待复核",
        );
        audit.evidence(
            "page-box-composition",
            &json!({"side":side,"inputs":inputs,"effectiveMm":value}),
            &source.source,
            "各盒子的独立声明优先级；没有合并任意嵌套容器或按字体大小换算百分比页边距",
        );
    }
}

fn composition(
    theme: &mut Value,
    declarations: &[Declaration],
    selected: &BTreeMap<(String, String), Declaration>,
    audit: &mut Audit,
) {
    let get = |r: &str, p: &str| selected.get(&(r.into(), p.into()));
    if let Some(d) = get("header", "display").filter(|d| d.value == "flex")
        && !get("header", "flex-direction").is_some_and(|d| d.value == "column")
    {
        write(
            theme,
            audit,
            "/composition/header",
            json!("split"),
            d,
            "flex 头部近似内置 split 头部，原子项宽度/顺序待审核",
        );
    }
    if let Some(d) = get("meta", "flex-direction").filter(|d| d.value == "column") {
        write(
            theme,
            audit,
            "/composition/contact",
            json!("stacked"),
            d,
            "联系人纵向排列推断",
        );
    }
    let grid = get("body", "grid-template-columns");
    let sidebar_decl = declarations.iter().find(|d| {
        d.role == "sidebar"
            && (normalized(d.source["component"].as_str().unwrap_or("")).contains("sidebar")
                || d.source["declaration"]["scope"]
                    .as_array()
                    .is_some_and(|scopes| {
                        scopes.iter().any(|s| {
                            s.as_str()
                                .is_some_and(|s| normalized(s).contains("sidebar"))
                        })
                    })
                || matches!(
                    d.property.as_str(),
                    "width" | "flex-basis" | "background" | "background-color" | "grid-area"
                ))
    });
    let sidebar =
        sidebar_decl.is_some() || grid.is_some_and(|d| d.value.split_whitespace().count() == 2);
    if !sidebar {
        return;
    }
    let source = sidebar_decl.or(grid).unwrap();
    let mut right = source.source["component"]
        .as_str()
        .is_some_and(|s| normalized(s).contains("right"));
    if get("sidebar", "float").is_some_and(|d| d.value == "right")
        || get("sidebar", "order").is_some_and(|d| number(&d.value).is_some_and(|n| n > 0.0))
        || get("sidebar", "grid-area").is_some_and(|d| d.value == "right")
    {
        right = true;
    }
    let mut width = None;
    if let Some(d) = get("sidebar", "width")
        .or_else(|| get("sidebar", "flex-basis"))
        .or_else(|| get("sidebar", "min-width"))
    {
        width = d.value.strip_suffix('%').and_then(number);
        if width.is_none() {
            audit.difference(
                "SIDEBAR_FIXED_WIDTH_APPROXIMATED",
                &d.source,
                "固定侧栏宽度依赖原容器尺寸，使用页面可用宽度作近似",
            );
        }
        if width.is_none() {
            width = length_pt(&d.value, 10.0, 12.0).map(|n| n * 100.0 / (178.0 * 72.0 / 25.4));
        }
    }
    if let Some(d) = grid {
        let columns = d.value.split_whitespace().collect::<Vec<_>>();
        if columns.len() == 2 {
            let pct = |s: &str| s.strip_suffix('%').and_then(number);
            let fr = |s: &str| s.strip_suffix("fr").and_then(number);
            let fixed = |s: &str| length_pt(s, 10.0, 12.0);
            let ratio = match (
                pct(columns[0]),
                pct(columns[1]),
                fr(columns[0]),
                fr(columns[1]),
            ) {
                (Some(a), _, _, _) => Some((a, false)),
                (_, Some(b), _, _) => Some((b, true)),
                (_, _, Some(a), Some(b)) if a > 0.0 && b > 0.0 => {
                    Some((a.min(b) / (a + b) * 100.0, a > b))
                }
                _ if fixed(columns[0]).is_some() && fr(columns[1]).is_some() => Some((
                    fixed(columns[0]).unwrap() * 100.0 / (178.0 * 72.0 / 25.4),
                    false,
                )),
                _ if fixed(columns[1]).is_some() && fr(columns[0]).is_some() => Some((
                    fixed(columns[1]).unwrap() * 100.0 / (178.0 * 72.0 / 25.4),
                    true,
                )),
                _ => None,
            };
            if let Some((w, r)) = ratio {
                if width.is_none() {
                    width = Some(w);
                }
                right |= r;
            }
        }
        audit.difference(
            "GRID_SIDEBAR_APPROXIMATED",
            &d.source,
            "两栏网格近似为 v2 侧栏；等宽栏、原容器宽度、行轨道和跨栏布局未等价复现",
        );
    }
    write(
        theme,
        audit,
        "/composition/preset",
        json!(if right {
            "sidebar-right"
        } else {
            "sidebar-left"
        }),
        source,
        "从侧栏角色/双栏网格推断布局",
    );
    bounded(
        theme,
        audit,
        "/composition/sidebarWidthPct",
        width,
        20.0,
        40.0,
        30.0,
        source,
        "侧栏比例投影，未知或超范围采用安全 30%",
    );
    for p in ["column-gap", "gap"] {
        if let Some(d) = get("body", p) {
            bounded(
                theme,
                audit,
                "/composition/gapMm",
                length_pt(&d.value, 10.0, 12.0).map(|n| n * 25.4 / 72.0),
                0.0,
                15.0,
                6.0,
                d,
                "列间距，未把行间距当作等价实现",
            );
            break;
        }
    }
    let mut explicit = false;
    for d in declarations.iter().filter(|d| d.sidebar) {
        if let Some(id) = d.section {
            write(
                theme,
                audit,
                &format!("/composition/placements/{id}"),
                json!("sidebar"),
                d,
                "选择器/组件名中的侧栏与模块标记推断位置",
            );
            explicit = true;
        }
        if matches!(d.role, "header" | "name" | "label") {
            write(
                theme,
                audit,
                "/composition/headerRegion",
                json!("sidebar"),
                d,
                "侧栏头部角色推断",
            );
            explicit = true;
        }
        if d.role == "summary" {
            write(
                theme,
                audit,
                "/composition/summaryRegion",
                json!("sidebar"),
                d,
                "侧栏摘要角色推断",
            );
            explicit = true;
        }
        if d.role == "meta"
            && normalized(d.source["component"].as_str().unwrap_or("")).contains("contact")
        {
            write(
                theme,
                audit,
                "/composition/contactRegion",
                json!("sidebar"),
                d,
                "侧栏联系角色推断",
            );
            explicit = true;
        }
    }
    if !explicit {
        for id in ["skills", "languages", "interests", "certificates"] {
            write(
                theme,
                audit,
                &format!("/composition/placements/{id}"),
                json!("sidebar"),
                source,
                "无静态 DOM 成员证据，推断辅助模块放侧栏；全部模块仍保留",
            );
        }
        write(
            theme,
            audit,
            "/composition/contactRegion",
            json!("sidebar"),
            source,
            "无 DOM 成员证据，推断联系信息放侧栏",
        );
        audit.difference(
            "SIDEBAR_MEMBERSHIP_INFERRED",
            &source.source,
            "当前 inspection 无完整 JSX 父子成员关系；辅助模块与联系人位置为待审核推断",
        );
    }
    // Ensure an otherwise empty sidebar still has content without deleting fields.
    let c = &theme["composition"];
    if !c["placements"]
        .as_object()
        .is_some_and(|p| p.values().any(|v| v == "sidebar"))
        && !["headerRegion", "summaryRegion", "contactRegion"]
            .iter()
            .any(|k| c[*k] == "sidebar")
    {
        write(
            theme,
            audit,
            "/composition/contactRegion",
            json!("sidebar"),
            source,
            "侧栏成员不足，采用可渲染的联系区域并保留所有字段",
        );
    }
    if theme["composition"]["headerRegion"] != "sidebar" {
        write(
            theme,
            audit,
            "/composition/headerRegion",
            json!("main"),
            source,
            "双栏头部默认为主栏，完整 JSX 成员关系尚待审核",
        );
    }
    if theme["composition"]["summaryRegion"] != "sidebar" {
        write(
            theme,
            audit,
            "/composition/summaryRegion",
            json!("main"),
            source,
            "双栏摘要默认为主栏，完整 JSX 成员关系尚待审核",
        );
    }
    if let Some(d) = get("sidebar", "font-size") {
        let body = theme["typography"]["body"]["sizePt"]
            .as_f64()
            .unwrap_or(10.0);
        bounded(
            theme,
            audit,
            "/composition/sidebarTypographyScale",
            length_pt(&d.value, body, 12.0).map(|v| v / body),
            0.7,
            1.2,
            1.0,
            d,
            "侧栏基准字号近似统一缩放，不能逐角色精确缩放",
        );
    }
}

fn font_requests(
    theme: &Value,
    selected: &BTreeMap<(String, String), Declaration>,
    audit: &mut Audit,
) -> Vec<Value> {
    let mut requests = Vec::new();
    for role in ROLES {
        let Some(d) = selected
            .get(&(role.into(), "font-family".into()))
            .or_else(|| selected.get(&("document".into(), "font-family".into())))
        else {
            continue;
        };
        let Some(families) = font_families(&d.value) else {
            audit.difference(
                "FONT_REQUEST_FAMILY_UNRESOLVED",
                &d.source,
                "字体字面含未解码转义或不完整引号，未猜测资产字族",
            );
            continue;
        };
        let family = families.iter().find(|s| {
            !s.is_empty()
                && ![
                    "serif",
                    "sans-serif",
                    "monospace",
                    "cursive",
                    "fantasy",
                    "system-ui",
                    "ui-serif",
                    "ui-sans-serif",
                    "ui-monospace",
                    "ui-rounded",
                    "emoji",
                    "math",
                    "fangsong",
                    "inherit",
                    "initial",
                    "unset",
                    "cjk-sans",
                    "cjk-serif",
                    "system-sans",
                    "system-serif",
                ]
                .iter()
                .any(|g| s.eq_ignore_ascii_case(g))
        });
        let Some(family) = family else {
            continue;
        };
        if family.contains(['\\', '(', ')']) {
            audit.difference(
                "FONT_REQUEST_FAMILY_UNRESOLVED",
                &d.source,
                "字体名含未解码转义或不完整引号，未猜测资产字族",
            );
            continue;
        }
        let request = json!({"role":role,"family":family,"families":families,"weight":theme["typography"][role]["weight"],
            "fontStyle":theme["typography"][role]["fontStyle"].as_str().unwrap_or("normal")});
        audit.evidence("fontRequests",&request,&d.source,"原 font-family 的第一非 generic 字族，仅请求后续精确 family/weight/slant 资产匹配；候选仍保留 CJK fallback");
        requests.push(request);
    }
    requests
}

fn readable_colors(theme: &mut Value, audit: &mut Audit) {
    let paper = theme["colors"]["paper"].as_str().unwrap().to_owned();
    for key in ["ink", "accent", "muted"] {
        let fallback =
            if rgb("#000000").contrast(rgb(&paper)) >= rgb("#FFFFFF").contrast(rgb(&paper)) {
                "#000000"
            } else {
                "#FFFFFF"
            };
        let original = theme["colors"][key].as_str().unwrap_or(fallback).to_owned();
        if rgb(&original).contrast(rgb(&paper)) < 4.5 {
            theme["colors"][key] = json!(fallback);
            audit.difference("COLOR_READABILITY_FALLBACK",&json!({"target":format!("/colors/{key}"),"original":original,"paper":paper,"chosen":fallback}),"前景与纸面对比度不足 4.5，明确换用可读安全色，不声称原颜色精确还原");
            audit.evidence(
                &format!("/colors/{key}"),
                &json!(fallback),
                &json!({"original":original}),
                "可读性安全替代",
            );
        }
    }
    let sidebar = theme["colors"]["sidebar"]
        .as_str()
        .unwrap_or(&paper)
        .to_owned();
    let has_sidebar = matches!(
        theme["composition"]["preset"].as_str(),
        Some("sidebar-left" | "sidebar-right")
    );
    for role in ROLES {
        let Some(original) = theme["typography"][role]["color"]
            .as_str()
            .map(str::to_owned)
        else {
            continue;
        };
        let region = match role {
            "name" | "label" => theme["composition"]["headerRegion"].as_str(),
            "summary" => theme["composition"]["summaryRegion"].as_str(),
            "meta" => theme["composition"]["contactRegion"].as_str(),
            _ => None,
        };
        let backgrounds = if !has_sidebar {
            vec![paper.as_str()]
        } else if region == Some("sidebar") {
            vec![sidebar.as_str()]
        } else if matches!(role, "name" | "label" | "summary") {
            vec![paper.as_str()]
        } else {
            vec![paper.as_str(), sidebar.as_str()]
        };
        if backgrounds
            .iter()
            .any(|bg| rgb(&original).contrast(rgb(bg)) < 4.5)
        {
            theme["typography"][role]
                .as_object_mut()
                .unwrap()
                .remove("color");
            audit.difference("ROLE_COLOR_READABILITY_FALLBACK",&json!({"target":format!("/typography/{role}/color"),"original":original,"backgrounds":backgrounds}),"移除不可读的角色覆盖色，显式交由 v2 可读前景色按区域处理；区域专属颜色无法用全局角色精确表达");
            audit.evidence(
                &format!("/typography/{role}/color"),
                &Value::Null,
                &json!({"original":original}),
                "移除覆盖色，采用按区域解析的可读前景色",
            );
        }
    }
}

pub(crate) fn static_section_title(text: &str) -> Option<&'static str> {
    match normalized(text).as_str() {
        "work"
        | "experience"
        | "workexperience"
        | "professionalexperience"
        | "employment"
        | "employmenthistory"
        | "career" => Some("work"),
        "education" | "academicbackground" | "academicqualifications" => Some("education"),
        "volunteer"
        | "volunteering"
        | "volunteerexperience"
        | "volunteeringexperience"
        | "serviceleadership"
        | "serviceandleadership" => Some("volunteer"),
        "skills" | "technicalskills" | "coreskills" | "competencies" => Some("skills"),
        "projects" | "selectedprojects" | "personalprojects" | "researchprojects" => {
            Some("projects")
        }
        "awards" | "honors" | "awardshonors" | "awardsandhonors" => Some("awards"),
        "certificates" | "certifications" => Some("certificates"),
        "publications" | "selectedpublications" => Some("publications"),
        "languages" => Some("languages"),
        "interests" | "hobbies" | "researchinterests" => Some("interests"),
        "references" => Some("references"),
        _ => None,
    }
}

// Existing native renderer has two built-in real faces, not an arbitrary CSS
// font engine. Preserve original requests before applying a disclosed fallback.
fn renderer_font_fallbacks(theme: &mut Value, audit: &mut Audit) {
    for role in ROLES {
        let before = theme["typography"][role].clone();
        let weight = before["weight"].as_u64().unwrap_or(400);
        let (family, weight) = if weight >= 500 {
            ("cjk-serif", 600)
        } else {
            ("cjk-sans", 400)
        };
        if before["family"] != family
            || before["weight"] != weight
            || before["fontStyle"].as_str().unwrap_or("normal") != "normal"
        {
            theme["typography"][role]["family"] = json!(family);
            theme["typography"][role]["weight"] = json!(weight);
            theme["typography"][role]["fontStyle"] = json!("normal");
            audit.difference("RENDERER_REAL_FACE_FALLBACK",&json!({"role":role,"requested":before,"chosen":theme["typography"][role]}),"未加载原字面前仅使用内置真实字面：无衬线 400 或衬线 600，不合成斜体；原请求另存 fontRequests，可由显式字体资料库恢复");
        }
    }
}

fn reserve_first_baseline_clearance(theme: &mut Value, audit: &mut Audit) {
    // The shared stack writer starts at a text baseline, whereas CSS padding
    // starts above the line box. A small CSS inset cannot be copied blindly.
    let line_box = theme["page"]["firstLinePosition"] == "line-box";
    let sans_metrics = layout::FontKind::Sans.vertical_metrics();
    let serif_metrics = layout::FontKind::Serif.vertical_metrics();
    let ascent = ROLES
        .iter()
        .map(|role| {
            let text = &theme["typography"][role];
            let (ascent, descent) = if text["family"] == "cjk-serif" {
                serif_metrics
            } else {
                sans_metrics
            };
            let factor = if line_box {
                (f64::from(ascent + descent) - text["lineHeight"].as_f64().unwrap_or(1.4)).max(0.0)
                    / 2.0
            } else {
                f64::from(ascent)
            };
            text["sizePt"].as_f64().unwrap_or(0.0) * factor
        })
        .fold(0.0, f64::max);
    let minimum = (ascent / f64::from(layout::PT_PER_MM) + 3.0).min(35.0);
    let top = theme["page"]["marginsMm"]["top"].as_f64().unwrap_or(16.0);
    if top < minimum {
        let chosen = (minimum * 100.0).ceil() / 100.0;
        audit.difference(
            "BASELINE_TOP_CLEARANCE_ADAPTED",
            &json!({"requestedMm":top,"chosenMm":chosen}),
            "CSS 盒顶内距与共享渲染器首行基线不同；保留字体上缘并增加 3mm 纸面留白",
        );
        put(theme, "/page/marginsMm/top", json!(chosen));
    }
}

fn linked_entry_declarations<'a>(
    graph: &Value,
    binding: &str,
    file: &str,
    by_component: &BTreeMap<(&str, &str), Vec<&'a Declaration>>,
) -> BTreeMap<String, &'a Declaration> {
    let mut selected = BTreeMap::new();
    let Some(nodes) = graph["nodes"].as_array() else {
        return selected;
    };
    let Some(node) = nodes
        .iter()
        .find(|node| node["id"] == binding && node["inheritanceLinked"] == true)
    else {
        return selected;
    };
    let mut priorities = BTreeMap::new();
    for (order, layer) in node["layers"].as_array().into_iter().flatten().enumerate() {
        let Some(name) = nodes
            .iter()
            .find(|node| node["id"] == *layer)
            .and_then(|node| node["name"].as_str())
        else {
            continue;
        };
        for &declaration in by_component.get(&(file, name)).into_iter().flatten() {
            // Base and derived styles target one element. Resolve both layers
            // before computing em; the base is not another DOM ancestor.
            let priority = (
                declaration.rank.0,
                declaration.rank.2,
                order,
                declaration.rank.3,
            );
            if priorities
                .get(&declaration.property)
                .is_none_or(|old| priority >= *old)
            {
                priorities.insert(declaration.property.clone(), priority);
                selected.insert(declaration.property.clone(), declaration);
            }
        }
    }
    selected
}

fn entry_headers(
    theme: &mut Value,
    report: &Value,
    declarations: &[Declaration],
    selected: &BTreeMap<(String, String), Declaration>,
    audit: &mut Audit,
) -> (Value, Vec<Value>) {
    let proof = match crate::entry_templates::extract(report) {
        Ok(proof) => proof,
        Err(error) => {
            audit.difference(
                "ENTRY_TEMPLATE_PROJECTION_UNSUPPORTED",
                &json!({"message":error}),
                "字段分组提取超出有界子集，保留标准模块排版",
            );
            return (json!({"error":error,"equivalent":false}), Vec::new());
        }
    };
    let mut requests = Vec::new();
    let (root_pt, _, document_pt) = font_context(selected);
    let mut by_component = BTreeMap::<(&str, &str), Vec<&Declaration>>::new();
    for declaration in declarations
        .iter()
        .filter(|d| matches!(d.selector.as_str(), "" | "&"))
    {
        if let (Some(file), Some(name)) = (
            declaration.source["file"].as_str(),
            declaration.source["component"].as_str(),
        ) {
            by_component
                .entry((file, name))
                .or_default()
                .push(declaration);
        }
    }
    for candidate in proof["candidates"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|c| c["usable"] == true && c["selectionUnambiguous"] == true)
    {
        let Some(module) = candidate["module"]
            .as_str()
            .filter(|s| MODULES[1..].contains(s))
        else {
            continue;
        };
        let Some(file) = candidate["entryFile"].as_str() else {
            continue;
        };
        let Some(template) = report["templatePrograms"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|t| t["entryFile"] == file && t["sourceSha256"] == candidate["sourceSha256"])
        else {
            continue;
        };
        let graph = &template["styleGraph"];
        let Some(rows) = candidate["rows"]
            .as_array()
            .filter(|rows| !rows.is_empty() && rows.len() <= 8)
        else {
            continue;
        };
        if rows.iter().any(|r| r["singleSourceStyle"] == false) {
            audit.difference("ENTRY_MIXED_INLINE_STYLES_UNREPRESENTABLE",candidate,"同一行含不同来源的字符样式，当前声明式标题行不能逐 span 表达，保留原模块排版而不静默拉平");
            continue;
        }
        let mut lines = Vec::new();
        let mut module_requests = Vec::new();
        let mut supported = true;
        for (index, row) in rows.iter().enumerate() {
            let Some(parts) = row["parts"]
                .as_array()
                .filter(|p| !p.is_empty() && p.len() <= 6)
            else {
                supported = false;
                break;
            };
            if parts.iter().any(|p| {
                ["prefix", "suffix"].iter().any(|key| {
                    p[*key].as_str().is_some_and(|s| {
                        s.chars().count() > 32
                            || s.contains(['<', '>'])
                            || s.chars().any(char::is_control)
                    })
                })
            }) {
                supported = false;
                break;
            }
            let mut probe = theme.clone();
            probe["typography"]["body"] = json!({"family":"cjk-sans","sizePt":document_pt,"weight":400,"fontStyle":"normal","lineHeight":1.4,"textTransform":"none"});
            let mut inherited = selected
                .iter()
                .filter(|((role, _), _)| {
                    matches!(role.as_str(), "root" | "document" | "body-container")
                })
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect::<BTreeMap<_, _>>();
            for ((role, property), d) in selected {
                if role == "document"
                    && matches!(
                        property.as_str(),
                        "font-family"
                            | "font-size"
                            | "font-weight"
                            | "font-style"
                            | "line-height"
                            | "letter-spacing"
                            | "text-transform"
                            | "color"
                    )
                {
                    inherited.insert(("body".into(), property.clone()), d.clone());
                }
            }
            let mut local = Audit::default();
            text_styles(&mut probe, &inherited, &mut local);
            let mut components = row["components"].as_array().cloned().unwrap_or_default();
            if let Some(binding) = row["styleComponentBinding"].as_str()
                && !components.iter().any(|c| c["binding"] == binding)
            {
                components.push(json!({"binding":binding}));
            }
            // Normalize inherited relative lengths before moving to a child.
            // A parent's absolute line-height and tracking remain absolute.
            for component in components.iter().chain(std::iter::once(&Value::Null)) {
                let size = probe["typography"]["body"]["sizePt"]
                    .as_f64()
                    .unwrap_or(document_pt);
                for key in ["font-size", "line-height", "letter-spacing"] {
                    if let Some(d) = inherited.get_mut(&("body".into(), key.into())) {
                        let computed = match key {
                            "font-size" => Some(size),
                            "line-height" if number(&d.value).is_none() => {
                                probe["typography"]["body"]["lineHeight"]
                                    .as_f64()
                                    .map(|n| n * size)
                            }
                            "letter-spacing" => Some(
                                probe["typography"]["body"]["trackingEm"]
                                    .as_f64()
                                    .unwrap_or(0.0)
                                    * size,
                            ),
                            _ => None,
                        };
                        if let Some(value) = computed {
                            d.value = format!("{value}pt");
                        }
                    }
                }
                if let Some(mut parent) =
                    inherited.get(&("body".into(), "font-size".into())).cloned()
                {
                    parent.root = false;
                    parent.body_container = false;
                    parent.container = true;
                    inherited.insert(("document".into(), "font-size".into()), parent);
                }
                let Some(binding) = component["binding"].as_str() else {
                    continue;
                };
                for (property, declaration) in
                    linked_entry_declarations(graph, binding, file, &by_component)
                {
                    let mut d = declaration.clone();
                    d.role = "body";
                    d.root = false;
                    d.body_container = false;
                    d.container = false;
                    inherited.insert(("body".into(), property), d);
                }
                text_styles(&mut probe, &inherited, &mut local);
            }
            let request = font_requests(&probe, &inherited, &mut local)
                .into_iter()
                .find(|r| r["role"] == "body");
            renderer_font_fallbacks(&mut probe, &mut local);
            if !local.differences.is_empty() {
                audit.difference(
                    "ENTRY_ROW_STYLE_ADAPTATION",
                    &json!({"module":module,"index":index,"differences":local.differences}),
                    "行样式的数值限制和真实字体回退保持显式记录",
                );
            }
            let mut style = probe["typography"]["body"].clone();
            let background = if theme["composition"]["placements"][module] == "sidebar" {
                theme["colors"]["sidebar"]
                    .as_str()
                    .or_else(|| theme["colors"]["paper"].as_str())
            } else {
                theme["colors"]["paper"].as_str()
            };
            if let (Some(color), Some(background)) = (style["color"].as_str(), background)
                && rgb(color).contrast(rgb(background)) < 4.5
            {
                style.as_object_mut().unwrap().remove("color");
                audit.difference(
                    "ENTRY_ROW_COLOR_READABILITY_FALLBACK",
                    row,
                    "行覆盖色与所在栏位背景不可读，保留区域默认前景色",
                );
            }
            let mut line = json!({"role":row["role"],"parts":parts,"style":style});
            if let Some(binding) = row["styleComponentBinding"].as_str() {
                let leaf = linked_entry_declarations(graph, binding, file, &by_component);
                for (property, key) in [
                    ("margin-top", "spaceBeforePt"),
                    ("margin-bottom", "spaceAfterPt"),
                ] {
                    if let Some(d) = leaf.get(property) {
                        if !d.value.contains('%')
                            && let Some(value) = length_pt(
                                &d.value,
                                probe["typography"]["body"]["sizePt"]
                                    .as_f64()
                                    .unwrap_or(document_pt),
                                root_pt,
                            )
                            .filter(|v| (0.0..=48.0).contains(v))
                        {
                            line[key] = json!(value);
                        }
                    }
                }
            }
            if let Some(request) = request {
                module_requests.push(json!({"path":format!("/sections/{module}/entryHeader/{index}/style"),"request":request,"source":row}));
            }
            lines.push(line);
        }
        if !supported {
            audit.difference(
                "ENTRY_HEADER_SCHEMA_LIMIT",
                candidate,
                "原字段分组超出声明式行数、字段数或纯文本长度限制，未截断输出",
            );
            continue;
        }
        let path = format!("/sections/{module}/entryHeader");
        put(theme, &path, json!(lines));
        put(theme, &format!("/sections/{module}/date"), json!("inline"));
        audit.evidence(&path,&json!(lines),candidate,"按真实 map 项字段绑定保留顺序、分行、前后缀和本地静态样式；日期格式适配、组件继承与完整 CSS 布局不作等价证明");
        requests.extend(module_requests);
    }
    (proof, requests)
}

/// Parse complete JSX snippets already present in the inspection. This uses
/// literal headings/ids, not arbitrary string searches for module names. A
/// heading's binding, reachability and runtime condition remain unverified.
fn source_layout(theme: &mut Value, files: &[Value], audit: &mut Audit) {
    let mut alternatives = Vec::<(Vec<&'static str>, Vec<Value>)>::new();
    for file in files {
        let mut found = Vec::<(u64, usize, &'static str, String, bool, Value)>::new();
        let mut parsed_count = 0;
        for expression in file["jsxExpressions"].as_array().into_iter().flatten() {
            let Some(raw) = snippet(&expression["expression"]) else {
                continue;
            };
            if !raw.contains('<') {
                continue;
            }
            parsed_count += 1;
            if parsed_count > 2048 {
                audit.difference(
                    "JSX_LAYOUT_EVIDENCE_LIMIT",
                    &json!({"file":file["file"]}),
                    "仅处理前 2048 个完整 JSX 表达式片段，剩余布局证据未证明",
                );
                break;
            }
            let source = format!("const __resume_style_order = <>{raw}</>;");
            let Ok((tree, _)) =
                crate::syntax::parse_javascript("<inspection-jsx-fragment>", source.as_bytes())
            else {
                continue;
            };
            if tree.root_node().has_error() {
                continue;
            }
            let mut pending = vec![(tree.root_node(), false)];
            while let Some((node, in_sidebar)) = pending.pop() {
                let mut sidebar = in_sidebar;
                if node.kind() == "jsx_element" || node.kind() == "jsx_self_closing_element" {
                    let open = node.child_by_field_name("open_tag").unwrap_or(node);
                    if let Some(name) = open.child_by_field_name("name") {
                        let name = &source[name.byte_range()];
                        sidebar |= normalized(name).contains("sidebar");
                        let mut title = None;
                        if role(name) == Some("sectionTitle") {
                            let mut literal = String::new();
                            let mut dynamic = false;
                            for i in 0..node.named_child_count() {
                                let child = node.named_child(i).unwrap();
                                if child.kind() == "jsx_text" {
                                    literal.push_str(&source[child.byte_range()]);
                                } else if !matches!(
                                    child.kind(),
                                    "jsx_opening_element" | "jsx_closing_element"
                                ) {
                                    dynamic = true;
                                }
                            }
                            if !dynamic {
                                title = Some(literal.trim().to_owned());
                            }
                        }
                        if title.is_none() && role(name) == Some("section") {
                            for i in 0..open.named_child_count() {
                                let attr = open.named_child(i).unwrap();
                                if attr.kind() != "jsx_attribute" {
                                    continue;
                                }
                                let Some(key) = attr.named_child(0) else {
                                    continue;
                                };
                                if !matches!(&source[key.byte_range()], "id" | "title") {
                                    continue;
                                }
                                let Some(value) = attr.named_child(1) else {
                                    continue;
                                };
                                if value.kind() != "string" {
                                    continue;
                                }
                                let value = &source[value.byte_range()];
                                if let Some(inner) = value
                                    .get(1..value.len().saturating_sub(1))
                                    .filter(|s| !s.contains(['\\', '&']))
                                {
                                    title = Some(inner.trim().to_owned());
                                    break;
                                }
                            }
                        }
                        if let Some(title) = title
                            && let Some(id) = static_section_title(&title)
                        {
                            found.push((expression["span"]["startByte"].as_u64().unwrap_or(0),node.start_byte(),id,title,sidebar,
                                json!({"file":file["file"],"expressionSpan":expression["span"],"heading":name,"literal":source.get(node.byte_range()),"bindingVerified":false})));
                        }
                    }
                }
                for i in (0..node.named_child_count()).rev() {
                    if let Some(child) = node.named_child(i) {
                        pending.push((child, sidebar));
                    }
                }
            }
        }
        found.sort_by_key(|f| (f.0, f.1));
        let mut order = Vec::new();
        let mut origins = Vec::new();
        for (_, _, id, title, sidebar, source) in found {
            if order.contains(&id) {
                continue;
            }
            order.push(id);
            origins.push(source.clone());
            // Source snippets can be an alternative branch/component. Keep all
            // changes inferred, and leave modules absent from the list intact.
            if title.len() <= 120
                && !title.contains(['<', '>'])
                && !title.chars().any(char::is_control)
            {
                let path = format!("/sections/{id}/title");
                put(theme, &path, json!(title));
                audit.evidence(
                    &path,
                    &json!(title),
                    &source,
                    "静态 JSX 标题字面值；分支与绑定未验证，不改简历数据",
                );
            }
            if sidebar
                && matches!(
                    theme["composition"]["preset"].as_str(),
                    Some("sidebar-left" | "sidebar-right")
                )
            {
                let path = format!("/composition/placements/{id}");
                put(theme, &path, json!("sidebar"));
                audit.evidence(
                    &path,
                    &json!("sidebar"),
                    &source,
                    "完整 JSX 片段内的 Sidebar 命名祖先推断模块位置，未验证组件绑定",
                );
            }
        }
        if !order.is_empty() {
            alternatives.push((order, origins));
        }
    }
    // Prefer the file with most distinct literal section headings. This is a
    // documented heuristic for multi-entry packages, not a dependency resolver.
    alternatives.sort_by_key(|(order, _)| std::cmp::Reverse(order.len()));
    if let Some((order, origins)) = alternatives.first() {
        put(theme, "/composition/moduleOrder", json!(order));
        audit.evidence(
            "/composition/moduleOrder",
            &json!(order),
            &json!(origins),
            "按完整 JSX 片段的源码顺序排列静态模块标题；不含 basics；未列出模块由渲染器保序追加",
        );
        audit.difference("MODULE_ORDER_INFERRED",&json!({"alternatives":alternatives}),"标题匹配、组件绑定、条件可达性及多入口选择未证明；仅采用静态片段中模块最多的文件的顺序");
    }
}

/// Returns (validated v2 candidate, standalone adaptation evidence report).
pub(crate) fn adapt(report: &Value, candidate: Value) -> Result<(Value, Value), String> {
    let files = report["syntaxEvidence"]
        .as_array()
        .ok_or("STYLE_ADAPT_INPUT: inspection.syntaxEvidence must be an array")?;
    if files.len() > 4096 {
        return Err("STYLE_ADAPT_LIMIT: more than 4096 syntax files".into());
    }
    let parsed = parse_theme(candidate.to_string().as_bytes())
        .map_err(|e| format!("STYLE_ADAPT_CANDIDATE: {e:?}"))?;
    let mut theme = parsed.normalized;
    let mut audit = Audit::default();
    for id in MODULES {
        if !theme["modules"].as_array().unwrap().iter().any(|v| v == id) {
            theme["modules"].as_array_mut().unwrap().push(json!(id));
        }
    }
    audit.evidence(
        "/modules",
        &theme["modules"],
        &Value::Null,
        "保留并补齐 12 个标准模块；不按原模板的隐藏条件删除简历字段",
    );
    // Make core defaults explicit so page margin/typography partial mappings are valid.
    for (role, size, weight) in [
        ("body", 10.0, 400),
        ("name", 26.0, 600),
        ("label", 10.0, 400),
        ("summary", 10.0, 400),
        ("sectionTitle", 13.0, 600),
        ("itemTitle", 10.6, 400),
        ("meta", 8.5, 400),
        ("sideDate", 8.5, 400),
    ] {
        for (key, value) in [
            ("family", json!("cjk-sans")),
            ("sizePt", json!(size)),
            ("weight", json!(weight)),
            ("lineHeight", json!(1.4)),
        ] {
            let path = format!("/typography/{role}/{key}");
            if theme.pointer(&path).is_none() {
                put(&mut theme, &path, value);
            }
        }
    }
    for side in ["top", "right", "bottom", "left"] {
        let p = format!("/page/marginsMm/{side}");
        if theme.pointer(&p).is_none() {
            put(&mut theme, &p, json!(16));
        }
    }
    if !theme["composition"].is_object() {
        theme["composition"] = json!({"preset":"single-column"});
    }
    if !theme["sections"].is_object() {
        theme["sections"] = json!({});
    }
    audit.evidence("candidate-defaults",&json!({"typography":theme["typography"],"page":theme["page"],"composition":theme["composition"]}),&Value::Null,"缺少静态证据的值采用候选/显式安全默认，不代表原模板样式");
    let mut declarations = Vec::new();
    let mut shared_variables = BTreeMap::<String, Option<(String, Vec<Value>)>>::new();
    for file in files {
        for (key, value) in root_variables(file) {
            let entry = shared_variables
                .entry(key)
                .or_insert_with(|| Some((value.clone(), Vec::new())));
            if let Some((old, sources)) = entry {
                if old == &value {
                    sources.push(file["file"].clone());
                } else {
                    *entry = None;
                }
            }
        }
        if shared_variables.len() > 4096 {
            return Err("STYLE_ADAPT_VARIABLE_LIMIT: more than4096 rootvariables".into());
        }
    }
    for (key, value) in &shared_variables {
        if value.is_none() {
            audit.difference(
                "CROSS_FILE_ROOT_VARIABLE_AMBIGUOUS",
                &json!({"variable":key}),
                "不同文件的根变量声明不同，未猜测其加载顺序；使用文件内既有投影或报告无法解析",
            );
        } else if let Some((value, sources)) = value {
            audit.evidence(
                "cross-file-root-variable",
                &json!({"name":key,"value":value,"sources":sources}),
                &Value::Null,
                "所有已读静态根声明值相同，可供没有本地声明的文件使用；未证明运行时加载或组件挂载",
            );
        }
    }
    for file in files {
        let name = file["file"].as_str().unwrap_or("<unknown>");
        let mut variables = root_variables(file);
        for (key, value) in &shared_variables {
            if !variables.contains_key(key)
                && let Some((value, _)) = value
            {
                variables.insert(key.clone(), value.clone());
            }
        }
        if !variables.is_empty() && file["styledComponents"].is_array() {
            audit.evidence("available-static-root-variables", &json!({"count":variables.len()}), &json!({"file":name}), "同文件静态根变量及无冲突的跨文件字面变量，值和来源另记在根变量证据中；未证明加载或挂载");
        }
        if file["syntaxValid"] == false {
            audit.difference(
                "SOURCE_SYNTAX_UNSUPPORTED",
                &json!({"file":name,"errors":file["errors"]}),
                "来源语法不完整；仅独立有效 CSS 块可以投影",
            );
        }
        if file["format"] == "css" || file["styles"].is_object() {
            audit.css_files += 1;
            collect_css(
                &file["styles"],
                name,
                None,
                None,
                &variables,
                &mut declarations,
                &mut audit,
            )?;
        }
        for style in file["styledComponents"].as_array().into_iter().flatten() {
            audit.styled += 1;
            let source = json!({"file":name,"component":style["name"],"span":style["span"]});
            if style["dynamicExpressions"]
                .as_array()
                .is_none_or(|v| !v.is_empty())
                || style["templateEscapesRequireDecoding"] == true
                || style["topLevelBinding"] != true
            {
                audit.difference(
                    "DYNAMIC_OR_SCOPED_STYLE_UNSUPPORTED",
                    &json!({"style":style,"file":name}),
                    "动态、转义未解码或非顶层样式不作部分猜测，不执行表达式",
                );
                continue;
            }
            if style["base"]["kind"] == "component" {
                audit.difference(
                    "COMPONENT_INHERITANCE_UNVERIFIED",
                    &source,
                    "仅采用本组件静态声明；导入组件及 styled 继承链没有计算样式等价证明",
                );
            }
            if style["constantSubstitutions"]
                .as_array()
                .is_some_and(|v| !v.is_empty())
            {
                audit.evidence(
                    "static-constant-substitutions",
                    &style["constantSubstitutions"],
                    &source,
                    "采用 inspection 既有受限常量折叠结果及原表达式位置；没有执行来源 JS",
                );
            }
            collect_css(
                &style["css"],
                name,
                style["name"].as_str(),
                style["base"]["name"].as_str(),
                &variables,
                &mut declarations,
                &mut audit,
            )?;
        }
        for template in file["unclassifiedTemplates"]
            .as_array()
            .into_iter()
            .flatten()
        {
            audit.difference(
                "UNCLASSIFIED_TEMPLATE_UNSUPPORTED",
                template,
                "未从普通 JS 模板字符串提取调色板或执行代码",
            );
        }
    }
    audit.evidence("role-projection-policy",&json!({"roles":audit.matched,"initialRootSizeAssumptionPt":12,"pxToPt":0.75,"media":if report["printProfile"]["profile"]=="print" {"unconditional-and-positive-print-projection"}else{"unconditional-only"}}),&Value::Null,"组件名/简单选择器推断角色；相对字号另记实际根/文档上下文；跨文件加载顺序、DOM 匹配、继承与完整级联均未验证");
    let selected = choose(&declarations, &mut audit);
    text_styles(&mut theme, &selected, &mut audit);
    apply_structure(&mut theme, &declarations, &selected, &mut audit);
    page_box(&mut theme, &declarations, &selected, &mut audit);
    composition(&mut theme, &declarations, &selected, &mut audit);
    source_layout(&mut theme, files, &mut audit);
    if let Some(order) = report["markupHints"]["moduleOrder"]
        .as_array()
        .filter(|o| o.len() >= 2)
    {
        put(&mut theme, "/composition/moduleOrder", json!(order));
        audit.evidence(
            "/composition/moduleOrder",
            &json!(order),
            &report["markupHints"],
            "完整源码中的实际字面模块标题顺序；没有从文件名字典序推断模块顺序",
        );
        let mut titled = BTreeSet::new();
        for heading in report["markupHints"]["literalHeadings"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|h| h["file"] == report["markupHints"]["orderSource"])
        {
            let (Some(id), Some(title)) = (heading["module"].as_str(), heading["title"].as_str())
            else {
                continue;
            };
            if MODULES[1..].contains(&id) && title.chars().count() <= 80 && titled.insert(id) {
                let path = format!("/sections/{id}/title");
                put(&mut theme, &path, json!(title));
                audit.evidence(
                    &path,
                    &json!(title),
                    heading,
                    "采用同一模块顺序来源中的完整字面标题，不执行模板表达式",
                );
            }
        }
    }
    let (entry_proof, entry_requests) =
        entry_headers(&mut theme, report, &declarations, &selected, &mut audit);
    readable_colors(&mut theme, &mut audit);
    let font_requests = font_requests(&theme, &selected, &mut audit);
    renderer_font_fallbacks(&mut theme, &mut audit);
    if !declarations.is_empty() {
        put(&mut theme, "/page/firstLinePosition", json!("line-box"));
        audit.evidence(
            "/page/firstLinePosition",
            &json!("line-box"),
            &Value::Null,
            "CSS 纸面起点为首个行盒顶边，使用实际字面度量定位首行基线；旧原生主题默认基线模式不变",
        );
    }
    reserve_first_baseline_clearance(&mut theme, &mut audit);
    put(&mut theme, "/provenance/fidelity", json!("adapted"));
    let parsed = parse_theme(theme.to_string().as_bytes())
        .map_err(|e| format!("STYLE_ADAPT_OUTPUT_INVALID: {e:?}"))?;
    layout::check_theme(&parsed).map_err(|e| format!("STYLE_ADAPT_LAYOUT_UNSUPPORTED: {e}"))?;
    let output = json!({"format":"resume-one-static-style-adaptation-v1","fidelity":"adapted","status":"needs-review",
        "visualVerified":false,"migrationComplete":false,"completeCssCascade":false,"executedSourceCode":false,
        "schemaValidated":true,"layoutValidated":true,
        "coverage":{"syntaxFiles":files.len(),"cssFiles":audit.css_files,"styledComponents":audit.styled,
            "declarationsSeen":audit.seen,"matchedRoles":audit.matched,"evidenceRecords":audit.evidence.len()},
        "fontRequests":font_requests,"entryHeaderFonts":entry_requests,"entryTemplates":entry_proof,"evidence":audit.evidence,"differences":audit.differences,
        "limitations":["角色/选择器映射为静态推断；不保证组件实际被使用","不模拟完整 CSS 级联、跨文件加载和 DOM 匹配",
            "条件 media/伪类/伪元素、动态样式、图片图标、任意网格及定位不等价",
            "字体显式 fallback；行宽、分页、字体度量和侧栏成员需要视觉审核"]});
    Ok((theme, output))
}

#[cfg(test)]
#[path = "adapt_style_tests.rs"]
mod tests;
