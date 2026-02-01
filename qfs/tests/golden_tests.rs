//! Golden tests for QFS search quality
//!
//! These tests compare QFS search results against expected "golden" files
//! to catch regressions in search quality and correctness.
//!
//! ## Running Tests
//!
//! ```bash
//! cargo test --test golden_tests
//! ```
//!
//! ## Updating Golden Files
//!
//! When search behavior intentionally changes, update golden files:
//!
//! ```bash
//! QFS_UPDATE_GOLDEN=1 cargo test --test golden_tests
//! ```

use qfs::{Indexer, SearchMode, SearchOptions, Store};
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use tempfile::tempdir;

/// Golden file format for search tests
#[derive(Debug, Clone, Serialize, Deserialize)]
struct GoldenSearch {
    /// The search query
    query: String,
    /// Search mode (bm25, vector, hybrid)
    mode: String,
    /// Expected minimum number of results
    #[serde(default)]
    min_count: usize,
    /// Expected maximum number of results (optional)
    max_count: Option<usize>,
    /// Expected top results (in order)
    top_results: Vec<GoldenResult>,
}

/// Expected result in golden file
#[derive(Debug, Clone, Serialize, Deserialize)]
struct GoldenResult {
    /// Substring that path must contain
    path_contains: String,
    /// Minimum score threshold (optional)
    #[serde(default)]
    min_score: Option<f64>,
    /// Maximum allowed rank position (1-indexed, optional)
    #[serde(default)]
    max_rank: Option<usize>,
}

/// Golden file format for index statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
struct GoldenIndexStats {
    /// Expected number of files indexed
    files_indexed: usize,
    /// Expected number of collections
    collections: usize,
    /// Expected total documents
    total_documents: usize,
}

/// Actual search result for comparison
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ActualSearchResult {
    rank: usize,
    path: String,
    score: f64,
    snippet: Option<String>,
}

/// Actual search output for golden file update
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ActualSearchOutput {
    query: String,
    mode: String,
    result_count: usize,
    results: Vec<ActualSearchResult>,
}

/// Test context with store and paths
struct TestContext {
    store: Store,
    #[allow(dead_code)]
    db_dir: tempfile::TempDir,
    #[allow(dead_code)]
    content_dir: tempfile::TempDir,
}

/// Set up test context with indexed corpus
fn setup_test_context() -> TestContext {
    let db_dir = tempdir().unwrap();
    let content_dir = tempdir().unwrap();

    // Copy corpus files to content directory
    let corpus_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/corpus");

    if corpus_dir.exists() {
        for entry in fs::read_dir(&corpus_dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_file() {
                let dest = content_dir.path().join(path.file_name().unwrap());
                fs::copy(&path, &dest).unwrap();
            }
        }
    }

    // Create store and add collection
    let store = Store::open(db_dir.path().join("test.sqlite")).unwrap();
    store
        .add_collection(
            "corpus",
            content_dir.path().to_str().unwrap(),
            &["**/*.md", "**/*.rs", "**/*.ts"],
        )
        .unwrap();

    // Index the collection
    let indexer = Indexer::new(&store);
    let _stats = indexer.index_collection("corpus").unwrap();

    TestContext {
        store,
        db_dir,
        content_dir,
    }
}

/// Get path to golden files directory
fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

/// Check if golden files should be updated
fn should_update_golden() -> bool {
    std::env::var("QFS_UPDATE_GOLDEN").is_ok()
}

/// Load a golden file
fn load_golden<T: for<'de> Deserialize<'de>>(name: &str) -> Option<T> {
    let path = golden_dir().join(name);
    if !path.exists() {
        return None;
    }

    let mut file = File::open(&path).unwrap();
    let mut content = String::new();
    file.read_to_string(&mut content).unwrap();

    serde_json::from_str(&content).ok()
}

/// Save a golden file
fn save_golden<T: Serialize>(name: &str, data: &T) {
    let path = golden_dir().join(name);

    // Ensure directory exists
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }

    let content = serde_json::to_string_pretty(data).unwrap();
    let mut file = File::create(&path).unwrap();
    file.write_all(content.as_bytes()).unwrap();

    println!("Updated golden file: {}", path.display());
}

/// Execute a search and return results
fn execute_search(
    store: &Store,
    query: &str,
    mode: SearchMode,
    limit: usize,
) -> Vec<ActualSearchResult> {
    let searcher = qfs::search::Searcher::new(store);
    let results = searcher
        .search(
            query,
            SearchOptions {
                mode,
                limit,
                ..Default::default()
            },
        )
        .unwrap_or_default();

    results
        .into_iter()
        .enumerate()
        .map(|(i, r)| ActualSearchResult {
            rank: i + 1,
            path: r.path,
            score: r.score,
            snippet: r.snippet,
        })
        .collect()
}

/// Compare actual results against golden expectations
fn verify_golden_search(
    actual: &[ActualSearchResult],
    golden: &GoldenSearch,
) -> Result<(), String> {
    // Check minimum count
    if actual.len() < golden.min_count {
        return Err(format!(
            "Expected at least {} results, got {}",
            golden.min_count,
            actual.len()
        ));
    }

    // Check maximum count if specified
    if let Some(max) = golden.max_count {
        if actual.len() > max {
            return Err(format!(
                "Expected at most {} results, got {}",
                max,
                actual.len()
            ));
        }
    }

    // Check expected top results
    for (i, expected) in golden.top_results.iter().enumerate() {
        // Find result matching path_contains
        let matching = actual
            .iter()
            .find(|r| r.path.contains(&expected.path_contains));

        if let Some(result) = matching {
            // Check max_rank if specified
            if let Some(max_rank) = expected.max_rank {
                if result.rank > max_rank {
                    return Err(format!(
                        "Expected '{}' within top {} results, but found at rank {}",
                        expected.path_contains, max_rank, result.rank
                    ));
                }
            }

            // Check min_score if specified
            if let Some(min_score) = expected.min_score {
                if result.score < min_score {
                    return Err(format!(
                        "Expected '{}' to have score >= {}, but got {}",
                        expected.path_contains, min_score, result.score
                    ));
                }
            }
        } else {
            return Err(format!(
                "Expected result containing '{}' at position {}, but not found in results",
                expected.path_contains,
                i + 1
            ));
        }
    }

    Ok(())
}

// =============================================================================
// Golden Tests
// =============================================================================

#[test]
fn test_golden_search_basic() {
    let ctx = setup_test_context();
    let golden_file = "search_basic.golden.json";

    let queries = vec![
        ("rust", SearchMode::Bm25),
        ("async await", SearchMode::Bm25),
        ("memory safety", SearchMode::Bm25),
        ("typescript", SearchMode::Bm25),
    ];

    if should_update_golden() {
        // Generate golden file
        let mut goldens = Vec::new();

        for (query, mode) in &queries {
            let results = execute_search(&ctx.store, query, *mode, 20);

            goldens.push(GoldenSearch {
                query: query.to_string(),
                mode: format!("{:?}", mode).to_lowercase(),
                min_count: results.len().saturating_sub(1).max(1),
                max_count: Some(results.len() + 2),
                top_results: results
                    .iter()
                    .take(3)
                    .map(|r| GoldenResult {
                        path_contains: Path::new(&r.path)
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or(&r.path)
                            .to_string(),
                        min_score: Some((r.score * 0.8 * 100.0).round() / 100.0),
                        max_rank: Some(r.rank + 2),
                    })
                    .collect(),
            });
        }

        save_golden(golden_file, &goldens);
        return;
    }

    // Load and verify golden file
    let goldens: Vec<GoldenSearch> = load_golden(golden_file)
        .expect("Golden file not found. Run with QFS_UPDATE_GOLDEN=1 to generate.");

    for golden in &goldens {
        let mode = match golden.mode.as_str() {
            "bm25" => SearchMode::Bm25,
            "vector" => SearchMode::Vector,
            "hybrid" => SearchMode::Hybrid,
            _ => SearchMode::Bm25,
        };

        let results = execute_search(&ctx.store, &golden.query, mode, 20);

        if let Err(e) = verify_golden_search(&results, golden) {
            panic!(
                "Golden test failed for query '{}': {}\n\nActual results:\n{:#?}",
                golden.query, e, results
            );
        }
    }
}

#[test]
fn test_golden_search_multiword() {
    let ctx = setup_test_context();
    let golden_file = "search_multiword.golden.json";

    let queries = vec![
        ("error handling", SearchMode::Bm25),
        ("database full text search", SearchMode::Bm25),
        ("api design patterns", SearchMode::Bm25),
        ("stack heap memory", SearchMode::Bm25),
    ];

    if should_update_golden() {
        let mut goldens = Vec::new();

        for (query, mode) in &queries {
            let results = execute_search(&ctx.store, query, *mode, 20);

            goldens.push(GoldenSearch {
                query: query.to_string(),
                mode: format!("{:?}", mode).to_lowercase(),
                min_count: if results.is_empty() { 0 } else { 1 },
                max_count: None,
                top_results: results
                    .iter()
                    .take(3)
                    .map(|r| GoldenResult {
                        path_contains: Path::new(&r.path)
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or(&r.path)
                            .to_string(),
                        min_score: None,
                        max_rank: Some(r.rank + 3),
                    })
                    .collect(),
            });
        }

        save_golden(golden_file, &goldens);
        return;
    }

    let goldens: Vec<GoldenSearch> = load_golden(golden_file)
        .expect("Golden file not found. Run with QFS_UPDATE_GOLDEN=1 to generate.");

    for golden in &goldens {
        let mode = match golden.mode.as_str() {
            "bm25" => SearchMode::Bm25,
            "vector" => SearchMode::Vector,
            "hybrid" => SearchMode::Hybrid,
            _ => SearchMode::Bm25,
        };

        let results = execute_search(&ctx.store, &golden.query, mode, 20);

        if let Err(e) = verify_golden_search(&results, golden) {
            panic!(
                "Golden test failed for query '{}': {}\n\nActual results:\n{:#?}",
                golden.query, e, results
            );
        }
    }
}

#[test]
fn test_golden_search_code() {
    let ctx = setup_test_context();
    let golden_file = "search_code.golden.json";

    let queries = vec![
        ("bm25 score", SearchMode::Bm25),
        ("debounce throttle", SearchMode::Bm25),
        ("pub fn", SearchMode::Bm25),
        ("async function", SearchMode::Bm25),
    ];

    if should_update_golden() {
        let mut goldens = Vec::new();

        for (query, mode) in &queries {
            let results = execute_search(&ctx.store, query, *mode, 20);

            goldens.push(GoldenSearch {
                query: query.to_string(),
                mode: format!("{:?}", mode).to_lowercase(),
                min_count: if results.is_empty() { 0 } else { 1 },
                max_count: None,
                top_results: results
                    .iter()
                    .take(3)
                    .map(|r| GoldenResult {
                        path_contains: Path::new(&r.path)
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or(&r.path)
                            .to_string(),
                        min_score: None,
                        max_rank: Some(r.rank + 2),
                    })
                    .collect(),
            });
        }

        save_golden(golden_file, &goldens);
        return;
    }

    let goldens: Vec<GoldenSearch> = load_golden(golden_file)
        .expect("Golden file not found. Run with QFS_UPDATE_GOLDEN=1 to generate.");

    for golden in &goldens {
        let mode = match golden.mode.as_str() {
            "bm25" => SearchMode::Bm25,
            "vector" => SearchMode::Vector,
            "hybrid" => SearchMode::Hybrid,
            _ => SearchMode::Bm25,
        };

        let results = execute_search(&ctx.store, &golden.query, mode, 20);

        if let Err(e) = verify_golden_search(&results, golden) {
            panic!(
                "Golden test failed for query '{}': {}\n\nActual results:\n{:#?}",
                golden.query, e, results
            );
        }
    }
}

#[test]
fn test_golden_index_stats() {
    let ctx = setup_test_context();
    let golden_file = "index_stats.golden.json";

    let doc_count = ctx.store.count_documents(Some("corpus")).unwrap();
    let collections = ctx.store.list_collections().unwrap();

    if should_update_golden() {
        let golden = GoldenIndexStats {
            files_indexed: doc_count as usize,
            collections: collections.len(),
            total_documents: doc_count as usize,
        };

        save_golden(golden_file, &golden);
        return;
    }

    let golden: GoldenIndexStats = load_golden(golden_file)
        .expect("Golden file not found. Run with QFS_UPDATE_GOLDEN=1 to generate.");

    assert_eq!(
        doc_count as usize, golden.total_documents,
        "Document count mismatch"
    );
    assert_eq!(
        collections.len(),
        golden.collections,
        "Collection count mismatch"
    );
}

#[test]
fn test_golden_search_no_results() {
    let ctx = setup_test_context();

    // These queries should return no results
    let queries = vec![
        "quantum entanglement blockchain",
        "zzznonexistenttermzzz",
        "xyzabc123notfound",
    ];

    for query in queries {
        let results = execute_search(&ctx.store, query, SearchMode::Bm25, 20);

        assert!(
            results.is_empty(),
            "Expected no results for '{}', but got {} results",
            query,
            results.len()
        );
    }
}

#[test]
fn test_golden_search_case_insensitive() {
    let ctx = setup_test_context();

    let test_cases = vec![("rust", "RUST"), ("async", "ASYNC"), ("memory", "Memory")];

    for (lower, upper) in test_cases {
        let lower_results = execute_search(&ctx.store, lower, SearchMode::Bm25, 20);
        let upper_results = execute_search(&ctx.store, upper, SearchMode::Bm25, 20);

        assert_eq!(
            lower_results.len(),
            upper_results.len(),
            "Case sensitivity issue: '{}' returned {} results, '{}' returned {}",
            lower,
            lower_results.len(),
            upper,
            upper_results.len()
        );
    }
}

#[test]
fn test_golden_search_ranking_consistency() {
    let ctx = setup_test_context();

    // Run same search multiple times, should get same results
    let query = "rust programming";

    let results1 = execute_search(&ctx.store, query, SearchMode::Bm25, 20);
    let results2 = execute_search(&ctx.store, query, SearchMode::Bm25, 20);

    assert_eq!(
        results1.len(),
        results2.len(),
        "Result count should be consistent"
    );

    for (r1, r2) in results1.iter().zip(results2.iter()) {
        assert_eq!(r1.path, r2.path, "Result order should be consistent");
        assert!(
            (r1.score - r2.score).abs() < 0.001,
            "Scores should be consistent"
        );
    }
}

#[test]
fn test_golden_search_score_normalization() {
    let ctx = setup_test_context();

    let queries = vec!["rust", "programming", "async await"];

    for query in queries {
        let results = execute_search(&ctx.store, query, SearchMode::Bm25, 20);

        for result in &results {
            assert!(
                result.score >= 0.0 && result.score <= 1.0,
                "Score {} for '{}' is not in [0, 1] range",
                result.score,
                query
            );
        }

        // Note: BM25 scores are normalized such that LOWER scores = BETTER matches
        // This is because BM25 returns negative values (more negative = better match)
        // and normalization with 1/(1+|score|) preserves this ordering
        // Results should be sorted by score ascending (lower = better, first)
        const EPSILON: f64 = 1e-6;
        for i in 1..results.len() {
            assert!(
                results[i - 1].score <= results[i].score + EPSILON,
                "Results not sorted by score (ascending) for '{}': {} > {}",
                query,
                results[i - 1].score,
                results[i].score
            );
        }
    }
}

#[test]
fn test_golden_search_limit() {
    let ctx = setup_test_context();

    // Search with different limits
    let query = "programming"; // Should match multiple docs

    for limit in [1, 3, 5, 10] {
        let results = execute_search(&ctx.store, query, SearchMode::Bm25, limit);

        assert!(
            results.len() <= limit,
            "Expected at most {} results, got {}",
            limit,
            results.len()
        );
    }
}

/// Test that generates a detailed report of search results (for debugging)
#[test]
#[ignore] // Run with: cargo test --test golden_tests test_search_report -- --ignored --nocapture
fn test_search_report() {
    let ctx = setup_test_context();

    let queries = vec![
        "rust",
        "async await",
        "memory safety",
        "typescript patterns",
        "database",
        "error handling",
        "bm25",
    ];

    println!("\n=== QFS Search Quality Report ===\n");

    for query in queries {
        let results = execute_search(&ctx.store, query, SearchMode::Bm25, 10);

        println!("Query: '{}' ({} results)", query, results.len());
        println!("{:-<60}", "");

        for result in results.iter().take(5) {
            println!(
                "  #{} [{:.3}] {}",
                result.rank,
                result.score,
                Path::new(&result.path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(&result.path)
            );

            if let Some(ref snippet) = result.snippet {
                let snippet_clean: String = snippet
                    .chars()
                    .take(80)
                    .map(|c| if c == '\n' { ' ' } else { c })
                    .collect();
                println!("       {}", snippet_clean);
            }
        }
        println!();
    }
}
