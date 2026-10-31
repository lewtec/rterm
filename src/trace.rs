#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Progress(String),
    RevealTerminal,
}

pub fn event(raw_line: &str) -> Option<Event> {
    let line = normalize(raw_line);
    if line.is_empty() {
        return None;
    }
    if needs_terminal(&line) {
        return Some(Event::RevealTerminal);
    }
    headline(&line).map(Event::Progress)
}

pub fn crumb(raw_line: &str) -> Option<String> {
    let line = normalize(raw_line);
    if line.is_empty() { None } else { Some(line) }
}

fn needs_terminal(line: &str) -> bool {
    line.contains("Next authentication method: password")
        || line.contains("Next authentication method: keyboard-interactive")
        || line.contains("The authenticity of host")
        || line.starts_with("Enter passphrase")
}

fn headline(line: &str) -> Option<String> {
    if let Some(host) = suffix_after("Connecting to ", line) {
        return Some(format!("Connecting to {}", short_host(&host)));
    }
    if line.contains("Connection established.") {
        return Some("Connected".into());
    }
    if let Some(rest) = suffix_after("Authenticating to ", line) {
        return Some(format!("Authenticating to {}", short_host(&rest)));
    }
    if line.contains("Offering public key") {
        return Some("Offering key".into());
    }
    if line.contains("Server accepts key") {
        return Some("Key accepted".into());
    }
    if let Some(rest) = suffix_after("Authenticated to ", line) {
        return Some(format!("Authenticated to {}", short_host(&rest)));
    }
    if line.contains("Sending command:") {
        return Some("Waiting for remote".into());
    }
    if line.contains("Could not resolve hostname") {
        return Some(line.strip_prefix("ssh: ").unwrap_or(line).to_string());
    }
    if line.contains("Connection timed out") || line.contains("Connection refused") {
        return Some(line.to_string());
    }
    if line.contains("Permission denied") {
        return Some("Permission denied".into());
    }
    if line.contains("Host key verification failed") {
        return Some("Host key verification failed".into());
    }
    None
}

fn normalize(raw: &str) -> String {
    let line = raw.trim();
    if let Some(rest) = line.strip_prefix("debug1: ") {
        rest.to_string()
    } else if let Some(rest) = line.strip_prefix("debug2: ") {
        rest.to_string()
    } else {
        line.to_string()
    }
}

fn suffix_after(marker: &str, line: &str) -> Option<String> {
    let index = line.find(marker)?;
    let rest = line[index + marker.len()..].trim();
    if rest.is_empty() {
        None
    } else {
        Some(rest.to_string())
    }
}

fn short_host(raw: &str) -> String {
    let mut value = raw;
    if let Some(index) = value.find(" using ") {
        value = &value[..index];
    }
    if let Some(index) = value.find(" as ") {
        return value[..index].trim().to_string();
    }
    if let Some(index) = value.find('[') {
        value = &value[..index];
    }
    if let Some(index) = value.find(" port ") {
        value = &value[..index];
    }
    value
        .trim_matches(|c: char| c.is_whitespace() || c == '.' || c == '(')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sending_command_keeps_waiting_for_tty() {
        assert_eq!(
            event("debug1: Sending command: exec \"$SHELL\" -lc herdr"),
            Some(Event::Progress("Waiting for remote".into()))
        );
    }

    #[test]
    fn entering_interactive_session_is_not_ready() {
        assert_eq!(event("debug1: Entering interactive session."), None);
    }

    #[test]
    fn password_reveals_terminal() {
        assert_eq!(
            event("debug1: Next authentication method: password"),
            Some(Event::RevealTerminal)
        );
        assert_eq!(
            event("debug1: Next authentication method: keyboard-interactive"),
            Some(Event::RevealTerminal)
        );
    }

    #[test]
    fn host_key_prompt_reveals_terminal() {
        assert_eq!(
            event("The authenticity of host 'whiterun (10.0.0.2)' can't be established."),
            Some(Event::RevealTerminal)
        );
    }

    #[test]
    fn progress_headlines() {
        assert_eq!(
            event("debug1: Connecting to whiterun [10.0.0.2] port 22."),
            Some(Event::Progress("Connecting to whiterun".into()))
        );
        assert_eq!(
            event("debug1: Connection established."),
            Some(Event::Progress("Connected".into()))
        );
        assert_eq!(
            event("debug1: Authenticating to whiterun:22 as 'ada'"),
            Some(Event::Progress("Authenticating to whiterun:22".into()))
        );
        assert_eq!(
            event("Authenticated to whiterun ([10.0.0.2]:22) using \"publickey\"."),
            Some(Event::Progress("Authenticated to whiterun".into()))
        );
        assert_eq!(
            event(
                "ssh: Could not resolve hostname nosuch: nodename nor servname provided, or not known"
            ),
            Some(Event::Progress(
                "Could not resolve hostname nosuch: nodename nor servname provided, or not known"
                    .into()
            ))
        );
    }

    #[test]
    fn crumb_strips_debug_prefix() {
        assert_eq!(
            crumb("debug1: Offering public key: id_ed25519").as_deref(),
            Some("Offering public key: id_ed25519")
        );
        assert_eq!(crumb("   "), None);
    }
}
