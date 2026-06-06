output "service_url" {
  description = "Public URL of the Container App."
  value       = module.site.service_url
}

output "registry_repository_url" {
  description = "ACR login server to push the SSR image to."
  value       = module.site.registry_repository_url
}

output "custom_domain_verification_id" {
  description = "TXT value (asuid.<domain>) used to validate the custom domain."
  value       = module.site.custom_domain_verification_id
}
