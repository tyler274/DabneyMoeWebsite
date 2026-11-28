variable "project_id" {
  type        = string
  description = "GCP project ID."
}

variable "region" {
  type        = string
  description = "GCP region."
  default     = "us-central1"
}

variable "max_instances" {
  type        = number
  description = "Maximum Cloud Run instances (caps worst-case compute spend; 1 instance handles 80 concurrent requests)."
  default     = 1
}

variable "billing_account_id" {
  type        = string
  description = "Optional GCP billing account ID (e.g. 01A29E-334D38-0B8604) to create a monthly budget alert. Empty skips budget creation."
  default     = ""
}

variable "monthly_budget_usd" {
  type        = number
  description = "Monthly GCP spend alert threshold in USD when billing_account_id is set."
  default     = 5
}

variable "image" {
  type        = string
  description = "SSR image reference incl. tag. May be empty for the phase-1 (registry-only) apply."
  default     = ""
}

variable "custom_domain" {
  type        = string
  description = "Custom domain to map (e.g. dabney.moe). Empty disables it."
  default     = "dabney.moe"
}

variable "manage_dns" {
  type        = bool
  description = "Create a Cloud DNS zone + records for the custom domain."
  default     = false
}

variable "dns_root" {
  type        = string
  description = "Apex DNS name with trailing dot (e.g. dabney.moe.), used when manage_dns = true."
  default     = ""
}

# --- Cloudflare DNS ----------------------------------------------------------

variable "enable_cloudflare_dns" {
  type        = bool
  description = "Create the Cloud Run domain-mapping records (and optional verification TXT) in Cloudflare."
  default     = true
}

variable "cloudflare_proxied" {
  type        = bool
  description = "Orange-cloud proxy dabney.moe DNS records through Cloudflare (free DDoS, CDN caching, edge rate limit). Only enable after Cloud Run's Google-managed TLS cert is active; also configures Anubis to read CF-Connecting-IP."
  default     = false
}

variable "enable_cloudflare_edge_protection" {
  type        = bool
  description = "Manage Cloudflare Free-tier SSL (Full strict), ACME-safe HTTPS redirect, Anubis-safe cache rules, Custom WAF, edge rate limit, and Bot Fight Mode via Terraform."
  default     = false
}

variable "enable_cloudflare_bot_detection" {
  type        = bool
  description = "Enable Cloudflare edge bot and automated-client detection (Browser Integrity Check, Bot Fight Mode, and Custom WAF rules challenging curl/headless browsers and cross-site API probes). Set false during development so automated tools can reach dabney.moe while keeping TLS, caching, rate limits, and exploit-scanner blocks active."
  default     = false
}

variable "cloudflare_api_token" {
  type        = string
  description = "Cloudflare API token with Zone:DNS:Edit on the dabney.moe zone. May be empty when enable_cloudflare_dns = false."
  default     = ""
  sensitive   = true
}

variable "cloudflare_zone_id" {
  type        = string
  description = "Cloudflare zone ID for dabney.moe. Required when enable_cloudflare_dns = true."
  default     = ""
}

variable "google_site_verification" {
  type        = string
  description = "Google Search Console site-verification token (the value after 'google-site-verification='). Empty skips the TXT record."
  default     = ""
}

# --- Doppler (secrets management) --------------------------------------------

variable "doppler_token" {
  type        = string
  description = "Doppler personal or service token. Empty disables all Doppler-managed resources."
  default     = ""
  sensitive   = true
}

variable "doppler_project" {
  type        = string
  description = "Doppler project name Terraform manages."
  default     = "dabney-moe-website"
}

variable "doppler_config" {
  type        = string
  description = "Doppler config (root config name = environment slug) the secrets are written to."
  default     = "prd"
}

# --- Observability (Grafana Cloud + GA4) -------------------------------------

variable "grafana_cloud_access_policy_token" {
  type        = string
  description = "Grafana Cloud org access policy token (org realm) with stacks:read|write|delete, accesspolicies:read|write|delete, and stack-service-accounts:write. Empty disables Grafana/Loki resources."
  default     = ""
  sensitive   = true
}

variable "grafana_cloud_existing_stack_slug" {
  type        = string
  description = "Adopt an existing Grafana Cloud stack by slug instead of creating dabneymoe. Required on free/trial plans (one stack per org). Set via GRAFANA_CLOUD_EXISTING_STACK_SLUG in Doppler."
  default     = ""
}

variable "grafana_cloud_region" {
  type        = string
  description = "Fallback Grafana Cloud API region when observability is disabled. Active stacks derive region from cluster_slug / region_slug automatically."
  default     = "prod-eu-west-0"
}

variable "ga4_measurement_id" {
  type        = string
  description = "Google Analytics 4 measurement ID (G-XXXXXXXX). Empty skips client analytics env injection."
  default     = ""
}

variable "ga4_property_id" {
  type        = string
  description = "GA4 property numeric ID, used for BigQuery export dataset naming (analytics_PROPERTY_ID)."
  default     = ""
}

variable "attach_existing_loki_secrets" {
  type        = bool
  description = "Attach pre-existing loki-url/loki-user/loki-token Secret Manager secrets to Cloud Run even when grafana_cloud_access_policy_token is omitted."
  default     = false
}

variable "clinical_api_url" {
  type        = string
  description = "Optional HTTPS origin of an external clinical workspace API proxied under /clinical/api. Empty uses the embedded in-process synthetic clinical workspace API."
  default     = ""
}

# --- Anubis (PoW bot protection fallback) ------------------------------------

variable "enable_anubis" {
  type        = bool
  description = "Optional Anubis proof-of-work ingress sidecar on Cloud Run (kept as a one-flag fallback behind Cloudflare Free edge protection)."
  default     = false
}

variable "anubis_version" {
  type        = string
  description = "Anubis tag mirrored into this project's Artifact Registry repo (Cloud Run does not allow mixing ghcr.io and pkg.dev images in one multi-container service)."
  default     = "v1.28.2"
}

variable "anubis_image" {
  type        = string
  description = "Optional full Artifact Registry image reference for Anubis. Empty defaults to <region>-docker.pkg.dev/<project_id>/dabney/anubis:<anubis_version>."
  default     = ""
}

variable "anubis_cpu" {
  type        = string
  description = "CPU limit for the Anubis ingress container (web gets 750m when Anubis is enabled so total instance CPU stays 1 vCPU)."
  default     = "250m"
}

variable "anubis_memory" {
  type        = string
  description = "Memory limit for the Anubis ingress container."
  default     = "256Mi"
}

variable "anubis_difficulty" {
  type        = number
  description = "Default SHA-256 proof-of-work difficulty for Anubis challenges."
  default     = 4
}

variable "anubis_webmaster_email" {
  type        = string
  description = "Contact email shown on Anubis reject pages for human appeals. Empty omits it."
  default     = "tp@dabney.moe"
}
