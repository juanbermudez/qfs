//! Error types for QFS

use thiserror::Error;

/// QFS error type
#[derive(Error, Debug)]
pub enum Error {
    /// Database error
    #[error("Database error: {0}")]
    Database(#[from] libsql::Error),

    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Collection not found
    #[error("Collection not found: {0}")]
    CollectionNotFound(String),

    /// Document not found
    #[error("Document not found: {0}")]
    DocumentNotFound(String),

    /// Invalid query
    #[error("Invalid query: {0}")]
    InvalidQuery(String),

    /// Index error
    #[error("Index error: {0}")]
    IndexError(String),

    /// Parse error
    #[error("Parse error: {0}")]
    ParseError(String),

    /// Configuration error
    #[error("Configuration error: {0}")]
    ConfigError(String),

    /// Embedding error (when embeddings are enabled)
    #[error("Embedding error: {0}")]
    EmbeddingError(String),

    /// Vector search requires embeddings
    #[error("Vector search requires embeddings. Run 'qfs embed' first or use --mode bm25")]
    EmbeddingsRequired,

    /// Generic error
    #[error("{0}")]
    Other(String),
}

/// Result type alias for QFS operations
pub type Result<T> = std::result::Result<T, Error>;

impl From<glob::PatternError> for Error {
    fn from(err: glob::PatternError) -> Self {
        Error::ConfigError(format!("Invalid glob pattern: {}", err))
    }
}

impl From<walkdir::Error> for Error {
    fn from(err: walkdir::Error) -> Self {
        Error::Io(err.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = Error::CollectionNotFound("notes".to_string());
        assert_eq!(err.to_string(), "Collection not found: notes");
    }
}
