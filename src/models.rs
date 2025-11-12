use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ListingRequest {
    pub images_source: ImagesSource,
    pub sku: String,
    pub merchant_location_key: String,
    pub fulfillment_policy_id: String,
    pub payment_policy_id: String,
    pub return_policy_id: String,
    #[serde(default)]
    pub marketplace: MarketplaceId,
    #[serde(default)]
    #[allow(dead_code)]
    pub llm_provider: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    pub llm_listing_model: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    pub llm_category_model: Option<String>,
    #[serde(default)]
    pub use_signed_urls: bool,
    #[serde(default)]
    pub overrides: Option<PipelineOverrides>,
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ListingResponse {
    pub listing_id: String,
    pub stages: Vec<StageReport>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct StageReport {
    pub name: String,
    pub elapsed_ms: u128,
    pub timestamp: DateTime<Utc>,
    pub output: Value,
}

impl StageReport {
    pub fn new(name: &str, elapsed_ms: u128, output: Value) -> Self {
        Self {
            name: name.to_string(),
            elapsed_ms,
            timestamp: Utc::now(),
            output,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ApiError {
    pub error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CategorySelectionInput {
    pub id: String,
    pub tree_id: String,
    pub label: String,
    pub confidence: f32,
    pub rationale: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PipelineOverrides {
    #[serde(default)]
    pub resolved_images: Option<Vec<String>>,
    #[serde(default)]
    pub category: Option<CategorySelectionInput>,
    #[serde(default)]
    pub product: Option<Value>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq, Hash)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[allow(clippy::enum_variant_names)]
pub enum MarketplaceId {
    #[default]
    EbayUs,
    EbayUk,
    EbayDe,
}

impl MarketplaceId {
    pub fn ebay_code(&self) -> &'static str {
        match self {
            MarketplaceId::EbayUs => "EBAY_US",
            MarketplaceId::EbayUk => "EBAY_GB",
            MarketplaceId::EbayDe => "EBAY_DE",
        }
    }

    pub fn from_str(input: &str) -> Option<Self> {
        match input.trim().to_uppercase().as_str() {
            "EBAY_US" => Some(MarketplaceId::EbayUs),
            "EBAY_GB" | "EBAY_UK" => Some(MarketplaceId::EbayUk),
            "EBAY_DE" => Some(MarketplaceId::EbayDe),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum ImagesSource {
    Single(String),
    Multiple(Vec<String>),
}
