import XCTest
@testable import rterm

@MainActor
final class ConnectProgressTests: XCTestCase {
    func testBeginClearsCrumbsWhenAsked() {
        let progress = ConnectProgress()
        progress.begin(clearCrumbs: true)
        progress.ingest("debug1: Connecting to whiterun [10.0.0.2] port 22.")
        XCTAssertEqual(progress.crumbs.count, 1)
        progress.begin(clearCrumbs: false)
        XCTAssertEqual(progress.crumbs.count, 1)
        XCTAssertTrue(progress.isVisible)
        progress.begin(clearCrumbs: true)
        XCTAssertEqual(progress.crumbs, [])
        XCTAssertEqual(progress.headline, PlaceAttach.connectingTitle)
    }

    func testIngestUpdatesHeadlineAndCapsCrumbs() {
        let progress = ConnectProgress()
        var headlines: [String] = []
        progress.onHeadline = { headlines.append($0) }
        progress.begin(clearCrumbs: true)
        progress.ingest("debug1: Connecting to whiterun [10.0.0.2] port 22.")
        progress.ingest("debug1: Connection established.")
        progress.ingest("debug1: Authenticating to whiterun:22 as 'ada'")
        XCTAssertEqual(progress.headline, "Authenticating to whiterun:22")
        XCTAssertEqual(progress.crumbs.count, 3)
        XCTAssertTrue(headlines.contains("Authenticating to whiterun:22"))
        for index in 0..<ConnectProgress.maxCrumbs {
            progress.ingest("debug1: extra crumb \(index)")
        }
        XCTAssertEqual(progress.crumbs.count, ConnectProgress.maxCrumbs)
    }

    func testRevealAndFinishHideOverlay() {
        let progress = ConnectProgress()
        progress.begin(clearCrumbs: true)
        progress.ingest("debug1: Next authentication method: password")
        XCTAssertFalse(progress.isVisible)
        progress.begin(clearCrumbs: false)
        progress.finish()
        XCTAssertFalse(progress.isVisible)
    }

    func testIngestIgnoredWhileHidden() {
        let progress = ConnectProgress()
        progress.ingest("debug1: Connecting to whiterun [10.0.0.2] port 22.")
        XCTAssertEqual(progress.crumbs, [])
        XCTAssertEqual(progress.headline, PlaceAttach.connectingTitle)
    }

    func testResetClearsAndHides() {
        let progress = ConnectProgress()
        progress.begin(clearCrumbs: true)
        progress.ingest("debug1: Connection established.")
        progress.reset()
        XCTAssertFalse(progress.isVisible)
        XCTAssertEqual(progress.crumbs, [])
        XCTAssertEqual(progress.headline, PlaceAttach.connectingTitle)
    }
}
