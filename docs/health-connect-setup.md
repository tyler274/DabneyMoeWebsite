# Abbott Lingo Validation Today & Future Google Health Connect / Apple HealthKit Setup

This guide covers:

1. **Immediate personal use (available today):** Loading and validating real
   **Abbott Lingo** continuous glucose monitor (CGM) exports locally in your
   browser, in the Android app, or via the offline CLI — with **zero PHI ever
   committed to Git or uploaded to a server**.
2. **Future native integration:** Clean-room architecture for **Android Google
   Health Connect** (`BloodGlucoseRecord`) and **iOS Apple HealthKit**
   (`HKQuantityTypeIdentifierBloodGlucose`) using a separate opt-in personal
   build flavor so the public Google Play demo (`moe.dabney.demo`) stays
   strictly zero-PHI and unblocked.

---

## 1. Validate Your Personal Abbott Lingo CGM Today (100% Local)

### Export from the Abbott Lingo App

1. Open the **Lingo** app on Android or iOS.
2. Tap **You** → **Profile** → **Actions** → **Export your glucose data**.
3. Tap **Export data** (one new cumulative export per day) → **Download file**.
4. Transfer or save the `.csv` file locally (**never** place personal exports
   inside this Git repository).

Alternative export paths supported by the same parser today:
- **iOS Apple Health:** *Health app → Profile → Export All Health Data* →
  unzip and select `export.xml` (automatically extracts
  `HKQuantityTypeIdentifierBloodGlucose` records with `sourceName="Lingo"`).
- **Android Health Connect JSON:** Any JSON export conforming to
  `dabney.health_bridge.v1` or Android Health Connect `BloodGlucoseRecord`
  serialization.

### Option A — Validate in the Web UI or Android App

1. Open `https://dabney.moe/mental-health` (or run `cargo leptos watch` locally
   and visit `http://127.0.0.1:3039/mental-health`, or open the **Mood & CGM**
   tab in the Dabney.moe Android app).
2. In **Abbott Lingo & Commercial CGM Import**, select your downloaded `.csv`,
   `export.xml`, or `.json` file.
3. Everything parses locally in Rust/WebAssembly inside device RAM (never
   uploaded). Inspect:
   - **Import Validation Report:** format, parsed reading count, date range,
     median cadence (`5.0 min` for Abbott Lingo), longest sensor gap,
     `<55 / >200 mg/dL` sensor sentinels, skipped/duplicate rows, and ATTD
     wear-time checks (`>=14 days`, `>=70%` active wear).
   - **Time window selector:** switch between `24h`, `3d`, `7d`, `14d` (ATTD
     standard), and `All`.
   - **ADA 5-Band TIR, GMI, GRI, MAGE, 24-Hour AGP** (binned by **local
     wall-clock hour** after parsing ISO-8601 timezone offsets such as
     `2026-02-07T11:38-05:00`), plus the downsampled **3D WebGL2** and **2D
     SVG** timelines.
4. Click **Clear personal data & restore sample** (or reload the page) to wipe
   all personal readings from memory.

### Option B — Validate Completely Offline via CLI

Run the workspace validator against your local export file outside Git:

```bash
nix develop -c cargo run -p ui --features ssr --bin validate-cgm -- \
  "$HOME/Downloads/lingo-glucose-export.csv" \
  --monitor abbott-lingo \
  --window 14d
```

Useful flags:
- `--window <24h|3d|7d|14d|all>` — scope clinical metrics (GMI, TIR, GRI, AGP)
  to a standard 14-day ATTD window even on multi-month cumulative Lingo files.
- `--json` — emit machine-readable JSON diagnostics and clinical metrics.
- `--export-bridge-json /tmp/lingo-bridge.json` — convert a real Lingo CSV or
  Apple Health `export.xml` into the canonical `dabney.health_bridge.v1` JSON
  envelope locally (keep outputs outside Git).

---

## 2. Two-Flavor Strategy: Keep Public Play Demo Zero-PHI

Google Play enforces strict Health Connect declaration, privacy policy, limited
use, and screencast verification requirements for any package requesting
`android.permission.health.*`. Keeping two distinct Android package IDs avoids
delaying or risking the public patient demo:

| Build Flavor | Package / Bundle ID | Permissions | Distribution |
| --- | --- | --- | --- |
| **Public Demo** (shipped now) | `moe.dabney.demo` | Zero Health Connect, zero sensitive device permissions (enforced fail-closed by `scripts/android-release.sh`) | Google Play Store (patients & reviewers) |
| **Personal / Opt-In** (implemented for Android) | `moe.dabney.personal` | `android.permission.health.READ_BLOOD_GLUCOSE` only (enforced by `scripts/android-personal.sh` + Cargo feature `health-connect`) | USB sideload (`adb install -r target/android-personal/dabney-personal-sideload.apk`) immediately; optional Google Play Health Connect submission later |

Never add `android.permission.health.*` to `src-tauri/tauri.android.conf.json`.
The personal flavor lives in its dedicated overlay config
(`src-tauri/tauri.android.personal.conf.json`) and Cargo feature
(`health-connect`).

---

## 3. Shared Bridge Contract (`dabney.health_bridge.v1`)

Both future native plugins (Android Health Connect Kotlin and iOS HealthKit
Swift) serialize records into the **same versioned JSON schema already parsed
and unit-tested in `crates/ui/src/mental_health_cgm.rs`**:

```json
{
  "schema": "dabney.health_bridge.v1",
  "platform": "android_health_connect",
  "exported_at": "2026-11-25T18:00:00-07:00",
  "records": [
    {
      "time": "2026-11-25T15:00:00Z",
      "zoneOffset": "-07:00",
      "levelMgDl": 108.0,
      "relationToMeal": "fasting",
      "specimenSource": "interstitial_fluid",
      "dataOrigin": "com.abbott.lingo"
    }
  ]
}
```

Key guarantees built into `parse_commercial_glucose_input`:
- Exact Gregorian calendar & UTC offset normalization for chronological
  ordering, active wear %, and cadence calculation.
- Local wall-clock conversion (`time` + `zoneOffset`) so the 24-hour AGP bins
  breakfast/lunch/sleep spikes into local clock hours (`00:00–23:00`).
- Automatic `com.abbott.lingo.wellness` / `com.abbott.lingo` / `"Lingo"` source
  detection, Lingo 55–200 mg/dL sensor-cap classification, and duplicate sync
  row deduplication.

---

## 4. Android Google Health Connect Setup (`moe.dabney.personal`)

### 4.1 Prerequisites on Your Android Phone

1. Android 14+ includes **Health Connect** in system settings (*Settings →
   Security & privacy → Privacy → Health Connect*). On Android 9–13, install
   the official **Health Connect by Android** app from Google Play.
2. In the **Abbott Lingo** Android app, enable **Health Connect** sync so Lingo
   writes interstitial glucose readings into Health Connect.

### 4.2 Implemented Personal Flavor Components

All Android Health Connect code is isolated behind Cargo feature `health-connect`
and `src-tauri/tauri.android.personal.conf.json`, so `moe.dabney.demo` contains
zero Health Connect dependencies, commands, or manifest permissions:

- **`src-tauri/tauri.android.personal.conf.json`**: Tauri v2 overlay config
  setting `identifier = "moe.dabney.personal"`, `productName = "Dabney.moe Personal"`,
  and `minSdkVersion = 26`.
- **`src-tauri/src/health_connect.rs`**: Feature-gated (`health-connect`) Rust
  Tauri commands (`health_connect_status`, `health_connect_request_permission`,
  `health_connect_read_glucose`) and Android plugin registration
  (`moe.dabney.personal.HealthConnectPlugin`).
- **`src-tauri/android-personal/HealthConnectPlugin.kt`**: Clean-room Tauri v2
  Kotlin `@TauriPlugin` calling `androidx.health.connect:connect-client:1.1.0`
  (`HealthConnectClient.getSdkStatus`, `PermissionController.createRequestPermissionResultContract()`,
  paginated `ReadRecordsRequest<BloodGlucoseRecord>`) and emitting canonical
  `dabney.health_bridge.v1` JSON.
- **`src-tauri/android-personal/PermissionsRationaleActivity.kt`**: Native
  privacy & rationale activity wired to both Android 13
  (`androidx.health.ACTION_SHOW_PERMISSIONS_RATIONALE`) and Android 14+
  (`android.intent.action.VIEW_PERMISSION_USAGE` / `HEALTH_PERMISSIONS`).
- **`crates/ui/src/mental_health.rs`**: Probes `health_connect_status` on mount
  via `window.__TAURI__.core.invoke` and displays the **Android Health Connect ·
  Live On-Device Sync** card (`Grant Blood Glucose Access`, `Sync Last 14 Days`,
  `Sync Last 90 Days`) **only** when `enabled: true` (`moe.dabney.personal`).
- **`scripts/android-personal.sh`**: Builds, patches Gradle (`connect-client:1.1.0`,
  16 KB alignment) & manifest (only `READ_BLOOD_GLUCOSE` + rationale filters;
  fails closed on unrelated sensitive permissions), verifies `aapt2` badging +
  ELF alignment, and debug-signs `target/android-personal/dabney-personal-sideload.apk`.

### 4.3 Build & Install on Your Android Phone (USB Sideload)

For personal use on your own Android phone, **no Google Play Console Health
Connect review is required**:

```bash
# 1. Build the 16 KB-aligned personal APK for ARM64 phones (and optionally x86_64 emulators):
nix develop .#android -c scripts/android-personal.sh --apk --target aarch64

# 2. Install onto your USB-connected Android phone (coexists cleanly alongside moe.dabney.demo):
adb install -r target/android-personal/dabney-personal-sideload.apk
```

Then open **Dabney.moe Personal** → **Mood & CGM** tab → tap **Grant Blood
Glucose Access** in the emerald **Android Health Connect** card → tap **Sync
Last 14 Days** (or **Sync Last 90 Days**) to inspect your live Abbott Lingo
metrics, 24h AGP, validation diagnostics, and local on-device chat strictly on your
phone (saved in the Android Keystore AES-256-GCM vault with one-tap cryptographic wipe).

### 4.4 Google Play Console Health Connect Approval (Only If Publishing Personal Flavor)

If you later distribute `moe.dabney.personal` on Google Play:
1. Update `https://dabney.moe/privacy` to explicitly disclose Health Connect
   `READ_BLOOD_GLUCOSE` limited use (read-only, processed locally on device,
   never sold, never used for advertising/credit/insurance).
2. Complete **App content → Health apps** and the **Health Connect declaration**
   form, selecting only `READ_BLOOD_GLUCOSE` (and any other actually displayed
   types).
3. Record a short unlisted screencast showing:
   - The in-app permission rationale screen,
   - The Android Health Connect permission grant sheet,
   - Blood glucose readings visibly rendered on `/mental-health` (KPIs + 24h
     AGP + timeline).

---

## 5. iOS Apple HealthKit Personal Flavor (`moe.dabney.personal`)

For iPhone Abbott Lingo users syncing Lingo → Apple Health, the personal flavor
reuses the exact same Cargo feature (`health-connect`), Rust commands
(`src-tauri/src/health_connect.rs`), `/mental-health` live sync card (which
automatically switches to **Apple HealthKit · Live On-Device Sync**), and
`dabney.health_bridge.v1` JSON schema:

- **`src-tauri/tauri.ios.personal.conf.json`**: iOS personal overlay
  (`identifier = "moe.dabney.personal"`, `minimumSystemVersion = "16.0"`).
- **`src-tauri/ios-personal/Sources/HealthKitPlugin.swift`**: Clean-room Swift
  Tauri v2 plugin (`@_cdecl("init_plugin_dabney_health_kit")`) calling
  `HKHealthStore`, `requestAuthorization(toShare: [], read: [bloodGlucose])`,
  and `HKSampleQuery` with `HKUnit(from: "mg/dL")`, preserving per-sample
  timezone offsets (`HKMetadataKeyTimeZone` / `TimeZone.current`) and source
  metadata (`sourceRevision.source.name`).
- **`src-tauri/ios-personal/Personal.entitlements`**: Enables read-only
  `com.apple.developer.healthkit`.
- **`scripts/ios-personal.sh`** (macOS + Xcode required): Initializes the iOS
  project with `tauri.ios.personal.conf.json`, copies the Swift plugin &
  entitlements, injects `NSHealthShareUsageDescription` (failing closed if
  write-access `NSHealthUpdateUsageDescription` is present), and builds/opens
  Xcode with `--features health-connect`.

```bash
# Run on macOS with Xcode & Apple signing team configured:
scripts/ios-personal.sh --open
```

---

## 6. Encrypted On-Device Personal Vault & Local LLM Chat (`moe.dabney.personal`)

The opt-in personal build (`--features health-connect`) also adds hardware-backed
local storage and on-device LLM chat without affecting the public zero-PHI demo
(`moe.dabney.demo`):

- **Hardware-Backed Encrypted Vault (`PersonalVaultPlugin.kt` + `src-tauri/src/personal_vault.rs`):**
  - Uses Android Keystore (`AndroidKeyStore`) **AES-256-GCM** (`dabney_personal_vault_v1`,
    random 12-byte IV per atomic write) inside app-private `filesDir` with
    `android:allowBackup="false"` enforced.
  - Persists synced CGM snapshots (`dabney.health_bridge.v1`), mood/anxiety/PHQ-9/GAD-7
    check-ins, private visit notes, and local chat threads across restarts.
  - Includes one-tap **Cryptographic Wipe** (`personal_vault_wipe`) that overwrites
    and deletes vault files and destroys the Android Keystore key alias.
- **On-Device LLM Chat (`LocalLlmPlugin.kt` + `src-tauri/src/local_llm.rs`):**
  - Uses Google's official **ML Kit GenAI Prompt API** (`com.google.mlkit:genai-prompt:1.0.0-beta4`)
    backed by **Android AICore (Gemini Nano)** on supported Android devices (such as
    Snapdragon flagship phones with `com.google.android.aicore`), running 100% locally
    on-device with zero multi-GB weights bundled in the APK and zero cloud egress.
  - Automatically falls back to deterministic grounded Rust synthesis on emulators,
    non-AICore phones, desktop, and web browsers while keeping every response strictly
    grounded in computed local CGM metrics or single-patient verbatim chart citations.
