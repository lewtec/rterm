import Foundation

final class CatalogIO {
    private let fileURL: URL
    private let fileManager: FileManager
    private var directoryFileDescriptor: CInt = -1
    private var directorySource: DispatchSourceFileSystemObject?
    private var ignoreUntil: Date?
    private var debounceWork: DispatchWorkItem?
    private let ioQueue = DispatchQueue(label: "br.tec.lew.rterm.catalog")

    init(fileURL: URL = CatalogPaths.placesFile, fileManager: FileManager = .default) {
        self.fileURL = fileURL
        self.fileManager = fileManager
    }

    deinit {
        stopWatching()
    }

    func prepareDirectory() throws {
        try fileManager.createDirectory(
            at: fileURL.deletingLastPathComponent(),
            withIntermediateDirectories: true
        )
    }

    func fileExists() -> Bool {
        fileManager.fileExists(atPath: fileURL.path)
    }

    func load() throws -> CatalogDocument {
        let data = try Data(contentsOf: fileURL)
        let text = String(decoding: data, as: UTF8.self)
        return try CatalogCodec.decode(text)
    }

    func save(_ document: CatalogDocument) throws {
        try prepareDirectory()
        let text = CatalogCodec.encode(document)
        guard let data = text.data(using: .utf8) else {
            throw CocoaError(.fileWriteInapplicableStringEncoding)
        }
        let directory = fileURL.deletingLastPathComponent()
        let temporary = directory.appendingPathComponent("places.toml.tmp")
        try data.write(to: temporary, options: .atomic)
        if fileManager.fileExists(atPath: fileURL.path) {
            _ = try fileManager.replaceItemAt(fileURL, withItemAt: temporary)
        } else {
            try fileManager.moveItem(at: temporary, to: fileURL)
        }
        ignoreUntil = Date().addingTimeInterval(0.4)
    }

    func startWatching(onChange: @escaping @Sendable () -> Void) throws {
        stopWatching()
        try prepareDirectory()
        let path = fileURL.deletingLastPathComponent().path
        let fd = open(path, O_EVTONLY)
        guard fd >= 0 else {
            throw POSIXError.fromErrno() ?? POSIXError(.EIO)
        }
        directoryFileDescriptor = fd
        let source = DispatchSource.makeFileSystemObjectSource(
            fileDescriptor: fd,
            eventMask: [.write, .rename, .delete, .extend, .attrib],
            queue: ioQueue
        )
        source.setEventHandler { [weak self] in
            guard let self else { return }
            if let ignoreUntil, Date() < ignoreUntil {
                return
            }
            self.debounceWork?.cancel()
            let work = DispatchWorkItem {
                onChange()
            }
            self.debounceWork = work
            self.ioQueue.asyncAfter(deadline: .now() + 0.15, execute: work)
        }
        source.setCancelHandler { [weak self] in
            guard let self, self.directoryFileDescriptor >= 0 else { return }
            close(self.directoryFileDescriptor)
            self.directoryFileDescriptor = -1
        }
        directorySource = source
        source.resume()
    }

    func stopWatching() {
        directorySource?.cancel()
        directorySource = nil
    }
}

private extension POSIXError {
    static func fromErrno() -> POSIXError? {
        POSIXErrorCode(rawValue: errno).map { POSIXError($0) }
    }
}
