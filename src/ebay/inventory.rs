#![allow(dead_code)]

use crate::ebay::config::ROOT;
use crate::ebay::listing::PackageWeightAndSizePayload;
use crate::http::build_client;
use reqwest::Client;
use serde::Serialize;
use std::collections::BTreeMap;
use thiserror::Error;
use urlencoding::encode;

#[derive(Debug, Error)]
pub enum EbayInventoryError {
    #[error("request failed: {0}")]
    Request(String),
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InventoryItemRequest {
    pub availability: InventoryAvailability,
    pub product: InventoryProduct,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package_weight_and_size: Option<PackageWeightAndSizePayload>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InventoryAvailability {
    pub ship_to_location_availability: ShipToLocationAvailability,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShipToLocationAvailability {
    pub quantity: i32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InventoryProduct {
    pub title: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aspects: Option<BTreeMap<String, Vec<String>>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub image_urls: Vec<String>,
}

pub async fn upsert_inventory_item(
    sku: &str,
    payload: &InventoryItemRequest,
    access_token: &str,
) -> Result<(), EbayInventoryError> {
    let client = build_client();
    let encoded_sku = encode(sku);
    let url = format!("{}/sell/inventory/v1/inventory_item/{}", *ROOT, encoded_sku);
    let response = client
        .put(url)
        .bearer_auth(access_token)
        .json(payload)
        .send()
        .await
        .map_err(|err| EbayInventoryError::Request(err.to_string()))?;

    if !response.status().is_success() {
        return Err(EbayInventoryError::Request(format!(
            "HTTP {}",
            response.status()
        )));
    }

    Ok(())
}

pub async fn upsert_inventory_location(
    merchant_location_key: &str,
    payload: &InventoryLocationRequest,
    access_token: &str,
) -> Result<(), EbayInventoryError> {
    let client = build_client();
    let encoded_key = encode(merchant_location_key);
    let url = format!("{}/sell/inventory/v1/location/{}", *ROOT, encoded_key);
    let response = client
        .put(url)
        .bearer_auth(access_token)
        .json(payload)
        .send()
        .await
        .map_err(|err| EbayInventoryError::Request(err.to_string()))?;
    if !response.status().is_success() {
        return Err(EbayInventoryError::Request(format!(
            "HTTP {}",
            response.status()
        )));
    }
    Ok(())
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InventoryLocationRequest {
    pub merchant_location_status: &'static str,
    pub location_types: Vec<&'static str>,
    pub name: String,
    pub location: LocationDetails,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocationDetails {
    pub address: LocationAddress,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub geo_coordinates: Option<LocationGeo>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocationAddress {
    pub address_line1: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address_line2: Option<String>,
    pub city: String,
    pub state_or_province: String,
    pub postal_code: String,
    pub country: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocationGeo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latitude: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub longitude: Option<String>,
}
