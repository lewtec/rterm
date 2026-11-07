//! macOS command-v does not arrive as a key.
//!
//! egui treats it as a paste command, reads only clipboard text, and drops
//! the event when that text is empty. An image-only pasteboard therefore
//! never reaches the pane. This monitor records the chord itself.

use std::sync::atomic::{AtomicBool, Ordering};

static EDITOR_OPEN: AtomicBool = AtomicBool::new(false);
static COMMAND_V: AtomicBool = AtomicBool::new(false);

pub fn install() {
    #[cfg(target_os = "macos")]
    imp::install();
}

pub fn set_editor_open(open: bool) {
    EDITOR_OPEN.store(open, Ordering::Relaxed);
}

pub fn take_command_v() -> bool {
    COMMAND_V.swap(false, Ordering::Relaxed)
}

#[cfg(any(test, target_os = "macos"))]
pub fn command_v_chord(
    command: bool,
    control: bool,
    option: bool,
    shift: bool,
    key: &str,
    repeat: bool,
) -> bool {
    !repeat && command && !control && !option && !shift && key.eq_ignore_ascii_case("v")
}

#[cfg(target_os = "macos")]
mod imp {
    use std::ffi::CStr;
    use std::ptr::NonNull;
    use std::sync::atomic::{AtomicBool, Ordering};

    use block2::RcBlock;
    use objc2::ClassType;
    use objc2_app_kit::{
        NSEvent, NSEventMask, NSEventModifierFlags, NSImage, NSPasteboard, NSPasteboardTypePNG,
        NSPasteboardTypeString, NSPasteboardTypeTIFF,
    };

    use super::{COMMAND_V, EDITOR_OPEN};

    static INSTALLED: AtomicBool = AtomicBool::new(false);

    pub fn install() {
        if INSTALLED.swap(true, Ordering::Relaxed) {
            return;
        }
        let block = RcBlock::new(|event: NonNull<NSEvent>| -> *mut NSEvent { handle(event) });
        let block = Box::leak(Box::new(block));
        let monitor = unsafe {
            NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::KeyDown, block)
        };
        std::mem::forget(monitor);
    }

    fn handle(event: NonNull<NSEvent>) -> *mut NSEvent {
        let raw = event.as_ptr();
        if EDITOR_OPEN.load(Ordering::Relaxed) {
            return raw;
        }
        let event = unsafe { event.as_ref() };
        let repeat = unsafe { event.isARepeat() };
        let flags = unsafe { event.modifierFlags() };
        let key = unsafe { event.charactersIgnoringModifiers() }
            .as_deref()
            .map(ns_string)
            .unwrap_or_default();
        if command_chord(flags, &key, repeat) && image_only() {
            COMMAND_V.store(true, Ordering::Relaxed);
        }
        raw
    }

    fn command_chord(flags: NSEventModifierFlags, key: &str, repeat: bool) -> bool {
        super::command_v_chord(
            flags.contains(NSEventModifierFlags::NSEventModifierFlagCommand),
            flags.contains(NSEventModifierFlags::NSEventModifierFlagControl),
            flags.contains(NSEventModifierFlags::NSEventModifierFlagOption),
            flags.contains(NSEventModifierFlags::NSEventModifierFlagShift),
            key,
            repeat,
        )
    }

    fn image_only() -> bool {
        unsafe {
            let board = NSPasteboard::generalPasteboard();
            if let Some(text) = board.stringForType(NSPasteboardTypeString) {
                let value = ns_string(&text);
                if !value.trim().is_empty() {
                    return false;
                }
            }
            board.dataForType(NSPasteboardTypePNG).is_some()
                || board.dataForType(NSPasteboardTypeTIFF).is_some()
                || NSImage::initWithPasteboard(NSImage::alloc(), &board).is_some()
        }
    }

    fn ns_string(value: &objc2_foundation::NSString) -> String {
        unsafe {
            let ptr = value.UTF8String();
            if ptr.is_null() {
                return String::new();
            }
            CStr::from_ptr(ptr).to_string_lossy().into_owned()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::command_v_chord;

    #[test]
    fn command_v_is_only_the_plain_chord() {
        assert!(command_v_chord(true, false, false, false, "v", false));
        assert!(command_v_chord(true, false, false, false, "V", false));
        assert!(!command_v_chord(true, true, false, false, "v", false));
        assert!(!command_v_chord(true, false, false, true, "v", false));
        assert!(!command_v_chord(true, false, false, false, "v", true));
        assert!(!command_v_chord(false, false, false, false, "v", false));
    }
}
