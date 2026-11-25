# Publishing Dabney.moe Clinical Demo (`moe.dabney.demo`) to F-Droid & IzzyOnDroid

`moe.dabney.demo` is 100% open source under the **MIT License**, built from pure Rust/WebAssembly (`crates/ui`, `crates/clinical-workspace`, `src-tauri`) with **zero proprietary SDKs**, zero Google Play Services, zero analytics/trackers, zero Health Connect permissions, and zero PHI.

> **Flavor boundary:** Only `moe.dabney.demo` is submitted to F-Droid / IzzyOnDroid. The opt-in personal sideload flavor (`moe.dabney.personal`) links Google's proprietary `com.google.mlkit:genai-prompt` artifact for Android AICore and is intentionally excluded from F-Droid.

## Shared Fastlane Metadata (`fastlane/metadata/android/en-US/`)

Both official F-Droid (`f-droid.org`), IzzyOnDroid, and Google Play ingest the Fastlane directory committed in this repository:

- `fastlane/metadata/android/en-US/title.txt`
- `fastlane/metadata/android/en-US/short_description.txt`
- `fastlane/metadata/android/en-US/full_description.txt`
- `fastlane/metadata/android/en-US/changelogs/1.txt` (`versionCode = 1`)
- `fastlane/metadata/android/en-US/images/icon.png` (512 × 512)
- `fastlane/metadata/android/en-US/images/featureGraphic.png` (1024 × 500)
- `fastlane/metadata/android/en-US/images/phoneScreenshots/{1,2,3,4}.png`

When bumping a release, increment `bundle.android.versionCode` and `version` in `src-tauri/tauri.android.conf.json`, add `fastlane/metadata/android/en-US/changelogs/<versionCode>.txt`, update `docs/fdroid/moe.dabney.demo.yml`, and push a matching Git tag `v<version>`.

## Track 1: Official F-Droid (`f-droid.org`)

Official F-Droid compiles `moe.dabney.demo` from source using `docs/fdroid/moe.dabney.demo.yml` and signs it with F-Droid's release key:

1. Ensure the release commit is merged to `main` and tagged `v1.0.0` on GitHub (`https://github.com/tyler274/DabneyMoeWebsite`).
2. Fork [`https://gitlab.com/fdroid/fdroiddata`](https://gitlab.com/fdroid/fdroiddata) on GitLab.
3. Copy `docs/fdroid/moe.dabney.demo.yml` into `metadata/moe.dabney.demo.yml` inside your `fdroiddata` fork.
4. Validate locally with `fdroidserver` if installed:
   ```bash
   fdroid readmeta
   fdroid lint moe.dabney.demo
   fdroid build -v -l moe.dabney.demo
   ```
5. Push your branch to your GitLab fork and open a Merge Request against `fdroid/fdroiddata` (`master`) using the **New App** template.

## Track 2: Fast-Track F-Droid via IzzyOnDroid (`apt.izzysoft.de/fdroid`)

IzzyOnDroid tracks signed universal release APKs attached to GitHub Releases directly and ingests `fastlane/metadata/android/en-US/` automatically (typically live within 24 hours after initial review):

1. Build and verify the signed universal release APK (`arm64-v8a` + `x86_64`, 16 KB ELF aligned):
   ```bash
   nix develop .#android -c bash -c '
     export ANDROID_KEYSTORE_PATH="$HOME/.config/dabney-android/upload-keystore.jks"
     export ANDROID_KEY_ALIAS="dabney-upload"
     read -r -s -p "Keystore password: " ANDROID_KEYSTORE_PASSWORD; echo
     export ANDROID_KEYSTORE_PASSWORD
     export ANDROID_KEY_PASSWORD="$ANDROID_KEYSTORE_PASSWORD"
     scripts/android-release.sh --apk --target aarch64 --target x86_64
   '
   ```
2. Publish GitHub Release `v1.0.0` attaching `dabney-demo-release.apk` (and `dabney-demo-release.apk.sha256`).
3. Open a New App inclusion issue in IzzyOnDroid pointing to `https://github.com/tyler274/DabneyMoeWebsite` (MIT License, Fastlane metadata enabled, zero trackers/blobs).
