//! MCP (Model Context Protocol) server implementation for QFS
//!
//! This module implements an MCP server using stdio transport, exposing
//! QFS search functionality to AI agents.

use anyhow::Result;
use qfs::{SearchMode, SearchOptions, Store};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

/// MCP server for QFS
pub struct McpServer {
    store: Store,
}

impl McpServer {
    /// Create a new MCP server
    pub fn new(db_path: &PathBuf) -> Result<Self> {
        let store = Store::open(db_path)?;
        Ok(Self { store })
    }

    /// Run the MCP server on stdio
    pub fn run(&self) -> Result<()> {
        let stdin = std::io::stdin();
        let stdout = std::io::stdout();
        let mut reader = BufReader::new(stdin.lock());
        let mut writer = stdout.lock();

        tracing::info!("QFS MCP server started");

        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) => {
                    tracing::info!("EOF received, shutting down");
                    break;
                }
                Ok(_) => {
                    let line = line.trim();
                    if line.is_empty() {
                        continue;
                    }

                    tracing::debug!("Received: {}", line);

                    match serde_json::from_str::<JsonRpcRequest>(line) {
                        Ok(request) => {
                            let response = self.handle_request(request);
                            let response_json = serde_json::to_string(&response)?;
                            writeln!(writer, "{}", response_json)?;
                            writer.flush()?;
                            tracing::debug!("Sent: {}", response_json);
                        }
                        Err(e) => {
                            let error_response = JsonRpcResponse {
                                jsonrpc: "2.0".to_string(),
                                id: None,
                                result: None,
                                error: Some(JsonRpcError {
                                    code: -32700,
                                    message: format!("Parse error: {}", e),
                                    data: None,
                                }),
                            };
                            let response_json = serde_json::to_string(&error_response)?;
                            writeln!(writer, "{}", response_json)?;
                            writer.flush()?;
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("Read error: {}", e);
                    break;
                }
            }
        }

        Ok(())
    }

    /// Handle a JSON-RPC request
    fn handle_request(&self, request: JsonRpcRequest) -> JsonRpcResponse {
        let result = match request.method.as_str() {
            "initialize" => self.handle_initialize(&request.params),
            "tools/list" => self.handle_tools_list(),
            "tools/call" => self.handle_tools_call(&request.params),
            "notifications/initialized" => {
                // Client notification, no response needed
                return JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: request.id,
                    result: Some(json!({})),
                    error: None,
                };
            }
            _ => Err(JsonRpcError {
                code: -32601,
                message: format!("Method not found: {}", request.method),
                data: None,
            }),
        };

        match result {
            Ok(value) => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id: request.id,
                result: Some(value),
                error: None,
            },
            Err(error) => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id: request.id,
                result: None,
                error: Some(error),
            },
        }
    }

    /// Handle initialize request
    fn handle_initialize(&self, _params: &Option<Value>) -> Result<Value, JsonRpcError> {
        Ok(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "tools": {}
            },
            "serverInfo": {
                "name": "qfs",
                "version": env!("CARGO_PKG_VERSION")
            }
        }))
    }

    /// Handle tools/list request
    fn handle_tools_list(&self) -> Result<Value, JsonRpcError> {
        Ok(json!({
            "tools": [
                {
                    "name": "qfs_search",
                    "description": "Full-text search across indexed documents using BM25 ranking. Returns relevant documents with snippets.",
                    "inputSchema": {
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
                    }
                },
                {
                    "name": "qfs_vsearch",
                    "description": "Semantic vector search using embeddings. Requires embeddings to be generated first.",
                    "inputSchema": {
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
                    }
                },
                {
                    "name": "qfs_query",
                    "description": "Hybrid search combining BM25 and vector search with Reciprocal Rank Fusion.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "query": {
                                "type": "string",
                                "description": "Search query"
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
                    }
                },
                {
                    "name": "qfs_get",
                    "description": "Get a specific document by its path.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "path": {
                                "type": "string",
                                "description": "Document path in format 'collection/relative_path'"
                            },
                            "include_content": {
                                "type": "boolean",
                                "description": "Whether to include file content (default: true)",
                                "default": true
                            }
                        },
                        "required": ["path"]
                    }
                },
                {
                    "name": "qfs_multi_get",
                    "description": "Get multiple documents by their paths.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "paths": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "Array of document paths in format 'collection/relative_path'"
                            },
                            "include_content": {
                                "type": "boolean",
                                "description": "Whether to include file content (default: true)",
                                "default": true
                            }
                        },
                        "required": ["paths"]
                    }
                },
                {
                    "name": "qfs_status",
                    "description": "Get the status of the QFS index, including collection and document counts.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {},
                        "required": []
                    }
                }
            ]
        }))
    }

    /// Handle tools/call request
    fn handle_tools_call(&self, params: &Option<Value>) -> Result<Value, JsonRpcError> {
        let params = params.as_ref().ok_or_else(|| JsonRpcError {
            code: -32602,
            message: "Missing params".to_string(),
            data: None,
        })?;

        let tool_name = params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| JsonRpcError {
                code: -32602,
                message: "Missing tool name".to_string(),
                data: None,
            })?;

        let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

        let result = match tool_name {
            "qfs_search" => self.tool_search(&arguments, SearchMode::Bm25),
            "qfs_vsearch" => self.tool_search(&arguments, SearchMode::Vector),
            "qfs_query" => self.tool_search(&arguments, SearchMode::Hybrid),
            "qfs_get" => self.tool_get(&arguments),
            "qfs_multi_get" => self.tool_multi_get(&arguments),
            "qfs_status" => self.tool_status(),
            _ => Err(JsonRpcError {
                code: -32602,
                message: format!("Unknown tool: {}", tool_name),
                data: None,
            }),
        };

        result.map(|content| {
            json!({
                "content": [
                    {
                        "type": "text",
                        "text": content
                    }
                ]
            })
        })
    }

    /// Execute search tool
    fn tool_search(&self, args: &Value, mode: SearchMode) -> Result<String, JsonRpcError> {
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| JsonRpcError {
                code: -32602,
                message: "Missing query parameter".to_string(),
                data: None,
            })?;

        let collection = args.get("collection").and_then(|v| v.as_str());
        let limit = args
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(20) as usize;

        let options = SearchOptions {
            mode,
            limit,
            min_score: 0.0,
            collection: collection.map(String::from),
            include_binary: false,
        };

        let searcher = qfs::search::Searcher::new(&self.store);
        let results = searcher.search(query, options).map_err(|e| JsonRpcError {
            code: -32000,
            message: e.to_string(),
            data: None,
        })?;

        serde_json::to_string_pretty(&results).map_err(|e| JsonRpcError {
            code: -32000,
            message: e.to_string(),
            data: None,
        })
    }

    /// Execute get tool
    fn tool_get(&self, args: &Value) -> Result<String, JsonRpcError> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| JsonRpcError {
                code: -32602,
                message: "Missing path parameter".to_string(),
                data: None,
            })?;

        let include_content = args
            .get("include_content")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        // Parse path as collection/relative_path
        let parts: Vec<&str> = path.splitn(2, '/').collect();
        if parts.len() != 2 {
            return Err(JsonRpcError {
                code: -32602,
                message: "Path must be in format 'collection/relative_path'".to_string(),
                data: None,
            });
        }

        let doc = self
            .store
            .get_document(parts[0], parts[1])
            .map_err(|e| JsonRpcError {
                code: -32000,
                message: e.to_string(),
                data: None,
            })?;

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
            if let Ok(content) = self.store.get_content(&doc.hash) {
                if let Ok(text) = String::from_utf8(content.data.clone()) {
                    result["content"] = json!(text);
                } else {
                    result["contentPointer"] = json!(format!("{}/{}", doc.collection, doc.path));
                }
                result["mimeType"] = json!(content.content_type);
                result["size"] = json!(content.size);
            }
        }

        serde_json::to_string_pretty(&result).map_err(|e| JsonRpcError {
            code: -32000,
            message: e.to_string(),
            data: None,
        })
    }

    /// Execute multi_get tool
    fn tool_multi_get(&self, args: &Value) -> Result<String, JsonRpcError> {
        let paths = args
            .get("paths")
            .and_then(|v| v.as_array())
            .ok_or_else(|| JsonRpcError {
                code: -32602,
                message: "Missing paths parameter".to_string(),
                data: None,
            })?;

        let include_content = args
            .get("include_content")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let mut results = Vec::new();
        for path_value in paths {
            if let Some(path) = path_value.as_str() {
                let parts: Vec<&str> = path.splitn(2, '/').collect();
                if parts.len() == 2 {
                    if let Ok(doc) = self.store.get_document(parts[0], parts[1]) {
                        let mut result = json!({
                            "id": doc.id,
                            "collection": doc.collection,
                            "path": doc.path,
                            "title": doc.title,
                            "fileType": doc.file_type,
                            "hash": doc.hash
                        });

                        if include_content {
                            if let Ok(content) = self.store.get_content(&doc.hash) {
                                if let Ok(text) = String::from_utf8(content.data.clone()) {
                                    result["content"] = json!(text);
                                }
                                result["mimeType"] = json!(content.content_type);
                            }
                        }

                        results.push(result);
                    }
                }
            }
        }

        serde_json::to_string_pretty(&json!({ "documents": results })).map_err(|e| JsonRpcError {
            code: -32000,
            message: e.to_string(),
            data: None,
        })
    }

    /// Execute status tool
    fn tool_status(&self) -> Result<String, JsonRpcError> {
        let collections = self.store.list_collections().map_err(|e| JsonRpcError {
            code: -32000,
            message: e.to_string(),
            data: None,
        })?;

        let total_docs = self.store.count_documents(None).unwrap_or(0);
        let total_embeddings = self.store.count_embeddings(None).unwrap_or(0);

        let collection_stats: Vec<Value> = collections
            .iter()
            .map(|col| {
                let doc_count = self.store.count_documents(Some(&col.name)).unwrap_or(0);
                let embed_count = self.store.count_embeddings(Some(&col.name)).unwrap_or(0);
                json!({
                    "name": col.name,
                    "path": col.path,
                    "documents": doc_count,
                    "embeddings": embed_count,
                    "patterns": col.patterns
                })
            })
            .collect();

        let status = json!({
            "version": env!("CARGO_PKG_VERSION"),
            "totalCollections": collections.len(),
            "totalDocuments": total_docs,
            "totalEmbeddings": total_embeddings,
            "collections": collection_stats
        });

        serde_json::to_string_pretty(&status).map_err(|e| JsonRpcError {
            code: -32000,
            message: e.to_string(),
            data: None,
        })
    }
}

/// JSON-RPC 2.0 request
#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    #[allow(dead_code)]
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Option<Value>,
}

/// JSON-RPC 2.0 response
#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

/// JSON-RPC 2.0 error
#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i32,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}
