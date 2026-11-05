//! Bounded projection of existing CSS syntax evidence into a print profile.
//! This never reads source, evaluates expressions, or implements a CSS cascade.

use std::collections::BTreeMap;

use serde_json::{Value, json};

const FORMAT: &str = "resume-one-css-print-profile-v1";
const ANNOTATION: &str = "printProfileProjection";
const MAX_DECLARATIONS: usize = 48_000;
const MAX_FILES: usize = 4096;
const MAX_BLOCKS: usize = 48_000;
const MAX_SCOPE_DEPTH: usize = 128;
const MAX_SCOPE_BYTES: usize = 16 * 1024;
const MAX_MEDIA_ALTERNATIVES: usize = 32;
const MAX_EVIDENCE: usize = 512;

struct Block {
    path: String,
    file: String,
    component: Value,
    static_style: bool,
}

struct Projection {
    original_index: usize,
    scope: Vec<Value>,
    removed: Vec<Value>,
}

struct Plan {
    block: Block,
    original_count: usize,
    projections: Vec<Projection>,
}

fn css_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\r' | '\n' | '\u{c}')
}

/// Comments separate tokens; `pr/**/int` must never turn into `print`.
fn header_without_comments(header: &str) -> Option<String> {
    let mut tail = header;
    let mut result = String::new();
    while let Some(start) = tail.find("/*") {
        result.push_str(&tail[..start]);
        result.push(' ');
        let end = tail[start + 2..].find("*/")? + start + 4;
        tail = &tail[end..];
    }
    result.push_str(tail);
    Some(result)
}

/// A deliberately closed grammar. Feature queries, `not`, escaped identifiers,
/// and unknown media types are rejected even when another OR arm says print.
fn positive_media(header: &str) -> Result<(), &'static str> {
    let header = header_without_comments(header).ok_or("unsupportedMediaCondition")?;
    let header = header.trim_matches(css_space);
    let end = header.find(css_space).unwrap_or(header.len());
    if !header[..end].eq_ignore_ascii_case("@media") {
        return Err("unsupportedAtRule");
    }
    let mut positive = false;
    for (index, alternative) in header[end..].split(',').enumerate() {
        if index >= MAX_MEDIA_ALTERNATIVES {
            return Err("mediaAlternativeLimit");
        }
        let mut tokens = alternative.split(css_space).filter(|s| !s.is_empty());
        let mut medium = tokens.next().ok_or("unsupportedMediaCondition")?;
        if medium.eq_ignore_ascii_case("only") {
            medium = tokens.next().ok_or("unsupportedMediaCondition")?;
        }
        if tokens.next().is_some() {
            return Err("unsupportedMediaCondition");
        }
        if medium.eq_ignore_ascii_case("print") || medium.eq_ignore_ascii_case("all") {
            positive = true;
        } else if !medium.eq_ignore_ascii_case("screen") {
            return Err("unsupportedMediaCondition");
        }
    }
    if positive {
        Ok(())
    } else {
        Err("nonPrintMedia")
    }
}

/// syntax::css records ordinary `block` ancestors, but the parser's
/// keyframe_block_list is a different node type. The @keyframes wrapper is
/// therefore absent from scope evidence; only `from`, `to`, or numeric step
/// lists remain. Reject those ambiguous heads wherever they occur, including
/// during unconditional replay, rather than inventing a missing ancestor.
fn keyframe_like_scope(head: &str) -> bool {
    let Some(head) = header_without_comments(head) else {
        return false;
    };
    head.split(',').all(|step| {
        let step = step.trim_matches(css_space);
        if step.eq_ignore_ascii_case("from") || step.eq_ignore_ascii_case("to") {
            return true;
        }
        let number = step.strip_prefix(['+', '-']).unwrap_or(step);
        // A number cannot start an unescaped CSS type/class selector. Be
        // conservative about malformed units/ranges too; do not need a numeric
        // evaluator to recognize a potentially omitted keyframe wrapper.
        let number = number.strip_prefix('.').unwrap_or(number);
        number.as_bytes().first().is_some_and(u8::is_ascii_digit)
    })
}

/// Returns retained selector scopes and removed, positively matched media heads.
/// All ancestors must qualify: an inner print rule cannot escape outer screen,
/// supports, layer, keyframes, or another unverified condition.
fn profile_scope(declaration: &Value) -> Result<(Vec<Value>, Vec<Value>), &'static str> {
    let scope = declaration["scope"].as_array().ok_or("invalidScope")?;
    if scope.len() > MAX_SCOPE_DEPTH {
        return Err("scopeLimit");
    }
    let mut bytes = 0usize;
    let mut retained = Vec::new();
    let mut removed = Vec::new();
    for entry in scope {
        let head = entry.as_str().ok_or("invalidScope")?;
        bytes = bytes.saturating_add(head.len());
        if bytes > MAX_SCOPE_BYTES {
            return Err("scopeLimit");
        }
        let head = head.trim_matches(css_space);
        if head.is_empty() {
            return Err("invalidScope");
        }
        if head.starts_with('@') && head != "@page" {
            positive_media(head)?;
            removed.push(entry.clone());
        } else {
            if keyframe_like_scope(head) {
                return Err("keyframeScopeUnverified");
            }
            // @page is already an explicitly supported, non-environmental
            // scope in collect_css. Keep it intact instead of promoting its
            // declarations to ordinary element declarations.
            retained.push(entry.clone());
        }
    }
    Ok((retained, removed))
}

fn owned_projection(declaration: &Value) -> bool {
    declaration[ANNOTATION]["format"].as_str() == Some(FORMAT)
}

fn source_span_present(declaration: &Value) -> bool {
    let span = &declaration["span"];
    matches!(
        (span["file"].as_str(), span["startByte"].as_u64(), span["endByte"].as_u64()),
        (Some(_), Some(start), Some(end)) if start <= end
    )
}

fn blocks(report: &Value) -> Result<Vec<Block>, String> {
    let files = report
        .get("syntaxEvidence")
        .and_then(Value::as_array)
        .ok_or("PRINT_PROFILE_INPUT: inspection.syntaxEvidence must be an array")?;
    if files.len() > MAX_FILES {
        return Err("PRINT_PROFILE_LIMIT: more than 4096 syntax files".into());
    }
    let mut result = Vec::new();
    let mut style_records = 0usize;
    for (file_index, file) in files.iter().enumerate() {
        if !file.is_object() {
            return Err("PRINT_PROFILE_INPUT: syntax file must be an object".into());
        }
        let name = file["file"].as_str().unwrap_or("<unknown>");
        if file["styles"].is_object() {
            result.push(Block {
                path: format!("/syntaxEvidence/{file_index}/styles"),
                file: name.into(),
                component: Value::Null,
                static_style: true,
            });
        }
        if let Some(styles) = file["styledComponents"].as_array() {
            style_records = style_records.saturating_add(styles.len());
            if style_records > MAX_BLOCKS {
                return Err("PRINT_PROFILE_LIMIT: too many styled component records".into());
            }
            for (style_index, style) in styles.iter().enumerate() {
                if style["css"].is_object() {
                    result.push(Block {
                        path: format!(
                            "/syntaxEvidence/{file_index}/styledComponents/{style_index}/css"
                        ),
                        file: name.into(),
                        component: style["name"].clone(),
                        static_style: style["topLevelBinding"] == true
                            && style["templateEscapesRequireDecoding"] != true
                            && style["dynamicExpressions"]
                                .as_array()
                                .is_some_and(Vec::is_empty),
                    });
                }
            }
        }
        if result.len() > MAX_BLOCKS {
            return Err("PRINT_PROFILE_LIMIT: too many CSS blocks".into());
        }
    }
    Ok(result)
}

/// Append annotated profile declarations to the existing syntax evidence.
/// Originals, values, importance, selectors, and source spans stay intact.
///
/// After the first eligible media declaration, replay subsequent unconditional
/// declarations too. Appending just the media declarations would incorrectly
/// make an early print rule beat a later ordinary rule. This preserves the AST
/// declaration sequence within a block without claiming DOM/cascade fidelity.
/// Owned projections are rebuilt on repeat calls, making expansion idempotent.
/// Preflight every block and the full output budget before changing the report.
pub(crate) fn project(report: &mut Value) -> Result<(), String> {
    let blocks = blocks(report)?;
    let mut plans = Vec::new();
    let mut input_total = 0usize;
    let mut original_total = 0usize;
    let mut projected_total = 0usize;
    let mut media_total = 0usize;
    let mut replay_total = 0usize;
    let mut skipped = BTreeMap::<&str, usize>::new();
    for block in blocks {
        let css = report.pointer(&block.path).expect("discovered CSS block");
        let declarations = css["declarations"]
            .as_array()
            .ok_or("PRINT_PROFILE_INPUT: CSS declarations must be an array")?;
        input_total = input_total.saturating_add(declarations.len());
        if input_total > MAX_DECLARATIONS {
            return Err("PRINT_PROFILE_LIMIT: more than 48000 input declarations".into());
        }
        let mut plan = Plan {
            block,
            original_count: 0,
            projections: Vec::new(),
        };
        let mut started = false;
        for declaration in declarations.iter().filter(|d| !owned_projection(d)) {
            let original_index = plan.original_count;
            plan.original_count += 1;
            original_total += 1;
            let rejected = if !plan.block.static_style {
                Some("dynamicOrScopedStyle")
            } else if css["syntaxValid"] != true {
                Some("invalidCssBlock")
            } else {
                None
            };
            if let Some(reason) = rejected {
                *skipped.entry(reason).or_default() += 1;
                continue;
            }
            let (scope, removed) = match profile_scope(declaration) {
                Ok(result) => result,
                Err(reason) => {
                    *skipped.entry(reason).or_default() += 1;
                    continue;
                }
            };
            if removed.is_empty() && !started {
                continue;
            }
            if !source_span_present(declaration) {
                *skipped.entry("missingSourceSpan").or_default() += 1;
                continue;
            }
            if removed.is_empty() {
                replay_total += 1;
            } else {
                started = true;
                media_total += 1;
            }
            projected_total += 1;
            plan.projections.push(Projection {
                original_index,
                scope,
                removed,
            });
        }
        plans.push(plan);
    }
    if original_total.saturating_add(projected_total) > MAX_DECLARATIONS {
        return Err("PRINT_PROFILE_LIMIT: projection would exceed 48000 declarations".into());
    }

    let mut evidence = Vec::new();
    let mut affected_blocks = 0usize;
    let block_count = plans.len();
    for plan in plans {
        if !plan.projections.is_empty() {
            affected_blocks += 1;
        }
        let declarations = report
            .pointer_mut(&plan.block.path)
            .and_then(|css| css["declarations"].as_array_mut())
            .expect("preflight checked declarations");
        declarations.retain(|d| !owned_projection(d));
        for (profile_index, projection) in plan.projections.into_iter().enumerate() {
            let original = &declarations[projection.original_index];
            let mut projected = original.clone();
            let kind = if projection.removed.is_empty() {
                "unconditional-source-order-replay"
            } else {
                "positive-print-media"
            };
            let annotation = json!({
                "format":FORMAT, "profile":"print", "kind":kind,
                "originalDeclarationIndex":projection.original_index,
                "projectedDeclarationIndex":plan.original_count + profile_index,
                "originalScope":original["scope"], "removedMediaScopes":projection.removed,
                "sourceSpanRetained":true, "importantRetained":true,
                "sourceOrderPreservedWithinBlock":true, "equivalent":false
            });
            projected["scope"] = json!(projection.scope);
            projected[ANNOTATION] = annotation.clone();
            if evidence.len() < MAX_EVIDENCE {
                evidence.push(json!({
                    "file":plan.block.file, "component":plan.block.component,
                    "cssPath":plan.block.path, "span":original["span"],
                    "property":original["property"], "projection":annotation
                }));
            }
            declarations.push(projected);
        }
    }
    report["printProfile"] = json!({
        "format":FORMAT, "profile":"print", "status":"inferred-needs-review",
        "executedSourceCode":false, "readSourceFiles":false, "requestedNetworkResources":false,
        "completeCssCascade":false, "visualVerified":false, "equivalent":false,
        "crossFileOrderInferred":true, "crossFileOrderVerified":false,
        "sourceOrderPreservedWithinBlock":true,
        "coverage":{
            "cssBlocks":block_count, "affectedCssBlocks":affected_blocks,
            "originalDeclarations":original_total, "projectedDeclarations":projected_total,
            "positiveMediaDeclarations":media_total, "unconditionalReplays":replay_total,
            "outputDeclarations":original_total + projected_total, "skipped":skipped,
            "evidenceRecords":evidence.len(), "evidenceTruncated":projected_total > evidence.len()
        },
        "limits":{
            "declarations":MAX_DECLARATIONS, "expansionFactor":2,
            "scopeDepth":MAX_SCOPE_DEPTH, "scopeBytes":MAX_SCOPE_BYTES,
            "mediaAlternatives":MAX_MEDIA_ALTERNATIVES, "evidenceRecords":MAX_EVIDENCE
        },
        "policy":{
            "media":"literal print/all, optional only; lists of those types and screen only",
            "nesting":"every enclosing media must independently match; unsupported at-rules retained without projection",
            "order":"originals retained; append eligible media and subsequent unconditional declarations in original AST order",
            "crossFileOrder":"inspection file/block order is inferred; no import, runtime injection, or stylesheet load order is established"
        },
        "evidence":evidence,
        "limitations":[
            "Static print profile inference only; selectors, specificity, inheritance, DOM matching and complete CSS cascade remain unverified.",
            "Feature queries, not queries, unknown conditions, supports, layers and keyframes are not projected.",
            "Syntax evidence omits @keyframes wrappers; ambiguous from/to/numeric step scopes are conservatively rejected, including literal from/to selectors.",
            "Root variables remain limited to the existing same-file :root/html policy; local variables are not promoted.",
            "Projection does not prove original visual equivalence, component reachability, fonts, layout or pagination."
        ]
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn analyze(path: &str, source: &str) -> Value {
        crate::syntax::analyze(path, source.as_bytes())
            .unwrap_or_else(|e| panic!("{}: {}", e.code, e.message))
            .unwrap()
    }

    fn css_report(source: &str) -> Value {
        let file = analyze("style.css", source);
        assert_eq!(file["styles"]["syntaxValid"], true, "{file}");
        json!({"syntaxEvidence":[file]})
    }

    fn declarations(report: &Value) -> &[Value] {
        report["syntaxEvidence"][0]["styles"]["declarations"]
            .as_array()
            .unwrap()
    }

    fn candidate() -> Value {
        json!({"formatVersion":2,"id":"print-profile-test","name":"Print profile", "version":"1.0.0",
            "license":"NOASSERTION","layout":"minimal",
            "colors":{"accent":"#315E63","seal":"#A14F40","paper":"#FFFFFF","canvas":"#EEEEEE","sidebar":"#EEEEEE"},
            "modules":["basics","work"]})
    }

    #[test]
    fn positive_media_keeps_originals_and_projects_root_variables_and_selectors() {
        let mut report = css_report(
            "body{font-size:10pt;} @media print{:root{--ink:#123456;}body{color:var(--ink);}} \
             @media all{h1{font-size:24pt;}} @media only print{h2{font-size:14pt;}} \
             @media screen, print{h3{font-size:12pt;}}",
        );
        let original = declarations(&report).to_vec();
        project(&mut report).unwrap();
        let result = declarations(&report);
        assert_eq!(&result[..original.len()], original);
        assert_eq!(result.len(), original.len() + 5);
        assert_eq!(result[original.len()]["scope"], json!([":root"]));
        assert_eq!(result[original.len() + 1]["scope"], json!(["body"]));
        for clone in &result[original.len()..] {
            assert!(owned_projection(clone));
            assert_eq!(clone[ANNOTATION]["kind"], "positive-print-media");
            assert_eq!(clone[ANNOTATION]["equivalent"], false);
        }
        let (theme, _) = crate::adapt_style::adapt(&report, candidate()).unwrap();
        assert_eq!(theme["colors"]["ink"], "#123456");
        assert_eq!(
            report["printProfile"]["coverage"]["positiveMediaDeclarations"],
            5
        );
        assert_eq!(report["printProfile"]["completeCssCascade"], false);
        assert_eq!(report["printProfile"]["crossFileOrderInferred"], true);
    }

    #[test]
    fn mixed_feature_negative_and_unknown_conditions_are_never_flattened() {
        let mut report = css_report(
            "@media screen{h1{color:red;}} @media not print{h1{color:red;}} \
             @media print and (min-width:1px){h1{color:red;}} \
             @media (prefers-color-scheme:dark){h1{color:red;}} \
             @media all and (max-width:1000px){h1{color:red;}} \
             @media print, (prefers-reduced-motion:reduce){h1{color:red;}} \
             @media print, not print{h1{color:red;}} \
             @media speech{h1{color:red;}} \
             @supports (display:grid){h1{color:red;}} \
             @keyframes pulse{from{color:red;}to{color:blue;}}",
        );
        let original = declarations(&report).to_vec();
        project(&mut report).unwrap();
        assert_eq!(declarations(&report), original);
        assert_eq!(
            report["printProfile"]["coverage"]["projectedDeclarations"],
            0
        );
        assert_eq!(
            report["printProfile"]["coverage"]["skipped"]["nonPrintMedia"],
            1
        );
        // The parser omits @keyframes itself; its two step blocks need a
        // separate conservative rejection rather than an invented at-rule.
        assert_eq!(
            report["printProfile"]["coverage"]["skipped"],
            json!({
                "nonPrintMedia":1, "unsupportedMediaCondition":7,
                "unsupportedAtRule":1, "keyframeScopeUnverified":2
            })
        );
    }

    #[test]
    fn all_nested_ancestors_must_qualify_and_page_scope_is_retained() {
        let mut report = css_report(
            "@media print{@media all{h1{color:#123456;}}} \
             @media print{@media screen{h1{color:red;}}} \
             @media screen{@media print{h1{color:red;}}} \
             @supports (display:grid){@media print{h1{color:red;}}} \
             @media print{@supports (display:grid){h1{color:red;}}} \
             @media print{@media (min-width:1px){h1{color:red;}}} \
             @media print{@keyframes pulse{from{color:red;}}} \
             @media print{@page{margin:12mm;}}",
        );
        assert_eq!(
            declarations(&report)[6]["scope"],
            json!(["@media print", "from"])
        );
        let count = declarations(&report).len();
        project(&mut report).unwrap();
        let clones = &declarations(&report)[count..];
        assert_eq!(clones.len(), 2, "{clones:#?}");
        assert_eq!(clones[0]["scope"], json!(["h1"]));
        assert_eq!(
            clones[0][ANNOTATION]["removedMediaScopes"],
            json!(["@media print", "@media all"])
        );
        assert_eq!(clones[1]["scope"], json!(["@page"]));
        assert_eq!(
            report["printProfile"]["coverage"]["skipped"]["keyframeScopeUnverified"],
            1
        );
    }

    #[test]
    fn omitted_keyframe_wrappers_never_project_steps_or_replay_them_as_rules() {
        let mut report = css_report(
            "@media print{h1{font-size:22pt;}} \
             @keyframes pulse{from{font-size:80pt;}to{font-size:81pt;}\
             0%,50%,100%{font-size:82pt;}} \
             @media print{@-webkit-keyframes spin{from,to{font-size:90pt;}\
             25%,75%{font-size:91pt;}}} h1{font-size:18pt;}",
        );
        let original = declarations(&report).to_vec();
        assert_eq!(original[1]["scope"], json!(["from"]));
        assert_eq!(original[2]["scope"], json!(["to"]));
        assert_eq!(original[3]["scope"], json!(["0%,50%,100%"]));
        assert_eq!(original[4]["scope"], json!(["@media print", "from,to"]));
        assert_eq!(original[5]["scope"], json!(["@media print", "25%,75%"]));
        project(&mut report).unwrap();
        let result = declarations(&report);
        assert_eq!(&result[..original.len()], original);
        let clones = &result[original.len()..];
        assert_eq!(clones.len(), 2, "{clones:#?}");
        assert_eq!(clones[0][ANNOTATION]["originalDeclarationIndex"], 0);
        assert_eq!(clones[1][ANNOTATION]["originalDeclarationIndex"], 6);
        assert_eq!(
            clones[1][ANNOTATION]["kind"],
            "unconditional-source-order-replay"
        );
        assert_eq!(
            report["printProfile"]["coverage"]["skipped"]["keyframeScopeUnverified"],
            5
        );
        let (theme, _) = crate::adapt_style::adapt(&report, candidate()).unwrap();
        assert_eq!(theme["typography"]["name"]["sizePt"], 18.0);
    }

    #[test]
    fn keyframe_like_scope_guard_is_conservative_and_checks_ancestors() {
        for scope in [
            json!(["@media print", "from", "h1"]),
            json!(["@media print", "to"]),
            json!(["@media print", "from, to, 50%"]),
            json!(["@media print", "0% /* start */, 100%"]),
            json!(["@media print", ".5%"]),
            json!(["@media print", "-10%"]),
        ] {
            assert_eq!(
                profile_scope(&json!({"scope":scope})),
                Err("keyframeScopeUnverified")
            );
        }
        for selector in ["h1", ".from", "#to", "from h1", "-my-element", "h1, h2"] {
            assert!(!keyframe_like_scope(selector), "{selector}");
        }
    }

    #[test]
    fn spans_values_and_important_are_preserved_byte_for_byte() {
        let source = "/* 中文 */ @media print{h1{color: /* original */ #123456 !important;}}";
        let mut report = css_report(source);
        let original = declarations(&report)[0].clone();
        project(&mut report).unwrap();
        let clone = &declarations(&report)[1];
        assert_eq!(clone["important"], true);
        assert_eq!(clone["value"], original["value"]);
        assert_eq!(clone["span"], original["span"]);
        let start = clone["span"]["startByte"].as_u64().unwrap() as usize;
        let end = clone["span"]["endByte"].as_u64().unwrap() as usize;
        assert_eq!(
            &source[start..end],
            original["raw"]["text"].as_str().unwrap()
        );
        let mut restored = clone.clone();
        restored.as_object_mut().unwrap().remove(ANNOTATION);
        restored["scope"] = original["scope"].clone();
        assert_eq!(restored, original);
    }

    #[test]
    fn later_unconditional_rules_are_replayed_in_real_order_with_importance() {
        for (source, expected) in [
            (
                "@media print{h1{font-size:22pt;}} h1{font-size:18pt;}",
                18.0,
            ),
            (
                "h1{font-size:18pt;} @media print{h1{font-size:22pt;}}",
                22.0,
            ),
            (
                "@media print{h1{font-size:22pt !important;}} h1{font-size:18pt;}",
                22.0,
            ),
            (
                "@media print{h1{font-size:22pt;}} h1{font-size:18pt !important;}",
                18.0,
            ),
        ] {
            let mut report = css_report(source);
            project(&mut report).unwrap();
            let (theme, _) = crate::adapt_style::adapt(&report, candidate()).unwrap();
            assert_eq!(theme["typography"]["name"]["sizePt"], expected, "{source}");
        }
        let mut report = css_report(
            "@media print{:root{--size:22pt;}} :root{--size:18pt;} h1{font-size:var(--size);}",
        );
        project(&mut report).unwrap();
        assert_eq!(
            declarations(&report)[4][ANNOTATION]["kind"],
            "unconditional-source-order-replay"
        );
        let (theme, _) = crate::adapt_style::adapt(&report, candidate()).unwrap();
        assert_eq!(theme["typography"]["name"]["sizePt"], 18.0);
    }

    #[test]
    fn static_styled_print_and_global_root_variables_use_original_js_spans() {
        let source = r#"import styled,{createGlobalStyle} from 'styled-components';
const Global=createGlobalStyle`@media print{:root{--size:23pt;}}`;
const Name=styled.h1`font-size:18pt;@media print{font-size:var(--size);}`;
const Body=styled.div`@media all{padding:12mm;}padding:14mm;`;
const Local=styled.div`@media print{--local:99pt;}`;"#;
        let file = analyze("src/Resume.jsx", source);
        assert_eq!(file["syntaxValid"], true);
        let mut report = json!({"syntaxEvidence":[file]});
        project(&mut report).unwrap();
        let styles = &report["syntaxEvidence"][0]["styledComponents"];
        assert_eq!(
            styles[0]["css"]["declarations"][1]["scope"],
            json!([":root"])
        );
        assert_eq!(styles[1]["css"]["declarations"][2]["scope"], json!([]));
        assert_eq!(styles[3]["css"]["declarations"][1]["scope"], json!([]));
        let clone = &styles[1]["css"]["declarations"][2];
        let start = clone["span"]["startByte"].as_u64().unwrap() as usize;
        let end = clone["span"]["endByte"].as_u64().unwrap() as usize;
        assert_eq!(&source[start..end], "font-size:var(--size);");
        let (theme, _) = crate::adapt_style::adapt(&report, candidate()).unwrap();
        assert_eq!(theme["typography"]["name"]["sizePt"], 23.0);
    }

    #[test]
    fn repeated_projection_is_identical_even_without_the_summary_marker() {
        let mut report = css_report("@media print{h1{font-size:22pt;}} h1{color:#123456;}");
        project(&mut report).unwrap();
        let once = report.clone();
        project(&mut report).unwrap();
        assert_eq!(report, once);
        report.as_object_mut().unwrap().remove("printProfile");
        project(&mut report).unwrap();
        assert_eq!(report, once);
    }

    #[test]
    fn constant_folded_print_declarations_retain_original_substitution_spans() {
        let source = "import styled from 'styled-components';const SIZE=23;const Name=styled.h1`@media print{font-size:${SIZE}pt;}`;";
        let file = analyze("name.jsx", source);
        let original = file["styledComponents"][0]["css"]["declarations"][0].clone();
        let mut report = json!({"syntaxEvidence":[file]});
        project(&mut report).unwrap();
        let css = &report["syntaxEvidence"][0]["styledComponents"][0]["css"];
        assert_eq!(css["sourceMapping"], "constant-folded");
        let projected = &css["declarations"][1];
        assert_eq!(projected["span"], original["span"]);
        assert_eq!(projected["value"]["text"], "23pt");
        let start = projected["span"]["startByte"].as_u64().unwrap() as usize;
        let end = projected["span"]["endByte"].as_u64().unwrap() as usize;
        assert_eq!(&source[start..end], "font-size:${SIZE}pt;");
    }

    #[test]
    fn malformed_input_errors_do_not_mutate_prior_blocks() {
        for mut report in [
            Value::Null,
            json!({"syntaxEvidence":{}}),
            json!({"syntaxEvidence":[null]}),
            json!({"syntaxEvidence":[
                analyze("ok.css", "@media print{h1{font-size:22pt;}}"),
                {"file":"broken.css","styles":{"syntaxValid":true,"declarations":{}}}
            ]}),
        ] {
            let original = report.clone();
            assert!(project(&mut report).is_err());
            assert_eq!(report, original);
        }
    }

    #[test]
    fn malformed_scopes_and_unsafe_styles_are_retained_without_projection() {
        let mut report = css_report("@media print{h1{font-size:22pt;}}");
        report["syntaxEvidence"][0]["styles"]["declarations"][0]["scope"] =
            json!(["@media print", 3]);
        let original = declarations(&report).to_vec();
        project(&mut report).unwrap();
        assert_eq!(declarations(&report), original);
        assert_eq!(
            report["printProfile"]["coverage"]["skipped"]["invalidScope"],
            1
        );

        let source = "import styled from 'styled-components';const Name=styled.h1`@media print{font-size:22pt;}`;";
        let mut file = analyze("name.jsx", source);
        file["styledComponents"][0]["topLevelBinding"] = json!(false);
        let original = file.clone();
        let mut report = json!({"syntaxEvidence":[file]});
        project(&mut report).unwrap();
        assert_eq!(report["syntaxEvidence"][0], original);
        assert_eq!(
            report["printProfile"]["coverage"]["skipped"]["dynamicOrScopedStyle"],
            1
        );
    }

    #[test]
    fn projection_and_input_limits_fail_atomically_across_blocks() {
        let file = analyze("large.css", "@media print{h1{font-size:22pt;}}");
        let declaration = file["styles"]["declarations"][0].clone();
        let mut report = json!({"syntaxEvidence":[file.clone(),file]});
        report["syntaxEvidence"][1]["styles"]["declarations"] =
            json!(vec![declaration.clone(); MAX_DECLARATIONS / 2]);
        let original = report.clone();
        assert!(
            project(&mut report)
                .unwrap_err()
                .starts_with("PRINT_PROFILE_LIMIT:")
        );
        assert_eq!(report, original);

        report["syntaxEvidence"][1]["styles"]["declarations"] =
            json!(vec![declaration; MAX_DECLARATIONS]);
        let original = report.clone();
        assert!(
            project(&mut report)
                .unwrap_err()
                .contains("input declarations")
        );
        assert_eq!(report, original);
    }

    #[test]
    fn evidence_is_capped_and_cross_file_order_is_explicitly_inferred() {
        let source = format!(
            "@media print{{h1{{{}}}}}",
            "font-size:22pt;".repeat(MAX_EVIDENCE + 1)
        );
        let mut report = css_report(&source);
        let second = analyze("later.css", "h1{font-size:18pt;}");
        report["syntaxEvidence"]
            .as_array_mut()
            .unwrap()
            .push(second);
        project(&mut report).unwrap();
        assert_eq!(
            report["printProfile"]["evidence"].as_array().unwrap().len(),
            MAX_EVIDENCE
        );
        assert_eq!(
            report["printProfile"]["coverage"]["evidenceTruncated"],
            true
        );
        assert_eq!(report["printProfile"]["crossFileOrderVerified"], false);
        let (theme, _) = crate::adapt_style::adapt(&report, candidate()).unwrap();
        assert_eq!(theme["typography"]["name"]["sizePt"], 18.0);
    }

    #[test]
    fn media_allowlist_is_literal_bounded_and_comment_aware() {
        for header in [
            "@media PRINT",
            "@media only all",
            "@media/**/print",
            "@media print, only screen",
        ] {
            assert!(positive_media(header).is_ok(), "{header}");
        }
        for header in [
            "@media pr/**/int",
            "@media print,",
            "@media not screen",
            "@media print or screen",
            "@media print and (width:800px)",
            "@media print, unknown",
            "@media \\70rint",
            "@media print/* unclosed",
            "@media print\u{a0}",
            "@mediaprint",
            "@media only only print",
        ] {
            assert!(positive_media(header).is_err(), "{header}");
        }
        let header = format!(
            "@media {}",
            vec!["print"; MAX_MEDIA_ALTERNATIVES + 1].join(",")
        );
        assert_eq!(positive_media(&header), Err("mediaAlternativeLimit"));
    }
}
