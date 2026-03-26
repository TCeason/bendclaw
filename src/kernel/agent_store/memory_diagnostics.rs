pub(crate) fn log_memory_write_failed(
    user_id: &str,
    key: &str,
    scope: &str,
    error: &impl std::fmt::Display,
) {
    crate::observability::log::slog!(
        error,
        "memory",
        "write_failed",
        user_id,
        key = %key,
        scope = %scope,
        error = %error,
    );
}

pub(crate) fn log_memory_search(user_id: &str, query: &str, results: usize) {
    crate::observability::log::slog!(info, "memory", "search", user_id, query, results,);
}

pub(crate) fn log_memory_get(user_id: &str, key: &str, found: bool) {
    crate::observability::log::slog!(info, "memory", "get", user_id, key, found,);
}

pub(crate) fn log_memory_delete_failed(user_id: &str, id: &str, error: &impl std::fmt::Display) {
    crate::observability::log::slog!(error, "memory", "delete_failed", user_id, id, error = %error,);
}
