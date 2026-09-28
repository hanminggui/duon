pub mod backend;
pub mod context;
pub mod engine;
pub mod error;
pub mod evidence;
pub mod verify;

pub use backend::{MockModelBackend, ModelBackend, ModelInvocationConfig, OpenAICompatibleBackend};
pub use context::ContextBuilder;
pub use engine::{
    ExtractEngine, ExtractRequestInternal, ExtractResponse, ExtractionMeta, ValidationSummary,
};
pub use error::ExtractorError;
pub use evidence::{EvidenceBinder, EvidenceItem, ValidationError};
pub use verify::{
    VerifiedField, VerifyEngine, VerifyMeta, VerifyRequestInternal, VerifyResponse, VerifySummary,
};
