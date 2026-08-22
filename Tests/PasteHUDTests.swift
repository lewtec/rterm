import XCTest
@testable import rterm

final class PasteHUDTests: XCTestCase {
    func testWorkingCopyDependsOnPlace() {
        XCTAssertEqual(PasteHUD.working(isLocal: true), .working("Writing image…"))
        XCTAssertEqual(PasteHUD.working(isLocal: false), .working("Uploading image…"))
    }

    func testFailureCopy() {
        XCTAssertEqual(PasteHUD.failure, .failed("Paste failed"))
    }

    func testSpinnerOnlyWhileWorking() {
        XCTAssertTrue(PasteHUD.working(isLocal: true).showsSpinner)
        XCTAssertTrue(PasteHUD.working(isLocal: false).showsSpinner)
        XCTAssertFalse(PasteHUD.failure.showsSpinner)
    }

    func testMessageMatchesAssociatedText() {
        XCTAssertEqual(PasteHUD.working(isLocal: true).message, "Writing image…")
        XCTAssertEqual(PasteHUD.failure.message, "Paste failed")
    }
}
