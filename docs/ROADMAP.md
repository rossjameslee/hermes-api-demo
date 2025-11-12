Security
- Harden auth and keys: rotateable API keys, secret management (Vault/Cloud KMS), no secrets in env for long-lived prod.
- Tighten CORS: restrict origins/methods/headers to known clients.
- Strict input validation: already have size + image count; add URL scheme/domain allowlists and MIME sniffing for images.
- AuthZ hooks: per‑org feature flags/toggles; lock/disable keys; audit trail.
- Protect /metrics and /openapi.json: auth-gate or IP‑allowlist; production docs behind internal perimeter.

Reliability & Resilience
- Idempotency persistence: move the in‑memory store to a durable backend (Redis/Supabase) with TTL + eviction policy.
- Durable jobs: replace in‑mem mpsc with broker-backed queues (e.g., NATS/Redis/Cloud Tasks) + replay support.
- Circuit breakers/timeouts/retries: for LLM/eBay/Supabase with exponential backoff; respect eBay rate-limit headers.
- Graceful shutdown: drain in‑flight requests; signal handling; bounded shutdown timeout.
- Concurrency controls: semaphore per integration; pool sizing via config.

Observability
- Tracing + correlation: add request IDs; propagate to LLM/eBay; structured logs (JSON) with org_id, api_key_id, listing_id.
- Metrics: add counters/histograms (requests_total{route}, latency_seconds_bucket{route}, job_state, eBay API calls, LLM calls).
- Dashboards/alerts: SLOs for p95 latency and error rates; queue depth; eBay/LLM failure spikes; idempotency saturation.

Data & Persistence
- Stage transcript store: optionally write ListingResponse + stage logs to a DB (short TTL) for support/debug.
- Supabase policies: when used live, RLS and least-privileged service role; periodic key rotation.
- No PII: confirm payloads/logs redaction; zero-PII by design.

Correctness & Safety
- eBay live mode safeguards: sandbox vs prod endpoints; explicit opt-in; smoke checks on policy/location validity.
- LLM hardening: stricter JSON schema validation for product; model timeouts; content guardrails; retry with different prompts/temperatures.
- Validation of overrides: deep schema validation for product JSON, category match to taxonomy, resolved URLs accessible.

API Surface & DX
- Versioning: prefix v1 for /listings and stage endpoints; deprecate gracefully.
- OpenAPI completeness: examples for every route; detailed error schemas; auth docs; rate-limit headers; idempotency semantics.
- SDKs or code samples: small TS/Python snippets for one‑shot, granular, and continue flows.

Deployment & Ops
- Docker hardening: non‑root user; minimal base image; multi‑arch builds.
- Kubernetes manifests/Helm: HPA, resource requests/limits, liveness/readiness probes, env/configmap/secrets wiring.
- Rollouts: blue/green or canary; rollback; feature flags.
- CI/CD: current CI has fmt/clippy/test/openapi‑lint. Add:
  - cargo deny + cargo audit (supply-chain vuln checks)
  - SBOM export (Syft) and artifact signing (cosign)
  - Image vulnerability scanning (Trivy/Grype)

Performance & Scale
- Load testing: k6/gatling profiles for common paths (one‑shot, granular+continue, queue).
- Cache eBay taxonomy: reduce cold latency; background refresh.
- Concurrency budgets: tune tokio runtime and HTTP client pools; set HTTP connect/read timeouts.

Runbooks & Governance
- Runbooks: common errors (eBay 429/5xx, LLM timeouts), throttle exceed, queue saturation; on-call steps.
- SLOs: publish targets (availability/latency), alert policies, and “what happens during maintenance.”

Small high‑value changes to land next
- Metrics: add hermes_requests_total and request latency histograms by route.
- Auth‑gate /metrics (shared key or org-based guard).
- eBay/Supabase/LLM client timeouts + retry/backoff policy (even for demo toggles).
- Idempotency persistence via Redis (feature‑flagged).
- K8s manifests (Deployment/Service/Ingress + HPA + Secrets).
- CI hardening: cargo deny/audit and SBOM step.
