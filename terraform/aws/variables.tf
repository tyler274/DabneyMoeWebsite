variable "region" {
  type        = string
  description = "AWS region."
  default     = "us-east-1"
}

variable "image" {
  type        = string
  description = "SSR image reference incl. tag. May be empty for the phase-1 (registry-only) apply."
  default     = ""
}

variable "custom_domain" {
  type        = string
  description = "Custom domain to associate (e.g. dabney.moe). Empty disables it."
  default     = ""
}

variable "manage_dns" {
  type        = bool
  description = "Create Route53 records (target + ACM validation) in route53_zone_id."
  default     = false
}

variable "route53_zone_id" {
  type        = string
  description = "Route53 hosted zone ID, used when manage_dns = true."
  default     = ""
}
