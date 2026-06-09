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

output "cloudflare_dns_records" {
  description = "Cloudflare records created for the custom domain mapping."
  value = [
    for r in cloudflare_dns_record.mapping : {
      name    = r.name
      type    = r.type
      content = r.content
    }
  ]
}

output "doppler_secret_names" {
  description = "Secret names Terraform persists in the Doppler config."
  value = sort(concat(
    keys(doppler_secret.plain),
    [for r in doppler_secret.cloudflare_api_token : r.name],
    [for r in doppler_secret.gcp_sa_key : r.name],
    [for r in doppler_secret.loki_url : r.name],
    [for r in doppler_secret.loki_user : r.name],
    [for r in doppler_secret.grafana_stack_url : r.name],
    [for r in doppler_secret.loki_token : r.name],
    [for r in doppler_secret.grafana_sa_token : r.name],
  ))
}

output "grafana_stack_url" {
  description = "Grafana Cloud stack URL (EU). Empty when observability is disabled."
  value       = try(local.grafana_stack_url, "")
}

output "loki_push_url" {
  description = "Loki push endpoint for the Cloud Run service."
  value       = local.loki_push_url
  sensitive   = true
}

output "doppler_ci_token" {
  description = "Read-only Doppler service token for CI (set as the GitHub DOPPLER_TOKEN secret)."
  value       = one(doppler_service_token.ci[*].key)
  sensitive   = true
}

output "ci_service_account_email" {
  description = "Email of the GitHub Actions CI deployer service account."
  value       = one(google_service_account.ci[*].email)
}
