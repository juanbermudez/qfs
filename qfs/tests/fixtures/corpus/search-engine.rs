//! Search Engine Implementation
//!
//! A simple search engine with BM25 ranking.

use std::collections::HashMap;

/// BM25 parameters
const K1: f64 = 1.2;
const B: f64 = 0.75;

/// Search index for documents
pub struct SearchIndex {
    /// Document frequency for each term
    doc_freq: HashMap<String, usize>,
    /// Total number of documents
    doc_count: usize,
    /// Average document length
    avg_doc_len: f64,
}

impl SearchIndex {
    /// Create a new search index
    pub fn new() -> Self {
        SearchIndex {
            doc_freq: HashMap::new(),
            doc_count: 0,
            avg_doc_len: 0.0,
        }
    }

    /// Calculate BM25 score for a term in a document
    pub fn bm25_score(&self, term: &str, doc_len: usize, term_freq: usize) -> f64 {
        let df = self.doc_freq.get(term).copied().unwrap_or(0) as f64;
        let idf = ((self.doc_count as f64 - df + 0.5) / (df + 0.5) + 1.0).ln();

        let tf = term_freq as f64;
        let norm = K1 * (1.0 - B + B * doc_len as f64 / self.avg_doc_len);

        idf * (tf * (K1 + 1.0)) / (tf + norm)
    }

    /// Index a document
    pub fn index_document(&mut self, _doc_id: &str, content: &str) {
        let terms: Vec<&str> = content.split_whitespace().collect();

        let mut seen = std::collections::HashSet::new();
        for term in &terms {
            let lower = term.to_lowercase();
            if seen.insert(lower.clone()) {
                *self.doc_freq.entry(lower).or_insert(0) += 1;
            }
        }

        self.doc_count += 1;
        self.avg_doc_len = (self.avg_doc_len * (self.doc_count - 1) as f64
            + terms.len() as f64) / self.doc_count as f64;
    }
}

/// Search result with score
#[derive(Debug)]
pub struct SearchResult {
    pub doc_id: String,
    pub score: f64,
}

impl Default for SearchIndex {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_index_document() {
        let mut index = SearchIndex::new();
        index.index_document("doc1", "hello world rust");
        index.index_document("doc2", "hello rust programming");

        assert_eq!(index.doc_count, 2);
        assert_eq!(index.doc_freq.get("hello"), Some(&2));
        assert_eq!(index.doc_freq.get("world"), Some(&1));
    }

    #[test]
    fn test_bm25_score() {
        let mut index = SearchIndex::new();
        index.index_document("doc1", "rust rust rust programming");
        index.index_document("doc2", "python programming");

        // Term with higher frequency should have higher score
        let score1 = index.bm25_score("rust", 4, 3);
        let score2 = index.bm25_score("rust", 4, 1);

        assert!(score1 > score2);
    }
}
