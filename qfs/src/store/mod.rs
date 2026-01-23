//! Database store for QFS
//!
//! The store manages the SQLite database containing:
//! - Content (content-addressable storage)
//! - Documents (file metadata)
//! - Collections (indexed directories)
//! - FTS5 index (full-text search)
//! - Embeddings (optional vector storage)

mod schema;

use crate::error::{Error, Result};
use rusqlite::{Connection, params};
use std::path::{Path, PathBuf};
use chrono::Utc;

pub use schema::SCHEMA_VERSION;

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

/// Content stored in content-addressable storage
#[derive(Debug, Clone)]
pub struct Content {
    pub hash: String,
    pub data: Vec<u8>,
    pub content_type: String,
    pub size: i64,
    pub created_at: String,
}

/// The main database store
pub struct Store {
    conn: Connection,
    path: PathBuf,
}

impl Store {
    /// Open or create a database at the given path
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref().to_path_buf();

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(&path)?;

        // Enable WAL mode for better concurrent access
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;

        let mut store = Store { conn, path };
        store.ensure_schema()?;

        Ok(store)
    }

    /// Open an in-memory database (for testing)
    pub fn open_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let mut store = Store {
            conn,
            path: PathBuf::from(":memory:"),
        };
        store.ensure_schema()?;
        Ok(store)
    }

    /// Ensure the database schema is up to date
    fn ensure_schema(&mut self) -> Result<()> {
        schema::ensure_schema(&self.conn)
    }

    /// Get the database path
    pub fn path(&self) -> &Path {
        &self.path
    }

    // -------------------------------------------------------------------------
    // Collection operations
    // -------------------------------------------------------------------------

    /// Add a new collection
    pub fn add_collection(
        &self,
        name: &str,
        path: &str,
        patterns: &[&str],
    ) -> Result<()> {
        self.add_collection_full(name, path, patterns, &[], None, false)
    }

    /// Add a collection with all options
    pub fn add_collection_full(
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

        self.conn.execute(
            "INSERT OR REPLACE INTO collections
             (name, path, patterns, exclude, context, embeddings_enabled, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
            params![name, path, patterns_json, exclude_json, context, embeddings_enabled, now],
        )?;

        Ok(())
    }

    /// Get a collection by name
    pub fn get_collection(&self, name: &str) -> Result<Collection> {
        let mut stmt = self.conn.prepare(
            "SELECT name, path, patterns, exclude, context, embeddings_enabled, created_at, updated_at
             FROM collections WHERE name = ?1"
        )?;

        let collection = stmt.query_row([name], |row| {
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
        }).map_err(|_| Error::CollectionNotFound(name.to_string()))?;

        Ok(collection)
    }

    /// List all collections
    pub fn list_collections(&self) -> Result<Vec<Collection>> {
        let mut stmt = self.conn.prepare(
            "SELECT name, path, patterns, exclude, context, embeddings_enabled, created_at, updated_at
             FROM collections ORDER BY name"
        )?;

        let collections = stmt.query_map([], |row| {
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
        })?.collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(collections)
    }

    /// Remove a collection and its documents
    pub fn remove_collection(&self, name: &str) -> Result<()> {
        // Delete documents first
        self.conn.execute(
            "DELETE FROM documents WHERE collection = ?1",
            [name],
        )?;

        // Delete the collection
        self.conn.execute(
            "DELETE FROM collections WHERE name = ?1",
            [name],
        )?;

        Ok(())
    }

    // -------------------------------------------------------------------------
    // Content operations (content-addressable storage)
    // -------------------------------------------------------------------------

    /// Check if content exists by hash
    pub fn content_exists(&self, hash: &str) -> Result<bool> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM content WHERE hash = ?1",
            [hash],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    /// Insert content (if not exists)
    pub fn insert_content(&self, hash: &str, data: &[u8], content_type: &str) -> Result<()> {
        if self.content_exists(hash)? {
            return Ok(());
        }

        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO content (hash, content, content_type, size, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![hash, data, content_type, data.len() as i64, now],
        )?;

        Ok(())
    }

    /// Get content by hash
    pub fn get_content(&self, hash: &str) -> Result<Content> {
        let mut stmt = self.conn.prepare(
            "SELECT hash, content, content_type, size, created_at FROM content WHERE hash = ?1"
        )?;

        let content = stmt.query_row([hash], |row| {
            Ok(Content {
                hash: row.get(0)?,
                data: row.get(1)?,
                content_type: row.get(2)?,
                size: row.get(3)?,
                created_at: row.get(4)?,
            })
        }).map_err(|_| Error::DocumentNotFound(hash.to_string()))?;

        Ok(content)
    }

    // -------------------------------------------------------------------------
    // Document operations
    // -------------------------------------------------------------------------

    /// Upsert a document
    pub fn upsert_document(
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
        self.conn.execute(
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
        )?;

        // Get the document ID
        let id: i64 = self.conn.query_row(
            "SELECT id FROM documents WHERE collection = ?1 AND path = ?2",
            [collection, path],
            |row| row.get(0),
        )?;

        // Update FTS index (FTS5 doesn't support ON CONFLICT, so delete first)
        self.conn.execute(
            "DELETE FROM documents_fts WHERE rowid = ?1",
            [id],
        )?;
        self.conn.execute(
            "INSERT INTO documents_fts (rowid, filepath, title, body)
             VALUES (?1, ?2, ?3, ?4)",
            params![id, filepath, title.unwrap_or(""), body],
        )?;

        Ok(id)
    }

    /// Get a document by collection and path
    pub fn get_document(&self, collection: &str, path: &str) -> Result<Document> {
        let mut stmt = self.conn.prepare(
            "SELECT id, collection, path, title, hash, file_type, created_at, modified_at, indexed_at, active
             FROM documents WHERE collection = ?1 AND path = ?2 AND active = 1"
        )?;

        let doc = stmt.query_row([collection, path], |row| {
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
        }).map_err(|_| Error::DocumentNotFound(format!("{}/{}", collection, path)))?;

        Ok(doc)
    }

    /// Get a document by ID
    pub fn get_document_by_id(&self, id: i64) -> Result<Document> {
        let mut stmt = self.conn.prepare(
            "SELECT id, collection, path, title, hash, file_type, created_at, modified_at, indexed_at, active
             FROM documents WHERE id = ?1"
        )?;

        let doc = stmt.query_row([id], |row| {
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
        }).map_err(|_| Error::DocumentNotFound(id.to_string()))?;

        Ok(doc)
    }

    /// Mark a document as inactive (soft delete)
    pub fn deactivate_document(&self, collection: &str, path: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE documents SET active = 0 WHERE collection = ?1 AND path = ?2",
            [collection, path],
        )?;
        Ok(())
    }

    /// Count documents in a collection
    pub fn count_documents(&self, collection: Option<&str>) -> Result<i64> {
        let count: i64 = if let Some(coll) = collection {
            self.conn.query_row(
                "SELECT COUNT(*) FROM documents WHERE collection = ?1 AND active = 1",
                [coll],
                |row| row.get(0),
            )?
        } else {
            self.conn.query_row(
                "SELECT COUNT(*) FROM documents WHERE active = 1",
                [],
                |row| row.get(0),
            )?
        };
        Ok(count)
    }

    /// Get database file size in bytes
    pub fn database_size(&self) -> Result<u64> {
        if self.path.to_str() == Some(":memory:") {
            return Ok(0);
        }
        let metadata = std::fs::metadata(&self.path)?;
        Ok(metadata.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_open_memory() {
        let store = Store::open_memory().unwrap();
        assert_eq!(store.path().to_str(), Some(":memory:"));
    }

    #[test]
    fn test_collection_operations() {
        let store = Store::open_memory().unwrap();

        // Add collection
        store.add_collection("test", "/tmp/test", &["**/*.md"]).unwrap();

        // Get collection
        let coll = store.get_collection("test").unwrap();
        assert_eq!(coll.name, "test");
        assert_eq!(coll.path, "/tmp/test");
        assert_eq!(coll.patterns, vec!["**/*.md"]);

        // List collections
        let collections = store.list_collections().unwrap();
        assert_eq!(collections.len(), 1);

        // Remove collection
        store.remove_collection("test").unwrap();
        let collections = store.list_collections().unwrap();
        assert_eq!(collections.len(), 0);
    }

    #[test]
    fn test_content_operations() {
        let store = Store::open_memory().unwrap();

        let hash = "abc123";
        let data = b"Hello, world!";

        // Insert content
        store.insert_content(hash, data, "text/plain").unwrap();

        // Check exists
        assert!(store.content_exists(hash).unwrap());
        assert!(!store.content_exists("nonexistent").unwrap());

        // Get content
        let content = store.get_content(hash).unwrap();
        assert_eq!(content.data, data);
        assert_eq!(content.content_type, "text/plain");
    }

    #[test]
    fn test_document_operations() {
        let store = Store::open_memory().unwrap();

        // Add collection first
        store.add_collection("test", "/tmp/test", &["**/*.md"]).unwrap();

        // Insert content
        store.insert_content("hash123", b"# Hello\n\nWorld", "text/markdown").unwrap();

        // Upsert document
        let id = store.upsert_document(
            "test",
            "hello.md",
            Some("Hello"),
            "hash123",
            ".md",
            "Hello World",
        ).unwrap();

        assert!(id > 0);

        // Get document
        let doc = store.get_document("test", "hello.md").unwrap();
        assert_eq!(doc.title, Some("Hello".to_string()));
        assert_eq!(doc.hash, "hash123");

        // Count documents
        assert_eq!(store.count_documents(Some("test")).unwrap(), 1);
        assert_eq!(store.count_documents(None).unwrap(), 1);

        // Deactivate
        store.deactivate_document("test", "hello.md").unwrap();
        assert_eq!(store.count_documents(Some("test")).unwrap(), 0);
    }
}
