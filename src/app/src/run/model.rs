use std::sync::Arc;

pub use crate::storage::model::RunEvent;
pub use crate::storage::model::RunEventKind;
pub use crate::storage::model::RunMeta;
pub use crate::storage::model::RunStatus;

pub type RunEventArc = Arc<RunEvent>;
