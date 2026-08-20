import Foundation

enum Driver {
    static let sshExecutable = "/usr/bin/ssh"

    static var localShell: String {
        ProcessInfo.processInfo.environment["SHELL"] ?? "/bin/zsh"
    }

    struct Launch: Equatable {
        var executable: String
        var arguments: [String]
    }

    static func launch(for place: Place) -> Launch {
        if place.isLocal {
            return Launch(executable: localShell, arguments: ["-lc", remoteCommand(for: place)])
        }
        return Launch(executable: sshExecutable, arguments: sshArguments(for: place))
    }

    static func sshTarget(for place: Place) -> String {
        if place.user.isEmpty {
            return place.host
        }
        return "\(place.user)@\(place.host)"
    }

    /// Run the herdr binary that owns the live server socket, not PATH.
    static let herdrAttachScript = """
        sock="${HOME}/.config/herdr/herdr.sock"
        if command -v herdr >/dev/null 2>&1; then
          js=$(herdr status server --json 2>/dev/null) || true
          parsed=$(printf %s "$js" | sed -n 's/.*"socket":"\\([^"]*\\)".*/\\1/p')
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
          exe=$(ps -p "$pid" -o args= 2>/dev/null | awk "{print \\$1}")
          if [ -x "$exe" ]; then exec "$exe"; fi
        fi
        exec herdr
        """

    static func remoteCommand(for place: Place) -> String {
        switch place.backend {
        case .herdr:
            return herdrAttachScript
        case .tmux:
            return "tmux new-session -A -s \(shellSingleQuote(place.session ?? ""))"
        case .screen:
            return "screen -d -R \(shellSingleQuote(place.session ?? ""))"
        }
    }

    static let serverAliveInterval = 5
    static let serverAliveCountMax = 2

    static func sshArguments(for place: Place) -> [String] {
        let target = sshTarget(for: place)
        let wrapped = "exec \"$SHELL\" -lc \(shellSingleQuote(remoteCommand(for: place)))"
        return [
            "-t",
            "-o", "ServerAliveInterval=\(serverAliveInterval)",
            "-o", "ServerAliveCountMax=\(serverAliveCountMax)",
            target,
            wrapped,
        ]
    }

    static func shellSingleQuote(_ value: String) -> String {
        "'" + value.replacingOccurrences(of: "'", with: "'\\''") + "'"
    }
}
