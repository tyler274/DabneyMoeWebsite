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
| **Personal / Opt-In** (future) | `moe.dabney.personal` | `android.permission.health.READ_BLOOD_GLUCOSE` (plus optional `READ_SLEEP` / `READ_STEPS` only when displayed in UI) | USB sideload (`adb install`) immediately; optional Google Play Health Connect submission later |

Never add `android.permission.health.*` to `src-tauri/tauri.android.conf.json`.
Create a dedicated overlay config (`src-tauri/tauri.android.personal.conf.json`)
for the personal build when wiring the native plugin.

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
- Automatic `com.abbott.lingo` / `"Lingo"` source detection and duplicate sync
  row deduplication.

---

## 4. Android Google Health Connect Setup Blueprint (Future Personal Flavor)

### 4.1 Prerequisites on Your Android Phone

1. Android 14+ includes **Health Connect** in system settings (*Settings →
   Security & privacy → Privacy → Health Connect*). On Android 9–13, install
   the official **Health Connect by Android** app from Google Play.
2. In the **Abbott Lingo** Android app, enable **Health Connect** sync so Lingo
   writes interstitial glucose readings into Health Connect.

### 4.2 Gradle Dependency (`app/build.gradle.kts`)

Matches our existing `minSdkVersion = 26` and `compileSdk = 36`:

```kotlin
dependencies {
    implementation("androidx.health.connect:connect-client:1.1.0")
}
```

### 4.3 Manifest Entries (`AndroidManifest.xml` — Personal Flavor Only)

Request **only** permissions that are visibly rendered in `/mental-health`:

```xml
<!-- Only declare permissions displayed in the UI -->
<uses-permission android:name="android.permission.health.READ_BLOOD_GLUCOSE" />

<!-- Required for HealthConnectClient.getSdkStatus(context) -->
<queries>
    <package android:name="com.google.android.apps.healthdata" />
</queries>

<application ...>
    <!-- Android 13 and below rationale activity -->
    <activity android:name=".PermissionsRationaleActivity" android:exported="true">
        <intent-filter>
            <action android:name="androidx.health.ACTION_SHOW_PERMISSIONS_RATIONALE" />
        </intent-filter>
    </activity>

    <!-- Android 14+ system Health Connect privacy policy / rationale entry -->
    <activity-alias
        android:name="ViewPermissionUsageActivity"
        android:exported="true"
        android:targetActivity=".PermissionsRationaleActivity"
        android:permission="android.permission.START_VIEW_PERMISSION_USAGE">
        <intent-filter>
            <action android:name="android.intent.action.VIEW_PERMISSION_USAGE" />
            <category android:name="android.intent.category.HEALTH_PERMISSIONS" />
        </intent-filter>
    </activity-alias>
</application>
```

### 4.4 Tauri v2 Kotlin Plugin Outline (`HealthConnectPlugin.kt`)

Implement as a clean-room Tauri v2 mobile plugin registered only under a Cargo
feature flag (e.g., `health-connect`):

1. **`checkAvailability()`**: Calls
   `HealthConnectClient.getSdkStatus(activity, "com.google.android.apps.healthdata")`
   and returns `"available"`, `"update_required"`, or `"unavailable"`.
2. **`requestGlucosePermission()`**: Requests
   `setOf(HealthPermission.getReadPermission(BloodGlucoseRecord::class))` via
   `PermissionController.createRequestPermissionResultContract()`.
3. **`readBloodGlucose(days: Int)`**: Queries `HealthConnectClient.readRecords`
   with `ReadRecordsRequest(BloodGlucoseRecord::class, TimeRangeFilter.between(start, end))`
   looping over `pageToken` until exhausted, maps each `BloodGlucoseRecord`
   (`time`, `zoneOffset`, `level.inMilligramsPerDeciliter`, `relationToMeal`,
   `specimenSource`, `metadata.dataOrigin.packageName`) into a
   `dabney.health_bridge.v1` JSON string, and returns it to WASM where
   `parse_commercial_glucose_input` validates and renders it immediately.

### 4.5 Immediate Personal Testing via USB Sideloading

For personal use on your own Android phone, **no Google Play Console Health
Connect review is required**:
1. Build the `aarch64` personal APK locally (`--target aarch64`).
2. Install onto your phone via `adb install -r <personal.apk>`.
3. Grant `Blood glucose (Read)` inside Health Connect and validate live Lingo
   sync directly against the `/mental-health` AGP & validation panel.

### 4.6 Google Play Console Health Connect Approval (Only If Publishing Personal Flavor)

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

## 5. iOS Apple HealthKit Blueprint (Future Personal Flavor)

For iPhone Lingo users syncing Lingo → Apple Health:
1. Enable the **HealthKit** capability (`com.apple.developer.healthkit`
   entitlement) in the Xcode project generated by `cargo tauri ios init`.
2. Add `NSHealthShareUsageDescription` in `Info.plist` explaining read-only
   on-device blood glucose visualization.
3. Implement a Tauri v2 Swift plugin running `HKSampleQuery` on
   `HKQuantityType.quantityType(forIdentifier: .bloodGlucose)` with
   `HKUnit(from: "mg/dL")`, serializing `startDate`, `TimeZone.current` offset,
   `quantity.doubleValue(for: unit)`, and `sourceRevision.source.name` (`"Lingo"`)
   into the identical `dabney.health_bridge.v1` JSON envelope.
