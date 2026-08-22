import SwiftUI

enum PasteHUD: Equatable {
    case working(String, sent: Int, total: Int)
    case failed(String)

    static let failClearNanoseconds: UInt64 = 1_500_000_000

    static var failure: PasteHUD { .failed("Paste failed") }

    static func working(isLocal: Bool, sent: Int = 0, total: Int) -> PasteHUD {
        .working(
            isLocal ? "Writing image…" : "Uploading image…",
            sent: sent,
            total: total
        )
    }

    var message: String {
        switch self {
        case .working(let text, _, _), .failed(let text):
            return text
        }
    }

    var fraction: Double? {
        switch self {
        case .working(_, let sent, let total):
            guard total > 0 else {
                return 0
            }
            return min(1, Double(sent) / Double(total))
        case .failed:
            return nil
        }
    }

    var percent: Int {
        Int(((fraction ?? 0) * 100).rounded())
    }

    var accessibilityText: String {
        switch self {
        case .working(let text, _, _):
            return "\(text) \(percent) percent"
        case .failed(let text):
            return text
        }
    }
}

struct PasteHUDView: View {
    var state: PasteHUD

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(state.message)
                .font(.caption)
                .foregroundStyle(.white)
            if let fraction = state.fraction {
                ProgressView(value: fraction)
                    .progressViewStyle(.linear)
                    .tint(.white)
                    .frame(width: 160)
            }
        }
        .padding(8)
        .background(Color.black.opacity(0.6), in: RoundedRectangle(cornerRadius: 8))
        .padding(12)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottomTrailing)
        .allowsHitTesting(false)
        .accessibilityElement(children: .combine)
        .accessibilityLabel(state.accessibilityText)
    }
}
