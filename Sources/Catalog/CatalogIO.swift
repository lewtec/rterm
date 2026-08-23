import Foundation

final class CatalogIO {
    let directoryURL: URL
    private let legacyFileURL: URL
    private let fileManager: FileManager
    private var directoryFileDescriptor: CInt = -1
    private var directorySource: DispatchSourceFileSystemObject?
    private var ignoreUntil: Date?
    private var debounceWork: DispatchWorkItem?
    private let ioQueue = DispatchQueue(label: "br.tec.lew.rterm.catalog")

    init(
        directoryURL: URL = CatalogPaths.placesDirectory,
        legacyFileURL: URL = CatalogPaths.placesFile,
        fileManager: FileManager = .default
    ) {
        self.directoryURL = directoryURL
        self.legacyFileURL = legacyFileURL
        self.fileManager = fileManager
    }

    convenience init(fileURL: URL, fileManager: FileManager = .default) {
        let directory = fileURL.deletingLastPathComponent()
            .appendingPathComponent("places", isDirectory: true)
        self.init(directoryURL: directory, legacyFileURL: fileURL, fileManager: fileManager)
    }

    deinit {
        stopWatching()
    }

    func prepareDirectory() throws {
        try fileManager.createDirectory(
            at: directoryURL,
            withIntermediateDirectories: true
        )
    }

    func fileExists() -> Bool {
        !placeFiles().isEmpty || fileManager.fileExists(atPath: legacyFileURL.path)
    }

    var pendingPlaceFile: URL {
        directoryURL.deletingLastPathComponent().appendingPathComponent("pending-place")
    }

    func load() throws -> CatalogDocument {
        try prepareDirectory()
        let files = placeFiles()
        if files.isEmpty, fileManager.fileExists(atPath: legacyFileURL.path) {
            let data = try Data(contentsOf: legacyFileURL)
            let text = String(decoding: data, as: UTF8.self)
            let document = try CatalogCodec.decode(text)
            try save(document)
            try? fileManager.removeItem(at: legacyFileURL)
            return document
        }
        var places: [Place] = []
        for file in files {
            let text = String(decoding: try Data(contentsOf: file), as: UTF8.self)
            places.append(try CatalogCodec.decodePlaceFile(text))
        }
        return CatalogDocument(version: CatalogCodec.currentVersion, places: places)
    }

    func save(_ document: CatalogDocument) throws {
        try prepareDirectory()
        ignoreUntil = Date().addingTimeInterval(0.4)
        let written = Set(
            try document.places.enumerated().map { index, place -> URL in
                let url = directoryURL.appendingPathComponent(PlaceFile.name(index: index, place: place))
                let data = Data(CatalogCodec.encodePlaceFile(place).utf8)
                try data.write(to: url, options: .atomic)
                return url.standardizedFileURL
            }
        )
        for file in placeFiles() where !written.contains(file.standardizedFileURL) {
            try? fileManager.removeItem(at: file)
        }
    }

    func startWatching(onChange: @escaping @Sendable () -> Void) throws {
        stopWatching()
        try prepareDirectory()
        let path = directoryURL.path
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

    private func placeFiles() -> [URL] {
        let urls = (try? fileManager.contentsOfDirectory(
            at: directoryURL,
            includingPropertiesForKeys: [.nameKey],
            options: [.skipsHiddenFiles]
        )) ?? []
        return urls
            .filter { $0.pathExtension == PlaceFile.pathExtension }
            .sorted { $0.lastPathComponent.localizedStandardCompare($1.lastPathComponent) == .orderedAscending }
    }
}

private extension POSIXError {
    static func fromErrno() -> POSIXError? {
        POSIXErrorCode(rawValue: errno).map { POSIXError($0) }
    }
}
