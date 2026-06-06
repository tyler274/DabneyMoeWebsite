# Declarative Android SDK: a single platform/build-tools/NDK plus an x86_64
# emulator system image for CI smoke tests. Bump versions here and
# `nix flake update` to roll the whole Android toolchain.
#
# `pkgsAndroid` is a separate nixpkgs import with `allowUnfree` +
# `android_sdk.accept_license`, so the default web/desktop shell stays free and
# license-prompt-free.
{ pkgs, pkgsAndroid }:
let
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
in
{
  inherit
    androidBuildToolsVersion
    androidNdkVersion
    androidSdk
    androidSdkRoot
    androidNdkRoot
    ;
}
