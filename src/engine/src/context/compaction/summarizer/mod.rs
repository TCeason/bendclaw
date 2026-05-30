//! Summarizer — LLM-generated compact memory summaries.

pub mod llm;
pub mod mode;
pub mod prompt;
pub mod serialize;
pub mod types;

pub use mode::SummarizerContext;
pub use mode::SummarizerMode;
pub use types::SummarizerError;
pub use types::SummarizerInput;
pub use types::SummarizerOutput;
