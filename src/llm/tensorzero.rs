use crate::http::build_client;
use eyre::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct LlmConfig {
    pub gateway_url: String,
    pub api_key: Option<String>,
    pub function_name: Option<String>,
    pub model: Option<String>,
}

impl LlmConfig {
    pub fn from_env() -> Self {
        Self {
            gateway_url: std::env::var("TENSORZERO_GATEWAY_URL")
                .unwrap_or_else(|_| "http://localhost:3000".into()),
            api_key: std::env::var("TENSORZERO_API_KEY").ok(),
            function_name: std::env::var("TENSORZERO_FUNCTION").ok(),
            model: std::env::var("TENSORZERO_MODEL").ok(),
        }
    }
}

#[derive(Debug, Error)]
pub enum LlmError {
    #[error("missing gateway url")]
    MissingGateway,
    #[error("http error: {0}")]
    Http(String),
    #[error("invalid response: {0}")]
    InvalidResponse(String),
}

#[derive(Debug, Clone, Serialize)]
pub struct LlmMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Deserialize)]
pub struct LlmResponse {
    pub text: String,
    #[allow(dead_code)]
    #[serde(default)]
    pub usage: Option<LlmUsage>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct LlmUsage {
    pub input_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
}

pub struct LlmClient {
    http: Client,
    config: LlmConfig,
}

impl LlmClient {
    pub fn new(config: LlmConfig) -> Self {
        Self {
            http: build_client(),
            config,
        }
    }

    pub async fn chat(&self, messages: &[LlmMessage]) -> Result<LlmResponse, LlmError> {
        let gateway = self.config.gateway_url.trim();
        if gateway.is_empty() {
            return Err(LlmError::MissingGateway);
        }

        let function_name = self
            .config
            .function_name
            .as_deref()
            .unwrap_or("hsuf_enrichment");
        let model_name = self.config.model.as_deref();

        let body = ChatRequest {
            function_name: function_name.to_string(),
            model_name: model_name.map(|value| value.to_string()),
            input: ChatInput {
                messages: messages.to_vec(),
            },
        };

        let mut request = self.http.post(format!("{gateway}/inference")).json(&body);

        if let Some(key) = &self.config.api_key {
            request = request.header("X-API-Key", key);
        }

        let response = request
            .send()
            .await
            .map_err(|err| LlmError::Http(err.to_string()))?;

        if !response.status().is_success() {
            return Err(LlmError::Http(format!("HTTP {}", response.status())));
        }

        let payload: TensorZeroResponse = response
            .json()
            .await
            .map_err(|err| LlmError::InvalidResponse(err.to_string()))?;

        let text = payload
            .content
            .into_iter()
            .find(|item| item.r#type == "text")
            .map(|item| item.text)
            .ok_or_else(|| LlmError::InvalidResponse("missing text".into()))?;

        Ok(LlmResponse {
            text,
            usage: payload.usage,
        })
    }
}

#[derive(Debug, Serialize)]
struct ChatRequest {
    function_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    model_name: Option<String>,
    input: ChatInput,
}

#[derive(Debug, Serialize)]
struct ChatInput {
    messages: Vec<LlmMessage>,
}

#[derive(Debug, Deserialize)]
struct TensorZeroResponse {
    content: Vec<ResponseContent>,
    #[serde(default)]
    usage: Option<LlmUsage>,
}

#[derive(Debug, Deserialize)]
struct ResponseContent {
    r#type: String,
    text: String,
}
