pub mod handlers;
pub mod jobs;
pub mod security;
pub mod state;

use axum::routing::{delete, get, post};
use axum::Router;
pub use security::{validate_url_ssrf, validate_url_ssrf_with_policy, EphemeralFile, SsrfPolicy};
pub use state::AppState;

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/v1/parse", post(handlers::parse_handler))
        .route("/v1/extract", post(handlers::extract_handler))
        .route("/v1/verify", post(handlers::verify_handler))
        .route("/v1/jobs/:job_id", get(handlers::get_job_handler))
        .route("/v1/jobs/:job_id", delete(handlers::delete_job_handler))
        .with_state(state)
}
