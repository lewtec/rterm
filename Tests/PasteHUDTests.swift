import XCTest
@testable import rterm

final class PasteHUDTests: XCTestCase {
    func testWorkingCopyDependsOnPlace() {
        XCTAssertEqual(
            PasteHUD.working(isLocal: true, total: 10),
            .working("Writing image…", sent: 0, total: 10)
        )
        XCTAssertEqual(
            PasteHUD.working(isLocal: false, sent: 4, total: 10),
            .working("Uploading image…", sent: 4, total: 10)
        )
    }

    func testFailureCopy() {
        XCTAssertEqual(PasteHUD.failure, .failed("Paste failed"))
        XCTAssertNil(PasteHUD.failure.fraction)
    }

    func testFractionAndPercent() {
        XCTAssertEqual(PasteHUD.working(isLocal: false, sent: 1, total: 4).fraction, 0.25)
        XCTAssertEqual(PasteHUD.working(isLocal: false, sent: 1, total: 3).percent, 33)
        XCTAssertEqual(PasteHUD.working(isLocal: false, sent: 10, total: 10).percent, 100)
        XCTAssertEqual(PasteHUD.working(isLocal: false, total: 0).fraction, 0)
    }

    func testAccessibilityIncludesPercent() {
        XCTAssertEqual(
            PasteHUD.working(isLocal: false, sent: 1, total: 2).accessibilityText,
            "Uploading image… 50 percent"
        )
        XCTAssertEqual(PasteHUD.failure.accessibilityText, "Paste failed")
    }
}
