//! Database store for QFS
//!
//! The store manages the SQLite database containing:
//! - Content (content-addressable storage)
//! - Documents (file metadata)
//! - Collections (indexed directories)
//! - FTS5 index (full-text search)
//! - Embeddings (optional vector storage)
//!
//! ## Vector Search
//!
//! The store supports two vector search modes:
//! - **Native** (via libsql): Uses `vector_top_k()` with an ANN index for efficient KNN search.
//!   Requires embeddings to be stored in libsql's vector format.
//! - **Legacy** (fallback): Loads all embeddings into memory and calculates cosine similarity.
//!   Works with embeddings stored as raw BLOB data.
//!
//! The search automatically falls back to legacy mode if native search is not available.

mod schema;

use crate::error::{Error, Result};
use chrono::Utc;
use glob::Pattern;
use libsql::{params, Builder, Connection, Database};
use std::path::{Path, PathBuf};

pub use schema::SCHEMA_VERSION;

/// Default max bytes for multi-get (10KB)
pub const DEFAULT_MULTI_GET_MAX_BYTES: usize = 10 * 1024;

/// Document metadata stored in the database
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Document {
    pub id: i64,
    pub collection: String,
    pub path: String,
    pub title: Option<String>,
    pub hash: String,
    pub file_type: String,
    pub created_at: String,
    pub modified_at: String,
    pub indexed_at: String,
    pub active: bool,
}

/// Collection configuration
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Collection {
    pub name: String,
    pub path: String,
    pub patterns: Vec<String>,
    pub exclude: Vec<String>,
    pub context: Option<String>,
    pub embeddings_enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// Context entry
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PathContext {
    pub id: i64,
    pub collection: Option<String>,
    pub path_prefix: String,
    pub context: String,
    pub created_at: String,
    pub updated_at: String,
}

/// Row returned from BM25 search query
#[derive(Debug, Clone)]
pub struct SearchResultRow {
    pub id: i64,
    pub collection: String,
    pub path: String,
    pub title: Option<String>,
    pub hash: String,
    pub file_type: String,
    pub content_type: String,
    pub size: i64,
    pub bm25_score: f64,
    pub snippet: Option<String>,
}

/// Content stored in content-addressable storage
#[derive(Debug, Clone)]
pub struct Content {
    pub hash: String,
    pub data: Vec<u8>,
    pub content_type: String,
    pub size: i64,
    pub created_at: String,
}

/// Result from multi-get operation
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MultiGetResult {
    pub path: String,
    pub collection: String,
    pub title: Option<String>,
    pub content: Option<String>,
    pub size: i64,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub skipped: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip_reason: Option<String>,
}

/// File entry for ls output
#[derive(Debug, Clone, serde::Serialize)]
pub struct FileEntry {
    pub collection: String,
    pub path: String,
    pub title: Option<String>,
    pub size: i64,
    pub modified_at: String,
}

/// Extract short docid from a full hash (first 6 characters).
pub fn get_docid(hash: &str) -> &str {
    &hash[..6.min(hash.len())]
}

/// Normalize a docid input by stripping quotes and leading #.
/// Handles: "#abc123", 'abc123', "abc123", #abc123, abc123
pub fn normalize_docid(docid: &str) -> String {
    let mut normalized = docid.trim().to_string();

    // Strip surrounding quotes
    if (normalized.starts_with('"') && normalized.ends_with('"'))
        || (normalized.starts_with('\'') && normalized.ends_with('\''))
    {
        normalized = normalized[1..normalized.len() - 1].to_string();
    }

    // Strip leading #
    if normalized.starts_with('#') {
        normalized = normalized[1..].to_string();
    }

    normalized
}

/// Check if a string looks like a docid reference.
/// Returns true if normalized form is valid hex of 6+ chars.
pub fn is_docid(input: &str) -> bool {
    let normalized = normalize_docid(input);
    normalized.len() >= 6 && normalized.chars().all(|c| c.is_ascii_hexdigit())
}

/// The main database store
pub struct Store {
    #[allow(dead_code)]
    db: Database,
    conn: Connection,
    path: PathBuf,
}

impl Store {
    /// Open or create a database at the given path
    pub async fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path_buf = path.as_ref().to_path_buf();

        // Ensure parent directory exists
        if let Some(parent) = path_buf.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let db = Builder::new_local(&path_buf).build().await?;
        let conn = db.connect()?;

        // Enable WAL mode for better concurrent access
        conn.query("PRAGMA journal_mode=WAL;", ()).await?;

        schema::ensure_schema(&conn).await?;

        Ok(Self {
            db,
            conn,
            path: path_buf,
        })
    }

    /// Open an in-memory database (for testing)
    pub async fn open_memory() -> Result<Self> {
        let db = Builder::new_local(":memory:").build().await?;
        let conn = db.connect()?;

        schema::ensure_schema(&conn).await?;

        Ok(Self {
            db,
            conn,
            path: PathBuf::from(":memory:"),
        })
    }

    /// Get the database path
    pub fn path(&self) -> &Path {
        &self.path
    }

    // -------------------------------------------------------------------------
    // Collection operations
    // -------------------------------------------------------------------------

    /// Add a new collection
    pub async fn add_collection(&self, name: &str, path: &str, patterns: &[&str]) -> Result<()> {
        self.add_collection_full(name, path, patterns, &[], None, false)
            .await
    }

    /// Add a collection with all options
    pub async fn add_collection_full(
        &self,
        name: &str,
        path: &str,
        patterns: &[&str],
        exclude: &[&str],
        context: Option<&str>,
        embeddings_enabled: bool,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let patterns_json = serde_json::to_string(&patterns)?;
        let exclude_json = serde_json::to_string(&exclude)?;

        self.conn
            .execute(
                "INSERT OR REPLACE INTO collections
             (name, path, patterns, exclude, context, embeddings_enabled, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
                params![
                    name,
                    path,
                    patterns_json,
                    exclude_json,
                    context,
                    embeddings_enabled,
                    now
                ],
            )
            .await?;

        Ok(())
    }

    /// Get a collection by name
    pub async fn get_collection(&self, name: &str) -> Result<Collection> {
        let mut rows = self
            .conn
            .query(
                "SELECT name, path, patterns, exclude, context, embeddings_enabled, created_at, updated_at
             FROM collections WHERE name = ?1",
                params![name],
            )
            .await?;

        if let Some(row) = rows.next().await? {
            let patterns_json: String = row.get(2)?;
            let exclude_json: String = row.get(3)?;

            Ok(Collection {
                name: row.get(0)?,
                path: row.get(1)?,
                patterns: serde_json::from_str(&patterns_json).unwrap_or_default(),
                exclude: serde_json::from_str(&exclude_json).unwrap_or_default(),
                context: row.get(4)?,
                embeddings_enabled: row.get(5)?,
                created_at: row.get(6)?,
                updated_at: row.get(7)?,
            })
        } else {
            Err(Error::CollectionNotFound(name.to_string()))
        }
    }

    /// List all collections
    pub async fn list_collections(&self) -> Result<Vec<Collection>> {
        let mut rows = self
            .conn
            .query(
                "SELECT name, path, patterns, exclude, context, embeddings_enabled, created_at, updated_at
             FROM collections ORDER BY name",
                (),
            )
            .await?;

        let mut collections = Vec::new();
        while let Some(row) = rows.next().await? {
            let patterns_json: String = row.get(2)?;
            let exclude_json: String = row.get(3)?;

            collections.push(Collection {
                name: row.get(0)?,
                path: row.get(1)?,
                patterns: serde_json::from_str(&patterns_json).unwrap_or_default(),
                exclude: serde_json::from_str(&exclude_json).unwrap_or_default(),
                context: row.get(4)?,
                embeddings_enabled: row.get(5)?,
                created_at: row.get(6)?,
                updated_at: row.get(7)?,
            });
        }

        Ok(collections)
    }

    /// Remove a collection and its documents
    pub async fn remove_collection(&self, name: &str) -> Result<()> {
        // Delete documents first
        self.conn
            .execute(
                "DELETE FROM documents WHERE collection = ?1",
                params![name],
            )
            .await?;

        // Delete the collection
        self.conn
            .execute("DELETE FROM collections WHERE name = ?1", params![name])
            .await?;

        Ok(())
    }

    // -------------------------------------------------------------------------
    // Content operations (content-addressable storage)
    // -------------------------------------------------------------------------

    /// Check if content exists by hash
    pub async fn content_exists(&self, hash: &str) -> Result<bool> {
        let mut rows = self
            .conn
            .query(
                "SELECT COUNT(*) FROM content WHERE hash = ?1",
                params![hash],
            )
            .await?;

        let count: i64 = if let Some(row) = rows.next().await? {
            row.get(0)?
        } else {
            0
        };

        Ok(count > 0)
    }

    /// Insert content (if not exists)
    pub async fn insert_content(&self, hash: &str, data: &[u8], content_type: &str) -> Result<()> {
        if self.content_exists(hash).await? {
            return Ok(());
        }

        let now = Utc::now().to_rfc3339();
        self.conn
            .execute(
                "INSERT INTO content (hash, content, content_type, size, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
                params![hash, data, content_type, data.len() as i64, now],
            )
            .await?;

        Ok(())
    }

    /// Get content by hash
    pub async fn get_content(&self, hash: &str) -> Result<Content> {
        let mut rows = self
            .conn
            .query(
                "SELECT hash, content, content_type, size, created_at FROM content WHERE hash = ?1",
                params![hash],
            )
            .await?;

        if let Some(row) = rows.next().await? {
            Ok(Content {
                hash: row.get(0)?,
                data: row.get(1)?,
                content_type: row.get(2)?,
                size: row.get(3)?,
                created_at: row.get(4)?,
            })
        } else {
            Err(Error::DocumentNotFound(hash.to_string()))
        }
    }

    // -------------------------------------------------------------------------
    // Document operations
    // -------------------------------------------------------------------------

    /// Upsert a document
    pub async fn upsert_document(
        &self,
        collection: &str,
        path: &str,
        title: Option<&str>,
        hash: &str,
        file_type: &str,
        body: &str,
    ) -> Result<i64> {
        let now = Utc::now().to_rfc3339();
        let filepath = format!("{}/{}", collection, path);

        // Insert or update the document
        self.conn
            .execute(
                "INSERT INTO documents (collection, path, title, hash, file_type, created_at, modified_at, indexed_at, active)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, ?6, 1)
             ON CONFLICT(collection, path) DO UPDATE SET
               title = excluded.title,
               hash = excluded.hash,
               file_type = excluded.file_type,
               modified_at = excluded.modified_at,
               indexed_at = excluded.indexed_at,
               active = 1",
                params![collection, path, title, hash, file_type, now],
            )
            .await?;

        // Get the document ID
        let mut rows = self
            .conn
            .query(
                "SELECT id FROM documents WHERE collection = ?1 AND path = ?2",
                params![collection, path],
            )
            .await?;

        let id: i64 = if let Some(row) = rows.next().await? {
            row.get(0)?
        } else {
            return Err(Error::DocumentNotFound(format!("{}/{}", collection, path)));
        };

        // Update FTS index (FTS5 doesn't support ON CONFLICT, so delete first)
        self.conn
            .execute("DELETE FROM documents_fts WHERE rowid = ?1", params![id])
            .await?;
        self.conn
            .execute(
                "INSERT INTO documents_fts (rowid, filepath, title, body)
             VALUES (?1, ?2, ?3, ?4)",
                params![id, filepath, title.unwrap_or(""), body],
            )
            .await?;

        Ok(id)
    }

    /// Get a document by collection and path
    pub async fn get_document(&self, collection: &str, path: &str) -> Result<Document> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, collection, path, title, hash, file_type, created_at, modified_at, indexed_at, active
             FROM documents WHERE collection = ?1 AND path = ?2 AND active = 1",
                params![collection, path],
            )
            .await?;

        if let Some(row) = rows.next().await? {
            Ok(Document {
                id: row.get(0)?,
                collection: row.get(1)?,
                path: row.get(2)?,
                title: row.get(3)?,
                hash: row.get(4)?,
                file_type: row.get(5)?,
                created_at: row.get(6)?,
                modified_at: row.get(7)?,
                indexed_at: row.get(8)?,
                active: row.get(9)?,
            })
        } else {
            Err(Error::DocumentNotFound(format!("{}/{}", collection, path)))
        }
    }

    /// Get a document by ID
    pub async fn get_document_by_id(&self, id: i64) -> Result<Document> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, collection, path, title, hash, file_type, created_at, modified_at, indexed_at, active
             FROM documents WHERE id = ?1",
                params![id],
            )
            .await?;

        if let Some(row) = rows.next().await? {
            Ok(Document {
                id: row.get(0)?,
                collection: row.get(1)?,
                path: row.get(2)?,
                title: row.get(3)?,
                hash: row.get(4)?,
                file_type: row.get(5)?,
                created_at: row.get(6)?,
                modified_at: row.get(7)?,
                indexed_at: row.get(8)?,
                active: row.get(9)?,
            })
        } else {
            Err(Error::DocumentNotFound(id.to_string()))
        }
    }

    /// Find a document by its short docid (first 6 characters of hash).
    /// Returns the first matching document if found.
    pub async fn get_document_by_docid(&self, docid: &str) -> Result<Document> {
        let short_hash = normalize_docid(docid);

        if short_hash.len() < 6 {
            return Err(Error::InvalidQuery(
                "Docid must be at least 6 characters".to_string(),
            ));
        }

        let pattern = format!("{}%", short_hash);

        let mut rows = self
            .conn
            .query(
                "SELECT id, collection, path, title, hash, file_type, created_at, modified_at, indexed_at, active
             FROM documents WHERE hash LIKE ?1 AND active = 1 LIMIT 1",
                params![pattern],
            )
            .await?;

        if let Some(row) = rows.next().await? {
            Ok(Document {
                id: row.get(0)?,
                collection: row.get(1)?,
                path: row.get(2)?,
                title: row.get(3)?,
                hash: row.get(4)?,
                file_type: row.get(5)?,
                created_at: row.get(6)?,
                modified_at: row.get(7)?,
                indexed_at: row.get(8)?,
                active: row.get(9)?,
            })
        } else {
            Err(Error::DocumentNotFound(format!("docid:{}", short_hash)))
        }
    }

    /// Mark a document as inactive (soft delete)
    pub async fn deactivate_document(&self, collection: &str, path: &str) -> Result<()> {
        // First get the document ID to remove from FTS
        if let Ok(doc) = self.get_document(collection, path).await {
            // Remove from FTS index
            self.conn
                .execute(
                    "DELETE FROM documents_fts WHERE rowid = ?1",
                    params![doc.id],
                )
                .await?;
        }

        self.conn
            .execute(
                "UPDATE documents SET active = 0 WHERE collection = ?1 AND path = ?2",
                params![collection, path],
            )
            .await?;
        Ok(())
    }

    /// List all documents in a collection
    pub async fn list_documents(&self, collection: &str) -> Result<Vec<Document>> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, collection, path, title, hash, file_type, created_at, modified_at, indexed_at, active
             FROM documents WHERE collection = ?1 AND active = 1
             ORDER BY path",
                params![collection],
            )
            .await?;

        let mut docs = Vec::new();
        while let Some(row) = rows.next().await? {
            docs.push(Document {
                id: row.get(0)?,
                collection: row.get(1)?,
                path: row.get(2)?,
                title: row.get(3)?,
                hash: row.get(4)?,
                file_type: row.get(5)?,
                created_at: row.get(6)?,
                modified_at: row.get(7)?,
                indexed_at: row.get(8)?,
                active: row.get(9)?,
            });
        }

        Ok(docs)
    }

    /// List all active documents across all collections
    pub async fn list_all_documents(&self) -> Result<Vec<Document>> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, collection, path, title, hash, file_type, created_at, modified_at, indexed_at, active
             FROM documents WHERE active = 1
             ORDER BY collection, path",
                (),
            )
            .await?;

        let mut docs = Vec::new();
        while let Some(row) = rows.next().await? {
            docs.push(Document {
                id: row.get(0)?,
                collection: row.get(1)?,
                path: row.get(2)?,
                title: row.get(3)?,
                hash: row.get(4)?,
                file_type: row.get(5)?,
                created_at: row.get(6)?,
                modified_at: row.get(7)?,
                indexed_at: row.get(8)?,
                active: row.get(9)?,
            });
        }

        Ok(docs)
    }

    /// Count documents in a collection
    pub async fn count_documents(&self, collection: Option<&str>) -> Result<i64> {
        let count: i64 = if let Some(coll) = collection {
            let mut rows = self
                .conn
                .query(
                    "SELECT COUNT(*) FROM documents WHERE collection = ?1 AND active = 1",
                    params![coll],
                )
                .await?;

            if let Some(row) = rows.next().await? {
                row.get(0)?
            } else {
                0
            }
        } else {
            let mut rows = self
                .conn
                .query("SELECT COUNT(*) FROM documents WHERE active = 1", ())
                .await?;

            if let Some(row) = rows.next().await? {
                row.get(0)?
            } else {
                0
            }
        };
        Ok(count)
    }

    /// List files in a collection, optionally filtered by path prefix.
    pub async fn list_files(
        &self,
        collection: &str,
        path_prefix: Option<&str>,
    ) -> Result<Vec<FileEntry>> {
        let mut entries = Vec::new();

        if let Some(prefix) = path_prefix {
            let pattern = format!("{}%", prefix);
            let mut rows = self
                .conn
                .query(
                    r#"
                SELECT d.collection, d.path, d.title, LENGTH(c.content) as size, d.modified_at
                FROM documents d
                JOIN content c ON c.hash = d.hash
                WHERE d.collection = ?1 AND d.path LIKE ?2 AND d.active = 1
                ORDER BY d.path
                "#,
                    params![collection, pattern],
                )
                .await?;

            while let Some(row) = rows.next().await? {
                entries.push(FileEntry {
                    collection: row.get(0)?,
                    path: row.get(1)?,
                    title: row.get(2)?,
                    size: row.get(3)?,
                    modified_at: row.get(4)?,
                });
            }
        } else {
            let mut rows = self
                .conn
                .query(
                    r#"
                SELECT d.collection, d.path, d.title, LENGTH(c.content) as size, d.modified_at
                FROM documents d
                JOIN content c ON c.hash = d.hash
                WHERE d.collection = ?1 AND d.active = 1
                ORDER BY d.path
                "#,
                    params![collection],
                )
                .await?;

            while let Some(row) = rows.next().await? {
                entries.push(FileEntry {
                    collection: row.get(0)?,
                    path: row.get(1)?,
                    title: row.get(2)?,
                    size: row.get(3)?,
                    modified_at: row.get(4)?,
                });
            }
        }

        Ok(entries)
    }

    // -------------------------------------------------------------------------
    // Context operations
    // -------------------------------------------------------------------------

    /// Add or update a context for a path prefix.
    /// Use collection=None for global context.
    pub async fn set_context(
        &self,
        collection: Option<&str>,
        path_prefix: &str,
        context: &str,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();

        // Normalize path prefix to start with /
        let normalized = if path_prefix.starts_with('/') {
            path_prefix.to_string()
        } else {
            format!("/{}", path_prefix)
        };

        self.conn
            .execute(
                "INSERT INTO path_contexts (collection, path_prefix, context, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?4)
             ON CONFLICT(collection, path_prefix) DO UPDATE SET
               context = excluded.context,
               updated_at = excluded.updated_at",
                params![collection, normalized, context, now],
            )
            .await?;

        Ok(())
    }

    /// Get the global context.
    pub async fn get_global_context(&self) -> Result<Option<String>> {
        let mut rows = self
            .conn
            .query(
                "SELECT context FROM path_contexts WHERE collection IS NULL AND path_prefix = '/'",
                (),
            )
            .await?;

        if let Some(row) = rows.next().await? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }

    /// Find the most specific context for a file path.
    /// Uses longest-prefix matching.
    pub async fn find_context_for_path(
        &self,
        collection: &str,
        file_path: &str,
    ) -> Result<Option<String>> {
        // Normalize file path
        let normalized = if file_path.starts_with('/') {
            file_path.to_string()
        } else {
            format!("/{}", file_path)
        };

        // Get all matching contexts for this collection (sorted by prefix length desc)
        let mut rows = self
            .conn
            .query(
                "SELECT path_prefix, context FROM path_contexts
             WHERE (collection = ?1 OR collection IS NULL)
             ORDER BY
               CASE WHEN collection IS NOT NULL THEN 0 ELSE 1 END,
               LENGTH(path_prefix) DESC",
                params![collection],
            )
            .await?;

        while let Some(row) = rows.next().await? {
            let prefix: String = row.get(0)?;
            let context: String = row.get(1)?;

            // Check if file path starts with this prefix
            if normalized.starts_with(&prefix) || normalized == prefix.trim_end_matches('/') {
                return Ok(Some(context));
            }
        }

        Ok(None)
    }

    /// Get all contexts for a file path (global + collection, ordered general to specific).
    /// Used for search results where we want all relevant context.
    pub async fn get_all_contexts_for_path(
        &self,
        collection: &str,
        file_path: &str,
    ) -> Result<Vec<String>> {
        let normalized = if file_path.starts_with('/') {
            file_path.to_string()
        } else {
            format!("/{}", file_path)
        };

        let mut rows = self
            .conn
            .query(
                "SELECT path_prefix, context, collection FROM path_contexts
             WHERE (collection = ?1 OR collection IS NULL)
             ORDER BY
               CASE WHEN collection IS NULL THEN 0 ELSE 1 END,
               LENGTH(path_prefix) ASC",
                params![collection],
            )
            .await?;

        let mut contexts = Vec::new();
        while let Some(row) = rows.next().await? {
            let prefix: String = row.get(0)?;
            let context: String = row.get(1)?;

            if normalized.starts_with(&prefix) || prefix == "/" {
                contexts.push(context);
            }
        }

        Ok(contexts)
    }

    /// List all contexts.
    pub async fn list_contexts(&self) -> Result<Vec<PathContext>> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, collection, path_prefix, context, created_at, updated_at
             FROM path_contexts
             ORDER BY
               CASE WHEN collection IS NULL THEN '' ELSE collection END,
               path_prefix",
                (),
            )
            .await?;

        let mut contexts = Vec::new();
        while let Some(row) = rows.next().await? {
            contexts.push(PathContext {
                id: row.get(0)?,
                collection: row.get(1)?,
                path_prefix: row.get(2)?,
                context: row.get(3)?,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
            });
        }

        Ok(contexts)
    }

    /// Remove a context.
    pub async fn remove_context(
        &self,
        collection: Option<&str>,
        path_prefix: &str,
    ) -> Result<bool> {
        let normalized = if path_prefix.starts_with('/') {
            path_prefix.to_string()
        } else {
            format!("/{}", path_prefix)
        };

        let rows_affected = self
            .conn
            .execute(
                "DELETE FROM path_contexts WHERE collection IS ?1 AND path_prefix = ?2",
                params![collection, normalized],
            )
            .await?;

        Ok(rows_affected > 0)
    }

    /// Get collections without any context defined.
    pub async fn get_collections_without_context(&self) -> Result<Vec<Collection>> {
        let all_collections = self.list_collections().await?;

        let mut without_context = Vec::new();
        for coll in all_collections {
            let coll_name = coll.name.clone();
            let mut rows = self
                .conn
                .query(
                    "SELECT COUNT(*) FROM path_contexts WHERE collection = ?1",
                    params![coll_name],
                )
                .await?;

            let has_context: i64 = if let Some(row) = rows.next().await? {
                row.get(0)?
            } else {
                0
            };

            if has_context == 0 {
                without_context.push(coll);
            }
        }

        Ok(without_context)
    }

    // -------------------------------------------------------------------------
    // Search operations
    // -------------------------------------------------------------------------

    /// Execute BM25 full-text search
    pub async fn search_bm25(
        &self,
        fts_query: &str,
        collection: Option<&str>,
        limit: usize,
        include_binary: bool,
    ) -> Result<Vec<SearchResultRow>> {
        let mut results = Vec::new();

        if fts_query.is_empty() {
            return Ok(results);
        }

        let mut rows = if let Some(coll) = collection {
            self.conn
                .query(
                    r#"
                SELECT
                    d.id,
                    d.collection,
                    d.path,
                    d.title,
                    d.hash,
                    d.file_type,
                    c.content_type,
                    c.size,
                    bm25(documents_fts) as bm25_score,
                    snippet(documents_fts, 2, '<mark>', '</mark>', '...', 64) as snippet
                FROM documents_fts
                JOIN documents d ON d.id = documents_fts.rowid
                JOIN content c ON c.hash = d.hash
                WHERE documents_fts MATCH ?1
                  AND d.collection = ?2
                  AND d.active = 1
                ORDER BY bm25_score
                LIMIT ?3
                "#,
                    params![fts_query, coll, limit as i64],
                )
                .await?
        } else {
            self.conn
                .query(
                    r#"
                SELECT
                    d.id,
                    d.collection,
                    d.path,
                    d.title,
                    d.hash,
                    d.file_type,
                    c.content_type,
                    c.size,
                    bm25(documents_fts) as bm25_score,
                    snippet(documents_fts, 2, '<mark>', '</mark>', '...', 64) as snippet
                FROM documents_fts
                JOIN documents d ON d.id = documents_fts.rowid
                JOIN content c ON c.hash = d.hash
                WHERE documents_fts MATCH ?1
                  AND d.active = 1
                ORDER BY bm25_score
                LIMIT ?2
                "#,
                    params![fts_query, limit as i64],
                )
                .await?
        };

        while let Some(row) = rows.next().await? {
            let content_type: String = row.get(6)?;

            // Skip binary files unless requested
            let is_binary = content_type.starts_with("application/octet")
                || content_type.starts_with("image/")
                || content_type.starts_with("audio/")
                || content_type.starts_with("video/");

            if is_binary && !include_binary {
                continue;
            }

            results.push(SearchResultRow {
                id: row.get(0)?,
                collection: row.get(1)?,
                path: row.get(2)?,
                title: row.get(3)?,
                hash: row.get(4)?,
                file_type: row.get(5)?,
                content_type,
                size: row.get(7)?,
                bm25_score: row.get(8)?,
                snippet: row.get(9)?,
            });
        }

        Ok(results)
    }

    /// Get database file size in bytes
    pub fn database_size(&self) -> Result<u64> {
        if self.path.to_str() == Some(":memory:") {
            return Ok(0);
        }
        let metadata = std::fs::metadata(&self.path)?;
        Ok(metadata.len())
    }

    // -------------------------------------------------------------------------
    // Embedding operations
    // -------------------------------------------------------------------------

    /// Insert embeddings for a document chunk
    /// Column is F32_BLOB(384) which automatically handles the vector format
    pub async fn insert_embedding(
        &self,
        hash: &str,
        chunk_index: i32,
        char_offset: i32,
        model: &str,
        embedding: &[u8],
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();

        // F32_BLOB column accepts raw bytes (little-endian f32 array)
        self.conn
            .execute(
                "INSERT OR REPLACE INTO embeddings (hash, chunk_index, char_offset, model, embedding, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![hash, chunk_index, char_offset, model, embedding, now],
            )
            .await?;

        Ok(())
    }

    /// Get all embeddings for a document hash
    pub async fn get_embeddings(&self, hash: &str) -> Result<Vec<EmbeddingRow>> {
        let mut rows = self
            .conn
            .query(
                "SELECT hash, chunk_index, char_offset, model, embedding, created_at
             FROM embeddings WHERE hash = ?1 ORDER BY chunk_index",
                params![hash],
            )
            .await?;

        let mut results = Vec::new();
        while let Some(row) = rows.next().await? {
            results.push(EmbeddingRow {
                hash: row.get(0)?,
                chunk_index: row.get(1)?,
                char_offset: row.get(2)?,
                model: row.get(3)?,
                embedding: row.get(4)?,
                created_at: row.get(5)?,
            });
        }

        Ok(results)
    }

    /// Check if embeddings exist for a document hash
    pub async fn has_embeddings(&self, hash: &str) -> Result<bool> {
        let mut rows = self
            .conn
            .query(
                "SELECT COUNT(*) FROM embeddings WHERE hash = ?1",
                params![hash],
            )
            .await?;

        let count: i64 = if let Some(row) = rows.next().await? {
            row.get(0)?
        } else {
            0
        };

        Ok(count > 0)
    }

    /// Delete embeddings for a document hash
    pub async fn delete_embeddings(&self, hash: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM embeddings WHERE hash = ?1", params![hash])
            .await?;
        Ok(())
    }

    /// Count documents with embeddings
    pub async fn count_embeddings(&self, collection: Option<&str>) -> Result<i64> {
        let count: i64 = if let Some(coll) = collection {
            let mut rows = self
                .conn
                .query(
                    "SELECT COUNT(DISTINCT e.hash)
                 FROM embeddings e
                 JOIN documents d ON d.hash = e.hash
                 WHERE d.collection = ?1 AND d.active = 1",
                    params![coll],
                )
                .await?;

            if let Some(row) = rows.next().await? {
                row.get(0)?
            } else {
                0
            }
        } else {
            let mut rows = self
                .conn
                .query("SELECT COUNT(DISTINCT hash) FROM embeddings", ())
                .await?;

            if let Some(row) = rows.next().await? {
                row.get(0)?
            } else {
                0
            }
        };
        Ok(count)
    }

    /// Get all embeddings for vector search (legacy - loads all into memory)
    /// Returns embeddings joined with document info
    /// DEPRECATED: Use search_vector_native() for efficient native vector search
    pub async fn get_all_embeddings_for_search(
        &self,
        collection: Option<&str>,
    ) -> Result<Vec<EmbeddingSearchRow>> {
        let mut results = Vec::new();

        let mut rows = if let Some(coll) = collection {
            self.conn
                .query(
                    r#"
                SELECT
                    e.hash,
                    e.chunk_index,
                    e.char_offset,
                    e.embedding,
                    d.id,
                    d.collection,
                    d.path,
                    d.title,
                    d.file_type
                FROM embeddings e
                JOIN documents d ON d.hash = e.hash
                WHERE d.collection = ?1 AND d.active = 1
                ORDER BY d.id, e.chunk_index
                "#,
                    params![coll],
                )
                .await?
        } else {
            self.conn
                .query(
                    r#"
                SELECT
                    e.hash,
                    e.chunk_index,
                    e.char_offset,
                    e.embedding,
                    d.id,
                    d.collection,
                    d.path,
                    d.title,
                    d.file_type
                FROM embeddings e
                JOIN documents d ON d.hash = e.hash
                WHERE d.active = 1
                ORDER BY d.id, e.chunk_index
                "#,
                    (),
                )
                .await?
        };

        while let Some(row) = rows.next().await? {
            results.push(EmbeddingSearchRow {
                hash: row.get(0)?,
                chunk_index: row.get(1)?,
                char_offset: row.get(2)?,
                embedding: row.get(3)?,
                doc_id: row.get(4)?,
                collection: row.get(5)?,
                path: row.get(6)?,
                title: row.get(7)?,
                file_type: row.get(8)?,
            });
        }

        Ok(results)
    }

    /// Ensure the vector index exists for native vector search.
    /// Returns true if the index was created, false if it already existed or couldn't be created.
    pub async fn ensure_vector_index(&self) -> Result<bool> {
        schema::ensure_vector_index(&self.conn).await
    }

    /// Check if native vector search is available
    pub async fn has_vector_index(&self) -> bool {
        schema::has_vector_index(&self.conn).await
    }

    /// Native vector search using libsql's vector_top_k()
    /// Uses the idx_embeddings_vector index for efficient KNN search.
    /// Returns None if native search is not available (falls back to legacy).
    pub async fn search_vector_native(
        &self,
        query_embedding: &[f32],
        collection: Option<&str>,
        limit: usize,
    ) -> Result<Option<Vec<VectorSearchResult>>> {
        // Try to ensure vector index exists
        self.ensure_vector_index().await?;

        // Check if index is actually available
        if !self.has_vector_index().await {
            return Ok(None); // Fall back to legacy search
        }

        // Convert f32 embedding to bytes (little-endian)
        let embedding_bytes: Vec<u8> = query_embedding
            .iter()
            .flat_map(|f| f.to_le_bytes())
            .collect();

        // Use vector_top_k for efficient KNN search
        // The function returns rowids from the index, which we join with embeddings and documents
        // F32_BLOB column accepts raw bytes directly
        let query_result = if let Some(coll) = collection {
            self.conn
                .query(
                    r#"
                SELECT
                    e.hash,
                    e.chunk_index,
                    e.char_offset,
                    d.id,
                    d.collection,
                    d.path,
                    d.title,
                    d.file_type,
                    vector_distance_cos(e.embedding, ?1) as distance
                FROM vector_top_k('idx_embeddings_vector', ?1, ?2) AS top_k
                JOIN embeddings e ON e.rowid = top_k.id
                JOIN documents d ON d.hash = e.hash
                WHERE d.collection = ?3 AND d.active = 1
                ORDER BY distance ASC
                "#,
                    params![embedding_bytes, limit as i64, coll],
                )
                .await
        } else {
            self.conn
                .query(
                    r#"
                SELECT
                    e.hash,
                    e.chunk_index,
                    e.char_offset,
                    d.id,
                    d.collection,
                    d.path,
                    d.title,
                    d.file_type,
                    vector_distance_cos(e.embedding, ?1) as distance
                FROM vector_top_k('idx_embeddings_vector', ?1, ?2) AS top_k
                JOIN embeddings e ON e.rowid = top_k.id
                JOIN documents d ON d.hash = e.hash
                WHERE d.active = 1
                ORDER BY distance ASC
                "#,
                    params![embedding_bytes, limit as i64],
                )
                .await
        };

        // If the query fails (e.g., index not working), return None to trigger fallback
        let mut rows = match query_result {
            Ok(rows) => rows,
            Err(e) => {
                tracing::debug!("Native vector search failed, using legacy: {}", e);
                return Ok(None);
            }
        };

        let mut results = Vec::new();

        while let Some(row) = rows.next().await? {
            let distance: f64 = row.get(8)?;
            // Convert cosine distance to similarity (1 - distance for cosine)
            let similarity = 1.0 - distance;

            results.push(VectorSearchResult {
                hash: row.get(0)?,
                chunk_index: row.get(1)?,
                char_offset: row.get(2)?,
                doc_id: row.get(3)?,
                collection: row.get(4)?,
                path: row.get(5)?,
                title: row.get(6)?,
                file_type: row.get(7)?,
                similarity,
            });
        }

        Ok(Some(results))
    }

    /// Legacy vector search - loads all embeddings and calculates similarity in Rust
    /// Used as fallback when native vector search is not available
    pub async fn search_vector_legacy(
        &self,
        query_embedding: &[f32],
        collection: Option<&str>,
        limit: usize,
    ) -> Result<Vec<VectorSearchResult>> {
        let embeddings = self.get_all_embeddings_for_search(collection).await?;

        let mut scored: Vec<(f64, EmbeddingSearchRow)> = embeddings
            .into_iter()
            .map(|row| {
                let embedding = bytes_to_embedding(&row.embedding);
                let similarity = cosine_similarity(query_embedding, &embedding);
                (similarity as f64, row)
            })
            .collect();

        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        Ok(scored
            .into_iter()
            .take(limit)
            .map(|(similarity, row)| VectorSearchResult {
                hash: row.hash,
                chunk_index: row.chunk_index,
                char_offset: row.char_offset,
                doc_id: row.doc_id,
                collection: row.collection,
                path: row.path,
                title: row.title,
                file_type: row.file_type,
                similarity,
            })
            .collect())
    }

    // -------------------------------------------------------------------------
    // Multi-get operations
    // -------------------------------------------------------------------------

    /// Match files by glob pattern.
    /// Returns files matching the pattern with their sizes (before loading content).
    pub async fn match_files_by_glob(&self, pattern: &str) -> Result<Vec<(String, String, i64)>> {
        let glob = Pattern::new(pattern)
            .map_err(|e| Error::InvalidQuery(format!("Invalid glob pattern: {}", e)))?;

        let mut rows = self
            .conn
            .query(
                "SELECT d.collection, d.path, LENGTH(c.content) as size
             FROM documents d
             JOIN content c ON c.hash = d.hash
             WHERE d.active = 1",
                (),
            )
            .await?;

        let mut matches = Vec::new();
        while let Some(row) = rows.next().await? {
            let collection: String = row.get(0)?;
            let path: String = row.get(1)?;
            let size: i64 = row.get(2)?;

            let full_path = format!("{}/{}", collection, path);
            let virtual_path = format!("qfs://{}/{}", collection, path);

            // Match against both formats
            if glob.matches(&full_path) || glob.matches(&path) || glob.matches(&virtual_path) {
                matches.push((collection, path, size));
            }
        }

        Ok(matches)
    }

    /// Parse comma-separated list of paths.
    /// Returns list of (collection, path, size) tuples found.
    pub async fn parse_comma_list(&self, input: &str) -> Result<Vec<(String, String, i64)>> {
        let names: Vec<&str> = input
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();

        let mut results = Vec::new();

        for name in names {
            // Try exact match first (collection/path format)
            if let Some((coll, path)) = name.split_once('/') {
                if let Ok(doc) = self.get_document(coll, path).await {
                    let content = self.get_content(&doc.hash).await?;
                    results.push((doc.collection, doc.path, content.size));
                    continue;
                }
            }

            // Try suffix match
            let pattern = format!("%{}", name);
            let mut rows = self
                .conn
                .query(
                    "SELECT d.collection, d.path, LENGTH(c.content) as size
                 FROM documents d
                 JOIN content c ON c.hash = d.hash
                 WHERE d.path LIKE ?1 AND d.active = 1
                 LIMIT 1",
                    params![pattern],
                )
                .await?;

            if let Some(row) = rows.next().await? {
                results.push((row.get(0)?, row.get(1)?, row.get(2)?));
            }
            // Note: silently skip if not found (could add error collection)
        }

        Ok(results)
    }

    /// Get multiple documents with size filtering.
    pub async fn multi_get(
        &self,
        pattern: &str,
        max_bytes: usize,
        max_lines: Option<usize>,
    ) -> Result<Vec<MultiGetResult>> {
        // Detect pattern type
        let is_glob = pattern.contains('*') || pattern.contains('?');
        let is_comma_list = pattern.contains(',') && !is_glob;

        let files = if is_glob {
            self.match_files_by_glob(pattern).await?
        } else if is_comma_list {
            self.parse_comma_list(pattern).await?
        } else {
            // Single file
            let parts: Vec<&str> = pattern.splitn(2, '/').collect();
            if parts.len() == 2 {
                if let Ok(doc) = self.get_document(parts[0], parts[1]).await {
                    let content = self.get_content(&doc.hash).await?;
                    vec![(doc.collection, doc.path, content.size)]
                } else {
                    vec![]
                }
            } else {
                vec![]
            }
        };

        let mut results = Vec::new();

        for (collection, path, size) in files {
            let full_path = format!("{}/{}", collection, path);

            // Check size limit
            if size as usize > max_bytes {
                results.push(MultiGetResult {
                    path: full_path,
                    collection,
                    title: None,
                    content: None,
                    size,
                    skipped: true,
                    skip_reason: Some(format!(
                        "File too large ({}KB > {}KB). Use 'qfs get' to retrieve.",
                        size / 1024,
                        max_bytes / 1024
                    )),
                });
                continue;
            }

            // Get document and content
            if let Ok(doc) = self.get_document(&collection, &path).await {
                if let Ok(content) = self.get_content(&doc.hash).await {
                    let mut text = String::from_utf8(content.data.clone())
                        .unwrap_or_else(|_| "[Binary content]".to_string());

                    // Apply line limit if specified
                    if let Some(limit) = max_lines {
                        let lines: Vec<&str> = text.lines().take(limit).collect();
                        let original_count = text.lines().count();
                        text = lines.join("\n");
                        if original_count > limit {
                            text.push_str(&format!(
                                "\n\n[... truncated {} more lines]",
                                original_count - limit
                            ));
                        }
                    }

                    results.push(MultiGetResult {
                        path: full_path,
                        collection: doc.collection,
                        title: doc.title,
                        content: Some(text),
                        size,
                        skipped: false,
                        skip_reason: None,
                    });
                }
            }
        }

        Ok(results)
    }
}

/// Row from embeddings table
#[derive(Debug, Clone)]
pub struct EmbeddingRow {
    pub hash: String,
    pub chunk_index: i32,
    pub char_offset: i32,
    pub model: String,
    pub embedding: Vec<u8>,
    pub created_at: String,
}

/// Result from native vector search
#[derive(Debug, Clone)]
pub struct VectorSearchResult {
    pub hash: String,
    pub chunk_index: i32,
    pub char_offset: i32,
    pub doc_id: i64,
    pub collection: String,
    pub path: String,
    pub title: Option<String>,
    pub file_type: String,
    /// Cosine similarity score (0.0 - 1.0)
    pub similarity: f64,
}

/// Convert bytes to f32 embedding (for legacy vector search)
fn bytes_to_embedding(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|chunk| {
            let arr: [u8; 4] = chunk.try_into().unwrap();
            f32::from_le_bytes(arr)
        })
        .collect()
}

/// Calculate cosine similarity between two embeddings (for legacy vector search)
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

/// Row for embedding search (joined with document info)
#[derive(Debug, Clone)]
pub struct EmbeddingSearchRow {
    pub hash: String,
    pub chunk_index: i32,
    pub char_offset: i32,
    pub embedding: Vec<u8>,
    pub doc_id: i64,
    pub collection: String,
    pub path: String,
    pub title: Option<String>,
    pub file_type: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_open_memory() {
        let store = Store::open_memory().await.unwrap();
        assert_eq!(store.path().to_str(), Some(":memory:"));
    }

    #[tokio::test]
    async fn test_collection_operations() {
        let store = Store::open_memory().await.unwrap();

        // Add collection
        store
            .add_collection("test", "/tmp/test", &["**/*.md"])
            .await
            .unwrap();

        // Get collection
        let coll = store.get_collection("test").await.unwrap();
        assert_eq!(coll.name, "test");
        assert_eq!(coll.path, "/tmp/test");
        assert_eq!(coll.patterns, vec!["**/*.md"]);

        // List collections
        let collections = store.list_collections().await.unwrap();
        assert_eq!(collections.len(), 1);

        // Remove collection
        store.remove_collection("test").await.unwrap();
        let collections = store.list_collections().await.unwrap();
        assert_eq!(collections.len(), 0);
    }

    #[tokio::test]
    async fn test_content_operations() {
        let store = Store::open_memory().await.unwrap();

        let hash = "abc123";
        let data = b"Hello, world!";

        // Insert content
        store.insert_content(hash, data, "text/plain").await.unwrap();

        // Check exists
        assert!(store.content_exists(hash).await.unwrap());
        assert!(!store.content_exists("nonexistent").await.unwrap());

        // Get content
        let content = store.get_content(hash).await.unwrap();
        assert_eq!(content.data, data);
        assert_eq!(content.content_type, "text/plain");
    }

    #[tokio::test]
    async fn test_document_operations() {
        let store = Store::open_memory().await.unwrap();

        // Add collection first
        store
            .add_collection("test", "/tmp/test", &["**/*.md"])
            .await
            .unwrap();

        // Insert content
        store
            .insert_content("hash123", b"# Hello\n\nWorld", "text/markdown")
            .await
            .unwrap();

        // Upsert document
        let id = store
            .upsert_document(
                "test",
                "hello.md",
                Some("Hello"),
                "hash123",
                ".md",
                "Hello World",
            )
            .await
            .unwrap();

        assert!(id > 0);

        // Get document
        let doc = store.get_document("test", "hello.md").await.unwrap();
        assert_eq!(doc.title, Some("Hello".to_string()));
        assert_eq!(doc.hash, "hash123");

        // Count documents
        assert_eq!(store.count_documents(Some("test")).await.unwrap(), 1);
        assert_eq!(store.count_documents(None).await.unwrap(), 1);

        // Deactivate
        store.deactivate_document("test", "hello.md").await.unwrap();
        assert_eq!(store.count_documents(Some("test")).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn test_search_bm25() {
        let store = Store::open_memory().await.unwrap();

        // Add collection
        store
            .add_collection("test", "/tmp/test", &["**/*.md"])
            .await
            .unwrap();

        // Insert content and documents
        store
            .insert_content(
                "hash1",
                b"# Rust Programming\n\nRust is a systems programming language.",
                "text/markdown",
            )
            .await
            .unwrap();
        store
            .insert_content(
                "hash2",
                b"# Python Tutorial\n\nPython is a dynamic language.",
                "text/markdown",
            )
            .await
            .unwrap();
        store
            .insert_content(
                "hash3",
                b"# JavaScript Guide\n\nJavaScript runs in browsers.",
                "text/markdown",
            )
            .await
            .unwrap();

        store
            .upsert_document(
                "test",
                "rust.md",
                Some("Rust Programming"),
                "hash1",
                ".md",
                "Rust Programming Rust is a systems programming language",
            )
            .await
            .unwrap();
        store
            .upsert_document(
                "test",
                "python.md",
                Some("Python Tutorial"),
                "hash2",
                ".md",
                "Python Tutorial Python is a dynamic language",
            )
            .await
            .unwrap();
        store
            .upsert_document(
                "test",
                "javascript.md",
                Some("JavaScript Guide"),
                "hash3",
                ".md",
                "JavaScript Guide JavaScript runs in browsers",
            )
            .await
            .unwrap();

        // Search for "rust"
        let results = store
            .search_bm25("\"rust\"*", None, 10, false)
            .await
            .unwrap();
        assert!(!results.is_empty(), "Should find results for 'rust'");
        assert_eq!(results[0].path, "rust.md");

        // Search for "programming"
        let results = store
            .search_bm25("\"programming\"*", None, 10, false)
            .await
            .unwrap();
        assert!(!results.is_empty(), "Should find results for 'programming'");

        // Search for "language"
        let results = store
            .search_bm25("\"language\"*", None, 10, false)
            .await
            .unwrap();
        assert_eq!(results.len(), 2, "Should find 2 results for 'language'");

        // Search with collection filter
        let results = store
            .search_bm25("\"rust\"*", Some("test"), 10, false)
            .await
            .unwrap();
        assert!(!results.is_empty());

        let results = store
            .search_bm25("\"rust\"*", Some("nonexistent"), 10, false)
            .await
            .unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_get_docid() {
        let hash = "abc123def456789012345678901234567890123456789012345678901234";
        assert_eq!(get_docid(hash), "abc123");
    }

    #[test]
    fn test_normalize_docid_with_hash() {
        assert_eq!(normalize_docid("#abc123"), "abc123");
    }

    #[test]
    fn test_normalize_docid_bare() {
        assert_eq!(normalize_docid("abc123"), "abc123");
    }

    #[test]
    fn test_normalize_docid_quoted() {
        assert_eq!(normalize_docid("\"#abc123\""), "abc123");
        assert_eq!(normalize_docid("'abc123'"), "abc123");
    }

    #[test]
    fn test_normalize_docid_whitespace() {
        assert_eq!(normalize_docid("  #abc123  "), "abc123");
    }

    #[test]
    fn test_is_docid_valid() {
        assert!(is_docid("#abc123"));
        assert!(is_docid("abc123"));
        assert!(is_docid("ABC123")); // Case insensitive
        assert!(is_docid("abc123def456")); // Longer is ok
    }

    #[test]
    fn test_is_docid_invalid() {
        assert!(!is_docid("abc12")); // Too short
        assert!(!is_docid("ghijkl")); // Non-hex
        assert!(!is_docid("abc123.md")); // Has extension
        assert!(!is_docid("qfs://collection/path")); // Virtual path
    }

    #[tokio::test]
    async fn test_get_document_by_docid() {
        let store = Store::open_memory().await.unwrap();
        store
            .add_collection("test", "/tmp/test", &["**/*.md"])
            .await
            .unwrap();
        store
            .insert_content("abc123def456", b"Test content", "text/plain")
            .await
            .unwrap();
        store
            .upsert_document(
                "test",
                "file.md",
                Some("Title"),
                "abc123def456",
                ".md",
                "Test",
            )
            .await
            .unwrap();

        let doc = store.get_document_by_docid("#abc123").await.unwrap();
        assert_eq!(doc.path, "file.md");

        let doc = store.get_document_by_docid("abc123").await.unwrap();
        assert_eq!(doc.path, "file.md");
    }

    #[tokio::test]
    async fn test_list_files() {
        let store = Store::open_memory().await.unwrap();
        store
            .add_collection("docs", "/tmp/docs", &["**/*.md"])
            .await
            .unwrap();
        store
            .insert_content("hash1", b"content1", "text/plain")
            .await
            .unwrap();
        store
            .insert_content("hash2", b"content2", "text/plain")
            .await
            .unwrap();

        store
            .upsert_document("docs", "readme.md", None, "hash1", ".md", "content")
            .await
            .unwrap();
        store
            .upsert_document("docs", "guide/intro.md", None, "hash2", ".md", "content")
            .await
            .unwrap();

        // List all
        let files = store.list_files("docs", None).await.unwrap();
        assert_eq!(files.len(), 2);

        // List with prefix
        let files = store.list_files("docs", Some("guide")).await.unwrap();
        assert_eq!(files.len(), 1);
        assert!(files[0].path.starts_with("guide"));
    }

    async fn setup_multi_get_test_store() -> Store {
        let store = Store::open_memory().await.unwrap();
        store
            .add_collection("docs", "/tmp/docs", &["**/*.md"])
            .await
            .unwrap();

        // Add test documents
        store
            .insert_content("hash1", b"# Doc 1\nSmall file", "text/markdown")
            .await
            .unwrap();
        store
            .insert_content("hash2", b"# Doc 2\nAnother small file", "text/markdown")
            .await
            .unwrap();
        store
            .insert_content("hash3", &vec![b'x'; 20000], "text/plain")
            .await
            .unwrap(); // Large file

        store
            .upsert_document("docs", "readme.md", Some("Readme"), "hash1", ".md", "Doc 1")
            .await
            .unwrap();
        store
            .upsert_document("docs", "guide.md", Some("Guide"), "hash2", ".md", "Doc 2")
            .await
            .unwrap();
        store
            .upsert_document("docs", "large.txt", Some("Large"), "hash3", ".txt", "Large")
            .await
            .unwrap();

        store
    }

    #[tokio::test]
    async fn test_multi_get_glob_pattern() {
        let store = setup_multi_get_test_store().await;
        let results = store.multi_get("docs/**/*.md", 10240, None).await.unwrap();

        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.path.ends_with(".md")));
        assert!(results.iter().all(|r| !r.skipped));
    }

    #[tokio::test]
    async fn test_multi_get_comma_list() {
        let store = setup_multi_get_test_store().await;
        let results = store
            .multi_get("docs/readme.md, docs/guide.md", 10240, None)
            .await
            .unwrap();

        assert_eq!(results.len(), 2);
        assert!(results.iter().any(|r| r.path == "docs/readme.md"));
        assert!(results.iter().any(|r| r.path == "docs/guide.md"));
    }

    #[tokio::test]
    async fn test_multi_get_max_bytes_skip() {
        let store = setup_multi_get_test_store().await;
        let results = store.multi_get("docs/**/*", 10240, None).await.unwrap();

        let large = results.iter().find(|r| r.path.contains("large")).unwrap();
        assert!(large.skipped);
        assert!(large.skip_reason.is_some());
        assert!(large.content.is_none());
    }

    #[tokio::test]
    async fn test_multi_get_max_lines_truncation() {
        let store = setup_multi_get_test_store().await;
        let results = store
            .multi_get("docs/readme.md", 10240, Some(1))
            .await
            .unwrap();

        assert_eq!(results.len(), 1);
        let content = results[0].content.as_ref().unwrap();
        assert!(content.contains("[... truncated"));
    }

    #[tokio::test]
    async fn test_multi_get_no_matches() {
        let store = setup_multi_get_test_store().await;
        let results = store
            .multi_get("nonexistent/**/*.xyz", 10240, None)
            .await
            .unwrap();

        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_set_and_get_global_context() {
        let store = Store::open_memory().await.unwrap();
        store
            .set_context(None, "/", "Global context")
            .await
            .unwrap();

        let ctx = store.get_global_context().await.unwrap();
        assert_eq!(ctx, Some("Global context".to_string()));
    }

    #[tokio::test]
    async fn test_set_and_find_collection_context() {
        let store = Store::open_memory().await.unwrap();
        store
            .add_collection("docs", "/tmp/docs", &["**/*.md"])
            .await
            .unwrap();

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
    async fn test_fallback_to_global() {
        let store = Store::open_memory().await.unwrap();
        store
            .add_collection("docs", "/tmp/docs", &["**/*.md"])
            .await
            .unwrap();

        store
            .set_context(None, "/", "Global fallback")
            .await
            .unwrap();

        let ctx = store
            .find_context_for_path("docs", "/any/path.md")
            .await
            .unwrap();
        assert_eq!(ctx, Some("Global fallback".to_string()));
    }

    #[tokio::test]
    async fn test_get_all_contexts() {
        let store = Store::open_memory().await.unwrap();
        store
            .add_collection("docs", "/tmp/docs", &["**/*.md"])
            .await
            .unwrap();

        store.set_context(None, "/", "Global").await.unwrap();
        store.set_context(Some("docs"), "/", "Docs").await.unwrap();
        store
            .set_context(Some("docs"), "/api", "API")
            .await
            .unwrap();

        let contexts = store
            .get_all_contexts_for_path("docs", "/api/file.md")
            .await
            .unwrap();
        assert_eq!(contexts, vec!["Global", "Docs", "API"]);
    }

    #[tokio::test]
    async fn test_remove_context() {
        let store = Store::open_memory().await.unwrap();
        store
            .set_context(Some("docs"), "/api", "API context")
            .await
            .unwrap();

        assert!(store
            .remove_context(Some("docs"), "/api")
            .await
            .unwrap());
        assert!(!store
            .remove_context(Some("docs"), "/api")
            .await
            .unwrap()); // Already removed
    }

    #[tokio::test]
    async fn test_list_contexts() {
        let store = Store::open_memory().await.unwrap();
        store.set_context(None, "/", "Global").await.unwrap();
        store.set_context(Some("docs"), "/", "Docs").await.unwrap();
        store
            .set_context(Some("docs"), "/api", "API")
            .await
            .unwrap();

        let contexts = store.list_contexts().await.unwrap();
        assert_eq!(contexts.len(), 3);

        // First should be global
        assert_eq!(contexts[0].collection, None);
        assert_eq!(contexts[0].path_prefix, "/");
        assert_eq!(contexts[0].context, "Global");
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
        store.set_context(Some("docs"), "/", "Docs").await.unwrap();

        let without = store.get_collections_without_context().await.unwrap();
        assert_eq!(without.len(), 1);
        assert_eq!(without[0].name, "code");
    }
}
