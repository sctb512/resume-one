//! Offline structural gate for a proposed public default theme source.
//! Human declarations are checked for completeness, not independently proven.

use std::{collections::HashSet, fs, path::Path, sync::OnceLock};

use jsonschema::Validator;
use serde_json::{Value, json};

use crate::{Catalog, GithubSource, MAX_FILE_BYTES, ThemeError, VerifiedTheme};

const INDEX: &str = "themes/index.json";
const REVIEW: &str = "themes/review.json";
const LOCAL_COMMIT: &str = "0000000000000000000000000000000000000000";

#[derive(Debug, Clone)]
pub struct SourceAudit {
    pub theme_count: usize,
    pub themes: Vec<Value>,
}

impl SourceAudit {
    pub fn as_json(&self) -> Value {
        json!({"themeCount": self.theme_count, "themes": self.themes,
            "structurallyVerified": true, "humanReviewIndependentlyVerified": false})
    }
}

fn read_source_file(root: &Path, relative: &str) -> Result<Vec<u8>, ThemeError> {
    let mut path = root.to_path_buf();
    let parts: Vec<_> = relative.split('/').collect();
    if parts.is_empty()
        || parts
            .iter()
            .any(|part| part.is_empty() || *part == "." || *part == "..")
    {
        return Err(ThemeError::new("RELEASE_PATH", "源文件路径不安全"));
    }
    for (index, part) in parts.iter().enumerate() {
        path.push(part);
        let metadata = fs::symlink_metadata(&path).map_err(|error| {
            ThemeError::new("RELEASE_IO", format!("{}: {error}", path.display()))
        })?;
        if metadata.file_type().is_symlink()
            || (index + 1 == parts.len() && !metadata.is_file())
            || (index + 1 < parts.len() && !metadata.is_dir())
        {
            return Err(ThemeError::new(
                "RELEASE_PATH",
                format!("不是普通源文件或目录：{}", path.display()),
            ));
        }
        if index + 1 == parts.len() && metadata.len() > MAX_FILE_BYTES as u64 {
            return Err(ThemeError::new(
                "RELEASE_TOO_LARGE",
                format!("源文件超过 1 MiB：{}", path.display()),
            ));
        }
    }
    let bytes = fs::read(&path)
        .map_err(|error| ThemeError::new("RELEASE_IO", format!("{}: {error}", path.display())))?;
    if bytes.len() > MAX_FILE_BYTES {
        return Err(ThemeError::new(
            "RELEASE_TOO_LARGE",
            format!("源文件超过 1 MiB：{}", path.display()),
        ));
    }
    Ok(bytes)
}

fn review_validator() -> &'static Validator {
    static VALIDATOR: OnceLock<Validator> = OnceLock::new();
    VALIDATOR.get_or_init(|| {
        jsonschema::draft7::new(
            &serde_json::from_str(include_str!(
                "../../../spec/theme-source-review-v1.schema.json"
            ))
            .expect("review schema JSON"),
        )
        .expect("review schema valid")
    })
}

/// Check a checked-out repository before proposing it as the default source.
/// Does not access the network, execute themes, or read a resume.
pub fn audit_release_source(directory: &Path) -> Result<SourceAudit, ThemeError> {
    let root = directory
        .canonicalize()
        .map_err(|error| ThemeError::new("RELEASE_DIRECTORY", error.to_string()))?;
    if !root.is_dir() {
        return Err(ThemeError::new("RELEASE_DIRECTORY", "源仓库必须是目录"));
    }
    let index_bytes = read_source_file(&root, INDEX)?;
    let source = GithubSource::new("local/release-audit", "main", INDEX)?;
    let catalog = Catalog::parse(source, LOCAL_COMMIT, &index_bytes)?;
    let review_bytes = read_source_file(&root, REVIEW)?;
    let review: Value = serde_json::from_slice(&review_bytes)
        .map_err(|_| ThemeError::new("RELEASE_REVIEW_JSON", "验收记录不是有效的 UTF-8 JSON"))?;
    if !review_validator().is_valid(&review) {
        return Err(ThemeError::new(
            "RELEASE_REVIEW_SCHEMA",
            "验收记录不符合 review v1 Schema",
        ));
    }
    let reviews = review["themes"].as_array().expect("review schema array");
    if reviews.len() != catalog.entries().len() {
        return Err(ThemeError::new(
            "RELEASE_REVIEW_COUNT",
            "验收记录须与清单逐项对应",
        ));
    }
    let mut reviewed_ids = HashSet::new();
    for item in reviews {
        if !reviewed_ids.insert(item["id"].as_str().expect("review schema id")) {
            return Err(ThemeError::new(
                "RELEASE_REVIEW_DUPLICATE",
                "验收记录包含重复主题 ID",
            ));
        }
    }
    let mut themes = Vec::with_capacity(catalog.entries().len());
    for entry in catalog.entries() {
        let expected_hash = entry.sha256.as_deref().ok_or_else(|| {
            ThemeError::new(
                "RELEASE_HASH_REQUIRED",
                format!("{} 缺少清单 SHA-256", entry.id),
            )
        })?;
        let bytes = read_source_file(&root, &catalog.manifest_path(entry))?;
        let verified = VerifiedTheme::parse(entry, &bytes)?;
        let theme = verified.manifest();
        let provenance = &theme.normalized["provenance"];
        if theme.source_version != 2
            || !theme.has_declared_license
            || provenance["fidelity"] != "adapted"
        {
            return Err(ThemeError::new(
                "RELEASE_NOT_MIGRATED",
                format!("{} 必须是有许可证声明的 v2 adapted 主题", entry.id),
            ));
        }
        for field in [
            "upstreamUrl",
            "upstreamRevision",
            "upstreamAuthor",
            "upstreamLicense",
            "migratedBy",
        ] {
            if !provenance[field]
                .as_str()
                .is_some_and(|value| !value.trim().is_empty())
            {
                return Err(ThemeError::new(
                    "RELEASE_PROVENANCE",
                    format!("{} 缺少来源字段 {field}", entry.id),
                ));
            }
        }
        let record = reviews
            .iter()
            .find(|item| item["id"] == entry.id)
            .ok_or_else(|| {
                ThemeError::new(
                    "RELEASE_REVIEW_MISSING",
                    format!("{} 缺少人工验收记录", entry.id),
                )
            })?;
        let matches = record["sha256"]
            .as_str()
            .is_some_and(|hash| hash.eq_ignore_ascii_case(expected_hash))
            && record["sha256"]
                .as_str()
                .is_some_and(|hash| hash.eq_ignore_ascii_case(verified.sha256()))
            && record["upstreamUrl"] == provenance["upstreamUrl"]
            && record["upstreamRevision"] == provenance["upstreamRevision"]
            && record["upstreamLicense"] == provenance["upstreamLicense"];
        if !matches {
            return Err(ThemeError::new(
                "RELEASE_REVIEW_MISMATCH",
                format!("{} 验收记录与主题来源或 SHA-256 不一致", entry.id),
            ));
        }
        themes.push(json!({"id": entry.id, "sha256": verified.sha256(),
            "upstreamUrl": provenance["upstreamUrl"]}));
    }
    Ok(SourceAudit {
        theme_count: themes.len(),
        themes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sha256;

    fn fixture() -> (tempfile::TempDir, Value, Value, Value) {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("themes")).unwrap();
        let mut theme: Value =
            serde_json::from_slice(include_bytes!("../../../assets/themes/zhibai.theme.json"))
                .unwrap();
        theme["provenance"] = json!({
            "fidelity": "adapted", "upstreamUrl": "https://github.com/example/theme",
            "upstreamRevision": "abc123", "upstreamAuthor": "Example",
            "upstreamLicense": "MIT", "migratedBy": "Reviewer"
        });
        let bytes = serde_json::to_vec(&theme).unwrap();
        let hash = sha256(&bytes);
        fs::write(root.path().join("themes/zhibai.theme.json"), bytes).unwrap();
        let index = json!({"version":1,"themes":[{"id":"zhibai","name":"知白",
            "manifest":"zhibai.theme.json","sha256":hash}]});
        let review = json!({"version":1,"themes":[{
            "id":"zhibai","sha256":hash,"upstreamUrl":"https://github.com/example/theme",
            "upstreamRevision":"abc123","upstreamLicense":"MIT",
            "licenseReviewedBy":"Example reviewer","visualReviewedBy":"Example reviewer",
            "reviewDate":"2026-10-19","visualSamples":["zh-short","zh-long","en-short",
                "en-long","missing-fields","multi-page"],"pdfReviewed":true,
            "limitations":"Fixture only; not a publication approval"
        }]});
        save(&root, &index, &review);
        (root, theme, index, review)
    }

    fn save(root: &tempfile::TempDir, index: &Value, review: &Value) {
        fs::write(root.path().join(INDEX), serde_json::to_vec(index).unwrap()).unwrap();
        fs::write(
            root.path().join(REVIEW),
            serde_json::to_vec(review).unwrap(),
        )
        .unwrap();
    }

    fn rewrite_theme(
        root: &tempfile::TempDir,
        theme: &Value,
        index: &mut Value,
        review: &mut Value,
    ) {
        let bytes = serde_json::to_vec(theme).unwrap();
        let hash = sha256(&bytes);
        fs::write(root.path().join("themes/zhibai.theme.json"), bytes).unwrap();
        index["themes"][0]["sha256"] = json!(hash);
        review["themes"][0]["sha256"] = json!(hash);
        save(root, index, review);
    }

    #[test]
    fn accepts_structurally_complete_source_without_claiming_independent_review() {
        let (root, _, _, _) = fixture();
        let audit = audit_release_source(root.path()).unwrap();
        assert_eq!(audit.theme_count, 1);
        assert_eq!(audit.as_json()["humanReviewIndependentlyVerified"], false);
    }

    #[test]
    fn refuses_missing_hash_review_and_false_claims() {
        let (root, mut theme, mut index, mut review) = fixture();
        index["themes"][0].as_object_mut().unwrap().remove("sha256");
        save(&root, &index, &review);
        assert_eq!(
            audit_release_source(root.path()).unwrap_err().code,
            "RELEASE_HASH_REQUIRED"
        );

        index["themes"][0]["sha256"] = review["themes"][0]["sha256"].clone();
        review["themes"][0]["visualSamples"] = json!(["zh-short"]);
        save(&root, &index, &review);
        assert_eq!(
            audit_release_source(root.path()).unwrap_err().code,
            "RELEASE_REVIEW_SCHEMA"
        );

        review["themes"][0]["visualSamples"] = json!([
            "zh-short",
            "zh-long",
            "en-short",
            "en-long",
            "missing-fields",
            "multi-page"
        ]);
        review["themes"][0]["upstreamRevision"] = json!("wrong");
        save(&root, &index, &review);
        assert_eq!(
            audit_release_source(root.path()).unwrap_err().code,
            "RELEASE_REVIEW_MISMATCH"
        );

        review["themes"][0]["upstreamRevision"] = json!("abc123");
        theme["provenance"]["fidelity"] = json!("native");
        rewrite_theme(&root, &theme, &mut index, &mut review);
        assert_eq!(
            audit_release_source(root.path()).unwrap_err().code,
            "RELEASE_NOT_MIGRATED"
        );
    }

    #[test]
    fn rejects_unrenderable_theme_and_file_tampering() {
        let (root, mut theme, mut index, mut review) = fixture();
        theme["composition"] = json!({"preset":"single-column","contact":"icons"});
        rewrite_theme(&root, &theme, &mut index, &mut review);
        assert_eq!(
            audit_release_source(root.path()).unwrap_err().code,
            "THEME_UNSUPPORTED"
        );

        theme.as_object_mut().unwrap().remove("composition");
        rewrite_theme(&root, &theme, &mut index, &mut review);
        fs::write(root.path().join("themes/zhibai.theme.json"), b"{}").unwrap();
        assert_eq!(
            audit_release_source(root.path()).unwrap_err().code,
            "THEME_HASH"
        );
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinked_manifest() {
        use std::os::unix::fs::symlink;
        let (root, _, _, _) = fixture();
        fs::rename(
            root.path().join("themes/zhibai.theme.json"),
            root.path().join("real.json"),
        )
        .unwrap();
        symlink(
            root.path().join("real.json"),
            root.path().join("themes/zhibai.theme.json"),
        )
        .unwrap();
        assert_eq!(
            audit_release_source(root.path()).unwrap_err().code,
            "RELEASE_PATH"
        );
    }
}
