#!/usr/bin/env bash
# Offline checks for scripts/deploy-serv00.sh and selfhost/serv00/watchdog.sh.
# Requires no SSH network access and modifies nothing outside a temp directory.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
deploy="$root/scripts/deploy-serv00.sh"
watchdog="$root/selfhost/serv00/watchdog.sh"

work=$(mktemp -d "${TMPDIR:-/tmp}/test-deploy-serv00.XXXXXX")
trap 'rm -rf "$work"' EXIT

failures=0
pass() { echo "ok   - $1"; }
fail() { echo "FAIL - $1" >&2; failures=$((failures + 1)); }

# 1. Syntax checks.
bash -n "$deploy" && pass "deploy-serv00.sh syntax valid" || fail "deploy-serv00.sh syntax error"
sh -n "$watchdog" && pass "watchdog.sh POSIX sh syntax valid" || fail "watchdog.sh syntax error"

# 2. Dry-run happy path and refusal cases.
run_deploy() {
  local name=$1 expected=$2 status=0
  shift 2
  env -u SERV00_HOST -u SERV00_USER -u SERV00_DOMAIN -u SERV00_PORT -u SERV00_WEB_IP \
    bash "$deploy" "$@" > "$work/$name.out" 2>&1 || status=$?
  if [ "$status" -eq "$expected" ]; then
    pass "$name exits $expected"
  else
    fail "$name exited $status, expected $expected: $(cat "$work/$name.out")"
  fi
}

run_deploy dry 0 --host s14.serv00.com --user tyler --domain git.dabney.moe --port 45123 --serv00-ip 198.51.100.10
grep -Fq 'devil www proxy git.dabney.moe -> localhost:45123' "$work/dry.out" && pass "dry-run prints proxy target" || fail "dry-run missing proxy target"
grep -Fq 'devil ssl www add 198.51.100.10 le le git.dabney.moe' "$work/dry.out" && pass "dry-run prints TLS target" || fail "dry-run missing TLS target"

run_deploy bad-port 2 --host s14.serv00.com --user tyler --domain git.dabney.moe --port 80
run_deploy bad-host 2 --host 'bad host' --user tyler --domain git.dabney.moe --port 45123
run_deploy bad-ip 2 --host s14.serv00.com --user tyler --domain git.dabney.moe --port 45123 --serv00-ip '1.2.3.4;id'
run_deploy missing-bin 1 --host s14.serv00.com --user tyler --domain git.dabney.moe --port 45123 --bin "$work/nope" --push

# 3. Watchdog offline lifecycle & secrets-permission enforcement.
cfg="$work/cfg"
data="$work/data"
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

# Refuse group/world-readable secrets.env.
printf 'LINEAR_API_KEY=not-real\n' > "$cfg/secrets.env"
chmod 0644 "$cfg/secrets.env"
if DABNEY_FORGE_CONFIG_DIR="$cfg" sh "$watchdog" ensure >/dev/null 2>&1; then
  fail "watchdog accepted mode 0644 secrets.env"
else
  pass "watchdog rejects mode 0644 secrets.env"
fi

chmod 0600 "$cfg/secrets.env"
DABNEY_FORGE_CONFIG_DIR="$cfg" sh "$watchdog" ensure >/dev/null
pid1=$(cat "$data/forge.pid")
pass "watchdog started stub forge (pid $pid1)"

DABNEY_FORGE_CONFIG_DIR="$cfg" sh "$watchdog" status | grep -Fq "pid $pid1" && pass "watchdog status reports live pid" || fail "watchdog status mismatch"

# Calling ensure again must keep the exact same PID (no duplicate instances).
DABNEY_FORGE_CONFIG_DIR="$cfg" sh "$watchdog" ensure >/dev/null
pid2=$(cat "$data/forge.pid")
[ "$pid1" = "$pid2" ] && pass "watchdog ensure is idempotent (pid $pid2)" || fail "watchdog respawned ($pid1 -> $pid2)"

grep -Fq '127.0.0.1:45123' "$data/last-args.txt" && pass "watchdog passes reserved loopback listen address" || fail "watchdog missing listen address"

DABNEY_FORGE_CONFIG_DIR="$cfg" sh "$watchdog" stop >/dev/null
if DABNEY_FORGE_CONFIG_DIR="$cfg" sh "$watchdog" status >/dev/null 2>&1; then
  fail "watchdog status still running after stop"
else
  pass "watchdog cleanly stops process"
fi

if [ "$failures" -ne 0 ]; then
  echo "$failures check(s) failed" >&2
  exit 1
fi
echo "all serv00 deploy checks passed"
