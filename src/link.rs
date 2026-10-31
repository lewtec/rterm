use std::path::Path;

use crate::codec;
use crate::place::Place;

pub const SCHEME: &str = "rterm";
pub const HOST: &str = "place";
pub const CONTENT_TYPE: &str = "br.tec.lew.rterm.place";

pub struct PlaceUrl {
    pub scheme: &'static str,
    pub host: &'static str,
    pub raw: String,
}

pub fn url_for_place(place: &Place) -> PlaceUrl {
    url_for_key(&place.switch_key())
}

pub fn url_for_key(key: &str) -> PlaceUrl {
    PlaceUrl {
        scheme: SCHEME,
        host: HOST,
        raw: format!("{SCHEME}://{HOST}/{}", encode_path(key)),
    }
}

pub fn switch_key_from(input: &str) -> Option<String> {
    if let Some(rest) = input.strip_prefix("rterm://") {
        let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
        if host != HOST {
            return None;
        }
        let path = path.trim_matches('/');
        if path.is_empty() {
            return None;
        }
        let decoded = percent_decode(path);
        if decoded.is_empty() {
            None
        } else {
            Some(decoded)
        }
    } else if input.contains("://") {
        None
    } else {
        switch_key_from_file(Path::new(input))
    }
}

pub fn switch_key_from_file(path: &Path) -> Option<String> {
    if path.extension().and_then(|ext| ext.to_str()) != Some(crate::place::FILE_EXTENSION) {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    codec::decode_place_file(&text)
        .ok()
        .map(|place| place.switch_key())
}

fn encode_path(key: &str) -> String {
    let mut out = String::new();
    for byte in key.bytes() {
        if is_path_byte(byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn is_path_byte(byte: u8) -> bool {
    matches!(
        byte,
        b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'.'
            | b'_'
            | b'~'
            | b'@'
            | b':'
            | b'('
            | b')'
            | b'/'
    )
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or("");
            if let Ok(value) = u8::from_str_radix(hex, 16) {
                out.push(value);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::Backend;
    use crate::catalog::CatalogIo;
    use crate::codec::CatalogDocument;
    use crate::place::{self, Place};
    use std::path::PathBuf;
    use uuid::Uuid;

    struct Tmp(PathBuf);

    impl Tmp {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("rterm-link-{}", Uuid::new_v4()));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn round_trip_uses_switch_key() {
        let place = Place::new("ada", "whiterun", Backend::Herdr, None);
        assert_eq!(place.switch_key(), "ada@whiterun:herdr");
        let url = url_for_place(&place);
        assert_eq!(url.scheme, "rterm");
        assert_eq!(url.host, "place");
        assert_eq!(
            switch_key_from(&url.raw).as_deref(),
            Some("ada@whiterun:herdr")
        );
        assert_eq!(
            switch_key_from("rterm://place/ada@whiterun:herdr").as_deref(),
            Some("ada@whiterun:herdr")
        );
        assert!(switch_key_from("https://example.com").is_none());
    }

    #[test]
    fn tmux_session_stays_in_the_key() {
        let place = Place::new("ada", "whiterun", Backend::Tmux, Some("foo".into()));
        assert_eq!(place.switch_key(), "ada@whiterun:tmux(foo)");
        assert_eq!(
            switch_key_from(&url_for_place(&place).raw).as_deref(),
            Some(place.switch_key().as_str())
        );
    }

    #[test]
    fn parse_switch_key_covers_spec_form() {
        let herdr = Place::parse_switch_key("ada@whiterun").unwrap();
        assert_eq!(herdr.user, "ada");
        assert_eq!(herdr.host, "whiterun");
        assert_eq!(herdr.backend, Backend::Herdr);
        assert!(herdr.session.is_none());
        assert_eq!(herdr.switch_key(), "ada@whiterun:herdr");

        let explicit = Place::parse_switch_key("ada@whiterun:herdr").unwrap();
        assert_eq!(explicit.switch_key(), "ada@whiterun:herdr");

        let tmux = Place::parse_switch_key("ada@whiterun:tmux(foo)").unwrap();
        assert_eq!(tmux.backend, Backend::Tmux);
        assert_eq!(tmux.session.as_deref(), Some("foo"));

        let local = Place::parse_switch_key("ada@localhost:herdr").unwrap();
        assert_eq!(local.host, "");
        assert_eq!(local.user, "ada");
        assert!(local.is_local());

        assert!(Place::parse_switch_key("ada@whiterun:tmux").is_none());
        assert!(Place::parse_switch_key("whiterun:herdr").is_none());
    }

    #[test]
    fn place_file_uses_the_switch_key() {
        let place = Place {
            id: Uuid::parse_str("33333333-3333-3333-3333-333333333333").unwrap(),
            user: "ada".into(),
            host: "whiterun".into(),
            backend: Backend::Herdr,
            session: None,
            label: None,
        };
        assert_eq!(
            place::file_name(0, &place),
            "00-ada@whiterun-herdr.rterm-place"
        );
        let tmp = Tmp::new();
        let directory = tmp.0.clone();
        let io = CatalogIo::with_parts(directory.clone(), tmp.0.join("missing.toml"));
        io.save(&CatalogDocument {
            version: 1,
            places: vec![place.clone()],
        })
        .unwrap();
        let file = directory.join(place::file_name(0, &place));
        assert_eq!(
            switch_key_from_file(&file).as_deref(),
            Some("ada@whiterun:herdr")
        );
        assert_eq!(io.load().unwrap().places, vec![place]);
    }
}
