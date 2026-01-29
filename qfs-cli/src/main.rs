//! QFS CLI - Quick File Search command-line interface

mod mcp;

use anyhow::Result;
use clap::{Parser, Subcommand};
use qfs::{Indexer, SearchMode, SearchOptions, Store};
use std::path::PathBuf;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

#[derive(Parser)]
#[command(name = "qfs")]
#[command(author, version, about = "Quick File Search - Universal local file search engine")]
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

    /// Index documents in a collection or all collections
    Index {
        /// Collection name (index all if not specified)
        name: Option<String>,
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

        /// Include binary files in results
        #[arg(long)]
        include_binary: bool,

        /// Output format (text, json)
        #[arg(long, short = 'o', default_value = "text")]
        format: String,
    },

    /// Get a document by path
    Get {
        /// Document path (collection/relative_path)
        path: String,

        /// Output format (text, json)
        #[arg(long, short = 'o', default_value = "text")]
        format: String,
    },

    /// Show database status and statistics
    Status,

    /// Start MCP server (stdio transport)
    Serve,
}

fn main() -> Result<()> {
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
        Commands::Init => cmd_init(&db_path),
        Commands::Add {
            name,
            path,
            patterns,
            exclude,
        } => cmd_add(&db_path, &name, &path, &patterns, &exclude),
        Commands::Remove { name } => cmd_remove(&db_path, &name),
        Commands::List => cmd_list(&db_path),
        Commands::Index { name } => cmd_index(&db_path, name.as_deref()),
        Commands::Search {
            query,
            mode,
            limit,
            min_score,
            collection,
            include_binary,
            format,
        } => cmd_search(
            &db_path,
            &query,
            &mode,
            limit,
            min_score,
            collection.as_deref(),
            include_binary,
            &format,
        ),
        Commands::Get { path, format } => cmd_get(&db_path, &path, &format),
        Commands::Status => cmd_status(&db_path),
        Commands::Serve => cmd_serve(&db_path),
    }
}

fn cmd_init(db_path: &PathBuf) -> Result<()> {
    println!("Initializing QFS database at: {}", db_path.display());
    let _store = Store::open(db_path)?;
    println!("Database initialized successfully.");
    Ok(())
}

fn cmd_add(
    db_path: &PathBuf,
    name: &str,
    path: &PathBuf,
    patterns: &[String],
    exclude: &[String],
) -> Result<()> {
    let store = Store::open(db_path)?;

    let pattern_refs: Vec<&str> = if patterns.is_empty() {
        vec!["**/*"]
    } else {
        patterns.iter().map(|s| s.as_str()).collect()
    };

    let path_str = path.to_string_lossy();
    store.add_collection(name, &path_str, &pattern_refs)?;

    // Store exclude patterns (would need schema update)
    let _ = exclude; // TODO: implement exclude patterns in schema

    println!("Added collection '{}' at {}", name, path.display());
    Ok(())
}

fn cmd_remove(db_path: &PathBuf, name: &str) -> Result<()> {
    let store = Store::open(db_path)?;
    store.remove_collection(name)?;
    println!("Removed collection '{}'", name);
    Ok(())
}

fn cmd_list(db_path: &PathBuf) -> Result<()> {
    let store = Store::open(db_path)?;
    let collections = store.list_collections()?;

    if collections.is_empty() {
        println!("No collections found. Use 'qfs add' to add a collection.");
        return Ok(());
    }

    println!("Collections:");
    for col in collections {
        let doc_count = store.count_documents(Some(&col.name)).unwrap_or(0);
        println!(
            "  {} ({} documents)\n    Path: {}",
            col.name, doc_count, col.path
        );
    }
    Ok(())
}

fn cmd_index(db_path: &PathBuf, name: Option<&str>) -> Result<()> {
    let store = Store::open(db_path)?;
    let indexer = Indexer::new(&store);

    let stats = if let Some(collection_name) = name {
        println!("Indexing collection '{}'...", collection_name);
        indexer.index_collection(collection_name)?
    } else {
        println!("Indexing all collections...");
        indexer.index_all()?
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

fn cmd_search(
    db_path: &PathBuf,
    query: &str,
    mode: &str,
    limit: usize,
    min_score: f64,
    collection: Option<&str>,
    include_binary: bool,
    format: &str,
) -> Result<()> {
    let store = Store::open(db_path)?;

    let search_mode: SearchMode = mode.parse()?;
    let options = SearchOptions {
        mode: search_mode,
        limit,
        min_score,
        collection: collection.map(String::from),
        include_binary,
    };

    let searcher = qfs::search::Searcher::new(&store);
    let results = searcher.search(query, options)?;

    if format == "json" {
        println!("{}", serde_json::to_string_pretty(&results)?);
    } else {
        if results.is_empty() {
            println!("No results found for '{}'", query);
            return Ok(());
        }

        println!("Found {} results for '{}':\n", results.len(), query);
        for (i, result) in results.iter().enumerate() {
            println!(
                "{}. {} (score: {:.3})",
                i + 1,
                result.path,
                result.score
            );
            if let Some(ref snippet) = result.snippet {
                println!("   {}", snippet.replace('\n', "\n   "));
            }
            println!();
        }
    }

    Ok(())
}

fn cmd_get(db_path: &PathBuf, path: &str, format: &str) -> Result<()> {
    let store = Store::open(db_path)?;

    // Parse path as collection/relative_path
    let parts: Vec<&str> = path.splitn(2, '/').collect();
    if parts.len() != 2 {
        anyhow::bail!("Path must be in format 'collection/relative_path'");
    }

    let doc = store.get_document(parts[0], parts[1])?;

    if format == "json" {
        println!("{}", serde_json::to_string_pretty(&doc)?);
    } else {
        println!("Path: {}/{}", doc.collection, doc.path);
        if let Some(ref title) = doc.title {
            println!("Title: {}", title);
        }
        println!("Type: {}", doc.file_type);
        println!("Hash: {}", doc.hash);

        // TODO: Get content from content table
    }

    Ok(())
}

fn cmd_status(db_path: &PathBuf) -> Result<()> {
    if !db_path.exists() {
        println!("Database not initialized. Run 'qfs init' first.");
        return Ok(());
    }

    let store = Store::open(db_path)?;
    let collections = store.list_collections()?;
    let total_docs = store.count_documents(None)?;

    println!("QFS Status");
    println!("===========");
    println!("Database: {}", db_path.display());
    println!("Collections: {}", collections.len());
    println!("Total documents: {}", total_docs);

    if !collections.is_empty() {
        println!("\nPer-collection stats:");
        for col in collections {
            let count = store.count_documents(Some(&col.name)).unwrap_or(0);
            println!("  {}: {} documents", col.name, count);
        }
    }

    Ok(())
}

fn cmd_serve(db_path: &PathBuf) -> Result<()> {
    let server = mcp::McpServer::new(db_path)?;
    Ok(server.run()?)
}
