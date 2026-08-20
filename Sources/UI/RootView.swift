import SwiftUI

struct RootView: View {
    @EnvironmentObject private var store: AppStore

    var body: some View {
        VStack(spacing: 0) {
            if let catalogError = store.catalogError {
                catalogBanner(catalogError)
            }
            tabStrip
            Divider()
            paneArea
        }
        .background(Color(nsColor: .textBackgroundColor))
        .sheet(isPresented: editorPresented) {
            PlaceEditorSheet()
                .environmentObject(store)
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
                .padding(.horizontal, 6)
                .padding(.vertical, 6)
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
        if let place = store.selectedPlace {
            let epoch = store.paneEpoch[place.id, default: 0]
            ZStack {
                LocalShellPane(
                    placeID: place.id,
                    onExit: { code in
                        store.markFailed(
                            place.id,
                            message: "process exited" + (code.map { " (\($0))" } ?? "")
                        )
                    }
                )
                .id("\(place.id.uuidString)-\(epoch)")

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
        } else {
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
            return .orange
        }
    }
}
