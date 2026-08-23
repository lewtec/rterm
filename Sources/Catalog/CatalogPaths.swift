import Foundation

enum CatalogPaths {
    static var applicationSupportDirectory: URL {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("rterm", isDirectory: true)
    }

    static var cachesDirectory: URL {
        FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("rterm", isDirectory: true)
    }

    static var placesFile: URL {
        applicationSupportDirectory.appendingPathComponent("places.toml")
    }

    static var pendingPlaceFile: URL {
        applicationSupportDirectory.appendingPathComponent("pending-place")
    }

    static var placesDirectory: URL {
        FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("rterm", isDirectory: true)
    }

    static var spotlightDirectory: URL { placesDirectory }
}
