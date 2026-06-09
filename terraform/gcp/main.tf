terraform {
  required_version = ">= 1.6"

  required_providers {
    google = {
      source  = "hashicorp/google"
      version = ">= 5.0"
    }
    cloudflare = {
      source  = "cloudflare/cloudflare"
      version = "~> 5.0"
    }
    doppler = {
      source  = "DopplerHQ/doppler"
      version = "~> 1.0"
    }
    grafana = {
      source  = "grafana/grafana"
      version = "~> 3.0"
    }
  }
}

provider "google" {
  project = var.project_id
  region  = var.region
}

# DNS for the custom domain lives in Cloudflare. The token only needs
# Zone:DNS:Edit on the dabney.moe zone. When enable_cloudflare_dns = false the
# token may be empty (no Cloudflare resources are created).
provider "cloudflare" {
  api_token = var.cloudflare_api_token
}

# Doppler is the system of record for the deploy/runtime secrets. Authenticated
# with a personal or service token (var.doppler_token); empty disables all
# Doppler-managed resources. The bootstrap secret values are injected once via
# TF_VAR_* (kept out of VCS); thereafter they can be read back into Terraform
# with `doppler run --name-transformer tf-var -- tofu apply`.
provider "doppler" {
  doppler_token = var.doppler_token
}

locals {
  # Runtime env for the SSR server. LEPTOS_SITE_ROOT is intentionally NOT set
  # here: the Nix image already points it at the in-store asset bundle. We only
  # pin the values that should be visible/overridable at the infra layer.
  leptos_env = merge({
    LEPTOS_OUTPUT_NAME  = "dabney"
    LEPTOS_SITE_PKG_DIR = "pkg"
    LEPTOS_SITE_ADDR    = "0.0.0.0:8080"
    LEPTOS_ENV          = "PROD"
    RUST_LOG            = "info"
    }, var.ga4_measurement_id != "" ? {
    GA4_MEASUREMENT_ID = var.ga4_measurement_id
  } : {})
}

module "site" {
  source = "../modules/gcp-cloudrun"

  project_id    = var.project_id
  region        = var.region
  image         = var.image
  env           = local.leptos_env
  secret_env    = local.cloudrun_secret_env
  service_account_email = length(google_service_account.web_runtime) > 0 ? google_service_account.web_runtime[0].email : ""
  custom_domain = var.custom_domain
  # DNS is managed in Cloudflare below, not Cloud DNS, so the module's own
  # Cloud DNS zone stays off regardless of var.manage_dns intent.
  manage_dns = false
  dns_root   = var.dns_root

  # Cloud Run references Secret Manager versions; wait until Loki values exist.
  depends_on = [
    google_secret_manager_secret_version.loki_url,
    google_secret_manager_secret_version.loki_user,
    google_secret_manager_secret_version.loki_token,
  ]
}

# --- Cloudflare DNS for the custom domain ------------------------------------
# dabney.moe is hosted on Cloudflare. We mirror the records the Cloud Run domain
# mapping reports into the Cloudflare zone. Records stay DNS-only (proxied =
# false): Google must terminate TLS to provision its managed certificate, and a
# proxied (orange-cloud) record intercepts the ACME challenge and wedges
# provisioning. They can be flipped to proxied (with SSL/TLS = Full (strict))
# once the certificate is active.

locals {
  # One Cloudflare record per rrdata. Cloud Run reports apex records with an
  # empty name; Cloudflare uses "@" for the zone apex. Trailing dots are
  # stripped from rrdata so CNAME targets validate.
  cf_mapping_records = var.enable_cloudflare_dns ? {
    for r in module.site.dns_records :
    "${r.type}/${r.rrdata}" => {
      name    = r.name == "" ? "@" : r.name
      type    = r.type
      content = trimsuffix(r.rrdata, ".")
    }
  } : {}
}

resource "cloudflare_dns_record" "mapping" {
  for_each = local.cf_mapping_records

  zone_id = var.cloudflare_zone_id
  name    = each.value.name
  type    = each.value.type
  content = each.value.content
  ttl     = 300
  proxied = false
  comment = "dabney.moe Cloud Run domain mapping"
}

# Google Search Console domain-ownership TXT. Required before the Cloud Run
# domain mapping will succeed; create it, then click Verify in Search Console.
resource "cloudflare_dns_record" "verification" {
  count = var.enable_cloudflare_dns && var.google_site_verification != "" ? 1 : 0

  zone_id = var.cloudflare_zone_id
  name    = "@"
  type    = "TXT"
  content = "google-site-verification=${var.google_site_verification}"
  ttl     = 300
  proxied = false
  comment = "Google Search Console verification for dabney.moe"
}

# --- Doppler-managed secrets -------------------------------------------------
# Terraform owns the Doppler project + its prd config and writes the deploy
# secrets into it, so Doppler becomes the single source of truth. The values are
# seeded from input variables on the first apply; later runs can pull them back
# (`doppler run --name-transformer tf-var -- tofu apply`) so the same vars are
# populated from Doppler instead of the environment.

locals {
  # nonsensitive() unwraps only the presence check (token set or not), so the
  # values derived from it (counts, for_each maps) aren't tainted as sensitive.
  doppler_enabled = nonsensitive(var.doppler_token != "")

  # Non-sensitive secrets, skipping empties. These are plain strings, so the map
  # is safe to use with for_each. The sensitive Cloudflare token is handled in
  # its own resource below (a sensitive value taints for_each / instance keys).
  # Observability-derived secrets (LOKI_URL, etc.) live in observability.tf as
  # individual resources: their values are unknown until the Grafana stack
  # exists, which breaks for_each on this map.
  doppler_plain_secrets = local.doppler_enabled ? {
    for k, v in merge({
      CLOUDFLARE_ZONE_ID       = var.cloudflare_zone_id
      GCP_PROJECT_ID           = var.project_id
      GCP_REGION               = var.region
      GOOGLE_SITE_VERIFICATION = var.google_site_verification
      }, var.ga4_measurement_id != "" ? {
      GA4_MEASUREMENT_ID = var.ga4_measurement_id
    } : {}, var.ga4_property_id != "" ? {
      GA4_PROPERTY_ID = var.ga4_property_id
    } : {}) : k => v if v != ""
  } : {}
}

resource "doppler_project" "this" {
  count = local.doppler_enabled ? 1 : 0

  name        = var.doppler_project
  description = "dabney.moe deploy + runtime secrets (managed by Terraform)."
}

resource "doppler_secret" "plain" {
  for_each = local.doppler_plain_secrets

  project = doppler_project.this[0].name
  config  = var.doppler_config
  name    = each.key
  value   = each.value
}

# The Cloudflare token is sensitive, so it gets its own resource (a sensitive
# value can't drive count/for_each keys). nonsensitive() unwraps only the
# presence check, never the value.
resource "doppler_secret" "cloudflare_api_token" {
  count = local.doppler_enabled && nonsensitive(var.cloudflare_api_token != "") ? 1 : 0

  project = doppler_project.this[0].name
  config  = var.doppler_config
  name    = "CLOUDFLARE_API_TOKEN"
  value   = var.cloudflare_api_token
}
