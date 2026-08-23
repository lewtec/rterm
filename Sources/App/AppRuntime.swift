import AppKit

@MainActor
enum AppRuntime {
    static weak var store: AppStore?

    static func handle(_ url: URL) {
        guard let key = PlaceLink.switchKey(from: url) else {
            return
        }
        PendingPlace.write(key, to: CatalogPaths.pendingPlaceFile)
        store?.openPlace(switchKey: key)
        revealMainWindow()
    }

    static func revealMainWindow() {
        promoteMainWindow()
        DispatchQueue.main.async {
            promoteMainWindow()
        }
    }

    private static func promoteMainWindow() {
        NSApp.activate()
        let windows = NSApp.windows.filter(isAppWindow)
        let main = windows.first { $0.identifier?.rawValue == RtermApp.mainWindowID }
            ?? windows.min { $0.windowNumber < $1.windowNumber }
        if let main {
            if main.isMiniaturized {
                main.deminiaturize(nil)
            }
            main.makeKeyAndOrderFront(nil)
        }
        for window in windows where window !== main {
            window.close()
        }
    }

    private static func isAppWindow(_ window: NSWindow) -> Bool {
        window.level == .normal
            && window.sheetParent == nil
            && window.styleMask.contains(.titled)
    }
}
