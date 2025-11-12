#![allow(dead_code)]
#![allow(non_snake_case)]

use crate::ebay::config::{DEFAULT_CATEGORY_TREE_ID, ROOT};
use crate::http::build_client;
use reqwest::Client;
use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EbayTaxonomyError {
    #[error("taxonomy request failed: {0}")]
    Request(String),
}

#[derive(Debug, Clone, Deserialize)]
pub struct TaxonomyResponse {
    #[serde(default)]
    pub aspects: Vec<Aspect>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Aspect {
    pub localizedAspectName: String,
    #[serde(default)]
    pub aspectValues: Vec<AspectValue>,
    #[serde(default)]
    pub aspectConstraint: Option<AspectConstraint>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AspectValue {
    pub localizedValue: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AspectConstraint {
    #[serde(default)]
    pub aspectMode: Option<String>,
    #[serde(default)]
    pub aspectRequired: Option<bool>,
    #[serde(default)]
    pub itemToAspectCardinality: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AspectMode {
    FreeText,
    SelectionOnly,
}

impl AspectMode {
    pub fn from_raw(value: &str) -> Option<Self> {
        match value.to_uppercase().as_str() {
            "FREE_TEXT" => Some(Self::FreeText),
            "SELECTION_ONLY" => Some(Self::SelectionOnly),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemCardinality {
    Multi,
    Single,
}

impl ItemCardinality {
    pub fn from_raw(value: Option<&str>) -> Self {
        match value.unwrap_or("").to_uppercase().as_str() {
            "MULTI" => Self::Multi,
            _ => Self::Single,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EbayCondition {
    New,
    NewOther,
    NewWithDefects,
    CertifiedRefurbished,
    Used,
    UsedVeryGood,
    UsedGood,
    UsedAcceptable,
    ForParts,
}

pub async fn fetch_category_aspects(
    category_id: &str,
    access_token: &str,
) -> Result<TaxonomyResponse, EbayTaxonomyError> {
    let client = build_client();
    let url = format!(
        "{}/commerce/taxonomy/v1/category_tree/{}/get_item_aspects_for_category",
        *ROOT, *DEFAULT_CATEGORY_TREE_ID
    );
    let response = client
        .get(url)
        .query(&[("category_id", category_id)])
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|err| EbayTaxonomyError::Request(err.to_string()))?;

    if !response.status().is_success() {
        return Err(EbayTaxonomyError::Request(format!(
            "HTTP {}",
            response.status()
        )));
    }

    response
        .json::<TaxonomyResponse>()
        .await
        .map_err(|err| EbayTaxonomyError::Request(err.to_string()))
}
