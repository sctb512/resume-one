//! Editable Word paragraphs using shared preview/PDF theme tokens.
//! Word owns wrapping and pagination; this is not a raster export.

use std::io::{Cursor, Write};

use docx_rs::{
    Docx, LineSpacing, LineSpacingType, PageMargin, Paragraph, ParagraphBorder,
    ParagraphBorderPosition, ParagraphBorders, Run, RunFonts, XMLDocx,
};

use crate::ThemeManifest;
use crate::layout::{PT_PER_MM, check_theme};
use crate::render::{Language, title};
use crate::semantic::ResumeContent;
use crate::theme_style::{Rgb, SectionHeading, TextStyle, ThemeStyle};

fn color(rgb: Rgb) -> String {
    rgb.hex()[1..].to_owned()
}
fn twips(pt: f32) -> u32 {
    (pt * 20.0).round() as u32
}

fn font(style: &TextStyle) -> RunFonts {
    let family = if style.family.ends_with("serif") {
        "Noto Serif SC"
    } else {
        "Noto Sans SC"
    };
    RunFonts::new()
        .ascii(family)
        .hi_ansi(family)
        .east_asia(family)
        .cs(family)
}

fn paragraph(text: &str, style: &TextStyle, ink: Rgb, before_pt: f32, after_pt: f32) -> Paragraph {
    let mut run = Run::new()
        .size((style.size_pt * 2.0).round() as usize)
        .color(color(ink))
        .fonts(font(style));
    if style.weight >= 600 {
        run = run.bold();
    }
    // Keep explicit newlines, but let Word wrap instead of hard-breaking at PDF wraps.
    for (index, line) in text.split('\n').enumerate() {
        if index > 0 {
            run = run.add_break(docx_rs::BreakType::TextWrapping);
        }
        run = run.add_text(line.trim_end_matches('\r'));
    }
    Paragraph::new()
        .add_run(run)
        .snap_to_grid(false)
        .widow_control(true)
        .line_spacing(
            LineSpacing::new()
                .before(twips(before_pt))
                .after(twips(after_pt))
                // Auto multiplies a CJK face's already generous font metrics.
                // AtLeast respects the requested leading without clipping tall glyphs.
                .line_rule(LineSpacingType::AtLeast)
                .line(twips(style.size_pt * style.line_height) as i32),
        )
}

fn section_heading(id: &str, language: Language, style: &ThemeStyle) -> Paragraph {
    let heading = style.section(id).heading;
    let gap = style.page.section_gap_mm * PT_PER_MM;
    let mut p = paragraph(
        title(id, language),
        &style.typography.section_title,
        if heading == SectionHeading::Filled {
            style.colors.paper
        } else {
            style.colors.accent
        },
        gap + if heading == SectionHeading::Filled {
            4.0
        } else {
            0.0
        },
        if heading == SectionHeading::Filled {
            8.0
        } else {
            4.0
        },
    );
    match heading {
        SectionHeading::Plain => {}
        SectionHeading::Line | SectionHeading::AccentLine => {
            p.property.borders = Some(
                ParagraphBorders::with_empty().set(
                    ParagraphBorder::new(ParagraphBorderPosition::Bottom)
                        .color(color(if heading == SectionHeading::Line {
                            style.colors.border
                        } else {
                            style.colors.accent
                        }))
                        .size(if heading == SectionHeading::Line {
                            5
                        } else {
                            10
                        })
                        .space(4),
                ),
            );
            let spacing = p.property.line_spacing.take().unwrap_or_default();
            p.property.line_spacing = Some(spacing.after(twips(10.0)));
        }
        SectionHeading::Filled => {
            p.property.shading = Some(docx_rs::Shading::new().fill(color(style.colors.accent)));
            p = p.indent(Some(twips(6.0) as i32), None, Some(twips(6.0) as i32), None);
            let mut borders = ParagraphBorders::with_empty();
            for (position, space) in [
                (ParagraphBorderPosition::Top, 4),
                (ParagraphBorderPosition::Bottom, 4),
                (ParagraphBorderPosition::Left, 6),
                (ParagraphBorderPosition::Right, 6),
            ] {
                borders = borders.set(
                    ParagraphBorder::new(position)
                        .color(color(style.colors.accent))
                        .size(1)
                        .space(space),
                );
            }
            p.property.borders = Some(borders);
        }
    }
    p
}

pub fn render(content: &ResumeContent, language: Language) -> Result<Vec<u8>, String> {
    pack(build_document(
        content,
        language,
        &ThemeStyle::built_in_fallback(),
    )?)
}

pub fn render_with_theme(
    content: &ResumeContent,
    language: Language,
    theme: &ThemeManifest,
) -> Result<Vec<u8>, String> {
    // Never silently flatten unsupported composition, decorations, assets or typography.
    check_theme(theme)?;
    pack(build_document(
        content,
        language,
        &ThemeStyle::from_manifest(theme),
    )?)
}

fn build_document(
    content: &ResumeContent,
    language: Language,
    style: &ThemeStyle,
) -> Result<XMLDocx, String> {
    let page = &style.page;
    let body = &style.typography.body;
    let meta = &style.typography.meta;
    let ink = style.colors.ink;
    let muted = style.colors.muted;
    let accent = style.colors.accent;
    let gap = page.section_gap_mm * PT_PER_MM;
    let mut document = Docx::new()
        .default_fonts(font(body))
        .default_size((body.size_pt * 2.0).round() as usize)
        .page_size(
            twips(page.width_mm * PT_PER_MM),
            twips(page.height_mm * PT_PER_MM),
        )
        .page_margin(PageMargin {
            top: twips(page.margins.top * PT_PER_MM) as i32,
            right: twips(page.margins.right * PT_PER_MM) as i32,
            bottom: twips(page.margins.bottom * PT_PER_MM) as i32,
            left: twips(page.margins.left * PT_PER_MM) as i32,
            header: 0,
            footer: 0,
            gutter: 0,
        });
    let mut header = Vec::new();
    if !content.name.is_empty() {
        header.push(paragraph(
            &content.name,
            &style.typography.name,
            ink,
            0.0,
            0.0,
        ));
    }
    if !content.label.is_empty() {
        header.push(paragraph(&content.label, body, accent, 0.0, 0.0));
    }
    if !content.contacts.is_empty() {
        header.push(paragraph(
            &content.contacts.join("   ·   "),
            meta,
            muted,
            0.0,
            0.0,
        ));
    }
    // A real paragraph border, not an image/table: reflows with the editable header.
    if let Some(last) = header.last_mut() {
        let spacing = last.property.line_spacing.take().unwrap_or_default();
        last.property.line_spacing = Some(spacing.after(twips(gap)));
        if style.layout != "minimal" {
            let (rule_color, eighth_points) = match style.layout.as_str() {
                "modern" => (accent, 16),
                "rules" => (accent, 10),
                _ => (style.colors.border, 6),
            };
            last.property.borders = Some(
                ParagraphBorders::with_empty().set(
                    ParagraphBorder::new(ParagraphBorderPosition::Bottom)
                        .color(color(rule_color))
                        .size(eighth_points)
                        .space(6),
                ),
            );
        }
    }
    let header_count = header.len();
    for (index, p) in header.into_iter().enumerate() {
        document = document.add_paragraph(p.keep_next(index + 1 < header_count));
    }
    if !content.summary.is_empty() {
        document = document.add_paragraph(paragraph(&content.summary, body, ink, 0.0, gap / 2.0));
    }
    let mut heading = body.clone();
    heading.size_pt *= 1.06;
    for section in &content.sections {
        let section_style = style.section(section.id);
        document = document.add_paragraph(
            section_heading(section.id, language, style)
                .keep_next(page.keep_heading_with_first_item && !section.items.is_empty())
                .keep_lines(true),
        );
        for item in &section.items {
            let mut paragraphs = Vec::new();
            if !item.heading.is_empty() {
                paragraphs.push(paragraph(&item.heading, &heading, ink, 0.0, 0.0));
            }
            if !item.details.is_empty() {
                paragraphs.push(paragraph(
                    &item.details.join("   ·   "),
                    meta,
                    muted,
                    0.0,
                    0.0,
                ));
            }
            for text in &item.paragraphs {
                paragraphs.push(paragraph(
                    text,
                    body,
                    ink,
                    section_style.paragraph_gap_pt(),
                    0.0,
                ));
            }
            if (section.id == "skills" || section.id == "interests") && !item.bullets.is_empty() {
                paragraphs.push(paragraph(
                    &item.bullets.join("   ·   "),
                    meta,
                    muted,
                    0.0,
                    0.0,
                ));
            } else {
                for bullet in &item.bullets {
                    paragraphs.push(
                        paragraph(&format!("•  {bullet}"), body, ink, 0.0, 0.0).indent(
                            Some(twips(11.0) as i32),
                            None,
                            None,
                            None,
                        ),
                    );
                }
            }
            if let Some(url) = &item.url {
                paragraphs.push(paragraph(url, meta, accent, 0.0, 0.0));
            }
            let count = paragraphs.len();
            for (index, mut p) in paragraphs.into_iter().enumerate() {
                if index == 0 {
                    let spacing = p.property.line_spacing.take().unwrap_or_default();
                    p.property.line_spacing =
                        Some(spacing.before(twips(section_style.item_gap_pt())));
                }
                // Link only this item, never a complete section or neighbouring items.
                // Oversized paragraphs stay splittable by Word to avoid overflow.
                document = document
                    .add_paragraph(p.keep_next(page.avoid_split_items && index + 1 < count));
            }
        }
    }
    // A repeating, behind-text page-sized vector shape prints consistently in
    // readers which ignore w:background. The header itself contains no text.
    document = document.header(docx_rs::Header::new());
    let mut package = document.build();
    let paper = color(style.colors.paper);
    package.headers[0] = format!(r##"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:v="urn:schemas-microsoft-com:vml"><w:p><w:pPr><w:spacing w:before="0" w:after="0" w:line="1" w:lineRule="exact"/></w:pPr><w:r><w:pict><v:rect id="ResumeOnePaper" style="position:absolute;margin-left:0;margin-top:0;width:{}pt;height:{}pt;z-index:-251654144;mso-position-horizontal-relative:page;mso-position-vertical-relative:page" fillcolor="#{paper}" stroked="f"><v:fill color="#{paper}"/></v:rect></w:pict></w:r></w:p></w:hdr>"##, page.width_mm * PT_PER_MM, page.height_mm * PT_PER_MM).into_bytes();
    // docx-rs has no document background API. Patch only generated trusted XML;
    // this value is a resolved six-digit RGB, never user markup.
    let xml = String::from_utf8(package.document).map_err(|error| error.to_string())?;
    if !xml.contains("<w:body>") {
        return Err("DOCX 输出缺少正文节点".into());
    }
    package.document = xml
        .replacen(
            "<w:body>",
            &format!(
                "<w:background w:color=\"{}\"/><w:body>",
                color(style.colors.paper)
            ),
            1,
        )
        .into_bytes();
    Ok(package)
}

fn pack(mut document: XMLDocx) -> Result<Vec<u8>, String> {
    // The bundled OFL fonts permit embedding. Full faces preserve editability
    // for newly typed text; never subset against only the current resume.
    document.font_table = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:fonts xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:font w:name="Noto Sans SC"><w:family w:val="swiss"/><w:embedRegular r:id="rIdSans" w:fontKey="{00000000-0000-0000-0000-000000000001}" w:subsetted="false"/></w:font><w:font w:name="Noto Serif SC"><w:family w:val="roman"/><w:embedBold r:id="rIdSerif" w:fontKey="{00000000-0000-0000-0000-000000000002}" w:subsetted="false"/></w:font></w:fonts>"#.to_vec();
    let content_types = String::from_utf8(document.content_type).map_err(|e| e.to_string())?;
    document.content_type = content_types.replace("</Types>", r#"<Default Extension="odttf" ContentType="application/vnd.openxmlformats-officedocument.obfuscatedFont"/></Types>"#).into_bytes();
    let settings = String::from_utf8(document.settings).map_err(|e| e.to_string())?;
    document.settings = settings.replace("</w:settings>", "<w:displayBackgroundShape/><w:embedTrueTypeFonts/><w:saveSubsetFonts w:val=\"false\"/></w:settings>").into_bytes();
    let mut bytes = Cursor::new(Vec::new());
    document
        .pack(&mut bytes)
        .map_err(|error| error.to_string())?;
    // Emit a fresh archive: readers disagree on appended OOXML packages, and
    // LibreOffice rejected the append form during cross-reader render QA.
    let mut parts = zip::ZipArchive::new(bytes).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .compression_level(Some(1));
    for index in 0..parts.len() {
        let mut part = parts.by_index(index).map_err(|e| e.to_string())?;
        if part.is_dir() {
            continue;
        }
        archive
            .start_file(part.name(), options)
            .map_err(|e| e.to_string())?;
        std::io::copy(&mut part, &mut archive).map_err(|e| e.to_string())?;
    }
    archive
        .start_file("word/_rels/fontTable.xml.rels", options)
        .map_err(|e| e.to_string())?;
    archive.write_all(br#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdSans" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/font" Target="fonts/sans.odttf"/><Relationship Id="rIdSerif" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/font" Target="fonts/serif.odttf"/></Relationships>"#).map_err(|e| e.to_string())?;
    for (name, data, key) in [
        ("sans", crate::layout::SANS_FONT_BYTES, 1u8),
        ("serif", crate::layout::SERIF_FONT_BYTES, 2u8),
    ] {
        archive
            .start_file(format!("word/fonts/{name}.odttf"), options)
            .map_err(|e| e.to_string())?;
        let mut obfuscated = data.to_vec();
        // ECMA-376: XOR the first 32 bytes with the fontKey in reverse byte order.
        // These fixed UUID keys have only their last byte set.
        obfuscated[0] ^= key;
        obfuscated[16] ^= key;
        archive.write_all(&obfuscated).map_err(|e| e.to_string())?;
    }
    Ok(archive.finish().map_err(|e| e.to_string())?.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{resume::parse_resume, semantic::project};

    #[test]
    fn section_headings_are_native_editable_and_compact_spacing_is_scoped() {
        let theme = crate::parse_theme(include_bytes!(
            "../../resume-one-cli/tests/fixtures/section-styles.theme.json"
        ))
        .unwrap();
        let style = ThemeStyle::from_manifest(&theme);
        let work = section_heading("work", Language::Zh, &style);
        assert_eq!(
            work.property.shading.as_ref().unwrap().fill,
            color(style.colors.accent)
        );
        assert!(work.property.borders.is_some());
        let content = project(&parse_resume(br#"{"work":[{"name":"WorkItem","summary":"WorkBody"}],"projects":[{"name":"ProjectItem","description":"ProjectBody"}],"education":[{"institution":"School"}]}"#).unwrap());
        let package = build_document(&content, Language::En, &style).unwrap();
        assert!(package.media.is_empty());
        let xml = String::from_utf8(package.document).unwrap();
        let paragraph_with = |text: &str| {
            xml.split("</w:p>")
                .find(|p| p.contains(&format!(">{text}</w:t>")))
                .unwrap()
                .to_owned()
        };
        let work = paragraph_with("Work");
        assert!(work.contains("<w:shd") && work.contains("w:fill=\"52756B\""));
        assert!(work.contains(&format!("w:color w:val=\"{}\"", color(style.colors.paper))));
        assert!(paragraph_with("WorkItem").contains("w:before=\"40\""));
        assert!(paragraph_with("WorkBody").contains("w:before=\"20\""));
        assert!(paragraph_with("ProjectItem").contains("w:before=\"80\""));
        assert!(paragraph_with("ProjectBody").contains("w:before=\"60\""));
        assert!(paragraph_with("Projects").contains("w:sz=\"10\""));
        assert!(paragraph_with("Education").contains("w:sz=\"5\""));
    }
    use std::io::Read;

    #[test]
    fn creates_readable_editable_docx_with_unicode() {
        let source = br#"{"basics":{"name":"\u674e\u660e","summary":"\u7b80\u5386\u6982\u8ff0"},"work":[{"name":"Acme","position":"Lead","highlights":["Shipped"]}],"custom":{"secret":"HIDDEN"}}"#;
        let content = project(&parse_resume(source).unwrap());
        let bytes = render(&content, Language::Zh).unwrap();
        assert!(bytes.starts_with(b"PK"));
        let body = docx_rs::read_docx(&bytes).unwrap().json();
        for text in ["李明", "工作经历", "Shipped"] {
            assert!(body.contains(text));
        }
        assert!(!body.contains("HIDDEN"));
        let mut archive = zip::ZipArchive::new(Cursor::new(&bytes)).unwrap();
        for (name, original, key) in [
            ("sans", crate::layout::SANS_FONT_BYTES, 1u8),
            ("serif", crate::layout::SERIF_FONT_BYTES, 2u8),
        ] {
            let mut decoded = Vec::new();
            archive
                .by_name(&format!("word/fonts/{name}.odttf"))
                .unwrap()
                .read_to_end(&mut decoded)
                .unwrap();
            decoded[0] ^= key;
            decoded[16] ^= key;
            assert_eq!(decoded, original, "embedded font must retain all glyphs");
        }
        let mut relationships = String::new();
        archive
            .by_name("word/_rels/fontTable.xml.rels")
            .unwrap()
            .read_to_string(&mut relationships)
            .unwrap();
        assert!(relationships.contains("rIdSans") && relationships.contains("rIdSerif"));
        assert!(!relationships.contains("TargetMode=\"External\""));
    }

    #[test]
    fn applies_page_palette_typography_and_native_breaks_without_images() {
        let theme = crate::parse_theme(br##"{"formatVersion":2,"id":"night","name":"Night","version":"1.0.0","license":"MIT","layout":"rules","colors":{"accent":"#C9AC77","seal":"#A14F40","paper":"#172426","canvas":"#101A1B"},"modules":["basics","work"],"page":{"size":"Letter","marginsMm":{"top":14,"right":17,"bottom":15,"left":18},"avoidSplitItems":false,"keepHeadingWithFirstItem":false},"typography":{"name":{"sizePt":30},"body":{"sizePt":11,"lineHeight":1.5}}}"##).unwrap();
        let content = project(&parse_resume(br#"{"basics":{"name":"Name","summary":"One\nTwo <&>"},"work":[{"name":"Company","summary":"text","highlights":["one","two"]}]}"#).unwrap());
        let package =
            build_document(&content, Language::En, &ThemeStyle::from_manifest(&theme)).unwrap();
        let xml = String::from_utf8(package.document).unwrap();
        assert!(xml.contains("w:background w:color=\"172426\""));
        assert!(xml.contains("w:w=\"12240\" w:h=\"15840\""));
        for (key, value) in [
            ("top", 794),
            ("right", 964),
            ("bottom", 850),
            ("left", 1020),
        ] {
            assert!(xml.contains(&format!("w:{key}=\"{value}\"")), "{xml}");
        }
        for token in [
            "w:sz w:val=\"60\"",
            "w:line=\"330\"",
            "Noto Serif SC",
            "Noto Sans SC",
            "C9AC77",
            "<w:br",
            "Two &lt;&amp;&gt;",
            "<w:pBdr>",
        ] {
            assert!(xml.contains(token), "missing {token}");
        }
        assert!(!xml.contains("<w:keepNext/>") && !xml.contains("<w:keepNext />"));
        assert!(package.media.is_empty());
        let header = String::from_utf8(package.headers[0].clone()).unwrap();
        assert!(header.contains("fillcolor=\"#172426\""));
        assert!(header.contains("width:612pt;height:792pt"));
        assert!(header.contains("z-index:-251654144"));
    }

    #[test]
    fn all_bundled_themes_export_and_minimal_has_no_rule() {
        let content = project(&parse_resume(br#"{"basics":{"name":"Name"}}"#).unwrap());
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/themes");
        let mut count = 0;
        for entry in std::fs::read_dir(root).unwrap() {
            let source = std::fs::read(entry.unwrap().path()).unwrap();
            let theme = crate::parse_theme(&source).unwrap();
            assert!(
                docx_rs::read_docx(&render_with_theme(&content, Language::Zh, &theme).unwrap())
                    .unwrap()
                    .json()
                    .contains("Name")
            );
            if theme.normalized["layout"] == "minimal" {
                let package =
                    build_document(&content, Language::Zh, &ThemeStyle::from_manifest(&theme))
                        .unwrap();
                assert!(
                    !String::from_utf8(package.document)
                        .unwrap()
                        .contains("<w:pBdr>")
                );
            }
            count += 1;
        }
        assert_eq!(count, 20);
    }

    #[test]
    fn rejects_unsupported_theme_instead_of_flattening() {
        let mut theme =
            crate::parse_theme(include_bytes!("../../../assets/themes/zhibai.theme.json")).unwrap();
        theme.normalized["composition"] = serde_json::json!({"preset":"sidebar-left"});
        let content = project(&parse_resume(b"{}").unwrap());
        assert!(
            render_with_theme(&content, Language::Zh, &theme)
                .unwrap_err()
                .contains("composition")
        );
    }
}
