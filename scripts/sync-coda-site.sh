#!/usr/bin/env bash
# Regenerate public/coda/ from coda-rust's static exporter (public-site +
# mdBook + seeded SQLite WASM physician demo) configured for https://dabney.moe/coda/.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
coda_root="${CODA_RUST_DIR:-$repo_root/../coda/coda-rust}"

if [ ! -f "$coda_root/scripts/build-pages-site.sh" ]; then
  echo "sync-coda-site.sh: expected coda-rust checkout at '$coda_root' (override with CODA_RUST_DIR)" >&2
  exit 1
fi

OUT_DIR="$repo_root/public/coda" \
BASE_PATH="/coda" \
ORIGIN="https://dabney.moe" \
  nix develop "${coda_root}#agent" --command bash "$coda_root/scripts/build-pages-site.sh"
