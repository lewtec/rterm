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
    static let connectTimeoutSeconds: TimeInterval = 5
    static let maxConnectAttempts = 3
    static let timedOutMessage = "connection timed out"
    static let maxCrumbs = 5

    private(set) var phase: Phase = .idle
    private(set) var generation: Int = 0
    private(set) var connectAttempt: Int = 0
    private(set) var crumbs: [String] = []
    private(set) var hidesConnectOverlay = false

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

    var isConnecting: Bool {
        guard case .attaching(let title) = phase else {
            return false
        }
        return Self.isConnectingHeadline(title)
    }

    var showsConnectOverlay: Bool {
        if case .attaching = phase {
            return !hidesConnectOverlay
        }
        return false
    }

    static func isConnectingHeadline(_ title: String) -> Bool {
        title == connectingTitle || title.hasPrefix("Connecting to ")
    }

    mutating func select() {
        if phase == .idle {
            beginConnect(bumpGeneration: false)
        }
    }

    mutating func reconnect() {
        beginConnect(bumpGeneration: true)
    }

    /// Remount the pane. The old local process dismantles with it.
    mutating func refresh() {
        generation += 1
    }

    mutating func noteProgress(_ headline: String) {
        guard case .attaching(let current) = phase, current != headline else {
            return
        }
        if current != Self.connectingTitle, headline == Self.connectingTitle {
            return
        }
        phase = .attaching(headline)
    }

    mutating func noteCrumb(_ line: String) {
        crumbs.append(line)
        if crumbs.count > Self.maxCrumbs {
            crumbs.removeFirst(crumbs.count - Self.maxCrumbs)
        }
    }

    mutating func hideConnectOverlay() {
        hidesConnectOverlay = true
    }

    mutating func noteTTY() {
        if case .attaching = phase {
            phase = .live
        }
    }

    mutating func sleep() {
        phase = .idle
        crumbs = []
        hidesConnectOverlay = false
    }

    mutating func failConnectTimeout() {
        guard isConnecting else {
            return
        }
        phase = .failed(Self.timedOutMessage)
    }

    mutating func setConnectAttempt(_ value: Int) {
        connectAttempt = value
    }

    mutating func noteConnectTimeout() {
        guard isConnecting else {
            return
        }
        if connectAttempt >= Self.maxConnectAttempts {
            failConnectTimeout()
            return
        }
        let next = connectAttempt + 1
        reconnect()
        connectAttempt = next
    }

    mutating func handleExit(_ code: Int32?, generation: Int? = nil) {
        if let generation, generation != self.generation {
            return
        }
        switch phase {
        case .attaching, .live:
            switch AttachExit.classify(code) {
            case .clean:
                phase = .idle
            case .failed(let message):
                phase = .failed(message)
            }
        case .idle, .failed:
            break
        }
    }

    private mutating func beginConnect(bumpGeneration: Bool) {
        if bumpGeneration {
            refresh()
        } else {
            crumbs = []
        }
        hidesConnectOverlay = false
        connectAttempt = 1
        phase = .attaching(Self.connectingTitle)
    }
}
