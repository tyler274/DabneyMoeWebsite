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
  ))
}
