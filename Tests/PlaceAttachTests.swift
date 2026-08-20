import XCTest
@testable import rterm

final class PlaceAttachTests: XCTestCase {
    func testIdleSelectGoesAttaching() {
        var attach = PlaceAttach()
        attach.select()
        XCTAssertEqual(attach.phase, .attaching(PlaceAttach.connectingTitle))
        XCTAssertEqual(attach.connectHeadline, PlaceAttach.connectingTitle)
        XCTAssertTrue(attach.showsPane)
        XCTAssertEqual(attach.generation, 0)
    }

    func testTTYPromotesAttachingToLive() {
        var attach = PlaceAttach()
        attach.select()
        attach.noteTTY()
        XCTAssertEqual(attach.phase, .live)
        XCTAssertTrue(attach.showsPane)
        attach.noteTTY()
        XCTAssertEqual(attach.phase, .live)
        XCTAssertEqual(attach.generation, 0)
    }

    func testLiveSelectDoesNotBumpGeneration() {
        var attach = PlaceAttach()
        attach.select()
        attach.noteTTY()
        attach.select()
        XCTAssertEqual(attach.phase, .live)
        XCTAssertEqual(attach.generation, 0)
    }

    func testReconnectFromIdleAndFailed() {
        var attach = PlaceAttach()
        attach.reconnect()
        XCTAssertEqual(attach.phase, .attaching(PlaceAttach.connectingTitle))
        XCTAssertEqual(attach.generation, 1)

        attach.handleExit(255)
        XCTAssertEqual(attach.phase, .failed("exited (255)"))
        XCTAssertFalse(attach.showsPane)

        attach.reconnect()
        XCTAssertEqual(attach.phase, .attaching(PlaceAttach.connectingTitle))
        XCTAssertEqual(attach.generation, 2)
        XCTAssertTrue(attach.showsPane)
    }

    func testProgressUpdatesAttachingTitle() {
        var attach = PlaceAttach()
        attach.select()
        attach.noteProgress("Authenticating to whiterun")
        XCTAssertEqual(attach.phase, .attaching("Authenticating to whiterun"))
        attach.noteProgress("Authenticating to whiterun")
        attach.noteTTY()
        attach.noteProgress("Waiting for remote")
        XCTAssertEqual(attach.phase, .live)
    }

    func testCleanExitGoesIdle() {
        var attach = PlaceAttach()
        attach.select()
        attach.handleExit(0)
        XCTAssertEqual(attach.phase, .idle)
        XCTAssertFalse(attach.showsPane)
    }

    func testExitFromAttachingFails() {
        var attach = PlaceAttach()
        attach.select()
        attach.handleExit(255)
        XCTAssertEqual(attach.phase, .failed("exited (255)"))
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
