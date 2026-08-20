import AppKit
import SwiftUI

/// System fullscreen lives in a Space and never includes the camera strip.
/// Apple documents `auxiliaryTopLeftArea` for a *custom* full-screen window
/// whose frame is `NSScreen.frame`. That is this type.
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
    private var savedFrame = NSRect.zero
    private var savedStyleMask: NSWindow.StyleMask = []
    private var savedPresentation: NSApplication.PresentationOptions = []
    private var savedMovable = true
    private var savedTransparent = false
    private var savedBackground: NSColor?
    private var savedHasShadow = true
    private weak var zoomButton: NSButton?
    private var savedZoomTarget: AnyObject?
    private var savedZoomAction: Selector?
    private var escapeMonitor: Any?

    func attach(to window: NSWindow?) {
        attachedWindow = window
        installCollectionBehavior(on: window)
        installZoomHook(on: window)
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
        guard let window, window.attachedSheet == nil else {
            return
        }
        if window.isKeyWindow {
            restoreTerminalFocus(in: window)
        } else {
            resignTerminal(in: window)
        }
    }

    func focusTerminalIfKey() {
        guard let window = attachedWindow ?? NSApp.keyWindow, window.isKeyWindow else {
            return
        }
        restoreTerminalFocus(in: window)
    }

    @objc func toggleFillScreen() {
        guard let window = attachedWindow ?? NSApp.keyWindow else {
            return
        }
        if isFillScreen {
            exitFillScreen(window)
        } else {
            enterFillScreen(window)
        }
    }

    private func enterFillScreen(_ window: NSWindow) {
        guard let screen = window.screen else {
            return
        }
        savedFrame = window.frame
        savedStyleMask = window.styleMask
        savedPresentation = NSApp.presentationOptions
        savedMovable = window.isMovable
        savedTransparent = window.titlebarAppearsTransparent
        savedBackground = window.backgroundColor
        savedHasShadow = window.hasShadow
        isFillScreen = true

        NSApp.presentationOptions = [.autoHideDock, .autoHideMenuBar]
        var mask = window.styleMask
        mask.remove([.titled, .closable, .miniaturizable, .resizable])
        window.styleMask = mask.union([.borderless, .fullSizeContentView])
        window.titlebarAppearsTransparent = true
        window.titleVisibility = .hidden
        window.hasShadow = false
        window.isMovable = false
        window.backgroundColor = .windowBackgroundColor
        window.setFrame(screen.frame, display: true)
        installEscapeMonitor()
        refreshGeometry(from: window)
        focusTerminal(in: window)
    }

    private func exitFillScreen(_ window: NSWindow) {
        removeEscapeMonitor()
        isFillScreen = false
        NSApp.presentationOptions = savedPresentation
        window.styleMask = savedStyleMask
        window.titlebarAppearsTransparent = savedTransparent
        window.hasShadow = savedHasShadow
        window.isMovable = savedMovable
        if let savedBackground {
            window.backgroundColor = savedBackground
        }
        window.setFrame(savedFrame, display: true)
        installZoomHook(on: window)
        clearGeometry()
        focusTerminal(in: window)
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
            guard window.isKeyWindow, window.attachedSheet == nil else {
                return
            }
            if let terminal = Self.visibleTerminal(in: window.contentView) {
                window.makeFirstResponder(terminal)
            }
        }
    }

    private func resignTerminal(in window: NSWindow) {
        guard let responder = window.firstResponder as? NSView,
              Self.isTerminal(responder)
        else {
            return
        }
        window.makeFirstResponder(nil)
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
        String(describing: type(of: view)).contains("LocalProcessTerminalView")
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

    private func installEscapeMonitor() {
        guard escapeMonitor == nil else {
            return
        }
        escapeMonitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self] event in
            guard let self, event.keyCode == 53, self.isFillScreen else {
                return event
            }
            self.toggleFillScreen()
            return nil
        }
    }

    private func removeEscapeMonitor() {
        if let escapeMonitor {
            NSEvent.removeMonitor(escapeMonitor)
        }
        escapeMonitor = nil
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
            center.addObserver(
                self,
                selector: #selector(handleKey),
                name: NSWindow.didResignKeyNotification,
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
