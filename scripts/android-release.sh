#!/usr/bin/env bash
# Reproducible builder, signer, and Google Play compliance verifier for the
# Dabney.moe Clinical Demo Android app (`moe.dabney.demo`).
#
# Run inside `nix develop .#android`:
#   scripts/android-release.sh --apk --target x86_64          # emulator APK
#   scripts/android-release.sh --aab                          # Play Store release AAB (arm64-v8a + x86_64)
#   scripts/android-release.sh --aab --all-abis               # Play Store release AAB (all 4 ABIs)
#   scripts/android-release.sh --generate-keystore ~/.config/dabney-android/upload-keystore.jks
#
# Signing (optional for local smoke tests; required for Play Console upload):
#   ANDROID_KEYSTORE_PATH=/path/to/upload-keystore.jks   (or ANDROID_KEYSTORE_BASE64)
#   ANDROID_KEY_ALIAS=dabney-upload
#   ANDROID_KEYSTORE_PASSWORD=...
#   ANDROID_KEY_PASSWORD=...
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

MODE=""
TARGETS=()
ALL_ABIS=0
CLEAN_INIT=0
GENERATE_KEYSTORE=""
EXPECTED_PACKAGE="moe.dabney.demo"

usage() {
  sed -n '2,18p' "$0" | sed 's/^# \{0,1\}//'
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
    --all-abis)
      ALL_ABIS=1
      shift
      ;;
    --clean-init)
      CLEAN_INIT=1
      shift
      ;;
    --generate-keystore)
      GENERATE_KEYSTORE="${2:?missing output path for --generate-keystore}"
      shift 2
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

if [ -n "$GENERATE_KEYSTORE" ]; then
  if [ -e "$GENERATE_KEYSTORE" ]; then
    echo "Refusing to overwrite existing keystore: $GENERATE_KEYSTORE" >&2
    exit 1
  fi
  mkdir -p "$(dirname "$GENERATE_KEYSTORE")"
  chmod 700 "$(dirname "$GENERATE_KEYSTORE")"
  alias_name="${ANDROID_KEY_ALIAS:-dabney-upload}"
  store_pass="${ANDROID_KEYSTORE_PASSWORD:-}"
  key_pass="${ANDROID_KEY_PASSWORD:-$store_pass}"
  if [ -z "$store_pass" ]; then
    echo "Set ANDROID_KEYSTORE_PASSWORD (and optionally ANDROID_KEY_PASSWORD, ANDROID_KEY_ALIAS) before generating a keystore." >&2
    exit 2
  fi
  export ANDROID_KEYSTORE_PASSWORD="$store_pass"
  export ANDROID_KEY_PASSWORD="$key_pass"
  keytool -genkeypair -v \
    -keystore "$GENERATE_KEYSTORE" \
    -storetype PKCS12 \
    -alias "$alias_name" \
    -keyalg RSA \
    -keysize 4096 \
    -validity 10000 \
    -storepass:env ANDROID_KEYSTORE_PASSWORD \
    -keypass:env ANDROID_KEY_PASSWORD \
    -dname "CN=Tyler Port, OU=Dabney.moe, O=Dabney.moe, L=Pasadena, ST=CA, C=US"
  chmod 600 "$GENERATE_KEYSTORE"
  echo "Generated Play upload keystore at $GENERATE_KEYSTORE (alias: $alias_name)."
  echo "Keep this file and its passwords backed up outside Git; never commit it."
  exit 0
fi

if [ -z "$MODE" ]; then
  echo "Choose --apk or --aab (or --generate-keystore <path>)." >&2
  usage >&2
  exit 2
fi

if [ "$ALL_ABIS" -eq 1 ]; then
  TARGETS=(aarch64 armv7 i686 x86_64)
elif [ "${#TARGETS[@]}" -eq 0 ]; then
  if [ "$MODE" = "aab" ]; then
    TARGETS=(aarch64 x86_64)
  else
    TARGETS=(x86_64)
  fi
fi

ANDROID_DIR="$ROOT/src-tauri/gen/android"
ROOT_GRADLE="$ANDROID_DIR/build.gradle.kts"
APP_GRADLE="$ANDROID_DIR/app/build.gradle.kts"
MANIFEST="$ANDROID_DIR/app/src/main/AndroidManifest.xml"
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

# Re-initialize if missing, requested, or still generated for a different package ID.
if [ "$CLEAN_INIT" -eq 1 ] || [ ! -f "$APP_GRADLE" ] || ! grep -q "\"$EXPECTED_PACKAGE\"" "$APP_GRADLE"; then
  echo "==> Initializing Tauri Android project for $EXPECTED_PACKAGE"
  rm -rf "$ANDROID_DIR"
  cargo tauri android init --skip-targets-install
fi

# Always sync latest generated Android adaptive/mipmap icons (in case src-tauri/icons/ changed after android init).
if [ -d "$ROOT/src-tauri/icons/android" ]; then
  cp -rf "$ROOT/src-tauri/icons/android"/. "$ANDROID_DIR/app/src/main/res/"
fi

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
  TEMP_KEYSTORE="$(mktemp "${TMPDIR:-/tmp}/dabney-upload-keystore.XXXXXX.jks")"
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
  echo "==> No signing keystore provided; building unsigned artifact (suitable for emulator / smoke tests)"
fi

# Patch generated Gradle + Manifest for Play Store 16 KB alignment, Nix SDK pinning, release signing, and zero-PHI hygiene.
python3 - "$ROOT_GRADLE" "$APP_GRADLE" "$MANIFEST" "$SIGNED" "$EXPECTED_PACKAGE" <<'PY'
import pathlib
import re
import sys

root_gradle_path = pathlib.Path(sys.argv[1])
gradle_path = pathlib.Path(sys.argv[2])
manifest_path = pathlib.Path(sys.argv[3])
signed = sys.argv[4] == "1"
expected_pkg = sys.argv[5]

# Pin buildToolsVersion across all Android subprojects (:app, :tauri-android,
# :tauri-plugin-opener) to the Nix-provisioned 37.0.0 so AGP never tries to
# auto-download 36.0.0 into the read-only /nix/store SDK directory.
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

# Pin compileSdk / targetSdk to the Nix-provisioned API 36 (meets Google Play 2026 targetSdk requirements).
gradle = re.sub(r"compileSdk\s*=\s*\d+", "compileSdk = 36", gradle)
gradle = re.sub(r"targetSdk\s*=\s*\d+", "targetSdk = 36", gradle)
if "buildToolsVersion" not in gradle:
    gradle = re.sub(
        r"(compileSdk\s*=\s*36)",
        r'\1\n    buildToolsVersion = "37.0.0"',
        gradle,
        count=1,
    )

# Ensure java.util.Properties import for keystore.properties
if "import java.util.Properties" not in gradle:
    gradle = "import java.util.Properties\n" + gradle

# Ensure uncompressed, 16 KB aligned JNI libraries in the AAB/APK packaging block.
if "useLegacyPackaging = false" not in gradle:
    packaging_block = """
    packaging {
        jniLibs {
            useLegacyPackaging = false
        }
    }
"""
    gradle, count = re.subn(
        r"(android\s*\{)",
        r"\1" + packaging_block,
        gradle,
        count=1,
    )
    if count != 1:
        raise SystemExit(f"Could not locate android {{ }} block in {gradle_path}")

# Wire release signingConfigs cleanly from git-ignored keystore.properties.
if signed and 'signingConfigs' not in gradle:
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
    gradle, count = re.subn(
        r"(buildTypes\s*\{)",
        signing_block + "\n    \\1",
        gradle,
        count=1,
    )
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
elif 'if (rootProject.file("keystore.properties").exists())' not in gradle and 'signingConfig = signingConfigs.getByName("release")' in gradle:
    gradle = gradle.replace(
        'signingConfig = signingConfigs.getByName("release")',
        'if (rootProject.file("keystore.properties").exists()) {\n                signingConfig = signingConfigs.getByName("release")\n            }',
        1,
    )

gradle_path.write_text(gradle, encoding="utf-8")

manifest = manifest_path.read_text(encoding="utf-8")
# Enforce no cleartext HTTP and no cloud backup in Play release manifest.
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

# Fail closed if any Health Connect or sensitive PHI-related permissions crept in.
forbidden_permissions = [
    "android.permission.health.",
    "android.permission.CAMERA",
    "android.permission.RECORD_AUDIO",
    "android.permission.ACCESS_FINE_LOCATION",
    "android.permission.ACCESS_COARSE_LOCATION",
    "android.permission.READ_CONTACTS",
    "android.permission.READ_EXTERNAL_STORAGE",
    "android.permission.WRITE_EXTERNAL_STORAGE",
]
for perm in forbidden_permissions:
    if perm in manifest:
        raise SystemExit(f"Forbidden zero-PHI permission found in {manifest_path}: {perm}")

manifest_path.write_text(manifest, encoding="utf-8")
print("Patched Gradle (16 KB packaging" + (", release signing" if signed else "") + ") and verified zero-PHI manifest.")
PY

BUILD_ARGS=("--$MODE")
for target in "${TARGETS[@]}"; do
  BUILD_ARGS+=("--target" "$target")
done

# Ensure AGP 9.x uses the newest Nix-provisioned aapt2 (37.0.0) so
# optimizeUniversalReleaseResources produces valid binary resources.
if [ -n "${ANDROID_HOME:-}" ] && [ -x "$ANDROID_HOME/build-tools/37.0.0/aapt2" ]; then
  export GRADLE_OPTS="-Dorg.gradle.project.android.aapt2FromMavenOverride=$ANDROID_HOME/build-tools/37.0.0/aapt2"
fi

echo "==> Building Android $MODE for targets: ${TARGETS[*]}"
cargo tauri android build "${BUILD_ARGS[@]}"

# Locate built artifact.
if [ "$MODE" = "aab" ]; then
  ARTIFACT="$(find "$ANDROID_DIR/app/build/outputs/bundle" -name "*.aab" | sort | head -n1)"
else
  # Prefer Gradle's freshly built APK and ignore any leftover emulator-signed artifacts.
  ARTIFACT="$(find "$ANDROID_DIR/app/build/outputs/apk" -name "*.apk" ! -name "*-emulator.apk" ! -name "*-aligned.apk" | sort | head -n1)"
fi

if [ -z "$ARTIFACT" ] || [ ! -f "$ARTIFACT" ]; then
  echo "Failed to locate generated .$MODE artifact under $ANDROID_DIR/app/build/outputs" >&2
  exit 1
fi

echo "==> Built artifact: $ARTIFACT ($(du -Lh "$ARTIFACT" | awk '{print $1}'))"

# Verify 16 KB ELF LOAD alignment on all 64-bit native .so libraries inside the artifact.
READELF="${NDK_HOME:?NDK_HOME must be set}/toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-readelf"
if [ ! -x "$READELF" ]; then
  READELF="$(command -v readelf || true)"
fi
if [ -z "$READELF" ]; then
  echo "Missing llvm-readelf / readelf to verify 16 KB ELF page alignment." >&2
  exit 1
fi

python3 - "$ARTIFACT" "$READELF" <<'PY'
import pathlib
import re
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
            if not loads:
                raise SystemExit(f"No ELF LOAD segments found in {member}")
            for line in loads:
                align_str = line.split()[-1]
                align = int(align_str, 16) if align_str.startswith("0x") else int(align_str)
                if align < 16384:
                    raise SystemExit(
                        f"16 KB alignment check FAILED for {member}: LOAD segment align={align_str} (< 0x4000)"
                    )
            checked += 1
            print(f"  [OK] 16 KB aligned: {member} ({len(loads)} LOAD segments >= 0x4000)")

print(f"Verified 16 KB ELF page-size compliance across {checked} 64-bit native library(s).")
PY

# Verify APK badging/permissions via aapt2 when building APK.
if [ "$MODE" = "apk" ] && command -v aapt2 >/dev/null 2>&1; then
  echo "==> Verifying APK manifest & permissions via aapt2"
  BADGING="$(aapt2 dump badging "$ARTIFACT")"
  grep -E "^(package:|sdkVersion:|targetSdkVersion:|application-label:|uses-permission:)" <<< "$BADGING" || true
  if [[ "$BADGING" != *"package: name='$EXPECTED_PACKAGE'"* ]]; then
    echo "Unexpected APK package name (expected $EXPECTED_PACKAGE)" >&2
    exit 1
  fi
  if grep -qE "android\.permission\.(health\.|CAMERA|RECORD_AUDIO|ACCESS_FINE_LOCATION|ACCESS_COARSE_LOCATION|READ_CONTACTS)" <<< "$BADGING"; then
    echo "Forbidden permission detected in built APK!" >&2
    exit 1
  fi
fi

# Verify cryptographic signature if release-signed; for unsigned local APKs,
# sign with the standard local debug keystore so `adb install` works immediately.
if [ "$SIGNED" -eq 1 ]; then
  echo "==> Verifying release signature"
  if [ "$MODE" = "aab" ]; then
    # Google Play upload certificates are self-signed (Play App Signing re-signs
    # with Google's distribution key), so omit `-strict` which fails on self-signed certs.
    VERIFY_OUT="$(jarsigner -verify "$ARTIFACT")"
    head -n 10 <<< "$VERIFY_OUT"
    if [[ "$VERIFY_OUT" != *"jar verified."* ]]; then
      echo "AAB signature verification failed!" >&2
      exit 1
    fi
  elif command -v apksigner >/dev/null 2>&1; then
    apksigner verify --print-certs "$ARTIFACT"
  else
    jarsigner -verify "$ARTIFACT"
  fi
elif [ "$MODE" = "apk" ] && [[ "$ARTIFACT" == *-unsigned.apk ]]; then
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
  ALIGNED_APK="${ARTIFACT%-unsigned.apk}-aligned.apk"
  SIGNED_APK="${ARTIFACT%-unsigned.apk}-emulator.apk"
  echo "==> Aligning (16 KB) & debug-signing local emulator APK"
  zipalign -f -P 16 4 "$ARTIFACT" "$ALIGNED_APK"
  apksigner sign \
    --ks "$DEBUG_KS" \
    --ks-key-alias androiddebugkey \
    --ks-pass pass:android \
    --key-pass pass:android \
    --out "$SIGNED_APK" \
    "$ALIGNED_APK"
  rm -f "$ALIGNED_APK"
  apksigner verify --print-certs "$SIGNED_APK"
  ARTIFACT="$SIGNED_APK"
fi

RELEASE_OUT_DIR="$ROOT/target/android-release"
mkdir -p "$RELEASE_OUT_DIR"
if [ "$MODE" = "aab" ]; then
  if [ "$SIGNED" -eq 1 ]; then
    OUT_FILE="$RELEASE_OUT_DIR/dabney-demo-release.aab"
  else
    OUT_FILE="$RELEASE_OUT_DIR/dabney-demo-unsigned.aab"
  fi
else
  if [ "$SIGNED" -eq 1 ]; then
    OUT_FILE="$RELEASE_OUT_DIR/dabney-demo-release.apk"
  else
    OUT_FILE="$RELEASE_OUT_DIR/dabney-demo-emulator.apk"
  fi
fi
cp -f "$ARTIFACT" "$OUT_FILE"
sha256sum "$OUT_FILE" > "$OUT_FILE.sha256"

echo "==> Android $MODE build and Google Play compliance checks passed:"
echo "    $OUT_FILE"
echo "    $(cat "$OUT_FILE.sha256")"
