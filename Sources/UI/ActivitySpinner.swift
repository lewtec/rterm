import SwiftUI

/// 12-spoke activity indicator. Drawn in SwiftUI so it composites over SwiftTerm.
struct ActivitySpinner: View {
    var size: CGFloat = 36
    var color: Color = .primary

    private static let spokes = 12

    var body: some View {
        TimelineView(.periodic(from: .now, by: 1.0 / 12.0)) { timeline in
            let step = Int(timeline.date.timeIntervalSinceReferenceDate * 12)
            ZStack {
                ForEach(0..<Self.spokes, id: \.self) { index in
                    let trail = (index - (step % Self.spokes) + Self.spokes) % Self.spokes
                    Capsule()
                        .fill(color.opacity(0.18 + 0.82 * (1 - Double(trail) / Double(Self.spokes))))
                        .frame(width: size * 3.5 / 36, height: size * 10 / 36)
                        .offset(y: -size * 11 / 36)
                        .rotationEffect(.degrees(Double(index) * 30))
                }
            }
            .frame(width: size, height: size)
        }
    }
}
