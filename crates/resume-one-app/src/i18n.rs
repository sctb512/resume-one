use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    sync::OnceLock,
};

use resume_one_core::render::Language;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Preference {
    #[default]
    Auto,
    Chinese,
    English,
}

impl Preference {
    pub fn from_setting(setting: Option<&str>) -> Self {
        match setting {
            Some("zh-CN") => Self::Chinese,
            Some("en") => Self::English,
            _ => Self::Auto,
        }
    }

    pub fn setting(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Chinese => "zh-CN",
            Self::English => "en",
        }
    }

    pub fn apply(self) -> Language {
        let (language, tag) = match self {
            Self::Auto => sys_locale::get_locales()
                .find_map(|locale| {
                    let language = language_for_locale(&locale)?;
                    let tag = locale
                        .split('.')
                        .next()?
                        .replace('_', "-")
                        .to_ascii_lowercase();
                    Some((language, tag))
                })
                .unwrap_or_else(|| (Language::En, "en".into())),
            Self::Chinese => (Language::Zh, "zh-cn".into()),
            Self::English => (Language::En, "en".into()),
        };
        UI_LANGUAGE.set(language);
        UI_LOCALE.with_borrow_mut(|locale| *locale = tag);
        gpui_component::set_locale(if language == Language::Zh {
            "zh-CN"
        } else {
            "en"
        });
        language
    }
}

thread_local! {
    static UI_LANGUAGE: Cell<Language> = const { Cell::new(Language::Zh) };
    static UI_LOCALE: RefCell<String> = RefCell::new("zh-cn".into());
}

pub(super) fn theme_locale() -> String {
    UI_LOCALE.with_borrow(Clone::clone)
}

pub(super) fn is_english() -> bool {
    language() == Language::En
}

pub(super) fn language() -> Language {
    UI_LANGUAGE.get()
}

pub(super) fn tr(chinese: &'static str) -> &'static str {
    match language() {
        Language::Zh => chinese,
        Language::En => english(chinese),
    }
}

pub(super) fn english(chinese: &'static str) -> &'static str {
    static ENGLISH: OnceLock<BTreeMap<String, String>> = OnceLock::new();
    ENGLISH
        .get_or_init(|| {
            serde_json::from_str(include_str!("locales/en.json")).expect("valid English UI strings")
        })
        .get(chinese)
        .map(String::as_str)
        .unwrap_or(chinese)
}

fn language_for_locale(locale: &str) -> Option<Language> {
    match locale
        .split(['-', '_', '.'])
        .next()?
        .to_ascii_lowercase()
        .as_str()
    {
        "zh" => Some(Language::Zh),
        "en" => Some(Language::En),
        _ => None,
    }
}

/// A filename such as CV.en.json or CV.zh-CN.resume.json can specify the
/// document's presentation language independently of the workspace language.
pub(super) fn document_language(
    filename: &str,
    document: &resume_one_core::resume::ResumeDocument,
    default: Language,
) -> Language {
    let mut parts = filename.rsplit('.');
    parts.next();
    let tag = parts.find(|part| !part.eq_ignore_ascii_case("resume"));
    if let Some(language) = tag.and_then(language_for_locale) {
        return language;
    }

    let sample = document.value["basics"]["summary"]
        .as_str()
        .filter(|summary| !summary.trim().is_empty())
        .or_else(|| document.value["basics"]["label"].as_str())
        .unwrap_or_default();
    let chinese = sample
        .chars()
        .filter(|ch| matches!(ch, '\u{3400}'..='\u{9fff}'))
        .count();
    let latin = sample.chars().filter(char::is_ascii_alphabetic).count();
    if chinese >= 2 && chinese * 3 >= latin {
        Language::Zh
    } else if latin >= 4 {
        Language::En
    } else {
        default
    }
}

pub(super) fn source_name(source: &resume_one_themes::GithubSource) -> &str {
    let name = source.display_name();
    if source
        .repository()
        .eq_ignore_ascii_case("sctb512/resume-one-themes")
    {
        match (source.index_path(), name) {
            ("originals/index.json", "原版 JSON Resume") => return tr("原版 JSON Resume"),
            ("index.json" | "migrated-index.json", "迁移版社区主题") => {
                return tr("迁移版社区主题");
            }
            _ => {}
        }
    }
    name
}

#[cfg(test)]
mod tests {
    use super::*;
    use resume_one_core::resume::parse_resume;

    #[test]
    fn ui_locale_and_resume_locale_are_independent() {
        let chinese = parse_resume(include_bytes!("../../../assets/example.resume.json")).unwrap();
        let english =
            parse_resume(include_bytes!("../../../assets/example.en.resume.json")).unwrap();
        assert_eq!(language_for_locale("zh-Hant-TW"), Some(Language::Zh));
        assert_eq!(language_for_locale("en_US.UTF-8"), Some(Language::En));
        assert_eq!(language_for_locale("fr-CA"), None);
        assert_eq!(
            document_language("CV.en.resume.json", &english, Language::Zh),
            Language::En
        );
        assert_eq!(
            document_language("CV.zh-CN.json", &chinese, Language::En),
            Language::Zh
        );
        assert_eq!(
            document_language("CV.json", &chinese, Language::En),
            Language::Zh
        );
        assert_eq!(
            document_language("CV.json", &english, Language::Zh),
            Language::En
        );
        assert_eq!(
            english.source,
            include_bytes!("../../../assets/example.en.resume.json")
        );
        assert!(
            english.value["basics"]["name"]
                .as_str()
                .unwrap()
                .contains("Alex")
        );
        assert_eq!(
            super::english("悬停试用 · 点击确认\n字体与配色仅保存在本机"),
            "Hover to preview · Click to apply\nFonts and colors are saved on this device"
        );
    }
}
