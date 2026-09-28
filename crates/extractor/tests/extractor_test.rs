use duon_core::ir::DocumentIR;
use duon_extractor::backend::{MockModelBackend, ModelInvocationConfig};
use duon_extractor::engine::{ExtractEngine, ExtractRequestInternal};
use serde_json::json;
use std::fs;
use std::path::Path;

#[tokio::test]
async fn test_schema_extraction_end_to_end() {
    let mock_path = Path::new("../../specs/mocks/sample_order_ir.json");
    let content = fs::read_to_string(mock_path)
        .or_else(|_| fs::read_to_string("specs/mocks/sample_order_ir.json"))
        .expect("Failed to read sample_order_ir.json");

    let doc_ir: DocumentIR = serde_json::from_str(&content).expect("Invalid IR JSON");

    // The mock LLM will return this JSON when asked
    let mock_llm_output = json!({
        "data": {
            "party_a": "吾立方公司",
            "party_b": "宋泽昊",
            "amount": 1600,
            "order_date": "2026-05-18"
        },
        "citations": {
            "/party_a": ["p1_i0004"],
            "/party_b": ["p1_i0006"],
            "/amount": ["p1_i0008"],
            "/order_date": ["p1_i0007"]
        }
    });

    let backend = MockModelBackend::new(mock_llm_output.to_string());
    let engine = ExtractEngine::new(backend);

    let schema = json!({
        "type": "object",
        "required": ["party_a", "party_b", "amount", "order_date"],
        "properties": {
            "party_a": { "type": "string" },
            "party_b": { "type": "string" },
            "amount": { "type": "number" },
            "order_date": { "type": "string" }
        }
    });

    let req = ExtractRequestInternal {
        schema: Some(schema),
        model_config: ModelInvocationConfig {
            base_url: None,
            api_key: None,
            model: "Qwen3-8B".to_string(),
            temperature: 0.0,
            seed: 42,
        },
    };

    let resp = engine
        .extract(&doc_ir, &req)
        .await
        .expect("Extraction should succeed");

    // Assert data unpolluted
    assert_eq!(resp.data["party_a"], "吾立方公司");
    assert_eq!(resp.data["amount"], 1600);

    // Assert evidence bound accurately
    assert!(resp.evidence.contains_key("/party_a"));
    let party_a_evidence = &resp.evidence["/party_a"];
    assert_eq!(party_a_evidence.len(), 1);
    assert_eq!(party_a_evidence[0].page, 1);
    assert_eq!(party_a_evidence[0].item_ids, vec!["p1_i0004"]);
    assert_eq!(party_a_evidence[0].quote, "吾立方公司");
    assert_eq!(party_a_evidence[0].bbox.x0, 112.0);

    // Assert validation is valid
    assert!(resp.validation.is_valid);
    assert!(resp.validation.errors.is_empty());

    // Assert meta
    assert_eq!(resp.meta.document_sha256, doc_ir.source.sha256);
    assert_eq!(resp.meta.model, "Qwen3-8B");
    assert_eq!(resp.meta.result_hash.len(), 64);
}

#[tokio::test]
async fn test_evidence_binder_filters_hallucinated_ids() {
    let mock_path = Path::new("../../specs/mocks/sample_order_ir.json");
    let content = fs::read_to_string(mock_path)
        .or_else(|_| fs::read_to_string("specs/mocks/sample_order_ir.json"))
        .expect("Failed to read sample_order_ir.json");

    let doc_ir: DocumentIR = serde_json::from_str(&content).expect("Invalid IR JSON");

    // The mock LLM hallucinates an item ID "p1_i9999" that does not exist
    let mock_llm_output = json!({
        "data": {
            "party_a": "吾立方公司"
        },
        "citations": {
            "/party_a": ["p1_i9999"]
        }
    });

    let backend = MockModelBackend::new(mock_llm_output.to_string());
    let engine = ExtractEngine::new(backend);

    let req = ExtractRequestInternal {
        schema: Some(json!({
            "type": "object",
            "properties": { "party_a": { "type": "string" } }
        })),
        model_config: ModelInvocationConfig::default(),
    };

    let resp = engine
        .extract(&doc_ir, &req)
        .await
        .expect("Extraction should execute");

    // Hallucinated ID must be rejected, not bound
    assert!(
        !resp.evidence.contains_key("/party_a") || resp.evidence["/party_a"].is_empty(),
        "Hallucinated evidence ID must not be bound"
    );
    // Validation must record error
    assert!(!resp.validation.is_valid);
    assert!(resp
        .validation
        .errors
        .iter()
        .any(|e| e.code == "HALLUCINATED_EVIDENCE_ID"));
}
