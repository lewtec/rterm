import AppKit
import Foundation
import SwiftUI

@MainActor
@Observable
final class AppStore {
    var places: [Place] = []
    var selectedID: UUID?
    var attaches: [UUID: PlaceAttach] = [:]
    var editor: PlaceEditorState?
    var debugOverlay = false
    var catalogError: String?

    @ObservationIgnored
    private var connectProgressByID: [UUID: ConnectProgress] = [:]
    @ObservationIgnored
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
        let wasIdle = attaches[id]?.phase == .idle || attaches[id] == nil
        mutateAttach(id) { $0.select() }
        if wasIdle {
            ensureProgress(id).begin(clearCrumbs: true)
        }
    }

    func selectPlace(at index: Int) {
        guard places.indices.contains(index) else { return }
        select(places[index].id)
    }

    func reconnect(_ id: UUID) {
        mutateAttach(id) { $0.reconnect() }
        ensureProgress(id).begin(clearCrumbs: false)
        debugOverlay = false
    }

    func reconnectSelected() {
        guard let selectedID else { return }
        reconnect(selectedID)
    }

    func dropAllConnections() {
        debugOverlay = false
        for id in places.map(\.id) {
            mutateAttach(id) { $0.sleep() }
            connectProgressByID[id]?.reset()
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
            if draft.user.isEmpty {
                draft.user = NSUserName()
            }
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
            shouldReattach = places[index] != place && attaches[place.id]?.phase != .idle
            places[index] = place
        } else {
            shouldReattach = false
            places.append(place)
            attaches[place.id] = PlaceAttach()
            _ = ensureProgress(place.id)
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
        attaches[id] = nil
        connectProgressByID[id] = nil
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

    func handleExit(_ id: UUID, code: Int32?, generation: Int? = nil) {
        mutateAttach(id) { $0.handleExit(code, generation: generation) }
        if attaches[id]?.showsPane != true {
            connectProgressByID[id]?.finish()
        }
    }

    func connectProgress(for id: UUID) -> ConnectProgress? {
        connectProgressByID[id]
    }

    func tabState(for id: UUID) -> TabState {
        attaches[id]?.tabState ?? .idle
    }

    func noteConnectTimeout(_ id: UUID, generation: Int? = nil) {
        guard let attach = attaches[id] else {
            return
        }
        if let generation, attach.generation != generation {
            return
        }
        guard attach.isConnecting else {
            return
        }
        if attach.connectAttempt >= PlaceAttach.maxConnectAttempts {
            mutateAttach(id) { $0.failConnectTimeout() }
            return
        }
        let next = attach.connectAttempt + 1
        reconnect(id)
        mutateAttach(id) { $0.setConnectAttempt(next) }
    }

    func noteTTY(_ id: UUID) {
        mutateAttach(id) { $0.noteTTY() }
    }

    func noteProgress(_ id: UUID, _ headline: String) {
        mutateAttach(id) { $0.noteProgress(headline) }
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
        attaches = attaches.filter { incomingIDs.contains($0.key) }
        connectProgressByID = connectProgressByID.filter { incomingIDs.contains($0.key) }
        for place in incoming where attaches[place.id] == nil {
            attaches[place.id] = PlaceAttach()
        }
        for place in incoming {
            _ = ensureProgress(place.id)
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

    @discardableResult
    private func ensureProgress(_ id: UUID) -> ConnectProgress {
        if let existing = connectProgressByID[id] {
            return existing
        }
        let progress = ConnectProgress()
        progress.onHeadline = { [weak self] text in
            self?.noteProgress(id, text)
        }
        connectProgressByID[id] = progress
        return progress
    }

    private func mutateAttach(_ id: UUID, _ body: (inout PlaceAttach) -> Void) {
        var attach = attaches[id] ?? PlaceAttach()
        let before = attach
        body(&attach)
        guard attach != before else {
            return
        }
        attaches[id] = attach
    }

    static func fixturePlaces() -> [Place] {
        [
            Place(user: NSUserName(), host: "", backend: .herdr),
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
