enum Backend: String, CaseIterable, Identifiable, Hashable, Sendable {
    case herdr
    case tmux
    case screen

    var id: String { rawValue }
}
