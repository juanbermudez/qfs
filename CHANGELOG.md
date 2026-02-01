# Changelog

All notable changes to QFS are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added
- `qfs embed` command for generating document embeddings
- Native libsql vector search using F32_BLOB(384) and `vector_top_k()`
- Query embedding at runtime for `--mode vector` and `--mode hybrid`
- Embedding statistics in `qfs status` output
- Vector index creation via `libsql_vector_idx()` for O(log n) KNN search

### Changed
- Migrated from rusqlite to libsql for async database operations
- Schema version bumped to 4 for F32_BLOB column type
- Vector search uses native libsql indexing instead of in-memory cosine similarity

## [0.1.0] - 2026-02-01

Initial release of QFS, a Rust port of [QMD](https://github.com/tobi/qmd).

### Added

#### Core Features
- **Collection Management**: Add, remove, and list document collections
- **File Indexing**: Incremental indexing with content-addressable storage
- **Multi-format Support**: Markdown, plain text, and code files

#### Search Capabilities
- **BM25 Search**: Full-text search via SQLite FTS5 with Porter stemmer
- **Vector Search**: Semantic similarity using fastembed (all-MiniLM-L6-v2, 384 dimensions)
- **Hybrid Search**: RRF fusion of BM25 + vector results (k=60)
- **Score Normalization**: All scores normalized to 0-1 range

#### QMD Feature Parity
- **Document IDs (docid)**: 6-character content hash for quick lookup (`#abc123`)
- **Line Range Extraction**: `:linenum` suffix, `--from`, `-l` flags
- **Multi-get Patterns**: Glob patterns and comma-separated lists
- **ls Command**: List collections and files with path prefixes
- **Context System**: Hierarchical path-based descriptions for AI agents

#### CLI Commands
- `qfs add <name> <path>` - Add a collection with glob patterns
- `qfs remove <name>` - Remove a collection
- `qfs list` - List all collections
- `qfs ls [path]` - List files in a collection
- `qfs index [name]` - Index collections (builds FTS5 index)
- `qfs embed [name]` - Generate embeddings for vector search
- `qfs search <query>` - Search with BM25, vector, or hybrid mode
- `qfs get <path>` - Get document by path or docid
- `qfs multi-get <pattern>` - Get multiple documents by glob or list
- `qfs context add|list|rm|check` - Manage path contexts
- `qfs status` - Show index health and stats
- `qfs serve` - Start MCP server

#### MCP Server
- `qfs_search` - BM25 keyword search
- `qfs_vsearch` - Semantic vector search
- `qfs_query` - Hybrid search with RRF fusion
- `qfs_get` - Retrieve document by path or docid
- `qfs_multi_get` - Retrieve multiple documents
- `qfs_status` - Index health and collection info

#### Output Formats
- Colorized CLI output with highlighted matches
- JSON output for agent integration (`--format json`)

### Technical Details
- Built with Rust for performance and minimal dependencies
- Uses libsql for async SQLite operations with native vector support
- fastembed for ONNX-based embedding generation
- Content-addressable storage for deduplication
- WAL mode for concurrent access

## Development History

| Date | Commit | Description |
|------|--------|-------------|
| 2026-02-01 | fe53003 | Native libsql vector search and embed command |
| 2026-02-01 | 853b030 | Migrate from rusqlite to libsql async |
| 2026-02-01 | e9aec89 | Port QMD features (docid, line range, multi-get, ls, context) |
| 2026-01-28 | ea5ab36 | Documentation updates with QMD credits |
| 2026-01-23 | 4e3ae43 | Add fastembed, vector search, and MCP server |
| 2026-01-23 | 6e5e991 | Implement BM25 full-text search with FTS5 |
| 2026-01-23 | e039d86 | Complete SQLite schema and storage layer |
| 2026-01-23 | 147ec59 | Initial repository setup |
