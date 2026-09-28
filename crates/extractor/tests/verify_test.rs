use duon_core::ir::DocumentIR;
use duon_extractor::backend::{MockModelBackend, ModelInvocationConfig};
use duon_extractor::verify::{VerifyEngine, VerifyRequestInternal};
use serde_json::json;
use std::fs;
use std::path::Path;

#[tokio::test]
async fn test_verify_detects_conflict_with_evidence() {
    let mock_path = Path::new("../../specs/mocks/sample_order_ir.json");
    let content = fs::read_to_string(mock_path)
        .or_else(|_| fs::read_to_string("specs/mocks/sample_order_ir.json"))
        .expect("Failed to read sample_order_ir.json");

    let doc_ir: DocumentIR = serde_json::from_str(&content).expect("Invalid IR JSON");

    // The system recorded amount as 1500, but document says 1600
    let system_data = json!({
        "party_a": "吾立方公司",
        "party_b": "宋泽昊",
        "amount": 1500,
        "order_date": "2026-05-18"
    });

    // Mock LLM discovers ground truth from document
    let mock_llm_output = json!({
        "fields": {
            "/party_a": {
                "document_value": "吾立方公司",
                "citations": ["p1_i0004"]
            },
            "/party_b": {
                "document_value": "宋泽昊",
                "citations": ["p1_i0006"]
            },
            "/amount": {
                "document_value": 1600,
                "citations": ["p1_i0008"]
            },
            "/order_date": {
                "document_value": "2026-05-18",
                "citations": ["p1_i0007"]
            }
        }
    });

    let backend = MockModelBackend::new(mock_llm_output.to_string());
    let engine = VerifyEngine::new(backend);

    let req = VerifyRequestInternal {
        data: system_data,
        schema: None,
        model_config: ModelInvocationConfig::default(),
    };

    let resp = engine
        .verify(&doc_ir, &req)
        .await
        .expect("Verification should succeed");

    // Overall status must be conflict
    assert_eq!(resp.status, "conflict");
    assert_eq!(resp.summary.total_checked, 4);
    assert_eq!(resp.summary.matched_count, 3);
    assert_eq!(resp.summary.conflict_count, 1);
    assert_eq!(resp.summary.missing_count, 0);

    // Verify /amount field detail
    let amount_field = &resp.fields["/amount"];
    assert_eq!(amount_field.status, "conflict");
    assert_eq!(amount_field.system_value, Some(json!(1500)));
    assert_eq!(amount_field.document_value, Some(json!(1600)));
    assert!(amount_field.diff_reason.is_some());
    assert_eq!(amount_field.evidence.len(), 1);
    assert_eq!(amount_field.evidence[0].item_ids, vec!["p1_i0008"]);
    assert_eq!(amount_field.evidence[0].quote, "费用价格：总金额为人民币 1600 元 税前。");

    // Verify /party_a field matched
    let party_a_field = &resp.fields["/party_a"];
    assert_eq!(party_a_field.status, "matched");
    assert_eq!(party_a_field.evidence.len(), 1);
}
