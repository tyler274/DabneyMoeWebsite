#!/usr/bin/env bash
# Create GA4 secrets in Doppler (prd) after setting up the property in Google Analytics.
set -euo pipefail

PROJECT="${DOPPLER_PROJECT:-dabney-moe-website}"
CONFIG="${DOPPLER_CONFIG:-prd}"

cat <<'EOF'
Create the GA4 property (one-time, in the browser):

  1. Open https://analytics.google.com/
  2. Admin (gear) → Create → Property → name: dabney.moe
  3. Data stream → Web → URL: https://dabney.moe
  4. Copy Measurement ID (G-XXXXXXXXXX) from the stream details
  5. Admin → Property settings → copy Property ID (numeric, top of page)

GDPR settings (same Admin area):

  - Data collection → disable Google signals / ads personalization
  - Data retention → 2 months

BigQuery link (after tofu apply creates the EU dataset):

  - Admin → Product links → BigQuery links → Link → project dabney-moe-website → EU

EOF

read -rp "GA4 Measurement ID (G-...): " MEASUREMENT_ID
read -rp "GA4 Property ID (numeric): " PROPERTY_ID

if [[ ! "$MEASUREMENT_ID" =~ ^G-[A-Z0-9]+$ ]]; then
  echo "error: measurement ID should look like G-XXXXXXXXXX" >&2
  exit 1
fi
if [[ ! "$PROPERTY_ID" =~ ^[0-9]+$ ]]; then
  echo "error: property ID should be numeric" >&2
  exit 1
fi

doppler secrets set \
  "GA4_MEASUREMENT_ID=$MEASUREMENT_ID" \
  "GA4_PROPERTY_ID=$PROPERTY_ID" \
  --project "$PROJECT" --config "$CONFIG"

echo "Set GA4_MEASUREMENT_ID and GA4_PROPERTY_ID in Doppler ($PROJECT / $CONFIG)."
echo "Run: ./scripts/tofu-apply.sh"
