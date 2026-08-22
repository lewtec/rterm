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
    static let chunkBytes = 64 * 1024

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
        pasteID: UUID = UUIDV7.generate(),
        fileManager: FileManager = .default,
        onProgress: ((Int, Int) -> Void)? = nil,
        upload: ((Data, [String]) throws -> Void)? = nil
    ) -> Result<String, ImagePasteError> {
        if png.count > maxBytes {
            return .failure(.tooLarge)
        }
        let path = hostPath(id: pasteID)
        onProgress?(0, png.count)
        if place.isLocal {
            do {
                try png.write(to: URL(fileURLWithPath: path), options: .atomic)
                onProgress?(png.count, png.count)
                return .success(path)
            } catch {
                return .failure(.writeFailed)
            }
        }
        guard fileManager.fileExists(atPath: Driver.controlPath(for: place)) else {
            return .failure(.uploadFailed)
        }
        do {
            let arguments = Driver.uploadArguments(for: place, remotePath: path)
            if let upload {
                try upload(png, arguments)
                onProgress?(png.count, png.count)
            } else {
                try sshCat(png, arguments: arguments, onProgress: onProgress)
            }
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

    static func writeChunks(
        _ data: Data,
        to handle: FileHandle,
        onProgress: ((Int, Int) -> Void)? = nil
    ) throws {
        var sent = 0
        while sent < data.count {
            let end = min(sent + chunkBytes, data.count)
            try handle.write(contentsOf: data[sent..<end])
            sent = end
            onProgress?(sent, data.count)
        }
    }

    static func sshCat(
        _ data: Data,
        arguments: [String],
        onProgress: ((Int, Int) -> Void)? = nil
    ) throws {
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
        try writeChunks(data, to: input.fileHandleForWriting, onProgress: onProgress)
        try input.fileHandleForWriting.close()
        process.waitUntilExit()
        if process.terminationStatus != 0 {
            throw ImagePasteError.uploadFailed
        }
    }
}
