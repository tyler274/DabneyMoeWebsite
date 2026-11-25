mock_provider "hcloud" {}
mock_provider "desec" {}
mock_provider "scaleway" {}

variables {
  hcloud_token   = "mock"
  desec_token    = "mock"
  ssh_public_key = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIMockKey deploy@dabney.moe"
}

override_resource {
  target = hcloud_firewall.web
  values = { id = "1001" }
}

override_resource {
  target = hcloud_server.app
  values = { ipv4_address = "198.51.100.20", ipv6_address = "2001:db8:1::1" }
}

run "tier1_vps_owns_the_site_hosts" {
  command = plan
  assert {
    condition     = hcloud_server.app[0].location == "hel1" && hcloud_server.app[0].server_type == "cx23"
    error_message = "Tier 1 is a CX23 in Helsinki by default."
  }
  assert {
    condition     = toset(keys(desec_rrset.a)) == toset(["", "www", "git", "cache", "auth"]) && desec_rrset.a[""].rdata == toset(["198.51.100.20"])
    error_message = "The apex, www, git, cache and auth must resolve to the VPS."
  }
  assert {
    condition     = !contains(keys(desec_rrset.a), "coda") && !contains(keys(desec_rrset.a), "app")
    error_message = "coda (git-pages) and app/api belong to coda-rust infra/eu by default."
  }
  assert {
    condition     = contains(desec_rrset.caa.rdata, "0 issue \"letsencrypt.org\"") && contains(desec_rrset.caa.rdata, "0 issuewild \";\"")
    error_message = "CAA must allow only Let's Encrypt and forbid wildcards."
  }
}

run "coda_moves_to_the_vps_only_when_asked" {
  command = plan
  variables {
    coda_on_vps = true
  }
  assert {
    condition     = contains(keys(desec_rrset.aaaa), "coda")
    error_message = "coda_on_vps must add coda to the VPS hosts."
  }
}

run "tier0_serverless_on_a_subdomain" {
  command = plan
  variables {
    enable_vps       = false
    serverless_image = "rg.fr-par.scw.cloud/dabney-moe/dabney-web:latest"
    serverless_host  = "beta"
  }
  override_resource {
    target = scaleway_container.web
    values = { id = "fr-par/11111111-2222-3333-4444-555555555555", public_endpoint = "https://dabneywebabc-dabney-web.functions.fnc.fr-par.scw.cloud" }
  }
  assert {
    condition     = length(hcloud_server.app) == 0 && length(desec_rrset.a) == 0
    error_message = "Tier 0 alone creates no VPS or A records."
  }
  assert {
    condition     = scaleway_container.web[0].min_scale == 0 && desec_rrset.serverless[0].rdata == toset(["dabneywebabc-dabney-web.functions.fnc.fr-par.scw.cloud."])
    error_message = "Tier 0 scales to zero and CNAMEs the subdomain to the container."
  }
  assert {
    condition     = scaleway_container_domain.web[0].hostname == "beta.dabney.moe"
    error_message = "The container must accept the custom hostname."
  }
}

run "serverless_apex_is_refused" {
  command = plan
  variables {
    serverless_host = "@"
  }
  expect_failures = [var.serverless_host]
}
