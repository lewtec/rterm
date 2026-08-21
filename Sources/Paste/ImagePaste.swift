import AppKit
import Foundation

enum ImagePasteError: Error, Equatable {
    case tooLarge
    case writeFailed
    case uploadFailed
}

enum ImagePasteGesture: Equatable {
    case commandV
    case controlV
}

enum ImagePastePlan: Equatable {
    case ignore
    case deliver
}

enum ImagePaste {
    static let maxBytes = 10 * 1024 * 1024

    static func plan(
        hasImage: Bool,
        hasText: Bool,
        isLocal: Bool,
        gesture: ImagePasteGesture
    ) -> ImagePastePlan {
        guard hasImage, hasText == false else {
            return .ignore
        }
        switch gesture {
        case .commandV:
            return .deliver
        case .controlV:
            return isLocal ? .ignore : .deliver
        }
    }

    static func hostPath(id: UUID) -> String {
        "/tmp/rterm-paste-\(id.uuidString).png"
    }

    static func isControlV(_ event: NSEvent) -> Bool {
        let flags = event.modifierFlags.intersection([.command, .control, .option, .shift])
        guard flags == .control else {
            return false
        }
        return event.charactersIgnoringModifiers?.lowercased() == "v"
    }

    static func snapshot(_ board: NSPasteboard) -> (hasImage: Bool, hasText: Bool, png: Data?) {
        let text = board.string(forType: .string)?
            .trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        let hasText = text.isEmpty == false
        let png = pngData(from: board)
        return (png != nil, hasText, png)
    }

    static func deliver(
        png: Data,
        place: Place,
        pasteID: UUID = UUID(),
        fileManager: FileManager = .default,
        upload: (Data, [String]) throws -> Void = ImagePaste.sshCat
    ) -> Result<String, ImagePasteError> {
        if png.count > maxBytes {
            return .failure(.tooLarge)
        }
        let path = hostPath(id: pasteID)
        if place.isLocal {
            do {
                try png.write(to: URL(fileURLWithPath: path), options: .atomic)
                return .success(path)
            } catch {
                return .failure(.writeFailed)
            }
        }
        guard fileManager.fileExists(atPath: Driver.controlPath(for: place)) else {
            return .failure(.uploadFailed)
        }
        do {
            try upload(png, Driver.uploadArguments(for: place, remotePath: path))
            return .success(path)
        } catch {
            return .failure(.uploadFailed)
        }
    }

    static func pngData(from board: NSPasteboard) -> Data? {
        if let png = board.data(forType: .png) {
            return png
        }
        guard let image = NSImage(pasteboard: board),
              let tiff = image.tiffRepresentation,
              let rep = NSBitmapImageRep(data: tiff)
        else {
            return nil
        }
        return rep.representation(using: .png, properties: [:])
    }

    static func sshCat(_ data: Data, arguments: [String]) throws {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: Driver.sshExecutable)
        process.arguments = arguments
        let input = Pipe()
        process.standardInput = input
        process.standardOutput = Pipe()
        process.standardError = Pipe()
        try process.run()
        defer {
            if process.isRunning {
                process.terminate()
            }
        }
        try input.fileHandleForWriting.write(contentsOf: data)
        try input.fileHandleForWriting.close()
        process.waitUntilExit()
        if process.terminationStatus != 0 {
            throw ImagePasteError.uploadFailed
        }
    }
}
