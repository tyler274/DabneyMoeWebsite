# Fast format gate: rustfmt, nixpkgs-fmt, and yamlfmt. Used by the pre-push
# git hook and runnable directly via `nix run .#format-check`.
{ pkgs, rustToolchain }:
pkgs.writeShellApplication {
  name = "format-check";
  runtimeInputs = [
    rustToolchain
    pkgs.nixpkgs-fmt
    pkgs.yamlfmt
  ];
  text = ''
    set -euo pipefail

    echo "==> nixpkgs-fmt (--check)"
    nixpkgs-fmt --check flake.nix nix/

    echo "==> rustfmt (--check)"
    cargo fmt --all --check

    echo "==> yamlfmt (-lint)"
    yamlfmt -lint .github/workflows/*.yml

    echo "==> format checks passed"
  '';
}
