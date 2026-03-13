use axum::routing::get;
use axum::Router;

use super::routes;
use super::state::AdminState;

pub fn admin_router(state: AdminState) -> Router {
    Router::new()
        .route("/admin/v1/can_suspend", get(routes::can_suspend))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use axum::body::to_bytes;
    use axum::http::Request;
    use tower::ServiceExt;

    use super::*;
    use crate::kernel::runtime::SuspendStatus;
    use crate::service::test_support::test_runtime;

    #[tokio::test]
    async fn can_suspend_reports_idle_runtime() {
        let runtime = test_runtime("admin-idle");
        let router = admin_router(AdminState {
            runtime: runtime.clone(),
        });

        let response = router
            .oneshot(
                Request::builder()
                    .uri("/admin/v1/can_suspend")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .expect("admin response");

        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let payload: SuspendStatus = serde_json::from_slice(&body).expect("admin payload");
        assert_eq!(payload, SuspendStatus {
            can_suspend: true,
            active_sessions: 0,
            active_tasks: 0,
        });
    }

    #[tokio::test]
    async fn can_suspend_reports_active_tasks() {
        let runtime = test_runtime("admin-active-task");
        let _task = runtime.track_task();
        let router = admin_router(AdminState {
            runtime: runtime.clone(),
        });

        let response = router
            .oneshot(
                Request::builder()
                    .uri("/admin/v1/can_suspend")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .expect("admin response");

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let payload: SuspendStatus = serde_json::from_slice(&body).expect("admin payload");
        assert_eq!(payload, SuspendStatus {
            can_suspend: false,
            active_sessions: 0,
            active_tasks: 1,
        });
    }
}
