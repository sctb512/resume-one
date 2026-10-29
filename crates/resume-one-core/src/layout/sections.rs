//! Bounded print-native section components, shared by preview and PDF.
use super::*;
use crate::theme_style::ItemLayout;

pub(super) struct ItemMeasure {
    pub full: f32,
    pub start: Option<f32>,
}

/// Measure using exactly the same inner lane as the real frame. Section-level
/// keep decisions must not measure a wider, unframed title.
pub(super) fn measure(
    page: &mut PageWriter<'_>,
    item: &Item,
    id: &str,
    style: &ThemeStyle,
) -> ItemMeasure {
    let (left, right) = (page.left, page.right);
    match style.section(id).items {
        ItemLayout::Cards => {
            page.left += 8.0;
            page.right += 8.0;
        }
        ItemLayout::Timeline => page.left += 16.0,
        _ => {}
    }
    let measured = ItemMeasure {
        full: item_height(page, item, id, style),
        start: item_start_height(page, item, id, style),
    };
    (page.left, page.right) = (left, right);
    measured
}

pub(super) fn heading_reservation(
    header: f32,
    item: &ItemMeasure,
    capacity: f32,
    avoid_split: bool,
) -> f32 {
    if avoid_split && header + item.full <= capacity {
        header + item.full
    } else {
        header + item.start.unwrap_or(0.0)
    }
}

pub(super) struct ItemFrame {
    kind: ItemLayout,
    left: f32,
    right: f32,
    first_page: usize,
    first_op: usize,
    first_top: f32,
}

impl ItemFrame {
    pub(super) fn begin(
        page: &mut PageWriter<'_>,
        item: &Item,
        id: &'static str,
        index: usize,
        style: &ThemeStyle,
        reserved_by_heading: bool,
    ) -> Self {
        let measured = measure(page, item, id, style);
        let kind = style.section(id).items;
        let (left, right) = (page.left, page.right);
        match kind {
            ItemLayout::Cards => {
                page.left += 8.0;
                page.right += 8.0;
            }
            ItemLayout::Timeline => page.left += 16.0,
            _ => {}
        }
        if !reserved_by_heading {
            let capacity = page.height - page.top - page.bottom;
            // An oversized item cannot be kept whole. Reserving a full page
            // here after painting its section heading creates an orphan page.
            page.ensure(
                if style.page.avoid_split_items && measured.full <= capacity {
                    measured.full
                } else {
                    measured.start.unwrap_or(style.section(id).item_gap_pt())
                },
            );
        }
        let first_op = page.ops.len();
        let first_page = page.pages.len();
        let inner = (page.left, page.right);
        page.left = left;
        page.right = right;
        page.mark(LayoutTarget {
            section: id,
            index: Some(index),
        });
        (page.left, page.right) = inner;
        page.gap(style.section(id).item_gap_pt());
        Self {
            kind,
            left,
            right,
            first_page: page.pages.len(),
            first_op: if first_page == page.pages.len() {
                first_op
            } else {
                0
            },
            first_top: page.y,
        }
    }

    pub(super) fn finish(self, page: &mut PageWriter<'_>, style: &ThemeStyle) {
        page.left = self.left;
        page.right = self.right;
        if !matches!(self.kind, ItemLayout::Cards | ItemLayout::Timeline) {
            return;
        }
        let ascent = font_kind(&style.typography.item_title)
            .expect("checked font")
            .vertical_metrics()
            .0
            * style.typography.item_title.size_pt;
        let last = page.pages.len();
        for index in self.first_page..=last {
            let top = if index == self.first_page {
                self.first_top + ascent + 2.0
            } else {
                page.height - page.top + ascent + 2.0
            };
            let bottom = if index == last { page.y } else { page.bottom };
            let top = top.min(page.height - 2.0);
            let height = (top - bottom).max(0.0);
            let width = page.width - self.left - self.right;
            let mut fills = Vec::new();
            let mut fill = |x, y, w, h, color| {
                fills.push(LayoutElement::Fill(LayoutFill {
                    left_pt: x,
                    bottom_pt: y,
                    width_pt: w,
                    height_pt: h,
                    color,
                }))
            };
            if self.kind == ItemLayout::Cards {
                fill(self.left, bottom, width, height, style.card_paper());
                let border = style.colors.border;
                for (x, y, w, h) in [
                    (self.left, bottom, width, 0.6),
                    (self.left, top - 0.6, width, 0.6),
                    (self.left, bottom, 0.6, height),
                    (page.width - self.right - 0.6, bottom, 0.6, height),
                ] {
                    fill(x, y, w, h, border);
                }
            } else {
                fill(self.left + 4.0, bottom, 1.0, height, style.colors.border);
                if index == self.first_page {
                    fill(self.left + 2.0, top - 7.0, 5.0, 5.0, style.colors.accent);
                }
            }
            let ops = if index == last {
                &mut page.ops
            } else {
                &mut page.pages[index]
            };
            let at = if index == self.first_page {
                self.first_op.min(ops.len())
            } else {
                0
            };
            ops.splice(at..at, fills);
        }
        page.y = (page.y - 7.0).max(page.bottom);
    }
}

pub(super) fn chips(page: &mut PageWriter<'_>, labels: &[String], style: &ThemeStyle) {
    let meta = &style.typography.meta;
    let font = font_kind(meta).expect("checked font");
    let height = meta.size_pt * meta.line_height + 6.0;
    let fill = style.colors.paper.mix(style.colors.accent, 0.08);
    let ink = style.colors.ink.readable(fill, style.colors.ink, 4.5);
    let mut x = 0.0;
    page.gap(4.0);
    page.ensure(height);
    for label in labels {
        for line in page.wrap_style(label, meta, (page.content_width() - 10.0).max(1.0)) {
            let width = (page.styled_width(&line, meta) + 10.0).min(page.content_width());
            if x > 0.0 && x + width > page.content_width() {
                page.y -= height + 3.0;
                page.ensure(height);
                x = 0.0;
            }
            page.ops.push(LayoutElement::Fill(LayoutFill {
                left_pt: page.left + x,
                bottom_pt: page.y - meta.size_pt * 0.3,
                width_pt: width,
                height_pt: height - 1.0,
                color: fill,
            }));
            page.ops.push(LayoutElement::Text(LayoutText {
                text: line,
                x_pt: page.left + x + 5.0,
                baseline_y_pt: page.y + 2.0,
                size_pt: meta.size_pt,
                leading_pt: height,
                tracking_pt: meta.tracking_pt(),
                color: ink,
                font,
                target: page.target,
                target_lane: page.target_lane,
            }));
            x += width + 4.0;
        }
    }
    page.y -= height;
}
