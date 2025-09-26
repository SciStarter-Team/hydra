use ulid::Ulid;
use uuid::Uuid;

/// Convert a ULID to a UUID for WebAuthn operations.
///
/// This conversion is deterministic - the same ULID will always produce
/// the same UUID. Both ULID and UUID are 128-bit identifiers, so we can
/// safely use the ULID bytes directly as UUID bytes.
pub fn ulid_to_uuid(ulid: Ulid) -> Uuid {
    let ulid_bytes = ulid.to_bytes();
    Uuid::from_bytes(ulid_bytes)
}

/// Convert a UUID back to a ULID.
///
/// This is the reverse of ulid_to_uuid() and allows us to get back
/// to our original ULID from a UUID used in WebAuthn operations.
pub fn uuid_to_ulid(uuid: Uuid) -> Ulid {
    let uuid_bytes = uuid.into_bytes();
    Ulid::from_bytes(uuid_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ulid_uuid_roundtrip() {
        let original_ulid = Ulid::new();

        // Convert ULID to UUID and back
        let uuid = ulid_to_uuid(original_ulid);
        let recovered_ulid = uuid_to_ulid(uuid);

        // Should be identical
        assert_eq!(original_ulid, recovered_ulid);
    }

    #[test]
    fn test_conversion_is_deterministic() {
        let ulid = Ulid::new();

        // Multiple conversions should produce the same result
        let uuid1 = ulid_to_uuid(ulid);
        let uuid2 = ulid_to_uuid(ulid);

        assert_eq!(uuid1, uuid2);
    }
}
