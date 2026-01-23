//! File scanner for discovering files to index

use crate::error::Result;
use glob::Pattern;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use walkdir::WalkDir;

/// Result of scanning a file
#[derive(Debug, Clone)]
pub struct ScanResult {
    /// Absolute path to the file
    pub path: PathBuf,
    /// Path relative to the collection root
    pub relative_path: String,
    /// File modification time
    pub modified: SystemTime,
    /// File size in bytes
    pub size: u64,
}

/// File scanner for discovering files matching patterns
pub struct Scanner {
    /// Root directory to scan
    root: PathBuf,
    /// Include patterns (glob)
    patterns: Vec<Pattern>,
    /// Exclude patterns (glob)
    exclude: Vec<Pattern>,
}

impl Scanner {
    /// Create a new scanner
    pub fn new<P: AsRef<Path>>(root: P, patterns: &[&str], exclude: &[&str]) -> Result<Self> {
        let root = root.as_ref().to_path_buf();

        let patterns = patterns
            .iter()
            .map(|p| Pattern::new(p))
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let exclude = exclude
            .iter()
            .map(|p| Pattern::new(p))
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(Scanner { root, patterns, exclude })
    }

    /// Scan for all matching files
    pub fn scan(&self) -> impl Iterator<Item = ScanResult> + '_ {
        WalkDir::new(&self.root)
            .follow_links(true)
            .into_iter()
            .filter_entry(|e| !self.is_excluded(e.path()))
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .filter(|e| self.matches(e.path()))
            .filter_map(|e| {
                let metadata = e.metadata().ok()?;
                let modified = metadata.modified().ok()?;
                let relative_path = e.path()
                    .strip_prefix(&self.root)
                    .ok()?
                    .to_string_lossy()
                    .to_string();

                Some(ScanResult {
                    path: e.path().to_path_buf(),
                    relative_path,
                    modified,
                    size: metadata.len(),
                })
            })
    }

    /// Scan for files modified since a given time (incremental scan)
    pub fn scan_since(&self, since: SystemTime) -> impl Iterator<Item = ScanResult> + '_ {
        self.scan().filter(move |r| r.modified > since)
    }

    /// Check if a path matches any include pattern
    fn matches(&self, path: &Path) -> bool {
        if self.patterns.is_empty() {
            return true;
        }

        let relative = path
            .strip_prefix(&self.root)
            .map(|p| p.to_string_lossy())
            .unwrap_or_default();

        // Also get the filename for simple patterns like "*.md"
        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");

        // Normalize path separators for cross-platform compatibility
        let relative_normalized = relative.replace('\\', "/");

        // Use case-insensitive matching with ** support
        let options = glob::MatchOptions {
            case_sensitive: false,
            require_literal_separator: false,
            require_literal_leading_dot: false,
        };

        self.patterns.iter().any(|p| {
            let pattern_str = p.as_str();

            // For patterns starting with **/, match against path or any suffix
            if pattern_str.starts_with("**/") {
                // Get the suffix pattern after **/
                let suffix = &pattern_str[3..];
                if let Ok(suffix_pattern) = Pattern::new(suffix) {
                    // Match if filename matches the suffix pattern
                    if suffix_pattern.matches_with(filename, options) {
                        return true;
                    }
                    // Or if any path component matches
                    if suffix_pattern.matches_with(&relative_normalized, options) {
                        return true;
                    }
                }
            }

            // Try matching against full relative path
            p.matches_with(&relative_normalized, options) ||
            // Try matching just the filename
            p.matches_with(filename, options)
        })
    }

    /// Check if a path should be excluded
    fn is_excluded(&self, path: &Path) -> bool {
        // Never exclude the root directory itself
        if path == self.root {
            return false;
        }

        let relative = path
            .strip_prefix(&self.root)
            .map(|p| p.to_string_lossy())
            .unwrap_or_default();

        // Always exclude hidden files and common non-content directories
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.starts_with('.') && name != "." && name != ".." {
            return true;
        }

        // Common directories to exclude
        const EXCLUDED_DIRS: &[&str] = &[
            "node_modules",
            "target",
            ".git",
            ".hg",
            ".svn",
            "__pycache__",
            ".venv",
            "venv",
            "dist",
            "build",
            ".next",
            ".nuxt",
        ];

        if path.is_dir() && EXCLUDED_DIRS.contains(&name) {
            return true;
        }

        // Check user-defined exclude patterns
        self.exclude.iter().any(|p| p.matches(&relative))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_scanner_basic() {
        let dir = tempdir().unwrap();

        // Create some test files
        fs::create_dir_all(dir.path().join("subdir")).unwrap();

        File::create(dir.path().join("file1.md"))
            .unwrap()
            .write_all(b"# Test 1")
            .unwrap();

        File::create(dir.path().join("file2.txt"))
            .unwrap()
            .write_all(b"Test 2")
            .unwrap();

        File::create(dir.path().join("subdir/file3.md"))
            .unwrap()
            .write_all(b"# Test 3")
            .unwrap();

        // Scan for markdown files
        let scanner = Scanner::new(dir.path(), &["**/*.md"], &[]).unwrap();
        let results: Vec<_> = scanner.scan().collect();

        assert_eq!(results.len(), 2);
        assert!(results.iter().any(|r| r.relative_path == "file1.md"));
        assert!(results.iter().any(|r| r.relative_path.ends_with("file3.md")));
    }

    #[test]
    fn test_scanner_exclude() {
        let dir = tempdir().unwrap();

        // Create files including in excluded directory
        fs::create_dir_all(dir.path().join("node_modules")).unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();

        File::create(dir.path().join("src/main.rs"))
            .unwrap()
            .write_all(b"fn main() {}")
            .unwrap();

        File::create(dir.path().join("node_modules/package.json"))
            .unwrap()
            .write_all(b"{}")
            .unwrap();

        // Scan all files
        let scanner = Scanner::new(dir.path(), &["**/*"], &[]).unwrap();
        let results: Vec<_> = scanner.scan().collect();

        // Should only find src/main.rs, not node_modules content
        assert_eq!(results.len(), 1);
        assert!(results[0].relative_path.contains("main.rs"));
    }

    #[test]
    fn test_scanner_multiple_patterns() {
        let dir = tempdir().unwrap();

        File::create(dir.path().join("readme.md"))
            .unwrap()
            .write_all(b"# Readme")
            .unwrap();

        File::create(dir.path().join("main.rs"))
            .unwrap()
            .write_all(b"fn main() {}")
            .unwrap();

        File::create(dir.path().join("data.json"))
            .unwrap()
            .write_all(b"{}")
            .unwrap();

        // Scan for multiple patterns
        let scanner = Scanner::new(dir.path(), &["*.md", "*.rs"], &[]).unwrap();
        let results: Vec<_> = scanner.scan().collect();

        assert_eq!(results.len(), 2);
    }
}
