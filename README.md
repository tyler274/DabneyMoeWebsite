# dabney.moe

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
| `public/` | Static assets served at the site root (e.g. `resume.pdf`). |
| `crates/icon-gen` | Small Rust dev tool that regenerates the source app icon. |

## Prerequisites

Everything is provided by the Nix flake — you only need Nix with flakes enabled
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

## Web (the dabney.moe homepage)

```bash
# Live-reloading SSR dev server on http://127.0.0.1:3039
cargo leptos watch

# Production build → target/site (server binary at target/release/web)
cargo leptos build --release
```

Serving the release build: run `target/release/web` with the `LEPTOS_*`
environment variables (see `[[workspace.metadata.leptos]]` in `Cargo.toml`), e.g.
`LEPTOS_OUTPUT_NAME=dabney LEPTOS_SITE_ROOT=target/site LEPTOS_SITE_PKG_DIR=pkg
LEPTOS_SITE_ADDR=0.0.0.0:3039 ./target/release/web`.

## Apps (Tauri)

```bash
# Desktop dev (launches the webview against the Trunk dev server)
cargo tauri dev

# Desktop build — compile only (no OS packaging)
cargo tauri build --no-bundle

# Desktop build — full installers (deb/rpm/AppImage on Linux). Requires the
# relevant OS packaging tools to be available.
cargo tauri build
```

The Tauri frontend is built by Trunk via the root `Trunk.toml`, configured
as Tauri's `beforeDevCommand` / `beforeBuildCommand`, emitting to `dist/`.

### Mobile (Android / iOS)

The project is wired for mobile (lib crate-types, mobile entry point, and icon
assets are already generated under `src-tauri/icons/android` and `.../ios`). To
enable a platform once its SDK is installed:

```bash
cargo tauri android init   # requires Android SDK + NDK
cargo tauri ios init       # requires Xcode (macOS only)

cargo tauri android dev
cargo tauri ios dev
```

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

- `wasm-bindgen` is pinned (`=0.2.121`) to match the `wasm-bindgen-cli` shipped
  by the Nix dev shell. If you bump nixpkgs, keep the two in sync.
- Don't run a bare `cargo build --workspace`: the `web` (ssr/hydrate) and
  `tauri-ui` (csr) crates select mutually exclusive Leptos features. Use the
  `cargo leptos` / `cargo tauri` / `trunk` commands above, which build each crate
  with the right feature set.
