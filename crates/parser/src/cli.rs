use crate::adapter::LiteParseAdapter;
use crate::error::ParserError;
use crate::traits::{DocumentParser, ParseOptions};
use async_trait::async_trait;
use duon_core::ir::DocumentIR;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;

/// Real DocumentParser implementation using system-installed `lit` CLI (LiteParse).
pub struct LiteParseCliParser {
    binary_path: String,
}

impl Default for LiteParseCliParser {
    fn default() -> Self {
        Self::new()
    }
}

impl LiteParseCliParser {
    pub fn new() -> Self {
        let binary_path = std::env::var("LIT_CLI_PATH")
            .unwrap_or_else(|_| "lit".to_string());
        Self { binary_path }
    }

    pub fn with_binary_path<S: Into<String>>(binary_path: S) -> Self {
        Self {
            binary_path: binary_path.into(),
        }
    }

    /// Checks if lit command is executable and available on the host system.
    pub async fn is_available(&self) -> bool {
        Command::new(&self.binary_path)
            .arg("--version")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map(|out| out.status.success())
            .unwrap_or(false)
    }

    /// Obtains version string from `lit --version`.
    pub async fn get_version(&self) -> String {
        Command::new(&self.binary_path)
            .arg("--version")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .ok()
            .and_then(|out| {
                if out.status.success() {
                    String::from_utf8(out.stdout).ok().map(|s| s.trim().to_string())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| "2.0.0".to_string())
    }
}

#[async_trait]
impl DocumentParser for LiteParseCliParser {
    async fn parse(
        &self,
        bytes: &[u8],
        options: &ParseOptions,
    ) -> Result<DocumentIR, ParserError> {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        let sha256_hex = format!("{:x}", hasher.finalize());

        // Determine target file path:
        // If options.filename is an existing local regular file, use it directly.
        // Otherwise, create an ephemeral temporary file.
        let (file_path, _ephemeral_guard) = if let Some(ref fname) = options.filename {
            let p = Path::new(fname);
            if p.is_file() {
                (p.to_path_buf(), None)
            } else {
                let temp = create_temp_file(bytes, Some(fname))?;
                let path = temp.path().to_path_buf();
                (path, Some(temp))
            }
        } else {
            let temp = create_temp_file(bytes, None)?;
            let path = temp.path().to_path_buf();
            (path, Some(temp))
        };

        let mut cmd = Command::new(&self.binary_path);
        cmd.arg("parse")
            .arg(&file_path)
            .arg("--format")
            .arg("json")
            .arg("-q");

        if !options.ocr_enabled {
            cmd.arg("--no-ocr");
        }

        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let output = cmd.output().await.map_err(|e| {
            ParserError::Execution(format!(
                "Failed to spawn lit process '{}': {}",
                self.binary_path, e
            ))
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ParserError::Execution(format!(
                "lit parse failed (exit code {:?}): {}",
                output.status.code(),
                stderr.trim()
            )));
        }

        let raw_json: serde_json::Value = serde_json::from_slice(&output.stdout).map_err(|e| {
            ParserError::CorruptedDocument(format!("Failed to parse lit JSON output: {}", e))
        })?;

        let version = self.get_version().await;
        let mut doc_ir = LiteParseAdapter::convert(
            &raw_json,
            &sha256_hex,
            options.mime_type.as_deref().unwrap_or("application/pdf"),
            options.filename.as_deref(),
            &version,
            "default_config",
        )?;

        doc_ir.source.size_bytes = Some(bytes.len() as u64);
        Ok(doc_ir)
    }
}

fn create_temp_file(
    bytes: &[u8],
    filename_hint: Option<&str>,
) -> Result<tempfile::NamedTempFile, ParserError> {
    let suffix = filename_hint
        .and_then(|f| Path::new(f).extension().and_then(|s| s.to_str()))
        .map(|ext| format!(".{}", ext))
        .unwrap_or_else(|| ".pdf".to_string());

    let mut temp = tempfile::Builder::new()
        .prefix("duon-lit-")
        .suffix(&suffix)
        .tempfile()
        .map_err(|e| ParserError::Execution(format!("Failed to create temp file: {}", e)))?;

    use std::io::Write;
    temp.write_all(bytes)
        .map_err(|e| ParserError::Execution(format!("Failed to write temp file: {}", e)))?;

    Ok(temp)
}
