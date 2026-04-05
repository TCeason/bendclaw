//! # Bend Agent
//!
//! A Rust framework for building autonomous AI agents that run the full
//! agentic loop in-process. Supports 15+ built-in tools, MCP integration,
//! permission systems, cost tracking, and more.
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use bend_agent::Agent;
//! use bend_agent::AgentOptions;
//!
//! #[tokio::main]
//! async fn main() {
//!     let mut agent = Agent::new(AgentOptions::default()).await.unwrap();
//!     let result = agent
//!         .prompt("What files are in the current directory?")
//!         .await
//!         .unwrap();
//!     println!("{}", result.text);
//! }
//! ```

pub mod agent;
pub mod api;
pub mod context;
pub mod costtracker;
pub mod hooks;
mod ids;
pub mod mcp;
pub mod permissions;
pub mod session;
pub mod tools;
pub mod types;
pub mod utils;

// Re-export commonly used types
pub use agent::Agent;
pub use agent::AgentOptions;
pub use agent::SubagentDefinition;
pub use api::ApiClient;
pub use api::ApiType;
pub use api::LLMProvider;
pub use api::ProviderKind;
pub use api::ProviderResponse;
pub use api::ResponseStream;
pub use api::StreamEvent;
pub use costtracker::CostTracker;
pub use hooks::HookConfig;
pub use hooks::HookEvent;
pub use hooks::HookFn;
pub use hooks::HookInput;
pub use hooks::HookNotification;
pub use hooks::HookOutput;
pub use hooks::HookRule;
pub use hooks::NotificationLevel;
pub use hooks::PermissionBehavior;
pub use hooks::PermissionUpdate;
pub use mcp::McpClient;
pub use session::append_to_session;
pub use session::delete_session;
pub use session::fork_session;
pub use session::get_session_info;
pub use session::get_session_messages;
pub use session::list_sessions;
pub use session::load_session;
pub use session::new_metadata;
pub use session::rename_session;
pub use session::save_session;
pub use session::tag_session;
pub use session::SessionData;
pub use session::SessionMetadata;
pub use tools::ToolRegistry;
pub use types::ApiToolParam;
pub use types::CanUseToolFn;
pub use types::ContentBlock;
pub use types::Message;
pub use types::MessageRole;
pub use types::PermissionDecision;
pub use types::PermissionMode;
pub use types::QueryResult;
pub use types::RunStreamSummary;
pub use types::RunSummary;
pub use types::SDKMessage;
pub use types::SandboxFilesystemConfig;
pub use types::SandboxNetworkConfig;
pub use types::SandboxSettings;
pub use types::StreamMetrics;
pub use types::ThinkingConfig;
pub use types::Tool;
pub use types::ToolError;
pub use types::ToolInputSchema;
pub use types::ToolResult;
pub use types::ToolResultContent;
pub use types::ToolUseContext;
pub use types::Usage;
pub use utils::compact::build_compaction_prompt;
pub use utils::compact::compact_conversation;
pub use utils::compact::strip_images_from_messages;
pub use utils::file_cache::FileStateCache;
pub use utils::tokens::estimate_cost;
pub use utils::tokens::estimate_tokens;
