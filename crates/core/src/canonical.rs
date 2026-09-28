use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use unicode_normalization::UnicodeNormalization;

/// Canonicalizes a serde_json::Value into a deterministic JSON string.
/// - Object keys are sorted lexicographically (using BTreeMap).
/// - Unicode strings are normalized to NFC.
/// - Floats are cleanly formatted (rounded to 2 decimal places if specified, or default standard).
/// - No whitespace between tokens.
pub fn canonicalize_json(val: &Value) -> String {
    match val {
        Value::Null => "null".to_string(),
        Value::Bool(b) => {
            if *b {
                "true".to_string()
            } else {
                "false".to_string()
            }
        }
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i.to_string()
            } else if let Some(u) = n.as_u64() {
                u.to_string()
            } else if let Some(f) = n.as_f64() {
                // Round float to 2 decimal places if it's near whole/clean, or print standard float
                format!("{:.2}", f)
                    .trim_end_matches('0')
                    .trim_end_matches('.')
                    .to_string()
            } else {
                n.to_string()
            }
        }
        Value::String(s) => {
            let nfc_str: String = s.nfc().collect();
            serde_json::to_string(&nfc_str).unwrap_or_else(|_| format!("\"{}\"", nfc_str))
        }
        Value::Array(arr) => {
            let items: Vec<String> = arr.iter().map(canonicalize_json).collect();
            format!("[{}]", items.join(","))
        }
        Value::Object(map) => {
            // Sort keys lexicographically via BTreeMap
            let sorted: BTreeMap<&String, &Value> = map.iter().collect();
            let mut entries = Vec::new();
            for (k, v) in sorted {
                let norm_k: String = k.nfc().collect();
                let k_str = serde_json::to_string(&norm_k).unwrap();
                entries.push(format!("{}:{}", k_str, canonicalize_json(v)));
            }
            format!("{{{}}}", entries.join(","))
        }
    }
}

/// Computes the deterministic SHA-256 result_hash for an extraction or verification result.
/// Formula: SHA256(canonical(data) + canonical(evidence) + pipeline_version + model)
pub fn compute_result_hash(
    data: &Value,
    evidence: &Value,
    pipeline_version: &str,
    model: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(canonicalize_json(data).as_bytes());
    hasher.update(canonicalize_json(evidence).as_bytes());
    hasher.update(pipeline_version.as_bytes());
    hasher.update(model.as_bytes());
    let result = hasher.finalize();
    format!("{:x}", result)
}
