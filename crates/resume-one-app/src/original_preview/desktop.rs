//! Native web surface owned by the center of the GPUI workspace.
use std::{cell::Cell, rc::Rc};

use futures_channel::mpsc;
use gpui::{App, Bounds, IntoElement, Pixels, Window, canvas, prelude::*};
use wry::{
    NewWindowResponse, Rect, WebView, WebViewBuilder, dpi::LogicalPosition, dpi::LogicalSize,
};

pub(super) struct Desktop {
    webview: WebView,
    visible: Cell<bool>,
    bounds: Cell<Option<Bounds<Pixels>>>,
}

impl Desktop {
    pub(super) fn open(
        window: &Window,
        sender: mpsc::UnboundedSender<String>,
    ) -> Result<Rc<Self>, String> {
        let webview = WebViewBuilder::new()
            .with_html(include_str!("shell.html"))
            .with_incognito(true)
            .with_focused(false)
            .with_visible(false)
            .with_devtools(false)
            .with_autoplay(false)
            .with_hotkeys_zoom(false)
            .with_navigation_handler(|url| matches!(url.as_str(), "about:blank" | "about:srcdoc"))
            .with_new_window_req_handler(|_, _| NewWindowResponse::Deny)
            .with_download_started_handler(|_, _| false)
            .with_drag_drop_handler(|_| true)
            .with_ipc_handler(move |request| {
                let message: &String = request.body();
                if message.len() <= 64 {
                    let _ = sender.unbounded_send(message.clone());
                }
            })
            .build_as_child(window)
            .map_err(|error| format!("无法打开系统预览组件：{error}"))?;
        Ok(Rc::new(Self {
            webview,
            visible: Cell::new(false),
            bounds: Cell::new(None),
        }))
    }

    pub(super) fn available(window: &Window) -> bool {
        #[cfg(test)]
        {
            let _ = window;
            false
        }
        #[cfg(not(test))]
        raw_window_handle::HasWindowHandle::window_handle(window).is_ok()
    }

    pub(super) fn evaluate(&self, script: &str) -> Result<(), String> {
        self.webview
            .evaluate_script(script)
            .map_err(|error| format!("原版预览更新失败：{error}"))
    }

    pub(super) fn hide(&self) {
        if self.visible.replace(false) {
            let _ = self.webview.set_visible(false);
        }
    }

    pub(super) fn element(self: &Rc<Self>) -> impl IntoElement + use<> {
        let desktop = self.clone();
        canvas(
            move |bounds, window: &mut Window, _: &mut App| {
                let bounds = bounds.intersect(&window.content_mask().bounds);
                if bounds.is_empty() {
                    desktop.hide();
                    return;
                }
                if desktop.bounds.get() != Some(bounds) {
                    let position = LogicalPosition::new(
                        f64::from(bounds.origin.x),
                        f64::from(bounds.origin.y),
                    );
                    let size = LogicalSize::new(
                        f64::from(bounds.size.width),
                        f64::from(bounds.size.height),
                    );
                    if desktop
                        .webview
                        .set_bounds(Rect {
                            position: position.into(),
                            size: size.into(),
                        })
                        .is_err()
                    {
                        desktop.hide();
                        return;
                    }
                    desktop.bounds.set(Some(bounds));
                }
                if !desktop.visible.get() && desktop.webview.set_visible(true).is_ok() {
                    desktop.visible.set(true);
                }
            },
            |_, _, _, _| {},
        )
        .size_full()
    }
}
