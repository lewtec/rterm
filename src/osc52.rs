//! OSC 52 clipboard writes.
//!
//! A program in the terminal, including a remote Grok, asks the host to copy
//! with `ESC ] 52 ; <selection> ; <base64> BEL`. The selection is ignored:
//! every write goes to the system clipboard.

const MAX_BYTES: usize = 8 * 1024 * 1024;

pub struct Sink;

impl vt100::Callbacks for Sink {
    fn copy_to_clipboard(&mut self, _: &mut vt100::Screen, selection: &[u8], data: &[u8]) {
        if let Some(text) = text_of(selection, data) {
            crate::clip::set_text(&text);
        }
    }
}

pub fn text_of(selection: &[u8], data: &[u8]) -> Option<String> {
    if !selection.is_empty() && !selection.iter().all(is_selector) {
        return None;
    }
    let bytes = decode_base64(data)?;
    if bytes.len() > MAX_BYTES {
        return None;
    }
    String::from_utf8(bytes).ok()
}

fn is_selector(byte: &u8) -> bool {
    matches!(byte, b'c' | b'p' | b'q' | b's' | b'0'..=b'7')
}

fn decode_base64(data: &[u8]) -> Option<Vec<u8>> {
    if data.is_empty() {
        return Some(Vec::new());
    }
    if !data.len().is_multiple_of(4) || data.len() / 4 * 3 > MAX_BYTES.saturating_add(2) {
        return None;
    }
    let mut out = Vec::with_capacity(data.len() / 4 * 3);
    for chunk in data.chunks_exact(4) {
        let mut pad = 0u8;
        let mut value = 0u32;
        for (index, byte) in chunk.iter().copied().enumerate() {
            let bits = if byte == b'=' {
                if index < 2 {
                    return None;
                }
                pad += 1;
                0
            } else if pad > 0 {
                return None;
            } else {
                sextet(byte)?
            };
            value = (value << 6) | u32::from(bits);
        }
        out.push((value >> 16) as u8);
        if pad < 2 {
            out.push((value >> 8) as u8);
        }
        if pad < 1 {
            out.push(value as u8);
        }
    }
    Some(out)
}

fn sextet(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU16;

    use super::text_of;

    #[test]
    fn decodes_a_clipboard_write() {
        assert_eq!(text_of(b"c", b"aGVsbG8=").as_deref(), Some("hello"));
        assert_eq!(text_of(b"c", b"YQ==").as_deref(), Some("a"));
        assert_eq!(text_of(b"c", b"YWI=").as_deref(), Some("ab"));
        assert_eq!(text_of(b"c", b"YWJj").as_deref(), Some("abc"));
        assert_eq!(text_of(b"", b"YQ==").as_deref(), Some("a"));
        assert_eq!(text_of(b"c", b"").as_deref(), Some(""));
    }

    #[test]
    fn rejects_a_bad_payload() {
        assert!(text_of(b"c", b"aGVsbG8").is_none());
        assert!(text_of(b"c", b"!!!!").is_none());
        assert!(text_of(b"c", b"YQ==YQ==").is_some());
        assert!(text_of(&[0xff], b"YQ==").is_none());
    }

    struct Grab {
        data: Vec<u8>,
    }

    impl vt100::Callbacks for Grab {
        fn copy_to_clipboard(&mut self, _: &mut vt100::Screen, _: &[u8], data: &[u8]) {
            self.data = data.to_vec();
        }
    }

    #[test]
    fn parser_hands_osc_52_to_the_host() {
        let mut parser = vt100::Parser::new_with_callbacks(
            NonZeroU16::new(4).unwrap(),
            NonZeroU16::new(20).unwrap(),
            0,
            Grab { data: Vec::new() },
        );
        parser.process(b"\x1b]52;c;aGVs");
        parser.process(b"bG8=\x1b\\visible");
        assert_eq!(parser.callbacks().data, b"aGVsbG8=");
        assert_eq!(
            text_of(b"c", &parser.callbacks().data).as_deref(),
            Some("hello")
        );
        let shown = parser.screen().contents();
        assert!(shown.contains("visible"));
        assert!(!shown.contains("aGVsbG8"));
    }
}
