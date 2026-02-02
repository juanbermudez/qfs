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
async fn create_test_store() -> (Store, tempfile::TempDir, tempfile::TempDir) {
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
    let store = Store::open(db_dir.path().join("test.sqlite"))
        .await
        .unwrap();
    store
        .add_collection("docs", content_dir.path().to_str().unwrap(), &["**/*.md"])
        .await
        .unwrap();

    // Index the collection
    let indexer = Indexer::new(&store);
    let stats = indexer.index_collection("docs").await.unwrap();
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

#[tokio::test]
async fn test_basic_search() {
    let (store, _db_dir, _content_dir) = create_test_store().await;

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
        .await
        .unwrap();

    // Should find the Rust guide
    assert!(
        !results.is_empty(),
        "Should find results for 'rust programming'"
    );
    assert!(
        results[0].path.contains("rust_guide"),
        "Top result should be rust_guide.md"
    );
}

#[tokio::test]
async fn test_search_ranking() {
    let (store, _db_dir, _content_dir) = create_test_store().await;

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
        .await
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

#[tokio::test]
async fn test_search_multiple_terms() {
    let (store, _db_dir, _content_dir) = create_test_store().await;

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
        .await
        .unwrap();

    assert!(
        !results.is_empty(),
        "Should find results for 'memory safety'"
    );
    assert!(
        results[0].path.contains("rust_guide"),
        "Top result should be rust_guide.md for 'memory safety'"
    );
}

#[tokio::test]
async fn test_search_no_results() {
    let (store, _db_dir, _content_dir) = create_test_store().await;

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
        .await
        .unwrap();

    assert!(
        results.is_empty(),
        "Should not find results for unrelated terms"
    );
}

#[tokio::test]
async fn test_search_case_insensitive() {
    let (store, _db_dir, _content_dir) = create_test_store().await;

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
        .await
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
        .await
        .unwrap();

    assert_eq!(
        results_lower.len(),
        results_upper.len(),
        "Case should not affect number of results"
    );
}

#[tokio::test]
async fn test_search_with_limit() {
    let (store, _db_dir, _content_dir) = create_test_store().await;

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
        .await
        .unwrap();

    assert!(results.len() <= 1, "Should respect limit parameter");
}

#[tokio::test]
async fn test_search_collection_filter() {
    let (store, _db_dir, _content_dir) = create_test_store().await;

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
        .await
        .unwrap();

    assert!(
        !results.is_empty(),
        "Should find results in docs collection"
    );

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
        .await
        .unwrap();

    assert!(
        results.is_empty(),
        "Should find no results in non-existent collection"
    );
}

#[tokio::test]
async fn test_search_score_normalization() {
    let (store, _db_dir, _content_dir) = create_test_store().await;

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
        .await
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

#[tokio::test]
async fn test_search_snippet_generation() {
    let (store, _db_dir, _content_dir) = create_test_store().await;

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
        .await
        .unwrap();

    // Should have snippets with matches highlighted
    let has_snippet = results.iter().any(|r| r.snippet.is_some());
    assert!(has_snippet, "Results should include snippets");

    // Snippets should contain the search term or its stem
    for result in &results {
        if let Some(ref snippet) = result.snippet {
            // FTS5 highlights with <mark> tags
            assert!(
                snippet.to_lowercase().contains("ownership") || snippet.contains("<mark>"),
                "Snippet should contain search term or highlights: {}",
                snippet
            );
        }
    }
}

#[tokio::test]
async fn test_prefix_matching() {
    let (store, _db_dir, _content_dir) = create_test_store().await;

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
        .await
        .unwrap();

    assert!(!results.is_empty(), "Prefix search should find matches");
}

#[tokio::test]
async fn test_incremental_indexing() {
    let db_dir = tempdir().unwrap();
    let content_dir = tempdir().unwrap();

    // Create initial document
    let path = content_dir.path().join("test.md");
    {
        let mut file = File::create(&path).unwrap();
        file.write_all(b"# Initial Content\n\nThis is the first version.")
            .unwrap();
    }

    let store = Store::open(db_dir.path().join("test.sqlite"))
        .await
        .unwrap();
    store
        .add_collection("test", content_dir.path().to_str().unwrap(), &["**/*.md"])
        .await
        .unwrap();

    let indexer = Indexer::new(&store);

    // First index
    let stats1 = indexer.index_collection("test").await.unwrap();
    assert_eq!(stats1.files_indexed, 1);

    // Re-index without changes (should skip)
    let stats2 = indexer.index_collection("test").await.unwrap();
    assert_eq!(stats2.files_indexed, 0);
    assert_eq!(stats2.files_skipped, 1);

    // Modify the file
    {
        let mut file = File::create(&path).unwrap();
        file.write_all(b"# Updated Content\n\nThis is the second version.")
            .unwrap();
    }

    // Re-index with changes (should index)
    let stats3 = indexer.index_collection("test").await.unwrap();
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
        .await
        .unwrap();

    assert!(!results.is_empty(), "Should find updated content");
}

// =============================================================================
// Document ID (docid) Tests
// =============================================================================

#[tokio::test]
async fn test_search_results_include_docid() {
    let (store, _db_dir, _content_dir) = create_test_store().await;

    let searcher = qfs::search::Searcher::new(&store);
    let results = searcher
        .search(
            "rust",
            SearchOptions {
                mode: SearchMode::Bm25,
                limit: 10,
                ..Default::default()
            },
        )
        .await
        .unwrap();

    assert!(!results.is_empty(), "Should find results for 'rust'");

    // Each result should have a docid
    for result in &results {
        assert!(result.docid.is_some(), "Search result should include docid");
        let docid = result.docid.as_ref().unwrap();
        assert!(
            docid.starts_with('#'),
            "Docid should start with #, got: {}",
            docid
        );
        // Remove # prefix and check length
        let id = &docid[1..];
        assert_eq!(id.len(), 6, "Docid should be 6 characters after #");
        assert!(
            id.chars().all(|c| c.is_ascii_hexdigit()),
            "Docid should be hexadecimal"
        );
    }
}

#[tokio::test]
async fn test_get_document_by_docid() {
    let (store, _db_dir, _content_dir) = create_test_store().await;

    // First, get a document to find its hash
    let doc = store.get_document("docs", "rust_guide.md").await.unwrap();
    let short_id = qfs::store::get_docid(&doc.hash);

    // Now retrieve by docid with # prefix
    let found = store
        .get_document_by_docid(&format!("#{}", short_id))
        .await
        .unwrap();
    assert_eq!(found.path, "rust_guide.md");

    // Also works without # prefix
    let found = store.get_document_by_docid(short_id).await.unwrap();
    assert_eq!(found.path, "rust_guide.md");
}

#[test]
fn test_docid_is_first_6_chars_of_hash() {
    use qfs::store::get_docid;

    let hash = "abc123def456789012345678901234567890123456789012345678901234";
    assert_eq!(get_docid(hash), "abc123");

    let short_hash = "abcd";
    assert_eq!(get_docid(short_hash), "abcd");
}

#[test]
fn test_normalize_docid_formats() {
    use qfs::store::normalize_docid;

    // With hash prefix
    assert_eq!(normalize_docid("#abc123"), "abc123");

    // Without prefix
    assert_eq!(normalize_docid("abc123"), "abc123");

    // With quotes and hash
    assert_eq!(normalize_docid("\"#abc123\""), "abc123");
    assert_eq!(normalize_docid("'abc123'"), "abc123");

    // With whitespace
    assert_eq!(normalize_docid("  #abc123  "), "abc123");
}

#[test]
fn test_is_docid_validation() {
    use qfs::store::is_docid;

    // Valid docids
    assert!(is_docid("#abc123"));
    assert!(is_docid("abc123"));
    assert!(is_docid("ABC123")); // Case insensitive hex
    assert!(is_docid("abc123def456")); // Longer is ok

    // Invalid docids
    assert!(!is_docid("abc12")); // Too short
    assert!(!is_docid("ghijkl")); // Non-hex characters
    assert!(!is_docid("abc123.md")); // Has extension
    assert!(!is_docid("qfs://collection/path")); // Virtual path
}

// =============================================================================
// Line Range Extraction Tests
// =============================================================================

#[test]
fn test_parse_path_with_line_suffix() {
    use qfs::parse_path_with_line;

    let (path, line) = parse_path_with_line("docs/file.md:50");
    assert_eq!(path, "docs/file.md");
    assert_eq!(line, Some(50));
}

#[test]
fn test_parse_path_without_line_suffix() {
    use qfs::parse_path_with_line;

    let (path, line) = parse_path_with_line("docs/file.md");
    assert_eq!(path, "docs/file.md");
    assert_eq!(line, None);
}

#[test]
fn test_parse_path_with_colon_not_linenum() {
    use qfs::parse_path_with_line;

    // Colons followed by non-digits should not be parsed as line numbers
    let (path, line) = parse_path_with_line("docs/10:30_meeting.md");
    assert_eq!(path, "docs/10:30_meeting.md");
    assert_eq!(line, None);
}

#[test]
fn test_extract_lines_from_middle() {
    use qfs::extract_lines;

    let content = "line1\nline2\nline3\nline4\nline5";
    let result = extract_lines(content, Some(2), Some(2));
    assert_eq!(result, "line2\nline3");
}

#[test]
fn test_extract_lines_to_end() {
    use qfs::extract_lines;

    let content = "line1\nline2\nline3";
    let result = extract_lines(content, Some(2), None);
    assert_eq!(result, "line2\nline3");
}

#[test]
fn test_extract_lines_out_of_bounds() {
    use qfs::extract_lines;

    let content = "line1\nline2";
    let result = extract_lines(content, Some(10), None);
    assert_eq!(result, "");
}

#[test]
fn test_extract_lines_max_exceeds_length() {
    use qfs::extract_lines;

    let content = "line1\nline2\nline3";
    let result = extract_lines(content, Some(2), Some(100));
    assert_eq!(result, "line2\nline3");
}

#[test]
fn test_add_line_numbers_from_start() {
    use qfs::add_line_numbers;

    let text = "foo\nbar\nbaz";
    let result = add_line_numbers(text, 1);
    assert_eq!(result, "1: foo\n2: bar\n3: baz");
}

#[test]
fn test_add_line_numbers_from_offset() {
    use qfs::add_line_numbers;

    let text = "foo\nbar\nbaz";
    let result = add_line_numbers(text, 10);
    assert_eq!(result, "10: foo\n11: bar\n12: baz");
}

// =============================================================================
// Multi-get with Patterns Tests
// =============================================================================

async fn setup_multi_get_store() -> (Store, tempfile::TempDir, tempfile::TempDir) {
    let db_dir = tempdir().unwrap();
    let content_dir = tempdir().unwrap();

    // Create nested directory structure
    std::fs::create_dir_all(content_dir.path().join("docs")).unwrap();
    std::fs::create_dir_all(content_dir.path().join("src")).unwrap();

    // Create files
    let files = [
        ("docs/readme.md", "# Readme\nThis is the readme."),
        ("docs/guide.md", "# Guide\nThis is the guide."),
        ("src/main.rs", "fn main() {\n    println!(\"Hello\");\n}"),
        ("config.toml", "[settings]\nvalue = 1"),
    ];

    for (path, content) in files {
        let full_path = content_dir.path().join(path);
        let mut file = File::create(&full_path).unwrap();
        file.write_all(content.as_bytes()).unwrap();
    }

    // Create a large file
    let large_content = "x".repeat(20000);
    let large_path = content_dir.path().join("docs/large.txt");
    let mut large_file = File::create(&large_path).unwrap();
    large_file.write_all(large_content.as_bytes()).unwrap();

    let store = Store::open(db_dir.path().join("test.sqlite"))
        .await
        .unwrap();
    store
        .add_collection("test", content_dir.path().to_str().unwrap(), &["**/*"])
        .await
        .unwrap();

    let indexer = Indexer::new(&store);
    indexer.index_collection("test").await.unwrap();

    (store, db_dir, content_dir)
}

#[tokio::test]
async fn test_multi_get_glob_pattern() {
    let (store, _db_dir, _content_dir) = setup_multi_get_store().await;

    let results = store.multi_get("test/**/*.md", 10240, None).await.unwrap();

    assert_eq!(results.len(), 2, "Should find 2 .md files");
    assert!(
        results.iter().all(|r| r.path.ends_with(".md")),
        "All results should be .md files"
    );
    assert!(
        results.iter().all(|r| !r.skipped),
        "No files should be skipped"
    );
}

#[tokio::test]
async fn test_multi_get_comma_separated() {
    let (store, _db_dir, _content_dir) = setup_multi_get_store().await;

    let results = store
        .multi_get("test/docs/readme.md, test/docs/guide.md", 10240, None)
        .await
        .unwrap();

    assert_eq!(results.len(), 2);
    assert!(results.iter().any(|r| r.path == "test/docs/readme.md"));
    assert!(results.iter().any(|r| r.path == "test/docs/guide.md"));
}

#[tokio::test]
async fn test_multi_get_max_bytes_skips_large_files() {
    let (store, _db_dir, _content_dir) = setup_multi_get_store().await;

    // Use a small max_bytes to trigger skipping
    let results = store
        .multi_get("test/docs/**/*", 1024, None)
        .await
        .unwrap();

    // The large.txt file should be skipped
    let large = results.iter().find(|r| r.path.contains("large"));
    assert!(large.is_some(), "Should find large file in results");
    let large = large.unwrap();
    assert!(large.skipped, "Large file should be skipped");
    assert!(large.skip_reason.is_some(), "Should have skip reason");
    assert!(
        large.content.is_none(),
        "Skipped file should have no content"
    );
}

#[tokio::test]
async fn test_multi_get_max_lines_truncates() {
    let (store, _db_dir, _content_dir) = setup_multi_get_store().await;

    // Get a file with max 1 line
    let results = store
        .multi_get("test/docs/readme.md", 10240, Some(1))
        .await
        .unwrap();

    assert_eq!(results.len(), 1);
    let content = results[0].content.as_ref().unwrap();
    assert!(
        content.contains("[... truncated"),
        "Content should indicate truncation"
    );
}

#[tokio::test]
async fn test_multi_get_no_matches() {
    let (store, _db_dir, _content_dir) = setup_multi_get_store().await;

    let results = store
        .multi_get("nonexistent/**/*.xyz", 10240, None)
        .await
        .unwrap();

    assert!(results.is_empty(), "Should return empty for no matches");
}

// =============================================================================
// Ls Command Tests
// =============================================================================

#[tokio::test]
async fn test_list_collections() {
    let (store, _db_dir, _content_dir) = create_test_store().await;

    let collections = store.list_collections().await.unwrap();
    assert_eq!(collections.len(), 1);
    assert_eq!(collections[0].name, "docs");
}

#[tokio::test]
async fn test_list_files_in_collection() {
    let (store, _db_dir, _content_dir) = create_test_store().await;

    let files = store.list_files("docs", None).await.unwrap();
    assert_eq!(files.len(), 3, "Should have 3 files");

    // All files should have required fields
    for file in &files {
        assert_eq!(file.collection, "docs");
        assert!(!file.path.is_empty());
        assert!(file.size > 0);
    }
}

#[tokio::test]
async fn test_list_files_with_path_prefix() {
    let (store, _db_dir, _content_dir) = setup_multi_get_store().await;

    // List only files in docs directory
    let files = store.list_files("test", Some("docs")).await.unwrap();

    // Should find files under docs/
    assert!(!files.is_empty());
    assert!(
        files.iter().all(|f| f.path.starts_with("docs")),
        "All files should be under docs/"
    );
}

#[tokio::test]
async fn test_list_files_nonexistent_collection() {
    let (store, _db_dir, _content_dir) = create_test_store().await;

    let files = store.list_files("nonexistent", None).await.unwrap();
    assert!(files.is_empty());
}

// =============================================================================
// Context System Tests
// =============================================================================

#[tokio::test]
async fn test_set_and_get_global_context() {
    let store = Store::open_memory().await.unwrap();

    store
        .set_context(None, "/", "Global context for all files")
        .await
        .unwrap();

    let ctx = store.get_global_context().await.unwrap();
    assert_eq!(ctx, Some("Global context for all files".to_string()));
}

#[tokio::test]
async fn test_set_and_find_collection_context() {
    let store = Store::open_memory().await.unwrap();
    store
        .add_collection("docs", "/tmp/docs", &["**/*.md"])
        .await
        .unwrap();

    // Set hierarchical contexts
    store
        .set_context(Some("docs"), "/", "Documentation")
        .await
        .unwrap();
    store
        .set_context(Some("docs"), "/api", "API reference")
        .await
        .unwrap();
    store
        .set_context(Some("docs"), "/api/v2", "API v2 docs")
        .await
        .unwrap();

    // Most specific match wins
    let ctx = store
        .find_context_for_path("docs", "/api/v2/endpoints.md")
        .await
        .unwrap();
    assert_eq!(ctx, Some("API v2 docs".to_string()));

    let ctx = store
        .find_context_for_path("docs", "/api/v1/old.md")
        .await
        .unwrap();
    assert_eq!(ctx, Some("API reference".to_string()));

    let ctx = store
        .find_context_for_path("docs", "/readme.md")
        .await
        .unwrap();
    assert_eq!(ctx, Some("Documentation".to_string()));
}

#[tokio::test]
async fn test_context_longest_prefix_matching() {
    let store = Store::open_memory().await.unwrap();
    store
        .add_collection("code", "/tmp/code", &["**/*.rs"])
        .await
        .unwrap();

    store
        .set_context(Some("code"), "/", "Rust codebase")
        .await
        .unwrap();
    store
        .set_context(Some("code"), "/src", "Source files")
        .await
        .unwrap();
    store
        .set_context(Some("code"), "/src/handlers", "HTTP handlers")
        .await
        .unwrap();

    // Should match the longest prefix
    let ctx = store
        .find_context_for_path("code", "/src/handlers/auth.rs")
        .await
        .unwrap();
    assert_eq!(ctx, Some("HTTP handlers".to_string()));

    // Falls back to shorter prefix when no exact match
    let ctx = store
        .find_context_for_path("code", "/src/models/user.rs")
        .await
        .unwrap();
    assert_eq!(ctx, Some("Source files".to_string()));
}

#[tokio::test]
async fn test_global_context_fallback() {
    let store = Store::open_memory().await.unwrap();
    store
        .add_collection("docs", "/tmp/docs", &["**/*.md"])
        .await
        .unwrap();

    store
        .set_context(None, "/", "Global fallback context")
        .await
        .unwrap();

    // Collection has no specific context, should fallback to global
    let ctx = store
        .find_context_for_path("docs", "/any/path.md")
        .await
        .unwrap();
    assert_eq!(ctx, Some("Global fallback context".to_string()));
}

#[tokio::test]
async fn test_get_all_contexts_for_path() {
    let store = Store::open_memory().await.unwrap();
    store
        .add_collection("docs", "/tmp/docs", &["**/*.md"])
        .await
        .unwrap();

    store.set_context(None, "/", "Global").await.unwrap();
    store
        .set_context(Some("docs"), "/", "Docs")
        .await
        .unwrap();
    store
        .set_context(Some("docs"), "/api", "API")
        .await
        .unwrap();

    // Get all matching contexts (general to specific)
    let contexts = store
        .get_all_contexts_for_path("docs", "/api/file.md")
        .await
        .unwrap();
    assert_eq!(contexts, vec!["Global", "Docs", "API"]);
}

#[tokio::test]
async fn test_context_appears_in_search_results() {
    let (store, _db_dir, _content_dir) = create_test_store().await;

    // Set context for the collection
    store
        .set_context(Some("docs"), "/", "Programming guides")
        .await
        .unwrap();

    let searcher = qfs::search::Searcher::new(&store);
    let results = searcher
        .search(
            "rust",
            SearchOptions {
                mode: SearchMode::Bm25,
                limit: 10,
                ..Default::default()
            },
        )
        .await
        .unwrap();

    assert!(!results.is_empty());

    // Results should include context
    let has_context = results.iter().any(|r| r.context.is_some());
    assert!(has_context, "At least one result should have context");

    // Check context content
    for result in &results {
        if let Some(ref ctx) = result.context {
            assert!(
                ctx.contains("Programming guides"),
                "Context should contain our set context"
            );
        }
    }
}

#[tokio::test]
async fn test_remove_context() {
    let store = Store::open_memory().await.unwrap();
    store
        .set_context(Some("docs"), "/api", "API context")
        .await
        .unwrap();

    // Remove should succeed
    assert!(store.remove_context(Some("docs"), "/api").await.unwrap());

    // Second remove should return false (already removed)
    assert!(!store.remove_context(Some("docs"), "/api").await.unwrap());
}

#[tokio::test]
async fn test_list_contexts() {
    let store = Store::open_memory().await.unwrap();
    store.set_context(None, "/", "Global").await.unwrap();
    store
        .set_context(Some("docs"), "/", "Docs")
        .await
        .unwrap();
    store
        .set_context(Some("docs"), "/api", "API")
        .await
        .unwrap();

    let contexts = store.list_contexts().await.unwrap();
    assert_eq!(contexts.len(), 3);

    // First should be global (sorted)
    assert_eq!(contexts[0].collection, None);
    assert_eq!(contexts[0].path_prefix, "/");
    assert_eq!(contexts[0].context, "Global");
}

#[tokio::test]
async fn test_delete_context() {
    let store = Store::open_memory().await.unwrap();
    store.set_context(None, "/", "Global").await.unwrap();

    let removed = store.remove_context(None, "/").await.unwrap();
    assert!(removed);

    let ctx = store.get_global_context().await.unwrap();
    assert_eq!(ctx, None);
}

#[tokio::test]
async fn test_get_collections_without_context() {
    let store = Store::open_memory().await.unwrap();
    store
        .add_collection("docs", "/tmp/docs", &["**/*.md"])
        .await
        .unwrap();
    store
        .add_collection("code", "/tmp/code", &["**/*.rs"])
        .await
        .unwrap();

    // Add context only to docs
    store
        .set_context(Some("docs"), "/", "Docs")
        .await
        .unwrap();

    let without = store.get_collections_without_context().await.unwrap();
    assert_eq!(without.len(), 1);
    assert_eq!(without[0].name, "code");
}

// =============================================================================
// Vector Index Tests
// =============================================================================

#[tokio::test]
async fn test_ensure_vector_index_no_embeddings() {
    let store = Store::open_memory().await.unwrap();

    // Should return false when no embeddings exist
    let created = store.ensure_vector_index().await.unwrap();
    assert!(!created, "Should not create index when no embeddings exist");
}

#[tokio::test]
async fn test_vector_search_fallback() {
    let store = Store::open_memory().await.unwrap();

    // Add collection and document
    store
        .add_collection("test", "/tmp/test", &["**/*.md"])
        .await
        .unwrap();
    store
        .insert_content("hash123", b"Test content", "text/plain")
        .await
        .unwrap();
    store
        .upsert_document("test", "file.md", Some("Title"), "hash123", ".md", "Test")
        .await
        .unwrap();

    // Create a mock embedding (384 dimensions as f32 bytes)
    let embedding: Vec<f32> = (0..384).map(|i| (i as f32) / 384.0).collect();
    let embedding_bytes: Vec<u8> = embedding.iter().flat_map(|f| f.to_le_bytes()).collect();

    // Insert embedding
    store
        .insert_embedding("hash123", 0, 0, "test-model", &embedding_bytes)
        .await
        .unwrap();

    // Native vector search may not be available (depends on libsql version and data format)
    // But the legacy fallback should always work
    let results = store
        .search_vector_legacy(&embedding, None, 10, None, None)
        .await
        .unwrap();

    assert_eq!(results.len(), 1, "Legacy search should find the embedding");
    assert!(
        results[0].similarity > 0.99,
        "Self-similarity should be ~1.0"
    );
}

#[tokio::test]
async fn test_search_with_date_filter() {
    let (store, _db_dir, _content_dir) = create_test_store().await;

    // Search without date filter should return results
    let results = store
        .search_bm25("rust", None, 10, false, None, None)
        .await
        .unwrap();
    assert!(!results.is_empty(), "Should find results without date filter");

    // Search with future from_date should return no results (documents were created "now")
    let results = store
        .search_bm25("rust", None, 10, false, Some("2099-01-01"), None)
        .await
        .unwrap();
    assert!(
        results.is_empty(),
        "Should not find results with future from_date"
    );

    // Search with past to_date should return no results
    let results = store
        .search_bm25("rust", None, 10, false, None, Some("2000-01-01"))
        .await
        .unwrap();
    assert!(
        results.is_empty(),
        "Should not find results with past to_date"
    );

    // Search with valid date range covering "now" should return results
    let results = store
        .search_bm25("rust", None, 10, false, Some("2020-01-01"), Some("2099-12-31"))
        .await
        .unwrap();
    assert!(
        !results.is_empty(),
        "Should find results within valid date range"
    );
}
