//! Goal subsystem — autonomous task execution with stop verification.

pub mod command;
pub mod coordinator;
pub mod display;
pub mod prompt;
pub mod result_tool;
pub mod runtime;
pub mod update_tasks_tool;
pub mod validate;
pub mod verifier;
pub mod verifier_agent;

pub use coordinator::GoalCoordinator;
pub use verifier::verify_goal;
pub use verifier::GoalVerdict;
