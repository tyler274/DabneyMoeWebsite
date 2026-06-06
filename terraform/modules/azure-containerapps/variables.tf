variable "resource_group_name" {
  type        = string
  description = "Resource group to create for all dabney.moe web resources."
  default     = "dabney-web"
}

variable "location" {
  type        = string
  description = "Azure region."
  default     = "eastus"
}

variable "service_name" {
  type        = string
  description = "Container App name (also used to name the environment and log workspace)."
  default     = "dabney-web"
}

variable "registry_name" {
  type        = string
  description = "Azure Container Registry name (globally unique, alphanumeric)."
  default     = "dabneyweb"
}

variable "image" {
  type        = string
  description = "Fully-qualified ACR image reference incl. tag (e.g. dabneyweb.azurecr.io/web:latest)."
}

variable "container_port" {
  type        = number
  description = "Port the SSR server listens on (matches LEPTOS_SITE_ADDR in the image)."
  default     = 8080
}

variable "cpu" {
  type        = number
  description = "vCPU per replica (e.g. 0.5)."
  default     = 0.5
}

variable "memory" {
  type        = string
  description = "Memory per replica (e.g. \"1Gi\"). Must pair with cpu per Azure's allowed combinations."
  default     = "1Gi"
}

variable "min_replicas" {
  type        = number
  description = "Minimum replica count (0 allows scale-to-zero)."
  default     = 0
}

variable "max_replicas" {
  type        = number
  description = "Maximum replica count."
  default     = 4
}

variable "env" {
  type        = map(string)
  description = "Runtime environment variables (the LEPTOS_* set is supplied by the root config)."
  default     = {}
}

# --- Optional custom domain + DNS -------------------------------------------

variable "custom_domain" {
  type        = string
  description = "Custom domain to bind to the Container App (e.g. dabney.moe). Empty disables binding."
  default     = ""
}

variable "certificate_id" {
  type        = string
  description = "Container App environment certificate ID for the custom domain. Required to actually bind a domain (Azure validates the domain first)."
  default     = ""
}

variable "manage_dns" {
  type        = bool
  description = "Create an Azure DNS zone for the apex domain."
  default     = false
}

variable "dns_zone_name" {
  type        = string
  description = "DNS zone name to create when manage_dns = true (e.g. dabney.moe)."
  default     = ""
}
