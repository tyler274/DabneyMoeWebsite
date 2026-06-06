# Deploying dabney.moe

The website (`crates/web`) is a Leptos **SSR** app: an Axum server binary plus a
hashed asset bundle. The deploy unit is therefore a **container**, not static
hosting. This directory provisions a serverless-container home for that image on
any of the three major clouds, with one reusable module per cloud and a thin
per-cloud root config so you can pick a target at apply time.

```
terraform/
  modules/
    gcp-cloudrun/         Artifact Registry + Cloud Run v2 service (+ optional domain/DNS)
    aws-apprunner/        ECR + App Runner service (+ optional custom domain/Route53)
    azure-containerapps/  ACR + Container Apps env/app (+ optional custom domain/DNS)
  gcp/  aws/  azure/       Root configs (provider + module wiring + backend)
```

All three modules share an input contract (`image`, `env`, `custom_domain`,
`manage_dns`, scaling/size knobs) and expose the same outputs (`service_url`,
`registry_repository_url`). Every service listens on **port 8080**, matching the
`LEPTOS_SITE_ADDR` baked into the image.

## The container image (built with Nix)

The image is produced by the flake at the repo root, not a Dockerfile:

```bash
nix build .#serverImage          # -> ./result (a docker-archive tarball)
```

It bundles `$out/bin/web` + the in-store asset bundle and sets the `LEPTOS_*`
runtime env (`LEPTOS_SITE_ROOT` points at the bundled assets, so the root
configs deliberately do **not** override it).

`skopeo` (provided by the dev shell) pushes the tarball to any registry. The
`--insecure-policy` flag uses skopeo's accept-all signature policy (Nix ships no
default `policy.json`); it controls image-signature verification, not transport
security:

```bash
skopeo --insecure-policy copy docker-archive:result docker://<registry>/<repo>/web:latest
```

To smoke-test the image locally against a (rootless) Docker daemon instead:

```bash
nix build .#serverImage
docker load --input result               # or: skopeo --insecure-policy copy docker-archive:result docker-daemon:dabney-web:latest
docker run --rm -p 8080:8080 dabney-web:latest
curl -fsS http://127.0.0.1:8080/         # SSR HTML; assets under /pkg/*
```

## Two-phase apply (registry -> image -> service)

A service can't be created until its image exists, and the image can't be pushed
until its registry exists. So apply in two phases:

```bash
cd terraform/gcp                 # or aws / azure
cp terraform.tfvars.example terraform.tfvars   # fill in project/region/etc.
tofu init                        # `terraform` works identically

# Phase 1 — create just the registry.
tofu apply -target=module.site.google_artifact_registry_repository.repo
#   aws:   -target=module.site.aws_ecr_repository.repo
#   azure: -target=module.site.azurerm_container_registry.acr

# Build + push the image to the registry the apply just created
# (see registry_repository_url in the apply output).
nix build .#serverImage
skopeo --insecure-policy copy docker-archive:result docker://<registry_repository_url>/web:latest

# Phase 2 — set `image` (in terraform.tfvars) and apply the rest.
tofu apply
```

CI automates this for GCP on green `main` (see `.github/workflows/ci.yml`).

## Per-cloud notes

| Cloud | Service | Registry | Auth for `apply` |
| --- | --- | --- | --- |
| GCP | Cloud Run v2 | Artifact Registry | `gcloud auth` / `GOOGLE_APPLICATION_CREDENTIALS` |
| AWS | App Runner | ECR | standard AWS provider creds (env/profile) |
| Azure | Container Apps | ACR | `az login` / `ARM_*` env vars |

- **GCP**: needs `project_id`. Enables the `run`/`artifactregistry`/`dns` APIs
  (toggle `enable_apis`). Public access is granted to `allUsers` by default.
- **AWS**: creates the IAM build-access role App Runner needs to pull from a
  private ECR repo.
- **Azure**: creates a resource group, ACR (admin enabled), Log Analytics, and
  the Container Apps environment + app.

## Custom domain (dabney.moe) and TLS

All three managed targets terminate TLS with their own platform-managed
certificates, so nothing extra is needed for HTTPS here. (For a self-hosted /
non-serverless host instead, see [`../selfhost`](../selfhost), which fronts the
image with Caddy + Let's Encrypt.)

Domain mapping and DNS are **optional and off by default** so a first apply works
before nameservers are delegated. Each cloud issues a managed certificate once
the domain is verified:

- **GCP**: set `custom_domain` (verify ownership in Google Search Console first).
  Set `manage_dns = true` + `dns_root = "dabney.moe."` to also create the Cloud
  DNS zone and the records the mapping reports.
- **GCP + Cloudflare** (dabney.moe is hosted on Cloudflare): instead of Cloud
  DNS, set `enable_cloudflare_dns = true`, `cloudflare_api_token` (Zone:DNS:Edit),
  and `cloudflare_zone_id`. The root config mirrors the records the Cloud Run
  mapping reports into Cloudflare as **DNS-only** (`proxied = false`) — Google has
  to terminate TLS to issue its managed cert, and a proxied record wedges
  provisioning by intercepting the ACME challenge. Set `google_site_verification`
  (the token from Search Console) to also create the ownership TXT. Because the
  apex A/AAAA records are only known after the mapping exists, apply in order:
  create the service + mapping (+ verification TXT) first, then a second `tofu
  apply` creates the Cloudflare A/AAAA records. Once the certificate is active you
  may flip records to proxied with SSL/TLS mode **Full (strict)**.
- **AWS**: set `custom_domain`; the association emits ACM validation records.
  Set `manage_dns = true` + `route53_zone_id` to write them (and the CNAME)
  automatically; otherwise add them at your registrar (see `dns_records` output).
- **Azure**: publish the `custom_domain_verification_id` output as an
  `asuid.<domain>` TXT record, then supply a managed/uploaded
  `certificate_id` to bind the domain. `manage_dns = true` creates the DNS zone.

## Secrets (Doppler)

The GCP root integrates [Doppler](https://docs.doppler.com/docs/terraform) as the
source of truth for deploy/runtime secrets. Set `doppler_token` (a personal token
for full management, or a service token) and Terraform will:

- create the Doppler project (`doppler_project`, default `dabney-moe-website`), and
- write the deploy secrets into its `prd` config (`doppler_secret`):
  `CLOUDFLARE_API_TOKEN`, `CLOUDFLARE_ZONE_ID`, `GCP_PROJECT_ID`, `GCP_REGION`,
  and `GOOGLE_SITE_VERIFICATION` (empty values are skipped).

The bootstrap values are injected once via environment variables (never commit
them):

```bash
export TF_VAR_doppler_token=dp.pt.xxxx
export TF_VAR_cloudflare_api_token=cf-token-with-Zone:DNS:Edit
tofu -chdir=terraform/gcp apply ...
```

Once the secrets live in Doppler, later runs can pull the whole set back in as
`TF_VAR_*` instead of exporting them by hand (the `doppler` CLI is in the dev
shell):

```bash
doppler run --name-transformer tf-var -- tofu -chdir=terraform/gcp apply ...
```

The Terraform `doppler` provider stores secret values in state, so secure your
state (see below). Leaving `doppler_token` empty disables all Doppler resources.

## State

Each root uses **local state** by default. Commented remote-backend stanzas
(`gcs` / `s3` / `azurerm`) live in each `backend.tf` — uncomment and `tofu init`
once the backing bucket/account exists.
