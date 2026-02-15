use super::ProviderRegistry;
use super::super::provider_core::{to_openai_messages, ProviderRuntime, ProviderTrial};
use super::super::{ChatMessage, LlmProvider};
use crate::error::{GscholarError, Result};
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;
use tracing::debug;

const API_ENDPOINT: &str = "https://www.aiping.cn/api/v1/chat/completions";
const DEFAULT_MODEL: &str = "Qwen3-235B-A22B";

pub struct AiPingProvider {
    runtime: ProviderRuntime,
}

impl AiPingProvider {
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
            GscholarError::Config("AIPing API key is required".to_string())
        })?;
        let runtime = ProviderRuntime::new(
            "AIPing",
            endpoint.unwrap_or(API_ENDPOINT),
            key,
            model,
        )?;
        Ok(Self { runtime })
    }
}

#[async_trait]
impl ProviderTrial for AiPingProvider {
    fn runtime(&self) -> &ProviderRuntime {
        &self.runtime
    }

    fn build_payload(&self, messages: Vec<ChatMessage>) -> serde_json::Value {
        json!({
            "model": self.runtime.model,
            "messages": to_openai_messages(messages),
            "stream": true,
            "extra_body": {
                "enable_thinking": false,
                "provider": {
                    "only": [],
                    "order": [],
                    "sort": null,
                    "input_price_range": [0, 0],
                    "output_price_range": [0, 0],
                    "input_length_range": [],
                    "throughput_range": [],
                    "latency_range": []
                }
            }
        })
    }

    fn parse_response_body(&self, body: &str) -> Result<String> {
        let mut full_content = String::new();
        let mut chunk_count = 0usize;

        for line in body.lines() {
            let line = line.trim();
            if line.is_empty() || line == "data: [DONE]" {
                continue;
            }

            let Some(json_str) = line.strip_prefix("data: ") else {
                continue;
            };

            let Ok(value) = serde_json::from_str::<serde_json::Value>(json_str) else {
                continue;
            };
            chunk_count += 1;

            let piece = value
                .get("choices")
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.first())
                .and_then(|c| c.get("delta"))
                .and_then(|d| d.get("content"))
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if !piece.is_empty() {
                full_content.push_str(piece);
            }
        }

        debug!(
            provider = self.runtime.provider_name,
            chunks = chunk_count,
            content_size = full_content.len(),
            "Parsed provider streaming response"
        );

        if !full_content.trim().is_empty() {
            return Ok(full_content.trim().to_string());
        }

        let upper = body.to_uppercase();
        if upper.contains("YES") {
            return Ok("YES".to_string());
        }
        if upper.contains("NO") {
            return Ok("NO".to_string());
        }
        Err(GscholarError::Parse("Empty AIPing response content".to_string()))
    }
}

#[async_trait]
impl LlmProvider for AiPingProvider {
    fn name(&self) -> &str {
        self.runtime.provider_name
    }

    async fn chat_completion(&self, messages: Vec<ChatMessage>) -> Result<String> {
        self.execute_trial(messages).await
    }
}

pub(crate) fn register(registry: &mut ProviderRegistry) {
    registry.insert("aiping".to_string(), build_provider);
}

fn build_provider(cfg: &crate::server::config::ProviderConfig) -> Result<Arc<dyn LlmProvider>> {
    Ok(Arc::new(AiPingProvider::with_model_and_endpoint(
        Some(&cfg.api_key),
        &cfg.model,
        cfg.endpoint.as_deref(),
    )?))
}
