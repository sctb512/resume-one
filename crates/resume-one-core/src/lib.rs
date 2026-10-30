//! Shared, code-free theme validation for Resume One.
//!
//! This crate never executes theme files and never reads a resume while parsing a theme.

use std::collections::{BTreeMap, HashSet};
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::OnceLock;

use jsonschema::Validator;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub mod appearance;
pub mod content_edit;
pub mod docx;
pub mod draft;
pub mod editor;
pub mod export;
pub mod fonts;
pub mod layout;
pub mod pdf;
pub mod render;
pub mod resume;
pub mod semantic;
pub mod source_map;
pub mod storage;
pub mod structure;
pub mod theme_style;

const MAX_THEME_BYTES: usize = 1_048_576;
const MAX_FONT_BYTES: usize = 8_388_608;
const V1_SCHEMA: &str = include_str!("../../../resume-one-theme-v1.schema.json");
const V2_SCHEMA: &str = include_str!("../../../resume-one-theme-v2.schema.json");

static V1_VALIDATOR: OnceLock<Validator> = OnceLock::new();
static V2_VALIDATOR: OnceLock<Validator> = OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: &'static str,
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct ThemeManifest {
    pub source_version: u64,
    pub id: String,
    pub name: String,
    pub modules: Vec<String>,
    /// In-memory v2 representation. The source file is never rewritten.
    pub normalized: Value,
    /// A declaration is not legal clearance; publishing still needs human license review.
    pub has_declared_license: bool,
    /// Verified, process-local font handles; never serialized into theme JSON.
    pub(crate) asset_fonts: BTreeMap<String, u16>,
}

impl ThemeManifest {
    /// All validated faces, including roles absent from the current resume.
    /// Native engines may cache a family's available weights at first use.
    pub fn font_asset_ids(&self) -> impl Iterator<Item = u16> + '_ {
        self.asset_fonts.values().copied()
    }
}

fn validator(version: u64) -> &'static Validator {
    match version {
        1 => V1_VALIDATOR.get_or_init(|| {
            jsonschema::draft7::new(
                &serde_json::from_str(V1_SCHEMA).expect("bundled v1 schema JSON"),
            )
            .expect("bundled v1 schema validity")
        }),
        2 => V2_VALIDATOR.get_or_init(|| {
            jsonschema::draft7::new(
                &serde_json::from_str(V2_SCHEMA).expect("bundled v2 schema JSON"),
            )
            .expect("bundled v2 schema validity")
        }),
        _ => unreachable!("version checked by parse_theme"),
    }
}

pub fn parse_theme(bytes: &[u8]) -> Result<ThemeManifest, Vec<Diagnostic>> {
    if bytes.len() > MAX_THEME_BYTES {
        return Err(vec![diagnostic(
            "THEME_TOO_LARGE",
            "",
            "主题文件超过 1 MiB 上限",
        )]);
    }
    let source: Value = serde_json::from_slice(bytes).map_err(|error| {
        vec![diagnostic(
            "INVALID_JSON",
            "",
            format!("主题 JSON 无效：{error}"),
        )]
    })?;
    let version = source
        .get("formatVersion")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            vec![diagnostic(
                "FORMAT_VERSION_REQUIRED",
                "/formatVersion",
                "缺少整数格式版本",
            )]
        })?;
    if version != 1 && version != 2 {
        return Err(vec![diagnostic(
            "UNSUPPORTED_THEME_VERSION",
            "/formatVersion",
            "仅支持主题格式 v1 和 v2",
        )]);
    }

    let problems: Vec<_> = validator(version)
        .iter_errors(&source)
        .map(|error| {
            diagnostic(
                "THEME_SCHEMA",
                error.instance_path().to_string(),
                error.to_string(),
            )
        })
        .collect();
    if !problems.is_empty() {
        return Err(problems);
    }

    let mut normalized = source.clone();
    if version == 1 {
        let object = normalized.as_object_mut().expect("schema checked object");
        object.insert("formatVersion".into(), json!(2));
        object.insert("version".into(), json!("0.0.0"));
        object.insert("license".into(), json!("NOASSERTION"));
    }
    let problems = semantic_issues(&normalized);
    if !problems.is_empty() {
        return Err(problems);
    }

    Ok(ThemeManifest {
        source_version: version,
        id: source["id"].as_str().expect("schema checked id").into(),
        name: source["name"].as_str().expect("schema checked name").into(),
        modules: source["modules"]
            .as_array()
            .expect("schema checked modules")
            .iter()
            .map(|module| module.as_str().expect("schema checked module").into())
            .collect(),
        has_declared_license: version == 2 && source["license"] != "NOASSERTION",
        normalized,
        asset_fonts: BTreeMap::new(),
    })
}

/// Load a theme under one trusted local root and verify bundled fonts before use.
/// `relative_path` must be a root `*.theme.json` or one-level `*/theme.json`.
pub fn load_theme(root: &Path, relative_path: &Path) -> Result<ThemeManifest, Vec<Diagnostic>> {
    let parts: Vec<_> = relative_path.components().collect();
    let allowed_name = match parts.as_slice() {
        [std::path::Component::Normal(name)] => name
            .to_str()
            .is_some_and(|name| name.ends_with(".theme.json")),
        [
            std::path::Component::Normal(_),
            std::path::Component::Normal(name),
        ] => name.to_str() == Some("theme.json"),
        _ => false,
    };
    if !allowed_name {
        return Err(vec![diagnostic(
            "UNSAFE_THEME_PATH",
            "",
            "主题路径必须是目录根部的 *.theme.json 或一级子目录的 theme.json",
        )]);
    }
    let root = std::fs::canonicalize(root)
        .map_err(|error| vec![diagnostic("THEME_IO", "", error.to_string())])?;
    let path = std::fs::canonicalize(root.join(relative_path))
        .map_err(|error| vec![diagnostic("THEME_IO", "", error.to_string())])?;
    if !path.starts_with(&root) || !path.is_file() {
        return Err(vec![diagnostic(
            "THEME_PATH_ESCAPE",
            "",
            "主题文件不在所选目录内",
        )]);
    }
    let bytes = read_limited(&path, MAX_THEME_BYTES)
        .map_err(|error| vec![diagnostic("THEME_IO", "", error.to_string())])?;
    let mut theme = parse_theme(&bytes)?;
    let theme_dir = path.parent().expect("canonical file has parent");
    let mut problems = Vec::new();
    let mut verified_fonts = Vec::new();
    if let Some(fonts) = theme
        .normalized
        .pointer("/assets/fonts")
        .and_then(Value::as_array)
    {
        for (index, font) in fonts.iter().enumerate() {
            let relative = font["path"].as_str().expect("schema checked font path");
            let font_path = match std::fs::canonicalize(theme_dir.join(relative)) {
                Ok(path) if path.starts_with(theme_dir) && path.is_file() => path,
                Ok(_) => {
                    problems.push(diagnostic(
                        "FONT_PATH_ESCAPE",
                        format!("/assets/fonts/{index}/path"),
                        "字体文件不在主题目录内",
                    ));
                    continue;
                }
                Err(error) => {
                    problems.push(diagnostic(
                        "FONT_IO",
                        format!("/assets/fonts/{index}/path"),
                        error.to_string(),
                    ));
                    continue;
                }
            };
            match read_limited(&font_path, MAX_FONT_BYTES) {
                Ok(bytes) => {
                    let actual = format!("{:x}", Sha256::digest(&bytes));
                    if !actual
                        .eq_ignore_ascii_case(font["sha256"].as_str().expect("schema checked hash"))
                    {
                        problems.push(diagnostic(
                            "FONT_HASH_MISMATCH",
                            format!("/assets/fonts/{index}/sha256"),
                            "字体内容与声明的 SHA-256 不一致",
                        ));
                    } else {
                        verified_fonts.push((
                            index,
                            font["id"].as_str().unwrap().to_owned(),
                            bytes,
                        ));
                    }
                }
                Err(error) => problems.push(diagnostic(
                    "FONT_IO",
                    format!("/assets/fonts/{index}/path"),
                    error.to_string(),
                )),
            }
        }
    }
    if !problems.is_empty() {
        return Err(problems);
    }
    for (index, id, bytes) in verified_fonts {
        match fonts::register_asset(bytes) {
            Ok(handle) => {
                theme.asset_fonts.insert(id, handle);
            }
            Err(error) => problems.push(diagnostic(
                "FONT_INVALID",
                format!("/assets/fonts/{index}"),
                error,
            )),
        }
    }
    if !problems.is_empty() {
        return Err(problems);
    }
    Ok(theme)
}

fn read_limited(path: &Path, max_bytes: usize) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(max_bytes as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > max_bytes {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "文件超过安全大小限制",
        ));
    }
    Ok(bytes)
}

fn diagnostic(
    code: &'static str,
    path: impl Into<String>,
    message: impl Into<String>,
) -> Diagnostic {
    Diagnostic {
        code,
        path: path.into(),
        message: message.into(),
    }
}

fn semantic_issues(theme: &Value) -> Vec<Diagnostic> {
    let mut issues = Vec::new();
    let composition = &theme["composition"];
    let preset = composition["preset"].as_str().unwrap_or("single-column");
    let placements = composition["placements"].as_object();
    let sidebar_modules: Vec<_> = placements
        .into_iter()
        .flat_map(|items| items.iter())
        .filter(|(_, value)| value.as_str() == Some("sidebar"))
        .map(|(module, _)| module.as_str())
        .collect();
    let has_sidebar = matches!(preset, "sidebar-left" | "sidebar-right");
    let sidebar_basics = ["headerRegion", "summaryRegion", "contactRegion"]
        .iter()
        .any(|key| composition[*key] == "sidebar");
    if has_sidebar && sidebar_modules.is_empty() && !sidebar_basics {
        issues.push(diagnostic(
            "SIDEBAR_EMPTY",
            "/composition/placements",
            "侧栏布局至少需要一个侧栏模块或基础信息区域",
        ));
    }
    if !has_sidebar && !sidebar_modules.is_empty() {
        issues.push(diagnostic(
            "SIDEBAR_NOT_AVAILABLE",
            "/composition/placements",
            "当前布局不支持侧栏模块",
        ));
    }
    if !has_sidebar {
        for (key, default) in [
            ("headerRegion", "page"),
            ("summaryRegion", "page"),
            ("contactRegion", "header"),
        ] {
            if composition[key].as_str().is_some_and(|region| {
                region != default && !(key == "summaryRegion" && region == "header")
            }) {
                issues.push(diagnostic(
                    "REGION_NOT_AVAILABLE",
                    format!("/composition/{key}"),
                    "独立主栏与侧栏区域仅适用于侧栏布局",
                ));
            }
        }
        if composition["sidebarTypographyScale"]
            .as_f64()
            .is_some_and(|scale| scale != 1.0)
        {
            issues.push(diagnostic(
                "REGION_NOT_AVAILABLE",
                "/composition/sidebarTypographyScale",
                "侧栏字号比例仅适用于侧栏布局",
            ));
        }
    }

    let mut font_ids = HashSet::new();
    let mut font_paths = HashSet::new();
    if let Some(fonts) = theme.pointer("/assets/fonts").and_then(Value::as_array) {
        for (index, font) in fonts.iter().enumerate() {
            let id = font["id"].as_str().unwrap_or_default();
            let path = font["path"].as_str().unwrap_or_default();
            if !font_ids.insert(id) {
                issues.push(diagnostic(
                    "DUPLICATE_FONT_ID",
                    format!("/assets/fonts/{index}/id"),
                    "字体 ID 重复",
                ));
            }
            if !font_paths.insert(path) {
                issues.push(diagnostic(
                    "DUPLICATE_FONT_PATH",
                    format!("/assets/fonts/{index}/path"),
                    "字体路径重复",
                ));
            }
            if path.starts_with('/')
                || path
                    .split('/')
                    .any(|part| part == ".." || part == "." || part.is_empty())
                || path.contains('\\')
            {
                issues.push(diagnostic(
                    "UNSAFE_FONT_PATH",
                    format!("/assets/fonts/{index}/path"),
                    "字体路径须位于主题目录内，不能包含回退或空路径段",
                ));
            }
        }
    }
    for kind in [
        "body",
        "name",
        "sectionTitle",
        "itemTitle",
        "meta",
        "label",
        "summary",
        "sideDate",
    ] {
        let pointer = format!("/typography/{kind}/family");
        if let Some(id) = theme
            .pointer(&pointer)
            .and_then(Value::as_str)
            .and_then(|family| family.strip_prefix("asset:"))
            && !font_ids.contains(id)
        {
            issues.push(diagnostic(
                "UNKNOWN_FONT_ASSET",
                pointer,
                "字体引用没有对应的本地资源",
            ));
        }
    }
    if let Some(sections) = theme["sections"].as_object() {
        for (module, style) in sections {
            match style["items"].as_str() {
                Some("timeline")
                    if !matches!(
                        module.as_str(),
                        "work" | "volunteer" | "education" | "projects"
                    ) =>
                {
                    issues.push(diagnostic(
                        "TIMELINE_UNSUPPORTED",
                        format!("/sections/{module}/items"),
                        "此模块不支持时间线",
                    ))
                }
                Some("chips")
                    if !matches!(module.as_str(), "skills" | "languages" | "interests") =>
                {
                    issues.push(diagnostic(
                        "CHIPS_UNSUPPORTED",
                        format!("/sections/{module}/items"),
                        "此模块不支持标签列表",
                    ))
                }
                _ => {}
            }
        }
    }
    issues
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const V2_EXAMPLE: &str = include_str!("../../../resume-one-theme-v2.example.json");
    const V1_BUILTIN: &str = r##"{
      "formatVersion":1,
      "id":"qinghe",
      "name":"青禾",
      "layout":"classic",
      "colors":{"accent":"#52756B","seal":"#A14F40","paper":"#FFFEFA","canvas":"#F6F3ED"},
      "modules":["basics","work","education","skills"]
    }"##;

    #[test]
    fn accepts_v2_example() {
        let theme = parse_theme(V2_EXAMPLE.as_bytes()).unwrap();
        assert_eq!(theme.source_version, 2);
        assert_eq!(theme.id, "qinghe-sidebar");
        assert!(theme.has_declared_license);
    }

    #[test]
    fn adapts_v1_without_changing_the_file() {
        let original = V1_BUILTIN.as_bytes().to_vec();
        let theme = parse_theme(&original).unwrap();
        assert_eq!(theme.source_version, 1);
        assert_eq!(theme.normalized["formatVersion"], 2);
        assert_eq!(theme.normalized["license"], "NOASSERTION");
        assert!(!theme.has_declared_license);
        assert_eq!(original, V1_BUILTIN.as_bytes());
    }

    #[test]
    fn refuses_code_or_unknown_fields() {
        let mut theme: Value = serde_json::from_str(V2_EXAMPLE).unwrap();
        theme["script"] = json!("alert(1)");
        assert!(
            parse_theme(theme.to_string().as_bytes())
                .unwrap_err()
                .iter()
                .any(|issue| issue.code == "THEME_SCHEMA")
        );
    }

    #[test]
    fn enforces_semantic_layout_rules() {
        let mut theme: Value = serde_json::from_str(V2_EXAMPLE).unwrap();
        theme["composition"]["preset"] = json!("single-column");
        assert!(
            parse_theme(theme.to_string().as_bytes())
                .unwrap_err()
                .iter()
                .any(|issue| issue.code == "SIDEBAR_NOT_AVAILABLE")
        );
    }

    #[test]
    fn validates_region_placement_and_all_static_weight_classes() {
        let mut theme: Value = serde_json::from_str(V2_EXAMPLE).unwrap();
        theme["composition"] = json!({"preset":"sidebar-right", "headerRegion":"main", "summaryRegion":"main", "contactRegion":"sidebar", "sidebarTypographyScale":0.9});
        for weight in [100, 200, 300, 400, 500, 600, 700, 800, 900] {
            theme["typography"]["body"]["weight"] = json!(weight);
            assert!(parse_theme(theme.to_string().as_bytes()).is_ok());
        }
        theme["composition"]["preset"] = json!("single-column");
        assert!(
            parse_theme(theme.to_string().as_bytes())
                .unwrap_err()
                .iter()
                .any(|issue| issue.code == "REGION_NOT_AVAILABLE")
        );
        theme["composition"]["preset"] = json!("sidebar-right");
        for (key, value) in [
            ("headerRegion", json!("floating")),
            ("summaryRegion", json!("floating")),
            ("contactRegion", json!("page")),
            ("sidebarTypographyScale", json!(0.5)),
        ] {
            let mut invalid = theme.clone();
            invalid["composition"][key] = value;
            assert!(parse_theme(invalid.to_string().as_bytes()).is_err());
        }
    }

    #[test]
    fn validates_tracking_bounds_and_declarative_case_transform() {
        let mut theme: Value = serde_json::from_str(V2_EXAMPLE).unwrap();
        for tracking in [-0.05, 0.0, 0.25, 0.5] {
            theme["typography"]["sectionTitle"]["trackingEm"] = json!(tracking);
            for transform in ["none", "uppercase", "lowercase"] {
                theme["typography"]["sectionTitle"]["textTransform"] = json!(transform);
                assert!(parse_theme(theme.to_string().as_bytes()).is_ok());
            }
        }
        for (key, value) in [
            ("trackingEm", json!(-0.051)),
            ("trackingEm", json!(0.501)),
            ("textTransform", json!("capitalize")),
            ("textTransform", json!("${resume.name}")),
        ] {
            let mut invalid = theme.clone();
            invalid["typography"]["sectionTitle"][key] = value;
            assert!(parse_theme(invalid.to_string().as_bytes()).is_err());
        }
    }

    #[test]
    fn validates_declarative_real_slant_without_css() {
        let mut theme: Value = serde_json::from_str(V2_EXAMPLE).unwrap();
        for role in [
            "body",
            "name",
            "sectionTitle",
            "itemTitle",
            "meta",
            "label",
            "summary",
            "sideDate",
        ] {
            for slant in [json!("normal"), json!("italic")] {
                theme["typography"][role]["fontStyle"] = slant;
                assert!(parse_theme(theme.to_string().as_bytes()).is_ok());
            }
            for slant in [
                json!("oblique 12deg"),
                json!("normal; color:red"),
                json!(true),
                json!(1),
            ] {
                let mut invalid = theme.clone();
                invalid["typography"][role]["fontStyle"] = slant;
                assert!(parse_theme(invalid.to_string().as_bytes()).is_err());
            }
        }
    }

    #[test]
    fn validates_scoped_heading_alignment_without_css() {
        let mut theme: Value = serde_json::from_str(V2_EXAMPLE).unwrap();
        for align in ["left", "center", "right"] {
            theme["sections"]["work"]["headingAlign"] = json!(align);
            assert!(parse_theme(theme.to_string().as_bytes()).is_ok());
        }
        for invalid in [
            json!("justify"),
            json!("center; position:absolute"),
            json!(3),
        ] {
            theme["sections"]["work"]["headingAlign"] = invalid;
            assert!(parse_theme(theme.to_string().as_bytes()).is_err());
        }
    }

    #[test]
    fn rejects_unsafe_font_path() {
        let mut theme: Value = serde_json::from_str(V2_EXAMPLE).unwrap();
        theme["assets"] = json!({"fonts": [{"id":"body", "path":"fonts/../evil.ttf", "sha256":"a".repeat(64), "license":"OFL-1.1"}]});
        assert!(parse_theme(theme.to_string().as_bytes()).is_err());
    }

    #[test]
    fn refuses_missing_font_reference() {
        let mut theme: Value = serde_json::from_str(V2_EXAMPLE).unwrap();
        theme["typography"]["body"]["family"] = json!("asset:missing");
        assert!(
            parse_theme(theme.to_string().as_bytes())
                .unwrap_err()
                .iter()
                .any(|issue| issue.code == "UNKNOWN_FONT_ASSET")
        );
    }

    #[test]
    fn refuses_incompatible_section_renderers() {
        let mut theme: Value = serde_json::from_str(V2_EXAMPLE).unwrap();
        theme["sections"]["skills"]["items"] = json!("timeline");
        assert!(
            parse_theme(theme.to_string().as_bytes())
                .unwrap_err()
                .iter()
                .any(|issue| issue.code == "TIMELINE_UNSUPPORTED")
        );
    }

    #[test]
    fn limits_input_size() {
        assert_eq!(
            parse_theme(&vec![b' '; MAX_THEME_BYTES + 1]).unwrap_err()[0].code,
            "THEME_TOO_LARGE"
        );
    }

    #[test]
    fn loads_local_theme_and_verifies_font_content() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("fonts")).unwrap();
        let data = include_bytes!("../tests/fixtures/theme-font.ttf");
        fs::write(root.path().join("fonts/body.ttf"), data).unwrap();
        let hash = format!("{:x}", Sha256::digest(data));
        let mut theme: Value = serde_json::from_str(V2_EXAMPLE).unwrap();
        theme["assets"] = json!({"fonts": [{"id":"body", "path":"fonts/body.ttf", "sha256":hash, "license":"OFL-1.1"}]});
        fs::write(root.path().join("example.theme.json"), theme.to_string()).unwrap();
        assert!(load_theme(root.path(), Path::new("example.theme.json")).is_ok());
        fs::write(root.path().join("fonts/body.ttf"), b"tampered").unwrap();
        assert!(
            load_theme(root.path(), Path::new("example.theme.json"))
                .unwrap_err()
                .iter()
                .any(|issue| issue.code == "FONT_HASH_MISMATCH")
        );
        let invalid = b"not a font";
        fs::write(root.path().join("fonts/body.ttf"), invalid).unwrap();
        theme["assets"]["fonts"][0]["sha256"] = json!(format!("{:x}", Sha256::digest(invalid)));
        fs::write(root.path().join("example.theme.json"), theme.to_string()).unwrap();
        assert!(
            load_theme(root.path(), Path::new("example.theme.json"))
                .unwrap_err()
                .iter()
                .any(|d| d.code == "FONT_INVALID")
        );
    }

    #[test]
    fn local_theme_fonts_share_real_faces_and_exports_without_rewriting_sources() {
        use crate::{
            layout::{FontKind, LayoutElement},
            render::Language,
        };
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("fonts")).unwrap();
        let bytes = include_bytes!("../tests/fixtures/theme-font.ttf");
        fs::write(root.path().join("fonts/fixture.ttf"), bytes).unwrap();
        let mut source: Value =
            serde_json::from_slice(include_bytes!("../../../assets/themes/zhibai.theme.json"))
                .unwrap();
        source["assets"] = json!({"fonts":[{"id":"fixture", "path":"fonts/fixture.ttf", "sha256":format!("{:x}", Sha256::digest(bytes)), "license":"OFL-1.1"}]});
        for role in [
            "body",
            "name",
            "sectionTitle",
            "itemTitle",
            "meta",
            "label",
            "summary",
            "sideDate",
        ] {
            source["typography"][role]["family"] = json!("asset:fixture");
            source["typography"][role]["weight"] = json!(400);
        }
        let original = serde_json::to_vec_pretty(&source).unwrap();
        fs::write(root.path().join("fixture.theme.json"), &original).unwrap();
        assert!(layout::check_theme(&parse_theme(&original).unwrap()).is_err());
        let mut loaded = load_theme(root.path(), Path::new("fixture.theme.json")).unwrap();
        layout::check_theme(&loaded).unwrap();
        let document = resume::parse_resume(br#"{"basics":{"name":"Ada"}}"#).unwrap();
        let mut content = semantic::project(&document);
        content.summary = "Ada 简里 漢".into();
        let measured = layout::layout_with_theme(&content, Language::Zh, &loaded).unwrap();
        let local = measured
            .pages
            .iter()
            .flatten()
            .find_map(|op| {
                if let LayoutElement::Text(text) = op {
                    if let FontKind::Local(id) = text.font {
                        Some(id)
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .unwrap();
        assert_eq!(fonts::get(local).family, "Resume One Font Fixture");
        assert_eq!(&*fonts::get(local).bytes, bytes);
        assert!(measured.pages.iter().flatten().any(|op| matches!(op, LayoutElement::Text(text) if text.font == FontKind::Sans && text.text.contains('漢'))));
        let pdf = pdf::render_with_theme(&content, Language::Zh, &loaded).unwrap();
        assert!(pdf.starts_with(b"%PDF-"));
        let word = docx::render_with_theme(&content, Language::Zh, &loaded).unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(word)).unwrap();
        let mut table = String::new();
        zip.by_name("word/fontTable.xml")
            .unwrap()
            .read_to_string(&mut table)
            .unwrap();
        assert!(table.contains("Resume One Font Fixture"));
        assert!(zip.by_name("word/fonts/local0.odttf").is_ok());
        assert_eq!(
            fs::read(root.path().join("fixture.theme.json")).unwrap(),
            original
        );
        assert_eq!(
            fs::read(root.path().join("fonts/fixture.ttf")).unwrap(),
            bytes
        );
        loaded.normalized["typography"]["body"]["weight"] = json!(700);
        assert!(
            layout::check_theme(&loaded)
                .unwrap_err()
                .contains("实际字重")
        );
    }

    #[test]
    fn refuses_paths_outside_the_local_theme_root() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(
            load_theme(root.path(), Path::new("../outside.theme.json")).unwrap_err()[0].code,
            "UNSAFE_THEME_PATH"
        );
        assert_eq!(
            load_theme(root.path(), Path::new("nested/deep/theme.json")).unwrap_err()[0].code,
            "UNSAFE_THEME_PATH"
        );
    }

    #[test]
    fn bundled_themes_are_real_valid_unique_declarative_files() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/themes");
        let mut paths = fs::read_dir(&root)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        paths.sort();
        assert_eq!(paths.len(), 20, "expected twenty bundled theme files");
        let mut ids = HashSet::new();
        let mut names = HashSet::new();
        let mut visual_structures = HashSet::new();
        for path in paths {
            let filename = path.file_name().unwrap();
            let theme = load_theme(&root, Path::new(filename)).unwrap();
            assert_eq!(theme.source_version, 2);
            assert!(theme.has_declared_license);
            assert_eq!(theme.modules.len(), 12);
            assert_eq!(
                filename.to_str().unwrap(),
                format!("{}.theme.json", theme.id)
            );
            assert!(ids.insert(theme.id.clone()), "duplicate id: {}", theme.id);
            assert!(
                names.insert(theme.name.clone()),
                "duplicate name: {}",
                theme.name
            );
            let style = theme_style::ThemeStyle::from_manifest(&theme);
            assert!(style.colors.ink.contrast(style.colors.paper) >= 4.5);
            assert!(style.colors.accent.contrast(style.colors.paper) >= 4.5);
            assert!(style.colors.muted.contrast(style.colors.paper) >= 4.5);
            assert!(matches!(
                style.composition_preset.as_str(),
                "single-column" | "split-header"
            ));
            assert!(theme.normalized.get("assets").is_none());
            layout::check_theme(&theme).unwrap();
            // A palette change alone is not a new built-in design. Keep the
            // page archetypes distinct even before a human reviews renders.
            let source = &theme.normalized;
            let structure = [
                source["layout"].as_str().unwrap(),
                source["page"]["size"].as_str().unwrap_or("A4"),
                source["composition"]["header"]
                    .as_str()
                    .unwrap_or("stacked"),
                source["composition"]["contact"]
                    .as_str()
                    .unwrap_or("inline"),
                source["typography"]["name"]["family"]
                    .as_str()
                    .unwrap_or("cjk-serif"),
                source["sections"]["work"]["heading"]
                    .as_str()
                    .unwrap_or("plain"),
                source["sections"]["work"]["date"]
                    .as_str()
                    .unwrap_or("inline"),
                source["sections"]["work"]["items"]
                    .as_str()
                    .unwrap_or("stack"),
                source["sections"]["projects"]["heading"]
                    .as_str()
                    .unwrap_or("plain"),
            ]
            .join("|");
            assert!(
                visual_structures.insert(structure),
                "duplicated non-color page structure: {}",
                theme.id
            );
        }
    }
}
