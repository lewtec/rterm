use rterm::paste::{self, Gesture, Plan};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NamedKey {
    Enter,
    Backspace,
    Tab,
    Escape,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Delete,
    Insert,
    Letter(u8),
    Digit(u8),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    Text(String),
    /// Clipboard text already decoded by the toolkit.
    Pasted(String),
    Key {
        key: NamedKey,
        pressed: bool,
        ctrl: bool,
        mac_cmd: bool,
        alt: bool,
        shift: bool,
        repeat: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Write(Vec<u8>),
    AddPlace,
    Reconnect,
    SelectIndex(usize),
    ToggleFill,
    Paste(Gesture),
    ExitFill,
    CloseEditor,
    SaveEditor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasteEffect {
    Image,
    Text,
    Byte(u8),
    None,
}

pub fn paste_effect(
    has_image: bool,
    has_text: bool,
    is_local: bool,
    gesture: Gesture,
) -> PasteEffect {
    if paste::plan(has_image, has_text, is_local, gesture) == Plan::Deliver {
        return PasteEffect::Image;
    }
    match gesture {
        Gesture::CommandV if has_text => PasteEffect::Text,
        Gesture::ControlV => PasteEffect::Byte(0x16),
        _ => PasteEffect::None,
    }
}

pub fn estimated_tab_width(label: &str) -> f64 {
    8.0 + 7.0 + 6.0 + label.chars().count() as f64 * 7.2 + 8.0
}

/// Text to write for one frame.
///
/// An IME commit replaces plain text from that frame, so a dead key is not
/// also sent as its base letter.
pub fn frame_text(commits: &[String], texts: &[String]) -> Vec<String> {
    let commits: Vec<String> = commits
        .iter()
        .filter(|text| !text.is_empty())
        .cloned()
        .collect();
    if commits.is_empty() {
        texts
            .iter()
            .filter(|text| !text.is_empty())
            .cloned()
            .collect()
    } else {
        commits
    }
}

#[cfg(test)]
pub fn interpret(batch: &[Input], fill: bool, editor_open: bool, text_field: bool) -> Vec<Action> {
    interpret_with(batch, fill, editor_open, text_field, 0, false)
}

pub fn interpret_with(
    batch: &[Input],
    fill: bool,
    editor_open: bool,
    text_field: bool,
    kitty_flags: u16,
    app_cursor: bool,
) -> Vec<Action> {
    let mut out = Vec::new();
    let mut pasted: Option<String> = None;
    let mut saw_paste_key = false;
    for input in batch {
        match input {
            Input::Text(text) => {
                if editor_open || text_field || is_special_text(text) {
                    continue;
                }
                out.push(Action::Write(text.as_bytes().to_vec()));
            }
            Input::Pasted(text) => {
                if !editor_open && !text_field {
                    pasted = Some(text.clone());
                }
            }
            Input::Key {
                pressed: false,
                key,
                ctrl,
                mac_cmd,
                alt,
                shift,
                ..
            } => {
                if let Some(bytes) = kitty_bytes(
                    *key,
                    *ctrl,
                    *mac_cmd,
                    *alt,
                    *shift,
                    rterm::kitty::Phase::Release,
                    kitty_flags,
                ) {
                    out.push(Action::Write(bytes));
                }
            }
            Input::Key {
                key,
                ctrl,
                mac_cmd,
                alt,
                shift,
                repeat,
                pressed: true,
            } => {
                if *ctrl && *mac_cmd && *key == NamedKey::Letter(b'f') {
                    out.push(Action::ToggleFill);
                    continue;
                }
                if editor_open || text_field {
                    if *key == NamedKey::Escape && editor_open {
                        out.push(Action::CloseEditor);
                    } else if *key == NamedKey::Enter && editor_open {
                        out.push(Action::SaveEditor);
                    } else if *mac_cmd
                        && !*ctrl
                        && let Some(action) = command_shortcut(*key)
                        && !matches!(action, Action::Paste(_))
                    {
                        out.push(action);
                    }
                    continue;
                }
                if *mac_cmd
                    && !*ctrl
                    && let Some(action) = command_shortcut(*key)
                {
                    if matches!(action, Action::Paste(_)) {
                        saw_paste_key = true;
                    }
                    out.push(action);
                    continue;
                }
                if *ctrl && !*mac_cmd && *key == NamedKey::Letter(b'v') {
                    saw_paste_key = true;
                    out.push(Action::Paste(Gesture::ControlV));
                    continue;
                }
                if *key == NamedKey::Escape {
                    if fill {
                        out.push(Action::ExitFill);
                    } else if let Some(bytes) = kitty_bytes(
                        *key,
                        *ctrl,
                        *mac_cmd,
                        *alt,
                        *shift,
                        phase(*repeat),
                        kitty_flags,
                    ) {
                        out.push(Action::Write(bytes));
                    } else {
                        out.push(Action::Write(b"\x1b".to_vec()));
                    }
                    continue;
                }
                if let Some(bytes) = kitty_bytes(
                    *key,
                    *ctrl,
                    *mac_cmd,
                    *alt,
                    *shift,
                    phase(*repeat),
                    kitty_flags,
                ) {
                    out.push(Action::Write(bytes));
                    continue;
                }
                if let Some(bytes) = special_bytes(*key, *shift, app_cursor) {
                    out.push(Action::Write(bytes));
                    continue;
                }
                if *ctrl
                    && !*mac_cmd
                    && !*alt
                    && let NamedKey::Letter(ch) = *key
                {
                    out.push(Action::Write(vec![ch - b'a' + 1]));
                }
            }
        }
    }
    if let Some(text) = pasted.filter(|_| !saw_paste_key)
        && !text.is_empty()
    {
        out.push(Action::Write(text.into_bytes()));
    }
    out
}

fn command_shortcut(key: NamedKey) -> Option<Action> {
    match key {
        NamedKey::Letter(b'n') => Some(Action::AddPlace),
        NamedKey::Letter(b'r') => Some(Action::Reconnect),
        NamedKey::Letter(b'v') => Some(Action::Paste(Gesture::CommandV)),
        NamedKey::Digit(0) => Some(Action::SelectIndex(9)),
        NamedKey::Digit(digit @ 1..=9) => Some(Action::SelectIndex((digit - 1) as usize)),
        _ => None,
    }
}

fn is_special_text(text: &str) -> bool {
    matches!(text, "\n" | "\r" | "\t")
}

pub fn special_bytes(key: NamedKey, shift: bool, app_cursor: bool) -> Option<Vec<u8>> {
    let cursor = |letter: u8| {
        if app_cursor {
            vec![0x1b, b'O', letter]
        } else {
            vec![0x1b, b'[', letter]
        }
    };
    Some(match key {
        NamedKey::Enter => b"\r".to_vec(),
        NamedKey::Backspace => b"\x7f".to_vec(),
        NamedKey::Tab if shift => b"\x1b[Z".to_vec(),
        NamedKey::Tab => b"\t".to_vec(),
        NamedKey::Up => cursor(b'A'),
        NamedKey::Down => cursor(b'B'),
        NamedKey::Right => cursor(b'C'),
        NamedKey::Left => cursor(b'D'),
        NamedKey::Home => cursor(b'H'),
        NamedKey::End => cursor(b'F'),
        NamedKey::PageUp => b"\x1b[5~".to_vec(),
        NamedKey::PageDown => b"\x1b[6~".to_vec(),
        NamedKey::Delete => b"\x1b[3~".to_vec(),
        NamedKey::Insert => b"\x1b[2~".to_vec(),
        NamedKey::Escape => b"\x1b".to_vec(),
        _ => return None,
    })
}

fn phase(repeat: bool) -> rterm::kitty::Phase {
    if repeat {
        rterm::kitty::Phase::Repeat
    } else {
        rterm::kitty::Phase::Press
    }
}

fn kitty_bytes(
    key: NamedKey,
    ctrl: bool,
    mac_cmd: bool,
    alt: bool,
    shift: bool,
    phase: rterm::kitty::Phase,
    flags: u16,
) -> Option<Vec<u8>> {
    let key = kitty_key(key)?;
    rterm::kitty::encode(
        flags,
        key,
        rterm::kitty::Mods {
            ctrl,
            alt,
            shift,
            super_key: mac_cmd,
        },
        phase,
    )
}

fn kitty_key(key: NamedKey) -> Option<rterm::kitty::Key> {
    use rterm::kitty::Key;
    Some(match key {
        NamedKey::Letter(ch) => Key::Char(ch as char),
        NamedKey::Digit(digit) => Key::Char((b'0' + digit) as char),
        NamedKey::Escape => Key::Escape,
        NamedKey::Enter => Key::Enter,
        NamedKey::Tab => Key::Tab,
        NamedKey::Backspace => Key::Backspace,
        NamedKey::Up => Key::Up,
        NamedKey::Down => Key::Down,
        NamedKey::Left => Key::Left,
        NamedKey::Right => Key::Right,
        NamedKey::Home => Key::Home,
        NamedKey::End => Key::End,
        NamedKey::PageUp => Key::PageUp,
        NamedKey::PageDown => Key::PageDown,
        NamedKey::Insert => Key::Insert,
        NamedKey::Delete => Key::Delete,
    })
}

#[cfg(test)]
fn key(key: NamedKey, ctrl: bool, mac_cmd: bool, alt: bool, shift: bool) -> Input {
    Input::Key {
        key,
        pressed: true,
        ctrl,
        mac_cmd,
        alt,
        shift,
        repeat: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paste_matrix_matches_the_pane_rules() {
        use PasteEffect::*;
        let image = (true, false);
        let text = (false, true);
        let both = (true, true);
        assert_eq!(
            paste_effect(image.0, image.1, true, Gesture::CommandV),
            Image
        );
        assert_eq!(
            paste_effect(image.0, image.1, false, Gesture::CommandV),
            Image
        );
        assert_eq!(
            paste_effect(image.0, image.1, true, Gesture::ControlV),
            Byte(0x16)
        );
        assert_eq!(
            paste_effect(image.0, image.1, false, Gesture::ControlV),
            Image
        );
        assert_eq!(paste_effect(text.0, text.1, true, Gesture::CommandV), Text);
        assert_eq!(
            paste_effect(text.0, text.1, false, Gesture::ControlV),
            Byte(0x16)
        );
        assert_eq!(paste_effect(both.0, both.1, false, Gesture::CommandV), Text);
        assert_eq!(
            paste_effect(both.0, both.1, true, Gesture::ControlV),
            Byte(0x16)
        );
        assert_eq!(paste_effect(false, false, true, Gesture::CommandV), None);
    }

    #[test]
    fn ime_commit_replaces_the_base_letter() {
        assert_eq!(
            frame_text(&["á".into()], &["a".into(), "á".into()]),
            vec!["á".to_string()]
        );
        assert_eq!(frame_text(&[], &["á".into()]), vec!["á".to_string()]);
        assert_eq!(
            frame_text(&[String::new()], &["a".into()]),
            vec!["a".to_string()]
        );
    }

    #[test]
    fn accented_text_is_utf8() {
        let batch = [Input::Text("á".into())];
        assert_eq!(
            interpret(&batch, false, false, false),
            vec![Action::Write("á".as_bytes().to_vec())]
        );
    }

    #[test]
    fn shortcuts_and_pty_bytes() {
        let batch = [
            key(NamedKey::Letter(b'v'), true, false, false, false),
            key(NamedKey::Letter(b'v'), false, true, false, false),
            key(NamedKey::Letter(b'a'), true, false, false, false),
            key(NamedKey::Enter, false, false, false, false),
            key(NamedKey::Tab, false, false, false, true),
            Input::Text("ab".into()),
            Input::Text("\n".into()),
        ];
        assert_eq!(
            interpret(&batch, false, false, false),
            vec![
                Action::Paste(Gesture::ControlV),
                Action::Paste(Gesture::CommandV),
                Action::Write(vec![1]),
                Action::Write(b"\r".to_vec()),
                Action::Write(b"\x1b[Z".to_vec()),
                Action::Write(b"ab".to_vec()),
            ]
        );
    }

    #[test]
    fn escape_depends_on_fill_and_editor() {
        let esc = [key(NamedKey::Escape, false, false, false, false)];
        assert_eq!(interpret(&esc, true, false, false), vec![Action::ExitFill]);
        assert_eq!(
            interpret(&esc, false, true, false),
            vec![Action::CloseEditor]
        );
        assert_eq!(
            interpret(&esc, false, false, false),
            vec![Action::Write(b"\x1b".to_vec())]
        );
    }

    #[test]
    fn editor_swallows_text_and_keeps_place_shortcuts() {
        let batch = [
            Input::Text("a".into()),
            key(NamedKey::Letter(b'n'), false, true, false, false),
            key(NamedKey::Enter, false, false, false, false),
            key(NamedKey::Digit(0), false, true, false, false),
        ];
        assert_eq!(
            interpret(&batch, false, true, true),
            vec![Action::AddPlace, Action::SaveEditor, Action::SelectIndex(9),]
        );
    }

    #[test]
    fn digit_shortcuts_select_places() {
        let batch = [key(NamedKey::Digit(1), false, true, false, false)];
        assert_eq!(
            interpret(&batch, false, false, false),
            vec![Action::SelectIndex(0)]
        );
    }

    #[test]
    fn fill_chord_and_tab_width() {
        let batch = [key(NamedKey::Letter(b'f'), true, true, false, false)];
        assert_eq!(
            interpret(&batch, false, true, true),
            vec![Action::ToggleFill]
        );
        assert_eq!(estimated_tab_width(""), 29.0);
        assert!((estimated_tab_width("ab") - (29.0 + 14.4)).abs() < 0.01);
    }

    #[test]
    fn toolkit_paste_text_is_not_doubled_after_the_key() {
        let batch = [
            key(NamedKey::Letter(b'v'), false, true, false, false),
            Input::Pasted("hello".into()),
        ];
        assert_eq!(
            interpret(&batch, false, false, false),
            vec![Action::Paste(Gesture::CommandV)]
        );
        let only = [Input::Pasted("hello".into())];
        assert_eq!(
            interpret(&only, false, false, false),
            vec![Action::Write(b"hello".to_vec())]
        );
    }
}
