# `selfhost/eu`: the sovereign dabney.moe stack on one EU VPS

Tier 1 runs every dabney.moe service on one Hetzner CX23 in Helsinki or Falkenstein, provisioned by [`terraform/eu`](../../terraform/eu/README.md), with DNS on deSEC. Caddy terminates HTTPS for each name and proxies to the compose services:

| Name | Service | Notes |
| --- | --- | --- |
| `dabney.moe` (`www` redirects) | `dabney-web` | Leptos SSR, streaming responses flushed immediately |
| `git.dabney.moe` | `dabney-forge` | Rust Git forge; 512 MB request bodies and 30-minute read/write timeouts for pushes and clones; Git over SSH on `FORGE_SSH_PORT` (2222) |
| `coda.dabney.moe` | static files from `./coda-site` | Only when `coda_on_vps = true`; otherwise Codeberg git-pages serves it |
| `cache.dabney.moe` | `attic` | Nix binary cache ([`attic/server.toml`](attic/server.toml)) |
| `auth.dabney.moe` | `pocket-id` | OIDC with passkeys |
| `app.dabney.moe`, `api.dabney.moe` | `physician-sample` + `physician-worker`, `duet-api` | Compose profile `coda`; DNS from coda-rust `infra/eu` |
| — | `radicle` | Compose profile `radicle`; seed node on `RADICLE_PORT` (8776) |
| — | `postgres:16`, `valkey:8` | One database and role per service ([`postgres-init`](postgres-init)); Valkey keeps nothing on disk |

> [!CAUTION]
> Public, demo and non-PHI only. No service in this stack may receive protected health information; the Coda demo profile runs synthetic data.

## Configuration

- `.env` (from [`.env.example`](.env.example)) holds non-secret settings: `ACME_EMAIL`, `SITE_DOMAIN`, image references and `COMPOSE_PROFILES`. `scripts/deploy.sh` keeps `SITE_DOMAIN` and `WEB_IMAGE` current.
- `secrets.env` (fields in [`secrets.env.example`](secrets.env.example)) reaches the containers through `env_file`. Commit only `secrets.sops.env`, encrypted with SOPS to age recipients; on each deploy the host decrypts it with its own key (`SOPS_AGE_KEY_FILE`, default `/etc/dabney-moe/age.key`) into `secrets.env` with mode `0600`.
- Image defaults point at their upstream registries, several of which are US-hosted (Docker Hub, GHCR). Set the `*_IMAGE` variables to copies in Scaleway Container Registry (`rg.fr-par.scw.cloud/<namespace>/...`) or Codeberg packages to run without them.

## Deploy

```sh
# Dry run (default), then for real:
./scripts/deploy.sh --target ssh --compose-dir selfhost/eu \
  --ssh-host deploy@<server_ipv4> --domain dabney.moe --acme-email ops@dabney.moe
./scripts/deploy.sh --target ssh --compose-dir selfhost/eu \
  --ssh-host deploy@<server_ipv4> --domain dabney.moe --acme-email ops@dabney.moe --push
```

The host needs Docker with the compose plugin (or Podman with `podman compose`), plus `sops` when `secrets.sops.env` is present. The script copies the compose file, Caddyfile, env templates, encrypted secrets, `postgres-init` and `attic`, loads or pulls the site image, and runs `compose up -d --remove-orphans`.

## Checks

```sh
ACME_EMAIL=ops@example.test docker compose -f selfhost/eu/docker-compose.yml config -q
ACME_EMAIL=ops@example.test caddy validate --config selfhost/eu/Caddyfile --adapter caddyfile
```
