# dabney.moe

Tyler Port's résumé, clinical workspace showcase, and freelance
software-engineering site, plus a multiplatform app suite. The web frontend is
a **Leptos** SSR app (Axum + WASM hydration); the same UI is reused inside a
**Tauri v2** shell (desktop + mobile) as a client-side rendered build.
Everything is driven by a **Nix flake** dev shell, with **Bun** for JS tooling
and a standalone **Dockerfile** for Nix-free container builds.

## Architecture

The page content and synthetic clinical domain live in shared Rust crates. The
web server and the Tauri shell are thin wrappers around them.

```
crates/clinical-workspace  clean-room synthetic clinical charts, grounded citation Q&A, optional Axum API
crates/ui                  shared Leptos components + App root (features: ssr/hydrate/csr)
crates/web                 cargo-leptos package: Axum SSR binary + WASM hydrate cdylib
crates/tauri-ui            Trunk CSR binary that mounts ui::App, output to dist/
src-tauri                  Tauri v2 app (desktop + mobile), serves dist/
```

| Path | Purpose |
| --- | --- |
| `flake.nix` / `rust-toolchain.toml` | Dev shell + pinned Rust toolchain (incl. `wasm32-unknown-unknown`). |
| `Cargo.toml` | Workspace + `[[workspace.metadata.leptos]]` config. |
| `Dockerfile` | Multi-stage container build (pure Rust `cargo-leptos` + Bun/Tailwind, no Nix required). |
| `scripts/deploy.sh` | Provider-agnostic deploy CLI (`oci`, `ssh`, `cloudrun`, `aws`, `azure`, `fly`; dry-run by default). |
| `tailwind.config.js`, `style/input.css` | Tailwind CSS source. |
| `package.json` | Bun-managed tooling (Tailwind CLI + scripts). |
| `public/` | Static assets served at the site root (`resume.pdf`, SVG/PNG icons, photos, original artwork). |
| `crates/icon-gen` | Small Rust dev tool that regenerates the source app icon. |

## Prerequisites

Everything is provided by the Nix flake; you only need Nix with flakes enabled
(and, optionally, `direnv`).

```bash
# One-off:
nix develop

# Or, with direnv (an .envrc with `use flake` is already present):
direnv allow
```

The shell provides: the Rust toolchain (+ wasm target), `cargo-leptos`, `trunk`,
`cargo-tauri`, `bun`, `tailwindcss`, and the Linux desktop libraries Tauri's
webview needs.

### Dev Container (no Nix required)

Contributors on Windows/macOS/Linux who have Docker + VS Code or Cursor but
*not* Nix can use the dev container in [`.devcontainer/`](.devcontainer/): it
builds on the `nixos/nix` image, enables flakes, and wires up `direnv` so the
editor and its terminal automatically enter this project's `nix develop` shell
(rust-analyzer, `cargo-leptos`, `trunk`, etc. all just work). Open the folder
and choose "Reopen in Container".

## Web (the dabney.moe homepage)

```bash
# Live-reloading SSR dev server on http://127.0.0.1:3039
cargo leptos watch

# Production build → target/site (server binary at target/release/web)
cargo leptos build --release
```

Serving the release build: run `target/release/web` with the `LEPTOS_*`
environment variables (see `[[workspace.metadata.leptos]]` in `Cargo.toml`), e.g.
`LEPTOS_OUTPUT_NAME=dabney LEPTOS_HASH_FILES=true LEPTOS_SITE_ROOT=target/site
LEPTOS_SITE_PKG_DIR=pkg LEPTOS_SITE_ADDR=0.0.0.0:3039 ./target/release/web`.
Keep the generated `hash.txt` beside the server executable: the SSR shell uses it
to resolve the matching content-hashed JS, WASM, and CSS filenames. Both the Nix
images and the root [`Dockerfile`](Dockerfile) package this manifest and enable
hash resolution automatically.

## Apps (Tauri)

```bash
# Desktop dev (launches the webview against the Trunk dev server)
cargo tauri dev

# Desktop build: compile only (no OS packaging)
cargo tauri build --no-bundle

# Desktop build: full installers (deb/rpm/AppImage on Linux). Requires the
# relevant OS packaging tools to be available.
cargo tauri build
```

The Tauri frontend is built by Trunk via the root `Trunk.toml`, configured
as Tauri's `beforeDevCommand` / `beforeBuildCommand`, emitting to `dist/`.

### Mobile (Android / iOS)

The Android app (`moe.dabney.demo`, configured via `src-tauri/tauri.android.conf.json`)
launches directly into the zero-PHI **Dabney.moe Clinical Demo** suite
(`Patient`, `Physician`, `Mood & CGM`, and `Overview` tabs) sharing the exact
same Rust/WASM components served in-browser at `dabney.moe/clinical`,
`dabney.moe/clinical/patient`, `dabney.moe/clinical/physician`, and
`dabney.moe/mental-health`.

Android tooling is provided declaratively by a dedicated flake dev shell,
`nix develop .#android`, which adds the Android SDK/NDK (via `androidenv`), a
JDK, the Android Rust targets, and the `ANDROID_HOME` / `NDK_HOME` / `JAVA_HOME`
environment. No manual SDK install or license clicking required:

```bash
nix develop .#android

# Emulator APK (16 KB ELF + zero-PHI manifest verified):
scripts/android-release.sh --apk --target x86_64

# Google Play Store release AAB (arm64-v8a + x86_64, 16 KB ELF verified):
scripts/android-release.sh --aab

# Generate 512x512 icon + 1024x500 Play Store feature graphic:
cargo run -p icon-gen -- --play-assets target/android-release/play-store-assets
```

See [`docs/google-play/README.md`](docs/google-play/README.md) for upload
keystore generation, Google Play Console Data Safety & Health Apps declarations,
and store listing copy, and [`docs/fdroid/README.md`](docs/fdroid/README.md)
for F-Droid, IzzyOnDroid, and self-hosted F-Droid repository publishing.

**Install on Android (`moe.dabney.demo`, 100% synthetic zero-PHI, MIT):**

- **Web Install Hub & QR Code**: [`https://dabney.moe/android`](https://dabney.moe/android)
- **Live Signed F-Droid Repository**: `https://codeberg.org/tyler274/fdroid/raw/branch/pages/repo` (SHA-256 fingerprint `BBA10E130E00DC452ED1B1C33D8D6074A098A5874F5DE3AE1390171550EDA5B7`)
- **Obtainium Config**: [`https://dabney.moe/obtainium-dabney-demo.json`](https://dabney.moe/obtainium-dabney-demo.json)
- **Direct Signed Universal APK**: [`moe.dabney.demo_1.apk`](https://codeberg.org/tyler274/fdroid/raw/branch/pages/repo/moe.dabney.demo_1.apk) (`arm64-v8a` + `x86_64`, 16 KB ELF aligned)

For validating a personal **Abbott Lingo** CGM export locally (`.csv`, Apple
Health `export.xml`, or Health Connect JSON) and for the future **Google Health
Connect** / **Apple HealthKit** personal build architecture (`moe.dabney.personal`,
kept separate from the zero-PHI public demo `moe.dabney.demo`), see
[`docs/health-connect-setup.md`](docs/health-connect-setup.md).

iOS still needs Xcode (macOS only): `cargo tauri ios init && cargo tauri ios dev`.

## Testing & CI

```bash
# Run the full gate locally (exactly what CI runs):
nix run .#ci          # nixpkgs-fmt, rustfmt, cargo-deny, clippy (-D warnings), and all tests
```

Tests live next to the code: synthetic chart and API tests in
`crates/clinical-workspace`, data-table invariants and SSR render assertions in
`crates/ui`, router and proxy tests in `crates/web`, and a Tauri-context parse
test in `src-tauri`. Because the `ui`/`web`/`tauri-ui` crates select mutually
exclusive Leptos features, each is tested with its own feature set (see the
`ci` runner in [`flake.nix`](flake.nix)).

CI/CD pipelines are provider-agnostic and available across multiple forges:

- **GitLab CI** ([`.gitlab-ci.yml`](.gitlab-ci.yml)): runs `nix run .#ci`,
  builds `.#serverImage`, and deploys on `main` via `scripts/deploy.sh`
  according to `DEPLOY_TARGET` (`oci`, `ssh`, `cloudrun`, `fly`).
- **Codeberg / Forgejo / Gitea Actions**
  ([`.forgejo/workflows/deploy.yml`](.forgejo/workflows/deploy.yml)): uses
  `https://code.forgejo.org/actions/checkout@v4` (zero GitHub dependency) and
  dispatches `scripts/deploy.sh` on `main`.
- **GitHub Actions** ([`.github/workflows/ci.yml`](.github/workflows/ci.yml)):
  parallel format, `cargo-deny`, clippy, tests, web/desktop/Android/OCI image
  builds, scoped via [`scripts/ci_scope.py`](scripts/ci_scope.py), plus an
  optional main-only Cloud Run rollout when `DOPPLER_TOKEN` is set.

The slim deploy shell (`nix develop .#deploy`) supplies `skopeo`, `openssh`,
`opentofu`, `gcloud`, and `doppler` without pulling the Rust or Tauri closure.

## Deployment (web)

`dabney.moe` deploys anywhere as a ~25 MB non-root OCI container built either
with Nix (`nix build .#serverImage`) or the standalone [`Dockerfile`](Dockerfile)
(`docker build` / `podman build`). Use [`scripts/deploy.sh`](scripts/deploy.sh)
(dry-run by default; pass `--push` to execute) to ship to any target:

```bash
# Any OCI registry (GitLab, Codeberg, Docker Hub, Quay, GCP AR, AWS ECR, Azure ACR):
./scripts/deploy.sh --target oci --image registry.gitlab.com/tyler274/dabney-moe/web --push

# Any Linux VPS over SSH (Docker or Podman + Caddy automatic Let's Encrypt HTTPS):
./scripts/deploy.sh --target ssh --ssh-host deploy@vps.example.com --domain dabney.moe --push

# Fly.io (using selfhost/fly.toml):
./scripts/deploy.sh --target fly --region iad --push

# Managed clouds (OpenTofu / Cloud Run / App Runner / Container Apps):
./scripts/deploy.sh --target cloudrun --project "$GCP_PROJECT_ID" --region us-central1 --push
./scripts/deploy.sh --target aws --region us-east-1 --push
./scripts/deploy.sh --target azure --region eastus --push
```

See [`docs/deploy.md`](docs/deploy.md) for the complete provider-agnostic guide,
[`selfhost/README.md`](selfhost/README.md) for VPS/Caddy and Fly.io details, and
[`terraform/README.md`](terraform/README.md) for multi-cloud OpenTofu modules.

`nix build .#serverImage` produces a **hardened, statically-linked
`x86_64-unknown-linux-musl`** image using the `mold` linker, fat LTO, and
Microsoft `mimalloc` in secure mode (`-DMI_SECURE`: guard pages, encrypted free
lists, randomized allocation) as the global allocator. Because the clean-room
`clinical-workspace` runs in-process with zero external weight files, the image
stays ~25 MB and glibc-free. The dynamically-linked glibc image remains
available as `nix build .#serverImageGlibc`.

## Styling

Tailwind CSS scans the Rust `view!` macros (`content` globs in
`tailwind.config.js`). The web build compiles it through `cargo-leptos`
(`tailwind-input-file`); the Trunk/Tauri build compiles it via a `pre_build`
hook in `Trunk.toml`. You can also run it manually with Bun:

```bash
bun run css:build   # style/input.css → style/output.css (minified)
```

## Regenerating the app icon

```bash
cargo run -p icon-gen            # writes src-tauri/icons/source.png
cargo tauri icon src-tauri/icons/source.png
```

## Notes

- `wasm-bindgen` is pinned (`=0.2.130`) to match the `wasm-bindgen-cli` shipped
  by the Nix dev shell. If you bump nixpkgs, keep the two in sync.
- Don't run a bare `cargo build --workspace`: the `web` (ssr/hydrate) and
  `tauri-ui` (csr) crates select mutually exclusive Leptos features. Use the
  `cargo leptos` / `cargo tauri` / `trunk` commands above, which build each crate
  with the right feature set.
