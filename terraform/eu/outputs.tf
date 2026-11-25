output "server_ipv4" {
  description = "Tier 1 VPS IPv4 (feed to coda-rust infra/eu server_ipv4)."
  value       = var.enable_vps ? hcloud_server.app[0].ipv4_address : null
}

output "server_ipv6" {
  description = "Tier 1 VPS IPv6 (feed to coda-rust infra/eu server_ipv6)."
  value       = var.enable_vps ? hcloud_server.app[0].ipv6_address : null
}

output "nameservers" {
  description = "Delegate the domain to these at the registrar."
  value       = ["ns1.desec.io", "ns2.desec.org"]
}

output "dnssec_ds" {
  description = "DS records to enter at the registrar to complete the DNSSEC chain."
  value       = flatten([for key in desec_domain.zone.keys : key.ds])
}

output "vps_hostnames" {
  description = "Names that resolve to the VPS."
  value       = sort([for host in local.vps_hosts : host == "" ? var.domain : "${host}.${var.domain}"])
}

output "registry_endpoint" {
  description = "Container Registry endpoint (rg.<region>.scw.cloud/<namespace>) for scripts/deploy.sh --target scaleway."
  value       = var.enable_registry ? scaleway_registry_namespace.images[0].endpoint : null
}

output "serverless_container_id" {
  description = "Serverless Container ID for scripts/deploy.sh --target scaleway (region-prefixed)."
  value       = local.serverless ? scaleway_container.web[0].id : null
}

output "serverless_url" {
  description = "Default HTTPS URL of the Serverless Container."
  value       = local.serverless ? "https://${local.serverless_hostname}" : null
}
