import XCTest
@testable import rterm

final class CatalogIOTests: XCTestCase {
    func testMigratesLegacyTomlIntoPlaceFiles() throws {
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        let toml = directory.appendingPathComponent("places.toml")
        let placesDir = directory.appendingPathComponent("places", isDirectory: true)
        let place = Place(
            id: UUID(uuidString: "11111111-1111-1111-1111-111111111111")!,
            user: "ada",
            host: "riverwood",
            backend: .herdr
        )
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        try CatalogCodec.encode(CatalogDocument(version: 1, places: [place]))
            .write(to: toml, atomically: true, encoding: .utf8)
        let io = CatalogIO(fileURL: toml)
        let loaded = try io.load()
        XCTAssertEqual(loaded.places, [place])
        XCTAssertFalse(FileManager.default.fileExists(atPath: toml.path))
        XCTAssertTrue(
            FileManager.default.fileExists(
                atPath: placesDir.appendingPathComponent(PlaceFile.name(index: 0, place: place)).path
            )
        )
    }

    func testSavePreservesTabOrder() throws {
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
            .appendingPathComponent("places", isDirectory: true)
        let places = [
            Place(user: "ada", host: "whiterun", backend: .herdr),
            Place(user: "ada", host: "riverwood", backend: .tmux, session: "foo"),
        ]
        let io = CatalogIO(directoryURL: directory)
        try io.save(CatalogDocument(version: 1, places: places))
        XCTAssertEqual(try io.load().places.map(\.switchKey), places.map(\.switchKey))
    }

    func testSaveRenumbersPrefixesAfterReorder() throws {
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
            .appendingPathComponent("places", isDirectory: true)
        let first = Place(user: "ada", host: "whiterun", backend: .herdr)
        let second = Place(user: "ada", host: "riverwood", backend: .tmux, session: "foo")
        let third = Place(user: "ada", host: "riften", backend: .herdr)
        let io = CatalogIO(directoryURL: directory)
        try io.save(CatalogDocument(version: 1, places: [first, second, third]))
        try io.save(CatalogDocument(version: 1, places: [second, third, first]))
        XCTAssertEqual(placeFileNames(in: directory), [
            PlaceFile.name(index: 0, place: second),
            PlaceFile.name(index: 1, place: third),
            PlaceFile.name(index: 2, place: first),
        ])
        XCTAssertEqual(
            try io.load().places.map(\.switchKey),
            [second, third, first].map(\.switchKey)
        )
    }

    private func placeFileNames(in directory: URL) -> [String] {
        ((try? FileManager.default.contentsOfDirectory(
            at: directory,
            includingPropertiesForKeys: nil
        )) ?? [])
            .filter { $0.pathExtension == PlaceFile.pathExtension }
            .map(\.lastPathComponent)
            .sorted { $0.localizedStandardCompare($1) == .orderedAscending }
    }
}
