//! Editable Word paragraphs using shared preview/PDF theme tokens.
//! Word owns wrapping and pagination; this is not a raster export.

use std::io::{Cursor, Write};
use unicode_segmentation::UnicodeSegmentation;

use docx_rs::{
    Docx, LineSpacing, LineSpacingType, PageMargin, Paragraph, ParagraphBorder,
    ParagraphBorderPosition, ParagraphBorders, Run, RunFonts, Table, TableCell, TableCellMargins,
    TableLayoutType, TableRow, WidthType, XMLDocx,
};

use crate::ThemeManifest;
use crate::layout::{PT_PER_MM, check_theme, side_date_lines_for_style};
use crate::render::{Language, title};
use crate::semantic::ResumeContent;
use crate::theme_style::{DatePlacement, ItemLayout, Rgb, SectionHeading, TextStyle, ThemeStyle};

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

fn styled_run(style: &TextStyle, ink: Rgb) -> Run {
    let mut run = Run::new()
        .size((style.size_pt * 2.0).round() as usize)
        .color(color(ink))
        .fonts(font(style));
    if effective_bold(style) {
        run = run.bold();
    }
    if crate::layout::font_kind(style)
        .expect("validated document font")
        .is_italic()
    {
        run = run.italic();
    }
    if style.tracking_em != 0.0 {
        run = run.character_spacing((style.tracking_pt() * 20.0).round() as i32);
    }
    run
}

fn add_lines(mut run: Run, text: &str) -> Run {
    // Keep explicit newlines, but let Word wrap instead of hard-breaking at PDF wraps.
    for (index, line) in text.split('\n').enumerate() {
        if index > 0 {
            run = run.add_break(docx_rs::BreakType::TextWrapping);
        }
        run = run.add_text(line.trim_end_matches('\r'));
    }
    run
}

fn text_runs(text: &str, style: &TextStyle, ink: Rgb) -> Vec<Run> {
    let displayed = style.display_text(text);
    let kind = crate::layout::font_kind(style).expect("validated document font");
    // Keep legacy upright runs unchanged. Explicitly separate missing-glyph
    // spans in italic runs so Word cannot synthesize italic on our upright CJK
    // fallback, which preview/PDF also render as a real upright face.
    let crate::layout::FontKind::Local(id) = kind else {
        return vec![add_lines(styled_run(style, ink), &displayed)];
    };
    if !kind.is_italic() || displayed.is_empty() {
        return vec![add_lines(styled_run(style, ink), &displayed)];
    }
    let font = crate::fonts::get(id);
    let face = ttf_parser::Face::parse(&font.bytes, 0).expect("validated font");
    let mut spans: Vec<(usize, usize, bool)> = Vec::new();
    for (start, cluster) in displayed.grapheme_indices(true) {
        let fallback = !cluster
            .chars()
            .all(|c| c.is_whitespace() || face.glyph_index(c).is_some());
        if let Some((_, end, previous)) = spans.last_mut()
            && *previous == fallback
        {
            *end = start + cluster.len();
        } else {
            spans.push((start, start + cluster.len(), fallback));
        }
    }
    let mut fallback = style.clone();
    fallback.family = "cjk-sans".into();
    fallback.weight = 400;
    fallback.italic = false;
    fallback.asset_font = None;
    spans
        .into_iter()
        .map(|(start, end, missing)| {
            add_lines(
                styled_run(if missing { &fallback } else { style }, ink),
                &displayed[start..end],
            )
        })
        .collect()
}

fn paragraph(text: &str, style: &TextStyle, ink: Rgb, before_pt: f32, after_pt: f32) -> Paragraph {
    let mut p = Paragraph::new();
    for run in text_runs(text, style, ink) {
        p = p.add_run(run);
    }
    p.snap_to_grid(false).widow_control(true).line_spacing(
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

fn keyword_chips(labels: &[String], style: &ThemeStyle) -> Paragraph {
    let meta = &style.typography.meta;
    let background = style.colors.paper.mix(style.colors.accent, 0.08);
    let ink = style.colors.ink.readable(background, style.colors.ink, 4.5);
    let mut p = Paragraph::new().snap_to_grid(false).line_spacing(
        LineSpacing::new()
            .before(twips(4.0))
            .line_rule(LineSpacingType::AtLeast)
            .line(twips(meta.size_pt * meta.line_height + 8.0) as i32),
    );
    for label in labels {
        for run in text_runs(&format!(" {label} "), meta, ink) {
            p = p.add_run(run.shading(docx_rs::Shading::new().fill(color(background))));
        }
        p = p.add_run(Run::new().add_text("  "));
    }
    p
}

fn decorate_item(mut p: Paragraph, kind: ItemLayout, style: &ThemeStyle) -> Paragraph {
    let cards = kind == ItemLayout::Cards;
    if !cards && kind != ItemLayout::Timeline {
        return p;
    }
    let mut borders = ParagraphBorders::with_empty();
    let border_color = if cards {
        style.colors.border
    } else {
        style.colors.accent
    };
    for (position, show) in [
        (ParagraphBorderPosition::Left, true),
        (ParagraphBorderPosition::Right, cards),
        (ParagraphBorderPosition::Top, cards),
        (ParagraphBorderPosition::Bottom, cards),
    ] {
        if show {
            borders = borders.set(
                ParagraphBorder::new(position)
                    .color(color(border_color))
                    .size(if cards { 5 } else { 8 })
                    .space(5),
            );
        }
    }
    p.property.borders = Some(borders);
    p = p.indent(
        Some(twips(if cards { 8.0 } else { 16.0 }) as i32),
        None,
        cards.then(|| twips(8.0) as i32),
        None,
    );
    if cards {
        p.property.shading = Some(docx_rs::Shading::new().fill(color(style.card_paper())));
    }
    p
}

/// A keepNext paragraph inside a page-spanning outer row can cause readers to
/// push the entire two-column table off the first page. Use small nested rows
/// for semantic keep groups instead; the outer lane row remains splittable.
fn lane_table(paragraphs: Vec<Paragraph>, width: usize) -> Table {
    let mut rows = Vec::new();
    let mut group = Vec::new();
    for mut paragraph in paragraphs {
        let keep = paragraph.property.keep_next.take().unwrap_or(false);
        group.push(paragraph);
        if !keep {
            let together = group.len() > 1;
            let mut cell = TableCell::new().width(width, WidthType::Dxa);
            for p in std::mem::take(&mut group) {
                cell = cell.add_paragraph(p);
            }
            let row = TableRow::new(vec![cell]);
            rows.push(if together { row.cant_split() } else { row });
        }
    }
    if !group.is_empty() {
        let mut cell = TableCell::new().width(width, WidthType::Dxa);
        for p in group {
            cell = cell.add_paragraph(p);
        }
        rows.push(TableRow::new(vec![cell]));
    }
    Table::without_borders(rows)
        .set_grid(vec![width])
        .layout(TableLayoutType::Fixed)
        .width(width, WidthType::Dxa)
        .margins(TableCellMargins::new().margin(0, 0, 0, 0))
}

fn lane_end() -> Paragraph {
    // OOXML requires a paragraph after a nested table. Keep it unobtrusive.
    Paragraph::new().add_run(Run::new().size(2)).line_spacing(
        LineSpacing::new()
            .before(0)
            .after(0)
            .line_rule(LineSpacingType::AtLeast)
            .line(20),
    )
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

enum BasicsBlock {
    Paragraph(Box<Paragraph>),
    Table(Box<Table>),
}

impl BasicsBlock {
    fn paragraph(p: Paragraph) -> Self {
        Self::Paragraph(Box::new(p))
    }
    fn table(t: Table) -> Self {
        Self::Table(Box::new(t))
    }
}

fn basics_blocks(content: &ResumeContent, style: &ThemeStyle, region: &str) -> Vec<BasicsBlock> {
    let page = &style.page;
    let label = &style.typography.label;
    let summary = &style.typography.summary;
    let meta = &style.typography.meta;
    let ink = style.colors.ink;
    let muted = style.colors.muted;
    let accent = style.colors.accent;
    let gap = page.section_gap_mm * PT_PER_MM;
    let mut blocks = Vec::new();
    if style.header_region == region {
        let split_header = style.split_header
            && style.contact_region == "header"
            && (!content.name.is_empty() || !content.label.is_empty())
            && !content.contacts.is_empty();
        if split_header {
            let total = twips((page.width_mm - page.margins.left - page.margins.right) * PT_PER_MM)
                as usize;
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
                left = left.add_paragraph(paragraph(&content.label, label, accent, 0.0, 0.0));
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
            blocks.push(BasicsBlock::table(
                Table::without_borders(vec![TableRow::new(vec![left, right])])
                    .set_grid(vec![left_width, right_width])
                    .layout(TableLayoutType::Fixed)
                    .width(total, WidthType::Dxa)
                    .margins(TableCellMargins::new().margin(0, 0, 0, 0)),
            ));
            if style.summary_region == "header" && !content.summary.is_empty() {
                blocks.push(BasicsBlock::paragraph(paragraph(
                    &content.summary,
                    summary,
                    ink,
                    gap / 2.0,
                    0.0,
                )));
            }
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
            blocks.push(BasicsBlock::paragraph(separator));
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
                header.push(paragraph(&content.label, label, accent, 0.0, 0.0));
            }
            if style.contact_region == "header" && !content.contacts.is_empty() {
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
            if style.summary_region == "header" && !content.summary.is_empty() {
                header.push(paragraph(&content.summary, summary, ink, gap / 2.0, 0.0));
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
                blocks.push(BasicsBlock::paragraph(
                    p.keep_next(index + 1 < header_count),
                ));
            }
        }
    }
    if style.contact_region == region && !content.contacts.is_empty() {
        blocks.push(BasicsBlock::paragraph(paragraph(
            &content.contacts.join(if style.stacked_contacts {
                "\n"
            } else {
                "   ·   "
            }),
            meta,
            muted,
            0.0,
            gap / 2.0,
        )));
    }
    if style.summary_region == region && !content.summary.is_empty() {
        blocks.push(BasicsBlock::paragraph(paragraph(
            &content.summary,
            summary,
            ink,
            0.0,
            gap / 2.0,
        )));
    }
    blocks
}

fn build_document(
    content: &ResumeContent,
    language: Language,
    style: &ThemeStyle,
) -> Result<XMLDocx, String> {
    let page = &style.page;
    let body = &style.typography.body;
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
    for block in basics_blocks(content, style, "page") {
        document = match block {
            BasicsBlock::Paragraph(p) => document.add_paragraph(*p),
            BasicsBlock::Table(t) => document.add_table(*t),
        };
    }
    let mut columns: [Vec<Paragraph>; 2] = Default::default();
    for section in &content.sections {
        let sidebar = style.in_sidebar(section.id);
        let mut local = style.column_style(sidebar);
        let kind = local.section(section.id).items;
        if kind == ItemLayout::Cards {
            local.page.margins.left += 8.0 / PT_PER_MM;
            local.page.margins.right += 8.0 / PT_PER_MM;
        } else if kind == ItemLayout::Timeline {
            local.page.margins.left += 16.0 / PT_PER_MM;
        }
        let style = &local;
        let page = &style.page;
        let body = &style.typography.body;
        let meta = &style.typography.meta;
        let date_style = &style.typography.side_date;
        let (ink, muted, accent) = (style.colors.ink, style.colors.muted, style.colors.accent);
        let heading = style.typography.item_title.clone();
        let column = &mut columns[usize::from(sidebar)];
        let section_style = style.section(section.id);
        column.push(
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
                    let mut date_paragraph = paragraph(date, date_style, muted, 0.0, 0.0);
                    if section_style.date == DatePlacement::Right {
                        date_paragraph = date_paragraph.align(docx_rs::AlignmentType::Right);
                    }
                    paragraphs.push(date_paragraph);
                } else if !lines.same_line {
                    let mut date_paragraph = paragraph(date, date_style, muted, 0.0, 0.0);
                    if section_style.date == DatePlacement::Right {
                        date_paragraph = date_paragraph.align(docx_rs::AlignmentType::Right);
                        paragraphs.push(paragraph(&item.heading, &heading, ink, 0.0, 0.0));
                        paragraphs.push(date_paragraph);
                    } else {
                        paragraphs.push(date_paragraph);
                        paragraphs.push(paragraph(&item.heading, &heading, ink, 0.0, 0.0));
                    }
                } else if section_style.date == DatePlacement::Left {
                    let mut p = paragraph(date, date_style, muted, 0.0, 0.0)
                        .add_tab(
                            docx_rs::Tab::new()
                                .val(docx_rs::TabValueType::Left)
                                .pos(twips(lines.date_width_pt + 12.0) as usize),
                        )
                        .add_run(styled_run(&heading, ink).add_tab());
                    for run in text_runs(&lines.heading[0], &heading, ink) {
                        p = p.add_run(run);
                    }
                    paragraphs.push(p);
                    for continuation in lines.heading.iter().skip(1) {
                        paragraphs.push(paragraph(continuation, &heading, ink, 0.0, 0.0).indent(
                            Some(twips(lines.date_width_pt + 12.0) as i32),
                            None,
                            None,
                            None,
                        ));
                    }
                } else {
                    let content_width =
                        (page.width_mm - page.margins.left - page.margins.right) * PT_PER_MM;
                    let mut p = paragraph(&lines.heading[0], &heading, ink, 0.0, 0.0)
                        .add_tab(
                            docx_rs::Tab::new()
                                .val(docx_rs::TabValueType::Right)
                                .pos(twips(content_width) as usize),
                        )
                        .add_run(styled_run(date_style, muted).add_tab());
                    for run in text_runs(date, date_style, muted) {
                        p = p.add_run(run);
                    }
                    paragraphs.push(p);
                    for continuation in lines.heading.iter().skip(1) {
                        paragraphs.push(paragraph(continuation, &heading, ink, 0.0, 0.0));
                    }
                }
            } else if section_style.items == ItemLayout::Chips
                && section.id == "languages"
                && !item.heading.is_empty()
            {
                paragraphs.push(keyword_chips(std::slice::from_ref(&item.heading), style));
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
                paragraphs.push(if section_style.items == ItemLayout::Chips {
                    keyword_chips(&item.bullets, style)
                } else {
                    paragraph(&item.bullets.join("   ·   "), meta, muted, 0.0, 0.0)
                });
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
                p = decorate_item(p, section_style.items, style);
                if index == 0 {
                    let spacing = p.property.line_spacing.take().unwrap_or_default();
                    p.property.line_spacing =
                        Some(spacing.before(twips(section_style.item_gap_pt())));
                }
                // Link only this item, never a complete section or neighbouring items.
                // Oversized paragraphs stay splittable by Word to avoid overflow.
                column.push(p.keep_next(page.avoid_split_items && index + 1 < count));
            }
            if section_style.items == ItemLayout::Cards {
                // Identical paragraph borders merge into one editable card in
                // Word. An unbordered spacer keeps adjacent cards independent.
                column.push(
                    Paragraph::new().line_spacing(
                        LineSpacing::new()
                            .line_rule(LineSpacingType::Exact)
                            .line(twips(4.0) as i32),
                    ),
                );
            }
        }
    }
    if style.has_sidebar() {
        let main_style = style.column_style(false);
        let side_style = style.column_style(true);
        let main_width = twips(
            (page.width_mm - main_style.page.margins.left - main_style.page.margins.right)
                * PT_PER_MM,
        ) as usize;
        let side_width = twips(
            (page.width_mm - side_style.page.margins.left - side_style.page.margins.right)
                * PT_PER_MM
                + 12.0,
        ) as usize;
        let total =
            twips((page.width_mm - page.margins.left - page.margins.right) * PT_PER_MM) as usize;
        let gap_width = total.saturating_sub(main_width + side_width);
        let mut main = TableCell::new().width(main_width, WidthType::Dxa);
        let mut side = TableCell::new()
            .width(side_width, WidthType::Dxa)
            .shading(docx_rs::Shading::new().fill(color(style.colors.sidebar)));
        side.property = side
            .property
            .margin_left(twips(6.0) as usize, WidthType::Dxa)
            .margin_right(twips(6.0) as usize, WidthType::Dxa);
        for (cell, local, region) in [
            (&mut main, &main_style, "main"),
            (&mut side, &side_style, "sidebar"),
        ] {
            for block in basics_blocks(content, local, region) {
                let current = std::mem::replace(cell, TableCell::new());
                *cell = match block {
                    BasicsBlock::Paragraph(p) => current.add_paragraph(*p),
                    BasicsBlock::Table(t) => current.add_table(*t),
                };
            }
        }
        if !columns[0].is_empty() {
            main = main.add_table(lane_table(std::mem::take(&mut columns[0]), main_width));
        }
        main = main.add_paragraph(lane_end());
        if !columns[1].is_empty() {
            side = side.add_table(lane_table(
                std::mem::take(&mut columns[1]),
                side_width.saturating_sub(twips(12.0) as usize),
            ));
        }
        side = side.add_paragraph(lane_end());
        let gap = TableCell::new()
            .width(gap_width, WidthType::Dxa)
            .add_paragraph(Paragraph::new().line_spacing(LineSpacing::new().line(20)));
        let (cells, grid) = if style.composition_preset == "sidebar-left" {
            (
                vec![side, gap, main],
                vec![side_width, gap_width, main_width],
            )
        } else {
            (
                vec![main, gap, side],
                vec![main_width, gap_width, side_width],
            )
        };
        // No cantSplit and no fixed height: both cells remain editable and
        // flow over as many pages as their content requires.
        document = document.add_table(
            Table::without_borders(vec![TableRow::new(cells)])
                .set_grid(grid)
                .layout(TableLayoutType::Fixed)
                .width(total, WidthType::Dxa)
                .margins(TableCellMargins::new().margin(0, 0, 0, 0)),
        );
    } else {
        for p in std::mem::take(&mut columns[0]) {
            document = document.add_paragraph(p);
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

fn add_font_embedding(
    entries: &mut String,
    family: &str,
    weight: u16,
    italic: bool,
    index: usize,
    key: u8,
) -> Result<(), String> {
    let face_tag = match (weight >= 600, italic) {
        (false, false) => "embedRegular",
        (true, false) => "embedBold",
        (false, true) => "embedItalic",
        (true, true) => "embedBoldItalic",
    };
    // One slot cannot represent two different actual faces. No silent dropping.
    if entries.contains(&format!("<w:{face_tag} ")) {
        return Err(format!(
            "字体「{family}」的多个静态字面占用同一 DOCX {face_tag} 槽位；请为这些字面使用独立字族名称"
        ));
    }
    entries.push_str(&format!(r#"<w:{face_tag} r:id="rIdLocal{index}" w:fontKey="{{00000000-0000-0000-0000-0000000000{key:02X}}}" w:subsetted="false"/>"#));
    Ok(())
}

fn pack(mut document: XMLDocx, style: &ThemeStyle) -> Result<Vec<u8>, String> {
    // The bundled OFL fonts permit embedding. Full faces preserve editability
    // for newly typed text; never subset against only the current resume.
    document.font_table = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:fonts xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:font w:name="Noto Sans SC"><w:family w:val="swiss"/><w:embedRegular r:id="rIdSans" w:fontKey="{00000000-0000-0000-0000-000000000001}" w:subsetted="false"/></w:font><w:font w:name="Noto Serif SC"><w:family w:val="roman"/><w:embedBold r:id="rIdSerif" w:fontKey="{00000000-0000-0000-0000-000000000002}" w:subsetted="false"/></w:font></w:fonts>"#.to_vec();
    let mut local_ids = std::collections::BTreeSet::new();
    for role in [
        &style.typography.body,
        &style.typography.item_title,
        &style.typography.name,
        &style.typography.section_title,
        &style.typography.meta,
        &style.typography.label,
        &style.typography.summary,
        &style.typography.side_date,
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
        let entries = family_entries.entry(name).or_default();
        // A slot cannot represent two distinct actual faces. Reject rather
        // than silently discarding a weight; migrated packages can use licensed
        // per-face primary-family identities to preserve all static faces.
        add_font_embedding(entries, &font.family, font.weight, font.italic, index, key)?;
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
    use docx_rs::BuildXML as _;

    #[test]
    fn all_four_embedding_slots_are_distinct_and_colliding_weights_fail_atomically() {
        let mut entries = String::new();
        for (i, (weight, italic, slot)) in [
            (400, false, "embedRegular"),
            (700, false, "embedBold"),
            (400, true, "embedItalic"),
            (700, true, "embedBoldItalic"),
        ]
        .into_iter()
        .enumerate()
        {
            add_font_embedding(&mut entries, "Test", weight, italic, i, i as u8 + 3).unwrap();
            assert!(entries.contains(&format!("<w:{slot} ")));
        }
        for (weight, italic) in [
            (200, false),
            (500, false),
            (600, false),
            (800, false),
            (500, true),
            (800, true),
        ] {
            let before = entries.clone();
            assert!(
                add_font_embedding(&mut entries, "Test", weight, italic, 4, 7)
                    .unwrap_err()
                    .contains("槽位")
            );
            assert_eq!(entries, before);
        }
    }

    #[test]
    fn italic_roles_use_real_embedded_faces_but_never_shear_missing_cjk() {
        let id = crate::fonts::register_asset(
            include_bytes!("../tests/fixtures/theme-italic-font.ttf").to_vec(),
        )
        .unwrap();
        let mut theme =
            crate::parse_theme(include_bytes!("../../../assets/themes/zhibai.theme.json")).unwrap();
        theme.asset_fonts.insert("italic-fixture".into(), id);
        for role in [
            "body",
            "name",
            "sectionTitle",
            "itemTitle",
            "meta",
            "label",
            "summary",
            "sideDate",
        ] {
            theme.normalized["typography"][role] = serde_json::json!({"family":"asset:italic-fixture","weight":400,"fontStyle":"italic"});
        }
        let content=project(&parse_resume("{\"basics\":{\"name\":\"Ada\",\"label\":\"Role 简里 End\",\"summary\":\"Summary\"},\"work\":[{\"name\":\"Company\",\"startDate\":\"2020\"}],\"skills\":[{\"name\":\"Skill\",\"keywords\":[\"Keyword 简里 End\"]}]}".as_bytes()).unwrap());
        for placement in ["left", "right"] {
            theme.normalized["sections"]["work"]["date"] = serde_json::json!(placement);
            theme.normalized["sections"]["skills"]["items"] = serde_json::json!("chips");
            let bytes = render_with_theme(&content, Language::En, &theme).unwrap();
            let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
            let mut xml = String::new();
            std::io::Read::read_to_string(&mut zip.by_name("word/document.xml").unwrap(), &mut xml)
                .unwrap();
            let mut checked_cjk = 0;
            for run in xml.split("</w:r>") {
                if run.contains("简里") {
                    let run = &run[run.rfind("<w:r>").unwrap()..];
                    assert!(run.contains("Noto Sans SC"));
                    assert!(!run.contains("<w:i") && !run.contains("<w:b"));
                    checked_cjk += 1;
                }
            }
            assert!(checked_cjk >= 2);
            for text in ["Ada", "Summary", "Company", "2020", "Keyword"] {
                let at = xml.find(text).unwrap();
                let run = &xml[xml[..at].rfind("<w:r>").unwrap()..at];
                assert!(
                    run.contains("<w:i") && run.contains("Resume One Italic Font Fixture"),
                    "{text}"
                );
            }
            let mut table = String::new();
            std::io::Read::read_to_string(
                &mut zip.by_name("word/fontTable.xml").unwrap(),
                &mut table,
            )
            .unwrap();
            assert!(table.contains("<w:embedItalic "));
            let entry = table
                .split("w:name=\"Resume One Italic Font Fixture\"")
                .nth(1)
                .unwrap()
                .split("</w:font>")
                .next()
                .unwrap();
            assert!(!entry.contains("<w:embedRegular ") && !entry.contains("<w:embedBold "));
        }
    }

    #[test]
    fn independent_label_summary_and_side_date_are_editable_styled_runs() {
        let mut theme =
            crate::parse_theme(include_bytes!("../../../assets/themes/zhibai.theme.json")).unwrap();
        theme.normalized["typography"]["label"] = serde_json::json!({"sizePt":14,"family":"cjk-serif","weight":600,"textTransform":"lowercase"});
        theme.normalized["typography"]["summary"] =
            serde_json::json!({"sizePt":12,"lineHeight":1.8});
        theme.normalized["typography"]["sideDate"] =
            serde_json::json!({"sizePt":11,"family":"cjk-serif","weight":600,"trackingEm":0.02});
        let content=project(&parse_resume(br#"{"basics":{"name":"Ada","label":"ROLE_LABEL","summary":"ROLE_SUMMARY"},"work":[{"name":"Role","startDate":"2020-01"}]}"#).unwrap());
        let xml = String::from_utf8(
            build_document(&content, Language::En, &ThemeStyle::from_manifest(&theme))
                .unwrap()
                .document,
        )
        .unwrap();
        for (text, size) in [("role_label", 28), ("ROLE_SUMMARY", 24), ("2020-01", 22)] {
            let at = xml.find(text).unwrap();
            let start = xml[..at].rfind("<w:r>").unwrap();
            let end = at + xml[at..].find("</w:r>").unwrap();
            let run = &xml[start..end];
            assert!(run.contains(&format!("w:sz w:val=\"{size}\"")), "{text}");
            if text != "ROLE_SUMMARY" {
                assert!(
                    run.contains("Noto Serif SC") && run.contains("<w:b />"),
                    "{text}"
                );
            }
        }
        assert!(!xml.contains("<w:drawing>"));
    }

    #[test]
    fn tracks_and_transforms_editable_text_in_paragraphs_date_tabs_and_chips() {
        let mut theme =
            crate::parse_theme(include_bytes!("../../../assets/themes/zhibai.theme.json")).unwrap();
        for role in ["name", "sectionTitle", "meta"] {
            theme.normalized["typography"][role]["trackingEm"] = serde_json::json!(0.1);
            theme.normalized["typography"][role]["textTransform"] = serde_json::json!("uppercase");
        }
        theme.normalized["typography"]["itemTitle"] = serde_json::json!({"family":"cjk-sans","sizePt":10,"weight":400,"trackingEm":-0.02,"textTransform":"lowercase"});
        theme.normalized["sections"] =
            serde_json::json!({"work":{"date":"right"},"skills":{"items":"chips"}});
        let document = parse_resume(br#"{"basics":{"name":"Ada"},"work":[{"name":"MixedCase","position":"Lead","startDate":"2020-01"}],"skills":[{"name":"Tools","keywords":["Rust"]}]}"#).unwrap();
        let content = project(&document);
        let xml = String::from_utf8(
            build_document(&content, Language::En, &ThemeStyle::from_manifest(&theme))
                .unwrap()
                .document,
        )
        .unwrap();
        for expected in [
            "ADA",
            "WORK",
            "RUST",
            "lead",
            "mixedcase",
            "<w:spacing w:val=\"62\"",
            "<w:spacing w:val=\"-4\"",
        ] {
            assert!(xml.contains(expected), "missing {expected}");
        }
        assert!(!xml.contains("<w:drawing>"));
        assert_eq!(document.value["basics"]["name"], "Ada");
    }

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
    fn cards_timeline_and_keyword_chips_are_native_editable_not_images() {
        let mut theme = crate::parse_theme(include_bytes!(
            "../../resume-one-cli/tests/fixtures/section-styles.theme.json"
        ))
        .unwrap();
        theme.normalized["sections"] = serde_json::json!({
            "work":{"items":"timeline"},"projects":{"items":"cards"},"skills":{"items":"chips"},"languages":{"items":"chips"}
        });
        let content = project(&parse_resume(br#"{"work":[{"name":"WorkTitle","summary":"WorkBody"}],"projects":[{"name":"CardTitle","description":"CardBody"}],"skills":[{"name":"Tools","keywords":["Rust","Final keyword"]}],"languages":[{"language":"English","fluency":"Native"}]}"#).unwrap());
        let package =
            build_document(&content, Language::En, &ThemeStyle::from_manifest(&theme)).unwrap();
        assert!(package.media.is_empty());
        let xml = String::from_utf8(package.document).unwrap();
        for text in [
            "WorkTitle",
            "WorkBody",
            "CardTitle",
            "CardBody",
            "Rust",
            "Final keyword",
            "English",
            "Native",
        ] {
            assert!(xml.contains(text));
        }
        let paragraph = |value: &str| xml.split("</w:p>").find(|p| p.contains(value)).unwrap();
        assert!(paragraph("WorkTitle").contains("<w:left"));
        assert!(paragraph("CardTitle").contains("<w:shd"));
        assert!(paragraph("CardBody").contains("<w:bottom"));
        assert!(paragraph("Final keyword").contains("<w:shd"));
        assert!(paragraph("English").contains("<w:shd"));
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
    fn lane_keep_groups_use_small_rows_not_outer_row_paragraph_keep_next() {
        use docx_rs::{BuildXML, TableChild};
        let table = lane_table(
            vec![
                Paragraph::new().keep_next(true),
                Paragraph::new().keep_next(false),
                Paragraph::new().keep_next(true),
                Paragraph::new().keep_next(false),
            ],
            2400,
        );
        assert_eq!(table.rows.len(), 2);
        for TableChild::TableRow(row) in &table.rows {
            assert!(row.property.cant_split.is_some());
        }
        let xml = String::from_utf8(table.build()).unwrap();
        assert!(!xml.contains("<w:keepNext"));
        assert!(!xml.contains("<w:trHeight"));
        assert_eq!(xml.matches("<w:cantSplit").count(), 2);
    }

    #[test]
    fn sidebar_uses_editable_splittable_cells_with_explicit_lane_widths_and_tint() {
        let mut theme = crate::parse_theme(include_bytes!(
            "../../resume-one-cli/tests/fixtures/sidebar.theme.json"
        ))
        .unwrap();
        let content = project(&parse_resume(br#"{"basics":{"name":"Name"},"work":[{"name":"MainCompany","summary":"MainBody"}],"skills":[{"name":"SideSkill","keywords":["Rust"]}],"languages":[{"language":"SideLanguage"}]}"#).unwrap());
        for direction in ["sidebar-left", "sidebar-right"] {
            theme.normalized["composition"]["preset"] = serde_json::json!(direction);
            let style = ThemeStyle::from_manifest(&theme);
            let package = build_document(&content, Language::En, &style).unwrap();
            let xml = String::from_utf8(package.document).unwrap();
            assert!(xml.contains("<w:tbl>"));
            assert!(xml.contains("w:fill=\"DFE8E3\""));
            assert!(xml.contains("<w:cantSplit")); // small semantic keep groups only
            assert!(!xml.contains("<w:trHeight"));
            assert!(
                xml.contains("MainBody") && xml.contains("Rust") && xml.contains("SideLanguage")
            );
            assert_eq!(
                xml.find("SideSkill").unwrap() < xml.find("MainCompany").unwrap(),
                direction == "sidebar-left"
            );
            assert!(!xml.contains("<w:drawing>"));
        }
    }

    #[test]
    fn rejects_unsupported_theme_instead_of_flattening() {
        let mut theme =
            crate::parse_theme(include_bytes!("../../../assets/themes/zhibai.theme.json")).unwrap();
        theme.normalized["composition"] = serde_json::json!({"preset":"sidebar-left"});
        let content = project(&parse_resume(b"{}").unwrap());
        assert!(
            render_with_theme(&content, Language::Zh, &theme)
                .err()
                .unwrap()
                .contains("composition")
        );
    }

    #[test]
    fn header_summary_owns_the_editable_rule_without_inheriting_center_alignment() {
        let content = project(&parse_resume(br#"{"basics":{"name":"Identity","summary":"Summary body","email":"contact@example.com"}}"#).unwrap());
        let mut theme =
            crate::parse_theme(include_bytes!("../../../assets/themes/zhibai.theme.json")).unwrap();
        for header in ["stacked", "centered", "split"] {
            theme.normalized["composition"] = serde_json::json!({"preset":"single-column", "header":header, "summaryRegion":"header"});
            theme.normalized["layout"] = serde_json::json!("classic");
            let style = ThemeStyle::from_manifest(&theme);
            let blocks = basics_blocks(&content, &style, "page");
            let summary = blocks
                .iter()
                .find_map(|block| match block {
                    BasicsBlock::Paragraph(p) => {
                        let xml = String::from_utf8(p.build()).unwrap();
                        xml.contains("Summary body").then_some(xml)
                    }
                    _ => None,
                })
                .unwrap();
            assert!(!summary.contains("w:val=\"center\""));
            if header != "split" {
                assert!(summary.contains("<w:pBdr>"));
            }
            let xml = String::from_utf8(
                build_document(&content, Language::En, &style)
                    .unwrap()
                    .document,
            )
            .unwrap();
            assert_eq!(xml.matches("Summary body").count(), 1);
        }
    }

    #[test]
    fn lane_local_identity_and_contacts_are_editable_and_never_duplicated() {
        let content = project(&parse_resume(br#"{"basics":{"name":"Identity","summary":"Summary","email":"contact@example.com"},"work":[{"name":"MainBody"}],"skills":[{"name":"SideBody"}]}"#).unwrap());
        let mut theme = crate::parse_theme(include_bytes!(
            "../../resume-one-cli/tests/fixtures/sidebar.theme.json"
        ))
        .unwrap();
        for identity in ["main", "sidebar"] {
            theme.normalized["composition"]["headerRegion"] = serde_json::json!(identity);
            theme.normalized["composition"]["summaryRegion"] = serde_json::json!("main");
            theme.normalized["composition"]["contactRegion"] = serde_json::json!("sidebar");
            let style = ThemeStyle::from_manifest(&theme);
            assert!(basics_blocks(&content, &style, "page").is_empty());
            let xml = String::from_utf8(
                build_document(&content, Language::En, &style)
                    .unwrap()
                    .document,
            )
            .unwrap();
            for value in [
                "Identity",
                "Summary",
                "contact@example.com",
                "MainBody",
                "SideBody",
            ] {
                assert_eq!(xml.matches(value).count(), 1);
            }
            assert!(xml.contains("<w:tbl>"));
            assert!(!xml.contains("<w:drawing>"));
            let identity_before_contacts =
                xml.find("Identity").unwrap() < xml.find("contact@example.com").unwrap();
            assert_eq!(identity_before_contacts, identity == "sidebar"); // left sidebar fixture
        }
    }
}
