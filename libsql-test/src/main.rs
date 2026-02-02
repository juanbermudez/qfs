//! libSQL Compatibility Test for QFS
//!
//! Tests critical SQLite features used by QFS:
//! - FTS5 with porter and unicode61 tokenizers
//! - bm25() scoring function
//! - snippet() function
//! - PRAGMA journal_mode=WAL
//! - ON CONFLICT (UPSERT)

use anyhow::Result;
use libsql::{params, Builder};

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== libSQL Compatibility Test for QFS ===\n");

    // Test 1: Create in-memory database
    println!("Test 1: Creating in-memory database...");
    let db = Builder::new_local(":memory:").build().await?;
    let conn = db.connect()?;
    println!("  ✓ Database created successfully\n");

    // Test 2: PRAGMA WAL mode
    println!("Test 2: Testing PRAGMA journal_mode=WAL...");
    match conn.execute("PRAGMA journal_mode=WAL;", ()).await {
        Ok(_) => println!("  ✓ WAL mode set (or accepted)\n"),
        Err(e) => println!("  ✗ WAL mode failed: {}\n", e),
    }

    // Test 3: Create standard table
    println!("Test 3: Creating standard table...");
    conn.execute(
        "CREATE TABLE IF NOT EXISTS documents (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            collection TEXT NOT NULL,
            path TEXT NOT NULL,
            title TEXT,
            body TEXT,
            hash TEXT NOT NULL,
            UNIQUE(collection, path)
        )",
        (),
    )
    .await?;
    println!("  ✓ Standard table created\n");

    // Test 4: FTS5 with porter tokenizer (CRITICAL)
    println!("Test 4: Creating FTS5 table with porter tokenizer...");
    match conn
        .execute(
            "CREATE VIRTUAL TABLE IF NOT EXISTS documents_fts USING fts5(
                filepath,
                title,
                body,
                tokenize='porter unicode61'
            )",
            (),
        )
        .await
    {
        Ok(_) => println!("  ✓ FTS5 with porter tokenizer created\n"),
        Err(e) => {
            println!("  ✗ FTS5 creation FAILED: {}", e);
            println!("  This is a CRITICAL failure - QFS search won't work!\n");

            // Try without porter tokenizer
            println!("  Trying FTS5 without porter tokenizer...");
            match conn
                .execute(
                    "CREATE VIRTUAL TABLE IF NOT EXISTS documents_fts_simple USING fts5(
                        filepath,
                        title,
                        body
                    )",
                    (),
                )
                .await
            {
                Ok(_) => println!("  ✓ Basic FTS5 works (but no porter stemming)\n"),
                Err(e2) => println!("  ✗ Basic FTS5 also failed: {}\n", e2),
            }
        }
    }

    // Test 5: Insert test data
    println!("Test 5: Inserting test documents...");
    conn.execute(
        "INSERT INTO documents (collection, path, title, body, hash) VALUES (?, ?, ?, ?, ?)",
        params![
            "docs",
            "rust_guide.md",
            "Rust Programming Guide",
            "Rust is a systems programming language focused on safety, speed, and concurrency.
             It achieves memory safety without garbage collection through its ownership system.",
            "hash1"
        ],
    )
    .await?;

    conn.execute(
        "INSERT INTO documents (collection, path, title, body, hash) VALUES (?, ?, ?, ?, ?)",
        params![
            "docs",
            "python_basics.md",
            "Python Basics",
            "Python is a high-level programming language known for its readability and simplicity.
             It supports multiple programming paradigms including procedural and object-oriented.",
            "hash2"
        ],
    )
    .await?;

    // Insert into FTS table
    conn.execute(
        "INSERT INTO documents_fts (rowid, filepath, title, body)
         SELECT id, path, title, body FROM documents",
        (),
    )
    .await?;
    println!("  ✓ Test documents inserted\n");

    // Test 6: FTS5 MATCH query
    println!("Test 6: Testing FTS5 MATCH query...");
    let mut rows = conn
        .query("SELECT filepath FROM documents_fts WHERE documents_fts MATCH ?", params!["rust"])
        .await?;

    let mut count = 0;
    while let Some(row) = rows.next().await? {
        let path: String = row.get(0)?;
        println!("  Found: {}", path);
        count += 1;
    }
    if count > 0 {
        println!("  ✓ MATCH query works ({} results)\n", count);
    } else {
        println!("  ✗ No results from MATCH query\n");
    }

    // Test 7: bm25() function (CRITICAL)
    println!("Test 7: Testing bm25() scoring function...");
    match conn
        .query(
            "SELECT filepath, bm25(documents_fts) as score
             FROM documents_fts
             WHERE documents_fts MATCH ?
             ORDER BY score",
            params!["programming"],
        )
        .await
    {
        Ok(mut rows) => {
            let mut found = false;
            while let Some(row) = rows.next().await? {
                let path: String = row.get(0)?;
                let score: f64 = row.get(1)?;
                println!("  {} (score: {:.4})", path, score);
                found = true;
            }
            if found {
                println!("  ✓ bm25() function works\n");
            } else {
                println!("  No results, but function executed\n");
            }
        }
        Err(e) => {
            println!("  ✗ bm25() FAILED: {}", e);
            println!("  This is a CRITICAL failure - QFS ranking won't work!\n");
        }
    }

    // Test 8: snippet() function (CRITICAL)
    println!("Test 8: Testing snippet() function...");
    match conn
        .query(
            "SELECT filepath, snippet(documents_fts, 2, '<mark>', '</mark>', '...', 64) as snip
             FROM documents_fts
             WHERE documents_fts MATCH ?",
            params!["programming"],
        )
        .await
    {
        Ok(mut rows) => {
            let mut found = false;
            while let Some(row) = rows.next().await? {
                let path: String = row.get(0)?;
                let snippet: String = row.get(1)?;
                println!("  {}: {}", path, snippet);
                found = true;
            }
            if found {
                println!("  ✓ snippet() function works\n");
            } else {
                println!("  No results, but function executed\n");
            }
        }
        Err(e) => {
            println!("  ✗ snippet() FAILED: {}", e);
            println!("  This will affect QFS search result previews!\n");
        }
    }

    // Test 9: ON CONFLICT (UPSERT)
    println!("Test 9: Testing ON CONFLICT (UPSERT)...");
    match conn
        .execute(
            "INSERT INTO documents (collection, path, title, body, hash)
             VALUES (?, ?, ?, ?, ?)
             ON CONFLICT(collection, path) DO UPDATE SET
                 title = excluded.title,
                 body = excluded.body,
                 hash = excluded.hash",
            params![
                "docs",
                "rust_guide.md",
                "Updated Rust Guide",
                "Updated content here",
                "hash1_updated"
            ],
        )
        .await
    {
        Ok(_) => println!("  ✓ ON CONFLICT (UPSERT) works\n"),
        Err(e) => println!("  ✗ UPSERT failed: {}\n", e),
    }

    // Test 10: Porter stemming verification
    println!("Test 10: Verifying porter stemming (running -> run)...");
    // Insert a doc with "running"
    conn.execute(
        "INSERT INTO documents (collection, path, title, body, hash) VALUES (?, ?, ?, ?, ?)",
        params!["docs", "running.md", "Running Tips", "Tips for running faster", "hash3"],
    )
    .await?;
    conn.execute(
        "INSERT INTO documents_fts (rowid, filepath, title, body)
         SELECT id, path, title, body FROM documents WHERE path = 'running.md'",
        (),
    )
    .await?;

    // Search for "run" should match "running" if porter stemmer works
    let mut rows = conn
        .query(
            "SELECT filepath FROM documents_fts WHERE documents_fts MATCH ?",
            params!["run"],
        )
        .await?;

    let mut found_running = false;
    while let Some(row) = rows.next().await? {
        let path: String = row.get(0)?;
        if path.contains("running") {
            found_running = true;
        }
    }

    if found_running {
        println!("  ✓ Porter stemming works ('run' matches 'running')\n");
    } else {
        println!("  ✗ Porter stemming NOT working ('run' didn't match 'running')");
        println!("  This may affect search quality!\n");
    }

    // Summary
    println!("=== Test Summary ===");
    println!("If all critical tests passed (FTS5, bm25, snippet), libSQL is viable for QFS.");
    println!("Note: API migration from rusqlite (sync) to libsql (async) still required.\n");

    Ok(())
}
