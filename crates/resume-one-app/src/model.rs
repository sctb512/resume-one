use std::path::{Path, PathBuf};

use resume_one_core::resume::{ResumeDocument, parse_resume};
use resume_one_core::semantic::{ResumeContent, project};
use resume_one_core::theme_style::ThemeStyle;
use resume_one_core::{ThemeManifest, parse_theme};

use crate::workspace::{LocalFolder, OpenedFile};

const BUILTIN_THEMES: &[&str] = &[
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

const EXAMPLE_RESUME: &[u8] = br#"{
  "basics": {
    "name": "\u6797\u6e05\u548c",
    "label": "\u4ea7\u54c1\u8bbe\u8ba1\u5e08",
    "email": "hello@example.com",
    "summary": "\u4e13\u6ce8\u4e8e\u6e05\u6670\u3001\u514b\u5236\u4e14\u6709\u6e29\u5ea6\u7684\u4f53\u9a8c\u8bbe\u8ba1\u3002"
  },
  "work": [{
    "name": "\u793a\u4f8b\u5de5\u4f5c\u5ba4",
    "position": "\u4ea7\u54c1\u8bbe\u8ba1\u5e08",
    "startDate": "2022-01",
    "summary": "\u4ece\u6982\u5ff5\u5230\u4ea4\u4ed8\u7684\u5b8c\u6574\u8bbe\u8ba1\u3002"
  }],
  "education": [{
    "institution": "\u793a\u4f8b\u5927\u5b66",
    "area": "\u8bbe\u8ba1\u5b66"
  }],
  "skills": [{"name": "\u8bbe\u8ba1\u5de5\u5177", "keywords": ["Figma", "\u539f\u578b\u8bbe\u8ba1"]}]
}"#;

pub struct BuiltinTheme {
    pub manifest: ThemeManifest,
    pub style: ThemeStyle,
}

pub struct AppModel {
    pub document: ResumeDocument,
    pub content: ResumeContent,
    pub opened_source: Vec<u8>,
    pub preview_ready: bool,
    pub document_label: String,
    pub document_path: Option<PathBuf>,
    pub folder: Option<LocalFolder>,
    pub themes: Vec<BuiltinTheme>,
    pub selected_theme: usize,
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
            .unwrap_or_else(|| "示例简历 · 未保存".to_string());
        let themes = BUILTIN_THEMES
            .iter()
            .map(|source| {
                let manifest = parse_theme(source.as_bytes())
                    .map_err(|issues| format!("内置主题无效：{issues:?}"))?;
                let style = ThemeStyle::from_manifest(&manifest);
                Ok(BuiltinTheme { manifest, style })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let content = project(&document);
        Ok(Self {
            document,
            content,
            opened_source,
            preview_ready,
            document_label,
            document_path,
            folder,
            themes,
            selected_theme: 0,
        })
    }

    pub fn selected_style(&self) -> &ThemeStyle {
        &self.themes[self.selected_theme].style
    }

    pub fn apply_opened(&mut self, opened: OpenedFile) {
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

    pub fn select_theme(&mut self, index: usize) {
        if index < self.themes.len() {
            self.selected_theme = index;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
