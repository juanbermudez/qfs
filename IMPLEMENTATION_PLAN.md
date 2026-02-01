# QFS Implementation Plan

## Current Task: `qfs embed` Command

### Status: COMPLETE ✅

### Overview

Add CLI support for generating vector embeddings to enable vector and hybrid search modes.

**Completed on: 2026-02-01**

---

### Stage 6: Add Embed Command to CLI ✅

**Goal**: Add `qfs embed` command that generates embeddings for indexed documents

**Files modified**:
- `qfs-cli/Cargo.toml` - Added `qfs-embed` dependency
- `qfs-cli/src/main.rs` - Added Embed command and `cmd_embed` handler
- `qfs/src/store/mod.rs` - Added `list_all_documents()` method

**Success Criteria**:
- [x] `qfs embed` generates embeddings for all documents
- [x] `qfs embed <collection>` generates for specific collection
- [x] `qfs embed --force` re-generates all embeddings
- [x] Progress reporting shows documents processed
- [x] Model download happens automatically on first run

**Status**: Complete ✅

---

### Stage 7: Enable Query Embedding for Search ✅

**Goal**: Make `qfs search --mode vector/hybrid` work by embedding the query

**Files modified**:
- `qfs-cli/src/main.rs` - Updated `cmd_search` to initialize embedder and embed query for vector/hybrid modes

**Success Criteria**:
- [x] `qfs search "query" --mode vector` works
- [x] `qfs search "query" --mode hybrid` works
- [x] Graceful error if no embeddings exist

**Status**: Complete ✅

---

### Stage 8: Add Embedding Stats to Status Command ✅

**Goal**: Show embedding statistics in `qfs status`

**Files modified**:
- `qfs-cli/src/main.rs` - Updated `cmd_status` to show embedding stats

**Success Criteria**:
- [x] Status shows total embeddings count
- [x] Status shows documents with/without embeddings
- [x] Per-collection embedding stats

**Status**: Complete ✅

---

### Key Differences from QMD

| Aspect | QMD | QFS |
|--------|-----|-----|
| Library | node-llama-cpp (GGUF) | fastembed (ONNX) |
| Model | embeddinggemma-300M (768d) | all-MiniLM-L6-v2 (384d) |
| Chunking | Token-based (800 tokens) | Word-based (256 words) |
| Formatting | `title: X \| text: Y` | Plain text |
| Vector DB | sqlite-vec virtual table | libsql F32_BLOB(384) + vector_top_k() |
| Search | Two-step (vec then join) | Native KNN with index |

---

## Completed Tasks (QMD Port)

This plan documents the features from QMD that need to be ported to QFS (the Rust port).

## Task Files

Detailed implementation specifications are in `.tasks/`:

| Task | File | Priority | Complexity |
|------|------|----------|------------|
| Document IDs (docid) | [01-docid-implementation.md](.tasks/01-docid-implementation.md) | High | Low |
| Line Range Extraction | [02-line-range-extraction.md](.tasks/02-line-range-extraction.md) | High | Low |
| Multi-get with Patterns | [03-multi-get-with-patterns.md](.tasks/03-multi-get-with-patterns.md) | High | Medium |
| ls Command | [04-ls-command.md](.tasks/04-ls-command.md) | Medium | Low |
| Context System | [05-context-system.md](.tasks/05-context-system.md) | High | Medium |

## Implementation Order

Recommended order based on dependencies and value:

```
Stage 1: 01-docid-implementation
         └─> Foundational for other features

Stage 2: 02-line-range-extraction
         └─> Builds on get command, no dependencies

Stage 3: 03-multi-get-with-patterns
         └─> Depends on docid (for pattern matching)

Stage 4: 04-ls-command
         └─> Independent, can run in parallel with Stage 3

Stage 5: 05-context-system
         └─> Affects search results, should be last
```

## Quality Gates (All Tasks)

Each task must pass these gates before completion:

### Code Quality
- [ ] `cargo fmt` passes with no changes needed
- [ ] `cargo clippy` passes with no warnings
- [ ] Code follows existing patterns in codebase
- [ ] No unwrap() except in tests

### Testing
- [ ] Unit tests written and passing
- [ ] Integration tests written and passing
- [ ] `cargo test` passes all tests
- [ ] Edge cases documented and tested

### Documentation
- [ ] Public functions have doc comments
- [ ] Complex logic has inline comments
- [ ] CLAUDE.md updated if new commands added

### Review Checklist
- [ ] Error messages are user-friendly
- [ ] JSON output follows existing camelCase convention
- [ ] CLI help text is clear and complete
- [ ] No breaking changes to existing commands

## Current State Comparison

### QFS Has
- Collection management (add, remove, list)
- BM25 search via SQLite FTS5
- Vector search with fastembed
- Hybrid search with RRF (k=60)
- MCP server with 6 tools
- Content-addressable storage
- Incremental indexing

### QMD Features Being Ported
1. **Document IDs** - 6-char hash prefix for quick lookup
2. **Line Range Extraction** - `:linenum` suffix, `--from`, `-l` flags
3. **Multi-get Patterns** - Glob and comma-separated lists
4. **ls Command** - List collections and files
5. **Context System** - Hierarchical path-based descriptions

### Deferred Features (Future Work)
| Feature | Reason |
|---------|--------|
| Query Expansion | Requires LLM integration |
| LLM Re-ranking | Requires LLM integration |
| YAML Configuration | Database storage simpler |
| Fuzzy Path Matching | Nice-to-have, not critical |

## File Change Summary

### Core Library (`qfs/src/`)
- `lib.rs` - Add line extraction utilities
- `store/schema.rs` - Add path_contexts table
- `store/mod.rs` - Add docid, multi-get, context, file listing
- `search/mod.rs` - Add docid, context to SearchResult
- `mcp/tools.rs` - Update all tools with new features

### CLI (`qfs-cli/src/`)
- `main.rs` - Add ls, multi-get commands; update get command; add context subcommand

## Agent Assignment

Each task file is self-contained with:
- QMD reference implementation details
- Current QFS state analysis
- Step-by-step implementation plan
- Code snippets ready to adapt
- Unit test templates
- Success criteria checklist

Agents should:
1. Read the task file completely
2. Verify current codebase state matches assumptions
3. Implement following the step-by-step plan
4. Run tests after each step
5. Mark success criteria as completed
6. Update CLAUDE.md if needed

## Notes

- Each task should be implemented as a separate branch/PR
- Tasks 1-4 are independent and can be parallelized
- Task 5 (Context System) should be done last as it touches search results
- Keep backward compatibility with existing database schema
- Run full test suite after each task completion
