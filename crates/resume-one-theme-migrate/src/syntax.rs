//! Read-only JS/JSX and CSS syntax evidence. Parsing never evaluates source.
//! A parsed declaration is not a supported runtime feature or fidelity proof.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tree_sitter::{Language, Node, ParseOptions, Parser, Tree};

const MAX_NODES: usize = 100_000;
const MAX_RECORDS: usize = 12_000;
const MAX_SNIPPET_BYTES: usize = 2_048;
const MAX_FOLDED_CSS_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug)]
pub(crate) struct SyntaxError {
    pub code: &'static str,
    pub message: String,
}

fn failure(code: &'static str, message: impl Into<String>) -> SyntaxError {
    SyntaxError {
        code,
        message: message.into(),
    }
}

fn parse(bytes: &[u8], language: Language) -> Result<Tree, SyntaxError> {
    let mut parser = Parser::new();
    parser
        .set_language(&language)
        .map_err(|error| failure("PARSER_LANGUAGE", error.to_string()))?;
    let start = Instant::now();
    let mut cancelled = |_: &tree_sitter::ParseState| start.elapsed() > Duration::from_secs(2);
    parser
        .parse_with_options(
            &mut |offset, _| bytes.get(offset..).unwrap_or_default(),
            None,
            Some(ParseOptions::new().progress_callback(&mut cancelled)),
        )
        .ok_or_else(|| {
            failure(
                "PARSER_LIMIT",
                "语法解析超时或已取消，不能把未读取部分当作已迁移",
            )
        })
}

/// Iterative traversal bounds hostile nesting without overflowing Rust's stack.
fn nodes(root: Node<'_>) -> Result<Vec<Node<'_>>, SyntaxError> {
    let mut pending = vec![root];
    let mut result = Vec::new();
    while let Some(node) = pending.pop() {
        if result.len() >= MAX_NODES {
            return Err(failure("AST_LIMIT", "单文件语法节点超过 100000 个"));
        }
        result.push(node);
        for index in (0..node.named_child_count()).rev() {
            if let Some(child) = node.named_child(index) {
                pending.push(child);
            }
        }
    }
    Ok(result)
}

/// Common parser entry point for evidence and template compilation. The pinned
/// vendored scanner handles legal JSX bare ampersands directly, without source
/// rewriting or general error recovery.
pub(crate) fn parse_javascript(
    _path: &str,
    source: &[u8],
) -> Result<(Tree, Vec<Value>), SyntaxError> {
    Ok((
        parse(source, tree_sitter_javascript::LANGUAGE.into())?,
        Vec::new(),
    ))
}

fn text<'a>(node: Node<'_>, bytes: &'a [u8]) -> &'a str {
    std::str::from_utf8(&bytes[node.byte_range()]).expect("UTF-8 checked before parsing")
}

fn snippet(value: &str) -> Value {
    let mut end = value.len().min(MAX_SNIPPET_BYTES);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    json!({"text": &value[..end], "truncated": end < value.len()})
}

struct Locations<'a> {
    path: &'a str,
    starts: Vec<usize>,
    size: usize,
}

impl<'a> Locations<'a> {
    fn new(path: &'a str, source: &[u8]) -> Self {
        let mut starts = vec![0];
        starts.extend(
            source
                .iter()
                .enumerate()
                .filter_map(|(at, byte)| (*byte == b'\n').then_some(at + 1)),
        );
        Self {
            path,
            starts,
            size: source.len(),
        }
    }

    fn span(&self, start: usize, end: usize) -> Value {
        let start = start.min(self.size);
        let line = self
            .starts
            .partition_point(|at| *at <= start)
            .saturating_sub(1);
        json!({"file": self.path, "startByte": start, "endByte": end.min(self.size),
            "line": line + 1, "column": start - self.starts[line] + 1, "columnUnit": "utf8-bytes"})
    }
}

// Escape-bearing module names remain explicit blockers rather than being
// incorrectly normalized. No JS string interpreter is hidden in the scanner.
fn literal_string(node: Node<'_>, source: &[u8]) -> Option<String> {
    if node.kind() != "string" {
        return None;
    }
    let raw = text(node, source);
    let inner = raw.get(1..raw.len().checked_sub(1)?)?;
    (!inner.contains('\\')).then(|| inner.to_owned())
}

#[derive(Clone, Copy)]
enum StyleApi {
    Styled,
    Global,
    Fragment,
}

fn imports_and_apis(
    root: Node<'_>,
    source: &[u8],
    locations: &Locations<'_>,
) -> Result<(Vec<Value>, BTreeMap<String, StyleApi>), SyntaxError> {
    let mut imports = Vec::new();
    let mut apis = BTreeMap::new();
    for node in nodes(root)? {
        if matches!(node.kind(), "import_statement" | "export_statement") {
            let Some(module_node) = node.child_by_field_name("source") else {
                continue;
            };
            let module = literal_string(module_node, source);
            imports.push(json!({"kind": node.kind(), "module": module, "specifier":snippet(text(module_node,source)), "span":locations.span(node.start_byte(),node.end_byte())}));
            if node.kind() == "import_statement" && module.as_deref() == Some("styled-components") {
                for child in nodes(node)? {
                    if child.kind() == "import_clause" {
                        for index in 0..child.named_child_count() {
                            let part = child.named_child(index).expect("child index");
                            if part.kind() == "identifier" {
                                apis.insert(text(part, source).into(), StyleApi::Styled);
                            }
                        }
                    } else if child.kind() == "import_specifier"
                        && let Some(name) = child.child_by_field_name("name")
                    {
                        let api = match text(name, source) {
                            "default" | "styled" => Some(StyleApi::Styled),
                            "createGlobalStyle" => Some(StyleApi::Global),
                            "css" => Some(StyleApi::Fragment),
                            _ => None,
                        };
                        if let Some(api) = api {
                            let local = child.child_by_field_name("alias").unwrap_or(name);
                            apis.insert(text(local, source).into(), api);
                        }
                    }
                }
            }
        } else if node.kind() == "call_expression"
            && let Some(function) = node.child_by_field_name("function")
            && (function.kind() == "import" || text(function, source) == "require")
        {
            let args = node.child_by_field_name("arguments");
            let first = args.and_then(|args| args.named_child(0));
            imports.push(json!({"kind":if function.kind()=="import"{"dynamic-import"}else{"require-call"},
                "module":first.and_then(|n|literal_string(n,source)), "span":locations.span(node.start_byte(),node.end_byte())}));
        }
    }
    Ok((imports, apis))
}

fn styled_base(
    function: Node<'_>,
    source: &[u8],
    apis: &BTreeMap<String, StyleApi>,
) -> Option<Value> {
    match function.kind() {
        "identifier" => match apis.get(text(function, source))? {
            StyleApi::Global => Some(json!({"kind":"global"})),
            StyleApi::Fragment => Some(json!({"kind":"fragment"})),
            StyleApi::Styled => None,
        },
        "member_expression" => {
            let object = function.child_by_field_name("object")?;
            if object.kind() != "identifier"
                || !matches!(apis.get(text(object, source)), Some(StyleApi::Styled))
            {
                return None;
            }
            let property = function.child_by_field_name("property")?;
            Some(json!({"kind":"element", "name":text(property,source)}))
        }
        "call_expression" => {
            let factory = function.child_by_field_name("function")?;
            if factory.kind() != "identifier"
                || !matches!(apis.get(text(factory, source)), Some(StyleApi::Styled))
            {
                return None;
            }
            let args = function.child_by_field_name("arguments")?;
            if args.named_child_count() != 1 {
                return None;
            }
            let base = args.named_child(0)?;
            if base.kind() != "identifier" {
                return None;
            }
            Some(json!({"kind":"component", "name":text(base,source)}))
        }
        _ => None,
    }
}

/// Constant replacement maps a rendered CSS token back to the entire original
/// substitution; literal segments retain exact byte positions.
struct CssSegment {
    output_start: usize,
    output_end: usize,
    source_start: usize,
    source_end: usize,
    substitution: bool,
}

fn css_offset(segments: &[CssSegment], at: usize, end: bool, fallback: usize) -> usize {
    let index = segments.partition_point(|s| {
        if end {
            s.output_end < at
        } else {
            s.output_end <= at
        }
    });
    if let Some(segment) = segments.get(index) {
        if segment.substitution {
            if end {
                segment.source_end
            } else {
                segment.source_start
            }
        } else {
            segment.source_start
                + at.saturating_sub(segment.output_start)
                    .min(segment.source_end - segment.source_start)
        }
    } else {
        segments.last().map_or(fallback, |s| s.source_end)
    }
}

fn css(
    body: &str,
    base: usize,
    wrap: bool,
    locations: &Locations<'_>,
    segments: Option<&[CssSegment]>,
) -> Result<Value, SyntaxError> {
    let prefix = if wrap { ".__resume_one__ {\n" } else { "" };
    let parsed = format!("{prefix}{body}{}", if wrap { "\n}" } else { "" });
    let bytes = parsed.as_bytes();
    let tree = parse(bytes, tree_sitter_css::LANGUAGE.into())?;
    let mut declarations = Vec::new();
    let mut directives = Vec::new();
    let mut errors = Vec::new();
    let mut pending = vec![(tree.root_node(), Vec::<String>::new(), 0usize)];
    let mut seen = 0;
    while let Some((node, mut scope, depth)) = pending.pop() {
        seen += 1;
        if seen > MAX_NODES || depth > 128 {
            return Err(failure("CSS_AST_LIMIT", "CSS 节点数量或嵌套深度超限"));
        }
        let original = |at: usize, end: bool| {
            let relative = at.saturating_sub(prefix.len()).min(body.len());
            segments.map_or(base + relative, |s| {
                css_offset(s, relative, end, base + relative)
            })
        };
        let start = original(node.start_byte(), false);
        let span = locations.span(start, original(node.end_byte(), true).max(start));
        if node.is_error() || node.is_missing() {
            errors.push(json!({"code":"CSS_SYNTAX","kind":node.kind(),"span":span}));
        }
        if node.kind() == "declaration" {
            let mut property = None;
            for index in 0..node.named_child_count() {
                let child = node.named_child(index).expect("child index");
                if child.kind() == "property_name" {
                    property = Some(child);
                    break;
                }
            }
            if let Some(property) = property {
                let raw = text(node, bytes);
                let colon = (0..node.child_count())
                    .filter_map(|i| node.child(i))
                    .find(|n| n.kind() == ":");
                let value = colon
                    .map(|colon| {
                        std::str::from_utf8(&bytes[colon.end_byte()..node.end_byte()])
                            .expect("CSS UTF8")
                            .trim()
                            .trim_end_matches(';')
                            .trim()
                    })
                    .unwrap_or_default();
                let important = (0..node.named_child_count())
                    .filter_map(|i| node.named_child(i))
                    .any(|n| n.kind() == "important");
                declarations.push(json!({"property":text(property,bytes),"value":snippet(value),"important":important,"raw":snippet(raw),"scope":scope,"span":span}));
                if declarations.len() + directives.len() + errors.len() > MAX_RECORDS {
                    return Err(failure("CSS_RECORD_LIMIT", "CSS 解析记录超限"));
                }
            }
            continue;
        }
        let block = (0..node.named_child_count())
            .filter_map(|i| node.named_child(i))
            .find(|n| n.kind() == "block");
        if let Some(block) = block {
            let head = std::str::from_utf8(&bytes[node.start_byte()..block.start_byte()])
                .expect("CSS UTF8")
                .trim();
            if !(wrap && node.start_byte() == 0 && head == ".__resume_one__")
                && node.kind() != "stylesheet"
            {
                scope.push(head.to_owned());
            }
        } else if matches!(
            node.kind(),
            "import_statement" | "charset_statement" | "namespace_statement"
        ) {
            directives.push(json!({"kind":node.kind(),"raw":snippet(text(node,bytes)),"scope":scope,"span":span}));
        }
        if declarations.len() + directives.len() + errors.len() > MAX_RECORDS {
            return Err(failure("CSS_RECORD_LIMIT", "CSS 解析记录超限"));
        }
        for index in (0..node.named_child_count()).rev() {
            if let Some(child) = node.named_child(index) {
                pending.push((child, scope.clone(), depth + 1));
            }
        }
    }
    Ok(
        json!({"syntaxValid":!tree.root_node().has_error(),"sourceMapping":if segments.is_some(){"constant-folded"}else{"literal"},"declarations":declarations,"directives":directives,"errors":errors}),
    )
}

struct FoldedCss {
    text: String,
    segments: Vec<CssSegment>,
    constants: Vec<Value>,
    unresolved: Vec<Value>,
}

fn css_piece(
    result: &mut FoldedCss,
    text: &str,
    start: usize,
    end: usize,
    substitution: bool,
) -> Result<(), SyntaxError> {
    if result.text.len().saturating_add(text.len()) > MAX_FOLDED_CSS_BYTES {
        return Err(failure("CSS_FOLD_LIMIT", "常量展开后的 CSS 超过 8 MiB"));
    }
    let output_start = result.text.len();
    result.text.push_str(text);
    if !text.is_empty() {
        result.segments.push(CssSegment {
            output_start,
            output_end: result.text.len(),
            source_start: start,
            source_end: end,
            substitution,
        });
    }
    Ok(())
}

fn fold_css(
    template: Node<'_>,
    source: &[u8],
    constants: &crate::constant_expr::Constants<'_, '_>,
    locations: &Locations<'_>,
) -> Result<FoldedCss, SyntaxError> {
    let mut result = FoldedCss {
        text: String::new(),
        segments: Vec::new(),
        constants: Vec::new(),
        unresolved: Vec::new(),
    };
    let mut cursor = template.start_byte() + 1;
    for index in 0..template.named_child_count() {
        let node = template.named_child(index).expect("template child");
        if node.kind() != "template_substitution" {
            continue;
        }
        css_piece(
            &mut result,
            std::str::from_utf8(&source[cursor..node.start_byte()]).expect("source utf8"),
            cursor,
            node.start_byte(),
            false,
        )?;
        let expressions = (0..node.named_child_count())
            .filter_map(|i| node.named_child(i))
            .filter(|n| n.kind() != "comment")
            .collect::<Vec<_>>();
        let value = if expressions.len() == 1 {
            constants.evaluate(expressions[0], template.start_byte())
        } else {
            Err("CONST_EXPRESSION: substitution requires exactly one expression".into())
        };
        let span = locations.span(node.start_byte(), node.end_byte());
        match value {
            Ok(value) => {
                css_piece(
                    &mut result,
                    &value,
                    node.start_byte(),
                    node.end_byte(),
                    true,
                )?;
                result.constants.push(
                    json!({"expression":snippet(text(node,source)),"value":value,"span":span}),
                );
            }
            Err(reason) => result
                .unresolved
                .push(json!({"expression":snippet(text(node,source)),"reason":reason,"span":span})),
        }
        cursor = node.end_byte();
    }
    let end = template.end_byte() - 1;
    css_piece(
        &mut result,
        std::str::from_utf8(&source[cursor..end]).expect("source utf8"),
        cursor,
        end,
        false,
    )?;
    Ok(result)
}

fn static_member_path(mut node: Node<'_>, source: &[u8]) -> Option<Vec<String>> {
    let mut parts = Vec::new();
    for _ in 0..32 {
        if node.kind() == "identifier" {
            parts.push(text(node, source).into());
            parts.reverse();
            return Some(parts);
        }
        if node.kind() != "member_expression" {
            return None;
        }
        let part = node.child_by_field_name("property")?;
        if part.kind() != "property_identifier" {
            return None;
        }
        parts.push(text(part, source).into());
        node = node.child_by_field_name("object")?;
    }
    None
}

pub(crate) fn analyze(path: &str, source: &[u8]) -> Result<Option<Value>, SyntaxError> {
    let extension = path
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !matches!(extension.as_str(), "js" | "jsx" | "mjs" | "cjs" | "css") {
        return Ok(None);
    }
    let source_text = std::str::from_utf8(source)
        .map_err(|_| failure("SOURCE_UTF8", format!("源码不是 UTF-8：{path}")))?;
    let locations = Locations::new(path, source);
    if extension == "css" {
        return Ok(Some(
            json!({"file":path,"format":"css","styles":css(source_text,0,false,&locations,None)?}),
        ));
    }
    let (tree, compatibility) = parse_javascript(path, source)?;
    let (imports, apis) = imports_and_apis(tree.root_node(), source, &locations)?;
    let constants = crate::constant_expr::Constants::from_program(tree.root_node(), source);
    let mut css_bytes = 0usize;
    if imports.len() > MAX_RECORDS {
        return Err(failure("JS_RECORD_LIMIT", "JavaScript 导入记录超限"));
    }
    let all = nodes(tree.root_node())?;
    let mut styles = Vec::new();
    let mut expressions = Vec::new();
    let mut templates = Vec::new();
    let mut errors = Vec::new();
    let mut handled = BTreeSet::new();
    for node in all {
        let span = locations.span(node.start_byte(), node.end_byte());
        if node.is_error() || node.is_missing() {
            errors.push(json!({"code":"JAVASCRIPT_SYNTAX","kind":node.kind(),"span":span}));
        }
        if node.kind() == "jsx_element"
            && let (Some(open), Some(close)) = (
                node.child_by_field_name("open_tag"),
                node.child_by_field_name("close_tag"),
            )
        {
            let open_name = open.child_by_field_name("name").map(|n| text(n, source));
            let close_name = close.child_by_field_name("name").map(|n| text(n, source));
            if open_name != close_name {
                errors.push(json!({"code":"JSX_TAG_MISMATCH","open":open_name,"close":close_name,"span":span}));
            }
        }
        if node.kind() == "call_expression"
            && let (Some(function), Some(template)) = (
                node.child_by_field_name("function"),
                node.child_by_field_name("arguments"),
            )
            && template.kind() == "template_string"
            && let Some(base) = styled_base(function, source, &apis)
        {
            handled.insert(template.start_byte());
            let parent = node.parent();
            let declaration = parent.filter(|p| p.kind() == "variable_declarator");
            let name = declaration
                .and_then(|p| p.child_by_field_name("name"))
                .filter(|p| p.kind() == "identifier")
                .map(|p| text(p, source));
            let top_level = declaration
                .and_then(|p| p.parent())
                .and_then(|p| p.parent())
                .is_some_and(|p| {
                    p.kind() == "program"
                        || p.kind() == "export_statement"
                            && p.parent().is_some_and(|p| p.kind() == "program")
                });
            let raw = text(template, source);
            let body = &raw[1..raw.len() - 1];
            let folded = fold_css(template, source, &constants, &locations)?;
            let escaped = body.contains('\\');
            css_bytes = css_bytes.saturating_add(folded.text.len());
            if css_bytes > MAX_FOLDED_CSS_BYTES {
                return Err(failure(
                    "CSS_FOLD_LIMIT",
                    "单文件 CSS 常量展开累计超过 8 MiB",
                ));
            }
            let parsed = if folded.unresolved.is_empty() && !escaped {
                Some(css(
                    &folded.text,
                    template.start_byte() + 1,
                    base["kind"] != "global",
                    &locations,
                    (!folded.constants.is_empty()).then_some(folded.segments.as_slice()),
                )?)
            } else {
                None
            };
            styles.push(json!({"name":name,"base":base,"topLevelBinding":top_level,"dynamicExpressions":folded.unresolved,"constantSubstitutions":folded.constants,"templateEscapesRequireDecoding":escaped,"css":parsed,"span":span,"templateSpan":locations.span(template.start_byte(),template.end_byte())}));
        } else if node.kind() == "jsx_expression" {
            let expression = node.named_child(0);
            expressions.push(json!({"kind":expression.map(|p|p.kind()),"memberPath":expression.and_then(|p|static_member_path(p,source)),"expression":snippet(text(node,source)),"span":span}));
        } else if node.kind() == "template_string" && !handled.contains(&node.start_byte()) {
            templates.push(json!({"kind":"unclassified-template","raw":snippet(text(node,source)),"span":span}));
        }
        if styles.len() + expressions.len() + templates.len() + errors.len() > MAX_RECORDS {
            return Err(failure("JS_RECORD_LIMIT", "JavaScript 解析记录超限"));
        }
    }
    Ok(Some(
        json!({"file":path,"format":"javascript-jsx","syntaxValid":!tree.root_node().has_error() && errors.is_empty(),"parserCompatibility":compatibility,"imports":imports,"styledComponents":styles,"jsxExpressions":expressions,"unclassifiedTemplates":templates,"errors":errors}),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn js(source: &str) -> Value {
        analyze("src/Resume.jsx", source.as_bytes())
            .unwrap_or_else(|e| panic!("{}: {}", e.code, e.message))
            .unwrap()
    }

    #[test]
    fn jsx_bare_ampersands_preserve_original_text_locations_and_other_errors() {
        let source = "const x = <h2>Awards & Honors & Service &amp; Leadership</h2>;";
        let result = js(source);
        assert_eq!(result["syntaxValid"], true, "{result}");
        let (tree, _) = parse_javascript("test.jsx", source.as_bytes())
            .unwrap_or_else(|e| panic!("{}", e.message));
        let all = nodes(tree.root_node()).unwrap();
        let texts = all
            .iter()
            .filter(|n| n.kind() == "jsx_text")
            .map(|n| text(*n, source.as_bytes()))
            .collect::<String>();
        assert_eq!(texts, "Awards & Honors & Service  Leadership");
        assert!(
            all.iter().any(|n| n.kind() == "html_character_reference"
                && text(*n, source.as_bytes()) == "&amp;")
        );
        let broken = js("const x=<h2>Awards & Honors</h2>; const = ;");
        assert_eq!(broken["syntaxValid"], false);
        let ordinary = js("const text='Awards & Honors'; const v=a&b; // &\n");
        assert!(
            ordinary["parserCompatibility"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert_eq!(ordinary["syntaxValid"], true);
    }

    #[test]
    fn jsx_ampersand_scanner_keeps_numeric_named_references_and_unclosed_tags_distinct() {
        let source = "const x=<>A & B &amp; &#123; &#x1F680; &#X1F680; &lt; C &unknown123;</>;";
        let (tree, _) = parse_javascript("test.jsx", source.as_bytes()).unwrap();
        assert!(
            !tree.root_node().has_error(),
            "{}",
            tree.root_node().to_sexp()
        );
        let all = nodes(tree.root_node()).unwrap();
        let entities = all
            .iter()
            .filter(|n| n.kind() == "html_character_reference")
            .map(|n| text(*n, source.as_bytes()))
            .collect::<Vec<_>>();
        assert_eq!(
            entities,
            ["&amp;", "&#123;", "&#x1F680;", "&#X1F680;", "&lt;"]
        );
        assert_eq!(js("const x=<div>A & B</p>;")["syntaxValid"], false);
        assert_eq!(js("const x=<div>A & B;")["syntaxValid"], false);
    }

    #[test]
    fn css_declaration_budget_is_enforced_even_without_other_node_kinds() {
        let source = format!("a {{{}}}", "margin: 0;".repeat(MAX_RECORDS + 1));
        let error = analyze("style.css", source.as_bytes()).unwrap_err();
        assert_eq!(error.code, "CSS_RECORD_LIMIT");
    }

    #[test]
    fn static_constants_compile_to_real_css_with_original_source_spans() {
        let source = r#"import styled from 'styled-components';
const INK='#123456'; const GRID=8; const FONT="'Merriweather', Georgia, serif";
// 真实来源，位置使用 UTF-8 字节
const Layout=styled.div`color:${INK}; padding:${GRID*10}px ${GRID*6}px; font-family:${FONT}; @media print {margin:${GRID}px;}`;"#;
        let result = js(source);
        let style = &result["styledComponents"][0];
        assert!(style["dynamicExpressions"].as_array().unwrap().is_empty());
        assert_eq!(style["constantSubstitutions"].as_array().unwrap().len(), 5);
        let css = &style["css"];
        assert_eq!(css["syntaxValid"], true);
        assert_eq!(css["sourceMapping"], "constant-folded");
        assert_eq!(css["declarations"][0]["value"]["text"], "#123456");
        assert_eq!(css["declarations"][1]["value"]["text"], "80px 48px");
        assert_eq!(css["declarations"][3]["scope"], json!(["@media print"]));
        let span = &css["declarations"][1]["span"];
        let start = span["startByte"].as_u64().unwrap() as usize;
        let end = span["endByte"].as_u64().unwrap() as usize;
        assert_eq!(&source[start..end], "padding:${GRID*10}px ${GRID*6}px;");
        let whole = js(
            "import s from 'styled-components';const DECL='color: red; padding: 8px;';const C=s.div`${DECL}`;",
        );
        for declaration in whole["styledComponents"][0]["css"]["declarations"]
            .as_array()
            .unwrap()
        {
            let start = declaration["span"]["startByte"].as_u64().unwrap() as usize;
            let end = declaration["span"]["endByte"].as_u64().unwrap() as usize;
            assert_eq!(
                &"import s from 'styled-components';const DECL='color: red; padding: 8px;';const C=s.div`${DECL}`;"
                    [start..end],
                "${DECL}"
            );
        }
    }

    #[test]
    fn unresolved_props_tdz_and_shadowed_constants_do_not_create_partial_css() {
        let result = js(
            "import s from 'styled-components';const COLOR='#fff';const C=s.div`color:${COLOR};padding:${p=>p.tight?4:8}px;`;",
        );
        let style = &result["styledComponents"][0];
        assert!(style["css"].is_null());
        assert_eq!(style["constantSubstitutions"].as_array().unwrap().len(), 1);
        assert_eq!(style["dynamicExpressions"].as_array().unwrap().len(), 1);
        assert!(
            style["dynamicExpressions"][0]["reason"]
                .as_str()
                .unwrap()
                .contains("CONST_UNSUPPORTED")
        );
        for source in [
            "import s from 'styled-components';const C=s.div`padding:${GRID}px;`;const GRID=8;",
            "import s from 'styled-components';const GRID=8;function f(GRID){};const C=s.div`padding:${GRID}px;`;",
        ] {
            let result = js(source);
            assert!(result["styledComponents"][0]["css"].is_null());
            assert_eq!(
                result["styledComponents"][0]["dynamicExpressions"]
                    .as_array()
                    .unwrap()
                    .len(),
                1
            );
        }
    }

    #[test]
    fn constant_expansion_is_bounded_before_css_parsing() {
        let source = format!(
            "import s from 'styled-components';const TEXT='{}';const C=s.div`{}`;",
            "x".repeat(4096),
            "content:${TEXT};".repeat(2050)
        );
        let error = analyze("Resume.jsx", source.as_bytes()).unwrap_err();
        assert_eq!(error.code, "CSS_FOLD_LIMIT");
    }

    #[test]
    fn ast_preserves_component_bases_duplicate_declarations_and_nested_print_rules() {
        let source = r#"import s from 'styled-components';
import {ContactInfo} from '@jsonresume/core';
// const Fake = s.div`color: #BADBAD;`;
const string = 'const Fake = s.h1`color: red;`';
const Name = s.h1`color: #111827; color: #123456; font-size: 28pt; @media print { margin: 0; } &:hover { color: #345678; }`;
export const Contact = s(ContactInfo)`font-style: italic;`;
function Resume({basics}) {return <Name>{basics?.name}</Name>;}"#;
        let result = js(source);
        assert_eq!(result["syntaxValid"], true);
        let styles = result["styledComponents"].as_array().unwrap();
        assert_eq!(styles.len(), 2);
        assert_eq!(styles[0]["name"], "Name");
        assert_eq!(styles[0]["base"], json!({"kind":"element","name":"h1"}));
        assert_eq!(
            styles[1]["base"],
            json!({"kind":"component","name":"ContactInfo"})
        );
        assert_eq!(styles[0]["topLevelBinding"], true);
        assert_eq!(styles[1]["topLevelBinding"], true);
        let declarations = styles[0]["css"]["declarations"].as_array().unwrap();
        assert_eq!(declarations.len(), 5);
        assert_eq!(declarations[0]["value"]["text"], "#111827");
        assert_eq!(declarations[1]["value"]["text"], "#123456");
        assert_eq!(declarations[3]["scope"], json!(["@media print"]));
        assert_eq!(declarations[4]["scope"], json!(["&:hover"]));
        let start = declarations[0]["span"]["startByte"].as_u64().unwrap() as usize;
        assert!(source[start..].starts_with("color: #111827;"));
        assert_eq!(
            result["jsxExpressions"][0]["memberPath"],
            json!(["basics", "name"])
        );
        assert!(
            result["unclassifiedTemplates"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn dynamic_and_escape_bearing_css_is_reported_not_evaluated_or_falsely_parsed() {
        let result = js(r#"import styled from 'styled-components';
const A=styled.div`color: ${props=>props.color};`;
const B=styled.div`content: '\\';`;
function f(styled) {const C=styled.div`color: red;`;return C;}
const other=html`<p>${secret()}</p>`;"#);
        assert_eq!(result["syntaxValid"], true);
        let styles = result["styledComponents"].as_array().unwrap();
        assert_eq!(styles.len(), 3);
        assert_eq!(styles[0]["dynamicExpressions"].as_array().unwrap().len(), 1);
        assert!(styles[0]["css"].is_null());
        assert_eq!(styles[1]["templateEscapesRequireDecoding"], true);
        assert!(styles[1]["css"].is_null());
        assert_eq!(styles[2]["topLevelBinding"], false);
        assert_eq!(result["unclassifiedTemplates"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn global_styles_css_imports_and_field_guards_keep_their_source_context() {
        let result = js(
            r#"import {createGlobalStyle as global, css} from 'styled-components';
const G=global`@import url('https://example.com/font.css'); @page {margin: .75in;} body {font-size: 12pt;}`;
const reusable=css`margin-bottom: 4px;`;
export {DateRange} from '@jsonresume/core';
const ignored='require("not-a-dependency")';
function Resume({basics,work}) { return <div>{basics.name && <b>{basics.name}</b>}{work.map(job=><p>{job.position}</p>)}</div>; }
require('node:fs'); import('./extra.js');"#,
        );
        assert_eq!(result["syntaxValid"], true);
        assert_eq!(result["styledComponents"][0]["base"]["kind"], "global");
        assert_eq!(
            result["styledComponents"][0]["css"]["directives"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            result["styledComponents"][0]["css"]["declarations"][0]["scope"],
            json!(["@page"])
        );
        let modules: Vec<_> = result["imports"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|i| i["module"].as_str())
            .collect();
        assert_eq!(
            modules,
            [
                "styled-components",
                "@jsonresume/core",
                "node:fs",
                "./extra.js"
            ]
        );
        assert!(
            result["jsxExpressions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v["kind"] == "binary_expression")
        );
        assert!(
            result["jsxExpressions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v["memberPath"] == json!(["job", "position"]))
        );
    }

    #[test]
    fn invalid_syntax_and_unsupported_formats_are_never_marked_complete() {
        let result = js(
            "import styled from 'styled-components'; const A = styled.div`color: red;`; const broken = (",
        );
        assert_eq!(result["syntaxValid"], false);
        assert!(!result["errors"].as_array().unwrap().is_empty());
        assert!(
            analyze("source.tsx", b"const x: number = 1")
                .unwrap_or_else(|e| panic!("{}", e.message))
                .is_none()
        );
        assert!(analyze("source.js", &[255]).is_err());
        let result = analyze(
            "style.css",
            b"p { color: red; @media print { margin: 0; } }",
        )
        .unwrap_or_else(|e| panic!("{}", e.message))
        .unwrap();
        assert_eq!(result["styles"]["syntaxValid"], true);
        assert_eq!(
            result["styles"]["declarations"][1]["scope"],
            json!(["p", "@media print"])
        );
    }

    #[test]
    fn declaration_comments_and_important_are_not_silently_removed_from_evidence() {
        let result = analyze("style.css", b"p { color /* why */ : red !important; }")
            .unwrap_or_else(|e| panic!("{}", e.message))
            .unwrap();
        assert_eq!(result["styles"]["syntaxValid"], true);
        let declaration = &result["styles"]["declarations"][0];
        assert_eq!(declaration["property"], "color");
        assert_eq!(declaration["value"]["text"], "red !important");
        assert_eq!(declaration["important"], true);
    }
}
