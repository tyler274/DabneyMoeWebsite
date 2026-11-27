# Provider-Agnostic Deployment Guide (`dabney.moe`)

`dabney.moe` is a self-contained Leptos SSR Rust web server (Axum + WASM
hydration + in-process `clinical-workspace` synthetic clinical charts). It has
zero external weight files, zero required cloud vendor lock-in, and zero GitHub
lock-in: you can build the OCI container with **Nix** or a standard
**Dockerfile** and ship it to any OCI registry, any Linux VPS, Fly.io, GCP
Cloud Run, AWS App Runner, or Azure Container Apps.

## Non-US hosting first

The preferred homes for dabney.moe are EU providers, spanning zero-card free hosting and a single EU VPS. Both keep `scripts/deploy.sh` and `scripts/deploy-serv00.sh` dry-run by default, and cover public, demo and non-PHI surfaces only.

| | Tier 0A (zero-card free EU) | Tier 0B (Scaleway free tier) | Tier 1 (one EU VPS) |
| --- | --- | --- | --- |
| Compute | Serv00 FreeBSD (`x86_64`, Poland) + Codeberg `git-pages` (Germany) | Scaleway Serverless Container (Paris), scales to zero (card required) | Hetzner CX23 in Helsinki or Falkenstein |
| Services | `dabney-forge` (`git.dabney.moe`) & `dabney-web` (`dabney.moe`) via native FreeBSD binaries; static clinical site & WASM demo on Codeberg `git-pages` | `dabney-web` on a subdomain | Everything in [`selfhost/eu`](../selfhost/eu/README.md): the SSR site, the Rust forge, Radicle, Postgres 16, Valkey 8, Attic, Pocket ID, and the optional clinical demo |
| DNS | deSEC with DNSSEC ([`terraform/eu`](../terraform/eu/README.md)) | deSEC with DNSSEC | deSEC with DNSSEC |
| Static clinical site | Codeberg `git-pages` (`_git-pages-repository` TXT) | Codeberg `git-pages` | Codeberg `git-pages`, or Caddy on the VPS |
| Images | Native FreeBSD `x86_64` binaries via `nix develop .#freebsd` | Scaleway Container Registry (`rg.fr-par.scw.cloud`) | Scaleway Container Registry or Codeberg packages |
| Deploy | `./scripts/deploy-serv00.sh --service forge\|web` ([guide](../selfhost/serv00/README.md)) | `./scripts/deploy.sh --target scaleway` | `./scripts/deploy.sh --target ssh --compose-dir selfhost/eu` |

A CNAME cannot sit at the zone apex, so Scaleway Tier 0B serves a subdomain such as
`beta.dabney.moe`; `dabney.moe` at the apex uses Serv00 `A` records (Tier 0A) or Tier 1. The `.moe` registry
backend is operated by a US company whichever registrar holds the domain.

```bash
# Tier 0A (zero credit card): cross-compile and deploy to Serv00 FreeBSD
nix develop .#freebsd --command cargo zigbuild --release --target x86_64-unknown-freebsd -p forge --bin dabney-forge
./scripts/deploy-serv00.sh --service forge --host sN.serv00.com --user <login> \
  --domain git.dabney.moe --port <reserved-port> --serv00-ip <ip> --push

# Provision (reviewed saved plan only): zone, VPS, registry, optional container
(cd terraform/eu && tofu init && tofu plan -out=eu.tfplan)

# Tier 1: the whole stack on the VPS
./scripts/deploy.sh --target ssh --compose-dir selfhost/eu \
  --ssh-host deploy@<server_ipv4> --domain dabney.moe --acme-email ops@dabney.moe --push

# Tier 0B: push to Scaleway Container Registry and roll the Serverless Container
SCW_SECRET_KEY=... ./scripts/deploy.sh --target scaleway --region fr-par --push
```

The US clouds below remain available.

## 1. Building the OCI Container Image

Two interchangeable build paths produce the same non-root runtime contract
(UID/GID `65534:65534`, port `8080`, `LEPTOS_*` runtime env):

### Option A: Reproducible Nix Static-Musl Image (Recommended)

```bash
nix build .#serverImage
```

Produces `./result`, a ~25 MB hardened `x86_64-unknown-linux-musl` OCI tarball
linked with `mold`, fat LTO, and `mimalloc` (`-DMI_SECURE`). Because the
clean-room `clinical-workspace` runs in-process without external model weights,
the image has zero glibc or external store dependencies.

A dynamically-linked glibc fallback is available as `nix build .#serverImageGlibc`.

### Option B: Standard Dockerfile (No Nix Required)

```bash
docker build -t dabney-web:latest .
# or with Podman:
podman build -t dabney-web:latest .
```

The root [`Dockerfile`](../Dockerfile) runs a multi-stage Rust (`cargo-leptos`)
+ Bun/Tailwind build and emits a minimal `debian:bookworm-slim` runtime image
running as `65534:65534` on port `8080`.

## 2. Unified Deploy CLI (`scripts/deploy.sh`)

[`scripts/deploy.sh`](../scripts/deploy.sh) drives every build and rollout path.
It runs in **dry-run mode by default** and only builds, pushes, or modifies a
target when `--push` is passed.

The slim deploy shell provides `skopeo`, `openssh`, `opentofu`, `gcloud`, and
`doppler` without pulling Rust or Tauri:

```bash
nix develop .#deploy
```

### Target: Any OCI Registry (`--target oci`)

Works with GitLab Container Registry, Codeberg Packages, Docker Hub, Quay.io,
GCP Artifact Registry, AWS ECR, Azure ACR, or any self-hosted OCI registry:

```bash
# Dry-run:
./scripts/deploy.sh --target oci --image registry.gitlab.com/tyler274/dabney-moe/web

# Push Nix tarball via skopeo (credentials via OCI_REGISTRY_USER/OCI_REGISTRY_PASS or registry login):
./scripts/deploy.sh --target oci \
  --image codeberg.org/tyler274/dabney-web \
  --tag v1.0.0 \
  --push
```

### Target: Any Linux VPS over SSH (`--target ssh`)

Deploys the [`selfhost/`](../selfhost/) Caddy + Compose stack to any Linux host
running **Docker** or **Podman**, with automatic Let's Encrypt HTTPS for
`dabney.moe`:

```bash
# Build locally (Nix or Dockerfile), transfer archive over SSH, load, and roll Compose:
./scripts/deploy.sh --target ssh \
  --ssh-host deploy@vps.example.com \
  --ssh-dir /opt/dabney-moe \
  --domain dabney.moe \
  --acme-email ops@dabney.moe \
  --push

# Or roll an image already in any OCI registry without transferring a tarball:
./scripts/deploy.sh --build none --target ssh \
  --image registry.gitlab.com/tyler274/dabney-moe/web:latest \
  --ssh-host deploy@vps.example.com \
  --domain dabney.moe \
  --push
```

See [`selfhost/README.md`](../selfhost/README.md) for firewall, DNS, Let's
Encrypt staging, and SELinux notes.

### Target: Scaleway Serverless Containers (`--target scaleway`)

Pushes the image to Scaleway Container Registry
(`rg.<region>.scw.cloud/<namespace>/dabney-web`, namespace from `--scw-namespace`,
default `dabney-moe`) and points the Serverless Container from
[`terraform/eu`](../terraform/eu/) at it with `scw container container update`,
which redeploys it. `SCW_SECRET_KEY` authenticates the push; the container ID
comes from `--scw-container-id` or the `serverless_container_id` output.
Regions: `fr-par`, `nl-ams`, `pl-waw`.

```bash
./scripts/deploy.sh --target scaleway --region fr-par           # dry-run
SCW_SECRET_KEY=... ./scripts/deploy.sh --target scaleway --region fr-par --push
```

### Target: the full EU stack over SSH (`--compose-dir selfhost/eu`)

`--compose-dir` selects which compose stack `--target ssh` syncs. The default,
`selfhost`, is the single-site stack; `selfhost/eu` is the multi-service stack
with per-service encrypted `secrets/*.sops.env` files, which the host decrypts
with its own age key into mode-`0600` `secrets/*.env` files.

### Target: Fly.io (`--target fly`)

Uses [`selfhost/fly.toml`](../selfhost/fly.toml) (256 MB shared-CPU VM, forced
HTTPS, scale-to-zero):

```bash
# Push a Nix-built tarball straight to registry.fly.io and deploy:
./scripts/deploy.sh --target fly --region iad --push

# Or build remotely from Dockerfile / deploy a pre-built OCI image:
./scripts/deploy.sh --build none --target fly \
  --image registry.gitlab.com/tyler274/dabney-moe/web:latest \
  --region iad \
  --push
```

### Target: GCP Cloud Run (`--target cloudrun`)

Pushes the image to Artifact Registry and rolls the Cloud Run service
provisioned by [`terraform/gcp/`](../terraform/gcp/):

```bash
./scripts/deploy.sh --target cloudrun \
  --project "$GCP_PROJECT_ID" \
  --region us-central1 \
  --push
```

### Target: AWS App Runner (`--target aws`)

Pushes the image to ECR and applies [`terraform/aws/`](../terraform/aws/) via
OpenTofu:

```bash
./scripts/deploy.sh --target aws --region us-east-1 --push
```

### Target: Azure Container Apps (`--target azure`)

Pushes the image to ACR and applies [`terraform/azure/`](../terraform/azure/)
via OpenTofu:

```bash
./scripts/deploy.sh --target azure --region eastus --push
```

See [`terraform/README.md`](../terraform/README.md) for initial two-phase
OpenTofu provisioning and custom-domain DNS configuration.

## 3. GitHub-Free CI/CD Pipelines

All CI pipelines run the same `nix run .#ci` quality gate and delegate rollouts
to `scripts/deploy.sh`:

| Forge / CI | Pipeline file | How to configure |
| --- | --- | --- |
| **Gitea (`go-gitea/gitea`)** | [`.gitea/workflows/deploy.yml`](../.gitea/workflows/deploy.yml) & [`.gitea/workflows/update-ownership.yml`](../.gitea/workflows/update-ownership.yml) | Works on `gitea.com` or self-hosted Gitea ([`selfhost/gitea-compose.yml`](../selfhost/gitea-compose.yml)). Uses `https://gitea.com/actions/checkout@v4` with `fetch-depth: 0` (zero GitHub dependency); refreshes code ownership metrics automatically, and `DEPLOY_TARGET=oci` defaults to Gitea's built-in OCI container registry (`<gitea-host>/<owner>/dabney-web`). |
| **Codeberg / Forgejo** | [`.forgejo/workflows/deploy.yml`](../.forgejo/workflows/deploy.yml) & [`.forgejo/workflows/update-ownership.yml`](../.forgejo/workflows/update-ownership.yml) | Uses `https://code.forgejo.org/actions/checkout@v4` with `fetch-depth: 0` (zero GitHub dependency). Set variable `DEPLOY_TARGET`, optional `OWNERSHIP_UPSTREAM_REPO_URL`, `FORGEJO_TOKEN`, and target secrets. |
| **GitLab CI** | [`.gitlab-ci.yml`](../.gitlab-ci.yml) | Full-history checkout (`GIT_DEPTH: "0"`) refreshes `crates/ui/data/ownership-metrics.json` as an artifact into `ci-checks` and `build-image`. Set CI/CD variable `DEPLOY_TARGET` to `oci`, `ssh`, `cloudrun`, or `fly`, plus optional `OWNERSHIP_UPSTREAM_REPO_URL` and `GITLAB_PUSH_TOKEN`. |
| **GitHub Actions** (optional) | [`.github/workflows/ci.yml`](../.github/workflows/ci.yml) | Set `DOPPLER_TOKEN` secret to enable automatic Cloud Run rollout on `main`. |

## 4. Automated CI Code Ownership Updates (`/ownership` & `/ownership.json`)

The public [`/ownership`](https://dabney.moe/ownership) report and the
machine-readable `GET /ownership.json` endpoint (`Content-Type: application/json;
charset=utf-8`, `Cache-Control: public, max-age=300`) are both driven from the
compile-time-embedded [`crates/ui/data/ownership-metrics.json`](../crates/ui/data/ownership-metrics.json)
snapshot (zero runtime file I/O; identical across SSR, WASM hydrate, CSR, and
`nix build .#serverImage`).

### Local & CI Generator (`scripts/update-ownership.py` / `nix run .#update-ownership`)

[`scripts/update-ownership.py`](../scripts/update-ownership.py) uses only the
Python 3 standard library and parallel `git blame --line-porcelain` to recompute
live authorship, tier breakdowns, and commit SHAs deterministically. Because
`generated_at` is derived from `max(website_commit_ts, upstream_commit_ts)` and
metrics-only commits are skipped when resolving the website revision, repeated
runs on the same commits produce byte-identical JSON with zero spurious CI
commit loops.

```bash
# Refresh against a local checkout of the standalone dabney-moe workspace:
nix run .#update-ownership -- --upstream-repo /path/to/dabney-moe --upstream-rev HEAD

# Refresh against a remote Git URL (or set OWNERSHIP_UPSTREAM_REPO_URL):
nix run .#update-ownership -- --upstream-url https://git.example.com/tyler274/dabney-moe.git

# Refresh website metrics offline while keeping the last verified upstream snapshot:
nix run .#update-ownership -- --allow-offline

# Verify the committed JSON matches live git blame (exits 1 with diff on drift):
nix run .#update-ownership -- --upstream-repo /path/to/dabney-moe --check
```

### Automated Pipeline Variables & Triggers

| Variable / Secret | Where to set | Purpose |
| --- | --- | --- |
| `OWNERSHIP_UPSTREAM_REPO_URL` | Repo/CI variable | Optional Git URL for the standalone `dabney-moe` workspace cloned into a temporary bare repo during CI refresh. When unset on push pipelines (`deploy.yml`), `--allow-offline` keeps the cached upstream metrics while still refreshing website metrics. |
| `GITEA_TOKEN` | Gitea secret | Allows `.gitea/workflows/deploy.yml` (with `[skip ci]`) and `.gitea/workflows/update-ownership.yml` (without `[skip ci]`, triggering a fresh deploy) to push updated `crates/ui/data/ownership-metrics.json` to `main`. |
| `FORGEJO_TOKEN` | Forgejo / Codeberg secret | Forgejo/Codeberg counterpart of `GITEA_TOKEN` for `.forgejo/workflows/deploy.yml` and `.forgejo/workflows/update-ownership.yml`. |
| `GITLAB_PUSH_TOKEN` | GitLab CI/CD masked variable | Project access token (`write_repository`) allowing the `commit-ownership` job in `.gitlab-ci.yml` to commit refreshed metrics back to `$CI_DEFAULT_BRANCH` (`-o ci.skip`). |

Both `.gitea/workflows/update-ownership.yml` and
`.forgejo/workflows/update-ownership.yml` run on a daily schedule (`06:00 UTC`)
and accept `workflow_dispatch` with an optional `upstream_url` input so upstream
`dabney-moe` CI can trigger an immediate refresh and redeploy on every push.

## 5. Runtime Environment Variables

| Variable | Default | Purpose |
| --- | --- | --- |
| `LEPTOS_OUTPUT_NAME` | `dabney` | Asset bundle prefix (`pkg/dabney.{js,wasm}`). |
| `LEPTOS_HASH_FILES` | `true` | Enables content-hashed asset lookup via `hash.txt`. |
| `LEPTOS_SITE_ROOT` | Baked into image | Directory holding static assets and `pkg/`. |
| `LEPTOS_SITE_PKG_DIR` | `pkg` | Subdirectory under `LEPTOS_SITE_ROOT` for JS/WASM/CSS. |
| `LEPTOS_SITE_ADDR` | `0.0.0.0:8080` | Bind address and port. |
| `LEPTOS_ENV` | `PROD` | Leptos runtime mode. |
| `RUST_LOG` | `info` | Structured log filter. |
| `CLINICAL_API_URL` | *(empty)* | Optional HTTPS origin proxied under `/clinical/api`; empty runs the embedded synthetic clinical workspace API in-process. |
| `CLINICAL_PUBLIC_ORIGIN` | *(empty)* | Optional public origin for same-origin CSRF checks when `CLINICAL_API_URL` is set. |
| `GA4_MEASUREMENT_ID` | *(empty)* | Optional Google Analytics 4 measurement ID (loaded only after cookie consent). |
| `LOKI_URL` / `LOKI_USER` / `LOKI_TOKEN` | *(empty)* | Optional Grafana Loki structured log forwarding. |
