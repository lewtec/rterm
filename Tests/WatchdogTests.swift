import XCTest
@testable import rterm

final class WatchdogTests: XCTestCase {
    func testFiresAfterInterval() {
        let fired = expectation(description: "fired")
        let dog = Watchdog(interval: 0.05) {
            fired.fulfill()
        }
        dog.pet()
        wait(for: [fired], timeout: 0.4)
    }

    func testPetDelaysFire() {
        let fired = expectation(description: "fired")
        let dog = Watchdog(interval: 0.08) {
            fired.fulfill()
        }
        dog.pet()
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.04) {
            dog.pet()
        }
        wait(for: [fired], timeout: 0.5)
    }

    func testCancelPreventsFire() {
        var count = 0
        let dog = Watchdog(interval: 0.05) {
            count += 1
        }
        dog.pet()
        dog.cancel()
        let waited = expectation(description: "waited")
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.12) {
            waited.fulfill()
        }
        wait(for: [waited], timeout: 0.4)
        XCTAssertEqual(count, 0)
    }
}
