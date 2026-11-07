//! The switcher icon on macOS.
//!
//! eframe builds the application icon from RGBA PNG bytes. macOS treats any
//! alpha channel as a custom shape and does not draw the icon plate in
//! Alt-Tab. An opaque image gets that plate.

#[cfg(any(test, target_os = "macos"))]
use std::io::Cursor;

#[cfg(any(test, target_os = "macos"))]
pub fn opaque_png(bytes: &[u8]) -> Option<Vec<u8>> {
    let image = image::load_from_memory(bytes).ok()?.into_rgb8();
    let mut png = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
        .ok()?;
    Some(png)
}

pub fn install() {
    #[cfg(target_os = "macos")]
    macos::install();
}

#[cfg(target_os = "macos")]
mod macos {
    use std::sync::atomic::{AtomicU8, Ordering};

    use objc2::ClassType;
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::{MainThreadMarker, NSData, NSSize};

    use super::opaque_png;

    static LEFT: AtomicU8 = AtomicU8::new(30);

    pub fn install() {
        let left = LEFT.load(Ordering::Relaxed);
        if left == 0 {
            return;
        }
        LEFT.store(left - 1, Ordering::Relaxed);
        let Some(png) = opaque_png(include_bytes!(
            "../Assets.xcassets/AppIcon.appiconset/icon_512x512.png"
        )) else {
            return;
        };
        unsafe {
            let data =
                NSData::dataWithBytes_length(png.as_ptr() as *mut std::ffi::c_void, png.len() as _);
            let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) else {
                return;
            };
            image.setSize(NSSize::new(512.0, 512.0));
            image.setTemplate(false);
            let mtm = MainThreadMarker::new_unchecked();
            NSApplication::sharedApplication(mtm).setApplicationIconImage(Some(&image));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::opaque_png;

    #[test]
    fn opaque_png_has_no_alpha_channel() {
        let mut rgba = Vec::new();
        image::RgbaImage::from_pixel(1, 1, image::Rgba([10, 20, 30, 255]))
            .write_to(
                &mut std::io::Cursor::new(&mut rgba),
                image::ImageFormat::Png,
            )
            .unwrap();
        let png = opaque_png(&rgba).unwrap();
        assert!(png.starts_with(b"\x89PNG"));
        assert_eq!(png[25], 2);
    }
}
