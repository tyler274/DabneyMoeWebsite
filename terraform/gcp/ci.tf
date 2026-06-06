# CI deployer: a GCP service account GitHub Actions uses to push images and roll
# new Cloud Run revisions. Its key is stored in Doppler (GCP_SA_KEY) rather than
# directly in GitHub; CI authenticates to Doppler with a single read-only service
# token (output below) held as the GitHub `DOPPLER_TOKEN` secret. Everything here
# is gated on Doppler being enabled, since that's where the key lands.

resource "google_service_account" "ci" {
  count = local.doppler_enabled ? 1 : 0

  project      = var.project_id
  account_id   = "ci-deployer"
  display_name = "GitHub Actions CI deployer"
}

# Roles the deploy job needs: push to Artifact Registry, deploy Cloud Run
# revisions, and act as the service's runtime service account during deploy.
resource "google_project_iam_member" "ci" {
  for_each = local.doppler_enabled ? toset([
    "roles/artifactregistry.writer",
    "roles/run.admin",
    "roles/iam.serviceAccountUser",
  ]) : toset([])

  project = var.project_id
  role    = each.value
  member  = "serviceAccount:${google_service_account.ci[0].email}"
}

resource "google_service_account_key" "ci" {
  count = local.doppler_enabled ? 1 : 0

  service_account_id = google_service_account.ci[0].name
}

# The service-account JSON key, stored in Doppler for CI to consume as
# google-github-actions/auth-style credentials. private_key is base64-encoded
# JSON; decode it so the stored value is the raw key file.
resource "doppler_secret" "gcp_sa_key" {
  count = local.doppler_enabled ? 1 : 0

  project = doppler_project.this[0].name
  config  = var.doppler_config
  name    = "GCP_SA_KEY"
  value   = base64decode(google_service_account_key.ci[0].private_key)
}

# Read-only Doppler service token scoped to the prd config. Its key becomes the
# GitHub `DOPPLER_TOKEN` secret so CI can pull GCP_SA_KEY / GCP_PROJECT_ID /
# GCP_REGION at runtime.
resource "doppler_service_token" "ci" {
  count = local.doppler_enabled ? 1 : 0

  project = doppler_project.this[0].name
  config  = var.doppler_config
  name    = "github-actions-ci"
  access  = "read"
}
