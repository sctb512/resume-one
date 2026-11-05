use super::*;

fn candidate() -> Value {
    json!({"formatVersion":2,"id":"style-adapter-test","name":"Style adapter", "version":"1.0.0",
        "license":"NOASSERTION","layout":"minimal",
        "colors":{"accent":"#315E63","seal":"#A14F40","paper":"#FFFFFF","canvas":"#EEEEEE","sidebar":"#EEEEEE"},
        "modules":["basics","work"]})
}

fn syntax(path: &str, source: &str) -> Value {
    crate::syntax::analyze(path, source.as_bytes())
        .unwrap_or_else(|e| panic!("{}: {}", e.code, e.message))
        .unwrap()
}

fn from_css(source: &str) -> (Value, Value) {
    adapt(
        &json!({"syntaxEvidence":[syntax("style.css",source)]}),
        candidate(),
    )
    .unwrap()
}

fn from_js(source: &str) -> (Value, Value) {
    adapt(
        &json!({"syntaxEvidence":[syntax("src/Resume.jsx",source)]}),
        candidate(),
    )
    .unwrap()
}

fn has(report: &Value, code: &str) -> bool {
    report["differences"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v["code"] == code)
}

fn valid(theme: &Value) {
    let parsed = parse_theme(theme.to_string().as_bytes()).unwrap();
    layout::check_theme(&parsed).unwrap();
    assert_eq!(theme["modules"].as_array().unwrap().len(), 12);
    for id in MODULES {
        assert!(theme["modules"].as_array().unwrap().iter().any(|v| v == id));
    }
}

#[test]
fn styled_roles_use_ast_and_explicit_family_fallback_requests() {
    let (theme, report) = from_js(
        r#"
        import styled from 'styled-components';
        const Layout=styled.div`font-family:'Merriweather',Georgia,serif;font-size:12pt;line-height:1.6;color:#111827;padding:60px 40px;background:white;`;
        const Header=styled.header`text-align:center;border-bottom:1px solid #667788;`;
        const Name=styled.h1`font-size:28pt;font-weight:700;letter-spacing:0.5px;`;
        const Label=styled.div`font-size:14pt;font-style:italic;`;
        const Summary=styled.p`font-size:12pt;line-height:1.8;`;
        const SectionTitle=styled.h2`font-size:16pt;text-transform:uppercase;letter-spacing:1px;border-bottom:2px solid #111827;`;
        const ItemTitle=styled.h3`font-size:13pt;font-weight:600;`;
        const Date=styled.time`font-size:11pt;float:right;`;
    "#,
    );
    valid(&theme);
    assert_eq!(theme["typography"]["body"]["family"], "cjk-sans");
    assert_eq!(theme["typography"]["name"]["sizePt"], 28.0);
    assert_eq!(theme["typography"]["label"]["fontStyle"], "normal");
    assert!(has(&report, "RENDERER_REAL_FACE_FALLBACK"));
    assert_eq!(theme["typography"]["summary"]["lineHeight"], 1.8);
    assert!(
        (theme["typography"]["name"]["trackingEm"].as_f64().unwrap() - 0.375 / 28.0).abs() < 1e-8
    );
    assert_eq!(
        theme["typography"]["sectionTitle"]["textTransform"],
        "uppercase"
    );
    assert_eq!(theme["composition"]["header"], "centered");
    assert_eq!(theme["sections"]["work"]["date"], "right");
    assert_eq!(theme["sections"]["work"]["heading"], "line");
    assert!(
        (theme["page"]["marginsMm"]["top"].as_f64().unwrap() - 60.0 * 25.4 / 96.0).abs() < 1e-8
    );
    let requests = report["fontRequests"].as_array().unwrap();
    assert_eq!(requests.len(), 8);
    assert!(requests.iter().any(|r| r["role"] == "label"
        && r["family"] == "Merriweather"
        && r["weight"] == 400
        && r["fontStyle"] == "italic"
        && r["families"] == json!(["Merriweather", "Georgia", "serif"])));
    assert!(has(&report, "FONT_FAMILY_FALLBACK"));
    assert_eq!(report["visualVerified"], false);
    assert_eq!(report["completeCssCascade"], false);
    assert_eq!(report["migrationComplete"], false);
}

#[test]
fn comments_never_become_palette_or_declarations() {
    let (theme, report) = from_css(
        r#"
        /* Name { color: #FF00FF; font-size:42pt } */
        body {color: /* #00FF00 */ #123456;background: white;}
        h1 { color:#234567; font-size:24px; /* color: #FF0000; */ }
    "#,
    );
    valid(&theme);
    assert_eq!(theme["colors"]["ink"], "#123456");
    assert_eq!(theme["typography"]["name"]["color"], "#234567");
    assert_eq!(theme["typography"]["name"]["sizePt"], 18.0);
    assert_eq!(report["coverage"]["declarationsSeen"], 4);
    assert!(!theme.to_string().contains("#FF00FF"));
    assert!(!theme.to_string().contains("#00FF00"));
}

#[test]
fn important_and_local_order_conflicts_have_evidence() {
    let (theme, report) = from_css(
        "h1{font-size:20pt!important;font-size:30pt;} h1{font-size:26pt !important;} h1{font-size:32pt;}",
    );
    valid(&theme);
    assert_eq!(theme["typography"]["name"]["sizePt"], 26.0);
    assert!(has(&report, "COLLAPSED_ROLE_CONFLICT"), "{report:#}");
    let (theme, _) = from_css("h1{font-size:20pt;font-size:24pt;}");
    assert_eq!(theme["typography"]["name"]["sizePt"], 24.0);
}

#[test]
fn nested_children_pseudo_and_media_do_not_override_parent_roles() {
    let (theme, report) = from_js(
        r#"
        import styled from 'styled-components';
        const Name=styled.h1`font-size:24pt;span{font-size:42pt;}&:hover{color:red;}@media print{font-size:40pt;}`;
    "#,
    );
    assert_eq!(theme["typography"]["name"]["sizePt"], 24.0);
    assert!(has(&report, "SELECTOR_ROLE_UNRESOLVED"));
    assert!(has(&report, "CONDITIONAL_RULE_UNSUPPORTED"));
    let (theme, report) = from_css(
        "h1{font-size:23pt;} @media screen {h1{font-size:40pt;}} @media print {h1{font-size:41pt;}}",
    );
    assert_eq!(theme["typography"]["name"]["sizePt"], 23.0);
    assert!(has(&report, "CONDITIONAL_RULE_UNSUPPORTED"));
}

#[test]
fn dynamic_block_is_not_partially_guessed_and_constants_are_static() {
    let (theme, report) = from_js(
        r#"
        import styled from 'styled-components';
        const value=27;
        const Name=styled.h1`font-size:${value}pt;`;
        const Label=styled.div`font-size:40pt;color:${props=>props.color};`;
    "#,
    );
    valid(&theme);
    assert_eq!(theme["typography"]["name"]["sizePt"], 27.0);
    assert_eq!(theme["typography"]["label"]["sizePt"], 10.0);
    assert!(has(&report, "DYNAMIC_OR_SCOPED_STYLE_UNSUPPORTED"));
}

#[test]
fn out_of_range_values_choose_safe_defaults_with_differences_not_clamps() {
    let (theme, report) = from_css(
        "body{padding:0;} h1{font-size:900pt;line-height:5;letter-spacing:-2em;font-weight:450;text-transform:capitalize;}",
    );
    valid(&theme);
    assert_eq!(theme["typography"]["name"]["sizePt"], 26.0);
    assert_eq!(theme["typography"]["name"]["lineHeight"], 1.4);
    assert_eq!(theme["typography"]["name"]["trackingEm"], 0.0);
    assert_eq!(theme["typography"]["name"]["weight"], 400);
    assert_eq!(theme["page"]["marginsMm"]["left"], 16.0);
    assert!(has(&report, "NUMERIC_VALUE_UNREPRESENTABLE"));
    assert!(has(&report, "FONT_WEIGHT_UNREPRESENTABLE"));
    assert!(has(&report, "TEXT_TRANSFORM_UNSUPPORTED"));
}

#[test]
fn pure_css_static_root_variables_font_shorthand_and_units() {
    let (theme, report) = from_css(
        r#"
        :root {--ink:#123456;--font:'Example Serif',serif;--leading:18pt;}
        body {font:italic 400 12pt / var(--leading) var(--font);color:var(--ink);background:rgb(100%,100%,100%);}
        .name {font-size:2em;letter-spacing:2px;}
        .section-title {font-size:1.25em;text-transform:lowercase;}
    "#,
    );
    valid(&theme);
    assert_eq!(theme["colors"]["ink"], "#123456");
    assert_eq!(theme["typography"]["body"]["lineHeight"], 1.5);
    assert_eq!(theme["typography"]["name"]["sizePt"], 24.0);
    assert_eq!(theme["typography"]["sectionTitle"]["sizePt"], 15.0);
    assert_eq!(theme["typography"]["name"]["fontStyle"], "normal");
    assert_eq!(report["fontRequests"][0]["fontStyle"], "italic");
    assert_eq!(report["fontRequests"][0]["family"], "Example Serif");
}

#[test]
fn static_global_style_variables_are_available_to_same_file_components_only() {
    let (theme, report) = from_js(
        r#"
        import styled,{createGlobalStyle} from 'styled-components';
        const GlobalStyle=createGlobalStyle`:root{--ink:#123456;--accent:#224466;--font:'Inter',sans-serif;} body{color:var(--ink);font-family:var(--font);}`;
        const Name=styled.h1`font-size:26pt;color:var(--accent);`;
        const Other=styled.div`:root{--local:#ff0000;}`;
        const Label=styled.div`color:var(--local);`;
    "#,
    );
    valid(&theme);
    assert_eq!(theme["colors"]["ink"], "#123456");
    assert_eq!(theme["typography"]["name"]["color"], "#224466");
    assert_ne!(theme["typography"]["label"]["color"], "#FF0000");
    assert_eq!(report["fontRequests"][0]["family"], "Inter");
    assert!(has(&report, "CSS_VARIABLE_UNRESOLVED"));
}

#[test]
fn one_modules_zero_margin_cannot_collapse_every_section() {
    let (theme, report) = from_css("section{margin-bottom:6mm;} .publications{margin-bottom:0;}");
    valid(&theme);
    assert_eq!(theme["page"]["sectionGapMm"], 6.0);
    assert!(has(&report, "MODULE_MARGIN_NOT_GLOBALIZED"));
    let (theme, report) = from_css("section{margin-bottom:0;}");
    assert_eq!(theme["page"]["sectionGapMm"], 4.0);
    assert!(has(&report, "COMPACT_SECTION_GAP_APPROXIMATED"));
}

#[test]
fn logical_padding_maps_to_known_physical_margins_without_invalid_keys() {
    let (theme, report) = from_css(
        "body{padding-inline-start:12mm;padding-inline-end:13mm;padding-block-start:14mm;padding-block-end:15mm;padding-inline:99mm;}",
    );
    valid(&theme);
    let margins = &theme["page"]["marginsMm"];
    for (edge, expected) in [
        ("left", 12.0),
        ("right", 13.0),
        ("top", 14.0),
        ("bottom", 15.0),
    ] {
        assert!((margins[edge].as_f64().unwrap() - expected).abs() < 1e-6);
    }
    assert_eq!(margins.as_object().unwrap().len(), 4);
    assert!(has(&report, "LOGICAL_PAGE_EDGE_APPROXIMATED"));
}

#[test]
fn tiny_top_padding_keeps_large_name_inside_the_page() {
    let (theme, report) = from_css("body{padding-top:3mm;} h1{font-size:30pt;font-weight:700;}");
    valid(&theme);
    assert!(theme["page"]["marginsMm"]["top"].as_f64().unwrap() > 10.0);
    assert!(has(&report, "BASELINE_TOP_CLEARANCE_ADAPTED"));
    let (theme, report) = from_css("body{padding-top:25mm;} h1{font-size:30pt;font-weight:700;}");
    assert!((theme["page"]["marginsMm"]["top"].as_f64().unwrap() - 25.0).abs() < 1e-6);
    assert!(!has(&report, "BASELINE_TOP_CLEARANCE_ADAPTED"));
}

#[test]
fn namespaced_selectors_keep_identity_label_and_real_document_font() {
    let (theme, report) = from_css(
        ".pro{max-width:800px;padding:18mm;font-family:'Inter',sans-serif;font-size:11pt;} .pro-name{font-size:25pt;font-weight:600;} .pro-label{font-size:12pt;} .pro-section-title{font-size:14pt;}",
    );
    valid(&theme);
    assert_eq!(theme["typography"]["name"]["sizePt"], 25.0);
    assert_eq!(theme["typography"]["label"]["sizePt"], 12.0);
    assert!(
        report["fontRequests"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["family"] == "Inter")
    );
    assert!(report["fontRequests"].as_array().unwrap().len() >= 6);
}

#[test]
fn section_label_aside_never_creates_persistent_resume_sidebar() {
    let (theme, _) = from_css(
        "body{font-size:12pt;} #content aside{text-align:right;padding-right:30px;} #header h2{font-size:18pt;}",
    );
    valid(&theme);
    assert_eq!(theme["composition"]["preset"], "single-column");
    assert_eq!(theme["typography"]["label"]["sizePt"], 18.0);
    assert_ne!(theme["typography"]["sectionTitle"]["sizePt"], 18.0);
}

#[test]
fn narrow_named_grid_area_is_side_column_and_does_not_shrink_identity() {
    let (theme, report) = from_css(
        "body{font-size:10pt;} #person-name{font-size:200%;} #left{grid-area:left;flex-basis:60%;} #right{grid-area:right;min-width:30%;flex-basis:30%;} #right h1{font-size:90%;} .list h1{font-size:10pt;text-transform:uppercase;}",
    );
    valid(&theme);
    assert_eq!(theme["composition"]["preset"], "sidebar-right");
    assert_eq!(theme["typography"]["name"]["sizePt"], 20.0);
    assert_eq!(theme["typography"]["sectionTitle"]["sizePt"], 10.0);
    assert_ne!(theme["typography"]["name"]["textTransform"], "uppercase");
    assert!(
        report["evidence"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["target"] == "side-column-inferred")
    );
    let (theme, _) = from_css("#right{min-width:30%;} #left{text-align:left;}");
    assert_eq!(theme["composition"]["preset"], "single-column");
}

#[test]
fn inherited_document_sizes_and_effective_print_side_columns_are_validated() {
    let (theme, report) = from_css("body{font-size:6pt;}");
    valid(&theme);
    for role in ["body", "label", "summary"] {
        assert!(theme["typography"][role]["sizePt"].as_f64().unwrap() >= 7.0);
    }
    assert!(has(&report, "NUMERIC_VALUE_UNREPRESENTABLE"));
    for css in [
        ".rail{grid-area:right;width:30%} @media print{.rail{grid-area:main;width:100%}}",
        ".rail{grid-area:right;width:30%} @media print{.rail{width:100%}}",
        ".rail{grid-area:right;width:30%} @media print{.rail, .other{width:100%}}",
        ".rail{grid-area:main!important;width:100%!important} @media print{.rail{grid-area:right;width:30%}}",
    ] {
        let mut inspection = json!({"syntaxEvidence":[syntax("page.css",css)]});
        crate::css_print::project(&mut inspection).unwrap();
        let (theme, _) = adapt(&inspection, candidate()).unwrap();
        valid(&theme);
        assert_eq!(theme["composition"]["preset"], "single-column", "{css}");
    }
}

#[test]
fn paragraph_specific_rules_do_not_become_global_header_inheritance_or_page_padding() {
    let (theme, report) = from_css(
        "body{font-family:Inter,sans-serif;font-size:12pt;padding:18mm;} .description{font-family:Lato,sans-serif;font-size:9pt;text-transform:uppercase;padding-left:30mm;} h1{font-size:25pt;}",
    );
    valid(&theme);
    assert_eq!(theme["typography"]["body"]["textTransform"], "uppercase");
    assert_ne!(theme["typography"]["name"]["textTransform"], "uppercase");
    assert!((theme["page"]["marginsMm"]["left"].as_f64().unwrap() - 18.0).abs() < 1e-6);
    let requests = report["fontRequests"].as_array().unwrap();
    assert_eq!(
        requests.iter().find(|r| r["role"] == "body").unwrap()["family"],
        "Lato"
    );
    assert_eq!(
        requests.iter().find(|r| r["role"] == "name").unwrap()["family"],
        "Inter"
    );
}

#[test]
fn local_custom_properties_are_not_promoted_and_cycles_are_reported() {
    let (theme, report) = from_css(
        ":root{--a:var(--b);--b:var(--a);} .other{--ink:red;} body{color:var(--ink);background:var(--a);} h1{color:var(--missing,#123456);}",
    );
    valid(&theme);
    assert_ne!(theme["colors"]["ink"], "#FF0000");
    assert_eq!(theme["typography"]["name"]["color"], "#123456");
    assert!(has(&report, "SCOPED_VARIABLE_UNSUPPORTED"));
    assert!(has(&report, "CSS_VARIABLE_UNRESOLVED"));
}

#[test]
fn sidebar_fields_regions_and_hidden_modules_preserve_all_twelve_modules() {
    let (theme, report) = from_css(
        r#"
        .resume{display:grid;grid-template-columns:30% 70%;column-gap:8mm;}
        .sidebar{background:#123456;font-size:9pt;}
        .sidebar .header{color:white;}
        .sidebar #skills{display:block;}
        .sidebar #languages{display:block;}
        #volunteer{display:none;}
        #work .section-title{border-left:2px solid #123456;text-align:center;}
    "#,
    );
    valid(&theme);
    assert_eq!(theme["composition"]["preset"], "sidebar-left");
    assert_eq!(theme["composition"]["sidebarWidthPct"], 30.0);
    assert_eq!(theme["composition"]["gapMm"], 8.0);
    assert_eq!(theme["composition"]["headerRegion"], "sidebar");
    assert_eq!(theme["composition"]["placements"]["skills"], "sidebar");
    assert_eq!(theme["composition"]["placements"]["languages"], "sidebar");
    assert_eq!(theme["sections"]["work"]["heading"], "accent-line");
    assert_eq!(theme["sections"]["work"]["headingAlign"], "center");
    assert!(theme["sections"]["education"]["headingAlign"].is_null());
    assert!(has(&report, "SOURCE_VISIBILITY_NOT_APPLIED"));
}

#[test]
fn fixed_grid_width_is_explicitly_approximated_and_unbound_members_are_inferred() {
    let (theme, report) = from_js(
        r#"
        import styled from 'styled-components';
        const Layout=styled.div`display:grid;grid-template-columns:315px 1fr;font-size:11pt;`;
        const Sidebar=styled.aside`background:#123456;color:white;`;
        const MainContent=styled.main`background:#f5f2ed;`;
    "#,
    );
    valid(&theme);
    assert_eq!(theme["composition"]["sidebarWidthPct"], 30.0);
    assert!(has(&report, "NUMERIC_VALUE_UNREPRESENTABLE"));
    assert!(has(&report, "GRID_SIDEBAR_APPROXIMATED"));
    assert!(has(&report, "SIDEBAR_MEMBERSHIP_INFERRED"));
    assert_eq!(theme["composition"]["contactRegion"], "sidebar");
}

#[test]
fn unreadable_role_colors_and_palette_are_explained() {
    let (theme, report) = from_css(
        "body{color:#eeeeee;background:white;}h1{color:#ffffff;}h2{color:#eeeeee;} .date{color:rgba(100,100,100,0.4);}",
    );
    valid(&theme);
    let parsed = parse_theme(theme.to_string().as_bytes()).unwrap();
    let style = resume_one_core::theme_style::ThemeStyle::from_manifest(&parsed);
    assert!(style.colors.ink.contrast(style.colors.paper) >= 4.5);
    assert!(
        style
            .typography
            .name
            .color_or(style.colors.ink)
            .contrast(style.colors.paper)
            >= 4.5
    );
    assert!(has(&report, "COLOR_READABILITY_FALLBACK"));
    assert!(has(&report, "ROLE_COLOR_READABILITY_FALLBACK"));
    assert!(has(&report, "COLOR_UNSUPPORTED"));
}

#[test]
fn page_rule_and_longhands_keep_order() {
    let (theme, report) = from_css(
        "@page{size:Letter;margin:10mm 12mm;}body{padding:8mm;padding-left:14mm;} .section{margin-bottom:7mm;} .item{page-break-inside:avoid;}",
    );
    valid(&theme);
    assert_eq!(theme["page"]["size"], "Letter");
    assert_eq!(theme["page"]["marginsMm"]["left"], 14.0);
    assert_eq!(theme["page"]["sectionGapMm"], 7.0);
    assert_eq!(theme["page"]["avoidSplitItems"], true);
    assert_eq!(report["schemaValidated"], true);
    assert_eq!(report["layoutValidated"], true);
}

#[test]
fn no_source_style_and_invalid_report_are_not_marked_exact() {
    assert!(adapt(&json!({"files":[]}), candidate()).is_err());
    let (theme, report) = adapt(&json!({"syntaxEvidence":[]}), candidate()).unwrap();
    valid(&theme);
    assert_eq!(report["status"], "needs-review");
    assert_eq!(report["coverage"]["declarationsSeen"], 0);
    assert_eq!(theme["provenance"]["fidelity"], "adapted");
}

#[test]
fn unknown_properties_and_source_spans_remain_reviewable() {
    let (_, report) = from_css("h1{filter:blur(2px);box-shadow:0 0 2px red;} .unknown{color:red;}");
    assert!(has(&report, "CSS_PROPERTY_UNSUPPORTED"));
    assert!(has(&report, "SELECTOR_ROLE_UNRESOLVED"));
    assert!(
        report["differences"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["source"]["declaration"]["span"]["file"] == "style.css")
    );
}

#[test]
fn literal_jsx_headings_supply_inferred_module_order_without_removing_others() {
    let (theme, report) = from_js(
        r#"
        const Resume=({resume})=><main>
          {resume.education?.length>0&&<section><h2>Education</h2></section>}
          {resume.work?.length>0&&<section><h2>Professional Experience</h2></section>}
          {resume.skills?.length>0&&<section><h2>Technical Skills</h2></section>}
        </main>;
    "#,
    );
    valid(&theme);
    assert_eq!(
        theme["composition"]["moduleOrder"],
        json!(["education", "work", "skills"])
    );
    assert_eq!(
        theme["sections"]["work"]["title"],
        "Professional Experience"
    );
    assert!(has(&report, "MODULE_ORDER_INFERRED"));
}

#[test]
fn non_temporal_left_rails_never_become_unsupported_timelines() {
    let (theme, report) = from_css(
        ".award-item{border-left:3px solid #9ca3af;} .skill-item{border-left:2px solid #999;} .work-item{border-left:2px solid #999;}",
    );
    valid(&theme);
    assert_eq!(theme["sections"]["awards"]["items"], "stack");
    assert_eq!(theme["sections"]["skills"]["items"], "stack");
    assert_eq!(theme["sections"]["work"]["items"], "timeline");
    assert!(has(&report, "BORDER_GEOMETRY_APPROXIMATED"));
}

#[test]
fn family_requests_preserve_quoted_comma_and_skip_generic_families() {
    let (_, report) = from_css(
        "body{font-family:serif,'Example, Display',sans-serif;font-weight:500;font-style:italic;}",
    );
    assert_eq!(
        report["fontRequests"][0],
        json!({"role":"body","family":"Example, Display","families":["serif","Example, Display","sans-serif"],"weight":500,"fontStyle":"italic"})
    );
    let (_, report) = from_css("body{font-family:serif,sans-serif;}");
    assert_eq!(report["fontRequests"], json!([]));
}

#[test]
fn logical_section_rules_retain_separator_and_vertical_rhythm() {
    let (theme, report) = from_css(
        ".pro-section-title{border-block-end:0.5pt solid #B8B8B2;} .pro-section{margin-block-start:6mm;}",
    );
    valid(&theme);
    assert_eq!(theme["sections"]["work"]["heading"], "line");
    assert_eq!(theme["colors"]["border"], "#B8B8B2");
    assert!((theme["page"]["sectionGapMm"].as_f64().unwrap() - 6.0).abs() < 1e-6);
    assert!(has(&report, "BORDER_GEOMETRY_APPROXIMATED"));
    let (theme, _) = from_css(".section-title{border-inline-start:2px solid #334455;}");
    assert_eq!(theme["sections"]["work"]["heading"], "accent-line");
}

/// Optional local corpus validation. Reads package text through the existing
/// parser only; no package entry points, build scripts or third-party code run.
#[test]
#[ignore = "set RESUME_STYLE_CORPUS to an existing local package inventory"]
fn local_corpus_projects_to_valid_themes() {
    let root = std::path::PathBuf::from(
        std::env::var_os("RESUME_STYLE_CORPUS").expect("RESUME_STYLE_CORPUS"),
    );
    let mut packages = 0;
    let mut styled = 0;
    let mut pure_css = 0;
    for package in std::fs::read_dir(root).unwrap() {
        let package = package.unwrap().path();
        if !package.is_dir() {
            continue;
        }
        let mut pending = vec![(package.clone(), 0)];
        let mut files = Vec::new();
        while let Some((path, depth)) = pending.pop() {
            for entry in std::fs::read_dir(path).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                if entry.file_type().unwrap().is_symlink() {
                    continue;
                }
                if path.is_dir()
                    && depth < 4
                    && !matches!(
                        entry.file_name().to_str(),
                        Some("node_modules" | ".git" | "fonts")
                    )
                {
                    pending.push((path, depth + 1));
                } else if path.is_file()
                    && matches!(
                        path.extension().and_then(|s| s.to_str()),
                        Some("js" | "jsx" | "css" | "mjs" | "cjs")
                    )
                {
                    let bytes = std::fs::read(&path).unwrap();
                    if let Some(evidence) = crate::syntax::analyze(
                        path.strip_prefix(&package).unwrap().to_str().unwrap(),
                        &bytes,
                    )
                    .unwrap_or_else(|e| panic!("{}: {}", path.display(), e.message))
                    {
                        files.push(evidence);
                    }
                }
            }
        }
        let has_styled = files.iter().any(|f| {
            f["styledComponents"]
                .as_array()
                .is_some_and(|v| !v.is_empty())
        });
        let has_css = files.iter().any(|f| f["format"] == "css");
        let report = json!({"syntaxEvidence":files});
        let (theme, result) =
            adapt(&report, candidate()).unwrap_or_else(|e| panic!("{}: {e}", package.display()));
        valid(&theme);
        assert_eq!(result["executedSourceCode"], false);
        packages += 1;
        styled += usize::from(has_styled);
        pure_css += usize::from(has_css);
        eprintln!(
            "{}: declarations={}, roles={}, differences={}",
            package.file_name().unwrap().to_string_lossy(),
            result["coverage"]["declarationsSeen"],
            result["coverage"]["matchedRoles"],
            result["differences"].as_array().unwrap().len()
        );
    }
    eprintln!("validated packages={packages}, with styled AST={styled}, with CSS AST={pure_css}");
    assert!(packages > 0);
}
