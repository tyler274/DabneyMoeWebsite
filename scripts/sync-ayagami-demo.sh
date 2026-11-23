#!/usr/bin/env bash
# Regenerate public/vtuber/demo/ for https://dabney.moe/vtuber/demo/: an
# unmodified web build of Ayagami's demo app (`ayagami-demo`) at one pinned
# upstream commit, built by Trunk from the dev shell with upstream's own
# release configuration.
#
# The viewer ships with no model. Upstream's tree holds no model files and no
# submodules; this script fails if either stops being true, and publishes only
# the app's page, script and WebAssembly beside the licence notices.
#
# To move the pin: change AYAGAMI_REV, run with AYAGAMI_UPDATE_LOCK=1 to
# re-resolve scripts/ayagami-demo.Cargo.lock (upstream commits no lockfile),
# and commit the regenerated public/vtuber/demo/ with both.
set -euo pipefail

AYAGAMI_REPOSITORY="https://github.com/AyagamiDev/ayagami"
AYAGAMI_REV="0d1d7aa3efe57b1b363b72e672e353c52a9c15d6"
PUBLIC_URL="/vtuber/demo/"

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
lockfile="$repo_root/scripts/ayagami-demo.Cargo.lock"
out_dir="$repo_root/public/vtuber/demo"
work_dir="$repo_root/target/ayagami-demo"
src_dir="$work_dir/src"
dist_dir="$work_dir/dist"

rm -rf "$work_dir"
mkdir -p "$src_dir"
git -C "$src_dir" init --quiet
git -C "$src_dir" fetch --quiet --depth 1 "$AYAGAMI_REPOSITORY" "$AYAGAMI_REV"
git -C "$src_dir" checkout --quiet --detach FETCH_HEAD
fetched_rev="$(git -C "$src_dir" rev-parse HEAD)"
if [ "$fetched_rev" != "$AYAGAMI_REV" ]; then
  echo "sync-ayagami-demo.sh: fetched $fetched_rev, expected $AYAGAMI_REV" >&2
  exit 1
fi

# A submodule or a model file in the tree could carry someone else's model
# into the build. Stop rather than publish one.
if [ -s "$src_dir/.gitmodules" ]; then
  echo "sync-ayagami-demo.sh: the pinned tree declares submodules; review them before building" >&2
  exit 1
fi
models="$(git -C "$src_dir" ls-files | grep -Ei '\.(moc3|model3\.json|cdi3\.json|physics3\.json|exp3\.json|motion3\.json|cmo3|zip|png|jpe?g|webp)$' || true)"
if [ -n "$models" ]; then
  echo "sync-ayagami-demo.sh: the pinned tree holds model or image files; refusing to build:" >&2
  echo "$models" >&2
  exit 1
fi

if [ "${AYAGAMI_UPDATE_LOCK:-0}" = "1" ]; then
  nix develop "$repo_root" --command cargo generate-lockfile --manifest-path "$src_dir/Cargo.toml"
  cp "$src_dir/Cargo.lock" "$lockfile"
else
  cp "$lockfile" "$src_dir/Cargo.lock"
fi

# Trunk takes wasm-bindgen and wasm-opt from the dev shell. The path remaps
# keep this machine's home and checkout paths out of the published binary.
nix develop "$repo_root" --command bash -euo pipefail -c '
  src_dir="$1"; dist_dir="$2"; public_url="$3"
  cargo_home="${CARGO_HOME:-$HOME/.cargo}"
  export RUSTFLAGS="--remap-path-prefix=$cargo_home=/cargo --remap-path-prefix=$src_dir=/ayagami"
  cd "$src_dir/ayagami-demo"
  trunk build --config Trunk.release.toml --release --locked \
    --public-url "$public_url" --dist "$dist_dir"
' bash "$src_dir" "$dist_dir" "$PUBLIC_URL"

unexpected="$(find "$dist_dir" -mindepth 1 \
  ! -name index.html ! -name 'ayagami-demo-*.js' ! -name 'ayagami-demo-*.wasm' -print)"
if [ -n "$unexpected" ]; then
  echo "sync-ayagami-demo.sh: unexpected files in the build output:" >&2
  echo "$unexpected" >&2
  exit 1
fi

rm -rf "$out_dir"
mkdir -p "$out_dir"
cp "$dist_dir"/index.html "$dist_dir"/ayagami-demo-*.js "$dist_dir"/ayagami-demo-*.wasm "$out_dir/"
cp "$src_dir/COPYRIGHT" "$out_dir/COPYRIGHT.txt"
cp "$src_dir/LICENSE-MIT" "$out_dir/LICENSE-MIT.txt"
cp "$src_dir/LICENSE-APACHE" "$out_dir/LICENSE-APACHE.txt"

cat > "$out_dir/NOTICE.txt" <<EOF
Model viewer at https://dabney.moe/vtuber/demo/

This directory is an unmodified web build of the Ayagami demo app
(ayagami-demo), from the Ayagami project:

  Source:  $AYAGAMI_REPOSITORY
  Commit:  $AYAGAMI_REV
  Built:   trunk build --config Trunk.release.toml --release --locked --public-url $PUBLIC_URL
  By:      scripts/sync-ayagami-demo.sh in https://github.com/tyler274/DabneyMoeWebsite

Ayagami is Copyright (c) The Ayagami Project Contributors and is dual-licensed
under the Apache License, Version 2.0 and the MIT license. See COPYRIGHT.txt,
LICENSE-APACHE.txt and LICENSE-MIT.txt beside this file. dabney.moe is not
part of the Ayagami project and this copy is not maintained by it.

The build statically links the Rust crates resolved in
scripts/ayagami-demo.Cargo.lock of the repository above, each under its own
licence, and embeds the Droid Sans Fallback font from the Ayagami tree
(Apache License, Version 2.0).

No model files are included. The viewer starts empty and reads only the files
a visitor opens, in their own browser; its content security policy blocks
network connections.

Ayagami is compatible with models in the Live2D model format. It is an
independent project that is not affiliated with, authorized by, endorsed by,
or sponsored by Live2D Inc. "Live2D" and "Cubism" are trademarks of Live2D
Inc., used here only to describe file-format compatibility.
EOF

echo "sync-ayagami-demo.sh: wrote $out_dir from ayagami@$AYAGAMI_REV"
