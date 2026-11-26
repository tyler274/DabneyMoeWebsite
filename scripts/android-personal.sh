#!/usr/bin/env bash
# Reproducible builder & verifier for the opt-in PERSONAL Dabney.moe Android app
# (`moe.dabney.personal`) with native Google Health Connect blood glucose sync.
#
# Strictly separate from the zero-PHI public Google Play demo (`moe.dabney.demo`,
# built by `scripts/android-release.sh`). Never run this script for Play Store demo
# uploads unless you intentionally complete Google Play's Health Connect review.
#
# Run inside `nix develop .#android`:
#   scripts/android-personal.sh --apk                         # USB sideload APK (aarch64)
#   scripts/android-personal.sh --apk --target x86_64         # emulator APK
#   scripts/android-personal.sh --apk --target aarch64 --target x86_64
#   scripts/android-personal.sh --aab                         # signed/unsigned bundle
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

MODE="apk"
TARGETS=()
CLEAN_INIT=0
EXPECTED_PACKAGE="moe.dabney.personal"
PERSONAL_CONFIG="$ROOT/src-tauri/tauri.android.personal.conf.json"
KOTLIN_SRC_DIR="$ROOT/src-tauri/android-personal"

usage() {
  sed -n '2,14p' "$0" | sed 's/^# \{0,1\}//'
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --apk)
      MODE="apk"
      shift
      ;;
    --aab)
      MODE="aab"
      shift
      ;;
    --target)
      TARGETS+=("${2:?missing value for --target}")
      shift 2
      ;;
    --clean-init)
      CLEAN_INIT=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if [ "${#TARGETS[@]}" -eq 0 ]; then
  if [ "$MODE" = "aab" ]; then
    TARGETS=(aarch64 x86_64)
  else
    TARGETS=(aarch64)
  fi
fi

ANDROID_DIR="$ROOT/src-tauri/gen/android"
ROOT_GRADLE="$ANDROID_DIR/build.gradle.kts"
APP_GRADLE="$ANDROID_DIR/app/build.gradle.kts"
MANIFEST="$ANDROID_DIR/app/src/main/AndroidManifest.xml"
JAVA_PKG_DIR="$ANDROID_DIR/app/src/main/java/moe/dabney/personal"
TEMP_KEYSTORE=""

cleanup() {
  if [ -n "$TEMP_KEYSTORE" ] && [ -f "$TEMP_KEYSTORE" ]; then
    rm -f "$TEMP_KEYSTORE"
  fi
  if [ -f "$ANDROID_DIR/keystore.properties" ]; then
    rm -f "$ANDROID_DIR/keystore.properties"
  fi
}
trap cleanup EXIT

# Re-initialize whenever missing, requested, or generated for a different flavor (e.g. moe.dabney.demo).
if [ "$CLEAN_INIT" -eq 1 ] || [ ! -f "$APP_GRADLE" ] || ! grep -q "\"$EXPECTED_PACKAGE\"" "$APP_GRADLE"; then
  echo "==> Initializing Tauri Android project for personal flavor $EXPECTED_PACKAGE"
  rm -rf "$ANDROID_DIR"
  cargo tauri android init --skip-targets-install -c "$PERSONAL_CONFIG"
fi

# Always sync latest generated Android adaptive/mipmap icons (in case src-tauri/icons/ changed after android init).
if [ -d "$ROOT/src-tauri/icons/android" ]; then
  cp -rf "$ROOT/src-tauri/icons/android"/. "$ANDROID_DIR/app/src/main/res/"
fi

# Copy clean-room Kotlin Health Connect, encrypted Personal Vault, on-device LLM, and local alert plugins.
mkdir -p "$JAVA_PKG_DIR"
cp -f "$KOTLIN_SRC_DIR/HealthConnectPlugin.kt" "$JAVA_PKG_DIR/HealthConnectPlugin.kt"
cp -f "$KOTLIN_SRC_DIR/PermissionsRationaleActivity.kt" "$JAVA_PKG_DIR/PermissionsRationaleActivity.kt"
cp -f "$KOTLIN_SRC_DIR/PersonalVaultPlugin.kt" "$JAVA_PKG_DIR/PersonalVaultPlugin.kt"
cp -f "$KOTLIN_SRC_DIR/LocalLlmPlugin.kt" "$JAVA_PKG_DIR/LocalLlmPlugin.kt"
cp -f "$KOTLIN_SRC_DIR/GlucoseAlertPlugin.kt" "$JAVA_PKG_DIR/GlucoseAlertPlugin.kt"

# Copy personal ProGuard/R8 keep rules for Tauri plugins, ML Kit ComponentRegistrar, and AICore.
# Tauri's generated app/build.gradle.kts automatically includes every app/**/*.pro file.
cp -f "$KOTLIN_SRC_DIR/proguard-personal.pro" "$ANDROID_DIR/app/proguard-personal.pro"

# Resolve optional release keystore from file path or base64 env var.
SIGNED=0
RESOLVED_KEYSTORE=""
if [ -n "${ANDROID_KEYSTORE_PATH:-}" ]; then
  if [ ! -r "$ANDROID_KEYSTORE_PATH" ]; then
    echo "ANDROID_KEYSTORE_PATH is not readable: $ANDROID_KEYSTORE_PATH" >&2
    exit 1
  fi
  RESOLVED_KEYSTORE="$(realpath "$ANDROID_KEYSTORE_PATH")"
elif [ -n "${ANDROID_KEYSTORE_BASE64:-}" ]; then
  TEMP_KEYSTORE="$(mktemp "${TMPDIR:-/tmp}/dabney-personal-keystore.XXXXXX.jks")"
  chmod 600 "$TEMP_KEYSTORE"
  printf '%s' "$ANDROID_KEYSTORE_BASE64" | base64 --decode > "$TEMP_KEYSTORE"
  RESOLVED_KEYSTORE="$TEMP_KEYSTORE"
fi

if [ -n "$RESOLVED_KEYSTORE" ]; then
  KEY_ALIAS="${ANDROID_KEY_ALIAS:?ANDROID_KEY_ALIAS is required when signing}"
  STORE_PASS="${ANDROID_KEYSTORE_PASSWORD:-${ANDROID_KEY_PASSWORD:-}}"
  KEY_PASS="${ANDROID_KEY_PASSWORD:-$STORE_PASS}"
  if [ -z "$STORE_PASS" ]; then
    echo "ANDROID_KEYSTORE_PASSWORD (or ANDROID_KEY_PASSWORD) is required when signing." >&2
    exit 2
  fi
  umask 077
  cat > "$ANDROID_DIR/keystore.properties" <<EOF
storeFile=$RESOLVED_KEYSTORE
storePassword=$STORE_PASS
keyAlias=$KEY_ALIAS
keyPassword=$KEY_PASS
EOF
  SIGNED=1
  echo "==> Configured release signing (alias: $KEY_ALIAS)"
else
  echo "==> No release keystore provided; will debug-sign local sideload APK"
fi

# Patch Gradle (Health Connect SDK, coroutines, Nix buildTools 37.0.0, 16 KB alignment)
# and AndroidManifest.xml (READ_BLOOD_GLUCOSE + rationale intent filters; fail closed on unrelated sensitive perms).
python3 - "$ROOT_GRADLE" "$APP_GRADLE" "$MANIFEST" "$SIGNED" "$EXPECTED_PACKAGE" <<'PY'
import pathlib
import re
import sys

root_gradle_path = pathlib.Path(sys.argv[1])
gradle_path = pathlib.Path(sys.argv[2])
manifest_path = pathlib.Path(sys.argv[3])
signed = sys.argv[4] == "1"
expected_pkg = sys.argv[5]

root_gradle = root_gradle_path.read_text(encoding="utf-8")
if 'buildToolsVersion = "37.0.0"' not in root_gradle:
    root_gradle += """
subprojects {
    plugins.withId("com.android.application") {
        extensions.configure<com.android.build.api.dsl.ApplicationExtension> {
            buildToolsVersion = "37.0.0"
        }
    }
    plugins.withId("com.android.library") {
        extensions.configure<com.android.build.api.dsl.LibraryExtension> {
            buildToolsVersion = "37.0.0"
        }
    }
}
"""
    root_gradle_path.write_text(root_gradle, encoding="utf-8")

gradle = gradle_path.read_text(encoding="utf-8")
if f'"{expected_pkg}"' not in gradle:
    raise SystemExit(f"Expected applicationId/namespace {expected_pkg!r} in {gradle_path}")

gradle = re.sub(r"compileSdk\s*=\s*\d+", "compileSdk = 36", gradle)
gradle = re.sub(r"targetSdk\s*=\s*\d+", "targetSdk = 36", gradle)
if "buildToolsVersion" not in gradle:
    gradle = re.sub(
        r"(compileSdk\s*=\s*36)",
        r'\1\n    buildToolsVersion = "37.0.0"',
        gradle,
        count=1,
    )

if "import java.util.Properties" not in gradle:
    gradle = "import java.util.Properties\n" + gradle

if "useLegacyPackaging = false" not in gradle:
    packaging_block = """
    packaging {
        jniLibs {
            useLegacyPackaging = false
        }
    }
"""
    gradle, count = re.subn(r"(android\s*\{)", r"\1" + packaging_block, gradle, count=1)
    if count != 1:
        raise SystemExit(f"Could not locate android {{ }} block in {gradle_path}")

if signed and "signingConfigs" not in gradle:
    signing_block = """
    signingConfigs {
        create("release") {
            val props = Properties()
            val propsFile = rootProject.file("keystore.properties")
            if (propsFile.exists()) {
                propsFile.inputStream().use { props.load(it) }
                storeFile = file(props.getProperty("storeFile"))
                storePassword = props.getProperty("storePassword")
                keyAlias = props.getProperty("keyAlias")
                keyPassword = props.getProperty("keyPassword")
            }
        }
    }
"""
    gradle, count = re.subn(r"(buildTypes\s*\{)", signing_block + "\n    \\1", gradle, count=1)
    if count != 1:
        raise SystemExit(f"Could not locate buildTypes {{ }} block in {gradle_path}")

if signed and 'signingConfig = signingConfigs.getByName("release")' not in gradle:
    gradle, count = re.subn(
        r'(getByName\("release"\)\s*\{)',
        r'\1\n            if (rootProject.file("keystore.properties").exists()) {\n                signingConfig = signingConfigs.getByName("release")\n            }',
        gradle,
        count=1,
    )
    if count != 1:
        raise SystemExit(f"Could not locate getByName(\"release\") block in {gradle_path}")

# Add Android Health Connect client, ML Kit GenAI Prompt (AICore / Gemini Nano), and Kotlin coroutines.
# Pin 1.0.0-beta4 (Kotlin metadata 2.3.0, compatible with Tauri's Kotlin 2.2.x Gradle plugin).
hc_deps = """    implementation("androidx.health.connect:connect-client:1.1.0")
    implementation("com.google.mlkit:genai-prompt:1.0.0-beta4")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.9.0")"""
if "androidx.health.connect:connect-client" not in gradle:
    gradle, count = re.subn(
        r"(dependencies\s*\{)",
        r"\1\n" + hc_deps,
        gradle,
        count=1,
    )
    if count != 1:
        raise SystemExit(f"Could not locate dependencies {{ }} block in {gradle_path}")
elif "com.google.mlkit:genai-prompt" not in gradle:
    gradle = gradle.replace(
        'implementation("androidx.health.connect:connect-client:1.1.0")',
        'implementation("androidx.health.connect:connect-client:1.1.0")\n    implementation("com.google.mlkit:genai-prompt:1.0.0-beta4")',
        1,
    )
else:
    gradle = re.sub(
        r'com\.google\.mlkit:genai-prompt:[0-9A-Za-z.\-]+',
        'com.google.mlkit:genai-prompt:1.0.0-beta4',
        gradle,
    )

gradle_path.write_text(gradle, encoding="utf-8")

manifest = manifest_path.read_text(encoding="utf-8")
if "android:usesCleartextTraffic" in manifest:
    manifest = re.sub(
        r'android:usesCleartextTraffic="[^"]*"',
        'android:usesCleartextTraffic="false"',
        manifest,
    )
else:
    manifest = manifest.replace(
        "<application",
        '<application\n        android:usesCleartextTraffic="false"',
        1,
    )
if "android:allowBackup" in manifest:
    manifest = re.sub(
        r'android:allowBackup="[^"]*"',
        'android:allowBackup="false"',
        manifest,
    )
else:
    manifest = manifest.replace(
        "<application",
        '<application\n        android:allowBackup="false"',
        1,
    )

# Inject strictly READ_BLOOD_GLUCOSE + <queries> for Health Connect and Android AICore.
if "android.permission.health.READ_BLOOD_GLUCOSE" not in manifest:
    manifest = manifest.replace(
        "<application",
        '    <uses-permission android:name="android.permission.health.READ_BLOOD_GLUCOSE" />\n'
        '    <queries>\n'
        '        <package android:name="com.google.android.apps.healthdata" />\n'
        '        <package android:name="com.google.android.aicore" />\n'
        '    </queries>\n\n    <application',
        1,
    )
elif "com.google.android.aicore" not in manifest:
    manifest = manifest.replace(
        '<package android:name="com.google.android.apps.healthdata" />',
        '<package android:name="com.google.android.apps.healthdata" />\n        <package android:name="com.google.android.aicore" />',
        1,
    )

# Inject Android 13+ POST_NOTIFICATIONS for opt-in local glucose range alerts.
if "android.permission.POST_NOTIFICATIONS" not in manifest:
    manifest = manifest.replace(
        "<application",
        '    <uses-permission android:name="android.permission.POST_NOTIFICATIONS" />\n    <application',
        1,
    )

# Inject Android 13 rationale activity + Android 14+ VIEW_PERMISSION_USAGE alias.
if "PermissionsRationaleActivity" not in manifest:
    rationale_xml = """
        <activity
            android:name=".PermissionsRationaleActivity"
            android:exported="true">
            <intent-filter>
                <action android:name="androidx.health.ACTION_SHOW_PERMISSIONS_RATIONALE" />
            </intent-filter>
        </activity>

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
    </application>"""
    manifest = manifest.replace("</application>", rationale_xml, 1)

# Still fail closed against unrelated sensitive device permissions.
forbidden_unrelated = [
    "android.permission.CAMERA",
    "android.permission.RECORD_AUDIO",
    "android.permission.ACCESS_FINE_LOCATION",
    "android.permission.ACCESS_COARSE_LOCATION",
    "android.permission.READ_CONTACTS",
    "android.permission.health.WRITE_",
]
for perm in forbidden_unrelated:
    if perm in manifest:
        raise SystemExit(f"Forbidden unrelated permission found in {manifest_path}: {perm}")

manifest_path.write_text(manifest, encoding="utf-8")
print("Patched personal Gradle (Health Connect 1.1.0, 16 KB packaging) and manifest (READ_BLOOD_GLUCOSE, POST_NOTIFICATIONS + rationale).")
PY

BUILD_ARGS=("--$MODE" "-c" "$PERSONAL_CONFIG" "--features" "health-connect")
for target in "${TARGETS[@]}"; do
  BUILD_ARGS+=("--target" "$target")
done

if [ -n "${ANDROID_HOME:-}" ] && [ -x "$ANDROID_HOME/build-tools/37.0.0/aapt2" ]; then
  export GRADLE_OPTS="-Dorg.gradle.project.android.aapt2FromMavenOverride=$ANDROID_HOME/build-tools/37.0.0/aapt2"
fi

echo "==> Building personal Android $MODE ($EXPECTED_PACKAGE, --features health-connect) for: ${TARGETS[*]}"
cargo tauri android build "${BUILD_ARGS[@]}"

if [ "$MODE" = "aab" ]; then
  ARTIFACT="$(find "$ANDROID_DIR/app/build/outputs/bundle" -name "*.aab" | sort | head -n1)"
else
  # Prefer Gradle's freshly built APK and ignore any leftover locally signed artifacts.
  ARTIFACT="$(find "$ANDROID_DIR/app/build/outputs/apk" -name "*.apk" ! -name "*-personal.apk" ! -name "*-aligned.apk" | sort | head -n1)"
fi

if [ -z "$ARTIFACT" ] || [ ! -f "$ARTIFACT" ]; then
  echo "Failed to locate generated .$MODE artifact under $ANDROID_DIR/app/build/outputs" >&2
  exit 1
fi

echo "==> Built personal artifact: $ARTIFACT ($(du -Lh "$ARTIFACT" | awk '{print $1}'))"

# Verify 16 KB ELF LOAD alignment on all 64-bit .so libraries.
READELF="${NDK_HOME:?NDK_HOME must be set}/toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-readelf"
if [ ! -x "$READELF" ]; then
  READELF="$(command -v readelf || true)"
fi
python3 - "$ARTIFACT" "$READELF" <<'PY'
import pathlib
import subprocess
import sys
import tempfile
import zipfile

artifact = pathlib.Path(sys.argv[1])
readelf = sys.argv[2]

checked = 0
with tempfile.TemporaryDirectory() as tmp:
    with zipfile.ZipFile(artifact) as zf:
        so_members = [
            name for name in zf.namelist()
            if name.endswith(".so") and ("/arm64-v8a/" in name or "/x86_64/" in name)
        ]
        if not so_members:
            raise SystemExit(f"No 64-bit .so libraries found inside {artifact}")
        for member in so_members:
            extracted = pathlib.Path(tmp) / pathlib.Path(member).name
            extracted.write_bytes(zf.read(member))
            out = subprocess.check_output([readelf, "-lW", str(extracted)], text=True)
            loads = [line for line in out.splitlines() if line.strip().startswith("LOAD")]
            for line in loads:
                align_str = line.split()[-1]
                align = int(align_str, 16) if align_str.startswith("0x") else int(align_str)
                if align < 16384:
                    raise SystemExit(f"16 KB alignment FAILED for {member}: align={align_str}")
            checked += 1
            print(f"  [OK] 16 KB aligned: {member} ({len(loads)} LOAD segments >= 0x4000)")
print(f"Verified 16 KB ELF alignment across {checked} 64-bit native library(s).")
PY

# Verify package ID, READ_BLOOD_GLUCOSE permission, and rationale intent filters via aapt2 for APKs.
if [ "$MODE" = "apk" ] && command -v aapt2 >/dev/null 2>&1; then
  echo "==> Verifying personal APK manifest & Health Connect permission via aapt2"
  BADGING="$(aapt2 dump badging "$ARTIFACT")"
  grep -E "^(package:|sdkVersion:|targetSdkVersion:|application-label:|uses-permission:)" <<< "$BADGING" || true
  if [[ "$BADGING" != *"package: name='$EXPECTED_PACKAGE'"* ]]; then
    echo "Unexpected APK package name (expected $EXPECTED_PACKAGE)" >&2
    exit 1
  fi
  if [[ "$BADGING" != *"uses-permission: name='android.permission.health.READ_BLOOD_GLUCOSE'"* ]]; then
    echo "Missing android.permission.health.READ_BLOOD_GLUCOSE in built personal APK!" >&2
    exit 1
  fi
  if [[ "$BADGING" != *"uses-permission: name='android.permission.POST_NOTIFICATIONS'"* ]]; then
    echo "Missing android.permission.POST_NOTIFICATIONS in built personal APK!" >&2
    exit 1
  fi
  XMLTREE="$(aapt2 dump xmltree "$ARTIFACT" --file AndroidManifest.xml)"
  if [[ "$XMLTREE" != *"PermissionsRationaleActivity"* ]] || [[ "$XMLTREE" != *"VIEW_PERMISSION_USAGE"* ]]; then
    echo "Missing Health Connect rationale activity or VIEW_PERMISSION_USAGE alias in built personal APK!" >&2
    exit 1
  fi
fi

OUT_DIR="$ROOT/target/android-personal"
mkdir -p "$OUT_DIR"
if [ "$MODE" = "aab" ]; then
  OUT_FILE="$OUT_DIR/dabney-personal.aab"
else
  OUT_FILE="$OUT_DIR/dabney-personal-sideload.apk"
fi

# Debug-sign local sideload APKs directly to $OUT_FILE when no release keystore was passed.
if [ "$SIGNED" -eq 0 ] && [ "$MODE" = "apk" ] && [[ "$ARTIFACT" == *-unsigned.apk ]]; then
  DEBUG_KS="${HOME}/.android/debug.keystore"
  if [ ! -f "$DEBUG_KS" ]; then
    mkdir -p "$(dirname "$DEBUG_KS")"
    keytool -genkeypair -v \
      -keystore "$DEBUG_KS" \
      -storetype PKCS12 \
      -alias androiddebugkey \
      -keyalg RSA \
      -keysize 2048 \
      -validity 10000 \
      -storepass android \
      -keypass android \
      -dname "CN=Android Debug,O=Android,C=US"
  fi
  ALIGNED_APK="$(mktemp "${TMPDIR:-/tmp}/dabney-personal-aligned.XXXXXX.apk")"
  echo "==> Aligning (16 KB) & debug-signing personal sideload APK"
  zipalign -f -P 16 4 "$ARTIFACT" "$ALIGNED_APK"
  apksigner sign \
    --ks "$DEBUG_KS" \
    --ks-key-alias androiddebugkey \
    --ks-pass pass:android \
    --key-pass pass:android \
    --out "$OUT_FILE" \
    "$ALIGNED_APK"
  rm -f "$ALIGNED_APK"
  apksigner verify --print-certs "$OUT_FILE"
else
  cp -f "$ARTIFACT" "$OUT_FILE"
fi
sha256sum "$OUT_FILE" > "$OUT_FILE.sha256"

echo "==> Personal Android Health Connect build verified:"
echo "    $OUT_FILE"
echo "    $(cat "$OUT_FILE.sha256")"
echo "    Install on your Android phone via USB: adb install -r $OUT_FILE"
