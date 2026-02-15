//! Shared HTTP trial template for LLM providers.
//!
//! Core only handles:
//! - HTTP execution (`reqwest`)
//! - common headers
//! - status/error handling
//! - tracing logs
//!
//! Provider-specific response post-processing must be implemented
//! in each provider's `parse_response_body`.

use super::ChatMessage;
use crate::error::{GscholarError, Result};
use async_trait::async_trait;
use reqwest::{Client, RequestBuilder};
use serde_json::{json, Value};
use std::time::Duration;
use tracing::debug;

const DEFAULT_TIMEOUT_SECS: u64 = 60;

#[derive(Debug, Clone)]
pub struct ProviderRuntime {
    pub provider_name: &'static str,
    pub endpoint: String,
    pub api_key: String,
    pub model: String,
    pub client: Client,
}

impl ProviderRuntime {
    pub fn new(
        provider_name: &'static str,
        endpoint: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
    ) -> Result<Self> {
        Ok(Self {
            provider_name,
            endpoint: endpoint.into(),
            api_key: api_key.into(),
            model: model.into(),
            client: Client::builder()
                .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
                .build()?,
        })
    }
}

#[async_trait]
pub trait ProviderTrial: Send + Sync {
    fn runtime(&self) -> &ProviderRuntime;
    fn build_payload(&self, messages: Vec<ChatMessage>) -> Value;
    fn parse_response_body(&self, body: &str) -> Result<String>;

    fn customize_request(&self, request: RequestBuilder) -> RequestBuilder {
        request
    }

    async fn execute_trial(&self, messages: Vec<ChatMessage>) -> Result<String> {
        let runtime = self.runtime();
        let payload = self.build_payload(messages);
        let payload_size = serde_json::to_string(&payload).map(|s| s.len()).unwrap_or(0);

        debug!(
            provider = runtime.provider_name,
            model = %runtime.model,
            endpoint = %runtime.endpoint,
            payload_size = payload_size,
            "Sending LLM provider request"
        );

        let request = runtime
            .client
            .post(&runtime.endpoint)
            .header("Authorization", format!("Bearer {}", runtime.api_key))
            .header("Content-Type", "application/json")
            .json(&payload);

        let request = self.customize_request(request);
        let response = request.send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(GscholarError::Api {
                code: status.as_u16() as i32,
                message: format!("{} API error: {} - {}", runtime.provider_name, status, body),
            });
        }

        let body = response.text().await?;
        debug!(
            provider = runtime.provider_name,
            body_size = body.len(),
            "Received LLM provider response body"
        );
        self.parse_response_body(&body)
    }
}

pub fn to_openai_messages(messages: Vec<ChatMessage>) -> Vec<Value> {
    messages
        .into_iter()
        .map(|m| json!({ "role": m.role, "content": m.content }))
        .collect()
}
