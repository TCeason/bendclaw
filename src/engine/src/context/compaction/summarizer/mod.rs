//! Summarizer — pluggable marker generation strategy (rule-based or LLM).

pub mod llm;
pub mod mode;
pub mod prompt;
pub mod rule_based;
pub mod serialize;
pub mod types;

pub use mode::SummarizerContext;
pub use mode::SummarizerMode;
pub use types::SummarizerError;
pub use types::SummarizerInput;
pub use types::SummarizerOutput;
