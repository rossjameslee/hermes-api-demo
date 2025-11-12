mod ebay;
mod hsuf;
mod http;
mod idempotency;
mod jobs;
mod llm;
mod metrics;
mod models;
mod pipeline;
mod security;
mod supabase;

use axum::{
    Json, Router,
    extract::{Extension, Path, State},
    http::StatusCode,
    middleware,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use models::{ApiError, ListingRequest, ListingResponse};
use pipeline::{Pipeline, PipelineError, PipelineErrorKind};
use security::{AuthContext, AuthState, require_api_auth};
use serde_json::json;
use std::{collections::HashMap, net::SocketAddr, sync::Arc};
use tokio::sync::Mutex;
// metrics macros disabled in demo build
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};
use tracing::{error, info};
use tracing_subscriber::{EnvFilter, fmt};

#[tokio::main]
async fn main() {
    if let Err(err) = run().await {
        error!(target = "hermes.api", "server crashed: {err}");
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    init_tracing();

    let auth_state = AuthState::from_env();
    let pipeline = Pipeline::demo();
    let (queue, _worker) = jobs::JobQueue::spawn(pipeline.clone());
    let openapi_raw = include_str!("../docs/openapi.yaml");
    let openapi: serde_json::Value =
        serde_yaml::from_str(openapi_raw).unwrap_or(serde_json::json!({"openapi":"3.0.3"}));
    let prometheus_handle = PrometheusBuilder::new()
        .install_recorder()
        .expect("prom recorder");
    let redis = std::env::var("REDIS_URL")
        .ok()
        .and_then(|u| redis::Client::open(u).ok());
    let state = AppState {
        pipeline,
        queue,
        openapi: Arc::new(openapi),
        idempotency: Arc::new(Mutex::new(HashMap::new())),
        prometheus_handle: prometheus_handle.clone(),
        redis,
    };

    let cors = CorsLayer::new()
        .allow_headers(Any)
        .allow_methods(Any)
        .allow_origin(Any);

    let protected = Router::new()
        .route("/listings", post(create_listing))
        .route("/listings/continue", post(create_listing_continue))
        .nest(
            "/stages",
            Router::new()
                .route("/resolve_images", post(stage_resolve_images))
                .route("/select_category", post(stage_select_category))
                .route("/extract_product", post(stage_extract_product))
                .route("/description", post(stage_description)),
        )
        .nest(
            "/jobs",
            Router::new()
                .route("/listings", post(enqueue_listing_job))
                .route("/listings/continue", post(enqueue_continue_job))
                .route("/{id}", get(get_job_status)),
        )
        .route_layer(middleware::from_fn_with_state(auth_state, require_api_auth));

    let app = Router::new()
        .route("/health", get(health))
        .route("/metrics", get(metrics_endpoint))
        .route("/openapi.json", get(openapi_json))
        .route("/docs", get(swagger_ui))
        .merge(protected)
        .with_state(AppState {
            openapi: Arc::new(
                serde_yaml::from_str(include_str!("../docs/openapi.yaml"))
                    .unwrap_or(serde_json::json!({"openapi":"3.0.3"})),
            ),
            idempotency: Arc::new(Mutex::new(HashMap::new())),
            prometheus_handle,
            ..state
        })
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .layer(axum::extract::DefaultBodyLimit::max(body_limit_from_env()));

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(8000);
    let addr: SocketAddr = ([0, 0, 0, 0], port).into();
    info!(target = "hermes.api", "listening on {addr}");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app.into_make_service()).await?;
    Ok(())
}

#[derive(Clone)]
struct AppState {
    pipeline: Pipeline,
    queue: jobs::JobQueue,
    openapi: Arc<serde_json::Value>,
    idempotency: Arc<Mutex<HashMap<String, ListingResponse>>>,
    prometheus_handle: PrometheusHandle,
    redis: Option<redis::Client>,
}

/// Health and readiness check.
///
/// - Method: `GET`
/// - Path: `/health`
/// - Auth: none
///
/// Returns a small JSON payload with `status` and `service`.
async fn health() -> Json<serde_json::Value> {
    Json(json!({
        "status": "ok",
        "service": "hermes-api-rs",
    }))
}

async fn openapi_json(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    if let Ok(key) = std::env::var("OPENAPI_KEY") {
        let presented = headers
            .get("X-Docs-Key")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if presented != key {
            return Err(AppError::Pipeline(PipelineError::invalid_input(
                "docs",
                "unauthorized",
            )));
        }
    }
    Ok(Json((*state.openapi).clone()))
}

async fn swagger_ui() -> axum::http::Response<String> {
    let html = r#"<!doctype html>
<html>
<head>
  <meta charset='utf-8'/>
  <title>Hermes API Docs</title>
  <link rel="stylesheet" href="https://unpkg.com/swagger-ui-dist@5/swagger-ui.css" />
</head>
<body>
  <div id="swagger-ui"></div>
  <script src="https://unpkg.com/swagger-ui-dist@5/swagger-ui-bundle.js"></script>
  <script>
    window.onload = () => {
      window.ui = SwaggerUIBundle({ url: '/openapi.json', dom_id: '#swagger-ui' });
    };
  </script>
</body>
</html>"#;
    axum::http::Response::builder()
        .header("Content-Type", "text/html; charset=utf-8")
        .body(html.to_string())
        .unwrap()
}

fn body_limit_from_env() -> usize {
    std::env::var("REQUEST_MAX_BYTES")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(256 * 1024)
}

async fn metrics_endpoint(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> axum::http::Response<String> {
    if let Ok(secret) = std::env::var("METRICS_KEY") {
        let presented = headers
            .get("X-Metrics-Key")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if presented != secret {
            return axum::http::Response::builder()
                .status(axum::http::StatusCode::UNAUTHORIZED)
                .body("unauthorized".into())
                .unwrap();
        }
    }
    let body = state.prometheus_handle.render();
    axum::http::Response::builder()
        .header("Content-Type", "text/plain; version=0.0.4")
        .body(body)
        .unwrap()
}

/// Run the images → eBay listing pipeline.
///
/// - Method: `POST`
/// - Path: `/listings`
/// - Auth: `Authorization: Bearer <key>` or `X-Hermes-Key: <key>`
/// - Body: `ListingRequest`
/// - Response: `ListingResponse` (synthetic `listing_id` + per‑stage transcript)
async fn create_listing(
    State(state): State<AppState>,
    Extension(context): Extension<AuthContext>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<ListingRequest>,
) -> Result<Json<ListingResponse>, AppError> {
    crate::metrics::inc_requests("/listings");
    let _handler_start = std::time::Instant::now();
    info!(
        target = "hermes.api",
        org_id = %context.org_id,
        api_key = %context.api_key_id,
        "listing pipeline invoked",
    );

    if let Some(key) = headers
        .get("Idempotency-Key")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        if let Some(client) = &state.redis {
            if let Some(existing) = idempotency::redis_get(client, &key).await {
                return Ok(Json(existing));
            }
            let response = state.pipeline.run(payload, Some(context)).await?;
            let ttl = std::env::var("IDEMPOTENCY_TTL_SECS")
                .ok()
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(3600);
            idempotency::redis_set(client, &key, &response, ttl).await;
            return Ok(Json(response));
        }
        if let Some(existing) = state.idempotency.lock().await.get(&key).cloned() {
            return Ok(Json(existing));
        }
        let response = state.pipeline.run(payload, Some(context)).await?;
        state.idempotency.lock().await.insert(key, response.clone());
        return Ok(Json(response));
    }

    let response = state.pipeline.run(payload, Some(context)).await?;

    Ok(Json(response))
}

#[derive(Debug, Deserialize)]
struct ContinueRequest {
    #[serde(default)]
    images_source: Option<models::ImagesSource>,
    sku: String,
    merchant_location_key: String,
    fulfillment_policy_id: String,
    payment_policy_id: String,
    return_policy_id: String,
    #[serde(default)]
    marketplace: models::MarketplaceId,
    #[serde(default)]
    overrides: Option<models::PipelineOverrides>,
}

/// Resume the pipeline with client-provided overrides.
///
/// - Method: `POST`
/// - Path: `/listings/continue`
/// - Body: ContinueRequest
/// - Response: ListingResponse
async fn create_listing_continue(
    State(state): State<AppState>,
    Extension(context): Extension<AuthContext>,
    Json(payload): Json<ContinueRequest>,
) -> Result<Json<ListingResponse>, AppError> {
    crate::metrics::inc_requests("/listings/continue");
    let images_source = payload
        .images_source
        .unwrap_or(models::ImagesSource::Single(String::new()));
    let req = ListingRequest {
        images_source,
        sku: payload.sku,
        merchant_location_key: payload.merchant_location_key,
        fulfillment_policy_id: payload.fulfillment_policy_id,
        payment_policy_id: payload.payment_policy_id,
        return_policy_id: payload.return_policy_id,
        marketplace: payload.marketplace,
        llm_provider: None,
        llm_listing_model: None,
        llm_category_model: None,
        use_signed_urls: false,
        overrides: payload.overrides,
        dry_run: false,
    };
    let response = state.pipeline.run(req, Some(context)).await?;
    Ok(Json(response))
}

#[derive(Debug)]
enum AppError {
    Pipeline(PipelineError),
}

impl From<PipelineError> for AppError {
    fn from(value: PipelineError) -> Self {
        Self::Pipeline(value)
    }
}

#[derive(Debug, Serialize)]
struct EnqueueResponse {
    job_id: String,
}

async fn enqueue_listing_job(
    State(state): State<AppState>,
    Extension(context): Extension<AuthContext>,
    Json(payload): Json<ListingRequest>,
) -> Result<Json<EnqueueResponse>, AppError> {
    crate::metrics::inc_requests("/jobs/listings");
    let id = state
        .queue
        .enqueue_listing(payload, context)
        .await
        .map_err(|err| AppError::Pipeline(PipelineError::internal("enqueue", err.error)))?;
    Ok(Json(EnqueueResponse {
        job_id: id.to_string(),
    }))
}

async fn enqueue_continue_job(
    State(state): State<AppState>,
    Extension(context): Extension<AuthContext>,
    Json(payload): Json<ContinueRequest>,
) -> Result<Json<EnqueueResponse>, AppError> {
    crate::metrics::inc_requests("/jobs/listings/continue");
    let images_source = payload
        .images_source
        .unwrap_or(models::ImagesSource::Single(String::new()));
    let req = ListingRequest {
        images_source,
        sku: payload.sku,
        merchant_location_key: payload.merchant_location_key,
        fulfillment_policy_id: payload.fulfillment_policy_id,
        payment_policy_id: payload.payment_policy_id,
        return_policy_id: payload.return_policy_id,
        marketplace: payload.marketplace,
        llm_provider: None,
        llm_listing_model: None,
        llm_category_model: None,
        use_signed_urls: false,
        overrides: payload.overrides,
        dry_run: false,
    };
    let id = state
        .queue
        .enqueue_listing(req, context)
        .await
        .map_err(|err| AppError::Pipeline(PipelineError::internal("enqueue", err.error)))?;
    Ok(Json(EnqueueResponse {
        job_id: id.to_string(),
    }))
}

async fn get_job_status(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<jobs::JobInfo>, AppError> {
    let Ok(uuid) = uuid::Uuid::parse_str(&id) else {
        return Err(AppError::Pipeline(PipelineError::invalid_input(
            "jobs",
            "invalid_job_id",
        )));
    };
    if let Some(info) = state.queue.get(uuid).await {
        Ok(Json(info))
    } else {
        Err(AppError::Pipeline(PipelineError::invalid_input(
            "jobs",
            "not_found",
        )))
    }
}
impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        match self {
            AppError::Pipeline(err) => {
                let status = match err.kind() {
                    PipelineErrorKind::InvalidInput => StatusCode::BAD_REQUEST,
                    PipelineErrorKind::Internal => StatusCode::INTERNAL_SERVER_ERROR,
                };
                let payload = ApiError {
                    error: err.stage().to_string(),
                    detail: Some(err.detail().to_string()),
                };
                (status, Json(payload)).into_response()
            }
        }
    }
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,tower_http=debug"));
    let _ = fmt().with_env_filter(filter).try_init();
}
// -------- Stage endpoints (manual granular control) --------
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
struct ResolveImagesRequest {
    images_source: models::ImagesSource,
    #[serde(default)]
    use_signed_urls: bool,
}

#[derive(Debug, Serialize)]
struct ResolveImagesResponse {
    images: Vec<String>,
}

async fn stage_resolve_images(
    Json(req): Json<ResolveImagesRequest>,
) -> Result<Json<ResolveImagesResponse>, AppError> {
    crate::metrics::inc_requests("/stages/resolve_images");
    let listing = ListingRequest {
        images_source: req.images_source,
        sku: "_stage_".into(),
        merchant_location_key: "_stage_".into(),
        fulfillment_policy_id: "_stage_".into(),
        payment_policy_id: "_stage_".into(),
        return_policy_id: "_stage_".into(),
        marketplace: models::MarketplaceId::default(),
        llm_provider: None,
        llm_listing_model: None,
        llm_category_model: None,
        use_signed_urls: req.use_signed_urls,
        overrides: None,
        dry_run: false,
    };
    let out = pipeline::stages::resolve_images(&listing)
        .await
        .map_err(AppError::from)?;
    Ok(Json(ResolveImagesResponse { images: out.value }))
}

#[derive(Debug, Deserialize)]
struct SelectCategoryRequest {
    images: Vec<String>,
    sku: String,
    merchant_location_key: String,
    fulfillment_policy_id: String,
    payment_policy_id: String,
    return_policy_id: String,
    #[serde(default)]
    marketplace: models::MarketplaceId,
}

#[derive(Debug, Serialize)]
struct SelectCategoryResponse {
    selection: pipeline::CategorySelection,
    alternatives: Vec<serde_json::Value>,
}

async fn stage_select_category(
    State(state): State<AppState>,
    Json(req): Json<SelectCategoryRequest>,
) -> Result<Json<SelectCategoryResponse>, AppError> {
    crate::metrics::inc_requests("/stages/select_category");
    // Build a lightweight ListingRequest to compute the seed deterministically
    let listing = ListingRequest {
        images_source: models::ImagesSource::Multiple(vec![]),
        sku: req.sku,
        merchant_location_key: req.merchant_location_key,
        fulfillment_policy_id: req.fulfillment_policy_id,
        payment_policy_id: req.payment_policy_id,
        return_policy_id: req.return_policy_id,
        marketplace: req.marketplace,
        llm_provider: None,
        llm_listing_model: None,
        llm_category_model: None,
        use_signed_urls: false,
        overrides: None,
        dry_run: false,
    };
    let seed = pipeline::compute_seed(&listing, &req.images);
    let out = pipeline::stages::select_category(
        &listing,
        &req.images,
        state.pipeline.config.categories,
        seed,
    )
    .await
    .map_err(AppError::from)?;
    // mirror stage output: include alternatives from the JSON output
    let alternatives = out
        .output
        .get("alternatives")
        .cloned()
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default();
    Ok(Json(SelectCategoryResponse {
        selection: out.value,
        alternatives,
    }))
}

#[derive(Debug, Deserialize)]
struct ExtractProductRequest {
    sku: String,
    images: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ExtractProductResponse {
    product: crate::hsuf::models::Product,
}

async fn stage_extract_product(
    State(state): State<AppState>,
    Json(req): Json<ExtractProductRequest>,
) -> Result<Json<ExtractProductResponse>, AppError> {
    crate::metrics::inc_requests("/stages/extract_product");
    let listing = ListingRequest {
        images_source: models::ImagesSource::Multiple(vec![]),
        sku: req.sku.clone(),
        merchant_location_key: "_stage_".into(),
        fulfillment_policy_id: "_stage_".into(),
        payment_policy_id: "_stage_".into(),
        return_policy_id: "_stage_".into(),
        marketplace: models::MarketplaceId::default(),
        llm_provider: None,
        llm_listing_model: None,
        llm_category_model: None,
        use_signed_urls: false,
        overrides: None,
        dry_run: false,
    };
    let llm = &state.pipeline.llm;
    let out = pipeline::stages::extract_product(&listing, &req.images, 0, llm)
        .await
        .map_err(AppError::from)?;
    Ok(Json(ExtractProductResponse { product: out.value }))
}

#[derive(Debug, Deserialize)]
struct DescriptionRequest {
    title: String,
    bullets: Vec<String>,
}

#[derive(Debug, Serialize)]
struct DescriptionResponse {
    description: String,
    used_fallback: bool,
}

async fn stage_description(
    State(state): State<AppState>,
    Json(req): Json<DescriptionRequest>,
) -> Result<Json<DescriptionResponse>, AppError> {
    crate::metrics::inc_requests("/stages/description");
    let prompt = format!(
        "Generate a compelling, policy-compliant eBay listing description. Title: {title}. Bullet points: {bullets:?}.",
        title = req.title,
        bullets = req.bullets,
    );
    let llm = &state.pipeline.llm;
    match llm
        .chat(&[llm::LlmMessage {
            role: "user".into(),
            content: prompt,
        }])
        .await
    {
        Ok(resp) => Ok(Json(DescriptionResponse {
            description: resp.text,
            used_fallback: false,
        })),
        Err(_) => {
            let mut fallback = String::new();
            fallback.push_str(&format!("{}\n\n", req.title));
            fallback.push_str("Highlights:\n");
            for b in &req.bullets {
                fallback.push_str(&format!("- {}\n", b));
            }
            fallback.push_str("\nAuto-generated demo description. Details may be approximations.");
            Ok(Json(DescriptionResponse {
                description: fallback,
                used_fallback: true,
            }))
        }
    }
}
