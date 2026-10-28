//! Explicit user-triggered screen sampling. No screenshots are retained.
use resume_one_core::theme_style::Rgb;
pub(super) type Selection = Result<Option<Rgb>, String>;
#[cfg(target_os = "windows")]
mod windows;

pub(super) fn sample() -> futures_channel::oneshot::Receiver<Selection> {
    let (sender, receiver) = futures_channel::oneshot::channel();
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::{NSColor, NSColorSampler, NSColorSpace};
        use std::cell::RefCell;
        let sender = RefCell::new(Some(sender));
        let block = block2::RcBlock::new(move |value: *mut NSColor| {
            // AppKit keeps the selected NSColor alive for this callback, and
            // invokes it on the main thread; nil means cancellation.
            let value = unsafe { value.as_ref() };
            let selected = match value {
                None => Ok(None),
                Some(color) => color
                    .colorUsingColorSpace(&NSColorSpace::sRGBColorSpace())
                    .ok_or_else(|| "无法将取色结果转换为 sRGB".to_owned())
                    .map(|color| {
                        Some(Rgb {
                            r: (color.redComponent().clamp(0., 1.) * 255.).round() as u8,
                            g: (color.greenComponent().clamp(0., 1.) * 255.).round() as u8,
                            b: (color.blueComponent().clamp(0., 1.) * 255.).round() as u8,
                        })
                    }),
            };
            if let Some(sender) = sender.borrow_mut().take() {
                let _ = sender.send(selected);
            }
        });
        // NSColorSampler retains its session and copies the block until completion.
        unsafe {
            NSColorSampler::new().showSamplerWithSelectionHandler(&block);
        }
    }
    #[cfg(target_os = "windows")]
    std::thread::spawn(move || windows::run(sender));
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = sender.send(Err("当前系统尚未提供屏幕取色".into()));
    }
    receiver
}
