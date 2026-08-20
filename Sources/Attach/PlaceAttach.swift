import Foundation

/// Per-place attach lifecycle. Catalog identity stays on `Place`; this owns
/// only the disposable local process.
struct PlaceAttach: Equatable {
    enum Phase: Equatable {
        case idle
        case attaching(String)
        case live
        case failed(String)
    }

    static let connectingTitle = "Connecting…"

    private(set) var phase: Phase = .idle
    private(set) var generation: Int = 0

    var showsPane: Bool {
        switch phase {
        case .attaching, .live:
            return true
        case .idle, .failed:
            return false
        }
    }

    var connectHeadline: String? {
        if case .attaching(let title) = phase {
            return title
        }
        return nil
    }

    var tabState: TabState {
        switch phase {
        case .idle:
            return .idle
        case .attaching:
            return .attaching
        case .live:
            return .live
        case .failed(let message):
            return .failed(message)
        }
    }

    mutating func select() {
        if phase == .idle {
            phase = .attaching(Self.connectingTitle)
        }
    }

    mutating func reconnect() {
        generation += 1
        phase = .attaching(Self.connectingTitle)
    }

    mutating func noteProgress(_ headline: String) {
        guard case .attaching(let current) = phase, current != headline else {
            return
        }
        phase = .attaching(headline)
    }

    mutating func noteTTY() {
        if case .attaching = phase {
            phase = .live
        }
    }

    mutating func sleep() {
        phase = .idle
    }

    mutating func handleExit(_ code: Int32?) {
        switch AttachExit.classify(code) {
        case .clean:
            phase = .idle
        case .failed(let message):
            phase = .failed(message)
        }
    }
}
