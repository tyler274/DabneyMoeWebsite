# `selfhost/eu`: the sovereign dabney.moe stack on one EU VPS

Tier 1 runs every dabney.moe service on one Hetzner CX23 in Helsinki or Falkenstein, provisioned by [`terraform/eu`](../../terraform/eu/README.md), with DNS on deSEC. Caddy terminates HTTPS for each name and proxies to the compose services over the `edge` network:

| Name | Service | Notes |
| --- | --- | --- |
| `dabney.moe` (`www` redirects) | `dabney-web` | Leptos SSR, streaming responses flushed immediately |
| `git.dabney.moe` | `dabney-forge` | Compose profile `forge`; Rust Git forge; 512 MB request bodies and 30-minute read/write timeouts for pushes and clones; Git over SSH on `FORGE_SSH_PORT` (2222) |
| `coda.dabney.moe` | static files from `./coda-site` | Only when `coda_on_vps = true`; otherwise Codeberg git-pages serves it |
| `cache.dabney.moe` | `attic` | Nix binary cache ([`attic/server.toml`](attic/server.toml)) |
| `auth.dabney.moe` | `pocket-id` | OIDC with passkeys |
| `app.dabney.moe`, `api.dabney.moe` | `physician-sample` + `physician-worker`, `duet-api` | Compose profile `coda`; DNS from coda-rust `infra/eu` |
| — | `radicle` | Compose profile `radicle`; seed node on `RADICLE_PORT` (8776), isolated on its own `radicle` network |
| — | `postgres:16`, `valkey:8` | One database and role per service ([`postgres-init`](postgres-init)) on the internal-only `internal` network; Valkey keeps nothing on disk on the internal-only `cache` network |

> [!CAUTION]
> Public, demo and non-PHI only. No service in this stack may receive protected health information; the Coda demo profile runs synthetic data.

## Configuration

- `.env` (from [`.env.example`](.env.example)) holds non-secret settings: `ACME_EMAIL`, `SITE_DOMAIN`, image references and `COMPOSE_PROFILES`. `scripts/deploy.sh` keeps `SITE_DOMAIN` and `WEB_IMAGE` current.
- Per-service secrets live in `secrets/<service>.env` (templates in [`secrets/`](secrets): `postgres.env.example`, `forge.env.example`, `pocket-id.env.example`, `attic.env.example`, `coda.env.example`) so each container receives only its own credentials. Commit only `secrets/<service>.sops.env`, encrypted with SOPS to age recipients; on each deploy the host decrypts them with its own key (`SOPS_AGE_KEY_FILE`, default `/etc/dabney-moe/age.key`) into `secrets/<service>.env` with mode `0600`.
- `MAIL_SMTP_*` in `secrets/coda.env.example` is currently read by coda-rust's `mail-relay` binary (`gmail::SmtpRelayConfig::from_env`) and applies to `physician-sample` and `duet-api` once those binaries wire `SmtpRelayConfig::from_env` (or route outbound mail via `mail-relay`).
- Image defaults point at their upstream registries, several of which are US-hosted (Docker Hub, GHCR). Set the `*_IMAGE` variables to copies in Scaleway Container Registry (`rg.fr-par.scw.cloud/<namespace>/...`) or Codeberg packages to run without them.

## Deploy

```sh
# Dry run (default), then for real:
./scripts/deploy.sh --target ssh --compose-dir selfhost/eu \
  --ssh-host deploy@<server_ipv4> --domain dabney.moe --acme-email ops@dabney.moe
./scripts/deploy.sh --target ssh --compose-dir selfhost/eu \
  --ssh-host deploy@<server_ipv4> --domain dabney.moe --acme-email ops@dabney.moe --push
```

The host needs Docker with the compose plugin (or Podman with `podman compose`), plus `sops` when `secrets/*.sops.env` files are present. The script copies the compose file, Caddyfile, env templates, `secrets/*.example` and `secrets/*.sops.*` (never plaintext `secrets/*.env`), `postgres-init` and `attic`, loads or pulls the site image, and runs `compose up -d --remove-orphans`.

## Checks

```sh
ACME_EMAIL=ops@example.test docker compose -f selfhost/eu/docker-compose.yml config -q
ACME_EMAIL=ops@example.test docker compose -f selfhost/eu/docker-compose.yml --profile forge --profile coda --profile radicle config -q
ACME_EMAIL=ops@example.test caddy validate --config selfhost/eu/Caddyfile --adapter caddyfile
```
