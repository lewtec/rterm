enum Driver {
    static let sshExecutable = "/usr/bin/ssh"

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

    static func sshArguments(for place: Place) -> [String] {
        let target = "\(place.user)@\(place.host)"
        let wrapped = "exec \"$SHELL\" -lc \(shellSingleQuote(remoteCommand(for: place)))"
        return ["-t", target, wrapped]
    }

    static func shellSingleQuote(_ value: String) -> String {
        "'" + value.replacingOccurrences(of: "'", with: "'\\''") + "'"
    }
}
