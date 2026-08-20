import XCTest
@testable import rterm

final class SSHConnectTraceTests: XCTestCase {
    func testSendingCommandKeepsWaitingForTTY() {
        XCTAssertEqual(
            SSHConnectTrace.event(for: "debug1: Sending command: exec \"$SHELL\" -lc herdr"),
            .progress("Waiting for remote")
        )
    }

    func testEnteringInteractiveSessionIsNotReady() {
        XCTAssertNil(
            SSHConnectTrace.event(for: "debug1: Entering interactive session.")
        )
    }

    func testPasswordRevealsTerminal() {
        XCTAssertEqual(
            SSHConnectTrace.event(for: "debug1: Next authentication method: password"),
            .revealTerminal
        )
        XCTAssertEqual(
            SSHConnectTrace.event(for: "debug1: Next authentication method: keyboard-interactive"),
            .revealTerminal
        )
    }

    func testHostKeyPromptRevealsTerminal() {
        XCTAssertEqual(
            SSHConnectTrace.event(
                for: "The authenticity of host 'whiterun (10.0.0.2)' can't be established."
            ),
            .revealTerminal
        )
    }

    func testProgressHeadlines() {
        XCTAssertEqual(
            SSHConnectTrace.event(for: "debug1: Connecting to whiterun [10.0.0.2] port 22."),
            .progress("Connecting to whiterun")
        )
        XCTAssertEqual(
            SSHConnectTrace.event(for: "debug1: Connection established."),
            .progress("Connected")
        )
        XCTAssertEqual(
            SSHConnectTrace.event(for: "debug1: Authenticating to whiterun:22 as 'ada'"),
            .progress("Authenticating to whiterun:22")
        )
        XCTAssertEqual(
            SSHConnectTrace.event(
                for: "Authenticated to whiterun ([10.0.0.2]:22) using \"publickey\"."
            ),
            .progress("Authenticated to whiterun")
        )
        XCTAssertEqual(
            SSHConnectTrace.event(
                for: "ssh: Could not resolve hostname nosuch: nodename nor servname provided, or not known"
            ),
            .progress("Could not resolve hostname nosuch: nodename nor servname provided, or not known")
        )
    }

    func testCrumbStripsDebugPrefix() {
        XCTAssertEqual(
            SSHConnectTrace.crumb(for: "debug1: Offering public key: id_ed25519"),
            "Offering public key: id_ed25519"
        )
        XCTAssertNil(SSHConnectTrace.crumb(for: "   "))
    }
}
