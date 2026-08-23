import Foundation

struct Place: Identifiable, Hashable, Sendable {
    var id: UUID
    var user: String
    var host: String
    var backend: Backend
    var session: String?
    var label: String?

    init(
        id: UUID = UUID(),
        user: String,
        host: String,
        backend: Backend,
        session: String? = nil,
        label: String? = nil
    ) {
        self.id = id
        self.user = user
        self.host = host
        self.backend = backend
        self.session = session
        self.label = label
    }

    var isLocal: Bool {
        Self.isLoopbackHost(host)
    }

    static func isLoopbackHost(_ host: String) -> Bool {
        var value = host.trimmingCharacters(in: .whitespacesAndNewlines)
        if value.hasPrefix("[") && value.hasSuffix("]") {
            value = String(value.dropFirst().dropLast())
        }
        value = value.trimmingCharacters(in: CharacterSet(charactersIn: ".")).lowercased()
        switch value {
        case "", "localhost", "localhost.localdomain", "127.0.0.1", "0.0.0.0", "::1":
            return true
        default:
            return value.hasSuffix(".localhost")
        }
    }

    var switchKey: String {
        let shownHost = isLocal ? "localhost" : host
        let shownUser = user.isEmpty && isLocal ? NSUserName() : user
        let core = shownUser.isEmpty ? shownHost : "\(shownUser)@\(shownHost)"
        switch backend {
        case .herdr:
            return "\(core):herdr"
        case .tmux:
            return "\(core):tmux(\(session ?? ""))"
        case .screen:
            return "\(core):screen(\(session ?? ""))"
        }
    }

    var displayLabel: String {
        if let label, !label.isEmpty {
            return label
        }
        return switchKey
    }

    func matchesSwitchKey(_ key: String) -> Bool {
        let needle = key.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !needle.isEmpty else {
            return false
        }
        if switchKey == needle || displayLabel == needle {
            return true
        }
        return switchKey.compare(needle, options: [.caseInsensitive]) == .orderedSame
            || displayLabel.compare(needle, options: [.caseInsensitive]) == .orderedSame
    }

    static func parseSwitchKey(_ key: String) -> Place? {
        var text = key.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else {
            return nil
        }

        let backend: Backend
        let session: String?
        if let suffix = parseBackendSuffix(text) {
            text = suffix.head
            backend = suffix.backend
            session = suffix.session
        } else {
            backend = .herdr
            session = nil
        }
        if backend != .herdr, session == nil || session?.isEmpty == true {
            return nil
        }

        let user: String
        let host: String
        if let at = text.lastIndex(of: "@") {
            user = String(text[..<at]).trimmingCharacters(in: .whitespacesAndNewlines)
            host = String(text[text.index(after: at)...]).trimmingCharacters(in: .whitespacesAndNewlines)
        } else {
            user = ""
            host = text.trimmingCharacters(in: .whitespacesAndNewlines)
        }

        var resolvedUser = user
        var resolvedHost = host
        if isLoopbackHost(resolvedHost) {
            resolvedHost = ""
            if resolvedUser.isEmpty {
                resolvedUser = NSUserName()
            }
        } else if resolvedUser.isEmpty || resolvedHost.isEmpty {
            return nil
        }

        return Place(
            user: resolvedUser,
            host: resolvedHost,
            backend: backend,
            session: backend == .herdr ? nil : session
        )
    }

    private static func parseBackendSuffix(_ text: String) -> (head: String, backend: Backend, session: String?)? {
        guard let colon = text.lastIndex(of: ":") else {
            return nil
        }
        let head = String(text[..<colon])
        let tail = String(text[text.index(after: colon)...])
        if tail == Backend.herdr.rawValue {
            return (head, .herdr, nil)
        }
        for backend in [Backend.tmux, Backend.screen] {
            let prefix = "\(backend.rawValue)("
            if tail.hasPrefix(prefix), tail.hasSuffix(")") {
                let inner = tail.dropFirst(prefix.count).dropLast()
                return (head, backend, String(inner))
            }
            if tail == backend.rawValue {
                return (head, backend, nil)
            }
        }
        return nil
    }
}
