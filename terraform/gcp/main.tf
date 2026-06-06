terraform {
  required_version = ">= 1.6"

  required_providers {
    google = {
      source  = "hashicorp/google"
      version = ">= 5.0"
    }
  }
}

provider "google" {
  project = var.project_id
  region  = var.region
}

locals {
  # Runtime env for the SSR server. LEPTOS_SITE_ROOT is intentionally NOT set
  # here: the Nix image already points it at the in-store asset bundle. We only
  # pin the values that should be visible/overridable at the infra layer.
  leptos_env = {
    LEPTOS_OUTPUT_NAME  = "dabney"
    LEPTOS_SITE_PKG_DIR = "pkg"
    LEPTOS_SITE_ADDR    = "0.0.0.0:8080"
    LEPTOS_ENV          = "PROD"
  }
}

module "site" {
  source = "../modules/gcp-cloudrun"

  project_id    = var.project_id
  region        = var.region
  image         = var.image
  env           = local.leptos_env
  custom_domain = var.custom_domain
  manage_dns    = var.manage_dns
  dns_root      = var.dns_root
}
