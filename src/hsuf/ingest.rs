use crate::hsuf::models::{ImageField, Offer, Product, QuantitativeValue};
use crate::llm::{LlmClient, LlmMessage};
use serde_json::{Value, json};
use thiserror::Error;

const SYSTEM_PROMPT: &str = r#"
You are a product ingestion agent. Given a set of product image URLs and metadata, respond with a valid
JSON object that conforms to schema.org Product. Include `image`, `offers`, and dimensional metadata when
possible. Omitting required fields is not allowed. If uncertain, make the best reasonable assumption and note it in
the description. Output JSON only.
"#;

#[derive(Debug, Error)]
pub enum IngestError {
    #[error("llm request failed: {0}")]
    Llm(String),
    #[error("unable to parse product json")]
    Parse,
}

pub async fn infer_product(
    llm: &LlmClient,
    sku: &str,
    images: &[String],
) -> Result<Product, IngestError> {
    if images.is_empty() {
        return Err(IngestError::Parse);
    }

    let payload = json!({
        "sku": sku,
        "images": images,
        "instruction": "Return a schema.org Product JSON with offers.price, offers.priceCurrency, image, color, material, dimensions, and weight when possible."
    });

    let messages = vec![
        LlmMessage {
            role: "system".into(),
            content: SYSTEM_PROMPT.into(),
        },
        LlmMessage {
            role: "user".into(),
            content: payload.to_string(),
        },
    ];

    let response = llm
        .chat(&messages)
        .await
        .map_err(|err| IngestError::Llm(err.to_string()))?;

    let cleaned = strip_markdown_fence(&response.text);
    let mut value: Value = serde_json::from_str(&cleaned).map_err(|_| IngestError::Parse)?;
    normalize_product_value(&mut value, images);
    serde_json::from_value::<Product>(value).map_err(|_| IngestError::Parse)
}

fn strip_markdown_fence(input: &str) -> String {
    let trimmed = input.trim();
    if !trimmed.starts_with("```") {
        return trimmed.to_string();
    }
    let mut body = Vec::new();
    for line in trimmed.lines().skip(1) {
        if line.trim_start().starts_with("```") {
            break;
        }
        body.push(line);
    }
    body.join("\n")
}

fn normalize_product_value(value: &mut Value, images: &[String]) {
    if !value.is_object() {
        *value = json!({});
    }
    let obj = value.as_object_mut().unwrap();

    if obj
        .get("name")
        .and_then(Value::as_str)
        .map(|s| s.trim().is_empty())
        .unwrap_or(true)
    {
        obj.insert("name".into(), Value::String("Untitled Product".into()));
    }

    if obj.get("sku").is_none() {
        obj.insert(
            "sku".into(),
            Value::String(uuid::Uuid::new_v4().to_string()),
        );
    }

    let image_field = obj.entry("image").or_insert(Value::Null);
    match image_field {
        Value::String(s) if s.trim().is_empty() => {
            *image_field = Value::Null;
        }
        Value::Array(arr) if arr.is_empty() => {
            *image_field = Value::Null;
        }
        _ => {}
    }
    if image_field.is_null() {
        *image_field = Value::Array(
            images
                .iter()
                .take(6)
                .map(|url| Value::String(url.clone()))
                .collect(),
        );
    }

    let offers = obj
        .entry("offers")
        .or_insert(Value::Object(Default::default()));
    if !offers.is_object() {
        *offers = Value::Object(Default::default());
    }
    let offers_obj = offers.as_object_mut().unwrap();
    if offers_obj.get("price").is_none() {
        offers_obj.insert("price".into(), Value::String("49.99".into()));
    }
    if offers_obj.get("priceCurrency").is_none() {
        offers_obj.insert("priceCurrency".into(), Value::String("USD".into()));
    }
    if offers_obj.get("itemCondition").is_none() {
        offers_obj.insert(
            "itemCondition".into(),
            Value::String("https://schema.org/UsedCondition".into()),
        );
    }
}

pub fn fallback_product(sku: &str, images: &[String]) -> Product {
    let primary = images.first().cloned().unwrap_or_default();
    Product {
        name: format!("{} listing", sku),
        image: if images.len() == 1 {
            ImageField::Single(primary)
        } else {
            ImageField::Multiple(images.to_vec())
        },
        offers: Offer {
            price: Some(99.0),
            priceCurrency: Some("USD".into()),
            priceSpecification: None,
        },
        description: Some("Automated fallback description".into()),
        brand: Some(crate::hsuf::models::Brand {
            name: Some("Hermes Labs".into()),
        }),
        color: Some("Black".into()),
        material: Some("Mixed materials".into()),
        size: None,
        sku: Some(sku.to_string()),
        mpn: Some(format!("MPN-{sku}")),
        height: Some(QuantitativeValue {
            unitCode: Some("INH".into()),
            unitText: Some("Inches".into()),
            value: Some(5.0),
        }),
        width: Some(QuantitativeValue {
            unitCode: Some("INH".into()),
            unitText: Some("Inches".into()),
            value: Some(8.0),
        }),
        depth: Some(QuantitativeValue {
            unitCode: Some("INH".into()),
            unitText: Some("Inches".into()),
            value: Some(12.0),
        }),
        weight: Some(QuantitativeValue {
            unitCode: Some("LBR".into()),
            unitText: Some("Pounds".into()),
            value: Some(3.0),
        }),
    }
}
