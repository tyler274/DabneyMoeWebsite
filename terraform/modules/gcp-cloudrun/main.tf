locals {
  required_services = [
    "run.googleapis.com",
    "artifactregistry.googleapis.com",
    "dns.googleapis.com",
    # Required by gcloud when operating with a service account (e.g. CI).
    # Without it gcloud emits a warning on every `auth activate-service-account`
    # call and cannot validate project membership.
    "cloudresourcemanager.googleapis.com",
  ]
}

resource "google_project_service" "services" {
  for_each = var.enable_apis ? toset(local.required_services) : toset([])

  project            = var.project_id
  service            = each.value
  disable_on_destroy = false
}

# Registry the SSR image is pushed to before the service is (re)deployed.
# Cleanup policies keep storage inside GCP Artifact Registry's 0.5 GiB free tier.
resource "google_artifact_registry_repository" "repo" {
  project       = var.project_id
  location      = var.region
  repository_id = var.repository_id
  format        = "DOCKER"
  description   = "dabney.moe web server images"

  cleanup_policies {
    id     = "keep-anubis-tags"
    action = "KEEP"
    condition {
      tag_state             = "TAGGED"
      package_name_prefixes = ["anubis"]
    }
  }

  cleanup_policies {
    id     = "keep-recent-web"
    action = "KEEP"
    most_recent_versions {
      keep_count            = 2
      package_name_prefixes = ["web"]
    }
  }

  cleanup_policies {
    id     = "delete-old-images"
    action = "DELETE"
    condition {
      tag_state  = "ANY"
      older_than = "3600s" # 1 hour (keeps 2 newest web + tagged anubis under 0.5 GiB free tier)
    }
  }

  depends_on = [google_project_service.services]
}

resource "google_cloud_run_v2_service" "web" {
  project              = var.project_id
  name                 = var.service_name
  location             = var.region
  ingress              = "INGRESS_TRAFFIC_ALL"
  default_uri_disabled = var.custom_domain != ""
  deletion_protection  = false

  template {
    timeout                          = "60s"
    max_instance_request_concurrency = 80

    scaling {
      min_instance_count = var.min_instances
      max_instance_count = var.max_instances
    }

    service_account = var.service_account_email != "" ? var.service_account_email : null

    # When Anubis runs with an in-memory challenge store across multiple
    # instances, session affinity keeps the short GET -> pass-challenge
    # handshake on the same instance; post-challenge Ed25519 cookies verify
    # statelessly on any instance.
    session_affinity = var.anubis != null ? true : null

    # Gen2 runs each instance inside gVisor, a user-space kernel that
    # intercepts and validates every syscall before forwarding it to the host.
    # This is the Cloud Run equivalent of AppArmor/SELinux: you can't load
    # kernel policies on the managed host, but gVisor's syscall filter is
    # always active and covers the same threat (process escape via kernel).
    # Gen2 is also required for Cloud Run multi-container sidecars.
    execution_environment = "EXECUTION_ENVIRONMENT_GEN2"

    dynamic "volumes" {
      for_each = var.anubis != null ? [var.anubis] : []
      content {
        name = "anubis-policy"
        secret {
          secret       = volumes.value.policy_secret_id
          default_mode = 292 # 0444: readable by Anubis container UID 1000
          items {
            version = volumes.value.policy_secret_version
            path    = "botPolicies.yaml"
          }
        }
      }
    }

    # When Anubis is enabled, it MUST be container 0 (the ingress container with
    # `ports`) and `web` is container 1 (sidecar with no `ports`). The Google
    # provider treats `ports` as Optional+Computed on an existing container 0, so
    # keeping the ingress container at index 0 ensures exactly one container
    # exposes a port.
    dynamic "containers" {
      for_each = var.anubis != null ? [var.anubis] : []
      content {
        name       = "anubis"
        image      = containers.value.image
        depends_on = ["web"]

        ports {
          container_port = var.container_port
        }

        resources {
          limits = {
            cpu    = containers.value.cpu
            memory = containers.value.memory
          }
          cpu_idle          = true
          startup_cpu_boost = false
        }

        startup_probe {
          initial_delay_seconds = 1
          timeout_seconds       = 2
          period_seconds        = 3
          failure_threshold     = 10
          tcp_socket {
            port = var.container_port
          }
        }

        volume_mounts {
          name       = "anubis-policy"
          mount_path = "/etc/anubis"
        }

        env {
          name  = "BIND"
          value = ":${var.container_port}"
        }

        env {
          name  = "METRICS_BIND"
          value = "127.0.0.1:9090"
        }

        env {
          name  = "TARGET"
          value = "http://127.0.0.1:${containers.value.app_port}"
        }

        env {
          name  = "DIFFICULTY"
          value = tostring(containers.value.difficulty)
        }

        env {
          name  = "POLICY_FNAME"
          value = "/etc/anubis/botPolicies.yaml"
        }

        env {
          name  = "OG_PASSTHROUGH"
          value = "true"
        }

        env {
          name  = "COOKIE_SECURE"
          value = "true"
        }

        env {
          name  = "SLOG_LEVEL"
          value = "INFO"
        }

        dynamic "env" {
          for_each = containers.value.redirect_domains != "" ? [containers.value.redirect_domains] : []
          content {
            name  = "REDIRECT_DOMAINS"
            value = env.value
          }
        }

        dynamic "env" {
          for_each = containers.value.webmaster_email != "" ? [containers.value.webmaster_email] : []
          content {
            name  = "WEBMASTER_EMAIL"
            value = env.value
          }
        }

        dynamic "env" {
          for_each = containers.value.custom_real_ip_header != "" ? [containers.value.custom_real_ip_header] : []
          content {
            name  = "CUSTOM_REAL_IP_HEADER"
            value = env.value
          }
        }

        env {
          name = "ED25519_PRIVATE_KEY_HEX"
          value_source {
            secret_key_ref {
              secret  = containers.value.signing_key_secret_id
              version = containers.value.signing_key_version
            }
          }
        }
      }
    }

    containers {
      name  = "web"
      image = var.image

      dynamic "ports" {
        for_each = var.anubis == null ? [1] : []
        content {
          container_port = var.container_port
        }
      }

      resources {
        limits = {
          cpu    = var.cpu
          memory = var.memory
        }
        cpu_idle          = true
        startup_cpu_boost = false
      }

      dynamic "startup_probe" {
        for_each = var.anubis != null ? [var.anubis] : []
        content {
          initial_delay_seconds = 1
          timeout_seconds       = 2
          period_seconds        = 3
          failure_threshold     = 10
          tcp_socket {
            port = startup_probe.value.app_port
          }
        }
      }

      dynamic "env" {
        for_each = var.env
        content {
          name  = env.key
          value = env.value
        }
      }

      dynamic "env" {
        for_each = var.secret_env
        content {
          name = env.key
          value_source {
            secret_key_ref {
              secret  = env.value.secret_id
              version = env.value.version
            }
          }
        }
      }
    }
  }

  depends_on = [google_project_service.services]

  # Terraform provisions the service with the initial image; subsequent `web`
  # image rollouts are owned by CI/CD (`gcloud run services update --container=web`).
  # Ignore `web` container image drift (index 1 when Anubis is container 0) so
  # `tofu apply` won't revert CI's latest `web` revision while still allowing
  # Terraform to manage the `anubis` image at index 0.
  lifecycle {
    ignore_changes = [template[0].containers[1].image]
  }
}

# Public access. Cloud Run is private by default; this opens it to the world.
resource "google_cloud_run_v2_service_iam_member" "public" {
  count = var.allow_unauthenticated ? 1 : 0

  project  = var.project_id
  location = google_cloud_run_v2_service.web.location
  name     = google_cloud_run_v2_service.web.name
  role     = "roles/run.invoker"
  member   = "allUsers"
}

# --- Optional custom domain + DNS -------------------------------------------

resource "google_cloud_run_domain_mapping" "domain" {
  count = var.custom_domain != "" ? 1 : 0

  project  = var.project_id
  location = var.region
  name     = var.custom_domain

  metadata {
    namespace = var.project_id
  }

  spec {
    route_name = google_cloud_run_v2_service.web.name
  }

  lifecycle {
    ignore_changes = [
      metadata[0].labels,
    ]
  }
}

resource "google_dns_managed_zone" "zone" {
  count = var.manage_dns ? 1 : 0

  project  = var.project_id
  name     = var.dns_zone_name
  dns_name = var.dns_root

  depends_on = [google_project_service.services]
}

# The records Cloud Run requires for the mapping (A/AAAA for apex, CNAME for
# subdomains) are reported by the domain mapping status; mirror them into the
# managed zone. Grouped by (name, type) because Cloud DNS rrsets are keyed that
# way while Cloud Run reports one record per rrdata.
locals {
  mapping_records = var.custom_domain != "" ? try(google_cloud_run_domain_mapping.domain[0].status[0].resource_records, []) : []

  grouped_records = {
    for key, recs in {
      for r in local.mapping_records : "${r.name}/${r.type}" => r...
    } : key => {
      name   = recs[0].name == "" ? var.dns_root : "${recs[0].name}.${var.dns_root}"
      type   = recs[0].type
      rrdatas = [for r in recs : r.rrdata]
    }
  }
}

resource "google_dns_record_set" "mapping" {
  for_each = var.manage_dns ? local.grouped_records : {}

  project      = var.project_id
  managed_zone = google_dns_managed_zone.zone[0].name
  name         = each.value.name
  type         = each.value.type
  ttl          = 300
  rrdatas      = each.value.rrdatas
}
