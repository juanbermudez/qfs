//! Content parsers for different file types

use crate::error::Result;
use std::collections::HashMap;
use std::path::Path;

/// Parsed document content
#[derive(Debug, Clone)]
pub struct ParsedDocument {
    /// Extracted title (if any)
    pub title: Option<String>,
    /// Document body text (for indexing)
    pub body: String,
    /// Extracted metadata
    pub metadata: HashMap<String, serde_json::Value>,
    /// Whether the file is binary
    pub is_binary: bool,
    /// MIME type
    pub mime_type: String,
}

/// Parse a file and extract searchable content
pub fn parse_file(path: &Path, content: &[u8]) -> Result<ParsedDocument> {
    // Check for binary content
    if crate::is_binary(content) {
        return Ok(ParsedDocument {
            title: path.file_stem().and_then(|s| s.to_str()).map(String::from),
            body: String::new(),
            metadata: HashMap::new(),
            is_binary: true,
            mime_type: mime_guess::from_path(path)
                .first_or_octet_stream()
                .to_string(),
        });
    }

    // Convert to string
    let text = String::from_utf8_lossy(content);

    // Get file extension
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    // Parse based on extension
    match ext.as_str() {
        "md" | "mdx" => parse_markdown(&text, path),
        "json" => parse_json(&text, path),
        "yaml" | "yml" => parse_yaml(&text, path),
        "jsonl" => parse_jsonl(&text, path),
        _ => parse_text(&text, path),
    }
}

/// Parse markdown content
fn parse_markdown(text: &str, path: &Path) -> Result<ParsedDocument> {
    let mut title = None;
    let mut body = String::new();
    let mut metadata = HashMap::new();
    let mut in_frontmatter = false;
    let mut frontmatter_lines = Vec::new();
    let mut _frontmatter_done = false;

    for (i, line) in text.lines().enumerate() {
        // Check for frontmatter
        if i == 0 && line.trim() == "---" {
            in_frontmatter = true;
            continue;
        }

        if in_frontmatter {
            if line.trim() == "---" {
                in_frontmatter = false;
                _frontmatter_done = true;

                // Parse frontmatter as YAML
                let frontmatter_text = frontmatter_lines.join("\n");
                if let Ok(serde_yaml::Value::Mapping(map)) =
                    serde_yaml::from_str::<serde_yaml::Value>(&frontmatter_text)
                {
                    for (k, v) in map {
                        if let serde_yaml::Value::String(key) = k {
                            // Extract title from frontmatter
                            if key == "title" {
                                if let serde_yaml::Value::String(t) = &v {
                                    title = Some(t.clone());
                                }
                            }
                            // Convert YAML value to JSON
                            if let Ok(json_val) = serde_json::to_value(&v) {
                                metadata.insert(key, json_val);
                            }
                        }
                    }
                }
                continue;
            }
            frontmatter_lines.push(line);
            continue;
        }

        // Extract title from first H1 if not in frontmatter
        if title.is_none() && line.starts_with("# ") {
            title = Some(line[2..].trim().to_string());
        }

        // Add to body
        body.push_str(line);
        body.push('\n');
    }

    // If no title found, use filename
    if title.is_none() {
        title = path.file_stem().and_then(|s| s.to_str()).map(String::from);
    }

    Ok(ParsedDocument {
        title,
        body: body.trim().to_string(),
        metadata,
        is_binary: false,
        mime_type: "text/markdown".to_string(),
    })
}

/// Parse JSON content
fn parse_json(text: &str, path: &Path) -> Result<ParsedDocument> {
    let title = path.file_stem().and_then(|s| s.to_str()).map(String::from);

    // Try to parse and flatten for search
    let body = if let Ok(value) = serde_json::from_str::<serde_json::Value>(text) {
        flatten_json(&value)
    } else {
        text.to_string()
    };

    Ok(ParsedDocument {
        title,
        body,
        metadata: HashMap::new(),
        is_binary: false,
        mime_type: "application/json".to_string(),
    })
}

/// Parse YAML content
fn parse_yaml(text: &str, path: &Path) -> Result<ParsedDocument> {
    let title = path.file_stem().and_then(|s| s.to_str()).map(String::from);

    // Try to parse and flatten for search
    let body = if let Ok(value) = serde_yaml::from_str::<serde_yaml::Value>(text) {
        if let Ok(json) = serde_json::to_value(&value) {
            flatten_json(&json)
        } else {
            text.to_string()
        }
    } else {
        text.to_string()
    };

    Ok(ParsedDocument {
        title,
        body,
        metadata: HashMap::new(),
        is_binary: false,
        mime_type: "text/yaml".to_string(),
    })
}

/// Parse JSONL content (JSON Lines, used for Claude sessions)
fn parse_jsonl(text: &str, path: &Path) -> Result<ParsedDocument> {
    let title = path.file_stem().and_then(|s| s.to_str()).map(String::from);

    // Parse each line as JSON and extract relevant content
    let mut body_parts = Vec::new();

    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }

        if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
            // Extract message content (common in Claude session format)
            if let Some(message) = value.get("message") {
                if let Some(content) = message.get("content") {
                    if let Some(s) = content.as_str() {
                        body_parts.push(s.to_string());
                    }
                }
            }
            // Also include any "text" or "content" fields at root
            if let Some(content) = value.get("content").and_then(|c| c.as_str()) {
                body_parts.push(content.to_string());
            }
            if let Some(text) = value.get("text").and_then(|t| t.as_str()) {
                body_parts.push(text.to_string());
            }
        }
    }

    let body = if body_parts.is_empty() {
        // Fallback: use raw text
        text.to_string()
    } else {
        body_parts.join("\n\n")
    };

    Ok(ParsedDocument {
        title,
        body,
        metadata: HashMap::new(),
        is_binary: false,
        mime_type: "application/x-jsonlines".to_string(),
    })
}

/// Parse plain text content
fn parse_text(text: &str, path: &Path) -> Result<ParsedDocument> {
    let title = path.file_stem().and_then(|s| s.to_str()).map(String::from);

    let mime_type = mime_guess::from_path(path)
        .first_or_text_plain()
        .to_string();

    Ok(ParsedDocument {
        title,
        body: text.to_string(),
        metadata: HashMap::new(),
        is_binary: false,
        mime_type,
    })
}

/// Flatten JSON value to searchable text
fn flatten_json(value: &serde_json::Value) -> String {
    let mut parts = Vec::new();
    flatten_json_recursive(value, &mut parts);
    parts.join(" ")
}

fn flatten_json_recursive(value: &serde_json::Value, parts: &mut Vec<String>) {
    match value {
        serde_json::Value::String(s) => parts.push(s.clone()),
        serde_json::Value::Number(n) => parts.push(n.to_string()),
        serde_json::Value::Bool(b) => parts.push(b.to_string()),
        serde_json::Value::Array(arr) => {
            for item in arr {
                flatten_json_recursive(item, parts);
            }
        }
        serde_json::Value::Object(obj) => {
            for (key, val) in obj {
                parts.push(key.clone());
                flatten_json_recursive(val, parts);
            }
        }
        serde_json::Value::Null => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_markdown_with_frontmatter() {
        let content = r#"---
title: Test Document
tags:
  - rust
  - search
---

# Hello World

This is the body content.
"#;

        let result = parse_file(Path::new("test.md"), content.as_bytes()).unwrap();

        assert_eq!(result.title, Some("Test Document".to_string()));
        assert!(result.body.contains("Hello World"));
        assert!(result.body.contains("body content"));
        assert!(!result.is_binary);
    }

    #[test]
    fn test_parse_markdown_no_frontmatter() {
        let content = "# My Title\n\nSome content here.";

        let result = parse_file(Path::new("test.md"), content.as_bytes()).unwrap();

        assert_eq!(result.title, Some("My Title".to_string()));
        assert!(result.body.contains("Some content"));
    }

    #[test]
    fn test_parse_json() {
        let content = r#"{"name": "test", "value": 42}"#;

        let result = parse_file(Path::new("data.json"), content.as_bytes()).unwrap();

        assert_eq!(result.title, Some("data".to_string()));
        assert!(result.body.contains("name"));
        assert!(result.body.contains("test"));
        assert!(result.body.contains("42"));
    }

    #[test]
    fn test_parse_binary() {
        let content = b"Hello\x00World"; // Contains NUL byte

        let result = parse_file(Path::new("file.bin"), content).unwrap();

        assert!(result.is_binary);
        assert!(result.body.is_empty());
    }

    #[test]
    fn test_parse_code() {
        let content = r#"
fn main() {
    println!("Hello, world!");
}
"#;

        let result = parse_file(Path::new("main.rs"), content.as_bytes()).unwrap();

        assert_eq!(result.title, Some("main".to_string()));
        assert!(result.body.contains("fn main()"));
        assert!(!result.is_binary);
    }
}
