use crate::ebay::auth::get_user_access_token_from_refresh;
use crate::ebay::inventory::{
    InventoryAvailability, InventoryItemRequest, InventoryLocationRequest, InventoryProduct,
    LocationAddress, LocationDetails, LocationGeo, ShipToLocationAvailability,
    upsert_inventory_item, upsert_inventory_location,
};
use crate::ebay::listing::{ListingPolicies, PackageWeightAndSizePayload};
use crate::ebay::offers::{self, CreateOfferRequest, Price, PricingSummary, UpdateOfferRequest};
use crate::ebay::taxonomy::{
    Aspect as EbayAspect, AspectConstraint as EbayAspectConstraint, AspectValue as EbayAspectValue,
    TaxonomyResponse as EbayTaxonomyResponse,
};
use crate::hsuf::ingest;
use crate::hsuf::{
    HsufListingContext, Product as HsufProduct, build_listing_draft, estimate_package,
};
use crate::llm::{LlmClient, LlmConfig, LlmMessage};
use crate::models::{ImagesSource, ListingRequest, ListingResponse, MarketplaceId, StageReport};
use crate::security::AuthContext;
use crate::supabase::{EbayOrgConfig, SupabaseClient};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashSet, hash_map::DefaultHasher},
    env,
    future::Future,
    hash::{Hash, Hasher},
    sync::Arc,
    time::Instant,
};
use thiserror::Error;
use tokio::time::{Duration, sleep};
use tracing::warn;
use uuid::Uuid;
// metrics integration optional; macros removed to keep build stable

#[derive(Clone)]
pub struct Pipeline {
    pub config: Arc<PipelineConfig>,
    pub llm: Arc<LlmClient>,
    ebay_refresh_token: Option<String>,
    ebay_network_enabled: bool,
    supabase: Option<SupabaseClient>,
}

impl Pipeline {
    pub fn new(config: PipelineConfig) -> Self {
        let llm_config = LlmConfig::from_env();
        let llm = LlmClient::new(llm_config);
        let ebay_refresh_token = env::var("EBAY_REFRESH_TOKEN").ok();
        let ebay_network_enabled = parse_env_bool("EBAY_ENABLE_NETWORK");
        let supabase = SupabaseClient::from_env();
        Self {
            config: Arc::new(config),
            llm: Arc::new(llm),
            ebay_refresh_token,
            ebay_network_enabled,
            supabase,
        }
    }

    pub fn demo() -> Self {
        Self::new(PipelineConfig::default())
    }

    #[allow(dead_code)]
    pub fn llm_client(&self) -> &LlmClient {
        &self.llm
    }

    // Public wrappers for granular stage endpoints
    #[allow(dead_code)]
    pub async fn stage_resolve_images(
        &self,
        request: &ListingRequest,
    ) -> Result<Vec<String>, PipelineError> {
        let out = stages::resolve_images(request).await?;
        Ok(out.value)
    }

    #[allow(dead_code)]
    pub async fn stage_select_category(
        &self,
        request: &ListingRequest,
        images: &[String],
    ) -> Result<(CategorySelection, Vec<serde_json::Value>), PipelineError> {
        let seed = compute_seed(request, images);
        let out = stages::select_category(request, images, self.config.categories, seed).await?;
        let alternatives = out
            .output
            .get("alternatives")
            .cloned()
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default();
        Ok((out.value, alternatives))
    }

    #[allow(dead_code)]
    pub async fn stage_extract_product(
        &self,
        request: &ListingRequest,
        images: &[String],
    ) -> Result<HsufProduct, PipelineError> {
        let out = stages::extract_product(request, images, 0, &self.llm).await?;
        Ok(out.value)
    }

    async fn fetch_ebay_token(&self) -> Result<String, PipelineError> {
        let refresh = self
            .ebay_refresh_token
            .as_ref()
            .ok_or_else(|| PipelineError::internal("ebay_auth", "EBAY_REFRESH_TOKEN is not set"))?;
        get_user_access_token_from_refresh(refresh, EBAY_USER_SCOPES)
            .await
            .map_err(|err| PipelineError::internal("ebay_auth", err.to_string()))
    }

    pub async fn run(
        &self,
        request: ListingRequest,
        auth: Option<AuthContext>,
    ) -> Result<ListingResponse, PipelineError> {
        let request = Arc::new(request);
        let mut stages = Vec::new();
        let org_config = match (auth.as_ref(), self.supabase.as_ref()) {
            (Some(ctx), Some(client)) => {
                let org_id = Uuid::parse_str(&ctx.org_id)
                    .map_err(|err| PipelineError::internal("supabase", err.to_string()))?;
                match client.fetch_ebay_org_config(org_id).await {
                    Ok(config) => config,
                    Err(err) => {
                        warn!(target = "hermes.supabase", org_id = %ctx.org_id, error = %err, "ebay_org_config_lookup_failed");
                        None
                    }
                }
            }
            _ => None,
        };

        let images = if let Some(ov) = &request.overrides {
            if let Some(imgs) = ov.resolved_images.clone() {
                if imgs.is_empty() {
                    return Err(PipelineError::invalid_input(
                        "resolve_images",
                        "no images provided",
                    ));
                }
                if imgs.len() > max_images_allowed() {
                    return Err(PipelineError::invalid_input(
                        "resolve_images",
                        "too_many_images",
                    ));
                }
                let started = Instant::now();
                let imgs2 = imgs.clone();
                let output = json!({
                    "count": imgs2.len(),
                    "preview": imgs2.iter().take(2).collect::<Vec<_>>(),
                    "use_signed_urls": request.use_signed_urls,
                    "source": "override",
                });
                stages.push(StageReport::new(
                    "resolve_images",
                    started.elapsed().as_millis(),
                    output,
                ));
                imgs
            } else {
                self.capture_stage("resolve_images", &mut stages, {
                    let req = request.clone();
                    async move { stages::resolve_images(&req).await }
                })
                .await?
            }
        } else {
            self.capture_stage("resolve_images", &mut stages, {
                let req = request.clone();
                async move { stages::resolve_images(&req).await }
            })
            .await?
        };

        let seed = compute_seed(&request, &images);

        let selection = if let Some(ov) = &request.overrides {
            if let Some(sel) = ov.category.clone() {
                self.capture_stage("select_category", &mut stages, {
                    let images = images.clone();
                    let selection = CategorySelection {
                        id: sel.id,
                        tree_id: sel.tree_id,
                        label: sel.label,
                        confidence: sel.confidence,
                        rationale: sel.rationale,
                    };
                    let categories = self.config.categories;
                    async move {
                        let alternatives = categories
                            .iter()
                            .filter(|c| c.label != selection.label)
                            .take(2)
                            .map(|item| {
                                json!({
                                    "id": item.id,
                                    "label": item.label,
                                    "keywords": item.keywords,
                                })
                            })
                            .collect::<Vec<_>>();
                        Ok(StageOutcome::new(
                            selection.clone(),
                            json!({
                                "selected": selection,
                                "alternatives": alternatives,
                                "image_signature": images.first(),
                                "source": "override",
                            }),
                        ))
                    }
                })
                .await?
            } else {
                self.capture_stage("select_category", &mut stages, {
                    let req = request.clone();
                    let images = images.clone();
                    let categories = self.config.categories;
                    async move { stages::select_category(&req, &images, categories, seed).await }
                })
                .await?
            }
        } else {
            self.capture_stage("select_category", &mut stages, {
                let req = request.clone();
                let images = images.clone();
                let categories = self.config.categories;
                async move { stages::select_category(&req, &images, categories, seed).await }
            })
            .await?
        };

        let taxonomy = self
            .capture_stage("fetch_taxonomy", &mut stages, {
                let selection = selection.clone();
                async move { stages::fetch_taxonomy(&selection).await }
            })
            .await?;

        let token = self
            .capture_stage("acquire_user_token", &mut stages, async move {
                stages::acquire_user_token().await
            })
            .await?;

        let conditions = self
            .capture_stage("prepare_conditions", &mut stages, {
                let selection = selection.clone();
                async move { stages::prepare_conditions(&selection).await }
            })
            .await?;

        let llm = self.llm.clone();
        let llm_for_extract = llm.clone();
        let product = if let Some(ov) = &request.overrides {
            if let Some(value) = ov.product.clone() {
                self.capture_stage("extract_product", &mut stages, {
                    let images = images.clone();
                    async move {
                        match serde_json::from_value::<HsufProduct>(value) {
                            Ok(product) => Ok(StageOutcome::new(
                                product.clone(),
                                json!({
                                    "name": product.name,
                                    "brand": product.brand.as_ref().and_then(|b| b.name.clone()),
                                    "color": product.color,
                                    "images": images.len(),
                                    "source": "override",
                                }),
                            )),
                            Err(_) => Err(PipelineError::invalid_input(
                                "extract_product",
                                "invalid_product_override",
                            )),
                        }
                    }
                })
                .await?
            } else {
                self
                    .capture_stage("extract_product", &mut stages, {
                        let req = request.clone();
                        let images = images.clone();
                        async move { stages::extract_product(&req, &images, seed, &llm_for_extract).await }
                    })
                    .await?
            }
        } else {
            self.capture_stage("extract_product", &mut stages, {
                let req = request.clone();
                let images = images.clone();
                async move { stages::extract_product(&req, &images, seed, &llm_for_extract).await }
            })
            .await?
        };
        let ebay_runtime = resolve_ebay_config(&request, org_config.as_ref())?;
        let llm_for_build = llm.clone();
        let listing = self
            .capture_stage("build_listing", &mut stages, {
                let req = request.clone();
                let product = product.clone();
                let taxonomy = taxonomy.clone();
                let conditions = conditions.clone();
                let ebay_cfg = ebay_runtime.clone();
                async move {
                    stages::build_listing(
                        &req,
                        &product,
                        &taxonomy,
                        &conditions,
                        &llm_for_build,
                        &ebay_cfg,
                    )
                    .await
                }
            })
            .await?;

        if request.dry_run {
            return Ok(ListingResponse {
                listing_id: format!("PREVIEW-{}", Uuid::new_v4().simple()),
                stages,
            });
        }

        let ebay_token = if self.ebay_network_enabled {
            Some(self.fetch_ebay_token().await?)
        } else {
            None
        };

        let inventory_token = ebay_token.clone();
        let location_cfg = ebay_runtime.location.clone();
        self.capture_stage("push_inventory", &mut stages, {
            let req = request.clone();
            let listing = listing.clone();
            async move {
                stages::push_inventory(
                    &req,
                    &listing,
                    inventory_token.as_deref(),
                    location_cfg.clone(),
                )
                .await
            }
        })
        .await?;

        let offer = self
            .capture_stage("publish_offer", &mut stages, {
                let req = request.clone();
                let listing = listing.clone();
                let token = token.clone();
                let selection = selection.clone();
                let offer_token = ebay_token.clone();
                async move {
                    stages::publish_offer(
                        &req,
                        &listing,
                        &selection,
                        &token,
                        offer_token.as_deref(),
                    )
                    .await
                }
            })
            .await?;

        Ok(ListingResponse {
            listing_id: offer.listing_id.clone(),
            stages,
        })
    }

    async fn capture_stage<T, Fut>(
        &self,
        name: &'static str,
        stages: &mut Vec<StageReport>,
        fut: Fut,
    ) -> Result<T, PipelineError>
    where
        Fut: Future<Output = Result<StageOutcome<T>, PipelineError>>,
    {
        let started = Instant::now();
        let outcome = fut.await?;
        let elapsed_ms = started.elapsed().as_millis();
        // Lightweight metrics: stage elapsed (trace-based)
        crate::metrics::stage_elapsed(name, elapsed_ms);
        stages.push(StageReport::new(name, elapsed_ms, outcome.output));
        Ok(outcome.value)
    }
}

#[derive(Clone)]
pub struct PipelineConfig {
    pub categories: &'static [CategoryDefinition],
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            categories: &CATEGORY_POOL,
        }
    }
}

#[derive(Clone, Copy)]
pub struct CategoryDefinition {
    id: &'static str,
    tree_id: &'static str,
    label: &'static str,
    narrative: &'static str,
    keywords: &'static [&'static str],
}

const CATEGORY_POOL: [CategoryDefinition; 5] = [
    CategoryDefinition {
        id: "11450",
        tree_id: "0",
        label: "Clothing, Shoes & Accessories",
        narrative: "image cues show lifestyle apparel and footwear",
        keywords: &["shoe", "sneaker", "apparel"],
    },
    CategoryDefinition {
        id: "31387",
        tree_id: "0",
        label: "Consumer Electronics",
        narrative: "close-up product shots with polished surfaces",
        keywords: &["headphones", "camera", "electronics"],
    },
    CategoryDefinition {
        id: "261178",
        tree_id: "0",
        label: "Collectibles",
        narrative: "studio backgrounds and creative props",
        keywords: &["collectible", "vintage", "retro"],
    },
    CategoryDefinition {
        id: "281",
        tree_id: "0",
        label: "Motors Parts & Accessories",
        narrative: "detail shots of textured materials and components",
        keywords: &["auto", "motors", "component"],
    },
    CategoryDefinition {
        id: "293",
        tree_id: "0",
        label: "Health & Beauty",
        narrative: "soft lighting and product laydowns",
        keywords: &["beauty", "wellness", "care"],
    },
];

const EBAY_USER_SCOPES: &[&str] = &[
    "https://api.ebay.com/oauth/api_scope/sell.inventory",
    "https://api.ebay.com/oauth/api_scope/sell.account",
];

#[derive(Debug, Error)]
#[error("stage `{stage}` failed: {message}")]
pub struct PipelineError {
    stage: &'static str,
    message: String,
    kind: PipelineErrorKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelineErrorKind {
    InvalidInput,
    Internal,
}

impl PipelineError {
    pub fn invalid_input(stage: &'static str, message: impl Into<String>) -> Self {
        Self {
            stage,
            message: message.into(),
            kind: PipelineErrorKind::InvalidInput,
        }
    }

    pub fn internal(stage: &'static str, message: impl Into<String>) -> Self {
        Self {
            stage,
            message: message.into(),
            kind: PipelineErrorKind::Internal,
        }
    }

    pub fn stage(&self) -> &'static str {
        self.stage
    }

    pub fn kind(&self) -> PipelineErrorKind {
        self.kind
    }

    pub fn detail(&self) -> &str {
        &self.message
    }
}

#[derive(Debug)]
pub struct StageOutcome<T> {
    pub value: T,
    pub output: Value,
}

impl<T> StageOutcome<T> {
    fn new(value: T, output: Value) -> Self {
        Self { value, output }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ImagesSource, ListingRequest, MarketplaceId};

    fn sample_request() -> ListingRequest {
        ListingRequest {
            images_source: ImagesSource::Multiple(vec![
                "https://example.com/a.jpg".to_string(),
                "https://example.com/b.jpg".to_string(),
            ]),
            sku: "test-sku-001".to_string(),
            merchant_location_key: "loc-1".to_string(),
            fulfillment_policy_id: "fulfill-123".to_string(),
            payment_policy_id: "payment-123".to_string(),
            return_policy_id: "return-123".to_string(),
            marketplace: MarketplaceId::EbayUs,
            llm_provider: None,
            llm_listing_model: None,
            llm_category_model: None,
            use_signed_urls: false,
            overrides: None,
            dry_run: false,
        }
    }

    #[tokio::test]
    async fn stage_resolve_images_basic() {
        let req = sample_request();
        let out = stages::resolve_images(&req).await.expect("resolve_images");
        assert_eq!(out.value.len(), 2);
        assert!(out.value[0].starts_with("https://"));
    }

    #[tokio::test]
    async fn stage_resolve_images_payload_fields() {
        let mut req = sample_request();
        req.use_signed_urls = true;
        let out = stages::resolve_images(&req).await.expect("resolve_images");
        assert!(out.output.get("count").is_some());
        assert_eq!(out.output["use_signed_urls"], serde_json::json!(true));
    }

    #[tokio::test]
    async fn stage_resolve_images_rejects_non_http() {
        let req = ListingRequest {
            images_source: ImagesSource::Multiple(vec![
                "ftp://example.com/a.jpg".to_string(),
                "file:///etc/passwd".to_string(),
            ]),
            ..sample_request()
        };
        let err = stages::resolve_images(&req)
            .await
            .expect_err("should reject");
        assert_eq!(err.kind(), PipelineErrorKind::InvalidInput);
        assert_eq!(err.stage(), "resolve_images");
    }

    #[tokio::test]
    async fn stage_select_category_and_taxonomy() {
        let req = sample_request();
        let images = vec!["https://example.com/a.jpg".to_string()];
        let selection = stages::select_category(&req, &images, &CATEGORY_POOL, 42)
            .await
            .expect("select_category");
        assert!(!selection.value.id.is_empty());
        let taxonomy = stages::fetch_taxonomy(&selection.value)
            .await
            .expect("fetch_taxonomy");
        assert!(taxonomy.value.aspects.len() >= 3);
    }

    #[tokio::test]
    async fn stage_prepare_conditions_rules() {
        let selection = CategorySelection {
            id: "11450".to_string(),
            tree_id: "0".to_string(),
            label: "Clothing, Shoes & Accessories".to_string(),
            confidence: 0.9,
            rationale: "demo".to_string(),
        };
        let out = stages::prepare_conditions(&selection)
            .await
            .expect("prepare_conditions");
        assert!(!out.value.allowed.is_empty());
        assert!(out.value.allowed.contains(&out.value.default_condition));
    }

    #[tokio::test]
    async fn stage_extract_product_fallback_ok() {
        let req = sample_request();
        let images = vec!["https://example.com/a.jpg".to_string()];
        let llm = LlmClient::new(LlmConfig::from_env());
        let out = stages::extract_product(&req, &images, 0, &llm)
            .await
            .expect("extract_product");
        assert!(!out.value.name.trim().is_empty());
        // ensure images present
        let imgs = out.value.image.as_vec();
        assert!(!imgs.is_empty());
    }

    #[tokio::test]
    async fn stage_build_listing_offline_description() {
        let req = sample_request();
        let images = vec!["https://example.com/a.jpg".to_string()];
        let llm = LlmClient::new(LlmConfig::from_env());
        // taxonomy
        let selection = stages::select_category(&req, &images, &CATEGORY_POOL, 7)
            .await
            .unwrap();
        let taxonomy = stages::fetch_taxonomy(&selection.value).await.unwrap();
        // product
        let product = stages::extract_product(&req, &images, 0, &llm)
            .await
            .unwrap();
        // conditions
        let conditions = stages::prepare_conditions(&selection.value).await.unwrap();
        // ebay cfg
        let ebay_runtime = resolve_ebay_config(&req, None).expect("ebay cfg");
        // build
        let listing = stages::build_listing(
            &req,
            &product.value,
            &taxonomy.value,
            &conditions.value,
            &llm,
            &ebay_runtime,
        )
        .await
        .expect("build_listing");
        assert!(!listing.value.description.trim().is_empty());
        assert!(!listing.value.title.trim().is_empty());
        assert!(listing.value.price > 0.0);
    }

    #[tokio::test]
    async fn pipeline_run_stage_sequence() {
        let pipeline = Pipeline::demo();
        let req = sample_request();
        let resp = pipeline.run(req, None).await.expect("pipeline run");
        let names: Vec<String> = resp.stages.iter().map(|s| s.name.clone()).collect();
        assert_eq!(
            names,
            vec![
                "resolve_images",
                "select_category",
                "fetch_taxonomy",
                "acquire_user_token",
                "prepare_conditions",
                "extract_product",
                "build_listing",
                "push_inventory",
                "publish_offer",
            ]
        );
    }

    #[tokio::test]
    async fn pipeline_dry_run_stops_at_build() {
        let pipeline = Pipeline::demo();
        let mut req = sample_request();
        req.dry_run = true;
        let resp = pipeline.run(req, None).await.expect("pipeline run");
        let names: Vec<String> = resp.stages.iter().map(|s| s.name.clone()).collect();
        assert_eq!(
            names,
            vec![
                "resolve_images",
                "select_category",
                "fetch_taxonomy",
                "acquire_user_token",
                "prepare_conditions",
                "extract_product",
                "build_listing",
            ]
        );
        assert!(resp.listing_id.starts_with("PREVIEW-"));
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CategorySelection {
    pub id: String,
    pub tree_id: String,
    pub label: String,
    pub confidence: f32,
    pub rationale: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaxonomySpec {
    pub category_id: String,
    pub tree_id: String,
    pub aspects: Vec<TaxonomyAspect>,
    #[serde(skip_serializing)]
    pub raw: EbayTaxonomyResponse,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaxonomyAspect {
    pub name: String,
    pub required: bool,
    pub samples: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DemoCredentials {
    pub token: String,
    pub expires_in: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConditionBundle {
    pub allowed: Vec<String>,
    pub default_condition: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ListingPlan {
    pub sku: String,
    pub title: String,
    pub price: f64,
    pub currency: String,
    pub condition: String,
    pub description: String,
    pub marketplace: MarketplaceId,
    pub merchant_location_key: String,
    pub category_id: String,
    pub media: Vec<String>,
    pub policies: ListingPolicies,
    pub aspects: BTreeMap<String, Vec<String>>,
    pub package: Option<PackageWeightAndSizePayload>,
}

#[derive(Debug, Clone, Serialize)]
pub struct InventoryReceipt {
    pub sku: String,
    pub location: String,
    pub quantity: u32,
    pub package: &'static str,
    pub status: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct OfferResult {
    pub listing_id: String,
    pub route: String,
    pub preview_url: String,
}

pub fn compute_seed(request: &ListingRequest, images: &[String]) -> u64 {
    let mut hasher = DefaultHasher::new();
    request.sku.hash(&mut hasher);
    request.merchant_location_key.hash(&mut hasher);
    request.fulfillment_policy_id.hash(&mut hasher);
    request.payment_policy_id.hash(&mut hasher);
    request.return_policy_id.hash(&mut hasher);
    request.marketplace.hash(&mut hasher);
    for image in images.iter().take(3) {
        image.hash(&mut hasher);
    }
    hasher.finish()
}

trait MarketplaceExt {
    fn marketplace_route(&self) -> &'static str;
}

impl MarketplaceExt for ListingRequest {
    fn marketplace_route(&self) -> &'static str {
        match self.marketplace {
            MarketplaceId::EbayUs => "https://api.ebay.com/sell",
            MarketplaceId::EbayUk => "https://api.ebay.co.uk/sell",
            MarketplaceId::EbayDe => "https://api.ebay.de/sell",
        }
    }
}

pub mod stages {
    use super::*;

    const TOKEN_SCOPES: &[&str] = &[
        "https://api.ebay.com/oauth/api_scope/sell.inventory",
        "https://api.ebay.com/oauth/api_scope/sell.account",
    ];

    pub async fn resolve_images(
        request: &ListingRequest,
    ) -> Result<StageOutcome<Vec<String>>, PipelineError> {
        short_pause(18).await;
        let mut resolved = match request.images_source.clone() {
            ImagesSource::Single(value) => tokenize(&value),
            ImagesSource::Multiple(values) => values
                .into_iter()
                .flat_map(|value| tokenize(&value))
                .collect::<Vec<_>>(),
        }
        .into_iter()
        .map(|entry| entry.trim().to_string())
        .filter(|entry| !entry.is_empty())
        .collect::<Vec<_>>();

        if request.use_signed_urls {
            resolved = resolved
                .into_iter()
                .map(|url| add_signature(&url))
                .collect();
        }

        resolved = deduplicate(resolved);

        let max_images = max_images_allowed();
        if resolved.len() > max_images {
            return Err(PipelineError::invalid_input(
                "resolve_images",
                "too_many_images",
            ));
        }

        if resolved.is_empty() {
            return Err(PipelineError::invalid_input(
                "resolve_images",
                "no images provided",
            ));
        }

        // Validate URL schemes and optional allowlist
        let allowlist = image_domain_allowlist();
        for url in &resolved {
            match reqwest::Url::parse(url) {
                Ok(parsed) => {
                    let scheme_ok = matches!(parsed.scheme(), "http" | "https");
                    if !scheme_ok {
                        return Err(PipelineError::invalid_input(
                            "resolve_images",
                            format!("unsupported_url_scheme: {url}"),
                        ));
                    }
                    if let Some(allowed) = &allowlist
                        && let Some(host) = parsed.host_str()
                        && !host_allowed(host, allowed)
                    {
                        return Err(PipelineError::invalid_input(
                            "resolve_images",
                            format!("domain_not_allowed: {host}"),
                        ));
                    }
                }
                Err(_) => {
                    return Err(PipelineError::invalid_input(
                        "resolve_images",
                        format!("invalid_image_url: {url}"),
                    ));
                }
            }
        }

        let preview: Vec<&str> = resolved
            .iter()
            .take(4)
            .map(|value| value.as_str())
            .collect();

        Ok(StageOutcome::new(
            resolved.clone(),
            json!({
                "count": resolved.len(),
                "preview": preview,
                "use_signed_urls": request.use_signed_urls,
            }),
        ))
    }

    pub async fn select_category(
        request: &ListingRequest,
        images: &[String],
        categories: &'static [CategoryDefinition],
        seed: u64,
    ) -> Result<StageOutcome<CategorySelection>, PipelineError> {
        short_pause(22).await;
        let idx = (seed as usize) % categories.len();
        let category = categories.get(idx).ok_or_else(|| {
            PipelineError::internal("select_category", "no categories configured")
        })?;

        let confidence: f32 = 0.55 + ((seed % 40) as f32 / 100.0);
        let rationale = format!(
            "sku signal `{}` + image hash matched `{}`",
            request.sku, category.narrative
        );

        let rounded_confidence = ((confidence.min(0.95) * 100.0).round() / 100.0).clamp(0.0, 0.99);

        let selection = CategorySelection {
            id: category.id.to_string(),
            tree_id: category.tree_id.to_string(),
            label: category.label.to_string(),
            confidence: rounded_confidence,
            rationale,
        };

        let alternatives = categories
            .iter()
            .enumerate()
            .filter(|(pos, _)| *pos != idx)
            .take(2)
            .map(|(_, item)| {
                json!({
                    "id": item.id,
                    "label": item.label,
                    "keywords": item.keywords,
                })
            })
            .collect::<Vec<_>>();

        Ok(StageOutcome::new(
            selection.clone(),
            json!({
                "selected": selection,
                "alternatives": alternatives,
                "image_signature": images.first(),
            }),
        ))
    }

    pub(super) async fn fetch_taxonomy(
        selection: &CategorySelection,
    ) -> Result<StageOutcome<TaxonomySpec>, PipelineError> {
        short_pause(25).await;
        let aspects = build_aspects(&selection.label);
        let ebay_aspects = aspects
            .iter()
            .map(|aspect| EbayAspect {
                localizedAspectName: aspect.name.clone(),
                aspectValues: aspect
                    .samples
                    .iter()
                    .map(|value| EbayAspectValue {
                        localizedValue: value.clone(),
                    })
                    .collect(),
                aspectConstraint: Some(EbayAspectConstraint {
                    aspectMode: Some(if aspect.required {
                        "SELECTION_ONLY".into()
                    } else {
                        "FREE_TEXT".into()
                    }),
                    aspectRequired: Some(aspect.required),
                    itemToAspectCardinality: Some("MULTI".into()),
                }),
            })
            .collect();

        let spec = TaxonomySpec {
            category_id: selection.id.clone(),
            tree_id: selection.tree_id.clone(),
            aspects,
            raw: EbayTaxonomyResponse {
                aspects: ebay_aspects,
            },
        };
        Ok(StageOutcome::new(
            spec.clone(),
            json!({
                "category_id": spec.category_id,
                "aspect_count": spec.aspects.len(),
                "sample_aspects": spec.aspects.iter().take(3).collect::<Vec<_>>(),
            }),
        ))
    }

    pub(super) async fn acquire_user_token() -> Result<StageOutcome<DemoCredentials>, PipelineError>
    {
        short_pause(12).await;
        let token_value = format!("demo_{}", Uuid::new_v4());
        let credentials = DemoCredentials {
            token: token_value.clone(),
            expires_in: 3600,
        };
        Ok(StageOutcome::new(
            credentials,
            json!({
                "token_preview": preview_token(&token_value),
                "scopes": TOKEN_SCOPES,
                "expires_in_seconds": 3600,
            }),
        ))
    }

    pub(super) async fn prepare_conditions(
        selection: &CategorySelection,
    ) -> Result<StageOutcome<ConditionBundle>, PipelineError> {
        short_pause(10).await;
        let mut allowed = match selection.label.to_lowercase().as_str() {
            label if label.contains("shoe") => {
                vec!["NEW_IN_BOX", "USED_LIKE_NEW", "USED_GOOD", "USED_FAIR"]
            }
            label if label.contains("collectible") => {
                vec!["NEW", "UNOPENED", "DISPLAY_ONLY", "USED"]
            }
            _ => vec!["NEW", "USED_LIKE_NEW", "USED_GOOD", "USED"],
        }
        .into_iter()
        .map(|entry| entry.to_string())
        .collect::<Vec<_>>();

        if allowed.is_empty() {
            allowed.push("USED".to_string());
        }

        let bundle = ConditionBundle {
            allowed: allowed.clone(),
            default_condition: allowed.first().cloned().unwrap_or_else(|| "USED".into()),
        };

        Ok(StageOutcome::new(
            bundle.clone(),
            json!({
                "allowed": bundle.allowed,
                "default": bundle.default_condition,
            }),
        ))
    }

    pub async fn extract_product(
        request: &ListingRequest,
        images: &[String],
        _seed: u64,
        llm: &LlmClient,
    ) -> Result<StageOutcome<HsufProduct>, PipelineError> {
        short_pause(40).await;
        let product = match ingest::infer_product(llm, &request.sku, images).await {
            Ok(product) => product,
            Err(err) => {
                warn!(target = "hermes.hsuf", sku = %request.sku, error = %err, "hsuf_ingest_fallback");
                ingest::fallback_product(&request.sku, images)
            }
        };

        Ok(StageOutcome::new(
            product.clone(),
            json!({
                "name": product.name,
                "brand": product.brand.as_ref().and_then(|b| b.name.clone()),
                "color": product.color,
                "images": images.len(),
            }),
        ))
    }

    pub(super) async fn build_listing(
        request: &ListingRequest,
        product: &HsufProduct,
        taxonomy: &TaxonomySpec,
        conditions: &ConditionBundle,
        llm: &LlmClient,
        ebay_cfg: &EbayRuntimeConfig,
    ) -> Result<StageOutcome<ListingPlan>, PipelineError> {
        short_pause(28).await;
        let ctx = HsufListingContext {
            taxonomy: &taxonomy.raw,
            category_id: &taxonomy.category_id,
            default_currency: "USD",
        };

        let draft = build_listing_draft(product, ctx)
            .map_err(|err| PipelineError::internal("build_listing", err.to_string()))?;
        let package = estimate_package(product);

        let bullets = bullet_points_from_product(product);
        let prompt = format!(
            "Generate a compelling, policy-compliant eBay listing description. Title: {title}. Bullet points: {bullets:?}.",
            title = draft.title,
            bullets = bullets,
        );

        let description = match llm
            .chat(&[LlmMessage {
                role: "user".into(),
                content: prompt,
            }])
            .await
        {
            Ok(resp) => resp.text,
            Err(err) => {
                warn!(
                    target = "hermes.llm",
                    error = %err,
                    "llm_description_fallback"
                );
                let mut fallback = String::new();
                fallback.push_str(&format!("{}\n\n", draft.title));
                fallback.push_str("Highlights:\n");
                for b in &bullets {
                    fallback.push_str(&format!("- {}\n", b));
                }
                fallback
                    .push_str("\nAuto-generated demo description. Details may be approximations.");
                fallback
            }
        };

        let listing = ListingPlan {
            sku: request.sku.clone(),
            title: draft.title.clone(),
            price: draft.price,
            currency: draft.currency.clone(),
            condition: conditions.default_condition.clone(),
            description,
            marketplace: ebay_cfg.marketplace,
            merchant_location_key: ebay_cfg.merchant_location_key.clone(),
            category_id: draft.category_id.clone(),
            media: draft.images.clone(),
            policies: ebay_cfg.policies.clone(),
            aspects: draft.aspects.clone(),
            package,
        };

        Ok(StageOutcome::new(
            listing.clone(),
            json!({
                "title": listing.title,
                "price": listing.price,
                "currency": listing.currency,
                "condition": listing.condition,
                "aspect_count": listing.aspects.len(),
            }),
        ))
    }

    pub(super) async fn push_inventory(
        request: &ListingRequest,
        listing: &ListingPlan,
        access_token: Option<&str>,
        location_cfg: Option<LocationMetadata>,
    ) -> Result<StageOutcome<InventoryReceipt>, PipelineError> {
        short_pause(15).await;
        let inventory_request = inventory_request_from_listing(listing);
        if let Some(token) = access_token {
            if let Some(location) = location_cfg.clone() {
                let location_payload = InventoryLocationRequest {
                    merchant_location_status: "ENABLED",
                    location_types: vec!["WAREHOUSE"],
                    name: location.name.clone(),
                    location: LocationDetails {
                        address: LocationAddress {
                            address_line1: location.address_line1.clone(),
                            address_line2: location.address_line2.clone(),
                            city: location.city.clone(),
                            state_or_province: location.state_or_province.clone(),
                            postal_code: location.postal_code.clone(),
                            country: location.country.clone(),
                        },
                        geo_coordinates: Some(LocationGeo {
                            latitude: location.latitude.clone(),
                            longitude: location.longitude.clone(),
                        }),
                    },
                };
                if !location_payload.location.address.address_line1.is_empty()
                    && upsert_inventory_location(
                        &listing.merchant_location_key,
                        &location_payload,
                        token,
                    )
                    .await
                    .is_err()
                {
                    warn!(
                        target = "hermes.ebay",
                        location = %listing.merchant_location_key,
                        "inventory_location_upsert_failed"
                    );
                }
            }
            upsert_inventory_item(&request.sku, &inventory_request, token)
                .await
                .map_err(|err| PipelineError::internal("push_inventory", err.to_string()))?;
        }
        let receipt = InventoryReceipt {
            sku: listing.sku.clone(),
            location: listing.merchant_location_key.clone(),
            quantity: 1,
            package: "DEFAULT_SHOES",
            status: "UPSERTED",
        };
        Ok(StageOutcome::new(
            receipt.clone(),
            json!({
                "sku": receipt.sku,
                "location": receipt.location,
                "status": receipt.status,
                "media_attached": listing.media.len(),
                "inventory_request": inventory_request,
            }),
        ))
    }

    pub(super) async fn publish_offer(
        request: &ListingRequest,
        listing: &ListingPlan,
        selection: &CategorySelection,
        token: &DemoCredentials,
        access_token: Option<&str>,
    ) -> Result<StageOutcome<OfferResult>, PipelineError> {
        short_pause(20).await;
        let listing_title = listing.title.clone();
        let media_count = listing.media.len();
        let (create_offer, update_offer) = build_offer_requests(listing);
        let create_offer_json = json!(&create_offer);
        let update_offer_json = json!(&update_offer);
        let (listing_id, offer_id) = if let Some(user_token) = access_token {
            match offers::create_offer(&create_offer, user_token).await {
                Ok(new_offer_id) => {
                    let published = offers::publish_offer(&new_offer_id, user_token)
                        .await
                        .map_err(|err| PipelineError::internal("publish_offer", err.to_string()))?;
                    let final_listing_id = if published.is_empty() {
                        fallback_listing_id()
                    } else {
                        published
                    };
                    (final_listing_id, Some(new_offer_id))
                }
                Err(offers::EbayOfferError::EntityExists) => {
                    reconcile_existing_offer(&create_offer, &update_offer, user_token).await?
                }
                Err(err) => return Err(PipelineError::internal("publish_offer", err.to_string())),
            }
        } else {
            (fallback_listing_id(), None)
        };
        let offer = OfferResult {
            listing_id: listing_id.clone(),
            route: format!("{}/offers", request.marketplace_route()),
            preview_url: format!(
                "https://sandbox.ebay.com/itm/{id}",
                id = listing_id.chars().take(12).collect::<String>()
            ),
        };
        Ok(StageOutcome::new(
            offer.clone(),
            json!({
                "listing_id": offer.listing_id,
                "category": selection.label,
                "token_preview": preview_token(&token.token),
                "title": listing_title,
                "media_count": media_count,
                "create_offer": create_offer_json,
                "update_offer": update_offer_json,
                "offer_id": offer_id,
            }),
        ))
    }

    fn tokenize(value: &str) -> Vec<String> {
        if value.chars().any(|ch| matches!(ch, '\n' | ',' | ';' | '|')) {
            value
                .split(['\n', ',', ';', '|'])
                .map(|entry| entry.trim())
                .filter(|entry| !entry.is_empty())
                .map(|entry| entry.to_string())
                .collect()
        } else {
            vec![value.trim().to_string()]
        }
    }

    fn image_domain_allowlist() -> Option<Vec<String>> {
        std::env::var("IMAGE_DOMAIN_ALLOWLIST")
            .ok()
            .map(|v| {
                v.split([',', ' ', '\n', '\t'])
                    .map(|s| s.trim().to_lowercase())
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
            })
            .filter(|v| !v.is_empty())
    }

    fn host_allowed(host: &str, allowed: &[String]) -> bool {
        let host = host.to_lowercase();
        allowed
            .iter()
            .any(|d| host == *d || host.ends_with(&format!(".{d}")))
    }

    fn deduplicate(values: Vec<String>) -> Vec<String> {
        let mut seen = HashSet::new();
        let mut result = Vec::new();
        for value in values {
            if seen.insert(value.clone()) {
                result.push(value);
            }
        }
        result
    }

    fn add_signature(url: &str) -> String {
        if url.contains("signature=demo") {
            url.to_string()
        } else if url.contains('?') {
            format!("{url}&signature=demo")
        } else {
            format!("{url}?signature=demo")
        }
    }

    fn preview_token(token: &str) -> String {
        token.chars().take(6).collect::<String>() + "…"
    }

    fn build_aspects(category_label: &str) -> Vec<TaxonomyAspect> {
        let base = vec![
            TaxonomyAspect {
                name: "Brand".into(),
                required: true,
                samples: vec!["Hermes Labs".into(), "Demo Labs".into()],
            },
            TaxonomyAspect {
                name: "Color".into(),
                required: true,
                samples: vec!["Black".into(), "White".into(), "Sand".into()],
            },
            TaxonomyAspect {
                name: "Condition".into(),
                required: false,
                samples: vec!["New".into(), "Used".into()],
            },
        ];

        if category_label.contains("Electronics") {
            let mut aspects = base;
            aspects.push(TaxonomyAspect {
                name: "BatteryIncluded".into(),
                required: false,
                samples: vec!["Yes".into(), "No".into()],
            });
            aspects
        } else {
            base
        }
    }

    fn short_pause(ms: u64) -> impl Future<Output = ()> {
        sleep(Duration::from_millis(ms))
    }
}

fn bullet_points_from_product(product: &HsufProduct) -> Vec<String> {
    let mut bullets = Vec::new();
    if let Some(brand) = product.brand.as_ref().and_then(|brand| brand.name.clone()) {
        bullets.push(format!("Authentic {brand} craftsmanship"));
    }
    if let Some(color) = &product.color {
        bullets.push(format!("Distinctive {color} finish"));
    }
    if let Some(material) = &product.material {
        bullets.push(format!("Premium {material} materials"));
    }
    if let Some(desc) = &product.description {
        bullets.push(desc.lines().next().unwrap_or(desc).to_string());
    }
    if bullets.is_empty() {
        bullets.push("LLM-enriched listing details".into());
    }
    bullets.truncate(4);
    bullets
}

fn inventory_request_from_listing(listing: &ListingPlan) -> InventoryItemRequest {
    let aspects = if listing.aspects.is_empty() {
        None
    } else {
        Some(listing.aspects.clone())
    };
    InventoryItemRequest {
        availability: InventoryAvailability {
            ship_to_location_availability: ShipToLocationAvailability { quantity: 1 },
        },
        product: InventoryProduct {
            title: listing.title.clone(),
            description: listing.description.clone(),
            aspects,
            image_urls: listing.media.clone(),
        },
        package_weight_and_size: listing.package.clone(),
    }
}

fn build_offer_requests(listing: &ListingPlan) -> (CreateOfferRequest, UpdateOfferRequest) {
    let pricing = PricingSummary {
        price: Price::from_amount(listing.price, &listing.currency),
    };
    let create = CreateOfferRequest {
        sku: listing.sku.clone(),
        marketplace_id: listing.marketplace.ebay_code().to_string(),
        format: "FIXED_PRICE",
        category_id: listing.category_id.clone(),
        listing_description: listing.description.clone(),
        pricing_summary: pricing.clone(),
        available_quantity: 1,
        merchant_location_key: listing.merchant_location_key.clone(),
        listing_policies: listing.policies.clone(),
        aspects: listing.aspects.clone(),
        package_weight_and_size: listing.package.clone(),
        image_urls: listing.media.clone(),
    };

    let update = UpdateOfferRequest {
        format: "FIXED_PRICE",
        category_id: listing.category_id.clone(),
        listing_description: listing.description.clone(),
        pricing_summary: pricing,
        available_quantity: 1,
        listing_policies: listing.policies.clone(),
        merchant_location_key: listing.merchant_location_key.clone(),
        package_weight_and_size: listing.package.clone(),
    };

    (create, update)
}

async fn reconcile_existing_offer(
    create_req: &CreateOfferRequest,
    update_req: &UpdateOfferRequest,
    access_token: &str,
) -> Result<(String, Option<String>), PipelineError> {
    let offers = offers::get_offers_by_sku(&create_req.sku, access_token)
        .await
        .map_err(|err| PipelineError::internal("publish_offer", err.to_string()))?;
    let candidate = offers
        .iter()
        .find(|offer| offer.marketplaceId.as_deref() == Some(&create_req.marketplace_id))
        .or_else(|| offers.first())
        .and_then(|offer| offer.offerId.clone())
        .ok_or_else(|| {
            PipelineError::internal(
                "publish_offer",
                "no existing offer found for reconciliation",
            )
        })?;

    if let Err(err) = offers::update_offer(&candidate, update_req, access_token).await {
        warn!(target = "hermes.ebay", offer_id = %candidate, error = %err, "offer_update_failed_withdraw_retry");
        offers::withdraw_offer(&candidate, access_token)
            .await
            .map_err(|withdraw_err| {
                PipelineError::internal("publish_offer", withdraw_err.to_string())
            })?;
        offers::update_offer(&candidate, update_req, access_token)
            .await
            .map_err(|update_err| {
                PipelineError::internal("publish_offer", update_err.to_string())
            })?;
    }
    let listing_id = offers::publish_offer(&candidate, access_token)
        .await
        .map_err(|err| PipelineError::internal("publish_offer", err.to_string()))?;
    let final_listing_id = if listing_id.is_empty() {
        fallback_listing_id()
    } else {
        listing_id
    };
    Ok((final_listing_id, Some(candidate)))
}

fn parse_env_bool(key: &str) -> bool {
    match env::var(key) {
        Ok(value) => matches!(
            value.trim().to_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        Err(_) => false,
    }
}

fn fallback_listing_id() -> String {
    format!("HER-{}", Uuid::new_v4().simple())
}

fn max_images_allowed() -> usize {
    std::env::var("MAX_IMAGES")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|v| *v >= 1)
        .unwrap_or(6)
}

#[derive(Clone)]
struct EbayRuntimeConfig {
    merchant_location_key: String,
    policies: ListingPolicies,
    marketplace: MarketplaceId,
    location: Option<LocationMetadata>,
}

#[derive(Clone)]
struct LocationMetadata {
    name: String,
    address_line1: String,
    address_line2: Option<String>,
    city: String,
    state_or_province: String,
    postal_code: String,
    country: String,
    latitude: Option<String>,
    longitude: Option<String>,
}

fn resolve_ebay_config(
    request: &ListingRequest,
    config: Option<&EbayOrgConfig>,
) -> Result<EbayRuntimeConfig, PipelineError> {
    let merchant_location_key = select_value(
        config.map(|cfg| cfg.merchant_location_key.as_str()),
        &request.merchant_location_key,
        "merchant_location_key",
    )?;
    let fulfillment_policy_id = select_value(
        config.map(|cfg| cfg.fulfillment_policy_id.as_str()),
        &request.fulfillment_policy_id,
        "fulfillment_policy_id",
    )?;
    let payment_policy_id = select_value(
        config.map(|cfg| cfg.payment_policy_id.as_str()),
        &request.payment_policy_id,
        "payment_policy_id",
    )?;
    let return_policy_id = select_value(
        config.map(|cfg| cfg.return_policy_id.as_str()),
        &request.return_policy_id,
        "return_policy_id",
    )?;

    let policies = ListingPolicies {
        fulfillment_policy_id,
        payment_policy_id,
        return_policy_id,
    };

    let marketplace = config
        .and_then(|cfg| cfg.marketplace.as_deref())
        .and_then(MarketplaceId::from_str)
        .unwrap_or(request.marketplace);

    let location = config.and_then(location_from_config);

    Ok(EbayRuntimeConfig {
        merchant_location_key,
        policies,
        marketplace,
        location,
    })
}

fn select_value(
    supabase_value: Option<&str>,
    request_value: &str,
    field: &str,
) -> Result<String, PipelineError> {
    let candidate = supabase_value
        .map(|value| value.to_string())
        .unwrap_or_else(|| request_value.to_string());
    if candidate.trim().is_empty() {
        Err(PipelineError::invalid_input(
            "ebay_config",
            format!("missing_{field}"),
        ))
    } else {
        Ok(candidate)
    }
}

fn location_from_config(cfg: &EbayOrgConfig) -> Option<LocationMetadata> {
    Some(LocationMetadata {
        name: cfg.location_name.as_ref()?.trim().to_string(),
        address_line1: cfg.address_line1.as_ref()?.trim().to_string(),
        address_line2: cfg.address_line2.clone().filter(|s| !s.trim().is_empty()),
        city: cfg.city.as_ref()?.trim().to_string(),
        state_or_province: cfg.state_or_province.as_ref()?.trim().to_string(),
        postal_code: cfg.postal_code.as_ref()?.trim().to_string(),
        country: cfg.country.as_ref()?.trim().to_string(),
        latitude: cfg.latitude.clone().filter(|s| !s.trim().is_empty()),
        longitude: cfg.longitude.clone().filter(|s| !s.trim().is_empty()),
    })
}
