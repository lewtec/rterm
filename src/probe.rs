use std::path::{Path, PathBuf};
use std::time::Duration;

use uuid::Uuid;

use crate::driver;
use crate::place::Place;
use crate::proc::run_bounded;
use crate::route::{self, Route};

/// How long one route may take before the assistant moves on.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(4);
const KEYGEN_TIMEOUT: Duration = Duration::from_secs(2);
const CONNECT_TIMEOUT_SECONDS: u64 = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Key {
    pub kind: String,
    pub fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probe {
    pub keys: Vec<Key>,
    /// known_hosts content written by the probe, keyed by the node alias.
    pub known_hosts: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attempt {
    pub route: Route,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChooseError {
    NoRoute {
        host: String,
        attempts: Vec<Attempt>,
    },
    HostKeyChanged {
        host: String,
        expected: String,
        found: Vec<Attempt>,
    },
}

impl std::fmt::Display for ChooseError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoRoute { host, attempts } => {
                write!(formatter, "no route to {host}")?;
                write_attempts(formatter, attempts)
            }
            Self::HostKeyChanged {
                host,
                expected,
                found,
            } => {
                write!(
                    formatter,
                    "{host} presented a different host key (pinned {expected})"
                )?;
                write_attempts(formatter, found)
            }
        }
    }
}

fn write_attempts(
    formatter: &mut std::fmt::Formatter<'_>,
    attempts: &[Attempt],
) -> std::fmt::Result {
    for (index, attempt) in attempts.iter().take(4).enumerate() {
        let separator = if index == 0 { ": " } else { "; " };
        write!(
            formatter,
            "{separator}{} ({})",
            attempt.route.display(),
            attempt.reason
        )?;
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub route: Route,
    /// Set when the place had no pin and the probe taught one.
    pub learned: Option<String>,
    pub known_hosts: String,
}

pub trait Prober {
    fn probe(&self, place: &Place, route: &Route) -> Result<Probe, String>;
}

/// Probe with the same `ssh` binary and the same `~/.ssh/config` path that the
/// attach will use. Host key exchange happens before auth, so a route counts as
/// reachable even when the server refuses the `none` method.
pub struct SshProber;

impl Prober for SshProber {
    fn probe(&self, place: &Place, route: &Route) -> Result<Probe, String> {
        let known_hosts = probe_path();
        let arguments = probe_arguments(place, route, &known_hosts);
        let output = run_bounded(&driver::ssh_program(), &arguments, PROBE_TIMEOUT)
            .ok_or_else(|| "timed out".to_string())?;
        let keys = fingerprints(&known_hosts).unwrap_or_default();
        let content = std::fs::read_to_string(&known_hosts).unwrap_or_default();
        let _ = std::fs::remove_file(&known_hosts);
        if keys.is_empty() {
            return Err(classify(&output));
        }
        Ok(Probe {
            keys,
            known_hosts: content,
        })
    }
}

pub fn choose(place: &Place) -> Result<Choice, ChooseError> {
    choose_with(place, &SshProber)
}

/// Attach path: try the route that worked last and nothing else. When the catalog
/// has no memory yet, fall back to discovery so old places still connect.
pub fn choose_preferred(place: &Place) -> Result<Choice, ChooseError> {
    choose_preferred_with(place, &SshProber)
}

fn choose_preferred_with(place: &Place, prober: &dyn Prober) -> Result<Choice, ChooseError> {
    match route::last_route(place) {
        Some(route) => choose_route_with(place, route, prober),
        None => choose_with(place, prober),
    }
}

/// Probe exactly one route, the one the user picked from the route list.
pub fn choose_route(place: &Place, route: &Route) -> Result<Choice, ChooseError> {
    choose_route_with(place, route, &SshProber)
}

fn choose_route_with(
    place: &Place,
    route: &Route,
    prober: &dyn Prober,
) -> Result<Choice, ChooseError> {
    let pin = pin_of(place);
    let failed = |reason: String| ChooseError::NoRoute {
        host: place.host.clone(),
        attempts: vec![Attempt {
            route: route.clone(),
            reason,
        }],
    };
    match prober.probe(place, route) {
        Err(reason) => Err(failed(reason)),
        Ok(probe) if probe.keys.is_empty() => Err(failed("no host key".into())),
        Ok(probe) => {
            let found = preferred(&probe.keys).unwrap_or_default();
            match pin.as_ref() {
                Some(expected) if !probe.keys.iter().any(|key| &key.fingerprint == expected) => {
                    Err(ChooseError::HostKeyChanged {
                        host: place.host.clone(),
                        expected: expected.clone(),
                        found: vec![Attempt {
                            route: route.clone(),
                            reason: found,
                        }],
                    })
                }
                Some(_) => Ok(Choice {
                    route: route.clone(),
                    learned: None,
                    known_hosts: probe.known_hosts,
                }),
                None => Ok(Choice {
                    route: route.clone(),
                    learned: Some(found),
                    known_hosts: probe.known_hosts,
                }),
            }
        }
    }
}

pub fn choose_with(place: &Place, prober: &dyn Prober) -> Result<Choice, ChooseError> {
    let pin = pin_of(place);
    let mut attempts = Vec::new();
    let mut mismatches = Vec::new();
    for route in route::candidates(place) {
        match prober.probe(place, &route) {
            Err(reason) => attempts.push(Attempt { route, reason }),
            Ok(probe) if probe.keys.is_empty() => attempts.push(Attempt {
                route,
                reason: "no host key".into(),
            }),
            Ok(probe) => {
                let found = preferred(&probe.keys).unwrap_or_default();
                match pin.as_ref() {
                    Some(expected)
                        if !probe.keys.iter().any(|key| &key.fingerprint == expected) =>
                    {
                        mismatches.push(Attempt {
                            route,
                            reason: found,
                        });
                    }
                    Some(_) => {
                        return Ok(Choice {
                            route,
                            learned: None,
                            known_hosts: probe.known_hosts,
                        });
                    }
                    None => {
                        return Ok(Choice {
                            route,
                            learned: Some(found),
                            known_hosts: probe.known_hosts,
                        });
                    }
                }
            }
        }
    }
    let host = place.host.clone();
    if !mismatches.is_empty() {
        Err(ChooseError::HostKeyChanged {
            host,
            expected: pin.unwrap_or_default(),
            found: mismatches,
        })
    } else {
        Err(ChooseError::NoRoute { host, attempts })
    }
}

pub fn pin_of(place: &Place) -> Option<String> {
    place
        .fingerprint
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// First attach uses the probe output as its trusted known_hosts. `ssh` then
/// runs with `StrictHostKeyChecking=yes` for that file, so a route that changes
/// identity between probe and attach still fails.
pub fn install_known_hosts(place: &Place, choice: &Choice) -> std::io::Result<PathBuf> {
    let path = driver::known_hosts_path(place);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut tmp = path.clone().into_os_string();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, &choice.known_hosts)?;
    std::fs::rename(&tmp, &path)?;
    Ok(path)
}

pub fn probe_arguments(place: &Place, route: &Route, known_hosts: &Path) -> Vec<String> {
    let alias = driver::host_key_alias(place);
    let mut arguments = vec![
        "-o".into(),
        "BatchMode=yes".into(),
        "-o".into(),
        format!("ConnectTimeout={CONNECT_TIMEOUT_SECONDS}"),
        "-o".into(),
        "ConnectionAttempts=1".into(),
        "-o".into(),
        "PreferredAuthentications=none".into(),
        "-o".into(),
        "StrictHostKeyChecking=accept-new".into(),
        "-o".into(),
        "GlobalKnownHostsFile=/dev/null".into(),
        "-o".into(),
        format!("UserKnownHostsFile={}", known_hosts.display()),
        "-o".into(),
        format!("HostKeyAlias={alias}"),
    ];
    arguments.extend(driver::route_options(place, route));
    arguments.push(driver::ssh_target(place));
    arguments.push("true".into());
    arguments
}

/// Discover every route and report each result. The first verified route pins
/// the fingerprint for the rest of the run, so a second machine that answers to
/// the same name is reported instead of merged.
pub fn discover_with(place: &Place, prober: &dyn Prober, report: &mut dyn FnMut(Discovered)) {
    let mut pin = pin_of(place);
    for route in route::candidates(place) {
        match prober.probe(place, &route) {
            Err(reason) => report(Discovered::Unreachable { route, reason }),
            Ok(probe) if probe.keys.is_empty() => report(Discovered::Unreachable {
                route,
                reason: "no host key".into(),
            }),
            Ok(probe) => {
                let found = preferred(&probe.keys).unwrap_or_default();
                if let Some(expected) = pin.as_ref() {
                    if probe.keys.iter().any(|key| &key.fingerprint == expected) {
                        report(Discovered::Verified {
                            route,
                            fingerprint: found,
                            learned: false,
                        });
                    } else {
                        report(Discovered::HostKeyMismatch {
                            route,
                            expected: expected.clone(),
                            found,
                        });
                    }
                } else {
                    pin = Some(found.clone());
                    report(Discovered::Verified {
                        route,
                        fingerprint: found,
                        learned: true,
                    });
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Discovered {
    Verified {
        route: Route,
        fingerprint: String,
        learned: bool,
    },
    HostKeyMismatch {
        route: Route,
        expected: String,
        found: String,
    },
    Unreachable {
        route: Route,
        reason: String,
    },
}

impl Discovered {
    pub fn route(&self) -> &Route {
        match self {
            Self::Verified { route, .. }
            | Self::HostKeyMismatch { route, .. }
            | Self::Unreachable { route, .. } => route,
        }
    }
}

pub fn preferred(keys: &[Key]) -> Option<String> {
    let mut best: Option<(&Key, u8)> = None;
    for key in keys {
        let rank = if key.kind.starts_with("ED25519") {
            0
        } else if key.kind.starts_with("ECDSA") {
            1
        } else if key.kind.starts_with("RSA") {
            2
        } else {
            3
        };
        if best.is_none_or(|(_, current)| rank < current) {
            best = Some((key, rank));
        }
    }
    best.map(|(key, _)| key.fingerprint.clone())
}

fn fingerprints(path: &Path) -> Option<Vec<Key>> {
    if !path.is_file() {
        return None;
    }
    let output = run_bounded(
        "ssh-keygen",
        &[
            "-lf".into(),
            path.to_string_lossy().into_owned(),
            "-E".into(),
            "sha256".into(),
        ],
        KEYGEN_TIMEOUT,
    )?;
    if !output.status.success() {
        return None;
    }
    Some(parse_keygen(&String::from_utf8_lossy(&output.stdout)))
}

pub fn parse_keygen(text: &str) -> Vec<Key> {
    let mut keys = Vec::new();
    for line in text.lines() {
        let mut fields = line.split_whitespace();
        let _bits = fields.next();
        let Some(fingerprint) = fields.next() else {
            continue;
        };
        if !fingerprint.starts_with("SHA256:") {
            continue;
        }
        let kind = line
            .rsplit_once('(')
            .and_then(|(_, tail)| tail.strip_suffix(')'))
            .unwrap_or("")
            .to_string();
        keys.push(Key {
            kind,
            fingerprint: fingerprint.to_string(),
        });
    }
    keys
}

fn classify(output: &std::process::Output) -> String {
    let text = String::from_utf8_lossy(&output.stderr);
    let last = text
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("no answer");
    let lower = last.to_ascii_lowercase();
    if lower.contains("could not resolve hostname") {
        "name not found".into()
    } else if lower.contains("connection refused") {
        "connection refused".into()
    } else if lower.contains("timed out") || lower.contains("timeout") {
        "timed out".into()
    } else if lower.contains("no route to host") {
        "no route".into()
    } else if lower.contains("network is unreachable") {
        "network unreachable".into()
    } else if lower.contains("host key verification failed")
        || lower.contains("identification has changed")
    {
        "host key changed".into()
    } else {
        last.trim_start_matches("ssh: ").to_string()
    }
}

fn probe_path() -> PathBuf {
    std::env::temp_dir().join(format!("rterm-probe-{}.known_hosts", Uuid::new_v4()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::Backend;
    use std::collections::HashMap;

    fn place(host: &str, fingerprint: Option<&str>) -> Place {
        let mut place = Place::new("ada", host, Backend::Herdr, None);
        place.fingerprint = fingerprint.map(str::to_string);
        place
    }

    struct Fake {
        answers: HashMap<String, Result<(&'static str, &'static str), String>>,
    }

    impl Prober for Fake {
        fn probe(&self, _place: &Place, route: &Route) -> Result<Probe, String> {
            match self.answers.get(&route.host) {
                Some(Ok((kind, fingerprint))) => Ok(Probe {
                    keys: vec![Key {
                        kind: (*kind).into(),
                        fingerprint: (*fingerprint).into(),
                    }],
                    known_hosts: format!("rterm-x ssh-ed25519 AAAA {fingerprint}\n"),
                }),
                Some(Err(reason)) => Err(reason.clone()),
                None => Err("unknown".into()),
            }
        }
    }

    fn fake() -> Fake {
        Fake {
            answers: HashMap::from([
                ("riverwood".into(), Err("timed out".into())),
                ("riften".into(), Err("timed out".into())),
                ("riverwood.local".into(), Ok(("ED25519", "SHA256:SAME"))),
                (
                    "riverwood.tailnet.ts.net".into(),
                    Ok(("ED25519", "SHA256:SAME")),
                ),
            ]),
        }
    }

    /// Answers like `fake`, and records which routes were asked in what order.
    struct Recording {
        calls: std::cell::RefCell<Vec<String>>,
    }

    impl Recording {
        fn new() -> Self {
            Self {
                calls: std::cell::RefCell::new(Vec::new()),
            }
        }

        fn calls(&self) -> Vec<String> {
            self.calls.borrow().clone()
        }
    }

    impl Prober for Recording {
        fn probe(&self, _place: &Place, route: &Route) -> Result<Probe, String> {
            self.calls.borrow_mut().push(route.host.clone());
            match route.host.as_str() {
                "riverwood.local" | "riverwood.tailnet.ts.net" => Ok(Probe {
                    keys: vec![Key {
                        kind: "ED25519".into(),
                        fingerprint: "SHA256:SAME".into(),
                    }],
                    known_hosts: "rterm-x ssh-ed25519 AAAA SHA256:SAME\n".into(),
                }),
                _ => Err("timed out".into()),
            }
        }
    }

    #[test]
    fn picks_the_first_verified_route() {
        let place = place("riverwood", None);
        let choice = choose_with(&place, &fake()).unwrap();
        assert_eq!(choice.route.host, "riverwood.local");
        assert_eq!(choice.learned.as_deref(), Some("SHA256:SAME"));
    }

    #[test]
    fn pinned_fingerprint_rejects_other_nodes() {
        let place = place("riverwood", Some("SHA256:OTHER"));
        let error = choose_with(&place, &fake()).unwrap_err();
        match error {
            ChooseError::HostKeyChanged { found, .. } => {
                assert!(!found.is_empty());
                assert_eq!(found[0].route.host, "riverwood.local");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn pinned_fingerprint_accepts_matching_route() {
        let place = place("riverwood", Some("SHA256:SAME"));
        let choice = choose_with(&place, &fake()).unwrap();
        assert_eq!(choice.route.host, "riverwood.local");
        assert_eq!(choice.learned, None);
    }

    #[test]
    fn no_route_lists_attempts() {
        let place = place("riften", None);
        let error = choose_with(&place, &fake()).unwrap_err();
        let text = error.to_string();
        assert!(text.contains("no route to riften"));
        assert!(text.contains("riften (timed out)"));
    }

    #[test]
    fn preferred_route_is_the_only_one_probed() {
        let mut place = place("riverwood", Some("SHA256:SAME"));
        place.routes = vec![
            Route {
                last: true,
                ..Route::mdns("riverwood.local")
            },
            Route::tailscale("riverwood.tailnet.ts.net"),
        ];
        let prober = Recording::new();
        let choice = choose_preferred_with(&place, &prober).unwrap();
        assert_eq!(choice.route.host, "riverwood.local");
        assert_eq!(prober.calls(), vec!["riverwood.local".to_string()]);
    }

    #[test]
    fn no_memory_falls_back_to_discovery() {
        let place = place("riverwood", None);
        let prober = Recording::new();
        let choice = choose_preferred_with(&place, &prober).unwrap();
        assert_eq!(choice.route.host, "riverwood.local");
        assert!(prober.calls().len() > 1);
    }

    #[test]
    fn chosen_route_that_mismatches_is_reported_alone() {
        let place = place("riverwood", Some("SHA256:OTHER"));
        let prober = Recording::new();
        let route = Route::mdns("riverwood.local");
        let error = choose_route_with(&place, &route, &prober).unwrap_err();
        match error {
            ChooseError::HostKeyChanged { found, .. } => {
                assert_eq!(found.len(), 1);
                assert_eq!(found[0].route.host, "riverwood.local");
            }
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(prober.calls(), vec!["riverwood.local".to_string()]);
    }

    #[test]
    fn parse_keygen_reads_type_and_fingerprint() {
        let keys = parse_keygen(
            "256 SHA256:C8olMcLxERkizT4dlzO1z85UVxdq/COQVR0T3I9IEK8 root@whiterun (ED25519)\n\
             3072 SHA256:AAA root@whiterun (RSA)\n",
        );
        assert_eq!(
            keys,
            vec![
                Key {
                    kind: "ED25519".into(),
                    fingerprint: "SHA256:C8olMcLxERkizT4dlzO1z85UVxdq/COQVR0T3I9IEK8".into(),
                },
                Key {
                    kind: "RSA".into(),
                    fingerprint: "SHA256:AAA".into(),
                },
            ]
        );
    }

    #[test]
    fn preferred_key_is_ed25519() {
        let keys = vec![
            Key {
                kind: "RSA".into(),
                fingerprint: "SHA256:RSA".into(),
            },
            Key {
                kind: "ED25519".into(),
                fingerprint: "SHA256:ED".into(),
            },
        ];
        assert_eq!(preferred(&keys).as_deref(), Some("SHA256:ED"));
    }

    #[test]
    fn probe_arguments_target_the_node_and_override_host_name() {
        let place = place("riverwood", None);
        let route = Route::direct("192.168.1.7");
        let arguments = probe_arguments(&place, &route, Path::new("/tmp/kh"));
        assert!(
            arguments
                .iter()
                .any(|argument| argument.starts_with("HostKeyAlias=rterm-"))
        );
        assert!(arguments.contains(&"HostName=192.168.1.7".to_string()));
        assert!(arguments.contains(&"PreferredAuthentications=none".to_string()));
        assert_eq!(arguments.last().map(String::as_str), Some("true"));
        assert_eq!(
            arguments.iter().rev().nth(1).map(String::as_str),
            Some("ada@riverwood")
        );
    }
}
