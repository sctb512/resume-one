//! Keep the upstream DOM and CSS while stripping elements that can execute or navigate.
use std::rc::Rc;

use html5ever::{
    ParseOpts, QualName, local_name, namespace_url, ns, parse_document, tendril::TendrilSink,
};
use markup5ever_rcdom::{Handle, NodeData, RcDom, SerializableHandle};
use resume_one_core::render::{Language, title};
use serde_json::Value;

use crate::ThemeError;

const POLICY: &str = "default-src 'none'; style-src 'unsafe-inline'; img-src data:; font-src data:; base-uri 'none'; form-action 'none'";

const PAPER_STYLE: &str = include_str!("original-paper.css");

/// Translate recognized upstream section labels while preserving the HTML
/// structure and every literal field value from the user's résumé.
pub(crate) fn localize_headings(
    html: &str,
    resume: &Value,
    language: Language,
    limit: usize,
) -> Result<String, ThemeError> {
    if language == Language::En {
        return Ok(html.to_owned());
    }
    let mut source_text = std::collections::HashSet::new();
    let mut fields = vec![resume];
    while let Some(field) = fields.pop() {
        match field {
            Value::String(text) => {
                source_text.insert(text.trim());
            }
            Value::Object(object) => fields.extend(object.values()),
            Value::Array(array) => fields.extend(array),
            _ => {}
        }
    }
    let dom = parse_document(RcDom::default(), ParseOpts::default()).one(html);
    let mut pending = vec![dom.document.clone()];
    while let Some(node) = pending.pop() {
        let children = node.children.borrow();
        pending.extend(children.iter().cloned());
        let NodeData::Element { name, attrs, .. } = &node.data else {
            continue;
        };
        if name.local == local_name!("html") {
            let mut attrs = attrs.borrow_mut();
            attrs.retain(|attr| attr.name.local != local_name!("lang"));
            attrs.push(html5ever::Attribute {
                name: QualName::new(None, ns!(), local_name!("lang")),
                value: "zh-CN".into(),
            });
        }
        let heading = matches!(name.local.as_ref(), "h2" | "h3" | "h4")
            || attrs
                .borrow()
                .iter()
                .any(|attr| match attr.name.local.as_ref() {
                    "role" => attr.value.as_ref() == "heading",
                    "class" => attr.value.split_ascii_whitespace().any(|class| {
                        matches!(
                            class,
                            "section-title" | "section-name" | "section-heading" | "section-header"
                        )
                    }),
                    _ => false,
                });
        if !heading {
            continue;
        }
        for child in children.iter() {
            let NodeData::Text { contents } = &child.data else {
                continue;
            };
            let mut text = contents.borrow_mut();
            let label = text.trim();
            if source_text.contains(label) {
                continue;
            }
            if let Some(translation) = chinese_heading(label) {
                *text = translation.into();
            }
        }
    }
    let mut bytes = Vec::new();
    html5ever::serialize(
        &mut bytes,
        &SerializableHandle::from(dom.document.clone()),
        Default::default(),
    )
    .expect("serialize localized HTML to memory");
    if bytes.len() > limit {
        return Err(too_large());
    }
    Ok(String::from_utf8(bytes).expect("UTF-8 HTML serializer"))
}

fn chinese_heading(label: &str) -> Option<&'static str> {
    let id = match label.trim_end_matches(':').trim().to_lowercase().as_str() {
        "summary" | "professional summary" | "about" | "about me" | "profile" => {
            return Some("简介");
        }
        "contact" | "contact details" | "contact information" => return Some("联系方式"),
        "work"
        | "experience"
        | "work experience"
        | "professional experience"
        | "employment"
        | "employment history"
        | "experiences professionnelles" => "work",
        "education" | "academic background" | "formation" => "education",
        "projects"
        | "projects experience"
        | "project experience"
        | "notable projects"
        | "research projects"
        | "open source projects"
        | "projets" => "projects",
        "skills" | "technical skills" | "competences" | "compétences" => "skills",
        "volunteer" | "volunteering" | "volunteer experience" | "volunteer work" | "bénévolat" => {
            "volunteer"
        }
        "awards" | "awards & honors" | "awards and honors" | "honors" | "honours" => "awards",
        "certificates" | "certifications" => "certificates",
        "publications" => "publications",
        "languages" | "langues" => "languages",
        "interests" | "research interests" => "interests",
        "references" => "references",
        _ => return None,
    };
    Some(title(id, Language::Zh))
}

pub(crate) fn prepare(
    html: &str,
    stylesheets: &Value,
    theme_id: &str,
    limit: usize,
) -> Result<String, ThemeError> {
    let mut options = ParseOpts::default();
    options.tree_builder.scripting_enabled = false;
    let dom = parse_document(RcDom::default(), options).one(html);
    let mut pending = vec![dom.document.clone()];
    let mut css_bytes = 0_usize;
    while let Some(parent) = pending.pop() {
        parent.children.borrow_mut().retain_mut(|child| {
            if let NodeData::Element { name, attrs, .. } = &child.data {
                let mut attrs = attrs.borrow_mut();
                let attr = |key: &str| {
                    attrs
                        .iter()
                        .find(|attr| attr.name.local.as_ref() == key)
                        .map(|attr| attr.value.as_ref())
                };
                match name.local.as_ref() {
                    "link" => {
                        let css = attr("rel")
                            .filter(|rel| {
                                rel.split_ascii_whitespace()
                                    .any(|part| part.eq_ignore_ascii_case("stylesheet"))
                            })
                            .and_then(|_| attr("href"))
                            .and_then(|href| stylesheets.get(href))
                            .and_then(Value::as_str);
                        if let Some(css) = css {
                            css_bytes = css_bytes.saturating_add(css.len().saturating_mul(4));
                            if css_bytes > limit {
                                return false;
                            }
                            let style = embedded_style(css, attr("media"));
                            drop(attrs);
                            *child = style;
                            return true;
                        }
                        return false;
                    }
                    "img" if !attr("src").is_some_and(is_image_data) => return false,
                    "script" | "iframe" | "frame" | "frameset" | "object" | "embed" | "portal"
                    | "base" | "meta" | "form" | "input" | "button" | "textarea" | "select"
                    | "audio" | "video" | "source" | "track" | "template" | "animate"
                    | "animateMotion" | "animateTransform" | "set" | "discard" => return false,
                    _ => {}
                }
                attrs.retain(|attr| {
                    let key = attr.name.local.as_ref();
                    let value = attr.value.trim();
                    match key {
                        "src" => name.local.as_ref() == "img" && is_image_data(value),
                        "href" => {
                            value.starts_with('#')
                                || (name.local.as_ref() == "image" && is_image_data(value))
                        }
                        "srcset" | "srcdoc" | "action" | "formaction" | "target" | "ping"
                        | "poster" | "background" | "autofocus" | "contenteditable" => false,
                        _ => !key.starts_with("on") && !key.starts_with("data-jianli-"),
                    }
                });
            }
            pending.push(child.clone());
            true
        });
        if css_bytes > limit {
            return Err(too_large());
        }
    }

    // Put the policy ahead of the theme's DOM. The iframe also runs in a
    // sandbox with scripts and top-level navigation disabled.
    let document = dom.document.children.borrow();
    let root = document
        .iter()
        .find(|node| matches!(node.data, NodeData::Element { .. }))
        .expect("HTML parser creates a root");
    let head = root.children.borrow().iter().find(|node| {
        matches!(&node.data, NodeData::Element { name, .. } if name.local == local_name!("head"))
    }).expect("HTML parser creates a head").clone();
    if let NodeData::Element { attrs, .. } = &root.data {
        attrs.borrow_mut().push(html5ever::Attribute {
            name: QualName::new(None, ns!(), "data-jianli-theme".into()),
            value: theme_id.into(),
        });
    }
    let policy = markup5ever_rcdom::Node::new(NodeData::Element {
        name: QualName::new(None, ns!(html), local_name!("meta")),
        attrs: std::cell::RefCell::new(vec![
            html5ever::Attribute {
                name: QualName::new(None, ns!(), local_name!("http-equiv")),
                value: "Content-Security-Policy".into(),
            },
            html5ever::Attribute {
                name: QualName::new(None, ns!(), local_name!("content")),
                value: POLICY.into(),
            },
        ]),
        template_contents: Default::default(),
        mathml_annotation_xml_integration_point: false,
    });
    policy.parent.set(Some(Rc::downgrade(&head)));
    head.children.borrow_mut().insert(0, policy);
    let paper_style = embedded_style(PAPER_STYLE, Some("screen"));
    paper_style.parent.set(Some(Rc::downgrade(&head)));
    head.children.borrow_mut().push(paper_style);
    let mut bytes = b"<!doctype html>".to_vec();
    html5ever::serialize(
        &mut bytes,
        &SerializableHandle::from(root.clone()),
        html5ever::serialize::SerializeOpts {
            traversal_scope: html5ever::serialize::TraversalScope::IncludeNode,
            ..Default::default()
        },
    )
    .expect("serialize HTML to memory");
    if bytes.len() > limit {
        return Err(too_large());
    }
    Ok(String::from_utf8(bytes).expect("UTF-8 HTML serializer"))
}

fn too_large() -> ThemeError {
    ThemeError::new("ORIGINAL_THEME_HTML", "原版主题的预览 HTML 超过 4 MiB")
}

fn is_image_data(src: &str) -> bool {
    src.trim()
        .get(..11)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("data:image/"))
}

fn embedded_style(css: &str, media: Option<&str>) -> Handle {
    let attrs = media.map(|media| html5ever::Attribute {
        name: QualName::new(None, ns!(), local_name!("media")),
        value: media.into(),
    });
    let node = markup5ever_rcdom::Node::new(NodeData::Element {
        name: QualName::new(None, ns!(html), local_name!("style")),
        attrs: std::cell::RefCell::new(attrs.into_iter().collect()),
        template_contents: Default::default(),
        mathml_annotation_xml_integration_point: false,
    });
    // The HTML serializer treats style text as raw markup. Escape every '<'
    // as a CSS escape so supplied CSS cannot close the element.
    let text = markup5ever_rcdom::Node::new(NodeData::Text {
        contents: std::cell::RefCell::new(css.replace('<', "\\3c ").into()),
    });
    text.parent.set(Some(Rc::downgrade(&node)));
    node.children.borrow_mut().push(text);
    node
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn original_heading_localization_preserves_values_and_markup() {
        let html = r#"<html lang="en"><body><h1>Alex Morgan</h1><h2><svg></svg> Experience</h2><h3>Skills</h3><div class="section-title">Education</div></body></html>"#;
        let data = json!({"work":[{"name":"Skills"}]});
        assert_eq!(
            localize_headings(html, &data, Language::En, 16384).unwrap(),
            html
        );
        let zh = localize_headings(html, &data, Language::Zh, 16384).unwrap();
        assert!(zh.contains(r#"<html lang="zh-CN">"#));
        assert!(zh.contains("<h1>Alex Morgan</h1>"));
        assert!(zh.contains("<h2><svg></svg>工作经历</h2>"));
        assert!(zh.contains("<h3>Skills</h3>"));
        assert!(zh.contains(r#"<div class="section-title">教育经历</div>"#));
        assert_eq!(data["work"][0]["name"], "Skills");
    }

    #[test]
    fn keeps_document_layout_and_blocks_active_or_external_markup() {
        let html = r##"<html lang="zh"><head><link rel="stylesheet" href="https://theme.test/local.css" media="print"><meta http-equiv="refresh" content="0;URL=https://outside.test"></head><body class="resume"><section id="work"><h1>林清和</h1><img src="https://outside.test/avatar" onerror="leak()"><img src="data:image/png;base64,abc" srcset="https://outside.test/2x"><script>leak()</script><a href="https://outside.test">网址</a><svg><use href="#shape"/><animate attributeName="href"/></svg></section></body></html>"##;
        let doc = prepare(
            html,
            &json!({"https://theme.test/local.css": ".resume{color:red} </style><script>leak()</script>"}),
            "original-test",
            16384,
        ).unwrap();
        for expected in [
            "Content-Security-Policy",
            "<html lang=\"zh\" data-jianli-theme=\"original-test\">",
            "<body class=\"resume\">",
            "<h1>林清和</h1>",
            "<style media=\"print\">",
            "data:image/png;base64,abc",
            "<a>网址</a>",
            "href=\"#shape\"",
        ] {
            assert!(doc.contains(expected), "missing {expected} in {doc}");
        }
        for blocked in ["outside.test", "<script", "onerror=", "<animate", "<link"] {
            assert!(!doc.contains(blocked), "{blocked} in {doc}");
        }
    }
}
