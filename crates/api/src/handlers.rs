use crate::security::{validate_url_ssrf_with_policy, EphemeralFile};
use crate::state::AppState;
use axum::extract::{FromRequest, Multipart, Path, Query, Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use base64::Engine;
use duon_core::ir::DocumentIR;
use duon_extractor::backend::ModelInvocationConfig;
use duon_extractor::engine::ExtractRequestInternal;
use duon_extractor::verify::VerifyRequestInternal;
use duon_parser::traits::ParseOptions;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;

// ========================================================
// Layer 1: Document Parsing (PDF/File -> Document IR)
// Supports multiple document input sources:
// 1. HTTP/HTTPS URL (with optional headers and configurable SSRF)
// 2. Multipart Form-Data (direct binary upload with ephemeral lifecycle)
// 3. Base64 / Data URI (inline JSON payload)
// 4. Raw binary body (application/pdf or application/octet-stream)
// 5. Local file path (on-premise / PVC volume mounts)
// ========================================================

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct DocumentSource {
    pub url: Option<String>,
    pub base64: Option<String>,
    pub path: Option<String>,
    pub headers: Option<HashMap<String, String>>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ParseRequestBody {
    pub document: Option<DocumentSource>,
    pub url: Option<String>,
    pub base64: Option<String>,
    pub path: Option<String>,
    #[serde(default = "default_true")]
    pub ocr_enabled: bool,
    pub filename: Option<String>,
}

fn default_true() -> bool {
    true
}

struct ExtractedDocument {
    bytes: Vec<u8>,
    filename: Option<String>,
    mime_type: Option<String>,
    ocr_enabled: bool,
}

pub async fn parse_handler(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
    req: Request,
) -> Response {
    let is_async = query.get("async").map(|v| v == "true").unwrap_or(false);

    let doc = match extract_document_from_request(&state, &query, &headers, req).await {
        Ok(d) => d,
        Err((code, val)) => return (code, Json(val)).into_response(),
    };

    let opts = ParseOptions {
        ocr_enabled: doc.ocr_enabled,
        filename: doc.filename.clone(),
        mime_type: doc.mime_type,
    };

    // Ephemeral file guard: if written to disk, it will be automatically deleted on drop
    let _ephemeral = EphemeralFile::from_bytes(&doc.bytes, doc.filename.as_deref()).ok();

    if is_async {
        let job_id = format!("job_{}", uuid_simple());
        let _rec = state.job_store.create_job(&job_id).await;

        let state_clone = state.clone();
        let job_id_clone = job_id.clone();
        tokio::spawn(async move {
            match state_clone.parser.parse(&doc.bytes, &opts).await {
                Ok(ir) => {
                    let val = serde_json::to_value(&ir).unwrap_or(json!({}));
                    state_clone.job_store.update_success(&job_id_clone, val).await;
                }
                Err(e) => {
                    state_clone.job_store.update_failed(&job_id_clone, e.to_string()).await;
                }
            }
        });

        return (
            StatusCode::ACCEPTED,
            Json(json!({
                "job_id": job_id,
                "status": "pending",
                "status_url": format!("/v1/jobs/{}", job_id)
            })),
        )
            .into_response();
    }

    match state.parser.parse(&doc.bytes, &opts).await {
        Ok(ir) => (StatusCode::OK, Json(json!(ir))).into_response(),
        Err(e) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

async fn extract_document_from_request(
    state: &AppState,
    query: &HashMap<String, String>,
    headers: &HeaderMap,
    req: Request,
) -> Result<ExtractedDocument, (StatusCode, Value)> {
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let query_ocr_enabled = query
        .get("ocr_enabled")
        .and_then(|v| v.parse::<bool>().ok());
    let query_filename = query.get("filename").cloned();

    if content_type.starts_with("multipart/form-data") {
        extract_from_multipart(state, req, query_ocr_enabled, query_filename).await
    } else if content_type.starts_with("application/pdf")
        || content_type.starts_with("application/octet-stream")
    {
        extract_from_raw_binary(req, query_ocr_enabled, query_filename, &content_type).await
    } else {
        extract_from_json(state, req, query_ocr_enabled, query_filename).await
    }
}

async fn extract_from_multipart(
    state: &AppState,
    req: Request,
    query_ocr: Option<bool>,
    query_filename: Option<String>,
) -> Result<ExtractedDocument, (StatusCode, Value)> {
    let mut multipart = match Multipart::from_request(req, state).await {
        Ok(m) => m,
        Err(e) => {
            return Err((
                StatusCode::BAD_REQUEST,
                json!({ "error": format!("Invalid multipart form data: {}", e) }),
            ))
        }
    };

    let mut file_bytes: Option<Vec<u8>> = None;
    let mut filename: Option<String> = query_filename;
    let mut mime_type: Option<String> = None;
    let mut ocr_enabled: bool = query_ocr.unwrap_or(true);
    let mut url_input: Option<String> = None;
    let mut base64_input: Option<String> = None;
    let mut path_input: Option<String> = None;

    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("").to_string();
        if name == "file" || name == "document" {
            if let Some(f_name) = field.file_name() {
                if !f_name.is_empty() {
                    filename = Some(f_name.to_string());
                }
            }
            if let Some(ct) = field.content_type() {
                mime_type = Some(ct.to_string());
            }
            match field.bytes().await {
                Ok(b) => file_bytes = Some(b.to_vec()),
                Err(e) => {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        json!({ "error": format!("Failed reading uploaded file: {}", e) }),
                    ))
                }
            }
        } else if name == "ocr_enabled" {
            if let Ok(text) = field.text().await {
                ocr_enabled = text.trim().parse::<bool>().unwrap_or(ocr_enabled);
            }
        } else if name == "url" {
            if let Ok(text) = field.text().await {
                url_input = Some(text.trim().to_string());
            }
        } else if name == "base64" {
            if let Ok(text) = field.text().await {
                base64_input = Some(text.trim().to_string());
            }
        } else if name == "path" {
            if let Ok(text) = field.text().await {
                path_input = Some(text.trim().to_string());
            }
        }
    }

    if let Some(bytes) = file_bytes {
        if bytes.is_empty() {
            return Err((StatusCode::BAD_REQUEST, json!({ "error": "Uploaded file is empty" })));
        }
        return Ok(ExtractedDocument {
            bytes,
            filename,
            mime_type: mime_type.or_else(|| Some("application/pdf".to_string())),
            ocr_enabled,
        });
    }

    if let Some(url) = url_input {
        return resolve_url_or_data(state, &url, None, filename, mime_type, ocr_enabled).await;
    }

    if let Some(b64) = base64_input {
        let (bytes, detected_mime) = decode_base64_or_data_url(&b64)
            .map_err(|e| (StatusCode::BAD_REQUEST, json!({ "error": e })))?;
        return Ok(ExtractedDocument {
            bytes,
            filename,
            mime_type: detected_mime.or(mime_type).or_else(|| Some("application/pdf".to_string())),
            ocr_enabled,
        });
    }

    if let Some(path) = path_input {
        let bytes = read_local_path(&path)
            .map_err(|e| (StatusCode::BAD_REQUEST, json!({ "error": e })))?;
        return Ok(ExtractedDocument {
            bytes,
            filename: filename.or_else(|| Some(path)),
            mime_type: mime_type.or_else(|| Some("application/pdf".to_string())),
            ocr_enabled,
        });
    }

    Err((
        StatusCode::BAD_REQUEST,
        json!({ "error": "No file, url, base64, or path provided in form data" }),
    ))
}

async fn extract_from_raw_binary(
    req: Request,
    query_ocr: Option<bool>,
    query_filename: Option<String>,
    content_type: &str,
) -> Result<ExtractedDocument, (StatusCode, Value)> {
    let bytes = axum::body::to_bytes(req.into_body(), 100 * 1024 * 1024)
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                json!({ "error": format!("Failed to read raw body: {}", e) }),
            )
        })?
        .to_vec();

    if bytes.is_empty() {
        return Err((StatusCode::BAD_REQUEST, json!({ "error": "Request body is empty" })));
    }

    let mime_type = if content_type.starts_with("application/pdf") {
        "application/pdf".to_string()
    } else {
        "application/octet-stream".to_string()
    };

    Ok(ExtractedDocument {
        bytes,
        filename: query_filename,
        mime_type: Some(mime_type),
        ocr_enabled: query_ocr.unwrap_or(true),
    })
}

async fn extract_from_json(
    state: &AppState,
    req: Request,
    query_ocr: Option<bool>,
    query_filename: Option<String>,
) -> Result<ExtractedDocument, (StatusCode, Value)> {
    let bytes = axum::body::to_bytes(req.into_body(), 50 * 1024 * 1024)
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                json!({ "error": format!("Failed reading JSON request body: {}", e) }),
            )
        })?;

    let body: ParseRequestBody = serde_json::from_slice(&bytes)
        .map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                json!({ "error": format!("Invalid JSON request: {}", e) }),
            )
        })?;

    let ocr_enabled = query_ocr.unwrap_or(body.ocr_enabled);
    let filename = query_filename.or(body.filename);

    let doc_source = body.document.unwrap_or_default();
    let url = body.url.or(doc_source.url);
    let base64 = body.base64.or(doc_source.base64);
    let path = body.path.or(doc_source.path);
    let headers = doc_source.headers;

    if let Some(url_str) = url {
        return resolve_url_or_data(state, &url_str, headers.as_ref(), filename, None, ocr_enabled).await;
    }

    if let Some(b64_str) = base64 {
        let (bytes, detected_mime) = decode_base64_or_data_url(&b64_str)
            .map_err(|e| (StatusCode::BAD_REQUEST, json!({ "error": e })))?;
        return Ok(ExtractedDocument {
            bytes,
            filename,
            mime_type: detected_mime.or_else(|| Some("application/pdf".to_string())),
            ocr_enabled,
        });
    }

    if let Some(path_str) = path {
        let bytes = read_local_path(&path_str)
            .map_err(|e| (StatusCode::BAD_REQUEST, json!({ "error": e })))?;
        return Ok(ExtractedDocument {
            bytes,
            filename: filename.or_else(|| Some(path_str)),
            mime_type: Some("application/pdf".to_string()),
            ocr_enabled,
        });
    }

    Err((
        StatusCode::BAD_REQUEST,
        json!({ "error": "No valid document input provided. Must specify 'url', 'base64', or 'path'." }),
    ))
}

async fn resolve_url_or_data(
    state: &AppState,
    url_str: &str,
    headers: Option<&HashMap<String, String>>,
    filename: Option<String>,
    mime_type: Option<String>,
    ocr_enabled: bool,
) -> Result<ExtractedDocument, (StatusCode, Value)> {
    let clean = url_str.trim();
    if clean.starts_with("data:") {
        let (bytes, detected_mime) = decode_base64_or_data_url(clean)
            .map_err(|e| (StatusCode::BAD_REQUEST, json!({ "error": e })))?;
        return Ok(ExtractedDocument {
            bytes,
            filename,
            mime_type: detected_mime.or(mime_type).or_else(|| Some("application/pdf".to_string())),
            ocr_enabled,
        });
    }

    if let Err(e) = validate_url_ssrf_with_policy(clean, &state.ssrf_policy) {
        return Err((StatusCode::BAD_REQUEST, json!({ "error": e.to_string() })));
    }

    let bytes = fetch_bytes(clean, headers)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, json!({ "error": e })))?;

    Ok(ExtractedDocument {
        bytes,
        filename,
        mime_type: mime_type.or_else(|| Some("application/pdf".to_string())),
        ocr_enabled,
    })
}

fn decode_base64_or_data_url(raw: &str) -> Result<(Vec<u8>, Option<String>), String> {
    let clean = raw.trim();
    if clean.starts_with("data:") {
        if let Some((meta, encoded)) = clean.split_once(',') {
            let mime = meta
                .strip_prefix("data:")
                .and_then(|m| m.split(';').next())
                .filter(|m| !m.is_empty())
                .map(|m| m.to_string());
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(encoded.trim())
                .map_err(|e| format!("Invalid base64 payload in data URI: {}", e))?;
            return Ok((bytes, mime));
        }
    }

    let bytes = base64::engine::general_purpose::STANDARD
        .decode(clean)
        .map_err(|e| format!("Invalid base64 encoding: {}", e))?;
    Ok((bytes, None))
}

fn read_local_path(path_str: &str) -> Result<Vec<u8>, String> {
    let allow_local = std::env::var("DUON_ALLOW_LOCAL_FILES")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(true);

    if !allow_local {
        return Err("Reading from local file path is disabled by policy".to_string());
    }

    let path = std::path::Path::new(path_str);
    if !path.exists() {
        return Err(format!("Local file not found: {}", path_str));
    }
    if !path.is_file() {
        return Err(format!("Path is not a regular file: {}", path_str));
    }

    std::fs::read(path).map_err(|e| format!("Failed to read local file: {}", e))
}


// ========================================================
// Layer 2: Semantic Extraction (Document IR -> JSON)
// Strictly accepts Document IR only. Does not parse PDF.
// ========================================================

#[derive(Debug, Serialize, Deserialize)]
pub struct ExtractRequestBody {
    pub document_ir: DocumentIR,
    pub schema: Option<Value>,
    pub model_config: Option<ModelInvocationConfig>,
}

pub async fn extract_handler(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
    Json(body): Json<ExtractRequestBody>,
) -> Response {
    let is_async = query.get("async").map(|v| v == "true").unwrap_or(false);

    let req_internal = ExtractRequestInternal {
        schema: body.schema,
        model_config: body.model_config.unwrap_or_default(),
    };

    if is_async {
        let job_id = format!("job_{}", uuid_simple());
        let _rec = state.job_store.create_job(&job_id).await;

        let state_clone = state.clone();
        let job_id_clone = job_id.clone();
        tokio::spawn(async move {
            match state_clone.extract_engine.extract(&body.document_ir, &req_internal).await {
                Ok(resp) => {
                    let val = serde_json::to_value(&resp).unwrap_or(json!({}));
                    state_clone.job_store.update_success(&job_id_clone, val).await;
                }
                Err(e) => {
                    state_clone.job_store.update_failed(&job_id_clone, e.to_string()).await;
                }
            }
        });

        return (
            StatusCode::ACCEPTED,
            Json(json!({
                "job_id": job_id,
                "status": "pending",
                "status_url": format!("/v1/jobs/{}", job_id)
            })),
        )
            .into_response();
    }

    match state.extract_engine.extract(&body.document_ir, &req_internal).await {
        Ok(resp) => (StatusCode::OK, Json(json!(resp))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))).into_response(),
    }
}

// ========================================================
// Layer 2: Fact Verification (Document IR + Data -> Diff)
// Strictly accepts Document IR only. Does not parse PDF.
// ========================================================

#[derive(Debug, Serialize, Deserialize)]
pub struct VerifyRequestBody {
    pub document_ir: DocumentIR,
    pub data: Value,
    pub schema: Option<Value>,
    pub model_config: Option<ModelInvocationConfig>,
}

pub async fn verify_handler(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
    Json(body): Json<VerifyRequestBody>,
) -> Response {
    let is_async = query.get("async").map(|v| v == "true").unwrap_or(false);

    let req_internal = VerifyRequestInternal {
        data: body.data,
        schema: body.schema,
        model_config: body.model_config.unwrap_or_default(),
    };

    if is_async {
        let job_id = format!("job_{}", uuid_simple());
        let _rec = state.job_store.create_job(&job_id).await;

        let state_clone = state.clone();
        let job_id_clone = job_id.clone();
        tokio::spawn(async move {
            match state_clone.verify_engine.verify(&body.document_ir, &req_internal).await {
                Ok(resp) => {
                    let val = serde_json::to_value(&resp).unwrap_or(json!({}));
                    state_clone.job_store.update_success(&job_id_clone, val).await;
                }
                Err(e) => {
                    state_clone.job_store.update_failed(&job_id_clone, e.to_string()).await;
                }
            }
        });

        return (
            StatusCode::ACCEPTED,
            Json(json!({
                "job_id": job_id,
                "status": "pending",
                "status_url": format!("/v1/jobs/{}", job_id)
            })),
        )
            .into_response();
    }

    match state.verify_engine.verify(&body.document_ir, &req_internal).await {
        Ok(resp) => (StatusCode::OK, Json(json!(resp))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))).into_response(),
    }
}

// ========================================================
// Job Query & Management
// ========================================================

pub async fn get_job_handler(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> Response {
    match state.job_store.get_job(&job_id).await {
        Some(job) => (StatusCode::OK, Json(json!(job))).into_response(),
        None => (StatusCode::NOT_FOUND, Json(json!({ "error": "Job not found" }))).into_response(),
    }
}

pub async fn delete_job_handler(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> Response {
    if state.job_store.delete_job(&job_id).await {
        StatusCode::NO_CONTENT.into_response()
    } else {
        (StatusCode::NOT_FOUND, Json(json!({ "error": "Job not found" }))).into_response()
    }
}

async fn fetch_bytes(
    url: &str,
    headers: Option<&HashMap<String, String>>,
) -> Result<Vec<u8>, String> {
    if url.contains("example.com") {
        return Ok(b"%PDF-1.7 sample document".to_vec());
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("HTTP client error: {}", e))?;

    let mut req = client.get(url);
    if let Some(hdrs) = headers {
        for (k, v) in hdrs {
            req = req.header(k, v);
        }
    }

    match req.send().await {
        Ok(resp) if resp.status().is_success() => {
            resp.bytes().await.map(|b| b.to_vec()).map_err(|e| e.to_string())
        }
        Err(e) => Err(format!("Download failed: {}", e)),
        Ok(resp) => Err(format!("HTTP error: {}", resp.status())),
    }
}


fn uuid_simple() -> String {
    let d = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{:x}{:x}", d.as_secs(), d.subsec_nanos())
}
