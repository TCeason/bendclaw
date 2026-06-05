/// Generate a new unique ID (UUID v7, time-ordered).
pub fn new_id() -> String {
    uuid::Uuid::now_v7().to_string()
}

/// Whether `id` is a well-formed session/run identifier.
///
/// IDs are joined into filesystem paths at the storage layer, so the only hard
/// requirement is that they cannot escape their parent directory. Allowing just
/// `[A-Za-z0-9_-]` rejects path separators (`/`, `\`) and dots (so `..` is
/// impossible) while accepting every identifier the system produces: UUID v7
/// from [`new_id`], legacy short hex, and dashed names. Enforced at the storage
/// layer so every caller is covered.
pub fn is_valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}
