# Self-hosting dabney.moe (Caddy + Let's Encrypt or Fly.io)

Managed cloud targets under [`../terraform`](../terraform) (Cloud Run, App
Runner, Container Apps) terminate TLS with their own certificates. This
directory supports running `dabney.moe` on **any Linux host or VPS** with
Docker or Podman (fronted by [Caddy](https://caddyserver.com) +
[Let's Encrypt](https://letsencrypt.org)), as well as [Fly.io](https://fly.io)
via [`fly.toml`](fly.toml).

```
internet ──▶ :443 HTTPS  ┌─────────┐  :8080  ┌──────────────┐
             :80  →  443  │  caddy  ├────────▶│ web (SSR app)│
                          └────┬────┘         └──────────────┘
                               │ ACME http-01 / tls-alpn-01
                               ▼
                         Let's Encrypt
```

Caddy provides automatic HTTP→HTTPS redirection, ACME certificate issuance and
renewal at randomized intervals, modern TLS defaults, and immediate streaming
flushes (`flush_interval -1`). Port 80 stays open for the ACME `http-01`
challenge and HTTP→HTTPS redirects.

## Prerequisites (VPS / self-hosted)

- A Linux host with a public IP and inbound **80 + 443** open (80 is required
  for the ACME `http-01` challenge and HTTP→HTTPS redirect).
- `dabney.moe`'s `A` (and `AAAA`) DNS record pointing at that host.
- **Docker** (`docker compose` or `docker-compose`) **or Podman**
  (`podman compose` or `podman-compose`).
- For automated remote deploys, SSH access (`user@host`) with permission to run
  Docker/Podman and write to `--ssh-dir` (default `/opt/dabney-moe`).

## Automated deploy over SSH (`scripts/deploy.sh`)

`scripts/deploy.sh` builds the container image (Nix static-musl by default, or
`Dockerfile` via `--build docker`), copies `selfhost/` configs and the image
tarball over SSH, loads and tags the image in Docker or Podman, and runs
`compose up -d`:

```bash
# Dry-run first (prints planned commands without mutating the remote host):
./scripts/deploy.sh --target ssh --ssh-host deploy@vps.example.com --domain dabney.moe

# Execute the rollout:
./scripts/deploy.sh --target ssh \
  --ssh-host deploy@vps.example.com \
  --ssh-dir /opt/dabney-moe \
  --domain dabney.moe \
  --acme-email ops@dabney.moe \
  --push

# Or deploy an image already published to any OCI registry (no tarball transfer):
./scripts/deploy.sh --build none --target ssh \
  --image registry.gitlab.com/tyler274/dabney-moe/web:latest \
  --ssh-host deploy@vps.example.com \
  --domain dabney.moe \
  --push
```

## Manual run on the host

```bash
cd selfhost
cp .env.example .env          # set ACME_EMAIL and (if needed) SITE_DOMAIN

# Option A: build with Nix (~25 MB static-musl OCI tarball) and load into Docker
nix -C .. build .#serverImage
skopeo --insecure-policy copy docker-archive:../result docker-daemon:dabney-web:latest

# Option B: build with Docker or Podman using the root Dockerfile (no Nix required)
docker build -t dabney-web:latest ..    # or: podman build -t dabney-web:latest ..

# Option C: set WEB_IMAGE in .env to any OCI registry reference and pull it
docker compose up -d                    # or: podman compose up -d
docker compose logs -f caddy            # watch the certificate get issued
```

Open `https://dabney.moe`. Plain `http://` redirects to HTTPS.

> **SELinux hosts (Fedora / RHEL / Rocky / Alma):** append `:Z` to the Caddyfile
> bind mount (`./Caddyfile:/etc/caddy/Caddyfile:ro,Z`) if container policy blocks
> reading the mounted file.

## Fly.io (`fly.toml`)

[`fly.toml`](fly.toml) runs the same container on port 8080 with forced HTTPS
and scale-to-zero (`256mb` shared-cpu VM):

```bash
./scripts/deploy.sh --target fly --push
# Or deploy a pre-built OCI image:
./scripts/deploy.sh --build none --target fly \
  --image registry.gitlab.com/tyler274/dabney-moe/web:latest --push
```

## Testing without burning rate limits

Let's Encrypt's production CA has
[rate limits](https://letsencrypt.org/docs/rate-limits/). While iterating, point
Caddy at the staging CA by uncommenting the `acme_ca` line in
[`Caddyfile`](Caddyfile); staging certificates trigger a browser warning while
exercising the full issuance flow. Remove the line and restart Caddy for a
production certificate.

## HSTS

[`Caddyfile`](Caddyfile) starts `Strict-Transport-Security` at 60 days per the
Let's Encrypt integration guidance so an early certificate misconfiguration
cannot lock visitors out for a year. Once HTTPS is verified stable, raise it to
`max-age=31536000; includeSubDomains; preload`.
