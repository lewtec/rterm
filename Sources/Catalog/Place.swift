import Foundation

struct Place: Identifiable, Hashable {
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
        if isLocal {
            switch backend {
            case .herdr:
                return "herdr"
            case .tmux:
                return "tmux(\(session ?? ""))"
            case .screen:
                return "screen(\(session ?? ""))"
            }
        }
        switch backend {
        case .herdr:
            return "\(user)@\(host)"
        case .tmux:
            return "\(user)@\(host):tmux(\(session ?? ""))"
        case .screen:
            return "\(user)@\(host):screen(\(session ?? ""))"
        }
    }
}
