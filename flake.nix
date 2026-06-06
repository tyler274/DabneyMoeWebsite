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

        # The Android SDK/NDK are unfree and require accepting Google's SDK
        # license; gate them behind a dedicated nixpkgs import so the default
        # web/desktop shell stays free and license-prompt-free.
        pkgsAndroid = import nixpkgs {
          inherit system overlays;
          config = {
            allowUnfree = true;
            android_sdk.accept_license = true;
          };
        };

        # Toolchain pinned via rust-toolchain.toml (includes the
        # wasm32-unknown-unknown target needed for Leptos hydrate/CSR builds).
        rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;

        # The same pinned toolchain plus the Android cross-compilation targets
        # Tauri needs. Layered separately so web-only contributors don't pay to
        # download Android std libraries in the default shell.
        rustToolchainAndroid = rustToolchain.override {
          targets = [
            "wasm32-unknown-unknown"
            "aarch64-linux-android"
            "armv7-linux-androideabi"
            "i686-linux-android"
            "x86_64-linux-android"
          ];
        };

        # System libraries Tauri's webview needs on Linux desktops.
        tauriDeps = with pkgs; [
          webkitgtk_4_1
          gtk3
          libsoup_3
          cairo
          pango
          harfbuzz
          atk
          atkmm
          gdk-pixbuf
          glib
          librsvg
          openssl
          # `libdbus-sys` (pulled in transitively by Tauri on Linux) links
          # against the system D-Bus library.
          dbus
          # `Requires.private` of the gtk `.pc` files that aren't propagated.
          zlib
        ];

        # Everything in the dev shell except the Rust toolchain itself (so the
        # default and Android shells can each supply their own toolchain).
        baseTools = with pkgs; [
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

        # Declarative Android SDK: a single platform/build-tools/NDK plus an
        # x86_64 emulator system image for CI smoke tests. Bump versions here
        # and `nix flake update` to roll the whole Android toolchain.
        androidBuildToolsVersion = "34.0.0";
        androidNdkVersion = "26.1.10909125";
        androidComposition = pkgsAndroid.androidenv.composeAndroidPackages {
          platformVersions = [ "34" ];
          buildToolsVersions = [ androidBuildToolsVersion ];
          includeNDK = true;
          ndkVersions = [ androidNdkVersion ];
          includeEmulator = true;
          includeSystemImages = true;
          systemImageTypes = [ "google_apis" ];
          abiVersions = [ "x86_64" ];
        };
        androidSdk = androidComposition.androidsdk;
        androidSdkRoot = "${androidSdk}/libexec/android-sdk";
        androidNdkRoot = "${androidSdkRoot}/ndk/${androidNdkVersion}";

        # The gtk/webkit `.pc` files declare transitive `Requires:` (pango ->
        # harfbuzz, gdk -> zlib, ...). Close over propagated build inputs so
        # every required `.pc` is discoverable without hand-listing each one.
        tauriDepsClosure = pkgs.lib.closePropagation tauriDeps;

        # Make pkg-config-discovered libs (webkit/gtk) visible to the linker.
        # Some `.pc` files live in share/pkgconfig (e.g. zlib) rather than
        # lib/pkgconfig, so search both.
        pkgConfigPath = pkgs.lib.concatStringsSep ":" [
          (pkgs.lib.makeSearchPathOutput "dev" "lib/pkgconfig" tauriDepsClosure)
          (pkgs.lib.makeSearchPathOutput "dev" "share/pkgconfig" tauriDepsClosure)
        ];
        ldLibraryPath = pkgs.lib.makeLibraryPath tauriDepsClosure;

        # Single source of truth for the CI gate: format, lint, and test each
        # crate with the right (mutually exclusive) Leptos feature set. Run it
        # locally with `nix run .#ci`; CI runs the very same script.
        ci = pkgs.writeShellApplication {
          name = "ci";
          runtimeInputs = [ rustToolchain ] ++ baseTools ++ tauriDeps;
          text = ''
            export PKG_CONFIG_PATH="${pkgConfigPath}"
            export LD_LIBRARY_PATH="${ldLibraryPath}"

            echo "==> rustfmt (--check)"
            cargo fmt --all --check

            echo "==> clippy: ui (ssr)"
            cargo clippy -p ui --features ssr -- -D warnings
            echo "==> clippy: web (ssr)"
            cargo clippy -p web --features ssr -- -D warnings
            echo "==> clippy: tauri-ui (csr, wasm32)"
            cargo clippy -p tauri-ui --target wasm32-unknown-unknown -- -D warnings
            echo "==> clippy: dabney-app"
            cargo clippy -p dabney-app -- -D warnings
            echo "==> clippy: icon-gen"
            cargo clippy -p icon-gen -- -D warnings

            echo "==> test: ui (data + ssr render)"
            cargo test -p ui --features ssr
            echo "==> test: web (ssr route list)"
            cargo test -p web --features ssr
            echo "==> test: dabney-app (tauri context)"
            cargo test -p dabney-app

            echo "==> CI checks passed"
          '';
        };
      in
      {
        devShells.default = pkgs.mkShell {
          packages = [ rustToolchain ] ++ baseTools ++ tauriDeps;

          PKG_CONFIG_PATH = pkgConfigPath;
          LD_LIBRARY_PATH = ldLibraryPath;

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
            echo "  CI gate:     nix run .#ci"
            echo "  Android:     nix develop .#android"
          '';
        };

        # Android cross-compilation / emulator shell. Provides the declarative
        # SDK + NDK, a JDK, the Android Rust targets, and the env vars Tauri,
        # Gradle, and the NDK toolchain expect.
        devShells.android = pkgs.mkShell {
          packages =
            [ rustToolchainAndroid ]
            ++ baseTools
            ++ tauriDeps
            ++ [ pkgs.jdk17 androidSdk ];

          PKG_CONFIG_PATH = pkgConfigPath;
          LD_LIBRARY_PATH = ldLibraryPath;

          ANDROID_HOME = androidSdkRoot;
          ANDROID_SDK_ROOT = androidSdkRoot;
          ANDROID_NDK_ROOT = androidNdkRoot;
          # Tauri's mobile tooling reads NDK_HOME specifically.
          NDK_HOME = androidNdkRoot;
          JAVA_HOME = pkgs.jdk17.home;
          # Point Gradle at the Nix-provided aapt2 (its bundled one is a
          # dynamically-linked binary that won't run on NixOS).
          GRADLE_OPTS = "-Dorg.gradle.project.android.aapt2FromMavenOverride=${androidSdkRoot}/build-tools/${androidBuildToolsVersion}/aapt2";

          shellHook = ''
            if [ -n "''${NO_COLOR:-}" ]; then export NO_COLOR=true; fi

            echo "dabney.moe Android dev shell"
            echo "  $(rustc --version)"
            echo "  ANDROID_HOME=$ANDROID_HOME"
            echo "  NDK_HOME=$NDK_HOME"
            echo "  JAVA_HOME=$JAVA_HOME"
            echo ""
            echo "  init:  cargo tauri android init"
            echo "  build: cargo tauri android build --apk"
            echo "  dev:   cargo tauri android dev"
          '';
        };

        packages.ci = ci;

        apps.ci = {
          type = "app";
          program = "${ci}/bin/ci";
        };

        formatter = pkgs.nixpkgs-fmt;
      }
    );
}
