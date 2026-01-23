//! # QFS - Quick File Search
//!
//! A universal local file search engine with hybrid BM25+vector search.
//!
//! QFS provides:
//! - **BM25 full-text search** via SQLite FTS5
//! - **Vector semantic search** via sqlite-vec (optional)
//! - **Hybrid search** combining both with Reciprocal Rank Fusion
//! - **MCP server** for AI agent integration
//!
//! ## Quick Start
//!
//! ```rust,ignore
//! use qfs::{Store, Indexer, SearchOptions, SearchMode};
//!
//! // Open or create a database
//! let store = Store::open("~/.cache/qfs/index.sqlite").unwrap();
//!
//! // Add a collection
//! store.add_collection("notes", "~/notes", &["**/*.md"]).unwrap();
//!
//! // Index the collection
//! let indexer = Indexer::new(&store);
//! indexer.index_collection("notes").unwrap();
//!
//! // Search
//! let searcher = qfs::search::Searcher::new(&store);
//! let results = searcher.search("rust async", SearchOptions {
//!     mode: SearchMode::Bm25,
//!     limit: 20,
//!     ..Default::default()
//! }).unwrap();
//! ```

pub mod error;
pub mod indexer;
pub mod parser;
pub mod scanner;
pub mod search;
pub mod store;

// Re-exports for convenience
pub use error::{Error, Result};
pub use indexer::Indexer;
pub use search::{SearchMode, SearchOptions, SearchResult};
pub use store::Store;

/// Library version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Default database path
pub fn default_db_path() -> std::path::PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("qfs")
        .join("index.sqlite")
}

/// Detect if content is binary using NUL-byte check (ripgrep strategy)
pub fn is_binary(content: &[u8]) -> bool {
    // Check first 8KB for NUL bytes
    content.iter().take(8192).any(|&b| b == 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_binary_text() {
        let text = b"Hello, world!\nThis is plain text.";
        assert!(!is_binary(text));
    }

    #[test]
    fn test_is_binary_with_nul() {
        let binary = b"Hello\x00World";
        assert!(is_binary(binary));
    }

    #[test]
    fn test_is_binary_empty() {
        let empty: &[u8] = b"";
        assert!(!is_binary(empty));
    }
}
