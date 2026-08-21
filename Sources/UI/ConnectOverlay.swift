import SwiftUI

@MainActor
final class ConnectProgress: ObservableObject {
    @Published private(set) var isVisible = false
    var onHeadline: ((String) -> Void)?
    var onCrumb: ((String) -> Void)?
    var onReveal: (() -> Void)?

    func beginRemote() {
        isVisible = true
        onHeadline?(PlaceAttach.connectingTitle)
    }

    func ingest(_ raw: String) {
        guard isVisible else {
            return
        }
        if let crumb = SSHConnectTrace.crumb(for: raw) {
            onCrumb?(crumb)
        }
        switch SSHConnectTrace.event(for: raw) {
        case .progress(let text):
            onHeadline?(text)
        case .revealTerminal:
            isVisible = false
            onReveal?()
        case .none:
            break
        }
    }

    func finish() {
        isVisible = false
    }
}

struct ConnectOverlay: View {
    var headline: String
    var crumbs: [String]

    var body: some View {
        ZStack {
            Color.black.opacity(0.45)
            VStack(spacing: 12) {
                ProgressView()
                    .controlSize(.large)
                    .tint(.white)
                    .frame(width: 36, height: 36)
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
        }
        .allowsHitTesting(false)
        .accessibilityElement(children: .combine)
        .accessibilityLabel(headline)
    }
}
