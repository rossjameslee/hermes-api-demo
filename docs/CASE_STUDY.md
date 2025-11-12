# Hermes Demo: Case Study

## Goals
- Demonstrate Rust/AI infra capability without exposing Python “crown jewels”.
- Show an image(s) → eBay listing agentic pipeline with staged outputs.
- Keep the service demo‑safe, offline by default, and simple to deploy.

## Design Highlights
- Axum/Tokio HTTP service with clean routing and state wiring.
- Staged pipeline with transcripts for each step; deterministic seeds for repeatability.
- Human‑in‑the‑loop via granular `/stages/*` endpoints + `/listings/continue`.
- Idempotency (in‑memory + optional Redis) with TTL; body size limits; max‑images guard; URL validation with optional domain allowlist.
- Optional toggles: LLM description (fallback on failure), live-ish eBay path (stubs when disabled).
- OpenAPI + Swagger UI; Prometheus exporter + `/metrics` endpoint; optional gates for both.

### HSUF in this demo
HSUF is the intermediate “product truth” the pipeline builds from images and
text. It follows schema.org/Google Merchant conventions so it maps cleanly into
e‑commerce channel payloads. Treat it like an ONNX‑style interchange format for
listings: portable, channel‑agnostic, and evolution‑friendly.

## Why This Architecture
- Clear separation between orchestration (pipeline) and integrations (eBay/LLM/Supabase).
- Easy demo path: works offline with realistic stubs; flips to live with env flags.
- Deterministic staging and transcripts make it ideal for debugging and interviews.

## What’s Stubbed vs. Live
- eBay flows default to stubs; enable `EBAY_ENABLE_NETWORK=true` and `EBAY_REFRESH_TOKEN` for live calls.
- LLM description uses TensorZero when configured; otherwise templated fallback.
- Metrics export is wired; recording kept lightweight in demo to avoid macro churn.

## Security & Safety
- API‑key auth with per‑org rate limiting.
- Request body limit, URL scheme/domain validation, max image count.
- Optional gates for `/openapi.json` and `/metrics` endpoints with static keys.

## Next Steps Toward Prod‑Grade
- Metrics: align crate versions and add labeled counters/histograms via a tiny helper (stable names; bounded label spaces).
- K8s: Ingress + TLS (added), consider a minimal Helm chart instead of raw manifests.
- Terraform: expose ingress outputs (added), optional Redis Helm release module.
- CI: add `cargo-audit`, `cargo-deny`, SBOM (Syft), and optional image signing (cosign).
- Observability: structured logs (JSON), sampling, request IDs, and traces.

## Demo Safety
- No secrets checked in; offline by default; live behavior is explicit and environment‑gated.
- Python implementation remains private; this Rust demo is an independent seed.

## Known Limitations
- URL checks are syntactic (scheme/host), not reachability checks.
- Metrics recording uses trace events in the demo build; counters/histograms can be wired later without changing call sites.
- No durable database aside from optional Redis for idempotency.
- HSUF extraction uses a fallback when LLM is not configured.
