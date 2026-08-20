import SwiftUI

@MainActor
final class ConnectProgress: ObservableObject {
    @Published private(set) var headline = "Connecting…"
    @Published private(set) var crumbs: [String] = []
    @Published private(set) var isVisible = false
    var onHeadline: ((String) -> Void)?

    func beginRemote() {
        headline = PlaceAttach.connectingTitle
        crumbs = []
        isVisible = true
        onHeadline?(headline)
    }

    func ingest(_ raw: String) {
        guard isVisible else {
            return
        }
        if let crumb = SSHConnectTrace.crumb(for: raw) {
            crumbs.append(crumb)
            if crumbs.count > 5 {
                crumbs.removeFirst(crumbs.count - 5)
            }
        }
        switch SSHConnectTrace.event(for: raw) {
        case .progress(let text):
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
}

struct ConnectOverlay: View {
    @ObservedObject var progress: ConnectProgress
    var headline: String

    var body: some View {
        ZStack {
            Color.black.opacity(0.55)
            VStack(spacing: 14) {
                ProgressView()
                    .controlSize(.large)
                    .tint(.white)
                Text(headline)
                    .font(.headline)
                    .foregroundStyle(.white)
                    .multilineTextAlignment(.center)
                VStack(alignment: .leading, spacing: 3) {
                    ForEach(Array(progress.crumbs.enumerated()), id: \.offset) { _, line in
                        Text(line)
                            .font(.system(.caption, design: .monospaced))
                            .foregroundStyle(.white.opacity(0.72))
                            .lineLimit(1)
                            .truncationMode(.middle)
                    }
                }
                .frame(maxWidth: 520, alignment: .leading)
            }
            .padding(28)
        }
        .allowsHitTesting(false)
    }
}
