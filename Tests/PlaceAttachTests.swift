import XCTest
@testable import rterm

final class PlaceAttachTests: XCTestCase {
    func testIdleSelectGoesAttaching() {
        var attach = PlaceAttach()
        attach.select()
        XCTAssertEqual(attach.phase, .attaching(PlaceAttach.connectingTitle))
        XCTAssertEqual(attach.connectHeadline, PlaceAttach.connectingTitle)
        XCTAssertTrue(attach.showsPane)
        XCTAssertTrue(attach.isConnecting)
        XCTAssertEqual(attach.generation, 0)
        XCTAssertEqual(attach.connectAttempt, 1)
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

    func testConnectingHeadline() {
        XCTAssertTrue(PlaceAttach.isConnectingHeadline(PlaceAttach.connectingTitle))
        XCTAssertTrue(PlaceAttach.isConnectingHeadline("Connecting to whiterun"))
        XCTAssertFalse(PlaceAttach.isConnectingHeadline("Connected"))
        XCTAssertFalse(PlaceAttach.isConnectingHeadline("Authenticating to whiterun"))
        XCTAssertFalse(PlaceAttach.isConnectingHeadline("Offering key"))
    }

    func testConnectTimeoutRetriesThenGivesUp() {
        var attach = PlaceAttach()
        attach.select()
        XCTAssertEqual(attach.connectAttempt, 1)

        attach.noteConnectTimeout()
        XCTAssertEqual(attach.phase, .attaching(PlaceAttach.connectingTitle))
        XCTAssertEqual(attach.connectAttempt, 2)
        XCTAssertEqual(attach.generation, 1)

        attach.noteProgress("Connecting to whiterun")
        attach.noteConnectTimeout()
        XCTAssertEqual(attach.phase, .attaching(PlaceAttach.connectingTitle))
        XCTAssertEqual(attach.connectAttempt, 3)
        XCTAssertEqual(attach.generation, 2)

        attach.noteConnectTimeout()
        XCTAssertEqual(attach.phase, .failed(PlaceAttach.timedOutMessage))
        XCTAssertEqual(attach.generation, 2)
        XCTAssertFalse(attach.showsPane)
        XCTAssertFalse(attach.isConnecting)

        attach.noteConnectTimeout()
        XCTAssertEqual(attach.phase, .failed(PlaceAttach.timedOutMessage))
        attach.handleExit(255)
        XCTAssertEqual(attach.phase, .failed(PlaceAttach.timedOutMessage))
    }

    func testProgressDoesNotRegressToConnectingTitle() {
        var attach = PlaceAttach()
        attach.select()
        attach.noteProgress("Connecting to whiterun")
        attach.noteProgress(PlaceAttach.connectingTitle)
        XCTAssertEqual(attach.phase, .attaching("Connecting to whiterun"))
    }

    func testConnectTimeoutIgnoredAfterConnecting() {
        var attach = PlaceAttach()
        attach.select()
        attach.noteProgress("Connected")
        XCTAssertFalse(attach.isConnecting)
        attach.noteConnectTimeout()
        XCTAssertEqual(attach.phase, .attaching("Connected"))
        XCTAssertEqual(attach.generation, 0)
        XCTAssertEqual(attach.connectAttempt, 1)

        attach.noteProgress("Offering key")
        attach.noteConnectTimeout()
        XCTAssertEqual(attach.phase, .attaching("Offering key"))
    }

    func testStaleExitDoesNotClobberRetry() {
        var attach = PlaceAttach()
        attach.select()
        attach.noteConnectTimeout()
        XCTAssertEqual(attach.generation, 1)
        attach.handleExit(255, generation: 0)
        XCTAssertEqual(attach.phase, .attaching(PlaceAttach.connectingTitle))
        attach.handleExit(255, generation: 1)
        XCTAssertEqual(attach.phase, .failed("exited (255)"))
    }

    func testRefreshRemountsWithoutResettingAttempts() {
        var attach = PlaceAttach()
        attach.select()
        attach.noteConnectTimeout()
        XCTAssertEqual(attach.connectAttempt, 2)
        XCTAssertEqual(attach.generation, 1)
        attach.refresh()
        XCTAssertEqual(attach.generation, 2)
        XCTAssertEqual(attach.connectAttempt, 2)
        XCTAssertEqual(attach.phase, .attaching(PlaceAttach.connectingTitle))
    }

    func testCrumbsSurviveReconnectAndClearOnFreshSelect() {
        var attach = PlaceAttach()
        attach.select()
        attach.noteCrumb("Connecting to whiterun")
        attach.noteCrumb("Connection established.")
        XCTAssertEqual(attach.crumbs.count, 2)
        XCTAssertTrue(attach.showsConnectOverlay)

        attach.reconnect()
        XCTAssertEqual(attach.crumbs, ["Connecting to whiterun", "Connection established."])
        XCTAssertTrue(attach.showsConnectOverlay)

        attach.hideConnectOverlay()
        XCTAssertFalse(attach.showsConnectOverlay)
        attach.reconnect()
        XCTAssertTrue(attach.showsConnectOverlay)
        XCTAssertEqual(attach.crumbs.count, 2)

        attach.sleep()
        attach.select()
        XCTAssertEqual(attach.crumbs, [])
        XCTAssertTrue(attach.showsConnectOverlay)
    }

    func testReconnectResetsConnectAttempts() {
        var attach = PlaceAttach()
        attach.select()
        attach.noteConnectTimeout()
        attach.noteConnectTimeout()
        attach.noteConnectTimeout()
        XCTAssertEqual(attach.phase, .failed(PlaceAttach.timedOutMessage))
        attach.reconnect()
        XCTAssertEqual(attach.connectAttempt, 1)
        XCTAssertTrue(attach.isConnecting)
    }
}
