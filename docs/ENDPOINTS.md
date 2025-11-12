# API Endpoints

Hermes exposes a minimal HTTP surface to run the images → eBay listing demo.

Base URL: `http://localhost:8000`

Auth: send either header on protected routes
- `Authorization: Bearer <key>`
- `X-Hermes-Key: <key>`

Default demo key: `demo-key` (org `demo-org`). Configure custom keys with `DEMO_API_KEYS="org1:key1,org2:key2"`.

---

GET /health
- Summary: Readiness probe
- Auth: none
- Response: 200 OK
  - Body:
    - `{ "status": "ok", "service": "hermes-api-rs" }`

Example:
```
curl -s http://localhost:8000/health
```

---

POST /listings
- Summary: Run the agentic pipeline to produce an eBay listing plan from image URLs
- Auth: required
- Request body: ListingRequest (JSON)
  - `images_source`: string | string[] – one or more image URLs
  - `sku`: string – your SKU identifier
  - `merchant_location_key`: string – eBay merchant location key
  - `fulfillment_policy_id`: string – eBay fulfillment policy ID
  - `payment_policy_id`: string – eBay payment policy ID
  - `return_policy_id`: string – eBay return policy ID
  - `marketplace`: "EBAY_US" | "EBAY_GB" | "EBAY_DE" (optional; default EBAY_US)
  - `use_signed_urls`: boolean (optional) – append `signature=demo` to images

Response: 200 OK, ListingResponse (JSON)
- `listing_id`: string – synthetic or live listing ID
- `stages`: StageReport[] – transcript of each pipeline stage
  - `name`: stage name
  - `elapsed_ms`: execution time
  - `timestamp`: RFC3339 timestamp
  - `output`: stage-specific JSON payload

Example:
```
curl -sS -X POST http://localhost:8000/listings \
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

Notes
- Offline by default: the service returns realistic stubs and a synthetic `HER-…` ID.
- Optional LLM: set `TENSORZERO_*` env vars. If unavailable, description falls back automatically.
- Optional live eBay callouts: set `EBAY_ENABLE_NETWORK=true` and `EBAY_REFRESH_TOKEN`.
 - Image URL validation: only `http`/`https` are accepted. Optionally restrict to domains via `IMAGE_DOMAIN_ALLOWLIST` (comma‑separated hostnames).
 - Image count: limited by `MAX_IMAGES` (default 6). Oversized requests are rejected.

Overrides (optional)
- Add an `overrides` object to the POST /listings request to inject manual edits:
  - `resolved_images`: string[] – skip image resolution
  - `category`: { id, tree_id, label, confidence, rationale } – skip selection
  - `product`: object – HSUF Product JSON (skip extraction)

Example:
```
{
  "images_source": "",  // ignored if overrides.resolved_images present
  "sku": "demo-1",
  "merchant_location_key": "loc-1",
  "fulfillment_policy_id": "f-1",
  "payment_policy_id": "p-1",
  "return_policy_id": "r-1",
  "overrides": {
    "resolved_images": ["https://…/a.jpg"],
    "category": {"id": "11450", "tree_id": "0", "label": "Clothing, Shoes & Accessories", "confidence": 0.9, "rationale": "manual"},
    "product": {"name": "Edited Title", "image": "https://…/a.jpg", "offers": {"price": 79.0, "priceCurrency": "USD"}}
  }
}
```

---

POST /listings/continue
- Summary: Resume the pipeline while honoring manual edits via `overrides`
- Auth: required
- Body: ContinueRequest (same fields as `POST /listings`, but `images_source` is optional)
- Response: ListingResponse

---

POST /jobs/listings
- Summary: Enqueue a listing job (returns `job_id`); useful for async processing
- Auth: required
- Body: ListingRequest
- Response: `{ "job_id": "…" }`

POST /jobs/listings/continue
- Summary: Enqueue a continue job with overrides
- Auth: required
- Body: ContinueRequest
- Response: `{ "job_id": "…" }`

GET /jobs/{id}
- Summary: Get job status
- Auth: required
- Response: `{ id, state: "queued|running|completed|failed", result?, error?, stage? }`

---

POST /stages/resolve_images
- Summary: Normalize and deduplicate image URLs
- Auth: required
- Body:
  - `images_source`: string | string[]
  - `use_signed_urls`: boolean (optional)
- Response: `{ "images": ["https://…", …] }`

Example:
```
curl -sS -X POST http://localhost:8000/stages/resolve_images \
  -H 'Content-Type: application/json' -H 'Authorization: Bearer demo-key' \
  -d '{"images_source": "https://ex.com/a.jpg, https://ex.com/b.jpg", "use_signed_urls": true}'
```

---

POST /stages/select_category
- Summary: Choose an eBay category deterministically
- Auth: required
- Body:
  - `images`: string[] – resolved image URLs
  - `sku`, `merchant_location_key`, `fulfillment_policy_id`, `payment_policy_id`, `return_policy_id`
  - `marketplace` (optional)
- Response: `{ "selection": {…}, "alternatives": [ … ] }`

---

POST /stages/extract_product
- Summary: Convert images → HSUF Product (LLM with fallback)
- Auth: required
- Body: `{ "sku": "…", "images": ["https://…"] }`
- Response: `{ "product": { … } }`

---

POST /stages/description
- Summary: Generate (or fallback) listing description from title + bullets
- Auth: required
- Body: `{ "title": "…", "bullets": ["…", "…"] }`
- Response: `{ "description": "…", "used_fallback": true|false }`

---

Docs & OpenAPI
- `GET /openapi.json` – OpenAPI JSON (served from `docs/openapi.yaml`). Optionally gate with `OPENAPI_KEY` and header `X-Docs-Key`.
- `GET /docs` – Swagger UI for browsing the API.

Metrics
- `GET /metrics` – Prometheus metrics endpoint (exporter installed). Optionally gate with `METRICS_KEY` and header `X-Metrics-Key`.

Request Limits & Validation
- `REQUEST_MAX_BYTES` – max body size in bytes (default 262,144).
- URL validation in `/stages/resolve_images` and `/listings`: only `http/https` schemes; optional domain allowlist via `IMAGE_DOMAIN_ALLOWLIST`.
