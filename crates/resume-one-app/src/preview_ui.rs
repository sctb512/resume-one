use std::{borrow::Cow, cell::RefCell, sync::Arc};

use gpui::{
    App, Bounds, Context, Font, FontWeight, IntoElement, Pixels, Point, Rgba, ShapedLine, TextRun,
    canvas, div, fill, point, prelude::*, px, size,
};
use resume_one_core::{
    layout::{self, FontKind, LayoutElement, LayoutTarget, PT_PER_MM, ResumeLayout},
    render::Language,
    structure::StructureEdit,
};

use crate::{
    ResumeOne, SECTION_LABELS, color, inline_ui,
    model::{AppModel, BuiltinTheme},
};

const PX_PER_MM: f32 = 3.0;
const PX_PER_PT: f32 = PX_PER_MM / PT_PER_MM;

pub(super) struct PreviewState {
    cache: RefCell<Option<Cache>>,
    font_error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{TestAppContext, px};
    use resume_one_core::{resume::parse_resume, semantic::project};

    #[test]
    fn hit_regions_preserve_empty_item_indices_and_exclude_page_footers() {
        let document =
            parse_resume(br#"{"basics":{"name":"A"},"work":[{}, {"name":"B"}]}"#).unwrap();
        let result = layout::layout(&project(&document), Language::Zh).unwrap();
        let regions = regions(&result);
        for index in 0..2 {
            assert!(regions.iter().any(|region| region.target
                == LayoutTarget {
                    section: "work",
                    index: Some(index),
                }));
        }
        let empty = regions
            .iter()
            .find(|region| region.target.index == Some(0))
            .unwrap();
        assert!(!empty.text);
        assert!(empty.bottom > empty.top);
        let bottom = result.height_mm * PT_PER_MM;
        assert!(
            regions
                .iter()
                .all(|region| region.top >= 0.0 && region.bottom < bottom - 20.0)
        );
        assert!(result.pages.iter().all(
            |page| matches!(page.last(), Some(LayoutElement::Text(text)) if text.target.is_none())
        ));
    }

    #[gpui::test]
    fn preview_caches_layout_and_invalidates_document_and_theme_reloads(cx: &mut TestAppContext) {
        let mut model = AppModel::load(None).unwrap();
        let preview = cx.update(PreviewState::new);
        let first = preview.resolve(&model, &model.themes[0]).unwrap();
        assert!(Arc::ptr_eq(
            &first,
            &preview.resolve(&model, &model.themes[0]).unwrap()
        ));
        let hovered = preview.resolve(&model, &model.themes[1]).unwrap();
        assert!(!Arc::ptr_eq(&first, &hovered));
        model.theme_epoch += 1;
        let reloaded = preview.resolve(&model, &model.themes[1]).unwrap();
        assert!(!Arc::ptr_eq(&hovered, &reloaded));
        model.document = parse_resume(br#"{"basics":{"name":"Changed"}}"#).unwrap();
        model.content = project(&model.document);
        let changed = preview.resolve(&model, &model.themes[1]).unwrap();
        assert!(!Arc::ptr_eq(&reloaded, &changed));
        assert!(
            changed.layout.pages.iter().flatten().any(
                |element| matches!(element, LayoutElement::Text(text) if text.text == "Changed")
            )
        );
    }

    #[gpui::test]
    fn paginated_preview_uses_shared_physical_page_dimensions(cx: &mut TestAppContext) {
        let mut model = AppModel::load(None).unwrap();
        model.document = parse_resume(include_bytes!(
            "../../resume-one-cli/tests/fixtures/visual-qa.json"
        ))
        .unwrap();
        model.content = project(&model.document);
        let expected =
            layout::layout_with_theme(&model.content, Language::Zh, &model.themes[0].manifest)
                .unwrap();
        assert_eq!(expected.pages.len(), 2);
        let (_, cx) = cx.add_window_view(|_, cx| ResumeOne::new(model, cx));
        cx.update(|window, _| window.refresh());
        for index in 0..expected.pages.len() {
            let selector = ["resume-paper", "resume-paper-1"][index];
            let bounds = cx.debug_bounds(selector).unwrap();
            assert_eq!(bounds.size.width, px(expected.width_mm * PX_PER_MM));
            assert_eq!(bounds.size.height, px(expected.height_mm * PX_PER_MM));
            if index > 0 {
                let previous = cx.debug_bounds("resume-paper").unwrap();
                assert!(bounds.top() >= previous.bottom());
            }
        }
    }
}

struct Cache {
    revision: String,
    theme_epoch: u64,
    theme_identity: String,
    result: Result<Arc<PreviewData>, String>,
}

struct PreviewData {
    layout: ResumeLayout,
    regions: Vec<Region>,
}

#[derive(Clone, Copy)]
struct Region {
    page: usize,
    target: LayoutTarget,
    top: f32,
    bottom: f32,
    text: bool,
}

impl PreviewState {
    pub fn new(cx: &mut App) -> Self {
        let font_error = cx
            .text_system()
            .add_fonts(vec![
                Cow::Borrowed(layout::SANS_FONT_BYTES),
                Cow::Borrowed(layout::SERIF_FONT_BYTES),
            ])
            .err()
            .map(|error| format!("无法载入简历字体：{error}"));
        Self {
            cache: RefCell::new(None),
            font_error,
        }
    }

    fn resolve(&self, model: &AppModel, theme: &BuiltinTheme) -> Result<Arc<PreviewData>, String> {
        if let Some(error) = &self.font_error {
            return Err(error.clone());
        }
        let mut cache = self.cache.borrow_mut();
        if let Some(cache) = cache.as_ref()
            && cache.revision == model.document.revision
            && cache.theme_epoch == model.theme_epoch
            && cache.theme_identity == theme.identity
        {
            return cache.result.clone();
        }
        let result = layout::layout_with_theme(&model.content, Language::Zh, &theme.manifest).map(
            |layout| {
                let regions = regions(&layout);
                Arc::new(PreviewData { layout, regions })
            },
        );
        *cache = Some(Cache {
            revision: model.document.revision.clone(),
            theme_epoch: model.theme_epoch,
            theme_identity: theme.identity.clone(),
            result: result.clone(),
        });
        result
    }
}

fn regions(layout: &ResumeLayout) -> Vec<Region> {
    let height = layout.height_mm * PT_PER_MM;
    let sans = FontKind::Sans.vertical_metrics();
    let serif = FontKind::Serif.vertical_metrics();
    let mut regions: Vec<Region> = Vec::new();
    for (page, elements) in layout.pages.iter().enumerate() {
        for element in elements {
            let (target, top, bottom, text) = match element {
                LayoutElement::Anchor(anchor) => (
                    anchor.target,
                    height - anchor.y_pt,
                    height - anchor.y_pt + 4.0,
                    false,
                ),
                LayoutElement::Text(text) => {
                    let Some(target) = text.target else { continue };
                    let (ascent, descent) = match text.font {
                        FontKind::Sans => sans,
                        FontKind::Serif => serif,
                    };
                    (
                        target,
                        height - text.baseline_y_pt - ascent * text.size_pt,
                        height - text.baseline_y_pt + descent * text.size_pt,
                        true,
                    )
                }
                LayoutElement::Rule(_) => continue,
            };
            if let Some(region) = regions
                .iter_mut()
                .find(|region| region.page == page && region.target == target)
            {
                if text && !region.text {
                    region.top = top;
                    region.bottom = bottom;
                } else if text {
                    region.top = region.top.min(top);
                    region.bottom = region.bottom.max(bottom);
                }
                region.text |= text;
            } else {
                regions.push(Region {
                    page,
                    target,
                    top,
                    bottom,
                    text,
                });
            }
        }
    }
    regions
}

enum Painted {
    Text(Box<ShapedLine>, Point<Pixels>, Pixels),
    Rule(Bounds<Pixels>, Rgba),
}

fn page_canvas(data: Arc<PreviewData>, page: usize) -> impl IntoElement {
    canvas(
        move |bounds, window, _| {
            let height = data.layout.height_mm * PT_PER_MM;
            data.layout.pages[page]
                .iter()
                .filter_map(|element| match element {
                    LayoutElement::Text(text) => {
                        let font = Font {
                            family: text.font.family().into(),
                            weight: match text.font {
                                FontKind::Sans => FontWeight::NORMAL,
                                FontKind::Serif => FontWeight::SEMIBOLD,
                            },
                            ..gpui::font(text.font.family())
                        };
                        let run = TextRun {
                            len: text.text.len(),
                            font,
                            color: color(text.color).into(),
                            background_color: None,
                            underline: None,
                            strikethrough: None,
                        };
                        let line = window.text_system().shape_line(
                            text.text.clone().into(),
                            px(text.size_pt * PX_PER_PT),
                            &[run],
                            None,
                        );
                        let leading =
                            px(text.leading_pt * PX_PER_PT).max(line.ascent + line.descent);
                        let baseline = (leading - line.ascent - line.descent) / 2.0 + line.ascent;
                        Some(Painted::Text(
                            Box::new(line),
                            bounds.origin
                                + point(
                                    px(text.x_pt * PX_PER_PT),
                                    px((height - text.baseline_y_pt) * PX_PER_PT) - baseline,
                                ),
                            leading,
                        ))
                    }
                    LayoutElement::Rule(rule) => Some(Painted::Rule(
                        Bounds::new(
                            bounds.origin
                                + point(
                                    px(rule.left_pt * PX_PER_PT),
                                    px((height - rule.y_pt - rule.thickness_pt / 2.0) * PX_PER_PT),
                                ),
                            size(
                                px((rule.right_pt - rule.left_pt) * PX_PER_PT),
                                px(rule.thickness_pt * PX_PER_PT),
                            ),
                        ),
                        color(rule.color),
                    )),
                    LayoutElement::Anchor(_) => None,
                })
                .collect::<Vec<_>>()
        },
        |_, elements, window, cx| {
            for element in elements {
                match element {
                    Painted::Text(line, origin, leading) => {
                        let _ = line.paint(origin, leading, window, cx);
                    }
                    Painted::Rule(bounds, color) => window.paint_quad(fill(bounds, color)),
                }
            }
        },
    )
    .absolute()
    .size_full()
}

impl ResumeOne {
    pub(super) fn resume_pages(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let data = match self.preview.resolve(&self.model, self.display_theme()) {
            Ok(data) => data,
            Err(message) => {
                return div()
                    .w(px(560.0))
                    .p_8()
                    .text_size(px(13.0))
                    .text_color(color(self.active_style().colors.seal))
                    .child(message)
                    .into_any_element();
            }
        };
        let style = self.active_style();
        let spotlight = self.insertion_active();
        let valid = self
            .editor
            .as_ref()
            .is_none_or(|editor| editor.buffer.issues().is_empty());
        let left = style.page.margins.left * PX_PER_MM;
        let content_width =
            (style.page.width_mm - style.page.margins.left - style.page.margins.right) * PX_PER_MM;
        let mut pages = div().flex().flex_col().gap_6();
        for page_index in 0..data.layout.pages.len() {
            let selector = if page_index == 0 {
                "resume-paper".into()
            } else {
                format!("resume-paper-{page_index}")
            };
            let mut page = div()
                .id(gpui::SharedString::from(selector.clone()))
                .debug_selector(move || selector.clone())
                .relative()
                .w(px(data.layout.width_mm * PX_PER_MM))
                .h(px(data.layout.height_mm * PX_PER_MM))
                .flex_none()
                .bg(color(data.layout.paper))
                .shadow_lg()
                .on_hover(cx.listener(|this, hovering: &bool, window, cx| {
                    this.inline.paper_hovered = *hovering;
                    if !hovering {
                        this.defer_inline_close(window, cx);
                    }
                }))
                .child(
                    div()
                        .absolute()
                        .size_full()
                        .opacity(1.0 - 0.76 * self.spotlight_progress)
                        .child(page_canvas(data.clone(), page_index)),
                );
            for region in data
                .regions
                .iter()
                .filter(|region| region.page == page_index)
            {
                if self
                    .model
                    .document
                    .value
                    .get(region.target.section)
                    .is_none()
                {
                    continue;
                }
                let form_page = self
                    .inline
                    .form
                    .as_ref()
                    .and_then(|form| form.page)
                    .filter(|page| {
                        data.regions.iter().any(|candidate| {
                            candidate.page == *page && candidate.target == region.target
                        })
                    })
                    .unwrap_or_else(|| {
                        data.regions
                            .iter()
                            .find(|candidate| candidate.target == region.target)
                            .unwrap()
                            .page
                    });
                page = page.child(self.preview_hit(
                    *region,
                    valid && !spotlight,
                    form_page == page_index,
                    cx,
                ));
                if !valid {
                    continue;
                }
                let target = region.target;
                if target.index.is_none() && target.section != "basics" {
                    if spotlight && self.selected_item_module.is_some() {
                        continue;
                    }
                    page = page.child(
                        div()
                            .absolute()
                            .left(px(left))
                            .top(px(region.top * PX_PER_PT - 14.0))
                            .w(px(content_width))
                            .child(self.module_insert_control(
                                Some(target.section.into()),
                                style,
                                cx,
                            )),
                    );
                } else if let Some(index) = target.index {
                    if spotlight && self.selected_item_module.as_deref() != Some(target.section) {
                        continue;
                    }
                    let first = data
                        .regions
                        .iter()
                        .find(|candidate| candidate.target == target)
                        .unwrap();
                    let last = data
                        .regions
                        .iter()
                        .rev()
                        .find(|candidate| candidate.target == target)
                        .unwrap();
                    if index == 0 && first.page == page_index {
                        page = page.child(
                            div()
                                .absolute()
                                .left(px(left))
                                .top(px(region.top * PX_PER_PT - 10.0))
                                .w(px(content_width))
                                .child(self.item_insert_control(target.section, 0, style, cx)),
                        );
                    }
                    if last.page == page_index {
                        page = page.child(
                            div()
                                .absolute()
                                .left(px(left))
                                .top(px(region.bottom * PX_PER_PT + 1.0))
                                .w(px(content_width))
                                .child(self.item_insert_control(
                                    target.section,
                                    index + 1,
                                    style,
                                    cx,
                                )),
                        );
                    }
                }
            }
            if valid {
                page = self.empty_module_controls(page, &data, page_index, cx);
                if page_index + 1 == data.layout.pages.len()
                    && (!spotlight || self.selected_item_module.is_none())
                {
                    let bottom = data
                        .regions
                        .iter()
                        .filter(|region| region.page == page_index)
                        .map(|region| region.bottom)
                        .fold(0.0, f32::max);
                    let position =
                        (bottom * PX_PER_PT + 18.0).min(data.layout.height_mm * PX_PER_MM - 30.0);
                    page = page.child(
                        div()
                            .absolute()
                            .left(px(left))
                            .top(px(position))
                            .w(px(content_width))
                            .child(self.module_insert_control(None, style, cx)),
                    );
                }
            }
            pages = pages.child(page);
        }
        pages.into_any_element()
    }

    fn preview_hit(
        &self,
        region: Region,
        valid: bool,
        show_form: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let style = self.active_style();
        let target = region.target;
        let id = target.section.to_owned();
        let hover_id = id.clone();
        let click_id = id.clone();
        let key_id = id.clone();
        let selector = if target.section == "basics" {
            "preview-basics".into()
        } else {
            format!(
                "preview-hit-{}-{:?}-{}",
                target.section, target.index, region.page
            )
        };
        let editable = target.index.is_some() || target.section == "basics";
        let hovered = match target.index {
            Some(index) => self.hovered_item.as_ref() == Some(&(id.clone(), index)),
            None => self.hovered_section.as_deref() == Some(target.section),
        };
        let mut hit = div()
            .id(gpui::SharedString::from(format!(
                "hit-{}-{:?}-{}",
                id, target.index, region.page
            )))
            .debug_selector(move || selector.clone())
            .absolute()
            .left(px(style.page.margins.left * PX_PER_MM))
            .top(px(region.top * PX_PER_PT))
            .w(px((style.page.width_mm
                - style.page.margins.left
                - style.page.margins.right)
                * PX_PER_MM))
            .h(px(((region.bottom - region.top) * PX_PER_PT).max(4.0)))
            .when(valid && editable, |hit| {
                hit.focusable().tab_index(0).key_context("PreviewItem")
            })
            .on_hover(cx.listener(move |this, hovering: &bool, window, cx| {
                if let Some(index) = target.index {
                    if *hovering {
                        this.hovered_item = Some((hover_id.clone(), index));
                    } else if this.hovered_item.as_ref() == Some(&(hover_id.clone(), index)) {
                        this.hovered_item = None;
                    }
                } else if *hovering {
                    this.hovered_section = Some(hover_id.clone());
                } else if this.hovered_section.as_deref() == Some(hover_id.as_str()) {
                    this.hovered_section = None;
                }
                if *hovering && editable && valid {
                    this.schedule_inline(&hover_id, target.index, region.page, window, cx);
                }
                cx.notify();
            }))
            .on_click(cx.listener(move |this, _, window, cx| {
                if editable && valid {
                    this.open_inline(&click_id, target.index, window, cx);
                    if let Some(form) = &mut this.inline.form {
                        form.page = Some(region.page);
                    }
                }
            }))
            .on_action(
                cx.listener(move |this, _: &inline_ui::OpenInline, window, cx| {
                    if editable && valid {
                        this.open_inline(&key_id, target.index, window, cx);
                        if let Some(form) = &mut this.inline.form {
                            form.page = Some(region.page);
                        }
                        this.focus_inline_first(window, cx);
                    }
                }),
            );
        if valid {
            hit = hit.child(
                div()
                    .id(gpui::SharedString::from(format!(
                        "remove-preview-{}-{:?}-{}",
                        id, target.index, region.page
                    )))
                    .absolute()
                    .right_0()
                    .top_0()
                    .px_2()
                    .cursor_pointer()
                    .opacity(if hovered { 1.0 } else { 0.0 })
                    .text_color(color(style.colors.seal))
                    .child("−")
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        let edit = match target.index {
                            Some(index) => StructureEdit::RemoveItem {
                                id: id.clone(),
                                index,
                            },
                            None => StructureEdit::RemoveSection { id: id.clone() },
                        };
                        this.apply_visual_structure(edit, window, cx);
                    })),
            );
        }
        if editable && show_form && self.inline_matches(target.section, target.index) {
            hit = hit.child(self.inline_form(cx));
        }
        hit
    }

    fn empty_module_controls(
        &self,
        mut page: gpui::Stateful<gpui::Div>,
        data: &PreviewData,
        page_index: usize,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let summaries = self.model.document.sections();
        let style = self.active_style();
        for (index, summary) in summaries.iter().enumerate() {
            if summary.id == "basics"
                || summary.count != 0
                || self.selected_item_module.as_deref() != Some(&summary.id)
            {
                continue;
            }
            let next = summaries.iter().skip(index + 1).find_map(|summary| {
                data.regions.iter().find(|region| {
                    region.target.section == summary.id && region.target.index.is_none()
                })
            });
            let (target_page, y) = next.map_or_else(
                || {
                    let last_page = data.layout.pages.len() - 1;
                    let bottom = data
                        .regions
                        .iter()
                        .filter(|region| region.page == last_page)
                        .map(|region| region.bottom)
                        .fold(0.0, f32::max);
                    (last_page, bottom * PX_PER_PT + 16.0)
                },
                |region| (region.page, region.top * PX_PER_PT - 14.0),
            );
            if target_page != page_index {
                continue;
            }
            if let Some(&(id, _)) = SECTION_LABELS.iter().find(|(id, _)| *id == summary.id) {
                page = page.child(
                    div()
                        .absolute()
                        .left(px(style.page.margins.left * PX_PER_MM))
                        .top(px(y))
                        .w(px((style.page.width_mm
                            - style.page.margins.left
                            - style.page.margins.right)
                            * PX_PER_MM))
                        .child(self.item_insert_control(id, 0, style, cx)),
                );
            }
        }
        page
    }
}
