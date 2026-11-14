//! Online browsing state. Network workers never receive a resume or GUI entity.
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::Arc,
};

use resume_one_themes::{
    Catalog, CatalogResult, GithubClient, GithubSource, ThemeError, VerifiedTheme, prefetch_window,
};

use crate::model::BuiltinTheme;

pub(super) trait Provider: Send + Sync {
    fn catalog(&self, source: &GithubSource) -> Result<CatalogResult, ThemeError>;
    fn theme(&self, catalog: &Catalog, id: &str) -> Result<VerifiedTheme, ThemeError>;
    fn remember_catalog(&self, _: &Catalog) {}
}

#[cfg(test)]
pub(super) mod test_support {
    use super::*;
    use serde_json::json;

    pub struct Fixture;

    pub fn source(name: &str) -> GithubSource {
        GithubSource::new(&format!("qa/{name}"), "main", "themes/index.json").unwrap()
    }

    pub fn bytes(index: usize) -> Vec<u8> {
        let mut value: serde_json::Value =
            serde_json::from_str(crate::model::BUILTIN_THEMES[index % 4]).unwrap();
        value["id"] = json!(format!("theme-{index}"));
        value["name"] = json!(format!("主题 {index}"));
        serde_json::to_vec(&value).unwrap()
    }

    pub fn catalog(source: &GithubSource) -> Catalog {
        let entries: Vec<_> = (0..50).map(|index| json!({"id":format!("theme-{index}"), "name":format!("主题 {index}"), "manifest":format!("theme-{index}.theme.json"), "sha256":resume_one_themes::sha256(&bytes(index))})).collect();
        Catalog::parse(
            source.clone(),
            &"a".repeat(40),
            &serde_json::to_vec(&json!({"version":1,"themes":entries})).unwrap(),
        )
        .unwrap()
    }

    impl Provider for Fixture {
        fn catalog(&self, source: &GithubSource) -> Result<CatalogResult, ThemeError> {
            Ok(CatalogResult {
                catalog: catalog(source),
                stale: false,
                warning: None,
            })
        }
        fn theme(&self, catalog: &Catalog, id: &str) -> Result<VerifiedTheme, ThemeError> {
            let index: usize = id.strip_prefix("theme-").unwrap().parse().unwrap();
            VerifiedTheme::parse(catalog.entry(id)?, &bytes(index))
        }
    }

    pub fn browser() -> OnlineThemes {
        let mut browser = OnlineThemes {
            provider: Some(Arc::new(Fixture)),
            ..Default::default()
        };
        browser.sync_sources(&[source("a"), source("b")]);
        browser.set_online(true);
        let job = browser.take_jobs().pop().unwrap();
        browser.complete(&job, job.run(&Fixture));
        browser
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use super::*;

    #[test]
    fn prefetch_is_twenty_and_scroll_replaces_queue_with_hover_priority() {
        let mut state = browser();
        assert_eq!(state.queue.len(), 20);
        let running = state.take_jobs();
        assert_eq!(running.len(), 3);
        state.scroll_to(45);
        assert_eq!(state.queue.len(), 20);
        assert!(
            state
                .queue
                .iter()
                .all(|job| job.id[6..].parse::<usize>().unwrap() >= 30)
        );
        state.hover("theme-29", true);
        assert_eq!(state.queue.front().unwrap().id, "theme-29");
        assert!(state.take_jobs().is_empty());
        state.complete(&running[0], running[0].run(&Fixture));
        let next = state.take_jobs();
        assert_eq!(next.len(), 1);
        assert_eq!(next[0].id, "theme-29");
        assert_eq!(state.active.len(), 3);
    }

    #[test]
    fn old_generation_never_changes_catalog_preview_selection_or_download() {
        let mut state = browser();
        state.apply("theme-0");
        state.download("theme-0");
        state.hover("theme-0", true);
        let old = state.take_jobs();
        state.select_source(1);
        assert!(state.take_jobs().is_empty());
        let effects = state.complete(&old[0], old[0].run(&Fixture));
        assert!(effects.apply.is_none() && effects.download.is_none());
        assert!(state.preview.is_none() && state.catalog.is_none());
        let new = state.take_jobs();
        assert_eq!(new.len(), 1);
        assert!(new[0].catalog.is_none());
        assert_eq!(new[0].source, source("b"));
        // More rapid switches still cannot exceed three real workers.
        state.select_source(0);
        state.select_source(1);
        assert!(state.take_jobs().is_empty());
    }

    #[test]
    fn pending_click_is_explicit_and_cancelled_by_leaving_or_configuration_changes() {
        let mut state = browser();
        assert!(state.apply("theme-0").is_none());
        let jobs = state.take_jobs();
        let selected = state
            .complete(&jobs[0], jobs[0].run(&Fixture))
            .apply
            .unwrap();
        assert!(selected.identity.contains(&source("a").key()));
        assert_eq!(selected.manifest.id, "theme-0");
        assert!(state.preview.is_none());
        state.apply("theme-1");
        state.cancel_interaction();
        assert!(
            state
                .complete(&jobs[1], jobs[1].run(&Fixture))
                .apply
                .is_none()
        );
        state.apply("theme-2");
        state.sync_sources(&[source("b")]);
        assert!(
            state
                .complete(&jobs[2], jobs[2].run(&Fixture))
                .apply
                .is_none()
        );
    }

    #[test]
    fn hover_completion_is_identity_guarded_and_keeps_last_valid_picture() {
        let mut state = browser();
        let jobs = state.take_jobs();
        state.hover("theme-0", true);
        state.complete(&jobs[0], jobs[0].run(&Fixture));
        assert_eq!(state.preview.as_ref().unwrap().manifest.id, "theme-0");
        state.hover("theme-1", true);
        assert_eq!(state.preview.as_ref().unwrap().manifest.id, "theme-0");
        state.hover("theme-2", true);
        state.complete(&jobs[1], jobs[1].run(&Fixture));
        assert_eq!(state.preview.as_ref().unwrap().manifest.id, "theme-0");
        state.complete(&jobs[2], jobs[2].run(&Fixture));
        assert_eq!(state.preview.as_ref().unwrap().manifest.id, "theme-2");
        state.hover("theme-1", false);
        assert!(state.preview.is_some());
        state.hover("theme-2", false);
        assert!(state.preview.is_none());
    }

    #[test]
    fn failures_are_diagnostic_only_and_memory_cache_is_bounded() {
        let mut state = browser();
        state.apply("theme-0");
        state.download("theme-0");
        let jobs = state.take_jobs();
        let effects = state.complete(
            &jobs[0],
            Reply::Theme(Err(ThemeError {
                code: "THEME_HASH",
                message: "摘要错误".into(),
            })),
        );
        assert!(effects.apply.is_none() && effects.download.is_none());
        assert!(state.pending_apply.is_none() && state.pending_download.is_none());
        assert!(state.diagnostics.back().unwrap().contains("摘要错误"));
        for job in &jobs[1..] {
            state.complete(job, job.run(&Fixture));
        }
        for center in [0, 20, 40] {
            state.scroll_to(center);
            loop {
                let jobs = state.take_jobs();
                if jobs.is_empty() {
                    break;
                }
                for job in jobs {
                    state.complete(&job, job.run(&Fixture));
                }
            }
        }
        assert!(state.ready.len() <= 32);
        assert_eq!(state.lru.len(), state.ready.len());
        for index in 0..50 {
            state.diagnostic(format!("error-{index}"));
        }
        assert_eq!(state.diagnostics.len(), 16);
    }

    #[test]
    fn download_requires_an_explicit_request_and_keeps_original_bytes() {
        let mut state = browser();
        assert!(state.download("theme-0").is_none());
        let jobs = state.take_jobs();
        let effects = state.complete(&jobs[0], jobs[0].run(&Fixture));
        let (source, theme) = effects.download.unwrap();
        assert_eq!(source, test_support::source("a"));
        assert_eq!(theme.bytes(), bytes(0));
        assert!(effects.apply.is_none());
        assert!(
            state
                .complete(&jobs[1], jobs[1].run(&Fixture))
                .download
                .is_none()
        );
    }

    #[test]
    fn failed_explicit_requests_can_retry_and_invalid_ids_never_lock_downloads() {
        let mut state = browser();
        assert!(state.download("missing").is_none());
        assert!(!state.download_pending());
        assert!(state.apply("missing").is_none());
        assert!(state.pending_apply.is_none());
        state.download("theme-0");
        let jobs = state.take_jobs();
        let result = state.complete(
            &jobs[0],
            Reply::Theme(Err(ThemeError {
                code: "SOURCE_NETWORK",
                message: "网络暂不可用".into(),
            })),
        );
        assert!(result.download_error.is_some());
        assert!(!state.download_pending());
        state.download("theme-0");
        assert!(state.download_pending());
        let retry = state.take_jobs();
        assert_eq!(retry.len(), 1);
        assert_eq!(retry[0].id, "theme-0");
        assert!(
            state
                .complete(&retry[0], retry[0].run(&Fixture))
                .download
                .is_some()
        );
        assert!(!state.download_pending());
    }
}

impl Provider for GithubClient {
    fn catalog(&self, source: &GithubSource) -> Result<CatalogResult, ThemeError> {
        self.catalog(source, false)
    }
    fn theme(&self, catalog: &Catalog, id: &str) -> Result<VerifiedTheme, ThemeError> {
        self.theme(catalog, id, false)
    }
    fn remember_catalog(&self, catalog: &Catalog) {
        let _ = GithubClient::remember_catalog(self, catalog);
    }
}

#[derive(Clone)]
pub(super) struct Job {
    generation: u64,
    pub source: GithubSource,
    pub catalog: Option<Arc<Catalog>>,
    pub id: String,
}

impl Job {
    pub fn run(&self, provider: &dyn Provider) -> Reply {
        match &self.catalog {
            Some(catalog) => Reply::Theme(provider.theme(catalog, &self.id)),
            None => Reply::Catalog(provider.catalog(&self.source)),
        }
    }
    fn key(&self) -> (u64, String) {
        (self.generation, self.id.clone())
    }
}

pub(super) enum Reply {
    Catalog(Result<CatalogResult, ThemeError>),
    Theme(Result<VerifiedTheme, ThemeError>),
}

#[derive(Default)]
pub(super) struct Effects {
    pub apply: Option<BuiltinTheme>,
    pub download: Option<(GithubSource, Arc<VerifiedTheme>)>,
    pub download_error: Option<String>,
}

#[derive(Default)]
pub(super) struct OnlineThemes {
    pub online: bool,
    pub sources_open: bool,
    pub sources: Vec<GithubSource>,
    pub source_index: usize,
    pub catalog: Option<Arc<Catalog>>,
    pub catalog_failed: bool,
    pub provider: Option<Arc<dyn Provider>>,
    pub preview: Option<BuiltinTheme>,
    pub diagnostics: VecDeque<String>,
    pub downloading: bool,
    generation: u64,
    queue: VecDeque<Job>,
    active: HashSet<(u64, String)>,
    ready: HashMap<String, Arc<VerifiedTheme>>,
    lru: VecDeque<String>,
    failed: HashSet<String>,
    hovered: Option<String>,
    pending_apply: Option<String>,
    failed_apply: Option<String>,
    pending_download: Option<String>,
    center: usize,
}

impl OnlineThemes {
    pub fn sync_sources(&mut self, sources: &[GithubSource]) {
        if self.sources == sources {
            return;
        }
        let selected = self.sources.get(self.source_index).map(GithubSource::key);
        self.sources = sources.to_vec();
        self.source_index = selected
            .and_then(|selected| {
                self.sources
                    .iter()
                    .position(|source| source.key() == selected)
            })
            .unwrap_or(0);
        self.restart();
    }

    pub fn select_source(&mut self, index: usize) {
        if index >= self.sources.len() {
            return;
        }
        self.source_index = index;
        self.sources_open = false;
        self.restart();
    }

    pub fn set_online(&mut self, online: bool) {
        if self.online == online {
            return;
        }
        self.online = online;
        self.sources_open = false;
        self.restart();
    }

    fn restart(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.queue.clear();
        self.catalog = None;
        self.catalog_failed = false;
        self.ready.clear();
        self.lru.clear();
        self.failed.clear();
        self.clear_hover();
        self.pending_apply = None;
        self.failed_apply = None;
        self.pending_download = None;
        self.center = 0;
        if self.online
            && let Some(source) = self.sources.get(self.source_index)
        {
            self.queue.push_back(Job {
                generation: self.generation,
                source: source.clone(),
                catalog: None,
                id: String::new(),
            });
        }
        // In-flight old generations retain their slots until they finish. A fast
        // series of source changes therefore cannot create unbounded workers.
    }

    pub fn clear_hover(&mut self) {
        self.hovered = None;
        self.preview = None;
    }

    pub fn cancel_interaction(&mut self) {
        self.clear_hover();
        self.pending_apply = None;
    }

    pub fn diagnostic(&mut self, message: String) {
        if self.diagnostics.back() == Some(&message) {
            return;
        }
        self.diagnostics.push_back(message);
        while self.diagnostics.len() > 16 {
            self.diagnostics.pop_front();
        }
    }

    pub fn hover(&mut self, id: &str, entering: bool) {
        if entering {
            self.hovered = Some(id.to_owned());
            if let Some(theme) = self.resolved(id) {
                self.preview = Some(theme);
            }
            // If not ready, retain the last valid picture until this request is
            // verified. Merely hovering never mutates the applied theme.
        } else if self.hovered.as_deref() == Some(id) {
            self.clear_hover();
        }
        self.requeue();
    }

    pub fn apply(&mut self, id: &str) -> Option<BuiltinTheme> {
        if self
            .catalog
            .as_ref()
            .is_none_or(|catalog| catalog.entry(id).is_err())
        {
            return None;
        }
        self.failed.remove(id);
        self.failed_apply = None;
        let ready = self.resolved(id);
        self.pending_apply = ready.is_none().then(|| id.to_owned());
        self.requeue();
        ready
    }

    pub fn apply_status(&self, id: &str) -> Option<&'static str> {
        if self.pending_apply.as_deref() == Some(id) {
            Some(crate::i18n::tr("正在应用…"))
        } else if self.failed_apply.as_deref() == Some(id) {
            Some(crate::i18n::tr("应用失败 · 点击重试"))
        } else {
            None
        }
    }

    pub fn download(&mut self, id: &str) -> Option<(GithubSource, Arc<VerifiedTheme>)> {
        if self.download_pending() {
            return None;
        }
        if self
            .catalog
            .as_ref()
            .is_none_or(|catalog| catalog.entry(id).is_err())
        {
            return None;
        }
        self.failed.remove(id);
        let source = self.sources.get(self.source_index)?.clone();
        if let Some(theme) = self.ready.get(id) {
            return Some((source, theme.clone()));
        }
        self.pending_download = Some(id.to_owned());
        self.requeue();
        None
    }

    pub fn download_pending(&self) -> bool {
        self.downloading || self.pending_download.is_some()
    }

    pub fn scroll_to(&mut self, center: usize) {
        self.clear_hover();
        self.center = center;
        self.requeue();
    }

    pub fn thumbnail_theme(&self, id: &str) -> Option<BuiltinTheme> {
        let theme = self.ready.get(id)?;
        BuiltinTheme::from_verified(theme, self.identity(id)?)
    }

    pub fn identity(&self, id: &str) -> Option<String> {
        let catalog = self.catalog.as_ref()?;
        catalog.entry(id).ok()?;
        Some(format!(
            "online:{}:{}:{}",
            catalog.source().key(),
            catalog.commit(),
            id
        ))
    }

    fn resolved(&mut self, id: &str) -> Option<BuiltinTheme> {
        let theme = self.ready.get(id)?;
        let mut result = BuiltinTheme::from_verified(theme, self.identity(id)?)?;
        result.source = self
            .catalog
            .as_ref()
            .map(|catalog| catalog.source().clone());
        self.lru.retain(|key| key != id);
        self.lru.push_back(id.to_owned());
        Some(result)
    }

    fn requeue(&mut self) {
        let Some(catalog) = self.catalog.clone() else {
            return;
        };
        self.queue.clear();
        let mut wanted = Vec::new();
        for id in [&self.hovered, &self.pending_apply, &self.pending_download]
            .into_iter()
            .flatten()
        {
            wanted.push(id.clone());
        }
        let window = prefetch_window(catalog.entries().len(), self.center);
        let mut nearby: Vec<_> = window.collect();
        nearby.sort_by_key(|index| index.abs_diff(self.center));
        wanted.extend(
            nearby
                .into_iter()
                .map(|index| catalog.entries()[index].id.clone()),
        );
        let mut unique = HashSet::new();
        for id in wanted {
            let key = (self.generation, id.clone());
            if catalog.entry(&id).is_ok()
                && unique.insert(id.clone())
                && !self.ready.contains_key(&id)
                && !self.failed.contains(&id)
                && !self.active.contains(&key)
            {
                self.queue.push_back(Job {
                    generation: self.generation,
                    source: catalog.source().clone(),
                    catalog: Some(catalog.clone()),
                    id,
                });
            }
        }
    }

    pub fn take_jobs(&mut self) -> Vec<Job> {
        let mut jobs = Vec::new();
        while self.active.len() < 3
            && let Some(job) = self.queue.pop_front()
        {
            self.active.insert(job.key());
            jobs.push(job);
        }
        jobs
    }

    pub fn complete(&mut self, job: &Job, reply: Reply) -> Effects {
        let mut effects = Effects::default();
        if !self.active.remove(&job.key())
            || job.generation != self.generation
            || !self.online
            || self.sources.get(self.source_index) != Some(&job.source)
        {
            return effects;
        }
        match reply {
            Reply::Catalog(Ok(result)) => {
                if result.catalog.source() != &job.source {
                    return effects;
                }
                if let Some(warning) = result.warning {
                    self.diagnostic(format!("{}：{warning}", job.source.repository()));
                }
                self.catalog = Some(Arc::new(result.catalog));
                self.requeue();
            }
            Reply::Catalog(Err(error)) => {
                self.catalog_failed = true;
                self.diagnostic(format!("{}：{error}", job.source.repository()))
            }
            Reply::Theme(Err(error)) => {
                self.failed.insert(job.id.clone());
                self.diagnostic(format!("{} / {}：{error}", job.source.repository(), job.id));
                if self.pending_apply.as_ref() == Some(&job.id) {
                    self.pending_apply = None;
                    self.failed_apply = Some(job.id.clone());
                }
                if self.pending_download.as_ref() == Some(&job.id) {
                    self.pending_download = None;
                    effects.download_error = Some(error.to_string());
                }
            }
            Reply::Theme(Ok(theme)) => {
                // Recheck the job's catalog identity, even for injected providers.
                if job.catalog.as_ref().is_none_or(|catalog| {
                    self.catalog
                        .as_ref()
                        .is_none_or(|current| catalog.commit() != current.commit())
                }) || theme.id() != job.id
                {
                    return effects;
                }
                let theme = Arc::new(theme);
                self.ready.insert(job.id.clone(), theme.clone());
                self.lru.retain(|id| id != &job.id);
                self.lru.push_back(job.id.clone());
                if self.hovered.as_ref() == Some(&job.id) {
                    self.preview = self.resolved(&job.id);
                }
                if self.pending_apply.as_ref() == Some(&job.id) {
                    self.pending_apply = None;
                    effects.apply = self.resolved(&job.id);
                }
                if self.pending_download.as_ref() == Some(&job.id) {
                    self.pending_download = None;
                    effects.download = Some((job.source.clone(), theme));
                }
                while self.ready.len() > 32 {
                    if let Some(old) = self.lru.pop_front() {
                        self.ready.remove(&old);
                    }
                }
            }
        }
        effects
    }
}
