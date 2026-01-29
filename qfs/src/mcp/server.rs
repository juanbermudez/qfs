//! MCP server implementation
//!
//! Implements the stdio transport for the Model Context Protocol.

use crate::store::Store;
use super::protocol::{
    JsonRpcError, JsonRpcRequest, JsonRpcResponse, MCP_PROTOCOL_VERSION,
    ServerCapabilities, ServerInfo, ToolResult,
};
use super::tools::{get_tool_definitions, handle_tool_call};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

/// MCP server for QFS
///
/// Implements the Model Context Protocol over stdio, exposing QFS
/// search and retrieval functionality to AI agents.
pub struct McpServer {
    store: Store,
}

impl McpServer {
    /// Create a new MCP server with a database path
    pub fn new<P: AsRef<Path>>(db_path: P) -> crate::Result<Self> {
        let store = Store::open(db_path)?;
        Ok(Self { store })
    }

    /// Create a new MCP server with an existing store
    pub fn with_store(store: Store) -> Self {
        Self { store }
    }

    /// Run the MCP server on stdio
    ///
    /// This method blocks and handles requests until EOF is received
    /// or an error occurs.
    pub fn run(&self) -> crate::Result<()> {
        let stdin = std::io::stdin();
        let stdout = std::io::stdout();
        let mut reader = BufReader::new(stdin.lock());
        let mut writer = stdout.lock();

        tracing::info!("QFS MCP server started (protocol version {})", MCP_PROTOCOL_VERSION);

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
                            let response = JsonRpcResponse::error(
                                None,
                                JsonRpcError::parse_error(format!("Parse error: {}", e)),
                            );
                            let response_json = serde_json::to_string(&response)?;
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

    /// Handle a single JSON-RPC request
    fn handle_request(&self, request: JsonRpcRequest) -> JsonRpcResponse {
        let result = match request.method.as_str() {
            "initialize" => self.handle_initialize(&request.params),
            "notifications/initialized" => {
                // Client notification, acknowledge with empty result
                return JsonRpcResponse::success(request.id, json!({}));
            }
            "tools/list" => self.handle_tools_list(),
            "tools/call" => self.handle_tools_call(&request.params),
            "ping" => Ok(json!({})),
            _ => Err(JsonRpcError::method_not_found(&request.method)),
        };

        match result {
            Ok(value) => JsonRpcResponse::success(request.id, value),
            Err(error) => JsonRpcResponse::error(request.id, error),
        }
    }

    /// Handle initialize request
    fn handle_initialize(&self, _params: &Option<Value>) -> std::result::Result<Value, JsonRpcError> {
        let capabilities = ServerCapabilities::default();
        let server_info = ServerInfo::default();

        Ok(json!({
            "protocolVersion": MCP_PROTOCOL_VERSION,
            "capabilities": capabilities,
            "serverInfo": server_info
        }))
    }

    /// Handle tools/list request
    fn handle_tools_list(&self) -> std::result::Result<Value, JsonRpcError> {
        let tools = get_tool_definitions();
        Ok(json!({ "tools": tools }))
    }

    /// Handle tools/call request
    fn handle_tools_call(&self, params: &Option<Value>) -> std::result::Result<Value, JsonRpcError> {
        let params = params
            .as_ref()
            .ok_or_else(|| JsonRpcError::invalid_params("Missing params"))?;

        let tool_name = params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| JsonRpcError::invalid_params("Missing tool name"))?;

        let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

        let result: ToolResult = handle_tool_call(&self.store, tool_name, &arguments)?;

        serde_json::to_value(result)
            .map_err(|e| JsonRpcError::server_error(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_server() -> McpServer {
        let store = Store::open_memory().unwrap();
        McpServer::with_store(store)
    }

    #[test]
    fn test_server_creation() {
        let server = create_test_server();
        // Server should be created successfully
        assert!(server.handle_initialize(&None).is_ok());
    }

    #[test]
    fn test_initialize_response() {
        let server = create_test_server();
        let result = server.handle_initialize(&None).unwrap();

        assert_eq!(result["protocolVersion"], MCP_PROTOCOL_VERSION);
        assert!(result["capabilities"].is_object());
        assert!(result["serverInfo"].is_object());
        assert_eq!(result["serverInfo"]["name"], "qfs");
    }

    #[test]
    fn test_tools_list() {
        let server = create_test_server();
        let result = server.handle_tools_list().unwrap();

        let tools = result["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 6);

        // Verify tool names
        let names: Vec<&str> = tools
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"qfs_search"));
        assert!(names.contains(&"qfs_status"));
    }

    #[test]
    fn test_tools_call_status() {
        let server = create_test_server();
        let params = json!({
            "name": "qfs_status",
            "arguments": {}
        });

        let result = server.handle_tools_call(&Some(params)).unwrap();
        assert!(result["content"].is_array());
    }

    #[test]
    fn test_tools_call_search() {
        let server = create_test_server();
        let params = json!({
            "name": "qfs_search",
            "arguments": {
                "query": "test",
                "limit": 10
            }
        });

        let result = server.handle_tools_call(&Some(params)).unwrap();
        assert!(result["content"].is_array());
    }

    #[test]
    fn test_tools_call_missing_name() {
        let server = create_test_server();
        let params = json!({
            "arguments": {}
        });

        let result = server.handle_tools_call(&Some(params));
        assert!(result.is_err());
    }

    #[test]
    fn test_unknown_method() {
        let server = create_test_server();
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(1)),
            method: "unknown/method".to_string(),
            params: None,
        };

        let response = server.handle_request(request);
        assert!(response.error.is_some());
        assert_eq!(response.error.unwrap().code, -32601);
    }

    #[test]
    fn test_ping() {
        let server = create_test_server();
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(1)),
            method: "ping".to_string(),
            params: None,
        };

        let response = server.handle_request(request);
        assert!(response.result.is_some());
        assert!(response.error.is_none());
    }

    #[test]
    fn test_notification_initialized() {
        let server = create_test_server();
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(1)),
            method: "notifications/initialized".to_string(),
            params: None,
        };

        let response = server.handle_request(request);
        assert!(response.result.is_some());
        assert!(response.error.is_none());
    }
}
