# syntax=docker/dockerfile:1
#
# Provider-agnostic multi-stage container build for dabney.moe (no Nix required).
# Produces a hardened, non-root runtime image (UID/GID 65534, port 8080)
# containing the Leptos SSR Axum binary (`web`), asset hash manifest
# (`hash.txt`), and compiled site assets (`site/`, including WASM hydration
# bundle and minified Tailwind CSS).
#
# Build with Docker or Podman:
#   docker build -t dabney-web:latest .
#   podman build -t dabney-web:latest .

ARG RUST_VERSION=1.96
ARG WASM_BINDGEN_VERSION=0.2.130
ARG CARGO_LEPTOS_VERSION=0.3.11

FROM oven/bun:1-debian AS bun-bin

FROM rust:${RUST_VERSION}-bookworm AS builder

ARG WASM_BINDGEN_VERSION
ARG CARGO_LEPTOS_VERSION
ARG DEBIAN_FRONTEND=noninteractive

RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      binaryen \
      ca-certificates \
      clang \
      pkg-config \
 && rm -rf /var/lib/apt/lists/*

COPY --from=bun-bin /usr/local/bin/bun /usr/local/bin/bun

RUN rustup target add wasm32-unknown-unknown \
 && cargo install --locked \
      cargo-leptos@${CARGO_LEPTOS_VERSION} \
      wasm-bindgen-cli@${WASM_BINDGEN_VERSION}

WORKDIR /src

# Install Tailwind CSS tooling via Bun first for better layer caching.
COPY package.json bun.lock ./
RUN bun install --frozen-lockfile

ENV PATH="/src/node_modules/.bin:${PATH}"

# Copy workspace manifests, configuration, assets, and Rust sources.
COPY Cargo.toml Cargo.lock rust-toolchain.toml tailwind.config.js ./
COPY crates ./crates
COPY src-tauri ./src-tauri
COPY style ./style
COPY public ./public

# Point cargo-leptos at the Bun-installed tailwindcss CLI so it never needs to
# download external binaries at build time.
RUN export LEPTOS_TAILWIND_VERSION="$(tailwindcss --help 2>/dev/null | head -n1 | awk '{print $NF}')" \
 && cargo leptos build --release

FROM debian:bookworm-slim AS runtime

ARG DEBIAN_FRONTEND=noninteractive

RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Leptos resolves `hash.txt` relative to current_exe(), so keep it beside `web`.
COPY --from=builder /src/target/release/web /app/web
COPY --from=builder /src/target/release/hash.txt /app/hash.txt
COPY --from=builder /src/target/site /app/site

ENV LEPTOS_OUTPUT_NAME=dabney \
    LEPTOS_HASH_FILES=true \
    LEPTOS_SITE_ROOT=/app/site \
    LEPTOS_SITE_PKG_DIR=pkg \
    LEPTOS_SITE_ADDR=0.0.0.0:8080 \
    LEPTOS_ENV=PROD \
    RUST_LOG=info \
    SSL_CERT_FILE=/etc/ssl/certs/ca-certificates.crt

EXPOSE 8080

# Run unprivileged as nobody:nogroup (UID/GID 65534).
USER 65534:65534

ENTRYPOINT ["/app/web"]
