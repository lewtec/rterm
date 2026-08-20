import AppKit
import SwiftTerm
import SwiftUI

struct AttachPane: NSViewRepresentable {
    var place: Place
    var onExit: (Int32?) -> Void

    func makeCoordinator() -> Coordinator {
        Coordinator(onExit: onExit)
    }

    func makeNSView(context: Context) -> LocalProcessTerminalView {
        let view = LocalProcessTerminalView(frame: .zero)
        view.processDelegate = context.coordinator
        view.startProcess(executable: Driver.sshExecutable, args: Driver.sshArguments(for: place))
        return view
    }

    func updateNSView(_ nsView: LocalProcessTerminalView, context: Context) {
        context.coordinator.onExit = onExit
        nsView.processDelegate = context.coordinator
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
