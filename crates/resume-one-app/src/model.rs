use std::{
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
};

use resume_one_core::appearance::Appearance;
use resume_one_core::resume::{ResumeDocument, parse_resume};
use resume_one_core::semantic::{ResumeContent, project};
use resume_one_core::theme_style::ThemeStyle;
use resume_one_core::{ThemeManifest, parse_theme};
use resume_one_themes::{OriginalTheme, VerifiedTheme};

use crate::workspace::{LocalFolder, OpenedFile};

pub(crate) const BUILTIN_THEMES: &[&str] = &[
    include_str!("../../../assets/themes/zhibai.theme.json"),
    include_str!("../../../assets/themes/moxu.theme.json"),
    include_str!("../../../assets/themes/songyan.theme.json"),
    include_str!("../../../assets/themes/qingci.theme.json"),
    include_str!("../../../assets/themes/yuebai.theme.json"),
    include_str!("../../../assets/themes/danzhu.theme.json"),
    include_str!("../../../assets/themes/zhimo.theme.json"),
    include_str!("../../../assets/themes/shanshui.theme.json"),
    include_str!("../../../assets/themes/linquan.theme.json"),
    include_str!("../../../assets/themes/xuanmu.theme.json"),
    include_str!("../../../assets/themes/yehe.theme.json"),
    include_str!("../../../assets/themes/fenglan.theme.json"),
    include_str!("../../../assets/themes/huanghun.theme.json"),
    include_str!("../../../assets/themes/xiaguang.theme.json"),
    include_str!("../../../assets/themes/mingsha.theme.json"),
    include_str!("../../../assets/themes/yanshan.theme.json"),
    include_str!("../../../assets/themes/zhuqing.theme.json"),
    include_str!("../../../assets/themes/qinghe.theme.json"),
    include_str!("../../../assets/themes/zhegui.theme.json"),
    include_str!("../../../assets/themes/xingye.theme.json"),
];

const EXAMPLE_RESUME: &[u8] = include_bytes!("../../../assets/example.resume.json");

#[derive(Clone)]
pub struct BuiltinTheme {
    pub manifest: ThemeManifest,
    pub style: ThemeStyle,
    pub path: Option<PathBuf>,
    pub identity: String,
    pub original: Option<Arc<OriginalTheme>>,
}

impl BuiltinTheme {
    pub fn from_original(
        theme: Arc<OriginalTheme>,
        identity: String,
        path: Option<PathBuf>,
    ) -> Self {
        // Only the shell and the module editor use the native defaults here.
        static SHELL: OnceLock<ThemeManifest> = OnceLock::new();
        let mut manifest = SHELL
            .get_or_init(|| {
                parse_theme(BUILTIN_THEMES[0].as_bytes()).expect("valid built-in shell")
            })
            .clone();
        manifest.id = theme.id.clone();
        manifest.name = theme.name.clone();
        manifest.modules = crate::SECTION_LABELS
            .iter()
            .map(|(id, _)| (*id).to_owned())
            .collect();
        manifest.normalized["license"] = serde_json::json!(theme.license);
        manifest.normalized["version"] = serde_json::json!(theme.version);
        let description = match theme.id.as_str() {
            "original-academic-cv-lite" => "居中页眉 · 学术履历",
            "original-colophon" => "编辑风格 · 栏目侧标",
            "original-elegant" => "独立页眉 · 细线分区",
            "original-even" => "浅灰页眉 · 双列阅读",
            "original-executive-slate" => "石蓝侧栏 · 商务双列",
            "original-flat" => "赭金标题 · 栏目侧标",
            "original-kendall" => "居中页眉 · 蓝绿双列",
            "original-lucide" => "亮蓝侧栏 · 清晰层级",
            "original-macchiato" => "墨绿细线 · 左栏信息",
            "original-minyma" => "居中姓名 · 黑白留白",
            "original-paper-plus-plus" => "纸本表格 · 左侧栏目",
            "original-pro" => "衬线姓名 · 细线分隔",
            "original-pumpkin" => "墨黑页眉 · 金色点缀",
            _ => "",
        };
        manifest.normalized["description"] = serde_json::json!(description);
        manifest.normalized["provenance"] = serde_json::json!({
            "upstreamUrl": theme.upstream, "upstreamRevision": theme.version, "upstreamAuthor": theme.author
        });
        let style = ThemeStyle::from_manifest(&manifest);
        Self {
            manifest,
            style,
            identity,
            path,
            original: Some(theme),
        }
    }

    pub fn from_verified(theme: &VerifiedTheme, identity: String) -> Option<Self> {
        if let Some(original) = theme.original() {
            return Some(Self::from_original(original.clone(), identity, None));
        }
        let manifest = theme.manifest()?.clone();
        let style = ThemeStyle::from_manifest(&manifest);
        Some(Self {
            manifest,
            style,
            identity,
            path: None,
            original: None,
        })
    }
}

pub struct AppModel {
    pub appearance: Appearance,
    appearance_preview: Option<Appearance>,
    pub document: ResumeDocument,
    pub content: ResumeContent,
    pub opened_source: Vec<u8>,
    pub preview_ready: bool,
    pub document_label: String,
    pub document_path: Option<PathBuf>,
    pub is_example: bool,
    pub folder: Option<LocalFolder>,
    pub themes: Vec<BuiltinTheme>,
    pub selected_theme: usize,
    pub theme_epoch: u64,
    pub applied_online: Option<BuiltinTheme>,
}

impl AppModel {
    pub fn load(path: Option<&Path>) -> Result<Self, String> {
        let example =
            parse_resume(EXAMPLE_RESUME).map_err(|issues| format!("内置示例无效：{issues:?}"))?;
        let opened = path
            .filter(|path| !path.is_dir())
            .map(OpenedFile::read)
            .transpose()?;
        let preview_ready = opened
            .as_ref()
            .is_none_or(|opened| opened.document.is_some());
        let opened_source = opened
            .as_ref()
            .map_or_else(|| example.source.clone(), |opened| opened.source.clone());
        let document = opened
            .as_ref()
            .and_then(|opened| opened.document.clone())
            .unwrap_or(example);
        let document_path = opened.as_ref().map(|opened| opened.path.clone());
        let folder = path
            .filter(|path| path.is_dir())
            .map(LocalFolder::scan)
            .transpose()?;
        let document_label = document_path
            .as_deref()
            .and_then(Path::file_name)
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "示例简历".to_string());
        let themes = BUILTIN_THEMES
            .iter()
            .map(|source| {
                let manifest = parse_theme(source.as_bytes())
                    .map_err(|issues| format!("内置主题无效：{issues:?}"))?;
                let style = ThemeStyle::from_manifest(&manifest);
                Ok(BuiltinTheme {
                    identity: format!("builtin:{}", manifest.id),
                    manifest,
                    style,
                    path: None,
                    original: None,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let content = project(&document);
        Ok(Self {
            appearance: Appearance::default(),
            appearance_preview: None,
            document,
            content,
            opened_source,
            preview_ready,
            document_label,
            document_path,
            is_example: opened.is_none(),
            folder,
            themes,
            selected_theme: 0,
            theme_epoch: 0,
            applied_online: None,
        })
    }

    pub fn selected_style(&self) -> ThemeStyle {
        self.style_with_appearance(self.applied_theme(), &self.appearance)
    }

    pub fn effective_style(&self, theme: &BuiltinTheme) -> ThemeStyle {
        self.style_with_appearance(theme, self.display_appearance())
    }

    fn style_with_appearance(&self, theme: &BuiltinTheme, appearance: &Appearance) -> ThemeStyle {
        if appearance == &Appearance::default() {
            theme.style.clone()
        } else {
            ThemeStyle::from_manifest(&appearance.apply(&theme.manifest))
        }
    }

    pub fn display_appearance(&self) -> &Appearance {
        self.appearance_preview.as_ref().unwrap_or(&self.appearance)
    }

    pub fn preview_appearance(&mut self, appearance: Option<Appearance>) {
        if self.display_appearance() != appearance.as_ref().unwrap_or(&self.appearance) {
            self.theme_epoch = self.theme_epoch.wrapping_add(1);
        }
        self.appearance_preview = appearance;
    }

    pub fn set_appearance(&mut self, appearance: Appearance) {
        if self.appearance != appearance {
            self.appearance = appearance;
            self.theme_epoch = self.theme_epoch.wrapping_add(1);
        }
    }

    pub fn applied_theme(&self) -> &BuiltinTheme {
        self.applied_online
            .as_ref()
            .unwrap_or(&self.themes[self.selected_theme])
    }

    pub fn replace_themes(&mut self, themes: Vec<BuiltinTheme>) {
        self.theme_epoch = self.theme_epoch.wrapping_add(1);
        let selected_identity = self
            .themes
            .get(self.selected_theme)
            .map(|theme| &theme.identity);
        self.selected_theme = selected_identity
            .and_then(|identity| themes.iter().position(|theme| &theme.identity == identity))
            .unwrap_or(0);
        self.themes = themes;
    }

    pub fn apply_opened(&mut self, opened: OpenedFile) {
        self.is_example = false;
        self.document_label = opened
            .path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        self.document_path = Some(opened.path);
        self.opened_source = opened.source;
        self.preview_ready = opened.document.is_some();
        if let (Some(document), Some(content)) = (opened.document, opened.content) {
            self.document = document;
            self.content = content;
        }
    }

    pub fn new_document(&mut self) {
        let document = parse_resume(b"{\n  \"basics\": {}\n}\n").expect("valid blank resume");
        self.content = project(&document);
        self.opened_source = document.source.clone();
        self.document = document;
        self.preview_ready = true;
        self.document_path = None;
        self.document_label = "未命名简历".into();
        self.is_example = false;
    }

    pub fn select_theme(&mut self, index: usize) {
        if index < self.themes.len() {
            self.selected_theme = index;
            self.applied_online = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applied_online_snapshot_survives_local_reload_and_browsing() {
        let mut model = AppModel::load(None).unwrap();
        let source = model.document.source.clone();
        let mut online = model.themes[2].clone();
        online.identity = "online:source-a:commit:songyan".into();
        model.applied_online = Some(online.clone());
        model.replace_themes(vec![model.themes[0].clone()]);
        assert_eq!(model.applied_theme().identity, online.identity);
        assert_eq!(model.selected_style().id, online.style.id);
        assert_eq!(model.document.source, source);
        model.select_theme(0);
        assert!(model.applied_online.is_none());
        assert_ne!(model.applied_theme().identity, online.identity);
    }

    #[test]
    fn local_selection_uses_file_identity_not_manifest_id() {
        let mut model = AppModel::load(None).unwrap();
        let mut a = model.themes[0].clone();
        a.identity = "local:/themes/a.theme.json".into();
        let mut b = a.clone();
        b.identity = "local:/themes/b.theme.json".into();
        model.replace_themes(vec![a.clone(), b.clone()]);
        model.select_theme(1);
        model.replace_themes(vec![b.clone(), a]);
        assert_eq!(model.selected_theme, 0);
        assert_eq!(model.applied_theme().identity, b.identity);
    }

    #[test]
    fn all_builtins_load_and_keep_resume_source_unchanged() {
        let mut model = AppModel::load(None).unwrap();
        assert_eq!(model.themes.len(), 20);
        let source = model.document.source.clone();
        for index in 0..model.themes.len() {
            model.select_theme(index);
            assert_eq!(model.selected_style().id, model.themes[index].manifest.id);
            assert_eq!(model.document.source, source);
        }
        model.select_theme(100);
        assert_eq!(model.selected_theme, 19);
    }

    #[test]
    fn loads_user_resume_without_rewriting_it() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("resume.json");
        let bytes = br#"{"basics":{"name":"User"},"custom":{"x":1}}"#;
        std::fs::write(&path, bytes).unwrap();
        let model = AppModel::load(Some(&path)).unwrap();
        assert_eq!(model.content.name, "User");
        assert_eq!(model.document.source, bytes);
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }

    #[test]
    fn invalid_user_resume_opens_for_repair_without_rewriting_it() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("invalid.json");
        let bytes = br#"{"work":[{"name":42}]}"#;
        std::fs::write(&path, bytes).unwrap();
        let model = AppModel::load(Some(&path)).unwrap();
        assert!(!model.preview_ready);
        assert_eq!(model.opened_source, bytes);
        assert_eq!(model.document_path, Some(path.canonicalize().unwrap()));
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }
}
