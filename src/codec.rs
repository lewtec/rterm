use uuid::Uuid;

use crate::backend::Backend;
use crate::place::Place;
use crate::route::{Route, RouteKind};
use crate::uuidv7::uuid_upper;

pub const CURRENT_VERSION: i64 = 2;
/// Files written before routes existed. They decode into a place with only the
/// host route and no pin.
pub const LEGACY_VERSION: i64 = 1;

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
    let mut current_route: Option<PartialRoute> = None;
    let mut current_start = 0usize;

    for (offset, raw_line) in split_lines(text).into_iter().enumerate() {
        let line_number = offset + 1;
        let line = trim_spaces(raw_line);
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == "[[place]]" {
            finish_route(&mut current, &mut current_route, line_number)?;
            if let Some(partial) = current.take() {
                places.push(partial.finish(current_start)?);
            }
            current = Some(PartialPlace::default());
            current_start = line_number;
            continue;
        }
        if line == "[[place.route]]" {
            finish_route(&mut current, &mut current_route, line_number)?;
            if current.is_none() {
                return Err(CatalogError::UnexpectedLine(line_number, line.to_string()));
            }
            current_route = Some(PartialRoute::default());
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
        if let Some(route) = current_route.as_mut() {
            match key {
                "kind" => {
                    let kind = RouteKind::parse(&value).ok_or_else(|| {
                        CatalogError::InvalidValue("route kind".into(), line_number)
                    })?;
                    route.kind = Some(kind);
                }
                "host" => route.host = Some(value),
                "port" => {
                    let port = value.parse::<u16>().map_err(|_| {
                        CatalogError::InvalidValue("route port".into(), line_number)
                    })?;
                    route.port = Some(port);
                }
                "last" => {
                    route.last = Some(match value.as_str() {
                        "true" => true,
                        "false" => false,
                        _ => {
                            return Err(CatalogError::InvalidValue(
                                "route last".into(),
                                line_number,
                            ));
                        }
                    });
                }
                _ => {
                    return Err(CatalogError::UnexpectedLine(line_number, line.to_string()));
                }
            }
            continue;
        }
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
            "fingerprint" => partial.fingerprint = Some(value),
            _ => {
                return Err(CatalogError::UnexpectedLine(line_number, line.to_string()));
            }
        }
    }
    finish_route(&mut current, &mut current_route, current_start)?;
    if let Some(partial) = current.take() {
        places.push(partial.finish(current_start)?);
    }
    let resolved = version.unwrap_or(0);
    if resolved != CURRENT_VERSION && resolved != LEGACY_VERSION {
        return Err(CatalogError::UnsupportedVersion(resolved));
    }
    Ok(CatalogDocument {
        version: resolved,
        places,
    })
}

fn finish_route(
    current: &mut Option<PartialPlace>,
    route: &mut Option<PartialRoute>,
    line: usize,
) -> Result<(), CatalogError> {
    let Some(route) = route.take() else {
        return Ok(());
    };
    let Some(place) = current.as_mut() else {
        return Err(CatalogError::UnexpectedLine(line, "[[place.route]]".into()));
    };
    place.routes.push(route.finish(line)?);
    Ok(())
}

pub fn encode(document: &CatalogDocument) -> String {
    let mut lines = vec![format!("version = {CURRENT_VERSION}"), String::new()];
    for place in &document.places {
        lines.push("[[place]]".into());
        lines.push(format!("id = {}", quote(&uuid_upper(place.id))));
        if !place.user.is_empty() {
            lines.push(format!("user = {}", quote(&place.user)));
        }
        if !Place::is_loopback_host(&place.host) && !place.host.is_empty() {
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
        if let Some(fingerprint) = place
            .fingerprint
            .as_ref()
            .filter(|fingerprint| !fingerprint.is_empty())
        {
            lines.push(format!("fingerprint = {}", quote(fingerprint)));
        }
        lines.push(String::new());
        for route in &place.routes {
            lines.push("[[place.route]]".into());
            lines.push(format!("kind = {}", quote(route.kind.as_str())));
            lines.push(format!("host = {}", quote(&route.host)));
            if let Some(port) = route.port {
                lines.push(format!("port = {port}"));
            }
            if route.last {
                lines.push("last = true".into());
            }
            lines.push(String::new());
        }
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
    fingerprint: Option<String>,
    routes: Vec<Route>,
}

#[derive(Default)]
struct PartialRoute {
    kind: Option<RouteKind>,
    host: Option<String>,
    port: Option<u16>,
    last: Option<bool>,
}

impl PartialRoute {
    fn finish(self, line: usize) -> Result<Route, CatalogError> {
        let kind = self
            .kind
            .ok_or_else(|| CatalogError::MissingField("route kind".into(), line))?;
        let host = self.host.unwrap_or_default().trim().to_string();
        if host.is_empty() {
            return Err(CatalogError::MissingField("route host".into(), line));
        }
        Ok(Route {
            kind,
            host,
            port: self.port,
            last: self.last.unwrap_or(false),
        })
    }
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
        // Routes are the truth. A file that only lists routes still names the
        // node, and the first route stands in for the missing endpoint.
        let routes = self.routes;
        if host.is_empty() {
            host = routes
                .iter()
                .find(|route| !route.host.trim().is_empty())
                .map(|route| route.host.trim().to_string())
                .unwrap_or_default();
        }
        if Place::is_loopback_host(&host) {
            host.clear();
        }
        let session = self.session.map(|value| value.trim().to_string());
        if backend != Backend::Herdr && session.as_ref().is_none_or(|value| value.is_empty()) {
            return Err(CatalogError::SessionRequired(line));
        }
        let label = self
            .label
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let fingerprint = self
            .fingerprint
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let local = host.is_empty();
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
            routes: if local { Vec::new() } else { routes },
            fingerprint: if local { None } else { fingerprint },
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
                routes: Vec::new(),
                fingerprint: None,
            },
            Place {
                id: id("22222222-2222-2222-2222-222222222222"),
                user: "ada".into(),
                host: "remote.example".into(),
                backend: Backend::Tmux,
                session: Some("foo".into()),
                label: Some("work".into()),
                routes: Vec::new(),
                fingerprint: None,
            },
        ];
        let encoded = encode(&CatalogDocument {
            version: 1,
            places: places.clone(),
        });
        let decoded = decode(&encoded).unwrap();
        assert_eq!(decoded.version, 2);
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
        host = "remote.example"
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
            routes: Vec::new(),
            fingerprint: None,
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
            routes: Vec::new(),
            fingerprint: None,
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
    fn remote_place_without_user_uses_the_ssh_login() {
        let text = swift(
            r#"
        version = 1

        [[place]]
        id = "11111111-1111-1111-1111-111111111111"
        host = "riverwood"
        backend = "herdr"
        "#,
        );
        let decoded = decode(&text).unwrap();
        assert_eq!(decoded.places[0].user, "");
        assert_eq!(decoded.places[0].host, "riverwood");
        assert_eq!(
            Place::parse_switch_key("riverwood:herdr")
                .unwrap()
                .switch_key(),
            decoded.places[0].switch_key()
        );
    }

    #[test]
    fn routes_alone_name_the_place() {
        let text = swift(
            r#"
        version = 2

        [[place]]
        id = "11111111-1111-1111-1111-111111111111"
        backend = "herdr"

        [[place.route]]
        kind = "direct"
        host = "riverwood"

        [[place.route]]
        kind = "mdns"
        host = "riverwood.local"
        last = true
        "#,
        );
        let decoded = decode(&text).unwrap();
        assert_eq!(decoded.places[0].host, "riverwood");
        assert_eq!(decoded.places[0].routes.len(), 2);
    }

    #[test]
    fn routes_and_fingerprint_round_trip() {
        let place = Place {
            id: id("11111111-1111-1111-1111-111111111111"),
            user: "ada".into(),
            host: "whiterun".into(),
            backend: Backend::Herdr,
            session: None,
            label: None,
            routes: vec![
                Route {
                    kind: RouteKind::Mdns,
                    host: "whiterun.local".into(),
                    port: None,
                    last: false,
                },
                Route {
                    kind: RouteKind::Tailscale,
                    host: "whiterun.tailnet.ts.net".into(),
                    port: Some(2222),
                    last: true,
                },
            ],
            fingerprint: Some("SHA256:C8olMcLxERkizT4dlzO1z85UVxdq".into()),
        };
        let encoded = encode_place_file(&place);
        assert!(encoded.contains("[[place.route]]"));
        assert!(encoded.contains("kind = \"mdns\""));
        assert!(encoded.contains("port = 2222"));
        assert_eq!(decode_place_file(&encoded).unwrap(), place);
    }

    #[test]
    fn version_one_place_has_no_routes_or_pin() {
        let text = swift(
            r#"
        version = 1

        [[place]]
        id = "11111111-1111-1111-1111-111111111111"
        user = "ada"
        host = "whiterun"
        backend = "herdr"
        "#,
        );
        let decoded = decode(&text).unwrap();
        assert_eq!(decoded.version, 1);
        assert!(decoded.places[0].routes.is_empty());
        assert!(decoded.places[0].fingerprint.is_none());
        assert!(encode(&decoded).contains("version = 2"));
    }

    #[test]
    fn route_requires_host() {
        let text = swift(
            r#"
        version = 2

        [[place]]
        id = "11111111-1111-1111-1111-111111111111"
        user = "ada"
        host = "whiterun"
        backend = "herdr"

        [[place.route]]
        kind = "mdns"
        "#,
        );
        assert_eq!(
            decode(&text).unwrap_err(),
            CatalogError::MissingField("route host".into(), 3)
        );
    }

    #[test]
    fn unknown_route_kind_is_rejected() {
        let text = swift(
            r#"
        version = 2

        [[place]]
        id = "11111111-1111-1111-1111-111111111111"
        user = "ada"
        host = "whiterun"
        backend = "herdr"

        [[place.route]]
        kind = "carrier-pigeon"
        host = "whiterun.local"
        "#,
        );
        assert_eq!(
            decode(&text).unwrap_err(),
            CatalogError::InvalidValue("route kind".into(), 10)
        );
    }

    #[test]
    fn rejects_unknown_version() {
        assert_eq!(
            decode("version = 3\n").unwrap_err(),
            CatalogError::UnsupportedVersion(3)
        );
    }
}
