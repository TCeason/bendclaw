use std::sync::Arc;

use crate::kernel::Runtime;

#[derive(Clone)]
pub struct AdminState {
    pub runtime: Arc<Runtime>,
}
