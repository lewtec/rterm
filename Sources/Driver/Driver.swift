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

    static func remoteCommand(for place: Place) -> String {
        switch place.backend {
        case .herdr:
            return "herdr"
        case .tmux:
            return "tmux new-session -A -s \(shellSingleQuote(place.session ?? ""))"
        case .screen:
            return "screen -d -R \(shellSingleQuote(place.session ?? ""))"
        }
    }

    static let serverAliveInterval = 5
    static let serverAliveCountMax = 2

    static func sshArguments(for place: Place) -> [String] {
        let target = "\(place.user)@\(place.host)"
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
