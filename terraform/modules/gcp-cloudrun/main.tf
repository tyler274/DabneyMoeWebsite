locals {
  required_services = [
    "run.googleapis.com",
    "artifactregistry.googleapis.com",
    "dns.googleapis.com",
  ]
}

resource "google_project_service" "services" {
  for_each = var.enable_apis ? toset(local.required_services) : toset([])

  project            = var.project_id
  service            = each.value
  disable_on_destroy = false
}

# Registry the SSR image is pushed to before the service is (re)deployed.
resource "google_artifact_registry_repository" "repo" {
  project       = var.project_id
  location      = var.region
  repository_id = var.repository_id
  format        = "DOCKER"
  description   = "dabney.moe web server images"

  depends_on = [google_project_service.services]
}

resource "google_cloud_run_v2_service" "web" {
  project             = var.project_id
  name                = var.service_name
  location            = var.region
  ingress             = "INGRESS_TRAFFIC_ALL"
  deletion_protection = false

  template {
    scaling {
      min_instance_count = var.min_instances
      max_instance_count = var.max_instances
    }

    # Gen2 runs each instance inside gVisor — a user-space kernel that
    # intercepts and validates every syscall before forwarding it to the host.
    # This is the Cloud Run equivalent of AppArmor/SELinux: you can't load
    # kernel policies on the managed host, but gVisor's syscall filter is
    # always active and covers the same threat (process escape via kernel).
    execution_environment = "EXECUTION_ENVIRONMENT_GEN2"

    containers {
      image = var.image

      ports {
        container_port = var.container_port
      }

      resources {
        limits = {
          cpu    = var.cpu
          memory = var.memory
        }
      }

      dynamic "env" {
        for_each = var.env
        content {
          name  = env.key
          value = env.value
        }
      }
    }
  }

  depends_on = [google_project_service.services]

  # Terraform provisions the service with the initial image; subsequent image
  # rollouts are owned by CI/CD (`gcloud run deploy`). Ignore image drift so the
  # two don't fight (a `tofu apply` won't revert CI's latest revision).
  lifecycle {
    ignore_changes = [template[0].containers[0].image]
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
