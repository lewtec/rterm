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
        XCTAssertEqual(store.attaches[first]?.phase, .failed("exited (255)"))
        XCTAssertEqual(store.attaches[first]?.showsPane, false)
        store.dropAllConnections()
        XCTAssertEqual(store.selectedID, first)
        XCTAssertEqual(store.attaches[first]?.phase, .idle)
        XCTAssertEqual(store.attaches[first]?.showsPane, false)
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

    func testConnectTimeoutRetriesThenGivesUp() throws {
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        let io = CatalogIO(fileURL: directory.appendingPathComponent("places.toml"))
        let store = AppStore(catalogIO: io)
        let first = try XCTUnwrap(store.places.first?.id)
        store.select(first)
        XCTAssertEqual(store.attaches[first]?.connectAttempt, 1)

        store.noteConnectTimeout(first)
        XCTAssertEqual(store.attaches[first]?.phase, .attaching(PlaceAttach.connectingTitle))
        XCTAssertEqual(store.attaches[first]?.connectAttempt, 2)
        XCTAssertEqual(store.attaches[first]?.generation, 1)

        store.noteConnectTimeout(first)
        store.noteConnectTimeout(first)
        XCTAssertEqual(store.attaches[first]?.phase, .failed(PlaceAttach.timedOutMessage))

        store.handleExit(first, code: 255, generation: store.attaches[first]?.generation)
        XCTAssertEqual(store.attaches[first]?.phase, .failed(PlaceAttach.timedOutMessage))
    }
}
