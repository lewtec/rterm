//! Kitty keyboard protocol.
//!
//! https://sw.kovidgoyal.net/kitty/keyboard-protocol/

const STACK_LIMIT: usize = 16;

#[derive(Clone, Debug)]
pub struct Keyboard {
    main: Vec<u16>,
    alt: Vec<u16>,
    alternate: bool,
    scan: Scan,
}

#[derive(Clone, Debug)]
enum Scan {
    Ground,
    Esc,
    Csi(Vec<u8>),
}

impl Default for Keyboard {
    fn default() -> Self {
        Self::new()
    }
}

impl Keyboard {
    pub fn new() -> Self {
        Self {
            main: Vec::new(),
            alt: Vec::new(),
            alternate: false,
            scan: Scan::Ground,
        }
    }

    pub fn flags(&self) -> u16 {
        let stack = if self.alternate {
            &self.alt
        } else {
            &self.main
        };
        stack.last().copied().unwrap_or(0)
    }

    /// Remove keyboard-protocol sequences. Other bytes, including mode
    /// changes, are returned for the vt100 parser. Replies go back to the pty.
    pub fn feed(&mut self, bytes: &[u8]) -> (Vec<u8>, Vec<u8>) {
        let mut rest = Vec::new();
        let mut reply = Vec::new();
        for byte in bytes {
            self.byte(*byte, &mut rest, &mut reply);
        }
        (rest, reply)
    }

    fn byte(&mut self, byte: u8, rest: &mut Vec<u8>, reply: &mut Vec<u8>) {
        match &mut self.scan {
            Scan::Ground => {
                if byte == 0x1b {
                    self.scan = Scan::Esc;
                } else {
                    rest.push(byte);
                }
            }
            Scan::Esc => {
                if byte == b'[' {
                    self.scan = Scan::Csi(Vec::new());
                } else if byte == 0x1b {
                    rest.push(0x1b);
                } else {
                    rest.push(0x1b);
                    rest.push(byte);
                    self.scan = Scan::Ground;
                }
            }
            Scan::Csi(buf) => {
                buf.push(byte);
                let done = (0x40..=0x7e).contains(&byte) || buf.len() > 48;
                if !done {
                    return;
                }
                let body = std::mem::take(buf);
                self.scan = Scan::Ground;
                if !self.dispatch(&body, reply) {
                    rest.push(0x1b);
                    rest.push(b'[');
                    rest.extend(body);
                }
            }
        }
    }

    fn dispatch(&mut self, body: &[u8], reply: &mut Vec<u8>) -> bool {
        let Some((&final_byte, params)) = body.split_last() else {
            return false;
        };
        let text = String::from_utf8_lossy(params);
        let (marker, numbers) = marker_and_numbers(&text);
        if final_byte == b'u' {
            return self.keyboard_command(marker, &numbers, reply);
        }
        if matches!(final_byte, b'h' | b'l') && marker == Some('?') {
            let on = final_byte == b'h';
            if numbers
                .iter()
                .any(|number| matches!(number, 47 | 1047 | 1049))
            {
                self.alternate = on;
            }
        }
        false
    }

    fn keyboard_command(
        &mut self,
        marker: Option<char>,
        numbers: &[u16],
        reply: &mut Vec<u8>,
    ) -> bool {
        match marker {
            Some('?') => {
                reply.extend(format!("\x1b[?{}u", self.flags()).into_bytes());
                true
            }
            Some('=') => {
                let flags = numbers.first().copied().unwrap_or(0);
                let mode = numbers.get(1).copied().unwrap_or(1);
                self.set_flags(flags, mode);
                true
            }
            Some('>') => {
                self.push_flags(numbers.first().copied().unwrap_or(0));
                true
            }
            Some('<') => {
                let count = numbers.first().copied().unwrap_or(1).max(1) as usize;
                self.pop(count);
                true
            }
            _ => false,
        }
    }

    fn stack(&mut self) -> &mut Vec<u16> {
        if self.alternate {
            &mut self.alt
        } else {
            &mut self.main
        }
    }

    fn set_flags(&mut self, flags: u16, mode: u16) {
        let current = self.flags();
        let next = match mode {
            2 => current | flags,
            3 => current & !flags,
            _ => flags,
        };
        let stack = self.stack();
        if let Some(top) = stack.last_mut() {
            *top = next;
        } else {
            stack.push(next);
        }
    }

    fn push_flags(&mut self, flags: u16) {
        let current = self.flags();
        let stack = self.stack();
        if stack.is_empty() {
            stack.push(current);
        }
        if stack.len() == STACK_LIMIT {
            stack.remove(0);
        }
        stack.push(flags);
    }

    fn pop(&mut self, count: usize) {
        let stack = self.stack();
        for _ in 0..count {
            if stack.pop().is_none() {
                break;
            }
        }
    }
}

fn marker_and_numbers(text: &str) -> (Option<char>, Vec<u16>) {
    let mut chars = text.chars();
    let marker = chars
        .next()
        .filter(|ch| matches!(ch, '?' | '=' | '>' | '<'));
    let rest = if marker.is_some() { &text[1..] } else { text };
    let numbers = rest
        .split(';')
        .filter(|part| !part.is_empty())
        .map(|part| part.parse().unwrap_or(0))
        .collect();
    (marker, numbers)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Escape,
    Enter,
    Tab,
    Backspace,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Mods {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub super_key: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Press,
    Repeat,
    Release,
}

/// Bytes for the pty when the protocol handles this key.
/// `None` means the legacy encoding still applies.
pub fn encode(flags: u16, key: Key, mods: Mods, phase: Phase) -> Option<Vec<u8>> {
    if flags == 0 {
        return None;
    }
    let report_events = flags & 2 != 0;
    if phase == Phase::Release && !report_events {
        return None;
    }
    let all = flags & 8 != 0;
    let disambiguate = flags & 1 != 0 || all;
    let modified = mods.ctrl || mods.alt || mods.super_key;
    if !all && !(disambiguate && (matches!(key, Key::Escape) || modified)) {
        return None;
    }
    let event = report_events.then_some(match phase {
        Phase::Press => 1,
        Phase::Repeat => 2,
        Phase::Release => 3,
    });
    let mod_bits = modifier_bits(mods);
    let with_text = all && flags & 16 != 0;
    Some(encode_key(key, mod_bits, event, with_text, mods.shift))
}

fn tilde_code(key: Key) -> Option<u32> {
    Some(match key {
        Key::Insert => 2,
        Key::Delete => 3,
        Key::PageUp => 5,
        Key::PageDown => 6,
        _ => return None,
    })
}

fn modifier_bits(mods: Mods) -> u32 {
    1 + u32::from(mods.shift)
        + 2 * u32::from(mods.alt)
        + 4 * u32::from(mods.ctrl)
        + 8 * u32::from(mods.super_key)
}

fn encode_key(key: Key, mods: u32, event: Option<u32>, with_text: bool, shift: bool) -> Vec<u8> {
    if let Some(code) = tilde_code(key) {
        return tilde(code, mods, event);
    }
    if let Some(letter) = functional_letter(key) {
        return functional(letter, mods, event);
    }
    let (code, text) = match key {
        Key::Escape => (27, None),
        Key::Enter => (13, None),
        Key::Tab => (9, None),
        Key::Backspace => (127, None),
        Key::Char(ch) => {
            let text = with_text.then_some(if shift {
                ch.to_ascii_uppercase() as u32
            } else {
                ch as u32
            });
            (ch as u32, text)
        }
        _ => (0, None),
    };
    csi_u(code, mods, event, text)
}

fn functional_letter(key: Key) -> Option<u8> {
    Some(match key {
        Key::Up => b'A',
        Key::Down => b'B',
        Key::Right => b'C',
        Key::Left => b'D',
        Key::Home => b'H',
        Key::End => b'F',
        _ => return None,
    })
}

fn functional(letter: u8, mods: u32, event: Option<u32>) -> Vec<u8> {
    if mods == 1 && event.is_none() {
        return vec![0x1b, b'[', letter];
    }
    let mut out = format!("\x1b[1;{mods}");
    if let Some(event) = event {
        // Event type is a sub-field of the modifiers, so the separator is a colon.
        out.push(':');
        out.push_str(&event.to_string());
    }
    out.push(letter as char);
    out.into_bytes()
}

fn tilde(code: u32, mods: u32, event: Option<u32>) -> Vec<u8> {
    if mods == 1 && event.is_none() {
        return format!("\x1b[{code}~").into_bytes();
    }
    let mut out = format!("\x1b[{code};{mods}");
    if let Some(event) = event {
        out.push(':');
        out.push_str(&event.to_string());
    }
    out.push('~');
    out.into_bytes()
}

fn csi_u(code: u32, mods: u32, event: Option<u32>, text: Option<u32>) -> Vec<u8> {
    let mut out = format!("\x1b[{code}");
    if mods != 1 || event.is_some() || text.is_some() {
        out.push(';');
        out.push_str(&mods.to_string());
        if let Some(event) = event {
            out.push(':');
            out.push_str(&event.to_string());
        }
    }
    if let Some(text) = text {
        out.push(';');
        out.push_str(&text.to_string());
    }
    out.push('u');
    out.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_starts_at_zero() {
        let mut keyboard = Keyboard::new();
        let (rest, reply) = keyboard.feed(b"\x1b[?u");
        assert!(rest.is_empty());
        assert_eq!(reply, b"\x1b[?0u");
    }

    #[test]
    fn push_one_is_the_quickstart_enable() {
        let mut keyboard = Keyboard::new();
        let (rest, reply) = keyboard.feed(b"\x1b[>1u");
        assert!(rest.is_empty());
        assert!(reply.is_empty());
        assert_eq!(keyboard.flags(), 1);
        let (_rest, reply) = keyboard.feed(b"\x1b[?u");
        assert_eq!(reply, b"\x1b[?1u");
        keyboard.feed(b"\x1b[<u");
        assert_eq!(keyboard.flags(), 0);
    }

    #[test]
    fn set_modes_change_bits() {
        let mut keyboard = Keyboard::new();
        keyboard.feed(b"\x1b[=1u");
        keyboard.feed(b"\x1b[=2;2u");
        assert_eq!(keyboard.flags(), 3);
        keyboard.feed(b"\x1b[=1;3u");
        assert_eq!(keyboard.flags(), 2);
    }

    #[test]
    fn color_sequences_still_reach_the_parser() {
        let mut keyboard = Keyboard::new();
        let (rest, reply) = keyboard.feed(b"\x1b[31mhi");
        assert!(reply.is_empty());
        assert_eq!(rest, b"\x1b[31mhi");
    }

    #[test]
    fn a_split_query_is_answered_on_the_next_chunk() {
        let mut keyboard = Keyboard::new();
        let (rest, reply) = keyboard.feed(b"\x1b[");
        assert!(rest.is_empty() && reply.is_empty());
        let (rest, reply) = keyboard.feed(b"?u");
        assert!(rest.is_empty());
        assert_eq!(reply, b"\x1b[?0u");
    }

    #[test]
    fn disambiguate_escape_and_ctrl_and_leaves_plain_text() {
        let mods = Mods::default();
        assert_eq!(
            encode(1, Key::Escape, mods, Phase::Press).as_deref(),
            Some(b"\x1b[27u".as_slice())
        );
        assert_eq!(
            encode(1, Key::Char('c'), Mods { ctrl: true, ..mods }, Phase::Press).as_deref(),
            Some(b"\x1b[99;5u".as_slice())
        );
        assert_eq!(encode(1, Key::Char('a'), mods, Phase::Press), None);
        assert_eq!(encode(1, Key::Char('a'), mods, Phase::Release), None);
    }

    #[test]
    fn report_all_keys_includes_letters_and_releases() {
        assert_eq!(
            encode(8, Key::Char('a'), Mods::default(), Phase::Press).as_deref(),
            Some(b"\x1b[97u".as_slice())
        );
        assert_eq!(
            encode(10, Key::Char('a'), Mods::default(), Phase::Release).as_deref(),
            Some(b"\x1b[97;1:3u".as_slice())
        );
    }

    #[test]
    fn ctrl_b_press_is_a_kitty_event() {
        let mods = Mods {
            ctrl: true,
            ..Mods::default()
        };
        // Herdr enables flags 7, so the prefix key carries an event type.
        assert_eq!(
            encode(7, Key::Char('b'), mods, Phase::Press).as_deref(),
            Some(b"\x1b[98;5:1u".as_slice())
        );
        assert_eq!(
            encode(7, Key::Char('b'), mods, Phase::Release).as_deref(),
            Some(b"\x1b[98;5:3u".as_slice())
        );
    }
}
