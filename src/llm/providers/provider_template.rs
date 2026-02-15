use super::ProviderRegistry;
use super::super::provider_core::{to_openai_messages, ProviderRuntime, ProviderTrial};
use super::super::{ChatMessage, LlmProvider};
use crate::error::{GscholarError, Result};
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;
use tracing::debug;

const API_ENDPOINT: &str = "https://example.com/v1/chat/completions";
const DEFAULT_MODEL: &str = "example-model";

pub struct ExampleProvider {
    runtime: ProviderRuntime,
}

impl ExampleProvider {
    pub fn with_model_and_endpoint(
        api_key: Option<&str>,
        model: &str,
        endpoint: Option<&str>,
    ) -> Result<Self> {
        let key = api_key.ok_or_else(|| {
            GscholarError::Config("Example provider API key is required".to_string())
        })?;

        let runtime = ProviderRuntime::new(
            "Example",
            endpoint.unwrap_or(API_ENDPOINT),
            key,
            model,
        )?;

        Ok(Self { runtime })
    }

    pub fn new(api_key: Option<&str>) -> Result<Self> {
        Self::with_model_and_endpoint(api_key, DEFAULT_MODEL, None)
    }
}

#[async_trait]
impl ProviderTrial for ExampleProvider {
    fn runtime(&self) -> &ProviderRuntime {
        &self.runtime
    }

    fn build_payload(&self, messages: Vec<ChatMessage>) -> serde_json::Value {
        json!({
            "model": self.runtime.model,
            "messages": to_openai_messages(messages),
            "temperature": 0.1
        })
    }

    fn parse_response_body(&self, body: &str) -> Result<String> {
        let parsed: serde_json::Value = serde_json::from_str(body).map_err(|e| {
            GscholarError::Parse(format!("Failed to parse {} response: {}", self.runtime.provider_name, e))
        })?;

        let content = parsed
            .get("choices")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();

        debug!(
            provider = self.runtime.provider_name,
            content_size = content.len(),
            "Parsed provider response in template"
        );

        if !content.is_empty() {
            return Ok(content);
        }

        Err(GscholarError::Parse(format!(
            "Empty {} response content",
            self.runtime.provider_name
        )))
    }
}

#[async_trait]
impl LlmProvider for ExampleProvider {
    fn name(&self) -> &str {
        self.runtime.provider_name
    }

    async fn chat_completion(&self, messages: Vec<ChatMessage>) -> Result<String> {
        self.execute_trial(messages).await
    }
}

pub(crate) fn register(registry: &mut ProviderRegistry) {
    registry.insert("example".to_string(), build_provider);
}

fn build_provider(cfg: &crate::server::config::ProviderConfig) -> Result<Arc<dyn LlmProvider>> {
    Ok(Arc::new(ExampleProvider::with_model_and_endpoint(
        Some(&cfg.api_key),
        &cfg.model,
        cfg.endpoint.as_deref(),
    )?))
}
