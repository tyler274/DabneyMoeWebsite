#!/bin/sh
# FreeBSD / POSIX supervisor watchdog for `dabney-forge` on Serv00 (and any
# non-systemd shared shell host).
#
# Usage:
#   watchdog.sh [ensure|start|stop|restart|status]
#
# Configuration files (overridable via DABNEY_FORGE_CONFIG_DIR):
#   $CONFIG_DIR/config.env   non-secret settings (DABNEY_FORGE_PORT, DABNEY_FORGE_DOMAIN, ...)
#   $CONFIG_DIR/secrets.env  optional secrets (must be mode 0600; e.g. webhook/mirror/Linear tokens)
#   $CONFIG_DIR/sync.toml    optional forge-linear-sync config
set -eu

action="${1:-ensure}"
case "$action" in
  ensure|start|stop|restart|status) ;;
  *)
    echo "usage: $0 [ensure|start|stop|restart|status]" >&2
    exit 2
    ;;
esac

CONFIG_DIR="${DABNEY_FORGE_CONFIG_DIR:-$HOME/.config/dabney-forge}"
CONFIG_FILE="$CONFIG_DIR/config.env"
SECRETS_FILE="$CONFIG_DIR/secrets.env"
SYNC_CONFIG="$CONFIG_DIR/sync.toml"

# Enforce private permissions on secrets.env before sourcing it.
check_private_file() {
  target="$1"
  if [ ! -e "$target" ]; then
    return 0
  fi
  if stat -f '%Lp' "$target" >/dev/null 2>&1; then
    mode=$(stat -L -f '%Lp' "$target")
  else
    mode=$(stat -L -c '%a' "$target")
  fi
  case "$mode" in
    600|400) return 0 ;;
    *)
      echo "error: refusing to load $target with mode $mode (must be 600 or 400)" >&2
      exit 1
      ;;
  esac
}

if [ -f "$CONFIG_FILE" ]; then
  set -a
  # shellcheck disable=SC1090
  . "$CONFIG_FILE"
  set +a
fi

if [ -f "$SECRETS_FILE" ]; then
  check_private_file "$SECRETS_FILE"
  set -a
  # shellcheck disable=SC1090
  . "$SECRETS_FILE"
  set +a
fi

FORGE_BIN="${DABNEY_FORGE_BIN:-$HOME/bin/dabney-forge}"
FORGE_DATA="${DABNEY_FORGE_DATA:-$HOME/forge-data}"
FORGE_PORT="${DABNEY_FORGE_PORT:-}"
FORGE_DOMAIN="${DABNEY_FORGE_DOMAIN:-git.dabney.moe}"
FORGE_BASE_URL="${DABNEY_FORGE_BASE_URL:-https://$FORGE_DOMAIN}"
FORGE_LOG="${DABNEY_FORGE_LOG:-$FORGE_DATA/forge.log}"
FORGE_PID_FILE="${DABNEY_FORGE_PID_FILE:-$FORGE_DATA/forge.pid}"
FORGE_LOCK_FILE="${DABNEY_FORGE_LOCK_FILE:-$FORGE_DATA/watchdog.lock}"
MAX_LOG_BYTES="${DABNEY_FORGE_MAX_LOG_BYTES:-5242880}"

mkdir -p "$FORGE_DATA"

# Serialize concurrent cron ticks without spawning duplicate instances.
if [ "${DABNEY_FORGE_LOCKED:-0}" != "1" ] && [ "$action" != "status" ]; then
  export DABNEY_FORGE_LOCKED=1
  if command -v lockf >/dev/null 2>&1; then
    exec lockf -s -t 0 "$FORGE_LOCK_FILE" "$0" "$action"
  elif command -v flock >/dev/null 2>&1; then
    exec flock -n -o "$FORGE_LOCK_FILE" "$0" "$action"
  fi
fi

expected_comm=$(basename "$FORGE_BIN")

is_running() {
  if [ ! -f "$FORGE_PID_FILE" ]; then
    return 1
  fi
  pid=$(cat "$FORGE_PID_FILE" 2>/dev/null || true)
  case "$pid" in
    ''|*[!0-9]*) return 1 ;;
  esac
  if ! kill -0 "$pid" 2>/dev/null; then
    return 1
  fi
  # Guard against PID reuse after an unclean exit.
  actual_comm=$(ps -p "$pid" -o comm= 2>/dev/null | awk '{print $1}' || true)
  actual_base=$(basename "${actual_comm:-}" 2>/dev/null || true)
  case "$actual_base" in
    "$expected_comm")
      return 0
      ;;
    *)
      # Fallback to full args check when comm is truncated (e.g. 15-char Linux comm) or wrapped.
      actual_args=$(ps -p "$pid" -o args= 2>/dev/null || true)
      case "$actual_args" in
        *"$FORGE_BIN"*|*"$expected_comm"*) return 0 ;;
        *) return 1 ;;
      esac
      ;;
  esac
}

rotate_log() {
  if [ ! -f "$FORGE_LOG" ]; then
    return 0
  fi
  size=$(wc -c < "$FORGE_LOG" 2>/dev/null | tr -d ' ' || echo 0)
  if [ "$size" -ge "$MAX_LOG_BYTES" ]; then
    mv -f "$FORGE_LOG" "$FORGE_LOG.1"
  fi
}

stop_forge() {
  if ! is_running; then
    rm -f "$FORGE_PID_FILE"
    return 0
  fi
  pid=$(cat "$FORGE_PID_FILE")
  kill "$pid" 2>/dev/null || true
  i=0
  while [ "$i" -lt 25 ]; do
    if ! kill -0 "$pid" 2>/dev/null; then
      rm -f "$FORGE_PID_FILE"
      return 0
    fi
    sleep 0.2
    i=$((i + 1))
  done
  kill -9 "$pid" 2>/dev/null || true
  rm -f "$FORGE_PID_FILE"
}

start_forge() {
  if is_running; then
    return 0
  fi
  if [ ! -x "$FORGE_BIN" ]; then
    echo "error: forge binary not executable: $FORGE_BIN" >&2
    exit 1
  fi
  case "$FORGE_PORT" in
    ''|*[!0-9]*)
      echo "error: DABNEY_FORGE_PORT must be set to a numeric reserved port" >&2
      exit 1
      ;;
  esac
  rotate_log
  set -- --data-dir "$FORGE_DATA" serve \
    --listen "127.0.0.1:$FORGE_PORT" \
    --base-url "$FORGE_BASE_URL"
  if [ -n "${DABNEY_FORGE_WEBHOOK_URL:-}" ]; then
    set -- "$@" --webhook-url "$DABNEY_FORGE_WEBHOOK_URL"
  fi
  if [ -f "$SYNC_CONFIG" ]; then
    set -- "$@" --sync-config "$SYNC_CONFIG" --sync-db "$FORGE_DATA/forge-linear-sync.sqlite"
  fi
  nohup "$FORGE_BIN" "$@" >> "$FORGE_LOG" 2>&1 </dev/null &
  new_pid=$!
  printf '%s\n' "$new_pid" > "$FORGE_PID_FILE"
  sleep 1
  if ! is_running; then
    echo "error: dabney-forge exited immediately; see $FORGE_LOG" >&2
    rm -f "$FORGE_PID_FILE"
    exit 1
  fi
}

case "$action" in
  status)
    if is_running; then
      echo "running (pid $(cat "$FORGE_PID_FILE"), listen 127.0.0.1:${FORGE_PORT:-?}, base $FORGE_BASE_URL)"
      exit 0
    else
      echo "stopped"
      exit 3
    fi
    ;;
  stop)
    stop_forge
    echo "stopped"
    ;;
  start|ensure)
    start_forge
    echo "running (pid $(cat "$FORGE_PID_FILE"))"
    ;;
  restart)
    stop_forge
    start_forge
    echo "running (pid $(cat "$FORGE_PID_FILE"))"
    ;;
esac
