# Task 05: Context System

## Overview

Implement a hierarchical context system that allows users to add descriptive context to collections and paths. Context helps AI agents understand the purpose of different file areas.

## QMD Reference Implementation

### Storage: YAML-Based (Not SQL)
Contexts are stored in `~/.config/qmd/index.yml`, not in the database.

### YAML Structure (collections.ts:17-40)
```yaml
global_context: "If you see a relevant [[WikiWord]], ..."

collections:
  journals:
    path: ~/Documents/Notes
    pattern: "**/*.md"
    context:
      "/": "Notes vault"
      "/journal/2024": "Daily notes from 2024"
      "/journal/2025": "Daily notes from 2025"
```

### Longest-Prefix Matching Algorithm (collections.ts:320-352)
```typescript
export function findContextForPath(collectionName: string, filePath: string): string | undefined {
  const config = loadConfig();
  const collection = config.collections[collectionName];

  if (!collection?.context) {
    return config.global_context;  // Fallback to global
  }

  const matches: Array<{ prefix: string; context: string }> = [];

  for (const [prefix, context] of Object.entries(collection.context)) {
    const normalizedPath = filePath.startsWith("/") ? filePath : `/${filePath}`;
    const normalizedPrefix = prefix.startsWith("/") ? prefix : `/${prefix}`;

    if (normalizedPath.startsWith(normalizedPrefix)) {
      matches.push({ prefix: normalizedPrefix, context });
    }
  }

  // Return most specific match (longest prefix)
  if (matches.length > 0) {
    matches.sort((a, b) => b.prefix.length - a.prefix.length);
    return matches[0]!.context;
  }

  return config.global_context;
}
```

### Context in Search Results (store.ts:1893)
All matching contexts (global + collection) are joined with `\n\n` and included.

### CLI Commands
- `qmd context add [path] "description"` - Add context
- `qmd context list` - List all contexts
- `qmd context check` - Find collections/paths without context
- `qmd context rm <path>` - Remove context

## Current QFS State

### Existing Code
- Collections stored in database via `collections` table
- No context field in collection schema
- No YAML configuration file support

### Design Decision
For simplicity and to match QFS patterns, we'll store contexts in the SQLite database rather than YAML. This avoids adding YAML config complexity while providing the same functionality.

### Files to Modify
1. `qfs/src/store/schema.rs` - Add path_contexts table
2. `qfs/src/store/mod.rs` - Add context CRUD operations
3. `qfs/src/search/mod.rs` - Include context in search results
4. `qfs-cli/src/main.rs` - Add context subcommands
5. `qfs/src/mcp/tools.rs` - Update search results

## Implementation Plan

### Step 1: Update Database Schema (qfs/src/store/schema.rs)

Add to schema creation:

```sql
CREATE TABLE IF NOT EXISTS path_contexts (
    id INTEGER PRIMARY KEY,
    collection TEXT,              -- NULL for global context, collection name otherwise
    path_prefix TEXT NOT NULL,    -- Path prefix (e.g., "/", "/guides", "/api/v2")
    context TEXT NOT NULL,        -- Description text
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(collection, path_prefix)
);

CREATE INDEX IF NOT EXISTS idx_path_contexts_collection ON path_contexts(collection);
```

### Step 2: Add Context Operations to Store (qfs/src/store/mod.rs)

```rust
/// Context entry
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PathContext {
    pub id: i64,
    pub collection: Option<String>,  // None = global
    pub path_prefix: String,
    pub context: String,
    pub created_at: String,
    pub updated_at: String,
}

impl Store {
    // -------------------------------------------------------------------------
    // Context operations
    // -------------------------------------------------------------------------

    /// Add or update a context for a path prefix.
    /// Use collection=None for global context.
    pub fn set_context(
        &self,
        collection: Option<&str>,
        path_prefix: &str,
        context: &str,
    ) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();

        // Normalize path prefix to start with /
        let normalized = if path_prefix.starts_with('/') {
            path_prefix.to_string()
        } else {
            format!("/{}", path_prefix)
        };

        self.conn.execute(
            "INSERT INTO path_contexts (collection, path_prefix, context, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?4)
             ON CONFLICT(collection, path_prefix) DO UPDATE SET
               context = excluded.context,
               updated_at = excluded.updated_at",
            params![collection, normalized, context, now],
        )?;

        Ok(())
    }

    /// Get the global context.
    pub fn get_global_context(&self) -> Result<Option<String>> {
        let result: Option<String> = self.conn.query_row(
            "SELECT context FROM path_contexts WHERE collection IS NULL AND path_prefix = '/'",
            [],
            |row| row.get(0),
        ).ok();

        Ok(result)
    }

    /// Find the most specific context for a file path.
    /// Uses longest-prefix matching.
    pub fn find_context_for_path(
        &self,
        collection: &str,
        file_path: &str,
    ) -> Result<Option<String>> {
        // Normalize file path
        let normalized = if file_path.starts_with('/') {
            file_path.to_string()
        } else {
            format!("/{}", file_path)
        };

        // Get all matching contexts for this collection (sorted by prefix length desc)
        let mut stmt = self.conn.prepare(
            "SELECT path_prefix, context FROM path_contexts
             WHERE (collection = ?1 OR collection IS NULL)
             ORDER BY
               CASE WHEN collection IS NOT NULL THEN 0 ELSE 1 END,
               LENGTH(path_prefix) DESC"
        )?;

        let mut rows = stmt.query([collection])?;

        while let Some(row) = rows.next()? {
            let prefix: String = row.get(0)?;
            let context: String = row.get(1)?;

            // Check if file path starts with this prefix
            if normalized.starts_with(&prefix) || normalized == prefix.trim_end_matches('/') {
                return Ok(Some(context));
            }
        }

        Ok(None)
    }

    /// Get all contexts for a file path (global + collection, ordered general to specific).
    /// Used for search results where we want all relevant context.
    pub fn get_all_contexts_for_path(
        &self,
        collection: &str,
        file_path: &str,
    ) -> Result<Vec<String>> {
        let normalized = if file_path.starts_with('/') {
            file_path.to_string()
        } else {
            format!("/{}", file_path)
        };

        let mut stmt = self.conn.prepare(
            "SELECT path_prefix, context, collection FROM path_contexts
             WHERE (collection = ?1 OR collection IS NULL)
             ORDER BY
               CASE WHEN collection IS NULL THEN 0 ELSE 1 END,
               LENGTH(path_prefix) ASC"
        )?;

        let mut contexts = Vec::new();
        let mut rows = stmt.query([collection])?;

        while let Some(row) = rows.next()? {
            let prefix: String = row.get(0)?;
            let context: String = row.get(1)?;

            if normalized.starts_with(&prefix) || prefix == "/" {
                contexts.push(context);
            }
        }

        Ok(contexts)
    }

    /// List all contexts.
    pub fn list_contexts(&self) -> Result<Vec<PathContext>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, collection, path_prefix, context, created_at, updated_at
             FROM path_contexts
             ORDER BY
               CASE WHEN collection IS NULL THEN '' ELSE collection END,
               path_prefix"
        )?;

        let mut contexts = Vec::new();
        let mut rows = stmt.query([])?;

        while let Some(row) = rows.next()? {
            contexts.push(PathContext {
                id: row.get(0)?,
                collection: row.get(1)?,
                path_prefix: row.get(2)?,
                context: row.get(3)?,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
            });
        }

        Ok(contexts)
    }

    /// Remove a context.
    pub fn remove_context(&self, collection: Option<&str>, path_prefix: &str) -> Result<bool> {
        let normalized = if path_prefix.starts_with('/') {
            path_prefix.to_string()
        } else {
            format!("/{}", path_prefix)
        };

        let rows = self.conn.execute(
            "DELETE FROM path_contexts WHERE collection IS ?1 AND path_prefix = ?2",
            params![collection, normalized],
        )?;

        Ok(rows > 0)
    }

    /// Get collections without any context defined.
    pub fn get_collections_without_context(&self) -> Result<Vec<Collection>> {
        let all_collections = self.list_collections()?;

        let mut without_context = Vec::new();
        for coll in all_collections {
            let has_context: i64 = self.conn.query_row(
                "SELECT COUNT(*) FROM path_contexts WHERE collection = ?1",
                [&coll.name],
                |row| row.get(0),
            )?;

            if has_context == 0 {
                without_context.push(coll);
            }
        }

        Ok(without_context)
    }
}
```

### Step 3: Include Context in Search Results (qfs/src/search/mod.rs)

Update SearchResult:

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    // ... existing fields ...

    /// Context description for this document's location
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
}
```

Update search methods to include context:

```rust
// In search_bm25, after building the result:
let context = self.store.get_all_contexts_for_path(&row.collection, &row.path)
    .ok()
    .map(|contexts| contexts.join("\n\n"))
    .filter(|s| !s.is_empty());
```

### Step 4: Add CLI Commands (qfs-cli/src/main.rs)

```rust
/// Manage context descriptions for collections and paths
Context {
    #[command(subcommand)]
    action: ContextAction,
},

#[derive(Subcommand)]
enum ContextAction {
    /// Add context for a path
    Add {
        /// Path (use "/" for global, "collection" for collection root, "collection/path" for specific path)
        #[arg(default_value = "/")]
        path: String,

        /// Context description
        description: String,
    },

    /// List all contexts
    List,

    /// Check for collections/paths without context
    Check,

    /// Remove a context
    Rm {
        /// Path to remove context from
        path: String,
    },
}
```

Handler:

```rust
fn cmd_context(db_path: &PathBuf, action: ContextAction) -> Result<()> {
    let store = Store::open(db_path)?;

    match action {
        ContextAction::Add { path, description } => {
            let (collection, path_prefix) = parse_context_path(&path);
            store.set_context(collection.as_deref(), &path_prefix, &description)?;

            if let Some(coll) = collection {
                println!("Added context for {}/{}", coll, path_prefix);
            } else {
                println!("Added global context");
            }
        }

        ContextAction::List => {
            let contexts = store.list_contexts()?;

            if contexts.is_empty() {
                println!("No contexts defined. Use 'qfs context add' to add context.");
                return Ok(());
            }

            println!("Contexts:\n");

            // Group by collection
            let mut current_collection: Option<Option<String>> = None;
            for ctx in contexts {
                if current_collection != Some(ctx.collection.clone()) {
                    current_collection = Some(ctx.collection.clone());
                    match &ctx.collection {
                        Some(coll) => println!("\n  Collection: {}", coll),
                        None => println!("\n  Global:"),
                    }
                }

                println!("    {} -> {}", ctx.path_prefix, ctx.context);
            }
        }

        ContextAction::Check => {
            let without_context = store.get_collections_without_context()?;

            if without_context.is_empty() {
                println!("All collections have context defined.");
            } else {
                println!("Collections without context:\n");
                for coll in without_context {
                    let doc_count = store.count_documents(Some(&coll.name)).unwrap_or(0);
                    println!("  {} ({} files)", coll.name, doc_count);
                    println!("    Suggested: qfs context add {} \"Description here\"", coll.name);
                }
            }
        }

        ContextAction::Rm { path } => {
            let (collection, path_prefix) = parse_context_path(&path);
            if store.remove_context(collection.as_deref(), &path_prefix)? {
                println!("Removed context for {}", path);
            } else {
                println!("No context found for {}", path);
            }
        }
    }

    Ok(())
}

/// Parse context path into (collection, path_prefix)
/// "/" -> (None, "/")
/// "collection" -> (Some("collection"), "/")
/// "collection/path" -> (Some("collection"), "/path")
/// "qfs://collection/path" -> (Some("collection"), "/path")
fn parse_context_path(path: &str) -> (Option<String>, String) {
    if path == "/" {
        return (None, "/".to_string());
    }

    // Handle qfs:// prefix
    let clean = if path.starts_with("qfs://") {
        &path[6..]
    } else {
        path
    };

    if let Some(slash_pos) = clean.find('/') {
        let collection = clean[..slash_pos].to_string();
        let prefix = format!("/{}", &clean[slash_pos + 1..]);
        (Some(collection), prefix)
    } else {
        (Some(clean.to_string()), "/".to_string())
    }
}
```

## Quality Gates

### Unit Tests

```rust
#[cfg(test)]
mod context_tests {
    use super::*;

    #[test]
    fn test_set_and_get_global_context() {
        let store = Store::open_memory().unwrap();
        store.set_context(None, "/", "Global context").unwrap();

        let ctx = store.get_global_context().unwrap();
        assert_eq!(ctx, Some("Global context".to_string()));
    }

    #[test]
    fn test_set_and_find_collection_context() {
        let store = Store::open_memory().unwrap();
        store.add_collection("docs", "/tmp/docs", &["**/*.md"]).unwrap();

        store.set_context(Some("docs"), "/", "Documentation").unwrap();
        store.set_context(Some("docs"), "/api", "API reference").unwrap();
        store.set_context(Some("docs"), "/api/v2", "API v2 docs").unwrap();

        // Most specific match wins
        let ctx = store.find_context_for_path("docs", "/api/v2/endpoints.md").unwrap();
        assert_eq!(ctx, Some("API v2 docs".to_string()));

        let ctx = store.find_context_for_path("docs", "/api/v1/old.md").unwrap();
        assert_eq!(ctx, Some("API reference".to_string()));

        let ctx = store.find_context_for_path("docs", "/readme.md").unwrap();
        assert_eq!(ctx, Some("Documentation".to_string()));
    }

    #[test]
    fn test_fallback_to_global() {
        let store = Store::open_memory().unwrap();
        store.add_collection("docs", "/tmp/docs", &["**/*.md"]).unwrap();

        store.set_context(None, "/", "Global fallback").unwrap();

        let ctx = store.find_context_for_path("docs", "/any/path.md").unwrap();
        assert_eq!(ctx, Some("Global fallback".to_string()));
    }

    #[test]
    fn test_get_all_contexts() {
        let store = Store::open_memory().unwrap();
        store.add_collection("docs", "/tmp/docs", &["**/*.md"]).unwrap();

        store.set_context(None, "/", "Global").unwrap();
        store.set_context(Some("docs"), "/", "Docs").unwrap();
        store.set_context(Some("docs"), "/api", "API").unwrap();

        let contexts = store.get_all_contexts_for_path("docs", "/api/file.md").unwrap();
        assert_eq!(contexts, vec!["Global", "Docs", "API"]);
    }

    #[test]
    fn test_remove_context() {
        let store = Store::open_memory().unwrap();
        store.set_context(Some("docs"), "/api", "API context").unwrap();

        assert!(store.remove_context(Some("docs"), "/api").unwrap());
        assert!(!store.remove_context(Some("docs"), "/api").unwrap()); // Already removed
    }

    #[test]
    fn test_parse_context_path() {
        assert_eq!(parse_context_path("/"), (None, "/".to_string()));
        assert_eq!(parse_context_path("docs"), (Some("docs".to_string()), "/".to_string()));
        assert_eq!(parse_context_path("docs/api"), (Some("docs".to_string()), "/api".to_string()));
        assert_eq!(parse_context_path("qfs://docs/api/v2"), (Some("docs".to_string()), "/api/v2".to_string()));
    }
}
```

## Success Criteria

- [ ] `qfs context add / "Global context"` sets global context
- [ ] `qfs context add docs "Documentation collection"` sets collection root context
- [ ] `qfs context add docs/api "API reference"` sets path-specific context
- [ ] `qfs context list` shows all contexts grouped by collection
- [ ] `qfs context check` shows collections without context
- [ ] `qfs context rm docs/api` removes specific context
- [ ] Search results include context in JSON output
- [ ] Longest prefix matching works correctly
- [ ] Global context is fallback when no collection context matches
- [ ] All existing tests pass
- [ ] Database migration adds path_contexts table

## Patterns to Follow

- Store contexts in SQLite (matches existing QFS patterns)
- Normalize paths to start with /
- Use `Option<String>` for collection (None = global)
- Follow existing CRUD patterns in Store
- Use subcommands for context operations (`qfs context add/list/check/rm`)

## Edge Cases to Handle

1. **Path normalization**: "/path" and "path" should be equivalent
2. **Empty context**: Don't allow empty context strings
3. **Collection validation**: Verify collection exists before adding context
4. **Trailing slashes**: "/api/" should match "/api"
5. **Overlapping prefixes**: Longest prefix wins

## Files Changed

1. `qfs/src/store/schema.rs` - Add path_contexts table
2. `qfs/src/store/mod.rs` - Add context CRUD operations
3. `qfs/src/search/mod.rs` - Include context in SearchResult
4. `qfs-cli/src/main.rs` - Add context subcommand
5. `qfs/src/mcp/tools.rs` - Context in search results
