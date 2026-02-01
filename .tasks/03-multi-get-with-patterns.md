# Task 03: Multi-Get with Glob Patterns

## Overview

Enhance the multi-get functionality to support glob patterns and comma-separated file lists, with max-bytes filtering for large files.

## QMD Reference Implementation

### Pattern Detection (store.ts:2370)
```typescript
const isCommaSeparated = pattern.includes(',') && !pattern.includes('*') && !pattern.includes('?');
```

### Glob Matching (store.ts:1414-1434)
```typescript
import { Glob } from "bun";

export function matchFilesByGlob(db: Database, pattern: string): { filepath: string; displayPath: string; bodyLength: number }[] {
  const allFiles = db.prepare(`
    SELECT
      'qmd://' || d.collection || '/' || d.path as virtual_path,
      LENGTH(content.doc) as body_length,
      d.path,
      d.collection
    FROM documents d
    JOIN content ON content.hash = d.hash
    WHERE d.active = 1
  `).all();

  const glob = new Glob(pattern);
  return allFiles
    .filter(f => glob.match(f.virtual_path) || glob.match(f.path))
    .map(f => ({
      filepath: f.virtual_path,
      displayPath: f.path,
      bodyLength: f.body_length
    }));
}
```

### Max-Bytes Filtering (store.ts:50, qmd.ts:977-987)
```typescript
export const DEFAULT_MULTI_GET_MAX_BYTES = 10 * 1024; // 10KB

// Skip large files
if (file.bodyLength > maxBytes) {
  results.push({
    file: file.filepath,
    skipped: true,
    skipReason: `File too large (${Math.round(file.bodyLength / 1024)}KB > ${Math.round(maxBytes / 1024)}KB)`,
  });
  continue;
}
```

### Comma-Separated Handling (qmd.ts:873-955)
- Split on commas, trim whitespace
- Support virtual paths, absolute paths, relative paths
- Suffix matching fallback (`LIKE %name`)
- Levenshtein distance suggestions for typos

## Current QFS State

### Existing Code
- `qfs_multi_get` in `qfs/src/mcp/tools.rs:289-335` accepts explicit paths array only
- No CLI `multi-get` command exists
- No glob matching implementation

### Files to Modify
1. `qfs-cli/src/main.rs` - Add `multi-get` command
2. `qfs/src/store/mod.rs` - Add glob matching and file listing
3. `qfs/src/mcp/tools.rs` - Update `qfs_multi_get` to support patterns
4. `Cargo.toml` - Add glob crate if needed (already have `glob = "0.3"`)

## Implementation Plan

### Step 1: Add Pattern Matching Utilities (qfs/src/store/mod.rs)

```rust
use glob::Pattern;

/// Default max bytes for multi-get (10KB)
pub const DEFAULT_MULTI_GET_MAX_BYTES: usize = 10 * 1024;

/// Result from multi-get operation
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MultiGetResult {
    pub path: String,
    pub collection: String,
    pub title: Option<String>,
    pub content: Option<String>,
    pub size: i64,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub skipped: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip_reason: Option<String>,
}

impl Store {
    /// Match files by glob pattern.
    /// Returns files matching the pattern with their sizes (before loading content).
    pub fn match_files_by_glob(&self, pattern: &str) -> Result<Vec<(String, String, i64)>> {
        let glob = Pattern::new(pattern)
            .map_err(|e| Error::InvalidQuery(format!("Invalid glob pattern: {}", e)))?;

        let mut stmt = self.conn.prepare(
            "SELECT d.collection, d.path, LENGTH(c.content) as size
             FROM documents d
             JOIN content c ON c.hash = d.hash
             WHERE d.active = 1"
        )?;

        let mut matches = Vec::new();
        let mut rows = stmt.query([])?;

        while let Some(row) = rows.next()? {
            let collection: String = row.get(0)?;
            let path: String = row.get(1)?;
            let size: i64 = row.get(2)?;

            let full_path = format!("{}/{}", collection, path);
            let virtual_path = format!("qfs://{}/{}", collection, path);

            // Match against both formats
            if glob.matches(&full_path) || glob.matches(&path) || glob.matches(&virtual_path) {
                matches.push((collection, path, size));
            }
        }

        Ok(matches)
    }

    /// Parse comma-separated list of paths.
    /// Returns list of (collection, path) tuples found.
    pub fn parse_comma_list(&self, input: &str) -> Result<Vec<(String, String, i64)>> {
        let names: Vec<&str> = input.split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();

        let mut results = Vec::new();

        for name in names {
            // Try exact match first (collection/path format)
            if let Some((coll, path)) = name.split_once('/') {
                if let Ok(doc) = self.get_document(coll, path) {
                    let content = self.get_content(&doc.hash)?;
                    results.push((doc.collection, doc.path, content.size));
                    continue;
                }
            }

            // Try suffix match
            let mut stmt = self.conn.prepare(
                "SELECT d.collection, d.path, LENGTH(c.content) as size
                 FROM documents d
                 JOIN content c ON c.hash = d.hash
                 WHERE d.path LIKE ?1 AND d.active = 1
                 LIMIT 1"
            )?;

            let pattern = format!("%{}", name);
            if let Ok(row) = stmt.query_row([&pattern], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?))
            }) {
                results.push(row);
            }
            // Note: silently skip if not found (could add error collection)
        }

        Ok(results)
    }

    /// Get multiple documents with size filtering.
    pub fn multi_get(
        &self,
        pattern: &str,
        max_bytes: usize,
        max_lines: Option<usize>,
    ) -> Result<Vec<MultiGetResult>> {
        // Detect pattern type
        let is_glob = pattern.contains('*') || pattern.contains('?');
        let is_comma_list = pattern.contains(',') && !is_glob;

        let files = if is_glob {
            self.match_files_by_glob(pattern)?
        } else if is_comma_list {
            self.parse_comma_list(pattern)?
        } else {
            // Single file
            let parts: Vec<&str> = pattern.splitn(2, '/').collect();
            if parts.len() == 2 {
                if let Ok(doc) = self.get_document(parts[0], parts[1]) {
                    let content = self.get_content(&doc.hash)?;
                    vec![(doc.collection, doc.path, content.size)]
                } else {
                    vec![]
                }
            } else {
                vec![]
            }
        };

        let mut results = Vec::new();

        for (collection, path, size) in files {
            let full_path = format!("{}/{}", collection, path);

            // Check size limit
            if size as usize > max_bytes {
                results.push(MultiGetResult {
                    path: full_path,
                    collection,
                    title: None,
                    content: None,
                    size,
                    skipped: true,
                    skip_reason: Some(format!(
                        "File too large ({}KB > {}KB). Use 'qfs get' to retrieve.",
                        size / 1024,
                        max_bytes / 1024
                    )),
                });
                continue;
            }

            // Get document and content
            if let Ok(doc) = self.get_document(&collection, &path) {
                if let Ok(content) = self.get_content(&doc.hash) {
                    let mut text = String::from_utf8(content.data.clone())
                        .unwrap_or_else(|_| "[Binary content]".to_string());

                    // Apply line limit if specified
                    if let Some(limit) = max_lines {
                        let lines: Vec<&str> = text.lines().take(limit).collect();
                        let original_count = text.lines().count();
                        text = lines.join("\n");
                        if original_count > limit {
                            text.push_str(&format!("\n\n[... truncated {} more lines]", original_count - limit));
                        }
                    }

                    results.push(MultiGetResult {
                        path: full_path,
                        collection: doc.collection,
                        title: doc.title,
                        content: Some(text),
                        size,
                        skipped: false,
                        skip_reason: None,
                    });
                }
            }
        }

        Ok(results)
    }
}
```

### Step 2: Add CLI Command (qfs-cli/src/main.rs)

```rust
/// Get multiple documents by pattern
MultiGet {
    /// Glob pattern or comma-separated list of paths
    /// Examples: "docs/**/*.md", "file1.md, file2.md"
    pattern: String,

    /// Maximum file size in bytes (default: 10KB)
    #[arg(long, default_value = "10240")]
    max_bytes: usize,

    /// Maximum lines per file
    #[arg(short = 'l', long)]
    max_lines: Option<usize>,

    /// Output format (text, json)
    #[arg(long, short = 'o', default_value = "text")]
    format: String,
},
```

Handler:

```rust
fn cmd_multi_get(
    db_path: &PathBuf,
    pattern: &str,
    max_bytes: usize,
    max_lines: Option<usize>,
    format: &str,
) -> Result<()> {
    let store = Store::open(db_path)?;
    let results = store.multi_get(pattern, max_bytes, max_lines)?;

    if results.is_empty() {
        println!("No files matched pattern: {}", pattern);
        return Ok(());
    }

    if format == "json" {
        println!("{}", serde_json::to_string_pretty(&results)?);
    } else {
        for result in results {
            println!("\n{}", "=".repeat(60));
            println!("File: {}", result.path);
            println!("{}", "=".repeat(60));

            if result.skipped {
                println!("[SKIPPED: {}]", result.skip_reason.unwrap_or_default());
            } else if let Some(content) = result.content {
                if let Some(title) = result.title {
                    println!("Title: {}\n", title);
                }
                println!("{}", content);
            }
        }
    }

    Ok(())
}
```

### Step 3: Update MCP Tool (qfs/src/mcp/tools.rs)

Update tool definition:

```rust
ToolDefinition {
    name: "qfs_multi_get".to_string(),
    description: "Get multiple documents by glob pattern or comma-separated list. Skips files larger than maxBytes.".to_string(),
    input_schema: json!({
        "type": "object",
        "properties": {
            "pattern": {
                "type": "string",
                "description": "Glob pattern (e.g., 'docs/**/*.md') or comma-separated paths"
            },
            "max_bytes": {
                "type": "integer",
                "description": "Skip files larger than this (default: 10240 = 10KB)",
                "default": 10240
            },
            "max_lines": {
                "type": "integer",
                "description": "Maximum lines per file"
            }
        },
        "required": ["pattern"]
    }),
},
```

Update handler:

```rust
fn tool_multi_get(store: &Store, args: &Value) -> Result<ToolResult, JsonRpcError> {
    let pattern = args
        .get("pattern")
        .and_then(|v| v.as_str())
        .ok_or_else(|| JsonRpcError::invalid_params("Missing pattern parameter"))?;

    let max_bytes = args
        .get("max_bytes")
        .and_then(|v| v.as_u64())
        .unwrap_or(10240) as usize;

    let max_lines = args
        .get("max_lines")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize);

    let results = store
        .multi_get(pattern, max_bytes, max_lines)
        .map_err(|e| JsonRpcError::server_error(e.to_string()))?;

    let text = serde_json::to_string_pretty(&results)
        .map_err(|e| JsonRpcError::server_error(e.to_string()))?;

    Ok(ToolResult::text(text))
}
```

## Quality Gates

### Unit Tests

```rust
#[cfg(test)]
mod multi_get_tests {
    use super::*;

    fn setup_test_store() -> Store {
        let store = Store::open_memory().unwrap();
        store.add_collection("docs", "/tmp/docs", &["**/*.md"]).unwrap();

        // Add test documents
        store.insert_content("hash1", b"# Doc 1\nSmall file", "text/markdown").unwrap();
        store.insert_content("hash2", b"# Doc 2\nAnother small file", "text/markdown").unwrap();
        store.insert_content("hash3", &vec![b'x'; 20000], "text/plain").unwrap(); // Large file

        store.upsert_document("docs", "readme.md", Some("Readme"), "hash1", ".md", "Doc 1").unwrap();
        store.upsert_document("docs", "guide.md", Some("Guide"), "hash2", ".md", "Doc 2").unwrap();
        store.upsert_document("docs", "large.txt", Some("Large"), "hash3", ".txt", "Large").unwrap();

        store
    }

    #[test]
    fn test_glob_pattern_match() {
        let store = setup_test_store();
        let results = store.multi_get("docs/**/*.md", 10240, None).unwrap();

        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.path.ends_with(".md")));
    }

    #[test]
    fn test_comma_list() {
        let store = setup_test_store();
        let results = store.multi_get("docs/readme.md, docs/guide.md", 10240, None).unwrap();

        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_max_bytes_skip() {
        let store = setup_test_store();
        let results = store.multi_get("docs/**/*", 10240, None).unwrap();

        let large = results.iter().find(|r| r.path.contains("large")).unwrap();
        assert!(large.skipped);
        assert!(large.skip_reason.is_some());
    }

    #[test]
    fn test_max_lines_truncation() {
        let store = setup_test_store();
        let results = store.multi_get("docs/readme.md", 10240, Some(1)).unwrap();

        assert_eq!(results.len(), 1);
        let content = results[0].content.as_ref().unwrap();
        assert!(content.contains("[... truncated"));
    }

    #[test]
    fn test_no_matches() {
        let store = setup_test_store();
        let results = store.multi_get("nonexistent/**/*.xyz", 10240, None).unwrap();

        assert!(results.is_empty());
    }
}
```

## Success Criteria

- [ ] `qfs multi-get "docs/**/*.md"` matches all markdown files in docs
- [ ] `qfs multi-get "file1.md, file2.md"` retrieves comma-separated files
- [ ] `--max-bytes 5000` skips files larger than 5KB with informative message
- [ ] `-l 50` truncates each file to 50 lines
- [ ] Skipped files show reason in output
- [ ] JSON output includes all metadata
- [ ] MCP `qfs_multi_get` supports pattern parameter
- [ ] All existing tests pass

## Patterns to Follow

- Use `glob::Pattern` for glob matching (already in dependencies)
- Return `Vec<MultiGetResult>` with skipped flag for large files
- Handle errors gracefully (missing files logged but don't fail operation)
- Use existing content retrieval patterns from Store
- Match existing JSON serialization style with serde

## Files Changed

1. `qfs/src/store/mod.rs` - Add multi_get, match_files_by_glob, parse_comma_list
2. `qfs-cli/src/main.rs` - Add MultiGet command
3. `qfs/src/mcp/tools.rs` - Update qfs_multi_get tool
