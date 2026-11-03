//! Small, explicit Tailwind/Twind convention projection for best-effort mode.
//! Does not load configuration, plugins or JS. Unknown utilities stay unknown.
use serde_json::{Value, json};

fn declaration(token: &str) -> Option<String> {
    let value = match token {
        "text-xs" => "font-size:9pt",
        "text-sm" => "font-size:10.5pt",
        "text-base" => "font-size:12pt",
        "text-lg" => "font-size:13.5pt",
        "text-xl" => "font-size:15pt",
        "text-2xl" => "font-size:18pt",
        "text-3xl" => "font-size:22.5pt",
        "text-4xl" => "font-size:27pt",
        "font-normal" => "font-weight:400",
        "font-medium" => "font-weight:500",
        "font-semibold" => "font-weight:600",
        "font-bold" => "font-weight:700",
        "font-light" => "font-weight:300",
        "italic" => "font-style:italic",
        "not-italic" => "font-style:normal",
        "uppercase" => "text-transform:uppercase",
        "lowercase" => "text-transform:lowercase",
        "normal-case" => "text-transform:none",
        "font-sans" => "font-family:ui-sans-serif,sans-serif",
        "font-serif" => "font-family:ui-serif,serif",
        "font-mono" => "font-family:ui-monospace,monospace",
        "text-left" => "text-align:left",
        "text-center" => "text-align:center",
        "text-right" => "text-align:right",
        "text-justify" => "text-align:justify",
        "leading-tight" => "line-height:1.25",
        "leading-snug" => "line-height:1.375",
        "leading-normal" => "line-height:1.5",
        "leading-relaxed" => "line-height:1.625",
        "leading-loose" => "line-height:2",
        "tracking-tight" => "letter-spacing:-0.025em",
        "tracking-normal" => "letter-spacing:0em",
        "tracking-wide" => "letter-spacing:0.025em",
        "tracking-wider" => "letter-spacing:0.05em",
        "tracking-widest" => "letter-spacing:0.1em",
        "text-black" => "color:#000000",
        "text-white" => "color:#ffffff",
        "text-gray-500" => "color:#6b7280",
        "text-gray-600" => "color:#4b5563",
        "text-gray-700" => "color:#374151",
        "text-gray-800" => "color:#1f2937",
        "text-gray-900" => "color:#111827",
        "bg-white" => "background:#ffffff",
        _ => return None,
    };
    Some(format!("{value};"))
}

pub(crate) fn project(
    node: tree_sitter::Node<'_>,
    source: &str,
    file: &str,
) -> Option<(String, Value)> {
    if !matches!(
        node.kind(),
        "jsx_opening_element" | "jsx_self_closing_element"
    ) {
        return None;
    }
    let name = node.child_by_field_name("name")?;
    let tag = &source[name.byte_range()];
    let element = node
        .parent()
        .filter(|p| p.kind() == "jsx_element")
        .unwrap_or(node);
    if element.byte_range().len() > 65536 {
        return None;
    }
    let content = &source[element.byte_range()];
    let role = match tag {
        "h1" => "name",
        "h2" => "section-title",
        "h3" => "item-title",
        "time" => "date",
        "main" => "body",
        "p" if content.contains("basics.label") => "label",
        "p" if content.contains("basics.summary") => "summary",
        _ => return None,
    };
    let attribute = (0..node.named_child_count())
        .filter_map(|i| node.named_child(i))
        .find(|n| {
            n.kind() == "jsx_attribute"
                && n.named_child(0)
                    .is_some_and(|key| &source[key.byte_range()] == "className")
        })?;
    let value = attribute.named_child(1).filter(|n| n.kind() == "string")?;
    let raw = &source[value.byte_range()];
    // JSX attribute literals are not JS cooked strings. No entities/escapes are
    // guessed, and dynamic cn(), variants, spreads and expressions are skipped.
    let classes = raw.get(1..raw.len() - 1)?;
    if classes.contains(['&', '\\']) || classes.len() > 4096 {
        return None;
    }
    let mut css = String::new();
    let mut used = Vec::new();
    let mut unknown = Vec::new();
    for token in classes.split_ascii_whitespace() {
        if let Some(d) = declaration(token) {
            css.push_str(&d);
            used.push(token);
        } else {
            unknown.push(token);
        }
    }
    if css.is_empty() {
        return None;
    }
    let record = json!({"file":file,"startByte":attribute.start_byte(),"endByte":attribute.end_byte(),
        "tag":tag,"role":role,"literalClasses":classes,"projected":used,"unresolved":unknown,
        "policy":"explicit-common-utility-conventions-only","configurationVerified":false,"equivalent":false,
        "limitation":"仅静态 className 与固有标签/字面字段的角色推断；不加载配置、插件、响应式或动态组合，不证明组件被挂载"});
    Some((format!("#{role}{{{css}}}"), record))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_literal_intrinsic_classes_and_explicit_conventions_are_projected() {
        let source = "const Hero=()=> <h1 className=\"text-2xl font-bold md:text-4xl text-brand\">{basics.name}</h1>;";
        let (tree, _) = crate::syntax::parse_javascript("hero.jsx", source.as_bytes()).unwrap();
        let mut nodes = vec![tree.root_node()];
        let mut found = Vec::new();
        while let Some(node) = nodes.pop() {
            if let Some(result) = project(node, source, "hero.jsx") {
                found.push(result);
            }
            for n in 0..node.named_child_count() {
                nodes.push(node.named_child(n).unwrap());
            }
        }
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0, "#name{font-size:18pt;font-weight:700;}");
        assert_eq!(
            found[0].1["unresolved"],
            json!(["md:text-4xl", "text-brand"])
        );
        assert_eq!(found[0].1["configurationVerified"], false);
        assert!(declaration("text-[37px]").is_none());
        assert!(declaration("print:text-sm").is_none());
    }
}
