//! Search functionality for QFS

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
    pub fn search(&self, query: &str, options: SearchOptions) -> Result<Vec<SearchResult>> {
        match options.mode {
            SearchMode::Bm25 => self.search_bm25(query, &options),
            SearchMode::Vector => self.search_vector(query, &options),
            SearchMode::Hybrid => self.search_hybrid(query, &options),
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

                // Get context for this path
                let context = self
                    .store
                    .get_all_contexts_for_path(&row.collection, &row.path)
                    .ok()
                    .map(|contexts| contexts.join("\n\n"))
                    .filter(|s| !s.is_empty());

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
                    docid: Some(format!("#{}", crate::store::get_docid(&row.hash))),
                    chunk_index: None,
                    context,
                }
            })
            .filter(|r| r.score >= options.min_score)
            .collect();

        Ok(results)
    }

    /// Vector semantic search
    fn search_vector(&self, _query: &str, options: &SearchOptions) -> Result<Vec<SearchResult>> {
        // Check if any embeddings exist
        let embed_count = self.store.count_embeddings(options.collection.as_deref())?;
        if embed_count == 0 {
            return Err(Error::EmbeddingsRequired);
        }

        // For vector search, we need the query embedding
        // This would be provided externally or computed here
        // For now, return an error indicating embeddings are needed
        // The actual search happens in search_vector_with_embedding
        Err(Error::EmbeddingError(
            "Vector search requires query embedding. Use search_vector_with_embedding() instead."
                .to_string(),
        ))
    }

    /// Vector search with pre-computed query embedding
    pub fn search_vector_with_embedding(
        &self,
        query_embedding: &[f32],
        options: &SearchOptions,
    ) -> Result<Vec<SearchResult>> {
        // Get all embeddings
        let embeddings = self
            .store
            .get_all_embeddings_for_search(options.collection.as_deref())?;

        if embeddings.is_empty() {
            return Err(Error::EmbeddingsRequired);
        }

        // Calculate similarities
        let mut scored: Vec<(f64, &crate::store::EmbeddingSearchRow)> = embeddings
            .iter()
            .map(|row| {
                let embedding = bytes_to_embedding(&row.embedding);
                let similarity = cosine_similarity(query_embedding, &embedding);
                (similarity as f64, row)
            })
            .collect();

        // Sort by similarity (descending)
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        // Take top results
        let results: Vec<SearchResult> = scored
            .into_iter()
            .take(options.limit)
            .filter(|(score, _)| *score >= options.min_score)
            .map(|(score, row)| {
                let name = std::path::Path::new(&row.path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(&row.path)
                    .to_string();

                // Get context for this path
                let context = self
                    .store
                    .get_all_contexts_for_path(&row.collection, &row.path)
                    .ok()
                    .map(|contexts| contexts.join("\n\n"))
                    .filter(|s| !s.is_empty());

                SearchResult {
                    id: row.doc_id,
                    path: format!("{}/{}", row.collection, row.path),
                    name,
                    mime_type: "text/plain".to_string(), // We'd need to look this up
                    file_size: 0,
                    is_binary: false,
                    score,
                    content: None,
                    content_pointer: None,
                    snippet: None,
                    line_start: None,
                    collection: row.collection.clone(),
                    title: row.title.clone(),
                    docid: Some(format!("#{}", crate::store::get_docid(&row.hash))),
                    chunk_index: Some(row.chunk_index),
                    context,
                }
            })
            .collect();

        Ok(results)
    }

    /// Hybrid search combining BM25 and vector with RRF
    fn search_hybrid(&self, _query: &str, options: &SearchOptions) -> Result<Vec<SearchResult>> {
        // Check if embeddings are available
        let embed_count = self.store.count_embeddings(options.collection.as_deref())?;
        if embed_count == 0 {
            return Err(Error::EmbeddingsRequired);
        }

        // For hybrid search, we need the query embedding
        // This would be provided externally
        Err(Error::EmbeddingError(
            "Hybrid search requires query embedding. Use search_hybrid_with_embedding() instead."
                .to_string(),
        ))
    }

    /// Hybrid search with pre-computed query embedding
    pub fn search_hybrid_with_embedding(
        &self,
        query: &str,
        query_embedding: &[f32],
        options: &SearchOptions,
    ) -> Result<Vec<SearchResult>> {
        // Get BM25 results
        let bm25_options = SearchOptions {
            limit: options.limit * 2, // Get more for fusion
            ..options.clone()
        };
        let bm25_results = self.search_bm25(query, &bm25_options)?;

        // Get vector results
        let vector_options = SearchOptions {
            limit: options.limit * 2,
            ..options.clone()
        };
        let vector_results = self.search_vector_with_embedding(query_embedding, &vector_options)?;

        // Apply Reciprocal Rank Fusion (RRF)
        let fused = reciprocal_rank_fusion(&bm25_results, &vector_results, 60.0);

        // Take top results
        Ok(fused.into_iter().take(options.limit).collect())
    }
}

/// Apply Reciprocal Rank Fusion (RRF) to combine two result sets
///
/// RRF score = 1 / (k + rank)
/// where k is a constant (typically 60)
fn reciprocal_rank_fusion(
    bm25_results: &[SearchResult],
    vector_results: &[SearchResult],
    k: f64,
) -> Vec<SearchResult> {
    let mut scores: HashMap<i64, (f64, Option<SearchResult>)> = HashMap::new();

    // Add BM25 scores
    for (rank, result) in bm25_results.iter().enumerate() {
        let rrf_score = 1.0 / (k + rank as f64 + 1.0);
        scores.entry(result.id).or_insert((0.0, None)).0 += rrf_score;
        if scores[&result.id].1.is_none() {
            scores.get_mut(&result.id).unwrap().1 = Some(result.clone());
        }
    }

    // Add vector scores
    for (rank, result) in vector_results.iter().enumerate() {
        let rrf_score = 1.0 / (k + rank as f64 + 1.0);
        scores.entry(result.id).or_insert((0.0, None)).0 += rrf_score;
        if scores[&result.id].1.is_none() {
            scores.get_mut(&result.id).unwrap().1 = Some(result.clone());
        }
    }

    // Sort by combined score
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
    let terms: Vec<&str> = query.split_whitespace().filter(|t| !t.is_empty()).collect();

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

/// Convert bytes to f32 embedding
fn bytes_to_embedding(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|chunk| {
            let arr: [u8; 4] = chunk.try_into().unwrap();
            f32::from_le_bytes(arr)
        })
        .collect()
}

/// Calculate cosine similarity between two embeddings
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }

    dot / (norm_a * norm_b)
}

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

    #[test]
    fn test_cosine_similarity_identical() {
        let a = vec![1.0, 2.0, 3.0];
        let sim = cosine_similarity(&a, &a);
        assert!((sim - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![0.0, 1.0, 0.0];
        let sim = cosine_similarity(&a, &b);
        assert!(sim.abs() < 1e-6);
    }

    #[test]
    fn test_bytes_to_embedding() {
        let embedding = [1.0f32, 2.0, -3.5];
        let bytes: Vec<u8> = embedding.iter().flat_map(|f| f.to_le_bytes()).collect();
        let restored = bytes_to_embedding(&bytes);

        assert_eq!(embedding.len(), restored.len());
        for (a, b) in embedding.iter().zip(restored.iter()) {
            assert!((a - b).abs() < 1e-6);
        }
    }

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

        // Document 2 appears in both, should be ranked higher
        assert_eq!(fused.len(), 3);

        // Find doc 2's score - it should have contributions from both rankings
        let doc2 = fused.iter().find(|r| r.id == 2).unwrap();
        let doc1 = fused.iter().find(|r| r.id == 1).unwrap();
        let doc3 = fused.iter().find(|r| r.id == 3).unwrap();

        // Doc 2 should have higher score since it's in both result sets
        assert!(doc2.score > doc1.score);
        assert!(doc2.score > doc3.score);
    }
}
