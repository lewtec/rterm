use std::path::{Path, PathBuf};

use uuid::Uuid;

use crate::paths::{self, caches_directory, home_dir};
use crate::place::Place;
use crate::uuidv7::uuid_upper;

pub const SSH_EXECUTABLE: &str = "/usr/bin/ssh";
pub const SERVER_ALIVE_INTERVAL: u32 = 5;
pub const SERVER_ALIVE_COUNT_MAX: u32 = 2;

/// Resolve the herdr binary that owns the live server socket, then fall back to PATH.
pub const HERDR_ATTACH_SCRIPT: &str = r#"sock="${HOME}/.config/herdr/herdr.sock"
if command -v herdr >/dev/null 2>&1; then
  js=$(herdr status server --json 2>/dev/null) || true
  parsed=$(printf %s "$js" | sed -n 's/.*"socket":"\([^"]*\)".*/\1/p')
  if [ -n "$parsed" ]; then sock=$parsed; fi
fi
pid=
if [ -S "$sock" ] && command -v lsof >/dev/null 2>&1; then
  pid=$(lsof -t -- "$sock" 2>/dev/null | head -n 1)
fi
if [ -z "$pid" ] && command -v pgrep >/dev/null 2>&1; then
  pid=$(pgrep -f "herdr server" 2>/dev/null | head -n 1)
fi
if [ -n "$pid" ] && [ -r "/proc/${pid}/exe" ]; then
  exe=$(readlink -f "/proc/${pid}/exe" 2>/dev/null || readlink "/proc/${pid}/exe")
  if [ -x "$exe" ]; then exec "$exe"; fi
fi
if [ -n "$pid" ]; then
  exe=$(ps -p "$pid" -o args= 2>/dev/null | awk "{print \$1}")
  if [ -x "$exe" ]; then exec "$exe"; fi
fi
exec herdr"#;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    pub executable: String,
    pub arguments: Vec<String>,
    pub current_directory: Option<PathBuf>,
}

pub fn local_shell() -> String {
    std::env::var("SHELL")
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "/bin/zsh".into())
}

/// Tests compare against `/usr/bin/ssh`. The GUI uses [`ssh_program`] when that path is absent.
pub fn ssh_program() -> String {
    if Path::new(SSH_EXECUTABLE).is_file() {
        SSH_EXECUTABLE.into()
    } else {
        "ssh".into()
    }
}

pub fn verbose_log_path(place_id: Uuid, generation: u32) -> PathBuf {
    paths::verbose_log_path(&uuid_upper(place_id), generation)
}

pub fn launch(place: &Place, verbose_log: Option<&str>) -> Launch {
    if place.is_local() {
        return Launch {
            executable: local_shell(),
            arguments: vec!["-lc".into(), remote_command(place)],
            current_directory: Some(home_dir()),
        };
    }
    prepare_control_socket_directory();
    Launch {
        executable: SSH_EXECUTABLE.into(),
        arguments: ssh_arguments(place, verbose_log),
        current_directory: None,
    }
}

pub fn control_path(place: &Place) -> String {
    caches_directory()
        .join(format!("cm-{}", uuid_upper(place.id)))
        .to_string_lossy()
        .into_owned()
}

pub fn control_master_options(place: &Place) -> Vec<String> {
    vec![
        "-o".into(),
        "ControlMaster=auto".into(),
        "-o".into(),
        format!("ControlPath={}", control_path(place)),
        "-o".into(),
        "ControlPersist=no".into(),
    ]
}

pub fn upload_arguments(place: &Place, remote_path: &str) -> Vec<String> {
    vec![
        "-o".into(),
        "BatchMode=yes".into(),
        "-o".into(),
        format!("ControlPath={}", control_path(place)),
        ssh_target(place),
        format!("cat > {}", shell_single_quote(remote_path)),
    ]
}

pub fn prepare_control_socket_directory() {
    let _ = std::fs::create_dir_all(caches_directory());
}

pub fn ssh_target(place: &Place) -> String {
    if place.user.is_empty() {
        place.host.clone()
    } else {
        format!("{}@{}", place.user, place.host)
    }
}

pub fn remote_command(place: &Place) -> String {
    match place.backend {
        crate::backend::Backend::Herdr => HERDR_ATTACH_SCRIPT.to_string(),
        crate::backend::Backend::Tmux => format!(
            "tmux new-session -A -s {}",
            shell_single_quote(place.session.as_deref().unwrap_or(""))
        ),
        crate::backend::Backend::Screen => format!(
            "screen -d -R {}",
            shell_single_quote(place.session.as_deref().unwrap_or(""))
        ),
    }
}

pub fn ssh_arguments(place: &Place, verbose_log: Option<&str>) -> Vec<String> {
    let wrapped = format!(
        "exec \"$SHELL\" -lc {}",
        shell_single_quote(&remote_command(place))
    );
    let mut arguments = vec!["-t".into()];
    if let Some(verbose_log) = verbose_log {
        arguments.extend(["-v".into(), "-E".into(), verbose_log.into()]);
    }
    arguments.extend([
        "-o".into(),
        format!("ServerAliveInterval={SERVER_ALIVE_INTERVAL}"),
        "-o".into(),
        format!("ServerAliveCountMax={SERVER_ALIVE_COUNT_MAX}"),
    ]);
    arguments.extend(control_master_options(place));
    arguments.push(ssh_target(place));
    arguments.push(wrapped);
    arguments
}

pub fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::Backend;
    use crate::paths::login_name;

    #[test]
    fn remote_tmux_uses_ssh_keepalives() {
        let place = Place::new("ada", "whiterun", Backend::Tmux, Some("foo".into()));
        assert_eq!(launch(&place, None).executable, SSH_EXECUTABLE);
        let mut expect = vec![
            "-t".into(),
            "-o".into(),
            "ServerAliveInterval=5".into(),
            "-o".into(),
            "ServerAliveCountMax=2".into(),
        ];
        expect.extend(control_master_options(&place));
        expect.push("ada@whiterun".into());
        expect.push("exec \"$SHELL\" -lc 'tmux new-session -A -s '\\''foo'\\'''".into());
        assert_eq!(ssh_arguments(&place, None), expect);
    }

    #[test]
    fn tmux_quotes_session_name() {
        let place = Place::new("ada", "whiterun", Backend::Tmux, Some("foo bar".into()));
        assert_eq!(remote_command(&place), "tmux new-session -A -s 'foo bar'");
        assert_eq!(
            ssh_arguments(&place, None).last().map(String::as_str),
            Some("exec \"$SHELL\" -lc 'tmux new-session -A -s '\\''foo bar'\\'''")
        );
    }

    #[test]
    fn screen_command() {
        let place = Place::new("ada", "phone", Backend::Screen, Some("foo".into()));
        assert_eq!(remote_command(&place), "screen -d -R 'foo'");
    }

    #[test]
    fn empty_host_is_local_login_shell() {
        let place = Place::new("", "", Backend::Herdr, None);
        let launched = launch(&place, None);
        assert_eq!(launched.executable, local_shell());
        assert_eq!(
            launched.arguments,
            vec!["-lc".to_string(), HERDR_ATTACH_SCRIPT.to_string()]
        );
        assert_eq!(launched.current_directory, Some(home_dir()));
        assert_eq!(
            place.display_label(),
            format!("{}@localhost:herdr", login_name())
        );
    }

    #[test]
    fn remote_herdr_uses_ssh_and_server_binary() {
        let place = Place::new("ada", "riverwood", Backend::Herdr, None);
        let launched = launch(&place, None);
        assert_eq!(launched.executable, SSH_EXECUTABLE);
        let command = remote_command(&place);
        assert!(command.contains("/proc/"));
        assert!(command.contains("exec herdr"));
        assert!(!command.contains("--remote"));
        assert_eq!(
            launched
                .arguments
                .iter()
                .take(5)
                .map(String::as_str)
                .collect::<Vec<_>>(),
            [
                "-t",
                "-o",
                "ServerAliveInterval=5",
                "-o",
                "ServerAliveCountMax=2"
            ]
        );
        assert_eq!(
            launched.arguments.iter().rev().nth(1).map(String::as_str),
            Some("ada@riverwood")
        );
        assert_eq!(
            control_master_options(&place)
                .iter()
                .take(2)
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["-o", "ControlMaster=auto"]
        );
    }

    #[test]
    fn verbose_log_url_is_unique_per_generation() {
        let id = Uuid::new_v4();
        assert_ne!(verbose_log_path(id, 0), verbose_log_path(id, 1));
        assert!(
            verbose_log_path(id, 2)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .contains("-2.log")
        );
    }

    #[test]
    fn remote_verbose_log_uses_dash_e() {
        let place = Place::new("ada", "whiterun", Backend::Herdr, None);
        let path = "/tmp/rterm-ssh.log";
        assert_eq!(
            ssh_arguments(&place, Some(path))
                .iter()
                .take(4)
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["-t", "-v", "-E", path]
        );
        assert_eq!(
            launch(&place, None)
                .arguments
                .iter()
                .take(2)
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["-t", "-o"]
        );
    }

    #[test]
    fn localhost_is_local_not_ssh() {
        let place = Place::new("ada", "localhost", Backend::Herdr, None);
        assert!(place.is_local());
        assert_eq!(place.display_label(), "ada@localhost:herdr");
        assert_eq!(launch(&place, None).executable, local_shell());
        assert_ne!(launch(&place, None).executable, SSH_EXECUTABLE);
    }

    #[test]
    fn loopback_literals_are_local() {
        assert!(Place::new("ada", "127.0.0.1", Backend::Herdr, None).is_local());
        assert!(Place::new("ada", "::1", Backend::Herdr, None).is_local());
        assert!(Place::new("ada", "[::1]", Backend::Herdr, None).is_local());
        assert!(!Place::new("ada", "riverwood", Backend::Herdr, None).is_local());
    }

    #[test]
    fn control_path_is_per_place() {
        let first = Place::new("ada", "whiterun", Backend::Herdr, None);
        let second = Place::new("ada", "whiterun", Backend::Herdr, None);
        assert_ne!(control_path(&first), control_path(&second));
        assert!(control_path(&first).contains(&uuid_upper(first.id)));
        assert!(control_path(&first).contains("/rterm/cm-"));
    }

    #[test]
    fn upload_uses_control_path_without_tty() {
        let place = Place::new("ada", "whiterun", Backend::Herdr, None);
        let remote = "/tmp/rterm-paste-x.png";
        assert_eq!(
            upload_arguments(&place, remote),
            vec![
                "-o".to_string(),
                "BatchMode=yes".into(),
                "-o".into(),
                format!("ControlPath={}", control_path(&place)),
                "ada@whiterun".into(),
                "cat > '/tmp/rterm-paste-x.png'".into(),
            ]
        );
    }

    #[test]
    fn empty_host_tmux_label() {
        let place = Place::new("", "  ", Backend::Tmux, Some("dev".into()));
        assert!(place.is_local());
        assert_eq!(
            place.display_label(),
            format!("{}@localhost:tmux(dev)", login_name())
        );
        assert_eq!(
            launch(&place, None).arguments,
            vec!["-lc".to_string(), "tmux new-session -A -s 'dev'".into()]
        );
    }
}
