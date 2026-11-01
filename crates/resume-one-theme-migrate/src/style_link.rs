//! Symbolic styled-component inheritance. This is not computed CSS/layout.
//! Keep layers and declarations in source order; never flatten as token hints.

use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

const MAX_STYLES: usize = 1024;
const MAX_DEPTH: usize = 64;
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
    "img",
];

struct Definition<'a> {
    style: &'a Value,
    binding: &'a Value,
}

fn issue(code: &str, name: &str, span: &Value, message: &str) -> Value {
    json!({"code":code,"component":name,"span":span,"message":message,"stage":"style-inheritance"})
}

pub(crate) fn link(path: &str, analysis: &Value, program: &Value) -> Result<Value, String> {
    let styles = analysis["styledComponents"]
        .as_array()
        .ok_or("STYLE_LINK_INPUT: JavaScript evidence required")?;
    if styles.len() > MAX_STYLES {
        return Err("STYLE_LINK_LIMIT: more than 1024 styled definitions".into());
    }
    let mut definitions = BTreeMap::new();
    let mut duplicates = BTreeSet::new();
    let mut blockers = Vec::new();
    let mut imports = BTreeMap::new();
    for dependency in program["dependencies"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|d| d["kind"] == "import")
    {
        for specifier in dependency["specifiers"].as_array().into_iter().flatten() {
            if let (Some(local), Some(module), Some(exported)) = (
                specifier["local"].as_str(),
                dependency["module"].as_str(),
                specifier["exported"].as_str(),
            ) && imports.insert(local, (module, exported)).is_some()
            {
                duplicates.insert(local.to_owned());
            }
        }
    }
    for style in styles {
        let Some(name) = style["name"].as_str() else {
            blockers.push(issue(
                "STYLE_BINDING_UNRESOLVED",
                "",
                &style["span"],
                "styled definition has no explicit lexical name",
            ));
            continue;
        };
        let bindings = program["bindings"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|b| {
                b["name"] == name
                    && b["scope"] == "module"
                    && b["value"]["kind"] == "styled-reference"
                    && b["value"]["span"]["startByte"] == style["span"]["startByte"]
            })
            .collect::<Vec<_>>();
        if bindings.len() != 1 || style["topLevelBinding"] != true {
            blockers.push(issue(
                "STYLE_BINDING_UNRESOLVED",
                name,
                &style["span"],
                "definition is not a unique, initialized module-scope styled binding",
            ));
            continue;
        }
        if definitions
            .insert(
                name,
                Definition {
                    style,
                    binding: bindings[0],
                },
            )
            .is_some()
        {
            duplicates.insert(name.to_owned());
        }
        if style["css"].is_null() || style["css"]["syntaxValid"] != true {
            blockers.push(issue("STYLE_RULES_UNRESOLVED",name,&style["span"],"CSS contains unresolved expressions, escapes or parse errors; no partial style is substituted"));
        }
    }
    for name in &duplicates {
        blockers.push(issue(
            "STYLE_SYMBOL_COLLISION",
            name,
            &Value::Null,
            "duplicate style/import name cannot be linked",
        ));
    }
    let mut nodes = Vec::new();
    // Preserve author order, not the lookup map's lexical order.
    for style in styles {
        let Some(name) = style["name"].as_str() else {
            continue;
        };
        let Some(definition) = definitions.get(name) else {
            continue;
        };
        if definition.style["span"] != style["span"] {
            continue;
        }
        let mut chain = Vec::new();
        let mut seen = BTreeSet::new();
        let mut current = name;
        let mut terminal = Value::Null;
        let mut linked = false;
        for depth in 0..=MAX_DEPTH {
            let d = &definitions[current];
            if depth == MAX_DEPTH {
                blockers.push(issue(
                    "STYLE_INHERITANCE_LIMIT",
                    name,
                    &style["span"],
                    "component inheritance exceeds depth 64",
                ));
                break;
            }
            if duplicates.contains(current) || !seen.insert(current) {
                blockers.push(issue(
                    "STYLE_INHERITANCE_CYCLE",
                    name,
                    &style["span"],
                    "cyclic or ambiguous component inheritance",
                ));
                break;
            }
            chain.push(d.binding["id"].clone());
            let base = &d.style["base"];
            match base["kind"].as_str() {
                Some("element") => {
                    let tag = base["name"].as_str().unwrap_or("");
                    terminal = json!({"kind":"intrinsic","name":tag});
                    if TAGS.contains(&tag) {
                        linked = true;
                    } else {
                        blockers.push(issue(
                            "STYLE_ELEMENT_UNSUPPORTED",
                            name,
                            &d.style["span"],
                            "native element is outside the finite visual tag list",
                        ));
                    }
                    break;
                }
                Some("component") => {
                    let next = base["name"].as_str().unwrap_or("");
                    if let Some(parent) = definitions.get(next) {
                        if parent.style["span"]["startByte"].as_u64()
                            >= d.style["span"]["startByte"].as_u64()
                        {
                            blockers.push(issue(
                                "STYLE_BASE_TDZ",
                                name,
                                &d.style["span"],
                                "styled base is not initialized before its derived definition",
                            ));
                            break;
                        }
                        current = next;
                    } else if let Some((module, exported)) = imports.get(next) {
                        terminal = json!({"kind":"import","module":module,"exported":exported,"local":next});
                        blockers.push(issue("STYLE_EXTERNAL_BASE_UNRESOLVED",name,&d.style["span"],"external base requires a pinned package and its primitive CSS/structure, not a generic wrapper"));
                        break;
                    } else {
                        terminal = json!({"kind":"unresolved","name":next});
                        blockers.push(issue(
                            "STYLE_BASE_UNRESOLVED",
                            name,
                            &d.style["span"],
                            "base is not a linked styled definition or explicit imported symbol",
                        ));
                        break;
                    }
                }
                _ => {
                    blockers.push(issue("STYLE_BASE_UNSUPPORTED",name,&d.style["span"],"global styles/fragments require a distinct sheet adapter, not a visual component"));
                    break;
                }
            }
        }
        chain.reverse();
        nodes.push(json!({"id":definition.binding["id"],"name":name,"base":style["base"],"terminal":terminal,"inheritanceLinked":linked,"layers":chain,"rules":style["css"],"span":style["span"]}));
    }
    Ok(
        json!({"format":"resume-one-style-inheritance-v1","source":path,"scope":"explicit styled definitions in this module only; not JavaScript/CSS safety or computed cascade",
        "inheritanceLinked":!nodes.is_empty() && blockers.is_empty(),"layoutCompiled":false,"nodes":nodes,"blockers":blockers,
        "cascadeContract":"layers are base-to-derived identifiers; keep each layer's declarations, selectors, media and duplicates in original order; never replace CSS cascade, font inheritance, margin collapse or print layout with this graph"}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn graph(source: &str) -> Value {
        let syntax = crate::syntax::analyze("Resume.jsx", source.as_bytes())
            .unwrap()
            .unwrap();
        let program = crate::template_ir::compile("Resume.jsx", source.as_bytes()).unwrap();
        link("Resume.jsx", &syntax, &program).unwrap()
    }
    fn codes(graph: &Value) -> Vec<&str> {
        graph["blockers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| b["code"].as_str().unwrap())
            .collect()
    }
    #[test]
    fn links_real_module_bindings_and_ordered_base_to_derived_layers_without_flattening() {
        let g = graph(
            "import s from 'styled-components';const Base=s.div`color:red;color:blue;`;const Card=s(Base)`padding:8px;@media print{padding:0;}`;export default()=> <Card/>;",
        );
        assert_eq!(g["inheritanceLinked"], true);
        assert_eq!(g["layoutCompiled"], false);
        let n = g["nodes"].as_array().unwrap();
        assert_eq!(n[1]["layers"], json!([n[0]["id"], n[1]["id"]]));
        assert_eq!(n[1]["terminal"], json!({"kind":"intrinsic","name":"div"}));
        assert_eq!(n[0]["rules"]["declarations"].as_array().unwrap().len(), 2);
        assert_eq!(
            n[1]["rules"]["declarations"][1]["scope"],
            json!(["@media print"])
        );
    }
    #[test]
    fn external_primitives_are_not_replaced_by_generic_sections() {
        let g = graph(
            "import s from 'styled-components';import {SectionTitle as Heading} from '@jsonresume/core';const Title=s(Heading)`font-size:16pt;`;export default()=> <Title/>;",
        );
        assert_eq!(g["inheritanceLinked"], false);
        assert_eq!(
            g["nodes"][0]["terminal"],
            json!({"kind":"import","module":"@jsonresume/core","exported":"SectionTitle","local":"Heading"})
        );
        assert!(codes(&g).contains(&"STYLE_EXTERNAL_BASE_UNRESOLVED"));
    }
    #[test]
    fn forward_bases_dynamic_styles_mutable_and_nested_bindings_stay_unresolved() {
        let g = graph(
            "import s from 'styled-components';const A=s(B)`color:red;`;const B=s.div`padding:${p=>p.gap};`;let C=s.div`color:black;`;function f(){const D=s.div`color:red;`;}export default()=> <A/>;",
        );
        let c = codes(&g);
        for code in [
            "STYLE_BASE_TDZ",
            "STYLE_RULES_UNRESOLVED",
            "STYLE_BINDING_UNRESOLVED",
        ] {
            assert!(c.contains(&code), "{g}");
        }
        assert_eq!(g["inheritanceLinked"], false);
    }
    #[test]
    fn global_fragments_and_nonvisual_elements_do_not_claim_a_visual_root() {
        let g = graph(
            "import s from 'styled-components';const X=s.script`color:red;`;export default()=> <X/>;",
        );
        assert!(codes(&g).contains(&"STYLE_ELEMENT_UNSUPPORTED"));
        assert_eq!(g["inheritanceLinked"], false);
    }
    #[test]
    fn unsupported_helper_component_and_no_styles_are_not_complete() {
        let g = graph(
            "import s from 'styled-components';function Helper(){return <p/>;}const X=s(Helper)`color:red;`;export default()=> <X/>;",
        );
        assert!(codes(&g).contains(&"STYLE_BASE_UNRESOLVED"));
        let g = graph("export default()=> <p/>;");
        assert_eq!(g["inheritanceLinked"], false);
        assert!(g["nodes"].as_array().unwrap().is_empty());
    }
}
