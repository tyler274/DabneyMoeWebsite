#!/usr/bin/env bash
# Headless Android emulator verification + Play Store screenshot capture for
# `moe.dabney.demo` (Dabney.moe Clinical Demo).
#
# Run inside `nix develop .#android`:
#   scripts/verify-android-emulator.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

APK="${APK_PATH:-$ROOT/target/android-release/dabney-demo-emulator.apk}"
OUT_DIR="${OUT_DIR:-$ROOT/target/android-release/play-store-assets}"
AVD_NAME="${AVD_NAME:-dabney-demo-verify}"
EMU_PORT="${EMU_PORT:-5570}"
SERIAL="emulator-${EMU_PORT}"
PACKAGE="moe.dabney.demo"
EMU_PID=""

if [ ! -f "$APK" ]; then
  echo "==> Emulator APK missing; building via scripts/android-release.sh --apk --target x86_64"
  "$ROOT/scripts/android-release.sh" --apk --target x86_64
fi

mkdir -p "$OUT_DIR"

cleanup() {
  if [ -n "$EMU_PID" ]; then
    adb -s "$SERIAL" emu kill >/dev/null 2>&1 || true
    kill "$EMU_PID" >/dev/null 2>&1 || true
    wait "$EMU_PID" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

if [ ! -d "${HOME}/.android/avd/${AVD_NAME}.avd" ]; then
  echo "==> Creating AVD '$AVD_NAME' (android-34;google_apis;x86_64, pixel_6)"
  echo "no" | avdmanager create avd \
    --force \
    --name "$AVD_NAME" \
    --package "system-images;android-34;google_apis;x86_64" \
    --device "pixel_6" >/dev/null
else
  echo "==> Reusing existing AVD '$AVD_NAME'"
fi

echo "==> Booting headless Android emulator on $SERIAL"
"$ANDROID_HOME/emulator/emulator" \
  -avd "$AVD_NAME" \
  -port "$EMU_PORT" \
  -cores 4 -memory 4096 \
  -no-window -no-audio -no-boot-anim -no-snapshot \
  -gpu swiftshader_indirect -accel auto \
  >"$OUT_DIR/emulator.log" 2>&1 &
EMU_PID=$!

adb -s "$SERIAL" wait-for-device
timeout 300 bash -c "until [ \"\$(adb -s '$SERIAL' shell getprop sys.boot_completed 2>/dev/null | tr -d '\\r')\" = '1' ]; do sleep 2; done"
sleep 5
adb -s "$SERIAL" shell wm dismiss-keyguard >/dev/null 2>&1 || true
adb -s "$SERIAL" shell input keyevent 82 >/dev/null 2>&1 || true
adb -s "$SERIAL" shell settings put global window_animation_scale 0 >/dev/null 2>&1 || true
adb -s "$SERIAL" shell settings put global transition_animation_scale 0 >/dev/null 2>&1 || true
adb -s "$SERIAL" shell settings put global animator_duration_scale 0 >/dev/null 2>&1 || true
adb -s "$SERIAL" shell settings put secure anr_show_background 0 >/dev/null 2>&1 || true
adb -s "$SERIAL" shell settings put global hide_error_dialogs 1 >/dev/null 2>&1 || true

SIZE="$(adb -s "$SERIAL" shell wm size | awk '/Physical size:/ {print $3}' | tr -d '\r')"
W="${SIZE%x*}"
H="${SIZE#*x}"

dismiss_anr() {
  local win
  win="$(adb -s "$SERIAL" shell dumpsys window 2>/dev/null | grep -E 'mCurrentFocus|Application Not Responding' || true)"
  if echo "$win" | grep -q "Application Not Responding"; then
    echo "  dismissing System UI ANR dialog (tapping Wait)"
    adb -s "$SERIAL" shell input tap $((W * 35 / 100)) $((H * 57 / 100)) >/dev/null 2>&1 || true
    sleep 3
    win="$(adb -s "$SERIAL" shell dumpsys window 2>/dev/null | grep -E 'mCurrentFocus|Application Not Responding' || true)"
    if echo "$win" | grep -q "Application Not Responding"; then
      echo "  restarting frozen System UI (tapping Close app)"
      adb -s "$SERIAL" shell input tap $((W * 35 / 100)) $((H * 52 / 100)) >/dev/null 2>&1 || true
      sleep 4
    fi
    return 0
  fi
  return 1
}

echo "==> Installing $APK"
# Uninstall any prior build signed with a different key (e.g. debug vs release upload key).
adb -s "$SERIAL" uninstall "$PACKAGE" >/dev/null 2>&1 || true
adb -s "$SERIAL" install -r "$APK"
adb -s "$SERIAL" shell pm list packages | grep -q "^package:${PACKAGE}$"
# Reset app WebView cache/state so interrupted previous runs never corrupt HTTP Cache.
adb -s "$SERIAL" shell pm clear "$PACKAGE" >/dev/null 2>&1 || true

adb -s "$SERIAL" logcat -c
echo "==> Launching $PACKAGE"
adb -s "$SERIAL" shell am start -W -n "${PACKAGE}/.MainActivity" >/dev/null
sleep 15
for _ in 1 2 3 4 5; do
  if ! dismiss_anr; then
    break
  fi
done
adb -s "$SERIAL" shell am start -W -n "${PACKAGE}/.MainActivity" >/dev/null 2>&1 || true
sleep 10
dismiss_anr || true
sleep 4

PID="$(adb -s "$SERIAL" shell pidof "$PACKAGE" 2>/dev/null | tr -d '\r' || true)"
if [ -z "$PID" ]; then
  echo "App process $PACKAGE is not running after launch; retrying am start once..."
  adb -s "$SERIAL" shell am start -W -n "${PACKAGE}/.MainActivity" >/dev/null 2>&1 || true
  sleep 6
  dismiss_anr || true
  PID="$(adb -s "$SERIAL" shell pidof "$PACKAGE" 2>/dev/null | tr -d '\r' || true)"
fi
if [ -z "$PID" ]; then
  echo "App process $PACKAGE is not running after launch!" >&2
  adb -s "$SERIAL" logcat -d -s AndroidRuntime:E chromium:E RustStdoutStderr:* | tail -n 80 >&2
  exit 1
fi
echo "==> $PACKAGE running with PID $PID"
echo "==> Display size: ${W}x${H}"
# Let initial SVG assets finish painting under software SwiftShader before capture 1.
sleep 3

# Capture 1: Launch screen (Patient Experience Demo)
dismiss_anr || true
adb -s "$SERIAL" exec-out screencap -p > "$OUT_DIR/screenshot-01-patient.png"
echo "  captured $OUT_DIR/screenshot-01-patient.png"

# Capture 2: Scroll down slightly on Patient Demo to show records + sharing controls
adb -s "$SERIAL" shell input swipe $((W / 2)) $((H * 75 / 100)) $((W / 2)) $((H * 22 / 100)) 300
sleep 3
dismiss_anr || true
adb -s "$SERIAL" exec-out screencap -p > "$OUT_DIR/screenshot-02-patient-records.png"
echo "  captured $OUT_DIR/screenshot-02-patient-records.png"

# Capture 3: Tap bottom nav tab 2 ("Physician") — bottom bar sits just above gesture bar (~96% H)
TAB_Y=$((H * 96 / 100))
adb -s "$SERIAL" shell input tap $((W * 38 / 100)) "$TAB_Y"
sleep 4
dismiss_anr || true
adb -s "$SERIAL" exec-out screencap -p > "$OUT_DIR/screenshot-03-physician.png"
echo "  captured $OUT_DIR/screenshot-03-physician.png"

# Capture 4: Tap bottom nav tab 3 ("Mood & CGM")
adb -s "$SERIAL" shell input tap $((W * 63 / 100)) "$TAB_Y"
sleep 4
dismiss_anr || true
adb -s "$SERIAL" exec-out screencap -p > "$OUT_DIR/screenshot-04-mood-cgm.png"
echo "  captured $OUT_DIR/screenshot-04-mood-cgm.png"

# Verify app is still alive and free of AndroidRuntime fatal exceptions.
PID_AFTER="$(adb -s "$SERIAL" shell pidof "$PACKAGE" 2>/dev/null | tr -d '\r' || true)"
if [ -z "$PID_AFTER" ]; then
  echo "App process $PACKAGE crashed during navigation!" >&2
  adb -s "$SERIAL" logcat -d | grep -E "($PACKAGE|FATAL EXCEPTION|SIGSEGV|SIGABRT|Process .* died|chromium:.*CONSOLE)" | tail -n 80 >&2 || true
  exit 1
fi

CRASHES="$(adb -s "$SERIAL" logcat -d -s AndroidRuntime:E | grep -i "$PACKAGE" || true)"
if [ -n "$CRASHES" ]; then
  echo "AndroidRuntime errors detected for $PACKAGE:" >&2
  echo "$CRASHES" >&2
  exit 1
fi

adb -s "$SERIAL" logcat -d -s chromium:I chromium:W chromium:E RustStdoutStderr:* > "$OUT_DIR/app-logcat.txt" || true
echo "==> Emulator verification passed ($PACKAGE PID $PID_AFTER, zero crashes)."
