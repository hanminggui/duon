use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::Engine;
use duon_api::security::validate_url_ssrf;
use duon_api::state::AppState;
use duon_api::{build_router, EphemeralFile};
use duon_core::ir::DocumentIR;
use duon_extractor::backend::MockModelBackend;
use duon_extractor::engine::ExtractEngine;
use duon_extractor::verify::VerifyEngine;
use duon_parser::mock::MockParser;
use serde_json::json;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use tower::ServiceExt;

fn create_test_state() -> (AppState, DocumentIR) {
    let mock_path = Path::new("../../specs/mocks/sample_order_ir.json");
    let content = fs::read_to_string(mock_path)
        .or_else(|_| fs::read_to_string("specs/mocks/sample_order_ir.json"))
        .expect("Failed to read sample_order_ir.json");

    let doc_ir: DocumentIR = serde_json::from_str(&content).expect("Invalid IR");
    let parser = Arc::new(MockParser::new(doc_ir.clone()));

    let mock_llm = json!({
        "data": {
            "party_a": "吾立方公司",
            "amount": 1600
        },
        "citations": {
            "/party_a": ["p1_i0004"],
            "/amount": ["p1_i0008"]
        }
    });

    let backend: Arc<dyn duon_extractor::backend::ModelBackend> =
        Arc::new(MockModelBackend::new(mock_llm.to_string()));
    let extract_engine = Arc::new(ExtractEngine::new(backend));

    let mock_verify_llm = json!({
        "fields": {
            "/amount": {
                "document_value": 1600,
                "citations": ["p1_i0008"]
            }
        }
    });
    let verify_backend: Arc<dyn duon_extractor::backend::ModelBackend> =
        Arc::new(MockModelBackend::new(mock_verify_llm.to_string()));
    let verify_engine = Arc::new(VerifyEngine::new(verify_backend));

    (AppState::new(parser, extract_engine, verify_engine), doc_ir)
}

#[tokio::test]
async fn test_ssrf_validation_blocks_private_and_local_ips() {
    assert!(validate_url_ssrf("http://127.0.0.1:8080/doc.pdf").is_err());
    assert!(validate_url_ssrf("http://localhost:3000/doc.pdf").is_err());
    assert!(validate_url_ssrf("http://169.254.169.254/latest/meta-data").is_err());
    assert!(validate_url_ssrf("http://10.0.0.5/test.pdf").is_err());
    assert!(validate_url_ssrf("http://192.168.1.1/secret.pdf").is_err());
    assert!(validate_url_ssrf("file:///etc/passwd").is_err());
    assert!(validate_url_ssrf("ftp://example.com/file.pdf").is_err());
}

#[tokio::test]
async fn test_ephemeral_file_guard_deletes_file_on_drop() {
    let temp_dir = tempfile::tempdir().unwrap();
    let file_path = temp_dir.path().join("ephemeral_test.pdf");
    fs::write(&file_path, b"test PDF bytes").unwrap();
    assert!(file_path.exists());

    {
        let _guard = EphemeralFile::new(file_path.clone());
        assert!(file_path.exists());
    } // guard drops here

    assert!(!file_path.exists(), "File should be deleted upon drop");
}

#[tokio::test]
async fn test_layer1_parse_endpoint() {
    let (state, _) = create_test_state();
    let app = build_router(state);

    let req_body = json!({
        "document": {
            "url": "https://example.com/order.pdf"
        }
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/parse")
                .header("Content-Type", "application/json")
                .body(Body::from(req_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let doc_ir: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    // Verify it returns DocumentIR
    assert_eq!(doc_ir["pages"][0]["page_number"], 1);
    assert_eq!(doc_ir["pages"][0]["items"][0]["id"], "p1_i0001");
}

#[tokio::test]
async fn test_ssrf_policy_allows_intranet_when_configured() {
    use duon_api::validate_url_ssrf_with_policy;
    use duon_api::SsrfPolicy;

    let default_policy = SsrfPolicy::strict();
    // Default strict blocks private IPs
    assert!(validate_url_ssrf_with_policy("http://10.0.0.5/test.pdf", &default_policy).is_err());
    assert!(validate_url_ssrf_with_policy("http://192.168.1.1/doc.pdf", &default_policy).is_err());

    let intranet_policy = SsrfPolicy::allow_intranet();
    // Intranet policy allows private RFC 1918 IPs
    assert!(validate_url_ssrf_with_policy("http://10.0.0.5/test.pdf", &intranet_policy).is_ok());
    assert!(validate_url_ssrf_with_policy("http://192.168.1.1/doc.pdf", &intranet_policy).is_ok());

    // But dangerous cloud metadata is still strictly blocked!
    assert!(validate_url_ssrf_with_policy("http://169.254.169.254/latest/meta-data", &intranet_policy).is_err());

    // Whitelist host works even in strict mode
    let mut whitelist_policy = SsrfPolicy::strict();
    whitelist_policy.allowed_hosts = vec!["minio.internal".to_string()];
    assert!(validate_url_ssrf_with_policy("http://minio.internal/bucket/file.pdf", &whitelist_policy).is_ok());
}

#[tokio::test]
async fn test_parse_with_base64_json() {
    let (state, _) = create_test_state();
    let app = build_router(state);

    let raw_content = b"%PDF-1.7 inline sample content";
    let encoded = base64::engine::general_purpose::STANDARD.encode(raw_content);

    let req_body = json!({
        "document": {
            "base64": encoded
        },
        "ocr_enabled": false
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/parse")
                .header("Content-Type", "application/json")
                .body(Body::from(req_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let doc_ir: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(doc_ir["pages"][0]["page_number"], 1);
}

#[tokio::test]
async fn test_parse_with_data_uri() {
    let (state, _) = create_test_state();
    let app = build_router(state);

    let raw_content = b"%PDF-1.7 data uri sample";
    let encoded = base64::engine::general_purpose::STANDARD.encode(raw_content);
    let data_uri = format!("data:application/pdf;base64,{}", encoded);

    let req_body = json!({
        "document": {
            "url": data_uri
        }
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/parse")
                .header("Content-Type", "application/json")
                .body(Body::from(req_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let doc_ir: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(doc_ir["pages"][0]["page_number"], 1);
}

#[tokio::test]
async fn test_parse_with_raw_binary() {
    let (state, _) = create_test_state();
    let app = build_router(state);

    let binary_bytes = b"%PDF-1.7 raw binary stream".to_vec();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/parse?ocr_enabled=true&filename=doc.pdf")
                .header("Content-Type", "application/pdf")
                .body(Body::from(binary_bytes))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let doc_ir: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(doc_ir["pages"][0]["page_number"], 1);
}

#[tokio::test]
async fn test_parse_with_local_file_path() {
    let (state, _) = create_test_state();
    let app = build_router(state);

    let temp_dir = tempfile::tempdir().unwrap();
    let file_path = temp_dir.path().join("local_contract.pdf");
    fs::write(&file_path, b"%PDF-1.7 local file test").unwrap();

    let req_body = json!({
        "document": {
            "path": file_path.to_str().unwrap()
        }
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/parse")
                .header("Content-Type", "application/json")
                .body(Body::from(req_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let doc_ir: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(doc_ir["pages"][0]["page_number"], 1);
}

#[tokio::test]
async fn test_parse_with_multipart_form_data() {
    let (state, _) = create_test_state();
    let app = build_router(state);

    let boundary = "----WebKitFormBoundary7MA4YWxkTrZu0gW";
    let body_payload = format!(
        "--{boundary}\r\n\
        Content-Disposition: form-data; name=\"file\"; filename=\"test_doc.pdf\"\r\n\
        Content-Type: application/pdf\r\n\r\n\
        %PDF-1.7 sample content from multipart form\r\n\
        --{boundary}\r\n\
        Content-Disposition: form-data; name=\"ocr_enabled\"\r\n\r\n\
        true\r\n\
        --{boundary}--\r\n",
        boundary = boundary
    );

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/parse")
                .header(
                    "Content-Type",
                    format!("multipart/form-data; boundary={}", boundary),
                )
                .body(Body::from(body_payload))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let doc_ir: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(doc_ir["pages"][0]["page_number"], 1);
}

#[tokio::test]
async fn test_ephemeral_file_from_bytes_cleanup() {
    let file_path;
    {
        let ephemeral = EphemeralFile::from_bytes(b"%PDF-1.7 temporary content", Some("test.pdf")).unwrap();
        file_path = ephemeral.path().to_path_buf();
        assert!(file_path.exists());
        assert!(file_path.to_str().unwrap().ends_with(".pdf"));
    } // ephemeral dropped here

    assert!(!file_path.exists(), "Ephemeral file must be deleted upon drop");
}


#[tokio::test]
async fn test_layer2_extract_endpoint_strictly_accepts_ir() {
    let (state, doc_ir) = create_test_state();
    let app = build_router(state);

    let req_body = json!({
        "document_ir": doc_ir,
        "schema": {
            "type": "object",
            "properties": {
                "party_a": { "type": "string" },
                "amount": { "type": "number" }
            }
        }
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/extract")
                .header("Content-Type", "application/json")
                .body(Body::from(req_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let resp_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    assert_eq!(resp_json["data"]["party_a"], "吾立方公司");
    assert_eq!(resp_json["data"]["amount"], 1600);
    assert!(resp_json["evidence"].is_object());
    assert!(resp_json["meta"]["result_hash"].is_string());
}

#[tokio::test]
async fn test_layer2_verify_endpoint_strictly_accepts_ir() {
    let (state, doc_ir) = create_test_state();
    let app = build_router(state);

    let req_body = json!({
        "document_ir": doc_ir,
        "data": {
            "amount": 1600
        }
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/verify")
                .header("Content-Type", "application/json")
                .body(Body::from(req_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let resp_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    assert_eq!(resp_json["status"], "matched");
    assert_eq!(resp_json["fields"]["/amount"]["status"], "matched");
}

#[tokio::test]
async fn test_async_extract_job_workflow() {
    let (state, doc_ir) = create_test_state();
    let app = build_router(state.clone());

    let req_body = json!({
        "document_ir": doc_ir
    });

    // 1. Submit async request with ?async=true
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/extract?async=true")
                .header("Content-Type", "application/json")
                .body(Body::from(req_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let accepted: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    let job_id = accepted["job_id"].as_str().unwrap();
    assert!(!job_id.is_empty());

    // 2. Poll job status
    let mut completed = false;
    for _ in 0..20 {
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let poll_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(&format!("/v1/jobs/{}", job_id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(poll_response.status(), StatusCode::OK);
        let poll_bytes = axum::body::to_bytes(poll_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let job_status: serde_json::Value = serde_json::from_slice(&poll_bytes).unwrap();

        if job_status["status"] == "completed" {
            assert!(job_status["result"].is_object());
            completed = true;
            break;
        }
    }
    assert!(completed, "Job should reach completed status");
}
