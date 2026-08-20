import XCTest
@testable import rterm

@MainActor
final class AppStoreSleepTests: XCTestCase {
    func testSleepDropsLiveAndFailedTabsWithoutChangingSelection() throws {
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        let io = CatalogIO(fileURL: directory.appendingPathComponent("places.toml"))
        let store = AppStore(catalogIO: io)
        let first = try XCTUnwrap(store.places.first?.id)
        store.select(first)
        store.handleExit(first, code: 255)
        XCTAssertTrue(store.hasPane(first))
        store.dropAllConnections()
        XCTAssertEqual(store.selectedID, first)
        XCTAssertEqual(store.tabStates[first], .idle)
        XCTAssertFalse(store.hasPane(first))
    }

    func testSelectPlaceAtIndex() throws {
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        let io = CatalogIO(fileURL: directory.appendingPathComponent("places.toml"))
        let store = AppStore(catalogIO: io)
        let first = try XCTUnwrap(store.places.first?.id)
        store.selectPlace(at: 0)
        XCTAssertEqual(store.selectedID, first)
        store.selectPlace(at: 99)
        XCTAssertEqual(store.selectedID, first)
    }
}
