{
  description = "dabney.moe — Tyler Port's resume & freelance site (Leptos SSR) plus a Tauri multiplatform app suite";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    { self
    , nixpkgs
    , flake-utils
    , rust-overlay
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };

        # Toolchain pinned via rust-toolchain.toml (includes the
        # wasm32-unknown-unknown target needed for Leptos hydrate/CSR builds).
        rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;

        # System libraries Tauri's webview needs on Linux desktops.
        tauriDeps = with pkgs; [
          webkitgtk_4_1
          gtk3
          libsoup_3
          cairo
          pango
          atkmm
          gdk-pixbuf
          glib
          librsvg
          openssl
        ];

        nativeTools = with pkgs; [
          rustToolchain
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
        ];
      in
      {
        devShells.default = pkgs.mkShell {
          packages = nativeTools ++ tauriDeps;

          # Make pkg-config-discovered libs (webkit/gtk) visible to the linker.
          PKG_CONFIG_PATH = pkgs.lib.makeSearchPathOutput "dev" "lib/pkgconfig" tauriDeps;
          LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath tauriDeps;

          shellHook = ''
            # Trunk maps $NO_COLOR onto a boolean (clap) flag, so a value like
            # "1" makes `trunk` (and thus `cargo tauri dev/build`) abort. Keep
            # the no-color intent but use a value clap can parse.
            if [ -n "''${NO_COLOR:-}" ]; then export NO_COLOR=true; fi

            echo "dabney.moe dev shell"
            echo "  $(rustc --version)"
            echo "  cargo-leptos $(cargo leptos --version 2>/dev/null | awk '{print $2}')"
            echo "  trunk $(trunk --version 2>/dev/null | awk '{print $2}')"
            echo "  tauri $(cargo tauri --version 2>/dev/null | awk '{print $NF}')"
            echo "  bun $(bun --version 2>/dev/null)"
            echo ""
            echo "  web (SSR):   cargo leptos watch"
            echo "  app (Tauri): cargo tauri dev"
          '';
        };

        formatter = pkgs.nixpkgs-fmt;
      }
    );
}
