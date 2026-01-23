//! # QFS Embed - Embedding Generation for QFS
//!
//! This crate provides optional vector embedding generation for QFS
//! using fastembed (Rust-native embeddings).
//!
//! ## Models
//!
//! - `all-MiniLM-L6-v2` (default, ~80MB, 384 dimensions)
//! - `bge-small-en-v1.5` (higher quality, ~130MB, 384 dimensions)
//!
//! ## Usage
//!
//! ```rust,ignore
//! use qfs_embed::Embedder;
//!
//! // Create embedder with default model
//! let embedder = Embedder::new()?;
//!
//! // Generate embeddings
//! let embeddings = embedder.embed(&["Hello world", "Search query"])?;
//!
//! // Chunk and embed a document
//! let chunks = qfs_embed::chunk_text("Long document...", 256, 32);
//! let chunk_embeddings = embedder.embed(&chunks.iter().map(|c| c.text.as_str()).collect::<Vec<_>>())?;
//! ```

use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use std::sync::Arc;
use thiserror::Error;

/// Embedding error types
#[derive(Error, Debug)]
pub enum EmbedError {
    /// Model not found or failed to load
    #[error("Model error: {0}")]
    ModelError(String),

    /// Embedding generation failed
    #[error("Embedding failed: {0}")]
    EmbeddingFailed(String),

    /// I/O error
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),
}

/// Result type for embedding operations
pub type Result<T> = std::result::Result<T, EmbedError>;

/// Supported embedding models
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Model {
    /// all-MiniLM-L6-v2 - Fast and small (~80MB, 384 dimensions)
    #[default]
    AllMiniLmL6V2,
    /// BGE Small EN v1.5 - Higher quality (~130MB, 384 dimensions)
    BgeSmallEnV1_5,
}

impl Model {
    /// Get the fastembed model enum
    fn to_fastembed(&self) -> EmbeddingModel {
        match self {
            Model::AllMiniLmL6V2 => EmbeddingModel::AllMiniLML6V2,
            Model::BgeSmallEnV1_5 => EmbeddingModel::BGESmallENV15,
        }
    }

    /// Get embedding dimensions for this model
    pub fn dimensions(&self) -> usize {
        match self {
            Model::AllMiniLmL6V2 => 384,
            Model::BgeSmallEnV1_5 => 384,
        }
    }

    /// Get model name
    pub fn name(&self) -> &'static str {
        match self {
            Model::AllMiniLmL6V2 => "all-MiniLM-L6-v2",
            Model::BgeSmallEnV1_5 => "bge-small-en-v1.5",
        }
    }
}

impl std::str::FromStr for Model {
    type Err = EmbedError;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "all-minilm-l6-v2" | "minilm" | "default" => Ok(Model::AllMiniLmL6V2),
            "bge-small-en-v1.5" | "bge-small" | "bge" => Ok(Model::BgeSmallEnV1_5),
            _ => Err(EmbedError::ModelError(format!("Unknown model: {}", s))),
        }
    }
}

/// Embedding model configuration
#[derive(Debug, Clone)]
pub struct EmbedConfig {
    /// Model to use
    pub model: Model,
    /// Cache directory for model files
    pub cache_dir: Option<std::path::PathBuf>,
    /// Show download progress
    pub show_download_progress: bool,
}

impl Default for EmbedConfig {
    fn default() -> Self {
        Self {
            model: Model::default(),
            cache_dir: None,
            show_download_progress: true,
        }
    }
}

/// Embedder for generating text embeddings
pub struct Embedder {
    model: Arc<TextEmbedding>,
    config: EmbedConfig,
}

impl Embedder {
    /// Create a new embedder with default configuration
    pub fn new() -> Result<Self> {
        Self::with_config(EmbedConfig::default())
    }

    /// Create a new embedder with custom configuration
    pub fn with_config(config: EmbedConfig) -> Result<Self> {
        tracing::info!("Initializing embedder with model: {}", config.model.name());

        let mut init_options = InitOptions::new(config.model.to_fastembed())
            .with_show_download_progress(config.show_download_progress);

        if let Some(ref cache_dir) = config.cache_dir {
            init_options = init_options.with_cache_dir(cache_dir.clone());
        }

        let model = TextEmbedding::try_new(init_options)
            .map_err(|e| EmbedError::ModelError(e.to_string()))?;

        tracing::info!("Embedder initialized successfully");
        Ok(Self {
            model: Arc::new(model),
            config,
        })
    }

    /// Get embedding dimensions for the current model
    pub fn dimensions(&self) -> usize {
        self.config.model.dimensions()
    }

    /// Get the model name
    pub fn model_name(&self) -> &'static str {
        self.config.model.name()
    }

    /// Generate embeddings for a batch of texts
    ///
    /// Returns a Vec of embedding vectors, one per input text.
    pub fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let texts_owned: Vec<String> = texts.iter().map(|s| s.to_string()).collect();

        self.model
            .embed(texts_owned, None)
            .map_err(|e| EmbedError::EmbeddingFailed(e.to_string()))
    }

    /// Generate embedding for a single text
    pub fn embed_one(&self, text: &str) -> Result<Vec<f32>> {
        let results = self.embed(&[text])?;
        Ok(results.into_iter().next().unwrap_or_default())
    }

    /// Generate embeddings for a batch of texts, returning with indices
    ///
    /// This is useful when you need to track which embedding corresponds to which input.
    pub fn embed_with_indices(&self, texts: &[&str]) -> Result<Vec<(usize, Vec<f32>)>> {
        let embeddings = self.embed(texts)?;
        Ok(embeddings.into_iter().enumerate().collect())
    }
}

/// A chunk of text with position information
#[derive(Debug, Clone)]
pub struct TextChunk {
    /// The chunk text
    pub text: String,
    /// Character offset in original document
    pub char_offset: usize,
    /// Chunk index (0-based)
    pub index: usize,
}

/// Chunk text into overlapping segments
///
/// Uses a simple word-based chunking strategy:
/// - `chunk_size`: Target number of words per chunk
/// - `overlap`: Number of words to overlap between chunks
///
/// Returns a Vec of TextChunks with position information.
pub fn chunk_text(text: &str, chunk_size: usize, overlap: usize) -> Vec<TextChunk> {
    if text.is_empty() || chunk_size == 0 {
        return Vec::new();
    }

    let words: Vec<(usize, &str)> = text
        .split_whitespace()
        .scan(0usize, |pos, word| {
            let start = text[*pos..].find(word).map(|i| *pos + i).unwrap_or(*pos);
            *pos = start + word.len();
            Some((start, word))
        })
        .collect();

    if words.is_empty() {
        return Vec::new();
    }

    let mut chunks = Vec::new();
    let step = chunk_size.saturating_sub(overlap).max(1);
    let mut chunk_index = 0;

    let mut i = 0;
    while i < words.len() {
        let end = (i + chunk_size).min(words.len());
        let chunk_words: Vec<&str> = words[i..end].iter().map(|(_, w)| *w).collect();
        let char_offset = words[i].0;

        chunks.push(TextChunk {
            text: chunk_words.join(" "),
            char_offset,
            index: chunk_index,
        });

        chunk_index += 1;

        if end >= words.len() {
            break;
        }

        i += step;
    }

    chunks
}

/// Serialize embedding to bytes for SQLite storage
pub fn embedding_to_bytes(embedding: &[f32]) -> Vec<u8> {
    embedding
        .iter()
        .flat_map(|f| f.to_le_bytes())
        .collect()
}

/// Deserialize embedding from bytes
pub fn bytes_to_embedding(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|chunk| {
            let arr: [u8; 4] = chunk.try_into().unwrap();
            f32::from_le_bytes(arr)
        })
        .collect()
}

/// Calculate cosine similarity between two embeddings
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }

    dot / (norm_a * norm_b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunk_text_basic() {
        let text = "one two three four five six seven eight nine ten";
        let chunks = chunk_text(text, 4, 1);

        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].text, "one two three four");
        assert_eq!(chunks[0].index, 0);
        assert_eq!(chunks[1].text, "four five six seven");
        assert_eq!(chunks[1].index, 1);
        assert_eq!(chunks[2].text, "seven eight nine ten");
        assert_eq!(chunks[2].index, 2);
    }

    #[test]
    fn test_chunk_text_small() {
        let text = "hello world";
        let chunks = chunk_text(text, 10, 2);

        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].text, "hello world");
    }

    #[test]
    fn test_chunk_text_empty() {
        let chunks = chunk_text("", 10, 2);
        assert!(chunks.is_empty());

        let chunks = chunk_text("hello", 0, 0);
        assert!(chunks.is_empty());
    }

    #[test]
    fn test_embedding_serialization() {
        let embedding = vec![1.0f32, 2.0, -3.5, 0.0, 100.123];
        let bytes = embedding_to_bytes(&embedding);
        let restored = bytes_to_embedding(&bytes);

        assert_eq!(embedding.len(), restored.len());
        for (a, b) in embedding.iter().zip(restored.iter()) {
            assert!((a - b).abs() < 1e-6);
        }
    }

    #[test]
    fn test_cosine_similarity_identical() {
        let a = vec![1.0, 2.0, 3.0];
        let sim = cosine_similarity(&a, &a);
        assert!((sim - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![0.0, 1.0, 0.0];
        let sim = cosine_similarity(&a, &b);
        assert!(sim.abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_opposite() {
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![-1.0, -2.0, -3.0];
        let sim = cosine_similarity(&a, &b);
        assert!((sim + 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_model_from_str() {
        assert_eq!("minilm".parse::<Model>().unwrap(), Model::AllMiniLmL6V2);
        assert_eq!("default".parse::<Model>().unwrap(), Model::AllMiniLmL6V2);
        assert_eq!("bge".parse::<Model>().unwrap(), Model::BgeSmallEnV1_5);
        assert!("invalid".parse::<Model>().is_err());
    }

    // Skip actual embedding tests in CI as they require model download
    // Run with: cargo test --features integration -- --ignored
    #[test]
    #[ignore]
    fn test_embedder_creation() {
        let embedder = Embedder::new().unwrap();
        assert_eq!(embedder.dimensions(), 384);
    }

    #[test]
    #[ignore]
    fn test_embed_texts() {
        let embedder = Embedder::new().unwrap();
        let texts = &["Hello world", "Test query"];
        let embeddings = embedder.embed(texts).unwrap();

        assert_eq!(embeddings.len(), 2);
        assert_eq!(embeddings[0].len(), 384);
        assert_eq!(embeddings[1].len(), 384);

        // Embeddings should be different for different texts
        let sim = cosine_similarity(&embeddings[0], &embeddings[1]);
        assert!(sim < 0.99); // Not identical
        assert!(sim > 0.0);  // But still somewhat similar (both English)
    }

    #[test]
    #[ignore]
    fn test_embed_similar_texts() {
        let embedder = Embedder::new().unwrap();
        let texts = &[
            "The quick brown fox jumps over the lazy dog",
            "A fast brown fox leaps over a sleepy dog",
            "Quantum physics is fascinating",
        ];
        let embeddings = embedder.embed(texts).unwrap();

        // First two should be more similar to each other than to the third
        let sim_01 = cosine_similarity(&embeddings[0], &embeddings[1]);
        let sim_02 = cosine_similarity(&embeddings[0], &embeddings[2]);
        let sim_12 = cosine_similarity(&embeddings[1], &embeddings[2]);

        assert!(sim_01 > sim_02);
        assert!(sim_01 > sim_12);
    }
}
