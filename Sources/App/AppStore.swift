import AppKit
import Foundation
import SwiftUI

@MainActor
final class AppStore: ObservableObject {
    @Published var places: [Place] = []
    @Published var selectedID: UUID?
    @Published var tabStates: [UUID: TabState] = [:]
    @Published var paneEpoch: [UUID: Int] = [:]
    @Published var editor: PlaceEditorState?
    @Published var debugOverlay = false
    @Published var catalogError: String?

    private let catalogIO: CatalogIO

    init(catalogIO: CatalogIO = CatalogIO()) {
        self.catalogIO = catalogIO
        loadFromDisk(creatingIfMissing: true)
        startWatching()
        if let selectedID {
            select(selectedID)
        }
    }

    var selectedPlace: Place? {
        places.first { $0.id == selectedID }
    }

    func select(_ id: UUID) {
        selectedID = id
        if tabStates[id] == nil || tabStates[id] == .idle {
            tabStates[id] = .live
        }
    }

    func reconnect(_ id: UUID) {
        paneEpoch[id, default: 0] += 1
        tabStates[id] = .live
        debugOverlay = false
    }

    func reconnectSelected() {
        guard let selectedID else { return }
        reconnect(selectedID)
    }

    func dropAllConnections() {
        debugOverlay = false
        for id in places.map(\.id) {
            tabStates[id] = .idle
        }
    }

    func beginAdd() {
        editor = PlaceEditorState(
            originID: nil,
            user: NSUserName(),
            host: "",
            backend: .herdr,
            session: "",
            label: ""
        )
    }

    func beginEdit(_ place: Place) {
        editor = PlaceEditorState(
            originID: place.id,
            user: place.user,
            host: place.host,
            backend: place.backend,
            session: place.session ?? "",
            label: place.label ?? ""
        )
    }

    func saveEditor() {
        guard var draft = editor else { return }
        draft.host = draft.host.trimmingCharacters(in: .whitespacesAndNewlines)
        draft.user = draft.user.trimmingCharacters(in: .whitespacesAndNewlines)
        if Place.isLoopbackHost(draft.host) {
            draft.host = ""
            draft.user = ""
        } else if draft.user.isEmpty {
            return
        }
        if draft.backend == .herdr {
            draft.session = ""
        } else if draft.session.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            return
        }

        let session: String? = draft.backend == .herdr
            ? nil
            : draft.session.trimmingCharacters(in: .whitespacesAndNewlines)
        let label = draft.label.trimmingCharacters(in: .whitespacesAndNewlines)
        let place = Place(
            id: draft.originID ?? UUID(),
            user: draft.user,
            host: draft.host,
            backend: draft.backend,
            session: session,
            label: label.isEmpty ? nil : label
        )

        let shouldReattach: Bool
        if let index = places.firstIndex(where: { $0.id == place.id }) {
            shouldReattach = places[index] != place && tabStates[place.id] != .idle
            places[index] = place
        } else {
            shouldReattach = false
            places.append(place)
            tabStates[place.id] = .idle
        }
        editor = nil
        persist()
        if shouldReattach {
            reconnect(place.id)
        } else {
            select(place.id)
        }
    }

    func delete(_ id: UUID) {
        places.removeAll { $0.id == id }
        tabStates[id] = nil
        paneEpoch[id] = nil
        if selectedID == id {
            if let next = places.first?.id {
                select(next)
            } else {
                selectedID = nil
            }
        }
        if editor?.originID == id {
            editor = nil
        }
        persist()
    }

    func handleExit(_ id: UUID, code: Int32?) {
        switch AttachExit.classify(code) {
        case .clean:
            tabStates[id] = .idle
        case .failed(let message):
            tabStates[id] = .failed(message)
        }
    }

    func hasPane(_ id: UUID) -> Bool {
        switch tabStates[id] {
        case .live, .failed:
            return true
        case .idle, .none:
            return false
        }
    }

    func revealCatalog() {
        do {
            try catalogIO.prepareDirectory()
            if !catalogIO.fileExists() {
                persist()
            }
        } catch {
            catalogError = error.localizedDescription
            return
        }
        NSWorkspace.shared.activateFileViewerSelecting([CatalogPaths.placesFile])
    }

    func reloadFromDisk() {
        loadFromDisk(creatingIfMissing: false)
    }

    private func loadFromDisk(creatingIfMissing: Bool) {
        do {
            try catalogIO.prepareDirectory()
            if !catalogIO.fileExists() {
                if creatingIfMissing {
                    applyPlaces(Self.fixturePlaces(), selectFirst: true)
                    persist()
                }
                return
            }
            let document = try catalogIO.load()
            applyPlaces(document.places, selectFirst: selectedID == nil)
            catalogError = nil
        } catch {
            catalogError = error.localizedDescription
        }
    }

    private func persist() {
        do {
            try catalogIO.save(
                CatalogDocument(version: CatalogCodec.currentVersion, places: places)
            )
            catalogError = nil
        } catch {
            catalogError = error.localizedDescription
        }
    }

    private func applyPlaces(_ incoming: [Place], selectFirst: Bool) {
        let incomingIDs = Set(incoming.map(\.id))
        tabStates = tabStates.filter { incomingIDs.contains($0.key) }
        paneEpoch = paneEpoch.filter { incomingIDs.contains($0.key) }
        for place in incoming where tabStates[place.id] == nil {
            tabStates[place.id] = .idle
        }
        places = incoming
        if selectFirst || selectedID.map(incomingIDs.contains) != true {
            selectedID = incoming.first?.id
        }
    }

    private func startWatching() {
        do {
            try catalogIO.startWatching { [weak self] in
                Task { @MainActor in
                    self?.reloadFromDisk()
                }
            }
        } catch {
            catalogError = error.localizedDescription
        }
    }

    static func fixturePlaces() -> [Place] {
        let user = NSUserName()
        return [
            Place(user: user, host: "riverwood", backend: .herdr),
            Place(user: user, host: "whiterun", backend: .tmux, session: "foo"),
        ]
    }
}

struct PlaceEditorState: Hashable {
    var originID: UUID?
    var user: String
    var host: String
    var backend: Backend
    var session: String
    var label: String
}
