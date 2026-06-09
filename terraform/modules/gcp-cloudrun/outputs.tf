output "service_url" {
  description = "Public HTTPS URL assigned to the Cloud Run service."
  value       = google_cloud_run_v2_service.web.uri
}

output "registry_repository_url" {
  description = "Docker registry path to push the SSR image to (append /<image>:<tag>)."
  value       = "${var.region}-docker.pkg.dev/${var.project_id}/${google_artifact_registry_repository.repo.repository_id}"
}

output "dns_records" {
  description = "Records the custom domain mapping requires (add these at your registrar if manage_dns is false)."
  value       = local.mapping_records
}

output "service_name" {
  description = "Cloud Run service name."
  value       = google_cloud_run_v2_service.web.name
}
