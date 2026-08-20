import XCTest
@testable import rterm

final class DriverTests: XCTestCase {
    func testHerdrUsesLoginShellOverSSH() {
        let place = Place(user: "ada", host: "riverwood", backend: .herdr)
        XCTAssertEqual(Driver.sshExecutable, "/usr/bin/ssh")
        XCTAssertEqual(
            Driver.sshArguments(for: place),
            ["-t", "ada@riverwood", "exec \"$SHELL\" -lc 'herdr'"]
        )
    }

    func testTmuxQuotesSessionName() {
        let place = Place(
            user: "ada",
            host: "whiterun",
            backend: .tmux,
            session: "foo bar"
        )
        XCTAssertEqual(
            Driver.remoteCommand(for: place),
            "tmux new-session -A -s 'foo bar'"
        )
        XCTAssertEqual(
            Driver.sshArguments(for: place).last,
            "exec \"$SHELL\" -lc 'tmux new-session -A -s '\\''foo bar'\\'''"
        )
    }

    func testScreenCommand() {
        let place = Place(user: "ada", host: "phone", backend: .screen, session: "foo")
        XCTAssertEqual(Driver.remoteCommand(for: place), "screen -d -R 'foo'")
    }
}
