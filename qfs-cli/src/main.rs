//! QFS CLI - Quick File Search command-line interface

mod mcp;

use anyhow::Result;
use clap::{Parser, Subcommand};
use qfs::{Indexer, SearchMode, SearchOptions, Store};
use std::path::{Path, PathBuf};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

#[derive(Parser)]
#[command(name = "qfs")]
#[command(
    author,
    version,
    about = "Quick File Search - Universal local file search engine"
)]
#[command(propagate_version = true)]
struct Cli {
    /// Database path (default: ~/.cache/qfs/index.sqlite)
    #[arg(long, short = 'd', env = "QFS_DB_PATH")]
    database: Option<PathBuf>,

    /// Enable verbose output
    #[arg(long, short = 'v', global = true)]
    verbose: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a new database
    Init,

    /// Add a collection to index
    Add {
        /// Collection name
        name: String,

        /// Path to the directory
        path: PathBuf,

        /// Glob patterns to include (default: all files)
        #[arg(long, short = 'p')]
        patterns: Vec<String>,

        /// Glob patterns to exclude
        #[arg(long, short = 'e')]
        exclude: Vec<String>,
    },

    /// Remove a collection
    Remove {
        /// Collection name
        name: String,
    },

    /// List all collections
    List,

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

    /// Index documents in a collection or all collections
    Index {
        /// Collection name (index all if not specified)
        name: Option<String>,
    },

    /// Generate embeddings for documents
    Embed {
        /// Collection name (embed all if not specified)
        name: Option<String>,

        /// Force re-embedding of all documents
        #[arg(long, short = 'f')]
        force: bool,

        /// Embedding model (default, minilm, bge)
        #[arg(long, short = 'm', default_value = "default")]
        model: String,

        /// Chunk size in words
        #[arg(long, default_value = "256")]
        chunk_size: usize,

        /// Chunk overlap in words
        #[arg(long, default_value = "32")]
        overlap: usize,
    },

    /// Search for documents
    Search {
        /// Search query
        query: String,

        /// Search mode (bm25, vector, hybrid)
        #[arg(long, short = 'm', default_value = "bm25")]
        mode: String,

        /// Maximum number of results
        #[arg(long, short = 'n', default_value = "20")]
        limit: usize,

        /// Minimum score threshold (0.0-1.0)
        #[arg(long, default_value = "0.0")]
        min_score: f64,

        /// Filter by collection
        #[arg(long, short = 'c')]
        collection: Option<String>,

        /// Filter documents modified on or after this date (ISO 8601, e.g., 2025-01-01)
        #[arg(long)]
        from_date: Option<String>,

        /// Filter documents modified on or before this date (ISO 8601, e.g., 2025-12-31)
        #[arg(long)]
        to_date: Option<String>,

        /// Include binary files in results
        #[arg(long)]
        include_binary: bool,

        /// Output format (text, json)
        #[arg(long, short = 'o', default_value = "text")]
        format: String,
    },

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

    /// Show database status and statistics
    Status,

    /// Start MCP server (stdio transport)
    Serve,

    /// Manage context descriptions for collections and paths
    Context {
        #[command(subcommand)]
        action: ContextAction,
    },
}

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

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize logging
    let filter = if cli.verbose {
        EnvFilter::new("debug")
    } else {
        EnvFilter::new("info")
    };

    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(filter)
        .init();

    // Get database path
    let db_path = cli.database.unwrap_or_else(qfs::default_db_path);

    // Ensure parent directory exists
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    match cli.command {
        Commands::Init => cmd_init(&db_path).await,
        Commands::Add {
            name,
            path,
            patterns,
            exclude,
        } => cmd_add(&db_path, &name, &path, &patterns, &exclude).await,
        Commands::Remove { name } => cmd_remove(&db_path, &name).await,
        Commands::List => cmd_list(&db_path).await,
        Commands::Ls { path, format } => cmd_ls(&db_path, path.as_deref(), &format).await,
        Commands::Index { name } => cmd_index(&db_path, name.as_deref()).await,
        Commands::Embed {
            name,
            force,
            model,
            chunk_size,
            overlap,
        } => cmd_embed(&db_path, name.as_deref(), force, &model, chunk_size, overlap).await,
        Commands::Search {
            query,
            mode,
            limit,
            min_score,
            collection,
            from_date,
            to_date,
            include_binary,
            format,
        } => {
            cmd_search(
                &db_path,
                &query,
                &mode,
                limit,
                min_score,
                collection.as_deref(),
                from_date.as_deref(),
                to_date.as_deref(),
                include_binary,
                &format,
            )
            .await
        }
        Commands::Get {
            path,
            from,
            max_lines,
            line_numbers,
            format,
        } => cmd_get(&db_path, &path, from, max_lines, line_numbers, &format).await,
        Commands::MultiGet {
            pattern,
            max_bytes,
            max_lines,
            format,
        } => cmd_multi_get(&db_path, &pattern, max_bytes, max_lines, &format).await,
        Commands::Status => cmd_status(&db_path).await,
        Commands::Serve => cmd_serve(&db_path).await,
        Commands::Context { action } => cmd_context(&db_path, action).await,
    }
}

async fn cmd_init(db_path: &Path) -> Result<()> {
    println!("Initializing QFS database at: {}", db_path.display());
    let _store = Store::open(db_path).await?;
    println!("Database initialized successfully.");
    Ok(())
}

async fn cmd_add(
    db_path: &Path,
    name: &str,
    path: &Path,
    patterns: &[String],
    exclude: &[String],
) -> Result<()> {
    let store = Store::open(db_path).await?;

    let pattern_refs: Vec<&str> = if patterns.is_empty() {
        vec!["**/*"]
    } else {
        patterns.iter().map(|s| s.as_str()).collect()
    };

    let path_str = path.to_string_lossy();
    store.add_collection(name, &path_str, &pattern_refs).await?;

    // Store exclude patterns (would need schema update)
    let _ = exclude; // TODO: implement exclude patterns in schema

    println!("Added collection '{}' at {}", name, path.display());
    Ok(())
}

async fn cmd_remove(db_path: &Path, name: &str) -> Result<()> {
    let store = Store::open(db_path).await?;
    store.remove_collection(name).await?;
    println!("Removed collection '{}'", name);
    Ok(())
}

async fn cmd_list(db_path: &Path) -> Result<()> {
    let store = Store::open(db_path).await?;
    let collections = store.list_collections().await?;

    if collections.is_empty() {
        println!("No collections found. Use 'qfs add' to add a collection.");
        return Ok(());
    }

    println!("Collections:");
    for col in collections {
        let doc_count = store.count_documents(Some(&col.name)).await.unwrap_or(0);
        println!(
            "  {} ({} documents)\n    Path: {}",
            col.name, doc_count, col.path
        );
    }
    Ok(())
}

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
    use chrono::{DateTime, Local};

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

/// Parse ls path argument into (collection, optional_path_prefix)
fn parse_ls_path(path: &str) -> (String, Option<String>) {
    // Handle qfs:// prefix
    let clean = if let Some(stripped) = path.strip_prefix("qfs://") {
        stripped
    } else if let Some(stripped) = path.strip_prefix("//") {
        stripped
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

async fn cmd_ls(db_path: &Path, path: Option<&str>, format: &str) -> Result<()> {
    let store = Store::open(db_path).await?;

    match path {
        None => {
            // List all collections
            let collections = store.list_collections().await?;

            if collections.is_empty() {
                println!("No collections found. Use 'qfs add' to add a collection.");
                return Ok(());
            }

            if format == "json" {
                let mut data = Vec::new();
                for c in &collections {
                    let count = store.count_documents(Some(&c.name)).await.unwrap_or(0);
                    data.push(serde_json::json!({
                        "name": c.name,
                        "path": c.path,
                        "documents": count,
                    }));
                }
                println!("{}", serde_json::to_string_pretty(&data)?);
            } else {
                println!("Collections:\n");
                for col in collections {
                    let doc_count = store.count_documents(Some(&col.name)).await.unwrap_or(0);
                    println!("  qfs://{}/  ({} files)", col.name, doc_count);
                }
            }
        }
        Some(path_arg) => {
            // Parse the path argument
            let (collection_name, path_prefix) = parse_ls_path(path_arg);

            // Verify collection exists
            if store.get_collection(&collection_name).await.is_err() {
                anyhow::bail!(
                    "Collection not found: {}\nRun 'qfs ls' to see available collections.",
                    collection_name
                );
            }

            let files = store
                .list_files(&collection_name, path_prefix.as_deref())
                .await?;

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
                let max_size_width = files
                    .iter()
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

async fn cmd_index(db_path: &Path, name: Option<&str>) -> Result<()> {
    let store = Store::open(db_path).await?;
    let indexer = Indexer::new(&store);

    let stats = if let Some(collection_name) = name {
        println!("Indexing collection '{}'...", collection_name);
        indexer.index_collection(collection_name).await?
    } else {
        println!("Indexing all collections...");
        indexer.index_all().await?
    };

    println!(
        "Indexing complete:\n  Files scanned: {}\n  Files indexed: {}\n  Files skipped: {}\n  Errors: {}\n  Duration: {:?}",
        stats.files_scanned,
        stats.files_indexed,
        stats.files_skipped,
        stats.errors,
        stats.duration
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn cmd_embed(
    db_path: &Path,
    collection: Option<&str>,
    force: bool,
    model: &str,
    chunk_size: usize,
    overlap: usize,
) -> Result<()> {
    use qfs_embed::{chunk_text, embedding_to_bytes, EmbedConfig, Embedder, Model};
    use std::io::Write;

    let store = Store::open(db_path).await?;

    // Initialize embedder (downloads model if needed)
    println!("Initializing embedding model...");
    let model: Model = model.parse().map_err(|e| anyhow::anyhow!("{}", e))?;
    let config = EmbedConfig {
        model,
        show_download_progress: true,
        ..Default::default()
    };
    let embedder = Embedder::with_config(config).map_err(|e| anyhow::anyhow!("{}", e))?;
    println!(
        "Using model: {} ({} dimensions)",
        embedder.model_name(),
        embedder.dimensions()
    );

    // Get documents to embed
    let documents = if let Some(coll) = collection {
        println!("Embedding collection '{}'...", coll);
        store.list_documents(coll).await?
    } else {
        println!("Embedding all collections...");
        store.list_all_documents().await?
    };

    let total = documents.len();
    if total == 0 {
        println!("No documents to embed.");
        return Ok(());
    }

    let mut embedded = 0;
    let mut skipped = 0;
    let mut errors = 0;
    let mut total_chunks = 0;

    for (i, doc) in documents.iter().enumerate() {
        // Skip if already has embeddings (unless force)
        if !force && store.has_embeddings(&doc.hash).await? {
            skipped += 1;
            continue;
        }

        // Delete existing embeddings if force
        if force {
            store.delete_embeddings(&doc.hash).await?;
        }

        // Get document content
        let content = match store.get_content(&doc.hash).await {
            Ok(c) => c,
            Err(e) => {
                errors += 1;
                tracing::warn!("Failed to get content for {}: {}", doc.path, e);
                continue;
            }
        };

        let text = match String::from_utf8(content.data) {
            Ok(t) => t,
            Err(_) => {
                skipped += 1; // Binary file
                continue;
            }
        };

        if text.trim().is_empty() {
            skipped += 1;
            continue;
        }

        // Chunk the document
        let chunks = chunk_text(&text, chunk_size, overlap);
        if chunks.is_empty() {
            skipped += 1;
            continue;
        }

        // Generate embeddings
        let texts: Vec<&str> = chunks.iter().map(|c| c.text.as_str()).collect();
        let embeddings = match embedder.embed(&texts) {
            Ok(e) => e,
            Err(e) => {
                errors += 1;
                tracing::warn!("Failed to embed {}: {}", doc.path, e);
                continue;
            }
        };

        // Store embeddings
        for (chunk, embedding) in chunks.iter().zip(embeddings.iter()) {
            let bytes = embedding_to_bytes(embedding);
            store
                .insert_embedding(
                    &doc.hash,
                    chunk.index as i32,
                    chunk.char_offset as i32,
                    embedder.model_name(),
                    &bytes,
                )
                .await?;
        }

        embedded += 1;
        total_chunks += chunks.len();

        // Progress update
        print!(
            "\rProgress: {}/{} documents ({} embedded, {} skipped)",
            i + 1,
            total,
            embedded,
            skipped
        );
        std::io::stdout().flush().ok();
    }
    println!();

    // Ensure vector index exists for efficient search
    if embedded > 0 {
        print!("Creating vector index...");
        std::io::stdout().flush().ok();
        match store.ensure_vector_index().await {
            Ok(true) => println!(" done"),
            Ok(false) => println!(" skipped (already exists or no embeddings)"),
            Err(e) => println!(" warning: {}", e),
        }
    }

    println!("\nEmbedding complete:");
    println!("  Documents embedded: {}", embedded);
    println!("  Documents skipped: {}", skipped);
    println!("  Total chunks: {}", total_chunks);
    println!("  Errors: {}", errors);

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn cmd_search(
    db_path: &Path,
    query: &str,
    mode: &str,
    limit: usize,
    min_score: f64,
    collection: Option<&str>,
    from_date: Option<&str>,
    to_date: Option<&str>,
    include_binary: bool,
    format: &str,
) -> Result<()> {
    let store = Store::open(db_path).await?;

    let search_mode: SearchMode = mode.parse()?;
    let options = SearchOptions {
        mode: search_mode.clone(),
        limit,
        min_score,
        collection: collection.map(String::from),
        include_binary,
        from_date: from_date.map(String::from),
        to_date: to_date.map(String::from),
    };

    let searcher = qfs::search::Searcher::new(&store);

    // For vector/hybrid modes, we need to embed the query first
    let results = match search_mode {
        SearchMode::Bm25 => searcher.search(query, options).await?,
        SearchMode::Vector => {
            // Check if embeddings exist
            let embed_count = store.count_embeddings(collection).await?;
            if embed_count == 0 {
                anyhow::bail!(
                    "No embeddings found. Run 'qfs embed' first to enable vector search."
                );
            }

            // Initialize embedder and embed query
            let embedder =
                qfs_embed::Embedder::new().map_err(|e| anyhow::anyhow!("Embedder error: {}", e))?;
            let query_embedding = embedder
                .embed_one(query)
                .map_err(|e| anyhow::anyhow!("Embedding error: {}", e))?;

            searcher
                .search_vector_with_embedding(&query_embedding, &options)
                .await?
        }
        SearchMode::Hybrid => {
            // Check if embeddings exist
            let embed_count = store.count_embeddings(collection).await?;
            if embed_count == 0 {
                anyhow::bail!(
                    "No embeddings found. Run 'qfs embed' first to enable hybrid search."
                );
            }

            // Initialize embedder and embed query
            let embedder =
                qfs_embed::Embedder::new().map_err(|e| anyhow::anyhow!("Embedder error: {}", e))?;
            let query_embedding = embedder
                .embed_one(query)
                .map_err(|e| anyhow::anyhow!("Embedding error: {}", e))?;

            searcher
                .search_hybrid_with_embedding(query, &query_embedding, &options)
                .await?
        }
    };

    if format == "json" {
        println!("{}", serde_json::to_string_pretty(&results)?);
    } else {
        if results.is_empty() {
            println!("No results found for '{}'", query);
            return Ok(());
        }

        println!("Found {} results for '{}':\n", results.len(), query);
        for (i, result) in results.iter().enumerate() {
            println!("{}. {} (score: {:.3})", i + 1, result.path, result.score);
            if let Some(ref snippet) = result.snippet {
                println!("   {}", snippet.replace('\n', "\n   "));
            }
            println!();
        }
    }

    Ok(())
}

async fn cmd_get(
    db_path: &Path,
    path: &str,
    from_line: Option<usize>,
    max_lines: Option<usize>,
    line_numbers: bool,
    format: &str,
) -> Result<()> {
    let store = Store::open(db_path).await?;

    // Parse :linenum suffix if --from not provided
    let (clean_path, suffix_line) = qfs::parse_path_with_line(path);
    let effective_from = from_line.or(suffix_line);

    // Check if input is a docid
    let doc = if qfs::store::is_docid(clean_path) {
        store.get_document_by_docid(clean_path).await?
    } else {
        // Parse path as collection/relative_path
        let parts: Vec<&str> = clean_path.splitn(2, '/').collect();
        if parts.len() != 2 {
            anyhow::bail!("Path must be in format 'collection/relative_path' or docid");
        }
        store.get_document(parts[0], parts[1]).await?
    };

    // Get content
    let content = store.get_content(&doc.hash).await?;
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

async fn cmd_multi_get(
    db_path: &Path,
    pattern: &str,
    max_bytes: usize,
    max_lines: Option<usize>,
    format: &str,
) -> Result<()> {
    let store = Store::open(db_path).await?;
    let results = store.multi_get(pattern, max_bytes, max_lines).await?;

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

async fn cmd_status(db_path: &Path) -> Result<()> {
    if !db_path.exists() {
        println!("Database not initialized. Run 'qfs init' first.");
        return Ok(());
    }

    let store = Store::open(db_path).await?;
    let collections = store.list_collections().await?;
    let total_docs = store.count_documents(None).await?;
    let docs_with_embeddings = store.count_embeddings(None).await?;

    println!("QFS Status");
    println!("===========");
    println!("Database: {}", db_path.display());
    println!("Collections: {}", collections.len());
    println!("Total documents: {}", total_docs);
    println!(
        "Documents with embeddings: {}/{}",
        docs_with_embeddings, total_docs
    );

    if docs_with_embeddings < total_docs && total_docs > 0 {
        println!("  (run 'qfs embed' to generate missing embeddings)");
    }

    if !collections.is_empty() {
        println!("\nPer-collection stats:");
        for col in &collections {
            let count = store.count_documents(Some(&col.name)).await.unwrap_or(0);
            let embedded = store.count_embeddings(Some(&col.name)).await.unwrap_or(0);
            println!(
                "  {}: {} documents ({} embedded)",
                col.name, count, embedded
            );
        }
    }

    // Check vector index status
    let has_index = store.has_vector_index().await;
    println!(
        "\nVector search: {}",
        if has_index {
            "ready"
        } else {
            "not indexed"
        }
    );

    Ok(())
}

async fn cmd_serve(db_path: &Path) -> Result<()> {
    let server = mcp::McpServer::new(db_path).await?;
    Ok(server.run().await?)
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
    let clean = if let Some(stripped) = path.strip_prefix("qfs://") {
        stripped
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

async fn cmd_context(db_path: &Path, action: ContextAction) -> Result<()> {
    let store = Store::open(db_path).await?;

    match action {
        ContextAction::Add { path, description } => {
            let (collection, path_prefix) = parse_context_path(&path);
            store
                .set_context(collection.as_deref(), &path_prefix, &description)
                .await?;

            if let Some(coll) = collection {
                println!("Added context for {}/{}", coll, path_prefix);
            } else {
                println!("Added global context");
            }
        }

        ContextAction::List => {
            let contexts = store.list_contexts().await?;

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
            let without_context = store.get_collections_without_context().await?;

            if without_context.is_empty() {
                println!("All collections have context defined.");
            } else {
                println!("Collections without context:\n");
                for coll in without_context {
                    let doc_count = store.count_documents(Some(&coll.name)).await.unwrap_or(0);
                    println!("  {} ({} files)", coll.name, doc_count);
                    println!(
                        "    Suggested: qfs context add {} \"Description here\"",
                        coll.name
                    );
                }
            }
        }

        ContextAction::Rm { path } => {
            let (collection, path_prefix) = parse_context_path(&path);
            if store
                .remove_context(collection.as_deref(), &path_prefix)
                .await?
            {
                println!("Removed context for {}", path);
            } else {
                println!("No context found for {}", path);
            }
        }
    }

    Ok(())
}

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
    fn test_parse_context_path_global() {
        assert_eq!(parse_context_path("/"), (None, "/".to_string()));
    }

    #[test]
    fn test_parse_context_path_collection_only() {
        assert_eq!(
            parse_context_path("docs"),
            (Some("docs".to_string()), "/".to_string())
        );
    }

    #[test]
    fn test_parse_context_path_with_prefix() {
        assert_eq!(
            parse_context_path("docs/api"),
            (Some("docs".to_string()), "/api".to_string())
        );
    }

    #[test]
    fn test_parse_context_path_virtual() {
        assert_eq!(
            parse_context_path("qfs://docs/api/v2"),
            (Some("docs".to_string()), "/api/v2".to_string())
        );
    }
}
