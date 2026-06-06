terraform {
  required_version = ">= 1.6"

  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = ">= 5.0"
    }
  }
}

provider "aws" {
  region = var.region
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
  source = "../modules/aws-apprunner"

  image           = var.image
  env             = local.leptos_env
  custom_domain   = var.custom_domain
  manage_dns      = var.manage_dns
  route53_zone_id = var.route53_zone_id
}
