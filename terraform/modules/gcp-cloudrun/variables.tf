variable "project_id" {
  type        = string
  description = "GCP project ID that owns the Cloud Run service and Artifact Registry repo."
}

variable "region" {
  type        = string
  description = "GCP region for the service and the Artifact Registry repository."
  default     = "us-central1"
}

variable "service_name" {
  type        = string
  description = "Cloud Run service name."
  default     = "dabney-web"
}

variable "repository_id" {
  type        = string
  description = "Artifact Registry (Docker) repository ID the image is pushed to."
  default     = "dabney"
}

variable "image" {
  type        = string
  description = "Fully-qualified container image reference, including tag (e.g. us-central1-docker.pkg.dev/PROJECT/dabney/web:latest)."
}

variable "container_port" {
  type        = number
  description = "Port the SSR server listens on (matches LEPTOS_SITE_ADDR in the image)."
  default     = 8080
}

variable "cpu" {
  type        = string
  description = "CPU limit per instance."
  default     = "1"
}

variable "memory" {
  type        = string
  description = "Memory limit per instance."
  default     = "512Mi"
}

variable "min_instances" {
  type        = number
  description = "Minimum number of warm instances (0 allows scale-to-zero)."
  default     = 0
}

variable "max_instances" {
  type        = number
  description = "Maximum number of instances."
  default     = 4
}

variable "env" {
  type        = map(string)
  description = "Runtime environment variables (the LEPTOS_* set is supplied by the root config)."
  default     = {}
}

variable "secret_env" {
  type = map(object({
    secret_id = string
    version   = optional(string, "latest")
  }))
  description = "Environment variables sourced from Secret Manager (name -> secret reference)."
  default     = {}
}

variable "service_account_email" {
  type        = string
  description = "Runtime service account for the Cloud Run revision. Empty uses the project default compute SA."
  default     = ""
}

variable "allow_unauthenticated" {
  type        = bool
  description = "Grant roles/run.invoker to allUsers so the site is publicly reachable."
  default     = true
}

variable "enable_apis" {
  type        = bool
  description = "Enable the run.googleapis.com / artifactregistry.googleapis.com / dns.googleapis.com services on the project."
  default     = true
}

# --- Optional custom domain + DNS -------------------------------------------

variable "custom_domain" {
  type        = string
  description = "Custom domain to map to the service (e.g. dabney.moe). Empty disables domain mapping."
  default     = ""
}

variable "manage_dns" {
  type        = bool
  description = "Create a Cloud DNS managed zone and the records the domain mapping requires."
  default     = false
}

variable "dns_zone_name" {
  type        = string
  description = "Cloud DNS managed zone resource name (used when manage_dns = true)."
  default     = "dabney-moe"
}

variable "dns_root" {
  type        = string
  description = "Apex DNS name for the managed zone, with trailing dot (e.g. dabney.moe.)."
  default     = ""
}
