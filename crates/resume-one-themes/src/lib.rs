//! Verified GitHub theme sources and an isolated renderer for original themes.

mod browser;
mod cache;
mod client;
mod original;
mod preview_html;
mod release;
mod source;

pub use browser::{
    print_localized_original, print_original, thumbnail_localized_original, thumbnail_original,
};
pub use cache::ThemeCache;
pub use client::{CatalogResult, GithubClient, HttpResponse, Transport, UreqTransport};
pub use original::{ORIGINAL_THEME_SUFFIX, OriginalTheme};
pub use release::{SourceAudit, audit_release_source};
pub use source::{Catalog, GithubSource, ThemeEntry, VerifiedTheme, prefetch_window};

use sha2::{Digest, Sha256};

pub const MAX_FILE_BYTES: usize = 1_048_576;
pub const MAX_THEMES: usize = 500;
pub const MAX_SOURCES: usize = 16;
pub const CACHE_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct ThemeError {
    pub code: &'static str,
    pub message: String,
}

impl ThemeError {
    pub(crate) fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ThemeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for ThemeError {}

pub fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn parse_sources(value: &serde_json::Value) -> Result<Vec<GithubSource>, ThemeError> {
    let array = value
        .as_array()
        .filter(|array| array.len() <= MAX_SOURCES)
        .ok_or_else(|| ThemeError::new("SOURCE_CONFIG", "在线源配置须为数组，最多 16 个来源"))?;
    let mut keys = std::collections::HashSet::new();
    let mut sources = Vec::new();
    for value in array {
        let source = GithubSource::from_json(value)?;
        if !keys.insert(source.key()) {
            return Err(ThemeError::new(
                "SOURCE_DUPLICATE",
                "已存在相同仓库、ref 和清单路径的来源",
            ));
        }
        sources.push(source);
    }
    Ok(sources)
}
