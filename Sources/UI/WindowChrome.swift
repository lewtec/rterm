import AppKit
import SwiftTerm
import SwiftUI

/// Custom fill-screen (not Spaces fullscreen). One flag, two apply paths.
@MainActor
final class WindowChrome: NSObject, ObservableObject {
    @Published private(set) var splitActive = false
    @Published private(set) var leftCap: CGFloat = .infinity
    @Published private(set) var rightCap: CGFloat = .infinity
    @Published private(set) var leftPad: CGFloat = 0
    @Published private(set) var notchWidth: CGFloat = 0
    @Published private(set) var rowHeight: CGFloat = 28

    private weak var attachedWindow: NSWindow?
    private var isFillScreen = false
    private var windowedFrame = NSRect.zero
    private var primedTitlebar = false
    private weak var zoomButton: NSButton?
    private var savedZoomTarget: AnyObject?
    private var savedZoomAction: Selector?
    private var keyMonitor: Any?

    func attach(to window: NSWindow?) {
        attachedWindow = window
        installCollectionBehavior(on: window)
        installZoomHook(on: window)
        installKeyMonitor()
        if !isFillScreen {
            applyWindowed(restoreFrame: false)
        }
    }

    private func applyWindowed(restoreFrame: Bool) {
        guard let window = attachedWindow else {
            return
        }
        isFillScreen = false
        NSApp.presentationOptions = []
        window.styleMask = [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView]
        window.titlebarAppearsTransparent = true
        window.titleVisibility = .hidden
        window.title = ""
        window.hasShadow = true
        window.isMovable = true
        if window.toolbar != nil {
            window.toolbar = nil
        }
        window.toolbarStyle = .unifiedCompact
        if restoreFrame, windowedFrame != .zero {
            window.setFrame(windowedFrame, display: true)
        }
        if !primedTitlebar {
            primedTitlebar = true
            DispatchQueue.main.async { [weak self] in
                guard let self, !self.isFillScreen, let window = self.attachedWindow else {
                    return
                }
                window.styleMask.remove(.titled)
                window.styleMask.insert([.titled, .fullSizeContentView])
                self.syncWindowedRowHeight(from: window)
            }
        }
        installZoomHook(on: window)
        clearGeometry()
        syncWindowedRowHeight(from: window)
    }

    private func applyFill() {
        guard let window = attachedWindow, let screen = window.screen else {
            return
        }
        windowedFrame = window.frame
        isFillScreen = true
        NSApp.presentationOptions = [.autoHideDock, .autoHideMenuBar]
        window.styleMask = [.borderless, .fullSizeContentView]
        window.titlebarAppearsTransparent = true
        window.titleVisibility = .hidden
        window.hasShadow = false
        window.isMovable = false
        window.backgroundColor = .windowBackgroundColor
        window.setFrame(screen.frame, display: true)
        refreshGeometry(from: window)
        focusTerminal(in: window)
    }

    private func syncWindowedRowHeight(from window: NSWindow) {
        guard !isFillScreen else {
            return
        }
        let height = Self.windowedRowHeight(for: window)
        guard !Self.capsAlmostEqual(rowHeight, height) else {
            return
        }
        DispatchQueue.main.async { [weak self] in
            guard let self, !self.isFillScreen else {
                return
            }
            if !Self.capsAlmostEqual(self.rowHeight, height) {
                self.rowHeight = height
            }
        }
    }

    private static func windowedRowHeight(for window: NSWindow) -> CGFloat {
        if let button = window.standardWindowButton(.closeButton),
           let container = button.superview
        {
            return max(container.frame.height, 28)
        }
        let measured = window.frame.height - window.contentLayoutRect.height
        return max(measured, 28)
    }

    func handleScreenChange(of window: NSWindow?) {
        guard isFillScreen, let window else {
            return
        }
        attachedWindow = window
        pinFrame(window)
        refreshGeometry(from: window)
    }

    func handleKeyChange(of window: NSWindow?) {
        guard let window, window.isKeyWindow, window.attachedSheet == nil else {
            return
        }
        restoreTerminalFocus(in: window)
    }

    func focusTerminalIfKey() {
        guard let window = attachedWindow ?? NSApp.mainWindow ?? NSApp.keyWindow else {
            return
        }
        if !window.isKeyWindow {
            window.makeKeyAndOrderFront(nil)
        }
        restoreTerminalFocus(in: window)
    }

    @objc func toggleFillScreen() {
        if attachedWindow == nil {
            attachedWindow = NSApp.keyWindow
        }
        if isFillScreen {
            applyWindowed(restoreFrame: true)
            if let window = attachedWindow {
                focusTerminal(in: window)
            }
        } else {
            applyFill()
        }
    }

    private func pinFrame(_ window: NSWindow) {
        guard let screen = window.screen else {
            return
        }
        let target = screen.frame
        if abs(window.frame.minX - target.minX) < 1,
           abs(window.frame.minY - target.minY) < 1,
           abs(window.frame.width - target.width) < 1,
           abs(window.frame.height - target.height) < 1
        {
            return
        }
        window.setFrame(target, display: true)
    }

    private func focusTerminal(in window: NSWindow) {
        window.makeKeyAndOrderFront(nil)
        restoreTerminalFocus(in: window)
    }

    private func restoreTerminalFocus(in window: NSWindow) {
        DispatchQueue.main.async {
            self.ensureTerminalFirstResponder(in: window)
        }
    }

    private func ensureTerminalFirstResponder(in window: NSWindow? = nil) {
        let window = window ?? attachedWindow
        guard let window, window.isKeyWindow, window.attachedSheet == nil else {
            return
        }
        if let current = window.firstResponder as? NSView, Self.isTerminal(current), !current.isHidden {
            return
        }
        if let terminal = Self.visibleTerminal(in: window.contentView) {
            window.makeFirstResponder(terminal)
        }
    }

    private static func visibleTerminal(in view: NSView?) -> NSView? {
        guard let view, !view.isHidden else {
            return nil
        }
        if isTerminal(view) {
            return view
        }
        for child in view.subviews {
            if let found = visibleTerminal(in: child) {
                return found
            }
        }
        return nil
    }

    private static func isTerminal(_ view: NSView) -> Bool {
        view is LocalProcessTerminalView
    }

    private func refreshGeometry(from window: NSWindow) {
        guard
            let screen = window.screen,
            let left = Self.optionalRect(screen.auxiliaryTopLeftArea),
            let right = Self.optionalRect(screen.auxiliaryTopRightArea),
            left.width > 1,
            right.width > 1
        else {
            apply(
                splitActive: true,
                leftCap: max(120, window.frame.width / 2 - Self.trafficLightWidth),
                rightCap: max(120, window.frame.width / 2 - Self.plusReserve),
                leftPad: Self.trafficLightWidth,
                notchWidth: 0,
                rowHeight: 32
            )
            return
        }

        apply(
            splitActive: true,
            leftCap: max(0, left.maxX - window.frame.minX - Self.trafficLightWidth),
            rightCap: max(0, window.frame.maxX - right.minX - Self.plusReserve),
            leftPad: Self.trafficLightWidth,
            notchWidth: max(0, right.minX - left.maxX),
            rowHeight: max(left.height, 28)
        )
    }

    private func clearGeometry() {
        apply(
            splitActive: false,
            leftCap: .infinity,
            rightCap: .infinity,
            leftPad: 0,
            notchWidth: 0,
            rowHeight: 28
        )
    }

    private func installCollectionBehavior(on window: NSWindow?) {
        guard let window else {
            return
        }
        var behavior = window.collectionBehavior
        behavior.remove(.fullScreenPrimary)
        behavior.remove(.fullScreenAllowsTiling)
        behavior.insert(.fullScreenNone)
        if window.collectionBehavior != behavior {
            window.collectionBehavior = behavior
        }
    }

    private func installZoomHook(on window: NSWindow?) {
        guard let zoom = window?.standardWindowButton(.zoomButton) else {
            return
        }
        if zoom === zoomButton, zoom.action == #selector(toggleFillScreen) {
            return
        }
        restoreZoomHook()
        savedZoomTarget = zoom.target as AnyObject?
        savedZoomAction = zoom.action
        zoom.target = self
        zoom.action = #selector(toggleFillScreen)
        zoomButton = zoom
    }

    private func restoreZoomHook() {
        if let zoom = zoomButton {
            zoom.target = savedZoomTarget
            zoom.action = savedZoomAction
        }
        zoomButton = nil
        savedZoomTarget = nil
        savedZoomAction = nil
    }

    private func installKeyMonitor() {
        guard keyMonitor == nil else {
            return
        }
        keyMonitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self] event in
            guard let self else {
                return event
            }
            if event.keyCode == 53, self.isFillScreen {
                self.toggleFillScreen()
                return nil
            }
            self.ensureTerminalFirstResponder()
            return event
        }
    }

    private func apply(
        splitActive: Bool,
        leftCap: CGFloat,
        rightCap: CGFloat,
        leftPad: CGFloat,
        notchWidth: CGFloat,
        rowHeight: CGFloat
    ) {
        guard self.splitActive != splitActive
            || !Self.capsAlmostEqual(self.leftCap, leftCap)
            || !Self.capsAlmostEqual(self.rightCap, rightCap)
            || !Self.capsAlmostEqual(self.leftPad, leftPad)
            || !Self.capsAlmostEqual(self.notchWidth, notchWidth)
            || !Self.capsAlmostEqual(self.rowHeight, rowHeight)
        else {
            return
        }
        self.splitActive = splitActive
        self.leftCap = leftCap
        self.rightCap = rightCap
        self.leftPad = leftPad
        self.notchWidth = notchWidth
        self.rowHeight = rowHeight
    }

    private static func capsAlmostEqual(_ a: CGFloat, _ b: CGFloat) -> Bool {
        if a.isInfinite && b.isInfinite {
            return true
        }
        if a.isInfinite || b.isInfinite {
            return false
        }
        return abs(a - b) <= 0.5
    }

    private static let plusReserve: CGFloat = 28
    static let trafficLightWidth: CGFloat = 78

    private static func optionalRect(_ rect: NSRect?) -> NSRect? {
        guard let rect, rect.width > 0, rect.height > 0 else {
            return nil
        }
        return rect
    }
}

struct WindowChromeReader: NSViewRepresentable {
    @ObservedObject var chrome: WindowChrome

    func makeNSView(context: Context) -> NSView {
        let view = ObserverView()
        view.chrome = chrome
        return view
    }

    func updateNSView(_ nsView: NSView, context: Context) {
        (nsView as? ObserverView)?.chrome = chrome
    }

    private final class ObserverView: NSView {
        var chrome: WindowChrome?

        override var acceptsFirstResponder: Bool { false }

        override func hitTest(_ point: NSPoint) -> NSView? { nil }

        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            retarget()
            chrome?.attach(to: window)
        }

        private func retarget() {
            let center = NotificationCenter.default
            center.removeObserver(self)
            guard let window else {
                return
            }
            center.addObserver(
                self,
                selector: #selector(handleScreen),
                name: NSWindow.didChangeScreenNotification,
                object: window
            )
            center.addObserver(
                self,
                selector: #selector(handleScreen),
                name: NSApplication.didChangeScreenParametersNotification,
                object: nil
            )
            center.addObserver(
                self,
                selector: #selector(handleKey),
                name: NSWindow.didBecomeKeyNotification,
                object: window
            )
        }

        @objc private func handleScreen(_ notification: Notification) {
            chrome?.handleScreenChange(of: window)
        }

        @objc private func handleKey(_ notification: Notification) {
            chrome?.handleKeyChange(of: window)
        }
    }
}
