use super::*;
use gpui::{
    Bounds, DispatchPhase, MouseButton, MouseExitEvent, MouseMoveEvent, MouseUpEvent, Pixels,
    Point, linear_color_stop, linear_gradient, relative, rgba,
};
use gpui_component::Colorize as _;
use resume_one_core::theme_style::Rgb;
use std::{cell::Cell, rc::Rc};

#[derive(Clone, Copy, Default, Debug)]
pub(super) struct Hsv {
    h: f32,
    s: f32,
    v: f32,
}
impl Hsv {
    fn from_rgb(rgb: Rgb) -> Self {
        let r = rgb.r as f32 / 255.;
        let g = rgb.g as f32 / 255.;
        let b = rgb.b as f32 / 255.;
        let v = r.max(g).max(b);
        let min = r.min(g).min(b);
        let d = v - min;
        let h = if d <= f32::EPSILON {
            0.
        } else if v == r {
            ((g - b) / d).rem_euclid(6.)
        } else if v == g {
            (b - r) / d + 2.
        } else {
            (r - g) / d + 4.
        };
        Self {
            h: h / 6.,
            s: if v == 0. { 0. } else { d / v },
            v,
        }
    }
    fn rgb(self) -> Rgb {
        let h = self.h.rem_euclid(1.) * 6.;
        let c = self.v * self.s;
        let x = c * (1. - (h.rem_euclid(2.) - 1.).abs());
        let m = self.v - c;
        let (r, g, b) = match h as u8 {
            0 => (c, x, 0.),
            1 => (x, c, 0.),
            2 => (0., c, x),
            3 => (0., x, c),
            4 => (x, 0., c),
            _ => (c, 0., x),
        };
        let channel = |v: f32| ((v + m).clamp(0., 1.) * 255.).round() as u8;
        Rgb {
            r: channel(r),
            g: channel(g),
            b: channel(b),
        }
    }
}
#[derive(Clone, Copy)]
enum DragKind {
    SaturationValue,
    Hue,
}
#[derive(Default)]
pub(super) struct PaletteState {
    pub(super) target: Option<usize>,
    hsv: Hsv,
    drag: Option<(DragKind, Bounds<Pixels>)>,
    before: Option<Appearance>,
    sv: Rc<Cell<Option<Bounds<Pixels>>>>,
    hue: Rc<Cell<Option<Bounds<Pixels>>>>,
    precise: bool,
    sampling: bool,
    pub(super) focus: Vec<gpui::FocusHandle>,
}
impl PaletteState {
    pub(super) fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }
    pub(super) fn is_sampling(&self) -> bool {
        self.sampling
    }
    pub(super) fn is_open(&self) -> bool {
        self.target.is_some()
    }
    pub(super) fn new(cx: &mut Context<AppearanceEditor>) -> Self {
        Self {
            focus: (0..9).map(|_| cx.focus_handle()).collect(),
            ..Default::default()
        }
    }
}
pub(super) fn reveal(
    bounds: Bounds<Pixels>,
    scroll: &ScrollHandle,
    window: &mut Window,
    cx: &mut gpui::App,
) {
    let viewport = scroll.bounds();
    let mut next = scroll.offset();
    if bounds.top() < viewport.top() + px(10.) {
        next.y += viewport.top() + px(10.) - bounds.top();
    } else if bounds.bottom() > viewport.bottom() - px(10.) {
        next.y -= bounds.bottom() - viewport.bottom() + px(10.);
    }
    next.y = next.y.max(-scroll.max_offset().height).min(px(0.));
    if (next.y - scroll.offset().y).abs() > px(0.5) {
        scroll.set_offset(next);
        window.defer(cx, |window, _| window.refresh());
    }
}

impl AppearanceEditor {
    pub(super) fn sync_palette_color(&mut self, index: usize, value: &str) {
        if value.trim().is_empty()
            && self.palette.target == Some(index)
            && self.palette.drag.is_none()
        {
            self.palette.hsv = Hsv::from_rgb(self.default_colors[index]);
            return;
        }
        if self.palette.target == Some(index)
            && self.palette.drag.is_none()
            && let Ok(hsla) = gpui::Hsla::parse_hex(value)
        {
            let rgba: gpui::Rgba = hsla.into();
            self.palette.hsv = Hsv::from_rgb(Rgb {
                r: (rgba.r * 255.).round() as u8,
                g: (rgba.g * 255.).round() as u8,
                b: (rgba.b * 255.).round() as u8,
            });
        }
    }
    pub(super) fn palette_escape(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette.sampling {
            return;
        }
        self.clear_trial(cx);
        if self.palette.drag.is_some() {
            self.end_color_drag(true, window, cx);
        } else {
            if let Some(index) = self.palette.target.take() {
                window.focus(&self.palette.focus[index]);
            }
        }
        cx.notify();
    }
    pub(super) fn open_color(
        &mut self,
        index: usize,
        value: Rgb,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.clear_trial(cx);
        if self.palette.target == Some(index) {
            self.palette.target = None;
        } else {
            self.palette.target = Some(index);
            self.palette.hsv = Hsv::from_rgb(value);
            window.focus(&self.palette.focus[7]);
        }
        cx.notify();
    }
    fn apply_hsv(&mut self, commit: bool, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(index), Some(owner)) = (self.palette.target, self.owner.upgrade()) else {
            return;
        };
        let mut appearance = owner.read(cx).model.appearance.clone();
        let hex = self.palette.hsv.rgb().hex();
        if appearance.set_color(COLOR_KEYS[index], &hex).is_err() {
            return;
        }
        if commit {
            self.apply(appearance, window, cx);
            if self.error.is_none() {
                self.inputs[index].update(cx, |input, cx| input.set_value(hex, window, cx));
            }
        } else {
            owner.update(cx, |app, cx| {
                app.model.set_appearance(appearance);
                crate::editor_ui::apply_editor_theme(&app.model.selected_style(), cx);
                cx.notify();
            });
        }
        cx.notify();
    }
    fn start_color_drag(
        &mut self,
        kind: DragKind,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.clear_trial(cx);
        let bounds = match kind {
            DragKind::SaturationValue => self.palette.sv.get(),
            DragKind::Hue => self.palette.hue.get(),
        };
        let Some(bounds) = bounds else {
            return;
        };
        window.focus(
            &self.palette.focus[match kind {
                DragKind::SaturationValue => 7,
                DragKind::Hue => 8,
            }],
        );
        self.palette.before = self
            .owner
            .upgrade()
            .map(|owner| owner.read(cx).model.appearance.clone());
        self.palette.drag = Some((kind, bounds));
        self.move_color_drag(position, window, cx);
    }
    fn move_color_drag(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((kind, bounds)) = self.palette.drag else {
            return;
        };
        let x =
            (f32::from(position.x - bounds.left()) / f32::from(bounds.size.width)).clamp(0., 1.);
        let y =
            (f32::from(position.y - bounds.top()) / f32::from(bounds.size.height)).clamp(0., 1.);
        match kind {
            DragKind::SaturationValue => {
                self.palette.hsv.s = x;
                self.palette.hsv.v = 1. - y;
            }
            DragKind::Hue => self.palette.hsv.h = x.min(0.999999),
        }
        self.apply_hsv(false, window, cx);
    }

    fn hover_color_at(
        &mut self,
        kind: DragKind,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.palette.target else {
            return;
        };
        let (bounds, source) = match kind {
            DragKind::SaturationValue => {
                (self.palette.sv.get(), trial::ColorSource::SaturationValue)
            }
            DragKind::Hue => (self.palette.hue.get(), trial::ColorSource::Hue),
        };
        let Some(bounds) = bounds else { return };
        let x =
            (f32::from(position.x - bounds.left()) / f32::from(bounds.size.width)).clamp(0., 1.);
        let y =
            (f32::from(position.y - bounds.top()) / f32::from(bounds.size.height)).clamp(0., 1.);
        let mut hsv = self.palette.hsv;
        match kind {
            DragKind::SaturationValue => {
                hsv.s = x;
                hsv.v = 1. - y;
            }
            DragKind::Hue => hsv.h = x.min(0.999999),
        }
        self.hover_color(index, source, hsv.rgb(), window, cx);
    }
    fn end_color_drag(&mut self, cancel: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette.drag.take().is_none() {
            return;
        }
        if cancel {
            if let Some(before) = self.palette.before.take() {
                self.apply(before, window, cx);
            }
        } else {
            self.palette.before = None;
            self.apply_hsv(true, window, cx);
        }
        cx.notify();
    }
    fn sample_color(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette.sampling {
            return;
        }
        let Some(index) = self.palette.target else {
            return;
        };
        self.clear_trial(cx);
        self.palette.sampling = true;
        self.error = None;
        cx.notify();
        let sample = crate::color_sampler::sample();
        cx.spawn_in(window, async move |this, cx| {
            let result = sample
                .await
                .unwrap_or_else(|_| Err("取色会话已关闭".into()));
            this.update_in(cx, |this, window, cx| {
                this.finish_sample(index, result, window, cx);
            })
            .ok();
        })
        .detach();
    }

    fn finish_sample(
        &mut self,
        index: usize,
        result: crate::color_sampler::Selection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.palette.sampling = false;
        match result {
            Ok(Some(rgb)) => {
                self.palette.target = Some(index);
                self.palette.hsv = Hsv::from_rgb(rgb);
                self.apply_hsv(true, window, cx);
            }
            Ok(None) => {}
            Err(error) => self.error = Some(error),
        }
        window.focus(&self.palette.focus[7]);
        cx.notify();
    }
    pub(super) fn color_row(
        &self,
        index: usize,
        value: Rgb,
        p: chrome::Palette,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let selector = format!("appearance-color-{}", COLOR_KEYS[index]);
        let raw = self
            .owner
            .upgrade()
            .and_then(|owner| {
                owner
                    .read(cx)
                    .model
                    .appearance
                    .color(COLOR_KEYS[index])
                    .and_then(|v| gpui::Hsla::parse_hex(v).ok())
            })
            .map(|v| {
                let rgba: gpui::Rgba = v.into();
                Rgb {
                    r: (rgba.r * 255.).round() as u8,
                    g: (rgba.g * 255.).round() as u8,
                    b: (rgba.b * 255.).round() as u8,
                }
            })
            .unwrap_or(value);
        let active = self.palette.target == Some(index);
        let focus = self.palette.focus[index].clone();
        let watched = focus.clone();
        let scroll = self.scroll.clone();
        div()
            .on_children_prepainted(move |children, window, cx| {
                if watched.is_focused(window)
                    && let Some(bounds) = children.first()
                {
                    reveal(*bounds, &scroll, window, cx);
                }
            })
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .w(px(66.))
                            .flex_none()
                            .text_size(px(12.))
                            .text_color(color(p.muted))
                            .child(color_label(COLOR_KEYS[index])),
                    )
                    .child(
                        div()
                            .id(gpui::SharedString::from(selector.clone()))
                            .debug_selector(move || selector.clone())
                            .flex_1()
                            .min_w_0()
                            .h(px(32.))
                            .px_2()
                            .flex()
                            .items_center()
                            .justify_between()
                            .rounded(px(5.))
                            .border_1()
                            .border_color(color(if active { p.accent } else { p.line }))
                            .bg(color(p.panel))
                            .cursor_pointer()
                            .tab_index(0)
                            .tab_stop(true)
                            .track_focus(&focus)
                            .focus(|v| v.border_color(color(p.accent)))
                            .child(
                                div()
                                    .flex_1()
                                    .h(px(18.))
                                    .rounded(px(3.))
                                    .border_1()
                                    .border_color(color(p.line))
                                    .bg(color(raw)),
                            )
                            .child(div().ml_2().text_size(px(12.)).child("⌄"))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.open_color(index, raw, window, cx)
                            }))
                            .on_key_down(cx.listener(move |this, event, window, cx| {
                                if chrome::activation_key(event) {
                                    cx.stop_propagation();
                                    this.open_color(index, raw, window, cx);
                                }
                            })),
                    ),
            )
            .when(active, |v| v.child(self.color_palette(p, cx)))
            .into_any_element()
    }
    fn color_palette(&self, p: chrome::Palette, cx: &mut Context<Self>) -> gpui::AnyElement {
        let hue = Hsv {
            h: self.palette.hsv.h,
            s: 1.,
            v: 1.,
        }
        .rgb();
        let sv_bounds = self.palette.sv.clone();
        let hue_bounds = self.palette.hue.clone();
        let sv_focus = self.palette.focus[7].clone();
        let hue_focus = self.palette.focus[8].clone();
        let sv_scroll = self.scroll.clone();
        let hue_scroll = self.scroll.clone();
        let mut hues = div().flex().size_full();
        for segment in 0..6 {
            let from = Hsv {
                h: segment as f32 / 6.,
                s: 1.,
                v: 1.,
            }
            .rgb();
            let to = Hsv {
                h: (segment + 1) as f32 / 6.,
                s: 1.,
                v: 1.,
            }
            .rgb();
            hues = hues.child(div().h_full().flex_1().bg(linear_gradient(
                90.,
                linear_color_stop(color(from), 0.),
                linear_color_stop(color(to), 1.),
            )));
        }
        let index = self.palette.target.unwrap();
        let mut presets = div().grid().grid_cols(8).gap_1();
        for (i, hex) in [
            "#182322", "#FFFFFF", "#52756B", "#8C5B4C", "#B84A3C", "#587B9E", "#B28A4B", "#756582",
        ]
        .iter()
        .enumerate()
        {
            let rgb: gpui::Rgba = gpui::Hsla::parse_hex(hex).unwrap().into();
            let rgb = Rgb {
                r: (rgb.r * 255.).round() as u8,
                g: (rgb.g * 255.).round() as u8,
                b: (rgb.b * 255.).round() as u8,
            };
            presets = presets.child(
                div()
                    .id(("color-preset", i))
                    .debug_selector(move || format!("appearance-color-preset-{i}"))
                    .h(px(20.))
                    .rounded(px(3.))
                    .border_1()
                    .border_color(color(p.line))
                    .bg(color(rgb))
                    .cursor_pointer()
                    .tab_index(0)
                    .tab_stop(true)
                    .on_hover(cx.listener(move |this, hovering: &bool, window, cx| {
                        if *hovering {
                            this.hover_color(index, trial::ColorSource::Preset(i), rgb, window, cx);
                        } else {
                            this.leave_color(index, trial::ColorSource::Preset(i), cx);
                        }
                    }))
                    .on_mouse_move(cx.listener(move |this, _: &MouseMoveEvent, window, cx| {
                        this.hover_color(index, trial::ColorSource::Preset(i), rgb, window, cx);
                    }))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.palette.hsv = Hsv::from_rgb(rgb);
                        this.apply_hsv(true, window, cx);
                    }))
                    .on_key_down(cx.listener(move |this, event, window, cx| {
                        if chrome::activation_key(event) {
                            cx.stop_propagation();
                            this.palette.hsv = Hsv::from_rgb(rgb);
                            this.apply_hsv(true, window, cx);
                        }
                    })),
            );
        }
        let watched = self.inputs[index].clone();
        let scroll = self.scroll.clone();
        div()
            .id("appearance-color-palette")
            .debug_selector(|| "appearance-color-palette".into())
            .flex()
            .flex_col()
            .gap_3()
            .p_2()
            .rounded(px(6.))
            .border_1()
            .border_color(color(p.line))
            .bg(color(p.panel))
            .child(
                div()
                    .on_children_prepainted(move |children, window, cx| {
                        sv_bounds.set(children.first().copied());
                        if sv_focus.is_focused(window)
                            && let Some(bounds) = children.first()
                        {
                            reveal(*bounds, &sv_scroll, window, cx);
                        }
                    })
                    .id("color-sv")
                    .track_focus(&self.palette.focus[7])
                    .debug_selector(|| "appearance-color-sv".into())
                    .relative()
                    .h(px(118.))
                    .w_full()
                    .rounded(px(4.))
                    .overflow_hidden()
                    .cursor_crosshair()
                    .tab_index(0)
                    .tab_stop(true)
                    .bg(linear_gradient(
                        90.,
                        linear_color_stop(gpui::white(), 0.),
                        linear_color_stop(color(hue), 1.),
                    ))
                    .child(div().absolute().inset_0().bg(linear_gradient(
                        180.,
                        linear_color_stop(rgba(0x00000000), 0.),
                        linear_color_stop(gpui::black(), 1.),
                    )))
                    .child(
                        div()
                            .absolute()
                            .left(relative(self.palette.hsv.s))
                            .top(relative(1. - self.palette.hsv.v))
                            .ml(px(-5.))
                            .mt(px(-5.))
                            .size(px(10.))
                            .rounded_full()
                            .border_2()
                            .border_color(gpui::white())
                            .shadow_sm(),
                    )
                    .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, window, cx| {
                        this.hover_color_at(DragKind::SaturationValue, event.position, window, cx);
                    }))
                    .on_hover(cx.listener(move |this, hovering: &bool, _, cx| {
                        if !*hovering {
                            this.leave_color(index, trial::ColorSource::SaturationValue, cx);
                        }
                    }))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &gpui::MouseDownEvent, window, cx| {
                            cx.stop_propagation();
                            this.start_color_drag(
                                DragKind::SaturationValue,
                                event.position,
                                window,
                                cx,
                            );
                        }),
                    )
                    .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                        let step = if event.keystroke.modifiers.shift {
                            0.1
                        } else {
                            0.01
                        };
                        match event.keystroke.key.as_str() {
                            "left" => this.palette.hsv.s = (this.palette.hsv.s - step).max(0.),
                            "right" => this.palette.hsv.s = (this.palette.hsv.s + step).min(1.),
                            "up" => this.palette.hsv.v = (this.palette.hsv.v + step).min(1.),
                            "down" => this.palette.hsv.v = (this.palette.hsv.v - step).max(0.),
                            _ => return,
                        }
                        cx.stop_propagation();
                        this.apply_hsv(true, window, cx);
                    })),
            )
            .child(
                div()
                    .on_children_prepainted(move |children, window, cx| {
                        hue_bounds.set(children.first().copied());
                        if hue_focus.is_focused(window)
                            && let Some(bounds) = children.first()
                        {
                            reveal(*bounds, &hue_scroll, window, cx);
                        }
                    })
                    .id("color-hue")
                    .track_focus(&self.palette.focus[8])
                    .debug_selector(|| "appearance-color-hue".into())
                    .relative()
                    .h(px(16.))
                    .rounded(px(4.))
                    .overflow_hidden()
                    .cursor_crosshair()
                    .tab_index(0)
                    .tab_stop(true)
                    .child(hues)
                    .child(
                        div()
                            .absolute()
                            .left(relative(self.palette.hsv.h))
                            .ml(px(-3.))
                            .top_0()
                            .bottom_0()
                            .w(px(6.))
                            .border_1()
                            .border_color(gpui::white())
                            .rounded(px(2.)),
                    )
                    .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, window, cx| {
                        this.hover_color_at(DragKind::Hue, event.position, window, cx);
                    }))
                    .on_hover(cx.listener(move |this, hovering: &bool, _, cx| {
                        if !*hovering {
                            this.leave_color(index, trial::ColorSource::Hue, cx);
                        }
                    }))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &gpui::MouseDownEvent, window, cx| {
                            cx.stop_propagation();
                            this.start_color_drag(DragKind::Hue, event.position, window, cx);
                        }),
                    )
                    .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                        let step = match event.keystroke.key.as_str() {
                            "left" => -0.01,
                            "right" => 0.01,
                            _ => return,
                        };
                        this.palette.hsv.h = (this.palette.hsv.h + step).rem_euclid(1.);
                        cx.stop_propagation();
                        this.apply_hsv(true, window, cx);
                    })),
            )
            .child(presets)
            .child(
                div()
                    .id("sample-appearance-color")
                    .debug_selector(|| "sample-appearance-color".into())
                    .h(px(30.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(4.))
                    .border_1()
                    .border_color(color(p.line))
                    .cursor_pointer()
                    .tab_index(0)
                    .tab_stop(true)
                    .text_size(px(12.))
                    .child(if self.palette.sampling {
                        "点击屏幕取色 · Esc 取消"
                    } else {
                        "从屏幕取色"
                    })
                    .on_click(cx.listener(|this, _, window, cx| this.sample_color(window, cx)))
                    .on_key_down(cx.listener(|this, event, window, cx| {
                        if chrome::activation_key(event) {
                            cx.stop_propagation();
                            this.sample_color(window, cx);
                        }
                    })),
            )
            .child(
                div()
                    .id("appearance-precise-color")
                    .debug_selector(|| "appearance-precise-color".into())
                    .h(px(24.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .cursor_pointer()
                    .tab_index(0)
                    .tab_stop(true)
                    .text_size(px(11.))
                    .text_color(color(p.muted))
                    .child("精确值")
                    .child(self.palette.hsv.rgb().hex())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.palette.precise = !this.palette.precise;
                        cx.notify();
                    }))
                    .on_key_down(cx.listener(|this, event, _, cx| {
                        if chrome::activation_key(event) {
                            cx.stop_propagation();
                            this.palette.precise = !this.palette.precise;
                            cx.notify();
                        }
                    })),
            )
            .when(self.palette.precise, |v| {
                v.child(
                    div()
                        .on_children_prepainted(move |children, window, cx| {
                            if watched.read(cx).focus_handle(cx).is_focused(window)
                                && let Some(bounds) = children.first()
                            {
                                reveal(*bounds, &scroll, window, cx);
                            }
                        })
                        .child(Input::new(&self.inputs[index]).small()),
                )
            })
            .into_any_element()
    }
    pub(super) fn palette_events(&self, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette.target.is_none() {
            return;
        }
        let view = cx.entity().downgrade();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
            if phase == DispatchPhase::Capture {
                let active = view
                    .update(cx, |this, cx| {
                        let active = this.palette.drag.is_some();
                        this.move_color_drag(event.position, window, cx);
                        active
                    })
                    .unwrap_or(false);
                if active {
                    cx.stop_propagation();
                }
            }
        });
        let view = cx.entity().downgrade();
        window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
            if phase == DispatchPhase::Capture && event.button == MouseButton::Left {
                let active = view
                    .update(cx, |this, cx| {
                        let active = this.palette.drag.is_some();
                        this.end_color_drag(false, window, cx);
                        active
                    })
                    .unwrap_or(false);
                if active {
                    cx.stop_propagation();
                }
            }
        });
        let view = cx.entity().downgrade();
        window.on_mouse_event(move |_: &MouseExitEvent, phase, window, cx| {
            if phase == DispatchPhase::Capture {
                view.update(cx, |this, cx| this.end_color_drag(false, window, cx))
                    .ok();
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{model::AppModel, theme_local::ThemeStore};
    use gpui::{Modifiers, TestAppContext, point};
    use std::{cell::RefCell, rc::Rc};

    #[gpui::test]
    fn palette_pointer_previews_without_drag_and_escape_or_window_exit_restores(
        cx: &mut TestAppContext,
    ) {
        cx.update(gpui_component::init);
        cx.update(crate::inline_ui::register);
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
        let button = cx.debug_bounds("appearance-color-accent").unwrap().center();
        cx.simulate_click(button, Modifiers::none());
        let hsv = cx.update(|_, cx| editor.read(cx).palette.hsv.rgb());
        let sv = cx.debug_bounds("appearance-color-sv").unwrap();
        cx.simulate_mouse_move(sv.origin + point(px(5.), px(5.)), None, Modifiers::none());
        cx.update(|_, cx| {
            let app = owner.read(cx);
            assert!(app.model.display_appearance().color("accent").is_some());
            assert!(app.model.appearance.color("accent").is_none());
            assert_eq!(
                editor.read(cx).palette.hsv.rgb(),
                hsv,
                "hover cannot move the confirmed marker"
            );
        });
        let hue = cx.debug_bounds("appearance-color-hue").unwrap();
        cx.simulate_mouse_move(hue.center(), None, Modifiers::none());
        cx.update(|_, cx| {
            assert!(
                owner
                    .read(cx)
                    .model
                    .display_appearance()
                    .color("accent")
                    .is_some()
            )
        });
        assert_eq!(std::fs::read(&settings).unwrap(), saved);
        cx.simulate_keystrokes("escape");
        cx.update(|_, cx| {
            assert!(
                owner
                    .read(cx)
                    .model
                    .display_appearance()
                    .color("accent")
                    .is_none()
            );
            assert!(editor.read(cx).palette.target.is_none());
        });
        cx.simulate_click(button, Modifiers::none());
        let preset = cx
            .debug_bounds("appearance-color-preset-4")
            .unwrap()
            .center();
        cx.simulate_mouse_move(preset, None, Modifiers::none());
        cx.update(|_, cx| {
            assert_eq!(
                owner.read(cx).model.display_appearance().color("accent"),
                Some("#B84A3C")
            )
        });
        cx.simulate_event(MouseExitEvent::default());
        cx.update(|_, cx| {
            assert!(
                owner
                    .read(cx)
                    .model
                    .display_appearance()
                    .color("accent")
                    .is_none()
            )
        });
        assert_eq!(std::fs::read(settings).unwrap(), saved);
    }

    #[gpui::test]
    fn sampler_result_cancel_and_failure_keep_document_clean_and_restore_focus(
        cx: &mut TestAppContext,
    ) {
        cx.update(gpui_component::init);
        let root = tempfile::tempdir().unwrap();
        let settings = root.path().join("settings.json");
        let store = ThemeStore::new(settings.clone(), root.path().join("themes")).unwrap();
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
        let original = cx.update(|_, cx| owner.read(cx).model.document.source.clone());
        // These are callback-contract tests, not synthetic claims that the OS
        // sampler has received a real desktop click or cancellation gesture.
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.palette.sampling = true;
                editor.finish_sample(
                    0,
                    Ok(Some(Rgb {
                        r: 51,
                        g: 102,
                        b: 153,
                    })),
                    window,
                    cx,
                );
                assert!(!editor.palette.sampling);
                assert!(editor.palette.focus[7].is_focused(window));
            })
        });
        cx.run_until_parked();
        let saved = std::fs::read(&settings).unwrap();
        cx.update(|window, cx| {
            assert_eq!(
                owner.read(cx).model.appearance.color("accent"),
                Some("#336699")
            );
            editor.update(cx, |editor, cx| {
                editor.palette.sampling = true;
                editor.finish_sample(0, Ok(None), window, cx);
                assert!(!editor.palette.sampling);
                assert!(editor.palette.focus[7].is_focused(window));
                editor.palette.sampling = true;
                editor.finish_sample(0, Err("test-native-sampler-error".into()), window, cx);
                assert_eq!(editor.error.as_deref(), Some("test-native-sampler-error"));
                assert!(!editor.palette.sampling);
            });
            assert_eq!(owner.read(cx).model.document.source, original);
            assert_eq!(
                owner.read(cx).model.appearance.color("accent"),
                Some("#336699")
            );
        });
        assert_eq!(std::fs::read(&settings).unwrap(), saved);
    }

    #[gpui::test]
    fn palette_drag_is_live_without_disk_writes_until_release_and_escape_reverts(
        cx: &mut TestAppContext,
    ) {
        cx.update(gpui_component::init);
        cx.update(crate::inline_ui::register);
        let root = tempfile::tempdir().unwrap();
        let settings = root.path().join("settings.json");
        let mut store = ThemeStore::new(settings.clone(), root.path().join("themes")).unwrap();
        store.save_appearance(Appearance::default()).unwrap();
        let original = std::fs::read(&settings).unwrap();
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
        cx.update(|window, _| window.refresh());
        let right = cx.debug_bounds("module-panel").unwrap();
        let paper = cx.debug_bounds("resume-paper").unwrap();
        let source = cx.update(|_, cx| owner.read(cx).model.document.source.clone());
        let button = cx.debug_bounds("appearance-color-accent").unwrap().center();
        cx.simulate_click(button, Modifiers::none());
        let palette = cx.debug_bounds("appearance-color-palette").unwrap();
        assert!(palette.left() >= right.left() && palette.right() <= right.right());
        let sv = cx.debug_bounds("appearance-color-sv").unwrap();
        cx.simulate_mouse_down(sv.center(), MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_move(
            point(sv.right() - px(10.), sv.top() + px(10.)),
            Some(MouseButton::Left),
            Modifiers::none(),
        );
        cx.run_until_parked();
        assert_eq!(std::fs::read(&settings).unwrap(), original);
        cx.update(|_, cx| assert!(owner.read(cx).model.appearance.color("accent").is_some()));
        cx.simulate_mouse_up(paper.center(), MouseButton::Left, Modifiers::none());
        cx.run_until_parked();
        let saved = std::fs::read(&settings).unwrap();
        assert_ne!(saved, original);
        assert_eq!(paper, cx.debug_bounds("resume-paper").unwrap());
        assert_eq!(right, cx.debug_bounds("module-panel").unwrap());
        let committed = cx.update(|_, cx| owner.read(cx).model.appearance.clone());
        cx.simulate_mouse_down(
            sv.origin + point(px(5.), px(5.)),
            MouseButton::Left,
            Modifiers::none(),
        );
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        cx.update(|_, cx| {
            assert_eq!(owner.read(cx).model.appearance, committed);
            assert_eq!(owner.read(cx).model.document.source, source);
            assert!(owner.read(cx).appearance_editor.is_some());
        });
        assert_eq!(std::fs::read(&settings).unwrap(), saved);
        cx.update(|window, cx| {
            let editor = owner.read(cx).appearance_editor.as_ref().unwrap().clone();
            editor.update(cx, |editor, cx| {
                editor.edit_color(0, "", true, window, cx);
                assert_eq!(editor.palette.hsv.rgb(), editor.default_colors[0]);
            });
            assert!(owner.read(cx).model.appearance.color("accent").is_none());
            editor.update(cx, |editor, _| editor.palette.sampling = true);
            window.focus(&owner.read(cx).focus);
        });
        // Even when focus has moved outside the inspector, a system sampler
        // must not let workspace Escape dismiss its owning inspector.
        cx.simulate_keystrokes("escape");
        cx.update(|_, cx| {
            let editor = owner.read(cx).appearance_editor.as_ref().unwrap().clone();
            assert!(editor.read(cx).palette.target.is_some());
            editor.update(cx, |editor, _| editor.palette.sampling = false);
        });
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        cx.update(|_, cx| {
            let editor = owner.read(cx).appearance_editor.as_ref().unwrap();
            assert!(editor.read(cx).palette.target.is_none());
        });
    }

    #[test]
    fn hsv_roundtrips_grays_and_saturated_colors_without_drift() {
        for r in (0..=255).step_by(17) {
            for g in (0..=255).step_by(17) {
                for b in (0..=255).step_by(17) {
                    let color = Rgb { r, g, b };
                    assert_eq!(Hsv::from_rgb(color).rgb(), color);
                }
            }
        }
    }
}
