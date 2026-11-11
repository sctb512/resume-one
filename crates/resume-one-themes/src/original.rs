//! Locally execute a bundled upstream JSON Resume renderer with no host bindings.
use std::{
    fs::File,
    io::Read,
    path::Path,
    sync::OnceLock,
    time::{Duration, Instant},
};

use jsonschema::Validator;
use rquickjs::{Context, Runtime};
use serde_json::{Value, json};

use crate::{MAX_FILE_BYTES, ThemeError};

pub const ORIGINAL_THEME_SUFFIX: &str = ".original.theme.json";
pub(crate) const MAX_HTML_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug)]
pub struct OriginalTheme {
    pub id: String,
    pub name: String,
    pub package: String,
    pub version: String,
    pub license: String,
    pub author: String,
    pub upstream: String,
    script: String,
    pub render_revision: String,
    pub(crate) stylesheets: Value,
}

pub(crate) struct RenderedHtml {
    pub html: String,
    pub print: Value,
}

impl OriginalTheme {
    pub(crate) fn parse(value: &Value) -> Result<Self, ThemeError> {
        static VALIDATOR: OnceLock<Validator> = OnceLock::new();
        let validator = VALIDATOR.get_or_init(|| {
            let schema = serde_json::from_str(include_str!(
                "../../../spec/json-resume-original-theme-v1.schema.json"
            ))
            .expect("original theme schema JSON");
            jsonschema::draft7::new(&schema).expect("valid original theme schema")
        });
        if !validator.is_valid(value) {
            return Err(ThemeError::new(
                "ORIGINAL_THEME_SCHEMA",
                "原版主题包不符合格式规范",
            ));
        }
        let string = |key: &str| value[key].as_str().expect("validated string").to_owned();
        let script = string("script");
        Ok(Self {
            id: string("id"),
            name: string("name"),
            package: string("package"),
            version: string("version"),
            license: string("license"),
            author: string("author"),
            upstream: string("upstream"),
            render_revision: crate::sha256(value.to_string().as_bytes()),
            stylesheets: value["stylesheets"].clone(),
            script,
        })
    }

    pub fn load(path: &Path) -> Result<Self, ThemeError> {
        let io_error =
            |error: std::io::Error| ThemeError::new("ORIGINAL_THEME_IO", error.to_string());
        let mut bytes = Vec::new();
        File::open(path)
            .map_err(io_error)?
            .take(MAX_FILE_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(io_error)?;
        if bytes.len() > MAX_FILE_BYTES {
            return Err(ThemeError::new("THEME_TOO_LARGE", "原版主题包超过 1 MiB"));
        }
        let value = serde_json::from_slice(&bytes)
            .map_err(|_| ThemeError::new("ORIGINAL_THEME_SCHEMA", "原版主题包不是有效的 JSON"))?;
        Self::parse(&value)
    }

    pub(crate) fn render_html(&self, resume: &Value) -> Result<RenderedHtml, ThemeError> {
        let js_error = |_| {
            ThemeError::new(
                "ORIGINAL_THEME_RENDER",
                "原版主题未返回 HTML，或已达到运行资源上限",
            )
        };
        let runtime = Runtime::new().map_err(js_error)?;
        runtime.set_memory_limit(64 * 1024 * 1024);
        runtime.set_max_stack_size(512 * 1024);
        let deadline = Instant::now() + Duration::from_secs(3);
        runtime.set_interrupt_handler(Some(Box::new(move || Instant::now() >= deadline)));
        let context = Context::full(&runtime).map_err(js_error)?;
        // Supply optional empty fields only on the renderer's private copy. Older
        // upstream themes access these directly, including on an empty resume.
        let mut input = resume.clone();
        if input.get("basics").is_none() {
            input["basics"] = json!({});
        }
        let basics = input["basics"]
            .as_object_mut()
            .ok_or_else(|| ThemeError::new("ORIGINAL_THEME_INPUT", "basics 须为对象"))?;
        basics.entry("location").or_insert_with(|| json!({}));
        basics.entry("profiles").or_insert_with(|| json!([]));
        alias(basics, "website", "url");
        alias(basics, "picture", "image");
        // Some pinned upstream templates predate the current JSON Resume
        // keys. Fill their historical names only on the isolated render copy.
        for section in [
            "work",
            "volunteer",
            "education",
            "projects",
            "publications",
            "certificates",
        ] {
            for item in input
                .get_mut(section)
                .and_then(Value::as_array_mut)
                .into_iter()
                .flatten()
            {
                let Some(item) = item.as_object_mut() else {
                    continue;
                };
                alias(item, "website", "url");
                match section {
                    "work" => alias(item, "company", "name"),
                    "education" => alias(item, "gpa", "score"),
                    _ => {}
                }
            }
        }

        context.with(|ctx| {
            let input = ctx.json_parse(input.to_string()).map_err(js_error)?;
            ctx.globals()
                .set("__resumeOneInput", input)
                .map_err(js_error)?;
            ctx.eval::<(), _>(self.script.as_str()).map_err(js_error)?;
            let html: String = ctx
                .eval("__resumeOneTheme.render(__resumeOneInput)")
                .map_err(js_error)?;
            if html.trim().is_empty() || html.len() > MAX_HTML_BYTES {
                return Err(ThemeError::new(
                    "ORIGINAL_THEME_HTML",
                    "主题生成的 HTML 为空或超过 4 MiB",
                ));
            }
            let options: String = ctx
                .eval("JSON.stringify(__resumeOneTheme.pdfRenderOptions || {})")
                .map_err(js_error)?;
            let print = serde_json::from_str(&options).unwrap_or(Value::Null);
            Ok(RenderedHtml { html, print })
        })
    }

    /// Prepare a static document for the desktop's live browser surface.
    pub fn render_preview_html(&self, resume: &Value) -> Result<String, ThemeError> {
        self.render_localized_preview_html(resume, resume_one_core::render::Language::En)
    }

    pub fn render_localized_preview_html(
        &self,
        resume: &Value,
        language: resume_one_core::render::Language,
    ) -> Result<String, ThemeError> {
        let rendered = self.render_html(resume)?;
        let html = crate::preview_html::localize_headings(
            &rendered.html,
            resume,
            language,
            MAX_HTML_BYTES,
        )?;
        crate::preview_html::prepare(&html, &self.stylesheets, &self.id, MAX_HTML_BYTES)
    }
}

fn alias(object: &mut serde_json::Map<String, Value>, legacy: &str, current: &str) {
    if !object.contains_key(legacy)
        && let Some(value) = object.get(current).cloned()
    {
        object.insert(legacy.into(), value);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn fixture(script: &str) -> Value {
        json!({
            "kind":"json-resume-original", "formatVersion":1, "id":"test", "name":"Test",
            "package":"jsonresume-theme-test", "version":"1.0.0", "license":"MIT", "author":"Example",
            "upstream":"https://www.npmjs.com/package/jsonresume-theme-test/v/1.0.0",
            "script":script
        })
    }

    #[test]
    fn no_host_capabilities_and_uses_a_fresh_resume_on_each_render() {
        let theme = OriginalTheme::parse(&fixture(r#"globalThis.__resumeOneTheme = {
            render(resume) {
                if ([typeof process, typeof require, typeof fetch, typeof window].some(t => t !== "undefined")) throw new Error();
                const name = resume.basics.name;
                resume.basics.name = "changed";
                return `<h1>${name}</h1>`;
            }
        }"#)).unwrap();
        let resume = json!({"basics":{"name":"谢明"}});
        for _ in 0..2 {
            assert_eq!(theme.render_html(&resume).unwrap().html, "<h1>谢明</h1>");
        }
        assert_eq!(resume["basics"]["name"], "谢明");
    }

    #[test]
    fn interrupts_infinite_upstream_renderer() {
        let theme = OriginalTheme::parse(&fixture(
            "globalThis.__resumeOneTheme = { render() { for (;;) {} } };",
        ))
        .unwrap();
        assert_eq!(
            theme.render_html(&json!({})).err().unwrap().code,
            "ORIGINAL_THEME_RENDER"
        );
    }

    #[test]
    fn old_template_keys_use_current_resume_fields_without_rewriting_the_resume() {
        let theme = OriginalTheme::parse(&fixture(
            r#"globalThis.__resumeOneTheme = {
          render(r) { return r.work.map(w => w.company).join("|") + "|" + r.basics.website; }
        }"#,
        ))
        .unwrap();
        let data = json!({"basics":{"url":"https://example.test"},"work":[{"name":"标准字段"},{"name":"标准","company":"旧字段"}]});
        assert_eq!(
            theme.render_html(&data).unwrap().html,
            "标准字段|旧字段|https://example.test"
        );
        assert!(data["work"][0].get("company").is_none());
    }
}
