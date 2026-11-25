#!/usr/bin/env bash
# Reproducible macOS/Xcode builder for the opt-in PERSONAL Dabney.moe iOS app
# (`moe.dabney.personal`) with native Apple HealthKit blood glucose sync.
#
# Strictly separate from the zero-PHI public demo (`moe.dabney.demo`).
# Must be run on macOS with Xcode + Apple signing team configured:
#   scripts/ios-personal.sh --build                  # build personal iOS .ipa / Xcode project
#   scripts/ios-personal.sh --open                   # initialize, patch Info.plist/entitlements, open in Xcode
set -euo pipefail

if [ "$(uname -s)" != "Darwin" ]; then
  echo "scripts/ios-personal.sh requires macOS with Xcode installed." >&2
  exit 2
fi

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

ACTION="build"
CLEAN_INIT=0
PERSONAL_CONFIG="$ROOT/src-tauri/tauri.ios.personal.conf.json"
IOS_SRC_DIR="$ROOT/src-tauri/ios-personal"
IOS_GEN_DIR="$ROOT/src-tauri/gen/apple"

while [ "$#" -gt 0 ]; do
  case "$1" in
    --build)
      ACTION="build"
      shift
      ;;
    --open)
      ACTION="open"
      shift
      ;;
    --clean-init)
      CLEAN_INIT=1
      shift
      ;;
    -h|--help)
      sed -n '2,9p' "$0" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      exit 2
      ;;
  esac
done

if [ "$CLEAN_INIT" -eq 1 ] || [ ! -d "$IOS_GEN_DIR" ]; then
  echo "==> Initializing Tauri iOS project for personal flavor moe.dabney.personal"
  rm -rf "$IOS_GEN_DIR"
  cargo tauri ios init -c "$PERSONAL_CONFIG"
fi

# Copy clean-room HealthKit entitlements and Swift plugin source into the generated iOS project.
cp -f "$IOS_SRC_DIR/Personal.entitlements" "$IOS_GEN_DIR/dabney-app_iOS/dabney-app_iOS.entitlements"
mkdir -p "$IOS_GEN_DIR/Sources/dabney-app/HealthKit"
cp -f "$IOS_SRC_DIR/Sources/HealthKitPlugin.swift" "$IOS_GEN_DIR/Sources/dabney-app/HealthKit/HealthKitPlugin.swift"

# Inject read-only NSHealthShareUsageDescription into Info.plist (never write-access NSHealthUpdateUsageDescription).
INFO_PLIST="$IOS_GEN_DIR/dabney-app_iOS/Info.plist"
if [ -f "$INFO_PLIST" ]; then
  /usr/libexec/PlistBuddy -c "Delete :NSHealthShareUsageDescription" "$INFO_PLIST" 2>/dev/null || true
  /usr/libexec/PlistBuddy -c "Add :NSHealthShareUsageDescription string 'Dabney.moe Personal reads your Apple Health Blood Glucose records (such as Abbott Lingo) strictly on-device in RAM to compute ADA Time-in-Range and 24-hour AGP charts. Data is never uploaded or shared.'" "$INFO_PLIST"
  if /usr/libexec/PlistBuddy -c "Print :NSHealthUpdateUsageDescription" "$INFO_PLIST" >/dev/null 2>&1; then
    echo "Refusing to build personal iOS app with write-access NSHealthUpdateUsageDescription!" >&2
    exit 1
  fi
fi

if [ "$ACTION" = "open" ]; then
  echo "==> Opening patched Xcode project (enable HealthKit capability & select signing team if prompted)"
  cargo tauri ios build --open -c "$PERSONAL_CONFIG" --features health-connect
else
  echo "==> Building personal iOS app (moe.dabney.personal, --features health-connect)"
  cargo tauri ios build -c "$PERSONAL_CONFIG" --features health-connect
fi
