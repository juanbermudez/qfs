# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

QFS (Quick File Search) is a Rust port of [QMD](https://github.com/tobi/qmd) by Tobi Lütke. It's a direct port with minor changes to support indexing non-markdown files. The original QMD implementation lives in `qmd/` for reference.

QMD/QFS is an on-device search engine combining BM25 full-text search, vector semantic search, and hybrid ranking via Reciprocal Rank Fusion (RRF). All processing runs locally using SQLite FTS5 and fastembed (Rust) or node-llama-cpp (TypeScript).

## Commands

```sh
# Build
cargo build                           # Debug build
cargo build --release                 # Release build

# Run tests
cargo test                            # All tests
cargo test -p qfs                     # Core library tests only
cargo test -p qfs-cli                 # CLI tests only
cargo test --test integration         # Integration tests
cargo test test_search_bm25           # Single test by name

# Run the CLI
cargo run -p qfs-cli -- <command>     # Run from source
cargo install --path qfs-cli          # Install globally as `qfs`

# Linting and formatting
cargo fmt                             # Format code
cargo clippy                          # Lint

# Embedding tests (require model download, run separately)
cargo test -p qfs-embed -- --ignored
```

## Architecture

```
qfs/                   # Workspace root
├── qfs/               # Core library crate
│   └── src/
│       ├── lib.rs     # Re-exports: Store, Indexer, SearchMode, SearchOptions, SearchResult
│       ├── store/     # SQLite database layer (collections, documents, content, embeddings)
│       ├── search/    # Search engine (BM25, vector, hybrid with RRF fusion)
│       ├── indexer/   # File indexing with content-addressable storage
│       ├── scanner/   # Glob-based file discovery
│       ├── parser/    # File parsing and content extraction
│       ├── mcp/       # MCP server (stdio transport, JSON-RPC)
│       └── error.rs   # Error types
├── qfs-cli/           # CLI binary crate (produces `qfs` binary)
├── qfs-embed/         # Optional embedding generation via fastembed
├── qfs-mcp/           # Standalone MCP server binary
└── qmd/               # Original TypeScript implementation (reference only)
```

### Key Components

- **Store** (`qfs/src/store/`): SQLite wrapper managing content-addressable storage, document metadata, FTS5 index, and embeddings table. Uses WAL mode.

- **Searcher** (`qfs/src/search/`): Three search modes:
  - `Bm25`: FTS5 full-text search with score normalization to 0-1 range
  - `Vector`: Cosine similarity on stored embeddings
  - `Hybrid`: RRF fusion of BM25 + vector results (k=60)

- **Indexer** (`qfs/src/indexer/`): Incremental indexing via content hashing. Skips unchanged files.

- **MCP Server** (`qfs/src/mcp/`): Exposes tools `qfs_search`, `qfs_vsearch`, `qfs_query`, `qfs_get`, `qfs_multi_get`, `qfs_status`.

### Data Flow

1. `Scanner` discovers files matching glob patterns
2. `Indexer` hashes content, stores in content-addressable table, updates FTS5 index
3. `Searcher` queries FTS5 (BM25) and/or embeddings table (vector)
4. Results normalized and optionally fused via RRF

## Testing Patterns

- Use `Store::open_memory()` for in-memory database in tests
- Use `tempfile::tempdir()` for filesystem fixtures
- Integration tests in `qfs/tests/integration_tests.rs` demonstrate full search workflows
- Embedding tests are `#[ignore]` by default (require model download)

## Database

- Default path: `~/.cache/qfs/index.sqlite`
- Override with `QFS_DB_PATH` env var or `--database` flag
- Schema: `collections`, `documents`, `documents_fts` (FTS5), `content`, `embeddings`

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `QFS_DB_PATH` | `~/.cache/qfs/index.sqlite` | Database location |
| `QFS_LOG_LEVEL` | `info` | Log level (trace, debug, info, warn, error) |
