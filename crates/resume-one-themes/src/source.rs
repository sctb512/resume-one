use std::{
    collections::HashSet,
    ops::Range,
    path::{Path, PathBuf},
    sync::OnceLock,
};

use jsonschema::Validator;
use resume_one_core::{ThemeManifest, parse_theme};
use serde_json::{Value, json};
use url::Url;

use crate::{MAX_FILE_BYTES, MAX_THEMES, ThemeError, sha256};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GithubSource {
    repository: String,
    reference: String,
    index_path: String,
}

fn safe_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 240
        && path.is_ascii()
        && path
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_./".contains(&byte))
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

impl GithubSource {
    pub fn new(repository: &str, reference: &str, index_path: &str) -> Result<Self, ThemeError> {
        let parts: Vec<_> = repository.split('/').collect();
        if parts.len() != 2
            || parts.iter().any(|part| {
                part.is_empty()
                    || part.len() > 100
                    || *part == "."
                    || *part == ".."
                    || !part
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
            })
        {
            return Err(ThemeError::new(
                "SOURCE_REPOSITORY",
                "来源必须为 GitHub owner/repository，不接受 URL 或凭据",
            ));
        }
        if !safe_path(reference) || reference.len() > 100 {
            return Err(ThemeError::new(
                "SOURCE_REF",
                "ref 仅允许分支、标签或 commit；不能含空格、查询参数或回退路径",
            ));
        }
        if !safe_path(index_path) || !index_path.ends_with(".json") {
            return Err(ThemeError::new(
                "SOURCE_PATH",
                "清单须为仓库内安全的相对 JSON 路径",
            ));
        }
        Ok(Self {
            repository: repository.to_ascii_lowercase(),
            reference: reference.into(),
            index_path: index_path.into(),
        })
    }

    pub fn from_json(value: &Value) -> Result<Self, ThemeError> {
        let object = value
            .as_object()
            .ok_or_else(|| ThemeError::new("SOURCE_CONFIG", "来源配置须为对象"))?;
        if object.len() != 3 {
            return Err(ThemeError::new(
                "SOURCE_CONFIG",
                "来源只接受 repository、ref、indexPath",
            ));
        }
        let string = |key| {
            value[key]
                .as_str()
                .ok_or_else(|| ThemeError::new("SOURCE_CONFIG", format!("缺少字符串字段 {key}")))
        };
        Self::new(string("repository")?, string("ref")?, string("indexPath")?)
    }

    pub fn as_json(&self) -> Value {
        json!({"repository": self.repository, "ref": self.reference, "indexPath": self.index_path})
    }

    pub fn repository(&self) -> &str {
        &self.repository
    }
    pub fn reference(&self) -> &str {
        &self.reference
    }
    pub fn index_path(&self) -> &str {
        &self.index_path
    }
    pub fn key(&self) -> String {
        sha256(self.as_json().to_string().as_bytes())
    }

    pub(crate) fn commit_url(&self) -> String {
        let mut url = Url::parse(&format!(
            "https://api.github.com/repos/{}/commits/",
            self.repository
        ))
        .expect("fixed HTTPS URL");
        url.path_segments_mut()
            .expect("hierarchical URL")
            .pop_if_empty()
            .push(&self.reference);
        url.into()
    }

    pub(crate) fn raw_url(&self, commit: &str, path: &str) -> String {
        format!(
            "https://raw.githubusercontent.com/{}/{commit}/{path}",
            self.repository
        )
    }

    pub fn valid_commit(commit: &str) -> bool {
        commit.len() == 40 && commit.bytes().all(|byte| byte.is_ascii_hexdigit())
    }
}

#[derive(Debug, Clone)]
pub struct ThemeEntry {
    pub id: String,
    pub name: String,
    pub manifest: String,
    pub sha256: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Catalog {
    pub(crate) source: GithubSource,
    pub(crate) commit: String,
    pub(crate) entries: Vec<ThemeEntry>,
    pub(crate) bytes: Vec<u8>,
    pub(crate) etag: Option<String>,
}

impl Catalog {
    pub fn source(&self) -> &GithubSource {
        &self.source
    }
    pub fn commit(&self) -> &str {
        &self.commit
    }
    pub fn entries(&self) -> &[ThemeEntry] {
        &self.entries
    }
    pub fn parse(source: GithubSource, commit: &str, bytes: &[u8]) -> Result<Self, ThemeError> {
        if !GithubSource::valid_commit(commit) {
            return Err(ThemeError::new(
                "SOURCE_COMMIT",
                "来源未解析为完整的 40 位 commit SHA",
            ));
        }
        if bytes.len() > MAX_FILE_BYTES {
            return Err(ThemeError::new("SOURCE_TOO_LARGE", "主题清单超过 1 MiB"));
        }
        let value: Value = serde_json::from_slice(bytes)
            .map_err(|_| ThemeError::new("SOURCE_JSON", "主题清单不是有效的 UTF-8 JSON"))?;
        static VALIDATOR: OnceLock<Validator> = OnceLock::new();
        let validator = VALIDATOR.get_or_init(|| {
            jsonschema::draft7::new(
                &serde_json::from_str(include_str!(
                    "../../../resume-one-theme-source-v1.schema.json"
                ))
                .expect("source schema JSON"),
            )
            .expect("source schema valid")
        });
        if !validator.is_valid(&value) {
            return Err(ThemeError::new(
                "SOURCE_SCHEMA",
                "主题清单不符合源索引 v1 Schema",
            ));
        }
        let mut ids = HashSet::new();
        let mut paths = HashSet::new();
        let mut entries = Vec::new();
        for item in value["themes"]
            .as_array()
            .expect("validated array")
            .iter()
            .take(MAX_THEMES)
        {
            let id = item["id"].as_str().expect("validated id");
            let manifest = item["manifest"].as_str().expect("validated path");
            if !safe_path(manifest)
                || !ids.insert(id.to_owned())
                || !paths.insert(manifest.to_owned())
            {
                return Err(ThemeError::new(
                    "SOURCE_ENTRY",
                    "清单包含重复 ID、重复文件或不安全的 manifest 路径",
                ));
            }
            entries.push(ThemeEntry {
                id: id.into(),
                name: item["name"].as_str().expect("validated name").into(),
                manifest: manifest.into(),
                sha256: item["sha256"].as_str().map(str::to_ascii_lowercase),
            });
        }
        Ok(Self {
            source,
            commit: commit.to_ascii_lowercase(),
            entries,
            bytes: bytes.into(),
            etag: None,
        })
    }

    pub fn entry(&self, id: &str) -> Result<&ThemeEntry, ThemeError> {
        self.entries
            .iter()
            .find(|entry| entry.id == id)
            .ok_or_else(|| ThemeError::new("THEME_NOT_FOUND", "来源清单中没有该主题"))
    }

    pub fn manifest_path(&self, entry: &ThemeEntry) -> String {
        self.source.index_path.rsplit_once('/').map_or_else(
            || entry.manifest.clone(),
            |(parent, _)| format!("{parent}/{}", entry.manifest),
        )
    }

    pub(crate) fn object_key(&self, entry: &ThemeEntry) -> String {
        sha256(
            format!(
                "{}\n{}\n{}",
                self.source.key(),
                self.commit,
                self.manifest_path(entry)
            )
            .as_bytes(),
        )
    }
}

#[derive(Debug, Clone)]
pub struct VerifiedTheme {
    pub(crate) manifest: ThemeManifest,
    pub(crate) bytes: Vec<u8>,
    pub(crate) sha256: String,
}

impl VerifiedTheme {
    pub fn manifest(&self) -> &ThemeManifest {
        &self.manifest
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn sha256(&self) -> &str {
        &self.sha256
    }
    pub fn parse(entry: &ThemeEntry, bytes: &[u8]) -> Result<Self, ThemeError> {
        if bytes.len() > MAX_FILE_BYTES {
            return Err(ThemeError::new("THEME_TOO_LARGE", "主题超过 1 MiB"));
        }
        let hash = sha256(bytes);
        if entry
            .sha256
            .as_ref()
            .is_some_and(|expected| expected != &hash)
        {
            return Err(ThemeError::new("THEME_HASH", "主题 SHA-256 与清单不一致"));
        }
        let manifest = parse_theme(bytes)
            .map_err(|_| ThemeError::new("THEME_INVALID", "主题未通过声明式格式及安全校验"))?;
        if manifest.id != entry.id || manifest.name != entry.name {
            return Err(ThemeError::new(
                "THEME_IDENTITY",
                "主题 ID 或名称与清单不一致",
            ));
        }
        resume_one_core::layout::check_theme(&manifest)
            .map_err(|message| ThemeError::new("THEME_UNSUPPORTED", message))?;
        Ok(Self {
            manifest,
            bytes: bytes.into(),
            sha256: hash,
        })
    }

    /// Explicit local installation; preserve original manifest bytes and never overwrite.
    pub fn install(&self, source: &GithubSource, directory: &Path) -> Result<PathBuf, ThemeError> {
        let directory = directory
            .canonicalize()
            .map_err(|error| ThemeError::new("THEME_DIRECTORY", error.to_string()))?;
        if !directory.is_dir() {
            return Err(ThemeError::new("THEME_DIRECTORY", "本地主题位置必须是目录"));
        }
        let prefix = &source.key()[..12];
        let path = directory.join(format!(
            "{prefix}-{}-{}.theme.json",
            self.manifest.id,
            &self.sha256[..12]
        ));
        // Idempotent only for an ordinary existing file with identical bytes.
        if let Ok(metadata) = std::fs::symlink_metadata(&path) {
            if metadata.file_type().is_file()
                && metadata.len() <= MAX_FILE_BYTES as u64
                && std::fs::read(&path).is_ok_and(|bytes| bytes == self.bytes)
            {
                return Ok(path);
            }
            return Err(ThemeError::new(
                "FILE_EXISTS",
                "本地同名文件已存在，保留原文件",
            ));
        }
        resume_one_core::export::write_export_unnamed(&path, &self.bytes, false)
            .map_err(|error| ThemeError::new(error.code, error.message))?;
        Ok(path)
    }
}

/// Exactly twenty nearby manifests when available; hover can be promoted by the caller.
pub fn prefetch_window(length: usize, center: usize) -> Range<usize> {
    let count = length.min(20);
    let start = center
        .min(length.saturating_sub(1))
        .saturating_sub(10)
        .min(length - count);
    start..start + count
}

#[cfg(test)]
mod tests {
    use super::*;
    const COMMIT: &str = "0123456789012345678901234567890123456789";
    fn source() -> GithubSource {
        GithubSource::new("Example/Themes", "feature/paper", "themes/index.json").unwrap()
    }
    #[test]
    fn restricts_sources_and_paths_and_encodes_refs() {
        let source = source();
        assert_eq!(source.repository(), "example/themes");
        assert!(source.commit_url().ends_with("feature%2Fpaper"));
        for repo in [
            "https://github.com/a/b",
            "a/b/c",
            "a/..",
            "a/b?token=secret",
            "a@b/c",
        ] {
            assert!(GithubSource::new(repo, "main", "themes/index.json").is_err());
        }
        for path in [
            "../index.json",
            "a/../index.json",
            "./index.json",
            "a//index.json",
            "/index.json",
            "a/%2e%2e/index.json",
            "a\\index.json",
        ] {
            assert!(GithubSource::new("a/b", "main", path).is_err());
        }
        assert_eq!(GithubSource::from_json(&source.as_json()).unwrap(), source);
    }
    #[test]
    fn validates_catalog_identity_hash_and_code_free_manifests() {
        let bytes = include_bytes!("../../../assets/themes/zhibai.theme.json");
        let manifest = parse_theme(bytes).unwrap();
        let value = json!({"version":1,"themes":[{"id":manifest.id,"name":manifest.name,"manifest":"zhibai.theme.json","sha256":sha256(bytes)}]});
        let catalog =
            Catalog::parse(source(), COMMIT, &serde_json::to_vec(&value).unwrap()).unwrap();
        let entry = &catalog.entries[0];
        assert_eq!(catalog.manifest_path(entry), "themes/zhibai.theme.json");
        let theme = VerifiedTheme::parse(entry, bytes).unwrap();
        assert_eq!(theme.bytes, bytes);
        assert_eq!(
            VerifiedTheme::parse(entry, b"module.exports = () => {};")
                .unwrap_err()
                .code,
            "THEME_HASH"
        );
        let mut entry = entry.clone();
        entry.sha256 = None;
        entry.name = "wrong".into();
        assert_eq!(
            VerifiedTheme::parse(&entry, bytes).unwrap_err().code,
            "THEME_IDENTITY"
        );
        let mut duplicate = value.clone();
        duplicate["themes"]
            .as_array_mut()
            .unwrap()
            .push(value["themes"][0].clone());
        assert_eq!(
            Catalog::parse(source(), COMMIT, &serde_json::to_vec(&duplicate).unwrap())
                .unwrap_err()
                .code,
            "SOURCE_ENTRY"
        );
    }

    #[test]
    fn centered_headers_are_accepted_online_but_icons_are_not_silently_flattened() {
        let bytes =
            include_bytes!("../../resume-one-cli/tests/fixtures/centered-header.theme.json");
        let mut entry = ThemeEntry {
            id: "centered-header".into(),
            name: "居中页眉验收".into(),
            manifest: "centered-header.theme.json".into(),
            sha256: Some(sha256(bytes)),
        };
        assert_eq!(VerifiedTheme::parse(&entry, bytes).unwrap().bytes(), bytes);
        let mut unsupported: serde_json::Value = serde_json::from_slice(bytes).unwrap();
        unsupported["composition"]["contact"] = serde_json::json!("icons");
        entry.sha256 = None;
        assert!(VerifiedTheme::parse(&entry, &serde_json::to_vec(&unsupported).unwrap()).is_err());
    }

    #[test]
    fn same_section_styles_are_accepted_online_without_executing_theme_code() {
        let bytes = include_bytes!("../../resume-one-cli/tests/fixtures/section-styles.theme.json");
        let entry = ThemeEntry {
            id: "section-styles".into(),
            name: "模块样式验收".into(),
            manifest: "section-styles.theme.json".into(),
            sha256: Some(sha256(bytes)),
        };
        let theme = VerifiedTheme::parse(&entry, bytes).unwrap();
        assert_eq!(theme.bytes(), bytes);
        let mut unsupported: serde_json::Value = serde_json::from_slice(bytes).unwrap();
        unsupported["sections"]["work"]["items"] = serde_json::json!("timeline");
        let mut entry = entry;
        entry.sha256 = None;
        assert!(VerifiedTheme::parse(&entry, &serde_json::to_vec(&unsupported).unwrap()).is_err());
    }
    #[test]
    fn prefetch_is_bounded_at_both_edges() {
        assert_eq!(prefetch_window(0, 0), 0..0);
        assert_eq!(prefetch_window(5, 100), 0..5);
        assert_eq!(prefetch_window(100, 0), 0..20);
        assert_eq!(prefetch_window(100, 50), 40..60);
        assert_eq!(prefetch_window(100, 99), 80..100);
    }

    #[test]
    fn rejects_unknown_fields_unsafe_entries_and_oversized_catalogs() {
        let base =
            json!({"version":1,"themes":[{"id":"safe","name":"Safe","manifest":"safe.json"}]});
        for path in [
            "../safe.json",
            "./safe.json",
            "a//safe.json",
            "https://example.com/safe.json",
            "a\\safe.json",
        ] {
            let mut value = base.clone();
            value["themes"][0]["manifest"] = json!(path);
            assert!(
                Catalog::parse(source(), COMMIT, &serde_json::to_vec(&value).unwrap()).is_err()
            );
        }
        let mut value = base.clone();
        value["script"] = json!("execute()");
        assert_eq!(
            Catalog::parse(source(), COMMIT, &serde_json::to_vec(&value).unwrap())
                .unwrap_err()
                .code,
            "SOURCE_SCHEMA"
        );
        assert!(Catalog::parse(source(), "main", b"{}").is_err());
        assert_eq!(
            Catalog::parse(source(), COMMIT, &vec![b' '; MAX_FILE_BYTES + 1])
                .unwrap_err()
                .code,
            "SOURCE_TOO_LARGE"
        );
        let sources = json!([source().as_json(), source().as_json()]);
        assert_eq!(
            crate::parse_sources(&sources).unwrap_err().code,
            "SOURCE_DUPLICATE"
        );
        assert!(crate::parse_sources(&json!([])).unwrap().is_empty());
        assert!(crate::parse_sources(&json!(vec![source().as_json(); 17])).is_err());
    }
}
