# Cloudflare Free-tier edge protection in front of Cloud Run + Anubis.
#
# When enable_cloudflare_edge_protection = true (paired with cloudflare_proxied = true
# and a Cloudflare API token covering Zone Settings, Cache Rules, Single Redirects,
# Zone WAF, and Bot Management), Cloudflare absorbs volumetric floods, caches
# static/versioned assets at the edge for $0 egress, enforces ACME-safe HTTPS,
# and applies free rate limiting + Bot Fight Mode before traffic reaches GCP.

locals {
  cf_edge_enabled = var.enable_cloudflare_dns && var.enable_cloudflare_edge_protection && var.cloudflare_zone_id != ""
}

# End-to-end TLS against Cloud Run's Google-managed certificate.
resource "cloudflare_zone_setting" "ssl" {
  count = local.cf_edge_enabled ? 1 : 0

  zone_id    = var.cloudflare_zone_id
  setting_id = "ssl"
  value      = "strict"
}

resource "cloudflare_zone_setting" "min_tls_version" {
  count = local.cf_edge_enabled ? 1 : 0

  zone_id    = var.cloudflare_zone_id
  setting_id = "min_tls_version"
  value      = "1.2"
}

resource "cloudflare_zone_setting" "browser_check" {
  count = local.cf_edge_enabled ? 1 : 0

  zone_id    = var.cloudflare_zone_id
  setting_id = "browser_check"
  value      = var.enable_cloudflare_bot_detection ? "on" : "off"
}

resource "cloudflare_zone_setting" "security_level" {
  count = local.cf_edge_enabled ? 1 : 0

  zone_id    = var.cloudflare_zone_id
  setting_id = "security_level"
  value      = var.enable_cloudflare_bot_detection ? "medium" : "essentially_off"
}

# Keep zone-level Always Use HTTPS off so Google Cloud Run's managed TLS cert
# renewal can complete HTTP-01 challenges on /.well-known/acme-challenge/*.
# All other HTTP requests are 301-redirected to HTTPS by the ruleset below.
resource "cloudflare_zone_setting" "always_use_https" {
  count = local.cf_edge_enabled ? 1 : 0

  zone_id    = var.cloudflare_zone_id
  setting_id = "always_use_https"
  value      = "off"
}

resource "cloudflare_ruleset" "https_redirect" {
  count = local.cf_edge_enabled ? 1 : 0

  zone_id     = var.cloudflare_zone_id
  name        = "dabney.moe ACME-safe HTTP to HTTPS redirect"
  description = "Redirect HTTP to HTTPS except Let's Encrypt / Google ACME HTTP-01 challenges"
  kind        = "zone"
  phase       = "http_request_dynamic_redirect"

  rules = [
    {
      action      = "redirect"
      description = "Redirect HTTP to HTTPS on ${var.custom_domain} except /.well-known/acme-challenge/"
      enabled     = true
      expression  = "(http.host eq \"${var.custom_domain}\") and (not ssl) and (not starts_with(http.request.uri.path, \"/.well-known/acme-challenge/\"))"
      action_parameters = {
        from_value = {
          status_code           = 301
          preserve_query_string = true
          target_url = {
            expression = "concat(\"https://\", http.host, http.request.uri.path)"
          }
        }
      }
    }
  ]
}

# Edge cache rules:
#   1. Never cache Anubis PoW API endpoints, app APIs, SSE, or ACME paths.
#   2. Cache versioned/static assets using respect_origin so Anubis challenge
#      pages (Cache-Control: private / no-store) are never cached even if a
#      bot hits an asset path before passing PoW.
resource "cloudflare_ruleset" "cache" {
  count = local.cf_edge_enabled ? 1 : 0

  zone_id     = var.cloudflare_zone_id
  name        = "dabney.moe Anubis-safe edge cache rules"
  description = "Bypass cache for Anubis/API/SSE and respect origin caching on static assets"
  kind        = "zone"
  phase       = "http_request_cache_settings"

  rules = [
    {
      action      = "set_cache_settings"
      description = "Bypass cache on Anubis API, app API, SSE, and ACME endpoints"
      enabled     = true
      expression  = "(http.host eq \"${var.custom_domain}\") and (starts_with(http.request.uri.path, \"/.within.website/x/cmd/anubis/api/\") or starts_with(http.request.uri.path, \"/api/\") or starts_with(http.request.uri.path, \"/clinical/api\") or starts_with(http.request.uri.path, \"/coda/physician/api\") or starts_with(http.request.uri.path, \"/.well-known/\"))"
      action_parameters = {
        cache = false
      }
    },
    {
      action      = "set_cache_settings"
      description = "Cache /coda/ hashed assets at Cloudflare edge (respecting origin Cache-Control)"
      enabled     = true
      expression  = "(http.host eq \"${var.custom_domain}\") and (starts_with(http.request.uri.path, \"/coda/\") or starts_with(http.request.uri.path, \"/.within.website/x/cmd/anubis/static/\") or starts_with(http.request.uri.path, \"/.within.website/x/xess/\"))"
      action_parameters = {
        cache = true
        edge_ttl = {
          mode = "respect_origin"
        }
        browser_ttl = {
          mode = "respect_origin"
        }
      }
    },
    {
      action      = "set_cache_settings"
      description = "Cache public static assets (/pkg, /photos, /commissions, favicon, resume) at Cloudflare edge"
      enabled     = true
      expression  = "(http.host eq \"${var.custom_domain}\") and (starts_with(http.request.uri.path, \"/pkg/\") or starts_with(http.request.uri.path, \"/photos/\") or starts_with(http.request.uri.path, \"/commissions/\") or http.request.uri.path in {\"/favicon.png\" \"/resume.pdf\"})"
      action_parameters = {
        cache = true
        edge_ttl = var.enable_anubis ? {
          mode = "respect_origin"
          } : {
          mode    = "override_origin"
          default = 86400
        }
        browser_ttl = {
          mode = "respect_origin"
        }
      }
    }
  ]
}

# Free-tier Custom WAF Ruleset (up to 5 rules on Cloudflare Free).
# Acts as the $0 edge equivalent of anubis/botPolicies.yaml: blocks AI scrapers
# and exploit scanners, and Managed-Challenges automated/suspicious non-browser
# clients at Cloudflare's edge before traffic ever wakes GCP Cloud Run.
resource "cloudflare_ruleset" "waf_custom" {
  count = local.cf_edge_enabled ? 1 : 0

  zone_id     = var.cloudflare_zone_id
  name        = "dabney.moe free WAF bot and scanner protection"
  description = "Edge equivalent of Anubis: block AI scrapers/scanners and Managed-Challenge automated clients"
  kind        = "zone"
  phase       = "http_request_firewall_custom"

  rules = [
    {
      action      = "block"
      description = "Block known AI training/scraping crawlers, SEO scrapers, and empty User-Agents"
      enabled     = var.enable_cloudflare_bot_detection
      expression  = "(http.host eq \"${var.custom_domain}\") and (not cf.client.bot) and (not starts_with(http.request.uri.path, \"/.well-known/\")) and (http.request.uri.path ne \"/robots.txt\") and (http.user_agent eq \"\" or lower(http.user_agent) contains \"gptbot\" or lower(http.user_agent) contains \"claudebot\" or lower(http.user_agent) contains \"claude-web\" or lower(http.user_agent) contains \"anthropic-ai\" or lower(http.user_agent) contains \"bytespider\" or lower(http.user_agent) contains \"ccbot\" or lower(http.user_agent) contains \"amazonbot\" or lower(http.user_agent) contains \"meta-externalagent\" or lower(http.user_agent) contains \"facebookbot\" or lower(http.user_agent) contains \"perplexitybot\" or lower(http.user_agent) contains \"oai-searchbot\" or lower(http.user_agent) contains \"google-extended\" or lower(http.user_agent) contains \"diffbot\" or lower(http.user_agent) contains \"imagesiftbot\" or lower(http.user_agent) contains \"ahrefsbot\" or lower(http.user_agent) contains \"semrushbot\" or lower(http.user_agent) contains \"dotbot\" or lower(http.user_agent) contains \"mj12bot\" or lower(http.user_agent) contains \"blexbot\" or lower(http.user_agent) contains \"petalbot\" or lower(http.user_agent) contains \"dataforseobot\")"
    },
    {
      action      = "block"
      description = "Block vulnerability scanners and non-existent CMS/config probes at the edge"
      enabled     = true
      expression  = "(http.host eq \"${var.custom_domain}\") and (lower(http.request.uri.path) contains \".php\" or lower(http.request.uri.path) contains \"/.env\" or lower(http.request.uri.path) contains \"/.git\" or starts_with(lower(http.request.uri.path), \"/wp-\") or starts_with(lower(http.request.uri.path), \"/wordpress\") or starts_with(lower(http.request.uri.path), \"/phpmyadmin\") or starts_with(lower(http.request.uri.path), \"/cgi-bin\") or starts_with(lower(http.request.uri.path), \"/actuator\"))"
    },
    {
      action      = "managed_challenge"
      description = "Managed Challenge non-browser/scripted clients and suspicious traffic (verified search/social bots allowed)"
      enabled     = var.enable_cloudflare_bot_detection
      expression  = "(http.host eq \"${var.custom_domain}\") and (not cf.client.bot) and (not starts_with(http.request.uri.path, \"/.well-known/\")) and (not (http.request.uri.path in {\"/robots.txt\" \"/favicon.ico\" \"/favicon.png\" \"/photos/tyler-dunes.jpg\"})) and (not (lower(http.user_agent) contains \"discordbot\" or lower(http.user_agent) contains \"slackbot\" or lower(http.user_agent) contains \"twitterbot\" or lower(http.user_agent) contains \"linkedinbot\" or lower(http.user_agent) contains \"telegrambot\" or lower(http.user_agent) contains \"whatsapp\")) and ((not (lower(http.user_agent) contains \"mozilla\" or lower(http.user_agent) contains \"opera\")) or lower(http.user_agent) contains \"headlesschrome\" or lower(http.user_agent) contains \"playwright\" or lower(http.user_agent) contains \"puppeteer\" or lower(http.user_agent) contains \"selenium\" or lower(http.user_agent) contains \"scrapy\" or cf.threat_score gt 10)"
    },
    {
      action      = "managed_challenge"
      description = "Challenge cross-site or non-same-origin probes to /api/*, /clinical/api*, and /coda/physician/api*"
      enabled     = var.enable_cloudflare_bot_detection
      expression  = "(http.host eq \"${var.custom_domain}\") and (starts_with(http.request.uri.path, \"/api/\") or starts_with(http.request.uri.path, \"/clinical/api\") or starts_with(http.request.uri.path, \"/coda/physician/api\")) and (not any(http.request.headers[\"sec-fetch-site\"][*] eq \"same-origin\"))"
    }
  ]
}

# Free-tier Cloudflare rate limit (1 rule, 10s period, 10s block timeout).
# Absorbs abusive bursts at Cloudflare's edge for $0 before they wake Cloud Run.
resource "cloudflare_ruleset" "rate_limit" {
  count = local.cf_edge_enabled ? 1 : 0

  zone_id     = var.cloudflare_zone_id
  name        = "dabney.moe edge rate limiting"
  description = "Throttle abusive request bursts per IP at Cloudflare edge"
  kind        = "zone"
  phase       = "http_ratelimit"

  rules = [
    {
      action      = "block"
      description = "Block bursts exceeding 120 req / 10s per IP on dynamic routes"
      enabled     = true
      expression  = "(http.host eq \"${var.custom_domain}\") and (not starts_with(http.request.uri.path, \"/pkg/\")) and (not starts_with(http.request.uri.path, \"/photos/\")) and (not starts_with(http.request.uri.path, \"/commissions/\")) and (not starts_with(http.request.uri.path, \"/.within.website/x/cmd/anubis/static/\")) and (not starts_with(http.request.uri.path, \"/.within.website/x/xess/\"))"
      ratelimit = {
        characteristics     = ["cf.colo.id", "ip.src"]
        period              = 10
        requests_per_period = 120
        mitigation_timeout  = 10
      }
    }
  ]
}

# Free-tier Cloudflare Bot Fight Mode (requires enable_js = true).
# Note: AI bot blocking is enforced in cloudflare_ruleset.waf_custom above
# (Cloudflare's legacy bot_management.ai_bots_protection field is ignored on
# newer Free zones).
resource "cloudflare_bot_management" "this" {
  count = local.cf_edge_enabled ? 1 : 0

  zone_id    = var.cloudflare_zone_id
  enable_js  = var.enable_cloudflare_bot_detection
  fight_mode = var.enable_cloudflare_bot_detection
}
