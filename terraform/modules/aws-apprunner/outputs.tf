output "service_url" {
  description = "Default HTTPS URL App Runner assigns to the service."
  value       = "https://${aws_apprunner_service.web.service_url}"
}

output "registry_repository_url" {
  description = "ECR repository URL to push the SSR image to (append :<tag>)."
  value       = aws_ecr_repository.repo.repository_url
}

output "dns_records" {
  description = "ACM validation records the custom domain association requires (add at your registrar if manage_dns is false)."
  value       = var.custom_domain != "" ? aws_apprunner_custom_domain_association.domain[0].certificate_validation_records : []
}
