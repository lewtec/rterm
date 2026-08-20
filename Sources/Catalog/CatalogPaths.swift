import Foundation

enum CatalogPaths {
    static var applicationSupportDirectory: URL {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("rterm", isDirectory: true)
    }

    static var placesFile: URL {
        applicationSupportDirectory.appendingPathComponent("places.toml")
    }
}
