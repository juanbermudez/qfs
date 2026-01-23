# QFS Integration Guide

This guide covers integrating QFS into various projects and tools.

## MCP Server Usage

QFS provides an MCP (Model Context Protocol) server for AI agent integration.

### Starting the MCP Server

```bash
qfs serve
```

The server uses stdio transport and implements JSON-RPC 2.0.

### Available Tools

| Tool | Description |
|------|-------------|
| `qfs_search` | Full-text BM25 search across indexed documents |
| `qfs_vsearch` | Semantic vector search (requires embeddings) |
| `qfs_query` | Hybrid search combining BM25 and vector |
| `qfs_get` | Get a specific document by path |
| `qfs_multi_get` | Get multiple documents by paths |
| `qfs_status` | Get index status and statistics |

### Tool Schemas

#### qfs_search

```json
{
  "name": "qfs_search",
  "description": "Full-text search across indexed documents using BM25 ranking",
  "inputSchema": {
    "type": "object",
    "properties": {
      "query": { "type": "string", "description": "Search query text" },
      "collection": { "type": "string", "description": "Optional collection filter" },
      "limit": { "type": "integer", "default": 20 }
    },
    "required": ["query"]
  }
}
```

#### qfs_get

```json
{
  "name": "qfs_get",
  "description": "Get a specific document by its path",
  "inputSchema": {
    "type": "object",
    "properties": {
      "path": { "type": "string", "description": "Document path (collection/relative_path)" },
      "include_content": { "type": "boolean", "default": true }
    },
    "required": ["path"]
  }
}
```

## Claude Code Integration

### Adding QFS to Claude Code

1. Build the QFS binary:
   ```bash
   cargo build --release
   ```

2. Add to your Claude Code settings (`~/.claude/settings.json`):
   ```json
   {
     "mcpServers": {
       "qfs": {
         "command": "/path/to/qfs",
         "args": ["--database", "~/.cache/qfs/index.sqlite", "serve"]
       }
     }
   }
   ```

3. Initialize and index your documents:
   ```bash
   qfs init
   qfs add notes ~/Documents/notes --patterns "**/*.md"
   qfs index
   ```

### Plugin Integration

To add QFS to a Claude Code plugin, add to your `plugin.json`:

```json
{
  "mcpServers": {
    "qfs": {
      "command": "${CLAUDE_PLUGIN_ROOT}/binaries/qfs",
      "args": ["serve"],
      "env": {
        "QFS_DB_PATH": "${CLAUDE_PLUGIN_ROOT}/.cache/qfs.sqlite"
      }
    }
  }
}
```

## Hypercraft CLI Integration

To integrate QFS into the Hypercraft CLI:

1. Add QFS as a Cargo dependency:
   ```toml
   [dependencies]
   qfs = { path = "../qfs/qfs" }
   ```

2. Use QFS for search functionality:
   ```rust
   use qfs::{Store, Indexer, SearchOptions, SearchMode};

   let store = Store::open("~/.cache/hypercraft/qfs.sqlite")?;
   let searcher = qfs::search::Searcher::new(&store);

   let results = searcher.search("query", SearchOptions {
       mode: SearchMode::Bm25,
       limit: 20,
       ..Default::default()
   })?;
   ```

## Tauri Desktop App Integration

To bundle QFS as a Tauri sidecar:

### 1. Build QFS for All Platforms

```bash
# macOS (Apple Silicon)
cargo build --release --target aarch64-apple-darwin

# macOS (Intel)
cargo build --release --target x86_64-apple-darwin

# Linux
cargo build --release --target x86_64-unknown-linux-gnu

# Windows
cargo build --release --target x86_64-pc-windows-msvc
```

### 2. Configure Tauri Sidecar

In `tauri.conf.json`:

```json
{
  "bundle": {
    "externalBin": [
      "binaries/qfs"
    ]
  }
}
```

### 3. Copy Binaries

Place binaries in `src-tauri/binaries/`:

```
src-tauri/binaries/
├── qfs-aarch64-apple-darwin
├── qfs-x86_64-apple-darwin
├── qfs-x86_64-unknown-linux-gnu
└── qfs-x86_64-pc-windows-msvc.exe
```

### 4. Use in Rust Backend

```rust
use tauri::api::process::{Command, CommandEvent};

fn start_qfs_server() -> Result<(), String> {
    let (mut rx, child) = Command::new_sidecar("qfs")
        .expect("failed to create QFS sidecar")
        .args(["serve"])
        .spawn()
        .expect("failed to spawn QFS");

    // Handle QFS stdout/stderr
    tauri::async_runtime::spawn(async move {
        while let Some(event) = rx.recv().await {
            match event {
                CommandEvent::Stdout(line) => {
                    // Handle MCP responses
                }
                CommandEvent::Stderr(line) => {
                    println!("QFS: {}", line);
                }
                _ => {}
            }
        }
    });

    Ok(())
}
```

### 5. Frontend Communication

Use Tauri's invoke system to communicate with QFS:

```typescript
import { invoke } from '@tauri-apps/api/tauri';

async function searchDocuments(query: string) {
  return await invoke('qfs_search', { query, limit: 20 });
}
```

## API Reference

### Store

```rust
// Open or create a database
let store = Store::open("path/to/db.sqlite")?;

// Add a collection
store.add_collection("name", "/path/to/dir", &["**/*.md"])?;

// Get collection
let collection = store.get_collection("name")?;

// List all collections
let collections = store.list_collections()?;
```

### Indexer

```rust
let indexer = Indexer::new(&store);

// Index a collection
let stats = indexer.index_collection("name")?;

// Index all collections
let stats = indexer.index_all()?;
```

### Search

```rust
let searcher = qfs::search::Searcher::new(&store);

// BM25 search
let results = searcher.search("query", SearchOptions {
    mode: SearchMode::Bm25,
    limit: 20,
    min_score: 0.0,
    collection: None,
    include_binary: false,
})?;

// Vector search (requires embeddings)
let results = searcher.search_vector_with_embedding(&query_embedding, &options)?;

// Hybrid search (requires embeddings)
let results = searcher.search_hybrid_with_embedding("query", &query_embedding, &options)?;
```

## Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `QFS_DB_PATH` | Database file path | `~/.cache/qfs/index.sqlite` |
| `QFS_LOG_LEVEL` | Log level (trace, debug, info, warn, error) | `info` |
