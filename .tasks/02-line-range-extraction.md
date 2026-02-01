# Task 02: Line Range Extraction

## Overview

Implement line range extraction for document retrieval. Users should be able to request specific line ranges from documents using `:linenum` suffix, `--from` flag, and `-l` (limit) flag.

## QMD Reference Implementation

### Syntax Support
1. **`:linenum` suffix**: `file.md:100` - start at line 100
2. **`--from` flag**: `--from 50` - start at line 50 (overrides suffix)
3. **`-l` flag**: `-l 10` - limit to 10 lines

### Parsing Logic (qmd.ts:692-701)
```typescript
// Parse :linenum suffix from filename
let inputPath = filename;
const colonMatch = inputPath.match(/:(\d+)$/);
if (colonMatch && !fromLine) {  // Only if --from not provided
  const matched = colonMatch[1];
  if (matched) {
    fromLine = parseInt(matched, 10);
    inputPath = inputPath.slice(0, -colonMatch[0].length);
  }
}
```

### Line Extraction Logic (qmd.ts:846-859)
```typescript
let output = doc.body;
const startLine = fromLine || 1;

if (fromLine !== undefined || maxLines !== undefined) {
  const lines = output.split('\n');
  const start = startLine - 1;  // Convert to 0-indexed
  const end = maxLines !== undefined ? start + maxLines : lines.length;
  output = lines.slice(start, end).join('\n');
}

// Add line numbers if requested
if (lineNumbers) {
  output = addLineNumbers(output, startLine);
}
```

### Line Number Formatting (formatter.ts:55-58)
```typescript
export function addLineNumbers(text: string, startLine: number = 1): string {
  const lines = text.split('\n');
  return lines.map((line, i) => `${startLine + i}: ${line}`).join('\n');
}
```

## Current QFS State

### Existing Code
- `cmd_get` in `qfs-cli/src/main.rs:302-327` retrieves documents but no line range support
- `tool_get` in `qfs/src/mcp/tools.rs:237-287` retrieves via MCP but no line range support
- Content stored in `content` table, retrieved via `store.get_content(hash)`

### Files to Modify
1. `qfs-cli/src/main.rs` - Update `get` command with flags
2. `qfs/src/store/mod.rs` - Add line extraction utility
3. `qfs/src/mcp/tools.rs` - Update `qfs_get` tool schema and handler

## Implementation Plan

### Step 1: Add Line Extraction Utilities (qfs/src/lib.rs or new utils module)

```rust
/// Parse a path that may contain a :linenum suffix.
/// Returns (path, optional_line_number)
pub fn parse_path_with_line(input: &str) -> (&str, Option<usize>) {
    // Match :digits at end of string
    if let Some(colon_pos) = input.rfind(':') {
        let suffix = &input[colon_pos + 1..];
        if !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit()) {
            if let Ok(line_num) = suffix.parse::<usize>() {
                return (&input[..colon_pos], Some(line_num));
            }
        }
    }
    (input, None)
}

/// Extract a range of lines from text content.
/// `from_line` is 1-indexed. Returns empty string if out of bounds.
pub fn extract_lines(content: &str, from_line: Option<usize>, max_lines: Option<usize>) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let start = from_line.unwrap_or(1).saturating_sub(1);  // Convert to 0-indexed
    let end = match max_lines {
        Some(limit) => (start + limit).min(lines.len()),
        None => lines.len(),
    };

    if start >= lines.len() {
        return String::new();
    }

    lines[start..end].join("\n")
}

/// Add line numbers to text, starting from the given line number.
pub fn add_line_numbers(text: &str, start_line: usize) -> String {
    text.lines()
        .enumerate()
        .map(|(i, line)| format!("{}: {}", start_line + i, line))
        .collect::<Vec<_>>()
        .join("\n")
}
```

### Step 2: Update CLI Get Command (qfs-cli/src/main.rs)

Update the `Commands::Get` struct:

```rust
/// Get a document by path
Get {
    /// Document path (collection/relative_path or docid)
    /// Supports :linenum suffix (e.g., "docs/file.md:50")
    path: String,

    /// Start from this line number (1-indexed, overrides :linenum suffix)
    #[arg(long)]
    from: Option<usize>,

    /// Maximum number of lines to return
    #[arg(short = 'l', long = "lines")]
    max_lines: Option<usize>,

    /// Add line numbers to output
    #[arg(long)]
    line_numbers: bool,

    /// Output format (text, json)
    #[arg(long, short = 'o', default_value = "text")]
    format: String,
},
```

Update `cmd_get`:

```rust
fn cmd_get(
    db_path: &PathBuf,
    path: &str,
    from_line: Option<usize>,
    max_lines: Option<usize>,
    line_numbers: bool,
    format: &str,
) -> Result<()> {
    let store = Store::open(db_path)?;

    // Parse :linenum suffix if --from not provided
    let (clean_path, suffix_line) = qfs::parse_path_with_line(path);
    let effective_from = from_line.or(suffix_line);

    // Check if input is a docid
    let doc = if qfs::store::is_docid(clean_path) {
        store.get_document_by_docid(clean_path)?
    } else {
        // Parse path as collection/relative_path
        let parts: Vec<&str> = clean_path.splitn(2, '/').collect();
        if parts.len() != 2 {
            anyhow::bail!("Path must be in format 'collection/relative_path' or docid");
        }
        store.get_document(parts[0], parts[1])?
    };

    // Get content
    let content = store.get_content(&doc.hash)?;
    let text = String::from_utf8(content.data.clone())
        .map_err(|_| anyhow::anyhow!("Content is not valid UTF-8"))?;

    // Apply line extraction
    let mut output = qfs::extract_lines(&text, effective_from, max_lines);

    // Add line numbers if requested
    if line_numbers {
        let start = effective_from.unwrap_or(1);
        output = qfs::add_line_numbers(&output, start);
    }

    if format == "json" {
        let result = serde_json::json!({
            "path": format!("{}/{}", doc.collection, doc.path),
            "title": doc.title,
            "fromLine": effective_from,
            "lineCount": output.lines().count(),
            "content": output,
        });
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!("Path: {}/{}", doc.collection, doc.path);
        if let Some(ref title) = doc.title {
            println!("Title: {}", title);
        }
        if let Some(from) = effective_from {
            println!("From line: {}", from);
        }
        println!("\n{}", output);
    }

    Ok(())
}
```

### Step 3: Update MCP Tool (qfs/src/mcp/tools.rs)

Update tool definition:

```rust
ToolDefinition {
    name: "qfs_get".to_string(),
    description: "Get a specific document by its path or docid. Supports line range extraction.".to_string(),
    input_schema: json!({
        "type": "object",
        "properties": {
            "path": {
                "type": "string",
                "description": "Document path (collection/relative_path), docid (#abc123), or path:linenum"
            },
            "from_line": {
                "type": "integer",
                "description": "Start from this line number (1-indexed)"
            },
            "max_lines": {
                "type": "integer",
                "description": "Maximum number of lines to return"
            },
            "line_numbers": {
                "type": "boolean",
                "description": "Add line numbers to output (format: 'N: content')",
                "default": false
            },
            "include_content": {
                "type": "boolean",
                "description": "Whether to include file content (default: true)",
                "default": true
            }
        },
        "required": ["path"]
    }),
},
```

Update `tool_get` handler to use the new line extraction.

## Quality Gates

### Unit Tests (qfs/src/lib.rs)

```rust
#[cfg(test)]
mod line_extraction_tests {
    use super::*;

    #[test]
    fn test_parse_path_with_line_suffix() {
        let (path, line) = parse_path_with_line("docs/file.md:50");
        assert_eq!(path, "docs/file.md");
        assert_eq!(line, Some(50));
    }

    #[test]
    fn test_parse_path_without_suffix() {
        let (path, line) = parse_path_with_line("docs/file.md");
        assert_eq!(path, "docs/file.md");
        assert_eq!(line, None);
    }

    #[test]
    fn test_parse_path_with_colon_in_name() {
        // Colons followed by non-digits should not be parsed
        let (path, line) = parse_path_with_line("docs/10:30_meeting.md");
        assert_eq!(path, "docs/10:30_meeting.md");
        assert_eq!(line, None);
    }

    #[test]
    fn test_extract_lines_from_start() {
        let content = "line1\nline2\nline3\nline4\nline5";
        let result = extract_lines(content, Some(2), Some(2));
        assert_eq!(result, "line2\nline3");
    }

    #[test]
    fn test_extract_lines_to_end() {
        let content = "line1\nline2\nline3";
        let result = extract_lines(content, Some(2), None);
        assert_eq!(result, "line2\nline3");
    }

    #[test]
    fn test_extract_lines_out_of_bounds() {
        let content = "line1\nline2";
        let result = extract_lines(content, Some(10), None);
        assert_eq!(result, "");
    }

    #[test]
    fn test_extract_lines_max_exceeds_length() {
        let content = "line1\nline2\nline3";
        let result = extract_lines(content, Some(2), Some(100));
        assert_eq!(result, "line2\nline3");
    }

    #[test]
    fn test_add_line_numbers() {
        let text = "foo\nbar\nbaz";
        let result = add_line_numbers(text, 10);
        assert_eq!(result, "10: foo\n11: bar\n12: baz");
    }

    #[test]
    fn test_add_line_numbers_from_one() {
        let text = "first\nsecond";
        let result = add_line_numbers(text, 1);
        assert_eq!(result, "1: first\n2: second");
    }
}
```

### CLI Integration Test

```rust
#[test]
fn test_get_with_line_range() {
    // Setup: create store with multi-line document
    // Test: qfs get "docs/file.md:3" -l 2
    // Verify: output contains lines 3-4 only
}

#[test]
fn test_get_with_from_flag() {
    // Test: qfs get "docs/file.md" --from 5 -l 3
    // Verify: output contains lines 5-7
}

#[test]
fn test_get_with_line_numbers() {
    // Test: qfs get "docs/file.md" --from 10 -l 2 --line-numbers
    // Verify: output shows "10: ..." and "11: ..."
}
```

## Success Criteria

- [ ] `qfs get "collection/file.md:50"` returns content starting at line 50
- [ ] `qfs get "collection/file.md" --from 10` works with flag
- [ ] `qfs get "collection/file.md" -l 20` limits output to 20 lines
- [ ] `qfs get "collection/file.md:50" -l 10` combines suffix and limit
- [ ] `--from` flag overrides `:linenum` suffix
- [ ] `--line-numbers` flag adds line numbers with correct offset
- [ ] Out-of-bounds line numbers return empty content gracefully
- [ ] MCP `qfs_get` tool supports all line range options
- [ ] All existing tests pass
- [ ] New unit tests pass

## Patterns to Follow

- Use `Option<usize>` for optional line parameters
- 1-indexed line numbers in user interface (convert to 0-indexed internally)
- Use `lines()` iterator for splitting (handles both \n and \r\n)
- Return empty string for out-of-bounds, not error
- Match existing CLI argument style (long flags with short aliases)

## Edge Cases to Handle

1. **Empty file**: Return empty string
2. **Line number 0**: Treat as line 1 (use `saturating_sub`)
3. **Line beyond EOF**: Return empty string
4. **max_lines = 0**: Return empty string
5. **Colon in filename**: Only parse if followed by digits at end
6. **Binary content**: Error with "not valid UTF-8"

## Files Changed

1. `qfs/src/lib.rs` - Add line extraction utilities
2. `qfs-cli/src/main.rs` - Update Get command and handler
3. `qfs/src/mcp/tools.rs` - Update qfs_get tool
