import AppKit
import SwiftTerm
import SwiftUI

struct AttachPane: View {
    var place: Place
    var generation: Int
    var active: Bool
    var progress: ConnectProgress
    var onExit: (Int32?) -> Void
    var onTTY: () -> Void
    var onTimeout: () -> Void

    var body: some View {
        ZStack {
            AttachTerminal(
                place: place,
                generation: generation,
                active: active,
                progress: progress,
                onExit: onExit,
                onTTY: onTTY,
                onTimeout: onTimeout
            )
            if progress.isVisible {
                ConnectOverlay(headline: progress.headline, crumbs: progress.crumbs)
                    .allowsHitTesting(false)
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
    var onTimeout: () -> Void

    func makeCoordinator() -> Coordinator {
        Coordinator(onExit: onExit, progress: progress, onTimeout: onTimeout)
    }

    func makeNSView(context: Context) -> FillTerminalView {
        let view = FillTerminalView(frame: .zero)
        view.processDelegate = context.coordinator
        let logURL = place.isLocal ? nil : Driver.verboseLogURL(placeID: place.id, generation: generation)
        context.coordinator.logURL = logURL
        context.coordinator.onTimeout = onTimeout
        context.coordinator.startConnectWatchdog()
        if logURL != nil {
            context.coordinator.startWatch()
        }
        let launch = Driver.launch(for: place, verboseLog: logURL?.path)
        view.place = place
        let onTTY = onTTY
        view.onTTYOutput = {
            onTTY()
            context.coordinator.stopWatch()
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
        context.coordinator.onTimeout = onTimeout
        nsView.processDelegate = context.coordinator
        nsView.place = place
        if nsView.isHidden == active {
            if !active, nsView.window?.firstResponder === nsView {
                nsView.window?.makeFirstResponder(nil)
            }
            nsView.isHidden = !active
            if active {
                nsView.refreshAfterReveal()
            }
        }
    }

    static func dismantleNSView(_ nsView: FillTerminalView, coordinator: Coordinator) {
        coordinator.stopWatch()
        nsView.cancelImagePaste()
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

        init(
            onExit: @escaping (Int32?) -> Void,
            progress: ConnectProgress,
            onTimeout: @escaping () -> Void
        ) {
            self.onExit = onExit
            self.progress = progress
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
    var place = Place(user: "", host: "", backend: .herdr)
    private var announcedTTY = false
    private var pasteGeneration = 0
    private var keyMonitor: Any?

    override func paste(_ sender: Any) {
        if startImagePaste(gesture: .commandV) {
            return
        }
        super.paste(sender)
    }

    func cancelImagePaste() {
        pasteGeneration += 1
        removeKeyMonitor()
    }

    @discardableResult
    private func startImagePaste(gesture: ImagePasteGesture) -> Bool {
        let snap = ImagePaste.snapshot(.general)
        guard ImagePaste.plan(
            hasImage: snap.hasImage,
            hasText: snap.hasText,
            isLocal: place.isLocal,
            gesture: gesture
        ) == .deliver, let png = snap.png else {
            return false
        }
        if png.count > ImagePaste.maxBytes {
            NSSound.beep()
            return true
        }
        pasteGeneration += 1
        let generation = pasteGeneration
        let place = place
        Task { @MainActor [weak self] in
            let result = await Task.detached {
                ImagePaste.deliver(png: png, place: place)
            }.value
            self?.finishImagePaste(result, generation: generation)
        }
        return true
    }

    private func finishImagePaste(_ result: Result<String, ImagePasteError>, generation: Int) {
        guard generation == pasteGeneration else {
            return
        }
        switch result {
        case .success(let path):
            pastePath(path)
        case .failure:
            NSSound.beep()
        }
    }

    private func pastePath(_ path: String) {
        if terminal.bracketedPasteMode {
            send(data: Self.bracketedPasteStart[...])
            send(txt: path)
            send(data: Self.bracketedPasteEnd[...])
        } else {
            send(txt: path)
        }
    }

    private static let bracketedPasteStart: [UInt8] = [0x1b, 0x5b, 0x32, 0x30, 0x30, 0x7e]
    private static let bracketedPasteEnd: [UInt8] = [0x1b, 0x5b, 0x32, 0x30, 0x31, 0x7e]

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        hideReservedScroller()
        if window == nil {
            removeKeyMonitor()
        } else {
            installKeyMonitor()
            if !isHidden {
                enableMetalIfNeeded()
            }
        }
    }

    override func setNeedsDisplay(_ invalidRect: NSRect) {
        if isHidden {
            return
        }
        super.setNeedsDisplay(invalidRect)
    }

    override func dataReceived(slice: ArraySlice<UInt8>) {
        if isHidden {
            terminal.feed(buffer: slice)
        } else {
            super.dataReceived(slice: slice)
        }
        guard !announcedTTY, !slice.isEmpty else {
            return
        }
        announcedTTY = true
        onTTYOutput?()
    }

    func refreshAfterReveal() {
        let hadMetal = isUsingMetalRenderer
        enableMetalIfNeeded()
        if isUsingMetalRenderer, !hadMetal {
            terminal.updateFullScreen()
            feed(byteArray: [])
            return
        }
        guard terminal.getUpdateRange() != nil else {
            return
        }
        feed(byteArray: [])
    }

    private func enableMetalIfNeeded() {
        guard !isUsingMetalRenderer else {
            return
        }
        try? setUseMetal(true)
    }

    private func installKeyMonitor() {
        guard keyMonitor == nil else {
            return
        }
        keyMonitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self] event in
            guard let self, self.window?.firstResponder === self else {
                return event
            }
            if ImagePaste.isControlV(event), self.startImagePaste(gesture: .controlV) {
                return nil
            }
            return event
        }
    }

    private func removeKeyMonitor() {
        if let keyMonitor {
            NSEvent.removeMonitor(keyMonitor)
            self.keyMonitor = nil
        }
    }

    override func setFrameSize(_ newSize: NSSize) {
        hideReservedScroller()
        guard newSize != frame.size else {
            return
        }
        super.setFrameSize(newSize)
    }

    private func hideReservedScroller() {
        for subview in subviews where subview is NSScroller && !subview.isHidden {
            subview.isHidden = true
        }
    }
}
