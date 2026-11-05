//! Bounded literal heading evidence. No template expressions are evaluated.
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub(crate) struct Headings {
    records: Vec<Value>,
    seen: BTreeSet<(String, usize, String)>,
}

fn module(title: &str) -> Option<&'static str> {
    crate::adapt_style::static_section_title(title)
}

impl Headings {
    fn add(
        &mut self,
        file: &str,
        start: usize,
        end: usize,
        tag: &str,
        title: &str,
        kind: &str,
        decoded_origin: Option<usize>,
    ) -> Result<(), String> {
        let title = title.trim().replace("&amp;", "&");
        let Some(id) = module(&title) else {
            return Ok(());
        };
        if title.len() > 120
            || !self.seen.insert((
                file.into(),
                decoded_origin.unwrap_or(0) + start,
                title.clone(),
            ))
        {
            return Ok(());
        }
        if self.records.len() >= 2048 {
            return Err("MARKUP_HINT_LIMIT: more than 2048 headings".into());
        }
        let mut record =
            json!({"file":file,"tag":tag,"title":title,"module":id,"kind":kind,"equivalent":false});
        if let Some(origin) = decoded_origin {
            record["literalFragmentSourceStartByte"] = json!(origin);
            record["decodedStartByte"] = json!(start);
            record["decodedEndByte"] = json!(end);
            record["sourceOffsetMappingVerified"] = json!(false);
        } else {
            record["startByte"] = json!(start);
            record["endByte"] = json!(end);
        }
        self.records.push(record);
        Ok(())
    }

    pub(crate) fn jsx(
        &mut self,
        node: tree_sitter::Node<'_>,
        source: &str,
        file: &str,
    ) -> Result<(), String> {
        if node.kind() != "jsx_element" {
            return Ok(());
        }
        let Some(open) = node.named_child(0) else {
            return Ok(());
        };
        let Some(name) = open.child_by_field_name("name") else {
            return Ok(());
        };
        let tag = &source[name.byte_range()];
        let norm = tag.to_ascii_lowercase();
        if !matches!(norm.as_str(), "h1" | "h2" | "h3" | "h4" | "h5" | "h6")
            && !norm.ends_with("sectiontitle")
        {
            return Ok(());
        }
        let mut text = String::new();
        for n in 1..node.named_child_count().saturating_sub(1) {
            let child = node.named_child(n).unwrap();
            if child.kind() != "jsx_text" && child.kind() != "html_character_reference" {
                return Ok(());
            }
            text.push_str(&source[child.byte_range()]);
        }
        self.add(
            file,
            node.start_byte(),
            node.end_byte(),
            tag,
            &text,
            "jsx-literal",
            None,
        )
    }

    pub(crate) fn literal(
        &mut self,
        text: &str,
        file: &str,
        origin: usize,
        decoded: bool,
    ) -> Result<(), String> {
        if text.len() > 1024 * 1024 {
            return Err("MARKUP_HINT_LIMIT: literal over 1 MiB".into());
        }
        let lower = text.to_ascii_lowercase();
        let mut at = 0;
        let mut tags = 0;
        while let Some(offset) = lower[at..].find('<') {
            let start = at + offset;
            tags += 1;
            if tags > 20000 {
                return Err("MARKUP_HINT_LIMIT: too many HTML tokens".into());
            }
            if lower[start..].starts_with("<!--") {
                let Some(end) = lower[start + 4..].find("-->") else {
                    break;
                };
                at = start + 4 + end + 3;
                continue;
            }
            // Consume the complete tag including quoted attributes. A fake
            // heading in an attribute must not become a document section.
            let mut quote = None;
            let mut end = None;
            for (i, c) in text[start + 1..].char_indices() {
                if let Some(q) = quote {
                    if c == q {
                        quote = None
                    }
                } else if c == '\'' || c == '"' {
                    quote = Some(c)
                } else if c == '>' {
                    end = Some(start + 1 + i + 1);
                    break;
                }
            }
            let Some(end) = end else { break };
            let head = lower[start + 1..end - 1].trim();
            let tag = head.split_ascii_whitespace().next().unwrap_or("");
            at = end;
            if matches!(tag, "script" | "style") {
                if let Some(close) = lower[end..].find(&format!("</{tag}>")) {
                    at = end + close + tag.len() + 3
                } else {
                    break;
                }
                continue;
            }
            if !matches!(tag, "h1" | "h2" | "h3" | "h4" | "h5" | "h6") {
                continue;
            }
            let Some(close) = lower[end..].find(&format!("</{tag}>")) else {
                continue;
            };
            if close > 4096 {
                continue;
            }
            let value = &text[end..end + close];
            if value.contains(['<', '{', '}', '$']) {
                continue;
            }
            let finish = end + close + tag.len() + 3;
            if decoded {
                self.add(
                    file,
                    start,
                    finish,
                    tag,
                    value,
                    "decoded-html-literal",
                    Some(origin),
                )?;
            } else {
                self.add(
                    file,
                    origin + start,
                    origin + finish,
                    tag,
                    value,
                    "html-literal",
                    None,
                )?;
            }
        }
        Ok(())
    }

    pub(crate) fn apply(mut self, report: &mut Value) {
        let position = |r: &Value| {
            (
                r["file"].as_str().unwrap_or("").to_owned(),
                r["startByte"]
                    .as_u64()
                    .or_else(|| r["literalFragmentSourceStartByte"].as_u64())
                    .unwrap_or(0),
                r["decodedStartByte"].as_u64().unwrap_or(0),
            )
        };
        self.records.sort_by_key(position);
        let mut native = BTreeMap::<String, BTreeSet<String>>::new();
        let mut groups = BTreeMap::<String, Vec<String>>::new();
        for r in &self.records {
            let tag = r["tag"].as_str().unwrap();
            let id = r["module"].as_str().unwrap();
            if tag.len() == 2 && tag.starts_with('h') {
                native.entry(tag.into()).or_default().insert(id.into());
            }
            let group = groups
                .entry(r["file"].as_str().unwrap().into())
                .or_default();
            if !group.iter().any(|s| s == id) {
                group.push(id.into())
            }
        }
        let heading_tag = if native.len() == 1 {
            native
                .iter()
                .next()
                .filter(|(_, ids)| ids.len() >= 2)
                .map(|(tag, _)| tag.clone())
        } else {
            None
        };
        let mut orders = groups
            .into_iter()
            .filter(|(_, order)| order.len() >= 2)
            .collect::<Vec<_>>();
        orders.sort_by(|(fa, a), (fb, b)| b.len().cmp(&a.len()).then(fa.cmp(fb)));
        if let Some(tag) = &heading_tag {
            for file in report["syntaxEvidence"]
                .as_array_mut()
                .into_iter()
                .flatten()
            {
                if file["styles"].is_object() {
                    file["styles"]["resumeOneModuleHeadingTag"] = json!(tag);
                }
                for style in file["styledComponents"]
                    .as_array_mut()
                    .into_iter()
                    .flatten()
                {
                    if style["css"].is_object() {
                        style["css"]["resumeOneModuleHeadingTag"] = json!(tag);
                    }
                }
            }
        }
        report["markupHints"] = json!({"literalHeadings":self.records,"moduleHeadingTag":heading_tag,
            "moduleOrder":orders.first().map(|(_,order)|order),"orderSource":orders.first().map(|(file,_)|file),
            "bindingVerified":false,"equivalent":false,"limitation":"只读实际 JSX 节点或字面 HTML；完整源码顺序和同级标题推断，不证明运行时分支或绑定"});
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn comments_scripts_attributes_and_dynamic_titles_do_not_create_headings() {
        let mut h = Headings::default();
        h.literal("<!-- <h3>Skills</h3> --><div title='<h3>Awards</h3>'></div><script>'<h3>Projects</h3>'</script><h3>${section}</h3><h3>Work</h3><h3>Education</h3>","page.html",0,false).unwrap();
        let mut r = json!({"syntaxEvidence":[{"styles":{}}]});
        h.apply(&mut r);
        assert_eq!(
            r["markupHints"]["moduleOrder"],
            json!(["work", "education"])
        );
        assert_eq!(
            r["syntaxEvidence"][0]["styles"]["resumeOneModuleHeadingTag"],
            "h3"
        );
        assert_eq!(
            r["markupHints"]["literalHeadings"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
    }
    #[test]
    fn mixed_native_heading_levels_are_not_forced_to_one_global_tag() {
        let mut h = Headings::default();
        h.literal("<h2>Work</h2><h3>Skills</h3>", "x.html", 0, false)
            .unwrap();
        let mut r = json!({"syntaxEvidence":[{"styles":{}}]});
        h.apply(&mut r);
        assert!(r["markupHints"]["moduleHeadingTag"].is_null());
    }

    #[test]
    fn decoded_literal_offsets_are_not_presented_as_source_spans() {
        let mut h = Headings::default();
        h.literal("\n<h3>Work</h3>", "template.js", 300, true)
            .unwrap();
        h.literal("<h3>Education</h3>", "template.js", 500, true)
            .unwrap();
        let mut r = json!({"syntaxEvidence":[]});
        h.apply(&mut r);
        let evidence = &r["markupHints"]["literalHeadings"][0];
        assert_eq!(evidence["literalFragmentSourceStartByte"], 300);
        assert_eq!(evidence["decodedStartByte"], 1);
        assert_eq!(evidence["sourceOffsetMappingVerified"], false);
        assert!(evidence.get("startByte").is_none());
        assert_eq!(
            r["markupHints"]["moduleOrder"],
            json!(["work", "education"])
        );
    }

    #[test]
    fn academic_module_titles_keep_their_source_names_and_order() {
        let mut h = Headings::default();
        h.literal("<h2>Research Projects</h2><h2>Service &amp; Leadership</h2><h2>Research Interests</h2>","academic.html",0,false).unwrap();
        let mut r = json!({"syntaxEvidence":[]});
        h.apply(&mut r);
        assert_eq!(
            r["markupHints"]["moduleOrder"],
            json!(["projects", "volunteer", "interests"])
        );
        assert_eq!(
            r["markupHints"]["literalHeadings"][1]["title"],
            "Service & Leadership"
        );
    }
}
