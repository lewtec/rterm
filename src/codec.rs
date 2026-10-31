use uuid::Uuid;

use crate::backend::Backend;
use crate::place::Place;
use crate::uuidv7::uuid_upper;

pub const CURRENT_VERSION: i64 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogDocument {
    pub version: i64,
    pub places: Vec<Place>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogError {
    UnsupportedVersion(i64),
    UnexpectedLine(usize, String),
    MissingField(String, usize),
    InvalidValue(String, usize),
    SessionRequired(usize),
}

impl std::fmt::Display for CatalogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedVersion(version) => {
                write!(f, "catalog version {version} is not supported")
            }
            Self::UnexpectedLine(line, text) => {
                write!(f, "catalog line {line}: unexpected {text}")
            }
            Self::MissingField(field, line) => {
                write!(f, "catalog line {line}: missing {field}")
            }
            Self::InvalidValue(field, line) => {
                write!(f, "catalog line {line}: invalid {field}")
            }
            Self::SessionRequired(line) => {
                write!(f, "catalog line {line}: tmux and screen require session")
            }
        }
    }
}

impl std::error::Error for CatalogError {}

pub fn decode(text: &str) -> Result<CatalogDocument, CatalogError> {
    let mut version: Option<i64> = None;
    let mut places = Vec::new();
    let mut current: Option<PartialPlace> = None;
    let mut current_start = 0usize;

    for (offset, raw_line) in split_lines(text).into_iter().enumerate() {
        let line_number = offset + 1;
        let line = trim_spaces(raw_line);
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == "[[place]]" {
            if let Some(partial) = current.take() {
                places.push(partial.finish(current_start)?);
            }
            current = Some(PartialPlace::default());
            current_start = line_number;
            continue;
        }
        let Some(eq) = line.find('=') else {
            return Err(CatalogError::UnexpectedLine(line_number, line.to_string()));
        };
        let key = trim_spaces(&line[..eq]);
        let raw_value = trim_spaces(&line[eq + 1..]);
        if current.is_none() {
            if key == "version" {
                let parsed = raw_value
                    .parse::<i64>()
                    .map_err(|_| CatalogError::InvalidValue("version".into(), line_number))?;
                version = Some(parsed);
                continue;
            }
            return Err(CatalogError::UnexpectedLine(line_number, line.to_string()));
        }
        let value = decode_scalar(raw_value, line_number)?;
        let partial = current.as_mut().expect("place table");
        match key {
            "id" => {
                let id = Uuid::parse_str(&value)
                    .map_err(|_| CatalogError::InvalidValue("id".into(), line_number))?;
                partial.id = Some(id);
            }
            "user" => partial.user = Some(value),
            "host" => partial.host = Some(value),
            "backend" => {
                let backend = Backend::parse(&value)
                    .ok_or_else(|| CatalogError::InvalidValue("backend".into(), line_number))?;
                partial.backend = Some(backend);
            }
            "session" => partial.session = Some(value),
            "label" => partial.label = Some(value),
            _ => {
                return Err(CatalogError::UnexpectedLine(line_number, line.to_string()));
            }
        }
    }
    if let Some(partial) = current.take() {
        places.push(partial.finish(current_start)?);
    }
    let resolved = version.unwrap_or(0);
    if resolved != CURRENT_VERSION {
        return Err(CatalogError::UnsupportedVersion(resolved));
    }
    Ok(CatalogDocument {
        version: resolved,
        places,
    })
}

pub fn encode(document: &CatalogDocument) -> String {
    let mut lines = vec![format!("version = {}", document.version), String::new()];
    for place in &document.places {
        lines.push("[[place]]".into());
        lines.push(format!("id = {}", quote(&uuid_upper(place.id))));
        if !place.user.is_empty() {
            lines.push(format!("user = {}", quote(&place.user)));
        }
        if !place.is_local() && !place.host.is_empty() {
            lines.push(format!("host = {}", quote(&place.host)));
        }
        lines.push(format!("backend = {}", quote(place.backend.as_str())));
        if place.backend != Backend::Herdr
            && let Some(session) = place.session.as_ref().filter(|session| !session.is_empty())
        {
            lines.push(format!("session = {}", quote(session)));
        }
        if let Some(label) = place.label.as_ref().filter(|label| !label.is_empty()) {
            lines.push(format!("label = {}", quote(label)));
        }
        lines.push(String::new());
    }
    lines.join("\n")
}

pub fn decode_place_file(text: &str) -> Result<Place, CatalogError> {
    let trimmed = text.trim();
    if trimmed.contains("[[place]]") || trimmed.contains("version") {
        let owned;
        let source = if trimmed.contains("version") {
            trimmed
        } else {
            owned = format!("version = {CURRENT_VERSION}\n\n{trimmed}");
            owned.as_str()
        };
        let document = decode(source)?;
        return document
            .places
            .into_iter()
            .next()
            .ok_or_else(|| CatalogError::MissingField("place".into(), 1));
    }
    Place::parse_switch_key(trimmed).ok_or_else(|| CatalogError::InvalidValue("place".into(), 1))
}

pub fn encode_place_file(place: &Place) -> String {
    encode(&CatalogDocument {
        version: CURRENT_VERSION,
        places: vec![place.clone()],
    })
}

fn decode_scalar(raw: &str, line: usize) -> Result<String, CatalogError> {
    if !raw.starts_with('"') {
        return Ok(raw.to_string());
    }
    if !raw.ends_with('"') || raw.len() < 2 {
        return Err(CatalogError::InvalidValue("string".into(), line));
    }
    let inner = &raw[1..raw.len() - 1];
    let mut out = String::new();
    let mut escape = false;
    for character in inner.chars() {
        if escape {
            match character {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                '\\' => out.push('\\'),
                '"' => out.push('"'),
                _ => return Err(CatalogError::InvalidValue("string".into(), line)),
            }
            escape = false;
        } else if character == '\\' {
            escape = true;
        } else {
            out.push(character);
        }
    }
    if escape {
        return Err(CatalogError::InvalidValue("string".into(), line));
    }
    Ok(out)
}

fn quote(value: &str) -> String {
    let mut escaped = String::from("\"");
    for character in value.chars() {
        match character {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\t' => escaped.push_str("\\t"),
            _ => escaped.push(character),
        }
    }
    escaped.push('"');
    escaped
}

#[derive(Default)]
struct PartialPlace {
    id: Option<Uuid>,
    user: Option<String>,
    host: Option<String>,
    backend: Option<Backend>,
    session: Option<String>,
    label: Option<String>,
}

impl PartialPlace {
    fn finish(self, line: usize) -> Result<Place, CatalogError> {
        let id = self
            .id
            .ok_or_else(|| CatalogError::MissingField("id".into(), line))?;
        let backend = self
            .backend
            .ok_or_else(|| CatalogError::MissingField("backend".into(), line))?;
        let mut host = self.host.unwrap_or_default().trim().to_string();
        let user = self.user.unwrap_or_default().trim().to_string();
        if Place::is_loopback_host(&host) {
            host.clear();
        } else if user.is_empty() {
            return Err(CatalogError::MissingField("user".into(), line));
        }
        let session = self.session.map(|value| value.trim().to_string());
        if backend != Backend::Herdr && session.as_ref().is_none_or(|value| value.is_empty()) {
            return Err(CatalogError::SessionRequired(line));
        }
        let label = self
            .label
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        Ok(Place {
            id,
            user,
            host,
            backend,
            session: if backend == Backend::Herdr {
                None
            } else {
                session
            },
            label,
        })
    }
}

/// Split on `\n`, `\r\n`, or `\r` and keep empty lines, matching Swift's
/// newline split (CRLF is one break).
fn split_lines(text: &str) -> Vec<&str> {
    let bytes = text.as_bytes();
    let mut lines = Vec::new();
    let mut start = 0;
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\n' || bytes[index] == b'\r' {
            lines.push(&text[start..index]);
            if bytes[index] == b'\r' && index + 1 < bytes.len() && bytes[index + 1] == b'\n' {
                index += 2;
            } else {
                index += 1;
            }
            start = index;
        } else {
            index += 1;
        }
    }
    lines.push(&text[start..]);
    lines
}

fn trim_spaces(value: &str) -> &str {
    value.trim_matches(|c: char| matches!(c, ' ' | '\t'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn id(text: &str) -> Uuid {
        Uuid::parse_str(text).unwrap()
    }

    /// Swift `"""` strips one shared indent. Test literals keep that indent.
    fn swift(text: &str) -> String {
        let text = text.trim_start_matches('\n');
        let indent = text
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| line.len() - line.trim_start().len())
            .min()
            .unwrap_or(0);
        let lines: Vec<_> = text
            .lines()
            .map(|line| {
                if line.len() >= indent {
                    &line[indent..]
                } else {
                    line.trim_start()
                }
            })
            .collect();
        lines.join("\n")
    }

    #[test]
    fn round_trip_preserves_order_and_optional_fields() {
        let places = vec![
            Place {
                id: id("11111111-1111-1111-1111-111111111111"),
                user: "ada".into(),
                host: "riverwood".into(),
                backend: Backend::Herdr,
                session: None,
                label: None,
            },
            Place {
                id: id("22222222-2222-2222-2222-222222222222"),
                user: "ada".into(),
                host: "whiterun".into(),
                backend: Backend::Tmux,
                session: Some("foo".into()),
                label: Some("work".into()),
            },
        ];
        let encoded = encode(&CatalogDocument {
            version: 1,
            places: places.clone(),
        });
        let decoded = decode(&encoded).unwrap();
        assert_eq!(decoded.version, 1);
        assert_eq!(decoded.places, places);
        let herdr_only = encode(&CatalogDocument {
            version: 1,
            places: vec![places[0].clone()],
        });
        assert!(!herdr_only.contains("session"));
        let place_file = encode_place_file(&places[1]);
        assert_eq!(decode_place_file(&place_file).unwrap(), places[1]);
    }

    #[test]
    fn herdr_session_is_dropped() {
        let text = swift(
            r#"
        version = 1

        [[place]]
        id = "11111111-1111-1111-1111-111111111111"
        user = "ada"
        host = "riverwood"
        backend = "herdr"
        session = "ignored"
        "#,
        );
        let decoded = decode(&text).unwrap();
        assert!(decoded.places[0].session.is_none());
    }

    #[test]
    fn tmux_requires_session() {
        let text = swift(
            r#"
        version = 1

        [[place]]
        id = "11111111-1111-1111-1111-111111111111"
        user = "ada"
        host = "whiterun"
        backend = "tmux"
        "#,
        );
        assert_eq!(decode(&text).unwrap_err(), CatalogError::SessionRequired(3));
    }

    #[test]
    fn quoted_escapes() {
        let place = Place {
            id: id("11111111-1111-1111-1111-111111111111"),
            user: "a\"b".into(),
            host: "host".into(),
            backend: Backend::Screen,
            session: Some("s".into()),
            label: Some("line\nbreak".into()),
        };
        let decoded = decode(&encode(&CatalogDocument {
            version: 1,
            places: vec![place.clone()],
        }))
        .unwrap();
        assert_eq!(decoded.places[0], place);
    }

    #[test]
    fn local_place_omits_user_and_host() {
        let place = Place {
            id: id("11111111-1111-1111-1111-111111111111"),
            user: String::new(),
            host: String::new(),
            backend: Backend::Herdr,
            session: None,
            label: None,
        };
        let encoded = encode(&CatalogDocument {
            version: 1,
            places: vec![place],
        });
        assert!(!encoded.contains("user ="));
        assert!(!encoded.contains("host ="));
        let decoded = decode(&encoded).unwrap();
        assert!(decoded.places[0].is_local());
        assert_eq!(decoded.places[0].user, "");
        assert_eq!(decoded.places[0].host, "");
    }

    #[test]
    fn localhost_host_normalizes_to_local_place() {
        let text = swift(
            r#"
        version = 1

        [[place]]
        id = "11111111-1111-1111-1111-111111111111"
        user = "ada"
        host = "localhost"
        backend = "herdr"
        "#,
        );
        let decoded = decode(&text).unwrap();
        assert_eq!(decoded.places[0].host, "");
        assert_eq!(decoded.places[0].user, "ada");
        assert!(decoded.places[0].is_local());
        let encoded = encode(&decoded);
        assert!(!encoded.contains("localhost"));
        assert!(encoded.contains("user = \"ada\""));
    }

    #[test]
    fn remote_place_still_requires_user() {
        let text = swift(
            r#"
        version = 1

        [[place]]
        id = "11111111-1111-1111-1111-111111111111"
        host = "riverwood"
        backend = "herdr"
        "#,
        );
        assert_eq!(
            decode(&text).unwrap_err(),
            CatalogError::MissingField("user".into(), 3)
        );
    }

    #[test]
    fn rejects_unknown_version() {
        assert_eq!(
            decode("version = 2\n").unwrap_err(),
            CatalogError::UnsupportedVersion(2)
        );
    }
}
