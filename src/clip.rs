use std::io::Write;
#[cfg(not(target_os = "macos"))]
use std::process::{Command, Stdio};
#[cfg(not(target_os = "macos"))]
use std::sync::mpsc;
#[cfg(not(target_os = "macos"))]
use std::time::Duration;

use image::codecs::png::PngEncoder;
use image::{ExtendedColorType, ImageEncoder};

pub struct Snapshot {
    pub has_image: bool,
    pub has_text: bool,
    pub png: Vec<u8>,
    pub text: String,
}

pub fn snapshot() -> Snapshot {
    #[cfg(target_os = "macos")]
    {
        macos_snapshot()
    }
    #[cfg(not(target_os = "macos"))]
    {
        other_snapshot()
    }
}

fn finish(text: String, png: Vec<u8>) -> Snapshot {
    Snapshot {
        has_image: !png.is_empty(),
        has_text: !text.is_empty(),
        png,
        text,
    }
}

#[cfg(not(target_os = "macos"))]
fn other_snapshot() -> Snapshot {
    let mut text = String::new();
    let mut png = Vec::new();
    if let Ok(mut clipboard) = arboard::Clipboard::new() {
        if let Ok(value) = clipboard.get_text() {
            text = value;
        }
        if let Ok(image) = clipboard.get_image()
            && let Some(encoded) = encode_png(image.bytes.as_ref(), image.width, image.height)
        {
            png = encoded;
        }
    }
    if png.is_empty()
        && let Some(bytes) = png_from_commands()
    {
        png = bytes;
    }
    if text.is_empty()
        && let Some(bytes) = text_from_commands()
        && let Ok(value) = String::from_utf8(bytes)
    {
        text = value;
    }
    finish(text, png)
}

#[cfg(target_os = "macos")]
fn macos_snapshot() -> Snapshot {
    use objc2::ClassType;
    use objc2_app_kit::{
        NSImage, NSPasteboard, NSPasteboardTypePNG, NSPasteboardTypeString, NSPasteboardTypeTIFF,
    };
    unsafe {
        let board = NSPasteboard::generalPasteboard();
        let text = board
            .stringForType(NSPasteboardTypeString)
            .as_deref()
            .map(ns_string)
            .unwrap_or_default()
            .trim()
            .to_string();
        let png = board
            .dataForType(NSPasteboardTypePNG)
            .as_deref()
            .and_then(nsdata_vec)
            .filter(|bytes| bytes.starts_with(b"\x89PNG"))
            .or_else(|| {
                board
                    .dataForType(NSPasteboardTypeTIFF)
                    .as_deref()
                    .and_then(tiff_png)
            })
            .or_else(|| {
                NSImage::initWithPasteboard(NSImage::alloc(), &board)
                    .and_then(|image| image.TIFFRepresentation())
                    .as_deref()
                    .and_then(tiff_png)
            })
            .unwrap_or_default();
        finish(text, png)
    }
}

pub fn beep() {
    if let Ok(mut tty) = std::fs::OpenOptions::new().write(true).open("/dev/tty") {
        let _ = tty.write_all(b"\x07");
    }
}

pub fn encode_png(rgba: &[u8], width: usize, height: usize) -> Option<Vec<u8>> {
    if width == 0 || height == 0 || rgba.len() < width.saturating_mul(height).saturating_mul(4) {
        return None;
    }
    let mut png = Vec::new();
    let encoder = PngEncoder::new(&mut png);
    encoder
        .write_image(
            &rgba[..width * height * 4],
            width as u32,
            height as u32,
            ExtendedColorType::Rgba8,
        )
        .ok()?;
    Some(png)
}

#[cfg(target_os = "macos")]
fn ns_string(value: &objc2_foundation::NSString) -> String {
    unsafe {
        let ptr = value.UTF8String();
        if ptr.is_null() {
            return String::new();
        }
        std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned()
    }
}

#[cfg(target_os = "macos")]
fn nsdata_vec(data: &objc2_foundation::NSData) -> Option<Vec<u8>> {
    use std::ptr::NonNull;
    let len = data.length() as usize;
    if len == 0 || len > rterm::paste::MAX_BYTES {
        return None;
    }
    let mut buf = vec![0u8; len];
    let ptr = NonNull::new(buf.as_mut_ptr().cast())?;
    unsafe { data.getBytes_length(ptr, len as _) };
    Some(buf)
}

#[cfg(target_os = "macos")]
fn tiff_png(data: &objc2_foundation::NSData) -> Option<Vec<u8>> {
    let bytes = nsdata_vec(data)?;
    let image = image::load_from_memory_with_format(&bytes, image::ImageFormat::Tiff).ok()?;
    let rgba = image.into_rgba8();
    encode_png(rgba.as_raw(), rgba.width() as usize, rgba.height() as usize)
}

#[cfg(not(target_os = "macos"))]
fn png_from_commands() -> Option<Vec<u8>> {
    for (program, args) in [
        ("wl-paste", &["--no-newline", "-t", "image/png"][..]),
        (
            "xclip",
            &["-selection", "clipboard", "-t", "image/png", "-o"][..],
        ),
    ] {
        if let Some(bytes) = output_timeout(program, args)
            && bytes.starts_with(b"\x89PNG")
        {
            return Some(bytes);
        }
    }
    None
}

#[cfg(not(target_os = "macos"))]
fn text_from_commands() -> Option<Vec<u8>> {
    for (program, args) in [
        ("wl-paste", &["--no-newline", "-t", "text"][..]),
        ("xclip", &["-selection", "clipboard", "-o"][..]),
    ] {
        if let Some(bytes) = output_timeout(program, args) {
            return Some(bytes);
        }
    }
    None
}

#[cfg(not(target_os = "macos"))]
fn output_timeout(program: &str, args: &[&str]) -> Option<Vec<u8>> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = std::io::Read::read_to_end(&mut stdout, &mut buf);
        let _ = tx.send(buf);
    });
    match rx.recv_timeout(Duration::from_millis(400)) {
        Ok(buf) if !buf.is_empty() => {
            let _ = child.wait();
            Some(buf)
        }
        _ => {
            let _ = child.kill();
            let _ = child.wait();
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_pixel_png_has_the_signature() {
        let png = encode_png(&[255, 0, 0, 255], 1, 1).unwrap();
        assert!(png.starts_with(b"\x89PNG"));
    }
}
