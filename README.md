# Hermes API (Rust demo)

This crate hosts a self‑contained Axum service that showcases an agentic listing
pipeline in Rust without exposing any “crown jewels” (billing, proprietary
logic, or auth internals). It keeps the `/listings` contract (request schema,
stage transcript, synthetic listing id) and returns stable JSON shapes, with a
deterministic, offline‑first implementation and live integrations gated by
environment variables.

## Endpoints

| Method | Path        | Notes |
| ------ | ----------- | ----- |
| GET    | `/health`   | Simple readiness check. |
| POST   | `/listings` | Executes the staged pipeline and returns a `ListingResponse`. |
| POST   | `/listings/continue` | Resumes pipeline with client overrides (granular flow). |
| POST   | `/stages/*` | Granular stage endpoints for human‑in‑the‑loop edits. |
| GET    | `/openapi.json` | OpenAPI JSON (served from `docs/openapi.yaml`). |
| GET    | `/docs` | Swagger UI. |
| GET    | `/metrics` | Prometheus metrics (optional gate). |

## Running locally

```bash
cd hermes-api-rs
cargo run --release  # set PORT to override the default 8000
```

The server logs to stdout via `tracing_subscriber`.
Common env vars:

- `PORT` (default `8000`)
- `DEMO_API_KEYS` (e.g., `demo-org:demo-key`; comma‑separated list)
- `RATE_LIMIT_PER_SEC`, `RATE_LIMIT_CAPACITY`
- `REQUEST_MAX_BYTES` (default `262144`)
- `MAX_IMAGES` (default `6`)
- `IMAGE_DOMAIN_ALLOWLIST` (comma‑separated hosts; subdomains allowed)
- `OPENAPI_KEY` (optional; require `X-Docs-Key` for `/openapi.json`)
- `METRICS_KEY` (optional; require `X-Metrics-Key` for `/metrics`)

Set `DEMO_API_KEYS` to control which API keys are accepted. Entries are comma-
separated `org_id:key` pairs (default `demo-org:demo-key`). Example:

```bash
export DEMO_API_KEYS="acme:sk_live_demo,venture:sk_live_other"
export RATE_LIMIT_PER_SEC=5
export RATE_LIMIT_CAPACITY=10
export TENSORZERO_GATEWAY_URL="http://localhost:3000"
export TENSORZERO_API_KEY="sk_your_tensorzero_api_key"
export TENSORZERO_FUNCTION="hsuf_enrichment"
export TENSORZERO_MODEL="openai::gpt-5-mini"
export EBAY_ENABLE_NETWORK=true
export EBAY_REFRESH_TOKEN="<ebay refresh token>"
export SUPABASE_URL="https://your-project.supabase.co"
export SUPABASE_SERVICE_ROLE_KEY="<service-role-key>"
export REQUEST_MAX_BYTES=262144
export MAX_IMAGES=6
export IMAGE_DOMAIN_ALLOWLIST="example.com, imgur.com"
```

All non‑health routes require either `Authorization: Bearer <key>` or
`X-Hermes-Key: <key>`. Per‑org rate limiting is enforced using a token bucket
fed by `RATE_LIMIT_PER_SEC` (tokens/sec) and `RATE_LIMIT_CAPACITY` (burst size).

`extract_product` can call a TensorZero gateway to convert the provided image
URLs into a Product (HSUF) payload; if not configured, a deterministic fallback
is used. `build_listing` similarly uses the gateway for description enrichment
when available. With `EBAY_ENABLE_NETWORK=true`, the pipeline can fetch a user
access token via `EBAY_REFRESH_TOKEN` and push inventory + offers to eBay. When
`SUPABASE_URL`/`SUPABASE_SERVICE_ROLE_KEY` are set, per‑org defaults (policies,
merchant location, address) are pulled from `public.ebay_org_config`.

## Example request

```bash
curl -X POST http://localhost:8000/listings \
  -H 'Content-Type: application/json' \
  -H 'Authorization: Bearer demo-key' \
  -d '{
        "images_source": [
          "https://example.com/image-1.jpg",
          "https://example.com/image-2.jpg"
        ],
        "sku": "demo-sku-001",
        "merchant_location_key": "loc-1",
        "fulfillment_policy_id": "fulfill-123",
        "payment_policy_id": "payment-123",
        "return_policy_id": "return-123"
      }'
```

The response uses the `ListingResponse` format:

```json
{
  "listing_id": "HER-14c8af...",
  "stages": [
    {"name": "resolve_images", "elapsed_ms": 19, "output": {"count": 2, ...}},
    {"name": "select_category", "elapsed_ms": 23, "output": {"selected": {...}}},
    ...
  ]
}
```

Each stage returns structured data so you can render the pipeline transcript in
clients or log drains.

## Two‑Terminal Demo

- Terminal 1 (server):
  - `cargo run --release`
  - Server listens on `0.0.0.0:8000`

- Terminal 2 (client):
  - Optional helpers:
    - `export BASE=http://localhost:8000`
    - `export AUTH="Authorization: Bearer demo-key"`
  - Health:
    - `curl -s $BASE/health | jq .`
  - One‑shot listing (dry run):
    - Inline: `curl -sS -X POST $BASE/listings -H "$AUTH" -H 'Content-Type: application/json' -d '{"images_source":["https://example.com/a.jpg","https://example.com/b.jpg"],"sku":"demo-sku-001","merchant_location_key":"loc-1","fulfillment_policy_id":"fulfill-123","payment_policy_id":"payment-123","return_policy_id":"return-123","dry_run":true}' | jq .`
    - From repo sample: `curl -sS -X POST $BASE/listings -H "$AUTH" -H 'Content-Type: application/json' -d @examples/requests/listing_dry_run.json | jq .`
  - Idempotency (same body/key):
    - `curl -sS -X POST $BASE/listings -H "$AUTH" -H 'Content-Type: application/json' -H 'Idempotency-Key: demo-123' -d @examples/requests/listing_dry_run.json | jq .`
    - `curl -sS -X POST $BASE/listings -H "$AUTH" -H 'Content-Type: application/json' -H 'Idempotency-Key: demo-123' -d @examples/requests/listing_dry_run.json | jq .`
  - Granular stages:
    - Resolve images: `curl -sS -X POST $BASE/stages/resolve_images -H "$AUTH" -H 'Content-Type: application/json' -d @examples/requests/stage_resolve_images.json | jq .`
    - Select category: `curl -sS -X POST $BASE/stages/select_category -H "$AUTH" -H 'Content-Type: application/json' -d '{"images":["https://example.com/a.jpg","https://example.com/b.jpg"],"sku":"demo-sku-001","merchant_location_key":"loc-1","fulfillment_policy_id":"fulfill-123","payment_policy_id":"payment-123","return_policy_id":"return-123","marketplace":"EBAY_US"}' | jq .`
    - Extract product: `curl -sS -X POST $BASE/stages/extract_product -H "$AUTH" -H 'Content-Type: application/json' -d '{"sku":"demo-sku-001","images":["https://example.com/a.jpg","https://example.com/b.jpg"]}' | jq .`
    - Description: `curl -sS -X POST $BASE/stages/description -H "$AUTH" -H 'Content-Type: application/json' -d '{"title":"Demo Title","bullets":["Feature A","Feature B","Feature C"]}' | jq .`
  - Resume with overrides:
    - `curl -sS -X POST $BASE/listings/continue -H "$AUTH" -H 'Content-Type: application/json' -d '{"images_source":[],"sku":"demo-sku-001","merchant_location_key":"loc-1","fulfillment_policy_id":"fulfill-123","payment_policy_id":"payment-123","return_policy_id":"return-123","overrides":{"resolved_images":["https://example.com/a.jpg","https://example.com/b.jpg"]}}' | jq .`
  - Async jobs:
    - Enqueue: `curl -sS -X POST $BASE/jobs/listings -H "$AUTH" -H 'Content-Type: application/json' -d @examples/requests/listing_dry_run.json | jq .`
    - Poll: `curl -sS $BASE/jobs/{id} -H "$AUTH" | jq .`
  - Docs & metrics:
    - Swagger UI: open `$BASE/docs`
    - OpenAPI JSON: `curl -sS $BASE/openapi.json | jq .`
    - Metrics: `curl -sS $BASE/metrics`

See also `scripts/demo.sh` for an automated walkthrough.

## How It Works (High Level)

- images → HSUF Product (normalize/dedupe image URLs, pick category, fetch taxonomy, extract product via LLM with fallback)
- HSUF Product → e‑commerce listing payload (title, description, aspects, pricing, packaging)
- listing payload → e‑commerce platform (inventory upsert → offer publish; stubbed by default, live when env‑gated)
- Every stage emits a transcript entry: `name`, `elapsed_ms`, `timestamp`, `output`

### What is HSUF?

HSUF (Hermes Structured Unified Format) is a normalized product schema aligned
with schema.org Product and Google Merchant requirements. Think of it as an
“ONNX for e‑commerce listings”: a portable, implementation‑agnostic
intermediate representation that the pipeline produces from images/LLM and then
transforms into marketplace‑specific payloads (eBay, etc.).

- Purpose: a stable IR between extraction and channel adapters.
- Coverage: core product fields (name, description, images, brand, MPN/GTIN),
  variant attributes (color/size/material), and commercial terms (offers: price
  + currency).
- Benefits: predictable shape, easier testing, loss‑aware mappings to multiple
  marketplaces, and safer evolution of extraction logic without touching channel
  code.

## Docs & Metrics

- `GET /openapi.json` – OpenAPI served from `docs/openapi.yaml` (optionally gated by `OPENAPI_KEY` + `X-Docs-Key`).
- `GET /docs` – Swagger UI that consumes `/openapi.json`.
- `GET /metrics` – Prometheus metrics (optionally gated by `METRICS_KEY` + `X-Metrics-Key`).

## Limitations (Demo Mode)

- Offline by default; deterministic stubs for external services.
- URL validation is basic (scheme + optional domain allowlist), no HEAD checks.
- Metrics endpoint is wired; recording is trace‑based in this demo.
- No persistent DB beyond optional Redis for idempotency.

## Next Steps

- Metrics: align crate versions and record labeled counters/histograms via a tiny helper.
- K8s/TF: Ingress with TLS (provided), plus optional Redis Helm release module; consider a minimal Helm chart.
- CI: add `cargo‑audit`, `cargo‑deny`, SBOM (Syft), optional image signing (cosign).
- Observability: JSON logs, request IDs, and tracing spans.

## Code map

- `src/main.rs` – Axum router, state wiring, JSON handlers.
- `src/models.rs` – Serde models matching the FastAPI request/response shapes.
- `src/pipeline.rs` – Staged orchestration with structured outputs per stage (`resolve_images`, `select_category`, `fetch_taxonomy`, …)
- `src/hsuf/*` – Product extraction + listing transformation helpers
- `src/ebay/*` – eBay request payloads and stubs
- `src/security.rs` – API‑key auth and per‑org rate limiting
- `docs/ENDPOINTS.md` – Full HTTP contract reference and examples
- `docs/ARCHITECTURE.md` – Text diagrams for one‑shot and granular paths
- `docs/CASE_STUDY.md` – Design, tradeoffs, and next steps

`cargo check` and `cargo fmt` pass, so you can iterate with standard Rust tooling.
