//! Deterministic UTF-8 text and Markdown serialization of semantic resume content.

use crate::semantic::{Item, ResumeContent};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Zh,
    En,
}

impl Language {
    pub fn tag(self) -> &'static str {
        match self {
            Self::Zh => "zh-CN",
            Self::En => "en",
        }
    }
}

pub fn title(id: &str, language: Language) -> &'static str {
    match (id, language) {
        ("basics", Language::Zh) => "基本信息",
        ("basics", Language::En) => "Basics",
        ("work", Language::Zh) => "工作经历",
        ("volunteer", Language::Zh) => "志愿经历",
        ("education", Language::Zh) => "教育经历",
        ("awards", Language::Zh) => "荣誉奖项",
        ("certificates", Language::Zh) => "证书",
        ("publications", Language::Zh) => "出版物",
        ("skills", Language::Zh) => "技能",
        ("languages", Language::Zh) => "语言",
        ("interests", Language::Zh) => "兴趣",
        ("references", Language::Zh) => "推荐人",
        ("projects", Language::Zh) => "项目经历",
        ("work", Language::En) => "Work",
        ("volunteer", Language::En) => "Volunteer",
        ("education", Language::En) => "Education",
        ("awards", Language::En) => "Awards",
        ("certificates", Language::En) => "Certificates",
        ("publications", Language::En) => "Publications",
        ("skills", Language::En) => "Skills",
        ("languages", Language::En) => "Languages",
        ("interests", Language::En) => "Interests",
        ("references", Language::En) => "References",
        ("projects", Language::En) => "Projects",
        _ => "Other",
    }
}

fn push_line(out: &mut String, text: &str) {
    out.push_str(text);
    out.push('\n');
}

pub fn plain_text(content: &ResumeContent, language: Language) -> String {
    let mut out = String::new();
    for heading in [&content.name, &content.label] {
        if !heading.is_empty() {
            push_line(&mut out, heading);
        }
    }
    for contact in &content.contacts {
        push_line(&mut out, contact);
    }
    if !content.summary.is_empty() {
        out.push('\n');
        push_line(&mut out, &content.summary);
    }
    for section in &content.sections {
        out.push('\n');
        push_line(&mut out, title(section.id, language));
        for item in &section.items {
            out.push('\n');
            plain_item(&mut out, item);
        }
    }
    out.trim_end().to_owned() + "\n"
}

fn plain_item(out: &mut String, item: &Item) {
    if !item.heading.is_empty() {
        push_line(out, &item.heading);
    }
    for detail in &item.details {
        push_line(out, detail);
    }
    for paragraph in &item.paragraphs {
        push_line(out, paragraph);
    }
    for bullet in &item.bullets {
        push_line(out, &format!("• {bullet}"));
    }
    if let Some(url) = &item.url {
        push_line(out, url);
    }
}

pub fn markdown(content: &ResumeContent, language: Language) -> String {
    let mut out = String::new();
    if !content.name.is_empty() {
        push_line(&mut out, &format!("# {}", escape_md(&content.name)));
    }
    if !content.label.is_empty() {
        push_line(&mut out, &escape_md(&content.label));
    }
    if !content.contacts.is_empty() {
        out.push('\n');
        push_line(
            &mut out,
            &content
                .contacts
                .iter()
                .map(|contact| escape_md(contact))
                .collect::<Vec<_>>()
                .join(" · "),
        );
    }
    if !content.summary.is_empty() {
        out.push('\n');
        push_line(&mut out, &escape_md(&content.summary));
    }
    for section in &content.sections {
        out.push('\n');
        push_line(&mut out, &format!("## {}", title(section.id, language)));
        for item in &section.items {
            out.push('\n');
            markdown_item(&mut out, item, language);
        }
    }
    out.trim_end().to_owned() + "\n"
}

fn markdown_item(out: &mut String, item: &Item, language: Language) {
    if !item.heading.is_empty() {
        push_line(out, &format!("### {}", escape_md(&item.heading)));
    }
    if !item.details.is_empty() {
        push_line(
            out,
            &item
                .details
                .iter()
                .map(|detail| escape_md(detail))
                .collect::<Vec<_>>()
                .join(" · "),
        );
    }
    for paragraph in &item.paragraphs {
        out.push('\n');
        push_line(out, &escape_md(paragraph));
    }
    if !item.bullets.is_empty() {
        out.push('\n');
        for bullet in &item.bullets {
            push_line(out, &format!("- {}", escape_md(bullet)));
        }
    }
    if let Some(url) = &item.url {
        out.push('\n');
        let link_label = if language == Language::Zh {
            "链接"
        } else {
            "Link"
        };
        if let Some(safe) = safe_link(url) {
            push_line(out, &format!("[{link_label}]({safe})"));
        } else {
            push_line(out, &escape_md(url));
        }
    }
}

fn escape_md(source: &str) -> String {
    let mut out = String::new();
    for character in source.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\n' => out.push_str("  \n"),
            '\\' | '`' | '*' | '_' | '{' | '}' | '[' | ']' | '(' | ')' | '#' | '+' | '-' | '!'
            | '|' => {
                out.push('\\');
                out.push(character);
            }
            _ => out.push(character),
        }
    }
    out
}

fn safe_link(source: &str) -> Option<String> {
    if !(source.starts_with("https://") || source.starts_with("http://"))
        || source
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, '<' | '>' | '"' | '\''))
    {
        return None;
    }
    Some(source.replace('(', "%28").replace(')', "%29"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{resume::parse_resume, semantic::project};

    #[test]
    fn both_formats_include_standard_content_and_not_extensions() {
        let source = br#"{"basics":{"name":"Ada","summary":"Designer"},"work":[{"position":"Lead","name":"Org","highlights":["Shipped"]}],"custom":{"secret":"HIDDEN"}}"#;
        let document = parse_resume(source).unwrap();
        let content = project(&document);
        let plain = plain_text(&content, Language::Zh);
        let md = markdown(&content, Language::En);
        assert!(plain.contains("工作经历\n\nLead · Org\n• Shipped"));
        assert!(md.contains("## Work\n\n### Lead · Org"));
        assert!(!plain.contains("HIDDEN"));
        assert!(!md.contains("HIDDEN"));
        assert_eq!(document.source, source);
    }

    #[test]
    fn markdown_escapes_content_and_rejects_unsafe_links() {
        let content = ResumeContent {
            name: "A # <B>".into(),
            label: String::new(),
            contacts: Vec::new(),
            summary: String::new(),
            sections: vec![crate::semantic::Section {
                id: "projects",
                items: vec![Item {
                    header_fields: Default::default(),
                    short_en_date_range: String::new(),
                    heading: "[Test]".into(),
                    details: Vec::new(),
                    date_detail_index: None,
                    paragraphs: Vec::new(),
                    bullets: vec!["*Bold*".into()],
                    url: Some("javascript:alert(1)".into()),
                }],
            }],
        };
        let md = markdown(&content, Language::En);
        assert!(md.contains("# A \\# &lt;B&gt;"));
        assert!(md.contains("### \\[Test\\]"));
        assert!(md.contains("- \\*Bold\\*"));
        assert!(!md.contains("](javascript:"));
    }
}
