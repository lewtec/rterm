import AppKit
import SwiftUI

struct RootView: View {
    @EnvironmentObject private var store: AppStore
    @EnvironmentObject private var chrome: WindowChrome

    var body: some View {
        VStack(spacing: 0) {
            chromeRow
            if let catalogError = store.catalogError {
                catalogBanner(catalogError)
            }
            paneArea
        }
        .background(Color(nsColor: .textBackgroundColor))
        .background(WindowChromeReader(chrome: chrome))
        .ignoresSafeArea(.container, edges: .top)
        .toolbar(.hidden)
        .sheet(isPresented: editorPresented) {
            PlaceEditorSheet()
                .environmentObject(store)
        }
        .onChange(of: store.selectedID) { _, _ in
            chrome.focusTerminalIfKey()
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

    @ViewBuilder
    private var chromeRow: some View {
        if chrome.splitActive {
            notchRow
        } else {
            windowedRow
        }
    }

    private var windowedRow: some View {
        ZStack {
            Color(nsColor: .windowBackgroundColor)
            WindowDragRegion()
            HStack(spacing: 0) {
                Color.clear
                    .frame(width: WindowChrome.trafficLightWidth)
                    .allowsHitTesting(false)
                tabStrip(store.places)
                    .frame(minWidth: 0, maxWidth: .infinity, alignment: .leading)
                addPlaceButton
                    .padding(.trailing, 8)
            }
        }
        .frame(height: chrome.rowHeight)
        .frame(maxWidth: .infinity)
        .overlay(alignment: .bottom) {
            Rectangle()
                .fill(Color(nsColor: .separatorColor))
                .frame(height: 1)
        }
    }

    private var notchRow: some View {
        let packed = packedPlaces
        return ZStack {
            Color(nsColor: .windowBackgroundColor)
            HStack(spacing: 0) {
                trafficLights
                tabStrip(packed.leading)
                    .frame(maxWidth: chrome.leftCap, alignment: .leading)
                Spacer(minLength: chrome.notchWidth)
                tabStrip(packed.trailing)
                    .frame(maxWidth: chrome.rightCap, alignment: .trailing)
                addPlaceButton
                    .padding(.trailing, 8)
            }
        }
        .frame(height: chrome.rowHeight)
        .frame(maxWidth: .infinity)
        .overlay(alignment: .bottom) {
            Rectangle()
                .fill(Color(nsColor: .separatorColor))
                .frame(height: 1)
        }
        .contentShape(Rectangle())
    }

    private var packedPlaces: (leading: [Place], trailing: [Place]) {
        NotchTabPack.split(
            places: store.places,
            leftCap: chrome.leftCap,
            widthOf: { PlaceTab.estimatedWidth(for: $0) }
        )
    }

    private func tabStrip(_ places: [Place]) -> some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 2) {
                ForEach(places) { place in
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
        }
        .frame(minWidth: 0)
    }

    private var trafficLights: some View {
        HStack(spacing: 8) {
            TrafficDot(color: Color(red: 1, green: 0.37, blue: 0.34)) {
                NSApp.keyWindow?.performClose(nil)
            }
            TrafficDot(color: Color(red: 1, green: 0.74, blue: 0.18)) {
                NSApp.keyWindow?.miniaturize(nil)
            }
            TrafficDot(color: Color(red: 0.16, green: 0.78, blue: 0.25)) {
                chrome.toggleFillScreen()
            }
        }
        .padding(.leading, 12)
        .frame(width: WindowChrome.trafficLightWidth, alignment: .leading)
    }

    private var addPlaceButton: some View {
        Button {
            store.beginAdd()
        } label: {
            Image(systemName: "plus")
                .font(.system(size: 13, weight: .medium))
                .frame(width: 18, height: 18)
        }
        .buttonStyle(.plain)
        .help("Add Place")
    }

    @ViewBuilder
    private var paneArea: some View {
        if store.places.isEmpty {
            emptyCatalog
        } else {
            GeometryReader { geo in
            ZStack {
                ForEach(store.places) { place in
                    if store.hasPane(place.id) {
                        let epoch = store.paneEpoch[place.id, default: 0]
                        AttachPane(
                            place: place,
                            active: store.selectedID == place.id,
                            onExit: { code in
                                store.handleExit(place.id, code: code)
                            }
                        )
                        .id("\(place.id.uuidString)-\(epoch)")
                        .frame(width: geo.size.width, height: geo.size.height)
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
            .frame(width: geo.size.width, height: geo.size.height)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
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

private struct WindowDragRegion: NSViewRepresentable {
    func makeNSView(context: Context) -> NSView {
        DragView()
    }

    func updateNSView(_ nsView: NSView, context: Context) {}

    private final class DragView: NSView {
        override var mouseDownCanMoveWindow: Bool { true }
    }
}

private struct TrafficDot: View {
    let color: Color
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Circle()
                .fill(color)
                .frame(width: 12, height: 12)
        }
        .buttonStyle(.plain)
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
        .padding(.horizontal, 8)
        .padding(.vertical, 2)
        .background(selected ? Color.accentColor.opacity(0.22) : Color.clear, in: Capsule())
        .overlay(
            Capsule().strokeBorder(selected ? Color.accentColor.opacity(0.5) : Color.clear)
        )
    }

    static func estimatedWidth(for place: Place) -> CGFloat {
        let text = (place.displayLabel as NSString).size(withAttributes: [
            .font: NSFont.systemFont(ofSize: NSFont.systemFontSize),
        ]).width
        return 8 + 7 + 6 + text + 8
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
