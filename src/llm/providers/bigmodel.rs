use super::ProviderRegistry;
use super::super::provider_core::{to_openai_messages, ProviderRuntime, ProviderTrial};
use super::super::{ChatMessage, LlmProvider};
use crate::error::{GscholarError, Result};
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;
use tracing::debug;

const API_ENDPOINT: &str = "https://open.bigmodel.cn/api/paas/v4/chat/completions";
const DEFAULT_MODEL: &str = "GLM-4.7-Flash";

pub struct BigModelProvider {
    runtime: ProviderRuntime,
}

impl BigModelProvider {
    pub fn new(api_key: Option<&str>) -> Result<Self> {
        Self::with_model_and_endpoint(api_key, DEFAULT_MODEL, None)
    }

    pub fn with_model(api_key: Option<&str>, model: &str) -> Result<Self> {
        Self::with_model_and_endpoint(api_key, model, None)
    }

    pub fn with_model_and_endpoint(
        api_key: Option<&str>,
        model: &str,
        endpoint: Option<&str>,
    ) -> Result<Self> {
        let key = api_key.ok_or_else(|| {
            GscholarError::Config("BigModel API key is required".to_string())
        })?;

        let runtime = ProviderRuntime::new(
            "BigModel",
            endpoint.unwrap_or(API_ENDPOINT),
            key,
            model,
        )?;
        Ok(Self { runtime })
    }
}

#[async_trait]
impl ProviderTrial for BigModelProvider {
    fn runtime(&self) -> &ProviderRuntime {
        &self.runtime
    }

    fn build_payload(&self, messages: Vec<ChatMessage>) -> serde_json::Value {
        json!({
            "model": self.runtime.model,
            "messages": to_openai_messages(messages),
            "stream": false,
            "temperature": 1.0
        })
    }

    fn parse_response_body(&self, body: &str) -> Result<String> {
        let parsed: serde_json::Value = serde_json::from_str(body).map_err(|e| {
            GscholarError::Parse(format!("Failed to parse BigModel response: {}", e))
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
            "Parsed provider JSON response (reasoning ignored)"
        );

        if !content.is_empty() {
            return Ok(content);
        }

        let upper = body.to_uppercase();
        if upper.contains("YES") {
            return Ok("YES".to_string());
        }
        if upper.contains("NO") {
            return Ok("NO".to_string());
        }
        Err(GscholarError::Parse("Empty BigModel response content".to_string()))
    }
}

#[async_trait]
impl LlmProvider for BigModelProvider {
    fn name(&self) -> &str {
        self.runtime.provider_name
    }

    async fn chat_completion(&self, messages: Vec<ChatMessage>) -> Result<String> {
        self.execute_trial(messages).await
    }
}

pub(crate) fn register(registry: &mut ProviderRegistry) {
    registry.insert("bigmodel".to_string(), build_provider);
}

fn build_provider(cfg: &crate::server::config::ProviderConfig) -> Result<Arc<dyn LlmProvider>> {
    Ok(Arc::new(BigModelProvider::with_model_and_endpoint(
        Some(&cfg.api_key),
        &cfg.model,
        cfg.endpoint.as_deref(),
    )?))
}
