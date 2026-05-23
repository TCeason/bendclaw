//! Goal subsystem — autonomous task execution with stop verification.

pub mod command;
pub mod coordinator;
pub mod display;
pub mod prompt;
pub mod result_tool;
pub mod runtime;
pub mod todo_write_tool;
pub mod update_tasks_tool;
pub mod validate;
pub mod verifier;
pub mod verifier_agent;

pub use coordinator::GoalCoordinator;
pub use todo_write_tool::TodoMeta;
pub use todo_write_tool::TodoState;
pub use todo_write_tool::TodoWriteTool;
pub use verifier::verify_goal;
pub use verifier::GoalVerdict;
