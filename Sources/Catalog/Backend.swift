enum Backend: String, CaseIterable, Identifiable, Hashable {
    case herdr
    case tmux
    case screen

    var id: String { rawValue }
}
