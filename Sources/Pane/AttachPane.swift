import AppKit
import SwiftTerm
import SwiftUI

struct AttachPane: View {
    var place: Place
    var generation: Int
    var active: Bool
    var onExit: (Int32?) -> Void
    var onTTY: () -> Void
    var onProgress: (String) -> Void
    var onTimeout: () -> Void

    @StateObject private var progress = ConnectProgress()

    var body: some View {
        ZStack {
            AttachTerminal(
                place: place,
                generation: generation,
                active: active,
                progress: progress,
                onExit: onExit,
                onTTY: onTTY,
                onProgress: onProgress,
                onTimeout: onTimeout
            )
            if active, progress.isVisible {
                ConnectOverlay(progress: progress)
            }
        }
    }
}

private struct AttachTerminal: NSViewRepresentable {
    var place: Place
    var generation: Int
    var active: Bool
    var progress: ConnectProgress
    var onExit: (Int32?) -> Void
    var onTTY: () -> Void
    var onProgress: (String) -> Void
    var onTimeout: () -> Void

    func makeCoordinator() -> Coordinator {
        Coordinator(onExit: onExit, progress: progress, onProgress: onProgress, onTimeout: onTimeout)
    }

    func makeNSView(context: Context) -> FillTerminalView {
        let view = FillTerminalView(frame: .zero)
        view.processDelegate = context.coordinator
        let logURL = place.isLocal ? nil : Driver.verboseLogURL(placeID: place.id, generation: generation)
        context.coordinator.logURL = logURL
        context.coordinator.onProgress = onProgress
        context.coordinator.onTimeout = onTimeout
        progress.onHeadline = onProgress
        context.coordinator.startConnectWatchdog()
        if logURL != nil {
            context.coordinator.startWatch()
        }
        let launch = Driver.launch(for: place, verboseLog: logURL?.path)
        let progress = progress
        let onTTY = onTTY
        view.onTTYOutput = {
            onTTY()
            context.coordinator.cancelConnectWatchdog()
            Task { @MainActor in
                progress.finish()
            }
        }
        view.startProcess(
            executable: launch.executable,
            args: launch.arguments,
            currentDirectory: launch.currentDirectory
        )
        view.isHidden = !active
        view.setContentHuggingPriority(.defaultLow, for: .horizontal)
        view.setContentHuggingPriority(.defaultLow, for: .vertical)
        view.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        view.setContentCompressionResistancePriority(.defaultLow, for: .vertical)
        return view
    }

    func updateNSView(_ nsView: FillTerminalView, context: Context) {
        context.coordinator.onExit = onExit
        context.coordinator.onProgress = onProgress
        context.coordinator.onTimeout = onTimeout
        let progress = progress
        Task { @MainActor in
            progress.onHeadline = onProgress
        }
        nsView.processDelegate = context.coordinator
        if nsView.isHidden == active {
            if !active, nsView.window?.firstResponder === nsView {
                nsView.window?.makeFirstResponder(nil)
            }
            nsView.isHidden = !active
        }
    }

    static func dismantleNSView(_ nsView: FillTerminalView, coordinator: Coordinator) {
        coordinator.stopWatch()
        nsView.terminate()
    }

    final class Coordinator: LocalProcessTerminalViewDelegate {
        var onExit: (Int32?) -> Void
        let progress: ConnectProgress
        var logURL: URL?
        var onTimeout: () -> Void
        private var timer: Timer?
        private var handle: FileHandle?
        private var leftover = Data()
        private var watchdog: Watchdog?

        var onProgress: (String) -> Void

        init(
            onExit: @escaping (Int32?) -> Void,
            progress: ConnectProgress,
            onProgress: @escaping (String) -> Void,
            onTimeout: @escaping () -> Void
        ) {
            self.onExit = onExit
            self.progress = progress
            self.onProgress = onProgress
            self.onTimeout = onTimeout
        }

        func startConnectWatchdog() {
            let dog = Watchdog(interval: PlaceAttach.connectTimeoutSeconds) { [weak self] in
                self?.onTimeout()
            }
            watchdog = dog
            dog.pet()
        }

        func cancelConnectWatchdog() {
            watchdog?.cancel()
            watchdog = nil
        }

        func startWatch() {
            guard let logURL else {
                return
            }
            FileManager.default.createFile(atPath: logURL.path, contents: nil)
            handle = try? FileHandle(forReadingFrom: logURL)
            let progress = progress
            Task { @MainActor in
                progress.beginRemote()
            }
            let timer = Timer(timeInterval: 0.05, repeats: true) { [weak self] _ in
                self?.pump()
            }
            RunLoop.main.add(timer, forMode: .common)
            self.timer = timer
        }

        func stopWatch() {
            cancelConnectWatchdog()
            timer?.invalidate()
            timer = nil
            try? handle?.close()
            handle = nil
            leftover = Data()
            let progress = progress
            Task { @MainActor in
                progress.finish()
            }
            if let logURL {
                try? FileManager.default.removeItem(at: logURL)
            }
        }

        private func pump() {
            guard let handle else {
                return
            }
            let chunk = handle.availableData
            guard !chunk.isEmpty else {
                return
            }
            leftover.append(chunk)
            let newline = Data([0x0A])
            while let range = leftover.range(of: newline) {
                let lineData = leftover.subdata(in: leftover.startIndex..<range.lowerBound)
                leftover.removeSubrange(leftover.startIndex..<range.upperBound)
                if let line = String(data: lineData, encoding: .utf8) {
                    watchdog?.pet()
                    let progress = progress
                    Task { @MainActor in
                        progress.ingest(line)
                    }
                }
            }
        }

        func sizeChanged(source: LocalProcessTerminalView, newCols: Int, newRows: Int) {}

        func setTerminalTitle(source: LocalProcessTerminalView, title: String) {}

        func hostCurrentDirectoryUpdate(source: TerminalView, directory: String?) {}

        func processTerminated(source: TerminalView, exitCode: Int32?) {
            stopWatch()
            onExit(exitCode)
        }
    }
}

/// Overlay scrollers still reserve a legacy gutter, which shows up as a black
/// strip. Hide that gutter so the cell grid can use the full view.
final class FillTerminalView: LocalProcessTerminalView {
    var onTTYOutput: (() -> Void)?
    private var announcedTTY = false

    override func dataReceived(slice: ArraySlice<UInt8>) {
        super.dataReceived(slice: slice)
        guard !announcedTTY, !slice.isEmpty else {
            return
        }
        announcedTTY = true
        onTTYOutput?()
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        hideReservedScroller()
    }

    override func setFrameSize(_ newSize: NSSize) {
        hideReservedScroller()
        super.setFrameSize(newSize)
    }

    private func hideReservedScroller() {
        for subview in subviews where subview is NSScroller && !subview.isHidden {
            subview.isHidden = true
        }
    }
}
