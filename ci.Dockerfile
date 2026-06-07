# CI base image for dabney.moe.
#
# Nix is pre-installed (single-user; runs as root in CI containers) so every
# container job can call nix commands directly — no nix-installer-action, no
# daemon-socket race, no permission errors.
#
# The Cargo vendor dir is also baked in so every `cargo` invocation runs
# fully offline against the pre-vendored closure, eliminating crates.io traffic.
#
# Built by the `build-ci-image` job in ci.yml and pushed to GHCR whenever
# Cargo.lock changes; all other ci.yml container jobs pull it as their base.
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
# password. Needed by some Nix operations even in single-user mode.
RUN echo 'root ALL=(ALL) NOPASSWD:ALL' >> /etc/sudoers

# Nix — single-user install (no daemon required when running as root in a
# container). Flakes and nix-command are enabled via /etc/nix/nix.conf.
RUN curl -sL https://nixos.org/nix/install | bash -s -- --no-daemon \
 && mkdir -p /etc/nix \
 && printf 'experimental-features = nix-command flakes\naccept-flake-config = true\n' \
    >> /etc/nix/nix.conf

ENV PATH=/root/.nix-profile/bin:$PATH
ENV SSL_CERT_FILE=/etc/ssl/certs/ca-certificates.crt

# The vendor dir and rewritten config.toml are staged by ci-image.yml / the
# bootstrap job before `docker build` runs (materialised from `nix build`).
COPY cargo-vendor /ci-cargo-vendor
COPY cargo-home-config.toml /ci-cargo-home/config.toml

# All cargo invocations in CI use the vendor dir; no crates.io traffic at all.
ENV CARGO_HOME=/ci-cargo-home
ENV CARGO_NET_OFFLINE=true
