//! Extract literal CSS data from JS syntax. No imports, helper calls or template
//! substitutions are evaluated. The data's actual runtime reachability is not proven.
use crate::{AppError, source_bundle, syntax};
use serde_json::{Value, json};
use std::{collections::BTreeSet, path::Path};

fn decode(raw: &str) -> Option<String> {
    decode_inner(raw.get(1..raw.len().checked_sub(1)?)?)
}

/// Cook only literal escape sequences, never substitutions or arbitrary JS.
/// Legacy octal escapes are deliberately unsupported rather than guessed.
fn decode_inner(raw: &str) -> Option<String> {
    let mut chars = raw.chars().peekable();
    let mut cooked = String::new();
    while let Some(c) = chars.next() {
        if c != '\\' {
            cooked.push(c);
            continue;
        }
        match chars.next()? {
            'n' => cooked.push('\n'),
            'r' => cooked.push('\r'),
            't' => cooked.push('\t'),
            'b' => cooked.push('\u{8}'),
            'f' => cooked.push('\u{c}'),
            'v' => cooked.push('\u{b}'),
            '0' if !chars.peek().is_some_and(char::is_ascii_digit) => cooked.push('\0'),
            '0'..='9' => return None,
            '\n' => {}
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
            }
            'x' => {
                let digits: String = chars.by_ref().take(2).collect();
                if digits.len() != 2 {
                    return None;
                }
                cooked.push(char::from_u32(u32::from_str_radix(&digits, 16).ok()?)?);
            }
            'u' => {
                let code = if chars.peek() == Some(&'{') {
                    chars.next();
                    let mut digits = String::new();
                    loop {
                        match chars.next()? {
                            '}' => break,
                            digit if digit.is_ascii_hexdigit() && digits.len() < 6 => {
                                digits.push(digit)
                            }
                            _ => return None,
                        }
                    }
                    u32::from_str_radix(&digits, 16).ok()?
                } else {
                    let digits: String = chars.by_ref().take(4).collect();
                    if digits.len() != 4 {
                        return None;
                    }
                    u32::from_str_radix(&digits, 16).ok()?
                };
                // Paired UTF-16 surrogates are uncommon in styles, but must not
                // silently turn into two unrelated replacement characters.
                let code = if (0xD800..=0xDBFF).contains(&code) {
                    if chars.next() != Some('\\') || chars.next() != Some('u') {
                        return None;
                    }
                    let digits: String = chars.by_ref().take(4).collect();
                    if digits.len() != 4 {
                        return None;
                    }
                    let low = u32::from_str_radix(&digits, 16).ok()?;
                    if !(0xDC00..=0xDFFF).contains(&low) {
                        return None;
                    }
                    0x10000 + ((code - 0xD800) << 10) + low - 0xDC00
                } else {
                    code
                };
                cooked.push(char::from_u32(code)?);
            }
            escaped => cooked.push(escaped),
        }
    }
    Some(cooked)
}

fn literal_parts(node: tree_sitter::Node<'_>, source: &str) -> Vec<(String, usize)> {
    if node.kind() != "template_string" {
        return decode(&source[node.byte_range()])
            .map(|text| (text, node.start_byte() + 1))
            .into_iter()
            .collect();
    }
    let mut parts = Vec::new();
    let mut start = node.start_byte() + 1;
    // A dynamic HTML template can still have a wholly literal <style> block.
    // Never join fragments across an interpolation or evaluate that expression.
    for index in 0..node.named_child_count() {
        let child = node.named_child(index).unwrap();
        if child.kind() == "template_substitution" {
            if let Some(part) = decode_inner(&source[start..child.start_byte()]) {
                parts.push((part, start));
            }
            start = child.end_byte();
        }
    }
    if let Some(part) = decode_inner(&source[start..node.end_byte() - 1]) {
        parts.push((part, start));
    }
    parts
}

fn candidates(text: &str) -> Vec<String> {
    let lower = text.to_ascii_lowercase();
    if lower.contains("<style") {
        let mut found = Vec::new();
        let mut offset = 0;
        while let Some(at) = lower[offset..].find("<style") {
            let start = offset + at;
            let Some(gt) = lower[start..].find('>').map(|n| n + start + 1) else {
                break;
            };
            let Some(end) = lower[gt..].find("</style>").map(|n| n + gt) else {
                break;
            };
            let body = &text[gt..end];
            if !body.contains("${") && !body.contains("{{") {
                found.push(body.to_owned());
            }
            offset = end + 8;
        }
        found
    } else if text.contains('{')
        && text.contains('}')
        && text.contains(':')
        && !text.contains("${")
        && !text.trim_start().starts_with('<')
        && !text.contains("{{")
    {
        vec![text.to_owned()]
    } else {
        Vec::new()
    }
}

pub(crate) fn augment(report: &mut Value, root: &Path) -> Result<(), AppError> {
    let snapshot =
        source_bundle::snapshot(root).map_err(|e| AppError::new(3, e.code, e.message))?;
    if report["sourceSnapshot"]["sourceSha256"] != snapshot.source_sha256 {
        return Err(AppError::new(
            3,
            "ADAPT_CHANGED",
            "嵌入样式读取期间来源变化",
        ));
    }
    let mut records = Vec::new();
    let mut headings = crate::markup_hints::Headings::default();
    let mut utilities = Vec::new();
    let mut programs = Vec::new();
    let mut program_issues = Vec::new();
    let mut program_bytes = 0usize;
    let utility_package = snapshot
        .files
        .iter()
        .find(|f| f.relative_path == "package.json")
        .and_then(|f| serde_json::from_slice::<Value>(&f.bytes).ok())
        .is_some_and(|package| {
            ["dependencies", "devDependencies", "peerDependencies"]
                .iter()
                .any(|kind| {
                    package[kind].get("twind").is_some()
                        || package[kind].get("tailwindcss").is_some()
                })
        });
    let mut seen = BTreeSet::new();
    let mut additions = Vec::new();
    let mut total = 0;
    for file in &snapshot.files {
        if matches!(
            file.relative_path.rsplit('.').next(),
            Some("html" | "hbs" | "mustache" | "template")
        ) && let Some(text) = file.text()
        {
            headings
                .literal(text, &file.relative_path, 0, false)
                .map_err(|e| AppError::new(3, "ADAPT_MARKUP", e))?;
        }
        if !matches!(
            file.relative_path.rsplit('.').next(),
            Some("js" | "jsx" | "mjs" | "cjs")
        ) {
            continue;
        }
        let Some(source) = file.text() else {
            continue;
        };
        let (tree, _) = syntax::parse_javascript(&file.relative_path, &file.bytes)
            .map_err(|e| AppError::new(3, e.code, e.message))?;
        if tree.root_node().has_error() {
            continue;
        }
        let mut nodes = vec![tree.root_node()];
        let mut visited = 0;
        let mut has_jsx = false;
        while let Some(node) = nodes.pop() {
            has_jsx |= matches!(node.kind(), "jsx_element" | "jsx_self_closing_element");
            visited += 1;
            if visited > 100_000 {
                return Err(AppError::new(
                    3,
                    "ADAPT_EMBEDDED_LIMIT",
                    "单个 JS 文件节点过多",
                ));
            }
            headings
                .jsx(node, source, &file.relative_path)
                .map_err(|e| AppError::new(3, "ADAPT_MARKUP", e))?;
            if utility_package
                && let Some((css, record)) =
                    crate::utility_style::project(node, source, &file.relative_path)
            {
                if utilities.len() >= 4096 {
                    return Err(AppError::new(
                        3,
                        "ADAPT_UTILITY_LIMIT",
                        "静态 utility 记录超过 4096 项",
                    ));
                }
                let name = format!("{}#utility-{}.css", file.relative_path, node.start_byte());
                if let Some(analysis) = syntax::analyze(&name, css.as_bytes())
                    .map_err(|e| AppError::new(3, e.code, e.message))?
                {
                    additions.push(analysis);
                    utilities.push(record);
                }
            }
            if matches!(node.kind(), "string" | "template_string") {
                let raw = &source[node.byte_range()];
                if raw.len() <= 1024 * 1024 {
                    for (decoded, origin) in literal_parts(node, source) {
                        headings
                            .literal(&decoded, &file.relative_path, origin, true)
                            .map_err(|e| AppError::new(3, "ADAPT_MARKUP", e))?;
                        for css in candidates(&decoded) {
                            if !seen.insert(crate::sha256(css.as_bytes())) {
                                continue;
                            }
                            total += css.len();
                            if total > 4 * 1024 * 1024 || additions.len() >= 128 {
                                return Err(AppError::new(
                                    3,
                                    "ADAPT_EMBEDDED_LIMIT",
                                    "嵌入 CSS 超过 4 MiB 或 128 个片段",
                                ));
                            }
                            let name =
                                format!("{}#literal-{}.css", file.relative_path, node.start_byte());
                            if let Some(analysis) = syntax::analyze(&name, css.as_bytes())
                                .map_err(|e| AppError::new(3, e.code, e.message))?
                                && analysis["styles"]["declarations"]
                                    .as_array()
                                    .is_some_and(|a| !a.is_empty())
                            {
                                records.push(json!({"file":file.relative_path,"sourceSha256":file.sha256,"startByte":node.start_byte(),"endByte":node.end_byte(),"cssSha256":crate::sha256(css.as_bytes()),"syntaxValid":analysis["styles"]["syntaxValid"],"runtimeReachabilityVerified":false,"equivalent":false}));
                                additions.push(analysis);
                            }
                        }
                    }
                }
                // Nested templates inside a dynamic template may contain literal CSS.
            }
            for i in (0..node.named_child_count()).rev() {
                nodes.push(node.named_child(i).unwrap());
            }
        }
        if has_jsx {
            if programs.len() >= 8 {
                program_issues
                    .push(json!({"file":file.relative_path,"code":"ADAPT_TEMPLATE_ENTRY_LIMIT"}));
                continue;
            }
            match crate::template_ir::compile(&file.relative_path,&file.bytes) {
                Ok(program)=>{
                    let analysis = syntax::analyze(&file.relative_path, &file.bytes)
                        .map_err(|e| AppError::new(3,e.code,e.message))?;
                    let graph = analysis.as_ref().map(|analysis|crate::style_link::link(&file.relative_path,analysis,&program)).transpose()
                        .map_err(|e|AppError::new(3,"ADAPT_STYLE_GRAPH",e))?;
                    let bytes=serde_json::to_vec(&program).map_or(usize::MAX,|v|v.len())
                        .saturating_add(serde_json::to_vec(&graph).map_or(usize::MAX,|v|v.len()));
                    if program_bytes.saturating_add(bytes)>4*1024*1024 {
                        program_issues.push(json!({"file":file.relative_path,"code":"ADAPT_TEMPLATE_BYTES_LIMIT"}));
                        continue;
                    }
                    program_bytes+=bytes;
                    programs.push(json!({"entryFile":file.relative_path,"sourceSha256":file.sha256,"program":program,"styleGraph":graph,"adaptationOnly":true}));
                }
                Err(error)=>program_issues.push(json!({"file":file.relative_path,"code":"ADAPT_TEMPLATE_UNSUPPORTED","message":error})),
            }
        }
    }
    report["syntaxEvidence"]
        .as_array_mut()
        .expect("inspection syntax evidence")
        .extend(additions);
    headings.apply(report);
    report["templatePrograms"] = json!(programs);
    report["adaptTemplateInspection"] = json!({"programBytes":program_bytes,"issues":program_issues,"sourceExecuted":false,"runtimeEntryReachabilityVerified":false});
    report["embeddedStyles"] = json!({"literalStyles":records,"executedSourceCode":false,"limitation":"仅恢复真实 JS 字符串或无动态插值的 style 区域作为数据；未证明加载顺序、最终使用或原模板等价"});
    report["utilityStyles"] = json!({"packageDeclaresTwindOrTailwind":utility_package,"projections":utilities,"configurationVerified":false,"equivalent":false});
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn literal_data_is_decoded_without_running_javascript() {
        assert_eq!(
            decode(r#""body{font-size:12pt;}""#).unwrap(),
            "body{font-size:12pt;}"
        );
        assert!(decode("`body{content:\\41;}`").is_none());
        assert_eq!(
            decode(r#"`body{content:"\\\\f052";}\n`"#).unwrap(),
            "body{content:\"\\\\f052\";}\n"
        );
        assert_eq!(decode(r#"'\uD83D\uDE00'"#).unwrap(), "😀");
        assert_eq!(
            candidates("<html>${name}<style>h1{color:red;}</style>${body}</html>"),
            ["h1{color:red;}"]
        );
        assert!(candidates("<style>h1{color:${color};}</style>").is_empty());
        assert!(candidates("<style>{{{styles}}}</style>").is_empty());
        assert_eq!(
            candidates("/* internal < entry */ body{color:#123456;}").len(),
            1
        );
    }

    #[test]
    fn dynamic_html_keeps_only_complete_literal_styles() {
        let source = r#"export const page=`<title>${name}</title><style>body{font-family:'Inter';color:#123456;}</style><a>${url.replace(/\\//g,'')}</a>`;"#;
        let (tree, _) = syntax::parse_javascript("template.js", source.as_bytes()).unwrap();
        assert!(!tree.root_node().has_error());
        let mut pending = vec![tree.root_node()];
        let mut styles = Vec::new();
        while let Some(node) = pending.pop() {
            if node.kind() == "template_string" {
                for (part, _) in literal_parts(node, source) {
                    styles.extend(candidates(&part));
                }
            }
            for n in 0..node.named_child_count() {
                pending.push(node.named_child(n).unwrap());
            }
        }
        assert_eq!(styles, ["body{font-family:'Inter';color:#123456;}"]);
        let source = "const s=`<style>h1{color:${color};}</style>`;";
        let (tree, _) = syntax::parse_javascript("dynamic.js", source.as_bytes()).unwrap();
        let template = tree
            .root_node()
            .named_child(0)
            .unwrap()
            .named_child(0)
            .unwrap()
            .child_by_field_name("value")
            .unwrap();
        assert!(
            literal_parts(template, source)
                .iter()
                .flat_map(|(part, _)| candidates(part))
                .next()
                .is_none()
        );
    }
}
