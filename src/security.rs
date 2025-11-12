use crate::models::ApiError;
use axum::{
    Json,
    body::Body,
    extract::State,
    http::{self, Request, StatusCode, header::HeaderValue},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use std::{collections::HashMap, convert::Infallible, env, sync::Arc, time::Instant};
use tokio::sync::Mutex;
use tracing::{info, warn};

#[derive(Clone)]
pub struct AuthState {
    records: Arc<HashMap<String, OrgRecord>>,
    limiter: Arc<TokenBuckets>,
}

#[derive(Clone, Debug)]
pub struct AuthContext {
    pub org_id: String,
    pub api_key_id: String,
}

#[derive(Clone)]
struct OrgRecord {
    org_id: String,
    api_key_id: String,
}

impl AuthState {
    pub fn from_env() -> Self {
        let records = Arc::new(load_keys_from_env());
        let limiter = Arc::new(TokenBuckets::from_env());
        Self { records, limiter }
    }

    fn authenticate(&self, presented: &str) -> Option<AuthContext> {
        self.records.get(presented).map(|record| AuthContext {
            org_id: record.org_id.clone(),
            api_key_id: record.api_key_id.clone(),
        })
    }

    async fn consume(&self, org_id: &str) -> Result<RatePermit, RateExceeded> {
        self.limiter.consume(org_id).await
    }
}

pub async fn require_api_auth(
    State(state): State<AuthState>,
    mut request: Request<Body>,
    next: Next,
) -> Result<Response, Infallible> {
    let Some(presented) = extract_api_key(request.headers()) else {
        let response =
            unauthorized_response("missing_api_key", "Provide X-Hermes-Key or Bearer token");
        return Ok(response);
    };

    let Some(context) = state.authenticate(&presented) else {
        let response = unauthorized_response("invalid_api_key", "Key not recognized");
        return Ok(response);
    };

    match state.consume(&context.org_id).await {
        Ok(permit) => {
            request.extensions_mut().insert(context.clone());
            let mut response = next.run(request).await;
            permit.apply_headers(response.headers_mut());
            Ok(response)
        }
        Err(exceeded) => {
            let mut response = too_many_requests("rate_limited", "Too many requests");
            exceeded.apply_headers(response.headers_mut());
            Ok(response)
        }
    }
}

fn extract_api_key(headers: &http::HeaderMap) -> Option<String> {
    if let Some(value) = headers.get(http::header::AUTHORIZATION)
        && let Ok(raw) = value.to_str()
        && raw.len() >= 7
        && raw[..6].eq_ignore_ascii_case("bearer")
    {
        return Some(raw[6..].trim().to_string());
    }
    headers
        .get("X-Hermes-Key")
        .and_then(|value| value.to_str().ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn unauthorized_response(code: &str, message: &str) -> Response {
    let payload = ApiError {
        error: code.to_string(),
        detail: Some(message.to_string()),
    };
    (StatusCode::UNAUTHORIZED, Json(payload)).into_response()
}

fn too_many_requests(code: &str, message: &str) -> Response {
    let payload = ApiError {
        error: code.to_string(),
        detail: Some(message.to_string()),
    };
    (StatusCode::TOO_MANY_REQUESTS, Json(payload)).into_response()
}

fn load_keys_from_env() -> HashMap<String, OrgRecord> {
    let raw = env::var("DEMO_API_KEYS").unwrap_or_else(|_| "demo-org:demo-key".to_string());
    let mut entries = HashMap::new();
    for (idx, token) in raw.split(',').enumerate() {
        let trimmed = token.trim();
        if trimmed.is_empty() {
            continue;
        }
        let mut parts = trimmed.splitn(2, ':');
        let org_id = parts.next().map(str::trim).filter(|s| !s.is_empty());
        let key = parts.next().map(str::trim).filter(|s| !s.is_empty());
        match (org_id, key) {
            (Some(org), Some(secret)) => {
                let record = OrgRecord {
                    org_id: org.to_string(),
                    api_key_id: format!("key-{:02}", idx + 1),
                };
                entries.insert(secret.to_string(), record);
            }
            _ => warn!(
                target = "hermes.api",
                "ignored malformed DEMO_API_KEYS entry: {trimmed}"
            ),
        }
    }

    if entries.is_empty() {
        warn!(
            target = "hermes.api",
            "DEMO_API_KEYS produced no keys; falling back to demo credentials"
        );
        entries.insert(
            "demo-key".to_string(),
            OrgRecord {
                org_id: "demo-org".to_string(),
                api_key_id: "key-01".to_string(),
            },
        );
    } else {
        info!(
            target = "hermes.api",
            key_count = entries.len(),
            "loaded API keys from env"
        );
    }

    entries
}

#[derive(Clone)]
struct TokenBuckets {
    rate_per_sec: f64,
    capacity: f64,
    buckets: Arc<Mutex<HashMap<String, BucketState>>>,
}

impl TokenBuckets {
    fn from_env() -> Self {
        let rate_per_sec = env::var("RATE_LIMIT_PER_SEC")
            .ok()
            .and_then(|value| value.parse::<f64>().ok())
            .filter(|value| *value > 0.0)
            .unwrap_or(5.0);
        let capacity = env::var("RATE_LIMIT_CAPACITY")
            .ok()
            .and_then(|value| value.parse::<f64>().ok())
            .filter(|value| *value >= 1.0)
            .unwrap_or(10.0);
        Self {
            rate_per_sec,
            capacity,
            buckets: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    async fn consume(&self, key: &str) -> Result<RatePermit, RateExceeded> {
        let mut guard = self.buckets.lock().await;
        let now = Instant::now();
        let state = guard.entry(key.to_string()).or_insert_with(|| BucketState {
            tokens: self.capacity,
            last_refill: now,
        });

        let elapsed = now.duration_since(state.last_refill).as_secs_f64();
        if elapsed > 0.0 {
            state.tokens = (state.tokens + elapsed * self.rate_per_sec).min(self.capacity);
            state.last_refill = now;
        }

        if state.tokens >= 1.0 {
            state.tokens -= 1.0;
            Ok(RatePermit {
                capacity: self.capacity,
                tokens: state.tokens,
                rate: self.rate_per_sec,
            })
        } else {
            let deficit = 1.0 - state.tokens;
            let retry_after = (deficit / self.rate_per_sec).max(0.0);
            Err(RateExceeded {
                retry_after,
                capacity: self.capacity,
                tokens: state.tokens,
                rate: self.rate_per_sec,
            })
        }
    }
}

struct BucketState {
    tokens: f64,
    last_refill: Instant,
}

#[derive(Debug, Clone, Serialize)]
pub struct RatePermit {
    capacity: f64,
    tokens: f64,
    rate: f64,
}

impl RatePermit {
    fn apply_headers(&self, headers: &mut http::HeaderMap) {
        let remaining = self.tokens.max(0.0).floor() as u64;
        let reset = ((self.capacity - self.tokens) / self.rate).ceil().max(0.0) as u64;
        headers.insert(
            "X-RateLimit-Limit",
            HeaderValue::from_str(&(self.capacity as u64).to_string())
                .unwrap_or_else(|_| HeaderValue::from_static("0")),
        );
        headers.insert(
            "X-RateLimit-Remaining",
            HeaderValue::from_str(&remaining.to_string())
                .unwrap_or_else(|_| HeaderValue::from_static("0")),
        );
        headers.insert(
            "X-RateLimit-Reset",
            HeaderValue::from_str(&reset.to_string())
                .unwrap_or_else(|_| HeaderValue::from_static("0")),
        );
    }
}

#[derive(Debug, Clone)]
pub struct RateExceeded {
    retry_after: f64,
    capacity: f64,
    tokens: f64,
    rate: f64,
}

impl RateExceeded {
    fn apply_headers(&self, headers: &mut http::HeaderMap) {
        let retry = self.retry_after.ceil().max(0.0) as u64;
        headers.insert(
            http::header::RETRY_AFTER,
            HeaderValue::from_str(&retry.to_string())
                .unwrap_or_else(|_| HeaderValue::from_static("1")),
        );
        headers.insert(
            "X-RateLimit-Limit",
            HeaderValue::from_str(&(self.capacity as u64).to_string())
                .unwrap_or_else(|_| HeaderValue::from_static("0")),
        );
        headers.insert("X-RateLimit-Remaining", HeaderValue::from_static("0"));
        let reset = ((self.capacity - self.tokens) / self.rate).ceil().max(0.0) as u64;
        headers.insert(
            "X-RateLimit-Reset",
            HeaderValue::from_str(&reset.to_string())
                .unwrap_or_else(|_| HeaderValue::from_static("0")),
        );
    }
}
