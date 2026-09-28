use crate::error::ParserError;
use async_trait::async_trait;
use duon_core::ir::DocumentIR;

#[derive(Debug, Clone)]
pub struct ParseOptions {
    pub ocr_enabled: bool,
    pub filename: Option<String>,
    pub mime_type: Option<String>,
}

impl Default for ParseOptions {
    fn default() -> Self {
        Self {
            ocr_enabled: true,
            filename: None,
            mime_type: None,
        }
    }
}

/// Seam for Document Parsers (LiteParse, Mock, Sandbox Worker).
/// Encapsulates all parsing, layout detection, and coordinates normalization.
#[async_trait]
pub trait DocumentParser: Send + Sync {
    async fn parse(
        &self,
        bytes: &[u8],
        options: &ParseOptions,
    ) -> Result<DocumentIR, ParserError>;
}
