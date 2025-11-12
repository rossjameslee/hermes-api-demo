#![allow(dead_code)]
#![allow(non_snake_case)]

use crate::ebay::config::ROOT;
use crate::ebay::listing::{ListingPolicies, PackageWeightAndSizePayload};
use crate::http::build_client;
use reqwest::Client;
use serde::Serialize;
use std::collections::BTreeMap;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EbayOfferError {
    #[error("request failed: {0}")]
    Request(String),
    #[error("entity already exists")]
    EntityExists,
}

#[derive(Debug, Clone, Serialize)]
pub struct PricingSummary {
    pub price: Price,
}

#[derive(Debug, Clone, Serialize)]
pub struct Price {
    pub value: String,
    pub currency: String,
}

impl Price {
    pub fn from_amount(amount: f64, currency: &str) -> Self {
        Self {
            value: format!("{amount:.2}"),
            currency: currency.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateOfferRequest {
    pub sku: String,
    pub marketplace_id: String,
    #[serde(default = "CreateOfferRequest::default_format")]
    pub format: &'static str,
    pub category_id: String,
    pub listing_description: String,
    pub pricing_summary: PricingSummary,
    pub available_quantity: i32,
    pub merchant_location_key: String,
    pub listing_policies: ListingPolicies,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub aspects: BTreeMap<String, Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package_weight_and_size: Option<PackageWeightAndSizePayload>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub image_urls: Vec<String>,
}

impl CreateOfferRequest {
    fn default_format() -> &'static str {
        "FIXED_PRICE"
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateOfferRequest {
    #[serde(default = "CreateOfferRequest::default_format")]
    pub format: &'static str,
    pub category_id: String,
    pub listing_description: String,
    pub pricing_summary: PricingSummary,
    pub available_quantity: i32,
    pub listing_policies: ListingPolicies,
    pub merchant_location_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package_weight_and_size: Option<PackageWeightAndSizePayload>,
}

pub async fn create_offer(
    request: &CreateOfferRequest,
    access_token: &str,
) -> Result<String, EbayOfferError> {
    let client = build_client();
    let url = format!("{}/sell/inventory/v1/offer", *ROOT);
    let response = client
        .post(url)
        .bearer_auth(access_token)
        .json(request)
        .send()
        .await
        .map_err(|err| EbayOfferError::Request(err.to_string()))?;
    if response.status() == 409 {
        return Err(EbayOfferError::EntityExists);
    }
    if !response.status().is_success() {
        return Err(EbayOfferError::Request(format!(
            "HTTP {}",
            response.status()
        )));
    }
    #[derive(serde::Deserialize)]
    struct OfferResponse {
        offerId: String,
    }
    let payload: OfferResponse = response
        .json()
        .await
        .map_err(|err| EbayOfferError::Request(err.to_string()))?;
    Ok(payload.offerId)
}

pub async fn publish_offer(offer_id: &str, access_token: &str) -> Result<String, EbayOfferError> {
    let client = build_client();
    let url = format!("{}/sell/inventory/v1/offer/{offer_id}/publish", *ROOT);
    let response = client
        .post(url)
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|err| EbayOfferError::Request(err.to_string()))?;
    if !response.status().is_success() {
        return Err(EbayOfferError::Request(format!(
            "HTTP {}",
            response.status()
        )));
    }
    #[derive(serde::Deserialize)]
    struct PublishResponse {
        listingId: Option<String>,
    }
    let payload: PublishResponse = response
        .json()
        .await
        .map_err(|err| EbayOfferError::Request(err.to_string()))?;
    Ok(payload.listingId.unwrap_or_default())
}

#[derive(Debug, serde::Deserialize)]
pub struct OfferSummary {
    pub offerId: Option<String>,
    pub marketplaceId: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct OfferSearchResponse {
    offers: Option<Vec<OfferSummary>>,
}

pub async fn get_offers_by_sku(
    sku: &str,
    access_token: &str,
) -> Result<Vec<OfferSummary>, EbayOfferError> {
    let client = build_client();
    let url = format!("{}/sell/inventory/v1/offer", *ROOT);
    let response = client
        .get(url)
        .bearer_auth(access_token)
        .query(&[("sku", sku)])
        .send()
        .await
        .map_err(|err| EbayOfferError::Request(err.to_string()))?;
    if !response.status().is_success() {
        return Err(EbayOfferError::Request(format!(
            "HTTP {}",
            response.status()
        )));
    }
    let payload: OfferSearchResponse = response
        .json()
        .await
        .map_err(|err| EbayOfferError::Request(err.to_string()))?;
    Ok(payload.offers.unwrap_or_default())
}

pub async fn update_offer(
    offer_id: &str,
    payload: &UpdateOfferRequest,
    access_token: &str,
) -> Result<(), EbayOfferError> {
    let client = build_client();
    let url = format!("{}/sell/inventory/v1/offer/{offer_id}", *ROOT);
    let response = client
        .put(url)
        .bearer_auth(access_token)
        .json(payload)
        .send()
        .await
        .map_err(|err| EbayOfferError::Request(err.to_string()))?;
    if !response.status().is_success() {
        return Err(EbayOfferError::Request(format!(
            "HTTP {}",
            response.status()
        )));
    }
    Ok(())
}

pub async fn delete_offer(offer_id: &str, access_token: &str) -> Result<(), EbayOfferError> {
    let client = build_client();
    let url = format!("{}/sell/inventory/v1/offer/{offer_id}", *ROOT);
    let response = client
        .delete(url)
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|err| EbayOfferError::Request(err.to_string()))?;
    if !response.status().is_success() {
        return Err(EbayOfferError::Request(format!(
            "HTTP {}",
            response.status()
        )));
    }
    Ok(())
}

pub async fn withdraw_offer(offer_id: &str, access_token: &str) -> Result<(), EbayOfferError> {
    let client = Client::new();
    let url = format!("{}/sell/inventory/v1/offer/{offer_id}/withdraw", *ROOT);
    let response = client
        .post(url)
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|err| EbayOfferError::Request(err.to_string()))?;
    if !response.status().is_success() {
        return Err(EbayOfferError::Request(format!(
            "HTTP {}",
            response.status()
        )));
    }
    Ok(())
}
