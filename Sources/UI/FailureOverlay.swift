import SwiftUI

struct FailureOverlay: View {
    let message: String
    let onRetry: () -> Void

    var body: some View {
        ZStack {
            Color.black.opacity(0.45)
                .ignoresSafeArea(edges: [])
            VStack(spacing: 12) {
                Image(systemName: "exclamationmark.triangle.fill")
                    .font(.system(size: 36))
                    .foregroundStyle(.yellow)
                Text(message)
                    .multilineTextAlignment(.center)
                    .foregroundStyle(.white)
                    .textSelection(.enabled)
                Text("Click to attach again")
                    .font(.caption)
                    .foregroundStyle(.white.opacity(0.75))
            }
            .padding(24)
        }
        .contentShape(Rectangle())
        .onTapGesture(perform: onRetry)
        .accessibilityAddTraits(.isButton)
        .accessibilityLabel("Attach failed. \(message). Click to attach again.")
    }
}
