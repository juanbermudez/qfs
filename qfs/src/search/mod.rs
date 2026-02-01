//! Search functionality for QFS
//!
//! Provides BM25, vector, and hybrid search across indexed documents.

use crate::error::{Error, Result};
use crate::store::Store;
use std::collections::HashMap;

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
    /// Short document ID (first 6 chars of hash)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub docid: Option<String>,
    /// Chunk index for vector search results
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chunk_index: Option<i32>,
    /// Context description for this document's location
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
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
    pub async fn search(&self, query: &str, options: SearchOptions) -> Result<Vec<SearchResult>> {
        match options.mode {
            SearchMode::Bm25 => self.search_bm25(query, &options).await,
            SearchMode::Vector => self.search_vector(query, &options).await,
            SearchMode::Hybrid => self.search_hybrid(query, &options).await,
        }
    }

    /// BM25 full-text search using FTS5
    async fn search_bm25(
        &self,
        query: &str,
        options: &SearchOptions,
    ) -> Result<Vec<SearchResult>> {
        let fts_query = sanitize_fts_query(query);

        if fts_query.is_empty() {
            return Ok(Vec::new());
        }

        let rows = self
            .store
            .search_bm25(
                &fts_query,
                options.collection.as_deref(),
                options.limit,
                options.include_binary,
            )
            .await?;

        let mut results = Vec::with_capacity(rows.len());

        for row in rows {
            let normalized_score = normalize_bm25_score(row.bm25_score);

            if normalized_score < options.min_score {
                continue;
            }

            let is_binary = row.content_type.starts_with("application/octet")
                || row.content_type.starts_with("image/")
                || row.content_type.starts_with("audio/")
                || row.content_type.starts_with("video/");

            let name = std::path::Path::new(&row.path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(&row.path)
                .to_string();

            let content_pointer = if is_binary {
                Some(format!("{}/{}", row.collection, row.path))
            } else {
                None
            };

            let context = self
                .store
                .get_all_contexts_for_path(&row.collection, &row.path)
                .await
                .ok()
                .map(|contexts| contexts.join("\n\n"))
                .filter(|s| !s.is_empty());

            results.push(SearchResult {
                id: row.id,
                path: format!("{}/{}", row.collection, row.path),
                name,
                mime_type: row.content_type,
                file_size: row.size,
                is_binary,
                score: normalized_score,
                content: None,
                content_pointer,
                snippet: row.snippet,
                line_start: None,
                collection: row.collection,
                title: row.title,
                docid: Some(format!("#{}", crate::store::get_docid(&row.hash))),
                chunk_index: None,
                context,
            });
        }

        Ok(results)
    }

    /// Vector semantic search
    async fn search_vector(
        &self,
        _query: &str,
        options: &SearchOptions,
    ) -> Result<Vec<SearchResult>> {
        let embed_count = self
            .store
            .count_embeddings(options.collection.as_deref())
            .await?;
        if embed_count == 0 {
            return Err(Error::EmbeddingsRequired);
        }

        Err(Error::EmbeddingError(
            "Vector search requires query embedding. Use search_vector_with_embedding() instead."
                .to_string(),
        ))
    }

    /// Vector search with pre-computed query embedding
    /// Uses libsql's native vector_top_k() for efficient KNN search when available,
    /// falls back to legacy in-memory cosine similarity calculation otherwise.
    pub async fn search_vector_with_embedding(
        &self,
        query_embedding: &[f32],
        options: &SearchOptions,
    ) -> Result<Vec<SearchResult>> {
        // Check if embeddings exist
        let embed_count = self
            .store
            .count_embeddings(options.collection.as_deref())
            .await?;
        if embed_count == 0 {
            return Err(Error::EmbeddingsRequired);
        }

        // Try native vector search first, fall back to legacy if not available
        let vector_results = match self
            .store
            .search_vector_native(query_embedding, options.collection.as_deref(), options.limit)
            .await?
        {
            Some(results) => results,
            None => {
                // Fall back to legacy search
                self.store
                    .search_vector_legacy(
                        query_embedding,
                        options.collection.as_deref(),
                        options.limit,
                    )
                    .await?
            }
        };

        let mut results = Vec::new();

        for row in vector_results {
            if row.similarity < options.min_score {
                continue;
            }

            let name = std::path::Path::new(&row.path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(&row.path)
                .to_string();

            let context = self
                .store
                .get_all_contexts_for_path(&row.collection, &row.path)
                .await
                .ok()
                .map(|contexts| contexts.join("\n\n"))
                .filter(|s| !s.is_empty());

            results.push(SearchResult {
                id: row.doc_id,
                path: format!("{}/{}", row.collection, row.path),
                name,
                mime_type: "text/plain".to_string(),
                file_size: 0,
                is_binary: false,
                score: row.similarity,
                content: None,
                content_pointer: None,
                snippet: None,
                line_start: None,
                collection: row.collection,
                title: row.title,
                docid: Some(format!("#{}", crate::store::get_docid(&row.hash))),
                chunk_index: Some(row.chunk_index),
                context,
            });
        }

        Ok(results)
    }

    /// Hybrid search combining BM25 and vector with RRF
    async fn search_hybrid(
        &self,
        _query: &str,
        options: &SearchOptions,
    ) -> Result<Vec<SearchResult>> {
        let embed_count = self
            .store
            .count_embeddings(options.collection.as_deref())
            .await?;
        if embed_count == 0 {
            return Err(Error::EmbeddingsRequired);
        }

        Err(Error::EmbeddingError(
            "Hybrid search requires query embedding. Use search_hybrid_with_embedding() instead."
                .to_string(),
        ))
    }

    /// Hybrid search with pre-computed query embedding
    pub async fn search_hybrid_with_embedding(
        &self,
        query: &str,
        query_embedding: &[f32],
        options: &SearchOptions,
    ) -> Result<Vec<SearchResult>> {
        let bm25_options = SearchOptions {
            limit: options.limit * 2,
            ..options.clone()
        };
        let bm25_results = self.search_bm25(query, &bm25_options).await?;

        let vector_options = SearchOptions {
            limit: options.limit * 2,
            ..options.clone()
        };
        let vector_results = self
            .search_vector_with_embedding(query_embedding, &vector_options)
            .await?;

        let fused = reciprocal_rank_fusion(&bm25_results, &vector_results, 60.0);

        Ok(fused.into_iter().take(options.limit).collect())
    }
}

/// Apply Reciprocal Rank Fusion (RRF) to combine two result sets
fn reciprocal_rank_fusion(
    bm25_results: &[SearchResult],
    vector_results: &[SearchResult],
    k: f64,
) -> Vec<SearchResult> {
    let mut scores: HashMap<i64, (f64, Option<SearchResult>)> = HashMap::new();

    for (rank, result) in bm25_results.iter().enumerate() {
        let rrf_score = 1.0 / (k + rank as f64 + 1.0);
        scores.entry(result.id).or_insert((0.0, None)).0 += rrf_score;
        if scores[&result.id].1.is_none() {
            scores.get_mut(&result.id).unwrap().1 = Some(result.clone());
        }
    }

    for (rank, result) in vector_results.iter().enumerate() {
        let rrf_score = 1.0 / (k + rank as f64 + 1.0);
        scores.entry(result.id).or_insert((0.0, None)).0 += rrf_score;
        if scores[&result.id].1.is_none() {
            scores.get_mut(&result.id).unwrap().1 = Some(result.clone());
        }
    }

    let mut results: Vec<_> = scores
        .into_iter()
        .filter_map(|(_, (score, result))| {
            result.map(|mut r| {
                r.score = score;
                r
            })
        })
        .collect();

    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    results
}

/// Sanitize a query string for FTS5
fn sanitize_fts_query(query: &str) -> String {
    let query = query.trim();

    if query.is_empty() {
        return String::new();
    }

    let terms: Vec<&str> = query.split_whitespace().filter(|t| !t.is_empty()).collect();

    if terms.is_empty() {
        return String::new();
    }

    let fts_terms: Vec<String> = terms
        .iter()
        .map(|term| {
            let clean: String = term
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
                .collect();

            if clean.is_empty() {
                return String::new();
            }

            format!("\"{}\"*", clean)
        })
        .filter(|t| !t.is_empty())
        .collect();

    if fts_terms.is_empty() {
        return String::new();
    }

    fts_terms.join(" AND ")
}

/// Normalize BM25 score to 0-1 range
pub fn normalize_bm25_score(bm25_score: f64) -> f64 {
    1.0 / (1.0 + bm25_score.abs())
}

// Note: bytes_to_embedding() and cosine_similarity() have been removed.
// Vector similarity is now calculated natively by libsql using vector_distance_cos()
// and efficient KNN search via vector_top_k() with the idx_embeddings_vector index.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_fts_query_basic() {
        assert_eq!(sanitize_fts_query("hello"), "\"hello\"*");
        assert_eq!(
            sanitize_fts_query("hello world"),
            "\"hello\"* AND \"world\"*"
        );
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
        assert!((normalize_bm25_score(0.0) - 1.0).abs() < 0.001);
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

    // Note: cosine_similarity and bytes_to_embedding tests removed.
    // Vector similarity is now computed natively by libsql's vector_distance_cos().

    #[test]
    fn test_reciprocal_rank_fusion() {
        let bm25 = vec![
            SearchResult {
                id: 1,
                path: "a.md".to_string(),
                name: "a.md".to_string(),
                mime_type: "text/markdown".to_string(),
                file_size: 100,
                is_binary: false,
                score: 0.9,
                content: None,
                content_pointer: None,
                snippet: None,
                line_start: None,
                collection: "test".to_string(),
                title: None,
                docid: None,
                chunk_index: None,
                context: None,
            },
            SearchResult {
                id: 2,
                path: "b.md".to_string(),
                name: "b.md".to_string(),
                mime_type: "text/markdown".to_string(),
                file_size: 100,
                is_binary: false,
                score: 0.8,
                content: None,
                content_pointer: None,
                snippet: None,
                line_start: None,
                collection: "test".to_string(),
                title: None,
                docid: None,
                chunk_index: None,
                context: None,
            },
        ];

        let vector = vec![
            SearchResult {
                id: 2,
                path: "b.md".to_string(),
                name: "b.md".to_string(),
                mime_type: "text/markdown".to_string(),
                file_size: 100,
                is_binary: false,
                score: 0.95,
                content: None,
                content_pointer: None,
                snippet: None,
                line_start: None,
                collection: "test".to_string(),
                title: None,
                docid: None,
                chunk_index: None,
                context: None,
            },
            SearchResult {
                id: 3,
                path: "c.md".to_string(),
                name: "c.md".to_string(),
                mime_type: "text/markdown".to_string(),
                file_size: 100,
                is_binary: false,
                score: 0.85,
                content: None,
                content_pointer: None,
                snippet: None,
                line_start: None,
                collection: "test".to_string(),
                title: None,
                docid: None,
                chunk_index: None,
                context: None,
            },
        ];

        let fused = reciprocal_rank_fusion(&bm25, &vector, 60.0);

        assert_eq!(fused.len(), 3);

        let doc2 = fused.iter().find(|r| r.id == 2).unwrap();
        let doc1 = fused.iter().find(|r| r.id == 1).unwrap();
        let doc3 = fused.iter().find(|r| r.id == 3).unwrap();

        assert!(doc2.score > doc1.score);
        assert!(doc2.score > doc3.score);
    }
}
