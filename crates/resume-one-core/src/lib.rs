//! Shared, code-free theme validation for Resume One.
//!
//! This crate never executes theme files and never reads a resume while parsing a theme.

use std::collections::HashSet;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::OnceLock;

use jsonschema::Validator;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub mod resume;

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
    let theme = parse_theme(&bytes)?;
    let theme_dir = path.parent().expect("canonical file has parent");
    let mut problems = Vec::new();
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
    if has_sidebar && sidebar_modules.is_empty() {
        issues.push(diagnostic(
            "SIDEBAR_EMPTY",
            "/composition/placements",
            "侧栏布局至少需要一个侧栏模块",
        ));
    }
    if !has_sidebar && !sidebar_modules.is_empty() {
        issues.push(diagnostic(
            "SIDEBAR_NOT_AVAILABLE",
            "/composition/placements",
            "当前布局不支持侧栏模块",
        ));
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
    for kind in ["body", "name", "sectionTitle", "meta"] {
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
        fs::write(root.path().join("fonts/body.ttf"), b"test font bytes").unwrap();
        let hash = format!("{:x}", Sha256::digest(b"test font bytes"));
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
}
