# Inputs for dabney.moe on non-US providers. Secrets arrive as
# TF_VAR_hcloud_token and TF_VAR_desec_token (for example from a
# SOPS-decrypted env file); Scaleway reads SCW_* from the environment.

variable "hcloud_token" {
  description = "Hetzner Cloud project API token (read/write)."
  type        = string
  sensitive   = true
}

variable "desec_token" {
  description = "deSEC API token allowed to create and manage the zone."
  type        = string
  sensitive   = true
}

variable "domain" {
  description = "Zone this root creates in deSEC (signed with DNSSEC)."
  type        = string
  default     = "dabney.moe"
}

variable "enable_vps" {
  description = "Tier 1: create the Hetzner VPS that runs selfhost/eu and point the vps_hosts at it."
  type        = bool
  default     = true
}

variable "location" {
  description = "Hetzner location: hel1 (Helsinki, FI), fsn1 or nbg1 (DE)."
  type        = string
  default     = "hel1"

  validation {
    condition     = contains(["hel1", "fsn1", "nbg1"], var.location)
    error_message = "location must be an EU Hetzner location: hel1, fsn1 or nbg1."
  }
}

variable "server_type" {
  description = "Hetzner server type. cx23 is 2 vCPU, 4 GB RAM, 40 GB disk."
  type        = string
  default     = "cx23"
}

variable "server_image" {
  description = "Hetzner OS image."
  type        = string
  default     = "debian-12"
}

variable "server_backups" {
  description = "Enable Hetzner's daily server backups."
  type        = bool
  default     = true
}

variable "ssh_public_key" {
  description = "OpenSSH public key of the deploy user (needed when enable_vps is true)."
  type        = string
  default     = ""
}

variable "ssh_source_ips" {
  description = "CIDRs allowed to reach host SSH on port 22."
  type        = list(string)
  default     = ["0.0.0.0/0", "::/0"]
}

variable "forge_ssh_port" {
  description = "Public port for Git over SSH to the forge (0 closes it)."
  type        = number
  default     = 2222
}

variable "radicle_port" {
  description = "Public port for the Radicle node (0 closes it)."
  type        = number
  default     = 8776
}

variable "vps_hosts" {
  description = "Subdomains (\"\" is the apex) that resolve to the VPS: apex SSR, www, the forge, the Attic cache and Pocket ID."
  type        = list(string)
  default     = ["", "www", "git", "cache", "auth"]
}

variable "curie_on_vps" {
  description = "Serve curie.dabney.moe from the VPS (selfhost/eu Caddy). Leave false while dabney-moe infra/eu points curie at Codeberg git-pages; the two must not both own the name."
  type        = bool
  default     = false
}

variable "caa_issuers" {
  description = "Certificate authorities allowed to issue for the zone."
  type        = list(string)
  default     = ["letsencrypt.org"]
}

variable "caa_iodef" {
  description = "Optional mailto: or https: URL for CAA violation reports."
  type        = string
  default     = ""
}

variable "scaleway_region" {
  description = "Scaleway region for the registry and Serverless Containers."
  type        = string
  default     = "fr-par"

  validation {
    condition     = contains(["fr-par", "nl-ams", "pl-waw"], var.scaleway_region)
    error_message = "scaleway_region must be fr-par, nl-ams or pl-waw."
  }
}

variable "enable_registry" {
  description = "Create a private Scaleway Container Registry namespace (rg.<region>.scw.cloud/<name>)."
  type        = bool
  default     = true
}

variable "registry_namespace" {
  description = "Scaleway Container Registry namespace name."
  type        = string
  default     = "dabney-moe"
}

variable "serverless_image" {
  description = "Tier 0: dabney-web image for a scale-to-zero Scaleway Serverless Container. Empty disables it."
  type        = string
  default     = ""
}

variable "serverless_host" {
  description = "Subdomain bound to the Serverless Container (CNAME plus Scaleway custom domain). An apex cannot be a CNAME."
  type        = string
  default     = ""

  validation {
    condition     = var.serverless_host == "" || can(regex("^[a-z0-9]([a-z0-9-]*[a-z0-9])?$", var.serverless_host))
    error_message = "serverless_host must be a single subdomain label; the apex needs the Tier 1 VPS."
  }
}

variable "serverless_environment" {
  description = "Non-secret environment for the Serverless Container."
  type        = map(string)
  default     = {}
}
