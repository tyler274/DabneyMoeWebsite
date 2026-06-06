variable "location" {
  type        = string
  description = "Azure region."
  default     = "eastus"
}

variable "image" {
  type        = string
  description = "SSR image reference incl. tag. May be empty for the phase-1 (registry-only) apply."
  default     = ""
}

variable "custom_domain" {
  type        = string
  description = "Custom domain to bind (e.g. dabney.moe). Empty disables it."
  default     = ""
}

variable "manage_dns" {
  type        = bool
  description = "Create an Azure DNS zone for the custom domain."
  default     = false
}

variable "dns_zone_name" {
  type        = string
  description = "DNS zone name to create when manage_dns = true (e.g. dabney.moe)."
  default     = ""
}
