//! Search functionality for QFS

use crate::error::{Error, Result};
use crate::store::Store;

/// Search mode
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SearchMode {
    /// BM25 full-text search (default)
    #[default]
    Bm25,
    /// Vector semantic search (requires embeddings)
    Vector,
    /// Hybrid search combining BM25 and vector
    Hybrid,
}

impl std::str::FromStr for SearchMode {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "bm25" => Ok(SearchMode::Bm25),
            "vector" => Ok(SearchMode::Vector),
            "hybrid" => Ok(SearchMode::Hybrid),
            _ => Err(Error::InvalidQuery(format!("Unknown search mode: {}", s))),
        }
    }
}

/// Search options
#[derive(Debug, Clone)]
pub struct SearchOptions {
    /// Search mode
    pub mode: SearchMode,
    /// Maximum number of results
    pub limit: usize,
    /// Minimum score threshold (0.0 - 1.0)
    pub min_score: f64,
    /// Filter by collection
    pub collection: Option<String>,
    /// Include binary files in results
    pub include_binary: bool,
}

impl Default for SearchOptions {
    fn default() -> Self {
        SearchOptions {
            mode: SearchMode::Bm25,
            limit: 20,
            min_score: 0.0,
            collection: None,
            include_binary: false,
        }
    }
}

/// Search result
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    /// Document ID
    pub id: i64,
    /// File path (collection/relative_path)
    pub path: String,
    /// File name
    pub name: String,
    /// MIME type
    pub mime_type: String,
    /// File size in bytes
    pub file_size: i64,
    /// Whether the file is binary
    pub is_binary: bool,
    /// Relevance score (0.0 - 1.0)
    pub score: f64,
    /// Content (null for binary files)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Content pointer (file:// URL for binary files)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_pointer: Option<String>,
    /// Snippet with match context
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
    /// Line number where match starts
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_start: Option<u32>,
    /// Collection name
    pub collection: String,
    /// Document title
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// Searcher for QFS
pub struct Searcher<'a> {
    store: &'a Store,
}

impl<'a> Searcher<'a> {
    /// Create a new searcher
    pub fn new(store: &'a Store) -> Self {
        Searcher { store }
    }

    /// Search for documents
    pub fn search(&self, query: &str, options: SearchOptions) -> Result<Vec<SearchResult>> {
        match options.mode {
            SearchMode::Bm25 => self.search_bm25(query, &options),
            SearchMode::Vector => Err(Error::EmbeddingsRequired),
            SearchMode::Hybrid => Err(Error::EmbeddingsRequired),
        }
    }

    /// BM25 full-text search using FTS5
    fn search_bm25(&self, query: &str, options: &SearchOptions) -> Result<Vec<SearchResult>> {
        // Sanitize query for FTS5
        let fts_query = sanitize_fts_query(query);

        if fts_query.is_empty() {
            return Ok(Vec::new());
        }

        // Execute search via Store
        let rows = self.store.search_bm25(
            &fts_query,
            options.collection.as_deref(),
            options.limit,
            options.include_binary,
        )?;

        // Convert rows to SearchResults with score normalization
        let results: Vec<SearchResult> = rows
            .into_iter()
            .map(|row| {
                let normalized_score = normalize_bm25_score(row.bm25_score);

                // Determine if binary
                let is_binary = row.content_type.starts_with("application/octet")
                    || row.content_type.starts_with("image/")
                    || row.content_type.starts_with("audio/")
                    || row.content_type.starts_with("video/");

                // Extract filename from path
                let name = std::path::Path::new(&row.path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(&row.path)
                    .to_string();

                // Build content pointer for binary files
                let content_pointer = if is_binary {
                    // Would need collection path to build file:// URL
                    // For now, return the relative path
                    Some(format!("{}/{}", row.collection, row.path))
                } else {
                    None
                };

                SearchResult {
                    id: row.id,
                    path: format!("{}/{}", row.collection, row.path),
                    name,
                    mime_type: row.content_type,
                    file_size: row.size,
                    is_binary,
                    score: normalized_score,
                    content: None, // Content loaded separately if needed
                    content_pointer,
                    snippet: row.snippet,
                    line_start: None, // Could parse from snippet
                    collection: row.collection,
                    title: row.title,
                }
            })
            .filter(|r| r.score >= options.min_score)
            .collect();

        Ok(results)
    }
}

/// Sanitize a query string for FTS5
///
/// - Removes special characters
/// - Adds prefix matching (term -> "term"*)
/// - Handles AND/OR operators
fn sanitize_fts_query(query: &str) -> String {
    let query = query.trim();

    if query.is_empty() {
        return String::new();
    }

    // Split into terms
    let terms: Vec<&str> = query
        .split_whitespace()
        .filter(|t| !t.is_empty())
        .collect();

    if terms.is_empty() {
        return String::new();
    }

    // Build FTS5 query with prefix matching
    let fts_terms: Vec<String> = terms
        .iter()
        .map(|term| {
            // Remove special FTS5 characters
            let clean: String = term
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
                .collect();

            if clean.is_empty() {
                return String::new();
            }

            // Add prefix matching
            format!("\"{}\"*", clean)
        })
        .filter(|t| !t.is_empty())
        .collect();

    if fts_terms.is_empty() {
        return String::new();
    }

    // Join with AND
    fts_terms.join(" AND ")
}

/// Normalize BM25 score to 0-1 range
///
/// FTS5 BM25 returns negative scores where lower is better.
/// This converts to 0-1 where higher is better.
pub fn normalize_bm25_score(bm25_score: f64) -> f64 {
    1.0 / (1.0 + bm25_score.abs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_fts_query_basic() {
        assert_eq!(sanitize_fts_query("hello"), "\"hello\"*");
        assert_eq!(sanitize_fts_query("hello world"), "\"hello\"* AND \"world\"*");
    }

    #[test]
    fn test_sanitize_fts_query_special_chars() {
        assert_eq!(sanitize_fts_query("hello@world"), "\"helloworld\"*");
        assert_eq!(sanitize_fts_query("foo-bar"), "\"foo-bar\"*");
    }

    #[test]
    fn test_sanitize_fts_query_empty() {
        assert_eq!(sanitize_fts_query(""), "");
        assert_eq!(sanitize_fts_query("   "), "");
        assert_eq!(sanitize_fts_query("@#$%"), "");
    }

    #[test]
    fn test_normalize_bm25_score() {
        // BM25 of 0 -> score of 1.0
        assert!((normalize_bm25_score(0.0) - 1.0).abs() < 0.001);

        // More negative BM25 -> lower score (but still positive)
        assert!(normalize_bm25_score(-5.0) < normalize_bm25_score(-1.0));
        assert!(normalize_bm25_score(-10.0) < normalize_bm25_score(-5.0));
    }

    #[test]
    fn test_search_mode_from_str() {
        assert_eq!("bm25".parse::<SearchMode>().unwrap(), SearchMode::Bm25);
        assert_eq!("BM25".parse::<SearchMode>().unwrap(), SearchMode::Bm25);
        assert_eq!("vector".parse::<SearchMode>().unwrap(), SearchMode::Vector);
        assert_eq!("hybrid".parse::<SearchMode>().unwrap(), SearchMode::Hybrid);
        assert!("invalid".parse::<SearchMode>().is_err());
    }
}
