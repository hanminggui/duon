use crate::backend::{ModelBackend, ModelInvocationConfig};
use crate::context::ContextBuilder;
use crate::error::ExtractorError;
use crate::evidence::{EvidenceBinder, EvidenceItem, ValidationError};
use duon_core::canonical::compute_result_hash;
use duon_core::ir::DocumentIR;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractRequestInternal {
    pub schema: Option<Value>,
    pub model_config: ModelInvocationConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationSummary {
    pub is_valid: bool,
    pub errors: Vec<ValidationError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionMeta {
    pub document_sha256: String,
    pub pipeline_version: String,
    pub result_hash: String,
    pub model: String,
    pub execution_time_ms: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractResponse {
    pub data: Value,
    pub evidence: HashMap<String, Vec<EvidenceItem>>,
    pub validation: ValidationSummary,
    pub meta: ExtractionMeta,
}

pub struct ExtractEngine<B: ModelBackend> {
    backend: Arc<B>,
    pipeline_version: String,
}

impl<B: ModelBackend> ExtractEngine<B> {
    pub fn new(backend: B) -> Self {
        Self {
            backend: Arc::new(backend),
            pipeline_version: "1.0.0".to_string(),
        }
    }

    pub async fn extract(
        &self,
        doc_ir: &DocumentIR,
        req: &ExtractRequestInternal,
    ) -> Result<ExtractResponse, ExtractorError> {
        let start_time = Instant::now();

        // 1. Context text
        let doc_context = ContextBuilder::build(doc_ir);

        // 2. Prepare schema description
        let (schema_json, is_generic) = match &req.schema {
            Some(s) => (s.clone(), false),
            None => {
                // Fallback generic schema
                let generic_schema = json!({
                    "document_type": "string",
                    "title": "string",
                    "facts": [{"key": "string", "value": "any"}],
                    "entities": [{"text": "string", "type": "string"}],
                    "dates": [{"text": "string", "type": "string", "normalized": "string"}],
                    "amounts": [{"text": "string", "value": "number", "currency": "string"}]
                });
                (generic_schema, true)
            }
        };

        // 3. System & user prompts
        let system_prompt = r#"You are Duon Document Intelligence extraction engine.
Your task is to extract structured facts strictly adhering to the requested JSON schema from the provided document context.
For each extracted field, cite the supporting item IDs (e.g. "p1_i0001") from the document under "citations" using JSON pointer paths.
Output format MUST be valid JSON with this exact envelope:
{
  "data": { ...extracted fields strictly matching schema... },
  "citations": {
    "/field_path": ["p1_i0001", "p1_i0002"]
  }
}
Do not fabricate facts. If a field cannot be found, use null or omit it according to schema requirements.
"#;

        let user_prompt = format!(
            "Target Schema:\n{}\n\nDocument Context:\n{}",
            serde_json::to_string_pretty(&schema_json).unwrap_or_default(),
            doc_context
        );

        // 4. Invoke LLM backend
        let raw_llm_output = self
            .backend
            .generate(system_prompt, &user_prompt, &req.model_config)
            .await?;

        // 5. Parse LLM JSON
        let parsed_envelope: Value = serde_json::from_str(&raw_llm_output)
            .map_err(|e| ExtractorError::InvalidJson(e, raw_llm_output.clone()))?;

        let extracted_data = parsed_envelope
            .get("data")
            .cloned()
            .unwrap_or(Value::Object(serde_json::Map::new()));

        let raw_citations: HashMap<String, Vec<String>> = parsed_envelope
            .get("citations")
            .and_then(|c| serde_json::from_value(c.clone()).ok())
            .unwrap_or_default();

        // 6. Bind Evidence & filter hallucinated IDs
        let (bound_evidence, mut validation_errors) =
            EvidenceBinder::bind(doc_ir, &raw_citations);

        // 7. Validate extracted data against schema (if not generic)
        if !is_generic {
            if let Ok(validator) = jsonschema::validator_for(&schema_json) {
                for error in validator.iter_errors(&extracted_data) {
                    validation_errors.push(ValidationError {
                        path: error.instance_path.to_string(),
                        message: error.to_string(),
                        code: "SCHEMA_VALIDATION_ERROR".to_string(),
                    });
                }
            }
        }

        // 8. Compute deterministic result_hash
        let evidence_val = serde_json::to_value(&bound_evidence).unwrap_or(json!({}));
        let result_hash = compute_result_hash(
            &extracted_data,
            &evidence_val,
            &self.pipeline_version,
            &req.model_config.model,
        );

        let execution_time_ms = start_time.elapsed().as_millis();

        Ok(ExtractResponse {
            data: extracted_data,
            evidence: bound_evidence,
            validation: ValidationSummary {
                is_valid: validation_errors.is_empty(),
                errors: validation_errors,
            },
            meta: ExtractionMeta {
                document_sha256: doc_ir.source.sha256.clone(),
                pipeline_version: self.pipeline_version.clone(),
                result_hash,
                model: req.model_config.model.clone(),
                execution_time_ms,
            },
        })
    }
}
