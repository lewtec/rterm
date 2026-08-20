import XCTest
@testable import rterm

final class PlaceAttachTests: XCTestCase {
    func testIdleSelectGoesLive() {
        var attach = PlaceAttach()
        attach.select()
        XCTAssertEqual(attach.phase, .live)
        XCTAssertTrue(attach.showsPane)
        XCTAssertEqual(attach.generation, 0)
    }

    func testLiveSelectDoesNotBumpGeneration() {
        var attach = PlaceAttach()
        attach.select()
        attach.select()
        XCTAssertEqual(attach.phase, .live)
        XCTAssertEqual(attach.generation, 0)
    }

    func testReconnectFromIdleAndFailed() {
        var attach = PlaceAttach()
        attach.reconnect()
        XCTAssertEqual(attach.phase, .live)
        XCTAssertEqual(attach.generation, 1)

        attach.handleExit(255)
        XCTAssertEqual(attach.phase, .failed("exited (255)"))
        XCTAssertFalse(attach.showsPane)

        attach.reconnect()
        XCTAssertEqual(attach.phase, .live)
        XCTAssertEqual(attach.generation, 2)
        XCTAssertTrue(attach.showsPane)
    }

    func testCleanExitGoesIdle() {
        var attach = PlaceAttach()
        attach.select()
        attach.handleExit(0)
        XCTAssertEqual(attach.phase, .idle)
        XCTAssertFalse(attach.showsPane)
    }

    func testFailedExitsDoNotShowPane() {
        var died = PlaceAttach()
        died.select()
        died.handleExit(nil)
        XCTAssertEqual(died.phase, .failed("process died"))
        XCTAssertFalse(died.showsPane)

        var failed = PlaceAttach()
        failed.select()
        failed.handleExit(255)
        XCTAssertEqual(failed.phase, .failed("exited (255)"))
        XCTAssertFalse(failed.showsPane)
    }

    func testSleepFromLiveAndFailedKeepsGeneration() {
        var attach = PlaceAttach()
        attach.reconnect()
        XCTAssertEqual(attach.generation, 1)
        attach.sleep()
        XCTAssertEqual(attach.phase, .idle)
        XCTAssertEqual(attach.generation, 1)

        attach.select()
        attach.handleExit(255)
        attach.sleep()
        XCTAssertEqual(attach.phase, .idle)
        XCTAssertEqual(attach.generation, 1)
    }
}
