//! Editable Word paragraphs using shared preview/PDF theme tokens.
//! Word owns wrapping and pagination; this is not a raster export.

use std::io::{Cursor, Write};

use docx_rs::{
    Docx, LineSpacing, LineSpacingType, PageMargin, Paragraph, ParagraphBorder,
    ParagraphBorderPosition, ParagraphBorders, Run, RunFonts, Table, TableCell, TableCellMargins,
    TableLayoutType, TableRow, WidthType, XMLDocx,
};

use crate::ThemeManifest;
use crate::layout::{PT_PER_MM, check_theme, side_date_lines_for_style};
use crate::render::{Language, title};
use crate::semantic::ResumeContent;
use crate::theme_style::{DatePlacement, Rgb, SectionHeading, TextStyle, ThemeStyle};

fn color(rgb: Rgb) -> String {
    rgb.hex()[1..].to_owned()
}
fn twips(pt: f32) -> u32 {
    (pt * 20.0).round() as u32
}

fn font(style: &TextStyle) -> RunFonts {
    let kind = crate::layout::font_kind(style).expect("validated document font");
    let family = kind.family();
    let east_asia = if let crate::layout::FontKind::Local(id) = kind {
        let data = crate::fonts::get(id);
        if ttf_parser::Face::parse(&data.bytes, 0)
            .ok()
            .is_some_and(|face| face.glyph_index('中').is_some())
        {
            family.clone()
        } else {
            "Noto Sans SC".into()
        }
    } else {
        family.clone()
    };
    RunFonts::new()
        .ascii(&family)
        .hi_ansi(&family)
        .east_asia(east_asia)
        .cs(family)
}

fn effective_bold(style: &TextStyle) -> bool {
    match crate::layout::font_kind(style).expect("validated document font") {
        crate::layout::FontKind::Local(id) => crate::fonts::get(id).weight >= 600,
        _ => style.weight >= 600,
    }
}

fn paragraph(text: &str, style: &TextStyle, ink: Rgb, before_pt: f32, after_pt: f32) -> Paragraph {
    let mut run = Run::new()
        .size((style.size_pt * 2.0).round() as usize)
        .color(color(ink))
        .fonts(font(style));
    if effective_bold(style) {
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
    let style = ThemeStyle::built_in_fallback();
    pack(build_document(content, language, &style)?, &style)
}

pub fn render_with_theme(
    content: &ResumeContent,
    language: Language,
    theme: &ThemeManifest,
) -> Result<Vec<u8>, String> {
    // Never silently flatten unsupported composition, decorations, assets or typography.
    check_theme(theme)?;
    let style = ThemeStyle::from_manifest(theme);
    pack(build_document(content, language, &style)?, &style)
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
    let split_header = style.split_header
        && (!content.name.is_empty() || !content.label.is_empty())
        && !content.contacts.is_empty();
    if split_header {
        let total =
            twips((page.width_mm - page.margins.left - page.margins.right) * PT_PER_MM) as usize;
        let left_width = (total as f32 * 0.58).round() as usize;
        let right_width = total - left_width;
        let mut left = TableCell::new().width(left_width, WidthType::Dxa);
        if !content.name.is_empty() {
            left = left.add_paragraph(paragraph(
                &content.name,
                &style.typography.name,
                ink,
                0.0,
                0.0,
            ));
        }
        if !content.label.is_empty() {
            left = left.add_paragraph(paragraph(&content.label, body, accent, 0.0, 0.0));
        }
        let mut right = TableCell::new()
            .width(right_width, WidthType::Dxa)
            .add_paragraph(
                paragraph(
                    &content.contacts.join(if style.stacked_contacts {
                        "\n"
                    } else {
                        "   ·   "
                    }),
                    meta,
                    muted,
                    0.0,
                    0.0,
                )
                .align(docx_rs::AlignmentType::Right),
            );
        right.property = right
            .property
            .margin_left(twips(14.0) as usize, WidthType::Dxa);
        document = document.add_table(
            Table::without_borders(vec![TableRow::new(vec![left, right])])
                .set_grid(vec![left_width, right_width])
                .layout(TableLayoutType::Fixed)
                .width(total, WidthType::Dxa)
                .margins(TableCellMargins::new().margin(0, 0, 0, 0)),
        );
        let mut separator = Paragraph::new()
            .line_spacing(
                LineSpacing::new()
                    .after(twips(gap))
                    .line_rule(LineSpacingType::AtLeast)
                    .line(twips(2.0) as i32),
            )
            .keep_next(!content.summary.is_empty() || !content.sections.is_empty());
        if style.layout != "minimal" {
            let (rule_color, eighth_points) = match style.layout.as_str() {
                "modern" => (accent, 16),
                "rules" => (accent, 10),
                _ => (style.colors.border, 6),
            };
            separator.property.borders = Some(
                ParagraphBorders::with_empty().set(
                    ParagraphBorder::new(ParagraphBorderPosition::Bottom)
                        .color(color(rule_color))
                        .size(eighth_points)
                        .space(6),
                ),
            );
        }
        document = document.add_paragraph(separator);
    } else {
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
                &content.contacts.join(if style.stacked_contacts {
                    "\n"
                } else {
                    "   ·   "
                }),
                meta,
                muted,
                0.0,
                0.0,
            ));
        }
        if style.centered_header {
            header = header
                .into_iter()
                .map(|p| p.align(docx_rs::AlignmentType::Center))
                .collect();
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
            if section_style.date != DatePlacement::Inline
                && let Some(date) = item.date()
            {
                let lines = side_date_lines_for_style(style, &item.heading, date);
                if lines.heading.is_empty() {
                    let mut date_paragraph = paragraph(date, meta, muted, 0.0, 0.0);
                    if section_style.date == DatePlacement::Right {
                        date_paragraph = date_paragraph.align(docx_rs::AlignmentType::Right);
                    }
                    paragraphs.push(date_paragraph);
                } else if !lines.same_line {
                    let mut date_paragraph = paragraph(date, meta, muted, 0.0, 0.0);
                    if section_style.date == DatePlacement::Right {
                        date_paragraph = date_paragraph.align(docx_rs::AlignmentType::Right);
                        paragraphs.push(paragraph(&item.heading, &heading, ink, 0.0, 0.0));
                        paragraphs.push(date_paragraph);
                    } else {
                        paragraphs.push(date_paragraph);
                        paragraphs.push(paragraph(&item.heading, &heading, ink, 0.0, 0.0));
                    }
                } else if section_style.date == DatePlacement::Left {
                    let mut heading_run = Run::new()
                        .add_tab()
                        .add_text(&lines.heading[0])
                        .size((heading.size_pt * 2.0).round() as usize)
                        .color(color(ink))
                        .fonts(font(&heading));
                    if effective_bold(&heading) {
                        heading_run = heading_run.bold();
                    }
                    paragraphs.push(
                        paragraph(date, meta, muted, 0.0, 0.0)
                            .add_tab(
                                docx_rs::Tab::new()
                                    .val(docx_rs::TabValueType::Left)
                                    .pos(twips(lines.date_width_pt + 12.0) as usize),
                            )
                            .add_run(heading_run),
                    );
                    for continuation in lines.heading.iter().skip(1) {
                        paragraphs.push(paragraph(continuation, &heading, ink, 0.0, 0.0).indent(
                            Some(twips(lines.date_width_pt + 12.0) as i32),
                            None,
                            None,
                            None,
                        ));
                    }
                } else {
                    let date_run = Run::new()
                        .add_tab()
                        .add_text(date)
                        .size((meta.size_pt * 2.0).round() as usize)
                        .color(color(muted))
                        .fonts(font(meta));
                    let content_width =
                        (page.width_mm - page.margins.left - page.margins.right) * PT_PER_MM;
                    paragraphs.push(
                        paragraph(&lines.heading[0], &heading, ink, 0.0, 0.0)
                            .add_tab(
                                docx_rs::Tab::new()
                                    .val(docx_rs::TabValueType::Right)
                                    .pos(twips(content_width) as usize),
                            )
                            .add_run(date_run),
                    );
                    for continuation in lines.heading.iter().skip(1) {
                        paragraphs.push(paragraph(continuation, &heading, ink, 0.0, 0.0));
                    }
                }
            } else if !item.heading.is_empty() {
                paragraphs.push(paragraph(&item.heading, &heading, ink, 0.0, 0.0));
            }
            let details = if section_style.date != DatePlacement::Inline && item.date().is_some() {
                item.other_details().join("   ·   ")
            } else {
                item.details.join("   ·   ")
            };
            if !details.is_empty() {
                paragraphs.push(paragraph(&details, meta, muted, 0.0, 0.0));
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

fn pack(mut document: XMLDocx, style: &ThemeStyle) -> Result<Vec<u8>, String> {
    // The bundled OFL fonts permit embedding. Full faces preserve editability
    // for newly typed text; never subset against only the current resume.
    document.font_table = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:fonts xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:font w:name="Noto Sans SC"><w:family w:val="swiss"/><w:embedRegular r:id="rIdSans" w:fontKey="{00000000-0000-0000-0000-000000000001}" w:subsetted="false"/></w:font><w:font w:name="Noto Serif SC"><w:family w:val="roman"/><w:embedBold r:id="rIdSerif" w:fontKey="{00000000-0000-0000-0000-000000000002}" w:subsetted="false"/></w:font></w:fonts>"#.to_vec();
    let mut local_ids = std::collections::BTreeSet::new();
    for role in [
        &style.typography.body,
        &style.typography.name,
        &style.typography.section_title,
        &style.typography.meta,
    ] {
        if let crate::layout::FontKind::Local(id) = crate::layout::font_kind(role)? {
            local_ids.insert(id);
        }
    }
    let mut local_fonts = Vec::new();
    let mut font_entries = String::new();
    let mut family_entries = std::collections::BTreeMap::<String, String>::new();
    let mut relationships = String::new();
    for (index, id) in local_ids.into_iter().enumerate() {
        let font = crate::fonts::get(id);
        if !font.editable_embed {
            return Err(format!(
                "字体「{}」的嵌入标记不允许嵌入可编辑 DOCX，请更换字体",
                font.family
            ));
        }
        let key = (index + 3) as u8;
        let name = xml_escape(&font.family);
        let face_tag = if font.weight >= 600 {
            "embedBold"
        } else {
            "embedRegular"
        };
        let entries = family_entries.entry(name).or_default();
        // Word only has regular/bold slots. A family without a bold face can
        // resolve two requested weights to the same regular face; never emit
        // duplicate slots or ask Word to synthesize a weight absent in preview.
        if entries.contains(&format!("<w:{face_tag} ")) {
            continue;
        }
        entries.push_str(&format!(r#"<w:{face_tag} r:id="rIdLocal{index}" w:fontKey="{{00000000-0000-0000-0000-0000000000{key:02X}}}" w:subsetted="false"/>"#));
        relationships.push_str(&format!(r#"<Relationship Id="rIdLocal{index}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/font" Target="fonts/local{index}.odttf"/>"#));
        local_fonts.push((format!("local{index}"), font.bytes, key));
    }
    for (name, faces) in family_entries {
        font_entries.push_str(&format!(
            r#"<w:font w:name="{name}"><w:family w:val="auto"/>{faces}</w:font>"#
        ));
    }
    document.font_table = String::from_utf8(document.font_table)
        .map_err(|e| e.to_string())?
        .replace("</w:fonts>", &format!("{font_entries}</w:fonts>"))
        .into_bytes();
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
    let rels = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdSans" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/font" Target="fonts/sans.odttf"/><Relationship Id="rIdSerif" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/font" Target="fonts/serif.odttf"/>{relationships}</Relationships>"#
    );
    archive
        .write_all(rels.as_bytes())
        .map_err(|e| e.to_string())?;
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
    for (name, data, key) in local_fonts {
        archive
            .start_file(format!("word/fonts/{name}.odttf"), options)
            .map_err(|e| e.to_string())?;
        let mut bytes = data.as_ref().clone();
        bytes[0] ^= key;
        bytes[16] ^= key;
        archive.write_all(&bytes).map_err(|e| e.to_string())?;
    }
    Ok(archive.finish().map_err(|e| e.to_string())?.into_inner())
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{resume::parse_resume, semantic::project};

    #[test]
    fn split_header_is_an_editable_borderless_two_cell_table() {
        let mut theme = crate::parse_theme(include_bytes!(
            "../../resume-one-cli/tests/fixtures/centered-header.theme.json"
        ))
        .unwrap();
        theme.normalized["composition"] =
            serde_json::json!({"preset":"split-header","header":"split","contact":"stacked"});
        check_theme(&theme).unwrap();
        let mut content = project(
            &parse_resume(br#"{"basics":{"name":"Name","label":"Designer","summary":"Summary"},"work":[{"name":"Company"}]}"#).unwrap(),
        );
        content.contacts = vec!["first@example.com".into(), "Last contact".into()];
        let package =
            build_document(&content, Language::En, &ThemeStyle::from_manifest(&theme)).unwrap();
        let xml = String::from_utf8(package.document).unwrap();
        let table = xml
            .split("<w:tbl>")
            .nth(1)
            .unwrap()
            .split("</w:tbl>")
            .next()
            .unwrap();
        assert_eq!(table.matches("<w:tc>").count(), 2);
        assert!(table.contains("w:type=\"fixed\""));
        assert!(table.contains("<w:tblGrid>"));
        assert!(table.contains("Name</w:t>"));
        assert!(table.contains("Designer</w:t>"));
        assert!(table.contains("first@example.com"));
        assert!(table.contains("Last contact"));
        assert!(table.contains("<w:br"));
        assert!(table.contains("w:val=\"right\""));
        assert!(!table.contains("Summary"));
        assert!(xml.find("</w:tbl>").unwrap() < xml.find("Summary").unwrap());
        assert!(package.media.is_empty());

        content.contacts.clear();
        let fallback =
            build_document(&content, Language::En, &ThemeStyle::from_manifest(&theme)).unwrap();
        assert!(
            !String::from_utf8(fallback.document)
                .unwrap()
                .contains("<w:tbl>")
        );
    }

    #[test]
    fn centered_identity_and_stacked_contacts_are_native_and_do_not_center_summary() {
        let mut theme = crate::parse_theme(include_bytes!(
            "../../resume-one-cli/tests/fixtures/centered-header.theme.json"
        ))
        .unwrap();
        let mut content = project(&parse_resume(br#"{"basics":{"name":"Name","label":"Designer","summary":"Summary"},"work":[{"name":"Company"}]}"#).unwrap());
        content.contacts = vec!["first@example.com".into(), "Last contact".into()];
        for centered in [true, false] {
            for stacked in [true, false] {
                theme.normalized["composition"]["header"] =
                    serde_json::json!(if centered { "centered" } else { "stacked" });
                theme.normalized["composition"]["contact"] =
                    serde_json::json!(if stacked { "stacked" } else { "inline" });
                check_theme(&theme).unwrap();
                let package =
                    build_document(&content, Language::En, &ThemeStyle::from_manifest(&theme))
                        .unwrap();
                assert!(package.media.is_empty());
                let xml = String::from_utf8(package.document).unwrap();
                let paragraph =
                    |text: &str| xml.split("</w:p>").find(|p| p.contains(text)).unwrap();
                for text in [">Name</w:t>", ">Designer</w:t>", "first@example.com"] {
                    assert_eq!(paragraph(text).contains("w:val=\"center\""), centered);
                }
                for text in [">Summary</w:t>", ">Work</w:t>", ">Company</w:t>"] {
                    assert!(!paragraph(text).contains("w:val=\"center\""));
                }
                let contacts = paragraph("first@example.com");
                assert_eq!(contacts.contains("<w:br"), stacked);
                assert_eq!(contacts.contains("   ·   "), !stacked);
                assert!(
                    contacts.find("first@example.com").unwrap()
                        < contacts.find("Last contact").unwrap()
                );
            }
        }
    }

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

    #[test]
    fn right_dates_use_editable_right_tab_and_leave_other_details_in_order() {
        let mut theme = crate::parse_theme(include_bytes!(
            "../../resume-one-cli/tests/fixtures/section-styles.theme.json"
        ))
        .unwrap();
        theme.normalized["sections"]["work"]["date"] = serde_json::json!("right");
        theme.normalized["sections"]["awards"] = serde_json::json!({"date":"right"});
        check_theme(&theme).unwrap();
        let content = project(&parse_resume(br#"{"work":[{"position":"Engineer","name":"Acme","startDate":"2020-01-01","endDate":"2022-12-31","location":"Berlin"}],"awards":[{"title":"Prize","awarder":"Institute","date":"2023-05-01"}]}"#).unwrap());
        let package =
            build_document(&content, Language::En, &ThemeStyle::from_manifest(&theme)).unwrap();
        let xml = String::from_utf8(package.document).unwrap();
        let work = xml
            .split("</w:p>")
            .find(|p| p.contains("Engineer · Acme"))
            .unwrap();
        assert!(work.contains("<w:tabs>") && work.contains("w:val=\"right\""));
        assert!(work.contains("<w:tab") && work.contains("2020-01-01 – 2022-12-31"));
        assert!(!work.contains("Berlin"));
        let award = xml.split("</w:p>").find(|p| p.contains("Prize")).unwrap();
        assert!(award.contains("2023-05-01") && award.contains("w:val=\"right\""));
        assert!(xml.contains("Berlin") && xml.contains("Institute"));
        assert_eq!(xml.matches("2020-01-01 – 2022-12-31").count(), 1);
        assert!(package.media.is_empty());
    }

    #[test]
    fn long_right_date_moves_to_its_own_right_aligned_paragraph() {
        let mut theme = crate::parse_theme(include_bytes!(
            "../../resume-one-cli/tests/fixtures/section-styles.theme.json"
        ))
        .unwrap();
        theme.normalized["sections"]["work"]["date"] = serde_json::json!("right");
        let mut content = project(
            &parse_resume(
                br#"{"work":[{"position":"Engineer","name":"Acme","startDate":"2020"}]}"#,
            )
            .unwrap(),
        );
        let date = "2020 - 2022, extended appointment and a second extended appointment";
        let item = &mut content.sections[0].items[0];
        item.details[item.date_detail_index.unwrap()] = date.into();
        let style = ThemeStyle::from_manifest(&theme);
        assert!(!side_date_lines_for_style(&style, &item.heading, date).same_line);
        let package = build_document(&content, Language::En, &style).unwrap();
        let xml = String::from_utf8(package.document).unwrap();
        let heading = xml
            .split("</w:p>")
            .find(|p| p.contains("Engineer · Acme"))
            .unwrap();
        let separate_date = xml.split("</w:p>").find(|p| p.contains(date)).unwrap();
        assert!(!heading.contains("<w:tabs>"));
        assert!(!heading.contains(date));
        assert!(separate_date.contains("w:val=\"right\""));
        assert_eq!(xml.matches(date).count(), 1);
    }

    #[test]
    fn long_heading_places_right_date_on_first_editable_line() {
        let mut theme = crate::parse_theme(include_bytes!(
            "../../resume-one-cli/tests/fixtures/section-styles.theme.json"
        ))
        .unwrap();
        theme.normalized["sections"]["work"]["date"] = serde_json::json!("right");
        let content = project(
            &parse_resume(br#"{"work":[{"position":"Very long cross-platform engineering position with many words to wrap safely","name":"Example Organization","startDate":"2020","endDate":"2022"}]}"#).unwrap(),
        );
        let style = ThemeStyle::from_manifest(&theme);
        let item = &content.sections[0].items[0];
        let lines = side_date_lines_for_style(&style, &item.heading, item.date().unwrap());
        assert!(lines.same_line && lines.heading.len() > 1);
        let package = build_document(&content, Language::En, &style).unwrap();
        let xml = String::from_utf8(package.document).unwrap();
        let first = xml
            .split("</w:p>")
            .find(|p| p.contains(&lines.heading[0]))
            .unwrap();
        assert!(first.contains("<w:tabs>") && first.contains("2020 – 2022"));
        let continuation = xml
            .split("</w:p>")
            .find(|p| p.contains(&lines.heading[1]))
            .unwrap();
        assert!(!continuation.contains("2020 – 2022"));
        assert_eq!(xml.matches("2020 – 2022").count(), 1);
    }

    #[test]
    fn left_date_precedes_heading_and_aligns_continuation_in_docx() {
        let mut theme = crate::parse_theme(include_bytes!(
            "../../resume-one-cli/tests/fixtures/section-styles.theme.json"
        ))
        .unwrap();
        theme.normalized["sections"]["work"]["date"] = serde_json::json!("left");
        check_theme(&theme).unwrap();
        let content = project(
            &parse_resume(br#"{"work":[{"position":"Very long cross-platform engineering position with many words to wrap safely","name":"Example Organization","startDate":"2020","endDate":"2022","location":"Berlin"}]}"#).unwrap(),
        );
        let style = ThemeStyle::from_manifest(&theme);
        let item = &content.sections[0].items[0];
        let lines = side_date_lines_for_style(&style, &item.heading, item.date().unwrap());
        assert!(lines.same_line && lines.heading.len() > 1);
        let package = build_document(&content, Language::En, &style).unwrap();
        let xml = String::from_utf8(package.document).unwrap();
        let first = xml
            .split("</w:p>")
            .find(|p| p.contains(&lines.heading[0]))
            .unwrap();
        assert!(first.contains("2020 – 2022"));
        assert!(first.contains("w:val=\"left\""));
        assert!(first.find("2020 – 2022").unwrap() < first.find(&lines.heading[0]).unwrap());
        let continuation = xml
            .split("</w:p>")
            .find(|p| p.contains(&lines.heading[1]))
            .unwrap();
        assert!(continuation.contains("<w:ind"));
        assert!(!continuation.contains("2020 – 2022"));
        assert!(xml.contains("Berlin"));
        assert_eq!(xml.matches("2020 – 2022").count(), 1);

        let mut long_date = content.clone();
        let item = &mut long_date.sections[0].items[0];
        item.details[item.date_detail_index.unwrap()] =
            "An exceptionally long appointment date that needs an entire line".into();
        let package = build_document(&long_date, Language::En, &style).unwrap();
        let xml = String::from_utf8(package.document).unwrap();
        let date = xml
            .split("</w:p>")
            .find(|p| p.contains("An exceptionally long appointment"))
            .unwrap();
        assert!(!date.contains("w:val=\"right\""));
        assert!(!date.contains("<w:tabs>"));
        assert!(!date.contains(&lines.heading[0]));
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
