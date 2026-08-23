import AppIntents
import AppKit
import CoreSpotlight

@MainActor
enum AppRuntime {
    static weak var store: AppStore?

    static func places() -> [Place] {
        if let store {
            return store.places
        }
        return (try? CatalogIO().load())?.places ?? []
    }

    static func handle(_ url: URL) {
        guard let key = PlaceLink.switchKey(from: url) else {
            return
        }
        PendingPlace.write(key, to: CatalogPaths.pendingPlaceFile)
        store?.openPlace(switchKey: key)
        NSApp.activate()
        for window in NSApp.windows {
            window.makeKeyAndOrderFront(nil)
        }
    }
}

struct PlaceEntity: AppEntity {
    static let typeDisplayRepresentation: TypeDisplayRepresentation = "Place"
    static let defaultQuery = PlaceEntityQuery()

    var id: UUID
    var name: String
    var switchKey: String

    init(_ place: Place) {
        id = place.id
        name = place.displayLabel
        switchKey = place.switchKey
    }

    var displayRepresentation: DisplayRepresentation {
        DisplayRepresentation(
            title: LocalizedStringResource(stringLiteral: name),
            subtitle: "rterm",
            image: .init(systemName: "terminal")
        )
    }
}

struct PlaceEntityQuery: EntityQuery {
    func entities(for identifiers: [PlaceEntity.ID]) async throws -> [PlaceEntity] {
        let wanted = Set(identifiers)
        return await MainActor.run {
            AppRuntime.places().filter { wanted.contains($0.id) }.map(PlaceEntity.init)
        }
    }

    func suggestedEntities() async throws -> [PlaceEntity] {
        await MainActor.run { AppRuntime.places().map(PlaceEntity.init) }
    }
}

extension PlaceEntityQuery: EnumerableEntityQuery {
    func allEntities() async throws -> [PlaceEntity] {
        try await suggestedEntities()
    }
}

extension PlaceEntityQuery: EntityStringQuery {
    func entities(matching string: String) async throws -> [PlaceEntity] {
        let needle = string.trimmingCharacters(in: .whitespacesAndNewlines)
        return await MainActor.run {
            AppRuntime.places()
                .filter { needle.isEmpty || $0.displayLabel.localizedCaseInsensitiveContains(needle) }
                .map(PlaceEntity.init)
        }
    }
}

struct OpenPlaceQueryIntent: AppIntent {
    static let title: LocalizedStringResource = "Open Place"
    static let description: IntentDescription = "Open a place by user@host:backend."
    static let persistentIdentifier = "br.tec.lew.rterm.open-place-query"
    static let openAppWhenRun = true
    static let isDiscoverable = true
    static let authenticationPolicy: IntentAuthenticationPolicy = .alwaysAllowed

    @available(macOS 26.0, *)
    static var supportedModes: IntentModes { .foreground }

    @Parameter(title: "Place")
    var query: String

    static var parameterSummary: some ParameterSummary {
        Summary("Open \(\.$query)")
    }

    init() {}

    init(query: String) {
        self.query = query
    }

    @MainActor
    func perform() async throws -> some IntentResult {
        AppRuntime.handle(PlaceLink.url(forKey: query))
        return .result()
    }
}

struct OpenPlaceIntent: AppIntent {
    static let title: LocalizedStringResource = "Open Place"
    static let description: IntentDescription = "Switch to a saved place."
    static let persistentIdentifier = "br.tec.lew.rterm.open-place"
    static let openAppWhenRun = true
    static let isDiscoverable = true
    static let authenticationPolicy: IntentAuthenticationPolicy = .alwaysAllowed

    @available(macOS 26.0, *)
    static var supportedModes: IntentModes { .foreground }

    @Parameter(title: "Place")
    var target: PlaceEntity

    static var parameterSummary: some ParameterSummary {
        Summary("Open \(\.$target)")
    }

    init() {}

    init(target: PlaceEntity) {
        self.target = target
    }

    @MainActor
    func perform() async throws -> some IntentResult {
        AppRuntime.handle(PlaceLink.url(forKey: target.switchKey))
        return .result()
    }
}

struct RtermShortcuts: AppShortcutsProvider {
    static var appShortcuts: [AppShortcut] {
        AppShortcut(
            intent: OpenPlaceQueryIntent(),
            phrases: [
                "Open place in \(.applicationName)",
                "/\(.applicationName)",
            ],
            shortTitle: "Open Place",
            systemImageName: "terminal"
        )
        AppShortcut(
            intent: OpenPlaceIntent(),
            phrases: [
                "Open \(\.$target) in \(.applicationName)",
                "Switch to \(\.$target) in \(.applicationName)",
                "\(.applicationName) \(\.$target)",
            ],
            shortTitle: "Open Saved Place",
            systemImageName: "terminal"
        )
    }
}

@available(macOS 15.4, *)
extension PlaceEntity: IndexedEntity {
    var hideInSpotlight: Bool { true }
}
