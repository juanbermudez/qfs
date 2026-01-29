# QFS - Quick File Search

An on-device search engine for everything you need to remember. Index your notes, code, documentation, and knowledge bases. Search with keywords or semantic similarity. Ideal for your agentic flows.

QFS combines BM25 full-text search, vector semantic search, and hybrid ranking using Reciprocal Rank Fusion (RRF)—all running locally. Built in Rust for speed with minimal dependencies.

This is a Rust port of [QMD](https://github.com/tobi/qmd) by [Tobi Lütke](https://github.com/tobi). All credit for the original design and implementation goes to him.

## Quick Start

```sh
# Install from source
cargo install --path qfs-cli

# Create collections for your notes, docs, and code
qfs add notes ~/notes --patterns "**/*.md"
qfs add docs ~/Documents --patterns "**/*.md" "**/*.txt"
qfs add code ~/projects --patterns "**/*.rs" "**/*.ts" "**/*.py"

# Generate embeddings for semantic search
qfs index

# Search across everything
qfs search "project timeline"              # Fast keyword search
qfs search "how to deploy" --mode vector   # Semantic search
qfs search "quarterly planning" --mode hybrid  # Hybrid (best quality)

# Get a specific document
qfs get "notes/meeting-2024-01-15.md"

# Search within a specific collection
qfs search "API" -c code
```

### Using with AI Agents

QFS's `--format json` output is designed for agentic workflows:

```sh
# Get structured results for an LLM
qfs search "authentication" --format json -n 10

# List all relevant files above a threshold
qfs search "error handling" --min-score 0.3 --format json

# Retrieve full document content
qfs get "docs/api-reference.md"
```

### MCP Server

QFS exposes an MCP (Model Context Protocol) server for tighter integration with AI agents.

**Tools exposed:**
- `qfs_search` - Fast BM25 keyword search (supports collection filter)
- `qfs_vsearch` - Semantic vector search (supports collection filter)
- `qfs_query` - Hybrid search with RRF fusion (supports collection filter)
- `qfs_get` - Retrieve document by path
- `qfs_multi_get` - Retrieve multiple documents by paths
- `qfs_status` - Index health and collection info

**Claude Desktop configuration** (`~/Library/Application Support/Claude/claude_desktop_config.json`):

```json
{
  "mcpServers": {
    "qfs": {
      "command": "qfs",
      "args": ["serve"]
    }
  }
}
```

**Claude Code configuration** (`~/.claude/settings.json`):

```json
{
  "mcpServers": {
    "qfs": {
      "command": "qfs",
      "args": ["serve"]
    }
  }
}
```

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                         QFS Hybrid Search Pipeline                          │
└─────────────────────────────────────────────────────────────────────────────┘

                              ┌─────────────────┐
                              │   User Query    │
                              └────────┬────────┘
                                       │
              ┌────────────────────────┼────────────────────────┐
              ▼                        │                        ▼
     ┌─────────────────┐               │               ┌─────────────────┐
     │   BM25 Search   │               │               │  Vector Search  │
     │   (SQLite FTS5) │               │               │   (fastembed)   │
     └────────┬────────┘               │               └────────┬────────┘
              │                        │                        │
              │  rank 1: doc_a         │         rank 1: doc_b  │
              │  rank 2: doc_b         │         rank 2: doc_a  │
              │  rank 3: doc_c         │         rank 3: doc_d  │
              │                        │                        │
              └────────────────────────┼────────────────────────┘
                                       │
                                       ▼
                          ┌───────────────────────┐
                          │      RRF Fusion       │
                          │        k=60           │
                          │  1/(k + rank) scores  │
                          └───────────┬───────────┘
                                      │
                                      ▼
                              Final ranking:
                              1. doc_a (0.033)
                              2. doc_b (0.032)
                              3. doc_c (0.016)
                              4. doc_d (0.016)
```

## Score Normalization

### Search Backends

| Backend | Raw Score | Conversion | Range |
|---------|-----------|------------|-------|
| **FTS (BM25)** | SQLite FTS5 BM25 | Normalized to 0-1 | 0.0 to 1.0 |
| **Vector** | Cosine similarity | Native | 0.0 to 1.0 |

### Score Interpretation

| Score | Meaning |
|-------|---------|
| 0.8 - 1.0 | Highly relevant |
| 0.5 - 0.8 | Moderately relevant |
| 0.2 - 0.5 | Somewhat relevant |
| 0.0 - 0.2 | Low relevance |

## Requirements

- Rust 1.70+
- SQLite 3.35+ (bundled)

## Installation

```sh
# From source
git clone https://github.com/yourusername/qfs.git
cd qfs
cargo build --release
cp target/release/qfs /usr/local/bin/

# Or install directly
cargo install --path qfs-cli
```

## Usage

### Collection Management

```sh
# Add a collection with glob patterns
qfs add notes ~/notes --patterns "**/*.md"

# Add with multiple patterns
qfs add code ~/projects --patterns "**/*.rs" "**/*.ts" "**/*.py"

# List all collections
qfs list

# Remove a collection
qfs remove notes
```

### Indexing

```sh
# Index all collections
qfs index

# Index a specific collection
qfs index notes

# Show index status
qfs status
```

### Search Commands

```
┌──────────────────────────────────────────────────────────────────┐
│                        Search Modes                              │
├──────────┬───────────────────────────────────────────────────────┤
│ bm25     │ BM25 full-text search only (default)                  │
│ vector   │ Semantic vector similarity only                       │
│ hybrid   │ BM25 + Vector with RRF fusion                         │
└──────────┴───────────────────────────────────────────────────────┘
```

```sh
# Full-text search (fast, keyword-based)
qfs search "authentication flow"

# Vector search (semantic similarity)
qfs search "how to login" --mode vector

# Hybrid search (best quality)
qfs search "user authentication" --mode hybrid
```

### Options

```sh
# Search options
-n, --limit <num>        # Number of results (default: 20)
-m, --mode <mode>        # bm25, vector, hybrid (default: bm25)
-c, --collection <name>  # Restrict to a collection
--min-score <num>        # Minimum score threshold (default: 0.0)
--include-binary         # Include binary files
-o, --format <format>    # text, json (default: text)
```

### Output Format

Default output is colorized CLI format:

```
notes/meeting.md (score: 0.89)
Title: Q4 Planning
  Discussion about code quality and **craftsmanship**
  in the development process.

docs/guide.md (score: 0.67)
Title: Software Craftsmanship
  This section covers the **craftsmanship** of building
  quality software with attention to detail.
```

JSON output for agents:

```sh
qfs search "craftsmanship" --format json
```

```json
{
  "results": [
    {
      "path": "notes/meeting.md",
      "score": 0.89,
      "title": "Q4 Planning",
      "snippet": "Discussion about code quality and **craftsmanship**..."
    }
  ],
  "total": 1,
  "query": "craftsmanship",
  "mode": "bm25"
}
```

### Index Maintenance

```sh
# Show index status and collections
qfs status

# Re-index all collections
qfs index

# Get document by path
qfs get notes/meeting.md
```

## Data Storage

Index stored in: `~/.cache/qfs/index.sqlite`

### Schema

```sql
collections     -- Indexed directories with name and glob patterns
documents       -- File content with metadata (path, hash, title)
documents_fts   -- FTS5 full-text index
embeddings      -- Vector embeddings for semantic search
```

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `QFS_DB_PATH` | `~/.cache/qfs/index.sqlite` | Database location |
| `QFS_LOG_LEVEL` | `info` | Log level (trace, debug, info, warn, error) |

## Differences from QMD

| Feature | QMD | QFS |
|---------|-----|-----|
| **Language** | TypeScript/Bun | Rust |
| **Runtime** | Node.js + GGUF models | Native binary |
| **Embeddings** | embeddinggemma (300MB) | fastembed |
| **LLM Re-ranking** | Yes | No |
| **Query Expansion** | Yes | No |
| **Binary Size** | ~3GB (with models) | ~15MB |
| **Startup Time** | Slower (model loading) | Instant |

## License

MIT
