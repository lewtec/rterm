//! Fill the screen without entering a separate Space, so the tab row can sit
//! in the menu-bar band beside the camera notch.
//!
//! This is the AppKit path from the old window chrome: auto-hide the menu bar
//! and dock, drop the title bar, and size the window to `screen.frame`.

use eframe::Frame;
use rterm::notch::FillSplit;

#[cfg(not(target_os = "macos"))]
#[derive(Clone, Copy)]
pub struct Backup;

#[cfg(not(target_os = "macos"))]
pub fn sync(_active: bool, _backup: &mut Option<Backup>, _frame: &Frame) -> Option<FillSplit> {
    None
}

#[cfg(target_os = "macos")]
mod mac {
    use super::*;
    use objc2::rc::Retained;
    use objc2_app_kit::{
        NSApplication, NSApplicationPresentationOptions, NSView, NSWindow,
        NSWindowCollectionBehavior, NSWindowStyleMask, NSWindowTitleVisibility,
    };
    use objc2_foundation::{MainThreadMarker, NSRect};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use rterm::notch::Band;

    #[derive(Clone, Copy)]
    pub struct Saved {
        style: NSWindowStyleMask,
        frame: NSRect,
        presentation: NSApplicationPresentationOptions,
        behavior: NSWindowCollectionBehavior,
    }

    pub type Backup = Saved;

    pub fn sync(active: bool, backup: &mut Option<Saved>, frame: &Frame) -> Option<FillSplit> {
        let view = ns_view(frame)?;
        let window = view.window()?;
        if active {
            if backup.is_none() {
                *backup = Some(capture(&window));
                enter(&window);
            }
            Some(read_split(&window))
        } else {
            if let Some(saved) = backup.take() {
                leave(&window, saved);
            }
            None
        }
    }

    fn ns_view(frame: &Frame) -> Option<Retained<NSView>> {
        let handle = frame.window_handle().ok()?;
        let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
            return None;
        };
        unsafe { Retained::retain(appkit.ns_view.as_ptr().cast()) }
    }

    fn capture(window: &NSWindow) -> Saved {
        let mtm = unsafe { MainThreadMarker::new_unchecked() };
        Saved {
            style: window.styleMask(),
            frame: window.frame(),
            presentation: NSApplication::sharedApplication(mtm).presentationOptions(),
            behavior: unsafe { window.collectionBehavior() },
        }
    }

    fn enter(window: &NSWindow) {
        let mtm = unsafe { MainThreadMarker::new_unchecked() };
        let mut behavior = unsafe { window.collectionBehavior() };
        behavior.remove(
            NSWindowCollectionBehavior::FullScreenPrimary
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::FullScreenAllowsTiling,
        );
        behavior.insert(
            NSWindowCollectionBehavior::FullScreenNone
                | NSWindowCollectionBehavior::FullScreenDisallowsTiling,
        );
        unsafe { window.setCollectionBehavior(behavior) };
        NSApplication::sharedApplication(mtm).setPresentationOptions(
            NSApplicationPresentationOptions::NSApplicationPresentationAutoHideDock
                | NSApplicationPresentationOptions::NSApplicationPresentationAutoHideMenuBar,
        );
        window.setStyleMask(NSWindowStyleMask::Borderless | NSWindowStyleMask::FullSizeContentView);
        window.setTitleVisibility(NSWindowTitleVisibility::NSWindowTitleHidden);
        window.setTitlebarAppearsTransparent(true);
        window.setHasShadow(false);
        window.setMovable(false);
        if let Some(screen) = window.screen() {
            window.setFrame_display(screen.frame(), true);
        }
    }

    fn leave(window: &NSWindow, saved: Saved) {
        let mtm = unsafe { MainThreadMarker::new_unchecked() };
        NSApplication::sharedApplication(mtm).setPresentationOptions(saved.presentation);
        window.setStyleMask(saved.style);
        window.setMovable(true);
        window.setHasShadow(true);
        window.setFrame_display(saved.frame, true);
        unsafe { window.setCollectionBehavior(saved.behavior) };
    }

    fn read_split(window: &NSWindow) -> FillSplit {
        let frame = window.frame();
        let min_x = frame.origin.x as f64;
        let max_x = min_x + frame.size.width as f64;
        let (left, right) = window
            .screen()
            .map(|screen| {
                (
                    band(unsafe { screen.auxiliaryTopLeftArea() }),
                    band(unsafe { screen.auxiliaryTopRightArea() }),
                )
            })
            .unwrap_or((None, None));
        rterm::notch::fill_split(min_x, max_x, left, right, 96.0)
    }

    fn band(rect: NSRect) -> Option<Band> {
        if rect.size.width <= 1.0 || rect.size.height <= 1.0 {
            return None;
        }
        Some(Band {
            min_x: rect.origin.x as f64,
            max_x: rect.origin.x as f64 + rect.size.width as f64,
            height: rect.size.height as f64,
        })
    }
}

#[cfg(target_os = "macos")]
pub use mac::{Backup, sync};
