import Foundation

enum PlaceFile {
    static let pathExtension = "rterm-place"

    static func name(index: Int, place: Place) -> String {
        let key = place.switchKey
            .replacingOccurrences(of: ":", with: "-")
            .replacingOccurrences(of: "/", with: "-")
        return String(format: "%02d-%@.%@", index, key, pathExtension)
    }
}
