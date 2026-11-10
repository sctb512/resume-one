use std::{collections::VecDeque, sync::Arc};

use gpui::{AnyElement, Context, IntoElement, Window, div, prelude::*, px};

use crate::{ResumeOne, color, model::BuiltinTheme};

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod desktop;

#[derive(Clone, PartialEq, Eq)]
struct Key {
    theme: String,
    theme_revision: String,
    resume_revision: String,
}

impl Key {
    fn new(view: &ResumeOne, theme: &BuiltinTheme) -> Option<Self> {
        Some(Self {
            theme: theme.identity.clone(),
            theme_revision: theme.original.as_ref()?.render_revision.clone(),
            resume_revision: view.model.document.revision.clone(),
        })
    }
}

#[derive(Default)]
pub(super) struct OriginalPreviewState {
    running: bool,
    recent: VecDeque<(Key, Result<Arc<str>, String>)>,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    desktop: Option<std::rc::Rc<desktop::Desktop>>,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    ready: bool,
    painted: Option<Key>,
    sent: Option<(u64, Key)>,
    sequence: u64,
    canvas: Option<resume_one_core::theme_style::Rgb>,
    last_declarative: Option<BuiltinTheme>,
    error: Option<String>,
}

impl ResumeOne {
    pub(super) fn request_original_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.source_mode || !self.model.preview_ready {
            self.original_preview.cancel_pending(None);
            return;
        }
        let displayed = self.display_theme();
        let (Some(theme), Some(key)) = (displayed.original.clone(), Key::new(self, displayed))
        else {
            if self
                .original_preview
                .last_declarative
                .as_ref()
                .is_none_or(|previous| previous.identity != displayed.identity)
            {
                self.original_preview.last_declarative = Some(displayed.clone());
            }
            self.original_preview.cancel_pending(None);
            if self.online.online
                && self.online.catalog.as_ref().is_some_and(|catalog| {
                    catalog.entries().iter().any(|entry| {
                        entry
                            .manifest
                            .ends_with(resume_one_themes::ORIGINAL_THEME_SUFFIX)
                    })
                })
            {
                self.ensure_original_surface(window, cx);
            }
            return;
        };
        self.ensure_original_surface(window, cx);
        let state = &mut self.original_preview;
        state.cancel_pending(Some(&key));
        if let Some(index) = state
            .recent
            .iter()
            .position(|(candidate, _)| candidate == &key)
        {
            let entry = state.recent.remove(index).expect("entry was found");
            if let Ok(html) = &entry.1 {
                state.present(&key, html, self.shell.style.colors.canvas);
            }
            state.recent.push_front(entry);
        } else if !state.running && state.error.is_none() {
            state.running = true;
            let resume = self.model.document.value.clone();
            let job = cx.background_executor().spawn(async move {
                theme
                    .render_preview_html(&resume)
                    .map(Arc::<str>::from)
                    .map_err(|error| error.message)
            });
            cx.spawn_in(window, async move |this, cx| {
                let result = job.await;
                this.update_in(cx, |this, _, cx| {
                    // Re-evaluate the currently hovered theme after this job.
                    // An outdated job can fill the cache but cannot paint.
                    let state = &mut this.original_preview;
                    state.running = false;
                    state.recent.push_front((key, result));
                    state.recent.truncate(4);
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    fn ensure_original_surface(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let state = &mut self.original_preview;
        if state.desktop.is_some() || state.error.is_some() || !desktop::Desktop::available(window)
        {
            return;
        }
        let (sender, mut receiver) = futures_channel::mpsc::unbounded();
        match desktop::Desktop::open(window, sender) {
            Ok(desktop) => state.desktop = Some(desktop),
            Err(error) => {
                state.error = Some(error);
                return;
            }
        }
        cx.spawn_in(window, async move |this, cx| {
            while let Ok(message) = receiver.recv().await {
                if this
                    .update_in(cx, |this, window, cx| {
                        let state = &mut this.original_preview;
                        match message.as_str() {
                            "ready" => {
                                state.ready = true;
                                state.painted = None;
                                state.sent = None;
                                state.canvas = None;
                                cx.notify();
                            }
                            "source" => {
                                this.toggle_source(&crate::editor_ui::ToggleSource, window, cx)
                            }
                            "save" => this.save_json(&crate::editor_ui::Save, window, cx),
                            "save-as" => this.save_json_as(&crate::editor_ui::SaveAs, window, cx),
                            "undo" => this.undo_visual_edit(
                                &crate::structure_ui::UndoVisualEdit,
                                window,
                                cx,
                            ),
                            "redo" => this.redo_visual_edit(
                                &crate::structure_ui::RedoVisualEdit,
                                window,
                                cx,
                            ),
                            "dismiss" => {
                                this.dismiss_inline(&crate::inline_ui::DismissInline, window, cx)
                            }
                            _ => {
                                if let Some((token, key)) = state.sent.as_ref()
                                    && message
                                        .strip_prefix("painted:")
                                        .and_then(|value| value.parse().ok())
                                        == Some(*token)
                                {
                                    state.painted = Some(key.clone());
                                    state.sent = None;
                                    cx.notify();
                                }
                            }
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    fn ensure_original_surface(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.original_preview.error = Some("当前平台没有可用的原版主题预览组件".into());
    }

    pub(super) fn original_surface_visible(&self, modal: bool) -> bool {
        !modal
            && self.view_blend < 0.5
            && self.model.preview_ready
            && self.display_theme().original.is_some()
            && !self.theme_manager_open
            && !self.files.open
            && !self.export_menu_open
            && self.drawer.is_none()
    }

    pub(super) fn original_surface(&self, visible: bool, cx: &mut Context<Self>) -> AnyElement {
        let state = &self.original_preview;
        let theme = self.display_theme();
        let key = Key::new(self, theme).expect("original theme");
        let error = state.error.as_ref().or_else(|| {
            state.recent.iter().find_map(|(candidate, result)| {
                (candidate == &key).then(|| result.as_ref().err()).flatten()
            })
        });
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        if visible
            && error.is_none()
            && state.painted.is_some()
            && let Some(desktop) = &state.desktop
        {
            return div()
                .id("original-preview")
                .debug_selector(|| "original-preview".into())
                .size_full()
                .child(desktop.element())
                .into_any_element();
        }
        let _ = visible;
        state.hide();
        if visible
            && error.is_none()
            && let Some(previous) = &state.last_declarative
        {
            return div()
                .size_full()
                .flex()
                .justify_center()
                .pt(px(24.))
                .child(self.resume_pages(previous, false, cx))
                .into_any_element();
        }
        let style = self.shell_style();
        div()
            .id("original-preview-wait")
            .size_full()
            .flex()
            .items_start()
            .justify_center()
            .pt(px(24.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .w(px(794. * self.preview_scale))
                    .min_h(px(if error.is_some() {
                        150.
                    } else {
                        1123. * self.preview_scale
                    }))
                    .shadow_sm()
                    .bg(color(style.colors.paper))
                    .text_color(color(style.colors.muted))
                    .children(error.cloned()),
            )
            .into_any_element()
    }
}

impl OriginalPreviewState {
    pub(super) fn reset_document(&mut self) {
        self.hide();
        self.painted = None;
        self.sent = None;
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        if let Some(desktop) = self.desktop.as_ref().filter(|_| self.ready) {
            let _ = desktop.evaluate("jianliReset()");
        }
    }

    fn cancel_pending(&mut self, keep: Option<&Key>) {
        if self.sent.as_ref().is_some_and(|(_, key)| Some(key) != keep) {
            self.sent = None;
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            if let Some(desktop) = self.desktop.as_ref().filter(|_| self.ready) {
                let _ = desktop.evaluate("jianliCancel()");
            }
        }
    }

    pub(super) fn hide(&self) {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        if let Some(desktop) = &self.desktop {
            desktop.hide();
        }
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    fn present(&mut self, key: &Key, html: &str, canvas: resume_one_core::theme_style::Rgb) {
        let Some(desktop) = self.desktop.as_ref().filter(|_| self.ready) else {
            return;
        };
        if self.canvas != Some(canvas) {
            let color = format!("#{:02x}{:02x}{:02x}", canvas.r, canvas.g, canvas.b);
            if let Err(error) = desktop.evaluate(&format!("jianliCanvas('{color}')")) {
                self.error = Some(error);
                return;
            }
            self.canvas = Some(canvas);
        }
        if self.painted.as_ref() == Some(key)
            || self.sent.as_ref().is_some_and(|(_, sent)| sent == key)
        {
            return;
        }
        self.sequence += 1;
        let html = serde_json::to_string(html).expect("encode HTML as a JS string");
        let token = self.sequence;
        if let Err(error) = desktop.evaluate(&format!("jianliRender({html}, {token})")) {
            self.error = Some(error);
        } else {
            self.sent = Some((token, key.clone()));
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    fn present(&mut self, _: &Key, _: &str, _: resume_one_core::theme_style::Rgb) {}
}
