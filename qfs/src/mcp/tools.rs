//! MCP tool handlers for QFS
//!
//! Each tool handler processes a specific tool call and returns results.

use super::protocol::{JsonRpcError, ToolDefinition, ToolResult};
use crate::search::{SearchMode, SearchOptions, Searcher};
use crate::store::Store;
use serde_json::{json, Value};

/// Get all tool definitions
pub fn get_tool_definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "qfs_search".to_string(),
            description: "Full-text search across indexed documents using BM25 ranking. Returns relevant documents with snippets.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query text"
                    },
                    "collection": {
                        "type": "string",
                        "description": "Optional collection name to search within"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of results (default: 20)",
                        "default": 20
                    }
                },
                "required": ["query"]
            }),
        },
        ToolDefinition {
            name: "qfs_vsearch".to_string(),
            description: "Semantic vector search using embeddings. Requires embeddings to be generated first.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query for semantic matching"
                    },
                    "collection": {
                        "type": "string",
                        "description": "Optional collection name"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of results (default: 20)",
                        "default": 20
                    }
                },
                "required": ["query"]
            }),
        },
        ToolDefinition {
            name: "qfs_query".to_string(),
            description: "Hybrid search combining BM25 and vector search with Reciprocal Rank Fusion. Requires mode parameter to select search type.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query"
                    },
                    "mode": {
                        "type": "string",
                        "enum": ["bm25", "vector", "hybrid"],
                        "description": "Search mode: bm25 (keyword), vector (semantic), or hybrid (combined)",
                        "default": "bm25"
                    },
                    "collection": {
                        "type": "string",
                        "description": "Optional collection name"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of results (default: 20)",
                        "default": 20
                    }
                },
                "required": ["query"]
            }),
        },
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
        ToolDefinition {
            name: "qfs_status".to_string(),
            description: "Get the status of the QFS index, including collection and document counts.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        },
    ]
}

/// Handle tool call dispatch
pub async fn handle_tool_call(
    store: &Store,
    tool_name: &str,
    arguments: &Value,
) -> Result<ToolResult, JsonRpcError> {
    match tool_name {
        "qfs_search" => tool_search(store, arguments, SearchMode::Bm25).await,
        "qfs_vsearch" => tool_search(store, arguments, SearchMode::Vector).await,
        "qfs_query" => tool_query(store, arguments).await,
        "qfs_get" => tool_get(store, arguments).await,
        "qfs_multi_get" => tool_multi_get(store, arguments).await,
        "qfs_status" => tool_status(store).await,
        _ => Err(JsonRpcError::invalid_params(format!(
            "Unknown tool: {}",
            tool_name
        ))),
    }
}

/// Execute search tool (qfs_search or qfs_vsearch)
async fn tool_search(
    store: &Store,
    args: &Value,
    mode: SearchMode,
) -> Result<ToolResult, JsonRpcError> {
    let query = args
        .get("query")
        .and_then(|v| v.as_str())
        .ok_or_else(|| JsonRpcError::invalid_params("Missing query parameter"))?;

    let collection = args.get("collection").and_then(|v| v.as_str());
    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as usize;

    let options = SearchOptions {
        mode,
        limit,
        min_score: 0.0,
        collection: collection.map(String::from),
        include_binary: false,
    };

    let searcher = Searcher::new(store);
    let results = searcher
        .search(query, options)
        .await
        .map_err(|e| JsonRpcError::server_error(e.to_string()))?;

    let text = serde_json::to_string_pretty(&results)
        .map_err(|e| JsonRpcError::server_error(e.to_string()))?;

    Ok(ToolResult::text(text))
}

/// Execute query tool with mode selection (qfs_query)
async fn tool_query(store: &Store, args: &Value) -> Result<ToolResult, JsonRpcError> {
    let query = args
        .get("query")
        .and_then(|v| v.as_str())
        .ok_or_else(|| JsonRpcError::invalid_params("Missing query parameter"))?;

    let mode_str = args.get("mode").and_then(|v| v.as_str()).unwrap_or("bm25");

    let mode: SearchMode = mode_str
        .parse()
        .map_err(|e: crate::Error| JsonRpcError::invalid_params(e.to_string()))?;

    let collection = args.get("collection").and_then(|v| v.as_str());
    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as usize;

    let options = SearchOptions {
        mode,
        limit,
        min_score: 0.0,
        collection: collection.map(String::from),
        include_binary: false,
    };

    let searcher = Searcher::new(store);
    let results = searcher
        .search(query, options)
        .await
        .map_err(|e| JsonRpcError::server_error(e.to_string()))?;

    let text = serde_json::to_string_pretty(&results)
        .map_err(|e| JsonRpcError::server_error(e.to_string()))?;

    Ok(ToolResult::text(text))
}

/// Execute get tool (qfs_get)
async fn tool_get(store: &Store, args: &Value) -> Result<ToolResult, JsonRpcError> {
    let path = args
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| JsonRpcError::invalid_params("Missing path parameter"))?;

    let include_content = args
        .get("include_content")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    let from_line = args
        .get("from_line")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize);

    let max_lines = args
        .get("max_lines")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize);

    let line_numbers = args
        .get("line_numbers")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // Parse :linenum suffix if from_line not provided
    let (clean_path, suffix_line) = crate::parse_path_with_line(path);
    let effective_from = from_line.or(suffix_line);

    // Check if input is a docid
    let doc = if crate::store::is_docid(clean_path) {
        store
            .get_document_by_docid(clean_path)
            .await
            .map_err(|e| JsonRpcError::server_error(e.to_string()))?
    } else {
        // Parse path as collection/relative_path
        let parts: Vec<&str> = clean_path.splitn(2, '/').collect();
        if parts.len() != 2 {
            return Err(JsonRpcError::invalid_params(
                "Path must be in format 'collection/relative_path' or docid (#abc123)",
            ));
        }
        store
            .get_document(parts[0], parts[1])
            .await
            .map_err(|e| JsonRpcError::server_error(e.to_string()))?
    };

    let mut result = json!({
        "id": doc.id,
        "collection": doc.collection,
        "path": doc.path,
        "title": doc.title,
        "fileType": doc.file_type,
        "hash": doc.hash,
        "createdAt": doc.created_at,
        "modifiedAt": doc.modified_at
    });

    if include_content {
        if let Ok(content) = store.get_content(&doc.hash).await {
            if let Ok(text) = String::from_utf8(content.data.clone()) {
                // Apply line extraction
                let mut output = crate::extract_lines(&text, effective_from, max_lines);

                // Add line numbers if requested
                if line_numbers {
                    let start = effective_from.unwrap_or(1);
                    output = crate::add_line_numbers(&output, start);
                }

                result["content"] = json!(output);
                if let Some(from) = effective_from {
                    result["fromLine"] = json!(from);
                }
                result["lineCount"] = json!(output.lines().count());
            } else {
                result["contentPointer"] = json!(format!("{}/{}", doc.collection, doc.path));
            }
            result["mimeType"] = json!(content.content_type);
            result["size"] = json!(content.size);
        }
    }

    let text = serde_json::to_string_pretty(&result)
        .map_err(|e| JsonRpcError::server_error(e.to_string()))?;

    Ok(ToolResult::text(text))
}

/// Execute multi_get tool (qfs_multi_get)
async fn tool_multi_get(store: &Store, args: &Value) -> Result<ToolResult, JsonRpcError> {
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
        .await
        .map_err(|e| JsonRpcError::server_error(e.to_string()))?;

    let text = serde_json::to_string_pretty(&results)
        .map_err(|e| JsonRpcError::server_error(e.to_string()))?;

    Ok(ToolResult::text(text))
}

/// Execute status tool (qfs_status)
async fn tool_status(store: &Store) -> Result<ToolResult, JsonRpcError> {
    let collections = store
        .list_collections()
        .await
        .map_err(|e| JsonRpcError::server_error(e.to_string()))?;

    let total_docs = store.count_documents(None).await.unwrap_or(0);
    let total_embeddings = store.count_embeddings(None).await.unwrap_or(0);

    let db_size = store.database_size().unwrap_or(0);

    let mut collection_stats = Vec::new();
    for col in &collections {
        let doc_count = store.count_documents(Some(&col.name)).await.unwrap_or(0);
        let embed_count = store.count_embeddings(Some(&col.name)).await.unwrap_or(0);
        collection_stats.push(json!({
            "name": col.name,
            "path": col.path,
            "documents": doc_count,
            "embeddings": embed_count,
            "patterns": col.patterns,
            "updatedAt": col.updated_at
        }));
    }

    let status = json!({
        "version": env!("CARGO_PKG_VERSION"),
        "totalCollections": collections.len(),
        "totalDocuments": total_docs,
        "totalEmbeddings": total_embeddings,
        "databaseSizeBytes": db_size,
        "collections": collection_stats
    });

    let text = serde_json::to_string_pretty(&status)
        .map_err(|e| JsonRpcError::server_error(e.to_string()))?;

    Ok(ToolResult::text(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_definitions_valid() {
        let tools = get_tool_definitions();
        assert_eq!(tools.len(), 6);

        // Verify all required tools are present
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"qfs_search"));
        assert!(names.contains(&"qfs_vsearch"));
        assert!(names.contains(&"qfs_query"));
        assert!(names.contains(&"qfs_get"));
        assert!(names.contains(&"qfs_multi_get"));
        assert!(names.contains(&"qfs_status"));
    }

    #[test]
    fn test_tool_definitions_have_schemas() {
        let tools = get_tool_definitions();
        for tool in tools {
            assert!(
                !tool.description.is_empty(),
                "{} has empty description",
                tool.name
            );
            assert!(
                tool.input_schema.is_object(),
                "{} has invalid schema",
                tool.name
            );
        }
    }

    #[tokio::test]
    async fn test_unknown_tool_error() {
        let store = Store::open_memory().await.unwrap();
        let result = handle_tool_call(&store, "unknown_tool", &json!({})).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.code, -32602);
    }

    #[tokio::test]
    async fn test_missing_query_error() {
        let store = Store::open_memory().await.unwrap();
        let result = handle_tool_call(&store, "qfs_search", &json!({})).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.message.contains("Missing query"));
    }

    #[tokio::test]
    async fn test_status_tool() {
        let store = Store::open_memory().await.unwrap();
        let result = handle_tool_call(&store, "qfs_status", &json!({})).await;
        assert!(result.is_ok());
        let tool_result = result.unwrap();
        assert!(!tool_result.content.is_empty());
        assert!(tool_result.content[0].text.contains("version"));
    }

    #[tokio::test]
    async fn test_search_empty_results() {
        let store = Store::open_memory().await.unwrap();
        let result = handle_tool_call(&store, "qfs_search", &json!({"query": "nonexistent"})).await;
        assert!(result.is_ok());
        let tool_result = result.unwrap();
        // Should return empty array, not error
        assert!(tool_result.content[0].text.contains("[]"));
    }

    #[tokio::test]
    async fn test_get_invalid_path() {
        let store = Store::open_memory().await.unwrap();
        let result = handle_tool_call(&store, "qfs_get", &json!({"path": "invalid"})).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.message.contains("format"));
    }
}
