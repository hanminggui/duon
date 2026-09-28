use thiserror::Error;

#[derive(Error, Debug)]
pub enum ParserError {
    #[error("I/O error during document parsing: {0}")]
    Io(#[from] std::io::Error),

    #[error("Unsupported file format or MIME type: {0}")]
    UnsupportedFormat(String),

    #[error("Corrupted document: {0}")]
    CorruptedDocument(String),

    #[error("Password protected document: {0}")]
    PasswordProtected(String),

    #[error("Parser execution error: {0}")]
    Execution(String),

    #[error("Core IR validation failed: {0}")]
    Core(#[from] duon_core::error::CoreError),
}
