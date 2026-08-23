import AppKit

final class AppDelegate: NSObject, NSApplicationDelegate {
    func application(_ application: NSApplication, open urls: [URL]) {
        Task { @MainActor in
            for url in urls {
                AppRuntime.handle(url)
            }
        }
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        PlaceLink.claimHandler()
    }
}
