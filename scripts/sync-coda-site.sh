#!/usr/bin/env bash
# Regenerate public/coda/ and vendor/coda-rust/ from coda-rust's static exporter
# and embeddable pure-Rust `physician-demo-api` crate slice for https://dabney.moe/coda/.
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

vendor_root="$repo_root/vendor/coda-rust"
rm -rf "$vendor_root"
mkdir -p "$vendor_root/crates"

for crate in agent-skills harness http physician-bus physician-wire physician-demo-db physician-demo-api; do
  src_crate="$coda_root/crates/$crate"
  dst_crate="$vendor_root/crates/$crate"
  mkdir -p "$dst_crate"
  cp "$src_crate/Cargo.toml" "$dst_crate/Cargo.toml"
  cp -r "$src_crate/src" "$dst_crate/src"
  if [ -d "$src_crate/assets" ]; then
    cp -r "$src_crate/assets" "$dst_crate/assets"
  fi
  if [ -d "$src_crate/tests/support" ]; then
    mkdir -p "$dst_crate/tests"
    cp -r "$src_crate/tests/support" "$dst_crate/tests/support"
  fi
  rm -rf "$dst_crate/src/bin"
done

if [ -d "$coda_root/skills" ]; then
  cp -r "$coda_root/skills" "$vendor_root/skills"
fi

nix develop "${coda_root}#agent" --command python3 - "$vendor_root/crates" <<'PY'
import pathlib, re, sys

crates_dir = pathlib.Path(sys.argv[1])
for manifest in sorted(crates_dir.glob("*/Cargo.toml")):
    text = manifest.read_text(encoding="utf-8")
    text = text.replace('edition.workspace = true', 'edition = "2024"')
    text = text.replace('license.workspace = true', 'license = "MIT"')
    text = text.replace('publish.workspace = true', 'publish = false')
    text = text.replace('version.workspace = true', 'version = "0.1.0"')
    text = re.sub(r'\n\[lints\]\nworkspace = true\n?', '\n', text)
    # Strip dev-dependencies (and the seed_demo_db binary target) so Cargo never
    # resolves test-only crates outside the runtime closure.
    text = re.sub(r'\n\[dev-dependencies\][\s\S]*$', '\n', text)
    manifest.write_text(text, encoding="utf-8")
PY
