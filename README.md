# dabney.moe

[![CI](https://github.com/tyler274/DabneyMoeWebsite/actions/workflows/ci.yml/badge.svg)](https://github.com/tyler274/DabneyMoeWebsite/actions/workflows/ci.yml)

Tyler Port's résumé and freelance software-engineering site, plus a multiplatform
app suite. The web frontend is a **Leptos** SSR app (Axum + WASM hydration); the
same UI is reused inside a **Tauri v2** shell (desktop + mobile) as a client-side
rendered build. Everything is driven by a **Nix flake** dev shell, with **Bun**
for JS tooling.

## Architecture

The page content lives once, in the shared `ui` crate. The web server and the
Tauri shell are thin wrappers around it.

```
crates/ui          shared Leptos components + App root (features: ssr/hydrate/csr)
crates/web         cargo-leptos package: Axum SSR binary + WASM hydrate cdylib
crates/tauri-ui    Trunk CSR binary that mounts ui::App, output to dist/
src-tauri          Tauri v2 app (desktop + mobile), serves dist/
```

| Path | Purpose |
| --- | --- |
| `flake.nix` / `rust-toolchain.toml` | Dev shell + pinned Rust toolchain (incl. `wasm32-unknown-unknown`). |
| `Cargo.toml` | Workspace + `[[workspace.metadata.leptos]]` config. |
| `tailwind.config.js`, `style/input.css` | Tailwind CSS source. |
| `package.json` | Bun-managed tooling (Tailwind CLI + scripts). |
| `public/` | Static assets served at the site root (`resume.pdf`, `/coda/` static export + WASM SQLite demo via `scripts/sync-coda-site.sh`, `/vtuber/demo/` model viewer via `scripts/sync-ayagami-demo.sh`). |
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
to resolve the matching content-hashed JS, WASM, and CSS filenames. The Nix images
package this manifest and enable hash resolution automatically.

### Model viewer (`/vtuber/demo/`)

The `/vtuber` page links to a model viewer served from `public/vtuber/demo/`.
It is an unmodified web build of the demo app from
[Ayagami](https://github.com/AyagamiDev/ayagami) (MIT or Apache-2.0), an
open-source 2D puppet renderer, at the commit pinned in
[`scripts/sync-ayagami-demo.sh`](scripts/sync-ayagami-demo.sh). The script
fetches that commit, builds it with the dev shell's `trunk` under the
`/vtuber/demo/` base path against the committed
`scripts/ayagami-demo.Cargo.lock`, and writes the result beside Ayagami's
copyright and licence notices. The output is checked in, like `public/coda/`,
so the web build and the server image need neither network access nor a second
toolchain.

```bash
./scripts/sync-ayagami-demo.sh                        # rebuild at the pinned commit
AYAGAMI_UPDATE_LOCK=1 ./scripts/sync-ayagami-demo.sh  # after changing AYAGAMI_REV
```

The viewer ships with no model files; a visitor opens their own, and it is
read in their browser. Keep it that way: models belong to their artists and
carry their own licences. Ayagami is compatible with the Live2D model format
and is not affiliated with or endorsed by Live2D Inc.

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

The project is wired for mobile (lib crate-types, mobile entry point, and icon
assets are already generated under `src-tauri/icons/android` and `.../ios`).

Android tooling is provided declaratively by a dedicated flake dev shell,
`nix develop .#android`, which adds the Android SDK/NDK (via `androidenv`), a
JDK, the Android Rust targets, and the `ANDROID_HOME` / `NDK_HOME` / `JAVA_HOME`
environment. No manual SDK install or license clicking required:

```bash
nix develop .#android

cargo tauri android init   # regenerates src-tauri/gen/android (git-ignored)
cargo tauri android build --apk
cargo tauri android dev
```

iOS still needs Xcode (macOS only): `cargo tauri ios init && cargo tauri ios dev`.

## Testing & CI

```bash
# Run the full gate locally (exactly what CI runs):
nix run .#ci          # rustfmt --check, cargo-deny, clippy (-D warnings), and all tests
```

Tests live next to the code: data-table invariants and SSR render assertions in
`crates/ui`, a router smoke test in `crates/web`, and a Tauri-context parse test
in `src-tauri`. Because the `ui`/`web`/`tauri-ui` crates select mutually
exclusive Leptos features, each is tested with its own feature set (see the
`ci` runner in [`flake.nix`](flake.nix)).

GitHub Actions ([`.github/workflows/ci.yml`](.github/workflows/ci.yml)) runs the
same checks plus web, desktop, and Android (APK) builds on every push/PR, then a
main-only deploy job that ships the web server to **Google Cloud Run** via a
Nix-built container image (infra is provisioned separately with
Terraform/OpenTofu, see [`terraform/`](terraform/)). The deploy no-ops until the
`DOPPLER_TOKEN` secret is set. Play Store publishing is still a placeholder
pending Play Console account setup.

How the workflow is laid out:

- **Parallel jobs.** After a short scope and CI-image check, format, clippy,
  tests, and every build (web, desktop, Android, server image) run side by
  side. Deploy waits for format, clippy, tests, the web build, and the image
  build, and is skipped if any of them fails, is cancelled, or is skipped.
- **Dependency guardrails.** `cargo deny` checks RustSec advisories, licences,
  bans, and crate sources against [`deny.toml`](deny.toml). It runs in its own
  job without the CI image and does not gate the deploy, so a newly published
  advisory cannot block shipping otherwise unchanged code.
- **Rust build cache.** Clippy, tests, the web build, and the desktop build
  restore `target/` from the Actions cache, keyed by job, `rustc` version,
  `flake.lock`, and `Cargo.lock`. A changed `Cargo.lock` falls back to the
  newest cache for the same `flake.lock`; a changed `flake.lock` starts cold,
  because cached build scripts and proc macros link against the old pin's
  glibc store path and cannot run under the new one. Only runs on `main` save it, after
  [`scripts/ci-trim-target.sh`](scripts/ci-trim-target.sh) strips workspace
  outputs so only dependency artifacts are stored. Pull requests read the
  cache but never write it.
- **Scope.** [`scripts/ci_scope.py`](scripts/ci_scope.py) skips all builds and
  the deploy when a change only touches Markdown, `terraform/`, or `selfhost/`,
  and skips the Android build unless its inputs changed. Manual runs build
  everything.
- **Concurrency.** A new commit cancels the previous run of the same pull
  request only. Runs on `main` are never cancelled, and deploys run one at a
  time.
- **Deploy shell.** The deploy job uses the slim `nix develop .#deploy` shell
  (`gcloud`, `skopeo`, `doppler`) instead of the full dev shell.

## Deployment (web)

The site is a Leptos SSR server, so it deploys as a container. The image is
built reproducibly with Nix and infra is described with Terraform-compatible HCL
(driven by OpenTofu in the dev shell):

```bash
nix build .#serverImage          # OCI image tarball at ./result
cd terraform/gcp                 # or terraform/aws, terraform/azure
tofu init && tofu apply          # two-phase: registry, push image, then service
```

See [`terraform/README.md`](terraform/README.md) for the full multi-cloud
workflow (GCP Cloud Run, AWS App Runner, Azure Container Apps) and custom-domain
setup.

`nix build .#serverImage` produces a **hardened, statically-linked
`x86_64-unknown-linux-musl`** build: the `mold` linker plus fat LTO, and
Microsoft `mimalloc` in secure mode (`-DMI_SECURE`: guard pages, encrypted free
lists, randomized allocation) as the global allocator. The result is glibc-free
(zero runtime store references, ~18.7 MB image). The original dynamically-linked
glibc image is still available as `nix build .#serverImageGlibc` if you ever need
it.

### Self-hosting with HTTPS (Caddy + Let's Encrypt)

The managed targets above terminate TLS themselves. To run the site on your own
host/VPS with HTTPS, [`selfhost/`](selfhost/) provides a Docker Compose stack
where a Caddy reverse proxy auto-issues and renews a Let's Encrypt certificate
for `dabney.moe`, redirects HTTP→HTTPS, and keeps port 80 open for the ACME
`http-01` challenge. See [`selfhost/README.md`](selfhost/README.md).

A separate, heavier
[Android emulator smoke test](.github/workflows/android-emulator.yml) boots a
Nix-provided AVD and installs the APK; it runs nightly and on demand (not as a
per-push gate). It relies on `/dev/kvm`, which GitHub-hosted Linux runners
expose.

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
