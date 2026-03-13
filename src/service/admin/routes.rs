use axum::extract::State;
use axum::Json;

use super::state::AdminState;
use crate::kernel::runtime::SuspendStatus;

pub async fn can_suspend(State(state): State<AdminState>) -> Json<SuspendStatus> {
    Json(state.runtime.suspend_status())
}
