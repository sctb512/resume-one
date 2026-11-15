//! Measure reversible layout alternatives at the theme's original paper size.
use super::{LayoutElement, PT_PER_MM, ResumeLayout, layout_with_style};
use crate::{
    render::Language,
    semantic::ResumeContent,
    theme_style::{DatePlacement, ItemLayout, ThemeStyle},
};

pub(crate) fn fit_layout(
    content: &ResumeContent,
    language: Language,
    style: ThemeStyle,
) -> Result<(ThemeStyle, ResumeLayout), String> {
    let natural = layout_with_style(content, language, &style)?;
    if !style.page.fit_to_one_page || natural.pages.len() == 1 {
        return Ok((style, natural));
    }

    let arrangements = arrangements(content, &style);
    // Prefer composition and wrapping changes. Compare each density tier before
    // moving to the next one, and reserve changes to text size for the last tier.
    for (gaps, leading, type_scale) in [
        (1.0, 0.0, 1.0),
        (0.83, 0.0, 1.0),
        (0.65, 0.55, 1.0),
        (0.65, 0.55, 0.94),
        (0.65, 0.55, 0.90),
    ] {
        let mut best = None;
        for (index, (arrangement, change_cost)) in arrangements.iter().enumerate() {
            if index == 0 && gaps == 1.0 {
                continue; // Already measured.
            }
            let mut candidate = arrangement.clone();
            if gaps != 1.0 {
                candidate.adjust_page_density(gaps, leading, type_scale);
            }
            let layout = layout_with_style(content, language, &candidate)?;
            if let Some(penalty) = page_penalty(&layout, &candidate) {
                let score = penalty + change_cost;
                if best.as_ref().is_none_or(|(old, _, _)| score < *old) {
                    best = Some((score, candidate, layout));
                }
            }
        }
        if let Some((_, candidate, layout)) = best {
            return Ok((candidate, layout));
        }
    }
    Ok((style, natural))
}

fn arrangements(content: &ResumeContent, style: &ThemeStyle) -> Vec<(ThemeStyle, f32)> {
    let mut alternatives = vec![(style.clone(), 0.0)];
    let mut reflow = style.clone();
    let mut reflow_cost = 0.0;
    if style.stacked_contacts && content.contacts.len() > 1 {
        reflow.stacked_contacts = false;
        reflow_cost += 0.07;
        alternatives.push((reflow.clone(), reflow_cost));
    }
    for section in style.ordered_sections(&content.sections) {
        let authored = style.section(section.id);
        // Move dates into a title row only when this can replace a separate
        // metadata line. Leave custom templates and chip layouts intact.
        if authored.date == DatePlacement::Inline
            && authored.items != ItemLayout::Chips
            && !style.entry_headers.contains_key(section.id)
            && !section.items.is_empty()
            && section
                .items
                .iter()
                .filter(|item| {
                    !item.heading.is_empty() && item.details.len() == 1 && item.date().is_some()
                })
                .count()
                >= section.items.len().div_ceil(2)
        {
            reflow
                .sections
                .entry(section.id.to_owned())
                .or_default()
                .date = DatePlacement::Right;
            reflow_cost += 0.03;
        }
    }
    if reflow_cost > alternatives.last().unwrap().1 {
        alternatives.push((reflow.clone(), reflow_cost));
    }
    if style.has_sidebar() {
        // A narrow sidebar can strand a long section on the next page even
        // when the main column has room. Measure both directions.
        for delta in [-7.0_f32, 7.0] {
            let width = (style.sidebar_width_pct + delta).clamp(20.0, 40.0);
            if width == style.sidebar_width_pct {
                continue;
            }
            let mut candidate = reflow.clone();
            candidate.sidebar_width_pct = width;
            alternatives.push((candidate, reflow_cost + 0.08));
        }
    }
    if style.split_header && style.contact_region == "header" && !content.contacts.is_empty() {
        for fraction in [0.50, 0.66] {
            let mut candidate = reflow.clone();
            candidate.header_identity_fraction = fraction;
            alternatives.push((candidate, reflow_cost + 0.05));
        }
        let mut candidate = reflow;
        candidate.split_header = false;
        alternatives.push((candidate, reflow_cost + 0.14));
    }
    alternatives
}

fn page_penalty(layout: &ResumeLayout, style: &ThemeStyle) -> Option<f32> {
    let [page] = layout.pages.as_slice() else {
        return None;
    };
    let height = layout.height_mm * PT_PER_MM;
    let footer = style.page.margins.bottom * PT_PER_MM;
    let usable = height - (style.page.margins.top + style.page.margins.bottom) * PT_PER_MM;
    let mut lowest = [height; 2];
    let mut bottom_room = usable;
    let mut occupied = [false; 2];
    let mut font_metrics = std::collections::BTreeMap::new();
    let column_edges = style.has_sidebar().then(|| {
        [false, true].map(|sidebar| {
            let margins = style.column_style(sidebar).page.margins;
            (
                margins.left * PT_PER_MM,
                (layout.width_mm - margins.right) * PT_PER_MM,
            )
        })
    });
    for element in page {
        let LayoutElement::Text(text) = element else {
            continue;
        };
        let Some(target) = text.target else { continue };
        let (ascent, descent) = *font_metrics
            .entry(text.font)
            .or_insert_with(|| text.font.vertical_metrics());
        let ink_bottom = text.baseline_y_pt - descent * text.size_pt;
        let ink_top = text.baseline_y_pt + ascent * text.size_pt;
        if ink_bottom < 0.0 || ink_top > height || text.baseline_y_pt < footer - 0.5 {
            return None;
        }
        bottom_room = bottom_room.min(ink_bottom - footer);
        if let Some((left, right)) = text.target_lane
            && (text.x_pt < left - 0.75 || text.x_pt + text.width_pt() > right + 0.75)
        {
            return None;
        }
        if let Some(edges) = column_edges {
            let lane = if target.section == "basics" {
                // A full-width header does not contribute to lane balance.
                text.target_lane.and_then(|(left, right)| {
                    edges
                        .iter()
                        .position(|&(a, b)| (left - a).abs() < 0.25 && (right - b).abs() < 0.25)
                })
            } else {
                Some(usize::from(style.in_sidebar(target.section)))
            };
            if let Some(lane) = lane {
                occupied[lane] = true;
                lowest[lane] = lowest[lane].min(ink_bottom);
            }
        }
    }
    let crowded = (12.0 - bottom_room).max(0.0) / 90.0;
    let imbalance = if occupied == [true, true] {
        (lowest[0] - lowest[1]).abs() / usable * 0.65
    } else {
        0.0
    };
    Some(crowded + imbalance)
}
