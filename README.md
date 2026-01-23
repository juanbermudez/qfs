# QFS - Quick File Search

An on-device search engine for all your local files. Index your notes, code, documentation, and knowledge bases. Search with keywords or semantic similarity—all running locally.

QFS combines **BM25 full-text search**, **vector semantic search**, and **hybrid ranking** using Reciprocal Rank Fusion (RRF). Built in Rust for speed, powered by SQLite FTS5 and [fastembed](https://github.com/Anush008/fastembed-rs) for embeddings.

## Quick Start

```bash
# Install from source
cargo install --path qfs-cli

# Initialize and index your files
qfs init
qfs add notes ~/notes --patterns "**/*.md"
qfs add code ~/projects --patterns "**/*.rs" "**/*.ts" "**/*.py"
qfs index

# Search
qfs search "async error handling"
```

**Output:**
```
Found 3 results for 'async error handling':

1. code/src/api/client.rs (score: 0.892)
   impl Client {
       pub <mark>async</mark> fn request(&self) -> Result<Response, <mark>Error</mark>> {
           // <mark>Error</mark> <mark>handling</mark> for network requests
       }
   }

2. notes/rust-patterns.md (score: 0.756)
   ## <mark>Error</mark> <mark>Handling</mark> in <mark>Async</mark> Code
   When working with futures, propagate errors using the ? operator...
```

## Search Modes

| Mode | Description | Use Case |
|------|-------------|----------|
| `bm25` | Keyword search via SQLite FTS5 | Fast, exact term matching |
| `vector` | Semantic search via embeddings | Find conceptually similar content |
| `hybrid` | BM25 + vector with RRF fusion | Best of both worlds |

```bash
# Keyword search (default)
qfs search "authentication middleware" --mode bm25

# Semantic search - finds related concepts
qfs search "how to handle user login" --mode vector

# Hybrid - combines both approaches
qfs search "secure session management" --mode hybrid
```

## AI Agent Integration (MCP)

QFS exposes tools via the [Model Context Protocol](https://modelcontextprotocol.io/) for AI agents like Claude Code, Cursor, and others.

```bash
# Start MCP server (stdio transport)
qfs serve
```

### Claude Code Configuration

Add to `~/.claude/settings.json`:

```json
{
  "mcpServers": {
    "qfs": {
      "command": "/path/to/qfs",
      "args": ["serve"]
    }
  }
}
```

### Available Tools

| Tool | Description |
|------|-------------|
| `qfs_search` | BM25 keyword search |
| `qfs_vsearch` | Vector semantic search |
| `qfs_query` | Hybrid search with RRF reranking |
| `qfs_get` | Get document by path |
| `qfs_multi_get` | Batch retrieve multiple documents |
| `qfs_status` | Index statistics and health |

### JSON Output for Agents

```bash
qfs search "error handling" --format json
```

```json
{
  "results": [
    {
      "path": "code/src/error.rs",
      "score": 0.923,
      "title": "Error Types",
      "snippet": "pub enum <mark>Error</mark> { ... }"
    }
  ],
  "total": 1,
  "query": "error handling",
  "mode": "bm25"
}
```

## CLI Reference

| Command | Description |
|---------|-------------|
| `qfs init` | Initialize database |
| `qfs add <name> <path>` | Add a collection with glob patterns |
| `qfs remove <name>` | Remove a collection |
| `qfs list` | List all collections |
| `qfs index [name]` | Index all or specific collection |
| `qfs search <query>` | Search documents |
| `qfs get <path>` | Get document by path |
| `qfs status` | Show database statistics |
| `qfs serve` | Start MCP server |

### Search Options

```bash
qfs search "query" [OPTIONS]

Options:
  -m, --mode <MODE>        bm25, vector, hybrid [default: bm25]
  -n, --limit <N>          Maximum results [default: 20]
  -c, --collection <NAME>  Filter by collection
  --min-score <SCORE>      Minimum score threshold [default: 0.0]
  --include-binary         Include binary files
  -o, --format <FORMAT>    text, json [default: text]
```

## Architecture

```
┌──────────────────────────────────────────────────────────────┐
│                         QFS CLI                               │
└─────────────────────────────┬────────────────────────────────┘
                              │
┌─────────────────────────────▼────────────────────────────────┐
│                        QFS Core                               │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────────┐  │
│  │ Scanner  │  │  Parser  │  │ Indexer  │  │   Searcher   │  │
│  │  (glob)  │  │ (extract)│  │ (hash)   │  │ (BM25+vec)   │  │
│  └──────────┘  └──────────┘  └──────────┘  └──────────────┘  │
│                              │                                │
│                         ┌────▼────┐                           │
│                         │  Store  │                           │
│                         └────┬────┘                           │
└──────────────────────────────┼───────────────────────────────┘
                               │
┌──────────────────────────────▼───────────────────────────────┐
│                      SQLite Database                          │
│  ┌────────────┐  ┌────────────┐  ┌────────────┐  ┌────────┐  │
│  │ documents  │  │  content   │  │    fts5    │  │embeddings│ │
│  │  (meta)    │  │  (blobs)   │  │  (search)  │  │(vectors)│  │
│  └────────────┘  └────────────┘  └────────────┘  └────────┘  │
└──────────────────────────────────────────────────────────────┘
```

### Hybrid Search Pipeline

```
Query: "async error handling"
         │
         ├──────────────────────────────────┐
         │                                  │
         ▼                                  ▼
   ┌──────────┐                      ┌──────────┐
   │  BM25    │                      │  Vector  │
   │  Search  │                      │  Search  │
   └────┬─────┘                      └────┬─────┘
        │                                 │
        │  rank 1: doc_a (0.89)           │  rank 1: doc_b (0.92)
        │  rank 2: doc_b (0.76)           │  rank 2: doc_a (0.88)
        │  rank 3: doc_c (0.65)           │  rank 3: doc_d (0.71)
        │                                 │
        └────────────┬────────────────────┘
                     │
                     ▼
           ┌─────────────────┐
           │  RRF Fusion     │
           │  k=60           │
           └────────┬────────┘
                    │
                    ▼
            Final ranking:
            1. doc_a (0.033)  ← appears in both
            2. doc_b (0.032)  ← appears in both
            3. doc_c (0.016)  ← BM25 only
            4. doc_d (0.016)  ← vector only
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

## Configuration

| Variable | Description | Default |
|----------|-------------|---------|
| `QFS_DB_PATH` | Database location | `~/.cache/qfs/index.sqlite` |
| `QFS_LOG_LEVEL` | Log level (trace, debug, info, warn, error) | `info` |

## Installation

### From Source

```bash
git clone https://github.com/juanbermudez/qfs.git
cd qfs
cargo build --release
cp target/release/qfs /usr/local/bin/
```

### Requirements

- Rust 1.70+
- SQLite 3.35+ (bundled)

## Development

```bash
# Run tests
cargo test

# Run with logging
RUST_LOG=debug cargo run -- search "test"

# Build release
cargo build --release
```

## Roadmap

- [x] Core BM25 search with SQLite FTS5
- [x] Vector embeddings with fastembed
- [x] Hybrid search with RRF fusion
- [x] MCP server for AI agents
- [ ] Watch mode for live reindexing
- [ ] Web UI for browsing results

## License

MIT License - see [LICENSE](LICENSE) for details.

## Credits

Inspired by [QMD](https://github.com/tobi/qmd) (Quick Markdown Search).
