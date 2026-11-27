#!/usr/bin/env bash
# Offline checks for scripts/deploy-serv00.sh, selfhost/serv00/watchdog.sh, and
# selfhost/serv00/sync.json.example. Requires no SSH network access and modifies
# nothing outside a temp directory.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
deploy="$root/scripts/deploy-serv00.sh"
watchdog="$root/selfhost/serv00/watchdog.sh"
sync_example="$root/selfhost/serv00/sync.json.example"

work=$(mktemp -d "${TMPDIR:-/tmp}/test-deploy-serv00.XXXXXX")
trap 'rm -rf "$work"' EXIT

failures=0
pass() { echo "ok   - $1"; }
fail() { echo "FAIL - $1" >&2; failures=$((failures + 1)); }

# 1. Syntax & JSON checks.
bash -n "$deploy" && pass "deploy-serv00.sh syntax valid" || fail "deploy-serv00.sh syntax error"
sh -n "$watchdog" && pass "watchdog.sh POSIX sh syntax valid" || fail "watchdog.sh syntax error"
if command -v python3 >/dev/null 2>&1; then
  python3 -m json.tool "$sync_example" >/dev/null && pass "sync.json.example is valid JSON" || fail "sync.json.example invalid JSON"
fi

# 2. Dry-run happy paths and refusal cases.
run_deploy() {
  local name=$1 expected=$2 status=0
  shift 2
  env -u SERV00_SERVICE -u SERV00_HOST -u SERV00_USER -u SERV00_DOMAIN -u SERV00_PORT -u SERV00_WEB_IP -u SERV00_SITE_DIR -u SERV00_HASH_FILE \
    bash "$deploy" "$@" > "$work/$name.out" 2>&1 || status=$?
  if [ "$status" -eq "$expected" ]; then
    pass "$name exits $expected"
  else
    fail "$name exited $status, expected $expected: $(cat "$work/$name.out")"
  fi
}

run_deploy dry-forge 0 --host s14.serv00.com --user tyler --domain git.dabney.moe --port 45123 --serv00-ip 198.51.100.10
grep -Fq 'Service: dabney-forge' "$work/dry-forge.out" && pass "forge dry-run names dabney-forge" || fail "forge dry-run missing service"
grep -Fq 'devil www proxy git.dabney.moe -> localhost:45123' "$work/dry-forge.out" && pass "forge dry-run prints proxy target" || fail "forge dry-run missing proxy target"
grep -Fq 'devil ssl www add 198.51.100.10 le le git.dabney.moe' "$work/dry-forge.out" && pass "forge dry-run prints TLS target" || fail "forge dry-run missing TLS target"

run_deploy dry-web 0 --service web --host s14.serv00.com --user tyler --domain dabney.moe --port 45124 --serv00-ip 198.51.100.10
grep -Fq 'Service: dabney-web' "$work/dry-web.out" && pass "web dry-run names dabney-web" || fail "web dry-run missing service"
grep -Fq 'hash.txt.new' "$work/dry-web.out" && pass "web dry-run uploads hash.txt beside web binary" || fail "web dry-run missing hash.txt upload"
grep -Fq 'web/site.new' "$work/dry-web.out" && pass "web dry-run streams site tarball" || fail "web dry-run missing site tarball"
grep -Fq 'devil www proxy dabney.moe -> localhost:45124' "$work/dry-web.out" && pass "web dry-run prints proxy target" || fail "web dry-run missing proxy target"

run_deploy bad-service 2 --service unknown --host s14.serv00.com --user tyler --domain git.dabney.moe --port 45123
run_deploy bad-port 2 --host s14.serv00.com --user tyler --domain git.dabney.moe --port 80
run_deploy bad-host 2 --host 'bad host' --user tyler --domain git.dabney.moe --port 45123
run_deploy bad-ip 2 --host s14.serv00.com --user tyler --domain git.dabney.moe --port 45123 --serv00-ip '1.2.3.4;id'
run_deploy missing-bin 1 --host s14.serv00.com --user tyler --domain git.dabney.moe --port 45123 --bin "$work/nope" --push

# Missing web site assets or hash.txt must fail before touching SSH.
fake_bin="$work/fake-web"
printf '#!/bin/sh\n' > "$fake_bin"
chmod 0755 "$fake_bin"
run_deploy web-missing-site 1 --service web --host s14.serv00.com --user tyler --domain dabney.moe --port 45124 --bin "$fake_bin" --allow-any-bin --site-dir "$work/no-site" --hash-file "$work/no-hash" --push
mkdir -p "$work/empty-site"
run_deploy web-missing-hash 1 --service web --host s14.serv00.com --user tyler --domain dabney.moe --port 45124 --bin "$fake_bin" --allow-any-bin --site-dir "$work/empty-site" --hash-file "$work/no-hash" --push

# 3. Forge watchdog offline lifecycle, sync.json wiring, and secrets-permission enforcement.
cfg="$work/cfg-forge"
data="$work/data-forge"
mkdir -p "$cfg" "$data"

stub="$work/dabney-forge"
cat > "$stub" <<'STUB'
#!/bin/sh
printf '%s\n' "$@" > "${DABNEY_FORGE_DATA}/last-args.txt"
trap 'exit 0' TERM INT
while :; do
  sleep 1
done
STUB
chmod 0755 "$stub"

cat > "$cfg/config.env" <<EOF
DABNEY_FORGE_BIN=$stub
DABNEY_FORGE_DATA=$data
DABNEY_FORGE_PORT=45123
DABNEY_FORGE_DOMAIN=git.dabney.moe
EOF
cp "$sync_example" "$cfg/sync.json"

# Refuse group/world-readable secrets.env.
printf 'LINEAR_API_KEY=not-real\n' > "$cfg/secrets.env"
chmod 0644 "$cfg/secrets.env"
if DABNEY_FORGE_CONFIG_DIR="$cfg" sh "$watchdog" ensure >/dev/null 2>&1; then
  fail "forge watchdog accepted mode 0644 secrets.env"
else
  pass "forge watchdog rejects mode 0644 secrets.env"
fi

chmod 0600 "$cfg/secrets.env"
DABNEY_FORGE_CONFIG_DIR="$cfg" sh "$watchdog" ensure >/dev/null
pid1=$(cat "$data/forge.pid")
pass "forge watchdog started stub forge (pid $pid1)"

DABNEY_FORGE_CONFIG_DIR="$cfg" sh "$watchdog" status | grep -Fq "pid $pid1" && pass "forge watchdog status reports live pid" || fail "forge watchdog status mismatch"

# Calling ensure again must keep the exact same PID (no duplicate instances).
DABNEY_FORGE_CONFIG_DIR="$cfg" sh "$watchdog" ensure >/dev/null
pid2=$(cat "$data/forge.pid")
[ "$pid1" = "$pid2" ] && pass "forge watchdog ensure is idempotent (pid $pid2)" || fail "forge watchdog respawned ($pid1 -> $pid2)"

grep -Fq '127.0.0.1:45123' "$data/last-args.txt" && pass "forge watchdog passes reserved loopback listen address" || fail "forge watchdog missing listen address"
grep -Fq "$cfg/sync.json" "$data/last-args.txt" && pass "forge watchdog passes sync.json when present" || fail "forge watchdog missing sync.json arg"

DABNEY_FORGE_CONFIG_DIR="$cfg" sh "$watchdog" stop >/dev/null
if DABNEY_FORGE_CONFIG_DIR="$cfg" sh "$watchdog" status >/dev/null 2>&1; then
  fail "forge watchdog status still running after stop"
else
  pass "forge watchdog cleanly stops process"
fi

# 4. Web watchdog offline lifecycle, hash.txt check, and LEPTOS_* env verification.
wcfg="$work/cfg-web"
wdata="$work/data-web"
wbin_dir="$work/web-bin"
wsite="$work/web-site"
mkdir -p "$wcfg" "$wdata" "$wbin_dir" "$wsite"

wstub="$wbin_dir/dabney-web"
cat > "$wstub" <<'WSTUB'
#!/bin/sh
{
  printf 'LEPTOS_OUTPUT_NAME=%s\n' "${LEPTOS_OUTPUT_NAME:-}"
  printf 'LEPTOS_HASH_FILES=%s\n' "${LEPTOS_HASH_FILES:-}"
  printf 'LEPTOS_SITE_ROOT=%s\n' "${LEPTOS_SITE_ROOT:-}"
  printf 'LEPTOS_SITE_PKG_DIR=%s\n' "${LEPTOS_SITE_PKG_DIR:-}"
  printf 'LEPTOS_SITE_ADDR=%s\n' "${LEPTOS_SITE_ADDR:-}"
  printf 'LEPTOS_ENV=%s\n' "${LEPTOS_ENV:-}"
} > "${DABNEY_WEB_DATA}/last-env.txt"
trap 'exit 0' TERM INT
while :; do
  sleep 1
done
WSTUB
chmod 0755 "$wstub"

cat > "$wcfg/config.env" <<EOF
DABNEY_WEB_BIN=$wstub
DABNEY_WEB_DATA=$wdata
DABNEY_WEB_SITE_ROOT=$wsite
DABNEY_WEB_PORT=45124
DABNEY_WEB_DOMAIN=dabney.moe
EOF

# Must refuse to start when hash.txt is missing beside the web binary.
if DABNEY_SERV00_SERVICE=web DABNEY_WEB_CONFIG_DIR="$wcfg" sh "$watchdog" ensure >/dev/null 2>&1; then
  fail "web watchdog started without hash.txt beside binary"
else
  pass "web watchdog refuses missing hash.txt beside binary"
fi

printf 'dummy-hash\n' > "$wbin_dir/hash.txt"
DABNEY_SERV00_SERVICE=web DABNEY_WEB_CONFIG_DIR="$wcfg" sh "$watchdog" ensure >/dev/null
wpid1=$(cat "$wdata/web.pid")
pass "web watchdog started stub web (pid $wpid1)"

DABNEY_SERV00_SERVICE=web DABNEY_WEB_CONFIG_DIR="$wcfg" sh "$watchdog" ensure >/dev/null
wpid2=$(cat "$wdata/web.pid")
[ "$wpid1" = "$wpid2" ] && pass "web watchdog ensure is idempotent (pid $wpid2)" || fail "web watchdog respawned ($wpid1 -> $wpid2)"

grep -Fq 'LEPTOS_SITE_ADDR=127.0.0.1:45124' "$wdata/last-env.txt" && pass "web watchdog exports LEPTOS_SITE_ADDR" || fail "web watchdog missing LEPTOS_SITE_ADDR"
grep -Fq "LEPTOS_SITE_ROOT=$wsite" "$wdata/last-env.txt" && pass "web watchdog exports LEPTOS_SITE_ROOT" || fail "web watchdog missing LEPTOS_SITE_ROOT"
grep -Fq 'LEPTOS_HASH_FILES=true' "$wdata/last-env.txt" && pass "web watchdog exports LEPTOS_HASH_FILES=true" || fail "web watchdog missing LEPTOS_HASH_FILES"
grep -Fq 'LEPTOS_ENV=PROD' "$wdata/last-env.txt" && pass "web watchdog exports LEPTOS_ENV=PROD" || fail "web watchdog missing LEPTOS_ENV"

DABNEY_SERV00_SERVICE=web DABNEY_WEB_CONFIG_DIR="$wcfg" sh "$watchdog" stop >/dev/null
if DABNEY_SERV00_SERVICE=web DABNEY_WEB_CONFIG_DIR="$wcfg" sh "$watchdog" status >/dev/null 2>&1; then
  fail "web watchdog status still running after stop"
else
  pass "web watchdog cleanly stops process"
fi

if [ "$failures" -ne 0 ]; then
  echo "$failures check(s) failed" >&2
  exit 1
fi
echo "all serv00 deploy checks passed"
