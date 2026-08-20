import Foundation
import SwiftUI

@MainActor
final class AppStore: ObservableObject {
    @Published var places: [Place]
    @Published var selectedID: UUID?
    @Published var tabStates: [UUID: TabState] = [:]
    @Published var paneEpoch: [UUID: Int] = [:]
    @Published var editor: PlaceEditorState?
    @Published var debugOverlay = false

    init(places: [Place] = AppStore.fixturePlaces()) {
        self.places = places
        self.selectedID = places.first?.id
        for place in places {
            tabStates[place.id] = .idle
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
        guard !draft.host.isEmpty, !draft.user.isEmpty else { return }
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

        if let index = places.firstIndex(where: { $0.id == place.id }) {
            places[index] = place
        } else {
            places.append(place)
            tabStates[place.id] = .idle
        }
        selectedID = place.id
        editor = nil
    }

    func delete(_ id: UUID) {
        places.removeAll { $0.id == id }
        tabStates[id] = nil
        paneEpoch[id] = nil
        if selectedID == id {
            selectedID = places.first?.id
        }
        if editor?.originID == id {
            editor = nil
        }
    }

    func markFailed(_ id: UUID, message: String) {
        tabStates[id] = .failed(message)
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
