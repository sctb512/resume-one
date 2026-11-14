use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::{Context, PathPromptOptions, Window, actions};
use resume_one_core::appearance::Appearance;
use resume_one_core::export::write_export_unnamed;
use resume_one_core::parse_theme;
use resume_one_core::theme_style::ThemeStyle;
use resume_one_themes::{
    GithubSource, ORIGINAL_THEME_SUFFIX, OriginalTheme, parse_sources, sha256,
};
use serde_json::{Value, json};

use crate::ResumeOne;
use crate::model::{BUILTIN_LOCALES, BUILTIN_THEMES, BuiltinTheme};

actions!(resume_one, [ChooseThemeDirectory, ResetThemeDirectory]);

pub(super) struct ThemeStore {
    settings_path: PathBuf,
    default_dir: PathBuf,
    pub directory: PathBuf,
    pub warnings: Vec<String>,
    pub sources: Vec<GithubSource>,
    pub appearance: Appearance,
    pub ui_language: crate::i18n::Preference,
    recent_files: Vec<PathBuf>,
    last_folder: Option<PathBuf>,
}

const MAX_RECENT_FILES: usize = 12;
const MAX_DOCUMENT_THEMES: usize = 32;

// Exact bytes shipped before the first visual refinement. Only untouched
// copies of these files may be replaced; a user's edits always win.
const LEGACY_BUNDLED_SHA256: &[(&str, &str)] = &[
    (
        "danzhu",
        "76b989a8e2b357b7da629fb7416c3b19b40b7aa5bc66499814b30ad13dff78a7",
    ),
    (
        "fenglan",
        "3987a85a64fcec1104222783a6d98b16089ede680aa391eec449d1b375d4a460",
    ),
    (
        "huanghun",
        "a731bbbe478c76b90c34eff2b87f63297af68174ae3286a145a7210ca127fb1d",
    ),
    (
        "linquan",
        "8c563530e830867c984b2c5a5d55fdf2e25dea243a174f1bc576357f5af4771b",
    ),
    (
        "mingsha",
        "6853f21768e4dbc27a33c0c1ada8b4e95797dd24471690be4568311e44ec7556",
    ),
    (
        "moxu",
        "a8e01a12b969e532d9fa6bb02a2b30a34573b65ece1704bf136da9081c8866b0",
    ),
    (
        "qingci",
        "0695c4fcb71cf3f0a6766262b7293fd912c1338fecb064f5f95589dd026f87f1",
    ),
    (
        "qinghe",
        "b13ae34609e36dd5b6107df189d2570cb219f51b59e1a6b092b27386eddc4d40",
    ),
    (
        "shanshui",
        "41f2695d1efcf97ef05fa957cc32896934489f7781c00792a68ada9628069f87",
    ),
    (
        "songyan",
        "7e327f18a21bf288628f7213d593a670ea0bfaa9ddec29e5dfa957145bf072b9",
    ),
    (
        "xiaguang",
        "faf0a58fbb9add3860e996a9b2b25fa37368a0ab405074cdc890a0b82aa1a6b1",
    ),
    (
        "xingye",
        "2a36464b0c5d06659d2984a54397f3fa797e63c575ab8111a4a390572afde191",
    ),
    (
        "xuanmu",
        "45faf93896f02c3027866e5adbf3730ac43e003b2474cf9fe7f5229c4c4144c1",
    ),
    (
        "yanshan",
        "33e2f54b102dc7d0d9926b9a33e088671907d0d7bd518d7ca6ac78c23e34c3da",
    ),
    (
        "yehe",
        "ae0781107acf1450250ce1ba7e56f1f1f4f1df57e11cec20adc8a4bf33651654",
    ),
    (
        "yuebai",
        "07f4ca984b15ade41fdb5d8bafa3d936581347c913a9f496fffe6e90d0237578",
    ),
    (
        "zhegui",
        "25b6fb7321e926f411c91657de2148e7137b3da618a55e1ffa2f4a2b5e4a8e48",
    ),
    (
        "zhibai",
        "5ea078182b2d3316fa0bc91373600305a37bf4bc0195aebd81642efbc116a342",
    ),
    (
        "zhimo",
        "dfaa95444519ad5ea0ad2f19efab7caa58b07da132fcc378bf41521b3b00a6d8",
    ),
    (
        "zhuqing",
        "fc8d32977a27a8da3f37eca560a6411635aca70b407cd67ad102a8ebe09216d8",
    ),
];

// The refined files were briefly available with their old version number.
// Recognize those exact bytes when promoting them to version 1.1.0.
const REFINED_PREVERSION_SHA256: &[(&str, &str)] = &[
    (
        "danzhu",
        "da1ea8f383444082c8501794a5bd1691e292f3bf0856daaa1ab1ca6254ee77e8",
    ),
    (
        "fenglan",
        "26628ec3039e2e9718171ca37acdc0f29de4d8ebde44d7d9634fc1b731e87ab1",
    ),
    (
        "huanghun",
        "c61fd4142a9d3b54d02c27fc1a87742fb9d5559cdd1435e3bc55df94bb1c778e",
    ),
    (
        "linquan",
        "c7da3a7d5036971d5b48549e136da82cdc048ebaaaae78a8864f79581388acbf",
    ),
    (
        "mingsha",
        "5dc4e9d61241704f1fd3ad811c3980fdb327905067305a1d48f08fcbc11015bb",
    ),
    (
        "moxu",
        "111fd073cce0a86d96dc4ab05e355de41e14044eb9f40c20fbad04a4ce15f73c",
    ),
    (
        "qingci",
        "0fad71bf56776a4c54aa982cf15d270d2060e2173cd04c19698d0665e8b0914f",
    ),
    (
        "qinghe",
        "fb4381e576c7ace72a0bfd4da2b2534d47d0294b713ed775eb8b1136cbda1eae",
    ),
    (
        "shanshui",
        "2a0d5c20b8046129d5a42d747a295481ddaf75e90a608e92c647df04ac533889",
    ),
    (
        "songyan",
        "d139b260f692743451ac18bacd9f9feba7362e8216ba8b19fefe9d6019237b43",
    ),
    (
        "xiaguang",
        "7adf482aedb65db61c9b3d23bf1a18055f4e27502c44758c9c751c1e5946b10a",
    ),
    (
        "xingye",
        "6daf8a58fd8a5ac11136f6bcf934b2936abc1cd95e2cf50acdac9f893de14d68",
    ),
    (
        "xuanmu",
        "38cd3b9f9ca5b0e41335e83cb625fccfc739934382bac347de882b15bbfd4142",
    ),
    (
        "yanshan",
        "645a829c2e5e4024778fe21d4f773a934842c59fd65d9a8277c7881eea08097a",
    ),
    (
        "yehe",
        "5461bcdb453c0c4ed9e7c520419091092d807c9404dcdf0d945920ca6bfcdc2f",
    ),
    (
        "yuebai",
        "88fd48c5c8de89a6ef259caece926711d16895702c04706e9cfb6675fd444be4",
    ),
    (
        "zhegui",
        "57d97ee73e5b8043d29063f2a1da5d26549dcfae14b66ad955c68fce6d897488",
    ),
    (
        "zhibai",
        "fca8d5ddf7ea1b73c4bc751148e2f4ae5ed84a06515425c3ab1f5aef6c98c4ab",
    ),
    (
        "zhimo",
        "ba4d90280da7bc08082c40c310bc6465767445605d1fcdc8413fa1b28a134b12",
    ),
    (
        "zhuqing",
        "0c1615db94d144c87d0ffeb0055fc36c68dace5c43e66a9afe92c9a8970832d9",
    ),
];

// The first refined 1.1.0 release remains eligible for exact-byte upgrade
// when a built-in acquires the split-header composition.
const REFINED_V1_1_SHA256: &[(&str, &str)] = &[
    (
        "danzhu",
        "f2b2ec11a3b8c86a82e4b85edabac459144b96a73be8ac924427299a5a861b6a",
    ),
    (
        "moxu",
        "58f3a959e1b3fe62fdcb2e1a6e1837177224f8a1783f43eb8d050bdfd0087762",
    ),
];

// Exact shipped 1.1 and 1.2 files. User-edited themes are preserved.
const PRE_REDESIGN_SHA256: &[(&str, &str)] = &[
    (
        "danzhu",
        "ca120964a471158a7f02e6edc0edb0b06206867967afbe5238d9a07082d4610f",
    ),
    (
        "fenglan",
        "64b2ac521adeb13d9b5dcd849696e050cadbcf15a228eaeb8182d2ab076f34e6",
    ),
    (
        "huanghun",
        "9d7be6dc68da30e435fdfa606911e4e6f7a223175ebfa41206640a5242f65aea",
    ),
    (
        "linquan",
        "bb0fe52d4f534d99e6ebc8b24bdaf016db0775fac95f9e05284b4a469fdb8a6a",
    ),
    (
        "mingsha",
        "4dcc876f72d001ea914d4eeb54589c9551ccc7062f010954c3ae06e07394e751",
    ),
    (
        "moxu",
        "1f374471fdf7c6ae77ec1743fff2e9f4f134bd14e13ea5aafb56780eff4a4bb7",
    ),
    (
        "qingci",
        "2df89654cfd615822986323ff484ed9514b2eb3fb0f6b8dc6dda4052fa261dd0",
    ),
    (
        "qinghe",
        "59c7d7589cd2b31a66cdcdaa422fcbd886631b9d2f89f889f77db4f179c0ddf2",
    ),
    (
        "shanshui",
        "6cde50c496b6fcd45a7ec728344e3b464c7d3f6ce94d10cb68df3954b047d524",
    ),
    (
        "songyan",
        "2cc31e9b39f44ea272f799706675cdfa64e4298d8e46ae917ef199e13b675ef0",
    ),
    (
        "xiaguang",
        "834a93ad1fcc19df09876fb96f0d7b9ee12a12f296cf10ce509e8e4945da0de2",
    ),
    (
        "xingye",
        "c614a8d8b8caa2a7f47b132ec3de5be3706eb335c6c474bcbf81decfdf257c47",
    ),
    (
        "xuanmu",
        "362d922e259fa7add917d582d7529b9058805a5e2bd24db5abb1d8b6e70ec11e",
    ),
    (
        "yanshan",
        "145b446f14e1c8a587265e413d5312afb4dd9c128f57d5eb58155c5bc8bbfd1b",
    ),
    (
        "yehe",
        "593973df6012b448150ce74d3f3cf3ded9d5f830bf6ba82e86139acd50714fc5",
    ),
    (
        "yuebai",
        "f3e59c7780af46573cfbf3181e0851b63b0cba0a88ebd912d378ec029a16af0b",
    ),
    (
        "zhegui",
        "64cf2af899dd537f114c58cb5010bd30d911626928ba9aa7d6bfee16298c36f7",
    ),
    (
        "zhibai",
        "f77acb8e4daad81ff84063e4e40a87e32bf93022f8fd5c0bbaee27ab4d23967f",
    ),
    (
        "zhimo",
        "0ca1cecd3a28bc9bc876ffcc901a74799f9e752b3cb6f7af0d08deb9c95c1ebd",
    ),
    (
        "zhuqing",
        "a6bd35d4928e37d108ef12fb19d679a30dd612d0e7e4d83f5caaa4205556227e",
    ),
];

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
            Ok(None) if warnings.is_empty() => GithubSource::defaults().to_vec(),
            Ok(sources) => sources.unwrap_or_default(),
            Err(error) => {
                warnings.push(error.to_string());
                Vec::new()
            }
        };
        let recent_files = recent_files_from(&settings);
        let appearance = match settings
            .get("appearance")
            .map(Appearance::from_json)
            .transpose()
        {
            Ok(value) => value.unwrap_or_default(),
            Err(message) => {
                warnings.push(message);
                Appearance::default()
            }
        };
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
        let mut store = Self {
            settings_path,
            default_dir,
            directory,
            warnings,
            sources,
            appearance,
            ui_language: crate::i18n::Preference::from_setting(settings["uiLanguage"].as_str()),
            recent_files,
            last_folder,
        };
        store
            .warnings
            .extend(Self::seed_defaults(&store.directory)?);
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

    pub fn save_ui_language(&mut self, preference: crate::i18n::Preference) -> Result<(), String> {
        let mut settings = Self::read_settings(&self.settings_path)?;
        settings["uiLanguage"] = json!(preference.setting());
        self.write_settings(&settings)?;
        self.ui_language = preference;
        Ok(())
    }

    pub fn save_appearance(&mut self, appearance: Appearance) -> Result<(), String> {
        let value = appearance.as_json();
        Appearance::from_json(&value)?;
        let mut settings = Self::read_settings(&self.settings_path)?;
        settings["appearance"] = value;
        self.write_settings(&settings)?;
        self.appearance = appearance;
        Ok(())
    }

    pub fn recent_files(&self) -> &[PathBuf] {
        &self.recent_files
    }

    pub fn last_folder(&self) -> Option<&Path> {
        self.last_folder.as_deref()
    }

    pub fn document_theme(&self, path: &Path) -> Option<Value> {
        let settings = Self::read_settings(&self.settings_path).ok()?;
        settings["documentThemes"]
            .as_array()?
            .iter()
            .take(MAX_DOCUMENT_THEMES)
            .find(|entry| {
                entry["file"]
                    .as_str()
                    .is_some_and(|file| Path::new(file) == path)
            })
            .map(|entry| entry["theme"].clone())
    }

    pub fn remember_document_theme(&self, path: &Path, theme: Value) -> Result<(), String> {
        let Some(path) = path.to_str() else {
            return Ok(());
        };
        let mut settings = Self::read_settings(&self.settings_path)?;
        let mut themes = settings["documentThemes"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        themes.retain(|entry| entry["file"].as_str() != Some(path));
        themes.insert(0, json!({"file": path, "theme": theme}));
        themes.truncate(MAX_DOCUMENT_THEMES);
        settings["documentThemes"] = json!(themes);
        self.write_settings(&settings)
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

    fn seed_defaults(directory: &Path) -> Result<Vec<String>, String> {
        fs::create_dir_all(directory).map_err(|error| format!("无法创建主题目录：{error}"))?;
        let mut warnings = Vec::new();
        for (source, locales) in BUILTIN_THEMES.iter().zip(BUILTIN_LOCALES) {
            let manifest = parse_theme(source.as_bytes())
                .map_err(|issues| format!("内置主题无效：{issues:?}"))?;
            let target = directory.join(format!("{}.theme.json", manifest.id));
            let mut overwrite = false;
            let mut write_layout = true;
            match fs::symlink_metadata(&target) {
                Ok(metadata) if metadata.file_type().is_file() && metadata.len() <= 1024 * 1024 => {
                    let bytes = match fs::read(&target) {
                        Ok(bytes) => bytes,
                        Err(error) => {
                            warnings.push(format!("无法读取已有主题 {}：{error}", manifest.id));
                            continue;
                        }
                    };
                    write_layout = bytes != source.as_bytes();
                    if write_layout {
                        let existing_hash = sha256(&bytes);
                        overwrite = LEGACY_BUNDLED_SHA256
                            .iter()
                            .chain(REFINED_PREVERSION_SHA256)
                            .chain(REFINED_V1_1_SHA256)
                            .chain(PRE_REDESIGN_SHA256)
                            .any(|(id, known_hash)| {
                                *id == manifest.id && existing_hash == *known_hash
                            });
                        if !overwrite {
                            continue;
                        }
                    }
                }
                Ok(_) => continue,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    warnings.push(format!("无法检查已有主题 {}：{error}", manifest.id));
                    continue;
                }
            }
            if write_layout
                && let Err(error) = write_export_unnamed(&target, source.as_bytes(), overwrite)
            {
                if overwrite {
                    warnings.push(format!(
                        "无法升级内置主题 {}：{}",
                        manifest.id, error.message
                    ));
                    continue;
                }
                if error.code == "FILE_EXISTS" {
                    continue;
                }
                return Err(format!(
                    "无法安装默认主题 {}：{}",
                    manifest.id, error.message
                ));
            }
            // Separate resources keep built-in layouts readable by previous releases after rollback.
            let translation = directory.join(format!("{}.localizations.txt", manifest.id));
            if let Err(error) = write_export_unnamed(&translation, locales, false)
                && error.code != "FILE_EXISTS"
            {
                warnings.push(format!(
                    "无法安装主题译文 {}：{}",
                    manifest.id, error.message
                ));
            }
        }
        Ok(warnings)
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

    fn scan(
        directory: &Path,
        hide_legacy: bool,
    ) -> Result<(Vec<BuiltinTheme>, Vec<String>), String> {
        let mut warnings = Self::seed_defaults(directory)?;
        let mut themes = Vec::new();
        for path in Self::theme_paths(directory)? {
            let identity = format!(
                "local:{}",
                path.canonicalize()
                    .unwrap_or_else(|_| path.clone())
                    .display()
            );
            if path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().ends_with(ORIGINAL_THEME_SUFFIX))
            {
                match OriginalTheme::load(&path) {
                    Ok(theme) => themes.push(BuiltinTheme::from_original(
                        Arc::new(theme),
                        identity,
                        Some(path),
                    )),
                    Err(error) => warnings.push(format!("{}：{error}", path.display())),
                }
                continue;
            }
            let relative = path
                .strip_prefix(directory)
                .expect("scanned inside theme directory");
            let manifest = match resume_one_core::load_theme(directory, relative) {
                Ok(manifest) => manifest,
                Err(issues) => {
                    warnings.push(format!("{}：{issues:?}", path.display()));
                    continue;
                }
            };
            if hide_legacy && crate::legacy_community::is_previous_default(&path, &manifest) {
                continue;
            }
            if let Err(error) = resume_one_core::layout::check_theme(&manifest) {
                warnings.push(format!("{}：{error}", path.display()));
                continue;
            }
            let style = ThemeStyle::from_manifest(&manifest);
            themes.push(BuiltinTheme {
                identity,
                source: None,
                original: None,
                manifest,
                style,
                path: Some(path),
            });
        }
        if themes.is_empty() {
            return Err("主题目录没有有效主题".into());
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
        let (themes, warnings) = Self::scan(&self.directory, self.directory == self.default_dir)?;
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
        let (themes, warnings) = Self::scan(&directory, directory == self.default_dir)?;
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
    fn exact_legacy_defaults_upgrade_but_user_changes_survive() {
        let legacy = include_bytes!("../tests/fixtures/zhibai-legacy.theme.json");
        let expected_hash = LEGACY_BUNDLED_SHA256
            .iter()
            .find(|(id, _)| *id == "zhibai")
            .unwrap()
            .1;
        assert_eq!(sha256(legacy), expected_hash);
        let ids: std::collections::HashSet<_> =
            LEGACY_BUNDLED_SHA256.iter().map(|(id, _)| *id).collect();
        assert_eq!(ids.len(), BUILTIN_THEMES.len());
        let refined_ids: std::collections::HashSet<_> = REFINED_PREVERSION_SHA256
            .iter()
            .map(|(id, _)| *id)
            .collect();
        assert_eq!(refined_ids, ids);
        for source in BUILTIN_THEMES {
            assert!(ids.contains(parse_theme(source.as_bytes()).unwrap().id.as_str()));
        }
        let refined_preversion =
            include_str!("../tests/fixtures/zhibai-refined-preversion.theme.json");
        let expected_refined_hash = REFINED_PREVERSION_SHA256
            .iter()
            .find(|(id, _)| *id == "zhibai")
            .unwrap()
            .1;
        assert_eq!(sha256(refined_preversion.as_bytes()), expected_refined_hash);

        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("themes");
        fs::create_dir(&directory).unwrap();
        let legacy_path = directory.join("zhibai.theme.json");
        fs::write(&legacy_path, legacy).unwrap();
        let custom_path = directory.join("moxu.theme.json");
        let mut custom: Value = serde_json::from_str(BUILTIN_THEMES[1]).unwrap();
        custom["description"] = json!("用户已经修改");
        let custom_bytes = serde_json::to_vec_pretty(&custom).unwrap();
        fs::write(&custom_path, &custom_bytes).unwrap();
        let settings = root.path().join("settings.json");
        fs::write(&settings, br#"{"future":{"preserve":true}}"#).unwrap();

        let mut store = ThemeStore::new(settings.clone(), directory).unwrap();
        assert_eq!(
            fs::read(&legacy_path).unwrap(),
            BUILTIN_THEMES[0].as_bytes()
        );
        assert_eq!(fs::read(&custom_path).unwrap(), custom_bytes);
        assert_eq!(store.load_themes().unwrap().len(), BUILTIN_THEMES.len());
        assert_eq!(fs::read(&custom_path).unwrap(), custom_bytes);
        assert_eq!(
            fs::read(&settings).unwrap(),
            br#"{"future":{"preserve":true}}"#
        );
        fs::write(&legacy_path, b"user edit after upgrade").unwrap();
        store.load_themes().unwrap();
        assert_eq!(fs::read(&legacy_path).unwrap(), b"user edit after upgrade");

        let preversion_dir = root.path().join("refined-preversion");
        fs::create_dir(&preversion_dir).unwrap();
        let preversion_path = preversion_dir.join("zhibai.theme.json");
        fs::write(&preversion_path, refined_preversion).unwrap();
        ThemeStore::new(root.path().join("other-settings.json"), preversion_dir).unwrap();
        assert_eq!(
            fs::read(preversion_path).unwrap(),
            BUILTIN_THEMES[0].as_bytes()
        );
    }

    #[test]
    fn pristine_refined_themes_upgrade_to_split_header_without_touching_custom_edits() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("themes");
        fs::create_dir(&directory).unwrap();
        for (id, current, old_description, new_description, composition) in [
            (
                "danzhu",
                include_str!("../tests/fixtures/danzhu-refined.theme.json"),
                "无衬线大标题、左侧时间与朱砂细线，现代而克制。",
                "朱砂无衬线分栏页眉、左侧时间与细线，现代而克制。",
                "  \"composition\": { \"preset\": \"split-header\", \"header\": \"split\", \"contact\": \"inline\" },\n",
            ),
            (
                "moxu",
                include_str!("../tests/fixtures/moxu-refined.theme.json"),
                "无衬线信息层级、左侧时间与墨青细线。",
                "无衬线分栏页眉、左侧时间与墨青细线。",
                "  \"composition\": { \"preset\": \"split-header\", \"header\": \"split\", \"contact\": \"stacked\" },\n",
            ),
        ] {
            let old = current
                .replace(new_description, old_description)
                .replace("\"version\": \"1.2.0\"", "\"version\": \"1.1.0\"")
                .replace(composition, "");
            let expected = REFINED_V1_1_SHA256
                .iter()
                .find(|(known_id, _)| *known_id == id)
                .unwrap()
                .1;
            assert_eq!(sha256(old.as_bytes()), expected);
            assert!(
                PRE_REDESIGN_SHA256
                    .iter()
                    .any(|(known_id, hash)| *known_id == id && *hash == sha256(current.as_bytes()))
            );
            fs::write(directory.join(format!("{id}.theme.json")), current).unwrap();
        }
        let mut store =
            ThemeStore::new(root.path().join("settings.json"), directory.clone()).unwrap();
        for id in ["danzhu", "moxu"] {
            let current = BUILTIN_THEMES
                .iter()
                .find(|source| parse_theme(source.as_bytes()).unwrap().id == id)
                .unwrap();
            assert_eq!(
                fs::read(directory.join(format!("{id}.theme.json"))).unwrap(),
                current.as_bytes()
            );
        }
        let customized = b"user customized split theme";
        fs::write(directory.join("danzhu.theme.json"), customized).unwrap();
        store.load_themes().unwrap();
        assert_eq!(
            fs::read(directory.join("danzhu.theme.json")).unwrap(),
            customized
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_old_default_is_never_replaced() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("themes");
        fs::create_dir(&directory).unwrap();
        let external = root.path().join("external.theme.json");
        let legacy = include_bytes!("../tests/fixtures/zhibai-legacy.theme.json");
        fs::write(&external, legacy).unwrap();
        let link = directory.join("zhibai.theme.json");
        std::os::unix::fs::symlink(&external, &link).unwrap();
        ThemeStore::new(root.path().join("settings.json"), directory).unwrap();
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read(&external).unwrap(), legacy);
    }

    #[test]
    fn appearance_settings_persist_independently_and_never_replace_damaged_settings() {
        let root = tempfile::tempdir().unwrap();
        let settings = root.path().join("settings.json");
        fs::write(&settings, br#"{"future":{"preserve":true}}"#).unwrap();
        let defaults = root.path().join("themes");
        let mut store = ThemeStore::new(settings.clone(), defaults.clone()).unwrap();
        let theme = defaults.join("zhibai.theme.json");
        let original = fs::read(&theme).unwrap();
        let mut appearance = Appearance::default();
        appearance.body_font = Some(resume_one_core::appearance::FontChoice::Serif);
        appearance.set_color("accent", "#315A71").unwrap();
        store.save_appearance(appearance.clone()).unwrap();
        store
            .save_sources(vec![
                GithubSource::new("Example/Themes", "main", "themes/index.json").unwrap(),
            ])
            .unwrap();
        let reopened = ThemeStore::new(settings.clone(), defaults.clone()).unwrap();
        assert_eq!(reopened.appearance, appearance);
        assert_eq!(reopened.sources.len(), 1);
        assert_eq!(fs::read(&theme).unwrap(), original);
        let json: Value = serde_json::from_slice(&fs::read(&settings).unwrap()).unwrap();
        assert_eq!(json["future"]["preserve"], true);
        fs::write(&settings, b"damaged settings").unwrap();
        assert!(store.save_appearance(Appearance::default()).is_err());
        assert_eq!(store.appearance, appearance);
        assert_eq!(fs::read(&settings).unwrap(), b"damaged settings");
    }

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
    fn local_font_packages_are_verified_before_becoming_available() {
        let root = tempfile::tempdir().unwrap();
        let mut store = ThemeStore::new(
            root.path().join("settings.json"),
            root.path().join("themes"),
        )
        .unwrap();
        let package = store.directory.join("font-fixture");
        fs::create_dir(&package).unwrap();
        let bytes = include_bytes!("../../resume-one-core/tests/fixtures/theme-font.ttf");
        fs::write(package.join("fixture.ttf"), bytes).unwrap();
        let mut theme: Value = serde_json::from_str(BUILTIN_THEMES[0]).unwrap();
        theme["id"] = json!("font-fixture");
        theme["assets"] = json!({"fonts":[{"id":"body", "path":"fixture.ttf", "sha256":sha256(bytes), "license":"OFL-1.1"}]});
        theme["typography"]["body"]["family"] = json!("asset:body");
        theme["typography"]["body"]["weight"] = json!(400);
        let original = theme.to_string();
        fs::write(package.join("theme.json"), &original).unwrap();
        let themes = store.load_themes().unwrap();
        assert_eq!(themes.len(), BUILTIN_THEMES.len() + 1);
        assert!(store.warnings.is_empty());
        let font_theme = themes
            .iter()
            .find(|theme| theme.manifest.id == "font-fixture")
            .unwrap();
        assert_eq!(font_theme.style.typography.body.family, "asset:body");
        assert!(resume_one_core::layout::check_theme(&font_theme.manifest).is_ok());
        fs::write(package.join("fixture.ttf"), b"tampered font").unwrap();
        assert_eq!(store.load_themes().unwrap().len(), BUILTIN_THEMES.len());
        assert!(
            store
                .warnings
                .iter()
                .any(|message| message.contains("FONT_HASH_MISMATCH"))
        );
        assert_eq!(
            fs::read_to_string(package.join("theme.json")).unwrap(),
            original
        );
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
    fn downloaded_original_is_available_from_the_configured_local_directory() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("themes");
        fs::create_dir(&path).unwrap();
        let file = path.join("local.original.theme.json");
        let theme = json!({
            "kind":"json-resume-original", "formatVersion":1, "id":"original-local", "name":"Local",
            "package":"jsonresume-theme-local", "version":"1.0.0", "license":"MIT", "author":"Example",
            "upstream":"https://example.com/theme",
            "script":"globalThis.__resumeOneTheme = { render() { return '<p>Example</p>'; } }"
        });
        fs::write(&file, serde_json::to_vec(&theme).unwrap()).unwrap();
        let (themes, warnings) = ThemeStore::scan(&path, false).unwrap();
        assert!(warnings.is_empty());
        let original = themes
            .iter()
            .find(|theme| theme.manifest.id == "original-local")
            .unwrap();
        assert!(original.original.is_some());
        assert_eq!(original.path.as_deref(), Some(file.as_path()));
        assert_eq!(original.manifest.modules.len(), crate::SECTION_LABELS.len());
        assert_eq!(themes.len(), BUILTIN_THEMES.len() + 1);
    }

    #[test]
    fn legacy_migrations_are_hidden_only_at_their_default_seed_paths() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path();
        let id = "community-colophon";
        let mut theme: Value = serde_json::from_str(BUILTIN_THEMES[0]).unwrap();
        theme["id"] = json!(id);
        theme["provenance"] = json!({
            "upstreamUrl": "https://example.com/colophon",
            "upstreamRevision": "1.0.0",
            "upstreamAuthor": "Example",
            "upstreamLicense": "MIT",
            "migratedBy": "resume-one-theme-migrate --adapt",
            "fidelity": "adapted",
        });
        let write = |name| {
            let package = directory.join(name);
            fs::create_dir(&package).unwrap();
            let file = package.join("theme.json");
            fs::write(&file, serde_json::to_vec(&theme).unwrap()).unwrap();
            file
        };
        let old = write(id);
        let downloaded = write("000000000000-community-colophon-000000000000");
        for (hide_legacy, expected) in [(true, 1), (false, 2)] {
            let (themes, warnings) = ThemeStore::scan(directory, hide_legacy).unwrap();
            assert!(warnings.is_empty(), "{warnings:?}");
            let copies: Vec<_> = themes
                .iter()
                .filter(|theme| theme.manifest.id == id)
                .collect();
            assert_eq!(copies.len(), expected);
            assert!(
                copies
                    .iter()
                    .any(|theme| theme.path.as_ref() == Some(&downloaded))
            );
        }
        assert!(old.is_file());
    }

    #[test]
    fn default_source_is_used_until_user_saves_sources() {
        let root = tempfile::tempdir().unwrap();
        let settings = root.path().join("settings.json");
        let defaults = root.path().join("themes");
        let mut store = ThemeStore::new(settings.clone(), defaults.clone()).unwrap();
        assert_eq!(store.sources, GithubSource::defaults());
        assert!(!settings.exists());
        store.save_sources(vec![]).unwrap();
        let reopened = ThemeStore::new(settings, defaults).unwrap();
        assert!(reopened.sources.is_empty());
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
        assert_eq!(themes.len(), BUILTIN_THEMES.len());
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
        assert_eq!(themes.len(), BUILTIN_THEMES.len() + 2);
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
        assert_eq!(store.reset().unwrap().len(), BUILTIN_THEMES.len());
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
        assert_eq!(store.load_themes().unwrap().len(), BUILTIN_THEMES.len());
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
