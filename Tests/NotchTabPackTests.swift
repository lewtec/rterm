import XCTest
@testable import rterm

final class NotchTabPackTests: XCTestCase {
    func testInfiniteCapKeepsEveryPlaceOnTheLeft() {
        let places = (0..<4).map { Self.place("h\($0)") }
        let packed = NotchTabPack.split(places: places, leftCap: .infinity, widthOf: { _ in 80 })
        XCTAssertEqual(packed.leading.map(\.host), ["h0", "h1", "h2", "h3"])
        XCTAssertTrue(packed.trailing.isEmpty)
    }

    func testOverflowMovesToTheRight() {
        let places = (0..<4).map { Self.place("h\($0)") }
        let packed = NotchTabPack.split(places: places, leftCap: 110, widthOf: { _ in 50 }, spacing: 2)
        XCTAssertEqual(packed.leading.map(\.host), ["h0", "h1"])
        XCTAssertEqual(packed.trailing.map(\.host), ["h2", "h3"])
    }

    func testFirstPlaceStaysLeftWhenWiderThanTheCap() {
        let places = [Self.place("wide"), Self.place("next")]
        let packed = NotchTabPack.split(places: places, leftCap: 10, widthOf: { _ in 50 })
        XCTAssertEqual(packed.leading.map(\.host), ["wide"])
        XCTAssertEqual(packed.trailing.map(\.host), ["next"])
    }

    func testEmptyCatalog() {
        let packed = NotchTabPack.split(places: [], leftCap: 200, widthOf: { _ in 50 })
        XCTAssertTrue(packed.leading.isEmpty)
        XCTAssertTrue(packed.trailing.isEmpty)
    }

    private static func place(_ host: String) -> Place {
        Place(user: "ada", host: host, backend: .herdr)
    }
}
