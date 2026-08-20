import SwiftUI

@main
struct RtermApp: App {
    @StateObject private var store = AppStore()

    var body: some Scene {
        Window("rterm", id: "main") {
            RootView()
                .environmentObject(store)
                .frame(minWidth: 720, minHeight: 420)
        }
        .windowResizability(.contentMinSize)
        .defaultSize(width: 960, height: 600)
        .commands {
            CommandGroup(replacing: .newItem) {}
            CommandMenu("Place") {
                Button("Add Place…") {
                    store.beginAdd()
                }
                .keyboardShortcut("n", modifiers: [.command])

                Button("Reconnect") {
                    store.reconnectSelected()
                }
                .keyboardShortcut("r", modifiers: [.command])
                .disabled(store.selectedID == nil)

                Divider()

                Button("Toggle Debug Overlay") {
                    store.debugOverlay.toggle()
                }

                Divider()

                Button("Reveal Catalog in Finder") {
                    store.revealCatalog()
                }
            }
        }
    }
}
