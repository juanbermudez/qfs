# Task 04: List (ls) Command

## Overview

Implement the `ls` command to list collections and files within collections, with support for path prefixes and virtual path format.

## QMD Reference Implementation

### Three Modes of Operation

1. **`ls`** (no args) - List all collections with file counts
2. **`ls <collection>`** - List all files in a collection
3. **`ls <collection>/<path>`** or `ls qfs://collection/path` - List files under a path prefix

### Collection Listing (qmd.ts:1107-1137)
```typescript
if (!pathArg) {
  const yamlCollections = yamlListCollections();

  for (const coll of collections) {
    console.log(`  ${c.dim}qmd://${c.reset}${c.cyan}${coll.name}/${c.reset}  ${c.dim}(${coll.file_count} files)${c.reset}`);
  }
}
```

### File Listing Query (qmd.ts:1171-1207)
```sql
SELECT d.path, d.title, d.modified_at, LENGTH(ct.doc) as size
FROM documents d
JOIN content ct ON d.hash = ct.hash
WHERE d.collection = ? AND d.path LIKE ? AND d.active = 1
ORDER BY d.path
```

### Output Formatting
- `ls -l` style: size, date, path
- Size: Human-readable (B, KB, MB, GB)
- Date: "Mon DD HH:MM" (recent) or "Mon DD YYYY" (>6 months old)
- Path: Colored with `qfs://collection/` prefix dimmed

### formatBytes (qmd.ts:267-272)
```typescript
function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes/1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes/(1024*1024)).toFixed(1)} MB`;
  return `${(bytes/(1024*1024*1024)).toFixed(1)} GB`;
}
```

### formatLsTime (qmd.ts:1226-1243)
- Shows "Mon DD HH:MM" for files modified in last 6 months
- Shows "Mon DD YYYY" for older files

## Current QFS State

### Existing Code
- `cmd_list` in `qfs-cli/src/main.rs:209-227` lists collections (basic)
- `list_collections` in `qfs/src/store/mod.rs:184-208` returns collection metadata
- No file listing within collections
- No path prefix filtering

### Files to Modify
1. `qfs-cli/src/main.rs` - Add `ls` command (rename/extend List)
2. `qfs/src/store/mod.rs` - Add file listing query

## Implementation Plan

### Step 1: Add File Listing to Store (qfs/src/store/mod.rs)

```rust
/// File entry for ls output
#[derive(Debug, Clone, serde::Serialize)]
pub struct FileEntry {
    pub collection: String,
    pub path: String,
    pub title: Option<String>,
    pub size: i64,
    pub modified_at: String,
}

impl Store {
    /// List files in a collection, optionally filtered by path prefix.
    pub fn list_files(
        &self,
        collection: &str,
        path_prefix: Option<&str>,
    ) -> Result<Vec<FileEntry>> {
        let sql = if path_prefix.is_some() {
            r#"
            SELECT d.collection, d.path, d.title, LENGTH(c.content) as size, d.modified_at
            FROM documents d
            JOIN content c ON c.hash = d.hash
            WHERE d.collection = ?1 AND d.path LIKE ?2 AND d.active = 1
            ORDER BY d.path
            "#
        } else {
            r#"
            SELECT d.collection, d.path, d.title, LENGTH(c.content) as size, d.modified_at
            FROM documents d
            JOIN content c ON c.hash = d.hash
            WHERE d.collection = ?1 AND d.active = 1
            ORDER BY d.path
            "#
        };

        let mut stmt = self.conn.prepare(sql)?;
        let mut entries = Vec::new();

        let mut rows = if let Some(prefix) = path_prefix {
            let pattern = format!("{}%", prefix);
            stmt.query(params![collection, pattern])?
        } else {
            stmt.query(params![collection])?
        };

        while let Some(row) = rows.next()? {
            entries.push(FileEntry {
                collection: row.get(0)?,
                path: row.get(1)?,
                title: row.get(2)?,
                size: row.get(3)?,
                modified_at: row.get(4)?,
            });
        }

        Ok(entries)
    }
}
```

### Step 2: Add Formatting Utilities (qfs-cli/src/main.rs or new formatter module)

```rust
/// Format bytes as human-readable size
fn format_bytes(bytes: i64) -> String {
    const KB: i64 = 1024;
    const MB: i64 = KB * 1024;
    const GB: i64 = MB * 1024;

    if bytes < KB {
        format!("{} B", bytes)
    } else if bytes < MB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else if bytes < GB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    }
}

/// Format timestamp for ls output
/// Shows "Mon DD HH:MM" for recent files, "Mon DD YYYY" for older
fn format_ls_time(timestamp: &str) -> String {
    use chrono::{DateTime, Utc, Local};

    if let Ok(dt) = DateTime::parse_from_rfc3339(timestamp) {
        let local = dt.with_timezone(&Local);
        let now = Local::now();
        let six_months_ago = now - chrono::Duration::days(180);

        if local > six_months_ago {
            local.format("%b %d %H:%M").to_string()
        } else {
            local.format("%b %d  %Y").to_string()
        }
    } else {
        timestamp[..16.min(timestamp.len())].to_string()
    }
}
```

### Step 3: Add CLI Command (qfs-cli/src/main.rs)

```rust
/// List collections or files
Ls {
    /// Optional: collection name or collection/path
    /// Examples: "docs", "docs/guides", "qfs://docs/api"
    #[arg(value_name = "PATH")]
    path: Option<String>,

    /// Output format (text, json)
    #[arg(long, short = 'o', default_value = "text")]
    format: String,
},
```

Handler:

```rust
fn cmd_ls(db_path: &PathBuf, path: Option<&str>, format: &str) -> Result<()> {
    let store = Store::open(db_path)?;

    match path {
        None => {
            // List all collections
            let collections = store.list_collections()?;

            if collections.is_empty() {
                println!("No collections found. Use 'qfs add' to add a collection.");
                return Ok(());
            }

            if format == "json" {
                let data: Vec<_> = collections.iter().map(|c| {
                    let count = store.count_documents(Some(&c.name)).unwrap_or(0);
                    serde_json::json!({
                        "name": c.name,
                        "path": c.path,
                        "documents": count,
                    })
                }).collect();
                println!("{}", serde_json::to_string_pretty(&data)?);
            } else {
                println!("Collections:\n");
                for col in collections {
                    let doc_count = store.count_documents(Some(&col.name)).unwrap_or(0);
                    println!("  qfs://{}/  ({} files)", col.name, doc_count);
                }
            }
        }
        Some(path_arg) => {
            // Parse the path argument
            let (collection_name, path_prefix) = parse_ls_path(path_arg);

            // Verify collection exists
            if store.get_collection(&collection_name).is_err() {
                anyhow::bail!(
                    "Collection not found: {}\nRun 'qfs ls' to see available collections.",
                    collection_name
                );
            }

            let files = store.list_files(&collection_name, path_prefix.as_deref())?;

            if files.is_empty() {
                if let Some(prefix) = path_prefix {
                    println!("No files found under: {}/{}", collection_name, prefix);
                } else {
                    println!("No files in collection: {}", collection_name);
                }
                return Ok(());
            }

            if format == "json" {
                println!("{}", serde_json::to_string_pretty(&files)?);
            } else {
                // Calculate max size width for alignment
                let max_size_width = files.iter()
                    .map(|f| format_bytes(f.size).len())
                    .max()
                    .unwrap_or(0);

                for file in files {
                    let size_str = format_bytes(file.size);
                    let time_str = format_ls_time(&file.modified_at);

                    println!(
                        "{:>width$}  {}  qfs://{}/{}",
                        size_str,
                        time_str,
                        file.collection,
                        file.path,
                        width = max_size_width
                    );
                }
            }
        }
    }

    Ok(())
}

/// Parse ls path argument into (collection, optional_path_prefix)
fn parse_ls_path(path: &str) -> (String, Option<String>) {
    // Handle qfs:// prefix
    let clean = if path.starts_with("qfs://") {
        &path[6..]
    } else if path.starts_with("//") {
        &path[2..]
    } else {
        path
    };

    // Split into collection and path
    if let Some(slash_pos) = clean.find('/') {
        let collection = clean[..slash_pos].to_string();
        let prefix = clean[slash_pos + 1..].to_string();
        if prefix.is_empty() {
            (collection, None)
        } else {
            (collection, Some(prefix))
        }
    } else {
        (clean.to_string(), None)
    }
}
```

## Quality Gates

### Unit Tests

```rust
#[cfg(test)]
mod ls_tests {
    use super::*;

    #[test]
    fn test_parse_ls_path_collection_only() {
        let (coll, prefix) = parse_ls_path("docs");
        assert_eq!(coll, "docs");
        assert_eq!(prefix, None);
    }

    #[test]
    fn test_parse_ls_path_with_prefix() {
        let (coll, prefix) = parse_ls_path("docs/guides");
        assert_eq!(coll, "docs");
        assert_eq!(prefix, Some("guides".to_string()));
    }

    #[test]
    fn test_parse_ls_path_virtual() {
        let (coll, prefix) = parse_ls_path("qfs://docs/api/v2");
        assert_eq!(coll, "docs");
        assert_eq!(prefix, Some("api/v2".to_string()));
    }

    #[test]
    fn test_parse_ls_path_trailing_slash() {
        let (coll, prefix) = parse_ls_path("docs/");
        assert_eq!(coll, "docs");
        assert_eq!(prefix, None);
    }

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(500), "500 B");
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_bytes(1048576), "1.0 MB");
        assert_eq!(format_bytes(1073741824), "1.0 GB");
    }

    #[test]
    fn test_list_files() {
        let store = Store::open_memory().unwrap();
        store.add_collection("docs", "/tmp/docs", &["**/*.md"]).unwrap();
        store.insert_content("hash1", b"content1", "text/plain").unwrap();
        store.insert_content("hash2", b"content2", "text/plain").unwrap();

        store.upsert_document("docs", "readme.md", None, "hash1", ".md", "content").unwrap();
        store.upsert_document("docs", "guide/intro.md", None, "hash2", ".md", "content").unwrap();

        // List all
        let files = store.list_files("docs", None).unwrap();
        assert_eq!(files.len(), 2);

        // List with prefix
        let files = store.list_files("docs", Some("guide")).unwrap();
        assert_eq!(files.len(), 1);
        assert!(files[0].path.starts_with("guide/"));
    }
}
```

## Success Criteria

- [ ] `qfs ls` lists all collections with file counts
- [ ] `qfs ls docs` lists all files in "docs" collection
- [ ] `qfs ls docs/guides` lists files under "guides/" prefix only
- [ ] `qfs ls qfs://docs/api` works with virtual path format
- [ ] Output shows size, date, and full virtual path
- [ ] Size is human-readable (KB, MB, GB)
- [ ] Date shows recent format for <6 months, year format for older
- [ ] JSON output available with `--format json`
- [ ] Non-existent collection shows helpful error
- [ ] All existing tests pass

## Patterns to Follow

- Use existing `list_collections` for collection metadata
- Add new `list_files` method to Store
- Follow existing CLI command patterns
- Use chrono for date formatting (already in dependencies)
- Return empty vec (not error) for no matches

## Edge Cases to Handle

1. **Empty collection**: Show "No files in collection"
2. **Non-existent path prefix**: Show "No files found under"
3. **Non-existent collection**: Error with suggestion to run `qfs ls`
4. **Trailing slash**: `docs/` should be same as `docs`
5. **Double slashes**: `qfs://docs//path` should normalize

## Files Changed

1. `qfs/src/store/mod.rs` - Add FileEntry struct, list_files method
2. `qfs-cli/src/main.rs` - Add Ls command, format utilities, handler
