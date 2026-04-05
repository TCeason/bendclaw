pub fn new_session_id() -> String {
    uuid::Uuid::now_v7().to_string()
}
