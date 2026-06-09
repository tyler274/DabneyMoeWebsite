variable "project_id" {
  type        = string
  description = "GCP project ID."
}

variable "region" {
  type        = string
  description = "GCP region."
  default     = "us-central1"
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
