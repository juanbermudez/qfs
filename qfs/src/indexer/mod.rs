//! Document indexer

use crate::error::Result;
use crate::parser::parse_file;
use crate::scanner::Scanner;
use crate::store::Store;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::time::{Duration, Instant};

/// Statistics from an indexing operation
#[derive(Debug, Clone, Default)]
pub struct IndexStats {
    /// Number of files scanned
    pub files_scanned: usize,
    /// Number of files indexed (new or updated)
    pub files_indexed: usize,
    /// Number of files skipped (unchanged)
    pub files_skipped: usize,
    /// Number of files removed (deleted from filesystem)
    pub files_removed: usize,
    /// Number of errors encountered
    pub errors: usize,
    /// Time taken
    pub duration: Duration,
}

/// Progress callback for indexing
pub trait IndexProgress: Send {
    /// Called when a file is processed
    fn on_file(&mut self, path: &Path, status: FileStatus);
    /// Called when indexing is complete
    fn on_complete(&mut self, stats: &IndexStats);
}

/// Status of a file during indexing
#[derive(Debug, Clone)]
pub enum FileStatus {
    /// File was indexed
    Indexed,
    /// File was skipped (unchanged)
    Skipped,
    /// File was removed
    Removed,
    /// Error processing file
    Error(String),
}

/// Document indexer
pub struct Indexer<'a> {
    store: &'a Store,
}

impl<'a> Indexer<'a> {
    /// Create a new indexer
    pub fn new(store: &'a Store) -> Self {
        Indexer { store }
    }

    /// Index a collection
    pub async fn index_collection(&self, name: &str) -> Result<IndexStats> {
        self.index_collection_with_progress(name, &mut NoopProgress)
            .await
    }

    /// Index a collection with progress reporting
    pub async fn index_collection_with_progress(
        &self,
        name: &str,
        progress: &mut dyn IndexProgress,
    ) -> Result<IndexStats> {
        let start = Instant::now();
        let mut stats = IndexStats::default();

        // Get collection config
        let collection = self.store.get_collection(name).await?;

        // Create scanner
        let patterns: Vec<&str> = collection.patterns.iter().map(|s| s.as_str()).collect();
        let exclude: Vec<&str> = collection.exclude.iter().map(|s| s.as_str()).collect();
        let scanner = Scanner::new(&collection.path, &patterns, &exclude)?;

        // Track which files we've seen
        let mut seen_paths = std::collections::HashSet::new();

        // Scan and index files
        for scan_result in scanner.scan() {
            stats.files_scanned += 1;
            seen_paths.insert(scan_result.relative_path.clone());

            match self
                .index_file(name, &scan_result.path, &scan_result.relative_path)
                .await
            {
                Ok(indexed) => {
                    if indexed {
                        stats.files_indexed += 1;
                        progress.on_file(&scan_result.path, FileStatus::Indexed);
                    } else {
                        stats.files_skipped += 1;
                        progress.on_file(&scan_result.path, FileStatus::Skipped);
                    }
                }
                Err(e) => {
                    stats.errors += 1;
                    progress.on_file(&scan_result.path, FileStatus::Error(e.to_string()));
                    tracing::warn!("Error indexing {}: {}", scan_result.relative_path, e);
                }
            }
        }

        // TODO: Mark documents not seen as inactive (for incremental index)
        // This requires listing existing documents and comparing

        stats.duration = start.elapsed();
        progress.on_complete(&stats);

        Ok(stats)
    }

    /// Index all collections
    pub async fn index_all(&self) -> Result<IndexStats> {
        let mut total_stats = IndexStats::default();
        let start = Instant::now();

        for collection in self.store.list_collections().await? {
            let stats = self.index_collection(&collection.name).await?;
            total_stats.files_scanned += stats.files_scanned;
            total_stats.files_indexed += stats.files_indexed;
            total_stats.files_skipped += stats.files_skipped;
            total_stats.files_removed += stats.files_removed;
            total_stats.errors += stats.errors;
        }

        total_stats.duration = start.elapsed();
        Ok(total_stats)
    }

    /// Index a single file
    ///
    /// Returns true if the file was indexed, false if skipped (unchanged)
    async fn index_file(
        &self,
        collection: &str,
        path: &Path,
        relative_path: &str,
    ) -> Result<bool> {
        // Read file content (sync file I/O is fine here)
        let content = std::fs::read(path)?;

        // Calculate hash
        let hash = calculate_hash(&content);

        // Check if content already exists
        if self.store.content_exists(&hash).await? {
            // Check if document exists with same hash
            if let Ok(doc) = self.store.get_document(collection, relative_path).await {
                if doc.hash == hash {
                    return Ok(false); // Skip, unchanged
                }
            }
        }

        // Parse the file
        let parsed = parse_file(path, &content)?;

        // Store content
        self.store
            .insert_content(&hash, &content, &parsed.mime_type)
            .await?;

        // Get file extension
        let file_type = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| format!(".{}", e))
            .unwrap_or_default();

        // Upsert document
        self.store
            .upsert_document(
                collection,
                relative_path,
                parsed.title.as_deref(),
                &hash,
                &file_type,
                &parsed.body,
            )
            .await?;

        Ok(true)
    }
}

/// Calculate SHA-256 hash of content
fn calculate_hash(content: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content);
    let result = hasher.finalize();
    hex::encode(result)
}

/// No-op progress reporter
struct NoopProgress;

impl IndexProgress for NoopProgress {
    fn on_file(&mut self, _path: &Path, _status: FileStatus) {}
    fn on_complete(&mut self, _stats: &IndexStats) {}
}

mod hex {
    pub fn encode(bytes: impl AsRef<[u8]>) -> String {
        bytes
            .as_ref()
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_index_collection() {
        let dir = tempdir().unwrap();

        // Create test files
        File::create(dir.path().join("test1.md"))
            .unwrap()
            .write_all(b"# Test 1\n\nContent here")
            .unwrap();

        File::create(dir.path().join("test2.md"))
            .unwrap()
            .write_all(b"# Test 2\n\nMore content")
            .unwrap();

        // Create store
        let store = Store::open_memory().await.unwrap();

        // Add collection
        store
            .add_collection("test", dir.path().to_str().unwrap(), &["**/*.md"])
            .await
            .unwrap();

        // Index
        let indexer = Indexer::new(&store);
        let stats = indexer.index_collection("test").await.unwrap();

        assert_eq!(stats.files_scanned, 2);
        assert_eq!(stats.files_indexed, 2);
        assert_eq!(stats.errors, 0);

        // Verify documents in database
        assert_eq!(store.count_documents(Some("test")).await.unwrap(), 2);
    }

    #[tokio::test]
    async fn test_incremental_index() {
        let dir = tempdir().unwrap();

        // Create initial file
        let file_path = dir.path().join("test.md");
        File::create(&file_path)
            .unwrap()
            .write_all(b"# Initial\n\nContent")
            .unwrap();

        // Create store and index
        let store = Store::open_memory().await.unwrap();
        store
            .add_collection("test", dir.path().to_str().unwrap(), &["**/*.md"])
            .await
            .unwrap();

        let indexer = Indexer::new(&store);

        // First index
        let stats1 = indexer.index_collection("test").await.unwrap();
        assert_eq!(stats1.files_indexed, 1);

        // Second index (unchanged)
        let stats2 = indexer.index_collection("test").await.unwrap();
        assert_eq!(stats2.files_indexed, 0);
        assert_eq!(stats2.files_skipped, 1);

        // Modify file
        File::create(&file_path)
            .unwrap()
            .write_all(b"# Modified\n\nNew content")
            .unwrap();

        // Third index (changed)
        let stats3 = indexer.index_collection("test").await.unwrap();
        assert_eq!(stats3.files_indexed, 1);
    }

    #[test]
    fn test_hash_calculation() {
        let content = b"Hello, World!";
        let hash = calculate_hash(content);

        // SHA-256 of "Hello, World!" in hex
        assert_eq!(hash.len(), 64);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
