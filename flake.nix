{
  description = "dabney.moe — Tyler Port's resume & freelance site (Leptos SSR) plus a Tauri multiplatform app suite";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    # Used to vendor the Cargo dependency closure so the SSR server image can
    # be built offline in the Nix sandbox.
    crane.url = "github:ipetkov/crane";
  };

  outputs =
    { self
    , nixpkgs
    , flake-utils
    , rust-overlay
    , crane
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

        # Terraform is unfree (BSL) in nixpkgs. Unfree tooling is permitted in
        # the dev shell, so pull it from a dedicated allowUnfree import rather
        # than flipping the whole default package set.
        # terraform = (import nixpkgs {
        #   inherit system overlays;
        #   config.allowUnfree = true;
        # }).terraform;

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
          # General scripting (CI helpers, ad-hoc tooling).
          python3
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

        # --- Web server container image -----------------------------------
        #
        # The dabney.moe web frontend is a Leptos SSR app: an Axum binary
        # (`web`) plus the hashed asset bundle under `target/site`. The deploy
        # unit is therefore a container, which we build reproducibly with Nix
        # and ship to any cloud's registry (see ./terraform).

        craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

        # Vendor the whole Cargo.lock closure into the store so `cargo leptos
        # build` runs without network access inside the sandbox.
        cargoVendorDir = craneLib.vendorCargoDeps { src = ./.; };

        # Tools cargo-leptos shells out to during a release build.
        leptosBuildTools = with pkgs; [
          cargo-leptos
          tailwindcss
          dart-sass
          wasm-bindgen-cli
          binaryen
          pkg-config
        ];

        # The release SSR build: `$out/bin/web` (server binary) plus
        # `$out/share/site` (hashed assets + the wasm `pkg/` dir).
        webServer = pkgs.stdenv.mkDerivation {
          pname = "dabney-web";
          version = "0.1.0";
          src = ./.;

          nativeBuildInputs = [ rustToolchain pkgs.removeReferencesTo ] ++ leptosBuildTools;

          configurePhase = ''
            runHook preConfigure
            export HOME="$TMPDIR"
            export CARGO_HOME="$TMPDIR/.cargo"
            mkdir -p "$CARGO_HOME"
            # crane's vendor dir ships a config.toml that redirects crates.io to
            # the vendored sources in the store; reuse it verbatim.
            cp ${cargoVendorDir}/config.toml "$CARGO_HOME/config.toml"
            export CARGO_NET_OFFLINE=true
            # Use the Nix-provided tailwind/sass binaries rather than letting
            # cargo-leptos download its own (the sandbox has no network).
            export LEPTOS_TAILWIND_VERSION="$(tailwindcss --help 2>/dev/null | head -n1 | awk '{print $NF}')"
            runHook postConfigure
          '';

          buildPhase = ''
            runHook preBuild
            # Offline is enforced via CARGO_NET_OFFLINE; cargo-leptos doesn't
            # accept cargo's --frozen passthrough.
            cargo leptos build --release
            runHook postBuild
          '';

          installPhase = ''
            runHook preInstall
            install -Dm755 target/release/web "$out/bin/web"
            mkdir -p "$out/share"
            cp -r target/site "$out/share/site"
            runHook postInstall
          '';

          # The release binary embeds source-path strings (panic locations)
          # pointing at the vendored crates and the toolchain's std sources.
          # They're never read at runtime, so scrub them to keep the runtime
          # closure (and thus the image) from dragging in the whole toolchain.
          postFixup = ''
            find "$out" -type f \
              -exec remove-references-to -t ${cargoVendorDir} -t ${rustToolchain} {} +
          '';
          disallowedReferences = [ cargoVendorDir rustToolchain ];

          doCheck = false;
        };

        # OCI image: copies the server + assets in, sets the LEPTOS_* runtime
        # env, and listens on 0.0.0.0:8080 (the port every cloud module wires
        # its ingress to). Build with `nix build .#serverImage` -> result is a
        # tarball you `skopeo copy docker-archive:result docker://<registry>`.
        serverImage = pkgs.dockerTools.buildLayeredImage {
          name = "dabney-web";
          tag = "latest";
          contents = [ webServer pkgs.cacert ];
          config = {
            Cmd = [ "${webServer}/bin/web" ];
            Env = [
              "LEPTOS_OUTPUT_NAME=dabney"
              "LEPTOS_SITE_ROOT=${webServer}/share/site"
              "LEPTOS_SITE_PKG_DIR=pkg"
              "LEPTOS_SITE_ADDR=0.0.0.0:8080"
              "LEPTOS_ENV=PROD"
              "SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
            ];
            ExposedPorts = { "8080/tcp" = { }; };
          };
        };
      in
      {
        devShells.default = pkgs.mkShell {
          # `skopeo` pushes the SSR image to a cloud registry; `tofu`
          # (OpenTofu, the MIT-licensed drop-in Terraform engine) drives the
          # infra under ./terraform. OpenTofu keeps this shell free/unfree-
          # prompt-free; the HCL is standard and `terraform` works identically.
          packages = [ rustToolchain ] ++ baseTools ++ tauriDeps ++ [ pkgs.skopeo pkgs.opentofu ];

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
        # The release SSR build (server binary + hashed assets).
        packages.web = webServer;
        # The deployable OCI image for the web server.
        packages.serverImage = serverImage;

        apps.ci = {
          type = "app";
          program = "${ci}/bin/ci";
        };

        formatter = pkgs.nixpkgs-fmt;
      }
    );
}
