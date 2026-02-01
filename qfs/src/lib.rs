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
pub mod mcp;
pub mod parser;
pub mod scanner;
pub mod search;
pub mod store;

// Re-exports for convenience
pub use error::{Error, Result};
pub use indexer::Indexer;
pub use search::{SearchMode, SearchOptions, SearchResult};
pub use store::{MultiGetResult, Store, DEFAULT_MULTI_GET_MAX_BYTES};

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

/// Parse a path that may contain a :linenum suffix.
/// Returns (path, optional_line_number)
///
/// # Examples
/// ```
/// use qfs::parse_path_with_line;
///
/// let (path, line) = parse_path_with_line("docs/file.md:50");
/// assert_eq!(path, "docs/file.md");
/// assert_eq!(line, Some(50));
///
/// let (path, line) = parse_path_with_line("docs/file.md");
/// assert_eq!(path, "docs/file.md");
/// assert_eq!(line, None);
/// ```
pub fn parse_path_with_line(input: &str) -> (&str, Option<usize>) {
    // Match :digits at end of string
    if let Some(colon_pos) = input.rfind(':') {
        let suffix = &input[colon_pos + 1..];
        if !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit()) {
            if let Ok(line_num) = suffix.parse::<usize>() {
                return (&input[..colon_pos], Some(line_num));
            }
        }
    }
    (input, None)
}

/// Extract a range of lines from text content.
/// `from_line` is 1-indexed. Returns empty string if out of bounds.
///
/// # Examples
/// ```
/// use qfs::extract_lines;
///
/// let content = "line1\nline2\nline3\nline4\nline5";
/// let result = extract_lines(content, Some(2), Some(2));
/// assert_eq!(result, "line2\nline3");
///
/// let result = extract_lines(content, Some(2), None);
/// assert_eq!(result, "line2\nline3\nline4\nline5");
/// ```
pub fn extract_lines(content: &str, from_line: Option<usize>, max_lines: Option<usize>) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let start = from_line.unwrap_or(1).saturating_sub(1); // Convert to 0-indexed
    let end = match max_lines {
        Some(limit) => (start + limit).min(lines.len()),
        None => lines.len(),
    };

    if start >= lines.len() {
        return String::new();
    }

    lines[start..end].join("\n")
}

/// Add line numbers to text, starting from the given line number.
///
/// # Examples
/// ```
/// use qfs::add_line_numbers;
///
/// let text = "foo\nbar\nbaz";
/// let result = add_line_numbers(text, 10);
/// assert_eq!(result, "10: foo\n11: bar\n12: baz");
/// ```
pub fn add_line_numbers(text: &str, start_line: usize) -> String {
    text.lines()
        .enumerate()
        .map(|(i, line)| format!("{}: {}", start_line + i, line))
        .collect::<Vec<_>>()
        .join("\n")
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

#[cfg(test)]
mod line_extraction_tests {
    use super::*;

    #[test]
    fn test_parse_path_with_line_suffix() {
        let (path, line) = parse_path_with_line("docs/file.md:50");
        assert_eq!(path, "docs/file.md");
        assert_eq!(line, Some(50));
    }

    #[test]
    fn test_parse_path_without_suffix() {
        let (path, line) = parse_path_with_line("docs/file.md");
        assert_eq!(path, "docs/file.md");
        assert_eq!(line, None);
    }

    #[test]
    fn test_parse_path_with_colon_in_name() {
        // Colons followed by non-digits should not be parsed
        let (path, line) = parse_path_with_line("docs/10:30_meeting.md");
        assert_eq!(path, "docs/10:30_meeting.md");
        assert_eq!(line, None);
    }

    #[test]
    fn test_extract_lines_from_start() {
        let content = "line1\nline2\nline3\nline4\nline5";
        let result = extract_lines(content, Some(2), Some(2));
        assert_eq!(result, "line2\nline3");
    }

    #[test]
    fn test_extract_lines_to_end() {
        let content = "line1\nline2\nline3";
        let result = extract_lines(content, Some(2), None);
        assert_eq!(result, "line2\nline3");
    }

    #[test]
    fn test_extract_lines_out_of_bounds() {
        let content = "line1\nline2";
        let result = extract_lines(content, Some(10), None);
        assert_eq!(result, "");
    }

    #[test]
    fn test_extract_lines_max_exceeds_length() {
        let content = "line1\nline2\nline3";
        let result = extract_lines(content, Some(2), Some(100));
        assert_eq!(result, "line2\nline3");
    }

    #[test]
    fn test_add_line_numbers() {
        let text = "foo\nbar\nbaz";
        let result = add_line_numbers(text, 10);
        assert_eq!(result, "10: foo\n11: bar\n12: baz");
    }

    #[test]
    fn test_add_line_numbers_from_one() {
        let text = "first\nsecond";
        let result = add_line_numbers(text, 1);
        assert_eq!(result, "1: first\n2: second");
    }
}
