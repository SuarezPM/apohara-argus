//! Error types for ARGUS.

use thiserror::Error;

pub type Result<T> = std::result::Result<T, ArgusError>;

#[derive(Error, Debug)]
pub enum ArgusError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Database error: {0}")]
    Database(String),

    #[error("LLM error: {0}")]
    Llm(String),

    #[error("GitHub API error: {0}")]
    GitHub(String),

    #[error("Crypto error: {0}")]
    Crypto(String),

    #[error("Prompt not found: {0}")]
    PromptNotFound(String),

    #[error("Invalid input: {0}")]
    InvalidInput(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

impl From<sqlx::Error> for ArgusError {
    fn from(e: sqlx::Error) -> Self {
        Self::Database(e.to_string())
    }
}
