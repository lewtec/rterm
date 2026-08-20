import XCTest
@testable import rterm

final class CatalogCodecTests: XCTestCase {
    func testRoundTripPreservesOrderAndOptionalFields() throws {
        let places = [
            Place(
                id: UUID(uuidString: "11111111-1111-1111-1111-111111111111")!,
                user: "ada",
                host: "riverwood",
                backend: .herdr
            ),
            Place(
                id: UUID(uuidString: "22222222-2222-2222-2222-222222222222")!,
                user: "ada",
                host: "whiterun",
                backend: .tmux,
                session: "foo",
                label: "work"
            ),
        ]
        let encoded = CatalogCodec.encode(CatalogDocument(version: 1, places: places))
        let decoded = try CatalogCodec.decode(encoded)
        XCTAssertEqual(decoded.version, 1)
        XCTAssertEqual(decoded.places, places)
        let herdrOnly = CatalogCodec.encode(CatalogDocument(version: 1, places: [places[0]]))
        XCTAssertFalse(herdrOnly.contains("session"))
    }

    func testHerdrSessionIsDropped() throws {
        let text = """
        version = 1

        [[place]]
        id = "11111111-1111-1111-1111-111111111111"
        user = "ada"
        host = "riverwood"
        backend = "herdr"
        session = "ignored"
        """
        let decoded = try CatalogCodec.decode(text)
        XCTAssertNil(decoded.places[0].session)
    }

    func testTmuxRequiresSession() {
        let text = """
        version = 1

        [[place]]
        id = "11111111-1111-1111-1111-111111111111"
        user = "ada"
        host = "whiterun"
        backend = "tmux"
        """
        XCTAssertThrowsError(try CatalogCodec.decode(text)) { error in
            XCTAssertEqual(error as? CatalogCodecError, .sessionRequired(3))
        }
    }

    func testQuotedEscapes() throws {
        let place = Place(
            id: UUID(uuidString: "11111111-1111-1111-1111-111111111111")!,
            user: "a\"b",
            host: "host",
            backend: .screen,
            session: "s",
            label: "line\nbreak"
        )
        let decoded = try CatalogCodec.decode(
            CatalogCodec.encode(CatalogDocument(version: 1, places: [place]))
        )
        XCTAssertEqual(decoded.places[0], place)
    }

    func testLocalPlaceOmitsUserAndHost() throws {
        let place = Place(
            id: UUID(uuidString: "11111111-1111-1111-1111-111111111111")!,
            user: "",
            host: "",
            backend: .herdr
        )
        let encoded = CatalogCodec.encode(CatalogDocument(version: 1, places: [place]))
        XCTAssertFalse(encoded.contains("user ="))
        XCTAssertFalse(encoded.contains("host ="))
        let decoded = try CatalogCodec.decode(encoded)
        XCTAssertTrue(decoded.places[0].isLocal)
        XCTAssertEqual(decoded.places[0].user, "")
        XCTAssertEqual(decoded.places[0].host, "")
    }

    func testRemotePlaceStillRequiresUser() {
        let text = """
        version = 1

        [[place]]
        id = "11111111-1111-1111-1111-111111111111"
        host = "riverwood"
        backend = "herdr"
        """
        XCTAssertThrowsError(try CatalogCodec.decode(text)) { error in
            XCTAssertEqual(error as? CatalogCodecError, .missingField("user", 3))
        }
    }

    func testRejectsUnknownVersion() {
        XCTAssertThrowsError(try CatalogCodec.decode("version = 2\n")) { error in
            XCTAssertEqual(error as? CatalogCodecError, .unsupportedVersion(2))
        }
    }
}
