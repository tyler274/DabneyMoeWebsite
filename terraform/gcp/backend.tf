# Remote state is recommended for shared/CI use. Defaults to local state so a
# fresh clone works without provisioning a bucket first. To enable, create a
# GCS bucket and uncomment:
#
# terraform {
#   backend "gcs" {
#     bucket = "dabney-tfstate"
#     prefix = "gcp/cloudrun"
#   }
# }
