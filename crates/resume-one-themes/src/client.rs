use std::{io::Read, time::Duration};

use crate::{
    Catalog, GithubSource, MAX_FILE_BYTES, ThemeCache, ThemeEntry, ThemeError, VerifiedTheme,
    cache::safe_etag,
    source::{DEFAULT_COMMUNITY_REPOSITORY, safe_path},
};

const THEME_TOKEN_ENV: &str = "RESUME_ONE_THEME_GITHUB_TOKEN";

pub struct HttpResponse {
    pub status: u16,
    pub etag: Option<String>,
    pub bytes: Vec<u8>,
}

/// Testable, GET-only transport. All generated URLs point to GitHub hosts.
pub trait Transport: Send + Sync {
    fn get(&self, url: &str, etag: Option<&str>, limit: usize) -> Result<HttpResponse, ThemeError>;
}

#[derive(Clone)]
pub struct UreqTransport {
    agent: ureq::Agent,
    community_token: Option<String>,
}

impl Default for UreqTransport {
    fn default() -> Self {
        Self {
            agent: ureq::Agent::config_builder()
                .https_only(true)
                .max_redirects(0)
                .http_status_as_error(false)
                .timeout_global(Some(Duration::from_secs(15)))
                .user_agent("Resume-One/0.1")
                .build()
                .into(),
            community_token: None,
        }
    }
}

struct RequestTarget {
    url: url::Url,
    accept: &'static str,
    community: bool,
}

fn request_target(url: &str, authenticated: bool) -> Result<RequestTarget, ThemeError> {
    let mut parsed =
        url::Url::parse(url).map_err(|_| ThemeError::new("SOURCE_URL", "来源地址无效"))?;
    if parsed.scheme() != "https"
        || !matches!(
            parsed.host_str(),
            Some("api.github.com" | "raw.githubusercontent.com")
        )
        || parsed.port().is_some()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(ThemeError::new("SOURCE_URL", "只允许 GitHub HTTPS 地址"));
    }
    if parsed.host_str() == Some("api.github.com") {
        let prefix = format!("/repos/{DEFAULT_COMMUNITY_REPOSITORY}/");
        let community = if let Some(path) = parsed.path().strip_prefix(&prefix) {
            if !path
                .strip_prefix("commits/")
                .is_some_and(|reference| !reference.is_empty() && !reference.contains('/'))
            {
                return Err(ThemeError::new(
                    "SOURCE_URL",
                    "默认主题源只允许读取提交和主题文件",
                ));
            }
            true
        } else {
            false
        };
        return Ok(RequestTarget {
            url: parsed,
            accept: "application/vnd.github.sha",
            community: community && authenticated,
        });
    }

    let prefix = format!("/{DEFAULT_COMMUNITY_REPOSITORY}/");
    if authenticated && let Some(path) = parsed.path().strip_prefix(&prefix) {
        let (commit, path) = path
            .split_once('/')
            .filter(|(commit, path)| GithubSource::valid_commit(commit) && safe_path(path))
            .ok_or_else(|| {
                ThemeError::new("SOURCE_URL", "私有主题文件须指定安全路径和完整 commit")
            })?;
        let mut target = url::Url::parse(&format!(
            "https://api.github.com/repos/{DEFAULT_COMMUNITY_REPOSITORY}/contents/{path}"
        ))
        .expect("fixed host and validated path");
        target.query_pairs_mut().append_pair("ref", commit);
        parsed = target;
        return Ok(RequestTarget {
            url: parsed,
            accept: "application/vnd.github.raw+json",
            community: true,
        });
    }
    Ok(RequestTarget {
        url: parsed,
        accept: "application/json",
        community: false,
    })
}

impl Transport for UreqTransport {
    fn get(&self, url: &str, etag: Option<&str>, limit: usize) -> Result<HttpResponse, ThemeError> {
        let token = self.community_token.as_deref().filter(|token| {
            !token.is_empty() && token.len() <= 1024 && token.bytes().all(|b| b.is_ascii_graphic())
        });
        let target = request_target(url, token.is_some())?;
        let mut request = self
            .agent
            .get(target.url.as_str())
            .header("Accept", target.accept);
        if let Some(token) = token.filter(|_| target.community) {
            request = request.header("Authorization", format!("Bearer {token}"));
        }
        if let Some(etag) = etag.filter(|tag| safe_etag(tag)) {
            request = request.header("If-None-Match", etag);
        }
        let mut response = request.call().map_err(|_| {
            ThemeError::new(
                "SOURCE_NETWORK",
                "无法读取 GitHub 主题源，请检查网络或稍后重试",
            )
        })?;
        let status = response.status().as_u16();
        if target.community && matches!(status, 401 | 404) {
            return Err(ThemeError::new(
                "SOURCE_AUTH_OR_NOT_FOUND",
                "无法读取私有默认主题源，请检查令牌的仓库读取权限以及主题路径",
            ));
        }
        let etag = response
            .headers()
            .get("etag")
            .and_then(|value| value.to_str().ok())
            .filter(|tag| safe_etag(tag))
            .map(str::to_owned);
        let mut bytes = Vec::new();
        if status == 200 {
            response
                .body_mut()
                .as_reader()
                .take(limit as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| ThemeError::new("SOURCE_NETWORK", "读取主题源响应失败"))?;
            if bytes.len() > limit {
                return Err(ThemeError::new(
                    "SOURCE_TOO_LARGE",
                    "主题源响应超过大小上限",
                ));
            }
        }
        Ok(HttpResponse {
            status,
            etag,
            bytes,
        })
    }
}

pub struct CatalogResult {
    pub catalog: Catalog,
    pub stale: bool,
    /// For management/diagnostics only, not a per-theme badge.
    pub warning: Option<ThemeError>,
}

pub struct GithubClient<T = UreqTransport> {
    transport: T,
    cache: ThemeCache,
}

impl GithubClient<UreqTransport> {
    pub fn new(cache: ThemeCache) -> Self {
        Self {
            transport: UreqTransport {
                community_token: std::env::var(THEME_TOKEN_ENV).ok(),
                ..UreqTransport::default()
            },
            cache,
        }
    }
}

impl<T: Transport> GithubClient<T> {
    pub fn with_transport(cache: ThemeCache, transport: T) -> Self {
        Self { transport, cache }
    }
    pub fn cache(&self) -> &ThemeCache {
        &self.cache
    }

    pub fn catalog(
        &self,
        source: &GithubSource,
        offline: bool,
    ) -> Result<CatalogResult, ThemeError> {
        let cached = self.cache.catalog(source);
        if offline {
            let catalog = cached?
                .ok_or_else(|| ThemeError::new("SOURCE_NOT_CACHED", "没有该来源的有效离线清单"))?;
            return Ok(CatalogResult {
                catalog,
                stale: true,
                warning: None,
            });
        }
        let previous = cached.as_ref().ok().and_then(Option::as_ref);
        match self.refresh(source, previous) {
            Ok(catalog) => {
                let warning = self.cache.store_catalog(&catalog).err();
                Ok(CatalogResult {
                    catalog,
                    stale: false,
                    warning,
                })
            }
            Err(error) => match cached {
                Ok(Some(catalog)) => Ok(CatalogResult {
                    catalog,
                    stale: true,
                    warning: Some(error),
                }),
                _ => Err(error),
            },
        }
    }

    fn refresh(
        &self,
        source: &GithubSource,
        previous: Option<&Catalog>,
    ) -> Result<Catalog, ThemeError> {
        let commit = if GithubSource::valid_commit(source.reference()) {
            source.reference().to_ascii_lowercase()
        } else {
            let response = self.transport.get(&source.commit_url(), None, 256)?;
            status(response.status)?;
            std::str::from_utf8(&response.bytes)
                .ok()
                .map(str::trim)
                .filter(|sha| GithubSource::valid_commit(sha))
                .ok_or_else(|| {
                    ThemeError::new("SOURCE_COMMIT", "GitHub 没有返回完整的 commit SHA")
                })?
                .to_ascii_lowercase()
        };
        let previous = previous.filter(|catalog| catalog.commit == commit);
        let response = self.transport.get(
            &source.raw_url(&commit, source.index_path()),
            previous.and_then(|catalog| catalog.etag.as_deref()),
            MAX_FILE_BYTES,
        )?;
        if response.status == 304 {
            return previous.cloned().ok_or_else(|| {
                ThemeError::new("SOURCE_HTTP", "来源返回 304，但没有相应的有效缓存")
            });
        }
        status(response.status)?;
        let mut catalog = Catalog::parse(source.clone(), &commit, &response.bytes)?;
        catalog.etag = response.etag.filter(|tag| safe_etag(tag));
        Ok(catalog)
    }

    pub fn theme(
        &self,
        catalog: &Catalog,
        id: &str,
        offline: bool,
    ) -> Result<VerifiedTheme, ThemeError> {
        let entry = catalog.entry(id)?;
        if let Ok(Some(theme)) = self.cache.theme(catalog, entry) {
            return Ok(theme);
        }
        if offline {
            return Err(ThemeError::new(
                "THEME_NOT_CACHED",
                "主题未缓存或缓存无效，联网后才能试穿或下载",
            ));
        }
        let response = self.transport.get(
            &catalog
                .source
                .raw_url(&catalog.commit, &catalog.manifest_path(entry)),
            None,
            MAX_FILE_BYTES,
        )?;
        status(response.status)?;
        let manifest_path = catalog.manifest_path(entry);
        let theme = VerifiedTheme::parse_with_files(entry, &response.bytes, |name, digest| {
            if let Ok(Some(bytes)) = self.cache.resource(name, digest) {
                return Ok(bytes);
            }
            let path = manifest_path.rsplit_once('/').map_or_else(
                || name.to_owned(),
                |(directory, _)| format!("{directory}/{name}"),
            );
            let response = self.transport.get(
                &catalog.source.raw_url(&catalog.commit, &path),
                None,
                crate::source::resource_limit(name),
            )?;
            status(response.status)?;
            if crate::sha256(&response.bytes) != digest {
                return Err(ThemeError::new(
                    "THEME_HASH",
                    "主题资源 SHA-256 与清单不一致",
                ));
            }
            self.cache.store_resource(digest, &response.bytes)?;
            Ok(response.bytes)
        })?;
        self.cache.store_theme(catalog, entry, &theme)?;
        Ok(theme)
    }

    pub fn cached_theme(
        &self,
        catalog: &Catalog,
        entry: &ThemeEntry,
    ) -> Result<Option<VerifiedTheme>, ThemeError> {
        self.cache.theme(catalog, entry)
    }

    pub fn remember_catalog(&self, catalog: &Catalog) -> Result<(), ThemeError> {
        self.cache.store_pinned_catalog(catalog)
    }

    pub fn pinned_theme(
        &self,
        source: &GithubSource,
        commit: &str,
        id: &str,
    ) -> Result<VerifiedTheme, ThemeError> {
        if !GithubSource::valid_commit(commit) {
            return Err(ThemeError::new(
                "SOURCE_COMMIT",
                "主题版本不是有效的提交标识",
            ));
        }
        let catalog = self
            .cache
            .pinned_catalog(source, commit)
            .ok()
            .flatten()
            .or_else(|| {
                self.cache
                    .catalog(source)
                    .ok()
                    .flatten()
                    .filter(|catalog| catalog.commit == commit)
            });
        let catalog = match catalog {
            Some(catalog) => catalog,
            None => {
                let response = self.transport.get(
                    &source.raw_url(commit, source.index_path()),
                    None,
                    MAX_FILE_BYTES,
                )?;
                status(response.status)?;
                let catalog = Catalog::parse(source.clone(), commit, &response.bytes)?;
                self.remember_catalog(&catalog)?;
                catalog
            }
        };
        self.theme(&catalog, id, false)
    }
}

fn status(status: u16) -> Result<(), ThemeError> {
    match status {
        200 => Ok(()),
        403 | 429 => Err(ThemeError::new(
            "SOURCE_RATE_LIMIT",
            "GitHub 拒绝或限制了请求，请稍后重试",
        )),
        404 => Err(ThemeError::new(
            "SOURCE_NOT_FOUND",
            "GitHub 仓库、ref 或主题文件不存在，或不是公开来源",
        )),
        _ => Err(ThemeError::new(
            "SOURCE_HTTP",
            format!("主题源返回 HTTP {status}，未跟随重定向"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sha256;
    use serde_json::json;
    use std::{
        collections::VecDeque,
        sync::{Arc, Mutex},
    };

    const COMMIT: &str = "0123456789012345678901234567890123456789";
    type Request = (String, Option<String>, usize);
    #[derive(Clone, Default)]
    struct Mock {
        replies: Arc<Mutex<VecDeque<Result<HttpResponse, ThemeError>>>>,
        calls: Arc<Mutex<Vec<Request>>>,
    }
    impl Mock {
        fn respond(&self, status: u16, bytes: Vec<u8>) {
            self.replies.lock().unwrap().push_back(Ok(HttpResponse {
                status,
                etag: Some("\"test\"".into()),
                bytes,
            }));
        }
        fn offline(&self) {
            self.replies
                .lock()
                .unwrap()
                .push_back(Err(ThemeError::new("SOURCE_NETWORK", "offline")));
        }
    }
    impl Transport for Mock {
        fn get(
            &self,
            url: &str,
            etag: Option<&str>,
            limit: usize,
        ) -> Result<HttpResponse, ThemeError> {
            self.calls
                .lock()
                .unwrap()
                .push((url.into(), etag.map(str::to_owned), limit));
            self.replies
                .lock()
                .unwrap()
                .pop_front()
                .expect("unexpected network request")
        }
    }
    fn source(reference: &str) -> GithubSource {
        GithubSource::new("Example/Themes", reference, "themes/index.json").unwrap()
    }
    fn index() -> Vec<u8> {
        let bytes = include_bytes!("../../../assets/themes/zhibai.theme.json");
        let manifest = resume_one_core::parse_theme(bytes).unwrap();
        serde_json::to_vec(
            &json!({"version":1,"themes":[{"id":manifest.id,"name":manifest.name,
            "manifest":"zhibai.theme.json","sha256":sha256(bytes)}]}),
        )
        .unwrap()
    }

    #[test]
    fn default_original_theme_is_commit_pinned_cached_and_installs_exact_bytes() {
        let directory = tempfile::tempdir().unwrap();
        let mock = Mock::default();
        let bytes = serde_json::to_vec(&crate::original::tests::fixture(
            "globalThis.__resumeOneTheme = { render() { return '<h1>Example</h1>'; } }",
        ))
        .unwrap();
        let source = GithubSource::defaults()[1].clone();
        assert_eq!(source.index_path(), "originals/index.json");
        mock.respond(200, COMMIT.as_bytes().into());
        mock.respond(200, serde_json::to_vec(&json!({"version":1,"themes":[{
            "id":"test", "name":"Test", "manifest":"test.original.theme.json", "sha256":sha256(&bytes)
        }]})).unwrap());
        mock.respond(200, bytes.clone());
        let client = GithubClient::with_transport(
            ThemeCache::new(&directory.path().join("cache")).unwrap(),
            mock.clone(),
        );
        let catalog = client.catalog(&source, false).unwrap().catalog;
        let verified = client.theme(&catalog, "test", false).unwrap();
        assert!(verified.original().is_some());
        let cached_catalog = client.catalog(&source, true).unwrap().catalog;
        let cached = client.theme(&cached_catalog, "test", true).unwrap();
        let installed = cached.install(&source, directory.path()).unwrap();
        assert_eq!(std::fs::read(installed).unwrap(), bytes);
        let calls = mock.calls.lock().unwrap();
        assert_eq!(calls.len(), 3);
        assert!(
            calls[2]
                .0
                .ends_with(&format!("/{COMMIT}/originals/test.original.theme.json"))
        );
    }

    #[test]
    fn resolves_ref_pins_all_requests_and_revalidates_cached_index() {
        let root = tempfile::tempdir().unwrap();
        let mock = Mock::default();
        mock.respond(200, COMMIT.as_bytes().into());
        mock.respond(200, index());
        mock.respond(
            200,
            include_bytes!("../../../assets/themes/zhibai.theme.json").into(),
        );
        let client =
            GithubClient::with_transport(ThemeCache::new(root.path()).unwrap(), mock.clone());
        let first = client.catalog(&source("main"), false).unwrap();
        assert!(!first.stale);
        assert!(first.warning.is_none());
        let theme = client.theme(&first.catalog, "zhibai", false).unwrap();
        let offline = client.theme(&first.catalog, "zhibai", true).unwrap();
        assert_eq!(theme.bytes, offline.bytes);
        mock.respond(200, COMMIT.as_bytes().into());
        mock.respond(304, Vec::new());
        assert!(!client.catalog(&source("main"), false).unwrap().stale);
        let calls = mock.calls.lock().unwrap();
        assert_eq!(calls.len(), 5);
        assert_eq!(
            calls[0].0,
            "https://api.github.com/repos/example/themes/commits/main"
        );
        assert_eq!(
            calls[1].0,
            format!("https://raw.githubusercontent.com/example/themes/{COMMIT}/themes/index.json")
        );
        assert_eq!(
            calls[2].0,
            format!(
                "https://raw.githubusercontent.com/example/themes/{COMMIT}/themes/zhibai.theme.json"
            )
        );
        assert_eq!(calls[4].1.as_deref(), Some("\"test\""));
    }

    #[test]
    fn reopens_the_selected_theme_offline_after_its_branch_moves() {
        let root = tempfile::tempdir().unwrap();
        let mock = Mock::default();
        let source = source("main");
        let client =
            GithubClient::with_transport(ThemeCache::new(root.path()).unwrap(), mock.clone());
        let bytes = include_bytes!("../../../assets/themes/zhibai.theme.json");
        mock.respond(200, COMMIT.as_bytes().into());
        mock.respond(200, index());
        mock.respond(200, bytes.into());
        let selected = client.catalog(&source, false).unwrap().catalog;
        client.theme(&selected, "zhibai", false).unwrap();
        client.remember_catalog(&selected).unwrap();
        let newer_commit = "a".repeat(40);
        mock.respond(200, newer_commit.as_bytes().into());
        mock.respond(200, index());
        assert_eq!(
            client.catalog(&source, false).unwrap().catalog.commit(),
            newer_commit
        );
        assert_eq!(
            client
                .pinned_theme(&source, COMMIT, "zhibai")
                .unwrap()
                .bytes(),
            bytes
        );
        assert_eq!(mock.calls.lock().unwrap().len(), 5);
    }

    #[test]
    fn offline_fallback_is_explicit_and_invalid_new_catalog_does_not_replace_old() {
        let root = tempfile::tempdir().unwrap();
        let mock = Mock::default();
        mock.respond(200, index());
        let client =
            GithubClient::with_transport(ThemeCache::new(root.path()).unwrap(), mock.clone());
        client.catalog(&source(COMMIT), false).unwrap();
        mock.offline();
        let stale = client.catalog(&source(COMMIT), false).unwrap();
        assert!(stale.stale);
        assert_eq!(stale.warning.unwrap().code, "SOURCE_NETWORK");
        mock.respond(200, b"<html>not JSON</html>".to_vec());
        let stale = client.catalog(&source(COMMIT), false).unwrap();
        assert!(stale.stale);
        assert_eq!(stale.warning.unwrap().code, "SOURCE_JSON");
        assert_eq!(
            client
                .catalog(&source(COMMIT), true)
                .unwrap()
                .catalog
                .entries
                .len(),
            1
        );
        assert_eq!(mock.calls.lock().unwrap().len(), 3);
        assert_eq!(
            client.catalog(&source("other"), true).err().unwrap().code,
            "SOURCE_NOT_CACHED"
        );
    }

    #[test]
    fn tampered_theme_cache_is_never_used_offline_and_repairs_only_with_verified_bytes() {
        let root = tempfile::tempdir().unwrap();
        let mock = Mock::default();
        mock.respond(200, index());
        let client =
            GithubClient::with_transport(ThemeCache::new(root.path()).unwrap(), mock.clone());
        let catalog = client.catalog(&source(COMMIT), false).unwrap().catalog;
        mock.respond(200, b"malicious theme".to_vec());
        assert_eq!(
            client.theme(&catalog, "zhibai", false).unwrap_err().code,
            "THEME_HASH"
        );
        let bytes = include_bytes!("../../../assets/themes/zhibai.theme.json");
        mock.respond(200, bytes.into());
        client.theme(&catalog, "zhibai", false).unwrap();
        let path = root
            .path()
            .join("objects")
            .join(format!("{}.json", catalog.object_key(&catalog.entries[0])));
        std::fs::write(&path, b"damaged").unwrap();
        assert_eq!(
            client.theme(&catalog, "zhibai", true).unwrap_err().code,
            "THEME_NOT_CACHED"
        );
        mock.respond(200, bytes.into());
        assert_eq!(
            client.theme(&catalog, "zhibai", false).unwrap().bytes,
            bytes
        );
    }

    #[test]
    fn private_theme_credentials_are_only_for_the_default_repository() {
        let source = GithubSource::defaults()[1].clone();
        let commit = request_target(&source.commit_url(), true).unwrap();
        assert!(commit.community);
        assert_eq!(commit.accept, "application/vnd.github.sha");
        let path = "themes/community-academic-cv-lite/theme.json";
        let manifest = request_target(&source.raw_url(COMMIT, path), true).unwrap();
        assert!(manifest.community);
        assert_eq!(manifest.accept, "application/vnd.github.raw+json");
        assert_eq!(
            manifest.url.as_str(),
            format!(
                "https://api.github.com/repos/{DEFAULT_COMMUNITY_REPOSITORY}/contents/{path}?ref={COMMIT}"
            )
        );
        for url in [
            "https://api.github.com/repos/sctb512/resume-one-themes-other/commits/main",
            "https://raw.githubusercontent.com/another/resume-one-themes/main/index.json",
        ] {
            let public = request_target(url, true).unwrap();
            assert!(!public.community);
            assert_eq!(public.url.as_str(), url);
        }
        assert!(
            request_target(
                "https://api.github.com/repos/sctb512/resume-one-themes/keys",
                true
            )
            .is_err()
        );
        let commit = request_target(&source.commit_url(), false).unwrap();
        assert!(!commit.community);
        assert_eq!(commit.accept, "application/vnd.github.sha");
        let raw_url = source.raw_url(COMMIT, path);
        let raw = request_target(&raw_url, false).unwrap();
        assert!(!raw.community);
        assert_eq!(raw.url.as_str(), raw_url);
    }

    #[test]
    fn refuses_redirects_rate_limits_and_untrusted_hosts() {
        let root = tempfile::tempdir().unwrap();
        let mock = Mock::default();
        mock.respond(302, Vec::new());
        mock.respond(429, Vec::new());
        let client =
            GithubClient::with_transport(ThemeCache::new(root.path()).unwrap(), mock.clone());
        assert_eq!(
            client.catalog(&source(COMMIT), false).err().unwrap().code,
            "SOURCE_HTTP"
        );
        assert_eq!(
            client.catalog(&source(COMMIT), false).err().unwrap().code,
            "SOURCE_RATE_LIMIT"
        );
        let transport = UreqTransport::default();
        for url in [
            "http://raw.githubusercontent.com/a/b/main/index.json",
            "https://localhost/index.json",
            "https://github.com/a/b",
            "https://api.github.com/repos/a/b?resume=private",
        ] {
            assert_eq!(
                transport.get(url, None, 1024).err().unwrap().code,
                "SOURCE_URL"
            );
        }
        assert_eq!(mock.calls.lock().unwrap().len(), 2);
    }

    #[test]
    fn explicit_download_is_clean_idempotent_and_does_not_overwrite_a_user_file() {
        let root = tempfile::tempdir().unwrap();
        let catalog = Catalog::parse(source(COMMIT), COMMIT, &index()).unwrap();
        let bytes = include_bytes!("../../../assets/themes/zhibai.theme.json");
        let theme = VerifiedTheme::parse(&catalog.entries[0], bytes).unwrap();
        let path = theme.install(&catalog.source, root.path()).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert_eq!(theme.install(&catalog.source, root.path()).unwrap(), path);
        std::fs::write(&path, b"user edits").unwrap();
        assert_eq!(
            theme
                .install(&catalog.source, root.path())
                .unwrap_err()
                .code,
            "FILE_EXISTS"
        );
        assert_eq!(std::fs::read(path).unwrap(), b"user edits");
    }
}
