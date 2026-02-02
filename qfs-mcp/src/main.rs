//! QFS MCP Server
//!
//! A Model Context Protocol (MCP) server that exposes QFS file search functionality
//! to AI agents over stdio transport.
//!
//! ## Usage
//!
//! ```bash
//! # Start with default database path (~/.cache/qfs/index.sqlite)
//! qfs-mcp
//!
//! # Start with custom database path
//! qfs-mcp --db-path /path/to/index.sqlite
//!
//! # Enable verbose logging
//! qfs-mcp --verbose
//! ```
//!
//! ## MCP Configuration
//!
//! Add to your MCP client configuration (e.g., Claude Desktop):
//!
//! ```json
//! {
//!   "mcpServers": {
//!     "qfs": {
//!       "command": "qfs-mcp",
//!       "args": ["--db-path", "/path/to/index.sqlite"]
//!     }
//!   }
//! }
//! ```
//!
//! ## Available Tools
//!
//! - **qfs_search**: BM25 full-text search with snippets
//! - **qfs_vsearch**: Semantic vector search (requires embeddings)
//! - **qfs_query**: Hybrid search with mode selection (bm25/vector/hybrid)
//! - **qfs_get**: Retrieve a document by path with content
//! - **qfs_multi_get**: Batch retrieve multiple documents
//! - **qfs_status**: Get index statistics and health

use anyhow::Result;
use clap::Parser;
use qfs::mcp::McpServer;
use std::path::PathBuf;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

/// QFS MCP Server - Expose file search to AI agents via Model Context Protocol
#[derive(Parser, Debug)]
#[command(name = "qfs-mcp")]
#[command(
    author,
    version,
    about = "QFS MCP Server - Model Context Protocol interface for file search"
)]
struct Args {
    /// Path to the QFS database file
    #[arg(long, short = 'd', env = "QFS_DB_PATH")]
    db_path: Option<PathBuf>,

    /// Enable verbose logging (outputs to stderr)
    #[arg(long, short = 'v')]
    verbose: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize logging to stderr (MCP uses stdout for protocol)
    let filter = if args.verbose {
        EnvFilter::new("debug")
    } else {
        // By default, suppress all logging to avoid interfering with MCP protocol
        EnvFilter::new("error")
    };

    tracing_subscriber::registry()
        .with(fmt::layer().with_writer(std::io::stderr))
        .with(filter)
        .init();

    // Get database path
    let db_path = args.db_path.unwrap_or_else(qfs::default_db_path);

    // Ensure database directory exists
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    tracing::info!(
        "Starting QFS MCP server with database: {}",
        db_path.display()
    );

    // Create and run the MCP server
    let server = McpServer::new(&db_path).await?;
    server.run().await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_args_parsing() {
        // Test default args
        let args = Args::try_parse_from(["qfs-mcp"]).unwrap();
        assert!(args.db_path.is_none());
        assert!(!args.verbose);

        // Test with db-path
        let args = Args::try_parse_from(["qfs-mcp", "--db-path", "/tmp/test.db"]).unwrap();
        assert_eq!(args.db_path, Some(PathBuf::from("/tmp/test.db")));

        // Test with verbose
        let args = Args::try_parse_from(["qfs-mcp", "-v"]).unwrap();
        assert!(args.verbose);
    }

    #[tokio::test]
    async fn test_server_creation_with_temp_db() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.sqlite");

        let server = McpServer::new(&db_path).await;
        assert!(server.is_ok(), "Server should be created successfully");
    }
}
