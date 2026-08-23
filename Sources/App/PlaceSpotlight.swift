import CoreSpotlight
import Foundation

enum PlaceSpotlight {
    @MainActor
    static func publish() {
        importSpotlightDirectory()
        guard CSSearchableIndex.isIndexingAvailable() else {
            return
        }
        CSSearchableIndex.default().deleteAllSearchableItems(completionHandler: nil)
    }

    private static func importSpotlightDirectory() {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/mdimport")
        process.arguments = [CatalogPaths.spotlightDirectory.path]
        try? process.run()
    }
}
