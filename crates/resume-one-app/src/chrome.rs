//! Desktop visual tokens taken from the approved v3 workspace, not print tokens.
use crate::i18n::tr;
use gpui::{
    Context, FontWeight, IntoElement, MouseButton, PathBuilder, canvas, div, point, prelude::*, px,
};
use resume_one_core::theme_style::{Colors, Rgb, ThemeStyle};
use std::time::{Duration, Instant};

use crate::ResumeOne;
use crate::color;

pub const TOOLBAR_HEIGHT: f32 = 60.0;
pub const THEME_WIDTH: f32 = 220.0;
pub const MODULE_WIDTH: f32 = 230.0;
pub const THEME_ROW_HEIGHT: f32 = 95.0;
pub const SERIF: &str = "Noto Serif SC";
pub const SANS: &str = "Noto Sans SC";

/// Only screen chrome is interpolated. Paper layout and export tokens always
/// come straight from the verified theme.
pub(super) struct Shell {
    pub style: ThemeStyle,
    from: Colors,
    target: Colors,
    started: Option<Instant>,
}

fn color_key(c: Colors) -> [Rgb; 9] {
    [
        c.accent, c.seal, c.paper, c.canvas, c.ink, c.muted, c.border, c.sidebar, c.chrome,
    ]
}

impl Shell {
    pub fn new(style: &ThemeStyle) -> Self {
        Self {
            style: style.clone(),
            from: style.colors,
            target: style.colors,
            started: None,
        }
    }

    pub fn advance(&mut self, target: Colors, now: Instant, reduced: bool) -> bool {
        if reduced {
            self.style.colors = target;
            self.target = target;
            self.started = None;
            return false;
        }
        // Sample before retargeting: scanning adjacent themes never jumps back
        // to the previous theme's endpoint.
        self.sample(now);
        if color_key(target) != color_key(self.target) {
            self.from = self.style.colors;
            self.target = target;
            self.started = Some(now);
        }
        self.started.is_some()
    }

    fn sample(&mut self, now: Instant) {
        let Some(started) = self.started else { return };
        let t = (now.saturating_duration_since(started).as_secs_f32()
            / Duration::from_millis(160).as_secs_f32())
        .clamp(0., 1.);
        let t = t * t * (3. - 2. * t);
        let a = self.from;
        let b = self.target;
        self.style.colors = Colors {
            accent: a.accent.mix(b.accent, t),
            seal: a.seal.mix(b.seal, t),
            paper: a.paper.mix(b.paper, t),
            canvas: a.canvas.mix(b.canvas, t),
            ink: a.ink.mix(b.ink, t),
            muted: a.muted.mix(b.muted, t),
            border: a.border.mix(b.border, t),
            sidebar: a.sidebar.mix(b.sidebar, t),
            chrome: a.chrome.mix(b.chrome, t),
        };
        if t >= 1. {
            self.started = None;
        }
    }
}

pub(super) fn activation_key(event: &gpui::KeyDownEvent) -> bool {
    matches!(event.keystroke.key.as_str(), "enter" | "space")
        && !event.keystroke.modifiers.modified()
}

#[derive(Clone, Copy)]
pub struct Palette {
    pub panel: Rgb,
    pub line: Rgb,
    pub soft: Rgb,
    pub ink: Rgb,
    pub muted: Rgb,
    pub accent: Rgb,
}

/// Preserve the theme hue when possible; black/white is the guaranteed fallback.
pub(super) fn readable_on(candidate: Rgb, background: Rgb) -> Rgb {
    if candidate.contrast(background) >= 4.5 {
        return candidate;
    }
    let black = Rgb { r: 0, g: 0, b: 0 };
    let white = Rgb {
        r: 255,
        g: 255,
        b: 255,
    };
    let fallback = if black.contrast(background) >= white.contrast(background) {
        black
    } else {
        white
    };
    for step in 1..=20 {
        let shade = candidate.mix(fallback, step as f32 / 20.);
        if shade.contrast(background) >= 4.5 {
            return shade;
        }
    }
    fallback
}

pub(super) fn theme_description(description: &str) -> String {
    // These are captions, not prose paragraphs. A terminal full stop must not
    // become a detached second line in a narrow rail.
    description.trim().trim_end_matches(['。', '.']).to_owned()
}

impl Palette {
    pub fn new(style: &ThemeStyle) -> Self {
        let c = style.colors;
        let panel = c.canvas.mix(c.paper, 0.72);
        let readable = |candidate: Rgb| readable_on(candidate, panel);
        Self {
            panel,
            line: panel.mix(c.ink, 0.13),
            soft: panel.mix(c.accent, 0.09),
            ink: readable(c.ink),
            muted: readable(c.muted),
            accent: readable(c.accent),
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
        let style = self.shell_style();
        let p = Palette::new(style);
        let compact = self.layout == crate::workspace_ui::Layout::Compact;
        let export_ink = readable_on(style.colors.paper, style.colors.accent);
        let dirty = self.editor.as_ref().is_some_and(|e| e.buffer.is_dirty());
        let status = if self.editor.as_ref().is_some_and(|e| e.saving) {
            tr("保存中…")
        } else if self.model.document_path.is_none() && self.model.is_example && !dirty {
            tr("示例预览")
        } else if dirty || self.model.document_path.is_none() {
            tr("未保存")
        } else {
            tr("已保存")
        };
        let filename = if self.model.is_example {
            tr("示例简历")
        } else {
            &self.model.document_label
        };
        div()
            .id("toolbar")
            .debug_selector(|| "toolbar".into())
            .w_full()
            .h(px(TOOLBAR_HEIGHT))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(if compact { 8. } else { 18. }))
            .px(px(if compact { 16. } else { 26. }))
            .when(cfg!(target_os = "macos"), |v| v.pl(px(90.)))
            .border_b_1()
            .border_color(color(p.line))
            .bg(color(p.panel))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &gpui::MouseDownEvent, window, cx| {
                    this.files.open = false;
                    this.export_menu_open = false;
                    if cfg!(target_os = "macos") {
                        if event.click_count == 2 {
                            window.titlebar_double_click();
                        } else {
                            this.window_drag_pending = true;
                        }
                    }
                    cx.notify();
                }),
            )
            .on_mouse_down_out(cx.listener(|this, _, _, _| this.window_drag_pending = false))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.window_drag_pending = false),
            )
            .on_mouse_move(cx.listener(|this, _, window, _| {
                if this.window_drag_pending {
                    this.window_drag_pending = false;
                    window.start_window_move();
                }
            }))
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
                            .text_color(color(readable_on(style.colors.paper, style.colors.seal)))
                            .child("简"),
                    )
                    .when(!compact, |brand| {
                        brand.child(
                            div()
                                .font_family(SERIF)
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_size(px(18.))
                                .text_color(color(p.accent))
                                .child("Resume One"),
                        )
                    })
                    .child(
                        div()
                            .font_family(SERIF)
                            .text_size(px(15.))
                            .text_color(color(p.accent))
                            .child("简里"),
                    ),
            )
            .when(!self.source_mode && !self.layout.theme_docked(), |v| {
                v.child(self.drawer_button(crate::workspace_ui::Drawer::Themes, cx))
            })
            .child(
                div()
                    .id("current-file")
                    .tab_index(0)
                    .focus(|v| v.bg(color(p.soft)))
                    .on_key_down(cx.listener(|this, event, window, cx| {
                        if activation_key(event) {
                            this.export_menu_open = false;
                            this.toggle_files(&crate::files::ToggleFiles, window, cx);
                            cx.stop_propagation();
                        }
                    }))
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
                            .when(!compact, |v| v.child(status)),
                    ),
            )
            .when(!self.layout.modules_docked(), |v| {
                v.child(self.drawer_button(crate::workspace_ui::Drawer::Modules, cx))
            })
            .child(
                div()
                    .id("open-menu")
                    .tab_index(0)
                    .focus(|v| v.border_color(color(p.accent)).bg(color(p.soft)))
                    .on_key_down(cx.listener(|this, event, window, cx| {
                        if activation_key(event) {
                            this.export_menu_open = false;
                            this.toggle_open_menu(window, cx);
                            cx.stop_propagation();
                        }
                    }))
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
                        tr("打开中…")
                    } else {
                        tr("打开")
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
                    .tab_index(0)
                    .border_1()
                    .border_color(color(style.colors.accent))
                    .focus(|v| v.border_color(color(export_ink)))
                    .on_key_down(cx.listener(|this, event, window, cx| {
                        if activation_key(event) {
                            this.close_drawer();
                            this.files.open = false;
                            this.export_menu_open = !this.export_menu_open;
                            this.export_choice = 0;
                            this.clear_inline();
                            window.focus(&this.focus);
                            cx.stop_propagation();
                            cx.notify();
                        }
                    }))
                    .debug_selector(|| "export-menu".into())
                    .h(px(36.))
                    .px_3()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded(px(6.))
                    .bg(color(style.colors.accent))
                    .text_color(color(export_ink))
                    .cursor_pointer()
                    .text_size(px(13.))
                    .hover(|v| v.bg(color(style.colors.accent.mix(style.colors.ink, 0.12))))
                    .child(icon("export", export_ink))
                    .child(if self.export_busy {
                        tr("导出中…")
                    } else {
                        tr("导出")
                    })
                    .child(icon("chevron", export_ink))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.close_drawer();
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
    fn shell_retargets_from_visible_colors_and_keeps_intermediate_text_readable() {
        let model = crate::model::AppModel::load(None).unwrap();
        let light = &model.themes[0].style;
        let dark = model
            .themes
            .iter()
            .find(|t| t.manifest.id == "yehe")
            .unwrap();
        let now = Instant::now();
        let mut shell = Shell::new(light);
        assert!(shell.advance(dark.style.colors, now, false));
        assert_eq!(color_key(shell.style.colors), color_key(light.colors));
        for frame in 1..=8 {
            shell.advance(
                dark.style.colors,
                now + Duration::from_millis(frame * 10),
                false,
            );
            let p = Palette::new(&shell.style);
            for text in [p.ink, p.muted, p.accent] {
                assert!(text.contrast(p.panel) >= 4.5);
            }
        }
        let middle = color_key(shell.style.colors);
        shell.advance(light.colors, now + Duration::from_millis(80), false);
        assert_eq!(color_key(shell.style.colors), middle);
        assert!(!shell.advance(light.colors, now + Duration::from_millis(240), false));
        assert_eq!(color_key(shell.style.colors), color_key(light.colors));
        assert!(!shell.advance(dark.style.colors, now, true));
        assert_eq!(color_key(shell.style.colors), color_key(dark.style.colors));
    }
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

    #[test]
    fn custom_middle_tones_have_safe_text_fallbacks() {
        for gray in (0..=255).step_by(5) {
            let background = Rgb {
                r: gray,
                g: gray,
                b: gray,
            };
            for candidate in [
                background,
                Rgb {
                    r: 120,
                    g: 150,
                    b: 138,
                },
                Rgb {
                    r: 190,
                    g: 77,
                    b: 61,
                },
            ] {
                assert!(readable_on(candidate, background).contrast(background) >= 4.5);
            }
        }
        assert_eq!(theme_description("克制的行距。"), "克制的行距");
        assert_eq!(theme_description("Quiet typography."), "Quiet typography");
    }
}
