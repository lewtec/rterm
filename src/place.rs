use std::sync::OnceLock;

use uuid::Uuid;

use crate::backend::Backend;
use crate::paths::login_name;
use crate::route::Route;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Place {
    pub id: Uuid,
    pub user: String,
    /// The first endpoint the user typed: the tab label and the default
    /// address. Not the node identity — `fingerprint` is.
    pub host: String,
    pub backend: Backend,
    pub session: Option<String>,
    pub label: Option<String>,
    /// More endpoints that reach the same node. `host` is tried as well.
    pub routes: Vec<Route>,
    /// Pinned SSH host key fingerprint (`SHA256:...`). This is the node
    /// identity. Empty means learn on the first successful attach.
    pub fingerprint: Option<String>,
}

impl Place {
    pub fn new(
        user: impl Into<String>,
        host: impl Into<String>,
        backend: Backend,
        session: Option<String>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            user: user.into(),
            host: host.into(),
            backend,
            session,
            label: None,
            routes: Vec::new(),
            fingerprint: None,
        }
    }

    pub fn is_local(&self) -> bool {
        Self::is_loopback_host(&self.host) || is_this_machine(&self.host)
    }

    /// A pinned place has learned (or been given) the node's host key.
    pub fn is_pinned(&self) -> bool {
        self.fingerprint
            .as_deref()
            .is_some_and(|pin| !pin.is_empty())
    }

    /// The access this tab opens. Tabs of the same node differ here: Herdr has
    /// no session, tmux and screen name one.
    pub fn same_attachment(&self, other: &Place) -> bool {
        self.backend == other.backend && self.session == other.session
    }

    pub fn is_loopback_host(host: &str) -> bool {
        let value = normalize_host(host);
        if is_ipv4_loopback(&value) {
            return true;
        }
        matches!(
            value.as_str(),
            "" | "localhost" | "localhost.localdomain" | "0.0.0.0" | "::1"
        ) || value.ends_with(".localhost")
    }

    pub fn switch_key(&self) -> String {
        let shown_host = if self.is_local() {
            "localhost"
        } else {
            self.host.as_str()
        };
        let login;
        let shown_user = if self.user.is_empty() && self.is_local() {
            login = login_name();
            login.as_str()
        } else {
            self.user.as_str()
        };
        let core = if shown_user.is_empty() {
            shown_host.to_string()
        } else {
            format!("{shown_user}@{shown_host}")
        };
        match self.backend {
            Backend::Herdr => format!("{core}:herdr"),
            Backend::Tmux => format!("{core}:tmux({})", self.session.as_deref().unwrap_or("")),
            Backend::Screen => {
                format!("{core}:screen({})", self.session.as_deref().unwrap_or(""))
            }
        }
    }

    pub fn display_label(&self) -> String {
        if let Some(label) = self.label.as_ref().filter(|label| !label.is_empty()) {
            return label.clone();
        }
        self.switch_key()
    }

    /// Every address that reaches this place, normalized.
    pub fn endpoint_hosts(&self) -> Vec<String> {
        let mut hosts = Vec::new();
        let host = normalize_host(&self.host);
        if !host.is_empty() {
            hosts.push(host);
        }
        for route in &self.routes {
            let host = normalize_host(&route.host);
            if !host.is_empty() && !hosts.contains(&host) {
                hosts.push(host);
            }
        }
        hosts
    }

    /// True when `address` is one of the ways to reach this place.
    pub fn points_at(&self, address: &str) -> bool {
        let address = normalize_host(address);
        !address.is_empty() && self.endpoint_hosts().iter().any(|host| host == &address)
    }

    /// `user@endpoint` names the login and the address in one field. Returns the
    /// login only when the text carries one; the address never keeps it.
    pub fn split_endpoint(address: &str) -> (Option<String>, String) {
        let address = address.trim();
        if let Some((user, host)) = address.rsplit_once('@') {
            let user = user.trim();
            let host = host.trim();
            if !user.is_empty() && !host.is_empty() && !user.contains('@') {
                return (Some(user.to_string()), host.to_string());
            }
        }
        (None, address.to_string())
    }

    pub fn matches_switch_key(&self, key: &str) -> bool {
        let needle = key.trim();
        if needle.is_empty() {
            return false;
        }
        if self.switch_key() == needle || self.display_label() == needle {
            return true;
        }
        self.switch_key().eq_ignore_ascii_case(needle)
            || self.display_label().eq_ignore_ascii_case(needle)
    }

    pub fn parse_switch_key(key: &str) -> Option<Self> {
        let mut text = key.trim().to_string();
        if text.is_empty() {
            return None;
        }
        let (backend, session) = if let Some(parsed) = parse_backend_suffix(&text) {
            text = parsed.head;
            (parsed.backend, parsed.session)
        } else {
            (Backend::Herdr, None)
        };
        if backend != Backend::Herdr
            && session
                .as_ref()
                .map(|value| value.is_empty())
                .unwrap_or(true)
        {
            return None;
        }
        let (user, host) = if let Some(at) = text.rfind('@') {
            let user = text[..at].trim().to_string();
            let host = text[at + 1..].trim().to_string();
            (user, host)
        } else {
            (String::new(), text.trim().to_string())
        };
        let mut resolved_user = user;
        let mut resolved_host = host;
        if Self::is_loopback_host(&resolved_host) {
            resolved_host.clear();
            if resolved_user.is_empty() {
                resolved_user = login_name();
            }
        } else if resolved_user.is_empty() || resolved_host.is_empty() {
            return None;
        }
        Some(Place {
            id: Uuid::new_v4(),
            user: resolved_user,
            host: resolved_host,
            backend,
            session: if backend == Backend::Herdr {
                None
            } else {
                session
            },
            label: None,
            routes: Vec::new(),
            fingerprint: None,
        })
    }
}

struct BackendSuffix {
    head: String,
    backend: Backend,
    session: Option<String>,
}

fn normalize_host(host: &str) -> String {
    let mut value = host.trim().to_string();
    if value.starts_with('[') && value.ends_with(']') && value.len() >= 2 {
        value = value[1..value.len() - 1].to_string();
    }
    value.trim_matches('.').to_ascii_lowercase()
}

fn is_ipv4_loopback(value: &str) -> bool {
    let mut parts = value.split('.');
    let Some(first) = parts.next() else {
        return false;
    };
    if first != "127" {
        return false;
    }
    let rest: Vec<_> = parts.collect();
    rest.len() == 3
        && rest.iter().all(|part| {
            !part.is_empty() && part.len() <= 3 && part.chars().all(|ch| ch.is_ascii_digit())
        })
}

fn is_this_machine(host: &str) -> bool {
    let value = normalize_host(host);
    if value.is_empty() {
        return false;
    }
    machine_names().iter().any(|name| name == &value)
}

fn machine_names() -> &'static Vec<String> {
    static NAMES: OnceLock<Vec<String>> = OnceLock::new();
    NAMES.get_or_init(|| {
        let mut names = Vec::new();
        if let Ok(text) = std::fs::read_to_string("/etc/hostname") {
            for line in text.lines() {
                remember_name(&mut names, line);
            }
        }
        if let Ok(output) = std::process::Command::new("hostname").output()
            && output.status.success()
            && let Ok(text) = String::from_utf8(output.stdout)
        {
            remember_name(&mut names, text.trim());
        }
        names
    })
}

fn remember_name(names: &mut Vec<String>, raw: &str) {
    let value = normalize_host(raw);
    if value.is_empty() || Place::is_loopback_host(&value) {
        return;
    }
    if !names.iter().any(|name| name == &value) {
        names.push(value.clone());
    }
    if let Some((short, _)) = value.split_once('.')
        && !short.is_empty()
        && !names.iter().any(|name| name == short)
    {
        names.push(short.to_string());
    }
}

fn parse_backend_suffix(text: &str) -> Option<BackendSuffix> {
    let colon = text.rfind(':')?;
    let head = text[..colon].to_string();
    let tail = &text[colon + 1..];
    if tail == Backend::Herdr.as_str() {
        return Some(BackendSuffix {
            head,
            backend: Backend::Herdr,
            session: None,
        });
    }
    for backend in [Backend::Tmux, Backend::Screen] {
        let prefix = format!("{}(", backend.as_str());
        if tail.starts_with(&prefix) && tail.ends_with(')') && tail.len() > prefix.len() {
            let inner = &tail[prefix.len()..tail.len() - 1];
            return Some(BackendSuffix {
                head,
                backend,
                session: Some(inner.to_string()),
            });
        }
        if tail == backend.as_str() {
            return Some(BackendSuffix {
                head,
                backend,
                session: None,
            });
        }
    }
    None
}

pub fn file_name(index: usize, place: &Place) -> String {
    let key = place.switch_key().replace([':', '/'], "-");
    format!("{index:02}-{key}.rterm-place")
}

pub const FILE_EXTENSION: &str = "rterm-place";
