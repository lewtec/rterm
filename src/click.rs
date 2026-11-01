use vt100::{MouseProtocolEncoding, MouseProtocolMode, Screen};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Click {
    Press,
    Release,
    Drag,
    Move,
}

/// Report a mouse event when the application asked for one.
/// `col` and `row` are 0-based cell coordinates.
pub fn report(
    mode: MouseProtocolMode,
    encoding: MouseProtocolEncoding,
    click: Click,
    button: u8,
    col: u16,
    row: u16,
) -> Option<Vec<u8>> {
    if mode == MouseProtocolMode::None {
        return None;
    }
    if matches!(click, Click::Move) && mode != MouseProtocolMode::AnyMotion {
        return None;
    }
    if matches!(click, Click::Drag)
        && !matches!(
            mode,
            MouseProtocolMode::ButtonMotion | MouseProtocolMode::AnyMotion
        )
    {
        return None;
    }
    if matches!(click, Click::Release) && mode == MouseProtocolMode::Press {
        return None;
    }
    let code = match click {
        Click::Press | Click::Release => button,
        Click::Drag => 32 + button,
        Click::Move => 35,
    };
    let column = u32::from(col) + 1;
    let line = u32::from(row) + 1;
    Some(match encoding {
        MouseProtocolEncoding::Sgr => {
            let action = if click == Click::Release { b'm' } else { b'M' };
            format!("\x1b[<{code};{column};{line}{}", action as char).into_bytes()
        }
        MouseProtocolEncoding::Default | MouseProtocolEncoding::Utf8 => {
            if column > 223 || line > 223 {
                return None;
            }
            vec![
                0x1b,
                b'[',
                b'M',
                32 + code,
                (32 + column) as u8,
                (32 + line) as u8,
            ]
        }
    })
}

pub fn link_at(screen: &Screen, row: u16, col: u16) -> Option<String> {
    for (start, end, url) in links_in_row(screen, row) {
        if col >= start && col <= end {
            return Some(url);
        }
    }
    None
}

pub fn links_in_row(screen: &Screen, row: u16) -> Vec<(u16, u16, String)> {
    let cols = screen.size().1.get();
    let mut plain = String::new();
    let mut span = Vec::new();
    let mut col = 0u16;
    while col < cols {
        let Some(cell) = screen.cell(row, col) else {
            break;
        };
        if cell.is_wide_continuation() {
            col += 1;
            continue;
        }
        let width = if cell.is_wide() { 2 } else { 1 };
        let contents = cell.contents();
        let chunk = if contents.is_empty() { " " } else { contents };
        for ch in chunk.chars() {
            let byte = plain.len();
            plain.push(ch);
            span.push((byte, col, col + width - 1));
        }
        col += width;
    }
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(rel) = plain[from..]
        .find("https://")
        .or_else(|| plain[from..].find("http://"))
    {
        let start = from + rel;
        let mut end = start;
        for (index, ch) in plain[start..].char_indices() {
            if ch.is_whitespace() || matches!(ch, '"' | '\'' | ')' | '>') {
                break;
            }
            end = start + index + ch.len_utf8();
        }
        if end > start
            && let Some((_, first, _)) = span.iter().find(|(byte, _, _)| *byte == start)
            && let Some((_, _, last)) = span.iter().rev().find(|(byte, _, _)| *byte < end)
        {
            found.push((*first, *last, plain[start..end].to_string()));
        }
        from = end.max(start + 1);
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sgr_click_is_one_based() {
        let bytes = report(
            MouseProtocolMode::PressRelease,
            MouseProtocolEncoding::Sgr,
            Click::Press,
            0,
            0,
            2,
        );
        assert_eq!(bytes.as_deref(), Some(b"\x1b[<0;1;3M".as_slice()));
        let bytes = report(
            MouseProtocolMode::PressRelease,
            MouseProtocolEncoding::Sgr,
            Click::Release,
            0,
            0,
            2,
        );
        assert_eq!(bytes.as_deref(), Some(b"\x1b[<0;1;3m".as_slice()));
    }

    #[test]
    fn motion_waits_for_the_tracking_mode() {
        assert!(
            report(
                MouseProtocolMode::PressRelease,
                MouseProtocolEncoding::Sgr,
                Click::Move,
                0,
                0,
                0,
            )
            .is_none()
        );
        assert!(
            report(
                MouseProtocolMode::AnyMotion,
                MouseProtocolEncoding::Sgr,
                Click::Move,
                0,
                4,
                1,
            )
            .is_some()
        );
    }

    #[test]
    fn https_cell_is_clickable() {
        let rows = std::num::NonZeroU16::new(5).unwrap();
        let cols = std::num::NonZeroU16::new(40).unwrap();
        let mut parser = vt100::Parser::new(rows, cols, 0);
        parser.process(b"see https://lew.tec.br/rterm now");
        assert_eq!(
            link_at(parser.screen(), 0, 5).as_deref(),
            Some("https://lew.tec.br/rterm")
        );
        assert!(link_at(parser.screen(), 0, 0).is_none());
    }
}
