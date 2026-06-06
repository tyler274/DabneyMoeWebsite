resource "azurerm_resource_group" "rg" {
  name     = var.resource_group_name
  location = var.location
}

# Registry the SSR image is pushed to before the app is (re)deployed.
resource "azurerm_container_registry" "acr" {
  name                = var.registry_name
  resource_group_name = azurerm_resource_group.rg.name
  location            = azurerm_resource_group.rg.location
  sku                 = "Basic"
  admin_enabled       = true
}

resource "azurerm_log_analytics_workspace" "law" {
  name                = "${var.service_name}-logs"
  resource_group_name = azurerm_resource_group.rg.name
  location            = azurerm_resource_group.rg.location
  sku                 = "PerGB2018"
  retention_in_days   = 30
}

resource "azurerm_container_app_environment" "env" {
  name                       = "${var.service_name}-env"
  resource_group_name        = azurerm_resource_group.rg.name
  location                   = azurerm_resource_group.rg.location
  log_analytics_workspace_id = azurerm_log_analytics_workspace.law.id
}

resource "azurerm_container_app" "web" {
  name                         = var.service_name
  resource_group_name          = azurerm_resource_group.rg.name
  container_app_environment_id = azurerm_container_app_environment.env.id
  revision_mode                = "Single"

  ingress {
    external_enabled = true
    target_port      = var.container_port

    traffic_weight {
      percentage      = 100
      latest_revision = true
    }
  }

  registry {
    server               = azurerm_container_registry.acr.login_server
    username             = azurerm_container_registry.acr.admin_username
    password_secret_name = "acr-password"
  }

  secret {
    name  = "acr-password"
    value = azurerm_container_registry.acr.admin_password
  }

  template {
    min_replicas = var.min_replicas
    max_replicas = var.max_replicas

    container {
      name   = "web"
      image  = var.image
      cpu    = var.cpu
      memory = var.memory

      # Azure Container Apps does not expose a security-context API through the
      # azurerm Terraform provider; container hardening must be applied at the
      # image level (the OCI User = "65534:65534" set in nix/web-server.nix).

      dynamic "env" {
        for_each = var.env
        content {
          name  = env.key
          value = env.value
        }
      }
    }
  }
}

# --- Optional custom domain + DNS -------------------------------------------

# Binding requires a validated managed/uploaded certificate; off unless a
# certificate_id is supplied. Use the verification ID output to validate the
# domain with Azure first, then provide the certificate.
resource "azurerm_container_app_custom_domain" "domain" {
  count = var.custom_domain != "" && var.certificate_id != "" ? 1 : 0

  name                                     = var.custom_domain
  container_app_id                         = azurerm_container_app.web.id
  container_app_environment_certificate_id = var.certificate_id
  certificate_binding_type                 = "SniEnabled"
}

resource "azurerm_dns_zone" "zone" {
  count = var.manage_dns ? 1 : 0

  name                = var.dns_zone_name
  resource_group_name = azurerm_resource_group.rg.name
}
