import SwiftUI

enum PasteHUD: Equatable {
    case working(String)
    case failed(String)

    static let failClearNanoseconds: UInt64 = 1_500_000_000

    static var failure: PasteHUD { .failed("Paste failed") }

    static func working(isLocal: Bool) -> PasteHUD {
        .working(isLocal ? "Writing image…" : "Uploading image…")
    }

    var message: String {
        switch self {
        case .working(let text), .failed(let text):
            return text
        }
    }

    var showsSpinner: Bool {
        if case .working = self {
            return true
        }
        return false
    }
}

struct PasteHUDView: View {
    var state: PasteHUD

    var body: some View {
        HStack(spacing: 8) {
            if state.showsSpinner {
                ActivitySpinner(size: 16, color: .white)
            }
            Text(state.message)
                .font(.caption)
                .foregroundStyle(.white)
        }
        .padding(8)
        .background(Color.black.opacity(0.6), in: RoundedRectangle(cornerRadius: 8))
        .padding(12)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottomTrailing)
        .allowsHitTesting(false)
        .accessibilityElement(children: .combine)
        .accessibilityLabel(state.message)
    }
}
