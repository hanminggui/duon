use crate::error::ExtractorError;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInvocationConfig {
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub model: String,
    pub temperature: f64,
    pub seed: u64,
}

impl Default for ModelInvocationConfig {
    fn default() -> Self {
        Self {
            base_url: None,
            api_key: None,
            model: "Qwen3-8B".to_string(),
            temperature: 0.0,
            seed: 42,
        }
    }
}

#[async_trait]
pub trait ModelBackend: Send + Sync {
    async fn generate(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        config: &ModelInvocationConfig,
    ) -> Result<String, ExtractorError>;
}

#[async_trait]
impl<B: ?Sized + ModelBackend> ModelBackend for std::sync::Arc<B> {
    async fn generate(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        config: &ModelInvocationConfig,
    ) -> Result<String, ExtractorError> {
        (**self).generate(system_prompt, user_prompt, config).await
    }
}

pub struct MockModelBackend {
    pub fixed_response: String,
}

impl MockModelBackend {
    pub fn new(fixed_response: String) -> Self {
        Self { fixed_response }
    }
}

#[async_trait]
impl ModelBackend for MockModelBackend {
    async fn generate(
        &self,
        _system_prompt: &str,
        _user_prompt: &str,
        _config: &ModelInvocationConfig,
    ) -> Result<String, ExtractorError> {
        Ok(self.fixed_response.clone())
    }
}

pub struct OpenAICompatibleBackend {
    client: reqwest::Client,
    default_base_url: String,
    default_api_key: Option<String>,
}

impl OpenAICompatibleBackend {
    pub fn new(default_base_url: String, default_api_key: Option<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            default_base_url,
            default_api_key,
        }
    }
}

#[async_trait]
impl ModelBackend for OpenAICompatibleBackend {
    async fn generate(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        config: &ModelInvocationConfig,
    ) -> Result<String, ExtractorError> {
        let base_url = config
            .base_url
            .as_deref()
            .unwrap_or(&self.default_base_url)
            .trim_end_matches('/');

        let endpoint = format!("{}/chat/completions", base_url);
        let api_key = config
            .api_key
            .as_deref()
            .or(self.default_api_key.as_deref());

        let payload = json!({
            "model": config.model,
            "temperature": config.temperature,
            "seed": config.seed,
            "response_format": { "type": "json_object" },
            "messages": [
                {
                    "role": "system",
                    "content": system_prompt
                },
                {
                    "role": "user",
                    "content": user_prompt
                }
            ]
        });

        let mut req = self.client.post(&endpoint).json(&payload);
        if let Some(key) = api_key {
            req = req.header("Authorization", format!("Bearer {}", key));
        }

        let resp = req.send().await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let err_text = resp.text().await.unwrap_or_default();
            return Err(ExtractorError::Backend(format!(
                "LLM backend returned HTTP {}: {}",
                status, err_text
            )));
        }

        let body: serde_json::Value = resp.json().await?;
        let content = body
            .get("choices")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
            .and_then(|first| first.get("message"))
            .and_then(|msg| msg.get("content"))
            .and_then(|c| c.as_str())
            .ok_or_else(|| {
                ExtractorError::Backend("Missing message content in choices[0]".to_string())
            })?;

        Ok(content.to_string())
    }
}
