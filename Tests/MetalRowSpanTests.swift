import XCTest
@testable import rterm

final class MetalRowSpanTests: XCTestCase {
    private final class Token {}

    private func stamp(_ token: Token, generation: UInt64) -> LineStamp {
        LineStamp(identity: ObjectIdentifier(token), generation: generation)
    }

    func testUpdateRangeBeatsForcedFull() {
        let span = metalRowSpan(
            updateStart: 4,
            updateEnd: 6,
            forcedFull: 0...40,
            pending: nil,
            painted: [:],
            current: { _ in nil },
            narrow: false
        )
        XCTAssertEqual(span, 4...6)
    }

    func testInvertedUpdateUsesForcedFull() {
        let span = metalRowSpan(
            updateStart: 8,
            updateEnd: 2,
            forcedFull: 10...34,
            pending: nil,
            painted: [:],
            current: { _ in nil },
            narrow: false
        )
        XCTAssertEqual(span, 10...34)
    }

    func testNilUpdateAndNilFullSkips() {
        let span = metalRowSpan(
            updateStart: nil,
            updateEnd: nil,
            forcedFull: nil,
            pending: nil,
            painted: [:],
            current: { _ in nil },
            narrow: true
        )
        XCTAssertNil(span)
    }

    func testSingleRowUpdateIsKept() {
        let span = metalRowSpan(
            updateStart: 3,
            updateEnd: 3,
            forcedFull: nil,
            pending: nil,
            painted: [:],
            current: { _ in nil },
            narrow: true
        )
        XCTAssertEqual(span, 3...3)
    }

    func testUnchangedRowsDropOutOfAWideUpdate() {
        let line = Token()
        let other = Token()
        let painted = [
            0: stamp(line, generation: 1),
            1: stamp(line, generation: 1),
            2: stamp(other, generation: 4),
            3: stamp(line, generation: 1),
        ]
        let current: (Int) -> LineStamp? = { row in
            if row == 2 {
                return self.stamp(other, generation: 5)
            }
            return self.stamp(line, generation: 1)
        }
        let span = metalRowSpan(
            updateStart: 0,
            updateEnd: 3,
            forcedFull: nil,
            pending: nil,
            painted: painted,
            current: current,
            narrow: true
        )
        XCTAssertEqual(span, 2...2)
    }

    func testChangedEndsIncludeTheGap() {
        let line = Token()
        let painted = [
            0: stamp(line, generation: 1),
            1: stamp(line, generation: 1),
            2: stamp(line, generation: 1),
        ]
        let current: (Int) -> LineStamp? = { row in
            let generation: UInt64 = (row == 1) ? 1 : 2
            return self.stamp(line, generation: generation)
        }
        let span = metalRowSpan(
            updateStart: 0,
            updateEnd: 2,
            forcedFull: nil,
            pending: nil,
            painted: painted,
            current: current,
            narrow: true
        )
        XCTAssertEqual(span, 0...2)
    }

    func testMatchingGenerationSkipsUnlessPending() {
        let line = Token()
        let painted = [1: stamp(line, generation: 7)]
        let current: (Int) -> LineStamp? = { _ in self.stamp(line, generation: 7) }
        let skipped = metalRowSpan(
            updateStart: 1,
            updateEnd: 1,
            forcedFull: nil,
            pending: nil,
            painted: painted,
            current: current,
            narrow: true
        )
        XCTAssertNil(skipped)
        let pending = metalRowSpan(
            updateStart: 1,
            updateEnd: 1,
            forcedFull: nil,
            pending: 4...9,
            painted: painted,
            current: current,
            narrow: true
        )
        XCTAssertEqual(pending, 4...9)
    }

    func testFullPaintKeepsUnchangedRows() {
        let line = Token()
        let painted = [0: stamp(line, generation: 1), 1: stamp(line, generation: 1)]
        let current: (Int) -> LineStamp? = { _ in self.stamp(line, generation: 1) }
        let span = metalRowSpan(
            updateStart: 0,
            updateEnd: 1,
            forcedFull: nil,
            pending: nil,
            painted: painted,
            current: current,
            narrow: false
        )
        XCTAssertEqual(span, 0...1)
    }

    func testPendingExtendsAPartialChange() {
        let line = Token()
        let painted = [
            0: stamp(line, generation: 1),
            1: stamp(line, generation: 1),
            2: stamp(line, generation: 1),
        ]
        let current: (Int) -> LineStamp? = { row in
            self.stamp(line, generation: row == 0 ? 2 : 1)
        }
        let span = metalRowSpan(
            updateStart: 0,
            updateEnd: 2,
            forcedFull: nil,
            pending: 2...5,
            painted: painted,
            current: current,
            narrow: true
        )
        XCTAssertEqual(span, 0...5)
    }

    func testRowsOutsideTheViewportAreIgnored() {
        let line = Token()
        let painted = [
            0: stamp(line, generation: 1),
            1: stamp(line, generation: 1),
        ]
        let current: (Int) -> LineStamp? = { row in
            if row == 2 {
                return nil
            }
            return self.stamp(line, generation: row == 0 ? 2 : 1)
        }
        let span = metalRowSpan(
            updateStart: 0,
            updateEnd: 2,
            forcedFull: nil,
            pending: nil,
            painted: painted,
            current: current,
            narrow: true
        )
        XCTAssertEqual(span, 0...0)
        let noneVisible = metalRowSpan(
            updateStart: 5,
            updateEnd: 6,
            forcedFull: nil,
            pending: nil,
            painted: painted,
            current: { _ in nil },
            narrow: true
        )
        XCTAssertNil(noneVisible)
    }

    func testVisibleSpanMatchesFullScreenEnd() {
        XCTAssertEqual(visibleRowSpan(top: 10, rows: 24), 10...34)
        XCTAssertNil(visibleRowSpan(top: 0, rows: 0))
        XCTAssertNil(visibleRowSpan(top: -1, rows: 10))
    }
}
