import Foundation

/// Maps OpenSSH `-v` lines (usually via `-E`) into overlay copy.
enum SSHConnectTrace {
    enum Event: Equatable {
        case progress(String)
        case revealTerminal
    }

    static func event(for rawLine: String) -> Event? {
        let line = normalize(rawLine)
        guard !line.isEmpty else {
            return nil
        }
        if needsTerminal(line) {
            return .revealTerminal
        }
        if let headline = headline(for: line) {
            return .progress(headline)
        }
        return nil
    }

    static func crumb(for rawLine: String) -> String? {
        let line = normalize(rawLine)
        guard !line.isEmpty else {
            return nil
        }
        return line
    }

    private static func needsTerminal(_ line: String) -> Bool {
        if line.contains("Next authentication method: password") {
            return true
        }
        if line.contains("Next authentication method: keyboard-interactive") {
            return true
        }
        if line.contains("The authenticity of host") {
            return true
        }
        if line.hasPrefix("Enter passphrase") {
            return true
        }
        return false
    }

    private static func headline(for line: String) -> String? {
        if let host = suffix(after: "Connecting to ", in: line) {
            return "Connecting to \(shortHost(host))"
        }
        if line.contains("Connection established.") {
            return "Connected"
        }
        if let rest = suffix(after: "Authenticating to ", in: line) {
            return "Authenticating to \(shortHost(rest))"
        }
        if line.contains("Offering public key") {
            return "Offering key"
        }
        if line.contains("Server accepts key") {
            return "Key accepted"
        }
        if let rest = suffix(after: "Authenticated to ", in: line) {
            return "Authenticated to \(shortHost(rest))"
        }
        if line.contains("Sending command:") {
            return "Waiting for remote"
        }
        if line.contains("Could not resolve hostname") {
            return line.hasPrefix("ssh: ") ? String(line.dropFirst(5)) : line
        }
        if line.contains("Connection timed out") || line.contains("Connection refused") {
            return line
        }
        if line.contains("Permission denied") {
            return "Permission denied"
        }
        if line.contains("Host key verification failed") {
            return "Host key verification failed"
        }
        return nil
    }

    private static func normalize(_ raw: String) -> String {
        var line = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        if line.hasPrefix("debug1: ") {
            line = String(line.dropFirst("debug1: ".count))
        } else if line.hasPrefix("debug2: ") {
            line = String(line.dropFirst("debug2: ".count))
        }
        return line
    }

    private static func suffix(after marker: String, in line: String) -> String? {
        guard let range = line.range(of: marker) else {
            return nil
        }
        let rest = line[range.upperBound...].trimmingCharacters(in: .whitespaces)
        return rest.isEmpty ? nil : rest
    }

    private static func shortHost(_ raw: String) -> String {
        var value = raw
        if let using = value.range(of: " using ") {
            value = String(value[..<using.lowerBound])
        }
        if let asUser = value.range(of: " as ") {
            return String(value[..<asUser.lowerBound])
                .trimmingCharacters(in: .whitespaces)
        }
        if let bracket = value.firstIndex(of: "[") {
            value = String(value[..<bracket])
        }
        if let port = value.range(of: " port ") {
            value = String(value[..<port.lowerBound])
        }
        return value.trimmingCharacters(in: .whitespaces.union(CharacterSet(charactersIn: ".(")))
    }
}
