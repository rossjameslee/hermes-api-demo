use crate::ebay::listing::{EbayListingDraft, PackageWeightAndSizePayload};
use crate::ebay::taxonomy::{Aspect, AspectMode, ItemCardinality, TaxonomyResponse};
use crate::hsuf::measurements::{
    quantitative_length_to_inches, quantitative_weight_to_pounds, round_one, round_two,
};
use crate::hsuf::models::{ImageField, Offer, Product, SizeField};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use thiserror::Error;

#[derive(Debug, Clone, Copy)]
pub struct HsufListingContext<'a> {
    pub taxonomy: &'a TaxonomyResponse,
    pub category_id: &'a str,
    pub default_currency: &'a str,
}

#[derive(Debug, Error)]
pub enum TransformError {
    #[error("offer missing price information")]
    MissingPrice,
    #[error("product image set is empty")]
    MissingImages,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Default)]
pub struct ListingPatch {
    pub condition: Option<String>,
    pub aspects: BTreeMap<String, Vec<String>>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Default)]
pub struct ListingPatchResponse {}

pub fn build_listing_draft(
    product: &Product,
    ctx: HsufListingContext<'_>,
) -> Result<EbayListingDraft, TransformError> {
    let (price, currency) = extract_price(&product.offers, ctx.default_currency)?;
    let images = extract_images(&product.image)?;
    let aspects = build_aspects(product, ctx.taxonomy);
    let description = product
        .description
        .clone()
        .unwrap_or_else(|| build_fallback_description(product));

    Ok(EbayListingDraft {
        sku: product.sku.clone().unwrap_or_else(|| "hsuf-sku".into()),
        title: truncate(&product.name, 80),
        description: truncate(&description, 50000),
        price,
        currency,
        category_id: ctx.category_id.to_string(),
        quantity: 1,
        aspects,
        images,
    })
}

pub fn estimate_package(product: &Product) -> Option<PackageWeightAndSizePayload> {
    let height = quantitative_length_to_inches(&product.height)?;
    let width = quantitative_length_to_inches(&product.width)?;
    let length = quantitative_length_to_inches(&product.depth)?;
    let weight = quantitative_weight_to_pounds(&product.weight)?;

    Some(PackageWeightAndSizePayload {
        package_weight: crate::ebay::listing::WeightPayload {
            value: round_two(weight.max(0.1)),
            unit: "POUND",
        },
        package_size: crate::ebay::listing::DimensionsPayload {
            height: round_one(height),
            length: round_one(length),
            width: round_one(width),
            unit: "INCH",
        },
    })
}

fn extract_price(offer: &Offer, default_currency: &str) -> Result<(f64, String), TransformError> {
    if let Some(price) = offer.price {
        let currency = offer
            .priceCurrency
            .as_deref()
            .unwrap_or(default_currency)
            .to_uppercase();
        return Ok((price, currency));
    }

    if let Some(spec) = &offer.priceSpecification
        && let Some(value) = spec.price
    {
        let currency = spec
            .priceCurrency
            .as_deref()
            .unwrap_or(default_currency)
            .to_uppercase();
        return Ok((value, currency));
    }

    Err(TransformError::MissingPrice)
}

fn extract_images(images: &ImageField) -> Result<Vec<String>, TransformError> {
    let cleaned: Vec<String> = images
        .as_vec()
        .into_iter()
        .filter(|value| !value.trim().is_empty())
        .collect();
    if cleaned.is_empty() {
        return Err(TransformError::MissingImages);
    }
    Ok(cleaned)
}

fn build_aspects(product: &Product, taxonomy: &TaxonomyResponse) -> BTreeMap<String, Vec<String>> {
    let mut values = BTreeMap::new();
    for aspect in &taxonomy.aspects {
        let name = aspect.localizedAspectName.trim();
        if name.is_empty() {
            continue;
        }
        let candidates = hsuf_values_for_aspect(product, aspect);
        if candidates.is_empty() {
            continue;
        }

        let filtered = apply_constraints(&candidates, aspect);
        if filtered.is_empty() {
            continue;
        }

        let cardinality = ItemCardinality::from_raw(
            aspect
                .aspectConstraint
                .as_ref()
                .and_then(|c| c.itemToAspectCardinality.as_deref()),
        );

        let stored = match cardinality {
            ItemCardinality::Multi => filtered,
            ItemCardinality::Single => vec![filtered[0].clone()],
        };
        values.insert(name.to_string(), stored);
    }
    values
}

fn hsuf_values_for_aspect(product: &Product, aspect: &Aspect) -> Vec<String> {
    let normalized = aspect.localizedAspectName.trim().to_lowercase();
    match normalized.as_str() {
        "brand" | "manufacturer" => extract_brand(product),
        "color" | "main color" => split_field(product.color.as_deref()),
        "mpn" => product
            .mpn
            .as_ref()
            .map(|value| vec![value.clone()])
            .unwrap_or_default(),
        "sku" => product
            .sku
            .as_ref()
            .map(|value| vec![value.clone()])
            .unwrap_or_default(),
        _ => vec![],
    }
}

fn extract_brand(product: &Product) -> Vec<String> {
    product
        .brand
        .as_ref()
        .and_then(|brand| brand.name.clone())
        .map(|value| vec![value])
        .unwrap_or_default()
}

fn split_field(value: Option<&str>) -> Vec<String> {
    let Some(raw) = value else { return vec![] };
    raw.split(['/', '|', ',', '&', '\n'])
        .map(|segment| segment.trim())
        .filter(|segment| !segment.is_empty())
        .map(|segment| segment.to_string())
        .collect()
}

fn apply_constraints(values: &[String], aspect: &Aspect) -> Vec<String> {
    let Some(constraint) = &aspect.aspectConstraint else {
        return values.to_vec();
    };
    if let Some(mode) = constraint
        .aspectMode
        .as_deref()
        .and_then(AspectMode::from_raw)
        && mode == AspectMode::SelectionOnly
    {
        let mut allowed = HashMap::new();
        for val in &aspect.aspectValues {
            allowed.insert(
                normalize_text(&val.localizedValue),
                val.localizedValue.trim().to_string(),
            );
        }
        let mut matched = Vec::new();
        for candidate in values {
            if let Some(value) = allowed.get(&normalize_text(candidate)) {
                matched.push(value.clone());
            }
        }
        return matched;
    }
    values.to_vec()
}

fn normalize_text(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn build_fallback_description(product: &Product) -> String {
    let mut lines = Vec::new();
    if let Some(brand) = product.brand.as_ref().and_then(|b| b.name.clone()) {
        lines.push(format!("Brand: {brand}"));
    }
    if let Some(color) = &product.color {
        lines.push(format!("Color: {color}"));
    }
    if let Some(material) = &product.material {
        lines.push(format!("Material: {material}"));
    }
    if let Some(size) = product.size.as_ref().and_then(resolve_size) {
        lines.push(format!("Size: {size}"));
    }
    if let Some(desc) = &product.description {
        lines.push(desc.clone());
    }
    if lines.is_empty() {
        product.name.clone()
    } else {
        lines.join("\n")
    }
}

fn resolve_size(size: &SizeField) -> Option<String> {
    match size {
        SizeField::Text(value) => Some(value.clone()),
        SizeField::Quantitative(value) => value.value.map(|v| v.to_string()),
        SizeField::Specification(spec) => spec.name.clone(),
    }
}

fn truncate(value: &str, limit: usize) -> String {
    if value.len() <= limit {
        value.to_string()
    } else {
        format!("{}...", value[..limit.saturating_sub(3)].trim())
    }
}
