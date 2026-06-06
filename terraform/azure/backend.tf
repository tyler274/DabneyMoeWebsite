# Remote state is recommended for shared/CI use. Defaults to local state so a
# fresh clone works without provisioning storage first. To enable, create a
# storage account + container and uncomment:
#
# terraform {
#   backend "azurerm" {
#     resource_group_name  = "dabney-tfstate"
#     storage_account_name = "dabneytfstate"
#     container_name       = "tfstate"
#     key                  = "azure/containerapps.tfstate"
#   }
# }
