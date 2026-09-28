use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobRecord {
    pub job_id: String,
    pub status: JobStatus,
    pub created_at: String,
    pub updated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone)]
pub struct JobStore {
    jobs: Arc<RwLock<HashMap<String, JobRecord>>>,
}

impl JobStore {
    pub fn new() -> Self {
        Self {
            jobs: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn create_job(&self, job_id: &str) -> JobRecord {
        let now = chrono_now();
        let record = JobRecord {
            job_id: job_id.to_string(),
            status: JobStatus::Pending,
            created_at: now.clone(),
            updated_at: now,
            result: None,
            error: None,
        };

        let mut lock = self.jobs.write().await;
        lock.insert(job_id.to_string(), record.clone());
        record
    }

    pub async fn update_success(&self, job_id: &str, result: Value) {
        let mut lock = self.jobs.write().await;
        if let Some(job) = lock.get_mut(job_id) {
            job.status = JobStatus::Completed;
            job.updated_at = chrono_now();
            job.result = Some(result);
        }
    }

    pub async fn update_failed(&self, job_id: &str, err: String) {
        let mut lock = self.jobs.write().await;
        if let Some(job) = lock.get_mut(job_id) {
            job.status = JobStatus::Failed;
            job.updated_at = chrono_now();
            job.error = Some(err);
        }
    }

    pub async fn get_job(&self, job_id: &str) -> Option<JobRecord> {
        let lock = self.jobs.read().await;
        lock.get(job_id).cloned()
    }

    pub async fn delete_job(&self, job_id: &str) -> bool {
        let mut lock = self.jobs.write().await;
        lock.remove(job_id).is_some()
    }
}

fn chrono_now() -> String {
    // Return standard ISO format
    let d = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}.{}Z", d.as_secs(), d.subsec_millis())
}
