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

fn request_target(url: &str) -> Result<RequestTarget, ThemeError> {
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
            community,
        });
    }

    let prefix = format!("/{DEFAULT_COMMUNITY_REPOSITORY}/");
    if let Some(path) = parsed.path().strip_prefix(&prefix) {
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
        let target = request_target(url)?;
        let mut request = self
            .agent
            .get(target.url.as_str())
            .header("Accept", target.accept);
        if target.community {
            let token = self
                .community_token
                .as_deref()
                .filter(|token| {
                    !token.is_empty()
                        && token.len() <= 1024
                        && token.bytes().all(|b| b.is_ascii_graphic())
                })
                .ok_or_else(|| {
                    ThemeError::new(
                        "SOURCE_AUTH_REQUIRED",
                        format!("默认主题源为私有仓库，请设置 {THEME_TOKEN_ENV} 后重新启动"),
                    )
                })?;
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
        let theme = VerifiedTheme::parse(entry, &response.bytes)?;
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
        let source = GithubSource::community_default();
        let commit = request_target(&source.commit_url()).unwrap();
        assert!(commit.community);
        assert_eq!(commit.accept, "application/vnd.github.sha");
        let path = "themes/community-academic-cv-lite/theme.json";
        let manifest = request_target(&source.raw_url(COMMIT, path)).unwrap();
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
            let public = request_target(url).unwrap();
            assert!(!public.community);
            assert_eq!(public.url.as_str(), url);
        }
        assert!(
            request_target("https://api.github.com/repos/sctb512/resume-one-themes/keys").is_err()
        );
        assert_eq!(
            UreqTransport::default()
                .get(&source.commit_url(), None, 256)
                .err()
                .unwrap()
                .code,
            "SOURCE_AUTH_REQUIRED"
        );
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
