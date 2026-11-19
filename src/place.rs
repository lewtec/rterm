use std::sync::OnceLock;

use uuid::Uuid;

use crate::backend::Backend;
use crate::paths::login_name;
use crate::route::Route;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Place {
    pub id: Uuid,
    pub user: String,
    pub backend: Backend,
    pub session: Option<String>,
    pub label: Option<String>,
    /// Every way to reach the node. The first route is the endpoint the user
    /// typed; the `last` flag marks the route in use. Empty means this machine.
    /// Routes are the truth: there is no separate endpoint field. An address is
    /// used verbatim and may itself contain `@` (ShellHub's `device@gateway`);
    /// the login is only `user`.
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
        let host = host.into();
        let mut routes = Vec::new();
        if !Self::is_local_host(&host) {
            routes.push(Route::direct(host.trim()));
        }
        Self {
            id: Uuid::new_v4(),
            user: user.into(),
            backend,
            session,
            label: None,
            routes,
            fingerprint: None,
        }
    }

    /// A host that means this machine, not an SSH target.
    pub fn is_local_host(host: &str) -> bool {
        host.trim().is_empty() || Self::is_loopback_host(host) || is_this_machine(host)
    }

    pub fn is_local(&self) -> bool {
        self.routes.is_empty()
    }

    /// The endpoint the user typed: the first route.
    pub fn endpoint(&self) -> Option<&Route> {
        self.routes.first()
    }

    pub fn endpoint_host(&self) -> String {
        self.endpoint()
            .map(|route| route.host.trim().to_string())
            .unwrap_or_default()
    }

    /// The route in use: the last one that worked, else the endpoint.
    fn in_use_route(&self) -> Option<&Route> {
        self.routes
            .iter()
            .find(|route| route.last)
            .or_else(|| self.routes.first())
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

    /// The login this tab reports for the node. An empty user is the ssh
    /// default, which is the local login.
    pub fn login_key(&self) -> String {
        if self.user.trim().is_empty() {
            login_name()
        } else {
            self.user.trim().to_string()
        }
    }

    pub fn switch_key(&self) -> String {
        let endpoint;
        let shown_host = if self.is_local() {
            "localhost"
        } else {
            endpoint = self.endpoint_host();
            endpoint.as_str()
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

    /// The name shown for the node: the label when the user set one, else the
    /// hostname of the route in use. The endpoint is only one route, so it is
    /// not the tab's name.
    pub fn hostname(&self) -> String {
        if let Some(label) = self
            .label
            .as_deref()
            .map(str::trim)
            .filter(|label| !label.is_empty())
        {
            return label.to_string();
        }
        if self.is_local() {
            return "localhost".to_string();
        }
        self.in_use_route()
            .map(|route| route.host.trim().trim_end_matches('.').to_string())
            .unwrap_or_default()
    }

    pub fn display_label(&self) -> String {
        let name = self.hostname();
        let core = format!("{}@{name}", self.login_key());
        match self.backend {
            Backend::Herdr => format!("{core}:herdr"),
            Backend::Tmux => format!("{core}:tmux({})", self.session.as_deref().unwrap_or("")),
            Backend::Screen => {
                format!("{core}:screen({})", self.session.as_deref().unwrap_or(""))
            }
        }
    }

    /// Every address that reaches this place, normalized.
    pub fn endpoint_hosts(&self) -> Vec<String> {
        let mut hosts = Vec::new();
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
        // The login cannot contain `@`, but the host can: ShellHub wants
        // `login@namespace.device@gateway`. Split at the first `@` so the host
        // keeps its own `@`.
        let (user, host) = if let Some(at) = text.find('@') {
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
        } else if resolved_host.is_empty() {
            return None;
        }
        let session = if backend == Backend::Herdr {
            None
        } else {
            session
        };
        Some(Self::new(resolved_user, resolved_host, backend, session))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::route::Route;

    #[test]
    fn hostname_is_the_route_in_use_not_the_endpoint() {
        let mut place = Place::new("ada", "shellhub", Backend::Herdr, None);
        place.routes = vec![
            Route::direct("shellhub"),
            Route {
                last: true,
                ..Route::mdns("riverwood.local")
            },
        ];
        assert_eq!(place.hostname(), "riverwood.local");
        assert_eq!(place.display_label(), "ada@riverwood.local:herdr");
    }

    #[test]
    fn hostname_falls_back_to_the_endpoint() {
        let place = Place::new("ada", "riverwood", Backend::Tmux, Some("dev".into()));
        assert_eq!(place.hostname(), "riverwood");
        assert_eq!(place.display_label(), "ada@riverwood:tmux(dev)");
    }

    #[test]
    fn label_replaces_only_the_hostname() {
        let mut place = Place::new("ada", "riverwood", Backend::Herdr, None);
        place.label = Some("work".into());
        assert_eq!(place.display_label(), "ada@work:herdr");
        assert_eq!(place.switch_key(), "ada@riverwood:herdr");
    }

    #[test]
    fn an_empty_login_shows_the_ssh_login() {
        let place = Place::new("", "riverwood", Backend::Herdr, None);
        assert_eq!(
            place.display_label(),
            format!("{}@riverwood:herdr", login_name())
        );
    }

    #[test]
    fn shellhub_address_keeps_its_at_and_the_login() {
        let place = Place::new(
            "lucasew",
            "lucao.whiterun@cloud.shellhub.io",
            Backend::Herdr,
            None,
        );
        assert_eq!(place.hostname(), "lucao.whiterun@cloud.shellhub.io");
        assert_eq!(
            place.display_label(),
            "lucasew@lucao.whiterun@cloud.shellhub.io:herdr"
        );
        assert_eq!(
            place.switch_key(),
            "lucasew@lucao.whiterun@cloud.shellhub.io:herdr"
        );
        let parsed = Place::parse_switch_key(&place.switch_key()).unwrap();
        assert_eq!(parsed.user, "lucasew");
        assert_eq!(parsed.endpoint_host(), "lucao.whiterun@cloud.shellhub.io");
    }
}
