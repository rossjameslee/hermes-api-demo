#![allow(dead_code)]

use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListingPolicies {
    pub fulfillment_policy_id: String,
    pub payment_policy_id: String,
    pub return_policy_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EbayListingDraft {
    pub sku: String,
    pub title: String,
    pub description: String,
    pub price: f64,
    pub currency: String,
    pub category_id: String,
    pub quantity: i32,
    pub aspects: BTreeMap<String, Vec<String>>,
    pub images: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageWeightAndSizePayload {
    pub package_weight: WeightPayload,
    pub package_size: DimensionsPayload,
}

#[derive(Debug, Clone, Serialize)]
pub struct WeightPayload {
    pub value: f64,
    pub unit: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct DimensionsPayload {
    pub height: f64,
    pub length: f64,
    pub width: f64,
    pub unit: &'static str,
}
