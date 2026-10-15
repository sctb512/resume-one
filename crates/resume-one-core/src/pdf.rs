//! PDF encoding of the shared, font-measured resume layout.
use crate::ThemeManifest;
use crate::layout::{
    self, FontKind, LayoutElement, PT_PER_MM, ResumeLayout, SANS_FONT_BYTES, SERIF_FONT_BYTES,
};
use crate::render::Language;
use crate::semantic::ResumeContent;
use crate::theme_style::Rgb;
use printpdf::{
    Color, Line, LinePoint, Mm, Op, PaintMode, ParsedFont, PdfDocument, PdfFontHandle, PdfPage,
    PdfSaveOptions, Point, Polygon, PolygonRing, Pt, Rgb as PdfRgb, TextItem, WindingOrder,
};

fn pdf_color(color: Rgb) -> Color {
    Color::Rgb(PdfRgb::new(
        color.r as f32 / 255.0,
        color.g as f32 / 255.0,
        color.b as f32 / 255.0,
        None,
    ))
}

pub fn render(content: &ResumeContent, language: Language) -> Result<Vec<u8>, String> {
    encode(content, &layout::layout(content, language)?)
}

pub fn render_with_theme(
    content: &ResumeContent,
    language: Language,
    theme: &ThemeManifest,
) -> Result<Vec<u8>, String> {
    encode(
        content,
        &layout::layout_with_theme(content, language, theme)?,
    )
}

fn encode(content: &ResumeContent, layout: &ResumeLayout) -> Result<Vec<u8>, String> {
    let mut font_warnings = Vec::new();
    let sans = ParsedFont::from_bytes(SANS_FONT_BYTES, 0, &mut font_warnings)
        .ok_or_else(|| "内置无衬线字体无法解析".to_owned())?;
    let serif = ParsedFont::from_bytes(SERIF_FONT_BYTES, 0, &mut font_warnings)
        .ok_or_else(|| "内置衬线字体无法解析".to_owned())?;
    let mut document = PdfDocument::new(if content.name.is_empty() {
        "Resume One"
    } else {
        &content.name
    });
    let sans_id = document.add_font(&sans);
    let serif_id = document.add_font(&serif);
    let width_pt = layout.width_mm * PT_PER_MM;
    let height_pt = layout.height_mm * PT_PER_MM;
    let pages = layout
        .pages
        .iter()
        .map(|elements| {
            let mut ops = Vec::new();
            if layout.paper
                != (Rgb {
                    r: 255,
                    g: 255,
                    b: 255,
                })
            {
                ops.extend([
                    Op::SetFillColor {
                        col: pdf_color(layout.paper),
                    },
                    Op::DrawPolygon {
                        polygon: Polygon {
                            rings: vec![PolygonRing {
                                points: [
                                    (0.0, 0.0),
                                    (width_pt, 0.0),
                                    (width_pt, height_pt),
                                    (0.0, height_pt),
                                ]
                                .into_iter()
                                .map(|(x, y)| LinePoint {
                                    p: Point { x: Pt(x), y: Pt(y) },
                                    bezier: false,
                                })
                                .collect(),
                            }],
                            mode: PaintMode::Fill,
                            winding_order: WindingOrder::NonZero,
                        },
                    },
                ]);
            }
            for element in elements {
                match element {
                    LayoutElement::Text(text) => ops.extend([
                        Op::StartTextSection,
                        Op::SetTextCursor {
                            pos: Point {
                                x: Pt(text.x_pt),
                                y: Pt(text.baseline_y_pt),
                            },
                        },
                        Op::SetFont {
                            font: PdfFontHandle::External(match text.font {
                                FontKind::Sans => sans_id.clone(),
                                FontKind::Serif => serif_id.clone(),
                            }),
                            size: Pt(text.size_pt),
                        },
                        Op::SetFillColor {
                            col: pdf_color(text.color),
                        },
                        Op::ShowText {
                            items: vec![TextItem::Text(text.text.clone())],
                        },
                        Op::EndTextSection,
                    ]),
                    LayoutElement::Rule(rule) => ops.extend([
                        Op::SetOutlineColor {
                            col: pdf_color(rule.color),
                        },
                        Op::SetOutlineThickness {
                            pt: Pt(rule.thickness_pt),
                        },
                        Op::DrawLine {
                            line: Line {
                                points: [rule.left_pt, rule.right_pt]
                                    .into_iter()
                                    .map(|x| LinePoint {
                                        p: Point {
                                            x: Pt(x),
                                            y: Pt(rule.y_pt),
                                        },
                                        bezier: false,
                                    })
                                    .collect(),
                                is_closed: false,
                            },
                        },
                    ]),
                    LayoutElement::Anchor(_) => {}
                }
            }
            PdfPage::new(Mm(layout.width_mm), Mm(layout.height_mm), ops)
        })
        .collect();
    let options = PdfSaveOptions {
        subset_fonts: true,
        ..Default::default()
    };
    let mut save_warnings = Vec::new();
    let bytes = document
        .with_pages(pages)
        .save(&options, &mut save_warnings);
    if !bytes.starts_with(b"%PDF-") {
        return Err("PDF 生成失败".into());
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{resume::parse_resume, semantic::project};
    #[test]
    fn generates_pdf_with_embedded_cjk_font_and_pages() {
        let source = br#"{"basics":{"name":"\u674e\u660e","summary":"\u4e2d\u6587\u7b80\u5386"},"work":[{"position":"Engineer","name":"Example","highlights":["Completed the project"]}]}"#;
        let content = project(&parse_resume(source).unwrap());
        let bytes = render(&content, Language::Zh).unwrap();
        assert!(bytes.starts_with(b"%PDF-"));
        assert!(bytes.len() > 10_000);
    }

    #[test]
    fn refuses_to_silently_omit_unsupported_glyphs() {
        let source = br#"{"basics":{"name":"Ada \ud83d\ude80"}}"#;
        let content = project(&parse_resume(source).unwrap());
        assert!(
            render(&content, Language::En)
                .unwrap_err()
                .contains("U+1F680")
        );
    }
}
