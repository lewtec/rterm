import XCTest
@testable import rterm

final class ImagePasteTests: XCTestCase {
    func testPlanIgnoresTextEvenWhenImagePresent() {
        XCTAssertEqual(
            ImagePaste.plan(hasImage: true, hasText: true, isLocal: false, gesture: .commandV),
            .ignore
        )
        XCTAssertEqual(
            ImagePaste.plan(hasImage: true, hasText: true, isLocal: false, gesture: .controlV),
            .ignore
        )
    }

    func testPlanDeliversImageOnlyCommandVOnBothPlaces() {
        XCTAssertEqual(
            ImagePaste.plan(hasImage: true, hasText: false, isLocal: true, gesture: .commandV),
            .deliver
        )
        XCTAssertEqual(
            ImagePaste.plan(hasImage: true, hasText: false, isLocal: false, gesture: .commandV),
            .deliver
        )
    }

    func testPlanControlVOnlyOnRemote() {
        XCTAssertEqual(
            ImagePaste.plan(hasImage: true, hasText: false, isLocal: true, gesture: .controlV),
            .ignore
        )
        XCTAssertEqual(
            ImagePaste.plan(hasImage: true, hasText: false, isLocal: false, gesture: .controlV),
            .deliver
        )
    }

    func testHostPathUsesPasteID() {
        let id = UUIDV7.generate()
        XCTAssertEqual(ImagePaste.hostPath(id: id), "/tmp/rterm-paste-\(id.uuidString).png")
    }

    func testDefaultPasteIDIsVersion7() {
        let now = Date(timeIntervalSince1970: 1_700_000_000)
        let id = UUIDV7.generate(now: now)
        XCTAssertEqual(UUIDV7.version(of: id), 7)
        XCTAssertEqual(id.uuid.8 >> 6, 0b10)
        XCTAssertEqual(
            UInt64(id.uuid.0) << 40
                | UInt64(id.uuid.1) << 32
                | UInt64(id.uuid.2) << 24
                | UInt64(id.uuid.3) << 16
                | UInt64(id.uuid.4) << 8
                | UInt64(id.uuid.5),
            1_700_000_000_000
        )
    }

    func testLocalDeliverWritesPNG() throws {
        let id = UUID()
        let png = Data("png-bytes".utf8)
        let place = Place(user: "ada", host: "", backend: .herdr)
        let result = ImagePaste.deliver(png: png, place: place, pasteID: id)
        let path = try result.get()
        defer { try? FileManager.default.removeItem(atPath: path) }
        XCTAssertEqual(path, ImagePaste.hostPath(id: id))
        XCTAssertEqual(try Data(contentsOf: URL(fileURLWithPath: path)), png)
    }

    func testRejectsOversizedPNG() {
        let png = Data(count: ImagePaste.maxBytes + 1)
        let place = Place(user: "ada", host: "", backend: .herdr)
        XCTAssertEqual(ImagePaste.deliver(png: png, place: place), .failure(.tooLarge))
    }

    func testRemoteDeliverFailsWithoutControlSocket() {
        let place = Place(user: "ada", host: "whiterun", backend: .herdr)
        let png = Data("png".utf8)
        XCTAssertEqual(
            ImagePaste.deliver(png: png, place: place) { _, _ in
                XCTFail("must not upload without a control socket")
            },
            .failure(.uploadFailed)
        )
    }

    func testRemoteDeliverUploadsWhenSocketExists() throws {
        let place = Place(user: "ada", host: "whiterun", backend: .herdr)
        let socket = URL(fileURLWithPath: Driver.controlPath(for: place))
        Driver.prepareControlSocketDirectory()
        FileManager.default.createFile(atPath: socket.path, contents: Data())
        defer { try? FileManager.default.removeItem(at: socket) }

        let id = UUID()
        let png = Data("png".utf8)
        var uploaded: (Data, [String])?
        let result = ImagePaste.deliver(png: png, place: place, pasteID: id) { data, arguments in
            uploaded = (data, arguments)
        }
        XCTAssertEqual(try result.get(), ImagePaste.hostPath(id: id))
        XCTAssertEqual(uploaded?.0, png)
        XCTAssertEqual(uploaded?.1, Driver.uploadArguments(for: place, remotePath: ImagePaste.hostPath(id: id)))
    }
}
