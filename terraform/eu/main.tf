# dabney.moe on non-US providers:
#   deSEC (DE) owns the zone and signs it with DNSSEC.
#   Tier 1: one Hetzner Cloud (FI/DE) CX23 VPS running selfhost/eu (Caddy, the
#           SSR site, the forge, Radicle, Postgres, Valkey, Attic, Pocket ID).
#   Tier 0: a scale-to-zero Scaleway (FR) Serverless Container for dabney-web,
#           with images in Scaleway Container Registry.
# dabney-moe's infra/eu adds the Curie records (app, api, curie via git-pages,
# mail) to the zone this root creates; the host lists must stay disjoint.

locals {
  labels = {
    managed-by = "opentofu"
    root       = "dabney-moe-website-terraform-eu"
  }

  hosts      = distinct(concat(var.vps_hosts, var.curie_on_vps ? ["curie"] : []))
  vps_hosts  = var.enable_vps ? toset(local.hosts) : toset([])
  serverless = var.serverless_image != ""

  # public_endpoint is a URL; DNS wants the bare host name.
  serverless_hostname = local.serverless ? replace(scaleway_container.web[0].public_endpoint, "/^https?://|/$/", "") : ""

  open_ports = { for name, port in {
    forge-ssh = var.forge_ssh_port
    radicle   = var.radicle_port
  } : name => port if port != 0 }

  caa = concat(
    [for issuer in var.caa_issuers : "0 issue \"${issuer}\""],
    ["0 issuewild \";\""],
    var.caa_iodef == "" ? [] : ["0 iodef \"${var.caa_iodef}\""],
  )
}

# --- DNS zone ---------------------------------------------------------------

resource "desec_domain" "zone" {
  name = var.domain
}

resource "desec_rrset" "caa" {
  domain  = desec_domain.zone.name
  subname = ""
  type    = "CAA"
  ttl     = 3600
  rdata   = local.caa
}

# --- Tier 1: Hetzner VPS ----------------------------------------------------

resource "hcloud_ssh_key" "deploy" {
  count      = var.enable_vps ? 1 : 0
  name       = "dabney-deploy"
  public_key = var.ssh_public_key
  labels     = local.labels

  lifecycle {
    precondition {
      condition     = startswith(var.ssh_public_key, "ssh-") || startswith(var.ssh_public_key, "ecdsa-")
      error_message = "enable_vps needs ssh_public_key in OpenSSH format."
    }
  }
}

resource "hcloud_firewall" "web" {
  count  = var.enable_vps ? 1 : 0
  name   = "dabney-web"
  labels = local.labels

  rule {
    description = "SSH"
    direction   = "in"
    protocol    = "tcp"
    port        = "22"
    source_ips  = var.ssh_source_ips
  }
  rule {
    description = "HTTP (ACME and redirect)"
    direction   = "in"
    protocol    = "tcp"
    port        = "80"
    source_ips  = ["0.0.0.0/0", "::/0"]
  }
  rule {
    description = "HTTPS"
    direction   = "in"
    protocol    = "tcp"
    port        = "443"
    source_ips  = ["0.0.0.0/0", "::/0"]
  }
  rule {
    description = "HTTP/3"
    direction   = "in"
    protocol    = "udp"
    port        = "443"
    source_ips  = ["0.0.0.0/0", "::/0"]
  }

  dynamic "rule" {
    for_each = local.open_ports
    content {
      description = rule.key
      direction   = "in"
      protocol    = "tcp"
      port        = tostring(rule.value)
      source_ips  = ["0.0.0.0/0", "::/0"]
    }
  }
}

resource "hcloud_server" "app" {
  count              = var.enable_vps ? 1 : 0
  name               = "dabney-eu-1"
  server_type        = var.server_type
  image              = var.server_image
  location           = var.location
  backups            = var.server_backups
  ssh_keys           = [hcloud_ssh_key.deploy[0].id]
  firewall_ids       = [hcloud_firewall.web[0].id]
  delete_protection  = true
  rebuild_protection = true
  labels             = local.labels

  public_net {
    ipv4_enabled = true
    ipv6_enabled = true
  }
}

resource "desec_rrset" "a" {
  for_each = local.vps_hosts
  domain   = desec_domain.zone.name
  subname  = each.value
  type     = "A"
  ttl      = 3600
  rdata    = [hcloud_server.app[0].ipv4_address]
}

resource "desec_rrset" "aaaa" {
  for_each = local.vps_hosts
  domain   = desec_domain.zone.name
  subname  = each.value
  type     = "AAAA"
  ttl      = 3600
  rdata    = [hcloud_server.app[0].ipv6_address]
}

# --- Images and Tier 0: Scaleway --------------------------------------------

resource "scaleway_registry_namespace" "images" {
  count     = var.enable_registry ? 1 : 0
  name      = var.registry_namespace
  region    = var.scaleway_region
  is_public = false
}

resource "scaleway_container_namespace" "web" {
  count  = local.serverless ? 1 : 0
  name   = "dabney-web"
  region = var.scaleway_region
}

resource "scaleway_container" "web" {
  count                  = local.serverless ? 1 : 0
  name                   = "dabney-web"
  namespace_id           = scaleway_container_namespace.web[0].id
  image                  = var.serverless_image
  port                   = 8080
  min_scale              = 0
  max_scale              = 2
  memory_limit_bytes     = 268435456
  cpu_limit              = 140
  privacy                = "public"
  protocol               = "http1"
  https_connections_only = true
  environment_variables  = merge({ RUST_LOG = "info" }, var.serverless_environment)
}

resource "desec_rrset" "serverless" {
  count   = local.serverless && var.serverless_host != "" ? 1 : 0
  domain  = desec_domain.zone.name
  subname = var.serverless_host
  type    = "CNAME"
  ttl     = 3600
  rdata   = ["${local.serverless_hostname}."]

  lifecycle {
    precondition {
      condition     = !contains(local.vps_hosts, var.serverless_host)
      error_message = "serverless_host is also a VPS host; a name has one owner."
    }
  }
}

resource "scaleway_container_domain" "web" {
  count        = local.serverless && var.serverless_host != "" ? 1 : 0
  container_id = scaleway_container.web[0].id
  hostname     = "${var.serverless_host}.${var.domain}"
  region       = var.scaleway_region
  depends_on   = [desec_rrset.serverless]
}
