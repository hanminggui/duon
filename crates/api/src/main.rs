use duon_api::build_router;
use duon_api::state::AppState;
use duon_core::ir::DocumentIR;
use duon_extractor::backend::{ModelBackend, OpenAICompatibleBackend};
use duon_extractor::engine::ExtractEngine;
use duon_extractor::verify::VerifyEngine;
use duon_parser::mock::MockParser;
use std::net::SocketAddr;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Automatically load .env file if present
    dotenvy::dotenv().ok();

    println!("Starting Duon Document Intelligence Service...");

    // Default configuration (can be overridden via ENV or per-request)
    let default_base_url = std::env::var("LLM_BASE_URL")
        .unwrap_or_else(|_| "http://localhost:8000/v1".to_string());
    let default_api_key = std::env::var("LLM_API_KEY").ok();
    let default_model = std::env::var("LLM_MODEL")
        .unwrap_or_else(|_| "Qwen3-8B".to_string());

    println!("Default LLM Base URL : {}", default_base_url);
    println!("Default Model        : {}", default_model);
    println!("API Key Configured   : {}", default_api_key.is_some());

    // Initialize document parser:
    // Prefer real LiteParse CLI if `lit` is installed and available,
    // otherwise fallback to MockParser.
    let use_mock = std::env::var("USE_MOCK_PARSER")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);

    let lit_cli = duon_parser::cli::LiteParseCliParser::new();
    let parser: Arc<dyn duon_parser::traits::DocumentParser> = if !use_mock && lit_cli.is_available().await {
        let version = lit_cli.get_version().await;
        println!("Document Parser      : LiteParse CLI (version {})", version);
        Arc::new(lit_cli)
    } else {
        println!("Document Parser      : MockParser (fallback / mock mode)");
        let template_ir = serde_json::from_str::<DocumentIR>(include_str!(
            "../../../specs/mocks/sample_order_ir.json"
        ))?;
        Arc::new(MockParser::new(template_ir))
    };


    // Initialize backend
    let backend: Arc<dyn ModelBackend> =
        Arc::new(OpenAICompatibleBackend::new(default_base_url, default_api_key));

    let extract_engine = Arc::new(ExtractEngine::new(backend.clone()));
    let verify_engine = Arc::new(VerifyEngine::new(backend));

    let state = AppState::new(parser, extract_engine, verify_engine);
    let app = build_router(state);

    let port: u16 = std::env::var("PORT")
        .unwrap_or_else(|_| "8080".to_string())
        .parse()
        .unwrap_or(8080);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    println!("Duon API listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
