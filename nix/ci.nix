# Single source of truth for the CI gate: format, lint, and test each crate with
# the right (mutually exclusive) Leptos feature set. Run it locally with
# `nix run .#ci`; GitHub Actions runs the very same script.
{ pkgs, rustToolchain, baseTools, tauriDeps, pkgConfigPath, ldLibraryPath }:
pkgs.writeShellApplication {
  name = "ci";
  runtimeInputs = [ rustToolchain ] ++ baseTools ++ tauriDeps;
  text = ''
    export PKG_CONFIG_PATH="${pkgConfigPath}"
    export LD_LIBRARY_PATH="${ldLibraryPath}"

    echo "==> nixpkgs-fmt (--check)"
    nixpkgs-fmt --check flake.nix nix/

    echo "==> rustfmt (--check)"
    cargo fmt --all --check

    echo "==> clippy: ui (ssr)"
    cargo clippy -p ui --features ssr -- -D warnings
    echo "==> clippy: web (ssr)"
    cargo clippy -p web --features ssr -- -D warnings
    echo "==> clippy: tauri-ui (csr, wasm32)"
    cargo clippy -p tauri-ui --target wasm32-unknown-unknown -- -D warnings
    echo "==> clippy: dabney-app"
    cargo clippy -p dabney-app -- -D warnings
    echo "==> clippy: icon-gen"
    cargo clippy -p icon-gen -- -D warnings

    echo "==> test: ui (data + ssr render)"
    cargo test -p ui --features ssr
    echo "==> test: web (ssr route list)"
    cargo test -p web --features ssr
    echo "==> test: dabney-app (tauri context)"
    cargo test -p dabney-app

    echo "==> CI checks passed"
  '';
}
