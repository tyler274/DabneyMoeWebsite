# Anubis proof-of-work bot firewall resources (GCP Secret Manager).
#
# When var.enable_anubis is true, Cloud Run runs Anubis as the ingress
# container on :8080 and proxies verified traffic to dabney-web on
# 127.0.0.1:3039.
#
# - anubis-ed25519-key: persistent 32-byte hex Ed25519 signing key so
#   verification cookies remain valid across scale-to-zero and multiple
#   Cloud Run instances.
# - anubis-bot-policies: mounted into the Anubis container at
#   /etc/anubis/botPolicies.yaml from ../../anubis/botPolicies.yaml.

resource "random_id" "anubis_ed25519_key" {
  count = var.enable_anubis ? 1 : 0

  byte_length = 32
}

resource "google_secret_manager_secret" "anubis_signing_key" {
  count = var.enable_anubis ? 1 : 0

  project   = var.project_id
  secret_id = "anubis-ed25519-key"

  replication {
    auto {}
  }

  depends_on = [google_project_service.observability]
}

resource "google_secret_manager_secret_version" "anubis_signing_key" {
  count = var.enable_anubis ? 1 : 0

  secret      = google_secret_manager_secret.anubis_signing_key[0].id
  secret_data = random_id.anubis_ed25519_key[0].hex

  lifecycle {
    create_before_destroy = true
  }
}

resource "google_secret_manager_secret" "anubis_policy" {
  count = var.enable_anubis ? 1 : 0

  project   = var.project_id
  secret_id = "anubis-bot-policies"

  replication {
    auto {}
  }

  depends_on = [google_project_service.observability]
}

resource "google_secret_manager_secret_version" "anubis_policy" {
  count = var.enable_anubis ? 1 : 0

  secret      = google_secret_manager_secret.anubis_policy[0].id
  secret_data = file("${path.module}/../../anubis/botPolicies.yaml")

  lifecycle {
    create_before_destroy = true
  }
}

locals {
  anubis_app_port = 3039
  anubis_resolved_image = (
    var.anubis_image != ""
    ? var.anubis_image
    : "${var.region}-docker.pkg.dev/${var.project_id}/dabney/anubis:${var.anubis_version}"
  )

  cloudrun_anubis_config = var.enable_anubis ? {
    image                 = local.anubis_resolved_image
    cpu                   = var.anubis_cpu
    memory                = var.anubis_memory
    app_port              = local.anubis_app_port
    difficulty            = var.anubis_difficulty
    redirect_domains      = var.custom_domain
    webmaster_email       = var.anubis_webmaster_email
    custom_real_ip_header = var.cloudflare_proxied ? "CF-Connecting-IP" : ""
    signing_key_secret_id = google_secret_manager_secret.anubis_signing_key[0].secret_id
    signing_key_version   = google_secret_manager_secret_version.anubis_signing_key[0].version
    policy_secret_id      = google_secret_manager_secret.anubis_policy[0].secret_id
    policy_secret_version = google_secret_manager_secret_version.anubis_policy[0].version
  } : null
}
