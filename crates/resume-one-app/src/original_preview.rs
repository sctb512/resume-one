use std::{collections::VecDeque, sync::Arc};

use gpui::{
    AnyElement, Context, Image, ImageFormat, IntoElement, Window, div, img, prelude::*, px,
};
use resume_one_themes::OriginalPreview;

use crate::{ResumeOne, color, model::BuiltinTheme};

#[derive(Clone, PartialEq, Eq)]
struct Key {
    theme: String,
    theme_revision: String,
    resume_revision: String,
}

impl Key {
    fn for_theme(view: &ResumeOne, theme: &BuiltinTheme) -> Option<Self> {
        Some(Self {
            theme: theme.identity.clone(),
            theme_revision: theme.original.as_ref()?.render_revision.clone(),
            resume_revision: view.model.document.revision.clone(),
        })
    }
}

struct Page {
    width: u32,
    strips: Vec<(u32, Arc<Image>)>,
}

impl From<OriginalPreview> for Page {
    fn from(preview: OriginalPreview) -> Self {
        Self {
            width: preview.width,
            strips: preview
                .strips
                .into_iter()
                .map(|(height, png)| (height, Arc::new(Image::from_bytes(ImageFormat::Png, png))))
                .collect(),
        }
    }
}

#[derive(Default)]
pub(super) struct OriginalPreviewState {
    running: bool,
    last_declarative: Option<BuiltinTheme>,
    recent: VecDeque<(Key, Result<Page, String>)>,
}

impl ResumeOne {
    pub(super) fn request_original_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.source_mode || !self.model.preview_ready {
            return;
        }
        let displayed = self.display_theme();
        if displayed.original.is_none() {
            if self
                .original_preview
                .last_declarative
                .as_ref()
                .is_none_or(|previous| previous.identity != displayed.identity)
            {
                self.original_preview.last_declarative = Some(displayed.clone());
            }
            return;
        }
        if self.original_preview.running {
            return;
        }
        let (Some(theme), Some(key)) =
            (displayed.original.clone(), Key::for_theme(self, displayed))
        else {
            return;
        };
        if self
            .original_preview
            .recent
            .iter()
            .any(|(candidate, _)| candidate == &key)
        {
            return;
        }
        self.original_preview.running = true;
        let resume = self.model.document.value.clone();
        let job = cx.background_executor().spawn(async move {
            resume_one_themes::preview_original(&theme, &resume)
                .map(Page::from)
                .map_err(|error| error.message)
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = job.await;
            this.update_in(cx, |this, _, cx| {
                this.original_preview.running = false;
                this.original_preview.recent.push_front((key, result));
                this.original_preview.recent.truncate(4);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn original_thumbnail(&self, theme: &BuiltinTheme) -> Option<AnyElement> {
        let key = Key::for_theme(self, theme)?;
        let (_, page) = self
            .original_preview
            .recent
            .iter()
            .find(|(candidate, _)| candidate == &key)?;
        let page = page.as_ref().ok()?;
        let (height, image) = page.strips.first()?;
        Some(
            img(image.clone())
                .w(px(48.))
                .h(px(*height as f32 * 48. / page.width as f32))
                .into_any_element(),
        )
    }

    pub(super) fn original_paper(
        &self,
        theme: &BuiltinTheme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let key = Key::for_theme(self, theme).expect("original theme");
        let style = self.active_style();
        let width = style.page.width_mm * 3. * self.preview_scale;
        let paper = div()
            .id("resume-paper")
            .debug_selector(|| "resume-paper".into())
            .w(px(width))
            .flex_none()
            .flex()
            .flex_col()
            .bg(color(style.colors.paper))
            .shadow_md();
        let recent = &self.original_preview.recent;
        let cached = recent
            .iter()
            .find(|(candidate, _)| candidate == &key)
            .map(|(_, page)| page);
        // Keep a valid paper for this document while a new theme paints.
        let picture = cached.or_else(|| {
            recent
                .iter()
                .find(|(candidate, page)| {
                    candidate.resume_revision == key.resume_revision && page.is_ok()
                })
                .map(|(_, page)| page)
        });
        match picture {
            Some(Ok(page)) => {
                let factor = width / page.width as f32;
                paper
                    .children(page.strips.iter().map(|(height, image)| {
                        img(image.clone())
                            .w_full()
                            .h(px(*height as f32 * factor))
                            .flex_none()
                    }))
                    .into_any_element()
            }
            Some(Err(error)) => paper
                .min_h(px(190.))
                .items_center()
                .justify_center()
                .p_6()
                .text_size(px(13.))
                .text_color(color(style.colors.muted))
                .child(error.clone())
                .into_any_element(),
            None => {
                let fallback = self
                    .original_preview
                    .last_declarative
                    .as_ref()
                    .unwrap_or(&self.model.themes[0]);
                self.resume_pages(fallback, false, cx)
            }
        }
    }
}
