terraform {
  required_version = ">= 1.6"

  required_providers {
    azurerm = {
      source  = "hashicorp/azurerm"
      version = ">= 3.80"
    }
  }
}

provider "azurerm" {
  features {}
  # Subscription is taken from ARM_SUBSCRIPTION_ID / az login context.
}

locals {
  # See terraform/gcp/main.tf for why LEPTOS_SITE_ROOT is omitted.
  leptos_env = {
    LEPTOS_OUTPUT_NAME  = "dabney"
    LEPTOS_SITE_PKG_DIR = "pkg"
    LEPTOS_SITE_ADDR    = "0.0.0.0:8080"
    LEPTOS_ENV          = "PROD"
  }
}

module "site" {
  source = "../modules/azure-containerapps"

  location      = var.location
  image         = var.image
  env           = local.leptos_env
  custom_domain = var.custom_domain
  manage_dns    = var.manage_dns
  dns_zone_name = var.dns_zone_name
}
