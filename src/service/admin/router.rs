use axum::routing::get;
use axum::Router;

use super::routes;
use super::state::AdminState;

pub fn admin_router(state: AdminState) -> Router {
    Router::new()
        .route("/admin/v1/can_suspend", get(routes::can_suspend))
        .with_state(state)
}
