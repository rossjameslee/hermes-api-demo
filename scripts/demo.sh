#!/usr/bin/env bash
set -euo pipefail

BASE_URL=${BASE_URL:-http://localhost:8000}
AUTH=${AUTH:-demo-key}
OUT=${OUT:-examples/responses}

mkdir -p "$OUT"

echo "[1/3] One-shot dry-run → $OUT/listing_dry_run.json"
curl -sS -X POST "$BASE_URL/listings" \
  -H 'Content-Type: application/json' \
  -H "Authorization: Bearer $AUTH" \
  -d @examples/requests/listing_dry_run.json \
  | jq . > "$OUT/listing_dry_run.json"

echo "[2/3] Stage: resolve_images → $OUT/stage_resolve_images.json"
curl -sS -X POST "$BASE_URL/stages/resolve_images" \
  -H 'Content-Type: application/json' \
  -H "Authorization: Bearer $AUTH" \
  -d @examples/requests/stage_resolve_images.json \
  | jq . > "$OUT/stage_resolve_images.json"

echo "[3/3] OpenAPI → $OUT/openapi.json"
curl -sS "$BASE_URL/openapi.json" | jq . > "$OUT/openapi.json"

echo "Done. See $OUT for sample payloads."

