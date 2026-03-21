use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use crate::kernel::Runtime;

#[derive(Clone)]
pub struct AdminState {
    pub runtime: Arc<Runtime>,
    pub shutdown_token: CancellationToken,
}
