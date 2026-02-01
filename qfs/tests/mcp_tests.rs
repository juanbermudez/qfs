//! Integration tests for MCP server functionality
//!
//! These tests verify the MCP protocol handling and tool execution.

use qfs::mcp::McpServer;
use qfs::{Indexer, Store};
use serde_json::json;
use std::fs::File;
use std::io::Write;
use tempfile::tempdir;

/// Create a test server with indexed documents
async fn create_test_server_with_docs() -> (Store, tempfile::TempDir, tempfile::TempDir) {
    let db_dir = tempdir().unwrap();
    let content_dir = tempdir().unwrap();

    // Create test documents
    let fixtures = [
        (
            "rust_guide.md",
            "# Rust Programming\n\nRust is a systems language.",
        ),
        (
            "python_basics.md",
            "# Python Basics\n\nPython is interpreted.",
        ),
    ];

    for (name, content) in fixtures {
        let path = content_dir.path().join(name);
        let mut file = File::create(&path).unwrap();
        file.write_all(content.as_bytes()).unwrap();
    }

    // Create store and index
    let store = Store::open(db_dir.path().join("test.sqlite"))
        .await
        .unwrap();
    store
        .add_collection("docs", content_dir.path().to_str().unwrap(), &["**/*.md"])
        .await
        .unwrap();

    let indexer = Indexer::new(&store);
    indexer.index_collection("docs").await.unwrap();

    (store, db_dir, content_dir)
}

#[tokio::test]
async fn test_mcp_server_creation() {
    let store = Store::open_memory().await.unwrap();
    let _server = McpServer::with_store(store);
    // Server creation should succeed (no panic)
}

#[tokio::test]
async fn test_mcp_tools_list() {
    // The tools list should contain 6 tools
    let tools = qfs::mcp::tools::get_tool_definitions();
    assert_eq!(tools.len(), 6);

    // Verify required tools
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&"qfs_search"));
    assert!(names.contains(&"qfs_vsearch"));
    assert!(names.contains(&"qfs_query"));
    assert!(names.contains(&"qfs_get"));
    assert!(names.contains(&"qfs_multi_get"));
    assert!(names.contains(&"qfs_status"));
}

#[tokio::test]
async fn test_mcp_search_tool_empty_results() {
    let store = Store::open_memory().await.unwrap();
    store
        .add_collection("test", "/tmp/test", &["**/*.md"])
        .await
        .unwrap();

    // Test search with empty results
    let result = qfs::mcp::tools::handle_tool_call(
        &store,
        "qfs_search",
        &json!({
            "query": "nonexistent",
            "limit": 10
        }),
    )
    .await;

    assert!(result.is_ok());
    let tool_result = result.unwrap();
    assert!(!tool_result.content.is_empty());
}

#[tokio::test]
async fn test_mcp_search_with_results() {
    let (store, _db_dir, _content_dir) = create_test_server_with_docs().await;

    let result = qfs::mcp::tools::handle_tool_call(
        &store,
        "qfs_search",
        &json!({
            "query": "rust",
            "limit": 10
        }),
    )
    .await;

    assert!(result.is_ok());
    let tool_result = result.unwrap();
    let text = &tool_result.content[0].text;

    // Should contain rust_guide in results
    assert!(text.contains("rust_guide"), "Should find rust_guide.md");
}

#[tokio::test]
async fn test_mcp_status_tool() {
    let store = Store::open_memory().await.unwrap();
    store
        .add_collection("test", "/tmp/test", &["**/*.md"])
        .await
        .unwrap();

    let result = qfs::mcp::tools::handle_tool_call(&store, "qfs_status", &json!({})).await;

    assert!(result.is_ok());
    let tool_result = result.unwrap();
    let text = &tool_result.content[0].text;

    // Should contain version and collection info
    assert!(text.contains("version"));
    assert!(text.contains("totalCollections"));
}

#[tokio::test]
async fn test_mcp_get_tool() {
    let db_dir = tempdir().unwrap();
    let content_dir = tempdir().unwrap();

    // Create a test file
    let file_path = content_dir.path().join("test.md");
    let mut file = File::create(&file_path).unwrap();
    file.write_all(b"# Test Document\n\nSome content here.")
        .unwrap();

    let store = Store::open(db_dir.path().join("test.sqlite"))
        .await
        .unwrap();
    store
        .add_collection("docs", content_dir.path().to_str().unwrap(), &["**/*.md"])
        .await
        .unwrap();

    let indexer = Indexer::new(&store);
    indexer.index_collection("docs").await.unwrap();

    // Test get tool
    let result = qfs::mcp::tools::handle_tool_call(
        &store,
        "qfs_get",
        &json!({
            "path": "docs/test.md",
            "include_content": true
        }),
    )
    .await;

    assert!(result.is_ok());
    let tool_result = result.unwrap();
    let text = &tool_result.content[0].text;

    // Should contain the document content
    assert!(text.contains("Test Document"));
}

#[tokio::test]
async fn test_mcp_get_invalid_path() {
    let store = Store::open_memory().await.unwrap();

    let result = qfs::mcp::tools::handle_tool_call(
        &store,
        "qfs_get",
        &json!({
            "path": "invalid-no-slash"
        }),
    )
    .await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.message.contains("format"));
}

#[tokio::test]
async fn test_mcp_multi_get_tool() {
    let db_dir = tempdir().unwrap();
    let content_dir = tempdir().unwrap();

    // Create test files
    let file1 = content_dir.path().join("doc1.md");
    let mut f1 = File::create(&file1).unwrap();
    f1.write_all(b"# Document 1").unwrap();

    let file2 = content_dir.path().join("doc2.md");
    let mut f2 = File::create(&file2).unwrap();
    f2.write_all(b"# Document 2").unwrap();

    let store = Store::open(db_dir.path().join("test.sqlite"))
        .await
        .unwrap();
    store
        .add_collection("docs", content_dir.path().to_str().unwrap(), &["**/*.md"])
        .await
        .unwrap();

    let indexer = Indexer::new(&store);
    indexer.index_collection("docs").await.unwrap();

    // Test multi_get tool with comma-separated pattern
    let result = qfs::mcp::tools::handle_tool_call(
        &store,
        "qfs_multi_get",
        &json!({
            "pattern": "docs/doc1.md, docs/doc2.md",
            "max_bytes": 10240
        }),
    )
    .await;

    assert!(result.is_ok());
    let tool_result = result.unwrap();
    let text = &tool_result.content[0].text;

    // Should contain both documents
    assert!(text.contains("Document 1"));
    assert!(text.contains("Document 2"));
}

#[tokio::test]
async fn test_mcp_query_tool_with_mode() {
    let store = Store::open_memory().await.unwrap();
    store
        .add_collection("test", "/tmp/test", &["**/*.md"])
        .await
        .unwrap();

    // Test query with bm25 mode
    let result = qfs::mcp::tools::handle_tool_call(
        &store,
        "qfs_query",
        &json!({
            "query": "test",
            "mode": "bm25",
            "limit": 10
        }),
    )
    .await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_mcp_unknown_tool() {
    let store = Store::open_memory().await.unwrap();

    let result = qfs::mcp::tools::handle_tool_call(&store, "unknown_tool", &json!({})).await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.code, -32602);
    assert!(err.message.contains("Unknown tool"));
}

#[tokio::test]
async fn test_mcp_missing_required_param() {
    let store = Store::open_memory().await.unwrap();

    // qfs_search requires "query" param
    let result = qfs::mcp::tools::handle_tool_call(
        &store,
        "qfs_search",
        &json!({"limit": 10}), // missing query
    )
    .await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.message.contains("Missing query"));
}

#[tokio::test]
async fn test_mcp_protocol_types() {
    use qfs::mcp::{JsonRpcError, JsonRpcResponse};

    // Test error creation
    let err = JsonRpcError::new(-32000, "Test error");
    assert_eq!(err.code, -32000);
    assert_eq!(err.message, "Test error");

    // Test response creation
    let success = JsonRpcResponse::success(Some(json!(1)), json!({"result": "ok"}));
    assert!(success.result.is_some());
    assert!(success.error.is_none());

    let error = JsonRpcResponse::error(Some(json!(1)), err);
    assert!(error.result.is_none());
    assert!(error.error.is_some());
}

#[tokio::test]
async fn test_mcp_vsearch_requires_embeddings() {
    let store = Store::open_memory().await.unwrap();
    store
        .add_collection("test", "/tmp/test", &["**/*.md"])
        .await
        .unwrap();

    // Vector search should fail without embeddings
    let result = qfs::mcp::tools::handle_tool_call(
        &store,
        "qfs_vsearch",
        &json!({
            "query": "test",
            "limit": 10
        }),
    )
    .await;

    // Should return an error about embeddings
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.message.contains("embeddings") || err.message.contains("Embedding"));
}

#[tokio::test]
async fn test_mcp_search_with_collection_filter() {
    let (store, _db_dir, _content_dir) = create_test_server_with_docs().await;

    // Search in the docs collection
    let result = qfs::mcp::tools::handle_tool_call(
        &store,
        "qfs_search",
        &json!({
            "query": "python",
            "collection": "docs",
            "limit": 10
        }),
    )
    .await;

    assert!(result.is_ok());
    let tool_result = result.unwrap();
    let text = &tool_result.content[0].text;

    // Should find python_basics
    assert!(text.contains("python"));
}

#[tokio::test]
async fn test_mcp_tool_definitions_schema() {
    let tools = qfs::mcp::tools::get_tool_definitions();

    for tool in &tools {
        // Each tool should have a valid input schema
        assert!(
            tool.input_schema.is_object(),
            "{} should have object schema",
            tool.name
        );

        // Schema should have "type": "object"
        assert_eq!(
            tool.input_schema["type"], "object",
            "{} schema should have type object",
            tool.name
        );

        // Schema should have properties
        assert!(
            tool.input_schema.get("properties").is_some(),
            "{} should have properties",
            tool.name
        );
    }
}
