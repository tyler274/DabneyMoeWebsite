#!/usr/bin/env bash
# Point this repo's git hooks at .githooks/ (pre-push format gate).
set -euo pipefail

root="$(git rev-parse --show-toplevel)"
cd "$root"

chmod +x .githooks/pre-push
git config core.hooksPath .githooks

echo "Installed git hooks from .githooks/"
echo "  pre-push -> nix run .#format-check"
echo ""
echo "Bypass once with: git push --no-verify"
