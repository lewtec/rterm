import SwiftUI

@MainActor
@Observable
final class ConnectProgress {
    static let maxCrumbs = 5

    private(set) var isVisible = false
    private(set) var headline = PlaceAttach.connectingTitle
    private(set) var crumbs: [String] = []
    @ObservationIgnored
    var onHeadline: ((String) -> Void)?

    func begin(clearCrumbs: Bool) {
        if clearCrumbs {
            crumbs = []
        }
        headline = PlaceAttach.connectingTitle
        isVisible = true
        onHeadline?(headline)
    }

    func ingest(_ raw: String) {
        guard isVisible else {
            return
        }
        if let crumb = SSHConnectTrace.crumb(for: raw) {
            crumbs.append(crumb)
            if crumbs.count > Self.maxCrumbs {
                crumbs.removeFirst(crumbs.count - Self.maxCrumbs)
            }
        }
        switch SSHConnectTrace.event(for: raw) {
        case .progress(let text):
            guard headline != text else {
                return
            }
            headline = text
            onHeadline?(text)
        case .revealTerminal:
            isVisible = false
        case .none:
            break
        }
    }

    func finish() {
        isVisible = false
    }

    func reset() {
        crumbs = []
        headline = PlaceAttach.connectingTitle
        isVisible = false
    }
}

struct ConnectOverlay: View {
    var headline: String
    var crumbs: [String]

    var body: some View {
        VStack(spacing: 12) {
            ActivitySpinner(size: 36, color: .white)
            Text(headline)
                .multilineTextAlignment(.center)
                .foregroundStyle(.white)
            VStack(alignment: .leading, spacing: 3) {
                ForEach(Array(crumbs.enumerated()), id: \.offset) { _, line in
                    Text(line)
                        .font(.system(.caption, design: .monospaced))
                        .foregroundStyle(.white.opacity(0.72))
                        .lineLimit(1)
                        .truncationMode(.middle)
                }
            }
            .frame(maxWidth: 520, alignment: .leading)
        }
        .padding(24)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Color.black.opacity(0.45), ignoresSafeAreaEdges: [])
        .accessibilityElement(children: .combine)
        .accessibilityLabel(headline)
    }
}
