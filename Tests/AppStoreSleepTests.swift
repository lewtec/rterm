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

    func testConnectProgressStaysOffAttachAndSurvivesReconnect() throws {
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        let io = CatalogIO(fileURL: directory.appendingPathComponent("places.toml"))
        let store = AppStore(catalogIO: io)
        let first = try XCTUnwrap(store.places.first?.id)
        store.select(first)
        let progress = try XCTUnwrap(store.connectProgress(for: first))
        progress.ingest("debug1: Connecting to whiterun [10.0.0.2] port 22.")
        progress.ingest("debug1: Connection established.")
        XCTAssertEqual(progress.crumbs.count, 2)
        store.reconnect(first)
        XCTAssertEqual(store.connectProgress(for: first)?.crumbs.count, 2)
        XCTAssertEqual(store.tabState(for: first), .attaching)
        store.dropAllConnections()
        XCTAssertEqual(store.connectProgress(for: first)?.crumbs ?? ["x"], [])
        XCTAssertFalse(store.connectProgress(for: first)?.isVisible ?? true)
        store.select(first)
        XCTAssertEqual(store.connectProgress(for: first)?.crumbs ?? ["x"], [])
        XCTAssertTrue(store.connectProgress(for: first)?.isVisible ?? false)
    }

    func testDuplicateProgressDoesNotChangeAttach() throws {
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        let io = CatalogIO(fileURL: directory.appendingPathComponent("places.toml"))
        let store = AppStore(catalogIO: io)
        let first = try XCTUnwrap(store.places.first?.id)
        store.select(first)
        let attach = try XCTUnwrap(store.attaches[first])
        store.noteProgress(first, PlaceAttach.connectingTitle)
        XCTAssertEqual(store.attaches[first], attach)
    }
}
