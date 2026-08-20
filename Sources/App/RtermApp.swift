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
        .windowToolbarStyle(.unified(showsTitle: false))
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

                ForEach(0..<9, id: \.self) { index in
                    Button(placeMenuTitle(at: index)) {
                        store.selectPlace(at: index)
                    }
                    .keyboardShortcut(KeyEquivalent(Character("\(index + 1)")), modifiers: [.command])
                    .disabled(!store.places.indices.contains(index))
                }
                Button(placeMenuTitle(at: 9)) {
                    store.selectPlace(at: 9)
                }
                .keyboardShortcut("0", modifiers: [.command])
                .disabled(!store.places.indices.contains(9))

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

    private func placeMenuTitle(at index: Int) -> String {
        guard store.places.indices.contains(index) else {
            return "Place \(index + 1)"
        }
        return store.places[index].displayLabel
    }
}
