import XCTest
@testable import rterm

final class AttachExitTests: XCTestCase {
    func testZeroIsCleanDetach() {
        XCTAssertEqual(AttachExit.classify(0), .clean)
    }

    func testNilAndNonZeroAreFailures() {
        XCTAssertEqual(AttachExit.classify(nil), .failed("process died"))
        XCTAssertEqual(AttachExit.classify(255), .failed("ssh exited (255)"))
    }
}
