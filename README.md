# QFS - Quick File Search

An on-device search engine for everything you need to remember. Index your notes, code, documentation, and knowledge bases. Search with keywords or semantic similarity. Ideal for your agentic flows.

QFS combines BM25 full-text search, vector semantic search, and hybrid ranking using Reciprocal Rank Fusion (RRF)—all running locally. Built in Rust for speed with minimal dependencies.

This is a Rust port of [QMD](https://github.com/tobi/qmd) by [Tobi Lutke](https://github.com/tobi). All credit for the original design and implementation goes to him.

## Quick Start

```sh
# Install from source
cargo install --path qfs-cli

# Create collections for your notes, docs, and code
qfs add notes ~/notes --patterns "**/*.md"
qfs add docs ~/Documents --patterns "**/*.md" "**/*.txt"
qfs add code ~/projects --patterns "**/*.rs" "**/*.ts" "**/*.py"

# Add context to help with search results
qfs context add notes "Personal notes and ideas"
qfs context add docs "Work documentation"
qfs context add code "Source code and projects"

# Generate embeddings for semantic search
qfs index

# Search across everything
qfs search "project timeline"              # Fast keyword search
qfs search "how to deploy" --mode vector   # Semantic search
qfs search "quarterly planning" --mode hybrid  # Hybrid (best quality)

# Get a specific document
qfs get "notes/meeting-2024-01-15.md"

# Get a document by docid (shown in search results)
qfs get "#abc123"

# Get multiple documents by glob pattern
qfs multi-get "notes/2025-05*.md"

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

# Get multiple documents for context
qfs multi-get "docs/*.md" --format json
```

### MCP Server

QFS exposes an MCP (Model Context Protocol) server for tighter integration with AI agents.

**Tools exposed:**
- `qfs_search` - Fast BM25 keyword search (supports collection filter)
- `qfs_vsearch` - Semantic vector search (supports collection filter)
- `qfs_query` - Hybrid search with RRF fusion (supports collection filter)
- `qfs_get` - Retrieve document by path or docid (with fuzzy matching suggestions)
- `qfs_multi_get` - Retrieve multiple documents by glob pattern, list, or docids
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

# List files in a collection
qfs ls notes
qfs ls notes/subfolder
```

### Listing Collections and Files

```sh
# List all collections
qfs ls

# List files in a collection
qfs ls notes

# List files with a path prefix
qfs ls notes/2025
qfs ls qfs://notes/api

# JSON output for scripting
qfs ls notes --format json
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

### Context Management

Context adds descriptive metadata to collections and paths, helping search understand your content. Context is shown in search results alongside each document.

```sh
# Add context to a collection
qfs context add notes "Personal notes and ideas"
qfs context add docs/api "API documentation"

# Add global context (applies to all collections)
qfs context add / "Knowledge base for my projects"

# List all contexts
qfs context list

# Check for collections without context
qfs context check

# Remove context
qfs context rm notes/old
```

### Document IDs (docid)

Each document has a unique short ID (docid) - the first 6 characters of its content hash. Docids are shown in search results as `#abc123` and can be used with `get` and `multi-get`:

```sh
# Search returns docid in results
qfs search "query" --format json
# Output includes: {"docid": "abc123", "score": 0.85, "path": "docs/readme.md", ...}

# Get document by docid
qfs get "#abc123"
qfs get abc123              # Leading # is optional

# Docids also work in multi-get comma-separated lists
qfs multi-get "#abc123, #def456"
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

### Get and Multi-Get

```sh
# Get a document by path
qfs get notes/meeting.md

# Get a document by docid (from search results)
qfs get "#abc123"

# Get document starting at line 50
qfs get notes/meeting.md:50

# Get document with line range
qfs get notes/meeting.md --from 50 --lines 100

# Add line numbers to output
qfs get notes/meeting.md --line-numbers

# Get multiple documents by glob pattern
qfs multi-get "notes/2025-05*.md"

# Get multiple documents by comma-separated list (supports docids)
qfs multi-get "doc1.md, doc2.md, #abc123"

# Limit multi-get to files under 20KB
qfs multi-get "docs/*.md" --max-bytes 20480

# Limit lines per file
qfs multi-get "docs/*.md" --max-lines 100

# Output multi-get as JSON for agent processing
qfs multi-get "docs/*.md" --format json
```

### Options

```sh
# Search options
-n, --limit <num>        # Number of results (default: 20)
-m, --mode <mode>        # bm25, vector, hybrid (default: bm25)
-c, --collection <name>  # Restrict to a collection
--min-score <num>        # Minimum score threshold (default: 0.0)
--include-binary         # Include binary files in results
-o, --format <format>    # text, json (default: text)

# Get options
qfs get <path>[:line]    # Get document, optionally starting at line
--from <num>             # Start from line number (1-indexed)
-l, --lines <num>        # Maximum lines to return
--line-numbers           # Add line numbers to output

# Multi-get options
--max-bytes <num>        # Skip files larger than N bytes (default: 10KB)
-l, --max-lines <num>    # Maximum lines per file
-o, --format <format>    # text, json (default: text)
```

### Output Format

Default output is colorized CLI format:

```
docs/guide.md:42 #a1b2c3
Title: Software Craftsmanship
Context: Work documentation
Score: 89%

This section covers the **craftsmanship** of building
quality software with attention to detail.


notes/meeting.md:15 #d4e5f6
Title: Q4 Planning
Context: Personal notes and ideas
Score: 67%

Discussion about code quality and craftsmanship
in the development process.
```

- **Path**: Collection-relative path (e.g., `docs/guide.md`)
- **Docid**: Short hash identifier (e.g., `#a1b2c3`) - use with `qfs get #a1b2c3`
- **Title**: Extracted from document (first heading or filename)
- **Context**: Path context if configured via `qfs context add`
- **Score**: Relevance score (percentage)
- **Snippet**: Context around match with query terms highlighted

JSON output for agents:

```sh
qfs search "craftsmanship" --format json
```

```json
{
  "results": [
    {
      "path": "notes/meeting.md",
      "docid": "d4e5f6",
      "score": 0.89,
      "title": "Q4 Planning",
      "context": "Personal notes and ideas",
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

# Re-index a specific collection
qfs index notes
```

## Data Storage

Index stored in: `~/.cache/qfs/index.sqlite`

### Schema

```sql
collections     -- Indexed directories with name and glob patterns
path_contexts   -- Context descriptions by virtual path (qfs://...)
documents       -- File content with metadata and docid (6-char hash)
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
