//! MCP (Model Context Protocol) server for QFS
//!
//! This module implements an MCP server using stdio transport, exposing
//! QFS search functionality to AI agents.
//!
//! ## Tools Exposed
//!
//! - `qfs_search` - BM25 keyword search
//! - `qfs_vsearch` - Vector semantic search (requires embeddings)
//! - `qfs_query` - Hybrid search with RRF fusion
//! - `qfs_get` - Retrieve document by path
//! - `qfs_multi_get` - Batch retrieve documents
//! - `qfs_status` - Index health and statistics
//!
//! ## Usage
//!
//! ```rust,ignore
//! use qfs::mcp::McpServer;
//! use std::path::PathBuf;
//!
//! let server = McpServer::new(&PathBuf::from("~/.cache/qfs/index.sqlite")).unwrap();
//! server.run().unwrap();
//! ```

mod server;
mod protocol;
pub mod tools;

pub use server::McpServer;
pub use protocol::{JsonRpcError, JsonRpcRequest, JsonRpcResponse, ToolResult, ToolDefinition};
