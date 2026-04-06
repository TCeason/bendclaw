//! MCP (Model Context Protocol) client support.
//!
//! Connect to MCP tool servers and use their tools seamlessly within yoagent.
//!
//! # Example
//!
//! ```rust,no_run
//! use bendengine::mcp::McpClient;
//!
//! # async fn example() -> Result<(), bendengine::mcp::McpError> {
//! // Connect to an MCP server via stdio
//! let client = McpClient::connect_stdio(
//!     "npx",
//!     &["-y", "@modelcontextprotocol/server-filesystem", "/tmp"],
//!     None,
//! )
//! .await?;
//! # Ok(())
//! # }
//! ```

pub mod client;
pub mod tool_adapter;
pub mod transport;
pub mod types;

pub use client::McpClient;
pub use tool_adapter::McpToolAdapter;
pub use transport::HttpTransport;
pub use transport::McpTransport;
pub use transport::StdioTransport;
pub use types::McpContent;
pub use types::McpError;
pub use types::McpToolCallResult;
pub use types::McpToolInfo;
pub use types::ServerInfo;
