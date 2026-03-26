pub(crate) fn log_cluster_client_deregister_failed(
    node_id: &str,
    http_status: reqwest::StatusCode,
    body: &str,
) {
    crate::observability::log::slog!(
        warn,
        "cluster",
        "deregister_failed",
        node_id = %node_id,
        http_status = %http_status,
        body = %body,
    );
}
