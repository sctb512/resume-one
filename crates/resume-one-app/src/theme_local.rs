use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use gpui::{Context, PathPromptOptions, Window, actions};
use resume_one_core::export::write_export_unnamed;
use resume_one_core::parse_theme;
use resume_one_core::theme_style::ThemeStyle;
use resume_one_themes::{GithubSource, parse_sources};
use serde_json::{Value, json};

use crate::ResumeOne;
use crate::model::{BUILTIN_THEMES, BuiltinTheme};

actions!(resume_one, [ChooseThemeDirectory, ResetThemeDirectory]);

pub(super) struct ThemeStore {
    settings_path: PathBuf,
    default_dir: PathBuf,
    pub directory: PathBuf,
    pub warnings: Vec<String>,
    pub sources: Vec<GithubSource>,
    recent_files: Vec<PathBuf>,
    last_folder: Option<PathBuf>,
}

const MAX_RECENT_FILES: usize = 12;

fn recent_files_from(settings: &Value) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Some(items) = settings.get("recentFiles").and_then(Value::as_array) {
        for item in items {
            let Some(path) = item.as_str().map(PathBuf::from) else {
                continue;
            };
            if !path.is_absolute()
                || !path.is_file()
                || !path
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
            {
                continue;
            }
            let Ok(path) = path.canonicalize() else {
                continue;
            };
            if !files.contains(&path) {
                files.push(path);
                if files.len() == MAX_RECENT_FILES {
                    break;
                }
            }
        }
    }
    files
}

impl ThemeStore {
    pub fn system() -> Result<Self, String> {
        let data = dirs::data_local_dir().ok_or("无法定位本地应用数据目录")?;
        let config = dirs::config_dir().ok_or("无法定位本地配置目录")?;
        Self::new(
            config.join("Resume One").join("settings.json"),
            data.join("Resume One").join("themes"),
        )
    }

    pub(super) fn new(settings_path: PathBuf, default_dir: PathBuf) -> Result<Self, String> {
        fs::create_dir_all(&default_dir)
            .map_err(|error| format!("无法创建默认主题目录：{error}"))?;
        let default_dir = default_dir
            .canonicalize()
            .map_err(|error| format!("无法定位默认主题目录：{error}"))?;
        let mut warnings = Vec::new();
        let settings = match Self::read_settings(&settings_path) {
            Ok(settings) => settings,
            Err(message) => {
                warnings.push(format!("{message}；暂时使用默认设置，未改写原文件"));
                json!({})
            }
        };
        let chosen = settings
            .get("localThemeDirectory")
            .and_then(Value::as_str)
            .map(PathBuf::from);
        let sources = match settings
            .get("onlineThemeSources")
            .map(parse_sources)
            .transpose()
        {
            Ok(sources) => sources.unwrap_or_default(),
            Err(error) => {
                warnings.push(error.to_string());
                Vec::new()
            }
        };
        let recent_files = recent_files_from(&settings);
        let last_folder = settings
            .get("lastOpenedDirectory")
            .and_then(Value::as_str)
            .map(PathBuf::from)
            .filter(|path| path.is_absolute() && path.is_dir())
            .and_then(|path| path.canonicalize().ok());
        let directory = match chosen {
            Some(path) if path.is_absolute() && path.is_dir() => path,
            Some(_) => {
                warnings.push("已设置的主题目录不可用，暂时使用默认目录".into());
                default_dir.clone()
            }
            None => default_dir.clone(),
        };
        let store = Self {
            settings_path,
            default_dir,
            directory,
            warnings,
            sources,
            recent_files,
            last_folder,
        };
        Self::seed_defaults(&store.directory)?;
        Ok(store)
    }

    fn read_settings(path: &Path) -> Result<Value, String> {
        use std::io::Read;
        let file = match fs::File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(json!({})),
            Err(error) => return Err(format!("无法读取主题设置：{error}")),
        };
        let mut bytes = Vec::new();
        file.take(1_048_577)
            .read_to_end(&mut bytes)
            .map_err(|error| format!("无法读取主题设置：{error}"))?;
        if bytes.len() > 1_048_576 {
            return Err("主题设置超过 1 MiB，拒绝覆盖".into());
        }
        serde_json::from_slice::<Value>(&bytes)
            .ok()
            .filter(Value::is_object)
            .ok_or_else(|| "主题设置不是有效的 JSON 对象，拒绝覆盖".into())
    }

    fn write_settings(&self, settings: &Value) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(settings).map_err(|error| error.to_string())?;
        if bytes.len() > 1_048_576 {
            return Err("主题设置将超过 1 MiB，未保存".into());
        }
        let parent = self.settings_path.parent().ok_or("主题设置路径无效")?;
        fs::create_dir_all(parent).map_err(|error| format!("无法创建配置目录：{error}"))?;
        write_export_unnamed(&self.settings_path, &bytes, true)
            .map_err(|error| format!("无法保存主题设置：{}", error.message))
    }

    pub fn save_sources(&mut self, sources: Vec<GithubSource>) -> Result<(), String> {
        let value = json!(
            sources
                .iter()
                .map(GithubSource::as_json)
                .collect::<Vec<_>>()
        );
        let sources = parse_sources(&value).map_err(|error| error.to_string())?;
        let mut settings = Self::read_settings(&self.settings_path)?;
        settings["onlineThemeSources"] = value;
        self.write_settings(&settings)?;
        self.sources = sources;
        Ok(())
    }

    pub fn recent_files(&self) -> &[PathBuf] {
        &self.recent_files
    }

    pub fn last_folder(&self) -> Option<&Path> {
        self.last_folder.as_deref()
    }

    pub fn remember_file(&mut self, path: &Path) -> Result<(), String> {
        if !path.is_absolute() || !path.is_file() {
            return Err("最近文件必须是已打开的绝对文件路径".into());
        }
        let path = path.canonicalize().map_err(|error| error.to_string())?;
        let mut recent = self.recent_files.clone();
        recent.retain(|item| item != &path);
        recent.insert(0, path);
        recent.truncate(MAX_RECENT_FILES);
        let mut settings = Self::read_settings(&self.settings_path)?;
        settings["recentFiles"] = json!(recent);
        self.write_settings(&settings)?;
        self.recent_files = recent;
        Ok(())
    }

    pub fn remember_folder(&mut self, path: &Path) -> Result<(), String> {
        if !path.is_absolute() || !path.is_dir() {
            return Err("最近目录必须是已打开的绝对目录路径".into());
        }
        let path = path.canonicalize().map_err(|error| error.to_string())?;
        let mut settings = Self::read_settings(&self.settings_path)?;
        settings["lastOpenedDirectory"] = json!(path);
        self.write_settings(&settings)?;
        self.last_folder = Some(path);
        Ok(())
    }

    fn seed_defaults(directory: &Path) -> Result<(), String> {
        fs::create_dir_all(directory).map_err(|error| format!("无法创建主题目录：{error}"))?;
        for source in BUILTIN_THEMES {
            let manifest = parse_theme(source.as_bytes())
                .map_err(|issues| format!("内置主题无效：{issues:?}"))?;
            let target = directory.join(format!("{}.theme.json", manifest.id));
            if target.exists() {
                continue;
            }
            if let Err(error) = write_export_unnamed(&target, source.as_bytes(), false)
                && error.code != "FILE_EXISTS"
            {
                return Err(format!(
                    "无法安装默认主题 {}：{}",
                    manifest.id, error.message
                ));
            }
        }
        Ok(())
    }

    fn theme_paths(directory: &Path) -> Result<Vec<PathBuf>, String> {
        let mut paths = Vec::new();
        let entries =
            fs::read_dir(directory).map_err(|error| format!("无法读取主题目录：{error}"))?;
        for entry in entries {
            let entry = entry.map_err(|error| format!("无法读取主题条目：{error}"))?;
            let file_type = entry
                .file_type()
                .map_err(|error| format!("无法读取主题类型：{error}"))?;
            let path = entry.path();
            if file_type.is_file()
                && path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().ends_with(".theme.json"))
            {
                paths.push(path);
            } else if file_type.is_dir() {
                let child = path.join("theme.json");
                if child.is_file()
                    && !fs::symlink_metadata(&child)
                        .map_err(|error| format!("无法读取主题类型：{error}"))?
                        .file_type()
                        .is_symlink()
                {
                    paths.push(child);
                }
            }
        }
        paths.sort();
        Ok(paths)
    }

    fn scan(directory: &Path) -> Result<(Vec<BuiltinTheme>, Vec<String>), String> {
        Self::seed_defaults(directory)?;
        let mut themes = Vec::new();
        let mut warnings = Vec::new();
        for path in Self::theme_paths(directory)? {
            match fs::metadata(&path) {
                Ok(metadata) if metadata.len() > 1024 * 1024 => {
                    warnings.push(format!("{}：主题文件超过 1 MiB 上限", path.display()));
                    continue;
                }
                Err(error) => {
                    warnings.push(format!("{}：{error}", path.display()));
                    continue;
                }
                _ => {}
            }
            let bytes = match fs::read(&path) {
                Ok(bytes) => bytes,
                Err(error) => {
                    warnings.push(format!("{}：{error}", path.display()));
                    continue;
                }
            };
            let manifest = match parse_theme(&bytes) {
                Ok(manifest) => manifest,
                Err(issues) => {
                    warnings.push(format!("{}：{issues:?}", path.display()));
                    continue;
                }
            };
            let style = ThemeStyle::from_manifest(&manifest);
            themes.push(BuiltinTheme {
                identity: format!(
                    "local:{}",
                    path.canonicalize()
                        .unwrap_or_else(|_| path.clone())
                        .display()
                ),
                manifest,
                style,
                path: Some(path),
            });
        }
        if themes.is_empty() {
            return Err("主题目录没有有效的声明式主题".into());
        }
        let builtin_order: HashMap<String, usize> = BUILTIN_THEMES
            .iter()
            .enumerate()
            .filter_map(|(index, source)| {
                parse_theme(source.as_bytes())
                    .ok()
                    .map(|manifest| (manifest.id, index))
            })
            .collect();
        themes.sort_by_key(|theme| {
            builtin_order
                .get(&theme.manifest.id)
                .copied()
                .unwrap_or(usize::MAX)
        });
        Ok((themes, warnings))
    }

    pub fn load_themes(&mut self) -> Result<Vec<BuiltinTheme>, String> {
        let (themes, warnings) = Self::scan(&self.directory)?;
        self.warnings.extend(warnings);
        Ok(themes)
    }

    fn switch(&mut self, directory: PathBuf) -> Result<Vec<BuiltinTheme>, String> {
        let mut settings = Self::read_settings(&self.settings_path)?;
        let directory = directory
            .canonicalize()
            .map_err(|error| format!("主题目录不可访问：{error}"))?;
        if !directory.is_dir() {
            return Err("主题位置必须是目录".into());
        }
        let (themes, warnings) = Self::scan(&directory)?;
        let value = if directory == self.default_dir {
            Value::Null
        } else {
            json!(directory)
        };
        settings["localThemeDirectory"] = value;
        self.write_settings(&settings)?;
        self.directory = directory;
        self.warnings = warnings;
        Ok(themes)
    }

    pub fn choose(&mut self, directory: PathBuf) -> Result<Vec<BuiltinTheme>, String> {
        self.switch(directory)
    }

    pub fn reset(&mut self) -> Result<Vec<BuiltinTheme>, String> {
        fs::create_dir_all(&self.default_dir)
            .map_err(|error| format!("无法创建默认主题目录：{error}"))?;
        self.switch(self.default_dir.clone())
    }
}

impl ResumeOne {
    pub(super) fn choose_theme_directory(
        &mut self,
        _: &ChooseThemeDirectory,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.online.download_pending()
            || self.files.busy
            || self.export_busy
            || self.editor.as_ref().is_some_and(|e| e.saving)
        {
            return;
        }
        let choice = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("选择本地主题目录".into()),
        });
        self.files.busy = true;
        cx.spawn_in(window, async move |this, cx| {
            let result = choice.await;
            this.update_in(cx, |this, window, cx| {
                this.files.busy = false;
                match result {
                    Ok(Ok(Some(paths))) => {
                        if let Some(path) = paths.into_iter().next() {
                            let changed = this
                                .theme_store
                                .as_mut()
                                .ok_or_else(|| "主题目录不可用".to_string())
                                .and_then(|store| store.choose(path));
                            this.finish_theme_change(changed, window, cx);
                        }
                    }
                    Ok(Ok(None)) | Err(_) => {}
                    Ok(Err(error)) => {
                        this.editor_warning("无法选择主题目录", &error.to_string(), window, cx)
                    }
                }
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn reset_theme_directory(
        &mut self,
        _: &ResetThemeDirectory,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.online.download_pending() || self.files.busy {
            return;
        }
        let changed = self
            .theme_store
            .as_mut()
            .ok_or_else(|| "主题目录不可用".to_string())
            .and_then(ThemeStore::reset);
        self.finish_theme_change(changed, window, cx);
    }

    fn finish_theme_change(
        &mut self,
        result: Result<Vec<BuiltinTheme>, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(themes) => {
                self.model.replace_themes(themes);
                self.hovered_theme = None;
                cx.notify();
            }
            Err(message) => self.editor_warning("无法切换主题目录", &message, window, cx),
        }
    }

    pub(super) fn reveal_theme(&self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self
            .model
            .themes
            .get(index)
            .and_then(|theme| theme.path.as_ref())
        else {
            return;
        };
        #[cfg(target_os = "macos")]
        let result = std::process::Command::new("open")
            .arg("-R")
            .arg(path)
            .spawn();
        #[cfg(target_os = "windows")]
        let result = std::process::Command::new("explorer")
            .arg(format!("/select,{}", path.display()))
            .spawn();
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let result = std::process::Command::new("xdg-open")
            .arg(path.parent().unwrap_or(path))
            .spawn();
        if let Err(error) = result {
            self.editor_warning("无法定位主题文件", &error.to_string(), window, cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_files_and_folder_persist_without_storing_resume_content() {
        let root = tempfile::tempdir().unwrap();
        let settings = root.path().join("settings.json");
        fs::write(&settings, br#"{"future":{"preserve":true}}"#).unwrap();
        let mut store = ThemeStore::new(settings.clone(), root.path().join("themes")).unwrap();
        let folder = root.path().join("resumes");
        fs::create_dir(&folder).unwrap();
        let mut paths = Vec::new();
        for index in 0..14 {
            let path = folder.join(format!("resume-{index}.json"));
            fs::write(&path, b"{\"private\":\"never in settings\"}").unwrap();
            store.remember_file(&path).unwrap();
            paths.push(path);
        }
        store.remember_file(&paths[5]).unwrap();
        store.remember_folder(&folder).unwrap();
        let reopened = ThemeStore::new(settings.clone(), root.path().join("themes")).unwrap();
        assert_eq!(
            reopened.last_folder(),
            Some(folder.canonicalize().unwrap().as_path())
        );
        assert_eq!(reopened.recent_files().len(), 12);
        assert_eq!(reopened.recent_files()[0], paths[5].canonicalize().unwrap());
        assert_eq!(
            reopened
                .recent_files()
                .iter()
                .filter(|path| **path == paths[5].canonicalize().unwrap())
                .count(),
            1
        );
        let saved = fs::read(&settings).unwrap();
        let value: Value = serde_json::from_slice(&saved).unwrap();
        assert_eq!(value["future"]["preserve"], true);
        assert!(!String::from_utf8_lossy(&saved).contains("never in settings"));
        assert_eq!(
            fs::read(&paths[5]).unwrap(),
            b"{\"private\":\"never in settings\"}"
        );
    }

    #[test]
    fn different_files_can_share_a_theme_id_without_being_dropped() {
        let root = tempfile::tempdir().unwrap();
        let mut store = ThemeStore::new(
            root.path().join("settings.json"),
            root.path().join("themes"),
        )
        .unwrap();
        fs::write(
            store.directory.join("downloaded.theme.json"),
            BUILTIN_THEMES[0],
        )
        .unwrap();
        let themes = store.load_themes().unwrap();
        let matches: Vec<_> = themes
            .iter()
            .filter(|theme| theme.manifest.id == "zhibai")
            .collect();
        assert_eq!(matches.len(), 2);
        assert_ne!(matches[0].identity, matches[1].identity);
        assert!(store.warnings.is_empty());
    }

    #[test]
    fn multiple_sources_persist_without_changing_local_directory_or_foreign_settings() {
        let root = tempfile::tempdir().unwrap();
        let settings = root.path().join("settings.json");
        let defaults = root.path().join("default");
        fs::write(&settings, br#"{"future":{"preserve":true}}"#).unwrap();
        let mut store = ThemeStore::new(settings.clone(), defaults.clone()).unwrap();
        let first = GithubSource::new("Example/Paper", "main", "themes/index.json").unwrap();
        let second = GithubSource::new("Example/Ink", "v1", "index.json").unwrap();
        store
            .save_sources(vec![first.clone(), second.clone()])
            .unwrap();
        let saved = fs::read(&settings).unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&saved).unwrap()["future"]["preserve"],
            true
        );
        assert!(
            store
                .save_sources(vec![first.clone(), first.clone()])
                .is_err()
        );
        assert_eq!(fs::read(&settings).unwrap(), saved);
        let directory = root.path().join("chosen");
        fs::create_dir(&directory).unwrap();
        store.choose(directory).unwrap();
        let reopened = ThemeStore::new(settings.clone(), defaults.clone()).unwrap();
        assert_eq!(reopened.sources, [first.clone(), second.clone()]);
        assert_eq!(reopened.directory, store.directory);
        store.reset().unwrap();
        assert_eq!(store.sources, [first, second.clone()]);
        store.save_sources(vec![second.clone()]).unwrap();
        assert_eq!(
            ThemeStore::new(settings, defaults).unwrap().sources,
            [second]
        );
    }

    #[test]
    fn source_changes_do_not_destroy_damaged_settings() {
        let root = tempfile::tempdir().unwrap();
        let settings = root.path().join("settings.json");
        fs::write(&settings, b"{broken").unwrap();
        let mut store = ThemeStore::new(settings.clone(), root.path().join("defaults")).unwrap();
        assert!(store.save_sources(vec![]).is_err());
        assert_eq!(fs::read(&settings).unwrap(), b"{broken");
        assert!(store.sources.is_empty());
        let near_limit = serde_json::to_vec(&json!({"future":"x".repeat(1_048_550)})).unwrap();
        fs::write(&settings, &near_limit).unwrap();
        assert!(store.save_sources(vec![]).is_err());
        assert_eq!(fs::read(settings).unwrap(), near_limit);
    }

    #[test]
    fn default_theme_files_are_real_and_user_files_survive_reset() {
        let root = tempfile::tempdir().unwrap();
        let settings = root.path().join("settings.json");
        let defaults = root.path().join("defaults");
        let mut store = ThemeStore::new(settings.clone(), defaults.clone()).unwrap();
        let themes = store.load_themes().unwrap();
        assert_eq!(themes.len(), 20);
        assert_eq!(themes[0].manifest.id, "zhibai");
        let path = themes[0].path.as_ref().unwrap();
        assert!(path.is_file());
        let custom = root.path().join("custom");
        fs::create_dir(&custom).unwrap();
        let user_file = custom.join("user.theme.json");
        fs::write(&user_file, b"not a theme").unwrap();
        let mut extra: Value = serde_json::from_str(BUILTIN_THEMES[0]).unwrap();
        extra["id"] = json!("custom-theme");
        extra["name"] = json!("自定义");
        fs::write(
            custom.join("extra.theme.json"),
            serde_json::to_vec(&extra).unwrap(),
        )
        .unwrap();
        let nested = custom.join("nested");
        fs::create_dir(&nested).unwrap();
        extra["id"] = json!("nested-theme");
        fs::write(
            nested.join("theme.json"),
            serde_json::to_vec(&extra).unwrap(),
        )
        .unwrap();
        let themes = store.choose(custom.clone()).unwrap();
        assert_eq!(themes.len(), 22);
        assert!(
            themes
                .iter()
                .any(|theme| theme.manifest.id == "custom-theme")
        );
        assert!(
            themes
                .iter()
                .any(|theme| theme.manifest.id == "nested-theme")
        );
        assert!(
            store
                .warnings
                .iter()
                .any(|warning| warning.contains("user.theme.json"))
        );
        assert_eq!(fs::read(&user_file).unwrap(), b"not a theme");
        let reopened = ThemeStore::new(settings, defaults.clone()).unwrap();
        assert_eq!(reopened.directory, custom.canonicalize().unwrap());
        let mut saved: Value =
            serde_json::from_slice(&fs::read(&store.settings_path).unwrap()).unwrap();
        saved["future"] = json!({"onlineSources": ["example"]});
        fs::write(&store.settings_path, serde_json::to_vec(&saved).unwrap()).unwrap();
        assert_eq!(store.reset().unwrap().len(), 20);
        assert_eq!(store.directory, defaults.canonicalize().unwrap());
        let saved: Value =
            serde_json::from_slice(&fs::read(&store.settings_path).unwrap()).unwrap();
        assert_eq!(saved["future"]["onlineSources"], json!(["example"]));
        assert_eq!(fs::read(&user_file).unwrap(), b"not a theme");
    }

    #[test]
    fn damaged_settings_fall_back_without_removing_theme_files() {
        let root = tempfile::tempdir().unwrap();
        let settings = root.path().join("settings.json");
        let defaults = root.path().join("defaults");
        fs::write(&settings, b"{invalid").unwrap();
        let mut store = ThemeStore::new(settings.clone(), defaults.clone()).unwrap();
        assert_eq!(store.load_themes().unwrap().len(), 20);
        assert!(!store.warnings.is_empty());
        assert_eq!(fs::read(&settings).unwrap(), b"{invalid");
        let missing = root.path().join("missing");
        fs::write(
            &settings,
            serde_json::to_vec(&json!({"localThemeDirectory": missing})).unwrap(),
        )
        .unwrap();
        let store = ThemeStore::new(settings, defaults).unwrap();
        assert!(store.directory.is_dir());
        assert!(!store.warnings.is_empty());
    }
}
