# Declarative Android SDK: a single platform/build-tools/NDK plus an x86_64
# emulator system image for CI smoke tests. Bump versions here and
# `nix flake update` to roll the whole Android toolchain.
#
# `pkgsAndroid` is a separate nixpkgs import with `allowUnfree` +
# `android_sdk.accept_license`, so the default web/desktop shell stays free and
# license-prompt-free.
{ pkgsAndroid }:
let
  androidBuildToolsVersion = "34.0.0";
  # The version used for signing tools (apksigner, zipalign) and exposed on
  # PATH. Must be present in buildToolsVersions below.
  androidSigningToolsVersion = "37.0.0";
  androidNdkVersion = "26.1.10909125";
  androidComposition = pkgsAndroid.androidenv.composeAndroidPackages {
    # Keep platform 34 for the system image; add 36 because the Tauri-generated
    # project targets compileSdk/targetSdk 36 (AGP default as of Tauri 2.x).
    platformVersions = [ "34" "36" ];
    # 34.0.0 is what we pin for aapt2 (via GRADLE_OPTS); 37.0.0 is what we
    # expose on PATH for apksigner/zipalign and what we tell Gradle to use so
    # it never tries to auto-download AGP's default 35.0.0 into the read-only
    # Nix store.
    buildToolsVersions = [ androidBuildToolsVersion androidSigningToolsVersion ];
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
in
{
  inherit
    androidBuildToolsVersion
    androidSigningToolsVersion
    androidNdkVersion
    androidSdk
    androidSdkRoot
    androidNdkRoot
    ;
}
