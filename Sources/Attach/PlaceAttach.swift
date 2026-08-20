import Foundation

/// Per-place attach lifecycle. Catalog identity stays on `Place`; this owns
/// only the disposable local process.
struct PlaceAttach: Equatable {
    enum Phase: Equatable {
        case idle
        case live
        case failed(String)
    }

    private(set) var phase: Phase = .idle
    private(set) var generation: Int = 0

    var showsPane: Bool {
        phase == .live
    }

    var tabState: TabState {
        switch phase {
        case .idle:
            return .idle
        case .live:
            return .live
        case .failed(let message):
            return .failed(message)
        }
    }

    mutating func select() {
        if phase == .idle {
            phase = .live
        }
    }

    mutating func reconnect() {
        generation += 1
        phase = .live
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
