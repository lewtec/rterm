import Foundation

enum CatalogCodecError: Error, Equatable, LocalizedError {
    case unsupportedVersion(Int)
    case unexpectedLine(Int, String)
    case missingField(String, Int)
    case invalidValue(String, Int)
    case sessionRequired(Int)

    var errorDescription: String? {
        switch self {
        case .unsupportedVersion(let version):
            return "catalog version \(version) is not supported"
        case .unexpectedLine(let line, let text):
            return "catalog line \(line): unexpected \(text)"
        case .missingField(let field, let line):
            return "catalog line \(line): missing \(field)"
        case .invalidValue(let field, let line):
            return "catalog line \(line): invalid \(field)"
        case .sessionRequired(let line):
            return "catalog line \(line): tmux and screen require session"
        }
    }
}

enum CatalogCodec {
    static let currentVersion = 1

    static func decode(_ text: String) throws -> CatalogDocument {
        var version: Int?
        var places: [Place] = []
        var current: PartialPlace?
        var currentStart = 0

        func flushCurrent() throws {
            guard let partial = current else { return }
            places.append(try partial.finish(startingAt: currentStart))
            current = nil
        }

        for (offset, rawLine) in text.split(omittingEmptySubsequences: false, whereSeparator: \.isNewline)
            .enumerated()
        {
            let lineNumber = offset + 1
            let line = rawLine.trimmingCharacters(in: .whitespaces)
            if line.isEmpty || line.hasPrefix("#") {
                continue
            }
            if line == "[[place]]" {
                try flushCurrent()
                current = PartialPlace()
                currentStart = lineNumber
                continue
            }
            guard let equals = line.firstIndex(of: "=") else {
                throw CatalogCodecError.unexpectedLine(lineNumber, line)
            }
            let key = line[..<equals].trimmingCharacters(in: .whitespaces)
            let rawValue = line[line.index(after: equals)...].trimmingCharacters(in: .whitespaces)

            if current == nil {
                if key == "version" {
                    guard let parsed = Int(rawValue) else {
                        throw CatalogCodecError.invalidValue("version", lineNumber)
                    }
                    version = parsed
                    continue
                }
                throw CatalogCodecError.unexpectedLine(lineNumber, line)
            }

            let value = try decodeScalar(rawValue, line: lineNumber)
            switch key {
            case "id":
                guard let id = UUID(uuidString: value) else {
                    throw CatalogCodecError.invalidValue("id", lineNumber)
                }
                current?.id = id
            case "user":
                current?.user = value
            case "host":
                current?.host = value
            case "backend":
                guard let backend = Backend(rawValue: value) else {
                    throw CatalogCodecError.invalidValue("backend", lineNumber)
                }
                current?.backend = backend
            case "session":
                current?.session = value
            case "label":
                current?.label = value
            default:
                throw CatalogCodecError.unexpectedLine(lineNumber, line)
            }
        }

        try flushCurrent()
        let resolvedVersion = version ?? 0
        guard resolvedVersion == currentVersion else {
            throw CatalogCodecError.unsupportedVersion(resolvedVersion)
        }
        return CatalogDocument(version: resolvedVersion, places: places)
    }

    static func encode(_ document: CatalogDocument) -> String {
        var lines = [
            "version = \(document.version)",
            "",
        ]
        for place in document.places {
            lines.append("[[place]]")
            lines.append("id = \(quote(place.id.uuidString))")
            if !place.user.isEmpty {
                lines.append("user = \(quote(place.user))")
            }
            if !place.isLocal, !place.host.isEmpty {
                lines.append("host = \(quote(place.host))")
            }
            lines.append("backend = \(quote(place.backend.rawValue))")
            if place.backend != .herdr, let session = place.session, !session.isEmpty {
                lines.append("session = \(quote(session))")
            }
            if let label = place.label, !label.isEmpty {
                lines.append("label = \(quote(label))")
            }
            lines.append("")
        }
        return lines.joined(separator: "\n")
    }

    private static func decodeScalar(_ raw: String, line: Int) throws -> String {
        if raw.hasPrefix("\"") {
            guard raw.hasSuffix("\""), raw.count >= 2 else {
                throw CatalogCodecError.invalidValue("string", line)
            }
            let inner = raw.dropFirst().dropLast()
            var out = ""
            var escape = false
            for character in inner {
                if escape {
                    switch character {
                    case "n": out.append("\n")
                    case "t": out.append("\t")
                    case "\\": out.append("\\")
                    case "\"": out.append("\"")
                    default:
                        throw CatalogCodecError.invalidValue("string", line)
                    }
                    escape = false
                } else if character == "\\" {
                    escape = true
                } else {
                    out.append(character)
                }
            }
            if escape {
                throw CatalogCodecError.invalidValue("string", line)
            }
            return out
        }
        return raw
    }

    private static func quote(_ value: String) -> String {
        var escaped = "\""
        for character in value {
            switch character {
            case "\\": escaped += "\\\\"
            case "\"": escaped += "\\\""
            case "\n": escaped += "\\n"
            case "\t": escaped += "\\t"
            default: escaped.append(character)
            }
        }
        escaped += "\""
        return escaped
    }
}

private struct PartialPlace {
    var id: UUID?
    var user: String?
    var host: String?
    var backend: Backend?
    var session: String?
    var label: String?

    func finish(startingAt line: Int) throws -> Place {
        guard let id else { throw CatalogCodecError.missingField("id", line) }
        guard let backend else { throw CatalogCodecError.missingField("backend", line) }
        var trimmedHost = (host ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
        var trimmedUser = (user ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
        if Place.isLoopbackHost(trimmedHost) {
            trimmedHost = ""
        } else if trimmedUser.isEmpty {
            throw CatalogCodecError.missingField("user", line)
        }
        let trimmedSession = session?.trimmingCharacters(in: .whitespacesAndNewlines)
        if backend != .herdr && (trimmedSession == nil || trimmedSession?.isEmpty == true) {
            throw CatalogCodecError.sessionRequired(line)
        }
        let trimmedLabel = label?.trimmingCharacters(in: .whitespacesAndNewlines)
        return Place(
            id: id,
            user: trimmedUser,
            host: trimmedHost,
            backend: backend,
            session: backend == .herdr ? nil : trimmedSession,
            label: (trimmedLabel?.isEmpty == false) ? trimmedLabel : nil
        )
    }
}
