output "service_url" {
  description = "Public URL of the Cloud Run service."
  value       = module.site.service_url
}

output "registry_repository_url" {
  description = "Registry path to push the SSR image to."
  value       = module.site.registry_repository_url
}

output "dns_records" {
  description = "Records the custom domain mapping requires."
  value       = module.site.dns_records
}
