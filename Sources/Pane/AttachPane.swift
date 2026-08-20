import AppKit
import SwiftTerm
import SwiftUI

struct AttachPane: NSViewRepresentable {
    var place: Place
    var active: Bool
    var onExit: (Int32?) -> Void

    func makeCoordinator() -> Coordinator {
        Coordinator(onExit: onExit)
    }

    func makeNSView(context: Context) -> LocalProcessTerminalView {
        let view = LocalProcessTerminalView(frame: .zero)
        view.processDelegate = context.coordinator
        let launch = Driver.launch(for: place)
        view.startProcess(
            executable: launch.executable,
            args: launch.arguments,
            currentDirectory: launch.currentDirectory
        )
        view.isHidden = !active
        return view
    }

    func updateNSView(_ nsView: LocalProcessTerminalView, context: Context) {
        context.coordinator.onExit = onExit
        nsView.processDelegate = context.coordinator
        if nsView.isHidden == active {
            if !active, nsView.window?.firstResponder === nsView {
                nsView.window?.makeFirstResponder(nil)
            }
            nsView.isHidden = !active
        }
    }

    static func dismantleNSView(_ nsView: LocalProcessTerminalView, coordinator: Coordinator) {
        nsView.terminate()
    }

    final class Coordinator: LocalProcessTerminalViewDelegate {
        var onExit: (Int32?) -> Void

        init(onExit: @escaping (Int32?) -> Void) {
            self.onExit = onExit
        }

        func sizeChanged(source: LocalProcessTerminalView, newCols: Int, newRows: Int) {}

        func setTerminalTitle(source: LocalProcessTerminalView, title: String) {}

        func hostCurrentDirectoryUpdate(source: TerminalView, directory: String?) {}

        func processTerminated(source: TerminalView, exitCode: Int32?) {
            onExit(exitCode)
        }
    }
}
