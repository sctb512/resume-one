//! Native web surface owned by the center of the GPUI workspace.
use std::{cell::Cell, rc::Rc};

use futures_channel::mpsc;
use futures_channel::oneshot;
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
        let html = include_str!("shell.html").replace("/* EDITING */", include_str!("editing.js"));
        let webview = WebViewBuilder::new()
            .with_html(html)
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
                if message.len() <= 96 * 1024 {
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

    pub(super) fn is_visible(&self) -> bool {
        self.visible.get()
    }

    pub(super) fn snapshot(&self) -> oneshot::Receiver<gpui::Image> {
        let (send, receive) = oneshot::channel();
        if self.visible.get() {
            self.capture(move |image| {
                let _ = send.send(image);
            });
        }
        receive
    }

    #[cfg(target_os = "macos")]
    fn capture(&self, complete: impl FnOnce(gpui::Image) + 'static) {
        use objc2_app_kit::NSImage;
        use wry::WebViewExtMacOS;
        let complete = Cell::new(Some(complete));
        let block = block2::RcBlock::new(move |image: *mut NSImage, _| {
            if let Some(complete) = complete.take()
                && let Some(image) = unsafe { image.as_ref() }
                && let Some(data) = image.TIFFRepresentation()
            {
                complete(gpui::Image::from_bytes(
                    gpui::ImageFormat::Tiff,
                    data.to_vec(),
                ));
            }
        });
        // Capture only the native preview, including its current scroll position.
        unsafe {
            let view: &objc2_web_kit::WKWebView = &self.webview.webview();
            view.takeSnapshotWithConfiguration_completionHandler(None, &block);
        }
    }

    #[cfg(target_os = "windows")]
    fn capture(&self, complete: impl FnOnce(gpui::Image) + 'static) {
        use webview2_com::{
            CapturePreviewCompletedHandler,
            Microsoft::Web::WebView2::Win32::COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG,
        };
        use windows::Win32::{
            System::Com::{STATFLAG_NONAME, STATSTG, STREAM_SEEK_SET},
            UI::Shell::SHCreateMemStream,
        };
        use wry::WebViewExtWindows;

        let Some(stream) = (unsafe { SHCreateMemStream(None) }) else {
            return;
        };
        let captured = stream.clone();
        let handler = CapturePreviewCompletedHandler::create(Box::new(move |result| {
            result?;
            let mut stat = STATSTG::default();
            unsafe {
                captured.Stat(&mut stat, STATFLAG_NONAME)?;
                if stat.cbSize > 32 * 1024 * 1024 {
                    return Ok(());
                }
                captured.Seek(0, STREAM_SEEK_SET, None)?;
                let mut bytes = vec![0; stat.cbSize as usize];
                let mut read = 0;
                captured
                    .Read(
                        bytes.as_mut_ptr().cast(),
                        bytes.len() as u32,
                        Some(&mut read),
                    )
                    .ok()?;
                bytes.truncate(read as usize);
                complete(gpui::Image::from_bytes(gpui::ImageFormat::Png, bytes));
            }
            Ok(())
        }));
        unsafe {
            let _ = self.webview.webview().CapturePreview(
                COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG,
                &stream,
                &handler,
            );
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
