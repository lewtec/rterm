import XCTest
@testable import rterm

final class DriverTests: XCTestCase {
    func testHerdrUsesLoginShellOverSSH() {
        let place = Place(user: "ada", host: "riverwood", backend: .herdr)
        XCTAssertEqual(Driver.sshExecutable, "/usr/bin/ssh")
        XCTAssertEqual(
            Driver.sshArguments(for: place),
            [
                "-t",
                "-o", "ServerAliveInterval=5",
                "-o", "ServerAliveCountMax=2",
                "ada@riverwood",
                "exec \"$SHELL\" -lc 'herdr'",
            ]
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

    func testEmptyHostIsLocalLoginShell() {
        let place = Place(user: "", host: "", backend: .herdr)
        let launch = Driver.launch(for: place)
        XCTAssertEqual(launch.executable, Driver.localShell)
        XCTAssertEqual(launch.arguments, ["-lc", "herdr"])
        XCTAssertEqual(place.displayLabel, "herdr")
    }

    func testEmptyHostTmuxLabel() {
        let place = Place(user: "", host: "  ", backend: .tmux, session: "dev")
        XCTAssertTrue(place.isLocal)
        XCTAssertEqual(place.displayLabel, "tmux(dev)")
        XCTAssertEqual(Driver.launch(for: place).arguments, ["-lc", "tmux new-session -A -s 'dev'"])
    }
}
