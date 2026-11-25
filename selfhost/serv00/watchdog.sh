#!/bin/sh
# FreeBSD / POSIX supervisor watchdog for `dabney-forge` and `dabney-web` on
# Serv00 (and any non-systemd shared shell host).
#
# Usage:
#   watchdog.sh [ensure|start|stop|restart|status]
#
# Service selection:
#   DABNEY_SERV00_SERVICE=forge|web  (defaults to `web` when invoked as *web*, else `forge`)
#
# Configuration files (overridable via DABNEY_FORGE_CONFIG_DIR / DABNEY_WEB_CONFIG_DIR):
#   $CONFIG_DIR/config.env   non-secret settings (port, domain, binary/data paths)
#   $CONFIG_DIR/secrets.env  optional secrets (must be mode 0600 or 0400)
#   $CONFIG_DIR/sync.json    (forge only) optional forge-linear-sync JSON config
set -eu

action="${1:-ensure}"
case "$action" in
  ensure|start|stop|restart|status) ;;
  *)
    echo "usage: $0 [ensure|start|stop|restart|status]" >&2
    exit 2
    ;;
esac

if [ -n "${DABNEY_SERV00_SERVICE:-}" ]; then
  SERVICE="$DABNEY_SERV00_SERVICE"
else
  case "$(basename "$0")" in
    *web*) SERVICE="web" ;;
    *) SERVICE="forge" ;;
  esac
fi

case "$SERVICE" in
  forge)
    CONFIG_DIR="${DABNEY_FORGE_CONFIG_DIR:-$HOME/.config/dabney-forge}"
    ;;
  web)
    CONFIG_DIR="${DABNEY_WEB_CONFIG_DIR:-$HOME/.config/dabney-web}"
    ;;
  *)
    echo "error: unsupported DABNEY_SERV00_SERVICE '$SERVICE' (expected forge or web)" >&2
    exit 2
    ;;
esac

CONFIG_FILE="$CONFIG_DIR/config.env"
SECRETS_FILE="$CONFIG_DIR/secrets.env"
SYNC_CONFIG="$CONFIG_DIR/sync.json"

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

if [ "$SERVICE" = "forge" ]; then
  SERVICE_BIN="${DABNEY_FORGE_BIN:-$HOME/bin/dabney-forge}"
  SERVICE_DATA="${DABNEY_FORGE_DATA:-$HOME/forge-data}"
  SERVICE_PORT="${DABNEY_FORGE_PORT:-}"
  SERVICE_DOMAIN="${DABNEY_FORGE_DOMAIN:-git.dabney.moe}"
  SERVICE_BASE_URL="${DABNEY_FORGE_BASE_URL:-https://$SERVICE_DOMAIN}"
  SERVICE_LOG="${DABNEY_FORGE_LOG:-$SERVICE_DATA/forge.log}"
  SERVICE_PID_FILE="${DABNEY_FORGE_PID_FILE:-$SERVICE_DATA/forge.pid}"
  SERVICE_LOCK_FILE="${DABNEY_FORGE_LOCK_FILE:-$SERVICE_DATA/watchdog.lock}"
  MAX_LOG_BYTES="${DABNEY_FORGE_MAX_LOG_BYTES:-5242880}"
  PORT_VAR="DABNEY_FORGE_PORT"
else
  SERVICE_BIN="${DABNEY_WEB_BIN:-$HOME/web/bin/dabney-web}"
  SERVICE_DATA="${DABNEY_WEB_DATA:-$HOME/web-data}"
  SERVICE_PORT="${DABNEY_WEB_PORT:-}"
  SERVICE_DOMAIN="${DABNEY_WEB_DOMAIN:-dabney.moe}"
  SERVICE_BASE_URL="https://$SERVICE_DOMAIN"
  WEB_SITE_ROOT="${DABNEY_WEB_SITE_ROOT:-$HOME/web/site}"
  SERVICE_LOG="${DABNEY_WEB_LOG:-$SERVICE_DATA/web.log}"
  SERVICE_PID_FILE="${DABNEY_WEB_PID_FILE:-$SERVICE_DATA/web.pid}"
  SERVICE_LOCK_FILE="${DABNEY_WEB_LOCK_FILE:-$SERVICE_DATA/watchdog.lock}"
  MAX_LOG_BYTES="${DABNEY_WEB_MAX_LOG_BYTES:-5242880}"
  PORT_VAR="DABNEY_WEB_PORT"
fi

mkdir -p "$SERVICE_DATA"

# Serialize concurrent cron ticks without spawning duplicate instances.
if [ "${DABNEY_SERV00_LOCKED:-0}" != "1" ] && [ "$action" != "status" ]; then
  export DABNEY_SERV00_LOCKED=1
  export DABNEY_SERV00_SERVICE="$SERVICE"
  if command -v lockf >/dev/null 2>&1; then
    exec lockf -s -t 0 "$SERVICE_LOCK_FILE" "$0" "$action"
  elif command -v flock >/dev/null 2>&1; then
    exec flock -n -o "$SERVICE_LOCK_FILE" "$0" "$action"
  fi
fi

expected_comm=$(basename "$SERVICE_BIN")

is_running() {
  if [ ! -f "$SERVICE_PID_FILE" ]; then
    return 1
  fi
  pid=$(cat "$SERVICE_PID_FILE" 2>/dev/null || true)
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
        *"$SERVICE_BIN"*|*"$expected_comm"*) return 0 ;;
        *) return 1 ;;
      esac
      ;;
  esac
}

rotate_log() {
  if [ ! -f "$SERVICE_LOG" ]; then
    return 0
  fi
  size=$(wc -c < "$SERVICE_LOG" 2>/dev/null | tr -d ' ' || echo 0)
  if [ "$size" -ge "$MAX_LOG_BYTES" ]; then
    mv -f "$SERVICE_LOG" "$SERVICE_LOG.1"
  fi
}

stop_service() {
  if ! is_running; then
    rm -f "$SERVICE_PID_FILE"
    return 0
  fi
  pid=$(cat "$SERVICE_PID_FILE")
  kill "$pid" 2>/dev/null || true
  i=0
  while [ "$i" -lt 25 ]; do
    if ! kill -0 "$pid" 2>/dev/null; then
      rm -f "$SERVICE_PID_FILE"
      return 0
    fi
    sleep 0.2
    i=$((i + 1))
  done
  kill -9 "$pid" 2>/dev/null || true
  rm -f "$SERVICE_PID_FILE"
}

start_service() {
  if is_running; then
    return 0
  fi
  if [ ! -x "$SERVICE_BIN" ]; then
    echo "error: $SERVICE binary not executable: $SERVICE_BIN" >&2
    exit 1
  fi
  case "$SERVICE_PORT" in
    ''|*[!0-9]*)
      echo "error: $PORT_VAR must be set to a numeric reserved port" >&2
      exit 1
      ;;
  esac
  rotate_log
  if [ "$SERVICE" = "forge" ]; then
    set -- --data-dir "$SERVICE_DATA" serve \
      --listen "127.0.0.1:$SERVICE_PORT" \
      --base-url "$SERVICE_BASE_URL"
    if [ -n "${DABNEY_FORGE_WEBHOOK_URL:-}" ]; then
      set -- "$@" --webhook-url "$DABNEY_FORGE_WEBHOOK_URL"
    fi
    if [ -f "$SYNC_CONFIG" ]; then
      set -- "$@" --sync-config "$SYNC_CONFIG" --sync-db "$SERVICE_DATA/forge-linear-sync.sqlite"
    fi
    nohup "$SERVICE_BIN" "$@" >> "$SERVICE_LOG" 2>&1 </dev/null &
  else
    if [ ! -d "$WEB_SITE_ROOT" ]; then
      echo "error: web site root not found: $WEB_SITE_ROOT" >&2
      exit 1
    fi
    hash_files="${LEPTOS_HASH_FILES:-true}"
    if [ "$hash_files" = "true" ] && [ ! -f "$(dirname "$SERVICE_BIN")/hash.txt" ]; then
      echo "error: missing Leptos hash.txt beside $SERVICE_BIN" >&2
      exit 1
    fi
    export LEPTOS_OUTPUT_NAME="${LEPTOS_OUTPUT_NAME:-dabney}"
    export LEPTOS_HASH_FILES="$hash_files"
    export LEPTOS_SITE_ROOT="$WEB_SITE_ROOT"
    export LEPTOS_SITE_PKG_DIR="${LEPTOS_SITE_PKG_DIR:-pkg}"
    export LEPTOS_SITE_ADDR="127.0.0.1:$SERVICE_PORT"
    export LEPTOS_ENV="${LEPTOS_ENV:-PROD}"
    export RUST_LOG="${RUST_LOG:-info}"
    nohup "$SERVICE_BIN" >> "$SERVICE_LOG" 2>&1 </dev/null &
  fi
  new_pid=$!
  printf '%s\n' "$new_pid" > "$SERVICE_PID_FILE"
  sleep 1
  if ! is_running; then
    echo "error: dabney-$SERVICE exited immediately; see $SERVICE_LOG" >&2
    rm -f "$SERVICE_PID_FILE"
    exit 1
  fi
}

case "$action" in
  status)
    if is_running; then
      echo "running ($SERVICE pid $(cat "$SERVICE_PID_FILE"), listen 127.0.0.1:${SERVICE_PORT:-?}, base $SERVICE_BASE_URL)"
      exit 0
    else
      echo "stopped"
      exit 3
    fi
    ;;
  stop)
    stop_service
    echo "stopped"
    ;;
  start|ensure)
    start_service
    echo "running ($SERVICE pid $(cat "$SERVICE_PID_FILE"))"
    ;;
  restart)
    stop_service
    start_service
    echo "running ($SERVICE pid $(cat "$SERVICE_PID_FILE"))"
    ;;
esac
