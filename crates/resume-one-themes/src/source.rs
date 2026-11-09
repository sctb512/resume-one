use std::{
    collections::{BTreeMap, HashSet},
    ops::Range,
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
};

use jsonschema::Validator;
use resume_one_core::{Diagnostic, MAX_FONT_BYTES, ThemeManifest, load_theme_fonts, parse_theme};
use serde_json::{Value, json};
use url::Url;

use crate::{MAX_FILE_BYTES, MAX_THEMES, ORIGINAL_THEME_SUFFIX, OriginalTheme, ThemeError, sha256};

pub(crate) const DEFAULT_COMMUNITY_REPOSITORY: &str = "sctb512/resume-one-themes";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GithubSource {
    repository: String,
    reference: String,
    index_path: String,
    name: Option<String>,
}

pub(crate) fn safe_path(path: &str) -> bool {
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
    pub fn defaults() -> [Self; 2] {
        [
            ("迁移版社区主题", "migrated-index.json"),
            ("原版 JSON Resume", "originals/index.json"),
        ]
        .map(|(name, path)| {
            Self::new(DEFAULT_COMMUNITY_REPOSITORY, "main", path)
                .and_then(|source| source.with_name(name))
                .expect("valid built-in source")
        })
    }

    pub fn with_name(mut self, name: &str) -> Result<Self, ThemeError> {
        let name = name.trim();
        if name.chars().count() > 60 || name.chars().any(char::is_control) {
            return Err(ThemeError::new(
                "SOURCE_NAME",
                "展示名称最多 60 个字符，且不能包含控制字符",
            ));
        }
        self.name = (!name.is_empty()).then(|| name.to_owned());
        Ok(self)
    }

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
            name: None,
        })
    }

    pub fn from_json(value: &Value) -> Result<Self, ThemeError> {
        let object = value
            .as_object()
            .ok_or_else(|| ThemeError::new("SOURCE_CONFIG", "来源配置须为对象"))?;
        if object
            .keys()
            .any(|key| !matches!(key.as_str(), "repository" | "ref" | "indexPath" | "name"))
        {
            return Err(ThemeError::new(
                "SOURCE_CONFIG",
                "来源只接受 repository、ref、indexPath、name",
            ));
        }
        let string = |key| {
            value[key]
                .as_str()
                .ok_or_else(|| ThemeError::new("SOURCE_CONFIG", format!("缺少字符串字段 {key}")))
        };
        let source = Self::new(string("repository")?, string("ref")?, string("indexPath")?)?;
        if object.contains_key("name") {
            source.with_name(string("name")?)
        } else {
            Ok(source)
        }
    }

    pub(crate) fn location_json(&self) -> Value {
        json!({"repository": self.repository, "ref": self.reference, "indexPath": self.index_path})
    }

    pub fn as_json(&self) -> Value {
        let mut value = self.location_json();
        if let Some(name) = &self.name {
            value["name"] = json!(name);
        }
        value
    }

    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    pub fn display_name(&self) -> &str {
        self.name().unwrap_or_else(|| {
            if self.repository == DEFAULT_COMMUNITY_REPOSITORY {
                match self.index_path.as_str() {
                    "originals/index.json" => return "原版 JSON Resume",
                    "index.json" | "migrated-index.json" => return "迁移版社区主题",
                    _ => {}
                }
            }
            self.repository()
        })
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
        sha256(self.location_json().to_string().as_bytes())
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
    pub files: BTreeMap<String, String>,
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
            let files = item["files"]
                .as_object()
                .into_iter()
                .flatten()
                .map(|(name, hash)| {
                    (
                        name.clone(),
                        hash.as_str().expect("validated hash").to_ascii_lowercase(),
                    )
                })
                .collect::<BTreeMap<_, _>>();
            if !files.is_empty()
                && (manifest.ends_with(ORIGINAL_THEME_SUFFIX)
                    || item["sha256"].is_null()
                    || files.keys().any(|name| {
                        !safe_path(name) || name.contains('/') || name.starts_with('.')
                    }))
            {
                return Err(ThemeError::new(
                    "SOURCE_ENTRY",
                    "主题资源必须属于带摘要的声明式主题包",
                ));
            }
            entries.push(ThemeEntry {
                id: id.into(),
                name: item["name"].as_str().expect("validated name").into(),
                manifest: manifest.into(),
                sha256: item["sha256"].as_str().map(str::to_ascii_lowercase),
                files,
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
    payload: ThemePayload,
    pub(crate) bytes: Vec<u8>,
    pub(crate) sha256: String,
    pub(crate) files: BTreeMap<String, Arc<[u8]>>,
}

#[derive(Debug, Clone)]
enum ThemePayload {
    Declarative(Box<ThemeManifest>),
    Original(Arc<OriginalTheme>),
}

impl VerifiedTheme {
    pub fn manifest(&self) -> Option<&ThemeManifest> {
        match &self.payload {
            ThemePayload::Declarative(manifest) => Some(manifest),
            ThemePayload::Original(_) => None,
        }
    }
    pub fn original(&self) -> Option<&Arc<OriginalTheme>> {
        match &self.payload {
            ThemePayload::Original(theme) => Some(theme),
            ThemePayload::Declarative(_) => None,
        }
    }
    pub fn id(&self) -> &str {
        match &self.payload {
            ThemePayload::Declarative(manifest) => &manifest.id,
            ThemePayload::Original(theme) => &theme.id,
        }
    }
    pub fn name(&self) -> &str {
        match &self.payload {
            ThemePayload::Declarative(manifest) => &manifest.name,
            ThemePayload::Original(theme) => &theme.name,
        }
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn sha256(&self) -> &str {
        &self.sha256
    }
    pub fn parse(entry: &ThemeEntry, bytes: &[u8]) -> Result<Self, ThemeError> {
        Self::parse_with_files(entry, bytes, |_, _| {
            Err(ThemeError::new("THEME_RESOURCE", "缺少主题包资源"))
        })
    }

    pub(crate) fn parse_with_files(
        entry: &ThemeEntry,
        bytes: &[u8],
        mut read: impl FnMut(&str, &str) -> Result<Vec<u8>, ThemeError>,
    ) -> Result<Self, ThemeError> {
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
        let mut files: BTreeMap<String, Arc<[u8]>> = BTreeMap::new();
        let payload = if entry.manifest.ends_with(ORIGINAL_THEME_SUFFIX) {
            if entry.sha256.is_none() {
                return Err(ThemeError::new("THEME_HASH", "原版主题必须提供 SHA-256"));
            }
            let value = serde_json::from_slice(bytes).map_err(|_| {
                ThemeError::new("ORIGINAL_THEME_SCHEMA", "原版主题包不是有效的 JSON")
            })?;
            ThemePayload::Original(Arc::new(OriginalTheme::parse(&value)?))
        } else {
            let manifest = parse_theme(bytes)
                .map_err(|_| ThemeError::new("THEME_INVALID", "主题未通过声明式格式及安全校验"))?;
            if manifest.id != entry.id || manifest.name != entry.name {
                return Err(ThemeError::new(
                    "THEME_IDENTITY",
                    "主题 ID 或名称与清单不一致",
                ));
            }
            if manifest.normalized["assets"]["fonts"]
                .as_array()
                .is_some_and(|fonts| {
                    fonts.iter().any(|font| {
                        let path = font["path"].as_str().expect("validated path");
                        let hash = font["sha256"].as_str().expect("validated digest");
                        !entry
                            .files
                            .get(path)
                            .is_some_and(|digest| digest.eq_ignore_ascii_case(hash))
                    })
                })
            {
                return Err(ThemeError::new(
                    "THEME_RESOURCE",
                    "主题包缺少声明的字体及摘要",
                ));
            }
            let mut total = bytes.len();
            for (name, digest) in &entry.files {
                let content = read(name, digest)?;
                total += content.len();
                if content.len() > resource_limit(name)
                    || total > 32 * 1024 * 1024
                    || sha256(&content) != *digest
                {
                    return Err(ThemeError::new(
                        "THEME_RESOURCE",
                        "主题资源超出大小限制或摘要不匹配",
                    ));
                }
                files.insert(name.clone(), content.into());
            }
            let manifest = load_theme_fonts(manifest, |path| {
                files
                    .get(path)
                    .map(|bytes| bytes.to_vec())
                    .ok_or_else(|| Diagnostic {
                        code: "FONT_IO",
                        path: String::new(),
                        message: "缺少主题字体".into(),
                    })
            })
            .map_err(|issues| ThemeError::new("THEME_RESOURCE", format!("{issues:?}")))?;
            resume_one_core::layout::check_theme(&manifest)
                .map_err(|message| ThemeError::new("THEME_UNSUPPORTED", message))?;
            ThemePayload::Declarative(Box::new(manifest))
        };
        let theme = Self {
            payload,
            bytes: bytes.into(),
            sha256: hash,
            files,
        };
        if theme.id() != entry.id || theme.name() != entry.name {
            return Err(ThemeError::new(
                "THEME_IDENTITY",
                "主题 ID 或名称与清单不一致",
            ));
        }
        Ok(theme)
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
        if !self.files.is_empty() {
            let package = directory.join(format!("{prefix}-{}-{}", self.id(), &self.sha256[..12]));
            return self.install_package(&package);
        }
        let suffix = if self.original().is_some() {
            ORIGINAL_THEME_SUFFIX
        } else {
            ".theme.json"
        };
        let path = directory.join(format!(
            "{prefix}-{}-{}{suffix}",
            self.id(),
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

    fn install_package(&self, package: &Path) -> Result<PathBuf, ThemeError> {
        use std::fs;
        let io = |error: std::io::Error| ThemeError::new("THEME_DIRECTORY", error.to_string());
        match fs::create_dir(package) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(io(error)),
        }
        if !fs::symlink_metadata(package).map_err(io)?.is_dir() {
            return Err(ThemeError::new("FILE_EXISTS", "本地同名位置不是普通目录"));
        }
        let files: Vec<_> = self
            .files
            .iter()
            .map(|(name, bytes)| (name.as_str(), bytes.as_ref()))
            .chain(std::iter::once(("theme.json", self.bytes.as_slice())))
            .collect();
        let mut missing = Vec::new();
        for (name, bytes) in files {
            let path = package.join(name);
            match fs::symlink_metadata(&path) {
                Ok(meta)
                    if meta.is_file()
                        && meta.len() == bytes.len() as u64
                        && fs::read(&path).map_err(io)? == bytes => {}
                Ok(_) => {
                    return Err(ThemeError::new(
                        "FILE_EXISTS",
                        "本地主题包已有不同文件，保留原文件",
                    ));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    missing.push((path, bytes))
                }
                Err(error) => return Err(io(error)),
            }
        }
        // Publish the manifest last so a failed or interrupted copy can be retried.
        for (path, bytes) in missing {
            if let Err(error) = resume_one_core::export::write_export_unnamed(&path, bytes, false) {
                if error.code != "FILE_EXISTS"
                    || !fs::symlink_metadata(&path)
                        .is_ok_and(|meta| meta.is_file() && meta.len() == bytes.len() as u64)
                    || !fs::read(&path).is_ok_and(|saved| saved == bytes)
                {
                    return Err(ThemeError::new(error.code, error.message));
                }
            }
        }
        Ok(package.join("theme.json"))
    }
}

pub(crate) fn resource_limit(name: &str) -> usize {
    if name.ends_with(".ttf") || name.ends_with(".otf") {
        MAX_FONT_BYTES
    } else {
        MAX_FILE_BYTES
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
    fn original_packages_are_explicit_and_require_a_hash() {
        let value = crate::original::tests::fixture("globalThis.__resumeOneTheme = {}");
        let bytes = serde_json::to_vec(&value).unwrap();
        let mut entry = ThemeEntry {
            id: "test".into(),
            name: "Test".into(),
            manifest: "test.original.theme.json".into(),
            sha256: None,
            files: BTreeMap::new(),
        };
        assert_eq!(
            VerifiedTheme::parse(&entry, &bytes).unwrap_err().code,
            "THEME_HASH"
        );
        entry.sha256 = Some(sha256(&bytes));
        let theme = VerifiedTheme::parse(&entry, &bytes).unwrap();
        assert!(theme.manifest().is_none());
        assert_eq!(theme.original().unwrap().package, "jsonresume-theme-test");
        let directory = tempfile::tempdir().unwrap();
        let path = theme.install(&source(), directory.path()).unwrap();
        assert_eq!(OriginalTheme::load(&path).unwrap().id, "test");
        entry.manifest = "test.theme.json".into();
        assert!(VerifiedTheme::parse(&entry, &bytes).is_err());
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
            files: BTreeMap::new(),
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
            files: BTreeMap::new(),
        };
        let theme = VerifiedTheme::parse(&entry, bytes).unwrap();
        assert_eq!(theme.bytes(), bytes);
        let mut side_date: serde_json::Value = serde_json::from_slice(bytes).unwrap();
        side_date["sections"]["work"]["date"] = serde_json::json!("right");
        let mut unpinned = entry.clone();
        unpinned.sha256 = None;
        VerifiedTheme::parse(&unpinned, &serde_json::to_vec(&side_date).unwrap()).unwrap();
        for items in ["timeline", "cards"] {
            let mut rich = side_date.clone();
            rich["sections"]["work"]["items"] = serde_json::json!(items);
            rich["sections"]["skills"] = serde_json::json!({"items":"chips"});
            VerifiedTheme::parse(&unpinned, &serde_json::to_vec(&rich).unwrap()).unwrap();
        }
        side_date["sections"]["work"]["date"] = serde_json::json!("left");
        VerifiedTheme::parse(&unpinned, &serde_json::to_vec(&side_date).unwrap()).unwrap();
        let mut unsupported: serde_json::Value = serde_json::from_slice(bytes).unwrap();
        unsupported["decorations"] = serde_json::json!({"headerMark":"band"});
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
