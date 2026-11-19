use std::time::Duration;

use crate::place::Place;
use crate::proc::run_bounded;

pub const TAILSCALE_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_CANDIDATES: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RouteKind {
    Direct,
    Mdns,
    Tailscale,
}

impl RouteKind {
    pub const ALL: [RouteKind; 3] = [RouteKind::Direct, RouteKind::Mdns, RouteKind::Tailscale];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Mdns => "mdns",
            Self::Tailscale => "tailscale",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Mdns => "mDNS",
            Self::Tailscale => "Tailscale",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "direct" => Some(Self::Direct),
            "mdns" => Some(Self::Mdns),
            "tailscale" => Some(Self::Tailscale),
            _ => None,
        }
    }
}

/// One way to reach a node. `host` is what SSH resolves; `port` is optional so
/// `~/.ssh/config` keeps owning the default. `last` marks the route that worked
/// most recently, so a reconnect tries it before asking about the others.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Route {
    pub kind: RouteKind,
    pub host: String,
    pub port: Option<u16>,
    pub last: bool,
}

impl Route {
    pub fn new(kind: RouteKind, host: impl Into<String>) -> Self {
        Self {
            kind,
            host: host.into(),
            port: None,
            last: false,
        }
    }

    pub fn direct(host: impl Into<String>) -> Self {
        Self::new(RouteKind::Direct, host)
    }

    pub fn mdns(host: impl Into<String>) -> Self {
        Self::new(RouteKind::Mdns, host)
    }

    pub fn tailscale(host: impl Into<String>) -> Self {
        Self::new(RouteKind::Tailscale, host)
    }

    pub fn display(&self) -> String {
        match self.port {
            Some(port) => format!("{}:{port}", self.host),
            None => self.host.clone(),
        }
    }

    pub fn normalized(&self) -> Self {
        Self {
            kind: self.kind,
            host: trim_host(&self.host),
            port: self.port,
            last: self.last,
        }
    }
}

/// The route that worked most recently, if the catalog remembers one.
pub fn last_route(place: &Place) -> Option<&Route> {
    place.routes.iter().find(|route| route.last)
}

/// The endpoint the place is using: the one marked last, or the first one the
/// attach would try when nothing worked yet.
pub fn current(place: &Place) -> Option<Route> {
    last_route(place)
        .cloned()
        .or_else(|| candidates(place).into_iter().next())
}

/// Mark `route` as the last one that worked and drop the mark from the rest.
/// The route is added to the catalog the first time it is used.
pub fn remember(routes: &mut Vec<Route>, route: Route) {
    let mut route = route.normalized();
    route.last = true;
    merge(routes, [route]);
}

/// Fold routes into a list without losing the last-used mark: a route that is
/// already there keeps its place, and an incoming `last` wins over the old one.
pub fn merge(routes: &mut Vec<Route>, incoming: impl IntoIterator<Item = Route>) {
    for route in incoming {
        let route = route.normalized();
        if route.host.is_empty() {
            continue;
        }
        if route.last {
            for existing in routes.iter_mut() {
                existing.last = false;
            }
        }
        if let Some(existing) = routes
            .iter_mut()
            .find(|existing| same_route(existing, &route))
        {
            existing.last |= route.last;
            continue;
        }
        routes.push(route);
    }
}

/// Every route rterm should try for a place, best first:
///
/// 1. the route that worked last
/// 2. routes saved in the catalog, in the order the user set
/// 3. the place host itself (plain DNS or an IP)
/// 4. `<short>.local` through mDNS
/// 5. Tailscale peers that answer to the node name (MagicDNS name, then IPs)
pub fn candidates(place: &Place) -> Vec<Route> {
    let mut out = Vec::new();
    if place.is_local() || place.host.trim().is_empty() {
        return out;
    }
    for route in place.routes.iter().filter(|route| route.last) {
        push_unique(&mut out, route.clone());
    }
    for route in &place.routes {
        push_unique(&mut out, route.clone());
    }
    push_unique(&mut out, Route::direct(place.host.clone()));
    if let Some(name) = mdns_name(&place.host) {
        push_unique(&mut out, Route::mdns(name));
    }
    for route in tailscale_routes(place) {
        push_unique(&mut out, route);
    }
    out.truncate(MAX_CANDIDATES);
    out
}

pub fn push_unique(routes: &mut Vec<Route>, route: Route) {
    let route = route.normalized();
    if route.host.is_empty() {
        return;
    }
    if routes.iter().any(|existing| same_route(existing, &route)) {
        return;
    }
    routes.push(route);
}

pub fn same_route(left: &Route, right: &Route) -> bool {
    left.kind == right.kind
        && left.host.eq_ignore_ascii_case(&right.host)
        && left.port == right.port
}

/// `whiterun` becomes `whiterun.local`. A FQDN uses its first label; addresses
/// are not mDNS names. An SSH address with `@` is a ShellHub-style target, not
/// a LAN host, so it never becomes a `.local` guess.
pub fn mdns_name(host: &str) -> Option<String> {
    let host = trim_host(host);
    if host.is_empty() || host.contains('@') || host.parse::<std::net::IpAddr>().is_ok() {
        return None;
    }
    let short = host.split('.').next().unwrap_or(&host);
    if short.is_empty() {
        return None;
    }
    if host.eq_ignore_ascii_case(&format!("{short}.local")) {
        return None;
    }
    Some(format!("{short}.local"))
}

fn trim_host(host: &str) -> String {
    host.trim().trim_end_matches('.').to_ascii_lowercase()
}

fn tailscale_routes(place: &Place) -> Vec<Route> {
    let name = trim_host(&place.host);
    if name.is_empty() {
        return Vec::new();
    }
    let Some(text) = tailscale_status() else {
        return Vec::new();
    };
    routes_for_peer(&text, &name)
}

fn tailscale_status() -> Option<String> {
    let program = std::env::var("RTERM_TAILSCALE").unwrap_or_else(|_| "tailscale".into());
    let output = run_bounded(
        &program,
        &["status".into(), "--json".into()],
        TAILSCALE_TIMEOUT,
    )?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Peer {
    host_name: String,
    dns_name: String,
    ips: Vec<String>,
    online: bool,
}

/// Parse `tailscale status --json` and return routes for the peer whose
/// hostname, MagicDNS name, or IP answers to `name`.
pub fn routes_for_peer(json: &str, name: &str) -> Vec<Route> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
        return Vec::new();
    };
    let mut peers = Vec::new();
    if let Some(own) = value.get("Self").and_then(peer_from) {
        peers.push(own);
    }
    if let Some(map) = value.get("Peer").and_then(|value| value.as_object()) {
        for peer in map.values().filter_map(peer_from) {
            peers.push(peer);
        }
    }
    let wanted = trim_host(name);
    let mut matched: Vec<Peer> = peers
        .into_iter()
        .filter(|peer| peer_matches(peer, &wanted))
        .collect();
    matched.sort_by_key(|peer| !peer.online);
    let mut routes = Vec::new();
    for peer in matched {
        let dns = trim_host(&peer.dns_name);
        if !dns.is_empty() {
            push_unique(&mut routes, Route::tailscale(dns));
        }
        for ip in peer.ips {
            push_unique(&mut routes, Route::tailscale(ip));
        }
    }
    routes
}

fn peer_from(value: &serde_json::Value) -> Option<Peer> {
    let object = value.as_object()?;
    let text = |key: &str| {
        object
            .get(key)
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .to_string()
    };
    let ips = object
        .get("TailscaleIPs")
        .and_then(|value| value.as_array())
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    Some(Peer {
        host_name: text("HostName"),
        dns_name: text("DNSName"),
        ips,
        online: object
            .get("Online")
            .and_then(|value| value.as_bool())
            .unwrap_or(false),
    })
}

fn peer_matches(peer: &Peer, wanted: &str) -> bool {
    if peer.host_name.eq_ignore_ascii_case(wanted) {
        return true;
    }
    let dns = trim_host(&peer.dns_name);
    if dns == wanted {
        return true;
    }
    let short = dns.split('.').next().unwrap_or(&dns);
    if short.eq_ignore_ascii_case(wanted) {
        return true;
    }
    peer.ips.iter().any(|ip| ip.eq_ignore_ascii_case(wanted))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::Backend;

    fn place(host: &str) -> Place {
        Place::new("ada", host, Backend::Herdr, None)
    }

    #[test]
    fn kinds_round_trip() {
        for kind in RouteKind::ALL {
            assert_eq!(RouteKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(RouteKind::parse("nope"), None);
    }

    #[test]
    fn candidates_keep_saved_order_first() {
        let mut place = place("riverwood");
        place.routes = vec![Route::direct("100.64.0.7"), Route::mdns("riverwood.local")];
        let routes = candidates(&place);
        assert_eq!(routes[0], Route::direct("100.64.0.7"));
        assert_eq!(routes[1], Route::mdns("riverwood.local"));
        assert_eq!(routes[2], Route::direct("riverwood"));
    }

    #[test]
    fn candidates_try_the_last_route_first() {
        let mut place = place("riverwood");
        place.routes = vec![
            Route::direct("100.64.0.7"),
            Route {
                last: true,
                ..Route::mdns("riverwood.local")
            },
        ];
        let routes = candidates(&place);
        assert_eq!(routes[0].host, "riverwood.local");
        assert_eq!(routes[1].host, "100.64.0.7");
    }

    #[test]
    fn current_is_the_last_route_or_the_first_candidate() {
        let mut place = place("riverwood");
        assert_eq!(current(&place), Some(Route::direct("riverwood")));
        place.routes = vec![
            Route::direct("100.64.0.7"),
            Route {
                last: true,
                ..Route::mdns("riverwood.local")
            },
        ];
        let in_use = current(&place).unwrap();
        assert_eq!(in_use.host, "riverwood.local");
        assert!(in_use.last);
    }

    #[test]
    fn remember_keeps_the_mark_on_one_route() {
        let mut routes = vec![Route::direct("a"), Route::direct("b")];
        remember(&mut routes, Route::direct("b"));
        assert!(!routes[0].last);
        assert!(routes[1].last);
        remember(&mut routes, Route::direct("c"));
        assert_eq!(routes.iter().filter(|route| route.last).count(), 1);
        assert!(routes[2].last);
    }

    #[test]
    fn merge_preserves_order_and_last() {
        let mut routes = vec![Route::direct("a")];
        merge(
            &mut routes,
            [
                Route {
                    last: true,
                    ..Route::direct("b")
                },
                Route::direct("a"),
            ],
        );
        assert_eq!(
            routes
                .iter()
                .map(|route| route.host.as_str())
                .collect::<Vec<_>>(),
            ["a", "b"]
        );
        assert!(routes[1].last);
    }

    #[test]
    fn candidates_are_deduped_and_short() {
        let mut place = place("riverwood");
        place.routes = vec![
            Route::mdns("RIVERWOOD.local."),
            Route::mdns("riverwood.local"),
        ];
        let routes = candidates(&place);
        assert_eq!(
            routes
                .iter()
                .filter(|route| route.kind == RouteKind::Mdns)
                .count(),
            1
        );
    }

    #[test]
    fn local_places_have_no_routes() {
        assert!(candidates(&place("")).is_empty());
        assert!(candidates(&place("localhost")).is_empty());
    }

    #[test]
    fn mdns_name_uses_first_label() {
        assert_eq!(mdns_name("whiterun").as_deref(), Some("whiterun.local"));
        assert_eq!(
            mdns_name("whiterun.home.arpa").as_deref(),
            Some("whiterun.local")
        );
        assert_eq!(mdns_name("riverwood.local"), None);
        assert_eq!(mdns_name("192.168.1.7"), None);
        assert_eq!(mdns_name("fd7a::1"), None);
        assert_eq!(mdns_name("lucao.whiterun@cloud.shellhub.io"), None);
    }

    #[test]
    fn tailscale_matches_hostname_and_magic_dns() {
        let json = r#"{
            "Self": {
                "HostName": "laptop",
                "DNSName": "laptop.tailnet.ts.net.",
                "TailscaleIPs": ["100.64.0.1"],
                "Online": true
            },
            "Peer": {
                "one": {
                    "HostName": "whiterun",
                    "DNSName": "whiterun.tailnet.ts.net.",
                    "TailscaleIPs": ["100.64.0.7", "fd7a::7"],
                    "Online": false
                },
                "two": {
                    "HostName": "riften",
                    "DNSName": "riften.tailnet.ts.net.",
                    "TailscaleIPs": ["100.64.0.9"],
                    "Online": true
                }
            }
        }"#;
        let routes = routes_for_peer(json, "whiterun");
        assert_eq!(
            routes,
            vec![
                Route::tailscale("whiterun.tailnet.ts.net"),
                Route::tailscale("100.64.0.7"),
                Route::tailscale("fd7a::7"),
            ]
        );
        assert!(
            routes_for_peer(json, "100.64.0.9")[0]
                .host
                .contains("riften")
        );
        assert!(routes_for_peer(json, "nobody").is_empty());
        assert!(routes_for_peer("not json", "whiterun").is_empty());
    }

    #[test]
    fn tailscale_online_peers_come_first() {
        let json = r#"{"Peer": {
            "a": {"HostName": "whiterun", "DNSName": "whiterun.off.ts.net.", "TailscaleIPs": ["100.0.0.1"], "Online": false},
            "b": {"HostName": "whiterun", "DNSName": "whiterun.on.ts.net.", "TailscaleIPs": ["100.0.0.2"], "Online": true}
        }}"#;
        let routes = routes_for_peer(json, "whiterun");
        assert_eq!(routes[0].host, "whiterun.on.ts.net");
    }
}
