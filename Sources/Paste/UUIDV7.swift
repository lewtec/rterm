import Foundation

enum UUIDV7 {
    static func generate(now: Date = Date()) -> UUID {
        var bytes = [UInt8](repeating: 0, count: 16)
        let milliseconds = UInt64(now.timeIntervalSince1970 * 1_000)
        bytes[0] = UInt8(truncatingIfNeeded: milliseconds >> 40)
        bytes[1] = UInt8(truncatingIfNeeded: milliseconds >> 32)
        bytes[2] = UInt8(truncatingIfNeeded: milliseconds >> 24)
        bytes[3] = UInt8(truncatingIfNeeded: milliseconds >> 16)
        bytes[4] = UInt8(truncatingIfNeeded: milliseconds >> 8)
        bytes[5] = UInt8(truncatingIfNeeded: milliseconds)
        var rng = SystemRandomNumberGenerator()
        for index in 6..<16 {
            bytes[index] = UInt8.random(in: .min ... .max, using: &rng)
        }
        bytes[6] = (bytes[6] & 0x0F) | 0x70
        bytes[8] = (bytes[8] & 0x3F) | 0x80
        return UUID(uuid: (
            bytes[0], bytes[1], bytes[2], bytes[3],
            bytes[4], bytes[5], bytes[6], bytes[7],
            bytes[8], bytes[9], bytes[10], bytes[11],
            bytes[12], bytes[13], bytes[14], bytes[15]
        ))
    }

    static func version(of id: UUID) -> Int {
        Int(id.uuid.6 >> 4)
    }
}
