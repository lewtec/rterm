use std::io::Read;
use std::time::{SystemTime, UNIX_EPOCH};

use uuid::Uuid;

pub fn generate() -> Uuid {
    generate_at(SystemTime::now())
}

pub fn generate_at(now: SystemTime) -> Uuid {
    let millis = now
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    let millis = u64::try_from(millis).unwrap_or(u64::MAX);
    generate_millis(millis)
}

pub fn generate_millis(millis: u64) -> Uuid {
    let mut bytes = [0u8; 16];
    bytes[0] = ((millis >> 40) & 0xff) as u8;
    bytes[1] = ((millis >> 32) & 0xff) as u8;
    bytes[2] = ((millis >> 24) & 0xff) as u8;
    bytes[3] = ((millis >> 16) & 0xff) as u8;
    bytes[4] = ((millis >> 8) & 0xff) as u8;
    bytes[5] = (millis & 0xff) as u8;
    if let Ok(mut file) = std::fs::File::open("/dev/urandom") {
        let _ = file.read_exact(&mut bytes[6..]);
    }
    bytes[6] = (bytes[6] & 0x0f) | 0x70;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

pub fn version_of(id: Uuid) -> u8 {
    id.as_bytes()[6] >> 4
}

pub fn uuid_upper(id: Uuid) -> String {
    id.to_string().to_ascii_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn default_paste_id_is_version_7() {
        let now = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let id = generate_at(now);
        assert_eq!(version_of(id), 7);
        assert_eq!(id.as_bytes()[8] >> 6, 0b10);
        let bytes = id.as_bytes();
        let millis = (u64::from(bytes[0]) << 40)
            | (u64::from(bytes[1]) << 32)
            | (u64::from(bytes[2]) << 24)
            | (u64::from(bytes[3]) << 16)
            | (u64::from(bytes[4]) << 8)
            | u64::from(bytes[5]);
        assert_eq!(millis, 1_700_000_000_000);
    }
}
