import XCTest
@testable import rterm

final class DriverTests: XCTestCase {
    func testRemoteTmuxUsesSSHKeepalives() {
        let place = Place(user: "ada", host: "whiterun", backend: .tmux, session: "foo")
        XCTAssertEqual(Driver.launch(for: place).executable, Driver.sshExecutable)
        XCTAssertEqual(
            Driver.sshArguments(for: place),
            [
                "-t",
                "-o", "ServerAliveInterval=5",
                "-o", "ServerAliveCountMax=2",
                "ada@whiterun",
                "exec \"$SHELL\" -lc 'tmux new-session -A -s '\\''foo'\\'''",
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
        XCTAssertEqual(launch.arguments, ["-lc", Driver.herdrAttachScript])
        XCTAssertEqual(place.displayLabel, "localhost")
    }

    func testRemoteHerdrUsesSSHAndServerBinary() {
        let place = Place(user: "ada", host: "riverwood", backend: .herdr)
        let launch = Driver.launch(for: place)
        XCTAssertEqual(launch.executable, Driver.sshExecutable)
        XCTAssertTrue(Driver.remoteCommand(for: place).contains("/proc/"))
        XCTAssertTrue(Driver.remoteCommand(for: place).contains("exec herdr"))
        XCTAssertFalse(Driver.remoteCommand(for: place).contains("--remote"))
        XCTAssertEqual(launch.arguments.prefix(5), ["-t", "-o", "ServerAliveInterval=5", "-o", "ServerAliveCountMax=2"])
        XCTAssertEqual(launch.arguments[5], "ada@riverwood")
    }

    func testLocalhostIsLocalNotUserAtLocalhost() {
        let place = Place(user: "ada", host: "localhost", backend: .herdr)
        XCTAssertTrue(place.isLocal)
        XCTAssertEqual(place.displayLabel, "ada@localhost")
        XCTAssertEqual(Driver.launch(for: place).executable, Driver.localShell)
        XCTAssertNotEqual(Driver.launch(for: place).executable, Driver.sshExecutable)
    }

    func testLoopbackLiteralsAreLocal() {
        XCTAssertTrue(Place(user: "ada", host: "127.0.0.1", backend: .herdr).isLocal)
        XCTAssertTrue(Place(user: "ada", host: "::1", backend: .herdr).isLocal)
        XCTAssertTrue(Place(user: "ada", host: "[::1]", backend: .herdr).isLocal)
        XCTAssertFalse(Place(user: "ada", host: "riverwood", backend: .herdr).isLocal)
    }

    func testEmptyHostTmuxLabel() {
        let place = Place(user: "", host: "  ", backend: .tmux, session: "dev")
        XCTAssertTrue(place.isLocal)
        XCTAssertEqual(place.displayLabel, "localhost:tmux(dev)")
        XCTAssertEqual(Driver.launch(for: place).arguments, ["-lc", "tmux new-session -A -s 'dev'"])
    }
}
