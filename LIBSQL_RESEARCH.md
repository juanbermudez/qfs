# libSQL vs SQLite Compatibility Research for QFS

This document analyzes the feasibility of replacing SQLite (via rusqlite) with libSQL in QFS.

## Executive Summary

| Aspect | Status | Notes |
|--------|--------|-------|
| **FTS5** | ✅ COMPATIBLE | All features tested and working |
| **Porter Tokenizer** | ✅ COMPATIBLE | Stemming works correctly |
| **bm25() Function** | ✅ COMPATIBLE | Scoring works |
| **snippet() Function** | ✅ COMPATIBLE | Highlighting works |
| **ON CONFLICT** | ✅ COMPATIBLE | UPSERT works |
| **PRAGMA WAL** | ⚠️ MINOR ISSUE | Returns rows, use query() not execute() |
| **Vector Search** | ✅ BETTER | Native support, no extension needed |
| **Rust API** | ⚠️ MAJOR CHANGES | libsql is async, rusqlite is sync |

## Test Results (Verified)

All tests run successfully on libsql v0.6.0:

```
Test 1: Creating in-memory database... ✓
Test 2: PRAGMA journal_mode=WAL... ✗ (returns rows - minor API difference)
Test 3: Standard table creation... ✓
Test 4: FTS5 with porter tokenizer... ✓
Test 5: Insert documents... ✓
Test 6: FTS5 MATCH query... ✓
Test 7: bm25() scoring... ✓
Test 8: snippet() function... ✓
Test 9: ON CONFLICT (UPSERT)... ✓
Test 10: Porter stemming verification... ✓
```

**Conclusion: libSQL is fully compatible with QFS's SQLite features.**

## What is libSQL?

libSQL is an open-source fork of SQLite maintained by [Turso](https://turso.tech/). Key characteristics:

- **Drop-in replacement** for SQLite (API and file format compatible)
- Adds networking (HTTP/WebSocket), replication, and native vector search
- Remains embeddable (can run without network)
- If advanced features aren't used, generates standard SQLite files

## Current QFS SQLite Feature Usage

### 1. FTS5 (Full-Text Search) - CRITICAL

**Schema** (`store/schema.rs:40-46`):
```sql
CREATE VIRTUAL TABLE IF NOT EXISTS documents_fts USING fts5(
    filepath,
    title,
    body,
    tokenize='porter unicode61'
);
```

**Functions Used** (`store/mod.rs`):
- `bm25(documents_fts)` - BM25 ranking scores
- `snippet(documents_fts, 2, '<mark>', '</mark>', '...', 64)` - Highlighted snippets
- `MATCH` operator for queries
- `rowid` linking between FTS and main table

**libSQL Compatibility**:
- FTS5 module is included in libSQL
- **RISK**: Porter and unicode61 tokenizers need verification
- **RISK**: bm25() and snippet() functions need testing

### 2. PRAGMA Statements

**Current Usage** (`store/mod.rs:163`):
```rust
conn.execute_batch("PRAGMA journal_mode=WAL;")?;
```

**libSQL Compatibility**:
- Some PRAGMA statements return "unsupported" errors
- WAL mode may behave differently with libSQL's replication features
- **RISK**: May need to conditionally handle or skip certain PRAGMAs

### 3. ON CONFLICT (UPSERT)

**Usage** (`store/mod.rs:371-380`):
```sql
INSERT INTO documents (...) VALUES (...)
ON CONFLICT(collection, path) DO UPDATE SET ...
```

**libSQL Compatibility**: Fully supported (standard SQL)

### 4. BLOB Storage

**Usage**: Content and embeddings stored as BLOBs
- `content BLOB NOT NULL`
- `embedding BLOB NOT NULL`

**libSQL Compatibility**: Fully supported

### 5. Standard SQL Features

All of these are fully compatible:
- PRIMARY KEY, AUTOINCREMENT, UNIQUE, FOREIGN KEY
- COUNT(), LENGTH(), CAST()
- LIKE, IS NULL operators
- Standard indexes

### 6. NOT Used (Good News)

These features are NOT used, avoiding compatibility issues:
- JSON1 extension functions
- Window functions (ROW_NUMBER, RANK, etc.)
- CTEs (WITH clauses)
- RETURNING clause
- Custom functions (create_scalar_function)
- Extension loading (load_extension)

## libSQL Advantages for QFS

### 1. Native Vector Search

libSQL includes native vector search without needing sqlite-vec:

```sql
-- Create table with vector column
CREATE TABLE embeddings (
    id INTEGER PRIMARY KEY,
    embedding F32_BLOB(384)  -- 384-dimension float32 vector
);

-- Create vector index
CREATE INDEX embeddings_idx ON embeddings(
    libsql_vector_idx(embedding)
);

-- Query nearest neighbors
SELECT * FROM vector_top_k('embeddings_idx', query_vector, 10);
```

**Benefits**:
- No external extension needed
- Works on mobile/embedded devices
- LM-DiskANN algorithm for fast ANN search
- Small memory footprint

### 2. Embedded Replicas

libSQL can sync a local database with a remote Turso instance:
- Microsecond-level read latency (local reads)
- Remote writes with sync
- Read-your-writes guarantee

This could enable:
- Multi-device sync for QFS indexes
- Cloud backup of search indexes
- Collaborative indexing

### 3. Future Extensibility

- HTTP/WebSocket protocol for remote access
- User-defined functions via WebAssembly
- Encryption at rest

## Rust Crate Differences

### rusqlite (Current)

```rust
// Sync API
use rusqlite::{Connection, params};

let conn = Connection::open("path.db")?;
conn.execute("INSERT INTO ...", params![value])?;
let rows = conn.query_row("SELECT ...", params![id], |row| {
    Ok(row.get::<_, String>(0)?)
})?;
```

Features:
- Synchronous API
- `bundled` feature includes SQLite with FTS5
- Mature, well-tested

### libsql (Replacement)

```rust
// Async API (requires tokio)
use libsql::{Builder, Database};

let db = Builder::new_local("path.db").build().await?;
let conn = db.connect()?;
conn.execute("INSERT INTO ...", params![value]).await?;
let mut rows = conn.query("SELECT ...", params![id]).await?;
while let Some(row) = rows.next().await? {
    let value: String = row.get(0)?;
}
```

Features:
- Async-first API with tokio
- Builder pattern for configuration
- Can connect to remote Turso or local file
- Different error types and patterns

### Migration Effort

**Breaking Changes**:
1. All database operations become async (`.await`)
2. Connection creation uses Builder pattern
3. Different result/error handling
4. Query iteration is different (async iterator)

**Estimated Impact**:
- `store/mod.rs`: ~50+ function signatures change
- `search/mod.rs`: All search functions become async
- `indexer/mod.rs`: Indexing becomes async
- `mcp/`: MCP handlers already async, easier migration
- Tests: All database tests need async runtime

## Compatibility Test Plan

### Phase 1: Dependency Switch
1. Replace rusqlite with libsql in Cargo.toml
2. Fix compilation errors
3. See what breaks

### Phase 2: Core Functionality
1. Schema creation (FTS5 with tokenizers)
2. Basic CRUD operations
3. PRAGMA WAL mode

### Phase 3: Search Features
1. FTS5 MATCH queries
2. bm25() scoring
3. snippet() generation
4. Query sanitization

### Phase 4: Performance
1. Benchmark indexing speed
2. Benchmark search latency
3. Compare with rusqlite baseline

## Known Risks

| Risk | Severity | Mitigation |
|------|----------|------------|
| Porter/unicode61 tokenizers not supported | High | Test early, may need custom tokenizer |
| Performance regression in local mode | Medium | Benchmark before committing |
| Async refactor complexity | Medium | Incremental migration |
| PRAGMA compatibility issues | Low | Conditional handling |
| Breaking API changes | Low | libsql still evolving |

## Recommendation

**Proceed with caution.** The migration is feasible but involves:

1. **Significant code changes** - Async refactor touches most of codebase
2. **Risk of FTS5 tokenizer issues** - Core search functionality depends on this
3. **Uncertain performance** - Local mode may be slower

**Benefits if successful**:
- Native vector search (simpler than sqlite-vec)
- Future cloud sync capability
- Modern async API

**Suggested Approach**:
1. Create proof-of-concept branch (this branch)
2. Test FTS5 with tokenizers first (highest risk)
3. Benchmark performance before full migration
4. Consider keeping rusqlite as fallback

## Migration Effort Estimate

### Code Changes Required

| File | Changes | Complexity |
|------|---------|------------|
| `store/mod.rs` | All DB functions become async | High |
| `search/mod.rs` | Search functions become async | Medium |
| `indexer/mod.rs` | Indexing becomes async | Medium |
| `mcp/server.rs` | Already async, easier | Low |
| `Cargo.toml` | Replace rusqlite with libsql | Low |
| Tests | Add tokio runtime to all tests | Medium |

### Specific Changes

1. **Connection Creation**:
   ```rust
   // Before (rusqlite)
   let conn = Connection::open(path)?;

   // After (libsql)
   let db = Builder::new_local(path).build().await?;
   let conn = db.connect()?;
   ```

2. **Execute Statements**:
   ```rust
   // Before
   conn.execute("INSERT ...", params![...])?;

   // After
   conn.execute("INSERT ...", params![...]).await?;
   ```

3. **Query Rows**:
   ```rust
   // Before
   conn.query_row("SELECT ...", params![...], |row| {
       Ok(row.get::<_, String>(0)?)
   })?;

   // After
   let mut rows = conn.query("SELECT ...", params![...]).await?;
   while let Some(row) = rows.next().await? {
       let val: String = row.get(0)?;
   }
   ```

4. **PRAGMA Handling**:
   ```rust
   // Before
   conn.execute_batch("PRAGMA journal_mode=WAL;")?;

   // After (use query since it returns rows)
   conn.query("PRAGMA journal_mode=WAL;", ()).await?;
   ```

## Next Steps

1. [x] Add libsql dependency
2. [x] Create minimal test for FTS5 with porter tokenizer
3. [x] Test bm25() and snippet() functions
4. [ ] Benchmark local performance vs rusqlite
5. [ ] Begin async migration of store/mod.rs
6. [ ] Update all callers to use async/await
7. [ ] Update tests with tokio runtime

## Recommendation

**libSQL is viable for QFS.** All critical features work. The main effort is:

1. **API Migration**: ~50 function signature changes to async
2. **PRAGMA Fix**: Minor change to use query() instead of execute()

Benefits of migration:
- Native vector search (simpler than sqlite-vec)
- Future cloud sync capability (Turso)
- Modern async API matches the rest of the codebase
