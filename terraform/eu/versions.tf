terraform {
  required_version = ">= 1.9.0, < 2.0.0"
  required_providers {
    hcloud = {
      source  = "hetznercloud/hcloud"
      version = "~> 1.52"
    }
    desec = {
      source  = "timofurrer/desec"
      version = "~> 0.6"
    }
    scaleway = {
      source  = "scaleway/scaleway"
      version = "~> 2.60"
    }
  }
  # Local state by default so a fresh clone validates without a bucket. For
  # shared use, keep state on Scaleway Object Storage (fr-par), not a US cloud:
  #   tofu init -backend-config=eu.s3.tfbackend   (after adding `backend "s3" {}`)
  # with bucket, key, region = "fr-par",
  # endpoints = { s3 = "https://s3.fr-par.scw.cloud" } and the skip_* flags.
}

provider "hcloud" {
  token = var.hcloud_token
}

provider "desec" {
  api_token = var.desec_token
}

# Credentials come from SCW_ACCESS_KEY, SCW_SECRET_KEY and
# SCW_DEFAULT_PROJECT_ID in the environment.
provider "scaleway" {
  region = var.scaleway_region
  zone   = "${var.scaleway_region}-1"
}
