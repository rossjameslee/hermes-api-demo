#![allow(dead_code)]
#![allow(non_snake_case)]

use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

#[skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Brand {
    pub name: Option<String>,
}

#[skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QuantitativeValue {
    pub unitCode: Option<String>,
    pub unitText: Option<String>,
    pub value: Option<f64>,
}

#[skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SizeSpecification {
    pub name: Option<String>,
    pub sizeGroup: Option<String>,
    pub sizeSystem: Option<String>,
}

#[skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Offer {
    pub price: Option<f64>,
    pub priceCurrency: Option<String>,
    #[serde(default)]
    pub priceSpecification: Option<UnitPriceSpecification>,
}

#[skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UnitPriceSpecification {
    pub price: Option<f64>,
    pub priceCurrency: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ImageField {
    Single(String),
    Multiple(Vec<String>),
}

impl ImageField {
    pub fn as_vec(&self) -> Vec<String> {
        match self {
            ImageField::Single(value) => vec![value.clone()],
            ImageField::Multiple(values) => values.clone(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum SizeField {
    Text(String),
    Quantitative(QuantitativeValue),
    Specification(SizeSpecification),
}

#[skip_serializing_none]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Product {
    pub name: String,
    pub image: ImageField,
    pub offers: Offer,
    pub description: Option<String>,
    pub brand: Option<Brand>,
    pub color: Option<String>,
    pub material: Option<String>,
    pub size: Option<SizeField>,
    pub sku: Option<String>,
    pub mpn: Option<String>,
    pub height: Option<QuantitativeValue>,
    pub width: Option<QuantitativeValue>,
    pub depth: Option<QuantitativeValue>,
    pub weight: Option<QuantitativeValue>,
}
