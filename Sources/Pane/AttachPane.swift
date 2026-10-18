import AppKit
import os
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

    @State private var pasteHUD: PasteHUD?

    var body: some View {
        ZStack {
            AttachTerminal(
                place: place,
                generation: generation,
                active: active,
                progress: progress,
                onExit: onExit,
                onTTY: onTTY,
                onTimeout: onTimeout,
                onPasteHUD: { pasteHUD = $0 }
            )
            if progress.isVisible {
                ConnectOverlay(headline: progress.headline, crumbs: progress.crumbs)
                    .allowsHitTesting(false)
            }
            if let pasteHUD {
                PasteHUDView(state: pasteHUD)
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
    var onPasteHUD: (PasteHUD?) -> Void

    func makeCoordinator() -> Coordinator {
        Coordinator(onExit: onExit, progress: progress, onTimeout: onTimeout)
    }

    func makeNSView(context: Context) -> FillTerminalView {
        let view = FillTerminalView(frame: .zero)
        view.onPasteHUD = onPasteHUD
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
            progress.finish()
        }
        view.armTTYAnnouncement()
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
        nsView.onPasteHUD = onPasteHUD
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
        nsView.onPasteHUD = nil
        nsView.onTTYOutput = nil
        nsView.disarmTTYAnnouncement()
        nsView.terminate()
    }

    final class Coordinator: LocalProcessTerminalViewDelegate {
        var onExit: (Int32?) -> Void
        let progress: ConnectProgress
        var logURL: URL?
        var onTimeout: () -> Void
        private var logSource: DispatchSourceFileSystemObject?
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
            guard let handle else {
                return
            }
            let source = DispatchSource.makeFileSystemObjectSource(
                fileDescriptor: handle.fileDescriptor,
                eventMask: [.extend, .write],
                queue: .main
            )
            source.setEventHandler { [weak self] in
                self?.pump()
            }
            logSource = source
            source.resume()
            pump()
        }

        func stopWatch() {
            cancelConnectWatchdog()
            logSource?.cancel()
            logSource = nil
            try? handle?.close()
            handle = nil
            leftover = Data()
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

        func processFailedToStart(source: TerminalView, error _: LocalProcessError) {
            stopWatch()
            onExit(1)
        }
    }
}

/// First process-output batch. `note` runs on the parse thread and hops
/// the callback to the main thread.
private final class FirstOutput: Sendable {
    private let state = OSAllocatedUnfairLock<(@MainActor () -> Void)?>(initialState: nil)

    func arm(_ body: @escaping @MainActor () -> Void) {
        state.withLock { $0 = body }
    }

    func disarm() {
        state.withLock { $0 = nil }
    }

    func note() {
        let body = state.withLock { stored -> (@MainActor () -> Void)? in
            let body = stored
            stored = nil
            return body
        }
        guard let body else {
            return
        }
        Task { @MainActor in
            body()
        }
    }
}

final class FillTerminalView: LocalProcessTerminalView {
    var onTTYOutput: (() -> Void)?
    var onPasteHUD: ((PasteHUD?) -> Void)?
    var place = Place(user: "", host: "", backend: .herdr)
    private let firstOutput = FirstOutput()
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
        onPasteHUD?(nil)
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
            presentPasteFailure()
            return true
        }
        pasteGeneration += 1
        let generation = pasteGeneration
        let place = place
        let isLocal = place.isLocal
        onPasteHUD?(PasteHUD.working(isLocal: isLocal, total: png.count))
        Task { @MainActor [weak self] in
            let (stream, continuation) = AsyncStream<(Int, Int)>.makeStream()
            let hudTask = Task { @MainActor [weak self] in
                for await (sent, total) in stream {
                    guard let self, self.pasteGeneration == generation else {
                        continue
                    }
                    self.onPasteHUD?(PasteHUD.working(isLocal: isLocal, sent: sent, total: total))
                }
            }
            let result = await Task.detached {
                defer { continuation.finish() }
                return ImagePaste.deliver(png: png, place: place, onProgress: { sent, total in
                    continuation.yield((sent, total))
                })
            }.value
            continuation.finish()
            await hudTask.value
            self?.finishImagePaste(result, generation: generation)
        }
        return true
    }

    private func presentPasteFailure() {
        pasteGeneration += 1
        let generation = pasteGeneration
        NSSound.beep()
        onPasteHUD?(PasteHUD.failure)
        clearPasteHUD(after: PasteHUD.failClearNanoseconds, generation: generation)
    }

    private func finishImagePaste(_ result: Result<String, ImagePasteError>, generation: Int) {
        guard generation == pasteGeneration else {
            return
        }
        switch result {
        case .success(let path):
            onPasteHUD?(nil)
            pasteText(path)
        case .failure:
            NSSound.beep()
            onPasteHUD?(PasteHUD.failure)
            clearPasteHUD(after: PasteHUD.failClearNanoseconds, generation: generation)
        }
    }

    private func clearPasteHUD(after nanoseconds: UInt64, generation: Int) {
        Task { @MainActor [weak self] in
            try? await Task.sleep(nanoseconds: nanoseconds)
            guard let self, self.pasteGeneration == generation else {
                return
            }
            self.onPasteHUD?(nil)
        }
    }

    func refreshAfterReveal() {
        enableMetalIfNeeded()
        requestRedraw()
    }

    func armTTYAnnouncement() {
        firstOutput.arm { [weak self] in
            guard let self else { return }
            self.setProcessOutputHandler(nil)
            self.onTTYOutput?()
        }
        setProcessOutputHandler { [firstOutput] in
            firstOutput.note()
        }
    }

    func disarmTTYAnnouncement() {
        setProcessOutputHandler(nil)
        firstOutput.disarm()
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        if window == nil {
            removeKeyMonitor()
        } else {
            installKeyMonitor()
            enableMetalIfNeeded()
        }
    }

    private func enableMetalIfNeeded() {
        guard window != nil, !isUsingMetalRenderer else {
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
}
