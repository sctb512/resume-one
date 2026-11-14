use std::{collections::VecDeque, sync::Arc};

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
use crate::i18n::tr;
use gpui::{AnyElement, Context, IntoElement, Window, div, prelude::*, px};

use crate::{ResumeOne, color, model::BuiltinTheme};

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod desktop;
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod editing;

#[derive(Clone, PartialEq, Eq)]
struct Key {
    theme: String,
    theme_revision: String,
    resume_revision: String,
    language: resume_one_core::render::Language,
    ui_language: resume_one_core::render::Language,
}

impl Key {
    fn new(view: &ResumeOne, theme: &BuiltinTheme) -> Option<Self> {
        let original = theme.original.as_ref()?;
        Some(Self {
            theme: theme.identity.clone(),
            theme_revision: original.render_revision.clone(),
            resume_revision: view.model.document.revision.clone(),
            language: view.model.presentation_language,
            ui_language: crate::i18n::language(),
        })
    }
}

#[derive(Default)]
pub(super) struct OriginalPreviewState {
    pub reading_target: Option<(String, Option<usize>)>,
    running: bool,
    recent: VecDeque<(Key, Result<Arc<str>, String>)>,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    desktop: Option<std::rc::Rc<desktop::Desktop>>,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    ready: bool,
    painted: Option<Key>,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    painted_token: Option<u64>,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    edit_session: Option<(u64, usize)>,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    edit_revision: Option<String>,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    edit_enabled: bool,
    sent: Option<(u64, Key)>,
    sequence: u64,
    canvas: Option<resume_one_core::theme_style::Rgb>,
    last_declarative: Option<BuiltinTheme>,
    snapshot: Option<(Key, Arc<gpui::RenderImage>)>,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    capture_sequence: u64,
    capture_pending: bool,
    overlay_open: bool,
    cover_key: Option<Key>,
    error: Option<String>,
    thumbnails: VecDeque<(Key, Option<Arc<gpui::Image>>)>,
    thumbnail_pending: bool,
    thumbnail_epoch: u64,
}

impl ResumeOne {
    pub(super) fn update_original_cover(
        &mut self,
        covered: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let newly_covered = covered && !self.original_preview.overlay_open;
        self.original_preview.overlay_open = covered;
        if newly_covered {
            self.original_preview.cover_key = self.original_preview.painted.clone();
        }
        if !covered {
            self.original_preview.cover_key = None;
        }
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        if newly_covered {
            self.capture_original_preview(window, cx);
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let _ = (newly_covered, window, cx);
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    fn capture_original_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let state = &mut self.original_preview;
        let (Some(desktop), Some(key)) = (state.desktop.as_ref(), state.painted.clone()) else {
            return;
        };
        if !desktop.is_visible() || state.capture_pending {
            return;
        }
        state.capture_sequence = state.capture_sequence.wrapping_add(1);
        let sequence = state.capture_sequence;
        state.capture_pending = true;
        let receive = desktop.snapshot();
        let task = cx
            .background_executor()
            .timer(std::time::Duration::from_millis(150));
        cx.spawn_in(window, async move |this, cx| {
            task.await;
            this.update_in(cx, |this, _, cx| {
                let state = &mut this.original_preview;
                if state.capture_sequence == sequence && state.capture_pending {
                    state.capture_pending = false;
                    state.capture_sequence = state.capture_sequence.wrapping_add(1);
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
        let renderer = cx.svg_renderer();
        let executor = cx.background_executor().clone();
        cx.spawn_in(window, async move |this, cx| {
            let snapshot = if let Ok(image) = receive.await {
                executor
                    .spawn(async move { image.to_image_data(renderer).ok() })
                    .await
            } else {
                None
            };
            this.update_in(cx, |this, _, cx| {
                let state = &mut this.original_preview;
                if state.capture_sequence != sequence {
                    return;
                }
                state.capture_pending = false;
                if let Some(snapshot) = snapshot
                    && state.painted.as_ref() == Some(&key)
                {
                    state.snapshot = Some((key, snapshot));
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn original_thumbnail(&self, theme: &BuiltinTheme) -> Option<Arc<gpui::Image>> {
        let key = Key::new(self, theme)?;
        self.original_preview
            .thumbnails
            .iter()
            .find_map(|(cached, image)| {
                (cached.theme == key.theme
                    && cached.theme_revision == key.theme_revision
                    && cached.language == key.language)
                    .then(|| image.clone())
                    .flatten()
            })
    }

    fn request_original_thumbnail(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.original_preview.thumbnail_pending || !self.model.preview_ready {
            return;
        }
        let offset = f32::from(self.theme_scroll.offset().y);
        let height =
            f32::from(self.theme_scroll.bounds().size.height).max(self.viewport_height - 210.);
        let candidates = if self.online.online {
            let entries = self.online_results();
            crate::theme_browser_ui::thumbnail_window(offset, height, entries.len())
                .filter_map(|index| self.online.thumbnail_theme(&entries[index].1.id))
                .collect::<Vec<_>>()
        } else {
            let themes = self.local_theme_results();
            crate::theme_browser_ui::thumbnail_window(offset, height, themes.len())
                .map(|index| themes[index].1.clone())
                .collect::<Vec<_>>()
        };
        let Some((key, original)) = candidates.iter().find_map(|theme| {
            let key = Key::new(self, theme)?;
            let original = theme.original.clone()?;
            (!self
                .original_preview
                .thumbnails
                .iter()
                .any(|(cached, _)| cached == &key))
            .then_some((key, original))
        }) else {
            return;
        };
        let resume = self.model.document.value.clone();
        let language = key.language;
        let epoch = self.original_preview.thumbnail_epoch;
        self.original_preview.thumbnail_pending = true;
        let job = cx.background_executor().spawn(async move {
            resume_one_themes::thumbnail_localized_original(&original, &resume, language)
                .ok()
                .map(|bytes| Arc::new(gpui::Image::from_bytes(gpui::ImageFormat::Png, bytes)))
        });
        cx.spawn_in(window, async move |this, cx| {
            let image = job.await;
            this.update_in(cx, |this, _, cx| {
                let state = &mut this.original_preview;
                state.thumbnail_pending = false;
                if state.thumbnail_epoch == epoch {
                    state.thumbnails.push_front((key, image));
                    state.thumbnails.truncate(48);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn request_original_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.source_mode || !self.model.preview_ready {
            self.original_preview.cancel_pending(None);
            return;
        }
        self.request_original_thumbnail(window, cx);
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
        let editable = self.structure_enabled()
            && !self.insertion_active()
            && self.original_surface_visible(self.recovery_pending)
            && self.model.applied_theme().identity == key.theme;
        let state = &mut self.original_preview;
        state.cancel_pending(Some(&key));
        state.set_edit_enabled(editable);
        if let Some(index) = state
            .recent
            .iter()
            .position(|(candidate, _)| candidate == &key)
        {
            let entry = state.recent.remove(index).expect("entry was found");
            if let Ok(html) = &entry.1 {
                state.present(
                    &key,
                    html,
                    &self.model.document.value,
                    self.shell.style.colors.canvas,
                );
            }
            state.recent.push_front(entry);
        } else if !state.running && state.error.is_none() {
            state.running = true;
            let resume = self.model.document.value.clone();
            let language = key.language;
            let job = cx.background_executor().spawn(async move {
                theme
                    .render_localized_preview_html(&resume, language)
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
                                state.painted_token = None;
                                state.edit_enabled = false;
                                state.edit_revision = None;
                                state.edit_session = None;
                                state.sent = None;
                                state.canvas = None;
                                cx.notify();
                            }
                            "source" => {
                                this.toggle_source(&crate::editor_ui::ToggleSource, window, cx)
                            }
                            message if message.starts_with("reading:") => {
                                state.reading_target =
                                    serde_json::from_str::<Option<(String, Option<usize>)>>(
                                        &message[8..],
                                    )
                                    .ok()
                                    .flatten()
                                    .filter(|(id, _)| {
                                        crate::SECTION_LABELS.iter().any(|(key, _)| id == key)
                                    });
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
                            message if message.starts_with('{') => {
                                this.edit_original_field(message, window, cx);
                            }
                            _ => {
                                if let Some((token, key)) = state.sent.as_ref()
                                    && message
                                        .strip_prefix("painted:")
                                        .and_then(|value| value.parse().ok())
                                        == Some(*token)
                                {
                                    state.painted = Some(key.clone());
                                    state.painted_token = Some(*token);
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
        self.original_preview.error = Some(tr("当前平台没有可用的原版主题预览组件").into());
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
        if (visible || state.overlay_open && state.capture_pending)
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
        if let Some((cached, image)) = &state.snapshot
            && (cached == &key || state.cover_key.as_ref() == Some(cached))
        {
            return gpui::img(image.clone())
                .size_full()
                .object_fit(gpui::ObjectFit::Fill)
                .into_any_element();
        }
        if error.is_none()
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
    pub(super) fn jump_to(&mut self, section: &str, index: Option<usize>) {
        self.reading_target = Some((section.into(), index));
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        if let Some(desktop) = self.desktop.as_ref().filter(|_| self.ready) {
            let section = serde_json::to_string(section).expect("encode section");
            let index = serde_json::to_string(&index).expect("encode item index");
            let _ = desktop.evaluate(&format!("jianliJump({section}, {index})"));
        }
    }

    pub(super) fn reset_document(&mut self) {
        self.reading_target = None;
        self.hide();
        self.painted = None;
        self.sent = None;
        self.snapshot = None;
        self.cover_key = None;
        self.capture_pending = false;
        self.thumbnail_epoch = self.thumbnail_epoch.wrapping_add(1);
        self.thumbnails.clear();
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            self.capture_sequence = self.capture_sequence.wrapping_add(1);
            self.painted_token = None;
            self.edit_revision = None;
            self.edit_session = None;
            self.set_edit_enabled(false);
            if let Some(desktop) = self.desktop.as_ref().filter(|_| self.ready) {
                let _ = desktop.evaluate("jianliReset()");
            }
        }
    }

    fn cancel_pending(&mut self, keep: Option<&Key>) {
        if keep.is_none() {
            self.set_edit_enabled(false);
        }
        if self.sent.as_ref().is_some_and(|(_, key)| Some(key) != keep) {
            self.sent = None;
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            if let Some(desktop) = self.desktop.as_ref().filter(|_| self.ready) {
                let _ = desktop.evaluate("jianliCancel()");
            }
        }
    }

    fn set_edit_enabled(&mut self, enabled: bool) {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        if self.edit_enabled != enabled
            && let Some(desktop) = self.desktop.as_ref().filter(|_| self.ready)
            && desktop
                .evaluate(&format!("jianliEditEnabled({enabled})"))
                .is_ok()
        {
            self.edit_enabled = enabled;
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let _ = enabled;
    }

    pub(super) fn hide(&self) {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        if let Some(desktop) = &self.desktop {
            desktop.hide();
        }
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    fn present(
        &mut self,
        key: &Key,
        html: &str,
        resume: &serde_json::Value,
        canvas: resume_one_core::theme_style::Rgb,
    ) {
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
        let identity = serde_json::to_string(&key.theme).expect("theme identity");
        let targets = editing::targets(resume);
        let locale = key.ui_language.tag();
        if let Err(error) = desktop.evaluate(&format!(
            "jianliRender({html}, {token}, {identity}, {targets}, '{locale}')"
        )) {
            self.error = Some(error);
        } else {
            self.sent = Some((token, key.clone()));
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    fn present(
        &mut self,
        _: &Key,
        _: &str,
        _: &serde_json::Value,
        _: resume_one_core::theme_style::Rgb,
    ) {
    }
}
