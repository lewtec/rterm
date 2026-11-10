//! The application icon.
//!
//! The artwork is a rounded tile matted on black. Those corners are cleared
//! so the icon shape is the tile. On macOS the tile is then inset to Apple's
//! icon grid, or it draws larger than the other Alt-Tab icons.

#[cfg(any(test, target_os = "macos"))]
use std::io::Cursor;

/// Apple's macOS icon grid: the rounded body is 824 px on a 1024 px canvas.
#[cfg(any(test, target_os = "macos"))]
const ICON_BODY: u32 = 824;
#[cfg(any(test, target_os = "macos"))]
const ICON_CANVAS: u32 = 1024;

#[cfg(target_os = "macos")]
pub fn png_bytes(bytes: &[u8]) -> Option<Vec<u8>> {
    encode_png(&icon_image(bytes)?)
}

pub fn icon_data(bytes: &[u8]) -> Option<eframe::egui::IconData> {
    let image = icon_image(bytes)?;
    Some(eframe::egui::IconData {
        width: image.width(),
        height: image.height(),
        rgba: image.into_raw(),
    })
}

fn icon_image(bytes: &[u8]) -> Option<image::RgbaImage> {
    let image = plated(bytes)?;
    #[cfg(any(test, target_os = "macos"))]
    {
        Some(on_the_grid(image))
    }
    #[cfg(not(any(test, target_os = "macos")))]
    {
        Some(image)
    }
}

#[cfg(any(test, target_os = "macos"))]
fn encode_png(image: &image::RgbaImage) -> Option<Vec<u8>> {
    let mut png = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
        .ok()?;
    Some(png)
}

#[cfg(any(test, target_os = "macos"))]
fn on_the_grid(image: image::RgbaImage) -> image::RgbaImage {
    let width = image.width();
    let height = image.height();
    let body_width = width * ICON_BODY / ICON_CANVAS;
    let body_height = height * ICON_BODY / ICON_CANVAS;
    if body_width == 0 || body_height == 0 || body_width >= width || body_height >= height {
        return image;
    }
    let scaled = image::imageops::resize(
        &image,
        body_width,
        body_height,
        image::imageops::FilterType::Lanczos3,
    );
    let mut canvas = image::RgbaImage::new(width, height);
    let x = (width - body_width) / 2;
    let y = (height - body_height) / 2;
    image::imageops::overlay(&mut canvas, &scaled, i64::from(x), i64::from(y));
    canvas
}

fn plated(bytes: &[u8]) -> Option<image::RgbaImage> {
    let mut image = image::load_from_memory(bytes).ok()?.into_rgba8();
    let width = image.width();
    let height = image.height();
    if width == 0 || height == 0 {
        return None;
    }
    let plate = image.get_pixel(width / 8, height / 2).0;
    let plate = [plate[0], plate[1], plate[2]];
    let mut matte = vec![false; (width as usize) * (height as usize)];
    let mut stack = vec![
        (0i32, 0i32),
        (width as i32 - 1, 0),
        (0, height as i32 - 1),
        (width as i32 - 1, height as i32 - 1),
    ];
    while let Some((x, y)) = stack.pop() {
        if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
            continue;
        }
        let index = y as usize * width as usize + x as usize;
        if matte[index] {
            continue;
        }
        let pixel = image.get_pixel(x as u32, y as u32).0;
        if !outside_the_tile(pixel, plate) {
            continue;
        }
        matte[index] = true;
        stack.extend([(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)]);
    }
    for (index, outside) in matte.iter().enumerate() {
        if !outside {
            continue;
        }
        let x = (index % width as usize) as u32;
        let y = (index / width as usize) as u32;
        image.put_pixel(x, y, image::Rgba([0, 0, 0, 0]));
    }
    Some(image)
}

fn outside_the_tile(pixel: [u8; 4], plate: [u8; 3]) -> bool {
    let red = i32::from(pixel[0]);
    let green = i32::from(pixel[1]);
    let blue = i32::from(pixel[2]);
    let from_black = red * red + green * green + blue * blue;
    let from_plate = (red - i32::from(plate[0])).pow(2)
        + (green - i32::from(plate[1])).pow(2)
        + (blue - i32::from(plate[2])).pow(2);
    from_black + 40 < from_plate && red < 90 && green < 100 && blue < 120
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

    use super::png_bytes;

    static LEFT: AtomicU8 = AtomicU8::new(30);

    pub fn install() {
        let left = LEFT.load(Ordering::Relaxed);
        if left == 0 {
            return;
        }
        LEFT.store(left - 1, Ordering::Relaxed);
        let Some(png) = png_bytes(include_bytes!(
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
    #[test]
    fn png_keeps_the_alpha_channel() {
        let mut image = image::RgbaImage::from_pixel(4, 4, image::Rgba([26, 32, 46, 255]));
        image.put_pixel(0, 0, image::Rgba([0, 0, 0, 255]));
        let mut rgba = Vec::new();
        image
            .write_to(
                &mut std::io::Cursor::new(&mut rgba),
                image::ImageFormat::Png,
            )
            .unwrap();
        let plated = super::plated(&rgba).unwrap();
        let png = super::encode_png(&plated).unwrap();
        assert!(png.starts_with(b"\x89PNG"));
        assert_eq!(png[25], 6);
        let decoded = image::load_from_memory(&png).unwrap().into_rgba8();
        assert_eq!(decoded.get_pixel(0, 0).0, [0, 0, 0, 0]);
        assert_eq!(decoded.get_pixel(1, 1).0[3], 255);
    }

    #[test]
    fn black_corners_are_transparent() {
        let mut image = image::RgbaImage::from_pixel(4, 4, image::Rgba([26, 32, 46, 255]));
        for (x, y) in [(0, 0), (3, 0), (0, 3), (3, 3)] {
            image.put_pixel(x, y, image::Rgba([0, 0, 0, 255]));
        }
        let mut png = Vec::new();
        image
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let plated = super::plated(&png).unwrap();
        assert_eq!(plated.get_pixel(0, 0).0, [0, 0, 0, 0]);
        assert_eq!(plated.get_pixel(1, 1).0, [26, 32, 46, 255]);
    }

    #[test]
    fn asset_corners_are_transparent() {
        let image = super::plated(include_bytes!(
            "../Assets.xcassets/AppIcon.appiconset/icon_512x512.png"
        ))
        .unwrap();
        assert_eq!(image.get_pixel(0, 0).0[3], 0);
        assert_eq!(
            image.get_pixel(image.width() - 1, image.height() - 1).0[3],
            0
        );
        assert_eq!(image.get_pixel(image.width() / 2, 80).0[3], 255);
        assert_eq!(image.get_pixel(36, 36).0[3], 255);
    }

    #[test]
    fn artwork_is_inset_to_the_macos_grid() {
        let image = super::icon_image(include_bytes!(
            "../Assets.xcassets/AppIcon.appiconset/icon_512x512.png"
        ))
        .unwrap();
        let margin = (image.width() - image.width() * super::ICON_BODY / super::ICON_CANVAS) / 2;
        assert_eq!(margin, 50);
        assert_eq!(
            image.get_pixel(margin - 1, image.height() / 2).0,
            [0, 0, 0, 0]
        );
        assert_eq!(image.get_pixel(margin + 4, image.height() / 2).0[3], 255);
        assert_eq!(
            image.get_pixel(image.width() / 2, image.height() / 2).0[3],
            255
        );
    }
}
