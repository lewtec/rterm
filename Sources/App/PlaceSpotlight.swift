import CoreSpotlight
import Foundation

enum PlaceSpotlight {
    static let domainIdentifier = "places"

    @MainActor
    static func publish() {
        RtermShortcuts.updateAppShortcutParameters()
        importSpotlightDirectory()
        guard CSSearchableIndex.isIndexingAvailable() else {
            return
        }
        CSSearchableIndex.default().deleteSearchableItems(
            withDomainIdentifiers: [domainIdentifier],
            completionHandler: nil
        )
        if #available(macOS 15.4, *) {
            Task {
                try? await CSSearchableIndex.default().deleteAppEntities(ofType: PlaceEntity.self)
            }
        }
    }

    private static func importSpotlightDirectory() {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/mdimport")
        process.arguments = [CatalogPaths.spotlightDirectory.path]
        try? process.run()
    }
}
