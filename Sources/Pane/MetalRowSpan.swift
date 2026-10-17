import Foundation

struct LineStamp: Equatable {
    var identity: ObjectIdentifier
    var generation: UInt64
}

/// Rows to upload for one Metal frame.
///
/// A scroll-invariant update wins. `forcedFull` is the visible span when the
/// viewport changed without one, or when the caller asked for a full paint and
/// the terminal did not leave a usable range. `pending` is a range already
/// sitting on the view (a selection, or a draw the GPU refused).
///
/// When `narrow` is set and a previous paint exists, rows whose line identity
/// and generation still match are left out. The renderer only takes one closed
/// range, so the result runs from the first changed row to the last.
func metalRowSpan(
    updateStart: Int?,
    updateEnd: Int?,
    forcedFull: ClosedRange<Int>?,
    pending: ClosedRange<Int>?,
    painted: [Int: LineStamp],
    current: (Int) -> LineStamp?,
    narrow: Bool
) -> ClosedRange<Int>? {
    let reported: ClosedRange<Int>?
    if let updateStart, let updateEnd, updateStart <= updateEnd {
        reported = updateStart...updateEnd
    } else {
        reported = forcedFull
    }
    let base: ClosedRange<Int>?
    if narrow, let reported, !painted.isEmpty {
        base = changedRows(in: reported, painted: painted, current: current)
    } else {
        base = reported
    }
    return unionRows(base, pending)
}

/// Same end coordinate `Terminal.updateFullScreen` stores: `top + rows`, one
/// past the last visible row. The renderer clips to lines that exist.
func visibleRowSpan(top: Int, rows: Int) -> ClosedRange<Int>? {
    guard rows > 0, top >= 0 else {
        return nil
    }
    return top...(top + rows)
}

private func changedRows(
    in span: ClosedRange<Int>,
    painted: [Int: LineStamp],
    current: (Int) -> LineStamp?
) -> ClosedRange<Int>? {
    var lower: Int?
    var upper: Int?
    for row in span {
        // `getLine` only sees the viewport. The full-screen end index is one
        // past the last visible row, and scrollback rows are off screen.
        guard let stamp = current(row) else {
            continue
        }
        let previous = painted[row]
        let changed = previous?.identity != stamp.identity || previous?.generation != stamp.generation
        guard changed else {
            continue
        }
        lower = lower.map { min($0, row) } ?? row
        upper = upper.map { max($0, row) } ?? row
    }
    guard let lower, let upper else {
        return nil
    }
    return lower...upper
}

private func unionRows(_ lhs: ClosedRange<Int>?, _ rhs: ClosedRange<Int>?) -> ClosedRange<Int>? {
    switch (lhs, rhs) {
    case let (left?, right?):
        return min(left.lowerBound, right.lowerBound)...max(left.upperBound, right.upperBound)
    case let (left?, nil):
        return left
    case let (nil, right?):
        return right
    case (nil, nil):
        return nil
    }
}
