# Provider-Agnostic Deployment Guide (`dabney.moe`)

`dabney.moe` is a self-contained Leptos SSR Rust web server (Axum + WASM
hydration + in-process `clinical-workspace` synthetic clinical charts). It has
zero external weight files, zero required cloud vendor lock-in, and zero GitHub
lock-in: you can build the OCI container with **Nix** or a standard
**Dockerfile** and ship it to any OCI registry, any Linux VPS, Fly.io, GCP
Cloud Run, AWS App Runner, or Azure Container Apps.

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
| **GitLab CI** | [`.gitlab-ci.yml`](../.gitlab-ci.yml) | Set CI/CD variable `DEPLOY_TARGET` to `oci`, `ssh`, `cloudrun`, or `fly` (plus target credentials). |
| **Codeberg / Forgejo / Gitea** | [`.forgejo/workflows/deploy.yml`](../.forgejo/workflows/deploy.yml) | Uses `https://code.forgejo.org/actions/checkout@v4` (zero GitHub dependency). Set variable `DEPLOY_TARGET` and target secrets. |
| **GitHub Actions** (optional) | [`.github/workflows/ci.yml`](../.github/workflows/ci.yml) | Set `DOPPLER_TOKEN` secret to enable automatic Cloud Run rollout on `main`. |

## 4. Runtime Environment Variables

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
