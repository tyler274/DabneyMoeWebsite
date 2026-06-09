#!/usr/bin/env bash
# Apply terraform/gcp using Doppler prd secrets + local gcloud auth.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PROJECT="${DOPPLER_PROJECT:-dabney-moe-website}"
CONFIG="${DOPPLER_CONFIG:-prd}"

cd "$ROOT"

if ! gcloud auth application-default print-access-token &>/dev/null; then
  echo "error: gcloud application-default credentials expired or missing." >&2
  echo "Run: gcloud auth login && gcloud auth application-default login" >&2
  exit 1
fi

# Doppler provider needs a token that can write secrets (personal CLI token, not CI read-only).
export TF_VAR_doppler_token="$(doppler configure get token --plain)"

# These Doppler names don't map via --name-transformer tf-var (GCP_* vs project_id/region).
export TF_VAR_project_id="$(doppler secrets get GCP_PROJECT_ID --project "$PROJECT" --config "$CONFIG" --plain)"
export TF_VAR_region="$(doppler secrets get GCP_REGION --project "$PROJECT" --config "$CONFIG" --plain)"

# Cloud Run domain mapping + Cloudflare DNS (defaults in variables.tf; override via Doppler).
if doppler secrets get CUSTOM_DOMAIN --project "$PROJECT" --config "$CONFIG" --plain &>/dev/null; then
  export TF_VAR_custom_domain="$(doppler secrets get CUSTOM_DOMAIN --project "$PROJECT" --config "$CONFIG" --plain)"
fi
export TF_VAR_enable_cloudflare_dns=true

# CLOUDFLARE_API_TOKEN, CLOUDFLARE_ZONE_ID, GRAFANA_*, GA4_* → TF_VAR_* via tf-var.
exec doppler run --name-transformer tf-var --project "$PROJECT" --config "$CONFIG" -- \
  tofu -chdir=terraform/gcp "$@"
