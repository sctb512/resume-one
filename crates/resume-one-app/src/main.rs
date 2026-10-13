#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod model;

use std::path::Path;

use gpui::{
    App, Application, Bounds, Context, FontWeight, IntoElement, Render, Window, WindowBounds,
    WindowOptions, div, prelude::*, px, rgb, size,
};
use model::AppModel;
use resume_one_core::semantic::{Item, Section};
use resume_one_core::theme_style::{Rgb, ThemeStyle};

const SECTION_LABELS: &[(&str, &str)] = &[
    ("basics", "基本信息"),
    ("work", "工作经历"),
    ("volunteer", "志愿经历"),
    ("education", "教育经历"),
    ("awards", "荣誉奖项"),
    ("certificates", "证书"),
    ("publications", "出版物"),
    ("skills", "技能"),
    ("languages", "语言"),
    ("interests", "兴趣"),
    ("references", "推荐人"),
    ("projects", "项目经历"),
];

fn color(value: Rgb) -> gpui::Rgba {
    rgb(u32::from(value.r) << 16 | u32::from(value.g) << 8 | u32::from(value.b))
}

fn label(id: &str) -> &str {
    SECTION_LABELS
        .iter()
        .find(|(key, _)| *key == id)
        .map_or(id, |(_, label)| label)
}

struct ResumeOne {
    model: AppModel,
    hovered_theme: Option<usize>,
}

impl ResumeOne {
    fn active_style(&self) -> &ThemeStyle {
        self.hovered_theme
            .and_then(|index| self.model.themes.get(index))
            .map_or_else(|| self.model.selected_style(), |theme| &theme.style)
    }

    fn theme_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let accent = color(self.active_style().colors.accent);
        let mut list = div()
            .id("theme-list")
            .flex()
            .flex_col()
            .gap_1()
            .overflow_y_scroll()
            .flex_1()
            .on_scroll_wheel(cx.listener(|this, _, _, cx| {
                if this.hovered_theme.take().is_some() {
                    cx.notify();
                }
            }));
        for (index, theme) in self.model.themes.iter().enumerate() {
            let selected = index == self.model.selected_theme;
            let row = div()
                .id(("theme", index))
                .debug_selector(|| format!("theme-{index}"))
                .flex()
                .items_center()
                .gap_3()
                .px_3()
                .py_3()
                .rounded_md()
                .cursor_pointer()
                .when(selected, |row| row.bg(rgb(0xffffff)))
                .hover(|row| row.bg(rgb(0xffffff)))
                .child(
                    div()
                        .w(px(5.0))
                        .h(px(34.0))
                        .rounded_sm()
                        .bg(color(theme.style.colors.accent)),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .text_size(px(13.0))
                                .text_color(if selected { accent } else { rgb(0x333933) })
                                .child(theme.manifest.name.clone()),
                        )
                        .child(
                            div()
                                .text_size(px(10.0))
                                .text_color(rgb(0x8e958e))
                                .child(theme.manifest.id.clone()),
                        ),
                )
                .on_hover(cx.listener(move |this, hovering: &bool, _, cx| {
                    let next = if *hovering {
                        Some(index)
                    } else if this.hovered_theme == Some(index) {
                        None
                    } else {
                        this.hovered_theme
                    };
                    if this.hovered_theme != next {
                        this.hovered_theme = next;
                        cx.notify();
                    }
                }))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.model.select_theme(index);
                    cx.notify();
                }));
            list = list.child(row);
        }
        div()
            .id("theme-panel")
            .debug_selector(|| "theme-panel".to_string())
            .w(px(206.0))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(rgb(0xe4e7e1))
            .bg(rgb(0xf6f7f3))
            .px_3()
            .pt_5()
            .pb_4()
            .gap_4()
            .child(
                div()
                    .px_2()
                    .flex()
                    .justify_between()
                    .text_size(px(11.0))
                    .text_color(rgb(0x758078))
                    .child("主题")
                    .child("内置 · 20"),
            )
            .child(list)
    }

    fn item(item: &Item, style: &ThemeStyle) -> impl IntoElement {
        let mut view = div().flex().flex_col().gap_1().mb_3();
        if !item.heading.is_empty() {
            view = view.child(
                div()
                    .text_size(px(style.typography.body.size_pt * 1.3))
                    .text_color(color(style.colors.ink))
                    .child(item.heading.clone()),
            );
        }
        for detail in &item.details {
            view = view.child(
                div()
                    .text_size(px(style.typography.meta.size_pt * 1.3))
                    .text_color(color(style.colors.muted))
                    .child(detail.clone()),
            );
        }
        for paragraph in &item.paragraphs {
            view = view.child(
                div()
                    .text_size(px(style.typography.body.size_pt * 1.3))
                    .line_height(px(style.typography.body.size_pt
                        * 1.3
                        * style.typography.body.line_height))
                    .text_color(color(style.colors.ink))
                    .child(paragraph.clone()),
            );
        }
        for bullet in &item.bullets {
            view = view.child(
                div()
                    .text_size(px(style.typography.body.size_pt * 1.3))
                    .text_color(color(style.colors.ink))
                    .child(format!("· {bullet}")),
            );
        }
        view
    }

    fn section(section: &Section, style: &ThemeStyle) -> impl IntoElement {
        let mut view = div().flex().flex_col().mt_5().gap_3().child(
            div()
                .pb_2()
                .border_b_1()
                .border_color(color(style.colors.border))
                .text_size(px(style.typography.section_title.size_pt * 1.3))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(color(style.colors.accent))
                .child(label(section.id).to_string()),
        );
        for item in &section.items {
            view = view.child(Self::item(item, style));
        }
        view
    }

    fn resume_page(&self) -> impl IntoElement {
        let style = self.active_style();
        let content = &self.model.content;
        let mm = 3.0;
        let mut page = div()
            .id("resume-paper")
            .debug_selector(|| "resume-paper".to_string())
            .w(px(style.page.width_mm * mm))
            .min_h(px(style.page.height_mm * mm))
            .flex_none()
            .flex()
            .flex_col()
            .bg(color(style.colors.paper))
            .shadow_lg()
            .pt(px(style.page.margins.top * mm))
            .pr(px(style.page.margins.right * mm))
            .pb(px(style.page.margins.bottom * mm))
            .pl(px(style.page.margins.left * mm))
            .child(
                div()
                    .text_size(px(style.typography.name.size_pt * 1.3))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(color(style.colors.accent))
                    .child(content.name.clone()),
            )
            .child(
                div()
                    .mt_2()
                    .text_size(px(style.typography.body.size_pt * 1.3))
                    .text_color(color(style.colors.muted))
                    .child(content.label.clone()),
            );
        if !content.contacts.is_empty() {
            page = page.child(
                div()
                    .mt_3()
                    .text_size(px(style.typography.meta.size_pt * 1.3))
                    .text_color(color(style.colors.muted))
                    .child(content.contacts.join("  ·  ")),
            );
        }
        if !content.summary.is_empty() {
            page = page.child(
                div()
                    .mt_6()
                    .text_size(px(style.typography.body.size_pt * 1.3))
                    .line_height(px(style.typography.body.size_pt
                        * 1.3
                        * style.typography.body.line_height))
                    .text_color(color(style.colors.ink))
                    .child(content.summary.clone()),
            );
        }
        for section in &content.sections {
            page = page.child(Self::section(section, style));
        }
        page
    }

    fn module_sidebar(&self) -> impl IntoElement {
        let accent = color(self.active_style().colors.accent);
        let mut list = div()
            .id("module-list")
            .flex()
            .flex_col()
            .gap_2()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll();
        let sections = self.model.document.sections();
        for (id, title) in SECTION_LABELS {
            let count = sections
                .iter()
                .find(|section| section.id == *id)
                .map_or(0, |section| section.count);
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .bg(rgb(0xffffff))
                    .text_size(px(12.0))
                    .text_color(rgb(0x464e47))
                    .child((*title).to_string())
                    .child(
                        div()
                            .text_size(px(10.0))
                            .text_color(accent)
                            .child(count.to_string()),
                    ),
            );
        }
        div()
            .id("module-panel")
            .debug_selector(|| "module-panel".to_string())
            .w(px(188.0))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .border_l_1()
            .border_color(rgb(0xe4e7e1))
            .bg(rgb(0xf7f8f5))
            .px_3()
            .pt_5()
            .pb_4()
            .gap_4()
            .child(
                div()
                    .px_2()
                    .text_size(px(11.0))
                    .text_color(rgb(0x758078))
                    .child("内容模块"),
            )
            .child(list)
    }
}

impl Render for ResumeOne {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let style = self.active_style();
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(0xf5f5f1))
            .child(
                div()
                    .h(px(55.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_6()
                    .border_b_1()
                    .border_color(rgb(0xe4e7e1))
                    .bg(color(style.colors.chrome))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_4()
                            .child(
                                div()
                                    .text_size(px(15.0))
                                    .text_color(color(style.colors.accent))
                                    .child("简里  resume one"),
                            )
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .text_color(color(style.colors.muted))
                                    .child(self.model.document_label.clone()),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(self.theme_sidebar(cx))
                    .child(
                        div()
                            .id("resume-preview")
                            .debug_selector(|| "resume-preview".to_string())
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .overflow_scroll()
                            .bg(color(style.colors.canvas))
                            .child(
                                div()
                                    .w_full()
                                    .min_w(px(style.page.width_mm * 3.0 + 64.0))
                                    .py_8()
                                    .flex()
                                    .justify_center()
                                    .child(self.resume_page()),
                            ),
                    )
                    .child(self.module_sidebar()),
            )
    }
}

fn main() {
    let path = std::env::args_os().nth(1);
    let model = match AppModel::load(path.as_deref().map(Path::new)) {
        Ok(model) => model,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(1);
        }
    };
    Application::new().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1120.0), px(780.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(800.0), px(600.0))),
                ..Default::default()
            },
            |window, cx| {
                window.set_window_title("Resume One · 简里");
                cx.new(|_| ResumeOne {
                    model,
                    hovered_theme: None,
                })
            },
        )
        .expect("failed to open Resume One window");
        cx.activate(true);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Modifiers, ScrollDelta, ScrollWheelEvent, TestAppContext, point};

    #[gpui::test]
    fn preview_window_renders_all_three_panes(cx: &mut TestAppContext) {
        let model = AppModel::load(None).unwrap();
        let (view, cx) = cx.add_window_view(|_, _| ResumeOne {
            model,
            hovered_theme: None,
        });
        cx.update(|window, _| window.refresh());
        let theme = cx.debug_bounds("theme-panel").unwrap();
        let preview = cx.debug_bounds("resume-preview").unwrap();
        let module = cx.debug_bounds("module-panel").unwrap();
        assert!(theme.right() <= preview.left());
        assert!(preview.right() <= module.left());
        let second = cx.debug_bounds("theme-1").unwrap();
        let location = point(
            second.origin.x + second.size.width / 2.0,
            second.origin.y + second.size.height / 2.0,
        );
        cx.simulate_mouse_move(location, None, Modifiers::none());
        cx.update(|_, cx| {
            let app = view.read(cx);
            assert_eq!(app.model.themes.len(), 20);
            assert_eq!(app.model.content.name, "林清和");
            assert_eq!(app.model.content.sections.len(), 3);
            assert_eq!(app.hovered_theme, Some(1));
            assert_eq!(app.model.selected_theme, 0);
        });
        let module_after_hover = cx.debug_bounds("module-panel").unwrap();
        assert_eq!(module, module_after_hover);
        cx.simulate_click(location, Modifiers::none());
        cx.update(|_, cx| {
            let app = view.read(cx);
            assert_eq!(app.model.selected_theme, 1);
        });
        let module_after_select = cx.debug_bounds("module-panel").unwrap();
        assert_eq!(module, module_after_select);
        cx.simulate_event(ScrollWheelEvent {
            position: location,
            delta: ScrollDelta::Pixels(point(px(0.0), px(-80.0))),
            ..Default::default()
        });
        cx.update(|_, cx| {
            assert_eq!(view.read(cx).hovered_theme, None);
            assert_eq!(view.read(cx).model.selected_theme, 1);
        });
        cx.simulate_mouse_move(
            point(
                preview.origin.x + preview.size.width / 2.0,
                preview.origin.y + preview.size.height / 2.0,
            ),
            None,
            Modifiers::none(),
        );
        cx.update(|_, cx| {
            let app = view.read(cx);
            assert_eq!(app.hovered_theme, None);
            assert_eq!(app.model.selected_theme, 1);
        });
        for index in 0..20 {
            view.update(cx, |app, cx| {
                app.model.select_theme(index);
                cx.notify();
            });
            cx.update(|window, _| window.refresh());
            let paper = cx.debug_bounds("resume-paper").unwrap();
            cx.update(|_, cx| {
                let app = view.read(cx);
                let expected = px(app.active_style().page.width_mm * 3.0);
                assert!(f32::from(paper.size.width - expected).abs() <= 1.0);
            });
            assert_eq!(module, cx.debug_bounds("module-panel").unwrap());
        }
        for width in [800.0, 1120.0, 1440.0] {
            cx.simulate_resize(size(px(width), px(600.0)));
            cx.update(|window, _| window.refresh());
            let preview = cx.debug_bounds("resume-preview").unwrap();
            let paper = cx.debug_bounds("resume-paper").unwrap();
            let modules = cx.debug_bounds("module-panel").unwrap();
            assert_eq!(modules.size.width, px(188.0));
            assert_eq!(modules.right(), px(width));
            assert!(paper.left() >= preview.left());
        }
    }
}
