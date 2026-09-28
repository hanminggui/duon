use crate::jobs::JobStore;
use crate::security::SsrfPolicy;
use duon_extractor::backend::ModelBackend;
use duon_extractor::engine::ExtractEngine;
use duon_extractor::verify::VerifyEngine;
use duon_parser::traits::DocumentParser;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub parser: Arc<dyn DocumentParser>,
    pub extract_engine: Arc<ExtractEngine<Arc<dyn ModelBackend>>>,
    pub verify_engine: Arc<VerifyEngine<Arc<dyn ModelBackend>>>,
    pub job_store: JobStore,
    pub ssrf_policy: SsrfPolicy,
}

impl AppState {
    pub fn new(
        parser: Arc<dyn DocumentParser>,
        extract_engine: Arc<ExtractEngine<Arc<dyn ModelBackend>>>,
        verify_engine: Arc<VerifyEngine<Arc<dyn ModelBackend>>>,
    ) -> Self {
        Self {
            parser,
            extract_engine,
            verify_engine,
            job_store: JobStore::new(),
            ssrf_policy: SsrfPolicy::default(),
        }
    }

    pub fn with_ssrf_policy(mut self, policy: SsrfPolicy) -> Self {
        self.ssrf_policy = policy;
        self
    }
}

