# Hermes Architecture (Text Diagram)

## High‑Level Flow

images → HSUF Product → e‑commerce listing payload → e‑commerce platform

- Images normalized/deduped, category selected, taxonomy fetched.
- Product extracted as HSUF (LLM with fallback when offline).
- Listing payload composed (title, description, pricing, aspects, package).
- Inventory upserted, then offer published (stubs by default; live when env‑gated).

See also: [What is HSUF?](../README.md#what-is-hsuf)

## One‑Shot Path (POST /listings)

Client
  |
  V
Resolve Images
  - If overrides.resolved_images → use provided
  - Else normalize/dedupe images_source
  |
  V
Select Category
  - If overrides.category → use provided
  - Else deterministic pick + alternatives
  |
  V
Fetch Taxonomy
  - Sample aspects derived from category
  |
  V
Acquire User Token
  - Demo token (live if configured)
  |
  V
Prepare Conditions
  - Allowed conditions by category
  |
  V
Extract Product
  - If overrides.product → use provided (HSUF Product)
  - Else LLM → HSUF Product (fallback if offline)
  |
  V
Build Listing
  - Title, aspects, description (LLM → fallback), packaging
  |
  V
[If dry_run = true] → Early return with PREVIEW-… id and stages
  |
  V
Push Inventory
  - Stub or live (env gated)
  |
  V
Publish Offer
  - Stub or live; returns synthetic or live ID
  |
  V
ListingResponse { listing_id, stages[] }

## Granular Edit + Continue Path

Client
  |
  +-> POST /stages/resolve_images → images[]
  |
  +-> POST /stages/select_category (images) → selection + alternatives
  |
  +-> POST /stages/extract_product (sku, images) → product
  |
  +-> (Optional) POST /stages/description (title, bullets) → description
  |
  +-> POST /listings/continue with overrides
      - resolved_images: images[]
      - category: selection
      - product: edited HSUF Product
      V
Server resumes:
  fetch_taxonomy → acquire_user_token → prepare_conditions
  (skip extract_product if provided)
  build_listing → push_inventory → publish_offer
  |
  V
ListingResponse
