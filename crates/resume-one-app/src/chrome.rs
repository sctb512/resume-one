//! Desktop visual tokens taken from the approved v3 workspace, not print tokens.
use gpui::{
    Context, FontWeight, IntoElement, MouseButton, PathBuilder, canvas, div, point, prelude::*, px,
};
use resume_one_core::theme_style::{Rgb, ThemeStyle};

use crate::ResumeOne;
use crate::color;

pub const TOOLBAR_HEIGHT: f32 = 60.0;
pub const THEME_WIDTH: f32 = 220.0;
pub const MODULE_WIDTH: f32 = 230.0;
pub const THEME_ROW_HEIGHT: f32 = 95.0;
pub const SERIF: &str = "Noto Serif SC";
pub const SANS: &str = "Noto Sans SC";

pub struct Palette {
    pub panel: Rgb,
    pub line: Rgb,
    pub soft: Rgb,
    pub ink: Rgb,
    pub muted: Rgb,
}

impl Palette {
    pub fn new(style: &ThemeStyle) -> Self {
        let c = style.colors;
        let panel = c.canvas.mix(c.paper, 0.72);
        let readable = |candidate: Rgb| {
            if candidate.contrast(panel) >= 4.5 {
                candidate
            } else {
                c.ink
            }
        };
        Self {
            panel,
            line: panel.mix(c.ink, 0.13),
            soft: panel.mix(c.accent, 0.09),
            ink: readable(c.ink),
            muted: readable(c.muted),
        }
    }
}

/// Small original line icons. No remote artwork, platform glyphs or SVG runtime.
pub fn icon(kind: &'static str, ink: Rgb) -> impl IntoElement {
    let paths: &[&[(f32, f32)]] = match kind {
        "basics" => &[
            &[
                (8., 3.),
                (12., 3.),
                (14., 5.),
                (14., 8.),
                (12., 10.),
                (8., 10.),
                (6., 8.),
                (6., 5.),
                (8., 3.),
            ],
            &[
                (3., 19.),
                (3., 16.),
                (6., 13.),
                (14., 13.),
                (17., 16.),
                (17., 19.),
            ],
        ],
        "work" | "projects" => &[
            &[(2., 7.), (18., 7.), (18., 18.), (2., 18.), (2., 7.)],
            &[(7., 7.), (7., 3.), (13., 3.), (13., 7.)],
            &[(2., 11.), (18., 11.)],
            &[(9., 10.), (11., 10.), (11., 13.), (9., 13.), (9., 10.)],
        ],
        "education" => &[
            &[(1., 7.), (10., 3.), (19., 7.), (10., 11.), (1., 7.)],
            &[(5., 10.), (5., 15.), (10., 18.), (15., 15.), (15., 10.)],
            &[(19., 7.), (19., 15.)],
        ],
        "skills" | "languages" => &[
            &[(4., 17.), (4., 10.)],
            &[(10., 17.), (10., 3.)],
            &[(16., 17.), (16., 7.)],
        ],
        "awards" => &[
            &[
                (5., 3.),
                (15., 3.),
                (15., 10.),
                (12., 13.),
                (8., 13.),
                (5., 10.),
                (5., 3.),
            ],
            &[(5., 5.), (2., 5.), (2., 9.), (5., 11.)],
            &[(15., 5.), (18., 5.), (18., 9.), (15., 11.)],
            &[(10., 13.), (10., 18.)],
            &[(6., 18.), (14., 18.)],
        ],
        "open" => &[&[
            (2., 5.),
            (8., 5.),
            (10., 7.),
            (18., 7.),
            (18., 17.),
            (2., 17.),
            (2., 5.),
        ]],
        "export" => &[
            &[(3., 10.), (3., 18.), (17., 18.), (17., 10.)],
            &[(10., 14.), (10., 2.)],
            &[(6., 6.), (10., 2.), (14., 6.)],
        ],
        "chevron" => &[&[(6., 8.), (10., 12.), (14., 8.)]],
        "volunteer" | "interests" => &[&[
            (10., 17.),
            (2., 9.),
            (2., 5.),
            (5., 3.),
            (8., 3.),
            (10., 6.),
            (12., 3.),
            (15., 3.),
            (18., 5.),
            (18., 9.),
            (10., 17.),
        ]],
        _ => &[
            &[
                (4., 2.),
                (12., 2.),
                (17., 7.),
                (17., 18.),
                (4., 18.),
                (4., 2.),
            ],
            &[(12., 2.), (12., 7.), (17., 7.)],
            &[(7., 11.), (14., 11.)],
            &[(7., 14.), (13., 14.)],
        ],
    };
    div().size(px(18.)).flex_none().child(
        canvas(
            move |bounds, _, _| bounds,
            move |_, bounds, window, _| {
                let mut builder = PathBuilder::stroke(px(1.15));
                for points in paths {
                    for (index, &(x, y)) in points.iter().enumerate() {
                        let p = bounds.origin + point(px(x * 0.9), px(y * 0.9));
                        if index == 0 {
                            builder.move_to(p);
                        } else {
                            builder.line_to(p);
                        }
                    }
                }
                if let Ok(path) = builder.build() {
                    window.paint_path(path, color(ink));
                }
            },
        )
        .size_full(),
    )
}

impl ResumeOne {
    pub(super) fn toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let style = self.active_style();
        let p = Palette::new(style);
        let dirty = self.editor.as_ref().is_some_and(|e| e.buffer.is_dirty());
        let status = if self.editor.as_ref().is_some_and(|e| e.saving) {
            "保存中…"
        } else if self.model.document_path.is_none() {
            "示例预览 · 未保存"
        } else if dirty {
            "未保存"
        } else {
            "已保存"
        };
        let filename = if self.model.document_path.is_none() {
            "示例简历"
        } else {
            &self.model.document_label
        };
        div()
            .id("toolbar")
            .debug_selector(|| "toolbar".into())
            .h(px(TOOLBAR_HEIGHT))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(18.))
            .px(px(26.))
            .border_b_1()
            .border_color(color(p.line))
            .bg(color(p.panel))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.files.open = false;
                    this.export_menu_open = false;
                    cx.notify();
                }),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(9.))
                    .child(
                        div()
                            .size(px(28.))
                            .rounded(px(3.))
                            .bg(color(style.colors.seal))
                            .flex()
                            .items_center()
                            .justify_center()
                            .font_family(SERIF)
                            .text_size(px(17.))
                            .text_color(color(
                                if style.colors.paper.contrast(style.colors.seal) >= 4.5 {
                                    style.colors.paper
                                } else {
                                    style.colors.ink
                                },
                            ))
                            .child("简"),
                    )
                    .child(
                        div()
                            .font_family(SERIF)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_size(px(18.))
                            .text_color(color(style.colors.accent))
                            .child("Resume One"),
                    )
                    .child(
                        div()
                            .font_family(SERIF)
                            .text_size(px(15.))
                            .text_color(color(style.colors.accent))
                            .child("简里"),
                    ),
            )
            .child(div().h(px(25.)).w(px(1.)).bg(color(p.line)))
            .child(
                div()
                    .id("current-file")
                    .debug_selector(|| "current-file".into())
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap_3()
                    .cursor_pointer()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.export_menu_open = false;
                        this.toggle_files(&crate::files::ToggleFiles, window, cx)
                    }))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .font_weight(FontWeight::MEDIUM)
                            .text_size(px(13.))
                            .child(filename.to_owned()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_size(px(12.))
                            .text_color(color(p.muted))
                            .child(div().size(px(5.)).rounded_full().bg(color(if dirty {
                                style.colors.seal
                            } else {
                                style.colors.accent
                            })))
                            .child(status),
                    ),
            )
            .child(
                div()
                    .id("open-menu")
                    .debug_selector(|| "open-menu".into())
                    .h(px(36.))
                    .px_3()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(color(p.line))
                    .cursor_pointer()
                    .text_size(px(13.))
                    .hover(|v| v.bg(color(p.soft)))
                    .child(icon("open", p.ink))
                    .child(if self.files.busy {
                        "打开中…"
                    } else {
                        "打开"
                    })
                    .child(icon("chevron", p.muted))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.export_menu_open = false;
                        this.toggle_open_menu(window, cx);
                    })),
            )
            .child(
                div()
                    .id("export-menu")
                    .debug_selector(|| "export-menu".into())
                    .h(px(36.))
                    .px_3()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded(px(6.))
                    .bg(color(style.colors.accent))
                    .text_color(color(style.colors.paper))
                    .cursor_pointer()
                    .text_size(px(13.))
                    .hover(|v| v.bg(color(style.colors.accent.mix(style.colors.ink, 0.12))))
                    .child(icon("export", style.colors.paper))
                    .child(if self.export_busy {
                        "导出中…"
                    } else {
                        "导出"
                    })
                    .child(icon("chevron", style.colors.paper))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.files.open = false;
                        this.export_menu_open = !this.export_menu_open;
                        this.export_choice = 0;
                        this.clear_inline();
                        window.focus(&this.focus);
                        cx.notify();
                    })),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn panel_text_remains_readable_in_light_and_dark_themes() {
        let mut model = crate::model::AppModel::load(None).unwrap();
        model.themes[0].manifest.normalized["colors"]["paper"] = serde_json::json!("#172426");
        model.themes[0].manifest.normalized["colors"]["canvas"] = serde_json::json!("#101A1B");
        model.themes[0].style = ThemeStyle::from_manifest(&model.themes[0].manifest);
        for theme in &model.themes {
            let p = Palette::new(&theme.style);
            assert!(p.ink.contrast(p.panel) >= 4.5);
            assert!(p.muted.contrast(p.panel) >= 4.5);
        }
    }
}
