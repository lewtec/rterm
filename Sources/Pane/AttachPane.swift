import AppKit
import MetalKit
@testable import SwiftTerm
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
            progress.finish()
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
        nsView.teardownRender()
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
    private var metalFrameWork: DispatchWorkItem?
    private var cursorBlinkTimer: Timer?
    private var windowObservers: [NSObjectProtocol] = []
    private var pendingKeyEcho = false
    private var lastPaintedCursor: (x: Int, y: Int)?

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

    override var isHidden: Bool {
        didSet {
            syncRenderState(paint: isLive)
        }
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        hideReservedScroller()
        if window == nil {
            removeKeyMonitor()
            teardownRender()
        } else {
            installKeyMonitor()
            attachWindowObservers()
            syncRenderState(paint: isLive)
        }
    }

    override func setNeedsDisplay(_ invalidRect: NSRect) {
        if isUsingMetalRenderer || isHidden {
            return
        }
        super.setNeedsDisplay(invalidRect)
    }

    override func dataReceived(slice: ArraySlice<UInt8>) {
        if !isLive {
            terminal.feed(buffer: slice)
        } else if isUsingMetalRenderer {
            terminal.feed(buffer: slice)
            if !terminal.synchronizedOutputActive {
                if pendingKeyEcho {
                    presentMetalFrame(force: true)
                } else {
                    enqueueMetalFrame()
                }
            }
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
        if isLive {
            enableMetalIfNeeded()
            if isUsingMetalRenderer, !hadMetal {
                terminal.updateFullScreen()
            }
        }
        syncRenderState(paint: true)
    }

    func teardownRender() {
        detachWindowObservers()
        metalFrameWork?.cancel()
        metalFrameWork = nil
        setCursorBlinkRunning(false)
        pauseMetalDisplay()
    }

    private func presentMetalFrame(force: Bool = false) {
        guard isLive, let metalKitView else {
            return
        }
        if terminal.synchronizedOutputActive {
            return
        }
        let cursor = terminal.getCursorLocation()
        let cursorUnchanged = lastPaintedCursor.map { $0 == cursor } ?? false
        if !force, terminal.getUpdateRange() == nil, cursorUnchanged {
            return
        }
        if let (startY, endY) = terminal.getScrollInvariantUpdateRange(), startY <= endY {
            metalDirtyRange = startY...endY
        } else {
            metalDirtyRange = nil
        }
        metalKitView.draw()
        terminal.clearUpdateRange()
        lastPaintedCursor = cursor
        pendingKeyEcho = false
    }

    private func enqueueMetalFrame() {
        guard metalFrameWork == nil else {
            return
        }
        let work = DispatchWorkItem { [weak self] in
            guard let self else { return }
            self.metalFrameWork = nil
            self.presentMetalFrame()
            if self.terminal.getUpdateRange() != nil {
                self.enqueueMetalFrame()
            }
        }
        metalFrameWork = work
        DispatchQueue.main.async(execute: work)
    }

    private var metalKitView: MTKView? {
        subviews.first { $0 is MTKView } as? MTKView
    }

    private var isLive: Bool {
        !isHidden && window != nil && windowIsVisible
    }

    private var windowIsVisible: Bool {
        window?.occlusionState.contains(.visible) == true
    }

    private var wantsCursorBlink: Bool {
        guard isLive, window?.isKeyWindow == true else {
            return false
        }
        switch terminal.options.cursorStyle {
        case .blinkBlock, .blinkUnderline, .blinkBar:
            return true
        case .steadyBlock, .steadyUnderline, .steadyBar:
            return false
        }
    }

    private func syncRenderState(paint: Bool = false) {
        if isLive {
            enableMetalIfNeeded()
            pauseMetalDisplay()
            if paint {
                presentMetalFrame(force: true)
            }
        } else {
            metalFrameWork?.cancel()
            metalFrameWork = nil
            pauseMetalDisplay()
        }
        setCursorBlinkRunning(wantsCursorBlink)
    }

    private func attachWindowObservers() {
        detachWindowObservers()
        guard let window else {
            return
        }
        let center = NotificationCenter.default
        let occlusion = center.addObserver(
            forName: NSWindow.didChangeOcclusionStateNotification,
            object: window,
            queue: .main
        ) { [weak self] _ in
            self?.syncRenderState(paint: true)
        }
        let becomeKey = center.addObserver(
            forName: NSWindow.didBecomeKeyNotification,
            object: window,
            queue: .main
        ) { [weak self] _ in
            self?.syncRenderState()
        }
        let resignKey = center.addObserver(
            forName: NSWindow.didResignKeyNotification,
            object: window,
            queue: .main
        ) { [weak self] _ in
            self?.syncRenderState()
        }
        windowObservers = [occlusion, becomeKey, resignKey]
    }

    private func detachWindowObservers() {
        for observer in windowObservers {
            NotificationCenter.default.removeObserver(observer)
        }
        windowObservers = []
    }

    private func setCursorBlinkRunning(_ running: Bool) {
        if running {
            guard cursorBlinkTimer == nil else {
                return
            }
            let timer = Timer(timeInterval: 0.7, repeats: true) { [weak self] _ in
                self?.presentCursorFrame()
            }
            RunLoop.main.add(timer, forMode: .common)
            cursorBlinkTimer = timer
        } else if cursorBlinkTimer != nil {
            cursorBlinkTimer?.invalidate()
            cursorBlinkTimer = nil
        }
    }

    private func presentCursorFrame() {
        guard wantsCursorBlink, let metalKitView else {
            return
        }
        if terminal.synchronizedOutputActive {
            return
        }
        metalDirtyRange = nil
        metalKitView.draw()
    }

    private func pauseMetalDisplay() {
        guard let metalKitView else {
            return
        }
        metalKitView.enableSetNeedsDisplay = false
        metalKitView.isPaused = true
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
            self.pendingKeyEcho = true
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
        syncRenderState(paint: true)
    }

    private func hideReservedScroller() {
        for subview in subviews where subview is NSScroller && !subview.isHidden {
            subview.isHidden = true
        }
    }
}
