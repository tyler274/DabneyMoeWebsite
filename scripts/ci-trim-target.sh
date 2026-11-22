#!/usr/bin/env bash
# Trim `target/` down to dependency artifacts before CI saves it as a cache.
#
# The cache exists so the next run does not recompile every dependency.
# Workspace outputs (linked binaries, the site bundle, incremental state) are
# rebuilt from the checked-out sources on every run anyway, so storing them
# only spends the 10 GB per-repository Actions cache budget.
set -euo pipefail

target="${1:-target}"
if [ ! -d "$target" ]; then
  echo "no $target directory; nothing to cache" >&2
  exit 1
fi

echo "before trim: $(du -sh "$target" | cut -f1)"

# cargo-leptos / rustdoc outputs and scratch space.
rm -rf "$target/site" "$target/tmp" "$target/doc"

find "$target" -type d \( -name incremental -o -name examples \) -prune -exec rm -rf {} +

# Every profile directory (target/<profile>, target/<triple>/<profile>, and
# cargo-leptos's target/front/<triple>/<profile>) holds a `deps` directory. Final artifacts are hard-linked next to it, and the
# linked test/bin executables inside it are extensionless; rlibs, rmeta and
# proc-macro `.so` files keep their extensions and stay.
find "$target" -maxdepth 4 -type d -name deps | while IFS= read -r deps; do
  profile="$(dirname "$deps")"
  find "$profile" -maxdepth 1 -type f -delete
  find "$deps" -maxdepth 1 -type f -perm -u+x ! -name '*.*' -delete
done

echo "after trim:  $(du -sh "$target" | cut -f1)"
du -sh "$target"/* 2>/dev/null || true
df -h . || true
