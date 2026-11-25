# Publishing Dabney.moe Clinical Demo (`moe.dabney.demo`) to Google Play

This guide covers building, signing, verifying, and publishing the zero-PHI
**Dabney.moe Clinical Demo** Android app (`moe.dabney.demo`) to Google Play.

All patient profiles, labs, CGM summaries, and visit notes bundled in this app
are **100% fictional synthetic examples containing zero Protected Health
Information (PHI)**. The app runs offline in Rust/WebAssembly inside Tauri v2,
collects zero user data, and requests zero Health Connect or sensitive device
permissions.

## Companion In-Browser Demos

Every demo inside the Android app is compiled from the exact same Rust/Leptos
UI crate (`crates/ui` + `crates/clinical-workspace`) that powers `dabney.moe`:

| Demo | In-Browser URL | Android App Tab |
| --- | --- | --- |
| Patient Experience Demo (`You` + `For My Doctor`) | `https://dabney.moe/clinical/patient` | **Patient** (default launch screen) |
| Physician Workspace Demo (citations & drafts) | `https://dabney.moe/clinical/physician` | **Physician** |
| Mental Health & Commercial CGM Analytics | `https://dabney.moe/mental-health` | **Mood & CGM** |
| Clinical Suite Overview | `https://dabney.moe/clinical` | **Overview** |
| Privacy Policy (Zero-PHI Android disclosure) | `https://dabney.moe/privacy` | **Privacy** |

## 1. One-Time Local Upload Keystore Setup

Google Play uses **Play App Signing**: you sign uploads locally with an
**upload key**, and Google manages the store distribution key.

Generate a 4096-bit PKCS12 upload keystore **outside this Git repository**
(never commit `.jks`, `.keystore`, or `keystore.properties`):

```bash
nix develop .#android -c bash -c '
  export ANDROID_KEY_ALIAS="dabney-upload"
  read -r -s -p "Keystore password: " ANDROID_KEYSTORE_PASSWORD; echo
  export ANDROID_KEYSTORE_PASSWORD
  export ANDROID_KEY_PASSWORD="$ANDROID_KEYSTORE_PASSWORD"
  scripts/android-release.sh --generate-keystore "$HOME/.config/dabney-android/upload-keystore.jks"
'
```

Back up `~/.config/dabney-android/upload-keystore.jks` and its password in a
password manager (or Doppler).

## 2. Build & Verify the Play-Ready Signed Release AAB

Build the multi-ABI release Android App Bundle (`arm64-v8a` + `x86_64`, or add
`--all-abis` to also include `armeabi-v7a` and `x86`):

```bash
nix develop .#android -c bash -c '
  export ANDROID_KEYSTORE_PATH="$HOME/.config/dabney-android/upload-keystore.jks"
  export ANDROID_KEY_ALIAS="dabney-upload"
  read -r -s -p "Keystore password: " ANDROID_KEYSTORE_PASSWORD; echo
  export ANDROID_KEYSTORE_PASSWORD
  export ANDROID_KEY_PASSWORD="$ANDROID_KEYSTORE_PASSWORD"
  scripts/android-release.sh --aab
'
```

`scripts/android-release.sh` automatically verifies:

1. **Application ID & manifest hygiene**: confirms `moe.dabney.demo`,
   `android:usesCleartextTraffic="false"`, `android:allowBackup="false"`, and
   fails closed if any Health Connect (`android.permission.health.*`) or
   sensitive device permissions are present.
2. **16 KB ELF page-size compliance (Android 15+ / Play Store requirement)**:
   runs `llvm-readelf -lW` across every 64-bit `.so` inside the `.aab` to verify
   every `LOAD` segment has alignment `>= 0x4000` (16384 bytes) and
   `useLegacyPackaging = false`.
3. **Cryptographic signature**: verifies the signed `.aab` with
   `jarsigner -verify`.

The output bundle and checksum are written to:
- `target/android-release/dabney-demo-release.aab`
- `target/android-release/dabney-demo-release.aab.sha256`

## 3. Emulator Verification (Before Uploading)

Build the `x86_64` APK and smoke-test on a local Android emulator:

```bash
nix develop .#android -c bash -c '
  scripts/android-release.sh --apk --target x86_64
'
```

Install onto an emulator (`adb install -r <apk>`) and confirm:
- App opens straight to **Patient Experience Demo** with the emerald
  `100% Synthetic Demo · Zero PHI` notice.
- Elephant logo renders crisply with no broken images.
- Switching profiles, toggling **Include for doctor**, switching between
  **Your Health** and **For My Doctor**, and tapping the bottom tabs
  (**Patient**, **Physician**, **Mood & CGM**, **Overview**) all work offline.

## 4. First Release in Google Play Console (Manual Upload Required Once)

Google Play's Android Publisher API cannot create a new app or accept the very
first AAB upload; complete the initial setup in Play Console once:

1. **Create App** in Google Play Console:
   - App name: `Dabney.moe Clinical Demo`
   - Default language: `English (United States)`
   - App or game: `App` · Free or paid: `Free`
   - Confirm Developer Program Policies & US export laws.
2. **Complete App Content Declarations** (exact answers in
   [data-safety-and-health-declaration.md](data-safety-and-health-declaration.md)):
   - Privacy policy URL: `https://dabney.moe/privacy`
   - Data Safety: **No user data collected or shared**
   - Health Apps declaration: educational / zero-PHI product demonstration
     (not a medical device, no diagnosis/treatment, no Health Connect)
   - Ads: **No, my app does not contain ads**
   - App access: **All functionality is available without special access** (no login)
   - Target audience: **18 and over**
   - Content rating (IARC): complete questionnaire (typically `Everyone`)
3. **Fill Store Listing** (copy ready in [store-listing.md](store-listing.md)):
   - Upload the 512×512 PNG app icon, 1024×500 feature graphic, and phone
     screenshots.
4. **Enroll in Play App Signing & Upload First `.aab`**:
   - Start with **Internal testing** (instant verification on your own Android
     phone via Play Store link), then **Closed testing** (note: personal Play
     Console accounts created after Nov 2023 require 12 opted-in closed testers
     for 14 continuous days before Production review), then **Production**.

## 5. Automating Subsequent Releases in CI

Once the first AAB has been uploaded manually and you have created a Google
Play API service account (with *Release to testing tracks* / *Release to
production* permissions in Play Console → *Users and permissions*):

Add these secrets to your CI environment (GitHub Actions / Gitea Actions):

- `ANDROID_KEYSTORE_BASE64`: `base64 -w0 ~/.config/dabney-android/upload-keystore.jks`
- `ANDROID_KEY_ALIAS`: `dabney-upload`
- `ANDROID_KEYSTORE_PASSWORD`
- `ANDROID_KEY_PASSWORD`
- `GOOGLE_PLAY_SERVICE_ACCOUNT_JSON`: service account JSON key

Remember to increment `bundle.android.versionCode` (and `version`) in
`src-tauri/tauri.android.conf.json` for every subsequent Play Console upload.
