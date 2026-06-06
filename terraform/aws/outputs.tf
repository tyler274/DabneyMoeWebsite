output "service_url" {
  description = "Default URL of the App Runner service."
  value       = module.site.service_url
}

output "registry_repository_url" {
  description = "ECR repository URL to push the SSR image to."
  value       = module.site.registry_repository_url
}

output "dns_records" {
  description = "ACM validation records the custom domain association requires."
  value       = module.site.dns_records
}
