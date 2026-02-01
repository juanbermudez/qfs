# LibSQL Migration Plan for QFS

## Executive Summary

This document provides a complete migration plan from rusqlite to libsql, including async API migration and native vector search integration.

| Aspect | Current | After Migration |
|--------|---------|-----------------|
| Database | rusqlite (sync) | libsql (async) |
| Vector Storage | BLOB + app-level similarity | F32_BLOB + native `vector_top_k()` |
| Vector Search | O(n) full scan in Rust | O(log n) indexed ANN search |
| Embedding Generation | fastembed (384-dim f32) | fastembed (unchanged) |
| FTS5 | porter + unicode61 | ✅ Compatible |

---

## Phase 1: Foundation (Estimated: 2-3 days)

### 1.1 Update Dependencies

**File: `Cargo.toml` (workspace)**

```toml
[workspace.dependencies]
# Remove
# rusqlite = { version = "0.32", features = ["bundled"] }

# Add
libsql = "0.6"
```

**File: `qfs/Cargo.toml`**

```toml
[dependencies]
libsql = { workspace = true }
# Remove rusqlite
```

### 1.2 Update Error Handling

**File: `qfs/src/error.rs`**

Add libsql error conversion:

```rust
#[derive(Debug, thiserror::Error)]
pub enum Error {
    // Existing errors...

    #[error("Database error: {0}")]
    Database(#[from] libsql::Error),
}
```

### 1.3 Migrate Schema Module

**File: `qfs/src/store/schema.rs`**

| Change | Lines | Description |
|--------|-------|-------------|
| Function signature | 96 | `pub async fn ensure_schema(conn: &libsql::Connection)` |
| execute_batch | 105-125 | Add `.await` to all executions |
| query_row | 127-130 | Convert to async iteration |
| migrate function | 134-151 | Add async + `.await` |

**Schema Changes for Native Vectors:**

```sql
-- Before (line 49-57)
CREATE TABLE IF NOT EXISTS embeddings (
    hash TEXT NOT NULL,
    chunk_index INTEGER NOT NULL,
    char_offset INTEGER NOT NULL,
    model TEXT NOT NULL,
    embedding BLOB NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY (hash, chunk_index)
);

-- After
CREATE TABLE IF NOT EXISTS embeddings (
    hash TEXT NOT NULL,
    chunk_index INTEGER NOT NULL,
    char_offset INTEGER NOT NULL,
    model TEXT NOT NULL,
    embedding F32_BLOB(384),  -- Native libsql vector
    created_at TEXT NOT NULL,
    PRIMARY KEY (hash, chunk_index)
);

CREATE INDEX IF NOT EXISTS idx_embeddings_vector
ON embeddings(libsql_vector_idx(embedding, 'metric=cosine'));
```

---

## Phase 2: Store Module Migration (Estimated: 3-4 days)

### 2.1 Connection Management

**File: `qfs/src/store/mod.rs`**

**Before (lines 145-180):**
```rust
pub struct Store {
    conn: Connection,
    path: PathBuf,
}

impl Store {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let conn = Connection::open(&path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        // ...
    }
}
```

**After:**
```rust
use libsql::{Builder, Connection, Database};

pub struct Store {
    db: Database,
    conn: Connection,
    path: PathBuf,
}

impl Store {
    pub async fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let db = Builder::new_local(&path).build().await?;
        let conn = db.connect()?;

        // PRAGMA returns rows in libsql, use query instead of execute
        conn.query("PRAGMA journal_mode=WAL;", ()).await?;

        ensure_schema(&conn).await?;
        Ok(Self { db, conn, path })
    }

    pub async fn open_memory() -> Result<Self> {
        let db = Builder::new_local(":memory:").build().await?;
        let conn = db.connect()?;
        ensure_schema(&conn).await?;
        Ok(Self {
            db,
            conn,
            path: PathBuf::from(":memory:")
        })
    }
}
```

### 2.2 Function Signature Changes

All 35+ public functions need `async`:

| Function | Lines | Await Points |
|----------|-------|--------------|
| `add_collection()` | 197-231 | 4 |
| `get_collection()` | 234-259 | 2 |
| `list_collections()` | 262-287 | 3 |
| `remove_collection()` | 290-300 | 2 |
| `content_exists()` | 307-314 | 1 |
| `insert_content()` | 317-330 | 2 |
| `get_content()` | 333-351 | 2 |
| `upsert_document()` | 358-401 | 5 |
| `get_document()` | 404-428 | 2 |
| `get_document_by_id()` | 431-455 | 2 |
| `get_document_by_docid()` | 459-493 | 2 |
| `deactivate_document()` | 496-509 | 2 |
| `list_documents()` | 512-539 | 3 |
| `count_documents()` | 542-557 | 2 |
| `list_files()` | 560-604 | 3 |
| `set_context()` | 612-637 | 1 |
| `get_global_context()` | 640-651 | 1 |
| `find_context_for_path()` | 655-689 | 3 |
| `get_all_contexts_for_path()` | 693-725 | 3 |
| `list_contexts()` | 728-752 | 3 |
| `remove_context()` | 755-768 | 1 |
| `get_collections_without_context()` | 771-788 | 3 |
| `search_bm25()` | 795-895 | 3 |
| `insert_embedding()` | 911-928 | 1 |
| `get_embeddings()` | 931-952 | 3 |
| `has_embeddings()` | 955-962 | 1 |
| `delete_embeddings()` | 965-969 | 1 |
| `count_embeddings()` | 972-989 | 2 |
| `get_all_embeddings_for_search()` | 993-1057 | 3 |
| `match_files_by_glob()` | 1065-1094 | 3 |
| `parse_comma_list()` | 1098-1140 | 5+ |
| `multi_get()` | 1143-1228 | 5+ |

**Total: ~80 `.await` insertions in store/mod.rs**

### 2.3 Query Pattern Migration

**Before (rusqlite):**
```rust
let result = self.conn.query_row(
    "SELECT id FROM documents WHERE collection = ?1",
    params![collection],
    |row| Ok(row.get::<_, i64>(0)?),
)?;
```

**After (libsql):**
```rust
let mut rows = self.conn.query(
    "SELECT id FROM documents WHERE collection = ?1",
    params![collection],
).await?;

let result = if let Some(row) = rows.next().await? {
    row.get::<i64>(0)?
} else {
    return Err(Error::NotFound);
};
```

### 2.4 Native Vector Search Integration

**New function to replace manual similarity search:**

```rust
/// Vector search using libsql's native vector_top_k()
pub async fn vector_search_native(
    &self,
    query_embedding: &[f32],
    collection: Option<&str>,
    limit: usize,
) -> Result<Vec<VectorSearchResult>> {
    let query_blob = query_embedding.iter()
        .flat_map(|f| f.to_le_bytes())
        .collect::<Vec<u8>>();

    let sql = match collection {
        Some(_) => r#"
            SELECT e.hash, e.chunk_index, e.char_offset,
                   d.id, d.collection, d.path, d.title
            FROM vector_top_k('idx_embeddings_vector', vector32(?), ?) AS v
            JOIN embeddings e ON e.rowid = v.id
            JOIN documents d ON d.hash = e.hash
            WHERE d.collection = ? AND d.active = 1
        "#,
        None => r#"
            SELECT e.hash, e.chunk_index, e.char_offset,
                   d.id, d.collection, d.path, d.title
            FROM vector_top_k('idx_embeddings_vector', vector32(?), ?) AS v
            JOIN embeddings e ON e.rowid = v.id
            JOIN documents d ON d.hash = e.hash
            WHERE d.active = 1
        "#,
    };

    // Execute and collect results...
}
```

---

## Phase 3: Search Module Migration (Estimated: 2 days)

### 3.1 Code to REMOVE

**File: `qfs/src/search/mod.rs`**

| Function | Lines | Reason |
|----------|-------|--------|
| `bytes_to_embedding()` | 442-451 | libsql handles natively |
| `cosine_similarity()` | 453-468 | libsql calculates similarity |
| Related tests | 516-540 | No longer needed |

**Estimated removal: ~50 lines**

### 3.2 Code to CHANGE

**`search_vector()` function (lines 203-291):**

**Before:**
```rust
pub fn search_vector(&self, query: &str, options: SearchOptions) -> Result<Vec<SearchResult>> {
    // Get query embedding
    let query_embedding = self.embedder.embed(query)?;

    // Load ALL embeddings from database
    let embeddings = self.store.get_all_embeddings_for_search(...)?;

    // Manual cosine similarity calculation
    let mut scored: Vec<_> = embeddings.iter().map(|row| {
        let embedding = bytes_to_embedding(&row.embedding);
        let similarity = cosine_similarity(&query_embedding, &embedding);
        (similarity, row)
    }).collect();

    // Sort manually
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());

    // Return top K
    scored.into_iter().take(limit).map(...)
}
```

**After:**
```rust
pub async fn search_vector(&self, query: &str, options: SearchOptions) -> Result<Vec<SearchResult>> {
    // Get query embedding (unchanged)
    let query_embedding = self.embedder.embed(query)?;

    // Single SQL query with native vector search
    let results = self.store.vector_search_native(
        &query_embedding,
        options.collection.as_deref(),
        options.limit,
    ).await?;

    // Map to SearchResult (simplified)
    results.into_iter().map(|r| SearchResult {
        id: r.doc_id,
        docid: get_docid(&r.hash),
        collection: r.collection,
        path: r.path,
        title: r.title,
        score: r.similarity,
        // ...
    }).collect()
}
```

### 3.3 Hybrid Search Update

**`search_hybrid()` maintains RRF logic but uses native vector search:**

```rust
pub async fn search_hybrid(&self, query: &str, options: SearchOptions) -> Result<Vec<SearchResult>> {
    // BM25 results (unchanged, just add .await)
    let bm25_results = self.store.search_bm25(query, ...).await?;

    // Vector results (now uses native search)
    let vector_results = self.search_vector(query, ...).await?;

    // RRF fusion (unchanged logic)
    reciprocal_rank_fusion(bm25_results, vector_results, options.limit)
}
```

---

## Phase 4: Dependent Modules (Estimated: 2 days)

### 4.1 Indexer Module

**File: `qfs/src/indexer/mod.rs`**

| Function | Lines | Changes |
|----------|-------|---------|
| `index_collection()` | 61-62 | Add `async` |
| `index_collection_with_progress()` | 66-115 | Add `async`, 5+ `.await` |
| `index_all()` | 118-133 | Add `async`, 3+ `.await` |
| `index_file()` | 138-180 | Add `async`, 4+ `.await` |

### 4.2 MCP Tools

**File: `qfs/src/mcp/tools.rs`**

| Function | Lines | Changes |
|----------|-------|---------|
| `tool_search()` | 176 | Add `async` |
| `tool_query()` | 205 | Add `async` |
| `tool_get()` | 240 | Add `async`, 3+ `.await` |
| `tool_multi_get()` | 331 | Add `async`, 1+ `.await` |
| `tool_status()` | 358 | Add `async`, 4+ `.await` |

### 4.3 MCP Server

**File: `qfs/src/mcp/server.rs`**

The MCP server already runs async. Changes needed:

| Function | Lines | Changes |
|----------|-------|---------|
| `new()` | 25 | Add `async`, await `Store::open()` |
| `handle_request()` | 95 | Propagate async to handlers |

---

## Phase 5: Test Migration (Estimated: 2-3 days)

### 5.1 Test File Summary

| File | Tests | Helpers | Await Points |
|------|-------|---------|--------------|
| `integration_tests.rs` | 44 | 2 async | ~120 |
| `mcp_tests.rs` | 15 | 1 async | ~40 |
| `golden_tests.rs` | 10 | 2 async | ~30 |
| **Total** | **69** | **5** | **~190** |

### 5.2 Changes Required

**All test functions:**
```rust
// Before
#[test]
fn test_basic_search() {
    let (store, _db, _content) = create_test_store();
    // ...
}

// After
#[tokio::test]
async fn test_basic_search() {
    let (store, _db, _content) = create_test_store().await;
    // ...
}
```

**Helper functions:**
```rust
// Before
fn create_test_store() -> (Store, TempDir, TempDir) {
    let store = Store::open(path).unwrap();
    // ...
}

// After
async fn create_test_store() -> (Store, TempDir, TempDir) {
    let store = Store::open(path).await.unwrap();
    // ...
}
```

### 5.3 New Test Dependencies

```toml
[dev-dependencies]
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

---

## Phase 6: Code Removal Summary

### Code to DELETE (After Migration)

| File | Function/Code | Lines | Reason |
|------|---------------|-------|--------|
| `search/mod.rs` | `bytes_to_embedding()` | ~10 | libsql native |
| `search/mod.rs` | `cosine_similarity()` | ~15 | libsql native |
| `search/mod.rs` | Manual similarity loop | ~40 | Replaced by SQL |
| `search/mod.rs` | Similarity tests | ~25 | No longer needed |
| `qfs-embed/lib.rs` | `embedding_to_bytes()` | ~5 | Optional |
| `qfs-embed/lib.rs` | `bytes_to_embedding()` | ~10 | Optional |
| `qfs-embed/lib.rs` | `cosine_similarity()` | ~15 | Duplicate |
| `store/mod.rs` | `get_all_embeddings_for_search()` | ~65 | Simplified |

**Total removable: ~185 lines**

### Code that STAYS

| Component | Reason |
|-----------|--------|
| `qfs-embed` crate | Still generates embeddings (fastembed) |
| BM25 search logic | FTS5 unchanged |
| RRF hybrid fusion | Still needed for combining results |
| Document/Collection CRUD | Core functionality |
| Context system | Unchanged |

---

## Embedding Compatibility

### Current Setup
- **Model**: all-MiniLM-L6-v2 (384 dimensions)
- **Precision**: f32 (32-bit floats)
- **Storage**: 1,536 bytes per embedding

### libsql Compatibility
- **Type**: F32_BLOB(384) - exact match
- **Precision**: 32-bit floats - no loss
- **Migration**: Direct copy, no regeneration needed

### Migration SQL
```sql
-- Existing embeddings can be migrated directly
-- The BLOB format is compatible with F32_BLOB
ALTER TABLE embeddings ADD COLUMN embedding_new F32_BLOB(384);
UPDATE embeddings SET embedding_new = embedding;
-- Then drop old column and rename
```

---

## Risk Assessment

| Risk | Severity | Mitigation |
|------|----------|------------|
| Async refactor complexity | Medium | Incremental migration by module |
| PRAGMA compatibility | Low | Use `query()` instead of `execute()` |
| Vector index performance | Low | Tested and verified working |
| Test migration effort | Medium | Automate with sed/regex |
| Embedding compatibility | Very Low | f32 → F32_BLOB is exact match |

---

## Migration Order

```
Week 1:
├── Phase 1: Foundation
│   ├── Update Cargo.toml
│   ├── Add error handling
│   └── Migrate schema.rs
│
├── Phase 2: Store (start)
│   ├── Connection management
│   └── Core CRUD operations

Week 2:
├── Phase 2: Store (complete)
│   ├── Search operations
│   ├── Embedding operations
│   └── Multi-get operations
│
├── Phase 3: Search module
│   ├── Remove manual similarity code
│   └── Add native vector search

Week 3:
├── Phase 4: Dependent modules
│   ├── Indexer
│   ├── MCP tools
│   └── MCP server
│
├── Phase 5: Tests
│   ├── integration_tests.rs
│   ├── mcp_tests.rs
│   └── golden_tests.rs

Week 4:
├── Phase 6: Cleanup
│   ├── Remove deprecated code
│   ├── Update documentation
│   └── Performance testing
```

---

## Success Criteria

- [ ] All 169 tests pass with libsql
- [ ] FTS5 search works identically
- [ ] Vector search uses native `vector_top_k()`
- [ ] Hybrid search maintains RRF fusion
- [ ] No precision loss in embeddings
- [ ] Performance equal or better than rusqlite
- [ ] MCP server functions correctly

---

## Appendix: File Change Summary

| File | Action | Complexity |
|------|--------|------------|
| `Cargo.toml` | Update deps | Low |
| `qfs/src/error.rs` | Add libsql error | Low |
| `qfs/src/store/schema.rs` | Async + vector schema | Medium |
| `qfs/src/store/mod.rs` | Major async refactor | High |
| `qfs/src/search/mod.rs` | Async + remove similarity | Medium |
| `qfs/src/indexer/mod.rs` | Async propagation | Medium |
| `qfs/src/mcp/tools.rs` | Async handlers | Medium |
| `qfs/src/mcp/server.rs` | Async init | Low |
| `tests/integration_tests.rs` | tokio::test + await | Medium |
| `tests/mcp_tests.rs` | tokio::test + await | Medium |
| `tests/golden_tests.rs` | tokio::test + await | Medium |
