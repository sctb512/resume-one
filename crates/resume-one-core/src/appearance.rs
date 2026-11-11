//! Local presentation preferences, applied to a temporary theme snapshot only.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::ThemeManifest;

pub const COLOR_KEYS: &[&str] = &[
    "accent", "paper", "canvas", "ink", "muted", "border", "seal",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FontChoice {
    Sans,
    Serif,
    Font(String),
}

impl FontChoice {
    pub fn family(&self) -> &str {
        match self {
            Self::Sans => "cjk-sans",
            Self::Serif => "cjk-serif",
            Self::Font(name) => name,
        }
    }

    fn weight(&self) -> u16 {
        match self {
            Self::Sans => 400,
            Self::Serif => 600,
            Self::Font(_) => 400,
        }
    }

    fn parse(value: &Value) -> Result<Option<Self>, String> {
        match value {
            Value::Null => Ok(None),
            Value::String(name) if name == "cjk-sans" => Ok(Some(Self::Sans)),
            Value::String(name) if name == "cjk-serif" => Ok(Some(Self::Serif)),
            Value::String(name)
                if name
                    .strip_prefix("font:")
                    .is_some_and(crate::fonts::valid_family) =>
            {
                Ok(Some(Self::Font(name.clone())))
            }
            _ => Err("字体须为 cjk-sans、cjk-serif、font:字体名称 或 null（跟随主题）".into()),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Appearance {
    pub body_font: Option<FontChoice>,
    pub heading_font: Option<FontChoice>,
    colors: BTreeMap<String, String>,
}

impl Appearance {
    pub fn from_json(value: &Value) -> Result<Self, String> {
        let object = value.as_object().ok_or("外观设置须为对象")?;
        if object.keys().any(|key| {
            !matches!(
                key.as_str(),
                "formatVersion" | "bodyFont" | "headingFont" | "colors"
            )
        }) || value["formatVersion"].as_u64() != Some(1)
        {
            return Err("外观设置版本或字段不受支持；未改写原设置".into());
        }
        let mut result = Self {
            body_font: FontChoice::parse(&value["bodyFont"])?,
            heading_font: FontChoice::parse(&value["headingFont"])?,
            ..Default::default()
        };
        if let Some(colors) = object.get("colors") {
            for (key, value) in colors.as_object().ok_or("外观 colors 须为对象")? {
                result.set_color(key, value.as_str().ok_or("颜色须为 #RRGGBB 字符串")?)?;
            }
        }
        Ok(result)
    }

    pub fn as_json(&self) -> Value {
        json!({
            "formatVersion": 1,
            "bodyFont": self.body_font.as_ref().map(FontChoice::family),
            "headingFont": self.heading_font.as_ref().map(FontChoice::family),
            "colors": self.colors
        })
    }

    pub fn color(&self, key: &str) -> Option<&str> {
        self.colors.get(key).map(String::as_str)
    }

    pub fn set_color(&mut self, key: &str, value: &str) -> Result<(), String> {
        if !COLOR_KEYS.contains(&key) {
            return Err(format!("不支持外观颜色 {key}"));
        }
        let value = value.trim();
        if value.is_empty() {
            self.colors.remove(key);
            return Ok(());
        }
        if value.len() != 7
            || !value.starts_with('#')
            || !value.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit)
        {
            return Err("请使用 #RRGGBB 格式，例如 #52756B；留空则跟随主题".into());
        }
        self.colors.insert(key.into(), value.to_ascii_uppercase());
        Ok(())
    }

    pub fn reset_colors(&mut self) {
        self.colors.clear();
    }

    pub fn apply(&self, theme: &ThemeManifest) -> ThemeManifest {
        let mut effective = theme.clone();
        for (key, value) in &self.colors {
            effective.normalized["colors"][key] = json!(value);
            // Explicit personal palette choices take precedence over authored
            // role colors. Removing the temporary literal restores the role's
            // semantic, readability-corrected palette fallback; the source
            // theme stays intact and reappears on reset.
            let roles: &[&str] = match key.as_str() {
                "ink" => &["body", "name", "itemTitle", "summary"],
                "muted" => &["meta", "sideDate"],
                "accent" => &["label", "sectionTitle"],
                "paper" => &[
                    "body",
                    "name",
                    "itemTitle",
                    "summary",
                    "meta",
                    "sideDate",
                    "label",
                    "sectionTitle",
                ],
                _ => &[],
            };
            for role in roles {
                if let Some(properties) = effective
                    .normalized
                    .get_mut("typography")
                    .and_then(|typography| typography.get_mut(*role))
                    .and_then(Value::as_object_mut)
                {
                    properties.remove("color");
                }
            }
            if let Some(sections) = effective
                .normalized
                .get_mut("sections")
                .and_then(Value::as_object_mut)
            {
                for section in sections.values_mut() {
                    for line in section
                        .get_mut("entryHeader")
                        .and_then(Value::as_array_mut)
                        .into_iter()
                        .flatten()
                    {
                        if roles.contains(&line["role"].as_str().unwrap_or("itemTitle"))
                            && let Some(properties) =
                                line.get_mut("style").and_then(Value::as_object_mut)
                        {
                            properties.remove("color");
                        }
                    }
                }
            }
        }
        for (font, roles) in [
            (self.body_font.as_ref(), &["body", "meta"][..]),
            (self.heading_font.as_ref(), &["name", "sectionTitle"][..]),
        ] {
            let Some(font) = font else { continue };
            if !effective.normalized["typography"].is_object() {
                effective.normalized["typography"] = json!({});
            }
            for role in roles {
                if !effective.normalized["typography"][role].is_object() {
                    effective.normalized["typography"][role] = json!({});
                }
                effective.normalized["typography"][role]["family"] = json!(font.family());
                // The picker selects a family, not an italic face. An explicit
                // personal family override uses upright faces consistently,
                // including CJK/system families with no real italic. Resetting
                // the preference restores the theme's original slant.
                effective.normalized["typography"][role]["fontStyle"] = json!("normal");
                let weight = if matches!(font, FontChoice::Font(_)) {
                    effective.normalized["typography"][role]["weight"]
                        .as_u64()
                        .unwrap_or(if matches!(*role, "name" | "sectionTitle") {
                            600
                        } else {
                            400
                        }) as u16
                } else {
                    font.weight()
                };
                effective.normalized["typography"][role]["weight"] = json!(weight);
            }
            // Explicit dependent roles follow body-font overrides. Keep absent
            // roles absent so they still inherit the effective body/meta style.
            if roles.contains(&"body") {
                for role in ["itemTitle", "label", "summary", "sideDate"] {
                    if !effective.normalized["typography"][role].is_object() {
                        continue;
                    }
                    effective.normalized["typography"][role]["family"] = json!(font.family());
                    effective.normalized["typography"][role]["fontStyle"] = json!("normal");
                    if !matches!(font, FontChoice::Font(_)) {
                        effective.normalized["typography"][role]["weight"] = json!(font.weight());
                    }
                }
                if let Some(sections) = effective
                    .normalized
                    .get_mut("sections")
                    .and_then(Value::as_object_mut)
                {
                    for section in sections.values_mut() {
                        for line in section
                            .get_mut("entryHeader")
                            .and_then(Value::as_array_mut)
                            .into_iter()
                            .flatten()
                        {
                            if let Some(properties) =
                                line.get_mut("style").and_then(Value::as_object_mut)
                            {
                                properties.insert("family".into(), json!(font.family()));
                                properties.insert("fontStyle".into(), json!("normal"));
                                // Missing weight must continue to inherit the
                                // selected role; a color-only row is not 400.
                                if !matches!(font, FontChoice::Font(_)) {
                                    properties.insert("weight".into(), json!(font.weight()));
                                }
                            }
                        }
                    }
                }
            }
        }
        effective
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn personal_palette_overrides_exact_role_colors_without_mutating_the_author_theme() {
        let mut theme = crate::parse_theme(crate::LEGACY_THEME_TEST_BYTES).unwrap();
        let groups: &[(&str, &[&str])] = &[
            ("ink", &["body", "name", "itemTitle", "summary"]),
            ("muted", &["meta", "sideDate"]),
            ("accent", &["label", "sectionTitle"]),
        ];
        for (_, roles) in groups {
            for role in *roles {
                theme.normalized["typography"][role]["color"] = json!("#123456");
            }
        }
        let original = theme.normalized.clone();
        for key in [
            "ink", "muted", "accent", "paper", "canvas", "border", "seal",
        ] {
            let mut prefs = Appearance::default();
            prefs.set_color(key, "#E0E0E0").unwrap();
            let effective = prefs.apply(&theme);
            crate::layout::check_theme(&effective).unwrap();
            for (semantic, roles) in groups {
                for role in *roles {
                    assert_eq!(
                        effective.normalized["typography"][role]
                            .get("color")
                            .is_none(),
                        key == *semantic || key == "paper",
                        "{key}/{role}"
                    );
                }
            }
            assert_eq!(theme.normalized, original);
            prefs.reset_colors();
            assert_eq!(prefs.apply(&theme).normalized, original);
        }
    }

    #[test]
    fn palette_override_does_not_insert_missing_typography_or_null_roles() {
        let mut theme = crate::parse_theme(crate::LEGACY_THEME_TEST_BYTES).unwrap();
        theme
            .normalized
            .as_object_mut()
            .unwrap()
            .remove("typography");
        for key in ["ink", "muted", "accent", "paper"] {
            let mut prefs = Appearance::default();
            prefs.set_color(key, "#224466").unwrap();
            let effective = prefs.apply(&theme);
            assert!(effective.normalized.get("typography").is_none());
            crate::layout::check_theme(&effective).unwrap();
        }
    }

    #[test]
    fn explicit_family_override_uses_real_upright_faces_and_reset_restores_theme_italic() {
        let mut theme = crate::parse_theme(crate::LEGACY_THEME_TEST_BYTES).unwrap();
        let id = crate::fonts::register_asset(
            include_bytes!("../tests/fixtures/theme-italic-font.ttf").to_vec(),
        )
        .unwrap();
        theme.asset_fonts.insert("italic-fixture".into(), id);
        for role in [
            "body",
            "meta",
            "name",
            "sectionTitle",
            "itemTitle",
            "label",
            "summary",
            "sideDate",
        ] {
            theme.normalized["typography"][role] = json!({"family":"asset:italic-fixture","weight":400,"fontStyle":"italic","sizePt":12});
        }
        let original = theme.normalized.clone();
        crate::layout::check_theme(&theme).unwrap();
        for choice in [FontChoice::Sans, FontChoice::Serif] {
            let appearance = Appearance {
                body_font: Some(choice.clone()),
                heading_font: Some(choice),
                ..Default::default()
            };
            let effective = appearance.apply(&theme);
            crate::layout::check_theme(&effective).unwrap();
            for role in [
                "body",
                "meta",
                "name",
                "sectionTitle",
                "itemTitle",
                "label",
                "summary",
                "sideDate",
            ] {
                assert_eq!(
                    effective.normalized["typography"][role]["fontStyle"],
                    "normal"
                );
            }
            assert_eq!(theme.normalized, original);
        }
        assert_eq!(Appearance::default().apply(&theme).normalized, original);
    }

    #[test]
    fn body_override_updates_explicit_item_title_without_persisting_it() {
        let mut theme = crate::parse_theme(crate::LEGACY_THEME_TEST_BYTES).unwrap();
        theme.normalized["typography"]["itemTitle"] =
            json!({"family":"cjk-serif","weight":600,"sizePt":13});
        let before = theme.normalized.clone();
        let prefs = Appearance {
            body_font: Some(FontChoice::Sans),
            ..Default::default()
        };
        let updated = prefs.apply(&theme);
        assert_eq!(
            updated.normalized["typography"]["itemTitle"]["family"],
            "cjk-sans"
        );
        assert_eq!(updated.normalized["typography"]["itemTitle"]["sizePt"], 13);
        crate::layout::check_theme(&updated).unwrap();
        assert_eq!(theme.normalized, before);
    }
    use crate::{layout, parse_theme, resume::parse_resume, semantic::project};

    #[test]
    fn overrides_are_independent_and_never_change_the_theme_or_resume() {
        let theme =
            parse_theme(include_bytes!("../../../assets/themes/danzhu.theme.json")).unwrap();
        let original = theme.normalized.clone();
        let resume = parse_resume(br#"{"basics":{"name":"Appearance"}}"#).unwrap();
        let source = resume.source.clone();
        let mut prefs = Appearance::default();
        prefs.set_color("accent", "#244b70").unwrap();
        prefs.body_font = Some(FontChoice::Serif);
        let effective = prefs.apply(&theme);
        assert_eq!(effective.normalized["colors"]["accent"], "#244B70");
        assert_eq!(
            effective.normalized["typography"]["body"]["family"],
            "cjk-serif"
        );
        assert_eq!(effective.normalized["typography"]["body"]["weight"], 600);
        assert_eq!(
            effective.normalized["typography"]["name"],
            original["typography"]["name"]
        );
        assert_eq!(theme.normalized, original);
        assert_eq!(resume.source, source);
        assert!(
            layout::layout_with_theme(&project(&resume), crate::render::Language::Zh, &effective)
                .is_ok()
        );
        assert_eq!(Appearance::from_json(&prefs.as_json()).unwrap(), prefs);
        prefs.body_font = None;
        assert_eq!(
            prefs.apply(&theme).normalized["typography"],
            original["typography"]
        );
        prefs.reset_colors();
        assert_eq!(prefs.apply(&theme).normalized, original);
    }

    #[test]
    fn rejects_unknown_settings_fonts_and_malformed_colors_without_losing_previous_values() {
        let mut prefs = Appearance::default();
        prefs.set_color("paper", "#172426").unwrap();
        let before = prefs.clone();
        for value in ["#fff", "#１２３４５６", "red", "#12345Z", "url(x)"] {
            assert!(prefs.set_color("paper", value).is_err());
            assert_eq!(prefs, before);
        }
        assert!(prefs.set_color("script", "#123456").is_err());
        for value in [
            json!({"formatVersion": 2}),
            json!({"formatVersion": 1, "bodyFont": "arbitrary-system-font"}),
            json!({"formatVersion": 1, "script": "x"}),
            json!({"formatVersion": 1, "colors": []}),
        ] {
            assert!(Appearance::from_json(&value).is_err());
        }
    }
    #[test]
    fn body_font_override_keeps_explicit_dependent_role_geometry_and_missing_roles_inherited() {
        let mut theme = crate::parse_theme(crate::LEGACY_THEME_TEST_BYTES).unwrap();
        for role in ["label", "summary", "sideDate"] {
            theme.normalized["typography"][role] = json!({"family":"cjk-serif","weight":600,"sizePt":12,"trackingEm":0.1,"textTransform":"uppercase"});
        }
        let source = theme.normalized.clone();
        let appearance =
            Appearance::from_json(&json!({"formatVersion":1,"bodyFont":"cjk-sans"})).unwrap();
        let effective = appearance.apply(&theme);
        for role in ["label", "summary", "sideDate"] {
            assert_eq!(
                effective.normalized["typography"][role]["family"],
                "cjk-sans"
            );
            assert_eq!(effective.normalized["typography"][role]["weight"], 400);
            assert_eq!(effective.normalized["typography"][role]["sizePt"], 12);
            assert_eq!(effective.normalized["typography"][role]["trackingEm"], 0.1);
            assert_eq!(
                effective.normalized["typography"][role]["textTransform"],
                "uppercase"
            );
        }
        assert_eq!(theme.normalized, source);
        let empty = crate::parse_theme(crate::LEGACY_THEME_TEST_BYTES).unwrap();
        let inherited = appearance.apply(&empty);
        for role in ["label", "summary", "sideDate"] {
            assert!(inherited.normalized["typography"][role].is_null());
        }
    }
}
