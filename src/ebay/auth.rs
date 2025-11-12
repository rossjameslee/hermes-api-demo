#![allow(dead_code)]

use crate::ebay::config::{APP_ID, APP_SECRET, OAUTH_TOKEN_URL};
use crate::http::build_client;
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use reqwest::Client;
use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EbayAuthError {
    #[error("missing ebay app credentials in env")]
    MissingCredentials,
    #[error("oauth request failed: {0}")]
    Request(String),
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
}

fn basic_auth_header() -> Result<String, EbayAuthError> {
    if APP_ID.is_empty() || APP_SECRET.is_empty() {
        return Err(EbayAuthError::MissingCredentials);
    }
    let raw = format!("{}:{}", *APP_ID, *APP_SECRET);
    Ok(BASE64.encode(raw))
}

pub async fn get_app_access_token(scopes: &[&str]) -> Result<String, EbayAuthError> {
    basic_auth_header()?;
    let body = [
        ("grant_type", "client_credentials"),
        ("scope", &scopes.join(" ")),
    ];
    request_token(&body).await
}

pub async fn get_user_access_token_from_refresh(
    refresh_token: &str,
    scopes: &[&str],
) -> Result<String, EbayAuthError> {
    basic_auth_header()?;
    let body = [
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
        ("scope", &scopes.join(" ")),
    ];
    request_token(&body).await
}

async fn request_token(params: &[(&str, &str)]) -> Result<String, EbayAuthError> {
    let client = build_client();
    let response = client
        .post(OAUTH_TOKEN_URL.as_str())
        .basic_auth(APP_ID.as_str(), Some(APP_SECRET.as_str()))
        .form(&params)
        .send()
        .await
        .map_err(|err| EbayAuthError::Request(err.to_string()))?;

    if !response.status().is_success() {
        return Err(EbayAuthError::Request(format!(
            "HTTP {}",
            response.status()
        )));
    }

    let payload: TokenResponse = response
        .json()
        .await
        .map_err(|err| EbayAuthError::Request(err.to_string()))?;
    Ok(payload.access_token)
}
