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

  # The per-system wiring lives in ./nix/*.nix; this file just imports those
  # modules and threads the shared values (pkgs, toolchains, tauri deps) between
  # them, then maps the results onto flake outputs.
  outputs =
    { nixpkgs
    , flake-utils
    , rust-overlay
    , crane
    , ...
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

        toolchains = import ./nix/toolchains.nix {
          inherit pkgs;
          rustToolchainFile = ./rust-toolchain.toml;
        };
        inherit (toolchains) rustToolchain rustToolchainAndroid rustToolchainMusl;

        tauri = import ./nix/tauri-deps.nix { inherit pkgs; };
        inherit (tauri) tauriDeps pkgConfigPath ldLibraryPath;

        baseTools = import ./nix/base-tools.nix { inherit pkgs; };

        android = import ./nix/android.nix { inherit pkgs pkgsAndroid; };

        ci = import ./nix/ci.nix {
          inherit pkgs rustToolchain baseTools tauriDeps pkgConfigPath ldLibraryPath;
        };

        web = import ./nix/web-server.nix {
          inherit pkgs crane rustToolchain rustToolchainMusl;
          src = ./.;
        };

        shells = import ./nix/dev-shells.nix {
          inherit pkgs rustToolchain rustToolchainAndroid baseTools tauriDeps
            pkgConfigPath ldLibraryPath android;
        };
      in
      {
        devShells.default = shells.default;
        devShells.android = shells.android;

        packages.ci = ci;
        # The release SSR build (server binary + hashed assets). Default is the
        # hardened static-musl build; the glibc build is kept as a fallback.
        packages.web = web.webServerStatic;
        packages.webGlibc = web.webServer;
        # The deployable OCI image for the web server. `serverImage` is the
        # hardened static-musl default; `serverImageGlibc` is the fallback.
        packages.serverImage = web.serverImage;
        packages.serverImageGlibc = web.serverImageGlibc;

        apps.ci = {
          type = "app";
          program = "${ci}/bin/ci";
        };

        formatter = pkgs.nixpkgs-fmt;
      }
    );
}
