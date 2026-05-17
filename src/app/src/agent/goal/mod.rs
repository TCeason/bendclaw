//! Goal subsystem — autonomous task execution with per-turn evaluation.

pub mod command;
pub mod coordinator;
pub mod display;
pub mod evaluator;
pub mod evaluator_agent;
pub mod policy;
pub mod prompt;
pub mod result_tool;
pub mod runtime;
pub mod validate;

pub use coordinator::GoalCoordinator;
pub use evaluator::evaluate_goal;
pub use evaluator::EvalVerdict;
pub use policy::decide;
pub use policy::Decision;
