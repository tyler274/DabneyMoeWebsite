output "service_url" {
  description = "Public HTTPS URL of the Container App."
  value       = "https://${azurerm_container_app.web.ingress[0].fqdn}"
}

output "registry_repository_url" {
  description = "ACR login server to push the SSR image to (append /<image>:<tag>)."
  value       = azurerm_container_registry.acr.login_server
}

output "custom_domain_verification_id" {
  description = "Value to publish as a TXT record (asuid.<domain>) to validate the custom domain before binding a certificate."
  value       = azurerm_container_app.web.custom_domain_verification_id
}
