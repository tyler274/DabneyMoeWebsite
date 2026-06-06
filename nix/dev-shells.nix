# Developer shells: the default web/desktop shell and the Android cross-compile
# / emulator shell. Each supplies its own Rust toolchain but shares the base
# tooling and Tauri dependency closure.
{ pkgs
, rustToolchain
, rustToolchainAndroid
, baseTools
, tauriDeps
, pkgConfigPath
, ldLibraryPath
, android
}:
{
  default = pkgs.mkShell {
    # `skopeo` pushes the SSR image to a cloud registry; `tofu` (OpenTofu, the
    # MIT-licensed drop-in Terraform engine) drives the infra under ./terraform.
    # OpenTofu keeps this shell free/unfree-prompt-free; the HCL is standard and
    # `terraform` works identically. `mold` is here for local static builds.
    packages = [ rustToolchain ] ++ baseTools ++ tauriDeps
      ++ [ pkgs.skopeo pkgs.opentofu pkgs.mold ];

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

  # Android cross-compilation / emulator shell. Provides the declarative SDK +
  # NDK, a JDK, the Android Rust targets, and the env vars Tauri, Gradle, and
  # the NDK toolchain expect.
  android = pkgs.mkShell {
    packages =
      [ rustToolchainAndroid ]
      ++ baseTools
      ++ tauriDeps
      ++ [ pkgs.jdk17 android.androidSdk ];

    PKG_CONFIG_PATH = pkgConfigPath;
    LD_LIBRARY_PATH = ldLibraryPath;

    ANDROID_HOME = android.androidSdkRoot;
    ANDROID_SDK_ROOT = android.androidSdkRoot;
    ANDROID_NDK_ROOT = android.androidNdkRoot;
    # Tauri's mobile tooling reads NDK_HOME specifically.
    NDK_HOME = android.androidNdkRoot;
    JAVA_HOME = pkgs.jdk17.home;
    # Point Gradle at the Nix-provided aapt2 (its bundled one is a
    # dynamically-linked binary that won't run on NixOS).
    GRADLE_OPTS = "-Dorg.gradle.project.android.aapt2FromMavenOverride=${android.androidSdkRoot}/build-tools/${android.androidBuildToolsVersion}/aapt2";

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
}
