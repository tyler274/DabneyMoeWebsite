# Rust toolchains, all derived from the pinned `rust-toolchain.toml` so a single
# channel bump rolls every target. The base toolchain already carries
# `wasm32-unknown-unknown` (for Leptos hydrate/CSR); the variants layer on the
# extra cross-compilation targets so contributors only pay for what they use.
{ pkgs, rustToolchainFile }:
let
  rustToolchain = pkgs.rust-bin.fromRustupToolchainFile rustToolchainFile;
in
{
  inherit rustToolchain;

  # The pinned toolchain plus the Android cross-compilation targets Tauri
  # needs. Layered separately so web-only contributors don't pay to download
  # Android std libraries in the default shell.
  rustToolchainAndroid = rustToolchain.override {
    targets = [
      "wasm32-unknown-unknown"
      "aarch64-linux-android"
      "armv7-linux-androideabi"
      "i686-linux-android"
      "x86_64-linux-android"
    ];
  };

  # The pinned toolchain plus the static-musl target used to build the
  # hardened, glibc-free SSR server image. Keeps the wasm target so the
  # hydrate bundle still compiles in the same derivation.
  rustToolchainMusl = rustToolchain.override {
    targets = [
      "wasm32-unknown-unknown"
      "x86_64-unknown-linux-musl"
    ];
  };
}
