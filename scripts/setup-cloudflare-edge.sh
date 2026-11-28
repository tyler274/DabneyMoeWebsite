#!/usr/bin/env bash
# Enable Cloudflare Free edge protection (proxied DNS, Full Strict SSL,
# ACME-safe HTTPS redirect, Anubis-safe static caching, free edge rate limit,
# and Bot Fight Mode) in front of dabney.moe on Cloud Run.
#
# Keeps Anubis config (anubis/botPolicies.yaml, terraform/gcp/anubis.tf, and
# the Artifact Registry mirror) intact so you can run either:
#   - Cloudflare + Anubis together (default: --mode both)
#   - Cloudflare primary with Anubis on standby fallback (--mode cloudflare-only)
#
# Usage:
#   direnv exec . ./scripts/setup-cloudflare-edge.sh [--mode both|cloudflare-only] [--bot-detection on|off] [--plan-only]
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PROJECT="${GCP_PROJECT_ID:-dabney-moe-website}"
REGION="${GCP_REGION:-us-central1}"
BILLING_ACCOUNT_ID="${GCP_BILLING_ACCOUNT_ID:-01A29E-334D38-0B8604}"
MONTHLY_BUDGET_USD="${GCP_MONTHLY_BUDGET_USD:-5}"
DOMAIN="${CUSTOM_DOMAIN:-dabney.moe}"
MODE="cloudflare-only"
BOT_DETECTION="off"
PLAN_ONLY="false"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --mode)
      MODE="${2:?--mode requires both or cloudflare-only}"
      shift 2
      ;;
    --bot-detection)
      BOT_DETECTION="${2:?--bot-detection requires on or off}"
      shift 2
      ;;
    --plan-only)
      PLAN_ONLY="true"
      shift
      ;;
    -h|--help)
      sed -n '2,13p' "$0"
      exit 0
      ;;
    *)
      echo "error: unknown flag: $1" >&2
      exit 1
      ;;
  esac
done

case "$MODE" in
  both)            ENABLE_ANUBIS="true" ;;
  cloudflare-only) ENABLE_ANUBIS="false" ;;
  *)
    echo "error: --mode must be 'both' or 'cloudflare-only' (got '$MODE')" >&2
    exit 1
    ;;
esac

case "$BOT_DETECTION" in
  on)  ENABLE_BOT_DETECTION="true" ;;
  off) ENABLE_BOT_DETECTION="false" ;;
  *)
    echo "error: --bot-detection must be 'on' or 'off' (got '$BOT_DETECTION')" >&2
    exit 1
    ;;
esac

for bin in tofu gcloud curl python3; do
  if ! command -v "$bin" >/dev/null 2>&1; then
    echo "error: '$bin' not found. Run inside the dev shell: direnv exec . $0" >&2
    exit 1
  fi
done

cat <<'EOF'
Cloudflare setup URLs (one-time, in your browser):

  1. Zone ID (dabney.moe Overview -> right sidebar -> API -> Zone ID):
       https://dash.cloudflare.com/?to=/:account/dabney.moe

  2. Create a scoped API Token -> "Create Custom Token":
       https://dash.cloudflare.com/profile/api-tokens
     Required permissions (Zone Resources -> Include -> Specific zone -> dabney.moe):
       - Zone -> DNS                -> Edit
       - Zone -> Zone Settings      -> Edit
       - Zone -> Cache Rules        -> Edit
       - Zone -> Dynamic Redirects  -> Edit   (or Single Redirect -> Edit)
       - Zone -> Zone WAF           -> Edit   (for edge rate limiting)
       - Zone -> Bot Management     -> Edit   (for Bot Fight Mode)

  3. Optional: Doppler dashboard (to also store CLOUDFLARE_API_TOKEN / ZONE_ID):
       https://dashboard.doppler.com/

EOF

if [[ -z "${CLOUDFLARE_ZONE_ID:-}" ]]; then
  read -rp "Cloudflare Zone ID for ${DOMAIN}: " CLOUDFLARE_ZONE_ID
fi
if [[ -z "${CLOUDFLARE_API_TOKEN:-}" ]]; then
  read -rsp "Cloudflare API Token (hidden): " CLOUDFLARE_API_TOKEN
  echo
fi

if [[ ! "$CLOUDFLARE_ZONE_ID" =~ ^[a-f0-9]{32}$ ]]; then
  echo "error: CLOUDFLARE_ZONE_ID should be a 32-char lowercase hex ID." >&2
  exit 1
fi
if [[ -z "$CLOUDFLARE_API_TOKEN" ]]; then
  echo "error: CLOUDFLARE_API_TOKEN cannot be empty." >&2
  exit 1
fi

echo "==> Verifying Cloudflare API token..."
curl -fsS -H "Authorization: Bearer $CLOUDFLARE_API_TOKEN" \
  "https://api.cloudflare.com/client/v4/user/tokens/verify" \
  | python3 -c 'import sys, json; d = json.load(sys.stdin); assert d.get("success"), d; print("    Token status:", d["result"]["status"])'

echo "==> Checking GCP credentials & live Cloud Run web image..."
if ! gcloud auth application-default print-access-token >/dev/null 2>&1; then
  echo "error: GCP ADC credentials missing/expired. Run:" >&2
  echo "  direnv exec . gcloud auth login && direnv exec . gcloud auth application-default login" >&2
  exit 1
fi
gcloud config set project "$PROJECT" >/dev/null
gcloud auth application-default set-quota-project "$PROJECT" >/dev/null 2>&1 || true

WEB_IMAGE="$(gcloud run services describe dabney-web \
  --project "$PROJECT" --region "$REGION" \
  --format='value(spec.template.spec.containers[name=web].image)' 2>/dev/null || true)"
if [[ -z "$WEB_IMAGE" ]]; then
  WEB_IMAGE="$(gcloud run services describe dabney-web \
    --project "$PROJECT" --region "$REGION" \
    --format='value(spec.template.spec.containers[0].image)')"
fi
echo "    Live web image: $WEB_IMAGE"

cd "$ROOT"

export TF_VAR_project_id="$PROJECT"
export TF_VAR_region="$REGION"
export TF_VAR_image="$WEB_IMAGE"
export TF_VAR_custom_domain="$DOMAIN"
export TF_VAR_ga4_measurement_id="${GA4_MEASUREMENT_ID:-G-EV45H5Y2KP}"
export TF_VAR_attach_existing_loki_secrets="true"
export TF_VAR_max_instances="${MAX_INSTANCES:-1}"
export TF_VAR_billing_account_id="$BILLING_ACCOUNT_ID"
export TF_VAR_monthly_budget_usd="$MONTHLY_BUDGET_USD"
export TF_VAR_enable_anubis="$ENABLE_ANUBIS"
export TF_VAR_enable_cloudflare_dns="true"
export TF_VAR_cloudflare_proxied="true"
export TF_VAR_enable_cloudflare_edge_protection="true"
export TF_VAR_enable_cloudflare_bot_detection="$ENABLE_BOT_DETECTION"
export TF_VAR_cloudflare_zone_id="$CLOUDFLARE_ZONE_ID"
export TF_VAR_cloudflare_api_token="$CLOUDFLARE_API_TOKEN"

echo "==> Importing existing Cloudflare A/AAAA records for ${DOMAIN} (prevents duplicate-record errors)..."
CF_RECORDS_JSON="$(curl -fsS -H "Authorization: Bearer $CLOUDFLARE_API_TOKEN" \
  "https://api.cloudflare.com/client/v4/zones/${CLOUDFLARE_ZONE_ID}/dns_records?name=${DOMAIN}&per_page=100")"

while IFS=$'\t' read -r rec_key rec_id; do
  [[ -z "$rec_key" ]] && continue
  addr="cloudflare_dns_record.mapping[\"${rec_key}\"]"
  if ! tofu -chdir=terraform/gcp state show "$addr" >/dev/null 2>&1; then
    echo "    Importing $addr ($rec_id)"
    tofu -chdir=terraform/gcp import "$addr" "${CLOUDFLARE_ZONE_ID}/${rec_id}"
  fi
done < <(python3 - "$CF_RECORDS_JSON" <<'PY'
import sys, json
data = json.loads(sys.argv[1])
assert data.get("success"), data
for r in data.get("result", []):
    if r["type"] in ("A", "AAAA"):
        print(f'{r["type"]}/{r["content"]}\t{r["id"]}')
PY
)

PLAN_FILE="terraform/gcp/cloudflare-edge.tfplan"
trap 'rm -f "$ROOT/$PLAN_FILE"' EXIT

echo "==> Generating OpenTofu plan (mode=$MODE, enable_anubis=$ENABLE_ANUBIS, bot_detection=$BOT_DETECTION)..."
tofu -chdir=terraform/gcp plan -out="$(basename "$PLAN_FILE")"

if [[ "$PLAN_ONLY" == "true" ]]; then
  echo "Plan-only mode complete; nothing was applied."
  exit 0
fi

read -rp "Apply this plan to live GCP + Cloudflare? [y/N] " CONFIRM
if [[ ! "$CONFIRM" =~ ^[Yy]$ ]]; then
  echo "Aborted."
  exit 0
fi

tofu -chdir=terraform/gcp apply "$(basename "$PLAN_FILE")"

echo "==> Smoke-testing https://${DOMAIN} through Cloudflare..."
curl -sI -A "Mozilla/5.0 Chrome/125.0" "https://${DOMAIN}/" \
  | grep -iE "^(HTTP/|cf-ray:|cf-cache-status:|server:|cache-control:)" || true

echo
echo "Done! To toggle bot detection or Anubis later:"
echo "  direnv exec . ./scripts/setup-cloudflare-edge.sh --mode cloudflare-only --bot-detection off"
echo "  direnv exec . ./scripts/setup-cloudflare-edge.sh --mode cloudflare-only --bot-detection on"
echo "  direnv exec . ./scripts/setup-cloudflare-edge.sh --mode both --bot-detection on"
