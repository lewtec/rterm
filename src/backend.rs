#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Backend {
    Herdr,
    Tmux,
    Screen,
}

impl Backend {
    pub const ALL: [Backend; 3] = [Backend::Herdr, Backend::Tmux, Backend::Screen];

    pub fn as_str(self) -> &'static str {
        match self {
            Backend::Herdr => "herdr",
            Backend::Tmux => "tmux",
            Backend::Screen => "screen",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "herdr" => Some(Backend::Herdr),
            "tmux" => Some(Backend::Tmux),
            "screen" => Some(Backend::Screen),
            _ => None,
        }
    }
}
