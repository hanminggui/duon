use crate::error::ParserError;
use crate::traits::{DocumentParser, ParseOptions};
use async_trait::async_trait;
use duon_core::ir::DocumentIR;
use sha2::{Digest, Sha256};

pub struct MockParser {
    template_ir: DocumentIR,
}

impl MockParser {
    pub fn new(template_ir: DocumentIR) -> Self {
        Self { template_ir }
    }
}

#[async_trait]
impl DocumentParser for MockParser {
    async fn parse(
        &self,
        bytes: &[u8],
        options: &ParseOptions,
    ) -> Result<DocumentIR, ParserError> {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        let sha256_hex = format!("{:x}", hasher.finalize());

        let mut ir = self.template_ir.clone();
        ir.document_id = format!("doc_{}", sha256_hex);
        ir.source.sha256 = sha256_hex;
        if let Some(ref filename) = options.filename {
            ir.source.filename = Some(filename.clone());
        }
        if let Some(ref mime) = options.mime_type {
            ir.source.mime_type = mime.clone();
        }
        ir.source.size_bytes = Some(bytes.len() as u64);

        Ok(ir)
    }
}
