import XCTest
@testable import rterm

final class PlaceLinkTests: XCTestCase {
    func testRoundTripUsesSwitchKey() {
        let place = Place(user: "ada", host: "whiterun", backend: .herdr)
        XCTAssertEqual(place.switchKey, "ada@whiterun:herdr")
        let url = PlaceLink.url(for: place)
        XCTAssertEqual(url.scheme, "rterm")
        XCTAssertEqual(url.host, "place")
        XCTAssertEqual(PlaceLink.switchKey(from: url), "ada@whiterun:herdr")
        XCTAssertEqual(
            PlaceLink.switchKey(from: URL(string: "rterm://place/ada@whiterun:herdr")!),
            "ada@whiterun:herdr"
        )
        XCTAssertNil(PlaceLink.switchKey(from: URL(string: "https://example.com")!))
    }

    func testTmuxSessionStaysInTheKey() {
        let place = Place(user: "ada", host: "whiterun", backend: .tmux, session: "foo")
        XCTAssertEqual(place.switchKey, "ada@whiterun:tmux(foo)")
        XCTAssertEqual(PlaceLink.switchKey(from: PlaceLink.url(for: place)), place.switchKey)
    }

    func testParseSwitchKeyCoversSPECForm() {
        let herdr = Place.parseSwitchKey("ada@whiterun")
        XCTAssertEqual(herdr?.user, "ada")
        XCTAssertEqual(herdr?.host, "whiterun")
        XCTAssertEqual(herdr?.backend, .herdr)
        XCTAssertNil(herdr?.session)
        XCTAssertEqual(herdr?.switchKey, "ada@whiterun:herdr")

        let explicit = Place.parseSwitchKey("ada@whiterun:herdr")
        XCTAssertEqual(explicit?.switchKey, "ada@whiterun:herdr")

        let tmux = Place.parseSwitchKey("ada@whiterun:tmux(foo)")
        XCTAssertEqual(tmux?.backend, .tmux)
        XCTAssertEqual(tmux?.session, "foo")

        let local = Place.parseSwitchKey("ada@localhost:herdr")
        XCTAssertEqual(local?.host, "")
        XCTAssertEqual(local?.user, "ada")
        XCTAssertTrue(local?.isLocal ?? false)

        XCTAssertNil(Place.parseSwitchKey("ada@whiterun:tmux"))
        XCTAssertNil(Place.parseSwitchKey("whiterun:herdr"))
    }

    func testPlaceFileUsesTheSwitchKey() throws {
        let place = Place(
            id: UUID(uuidString: "33333333-3333-3333-3333-333333333333")!,
            user: "ada",
            host: "whiterun",
            backend: .herdr
        )
        XCTAssertEqual(PlaceFile.name(index: 0, place: place), "00-ada@whiterun-herdr.rterm-place")
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        let io = CatalogIO(directoryURL: directory)
        try io.save(CatalogDocument(version: 1, places: [place]))
        let file = directory.appendingPathComponent(PlaceFile.name(index: 0, place: place))
        XCTAssertEqual(PlaceLink.switchKey(from: file), "ada@whiterun:herdr")
        let loaded = try io.load()
        XCTAssertEqual(loaded.places, [place])
    }
}

final class PendingPlaceTests: XCTestCase {
    func testTakeReadsAndRemovesTheFile() throws {
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
            .appendingPathComponent("pending-place")
        PendingPlace.write("ada@whiterun:herdr", to: url)
        XCTAssertEqual(PendingPlace.take(from: url), "ada@whiterun:herdr")
        XCTAssertNil(PendingPlace.take(from: url))
    }
}

@MainActor
final class AppStoreOpenTests: XCTestCase {
    func testConsumePendingPlaceSelectsMatchingPlace() throws {
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        let io = CatalogIO(fileURL: directory.appendingPathComponent("places.toml"))
        let store = AppStore(catalogIO: io)
        let first = try XCTUnwrap(store.places.first?.id)
        store.beginAdd()
        store.editor?.user = "ada"
        store.editor?.host = "whiterun"
        store.editor?.backend = .herdr
        store.saveEditor()
        let second = try XCTUnwrap(store.places.last?.id)
        store.select(first)
        PendingPlace.write("ada@whiterun:herdr", to: io.pendingPlaceFile)
        store.consumePendingPlace()
        XCTAssertEqual(store.selectedID, second)
        store.consumePendingPlace()
        XCTAssertEqual(store.selectedID, second)
    }

    func testOpenPlaceSelectsMatchingPlace() throws {
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        let io = CatalogIO(fileURL: directory.appendingPathComponent("places.toml"))
        let store = AppStore(catalogIO: io)
        let first = try XCTUnwrap(store.places.first?.id)
        store.beginAdd()
        store.editor?.user = "ada"
        store.editor?.host = "whiterun"
        store.editor?.backend = .herdr
        store.saveEditor()
        let second = try XCTUnwrap(store.places.last?.id)
        store.select(first)
        store.openPlace(second)
        XCTAssertEqual(store.selectedID, second)
        store.select(first)
        store.openPlace(switchKey: "ada@whiterun:herdr")
        XCTAssertEqual(store.selectedID, second)
        store.openPlace(UUID())
        XCTAssertEqual(store.selectedID, second)
    }

    func testMovePlaceRenumbersFiles() throws {
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
            .appendingPathComponent("places", isDirectory: true)
        let first = Place(user: "ada", host: "whiterun", backend: .herdr)
        let second = Place(user: "ada", host: "riverwood", backend: .herdr)
        let third = Place(user: "ada", host: "riften", backend: .herdr)
        let io = CatalogIO(directoryURL: directory)
        try io.save(CatalogDocument(version: 1, places: [first, second, third]))
        let store = AppStore(catalogIO: io)
        store.movePlace(first.id, onto: third.id)
        XCTAssertEqual(store.places.map(\.id), [second.id, third.id, first.id])
        store.movePlace(first.id, onto: second.id)
        XCTAssertEqual(store.places.map(\.id), [first.id, second.id, third.id])
        store.movePlace(first.id, onto: first.id)
        XCTAssertEqual(store.places.map(\.id), [first.id, second.id, third.id])
        XCTAssertEqual(
            try FileManager.default.contentsOfDirectory(
                at: directory,
                includingPropertiesForKeys: nil
            )
            .filter { $0.pathExtension == PlaceFile.pathExtension }
            .map(\.lastPathComponent)
            .sorted { $0.localizedStandardCompare($1) == .orderedAscending },
            [
                PlaceFile.name(index: 0, place: first),
                PlaceFile.name(index: 1, place: second),
                PlaceFile.name(index: 2, place: third),
            ]
        )
    }

    func testOpenPlaceCreatesMissingSwitchKeyOnce() throws {
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        let io = CatalogIO(fileURL: directory.appendingPathComponent("places.toml"))
        let store = AppStore(catalogIO: io)
        let before = store.places.count
        store.openPlace(switchKey: "ada@riverwood:tmux(dev)")
        XCTAssertEqual(store.places.count, before + 1)
        XCTAssertEqual(store.selectedPlace?.switchKey, "ada@riverwood:tmux(dev)")
        let id = try XCTUnwrap(store.selectedID)
        store.openPlace(switchKey: "ada@riverwood:tmux(dev)")
        XCTAssertEqual(store.places.count, before + 1)
        XCTAssertEqual(store.selectedID, id)
    }

}
