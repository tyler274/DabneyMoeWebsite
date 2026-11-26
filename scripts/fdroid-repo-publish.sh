#!/usr/bin/env bash
# Build, sign, and optionally publish the self-hosted Dabney.moe F-Droid repository
# for `moe.dabney.demo` (Codeberg raw HTTPS: https://codeberg.org/tyler274/fdroid/raw/branch/pages/repo).
#
# Run inside `nix develop .#android` with `nix shell nixpkgs#fdroidserver nixpkgs#qrencode`:
#   scripts/fdroid-repo-publish.sh          # build & verify locally into target/fdroid-repo/
#   scripts/fdroid-repo-publish.sh --push   # also push branch `pages` to codeberg.org:tyler274/fdroid.git
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

PUSH=0
APK_PATH="$ROOT/target/android-release/dabney-demo-release.apk"
REPO_REMOTE="${FDROID_REPO_REMOTE:-git@codeberg.org:tyler274/fdroid.git}"
REPO_URL="${FDROID_REPO_URL:-https://codeberg.org/tyler274/fdroid/raw/branch/pages/repo}"
KEYSTORE_DIR="${HOME}/.config/dabney-android"
REPO_KEYSTORE="${FDROID_REPO_KEYSTORE:-$KEYSTORE_DIR/fdroid-repo-keystore.jks}"
REPO_PASS_FILE="${FDROID_REPO_PASS_FILE:-$KEYSTORE_DIR/fdroid-repo-keystore.pass}"
REPO_ALIAS="${FDROID_REPO_KEY_ALIAS:-dabney-fdroid-repo}"

while [ "$#" -gt 0 ]; do
  case "$1" in
    --push)
      PUSH=1
      shift
      ;;
    --apk)
      APK_PATH="${2:?missing value for --apk}"
      shift 2
      ;;
    *)
      echo "Unknown argument: $1" >&2
      exit 2
      ;;
  esac
done

if [ ! -f "$APK_PATH" ]; then
  echo "Signed release APK not found: $APK_PATH" >&2
  echo "Build it first with scripts/android-release.sh --apk --target aarch64 --target x86_64" >&2
  exit 1
fi

# Ensure dedicated F-Droid repo signing keystore exists outside Git (never committed).
mkdir -p "$KEYSTORE_DIR"
chmod 700 "$KEYSTORE_DIR"
if [ ! -f "$REPO_KEYSTORE" ]; then
  if [ ! -f "$REPO_PASS_FILE" ]; then
    umask 077
    python3 -c 'import secrets; print(secrets.token_urlsafe(24))' > "$REPO_PASS_FILE"
    chmod 600 "$REPO_PASS_FILE"
  fi
  REPO_PASS="$(tr -d '\r\n' < "$REPO_PASS_FILE")"
  echo "==> Generating dedicated F-Droid repo signing keystore at $REPO_KEYSTORE"
  keytool -genkeypair \
    -keystore "$REPO_KEYSTORE" \
    -storetype PKCS12 \
    -alias "$REPO_ALIAS" \
    -keyalg RSA \
    -keysize 4096 \
    -validity 10000 \
    -storepass "$REPO_PASS" \
    -keypass "$REPO_PASS" \
    -dname "CN=Dabney.moe F-Droid Repo, OU=Dabney.moe, O=Dabney.moe, L=Pasadena, ST=CA, C=US"
  chmod 600 "$REPO_KEYSTORE"
else
  REPO_PASS="$(tr -d '\r\n' < "$REPO_PASS_FILE")"
fi

FINGERPRINT="$(keytool -list -v -keystore "$REPO_KEYSTORE" -alias "$REPO_ALIAS" -storepass "$REPO_PASS" \
  | awk '/SHA256:/ {gsub(/:/, "", $2); print toupper($2); exit}')"
if [ "${#FINGERPRINT}" -ne 64 ]; then
  echo "Failed to extract 64-hex SHA-256 fingerprint from $REPO_KEYSTORE" >&2
  exit 1
fi
echo "==> F-Droid repo signing fingerprint: $FINGERPRINT"

STAGE="$ROOT/target/fdroid-repo"
trap 'rm -f "${STAGE:-}/config.yml"' EXIT
rm -rf "$STAGE"
mkdir -p "$STAGE/repo/icons" "$STAGE/metadata/moe.dabney.demo/en-US/phoneScreenshots" "$STAGE/metadata/moe.dabney.demo/en-US/changelogs"

# Copy signed universal APK (named <package>_<versionCode>.apk per F-Droid convention).
VERSION_CODE="$(python3 -c 'import json; print(json.load(open("src-tauri/tauri.android.conf.json"))["bundle"]["android"]["versionCode"])')"
VERSION_NAME="$(python3 -c 'import json; print(json.load(open("src-tauri/tauri.android.conf.json"))["version"])')"
APK_NAME="moe.dabney.demo_${VERSION_CODE}.apk"
cp -f "$APK_PATH" "$STAGE/repo/$APK_NAME"

# Copy repo icon (F-Droid expects repo_icon inside repo/icons/) and Fastlane metadata.
cp -f "$ROOT/fastlane/metadata/android/en-US/images/icon.png" "$STAGE/repo/icons/repo-icon.png"
cp -f "$ROOT/fastlane/metadata/android/en-US/title.txt" "$STAGE/metadata/moe.dabney.demo/en-US/title.txt"
cp -f "$ROOT/fastlane/metadata/android/en-US/short_description.txt" "$STAGE/metadata/moe.dabney.demo/en-US/short_description.txt"
cp -f "$ROOT/fastlane/metadata/android/en-US/full_description.txt" "$STAGE/metadata/moe.dabney.demo/en-US/full_description.txt"
cp -f "$ROOT/fastlane/metadata/android/en-US/changelogs/${VERSION_CODE}.txt" "$STAGE/metadata/moe.dabney.demo/en-US/changelogs/${VERSION_CODE}.txt"
cp -f "$ROOT/fastlane/metadata/android/en-US/images/icon.png" "$STAGE/metadata/moe.dabney.demo/en-US/icon.png"
cp -f "$ROOT/fastlane/metadata/android/en-US/images/featureGraphic.png" "$STAGE/metadata/moe.dabney.demo/en-US/featureGraphic.png"
cp -f "$ROOT/fastlane/metadata/android/en-US/images/phoneScreenshots/"*.png "$STAGE/metadata/moe.dabney.demo/en-US/phoneScreenshots/"

cat > "$STAGE/metadata/moe.dabney.demo.yml" <<EOF
Categories:
  - Science & Education
  - Sports & Health
License: MIT
AuthorName: Tyler Port
AuthorEmail: tp@dabney.moe
AuthorWebSite: https://dabney.moe
WebSite: https://dabney.moe/clinical
SourceCode: https://codeberg.org/tyler274/dabney-moe-website
IssueTracker: https://codeberg.org/tyler274/dabney-moe-website/issues
Changelog: https://codeberg.org/tyler274/dabney-moe-website/releases
AutoName: Dabney.moe Clinical Demo
CurrentVersion: ${VERSION_NAME}
CurrentVersionCode: ${VERSION_CODE}
EOF

umask 077
cat > "$STAGE/config.yml" <<EOF
repo_url: "${REPO_URL}"
repo_name: "Dabney.moe F-Droid Repository"
repo_icon: "repo-icon.png"
repo_description: >-
  Official F-Droid repository for Dabney.moe Clinical Demo (moe.dabney.demo) —
  100% synthetic zero-PHI clinical & CGM workflow demo built in pure Rust/WebAssembly
  (MIT License, zero trackers, zero ads).
archive_older: 0
keystore: "${REPO_KEYSTORE}"
keystorepass: "${REPO_PASS}"
repo_keyalias: "${REPO_ALIAS}"
keypass: "${REPO_PASS}"
keydname: "CN=Dabney.moe F-Droid Repo, OU=Dabney.moe, O=Dabney.moe, L=Pasadena, ST=CA, C=US"
EOF
chmod 600 "$STAGE/config.yml"

echo "==> Running fdroid update to generate signed index-v1.jar, index-v2.json, and entry.jar"
(cd "$STAGE" && fdroid update --pretty)

# Verify expected F-Droid v1 & v2 signed index artifacts exist.
for req in "$STAGE/repo/index-v1.jar" "$STAGE/repo/index-v2.json" "$STAGE/repo/entry.jar" "$STAGE/repo/$APK_NAME"; do
  if [ ! -f "$req" ]; then
    echo "Missing expected F-Droid repository artifact: $req" >&2
    exit 1
  fi
done

FDROID_URI="fdroidrepos://${REPO_URL#https://}?fingerprint=${FINGERPRINT}"
echo "$FINGERPRINT" > "$STAGE/fingerprint.txt"
echo "$FDROID_URI" > "$STAGE/fdroid-uri.txt"

# Generate crisp scannable SVG QR code for dabney.moe/android and the Codeberg Pages landing page.
if command -v qrencode >/dev/null 2>&1; then
  qrencode -t SVG -l M -m 3 --svg-path -o "$ROOT/public/fdroid-repo-qr.svg" "$FDROID_URI"
  echo "Wrote QR SVG via qrencode to $ROOT/public/fdroid-repo-qr.svg"
else
  python3 - "$FDROID_URI" "$ROOT/public/fdroid-repo-qr.svg" <<'PY'
import sys
import qrcode
import qrcode.image.svg

uri, out_path = sys.argv[1], sys.argv[2]
qr = qrcode.QRCode(
    version=None,
    error_correction=qrcode.constants.ERROR_CORRECT_M,
    box_size=10,
    border=3,
)
qr.add_data(uri)
qr.make(fit=True)
img = qr.make_image(image_factory=qrcode.image.svg.SvgPathImage)
svg = img.to_string(encoding="unicode")
svg = svg.replace("<svg ", '<svg style="background:#ffffff;border-radius:12px" ', 1)
with open(out_path, "w", encoding="utf-8") as f:
    f.write(svg)
print(f"Wrote QR SVG to {out_path} ({len(svg)} bytes)")
PY
fi

# Assemble clean static publish directory (ONLY public repo/ + landing index.html — NEVER config.yml or keys!).
PUB="$ROOT/target/fdroid-pages-publish"
rm -rf "$PUB"
mkdir -p "$PUB"
cp -a "$STAGE/repo" "$PUB/repo"
cp -f "$ROOT/public/fdroid-repo-qr.svg" "$PUB/fdroid-repo-qr.svg"

APK_SHA256="$(sha256sum "$APK_PATH" | awk '{print $1}')"
cat > "$PUB/index.html" <<EOF
<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Dabney.moe F-Droid Repository</title>
  <style>
    body { font-family: system-ui, -apple-system, sans-serif; background: #01081b; color: #e2e8f0; max-width: 680px; margin: 2rem auto; padding: 0 1.25rem; line-height: 1.6; }
    .card { background: #0f172a; border: 1px solid rgba(56,189,248,.25); border-radius: 16px; padding: 1.5rem; margin: 1.25rem 0; }
    a.btn { display: inline-block; background: #10b981; color: #020617; font-weight: 700; text-decoration: none; padding: .75rem 1.25rem; border-radius: 999px; margin: .5rem .5rem .5rem 0; }
    a.btn.alt { background: #22d3ee; }
    code { background: #020617; padding: .2rem .45rem; border-radius: 6px; word-break: break-all; font-size: .85rem; }
    .qr { width: 220px; height: 220px; display: block; margin: 1rem 0; }
  </style>
</head>
<body>
  <h1>Dabney.moe F-Droid Repository</h1>
  <p>Official cryptographically signed F-Droid repository for <strong>Dabney.moe Clinical Demo (<code>moe.dabney.demo</code>)</strong> — 100% synthetic zero-PHI clinical &amp; CGM analytics demo built in pure Rust/WebAssembly (MIT License, zero trackers, zero ads).</p>
  <div class="card">
    <h2>Add to F-Droid / Droid-ify / Neo Store</h2>
    <p><a class="btn" href="${FDROID_URI}">Add Repository in F-Droid</a> <a class="btn alt" href="./repo/${APK_NAME}">Download Signed APK (v${VERSION_NAME})</a></p>
    <img class="qr" src="./fdroid-repo-qr.svg" alt="QR code to add Dabney.moe F-Droid repository">
    <p><strong>Repo URL:</strong> <code>${REPO_URL}</code></p>
    <p><strong>Signing Fingerprint (SHA-256):</strong><br><code>${FINGERPRINT}</code></p>
    <p><strong>APK SHA-256 (v${VERSION_NAME}):</strong><br><code>${APK_SHA256}</code></p>
  </div>
  <p>Full guide, in-browser demos, and privacy policy: <a style="color:#38bdf8" href="https://dabney.moe/android">https://dabney.moe/android</a></p>
</body>
</html>
EOF

# Remove sensitive staging config.yml after generating signed artifacts.
rm -f "$STAGE/config.yml"

echo "==> Built signed F-Droid repository in $PUB"
ls -la "$PUB/repo"

if [ "$PUSH" -eq 1 ]; then
  echo "==> Publishing branch 'pages' to $REPO_REMOTE"
  (
    cd "$PUB"
    git init -q -b pages
    git config user.name "Tyler Port"
    git config user.email "tp@dabney.moe"
    git add -A
    git commit -q -m "Publish Dabney.moe F-Droid repo (moe.dabney.demo v${VERSION_NAME}, vc ${VERSION_CODE})"
    git push --force "$REPO_REMOTE" pages:pages
  )
  echo "==> Published! Live URL: $REPO_URL"
  echo "    FDROID_URI=$FDROID_URI"
fi
