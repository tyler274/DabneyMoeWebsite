# Everything in the dev shells except the Rust toolchain itself (so the default
# and Android shells can each supply their own toolchain).
{ pkgs }:
with pkgs; [
  # Leptos full-stack build orchestrator (ssr + hydrate).
  cargo-leptos
  # CSR bundler used for the Tauri frontend.
  trunk
  wasm-bindgen-cli
  binaryen
  # Tauri v2 CLI (`cargo tauri ...`).
  cargo-tauri
  # JS/TS engine & toolchain of choice.
  bun
  nodejs_22
  # Styling.
  tailwindcss
  dart-sass
  # Build glue.
  pkg-config
  gcc
  # General scripting (CI helpers, ad-hoc tooling).
  python3
  # Nix formatter (matches the flake `formatter`); also gated in CI.
  nixpkgs-fmt
  # GitHub Actions workflow formatter; gated by the pre-push hook.
  yamlfmt
  # Local GitHub Actions runner (validate workflows without pushing).
  act
  # clippy
]
