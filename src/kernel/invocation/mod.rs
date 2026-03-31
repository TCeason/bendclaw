//! Invocation layer — orthogonal request model for running agents.

pub mod execute;
pub mod request;
pub mod session_route;

pub use execute::validate;
pub use request::*;
