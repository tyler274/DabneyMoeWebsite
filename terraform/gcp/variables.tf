variable "project_id" {
  type        = string
  description = "GCP project ID."
}

variable "region" {
  type        = string
  description = "GCP region."
  default     = "us-central1"
}

variable "image" {
  type        = string
  description = "SSR image reference incl. tag. May be empty for the phase-1 (registry-only) apply."
  default     = ""
}

variable "custom_domain" {
  type        = string
  description = "Custom domain to map (e.g. dabney.moe). Empty disables it."
  default     = ""
}

variable "manage_dns" {
  type        = bool
  description = "Create a Cloud DNS zone + records for the custom domain."
  default     = false
}

variable "dns_root" {
  type        = string
  description = "Apex DNS name with trailing dot (e.g. dabney.moe.), used when manage_dns = true."
  default     = ""
}
