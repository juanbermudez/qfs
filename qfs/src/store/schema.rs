//! Database schema for QFS

use crate::error::Result;
use rusqlite::Connection;

/// Current schema version
pub const SCHEMA_VERSION: i64 = 1;

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
CREATE TABLE IF NOT EXISTS embeddings (
    hash TEXT NOT NULL,
    chunk_index INTEGER NOT NULL,
    char_offset INTEGER NOT NULL,
    model TEXT NOT NULL,
    embedding BLOB NOT NULL,
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
"#;

/// Ensure the database schema is up to date
pub fn ensure_schema(conn: &Connection) -> Result<()> {
    // Check if schema exists
    let table_exists: bool = conn.query_row(
        "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='index_state'",
        [],
        |row| row.get(0),
    )?;

    if !table_exists {
        // Create initial schema
        conn.execute_batch(SCHEMA_SQL)?;

        // Set schema version
        conn.execute(
            "INSERT INTO index_state (key, value) VALUES ('schema_version', ?1)",
            [SCHEMA_VERSION.to_string()],
        )?;

        tracing::info!("Created database schema version {}", SCHEMA_VERSION);
    } else {
        // Check schema version
        let version: i64 = conn
            .query_row(
                "SELECT CAST(value AS INTEGER) FROM index_state WHERE key = 'schema_version'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        if version < SCHEMA_VERSION {
            migrate(conn, version)?;
        }
    }

    Ok(())
}

/// Migrate from an older schema version
fn migrate(conn: &Connection, from_version: i64) -> Result<()> {
    tracing::info!(
        "Migrating database from version {} to {}",
        from_version,
        SCHEMA_VERSION
    );

    // Add migration steps here as schema evolves
    // For now, we only have version 1

    // Update schema version
    conn.execute(
        "UPDATE index_state SET value = ?1 WHERE key = 'schema_version'",
        [SCHEMA_VERSION.to_string()],
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_schema_creation() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_schema(&conn).unwrap();

        // Verify tables exist
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();

        assert!(tables.contains(&"content".to_string()));
        assert!(tables.contains(&"documents".to_string()));
        assert!(tables.contains(&"collections".to_string()));
        assert!(tables.contains(&"embeddings".to_string()));
        assert!(tables.contains(&"index_state".to_string()));
    }

    #[test]
    fn test_schema_version() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_schema(&conn).unwrap();

        let version: i64 = conn
            .query_row(
                "SELECT CAST(value AS INTEGER) FROM index_state WHERE key = 'schema_version'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(version, SCHEMA_VERSION);
    }

    #[test]
    fn test_idempotent_schema() {
        let conn = Connection::open_in_memory().unwrap();

        // Call ensure_schema multiple times
        ensure_schema(&conn).unwrap();
        ensure_schema(&conn).unwrap();
        ensure_schema(&conn).unwrap();

        // Should still work
        let version: i64 = conn
            .query_row(
                "SELECT CAST(value AS INTEGER) FROM index_state WHERE key = 'schema_version'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(version, SCHEMA_VERSION);
    }
}
