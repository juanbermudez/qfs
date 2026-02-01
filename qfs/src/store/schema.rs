//! Database schema for QFS

use crate::error::Result;
use libsql::Connection;

/// Current schema version
/// v4: Changed embeddings column from BLOB to F32_BLOB(384) for native vector indexing
pub const SCHEMA_VERSION: i64 = 4;

/// SQL to create the database schema
const SCHEMA_SQL: &str = r#"
-- Content-addressable storage (source of truth for document content)
CREATE TABLE IF NOT EXISTS content (
    hash TEXT PRIMARY KEY,
    content BLOB NOT NULL,
    content_type TEXT NOT NULL,
    size INTEGER NOT NULL,
    created_at TEXT NOT NULL
);

-- Documents table (file system metadata)
CREATE TABLE IF NOT EXISTS documents (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    collection TEXT NOT NULL,
    path TEXT NOT NULL,
    title TEXT,
    hash TEXT NOT NULL REFERENCES content(hash),
    file_type TEXT NOT NULL,
    created_at TEXT NOT NULL,
    modified_at TEXT NOT NULL,
    indexed_at TEXT NOT NULL,
    active INTEGER NOT NULL DEFAULT 1,
    UNIQUE(collection, path)
);

-- Indexes for common queries
CREATE INDEX IF NOT EXISTS idx_documents_collection ON documents(collection, active);
CREATE INDEX IF NOT EXISTS idx_documents_hash ON documents(hash);
CREATE INDEX IF NOT EXISTS idx_documents_path ON documents(path, active);

-- Full-text search index using FTS5 with Porter stemmer
CREATE VIRTUAL TABLE IF NOT EXISTS documents_fts USING fts5(
    filepath,
    title,
    body,
    tokenize='porter unicode61'
);

-- Vector embeddings (for optional semantic search)
-- Using libsql native F32_BLOB(384) for vector storage and indexing
-- 384 dimensions matches all-MiniLM-L6-v2 (default model)
CREATE TABLE IF NOT EXISTS embeddings (
    hash TEXT NOT NULL,
    chunk_index INTEGER NOT NULL,
    char_offset INTEGER NOT NULL,
    model TEXT NOT NULL,
    embedding F32_BLOB(384),
    created_at TEXT NOT NULL,
    PRIMARY KEY (hash, chunk_index)
);

-- Collections (indexed directories)
CREATE TABLE IF NOT EXISTS collections (
    name TEXT PRIMARY KEY,
    path TEXT NOT NULL,
    patterns TEXT NOT NULL,
    exclude TEXT,
    context TEXT,
    embeddings_enabled INTEGER DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

-- Index state (for tracking schema version, last index time, etc.)
CREATE TABLE IF NOT EXISTS index_state (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- Path contexts (hierarchical context descriptions for AI agents)
CREATE TABLE IF NOT EXISTS path_contexts (
    id INTEGER PRIMARY KEY,
    collection TEXT,              -- NULL for global context, collection name otherwise
    path_prefix TEXT NOT NULL,    -- Path prefix (e.g., "/", "/guides", "/api/v2")
    context TEXT NOT NULL,        -- Description text
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(collection, path_prefix)
);

CREATE INDEX IF NOT EXISTS idx_path_contexts_collection ON path_contexts(collection);

-- Note: FTS5 doesn't support traditional triggers, so we sync manually
-- in the application code using DELETE + INSERT pattern.
-- This is the recommended approach for FTS5 external content.

-- Note: Vector index for native semantic search (idx_embeddings_vector)
-- is created lazily when first embedding is inserted to allow libsql
-- to detect the vector dimensions. See ensure_vector_index().
"#;

/// Ensure the database schema is up to date
pub async fn ensure_schema(conn: &Connection) -> Result<()> {
    // Check if schema exists
    let mut rows = conn
        .query(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='index_state'",
            (),
        )
        .await?;

    let table_exists: bool = if let Some(row) = rows.next().await? {
        row.get::<i64>(0)? > 0
    } else {
        false
    };

    if !table_exists {
        // Create initial schema
        conn.execute_batch(SCHEMA_SQL).await?;

        // Set schema version
        conn.execute(
            "INSERT INTO index_state (key, value) VALUES ('schema_version', ?1)",
            [SCHEMA_VERSION.to_string()],
        )
        .await?;

        tracing::info!("Created database schema version {}", SCHEMA_VERSION);
    } else {
        // Check schema version
        let mut rows = conn
            .query(
                "SELECT CAST(value AS INTEGER) FROM index_state WHERE key = 'schema_version'",
                (),
            )
            .await?;

        let version: i64 = if let Some(row) = rows.next().await? {
            row.get(0)?
        } else {
            0
        };

        if version < SCHEMA_VERSION {
            migrate(conn, version).await?;
        }
    }

    Ok(())
}

/// Migrate from an older schema version
async fn migrate(conn: &Connection, from_version: i64) -> Result<()> {
    tracing::info!(
        "Migrating database from version {} to {}",
        from_version,
        SCHEMA_VERSION
    );

    // Add migration steps here as schema evolves
    // Version 2: libsql migration (schema compatible, just version bump)
    // Version 3: Vector index support (created lazily, see ensure_vector_index)

    // Update schema version
    conn.execute(
        "UPDATE index_state SET value = ?1 WHERE key = 'schema_version'",
        [SCHEMA_VERSION.to_string()],
    )
    .await?;

    Ok(())
}

/// Ensure the vector index exists for native vector search.
/// This is called lazily when embeddings are present, since libsql
/// needs to detect vector dimensions from existing data.
///
/// Note: Vector index creation may fail if embeddings were stored as raw BLOB
/// rather than using libsql's vector32() function. In that case, the legacy
/// manual cosine similarity search will be used as fallback.
pub async fn ensure_vector_index(conn: &Connection) -> Result<bool> {
    // Check if index already exists
    let mut rows = conn
        .query(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='idx_embeddings_vector'",
            (),
        )
        .await?;

    let exists: bool = if let Some(row) = rows.next().await? {
        row.get::<i64>(0)? > 0
    } else {
        false
    };

    if exists {
        return Ok(false); // Index already exists
    }

    // Check if there are any embeddings to index
    let mut rows = conn.query("SELECT COUNT(*) FROM embeddings", ()).await?;

    let count: i64 = if let Some(row) = rows.next().await? {
        row.get(0)?
    } else {
        0
    };

    if count == 0 {
        return Ok(false); // No embeddings to index
    }

    // Try to create the vector index
    // Uses cosine similarity metric (recommended for text embeddings)
    // This may fail if embeddings were stored as raw BLOB (legacy format)
    let result = conn
        .execute(
            "CREATE INDEX IF NOT EXISTS idx_embeddings_vector
                ON embeddings(libsql_vector_idx(embedding, 'metric=cosine', 'compress_neighbors=float8', 'max_neighbors=32'))",
            (),
        )
        .await;

    match result {
        Ok(_) => {
            tracing::info!(
                "Created vector index for native semantic search ({} embeddings)",
                count
            );
            Ok(true)
        }
        Err(e) => {
            // Index creation failed - likely due to legacy BLOB format
            // This is expected for databases with pre-existing embeddings
            tracing::debug!(
                "Vector index creation skipped (embeddings may be in legacy format): {}",
                e
            );
            Ok(false)
        }
    }
}

/// Check if the vector index exists and is usable
pub async fn has_vector_index(conn: &Connection) -> bool {
    let rows = conn
        .query(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='idx_embeddings_vector'",
            (),
        )
        .await;

    match rows {
        Ok(mut rows) => {
            if let Ok(Some(row)) = rows.next().await {
                row.get::<i64>(0).unwrap_or(0) > 0
            } else {
                false
            }
        }
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libsql::Builder;

    #[tokio::test]
    async fn test_schema_creation() {
        let db = Builder::new_local(":memory:").build().await.unwrap();
        let conn = db.connect().unwrap();
        ensure_schema(&conn).await.unwrap();

        // Verify tables exist
        let mut rows = conn
            .query(
                "SELECT name FROM sqlite_master WHERE type='table' ORDER BY name",
                (),
            )
            .await
            .unwrap();

        let mut tables = Vec::new();
        while let Some(row) = rows.next().await.unwrap() {
            tables.push(row.get::<String>(0).unwrap());
        }

        assert!(tables.contains(&"content".to_string()));
        assert!(tables.contains(&"documents".to_string()));
        assert!(tables.contains(&"collections".to_string()));
        assert!(tables.contains(&"embeddings".to_string()));
        assert!(tables.contains(&"index_state".to_string()));
    }

    #[tokio::test]
    async fn test_schema_version() {
        let db = Builder::new_local(":memory:").build().await.unwrap();
        let conn = db.connect().unwrap();
        ensure_schema(&conn).await.unwrap();

        let mut rows = conn
            .query(
                "SELECT CAST(value AS INTEGER) FROM index_state WHERE key = 'schema_version'",
                (),
            )
            .await
            .unwrap();

        let version: i64 = rows.next().await.unwrap().unwrap().get(0).unwrap();

        assert_eq!(version, SCHEMA_VERSION);
    }

    #[tokio::test]
    async fn test_idempotent_schema() {
        let db = Builder::new_local(":memory:").build().await.unwrap();
        let conn = db.connect().unwrap();

        // Call ensure_schema multiple times
        ensure_schema(&conn).await.unwrap();
        ensure_schema(&conn).await.unwrap();
        ensure_schema(&conn).await.unwrap();

        // Should still work
        let mut rows = conn
            .query(
                "SELECT CAST(value AS INTEGER) FROM index_state WHERE key = 'schema_version'",
                (),
            )
            .await
            .unwrap();

        let version: i64 = rows.next().await.unwrap().unwrap().get(0).unwrap();

        assert_eq!(version, SCHEMA_VERSION);
    }
}
