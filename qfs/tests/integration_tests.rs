//! Integration tests for QFS search functionality
//!
//! These tests verify end-to-end search behavior including:
//! - Index creation and population
//! - BM25 full-text search
//! - Score normalization
//! - Result ranking

use qfs::{Indexer, SearchMode, SearchOptions, Store};
use std::fs::File;
use std::io::Write;
use tempfile::tempdir;

/// Create a test store with fixture documents
fn create_test_store() -> (Store, tempfile::TempDir, tempfile::TempDir) {
    let db_dir = tempdir().unwrap();
    let content_dir = tempdir().unwrap();

    // Create test documents
    let fixtures = [
        ("rust_guide.md", RUST_GUIDE),
        ("python_basics.md", PYTHON_BASICS),
        ("web_development.md", WEB_DEVELOPMENT),
    ];

    for (name, content) in fixtures {
        let path = content_dir.path().join(name);
        let mut file = File::create(&path).unwrap();
        file.write_all(content.as_bytes()).unwrap();
    }

    // Create store and add collection
    let store = Store::open(db_dir.path().join("test.sqlite")).unwrap();
    store
        .add_collection(
            "docs",
            content_dir.path().to_str().unwrap(),
            &["**/*.md"],
        )
        .unwrap();

    // Index the collection
    let indexer = Indexer::new(&store);
    let stats = indexer.index_collection("docs").unwrap();
    assert_eq!(stats.files_indexed, 3);

    (store, db_dir, content_dir)
}

const RUST_GUIDE: &str = r#"# Rust Programming Guide

Rust is a systems programming language that runs blazingly fast, prevents segfaults, and guarantees thread safety.

## Getting Started

To install Rust, use rustup.

## Key Features

- Zero-cost abstractions
- Move semantics
- Pattern matching
- Type inference

## Memory Safety

Rust's ownership system ensures memory safety without garbage collection. The borrow checker validates all references at compile time.

## Concurrency

Rust prevents data races at compile time through its ownership and type system.
"#;

const PYTHON_BASICS: &str = r#"# Python Basics

Python is a high-level, interpreted programming language known for its readability and simplicity.

## Variables and Types

Python uses dynamic typing.

## Functions

Functions are defined using the def keyword.

## Classes

Python supports object-oriented programming.

## File Handling

Reading and writing files is straightforward in Python.
"#;

const WEB_DEVELOPMENT: &str = r#"# Web Development Fundamentals

Modern web development encompasses frontend, backend, and full-stack development.

## Frontend Technologies

HTML provides the structure of web pages. CSS handles visual presentation. JavaScript adds dynamic behavior.

## Backend Development

Backend technologies handle server-side logic including Node.js, Python, Django, and Rust.

## Databases

Common database choices include PostgreSQL, MongoDB, Redis, and SQLite.
"#;

#[test]
fn test_basic_search() {
    let (store, _db_dir, _content_dir) = create_test_store();

    let searcher = qfs::search::Searcher::new(&store);
    let results = searcher
        .search(
            "rust programming",
            SearchOptions {
                mode: SearchMode::Bm25,
                limit: 10,
                ..Default::default()
            },
        )
        .unwrap();

    // Should find the Rust guide
    assert!(!results.is_empty(), "Should find results for 'rust programming'");
    assert!(
        results[0].path.contains("rust_guide"),
        "Top result should be rust_guide.md"
    );
}

#[test]
fn test_search_ranking() {
    let (store, _db_dir, _content_dir) = create_test_store();

    let searcher = qfs::search::Searcher::new(&store);

    // Search for "python"
    let results = searcher
        .search(
            "python",
            SearchOptions {
                mode: SearchMode::Bm25,
                limit: 10,
                ..Default::default()
            },
        )
        .unwrap();

    assert!(!results.is_empty(), "Should find results for 'python'");

    // Python basics should rank higher than web development for "python"
    let python_idx = results
        .iter()
        .position(|r| r.path.contains("python_basics"));
    let web_idx = results
        .iter()
        .position(|r| r.path.contains("web_development"));

    if let (Some(py), Some(web)) = (python_idx, web_idx) {
        assert!(
            py < web,
            "Python basics should rank higher than web development for 'python' query"
        );
    }
}

#[test]
fn test_search_multiple_terms() {
    let (store, _db_dir, _content_dir) = create_test_store();

    let searcher = qfs::search::Searcher::new(&store);

    // Search for "memory safety" - should match Rust guide
    let results = searcher
        .search(
            "memory safety",
            SearchOptions {
                mode: SearchMode::Bm25,
                limit: 10,
                ..Default::default()
            },
        )
        .unwrap();

    assert!(!results.is_empty(), "Should find results for 'memory safety'");
    assert!(
        results[0].path.contains("rust_guide"),
        "Top result should be rust_guide.md for 'memory safety'"
    );
}

#[test]
fn test_search_no_results() {
    let (store, _db_dir, _content_dir) = create_test_store();

    let searcher = qfs::search::Searcher::new(&store);

    // Search for something not in any document
    let results = searcher
        .search(
            "quantum entanglement blockchain",
            SearchOptions {
                mode: SearchMode::Bm25,
                limit: 10,
                ..Default::default()
            },
        )
        .unwrap();

    assert!(
        results.is_empty(),
        "Should not find results for unrelated terms"
    );
}

#[test]
fn test_search_case_insensitive() {
    let (store, _db_dir, _content_dir) = create_test_store();

    let searcher = qfs::search::Searcher::new(&store);

    // Search with different cases
    let results_lower = searcher
        .search(
            "rust",
            SearchOptions {
                mode: SearchMode::Bm25,
                limit: 10,
                ..Default::default()
            },
        )
        .unwrap();

    let results_upper = searcher
        .search(
            "RUST",
            SearchOptions {
                mode: SearchMode::Bm25,
                limit: 10,
                ..Default::default()
            },
        )
        .unwrap();

    assert_eq!(
        results_lower.len(),
        results_upper.len(),
        "Case should not affect number of results"
    );
}

#[test]
fn test_search_with_limit() {
    let (store, _db_dir, _content_dir) = create_test_store();

    let searcher = qfs::search::Searcher::new(&store);

    // Search for something that matches multiple documents
    let results = searcher
        .search(
            "programming",
            SearchOptions {
                mode: SearchMode::Bm25,
                limit: 1,
                ..Default::default()
            },
        )
        .unwrap();

    assert!(
        results.len() <= 1,
        "Should respect limit parameter"
    );
}

#[test]
fn test_search_collection_filter() {
    let (store, _db_dir, _content_dir) = create_test_store();

    let searcher = qfs::search::Searcher::new(&store);

    // Search within the docs collection
    let results = searcher
        .search(
            "rust",
            SearchOptions {
                mode: SearchMode::Bm25,
                collection: Some("docs".to_string()),
                limit: 10,
                ..Default::default()
            },
        )
        .unwrap();

    assert!(!results.is_empty(), "Should find results in docs collection");

    // Search in non-existent collection
    let results = searcher
        .search(
            "rust",
            SearchOptions {
                mode: SearchMode::Bm25,
                collection: Some("nonexistent".to_string()),
                limit: 10,
                ..Default::default()
            },
        )
        .unwrap();

    assert!(results.is_empty(), "Should find no results in non-existent collection");
}

#[test]
fn test_search_score_normalization() {
    let (store, _db_dir, _content_dir) = create_test_store();

    let searcher = qfs::search::Searcher::new(&store);

    let results = searcher
        .search(
            "programming",
            SearchOptions {
                mode: SearchMode::Bm25,
                limit: 10,
                ..Default::default()
            },
        )
        .unwrap();

    // All scores should be between 0 and 1
    for result in &results {
        assert!(
            result.score >= 0.0 && result.score <= 1.0,
            "Scores should be normalized to 0-1 range, got {}",
            result.score
        );
    }

    // Results should be sorted by score (descending) with tolerance for floating point
    // Note: FTS5 BM25 scores can be very close for similar documents
    for i in 1..results.len() {
        let epsilon = 1e-5; // Allow for small floating point differences
        assert!(
            results[i - 1].score >= results[i].score - epsilon,
            "Results should be sorted by score descending: {} vs {}",
            results[i - 1].score,
            results[i].score
        );
    }
}

#[test]
fn test_search_snippet_generation() {
    let (store, _db_dir, _content_dir) = create_test_store();

    let searcher = qfs::search::Searcher::new(&store);

    let results = searcher
        .search(
            "ownership",
            SearchOptions {
                mode: SearchMode::Bm25,
                limit: 10,
                ..Default::default()
            },
        )
        .unwrap();

    // Should have snippets with matches highlighted
    let has_snippet = results.iter().any(|r| r.snippet.is_some());
    assert!(has_snippet, "Results should include snippets");

    // Snippets should contain the search term or its stem
    for result in &results {
        if let Some(ref snippet) = result.snippet {
            // FTS5 highlights with <mark> tags
            assert!(
                snippet.to_lowercase().contains("ownership") ||
                snippet.contains("<mark>"),
                "Snippet should contain search term or highlights: {}",
                snippet
            );
        }
    }
}

#[test]
fn test_prefix_matching() {
    let (store, _db_dir, _content_dir) = create_test_store();

    let searcher = qfs::search::Searcher::new(&store);

    // Search for partial term "prog" should match "programming"
    let results = searcher
        .search(
            "prog",
            SearchOptions {
                mode: SearchMode::Bm25,
                limit: 10,
                ..Default::default()
            },
        )
        .unwrap();

    assert!(!results.is_empty(), "Prefix search should find matches");
}

#[test]
fn test_incremental_indexing() {
    let db_dir = tempdir().unwrap();
    let content_dir = tempdir().unwrap();

    // Create initial document
    let path = content_dir.path().join("test.md");
    {
        let mut file = File::create(&path).unwrap();
        file.write_all(b"# Initial Content\n\nThis is the first version.").unwrap();
    }

    let store = Store::open(db_dir.path().join("test.sqlite")).unwrap();
    store
        .add_collection(
            "test",
            content_dir.path().to_str().unwrap(),
            &["**/*.md"],
        )
        .unwrap();

    let indexer = Indexer::new(&store);

    // First index
    let stats1 = indexer.index_collection("test").unwrap();
    assert_eq!(stats1.files_indexed, 1);

    // Re-index without changes (should skip)
    let stats2 = indexer.index_collection("test").unwrap();
    assert_eq!(stats2.files_indexed, 0);
    assert_eq!(stats2.files_skipped, 1);

    // Modify the file
    {
        let mut file = File::create(&path).unwrap();
        file.write_all(b"# Updated Content\n\nThis is the second version.").unwrap();
    }

    // Re-index with changes (should index)
    let stats3 = indexer.index_collection("test").unwrap();
    assert_eq!(stats3.files_indexed, 1);

    // Verify search reflects update
    let searcher = qfs::search::Searcher::new(&store);
    let results = searcher
        .search(
            "updated",
            SearchOptions {
                mode: SearchMode::Bm25,
                limit: 10,
                ..Default::default()
            },
        )
        .unwrap();

    assert!(!results.is_empty(), "Should find updated content");
}
