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

    var displayLabel: String {
        if let label, !label.isEmpty {
            return label
        }
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
}
