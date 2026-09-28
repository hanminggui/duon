use thiserror::Error;

#[derive(Error, Debug)]
pub enum ExtractorError {
    #[error("HTTP error communicating with LLM backend: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Failed to parse JSON response from LLM: {0}, raw response: {1}")]
    InvalidJson(serde_json::Error, String),

    #[error("Backend execution error: {0}")]
    Backend(String),

    #[error("Schema compilation error: {0}")]
    SchemaCompilation(String),

    #[error("Core error: {0}")]
    Core(#[from] duon_core::error::CoreError),
}
