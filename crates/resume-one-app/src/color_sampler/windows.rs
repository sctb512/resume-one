//! Frozen-screen, click-consuming sampling window; never clicks another app.
use super::{Rgb, Selection};
use std::{
    ptr::null_mut,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM},
    Graphics::Gdi::*,
    System::LibraryLoader::GetModuleHandleW,
    UI::{HiDpi::*, Input::KeyboardAndMouse::SetFocus, WindowsAndMessaging::*},
};

struct Capture {
    dc: HDC,
    bitmap: HBITMAP,
    old: HGDIOBJ,
    left: i32,
    top: i32,
    width: i32,
    height: i32,
}
impl Drop for Capture {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            DeleteObject(self.bitmap);
            DeleteDC(self.dc);
        }
    }
}
impl Capture {
    unsafe fn new() -> Result<Self, String> {
        // All GDI resources are either returned under RAII or released below.
        unsafe {
            let left = GetSystemMetrics(SM_XVIRTUALSCREEN);
            let top = GetSystemMetrics(SM_YVIRTUALSCREEN);
            let width = GetSystemMetrics(SM_CXVIRTUALSCREEN);
            let height = GetSystemMetrics(SM_CYVIRTUALSCREEN);
            if width <= 0 || height <= 0 || i64::from(width) * i64::from(height) > 64 * 1024 * 1024
            {
                return Err("屏幕尺寸超出取色范围".into());
            }
            let screen = GetDC(null_mut());
            if screen.is_null() {
                return Err("无法读取屏幕".into());
            }
            let dc = CreateCompatibleDC(screen);
            let bitmap = CreateCompatibleBitmap(screen, width, height);
            if dc.is_null() || bitmap.is_null() {
                if !dc.is_null() {
                    DeleteDC(dc);
                }
                if !bitmap.is_null() {
                    DeleteObject(bitmap);
                }
                ReleaseDC(null_mut(), screen);
                return Err("无法创建取色缓冲".into());
            }
            let old = SelectObject(dc, bitmap);
            let copied = BitBlt(
                dc,
                0,
                0,
                width,
                height,
                screen,
                left,
                top,
                SRCCOPY | CAPTUREBLT,
            );
            ReleaseDC(null_mut(), screen);
            let capture = Self {
                dc,
                bitmap,
                old,
                left,
                top,
                width,
                height,
            };
            if copied == 0 {
                return Err("无法读取屏幕像素".into());
            }
            Ok(capture)
        }
    }
}
struct Session {
    capture: Capture,
    sender: Option<futures_channel::oneshot::Sender<Selection>>,
    result: Selection,
    started: Instant,
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // The session Box outlives this window and its message loop. Only this
    // thread handles messages; WM_NCCREATE installs the pointer before use.
    unsafe {
        if message == WM_NCCREATE {
            let create = &*(lparam as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
            return 1;
        }
        let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Session;
        if ptr.is_null() {
            return DefWindowProcW(hwnd, message, wparam, lparam);
        }
        let session = &mut *ptr;
        match message {
            WM_PAINT => {
                let mut paint: PAINTSTRUCT = std::mem::zeroed();
                let dc = BeginPaint(hwnd, &mut paint);
                BitBlt(
                    dc,
                    0,
                    0,
                    session.capture.width,
                    session.capture.height,
                    session.capture.dc,
                    0,
                    0,
                    SRCCOPY,
                );
                EndPaint(hwnd, &paint);
                0
            }
            WM_LBUTTONUP => {
                let mut point = POINT { x: 0, y: 0 };
                session.result = if GetCursorPos(&mut point) == 0 {
                    Err("无法读取取色位置".into())
                } else {
                    let pixel = GetPixel(
                        session.capture.dc,
                        point.x - session.capture.left,
                        point.y - session.capture.top,
                    );
                    if pixel == CLR_INVALID {
                        Err("取色位置无效".into())
                    } else {
                        Ok(Some(Rgb {
                            r: (pixel & 255) as u8,
                            g: ((pixel >> 8) & 255) as u8,
                            b: ((pixel >> 16) & 255) as u8,
                        }))
                    }
                };
                DestroyWindow(hwnd);
                0
            }
            WM_KEYDOWN if wparam == 0x1B => {
                DestroyWindow(hwnd);
                0
            }
            WM_RBUTTONUP | WM_CLOSE | WM_DISPLAYCHANGE => {
                DestroyWindow(hwnd);
                0
            }
            WM_TIMER => {
                if session.started.elapsed() > Duration::from_secs(60)
                    || session.sender.as_ref().is_none_or(|s| s.is_canceled())
                {
                    DestroyWindow(hwnd);
                }
                0
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                0
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }
}
pub(super) fn run(sender: futures_channel::oneshot::Sender<Selection>) {
    // A session-local topmost window consumes both press and release. Nothing
    // is injected into another window, and the screenshot exists only in RAM.
    unsafe {
        SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let capture = match Capture::new() {
            Ok(capture) => capture,
            Err(error) => {
                let _ = sender.send(Err(error));
                return;
            }
        };
        let class: Vec<u16> = "ResumeOneColorSampler\0".encode_utf16().collect();
        let instance = GetModuleHandleW(null_mut());
        let mut wc: WNDCLASSW = std::mem::zeroed();
        wc.lpfnWndProc = Some(window_proc);
        wc.hInstance = instance;
        wc.lpszClassName = class.as_ptr();
        wc.hCursor = LoadCursorW(null_mut(), IDC_CROSS);
        RegisterClassW(&wc);
        let mut session = Box::new(Session {
            capture,
            sender: Some(sender),
            result: Ok(None),
            started: Instant::now(),
        });
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            class.as_ptr(),
            class.as_ptr(),
            WS_POPUP,
            session.capture.left,
            session.capture.top,
            session.capture.width,
            session.capture.height,
            null_mut(),
            null_mut(),
            instance,
            (&mut *session as *mut Session).cast(),
        );
        if hwnd.is_null() {
            let _ = session
                .sender
                .take()
                .unwrap()
                .send(Err("无法打开屏幕取色器".into()));
            return;
        }
        SetTimer(hwnd, 1, 100, None);
        ShowWindow(hwnd, SW_SHOW);
        SetForegroundWindow(hwnd);
        SetFocus(hwnd);
        let mut message: MSG = std::mem::zeroed();
        while GetMessageW(&mut message, null_mut(), 0, 0) > 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        if IsWindow(hwnd) != 0 {
            DestroyWindow(hwnd);
        }
        if let Some(sender) = session.sender.take() {
            let _ = sender.send(session.result.clone());
        }
    }
}
