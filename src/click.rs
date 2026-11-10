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

/// A wheel delta that has not yet filled a whole cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WheelUnit {
    /// Already measured in rows.
    Line,
    /// Logical pixels. Accumulate until a cell is covered.
    Point,
    /// One step is `page_rows` lines.
    Page,
}

/// Whole rows from one wheel event.
///
/// Positive values move the view up into older rows, matching the egui
/// wheel vector (positive Y moves the content down).
pub fn take_wheel_lines(
    remainder: &mut f32,
    unit: WheelUnit,
    delta_y: f32,
    cell_height: f32,
    page_rows: f32,
) -> i32 {
    if delta_y == 0.0 || !delta_y.is_finite() {
        return 0;
    }
    match unit {
        WheelUnit::Point => {
            if cell_height <= 0.0 {
                return 0;
            }
            *remainder += delta_y;
            let lines = (*remainder / cell_height).trunc();
            *remainder -= lines * cell_height;
            lines as i32
        }
        WheelUnit::Line | WheelUnit::Page => {
            *remainder = 0.0;
            let scaled = if unit == WheelUnit::Page {
                delta_y * page_rows.max(1.0)
            } else {
                delta_y
            };
            whole_lines(scaled)
        }
    }
}

fn whole_lines(scaled: f32) -> i32 {
    let rounded = scaled.round() as i32;
    if rounded != 0 {
        rounded
    } else if scaled > 0.0 {
        1
    } else {
        -1
    }
}

/// Where one wheel notch goes.
///
/// The app receives button 64 (up) or 65 (down) while it tracks the mouse.
/// The alternate screen receives cursor keys. Otherwise the wheel moves
/// local scrollback. Shift scrolls locally, except on the alternate screen,
/// which has no scrollback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WheelAction {
    Report { button: u8, times: u32 },
    Cursor { up: bool, times: u32 },
    Scroll(i32),
}

const WHEEL_REPORT_CAP: u32 = 8;

pub fn wheel_action(mouse: bool, alternate: bool, shift: bool, lines: i32) -> Option<WheelAction> {
    if lines == 0 {
        return None;
    }
    let up = lines > 0;
    let times = lines.unsigned_abs().clamp(1, WHEEL_REPORT_CAP);
    if mouse && !shift {
        return Some(WheelAction::Report {
            button: if up { 64 } else { 65 },
            times,
        });
    }
    if alternate {
        if shift {
            return None;
        }
        return Some(WheelAction::Cursor { up, times });
    }
    Some(WheelAction::Scroll(lines))
}

pub fn scroll_screen(screen: &mut Screen, rows: i32) {
    let next = if rows >= 0 {
        screen.scrollback().saturating_add(rows as usize)
    } else {
        screen
            .scrollback()
            .saturating_sub(rows.unsigned_abs() as usize)
    };
    screen.set_scrollback(next);
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
    fn wheel_up_is_button_64() {
        let bytes = report(
            MouseProtocolMode::PressRelease,
            MouseProtocolEncoding::Sgr,
            Click::Press,
            64,
            1,
            2,
        );
        assert_eq!(bytes.as_deref(), Some(b"\x1b[<64;2;3M".as_slice()));
    }

    #[test]
    fn point_deltas_accumulate_to_a_row() {
        let mut remainder = 0.0;
        assert_eq!(
            take_wheel_lines(&mut remainder, WheelUnit::Point, 8.0, 16.0, 24.0),
            0
        );
        assert_eq!(
            take_wheel_lines(&mut remainder, WheelUnit::Point, 8.0, 16.0, 24.0),
            1
        );
        assert_eq!(
            take_wheel_lines(&mut remainder, WheelUnit::Point, -16.0, 16.0, 24.0),
            -1
        );
    }

    #[test]
    fn a_line_notch_moves_at_least_one_row() {
        let mut remainder = 3.0;
        assert_eq!(
            take_wheel_lines(&mut remainder, WheelUnit::Line, 1.2, 16.0, 24.0),
            1
        );
        assert_eq!(remainder, 0.0);
        assert_eq!(
            take_wheel_lines(&mut remainder, WheelUnit::Line, -0.2, 16.0, 24.0),
            -1
        );
        assert_eq!(
            take_wheel_lines(&mut remainder, WheelUnit::Page, 1.0, 16.0, 24.0),
            24
        );
    }

    #[test]
    fn wheel_routes_like_the_terminal_view() {
        assert_eq!(
            wheel_action(true, false, false, 1),
            Some(WheelAction::Report {
                button: 64,
                times: 1
            })
        );
        assert_eq!(
            wheel_action(true, true, false, -3),
            Some(WheelAction::Report {
                button: 65,
                times: 3
            })
        );
        assert_eq!(
            wheel_action(true, false, false, 100),
            Some(WheelAction::Report {
                button: 64,
                times: 8
            })
        );
        assert_eq!(
            wheel_action(true, false, true, 4),
            Some(WheelAction::Scroll(4))
        );
        assert_eq!(
            wheel_action(false, true, false, -2),
            Some(WheelAction::Cursor {
                up: false,
                times: 2
            })
        );
        assert_eq!(wheel_action(false, true, true, 2), None);
        assert_eq!(
            wheel_action(false, false, false, 5),
            Some(WheelAction::Scroll(5))
        );
        assert_eq!(wheel_action(true, false, false, 0), None);
    }

    #[test]
    fn wheel_up_shows_older_rows() {
        let rows = std::num::NonZeroU16::new(2).unwrap();
        let cols = std::num::NonZeroU16::new(8).unwrap();
        let mut parser = vt100::Parser::new(rows, cols, 10);
        parser.process(b"aaa\r\nbbb\r\nccc\r\n");
        let live = parser.screen().contents();
        scroll_screen(parser.screen_mut(), 1);
        assert!(parser.screen().scrollback() >= 1);
        assert_ne!(parser.screen().contents(), live);
        scroll_screen(parser.screen_mut(), -10);
        assert_eq!(parser.screen().scrollback(), 0);
        assert_eq!(parser.screen().contents(), live);
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
