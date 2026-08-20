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

    var displayLabel: String {
        if let label, !label.isEmpty {
            return label
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
