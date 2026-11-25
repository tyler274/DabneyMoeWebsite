# Google Play Data Safety, Health Apps & Permissions Declaration

Reference answers for Google Play Console → **Policy and programs → App content**
for `moe.dabney.demo` (**Dabney.moe Clinical Demo**).

## 1. Data Safety Form

- **Does your app collect or share any of the required user data types?**
  **No.**
- **Justification (verified by code & build checks):**
  - The Android app (`AppShellMode::ClinicalDemoApp`) disables the web cookie
    banner, Google Analytics 4, and client error reporting completely.
  - All clinical profiles, labs, CGM curves, medications, and visit drafts are
    compiled-in fictional synthetic fixtures (`crates/clinical-workspace`).
  - No user accounts, login, backend API calls, cloud sync, or third-party SDKs
    are invoked by the app.
  - Simulated "For My Doctor" sending updates in-memory UI state only and makes
    zero network requests.
  - `android:allowBackup="false"` and `android:usesCleartextTraffic="false"` are
    enforced in `AndroidManifest.xml`.

## 2. Privacy Policy

- **Privacy Policy URL**: `https://dabney.moe/privacy`
- The `/privacy` page explicitly names `moe.dabney.demo` (*Dabney.moe Clinical
  Demo Android App*), documents the zero-PHI / zero-data-collection guarantee,
  discloses that it is not medical advice or a medical device, and lists the
  developer contact (`tp@dabney.moe`).

## 3. Health Apps Declaration

Google Play requires every app in Health/Medical categories (or displaying
health UI) to complete the **Health Apps** declaration:

- **Does your app provide any health features?**
  - Declare accurately based on the current Play Console options:
    - Select **Healthcare Services and Management** *only* if describing the
      simulated patient-clinician record preview, **or** **My app does not have
      any health features** if Play's guidance classifies pure synthetic
      educational/portfolio software demos outside live clinical features.
    - When declaring under **Health Apps**, explicitly answer:
      - **Medical device / SaMD**: **No** — strictly a non-clinical software
        demonstration using fictional synthetic data.
      - **Clinical decision support / diagnosis / treatment**: **No** — displays
        prominent "100% Synthetic Demo · Zero PHI · Not Medical Advice" notice
        on every launch.
      - **Health Connect APIs**: **No** — zero `android.permission.health.*`
        permissions declared in the manifest (enforced by `scripts/android-release.sh`).
      - **Human subjects / real PHI**: **No** — zero real patient data.

## 4. App Access, Ads, Target Audience & Content Rating

- **App access**: All functionality is available without special access (no
  login, no paywall, no demo credentials needed).
- **Ads**: No, this app does not contain ads.
- **Target audience**: Ages 18 and over (adults evaluating healthcare software).
- **Government apps**: No.
- **Financial features**: None in the Android demo flow.
- **News app**: No.

## 5. Android Manifest Permissions Audit

`scripts/android-release.sh` audits the built APK/AAB manifest and fails the
build if any sensitive permission appears:

| Permission | Status | Reason |
| --- | --- | --- |
| `android.permission.INTERNET` | Present (Tauri baseline) | Allows optional external browser handoff when the user taps "Open dabney.moe" |
| `android.permission.health.*` (Health Connect) | **Absent (blocked by CI/script)** | Zero-PHI synthetic demo; never reads real device health data |
| `CAMERA`, `RECORD_AUDIO`, `LOCATION`, `CONTACTS`, `STORAGE` | **Absent (blocked by CI/script)** | Not needed and never requested |
| `android:usesCleartextTraffic` | **`false`** | HTTPS-only transport posture |
| `android:allowBackup` | **`false`** | Prevents Android cloud backup of local WebView state |
