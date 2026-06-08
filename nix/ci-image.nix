# CI base image inputs.
#
# Exposes `cargoVendorDir`: the entire Cargo.lock closure vendored offline by
# crane. crane's vendorCargoDeps runs `cargo vendor` under the hood, which
# includes platform-conditional deps (windows-sys, libc variants, etc.) for
# all targets in one pass, so this single derivation covers the host toolchain,
# wasm32-unknown-unknown, and all four Android cross-compilation targets.
#
# The derivation is published as `packages.cargoVendorDir` and consumed by the
# `ci-image.yml` workflow, which rewrites the embedded Nix store path to a
# stable Docker path (/ci-cargo-vendor), builds an ubuntu:22.04-based CI image,
# and pushes it to GHCR. Jobs in ci.yml then use that image as their container
# so that every `cargo` invocation runs offline from day one - no `cargo fetch`
# loop, no crates.io latency.
{ pkgs, crane, rustToolchain, src }:
let
  craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;
in
{
  cargoVendorDir = craneLib.vendorCargoDeps { inherit src; };
}
