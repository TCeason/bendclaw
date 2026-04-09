/// Generate a new unique ID (UUID v7, time-ordered).
pub fn new_id() -> String {
    uuid::Uuid::now_v7().to_string()
}
