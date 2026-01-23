# QFS - Quick File Search

A universal local file search engine with hybrid BM25 + vector search, designed for AI agent integration via MCP.

## Features

- **BM25 Full-Text Search** - Fast keyword search using SQLite FTS5 with porter stemming
- **Vector Semantic Search** - Optional embedding-based search using fastembed (Phase 2)
- **Hybrid Search** - Combine BM25 and vector search with Reciprocal Rank Fusion (RRF)
- **MCP Server** - Expose search tools to AI agents via Model Context Protocol (Phase 3)
- **Universal File Support** - Index any text file: code, markdown, JSON, YAML, and more
- **Incremental Indexing** - Only re-index changed files using content hashing
- **Binary File Handling** - Metadata-only indexing with content pointers for binary files

## Installation

```bash
# From source
cargo install --path qfs-cli

# Or build directly
cargo build --release
cp target/release/qfs /usr/local/bin/
```

## Quick Start

```bash
# Initialize database
qfs init

# Add a collection
qfs add notes ~/notes --patterns "**/*.md"
qfs add code ~/projects/myapp --patterns "**/*.rs" "**/*.ts"

# Index documents
qfs index

# Search
qfs search "async rust tokio"
qfs search "authentication" --collection code --limit 10
qfs search "machine learning" --mode hybrid  # (Phase 2)
```

## CLI Commands

| Command | Description |
|---------|-------------|
| `qfs init` | Initialize database |
| `qfs add <name> <path>` | Add a collection |
| `qfs remove <name>` | Remove a collection |
| `qfs list` | List all collections |
| `qfs index [name]` | Index documents |
| `qfs search <query>` | Search documents |
| `qfs get <path>` | Get document by path |
| `qfs status` | Show database stats |
| `qfs serve` | Start MCP server (Phase 3) |

### Search Options

```bash
qfs search "query" [OPTIONS]

Options:
  -m, --mode <MODE>        Search mode: bm25, vector, hybrid [default: bm25]
  -n, --limit <N>          Maximum results [default: 20]
  -c, --collection <NAME>  Filter by collection
  --min-score <SCORE>      Minimum score threshold [default: 0.0]
  --include-binary         Include binary files in results
  -o, --format <FORMAT>    Output format: text, json [default: text]
```

## Library Usage

```rust
use qfs::{Store, Indexer, SearchOptions, SearchMode};

// Open database
let store = Store::open("~/.cache/qfs/index.sqlite")?;

// Add and index a collection
store.add_collection("notes", "~/notes", &["**/*.md"])?;
let indexer = Indexer::new(&store);
indexer.index_collection("notes")?;

// Search
let searcher = qfs::search::Searcher::new(&store);
let results = searcher.search("rust async", SearchOptions {
    mode: SearchMode::Bm25,
    limit: 20,
    ..Default::default()
})?;

for result in results {
    println!("{}: {:.3}", result.path, result.score);
}
```

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                        QFS CLI                           │
│                   (qfs-cli crate)                        │
└────────────────────────┬────────────────────────────────┘
                         │
┌────────────────────────▼────────────────────────────────┐
│                     QFS Core                             │
│                    (qfs crate)                           │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐ │
│  │ Scanner  │  │  Parser  │  │ Indexer  │  │  Search  │ │
│  └──────────┘  └──────────┘  └──────────┘  └──────────┘ │
│                         │                                │
│                    ┌────▼────┐                           │
│                    │  Store  │                           │
│                    └────┬────┘                           │
└─────────────────────────┼───────────────────────────────┘
                          │
┌─────────────────────────▼───────────────────────────────┐
│                  SQLite Database                         │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐   │
│  │  documents   │  │   content    │  │ documents_fts│   │
│  │   (meta)     │  │   (blobs)    │  │    (FTS5)    │   │
│  └──────────────┘  └──────────────┘  └──────────────┘   │
└─────────────────────────────────────────────────────────┘
```

## Search Modes

### BM25 (Default)
Traditional full-text search using SQLite FTS5. Fast and effective for keyword matching.

```bash
qfs search "error handling rust" --mode bm25
```

### Vector (Phase 2)
Semantic search using text embeddings. Finds conceptually similar documents even without exact keyword matches.

```bash
qfs search "how to handle failures" --mode vector
```

### Hybrid (Phase 2)
Combines BM25 and vector search using Reciprocal Rank Fusion (RRF, k=60). Best of both worlds.

```bash
qfs search "async programming patterns" --mode hybrid
```

## MCP Integration (Phase 3)

QFS exposes search tools via the Model Context Protocol for AI agent integration:

```bash
# Start MCP server
qfs serve
```

### Available Tools

| Tool | Description |
|------|-------------|
| `qfs_search` | BM25 keyword search |
| `qfs_vsearch` | Vector semantic search |
| `qfs_query` | Hybrid search with reranking |
| `qfs_get` | Retrieve document by path or ID |
| `qfs_multi_get` | Batch retrieve documents |
| `qfs_status` | Index health and statistics |

## Configuration

QFS uses environment variables for configuration:

| Variable | Description | Default |
|----------|-------------|---------|
| `QFS_DB_PATH` | Database location | `~/.cache/qfs/index.sqlite` |
| `QFS_EMBED_MODEL` | Embedding model | `all-MiniLM-L6-v2` |
| `QFS_EMBED_CACHE` | Model cache dir | `~/.cache/qfs/models` |

## Development

```bash
# Run tests
cargo test

# Run with logging
RUST_LOG=debug cargo run -- search "test query"

# Build release
cargo build --release
```

## Roadmap

- [x] **Phase 1**: Core BM25 search, CLI
- [x] **Phase 2**: Vector embeddings (fastembed), hybrid search
- [x] **Phase 3**: MCP server, AI agent integration
- [ ] **Phase 4**: Hypercraft desktop integration

## License

MIT License - see [LICENSE](LICENSE) for details.

## Credits

Inspired by [QMD](https://github.com/nicobako/qmd) (Quick Markdown Search).
