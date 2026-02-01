# Task 01: Document ID (docid) Implementation

## Overview

Implement short document IDs (docid) for quick document lookup. A docid is the first 6 characters of the document's SHA256 content hash.

## QMD Reference Implementation

### Hash Generation
- Algorithm: SHA256
- Output: First 6 hex characters of the full hash
- Location in QMD: `store.ts:759-764`, `store.ts:1035-1039`

```typescript
// QMD implementation
export function getDocid(hash: string): string {
  return hash.slice(0, 6);
}
```

### Docid Parsing & Normalization
Accepts multiple input formats:
- `#abc123` (with hash prefix)
- `abc123` (bare hex)
- `"#abc123"`, `'abc123'` (quoted)
- Case-insensitive validation

```typescript
// QMD normalization (store.ts:1343-1374)
export function normalizeDocid(docid: string): string {
  let normalized = docid.trim();
  // Strip quotes
  if ((normalized.startsWith('"') && normalized.endsWith('"')) ||
      (normalized.startsWith("'") && normalized.endsWith("'"))) {
    normalized = normalized.slice(1, -1);
  }
  // Strip leading #
  if (normalized.startsWith('#')) {
    normalized = normalized.slice(1);
  }
  return normalized;
}

export function isDocid(input: string): boolean {
  const normalized = normalizeDocid(input);
  return normalized.length >= 6 && /^[a-f0-9]+$/i.test(normalized);
}
```

### Docid Lookup
Uses SQL `LIKE` with prefix matching:
```sql
SELECT 'qfs://' || d.collection || '/' || d.path as filepath, d.hash
FROM documents d
WHERE d.hash LIKE ? AND d.active = 1
LIMIT 1
```

## Current QFS State

### Existing Code
- Hash stored in `documents.hash` (full SHA256, 64 chars) - `qfs/src/store/mod.rs:27`
- Hash calculated in `indexer/mod.rs:183-188` using `sha2::Sha256`
- No docid extraction or lookup currently implemented

### Files to Modify
1. `qfs/src/store/mod.rs` - Add docid functions
2. `qfs/src/search/mod.rs` - Include docid in SearchResult
3. `qfs-cli/src/main.rs` - Update `get` command to accept docid
4. `qfs/src/mcp/tools.rs` - Update `qfs_get` tool

## Implementation Plan

### Step 1: Add Docid Utilities (qfs/src/store/mod.rs)

Add after line 70:

```rust
/// Extract short docid from a full hash (first 6 characters).
pub fn get_docid(hash: &str) -> &str {
    &hash[..6.min(hash.len())]
}

/// Normalize a docid input by stripping quotes and leading #.
/// Handles: "#abc123", 'abc123', "abc123", #abc123, abc123
pub fn normalize_docid(docid: &str) -> String {
    let mut normalized = docid.trim().to_string();

    // Strip surrounding quotes
    if (normalized.starts_with('"') && normalized.ends_with('"')) ||
       (normalized.starts_with('\'') && normalized.ends_with('\'')) {
        normalized = normalized[1..normalized.len()-1].to_string();
    }

    // Strip leading #
    if normalized.starts_with('#') {
        normalized = normalized[1..].to_string();
    }

    normalized
}

/// Check if a string looks like a docid reference.
/// Returns true if normalized form is valid hex of 6+ chars.
pub fn is_docid(input: &str) -> bool {
    let normalized = normalize_docid(input);
    normalized.len() >= 6 && normalized.chars().all(|c| c.is_ascii_hexdigit())
}
```

### Step 2: Add Document Lookup by Docid (qfs/src/store/mod.rs)

Add to Store impl:

```rust
/// Find a document by its short docid (first 6 characters of hash).
/// Returns the first matching document if found.
pub fn get_document_by_docid(&self, docid: &str) -> Result<Document> {
    let short_hash = normalize_docid(docid);

    if short_hash.len() < 6 {
        return Err(Error::InvalidQuery("Docid must be at least 6 characters".to_string()));
    }

    let pattern = format!("{}%", short_hash);

    let mut stmt = self.conn.prepare(
        "SELECT id, collection, path, title, hash, file_type, created_at, modified_at, indexed_at, active
         FROM documents WHERE hash LIKE ?1 AND active = 1 LIMIT 1"
    )?;

    let doc = stmt.query_row([&pattern], |row| {
        Ok(Document {
            id: row.get(0)?,
            collection: row.get(1)?,
            path: row.get(2)?,
            title: row.get(3)?,
            hash: row.get(4)?,
            file_type: row.get(5)?,
            created_at: row.get(6)?,
            modified_at: row.get(7)?,
            indexed_at: row.get(8)?,
            active: row.get(9)?,
        })
    }).map_err(|_| Error::DocumentNotFound(format!("docid:{}", short_hash)))?;

    Ok(doc)
}
```

### Step 3: Add Docid to SearchResult (qfs/src/search/mod.rs)

Update SearchResult struct (around line 60):

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    // ... existing fields ...

    /// Short document ID (first 6 chars of hash)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub docid: Option<String>,

    // ... rest of fields ...
}
```

Update search result construction in `search_bm25` to include:
```rust
docid: Some(format!("#{}", crate::store::get_docid(&row.hash))),
```

### Step 4: Update CLI Get Command (qfs-cli/src/main.rs)

Update `cmd_get` function to detect and handle docid:

```rust
fn cmd_get(db_path: &PathBuf, path: &str, format: &str) -> Result<()> {
    let store = Store::open(db_path)?;

    // Check if input is a docid
    let doc = if qfs::store::is_docid(path) {
        store.get_document_by_docid(path)?
    } else {
        // Parse path as collection/relative_path
        let parts: Vec<&str> = path.splitn(2, '/').collect();
        if parts.len() != 2 {
            anyhow::bail!("Path must be in format 'collection/relative_path' or docid (#abc123)");
        }
        store.get_document(parts[0], parts[1])?
    };

    // ... rest of function ...
}
```

### Step 5: Update MCP Tool (qfs/src/mcp/tools.rs)

Update `tool_get` to support docid in path parameter.

## Quality Gates

### Tests to Add (qfs/src/store/mod.rs)

```rust
#[cfg(test)]
mod docid_tests {
    use super::*;

    #[test]
    fn test_get_docid() {
        let hash = "abc123def456789012345678901234567890123456789012345678901234";
        assert_eq!(get_docid(hash), "abc123");
    }

    #[test]
    fn test_normalize_docid_with_hash() {
        assert_eq!(normalize_docid("#abc123"), "abc123");
    }

    #[test]
    fn test_normalize_docid_bare() {
        assert_eq!(normalize_docid("abc123"), "abc123");
    }

    #[test]
    fn test_normalize_docid_quoted() {
        assert_eq!(normalize_docid("\"#abc123\""), "abc123");
        assert_eq!(normalize_docid("'abc123'"), "abc123");
    }

    #[test]
    fn test_normalize_docid_whitespace() {
        assert_eq!(normalize_docid("  #abc123  "), "abc123");
    }

    #[test]
    fn test_is_docid_valid() {
        assert!(is_docid("#abc123"));
        assert!(is_docid("abc123"));
        assert!(is_docid("ABC123"));  // Case insensitive
        assert!(is_docid("abc123def456"));  // Longer is ok
    }

    #[test]
    fn test_is_docid_invalid() {
        assert!(!is_docid("abc12"));  // Too short
        assert!(!is_docid("ghijkl"));  // Non-hex
        assert!(!is_docid("abc123.md"));  // Has extension
        assert!(!is_docid("qfs://collection/path"));  // Virtual path
    }

    #[test]
    fn test_get_document_by_docid() {
        let store = Store::open_memory().unwrap();
        store.add_collection("test", "/tmp/test", &["**/*.md"]).unwrap();
        store.insert_content("abc123def456", b"Test content", "text/plain").unwrap();
        store.upsert_document("test", "file.md", Some("Title"), "abc123def456", ".md", "Test").unwrap();

        let doc = store.get_document_by_docid("#abc123").unwrap();
        assert_eq!(doc.path, "file.md");

        let doc = store.get_document_by_docid("abc123").unwrap();
        assert_eq!(doc.path, "file.md");
    }
}
```

### Integration Test (qfs/tests/integration_tests.rs)

Add test for docid in search results and retrieval.

## Success Criteria

- [ ] `qfs get "#abc123"` retrieves document by docid
- [ ] `qfs get "abc123"` works without hash prefix
- [ ] `qfs get "ABC123"` is case-insensitive
- [ ] Search results include `docid` field in JSON output
- [ ] MCP `qfs_get` tool accepts docid format
- [ ] All existing tests pass
- [ ] New unit tests pass
- [ ] Code follows existing patterns (Result types, error handling)

## Patterns to Follow

- Use `Result<T>` with `?` operator for error propagation
- Match existing naming conventions (`get_document`, `get_document_by_docid`)
- Add `#[cfg(test)]` module for unit tests
- Use `rusqlite::params!` macro for SQL parameters
- Follow existing serialization patterns with serde

## Files Changed

1. `qfs/src/store/mod.rs` - Add docid utilities and lookup
2. `qfs/src/search/mod.rs` - Add docid to SearchResult
3. `qfs-cli/src/main.rs` - Update get command
4. `qfs/src/mcp/tools.rs` - Update qfs_get tool
5. `qfs/tests/integration_tests.rs` - Add integration test
