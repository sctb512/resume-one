//! Reversible, non-persistent appearance trials. Exports only read committed preferences.
use super::*;
use resume_one_core::theme_style::Rgb;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ColorSource {
    Preset(usize),
    SaturationValue,
    Hue,
}

#[derive(Clone, PartialEq, Eq)]
enum Trial {
    Font {
        heading: bool,
        choice: Option<FontChoice>,
    },
    Color {
        index: usize,
        source: ColorSource,
        value: Rgb,
    },
}

#[derive(Default)]
pub(super) struct TrialState {
    current: Option<Trial>,
    generation: u64,
    resolving: bool,
    pending: Option<(u64, Appearance)>,
    pub(super) error: Option<String>,
}

impl AppearanceEditor {
    pub(super) fn clear_trial(&mut self, cx: &mut Context<Self>) {
        self.trial.generation = self.trial.generation.wrapping_add(1);
        self.trial.pending = None;
        self.trial.error = None;
        if self.trial.current.take().is_some() {
            if let Some(owner) = self.owner.upgrade() {
                owner.update(cx, |app, cx| app.show_appearance_trial(None, cx));
            }
            cx.notify();
        }
    }

    pub(super) fn hover_font(
        &mut self,
        heading: bool,
        choice: Option<FontChoice>,
        hovering: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let trial = Trial::Font { heading, choice };
        if hovering {
            if self.font_target == Some(heading) && !self.font_busy {
                self.begin_trial(trial, window, cx);
            }
        } else if self.trial.current.as_ref() == Some(&trial) {
            self.clear_trial(cx);
        }
    }

    pub(super) fn leave_font_list(&mut self, cx: &mut Context<Self>) {
        if matches!(self.trial.current, Some(Trial::Font { .. })) {
            self.clear_trial(cx);
        }
    }

    pub(super) fn hover_color(
        &mut self,
        index: usize,
        source: ColorSource,
        value: Rgb,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.palette.target == Some(index)
            && !self.palette.is_dragging()
            && !self.palette.is_sampling()
        {
            self.begin_trial(
                Trial::Color {
                    index,
                    source,
                    value,
                },
                window,
                cx,
            );
        }
    }

    pub(super) fn leave_color(
        &mut self,
        index: usize,
        source: ColorSource,
        cx: &mut Context<Self>,
    ) {
        if matches!(self.trial.current, Some(Trial::Color { index: active, source: origin, .. })
            if index == active && source == origin)
        {
            self.clear_trial(cx);
        }
    }

    fn begin_trial(&mut self, trial: Trial, window: &mut Window, cx: &mut Context<Self>) {
        if self.trial.current.as_ref() == Some(&trial) {
            return;
        }
        let Some(owner) = self.owner.upgrade() else {
            return;
        };
        let mut appearance = owner.read(cx).model.appearance.clone();
        match &trial {
            Trial::Font {
                heading: true,
                choice,
            } => appearance.heading_font = choice.clone(),
            Trial::Font {
                heading: false,
                choice,
            } => appearance.body_font = choice.clone(),
            Trial::Color { index, value, .. } => {
                if appearance
                    .set_color(COLOR_KEYS[*index], &value.hex())
                    .is_err()
                {
                    return;
                }
            }
        }
        self.trial.generation = self.trial.generation.wrapping_add(1);
        self.trial.error = None;
        self.trial.pending = None;
        let native_font = matches!(
            &trial,
            Trial::Font {
                choice: Some(FontChoice::Font(_)),
                ..
            }
        );
        self.trial.current = Some(trial);
        if native_font {
            let generation = self.trial.generation;
            if self.trial.resolving {
                // One active read and one replaceable latest request, never one per row.
                self.trial.pending = Some((generation, appearance));
            } else {
                self.resolve_trial(generation, appearance, window, cx);
            }
        } else {
            owner.update(cx, |app, cx| {
                app.show_appearance_trial(Some(appearance), cx)
            });
        }
        cx.notify();
    }

    fn resolve_trial(
        &mut self,
        generation: u64,
        appearance: Appearance,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(owner) = self.owner.upgrade() else {
            return;
        };
        let theme = appearance.apply(&owner.read(cx).model.applied_theme().manifest);
        self.trial.resolving = true;
        let task = cx
            .background_executor()
            .spawn(async move { resume_one_core::layout::check_theme(&theme) });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            this.update_in(cx, |this, window, cx| {
                this.complete_trial(generation, appearance, result, cx);
                if let Some((generation, appearance)) = this.trial.pending.take()
                    && generation == this.trial.generation
                    && this.trial.current.is_some()
                {
                    this.resolve_trial(generation, appearance, window, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn complete_trial(
        &mut self,
        generation: u64,
        appearance: Appearance,
        result: Result<(), String>,
        cx: &mut Context<Self>,
    ) {
        self.trial.resolving = false;
        let Some(owner) = self.owner.upgrade() else {
            return;
        };
        if generation != self.trial.generation
            || self.trial.current.is_none()
            || owner.read(cx).appearance_editor.as_ref() != Some(&cx.entity())
        {
            return;
        }
        match result {
            Ok(()) => owner.update(cx, |app, cx| {
                app.show_appearance_trial(Some(appearance), cx)
            }),
            Err(error) => {
                owner.update(cx, |app, cx| app.show_appearance_trial(None, cx));
                self.trial.error = Some(error);
            }
        }
        cx.notify();
    }

    pub(super) fn trial_events(&self, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.entity().downgrade();
        window.on_mouse_event(move |_: &gpui::MouseExitEvent, phase, _, cx| {
            if phase == gpui::DispatchPhase::Capture {
                view.update(cx, |this, cx| this.clear_trial(cx)).ok();
            }
        });
    }
}

impl ResumeOne {
    pub(super) fn show_appearance_trial(
        &mut self,
        appearance: Option<Appearance>,
        cx: &mut Context<Self>,
    ) {
        self.model.preview_appearance(appearance);
        crate::editor_ui::apply_editor_theme(&self.active_style(), cx);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{model::AppModel, theme_local::ThemeStore};
    use gpui::TestAppContext;
    use std::{cell::RefCell, rc::Rc};

    #[gpui::test]
    fn font_trial_queue_is_bounded_and_stale_or_closed_results_cannot_apply(
        cx: &mut TestAppContext,
    ) {
        cx.update(gpui_component::init);
        let root = tempfile::tempdir().unwrap();
        let settings = root.path().join("settings.json");
        let mut store = ThemeStore::new(settings.clone(), root.path().join("themes")).unwrap();
        store.save_appearance(Appearance::default()).unwrap();
        let saved = std::fs::read(&settings).unwrap();
        let child = Rc::new(RefCell::new(None));
        let output = child.clone();
        let (_, cx) = cx.add_window_view(|window, cx| {
            let owner = cx.new(|cx| {
                let mut app = ResumeOne::new(AppModel::load(None).unwrap(), cx);
                app.theme_store = Some(store);
                app
            });
            *output.borrow_mut() = Some(owner.clone());
            gpui_component::Root::new(owner, window, cx)
        });
        let owner = child.borrow_mut().take().unwrap();
        cx.update(|window, cx| owner.update(cx, |app, cx| app.show_appearance(window, cx)));
        let editor = cx.update(|_, cx| owner.read(cx).appearance_editor.as_ref().unwrap().clone());
        cx.update(|window, cx| {
            editor.update(cx, |this, cx| {
                this.font_target = Some(false);
                this.trial.resolving = true; // Hold the worker to exercise order deterministically.
                let first = Some(FontChoice::Font("font:First".into()));
                let latest = Some(FontChoice::Font("font:Latest".into()));
                this.hover_font(false, first.clone(), true, window, cx);
                let old = this.trial.generation;
                for _ in 0..30 {
                    this.hover_font(false, latest.clone(), true, window, cx);
                }
                let generation = this.trial.generation;
                assert!(generation > old);
                assert_eq!(this.trial.pending.as_ref().unwrap().1.body_font, latest);
                this.hover_font(false, first, false, window, cx);
                assert!(
                    this.trial.current.is_some(),
                    "an old leave cannot cancel the new row"
                );
                this.complete_trial(old, Appearance::default(), Err("stale error".into()), cx);
                assert!(this.trial.error.is_none());
                this.complete_trial(
                    generation,
                    Appearance::default(),
                    Err("unavailable font".into()),
                    cx,
                );
                assert_eq!(this.trial.error.as_deref(), Some("unavailable font"));
                this.clear_trial(cx);
                assert!(this.trial.pending.is_none());
                assert!(this.trial.error.is_none());
                this.complete_trial(generation, Appearance::default(), Ok(()), cx);
            })
        });
        cx.update(|_, cx| {
            assert_eq!(
                owner.read(cx).model.display_appearance(),
                &Appearance::default()
            )
        });
        cx.update(|window, cx| {
            editor.update(cx, |this, cx| {
                this.hover_font(false, Some(FontChoice::Serif), true, window, cx);
            })
        });
        cx.simulate_event(gpui::MouseExitEvent::default());
        cx.update(|_, cx| {
            assert_eq!(
                owner.read(cx).model.display_appearance(),
                &Appearance::default()
            )
        });
        let generation = cx.update(|window, cx| {
            editor.update(cx, |this, cx| {
                this.hover_font(false, Some(FontChoice::Serif), true, window, cx);
                this.trial.generation
            })
        });
        cx.update(|window, cx| owner.update(cx, |app, cx| app.finish_appearance(window, cx)));
        cx.update(|_, cx| {
            editor.update(cx, |this, cx| {
                let mut appearance = Appearance::default();
                appearance.body_font = Some(FontChoice::Serif);
                this.complete_trial(generation, appearance, Ok(()), cx);
            })
        });
        cx.update(|_, cx| {
            assert_eq!(
                owner.read(cx).model.display_appearance(),
                &Appearance::default()
            )
        });
        assert_eq!(std::fs::read(settings).unwrap(), saved);
    }
}
