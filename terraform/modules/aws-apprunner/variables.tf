variable "service_name" {
  type        = string
  description = "App Runner service name (also used to name the IAM role and scaling config)."
  default     = "dabney-web"
}

variable "repository_name" {
  type        = string
  description = "ECR repository name the SSR image is pushed to."
  default     = "dabney-web"
}

variable "image" {
  type        = string
  description = "Fully-qualified ECR image reference incl. tag (e.g. ACCT.dkr.ecr.us-east-1.amazonaws.com/dabney-web:latest)."
}

variable "container_port" {
  type        = number
  description = "Port the SSR server listens on (matches LEPTOS_SITE_ADDR in the image)."
  default     = 8080
}

variable "cpu" {
  type        = string
  description = "App Runner CPU units (e.g. \"1024\" = 1 vCPU)."
  default     = "1024"
}

variable "memory" {
  type        = string
  description = "App Runner memory in MB (e.g. \"2048\")."
  default     = "2048"
}

variable "min_instances" {
  type        = number
  description = "Minimum provisioned instances."
  default     = 1
}

variable "max_instances" {
  type        = number
  description = "Maximum instances App Runner will scale to."
  default     = 4
}

variable "env" {
  type        = map(string)
  description = "Runtime environment variables (the LEPTOS_* set is supplied by the root config)."
  default     = {}
}

variable "auto_deployments_enabled" {
  type        = bool
  description = "Whether App Runner redeploys automatically when a new image tag is pushed."
  default     = false
}

# --- Optional custom domain + DNS -------------------------------------------

variable "custom_domain" {
  type        = string
  description = "Custom domain to associate with the service (e.g. dabney.moe). Empty disables the association."
  default     = ""
}

variable "manage_dns" {
  type        = bool
  description = "Create Route53 records (domain target + ACM validation) in the provided hosted zone."
  default     = false
}

variable "route53_zone_id" {
  type        = string
  description = "Route53 hosted zone ID to write records into when manage_dns = true."
  default     = ""
}
