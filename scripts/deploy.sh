#!/usr/bin/env bash
# Provider-agnostic deployment script for dabney.moe.
#
# Dry-run by default: pass --push to build, push, and roll out the target.
# Supports both Nix (`nix build .#serverImage`) and Dockerfile (`docker`/`podman`)
# builds, and targets any OCI registry, any Linux VPS over SSH (Docker or Podman
# + Caddy automatic Let's Encrypt HTTPS; selfhost/ or the full EU stack in
# selfhost/eu), Scaleway Serverless Containers, GCP Cloud Run, AWS App Runner,
# Azure Container Apps, or Fly.io.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

BUILD_MODE=""
TARGET=""
IMAGE=""
IMAGE_ARCHIVE=""
TAG=""
SSH_HOST="${DEPLOY_SSH_HOST:-}"
SSH_DIR="${DEPLOY_SSH_DIR:-/opt/dabney-moe}"
SSH_KEY="${DEPLOY_SSH_KEY_FILE:-}"
DOMAIN="${SITE_DOMAIN:-dabney.moe}"
DOMAIN_EXPLICIT=0
ACME_EMAIL_ARG="${ACME_EMAIL:-}"
REGION=""
SERVICE_NAME="${DEPLOY_SERVICE_NAME:-dabney-web}"
GCP_PROJECT="${GCP_PROJECT_ID:-}"
REGISTRY_USER="${OCI_REGISTRY_USER:-}"
REGISTRY_PASS="${OCI_REGISTRY_PASS:-}"
PUSH=0
COMPOSE_DIR="${DEPLOY_COMPOSE_DIR:-selfhost}"
SCW_NAMESPACE="${SCW_REGISTRY_NAMESPACE:-dabney-moe}"
SCW_CONTAINER_ID="${SCW_CONTAINER_ID:-}"

TMP_DIR=""
cleanup() {
  if [[ -n "$TMP_DIR" && -d "$TMP_DIR" ]]; then
    rm -rf "$TMP_DIR"
  fi
}
trap cleanup EXIT

usage() {
  cat <<'EOF'
Usage: scripts/deploy.sh --target <oci|ssh|scaleway|cloudrun|aws|azure|fly> [options]

Dry-run by default: prints planned commands without mutating remote targets.
Pass --push to execute the build, upload/push, and service rollout.

Options:
  --build <nix|docker|none>   Build strategy (default: nix if on PATH, else docker)
  --target <target>           Deployment target (required):
                                oci      Push image to any OCI registry
                                ssh      Deploy to any Linux VPS (Docker/Podman + Caddy)
                                scaleway Push to Scaleway Container Registry + roll a
                                         Serverless Container (scw CLI)
                                cloudrun Push to Artifact Registry + gcloud run deploy
                                aws      Push to ECR + tofu -chdir=terraform/aws apply
                                azure    Push to ACR + tofu -chdir=terraform/azure apply
                                fly      Deploy to Fly.io using selfhost/fly.toml
  --image <ref>               Target OCI image reference (e.g. registry.gitlab.com/u/dabney/web)
  --image-archive <path>      Pre-built docker-archive tarball (e.g. ./result or server-image.tar.gz)
  --tag <tag>                 Image tag (default: git short SHA or UTC timestamp)
  --ssh-host <user@host>      SSH destination for --target ssh (env: DEPLOY_SSH_HOST)
  --ssh-dir <dir>             Remote directory for --target ssh (default: /opt/dabney-moe)
  --ssh-key <path>            Optional SSH private key for --target ssh (env: DEPLOY_SSH_KEY_FILE)
  --compose-dir <dir>         Compose stack for --target ssh, relative to the repo
                              (default: selfhost; selfhost/eu is the full EU stack;
                              env: DEPLOY_COMPOSE_DIR)
  --scw-namespace <name>      Scaleway registry namespace (default: dabney-moe;
                              env: SCW_REGISTRY_NAMESPACE)
  --scw-container-id <id>     Serverless Container ID (default: terraform/eu output
                              serverless_container_id; env: SCW_CONTAINER_ID)
  --domain <host>             Site domain (default: dabney.moe; env: SITE_DOMAIN)
  --acme-email <email>        Let's Encrypt email for initial SSH .env (env: ACME_EMAIL)
  --region <region>           Cloud/Fly region (e.g. fr-par, us-central1, us-east-1, eastus, iad)
  --project <id>              GCP project ID for --target cloudrun (env: GCP_PROJECT_ID)
  --service <name>            Service/app name for cloudrun/fly (default: dabney-web)
  --registry-user <user>      OCI registry username (env: OCI_REGISTRY_USER)
  --registry-pass <pass>      OCI registry password/token (env: OCI_REGISTRY_PASS)
  --push                      Execute build/push/deploy (default is dry-run)
  -h, --help                  Show this help

Examples:
  # Dry-run pushing a Nix image to GitLab / Codeberg / Docker Hub / Quay:
  ./scripts/deploy.sh --target oci --image registry.gitlab.com/tyler274/dabney-moe/web

  # Deploy directly to a Linux VPS with Docker or Podman + Caddy HTTPS:
  ./scripts/deploy.sh --target ssh --ssh-host deploy@vps.example.com --domain dabney.moe --push

  # Deploy the full EU stack (selfhost/eu) to the Hetzner VPS from terraform/eu:
  ./scripts/deploy.sh --target ssh --compose-dir selfhost/eu \
    --ssh-host deploy@eu-vps.example --domain dabney.moe --push

  # Tier 0: push to Scaleway Container Registry and roll the Serverless Container
  # (SCW_SECRET_KEY authenticates the registry push; scw reads its own config):
  ./scripts/deploy.sh --target scaleway --region fr-par --push

  # Roll out an already-built archive to Cloud Run:
  ./scripts/deploy.sh --build none --target cloudrun --image-archive server-image.tar.gz \
    --project my-gcp-project --region us-central1 --push

  # Deploy to Fly.io:
  ./scripts/deploy.sh --target fly --region iad --push
EOF
}

die() {
  echo "error: $*" >&2
  exit 1
}

info() {
  echo "==> $*"
}

need_cmd() {
  local cmd="$1"
  if [[ "$PUSH" -eq 1 ]] && ! command -v "$cmd" >/dev/null 2>&1; then
    die "required command not found on PATH: $cmd"
  fi
}

print_cmd() {
  local out=""
  local arg
  for arg in "$@"; do
    out+=" $(printf '%q' "$arg")"
  done
  echo "${out# }"
}

run_cmd() {
  if [[ "$PUSH" -eq 1 ]]; then
    echo "+ $(print_cmd "$@")"
    "$@"
  else
    echo "[dry-run] $(print_cmd "$@")"
  fi
}

# Run a command whose arguments may contain secrets: print a redacted summary
# and execute the real argv when --push is enabled.
run_cmd_redacted() {
  local display="$1"
  shift
  if [[ "$PUSH" -eq 1 ]]; then
    echo "+ $display"
    "$@"
  else
    echo "[dry-run] $display"
  fi
}

has_tag_or_digest() {
  local ref="$1"
  local last="${ref##*/}"
  [[ "$last" == *:* || "$last" == *@* ]]
}

image_with_tag() {
  local ref="$1"
  local tag="$2"
  if has_tag_or_digest "$ref"; then
    printf '%s\n' "$ref"
  else
    printf '%s:%s\n' "$ref" "$tag"
  fi
}

image_repo_only() {
  local ref="$1"
  local prefix=""
  local last="$ref"
  if [[ "$ref" == */* ]]; then
    prefix="${ref%/*}/"
    last="${ref##*/}"
  fi
  last="${last%%@*}"
  last="${last%%:*}"
  printf '%s%s\n' "$prefix" "$last"
}

detect_container_cli() {
  if command -v docker >/dev/null 2>&1; then
    echo "docker"
  elif command -v podman >/dev/null 2>&1; then
    echo "podman"
  else
    echo "docker"
  fi
}

ensure_tmp_dir() {
  if [[ -z "$TMP_DIR" ]]; then
    TMP_DIR="$(mktemp -d)"
  fi
}

prepare_skopeo_registries_conf() {
  # Ubuntu runners often ship a v1 /etc/containers/registries.conf, while
  # Nix-provided skopeo requires v2. Point at a clean minimal v2 TOML file.
  if [[ "$PUSH" -eq 1 ]]; then
    ensure_tmp_dir
    local conf="$TMP_DIR/registries.conf"
    printf 'unqualified-search-registries = []\n' > "$conf"
    export CONTAINERS_REGISTRIES_CONF="$conf"
  fi
}

skopeo_copy_archive() {
  local archive="$1"
  local dest="$2"
  local creds="${3:-}"

  need_cmd skopeo
  prepare_skopeo_registries_conf

  if [[ -n "$creds" ]]; then
    run_cmd_redacted \
      "skopeo --insecure-policy copy --dest-creds <redacted> docker-archive:${archive} docker://${dest}" \
      skopeo --insecure-policy copy --dest-creds "$creds" "docker-archive:${archive}" "docker://${dest}"
  else
    run_cmd skopeo --insecure-policy copy "docker-archive:${archive}" "docker://${dest}"
  fi
}

push_image_to_registry() {
  local dest_ref="$1"
  local explicit_creds="${2:-}"
  local local_ref="${3:-}"
  local creds="$explicit_creds"

  if [[ -z "$creds" ]]; then
    if [[ -n "${OCI_REGISTRY_CREDS:-}" ]]; then
      creds="$OCI_REGISTRY_CREDS"
    elif [[ -n "$REGISTRY_USER" && -n "$REGISTRY_PASS" ]]; then
      creds="${REGISTRY_USER}:${REGISTRY_PASS}"
    fi
  fi

  local repo
  repo="$(image_repo_only "$dest_ref")"
  local also_latest=0
  if ! has_tag_or_digest "$IMAGE" && [[ "$TAG" != "latest" ]]; then
    also_latest=1
  fi

  if [[ -n "$IMAGE_ARCHIVE" ]]; then
    skopeo_copy_archive "$IMAGE_ARCHIVE" "$dest_ref" "$creds"
    if [[ "$also_latest" -eq 1 ]]; then
      skopeo_copy_archive "$IMAGE_ARCHIVE" "${repo}:latest" "$creds"
    fi
  else
    local cli
    cli="$(detect_container_cli)"
    need_cmd "$cli"
    if [[ -n "$local_ref" && "$local_ref" != "$dest_ref" ]]; then
      run_cmd "$cli" tag "$local_ref" "$dest_ref"
    fi
    run_cmd "$cli" push "$dest_ref"
    if [[ "$also_latest" -eq 1 ]]; then
      run_cmd "$cli" tag "$dest_ref" "${repo}:latest"
      run_cmd "$cli" push "${repo}:latest"
    fi
  fi
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --build)
      [[ $# -ge 2 ]] || die "--build requires a value (nix|docker|none)"
      BUILD_MODE="$2"
      shift 2
      ;;
    --target)
      [[ $# -ge 2 ]] || die "--target requires a value (oci|ssh|scaleway|cloudrun|aws|azure|fly)"
      TARGET="$2"
      shift 2
      ;;
    --image)
      [[ $# -ge 2 ]] || die "--image requires a value"
      IMAGE="$2"
      shift 2
      ;;
    --image-archive)
      [[ $# -ge 2 ]] || die "--image-archive requires a path"
      IMAGE_ARCHIVE="$2"
      shift 2
      ;;
    --tag)
      [[ $# -ge 2 ]] || die "--tag requires a value"
      TAG="$2"
      shift 2
      ;;
    --ssh-host)
      [[ $# -ge 2 ]] || die "--ssh-host requires user@host"
      SSH_HOST="$2"
      shift 2
      ;;
    --ssh-dir)
      [[ $# -ge 2 ]] || die "--ssh-dir requires a directory path"
      SSH_DIR="$2"
      shift 2
      ;;
    --ssh-key)
      [[ $# -ge 2 ]] || die "--ssh-key requires a key path"
      SSH_KEY="$2"
      shift 2
      ;;
    --compose-dir)
      [[ $# -ge 2 ]] || die "--compose-dir requires a directory"
      COMPOSE_DIR="$2"
      shift 2
      ;;
    --scw-namespace)
      [[ $# -ge 2 ]] || die "--scw-namespace requires a registry namespace"
      SCW_NAMESPACE="$2"
      shift 2
      ;;
    --scw-container-id)
      [[ $# -ge 2 ]] || die "--scw-container-id requires a container ID"
      SCW_CONTAINER_ID="$2"
      shift 2
      ;;
    --domain)
      [[ $# -ge 2 ]] || die "--domain requires a hostname"
      DOMAIN="$2"
      DOMAIN_EXPLICIT=1
      shift 2
      ;;
    --acme-email)
      [[ $# -ge 2 ]] || die "--acme-email requires an email address"
      ACME_EMAIL_ARG="$2"
      shift 2
      ;;
    --region)
      [[ $# -ge 2 ]] || die "--region requires a value"
      REGION="$2"
      shift 2
      ;;
    --project)
      [[ $# -ge 2 ]] || die "--project requires a GCP project ID"
      GCP_PROJECT="$2"
      shift 2
      ;;
    --service)
      [[ $# -ge 2 ]] || die "--service requires a service name"
      SERVICE_NAME="$2"
      shift 2
      ;;
    --registry-user)
      [[ $# -ge 2 ]] || die "--registry-user requires a username"
      REGISTRY_USER="$2"
      shift 2
      ;;
    --registry-pass)
      [[ $# -ge 2 ]] || die "--registry-pass requires a password/token"
      REGISTRY_PASS="$2"
      shift 2
      ;;
    --push)
      PUSH=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      die "unknown argument: $1 (run with --help)"
      ;;
  esac
done

[[ -n "$TARGET" ]] || die "--target is required (oci|ssh|scaleway|cloudrun|aws|azure|fly)"
case "$TARGET" in
  oci|ssh|scaleway|cloudrun|aws|azure|fly) ;;
  *) die "unsupported --target '$TARGET' (expected: oci|ssh|scaleway|cloudrun|aws|azure|fly)" ;;
esac

if [[ -z "$BUILD_MODE" ]]; then
  if command -v nix >/dev/null 2>&1; then
    BUILD_MODE="nix"
  else
    BUILD_MODE="docker"
  fi
fi

case "$BUILD_MODE" in
  nix|docker|none) ;;
  *) die "unsupported --build '$BUILD_MODE' (expected: nix|docker|none)" ;;
esac

if [[ -z "$TAG" ]]; then
  TAG="$(git -C "$REPO_ROOT" rev-parse --short=12 HEAD 2>/dev/null || date -u +%Y%m%d%H%M%S)"
fi

if [[ "$PUSH" -eq 0 ]]; then
  info "Dry-run mode (pass --push to execute). target=${TARGET} build=${BUILD_MODE} tag=${TAG} domain=${DOMAIN}"
else
  info "Deploying dabney.moe: target=${TARGET} build=${BUILD_MODE} tag=${TAG} domain=${DOMAIN}"
fi

# --- Phase 1: Build image (unless --build none) ------------------------------

LOCAL_DOCKER_IMAGE=""

case "$BUILD_MODE" in
  nix)
    need_cmd nix
    if [[ -z "$IMAGE_ARCHIVE" ]]; then
      ensure_tmp_dir
      IMAGE_ARCHIVE="${TMP_DIR}/server-image.tar.gz"
    fi
    info "Building hardened static-musl OCI image with Nix (.#serverImage)"
    run_cmd nix build "${REPO_ROOT}#serverImage" --out-link "${IMAGE_ARCHIVE}.link"
    run_cmd cp -L "${IMAGE_ARCHIVE}.link" "$IMAGE_ARCHIVE"
    run_cmd rm -f "${IMAGE_ARCHIVE}.link"
    ;;
  docker)
    CONTAINER_CLI="$(detect_container_cli)"
    need_cmd "$CONTAINER_CLI"
    LOCAL_DOCKER_IMAGE="dabney-web:${TAG}"
    info "Building container image with ${CONTAINER_CLI} (Dockerfile)"
    run_cmd "$CONTAINER_CLI" build -f "${REPO_ROOT}/Dockerfile" -t "$LOCAL_DOCKER_IMAGE" "$REPO_ROOT"
    if [[ "$TARGET" == "ssh" ]]; then
      if [[ -z "$IMAGE_ARCHIVE" ]]; then
        ensure_tmp_dir
        IMAGE_ARCHIVE="${TMP_DIR}/server-image.tar.gz"
      fi
      run_cmd "$CONTAINER_CLI" save -o "$IMAGE_ARCHIVE" "$LOCAL_DOCKER_IMAGE"
    fi
    ;;
  none)
    if [[ -n "$IMAGE_ARCHIVE" && "$PUSH" -eq 1 && ! -f "$IMAGE_ARCHIVE" ]]; then
      die "--image-archive does not exist: $IMAGE_ARCHIVE"
    fi
    ;;
esac

# --- Phase 2: Target rollout -------------------------------------------------

case "$TARGET" in
  oci)
    [[ -n "$IMAGE" ]] || die "--target oci requires --image <registry/repo/name[:tag]>"
    if [[ "$BUILD_MODE" == "none" && -z "$IMAGE_ARCHIVE" ]]; then
      info "No --image-archive provided; pushing local container tag via $(detect_container_cli)"
    fi
    FULL_IMAGE="$(image_with_tag "$IMAGE" "$TAG")"
    info "Pushing OCI image to ${FULL_IMAGE}"
    push_image_to_registry "$FULL_IMAGE" "" "$LOCAL_DOCKER_IMAGE"
    ;;

  ssh)
    [[ -n "$SSH_HOST" ]] || die "--target ssh requires --ssh-host <user@host>"
    if [[ "$BUILD_MODE" == "none" && -z "$IMAGE_ARCHIVE" && -z "$IMAGE" ]]; then
      die "--target ssh with --build none requires either --image-archive <path> or --image <ref>"
    fi
    need_cmd ssh
    need_cmd scp

    SSH_OPTS=(-o BatchMode=yes -o StrictHostKeyChecking=accept-new)
    SCP_OPTS=(-o BatchMode=yes -o StrictHostKeyChecking=accept-new)
    if [[ -n "$SSH_KEY" ]]; then
      SSH_OPTS+=(-i "$SSH_KEY")
      SCP_OPTS+=(-i "$SSH_KEY")
    fi

    STACK_DIR="${REPO_ROOT}/${COMPOSE_DIR#./}"
    STACK_DIR="${STACK_DIR%/}"
    for required in docker-compose.yml Caddyfile .env.example; do
      [[ -f "${STACK_DIR}/${required}" ]] || die "--compose-dir ${COMPOSE_DIR} has no ${required}"
    done
    STACK_FILES=("${STACK_DIR}/docker-compose.yml" "${STACK_DIR}/Caddyfile" "${STACK_DIR}/.env.example")
    # Optional pieces of larger stacks (selfhost/eu): encrypted secrets, the
    # secrets template, and config directories. Plaintext secrets.env never leaves.
    for optional in secrets.sops.env secrets.env.example postgres-init attic; do
      if [[ -e "${STACK_DIR}/${optional}" ]]; then
        STACK_FILES+=("${STACK_DIR}/${optional}")
      fi
    done

    info "Syncing ${COMPOSE_DIR}/ Caddy + Compose stack to ${SSH_HOST}:${SSH_DIR}"
    run_cmd ssh "${SSH_OPTS[@]}" "$SSH_HOST" "mkdir -p $(printf '%q' "$SSH_DIR")"
    run_cmd scp -r "${SCP_OPTS[@]}" "${STACK_FILES[@]}" "${SSH_HOST}:${SSH_DIR}/"

    REMOTE_ARCHIVE=""
    REMOTE_IMAGE=""
    if [[ -n "$IMAGE_ARCHIVE" ]]; then
      REMOTE_ARCHIVE="${SSH_DIR}/.dabney-web-${TAG}.tar.gz"
      REMOTE_IMAGE="dabney-web:${TAG}"
      info "Uploading OCI archive to ${SSH_HOST}:${REMOTE_ARCHIVE}"
      run_cmd scp "${SCP_OPTS[@]}" "$IMAGE_ARCHIVE" "${SSH_HOST}:${REMOTE_ARCHIVE}"
    else
      REMOTE_IMAGE="$(image_with_tag "$IMAGE" "$TAG")"
    fi

    # Build a portable POSIX sh rollout script that supports both Docker and Podman.
    REMOTE_SCRIPT="$(cat <<'REMOTE_EOF'
set -eu
SSH_DIR="$1"
DOMAIN="$2"
REMOTE_IMAGE="$3"
REMOTE_ARCHIVE="$4"
ACME_EMAIL_VAL="$5"

cd "$SSH_DIR"

if command -v docker >/dev/null 2>&1; then
  RT="docker"
  if docker compose version >/dev/null 2>&1; then
    COMPOSE="docker compose"
  elif command -v docker-compose >/dev/null 2>&1; then
    COMPOSE="docker-compose"
  else
    echo "error: docker is installed but neither 'docker compose' nor 'docker-compose' was found" >&2
    exit 1
  fi
elif command -v podman >/dev/null 2>&1; then
  RT="podman"
  if podman compose version >/dev/null 2>&1; then
    COMPOSE="podman compose"
  elif command -v podman-compose >/dev/null 2>&1; then
    COMPOSE="podman-compose"
  else
    echo "error: podman is installed but neither 'podman compose' nor 'podman-compose' was found" >&2
    exit 1
  fi
else
  echo "error: neither docker nor podman found on remote host" >&2
  exit 1
fi

if [ -n "$REMOTE_ARCHIVE" ]; then
  if [ "$RT" = "docker" ] && command -v skopeo >/dev/null 2>&1; then
    skopeo --insecure-policy copy "docker-archive:$REMOTE_ARCHIVE" "docker-daemon:$REMOTE_IMAGE"
    docker tag "$REMOTE_IMAGE" "dabney-web:latest"
  else
    LOAD_OUT="$("$RT" load -i "$REMOTE_ARCHIVE")"
    printf '%s\n' "$LOAD_OUT"
    LOADED_REF="$(printf '%s\n' "$LOAD_OUT" | awk '/Loaded image:/{print $NF} /Loaded image\(s\):/{print $NF}' | tail -n1)"
    if [ -z "$LOADED_REF" ]; then
      echo "error: could not parse loaded image tag from '$RT load' output" >&2
      exit 1
    fi
    "$RT" tag "$LOADED_REF" "$REMOTE_IMAGE"
    "$RT" tag "$LOADED_REF" "dabney-web:latest"
  fi
  rm -f "$REMOTE_ARCHIVE"
else
  "$RT" pull "$REMOTE_IMAGE"
fi

if [ ! -f .env ]; then
  cp .env.example .env
fi

# Decrypt container secrets on the host with its own age key; the plaintext
# stays on the host with mode 0600.
if [ -f secrets.sops.env ]; then
  if ! command -v sops >/dev/null 2>&1; then
    echo "error: secrets.sops.env is present but sops is not installed on the host" >&2
    exit 1
  fi
  (
    umask 077
    SOPS_AGE_KEY_FILE="${SOPS_AGE_KEY_FILE:-/etc/dabney-moe/age.key}" \
      sops decrypt --input-type dotenv --output-type dotenv --output secrets.env.tmp secrets.sops.env
    mv secrets.env.tmp secrets.env
  )
fi

upsert_env() {
  key="$1"
  val="$2"
  tmp=".env.tmp.$$"
  awk -v k="$key" -v v="$val" '
    BEGIN { updated = 0 }
    index($0, k "=") == 1 { print k "=" v; updated = 1; next }
    { print }
    END { if (!updated) print k "=" v }
  ' .env > "$tmp"
  mv "$tmp" .env
}

upsert_env "SITE_DOMAIN" "$DOMAIN"
upsert_env "WEB_IMAGE" "$REMOTE_IMAGE"
if [ -n "$ACME_EMAIL_VAL" ]; then
  upsert_env "ACME_EMAIL" "$ACME_EMAIL_VAL"
fi

if grep -q '^ACME_EMAIL=you@example.com$' .env; then
  echo "warning: ACME_EMAIL is still 'you@example.com' in ${SSH_DIR}/.env; set a real email for Let's Encrypt." >&2
fi

$COMPOSE up -d --remove-orphans
REMOTE_EOF
)"

    info "Loading image and rolling Compose stack on ${SSH_HOST} (Docker/Podman + Caddy HTTPS for ${DOMAIN})"
    if [[ "$PUSH" -eq 1 ]]; then
      echo "+ ssh ${SSH_OPTS[*]} ${SSH_HOST} sh -s -- ${SSH_DIR} ${DOMAIN} ${REMOTE_IMAGE} ${REMOTE_ARCHIVE:-<registry-pull>} <acme-email>"
      ssh "${SSH_OPTS[@]}" "$SSH_HOST" sh -s -- \
        "$SSH_DIR" "$DOMAIN" "$REMOTE_IMAGE" "$REMOTE_ARCHIVE" "$ACME_EMAIL_ARG" <<< "$REMOTE_SCRIPT"
    else
      echo "[dry-run] ssh ${SSH_OPTS[*]} ${SSH_HOST} 'detect docker|podman; load/pull ${REMOTE_IMAGE}; update ${SSH_DIR}/.env (SITE_DOMAIN=${DOMAIN}, WEB_IMAGE=${REMOTE_IMAGE}); <compose> up -d --remove-orphans'"
    fi
    ;;

  scaleway)
    REGION="${REGION:-${SCW_DEFAULT_REGION:-fr-par}}"
    case "$REGION" in
      fr-par|nl-ams|pl-waw) ;;
      *) die "--target scaleway needs a Scaleway region (fr-par|nl-ams|pl-waw), got '${REGION}'" ;;
    esac
    need_cmd scw

    if [[ -n "$IMAGE" ]]; then
      FULL_IMAGE="$(image_with_tag "$IMAGE" "$TAG")"
    else
      FULL_IMAGE="rg.${REGION}.scw.cloud/${SCW_NAMESPACE}/${SERVICE_NAME}:${TAG}"
    fi

    # Scaleway Container Registry accepts any user name with an API secret key.
    SCW_CREDS=""
    if [[ -n "${SCW_SECRET_KEY:-}" ]]; then
      SCW_CREDS="nologin:${SCW_SECRET_KEY}"
    elif [[ "$PUSH" -eq 0 ]]; then
      SCW_CREDS="nologin:<SCW_SECRET_KEY>"
    fi

    if [[ -z "$SCW_CONTAINER_ID" && "$PUSH" -eq 1 ]]; then
      need_cmd tofu
      SCW_CONTAINER_ID="$(tofu -chdir="${REPO_ROOT}/terraform/eu" output -raw serverless_container_id 2>/dev/null || true)"
      [[ -n "$SCW_CONTAINER_ID" && "$SCW_CONTAINER_ID" != "null" ]] \
        || die "no Serverless Container: set serverless_image in terraform/eu or pass --scw-container-id"
    fi
    # The OpenTofu ID is <region>/<uuid>; scw wants the UUID.
    SCW_CONTAINER_ID="${SCW_CONTAINER_ID##*/}"
    SCW_CONTAINER_ID="${SCW_CONTAINER_ID:-<serverless_container_id>}"

    info "Pushing image to ${FULL_IMAGE} and rolling Scaleway Serverless Container ${SCW_CONTAINER_ID} (${REGION})"
    if [[ "$BUILD_MODE" != "none" || -n "$IMAGE_ARCHIVE" ]]; then
      push_image_to_registry "$FULL_IMAGE" "$SCW_CREDS" "$LOCAL_DOCKER_IMAGE"
    fi
    # Updating the image redeploys the container.
    run_cmd scw container container update "$SCW_CONTAINER_ID" \
      "registry-image=${FULL_IMAGE}" "region=${REGION}"
    ;;

  cloudrun)
    REGION="${REGION:-${GCP_REGION:-us-central1}}"
    need_cmd gcloud

    if [[ -n "$IMAGE" ]]; then
      FULL_IMAGE="$(image_with_tag "$IMAGE" "$TAG")"
    else
      [[ -n "$GCP_PROJECT" ]] || die "--target cloudrun requires --project <gcp-project-id> (or GCP_PROJECT_ID) or explicit --image"
      FULL_IMAGE="${REGION}-docker.pkg.dev/${GCP_PROJECT}/dabney/web:${TAG}"
    fi

    GCP_CREDS=""
    if [[ -n "$IMAGE_ARCHIVE" ]]; then
      if [[ "$PUSH" -eq 1 ]]; then
        GCP_CREDS="oauth2accesstoken:$(gcloud auth print-access-token)"
      else
        GCP_CREDS="oauth2accesstoken:<gcloud-access-token>"
      fi
    fi

    info "Pushing image to ${FULL_IMAGE} and deploying Cloud Run service ${SERVICE_NAME} (${REGION})"
    if [[ "$BUILD_MODE" != "none" || -n "$IMAGE_ARCHIVE" ]]; then
      push_image_to_registry "$FULL_IMAGE" "$GCP_CREDS" "$LOCAL_DOCKER_IMAGE"
    fi

    DEPLOY_ARGS=(gcloud run deploy "$SERVICE_NAME" --image "$FULL_IMAGE" --region "$REGION" --quiet)
    if [[ -n "$GCP_PROJECT" ]]; then
      DEPLOY_ARGS+=(--project "$GCP_PROJECT")
    fi
    run_cmd "${DEPLOY_ARGS[@]}"
    ;;

  aws)
    REGION="${REGION:-${AWS_REGION:-us-east-1}}"
    need_cmd tofu
    need_cmd aws

    if [[ -n "$IMAGE" ]]; then
      FULL_IMAGE="$(image_with_tag "$IMAGE" "$TAG")"
    elif [[ "$PUSH" -eq 1 ]]; then
      ECR_REPO="$(tofu -chdir="${REPO_ROOT}/terraform/aws" output -raw registry_repository_url)"
      [[ -n "$ECR_REPO" ]] || die "could not read registry_repository_url from terraform/aws; pass --image"
      FULL_IMAGE="${ECR_REPO}:${TAG}"
    else
      FULL_IMAGE="<terraform/aws registry_repository_url>:${TAG}"
    fi

    AWS_CREDS=""
    if [[ -n "$IMAGE_ARCHIVE" ]]; then
      if [[ "$PUSH" -eq 1 ]]; then
        AWS_CREDS="AWS:$(aws ecr get-login-password --region "$REGION")"
      else
        AWS_CREDS="AWS:<aws-ecr-login-password>"
      fi
    fi

    info "Pushing image to ${FULL_IMAGE} and applying terraform/aws (${REGION})"
    if [[ "$BUILD_MODE" != "none" || -n "$IMAGE_ARCHIVE" ]]; then
      push_image_to_registry "$FULL_IMAGE" "$AWS_CREDS" "$LOCAL_DOCKER_IMAGE"
    fi

    TOFU_ARGS=(
      tofu -chdir="${REPO_ROOT}/terraform/aws" apply -auto-approve
      "-var=region=${REGION}"
      "-var=image=${FULL_IMAGE}"
    )
    if [[ "$DOMAIN_EXPLICIT" -eq 1 ]]; then
      TOFU_ARGS+=("-var=custom_domain=${DOMAIN}")
    fi
    run_cmd "${TOFU_ARGS[@]}"
    ;;

  azure)
    REGION="${REGION:-${AZURE_LOCATION:-eastus}}"
    need_cmd tofu

    if [[ -n "$IMAGE" ]]; then
      FULL_IMAGE="$(image_with_tag "$IMAGE" "$TAG")"
    elif [[ "$PUSH" -eq 1 ]]; then
      ACR_REPO="$(tofu -chdir="${REPO_ROOT}/terraform/azure" output -raw registry_repository_url)"
      [[ -n "$ACR_REPO" ]] || die "could not read registry_repository_url from terraform/azure; pass --image"
      FULL_IMAGE="${ACR_REPO}:${TAG}"
    else
      FULL_IMAGE="<terraform/azure registry_repository_url>:${TAG}"
    fi

    info "Pushing image to ${FULL_IMAGE} and applying terraform/azure (${REGION})"
    if [[ "$BUILD_MODE" != "none" || -n "$IMAGE_ARCHIVE" ]]; then
      push_image_to_registry "$FULL_IMAGE" "" "$LOCAL_DOCKER_IMAGE"
    fi

    TOFU_ARGS=(
      tofu -chdir="${REPO_ROOT}/terraform/azure" apply -auto-approve
      "-var=location=${REGION}"
      "-var=image=${FULL_IMAGE}"
    )
    if [[ "$DOMAIN_EXPLICIT" -eq 1 ]]; then
      TOFU_ARGS+=("-var=custom_domain=${DOMAIN}")
    fi
    run_cmd "${TOFU_ARGS[@]}"
    ;;

  fly)
    need_cmd flyctl
    FLY_CONFIG="${REPO_ROOT}/selfhost/fly.toml"
    [[ -f "$FLY_CONFIG" ]] || die "missing Fly.io config: $FLY_CONFIG"

    FLY_ARGS=(flyctl deploy --config "$FLY_CONFIG")
    if [[ -n "$REGION" ]]; then
      FLY_ARGS+=(--region "$REGION")
    fi

    if [[ -n "$IMAGE" ]]; then
      FULL_IMAGE="$(image_with_tag "$IMAGE" "$TAG")"
      if [[ "$BUILD_MODE" != "none" || -n "$IMAGE_ARCHIVE" ]]; then
        info "Pushing image to ${FULL_IMAGE} before Fly.io rollout"
        push_image_to_registry "$FULL_IMAGE" "" "$LOCAL_DOCKER_IMAGE"
      fi
      FLY_ARGS+=(--image "$FULL_IMAGE")
    elif [[ -n "$IMAGE_ARCHIVE" ]]; then
      FULL_IMAGE="registry.fly.io/${SERVICE_NAME}:${TAG}"
      if [[ "$PUSH" -eq 1 ]]; then
        FLY_CREDS="x:$(flyctl auth token)"
      else
        FLY_CREDS="x:<flyctl-auth-token>"
      fi
      info "Pushing OCI archive to ${FULL_IMAGE} and deploying to Fly.io"
      skopeo_copy_archive "$IMAGE_ARCHIVE" "$FULL_IMAGE" "$FLY_CREDS"
      FLY_ARGS+=(--image "$FULL_IMAGE")
    else
      info "Building and deploying on Fly.io from root Dockerfile"
      FLY_ARGS+=(--dockerfile "${REPO_ROOT}/Dockerfile" "$REPO_ROOT")
    fi

    run_cmd "${FLY_ARGS[@]}"
    ;;
esac

if [[ "$PUSH" -eq 0 ]]; then
  info "Dry-run complete. Re-run with --push to execute."
else
  info "Deployment complete."
fi
