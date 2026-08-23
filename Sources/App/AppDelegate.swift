import AppKit

final class AppDelegate: NSObject, NSApplicationDelegate {
    func application(_ application: NSApplication, open urls: [URL]) {
        Task { @MainActor in
            for url in urls {
                AppRuntime.handle(url)
            }
        }
    }

    func applicationShouldHandleReopen(
        _ sender: NSApplication,
        hasVisibleWindows flag: Bool
    ) -> Bool {
        Task { @MainActor in
            AppRuntime.revealMainWindow()
        }
        return false
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        PlaceLink.claimHandler()
    }
}
