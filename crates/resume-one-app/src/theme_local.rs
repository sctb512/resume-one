use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use gpui::{Context, PathPromptOptions, Window, actions};
use resume_one_core::export::write_export_unnamed;
use resume_one_core::parse_theme;
use resume_one_core::theme_style::ThemeStyle;
use serde_json::{Value, json};

use crate::ResumeOne;
use crate::model::{BUILTIN_THEMES, BuiltinTheme};

actions!(resume_one, [ChooseThemeDirectory, ResetThemeDirectory]);

pub(super) struct ThemeStore {
    settings_path: PathBuf,
    default_dir: PathBuf,
    pub directory: PathBuf,
    pub warnings: Vec<String>,
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

    fn new(settings_path: PathBuf, default_dir: PathBuf) -> Result<Self, String> {
        fs::create_dir_all(&default_dir)
            .map_err(|error| format!("无法创建默认主题目录：{error}"))?;
        let default_dir = default_dir
            .canonicalize()
            .map_err(|error| format!("无法定位默认主题目录：{error}"))?;
        let mut warnings = Vec::new();
        let chosen = match fs::read(&settings_path) {
            Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
                Ok(settings) => settings
                    .get("localThemeDirectory")
                    .and_then(Value::as_str)
                    .map(PathBuf::from),
                Err(error) => {
                    warnings.push(format!("主题设置无效，已使用默认目录：{error}"));
                    None
                }
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(format!("无法读取主题设置：{error}")),
        };
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
        };
        Self::seed_defaults(&store.directory)?;
        Ok(store)
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
        let mut ids = HashSet::new();
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
            if !ids.insert(manifest.id.clone()) {
                warnings.push(format!("主题 ID 重复，已跳过：{}", path.display()));
                continue;
            }
            let style = ThemeStyle::from_manifest(&manifest);
            themes.push(BuiltinTheme {
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
        let mut settings = fs::read(&self.settings_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .filter(Value::is_object)
            .unwrap_or_else(|| json!({}));
        settings["localThemeDirectory"] = value;
        let bytes = serde_json::to_vec_pretty(&settings)
            .map_err(|error| format!("无法序列化主题设置：{error}"))?;
        let parent = self.settings_path.parent().ok_or("主题设置路径无效")?;
        fs::create_dir_all(parent).map_err(|error| format!("无法创建配置目录：{error}"))?;
        write_export_unnamed(&self.settings_path, &bytes, true)
            .map_err(|error| format!("无法保存主题设置：{}", error.message))?;
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
        if self.files.busy || self.export_busy || self.editor.as_ref().is_some_and(|e| e.saving) {
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
