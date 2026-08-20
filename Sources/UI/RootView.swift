import AppKit
import SwiftUI

struct RootView: View {
    @EnvironmentObject private var store: AppStore

    var body: some View {
        VStack(spacing: 0) {
            if let catalogError = store.catalogError {
                catalogBanner(catalogError)
            }
            paneArea
        }
        .background(Color(nsColor: .textBackgroundColor))
        .toolbar {
            ToolbarItem(placement: .principal) {
                tabStrip
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
        .sheet(isPresented: editorPresented) {
            PlaceEditorSheet()
                .environmentObject(store)
        }
        .onReceive(NSWorkspace.shared.notificationCenter.publisher(for: NSWorkspace.willSleepNotification)) { _ in
            store.dropAllConnections()
        }
        .onReceive(NSWorkspace.shared.notificationCenter.publisher(for: NSWorkspace.willPowerOffNotification)) { _ in
            store.dropAllConnections()
        }
    }

    private var editorPresented: Binding<Bool> {
        Binding(
            get: { store.editor != nil },
            set: { if !$0 { store.editor = nil } }
        )
    }

    private func catalogBanner(_ message: String) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Image(systemName: "exclamationmark.triangle.fill")
            Text(message)
                .textSelection(.enabled)
            Spacer()
            Button("Reveal") {
                store.revealCatalog()
            }
            Button("Reload") {
                store.reloadFromDisk()
            }
        }
        .padding(8)
        .background(Color.orange.opacity(0.25))
    }

    private var tabStrip: some View {
        HStack(spacing: 0) {
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 2) {
                    ForEach(store.places) { place in
                        PlaceTab(
                            place: place,
                            state: store.tabStates[place.id] ?? .idle,
                            selected: store.selectedID == place.id
                        )
                        .onTapGesture {
                            store.select(place.id)
                        }
                        .contextMenu {
                            Button("Reconnect") {
                                store.reconnect(place.id)
                            }
                            Button("Edit…") {
                                store.beginEdit(place)
                            }
                            Button("Delete", role: .destructive) {
                                store.delete(place.id)
                            }
                        }
                    }
                }
                .padding(.horizontal, 4)
            }
            Button {
                store.beginAdd()
            } label: {
                Image(systemName: "plus")
                    .padding(8)
            }
            .buttonStyle(.plain)
            .help("Add Place")
        }
        .background(.bar)
    }

    @ViewBuilder
    private var paneArea: some View {
        if store.places.isEmpty {
            emptyCatalog
        } else {
            ZStack {
                ForEach(store.places) { place in
                    if store.hasPane(place.id) {
                        let epoch = store.paneEpoch[place.id, default: 0]
                        AttachPane(
                            place: place,
                            onExit: { code in
                                store.handleExit(place.id, code: code)
                            }
                        )
                        .id("\(place.id.uuidString)-\(epoch)")
                        .opacity(store.selectedID == place.id ? 1 : 0)
                        .allowsHitTesting(store.selectedID == place.id)
                    }
                }

                if let place = store.selectedPlace, !store.hasPane(place.id) {
                    ContentUnavailableView {
                        Label("Idle", systemImage: "moon.zzz")
                    } description: {
                        Text("Click this tab or Reconnect to attach.")
                    }
                }

                if let place = store.selectedPlace {
                    if store.debugOverlay {
                        FailureOverlay(
                            message: "debug overlay",
                            onRetry: { store.reconnect(place.id) }
                        )
                    } else if case .failed(let message) = store.tabStates[place.id] {
                        FailureOverlay(
                            message: message,
                            onRetry: { store.reconnect(place.id) }
                        )
                    }
                }
            }
        }
    }

    private var emptyCatalog: some View {
        ContentUnavailableView {
            Label("No places", systemImage: "terminal")
        } description: {
            Text("Add a place to attach later.")
        } actions: {
            Button("Add Place…") {
                store.beginAdd()
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

private struct PlaceTab: View {
    let place: Place
    let state: TabState
    let selected: Bool

    var body: some View {
        HStack(spacing: 6) {
            Circle()
                .fill(dotColor)
                .frame(width: 7, height: 7)
            Text(place.displayLabel)
                .lineLimit(1)
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 5)
        .background(selected ? Color.accentColor.opacity(0.22) : Color.clear, in: Capsule())
        .overlay(
            Capsule().strokeBorder(selected ? Color.accentColor.opacity(0.5) : Color.clear)
        )
    }

    private var dotColor: Color {
        switch state {
        case .idle:
            return .secondary
        case .live:
            return .green
        case .failed:
            return .red
        }
    }
}
