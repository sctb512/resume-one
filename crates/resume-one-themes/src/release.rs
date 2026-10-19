//! Offline structural gate for a proposed public default theme source.
//! Human declarations are checked for completeness, not independently proven.

use std::{collections::HashSet, fs, io::Read, path::Path, sync::OnceLock};

use jsonschema::Validator;
use serde_json::{Value, json};

use crate::{Catalog, GithubSource, MAX_FILE_BYTES, ThemeError, VerifiedTheme, sha256};

const INDEX: &str = "themes/index.json";
const REVIEW: &str = "themes/review.json";
const LOCAL_COMMIT: &str = "0000000000000000000000000000000000000000";
const MAX_PNG_BYTES: usize = 8 * 1024 * 1024;
const MAX_PDF_BYTES: usize = 30 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct SourceAudit {
    pub theme_count: usize,
    pub evidence_files_verified: usize,
    pub themes: Vec<Value>,
}

impl SourceAudit {
    pub fn as_json(&self) -> Value {
        json!({"themeCount": self.theme_count, "evidenceFilesVerified": self.evidence_files_verified,
            "themes": self.themes,
            "structurallyVerified": true, "humanReviewIndependentlyVerified": false})
    }
}

fn read_source_file(root: &Path, relative: &str, limit: usize) -> Result<Vec<u8>, ThemeError> {
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
        if index + 1 == parts.len() && metadata.len() > limit as u64 {
            return Err(ThemeError::new(
                "RELEASE_TOO_LARGE",
                format!("源文件超过 {} 字节：{}", limit, path.display()),
            ));
        }
    }
    let file = fs::File::open(&path)
        .map_err(|error| ThemeError::new("RELEASE_IO", format!("{}: {error}", path.display())))?;
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| ThemeError::new("RELEASE_IO", format!("{}: {error}", path.display())))?;
    if bytes.len() > limit {
        return Err(ThemeError::new(
            "RELEASE_TOO_LARGE",
            format!("源文件超过 {} 字节：{}", limit, path.display()),
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

fn check_evidence(
    root: &Path,
    path: &str,
    expected_hash: &str,
    extension: &str,
    paths: &mut HashSet<String>,
    total_bytes: &mut usize,
) -> Result<(), ThemeError> {
    if !path.starts_with("qa/") || !path.ends_with(extension) || !paths.insert(path.to_owned()) {
        return Err(ThemeError::new(
            "RELEASE_EVIDENCE_PATH",
            format!("证据路径无效或重复：{path}"),
        ));
    }
    let limit = if extension == ".png" {
        MAX_PNG_BYTES
    } else {
        MAX_PDF_BYTES
    };
    let bytes = read_source_file(root, path, limit)?;
    *total_bytes = total_bytes.saturating_add(bytes.len());
    if *total_bytes > 512 * 1024 * 1024 {
        return Err(ThemeError::new(
            "RELEASE_EVIDENCE_TOO_LARGE",
            "验收证据合计超过 512 MiB",
        ));
    }
    let signature_ok = if extension == ".png" {
        bytes.len() >= 24 && bytes.starts_with(b"\x89PNG\r\n\x1a\n") && &bytes[12..16] == b"IHDR"
    } else {
        bytes.starts_with(b"%PDF-")
    };
    if !signature_ok || !sha256(&bytes).eq_ignore_ascii_case(expected_hash) {
        return Err(ThemeError::new(
            "RELEASE_EVIDENCE_INVALID",
            format!("证据格式或 SHA-256 不匹配：{path}"),
        ));
    }
    Ok(())
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
    let index_bytes = read_source_file(&root, INDEX, MAX_FILE_BYTES)?;
    let source = GithubSource::new("local/release-audit", "main", INDEX)?;
    let catalog = Catalog::parse(source, LOCAL_COMMIT, &index_bytes)?;
    let review_bytes = read_source_file(&root, REVIEW, MAX_FILE_BYTES)?;
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
    let mut evidence_paths = HashSet::new();
    let mut evidence_bytes = 0usize;
    for entry in catalog.entries() {
        let expected_hash = entry.sha256.as_deref().ok_or_else(|| {
            ThemeError::new(
                "RELEASE_HASH_REQUIRED",
                format!("{} 缺少清单 SHA-256", entry.id),
            )
        })?;
        let bytes = read_source_file(&root, &catalog.manifest_path(entry), MAX_FILE_BYTES)?;
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
        let visual = record["visualEvidence"]
            .as_array()
            .expect("review schema array");
        let mut samples = HashSet::new();
        for item in visual {
            let sample = item["sample"].as_str().expect("review schema sample");
            if !samples.insert(sample) {
                return Err(ThemeError::new(
                    "RELEASE_EVIDENCE_DUPLICATE",
                    format!("{} 重复样例 {sample}", entry.id),
                ));
            }
            check_evidence(
                &root,
                item["upstream"].as_str().expect("review schema path"),
                item["upstreamSha256"].as_str().expect("review schema hash"),
                ".png",
                &mut evidence_paths,
                &mut evidence_bytes,
            )?;
            check_evidence(
                &root,
                item["migrated"].as_str().expect("review schema path"),
                item["migratedSha256"].as_str().expect("review schema hash"),
                ".png",
                &mut evidence_paths,
                &mut evidence_bytes,
            )?;
        }
        let pdf = &record["pdfEvidence"];
        check_evidence(
            &root,
            pdf["path"].as_str().expect("review schema path"),
            pdf["sha256"].as_str().expect("review schema hash"),
            ".pdf",
            &mut evidence_paths,
            &mut evidence_bytes,
        )?;
        themes.push(json!({"id": entry.id, "sha256": verified.sha256(),
            "upstreamUrl": provenance["upstreamUrl"]}));
    }
    Ok(SourceAudit {
        theme_count: themes.len(),
        evidence_files_verified: evidence_paths.len(),
        themes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01";
    const PDF: &[u8] = b"%PDF-1.7\nfixture";

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
        fs::create_dir_all(root.path().join("qa/zhibai")).unwrap();
        let visual_evidence: Vec<Value> = [
            "zh-short",
            "zh-long",
            "en-short",
            "en-long",
            "missing-fields",
            "multi-page",
        ]
        .into_iter()
        .map(|sample| {
            let upstream = format!("qa/zhibai/{sample}-upstream.png");
            let migrated = format!("qa/zhibai/{sample}-migrated.png");
            fs::write(root.path().join(&upstream), PNG).unwrap();
            fs::write(root.path().join(&migrated), PNG).unwrap();
            json!({"sample":sample,"upstream":upstream,"upstreamSha256":sha256(PNG),
                "migrated":migrated,"migratedSha256":sha256(PNG)})
        })
        .collect();
        fs::write(root.path().join("qa/zhibai/multi-page.pdf"), PDF).unwrap();
        let review = json!({"version":1,"themes":[{
            "id":"zhibai","sha256":hash,"upstreamUrl":"https://github.com/example/theme",
            "upstreamRevision":"abc123","upstreamLicense":"MIT",
            "licenseReviewedBy":"Example reviewer","visualReviewedBy":"Example reviewer",
            "reviewDate":"2026-10-19","visualSamples":["zh-short","zh-long","en-short",
                "en-long","missing-fields","multi-page"],"visualEvidence":visual_evidence,
            "pdfReviewed":true,"pdfEvidence":{"path":"qa/zhibai/multi-page.pdf","sha256":sha256(PDF)},
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
        assert_eq!(audit.evidence_files_verified, 13);
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

    #[test]
    fn rejects_missing_tampered_duplicate_or_unsafe_evidence() {
        let (root, _, index, mut review) = fixture();
        let image = root.path().join("qa/zhibai/zh-short-upstream.png");
        fs::write(&image, b"not an image").unwrap();
        assert_eq!(
            audit_release_source(root.path()).unwrap_err().code,
            "RELEASE_EVIDENCE_INVALID"
        );
        fs::write(&image, PNG).unwrap();

        review["themes"][0]["visualEvidence"][1]["sample"] = json!("zh-short");
        save(&root, &index, &review);
        assert_eq!(
            audit_release_source(root.path()).unwrap_err().code,
            "RELEASE_EVIDENCE_DUPLICATE"
        );

        review["themes"][0]["visualEvidence"][1]["sample"] = json!("zh-long");
        review["themes"][0]["visualEvidence"][0]["upstream"] = json!("qa/../real.png");
        save(&root, &index, &review);
        assert_eq!(
            audit_release_source(root.path()).unwrap_err().code,
            "RELEASE_PATH"
        );

        review["themes"][0]["visualEvidence"][0]["upstream"] =
            json!("qa/zhibai/zh-short-upstream.png");
        review["themes"][0]["visualEvidence"][0]["migrated"] =
            json!("qa/zhibai/zh-short-upstream.png");
        save(&root, &index, &review);
        assert_eq!(
            audit_release_source(root.path()).unwrap_err().code,
            "RELEASE_EVIDENCE_PATH"
        );

        review["themes"][0]["visualEvidence"][0]["migrated"] =
            json!("qa/zhibai/zh-short-migrated.png");
        save(&root, &index, &review);
        fs::remove_file(&image).unwrap();
        assert_eq!(
            audit_release_source(root.path()).unwrap_err().code,
            "RELEASE_IO"
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

    #[cfg(unix)]
    #[test]
    fn rejects_symlinked_evidence() {
        use std::os::unix::fs::symlink;
        let (root, _, _, _) = fixture();
        let image = root.path().join("qa/zhibai/zh-short-upstream.png");
        fs::rename(&image, root.path().join("image.png")).unwrap();
        symlink(root.path().join("image.png"), &image).unwrap();
        assert_eq!(
            audit_release_source(root.path()).unwrap_err().code,
            "RELEASE_PATH"
        );
    }
}
