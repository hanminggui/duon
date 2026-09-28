use crate::backend::{ModelBackend, ModelInvocationConfig};
use crate::context::ContextBuilder;
use crate::error::ExtractorError;
use crate::evidence::{EvidenceBinder, EvidenceItem};
use duon_core::canonical::compute_result_hash;
use duon_core::ir::DocumentIR;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyRequestInternal {
    pub data: Value,
    pub schema: Option<Value>,
    pub model_config: ModelInvocationConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VerifiedField {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_value: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document_value: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff_reason: Option<String>,
    #[serde(default)]
    pub evidence: Vec<EvidenceItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VerifySummary {
    pub total_checked: usize,
    pub matched_count: usize,
    pub conflict_count: usize,
    pub missing_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyMeta {
    pub document_sha256: String,
    pub result_hash: String,
    pub model: String,
    pub execution_time_ms: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyResponse {
    pub status: String,
    pub summary: VerifySummary,
    pub fields: HashMap<String, VerifiedField>,
    pub meta: VerifyMeta,
}

pub struct VerifyEngine<B: ModelBackend> {
    backend: Arc<B>,
    pipeline_version: String,
}

impl<B: ModelBackend> VerifyEngine<B> {
    pub fn new(backend: B) -> Self {
        Self {
            backend: Arc::new(backend),
            pipeline_version: "1.0.0".to_string(),
        }
    }

    pub async fn verify(
        &self,
        doc_ir: &DocumentIR,
        req: &VerifyRequestInternal,
    ) -> Result<VerifyResponse, ExtractorError> {
        let start_time = Instant::now();
        let doc_context = ContextBuilder::build(doc_ir);

        let system_prompt = r#"You are Duon Verification Engine.
Given known system data fields and the document context, verify each field by finding the ground truth value in the document.
For each field, output its document_value and cite the item IDs (e.g. "p1_i0001") supporting it under citations.
Output format MUST be valid JSON with this exact envelope:
{
  "fields": {
    "/field_path": {
      "document_value": ...,
      "citations": ["p1_i0001"]
    }
  }
}
If a field is not found in the document, set document_value to null and citations to [].
"#;

        let user_prompt = format!(
            "Fields to verify (from system database):\n{}\n\nDocument Context:\n{}",
            serde_json::to_string_pretty(&req.data).unwrap_or_default(),
            doc_context
        );

        let raw_llm_output = self
            .backend
            .generate(system_prompt, &user_prompt, &req.model_config)
            .await?;

        let parsed: Value = serde_json::from_str(&raw_llm_output)
            .map_err(|e| ExtractorError::InvalidJson(e, raw_llm_output.clone()))?;

        let llm_fields = parsed
            .get("fields")
            .and_then(|f| f.as_object())
            .cloned()
            .unwrap_or_default();

        let mut citations_map: HashMap<String, Vec<String>> = HashMap::new();
        for (field_key, field_obj) in &llm_fields {
            if let Some(cits) = field_obj.get("citations").and_then(|c| c.as_array()) {
                let ids: Vec<String> = cits.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect();
                citations_map.insert(field_key.clone(), ids);
            }
        }

        let (bound_evidence, _val_errors) = EvidenceBinder::bind(doc_ir, &citations_map);

        let mut verified_fields = HashMap::new();
        let mut matched_count = 0;
        let mut conflict_count = 0;
        let mut missing_count = 0;

        // Iterate through requested fields in system data
        let system_map = match &req.data {
            Value::Object(map) => map.clone(),
            _ => serde_json::Map::new(),
        };

        for (k, sys_val) in system_map {
            let pointer = if k.starts_with('/') {
                k.clone()
            } else {
                format!("/{}", k)
            };

            let llm_field_data = llm_fields.get(&pointer).or_else(|| llm_fields.get(&k));
            let doc_val = llm_field_data.and_then(|f| f.get("document_value")).cloned();
            let ev_list = bound_evidence.get(&pointer).cloned().unwrap_or_default();

            let (status, diff_reason) = match &doc_val {
                None | Some(Value::Null) => {
                    missing_count += 1;
                    ("missing_in_doc".to_string(), Some("Field not found in document".to_string()))
                }
                Some(doc_v) => {
                    if values_match(&sys_val, doc_v) {
                        matched_count += 1;
                        ("matched".to_string(), None)
                    } else {
                        conflict_count += 1;
                        (
                            "conflict".to_string(),
                            Some(format!(
                                "Discrepancy: system recorded {}, but document states {}",
                                sys_val, doc_v
                            )),
                        )
                    }
                }
            };

            verified_fields.insert(
                pointer,
                VerifiedField {
                    status,
                    system_value: Some(sys_val),
                    document_value: doc_val,
                    diff_reason,
                    evidence: ev_list,
                },
            );
        }

        let total_checked = verified_fields.len();
        let overall_status = if conflict_count > 0 {
            "conflict".to_string()
        } else if matched_count == total_checked && total_checked > 0 {
            "matched".to_string()
        } else {
            "partial_match".to_string()
        };

        let summary = VerifySummary {
            total_checked,
            matched_count,
            conflict_count,
            missing_count,
        };

        let fields_val = serde_json::to_value(&verified_fields).unwrap_or(json!({}));
        let summary_val = serde_json::to_value(&summary).unwrap_or(json!({}));
        let result_hash = compute_result_hash(
            &fields_val,
            &summary_val,
            &self.pipeline_version,
            &req.model_config.model,
        );

        let execution_time_ms = start_time.elapsed().as_millis();

        Ok(VerifyResponse {
            status: overall_status,
            summary,
            fields: verified_fields,
            meta: VerifyMeta {
                document_sha256: doc_ir.source.sha256.clone(),
                result_hash,
                model: req.model_config.model.clone(),
                execution_time_ms,
            },
        })
    }
}

fn values_match(a: &Value, b: &Value) -> bool {
    if a == b {
        return true;
    }

    // Number fuzzy equality (e.g. 1600 vs 1600.0)
    if let (Some(num_a), Some(num_b)) = (a.as_f64(), b.as_f64()) {
        return (num_a - num_b).abs() < 1e-4;
    }

    // String trimmed equality
    if let (Some(s_a), Some(s_b)) = (a.as_str(), b.as_str()) {
        return s_a.trim() == s_b.trim();
    }

    false
}
