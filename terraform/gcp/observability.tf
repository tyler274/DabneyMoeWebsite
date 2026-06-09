# Grafana Cloud (EU) + GCP Secret Manager wiring for Loki log push and BigQuery analytics.

locals {
  observability_enabled = nonsensitive(var.grafana_cloud_access_policy_token != "")
  adopt_grafana_stack   = local.observability_enabled && var.grafana_cloud_existing_stack_slug != ""
  create_grafana_stack  = local.observability_enabled && !local.adopt_grafana_stack

  grafana_stack_id = local.observability_enabled ? (
    local.adopt_grafana_stack ? data.grafana_cloud_stack.existing[0].id : grafana_cloud_stack.observability[0].id
  ) : ""

  grafana_stack_slug = local.observability_enabled ? (
    local.adopt_grafana_stack ? data.grafana_cloud_stack.existing[0].slug : grafana_cloud_stack.observability[0].slug
  ) : ""

  # Access policies are regional; must match the stack (not hardcoded EU when adopting a US stack).
  grafana_cloud_api_region = local.observability_enabled ? (
    local.adopt_grafana_stack ? data.grafana_cloud_stack.existing[0].region_slug : grafana_cloud_stack.observability[0].cluster_slug
  ) : var.grafana_cloud_region

  # tracing-loki appends /loki/api/v1/push to this base URL.
  loki_push_url = local.observability_enabled ? (
    local.adopt_grafana_stack ? data.grafana_cloud_stack.existing[0].logs_url : grafana_cloud_stack.observability[0].logs_url
  ) : ""

  grafana_stack_url = local.observability_enabled ? "https://${local.grafana_stack_slug}.grafana.net" : ""

  cloudrun_secret_env = local.observability_enabled ? {
    LOKI_URL   = { secret_id = google_secret_manager_secret.loki_url[0].secret_id }
    LOKI_USER  = { secret_id = google_secret_manager_secret.loki_user[0].secret_id }
    LOKI_TOKEN = { secret_id = google_secret_manager_secret.loki_token[0].secret_id }
  } : {}
}

provider "grafana" {
  alias                     = "cloud"
  cloud_access_policy_token = var.grafana_cloud_access_policy_token
}

# --- Grafana Cloud stack (EU) ------------------------------------------------

data "grafana_cloud_stack" "existing" {
  count = local.adopt_grafana_stack ? 1 : 0

  provider = grafana.cloud
  slug     = var.grafana_cloud_existing_stack_slug
}

resource "grafana_cloud_stack" "observability" {
  count = local.create_grafana_stack ? 1 : 0

  provider    = grafana.cloud
  name        = "dabneymoe"
  slug        = "dabneymoe"
  region_slug = "eu"
  description = "dabney.moe observability (Loki + Grafana dashboards)"
}

resource "grafana_cloud_access_policy" "loki_push" {
  count = local.observability_enabled ? 1 : 0

  provider     = grafana.cloud
  region       = local.grafana_cloud_api_region
  name         = "dabney-loki-push"
  display_name = "dabney.moe Loki push"
  scopes       = ["logs:write"]

  realm {
    type       = "stack"
    identifier = local.grafana_stack_id
  }
}

resource "grafana_cloud_access_policy_token" "loki_push" {
  count = local.observability_enabled ? 1 : 0

  provider         = grafana.cloud
  region           = local.grafana_cloud_api_region
  access_policy_id = grafana_cloud_access_policy.loki_push[0].policy_id
  name             = "dabney-loki-push"
  display_name     = "dabney.moe Cloud Run Loki push"
  # Grafana requires expiry ≤90 days; ignore_changes avoids replan churn on timestamp().
  expires_at = formatdate("YYYY-MM-DD'T'hh:mm:ss'Z'", timeadd(timestamp(), "2136h"))

  lifecycle {
    ignore_changes = [expires_at]
  }
}

resource "grafana_cloud_stack_service_account" "observability" {
  count = local.observability_enabled ? 1 : 0

  provider   = grafana.cloud
  stack_slug = local.grafana_stack_slug
  name       = "observability"
  role       = "Admin"
}

resource "grafana_cloud_stack_service_account_token" "observability" {
  count = local.observability_enabled ? 1 : 0

  provider           = grafana.cloud
  stack_slug         = local.grafana_stack_slug
  service_account_id = grafana_cloud_stack_service_account.observability[0].id
  name               = "terraform-observability"
}

# --- GCP APIs for Secret Manager + BigQuery ----------------------------------

resource "google_project_service" "observability" {
  for_each = local.observability_enabled || var.ga4_measurement_id != "" ? toset([
    "secretmanager.googleapis.com",
    "bigquery.googleapis.com",
  ]) : toset([])

  project            = var.project_id
  service            = each.value
  disable_on_destroy = false
}

# --- Cloud Run runtime service account -----------------------------------------

resource "google_service_account" "web_runtime" {
  count = local.observability_enabled ? 1 : 0

  project      = var.project_id
  account_id   = "web-runtime"
  display_name = "dabney-web Cloud Run runtime"
}

resource "google_project_iam_member" "web_runtime_secret_accessor" {
  count = local.observability_enabled ? 1 : 0

  project = var.project_id
  role    = "roles/secretmanager.secretAccessor"
  member  = "serviceAccount:${google_service_account.web_runtime[0].email}"
}

# --- Secret Manager (Loki credentials) ---------------------------------------

resource "google_secret_manager_secret" "loki_url" {
  count = local.observability_enabled ? 1 : 0

  project   = var.project_id
  secret_id = "loki-url"

  replication {
    auto {}
  }

  depends_on = [google_project_service.observability]
}

resource "google_secret_manager_secret_version" "loki_url" {
  count = local.observability_enabled ? 1 : 0

  secret      = google_secret_manager_secret.loki_url[0].id
  secret_data = local.loki_push_url
}

resource "google_secret_manager_secret" "loki_user" {
  count = local.observability_enabled ? 1 : 0

  project   = var.project_id
  secret_id = "loki-user"

  replication {
    auto {}
  }

  depends_on = [google_project_service.observability]
}

resource "google_secret_manager_secret_version" "loki_user" {
  count = local.observability_enabled ? 1 : 0

  secret      = google_secret_manager_secret.loki_user[0].id
  secret_data = local.observability_enabled ? tostring(
    local.adopt_grafana_stack ? data.grafana_cloud_stack.existing[0].logs_user_id : grafana_cloud_stack.observability[0].logs_user_id
  ) : ""
}

resource "google_secret_manager_secret" "loki_token" {
  count = local.observability_enabled ? 1 : 0

  project   = var.project_id
  secret_id = "loki-token"

  replication {
    auto {}
  }

  depends_on = [google_project_service.observability]
}

resource "google_secret_manager_secret_version" "loki_token" {
  count = local.observability_enabled ? 1 : 0

  secret      = google_secret_manager_secret.loki_token[0].id
  secret_data = grafana_cloud_access_policy_token.loki_push[0].token
}

# --- BigQuery (GA4 export destination / Grafana datasource) --------------------

resource "google_bigquery_dataset" "analytics" {
  count = var.ga4_measurement_id != "" ? 1 : 0

  project    = var.project_id
  dataset_id = "dabney_analytics"
  location   = "EU"

  description = "Placeholder for GA4 BigQuery exports and Grafana analytics panels."

  depends_on = [google_project_service.observability]
}

# --- Doppler: observability secrets (apply-time values; not for_each) ----------

resource "doppler_secret" "loki_url" {
  count = local.doppler_enabled && local.observability_enabled ? 1 : 0

  project = doppler_project.this[0].name
  config  = var.doppler_config
  name    = "LOKI_URL"
  value   = local.loki_push_url
}

resource "doppler_secret" "loki_user" {
  count = local.doppler_enabled && local.observability_enabled ? 1 : 0

  project = doppler_project.this[0].name
  config  = var.doppler_config
  name    = "LOKI_USER"
  value = local.observability_enabled ? tostring(
    local.adopt_grafana_stack ? data.grafana_cloud_stack.existing[0].logs_user_id : grafana_cloud_stack.observability[0].logs_user_id
  ) : ""
}

resource "doppler_secret" "grafana_stack_url" {
  count = local.doppler_enabled && local.observability_enabled ? 1 : 0

  project = doppler_project.this[0].name
  config  = var.doppler_config
  name    = "GRAFANA_STACK_URL"
  value   = local.grafana_stack_url
}

resource "doppler_secret" "loki_token" {
  count = local.doppler_enabled && local.observability_enabled ? 1 : 0

  project = doppler_project.this[0].name
  config  = var.doppler_config
  name    = "LOKI_TOKEN"
  value   = grafana_cloud_access_policy_token.loki_push[0].token
}

resource "doppler_secret" "grafana_sa_token" {
  count = local.doppler_enabled && local.observability_enabled ? 1 : 0

  project = doppler_project.this[0].name
  config  = var.doppler_config
  name    = "GRAFANA_CLOUD_SA_TOKEN"
  value   = grafana_cloud_stack_service_account_token.observability[0].key
}
