# CI base image for dabney.moe.
#
# Built by .github/workflows/ci-image.yml and pushed to GHCR whenever
# Cargo.lock changes (or on a weekly schedule). Consumed by ci.yml jobs via
# `container: ghcr.io/...` so that every `cargo` invocation runs fully offline
# against the pre-baked vendor dir — eliminating `cargo fetch` from every run.
#
# Nix is NOT pre-installed here; each job still runs nix-installer-action, which
# is designed to work inside Docker containers (daemon spawned in foreground when
# systemd is absent). What this image provides is:
#   /ci-cargo-vendor/  – the crane-vendored source trees for every crate in
#                        Cargo.lock (all platforms, including Android targets).
#   /ci-cargo-home/config.toml – redirects [source.crates-io] to the vendor dir
#                                so CARGO_NET_OFFLINE=true is all cargo needs.
FROM ubuntu:22.04

ARG DEBIAN_FRONTEND=noninteractive

RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      bash \
      ca-certificates \
      curl \
      git \
      sudo \
      xz-utils \
 && curl -fsSL https://deb.nodesource.com/setup_24.x | bash - \
 && apt-get install -y nodejs \
 && rm -rf /var/lib/apt/lists/*

# Allow root (the default GitHub Actions container user) to sudo without a
# password. The Nix installer needs this to create build users.
RUN echo 'root ALL=(ALL) NOPASSWD:ALL' >> /etc/sudoers

# The vendor dir and rewritten config.toml are staged by ci-image.yml before
# `docker build` runs (the workflow materialises them from `nix build`).
COPY cargo-vendor /ci-cargo-vendor
COPY cargo-home-config.toml /ci-cargo-home/config.toml

# All cargo invocations in CI use the vendor dir; no crates.io traffic at all.
ENV CARGO_HOME=/ci-cargo-home
ENV CARGO_NET_OFFLINE=true
ENV SSL_CERT_FILE=/etc/ssl/certs/ca-certificates.crt
