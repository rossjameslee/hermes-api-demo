use crate::http::build_client;
use reqwest::Client;
use serde::Deserialize;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct SupabaseClient {
    base_url: String,
    service_key: String,
    http: Client,
}

#[derive(Debug, Error)]
pub enum SupabaseError {
    #[error("request failed: {0}")]
    Request(String),
    #[error("invalid response: {0}")]
    Deserialize(String),
}

#[derive(Debug, Clone, Deserialize)]
pub struct EbayOrgConfig {
    #[allow(dead_code)]
    pub org_id: Uuid,
    pub merchant_location_key: String,
    pub fulfillment_policy_id: String,
    pub payment_policy_id: String,
    pub return_policy_id: String,
    pub marketplace: Option<String>,
    pub location_name: Option<String>,
    pub address_line1: Option<String>,
    pub address_line2: Option<String>,
    pub city: Option<String>,
    pub state_or_province: Option<String>,
    pub postal_code: Option<String>,
    pub country: Option<String>,
    pub latitude: Option<String>,
    pub longitude: Option<String>,
}

impl SupabaseClient {
    pub fn from_env() -> Option<Self> {
        let base_url = std::env::var("SUPABASE_URL").ok()?;
        let service_key = std::env::var("SUPABASE_SERVICE_ROLE_KEY")
            .or_else(|_| std::env::var("SUPABASE_SERVICE_KEY"))
            .or_else(|_| std::env::var("SUPABASE_KEY"))
            .ok()?;
        Some(Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            service_key,
            http: build_client(),
        })
    }

    pub async fn fetch_ebay_org_config(
        &self,
        org_id: Uuid,
    ) -> Result<Option<EbayOrgConfig>, SupabaseError> {
        let url = format!(
            "{}/rest/v1/ebay_org_config?org_id=eq.{}&select=*&limit=1",
            self.base_url, org_id
        );
        let response = self
            .http
            .get(url)
            .header("apikey", &self.service_key)
            .header("Authorization", format!("Bearer {}", self.service_key))
            .send()
            .await
            .map_err(|err| SupabaseError::Request(err.to_string()))?;

        if !response.status().is_success() {
            return Err(SupabaseError::Request(format!(
                "HTTP {}",
                response.status()
            )));
        }

        let mut payload: Vec<EbayOrgConfig> = response
            .json()
            .await
            .map_err(|err| SupabaseError::Deserialize(err.to_string()))?;
        Ok(payload.pop())
    }
}
