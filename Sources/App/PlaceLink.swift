import AppKit
import Foundation
import UniformTypeIdentifiers

enum PlaceLink {
    static let scheme = "rterm"
    static let host = "place"
    static let contentTypeIdentifier = "br.tec.lew.rterm.place"
    static let fileExtension = "rterm-place"

    static func url(for place: Place) -> URL {
        url(forKey: place.switchKey)
    }

    static func url(forKey key: String) -> URL {
        let path = key.addingPercentEncoding(withAllowedCharacters: Self.pathCharacters) ?? key
        return URL(string: "\(scheme)://\(host)/\(path)")!
    }

    static func switchKey(from url: URL) -> String? {
        if url.isFileURL {
            return switchKey(fromFile: url)
        }
        guard url.scheme == scheme, url.host == host else {
            return nil
        }
        let raw = url.path.trimmingCharacters(in: CharacterSet(charactersIn: "/"))
        guard !raw.isEmpty else {
            return nil
        }
        return raw.removingPercentEncoding ?? raw
    }

    static func switchKey(fromFile url: URL) -> String? {
        guard url.pathExtension == fileExtension || url.pathExtension == PlaceFile.pathExtension else {
            return nil
        }
        guard let text = try? String(contentsOf: url, encoding: .utf8) else {
            return nil
        }
        if let place = try? CatalogCodec.decodePlaceFile(text) {
            return place.switchKey
        }
        return nil
    }

    private static let pathCharacters: CharacterSet = {
        var allowed = CharacterSet.urlPathAllowed
        allowed.insert(charactersIn: "@:()")
        return allowed
    }()

    static func claimHandler() {
        NSWorkspace.shared.setDefaultApplication(
            at: Bundle.main.bundleURL,
            toOpenURLsWithScheme: scheme
        ) { _ in }
        NSWorkspace.shared.setDefaultApplication(
            at: Bundle.main.bundleURL,
            toOpen: UTType(exportedAs: contentTypeIdentifier)
        ) { _ in }
    }
}
