use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::mpsc;
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
    let mut text = String::new();
    let mut png = Vec::new();
    if let Some(bytes) = png_from_pasteboard() {
        png = bytes;
    }
    if let Ok(mut clipboard) = arboard::Clipboard::new() {
        if let Ok(value) = clipboard.get_text() {
            text = value;
        }
        if png.is_empty()
            && let Ok(image) = clipboard.get_image()
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
    Snapshot {
        has_image: !png.is_empty(),
        has_text: !text.is_empty(),
        png,
        text,
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

fn png_from_pasteboard() -> Option<Vec<u8>> {
    #[cfg(target_os = "macos")]
    {
        return macos_png();
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

#[cfg(target_os = "macos")]
fn macos_png() -> Option<Vec<u8>> {
    use objc2_app_kit::{NSPasteboard, NSPasteboardTypePNG};
    use std::ptr::NonNull;
    unsafe {
        let board = NSPasteboard::generalPasteboard();
        let data = board.dataForType(NSPasteboardTypePNG)?;
        let len = data.length() as usize;
        if len == 0 || len > rterm::paste::MAX_BYTES {
            return None;
        }
        let mut buf = vec![0u8; len];
        let ptr = NonNull::new(buf.as_mut_ptr().cast())?;
        data.getBytes_length(ptr, len as _);
        buf.starts_with(b"\x89PNG").then_some(buf)
    }
}

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
