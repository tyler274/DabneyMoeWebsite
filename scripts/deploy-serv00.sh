#!/usr/bin/env bash
# Deploy `dabney-forge` (`git.dabney.moe`) or `dabney-web` (`dabney.moe`) to a
# zero-cost EU Serv00 (FreeBSD) shell account. Dry-run by default; pass --push
# to execute remote SSH/SCP steps.
#
# Usage:
#   scripts/deploy-serv00.sh [--service forge|web] --host <sN.serv00.com> \
#     --user <login> --domain <hostname> --port <reserved-tcp-port> [options]
#
# Options:
#   --service <forge|web>      Service to deploy (default: forge)
#   --host <host>              Serv00 SSH host (e.g. s14.serv00.com)
#   --user <user>              Serv00 account login
#   --domain <host>            Public hostname (e.g. git.dabney.moe, dabney.moe, or <user>.serv00.net)
#   --port <1024-65535>        TCP port reserved via `devil port add tcp random`
#   --bin <path>               Path to compiled x86_64-unknown-freebsd binary
#                              (default: target/x86_64-unknown-freebsd/release/{dabney-forge|web})
#   --site-dir <path>          (--service web) Path to compiled Leptos site assets (default: target/site)
#   --hash-file <path>         (--service web) Path to Leptos hash.txt (default: target/release/hash.txt)
#   --serv00-ip <ip>           Optional Serv00 web IP (`devil vhost list`) to request Let's Encrypt TLS
#   --ssh-key <path>           Optional SSH private key path
#   --ssh-known-hosts <path>   Optional pinned known_hosts file (enables StrictHostKeyChecking=yes)
#   --allow-any-bin            Skip the FreeBSD ELF check on --bin
#   --push                     Execute remote deploy (dry-run without --push)
#   -h, --help                 Show this help
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

service="${SERV00_SERVICE:-forge}"
host="${SERV00_HOST:-}"
user="${SERV00_USER:-}"
domain="${SERV00_DOMAIN:-}"
port="${SERV00_PORT:-}"
bin_path=""
site_dir="${SERV00_SITE_DIR:-$root/target/site}"
hash_file="${SERV00_HASH_FILE:-$root/target/release/hash.txt}"
serv00_ip="${SERV00_WEB_IP:-}"
ssh_key="${SERV00_SSH_KEY:-}"
known_hosts="${SERV00_SSH_KNOWN_HOSTS:-}"
allow_any_bin=0
do_push=0

usage() {
  sed -n '2,/^set /{/^#/p}' "$0" | sed 's/^# \{0,1\}//'
}

die() {
  echo "error: $1" >&2
  exit "${2:-2}"
}

sh_quote() {
  printf "'%s'" "${1//\'/\'\\\'\'}"
}

while [ $# -gt 0 ]; do
  case "$1" in
    --service) service="${2:?missing value for --service}"; shift 2 ;;
    --host) host="${2:?missing value for --host}"; shift 2 ;;
    --user) user="${2:?missing value for --user}"; shift 2 ;;
    --domain) domain="${2:?missing value for --domain}"; shift 2 ;;
    --port) port="${2:?missing value for --port}"; shift 2 ;;
    --bin) bin_path="${2:?missing value for --bin}"; shift 2 ;;
    --site-dir) site_dir="${2:?missing value for --site-dir}"; shift 2 ;;
    --hash-file) hash_file="${2:?missing value for --hash-file}"; shift 2 ;;
    --serv00-ip) serv00_ip="${2:?missing value for --serv00-ip}"; shift 2 ;;
    --ssh-key) ssh_key="${2:?missing value for --ssh-key}"; shift 2 ;;
    --ssh-known-hosts) known_hosts="${2:?missing value for --ssh-known-hosts}"; shift 2 ;;
    --allow-any-bin) allow_any_bin=1; shift ;;
    --push) do_push=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) die "unknown argument: $1" ;;
  esac
done

case "$service" in
  forge)
    if [ -z "$bin_path" ]; then
      bin_path="${SERV00_FORGE_BIN:-$root/target/x86_64-unknown-freebsd/release/dabney-forge}"
    fi
    build_hint="nix develop .#freebsd --command cargo zigbuild --release --target x86_64-unknown-freebsd -p forge --bin dabney-forge"
    ;;
  web)
    if [ -z "$bin_path" ]; then
      bin_path="${SERV00_WEB_BIN:-$root/target/x86_64-unknown-freebsd/release/web}"
    fi
    build_hint="nix develop .#freebsd --command cargo zigbuild --release --target x86_64-unknown-freebsd -p web --features ssr --bin web"
    ;;
  *)
    die "unsupported --service '$service' (expected forge or web)"
    ;;
esac

[ -n "$host" ] || die "--host <sN.serv00.com> is required"
[ -n "$user" ] || die "--user <login> is required"
[ -n "$domain" ] || die "--domain <hostname> is required"
[ -n "$port" ] || die "--port <1024-65535> is required"

[[ "$host" =~ ^[A-Za-z0-9]([A-Za-z0-9.-]*[A-Za-z0-9])?$ ]] || die "invalid --host '$host'"
[[ "$user" =~ ^[A-Za-z0-9_.-]+$ ]] || die "invalid --user '$user'"
[[ "$domain" =~ ^[A-Za-z0-9]([A-Za-z0-9.-]*[A-Za-z0-9])?$ ]] || die "invalid --domain '$domain'"
[[ "$port" =~ ^[0-9]+$ ]] && [ "$port" -ge 1024 ] && [ "$port" -le 65535 ] || die "invalid --port '$port' (expected 1024..65535)"

if [ -n "$serv00_ip" ]; then
  [[ "$serv00_ip" =~ ^[0-9a-fA-F.:]+$ ]] || die "invalid --serv00-ip '$serv00_ip'"
fi

watchdog_src="$root/selfhost/serv00/watchdog.sh"
[ -f "$watchdog_src" ] || die "missing $watchdog_src" 1

if [ "$do_push" -eq 1 ]; then
  [ -f "$bin_path" ] || die "missing compiled binary: $bin_path (build with: $build_hint)" 1
  if [ "$allow_any_bin" -eq 0 ] && command -v file >/dev/null 2>&1; then
    bin_desc=$(file -b "$bin_path" || true)
    case "$bin_desc" in
      *FreeBSD*x86-64*|*FreeBSD*x86_64*) ;;
      *) die "--bin '$bin_path' does not look like a FreeBSD x86_64 ELF binary ($bin_desc); pass --allow-any-bin to override" 1 ;;
    esac
  fi
  if [ "$service" = "web" ]; then
    [ -d "$site_dir" ] || die "missing Leptos site assets directory: $site_dir (build with: cargo leptos build --release or nix build .#web)" 1
    [ -f "$hash_file" ] || die "missing Leptos hash.txt: $hash_file (build with: cargo leptos build --release or nix build .#web)" 1
  fi
fi

ssh_opts=(-o BatchMode=yes)
if [ -n "$known_hosts" ]; then
  ssh_opts+=(-o "UserKnownHostsFile=$known_hosts" -o StrictHostKeyChecking=yes)
else
  ssh_opts+=(-o StrictHostKeyChecking=accept-new)
fi
if [ -n "$ssh_key" ]; then
  if [ "$do_push" -eq 1 ] && [ ! -r "$ssh_key" ]; then
    die "SSH key not readable: $ssh_key"
  fi
  ssh_opts+=(-i "$ssh_key" -o IdentitiesOnly=yes)
fi

remote="${user}@${host}"

q_domain=$(sh_quote "$domain")
q_port=$(sh_quote "$port")
q_ip=$(sh_quote "$serv00_ip")

if [ "$service" = "forge" ]; then
  remote_setup=$(cat <<EOF
set -eu
devil binexec on >/dev/null 2>&1 || true
mkdir -p "\$HOME/bin" "\$HOME/forge-data" "\$HOME/.config/dabney-forge"
chmod 700 "\$HOME/forge-data" "\$HOME/.config/dabney-forge"
EOF
)
  remote_activate=$(cat <<EOF
set -eu
chmod 755 "\$HOME/bin/dabney-forge.new"
mv -f "\$HOME/bin/dabney-forge.new" "\$HOME/bin/dabney-forge"
chmod 700 "\$HOME/bin/dabney-forge-watchdog.sh"

umask 077
cat > "\$HOME/.config/dabney-forge/config.env" <<'CFG'
DABNEY_SERV00_SERVICE=forge
DABNEY_FORGE_DOMAIN=$domain
DABNEY_FORGE_BASE_URL=https://$domain
DABNEY_FORGE_PORT=$port
DABNEY_FORGE_DATA=\$HOME/forge-data
DABNEY_FORGE_BIN=\$HOME/bin/dabney-forge
CFG
chmod 600 "\$HOME/.config/dabney-forge/config.env"

if ! devil www list 2>/dev/null | grep -Fq -- $q_domain; then
  devil www add $q_domain proxy localhost $q_port
fi

if [ -n $q_ip ]; then
  if ! devil ssl www list 2>/dev/null | grep -Fq -- $q_domain; then
    devil ssl www add $q_ip le le $q_domain
  fi
fi

# Install idempotent @reboot and 5-minute watchdog cron entries without clobbering other jobs.
existing=\$(crontab -l 2>/dev/null | grep -v '# dabney-forge-managed' || true)
{
  [ -n "\$existing" ] && printf '%s\n' "\$existing"
  printf '@reboot %s/bin/dabney-forge-watchdog.sh ensure # dabney-forge-managed\n' "\$HOME"
  printf '*/5 * * * * %s/bin/dabney-forge-watchdog.sh ensure # dabney-forge-managed\n' "\$HOME"
} | crontab -

"\$HOME/bin/dabney-forge-watchdog.sh" restart
"\$HOME/bin/dabney-forge-watchdog.sh" status
EOF
)
else
  remote_setup=$(cat <<EOF
set -eu
devil binexec on >/dev/null 2>&1 || true
mkdir -p "\$HOME/bin" "\$HOME/web/bin" "\$HOME/web/site.new" "\$HOME/web-data" "\$HOME/.config/dabney-web"
chmod 700 "\$HOME/web-data" "\$HOME/.config/dabney-web"
rm -rf "\$HOME/web/site.new"
mkdir -p "\$HOME/web/site.new"
EOF
)
  remote_activate=$(cat <<EOF
set -eu
chmod 755 "\$HOME/web/bin/dabney-web.new"
mv -f "\$HOME/web/bin/dabney-web.new" "\$HOME/web/bin/dabney-web"
chmod 644 "\$HOME/web/bin/hash.txt.new"
mv -f "\$HOME/web/bin/hash.txt.new" "\$HOME/web/bin/hash.txt"
rm -rf "\$HOME/web/site.prev"
if [ -d "\$HOME/web/site" ]; then
  mv "\$HOME/web/site" "\$HOME/web/site.prev"
fi
mv "\$HOME/web/site.new" "\$HOME/web/site"
rm -rf "\$HOME/web/site.prev"
chmod 700 "\$HOME/bin/dabney-web-watchdog.sh"

umask 077
cat > "\$HOME/.config/dabney-web/config.env" <<'CFG'
DABNEY_SERV00_SERVICE=web
DABNEY_WEB_DOMAIN=$domain
DABNEY_WEB_PORT=$port
DABNEY_WEB_DATA=\$HOME/web-data
DABNEY_WEB_BIN=\$HOME/web/bin/dabney-web
DABNEY_WEB_SITE_ROOT=\$HOME/web/site
CFG
chmod 600 "\$HOME/.config/dabney-web/config.env"

if ! devil www list 2>/dev/null | grep -Fq -- $q_domain; then
  devil www add $q_domain proxy localhost $q_port
fi

if [ -n $q_ip ]; then
  if ! devil ssl www list 2>/dev/null | grep -Fq -- $q_domain; then
    devil ssl www add $q_ip le le $q_domain
  fi
fi

# Install idempotent @reboot and 5-minute watchdog cron entries without clobbering other jobs.
existing=\$(crontab -l 2>/dev/null | grep -v '# dabney-web-managed' || true)
{
  [ -n "\$existing" ] && printf '%s\n' "\$existing"
  printf '@reboot %s/bin/dabney-web-watchdog.sh ensure # dabney-web-managed\n' "\$HOME"
  printf '*/5 * * * * %s/bin/dabney-web-watchdog.sh ensure # dabney-web-managed\n' "\$HOME"
} | crontab -

"\$HOME/bin/dabney-web-watchdog.sh" restart
"\$HOME/bin/dabney-web-watchdog.sh" status
EOF
)
fi

if [ "$do_push" -eq 0 ]; then
  echo "[dry-run] Service: dabney-$service | Target: $remote (domain=https://$domain, localhost:$port)"
  echo "[dry-run] ssh ${ssh_opts[*]} $remote '<setup binexec & directories>'"
  if [ "$service" = "forge" ]; then
    echo "[dry-run] scp ${ssh_opts[*]} $(sh_quote "$bin_path") $remote:~/bin/dabney-forge.new"
    echo "[dry-run] scp ${ssh_opts[*]} $(sh_quote "$watchdog_src") $remote:~/bin/dabney-forge-watchdog.sh"
  else
    echo "[dry-run] scp ${ssh_opts[*]} $(sh_quote "$bin_path") $remote:~/web/bin/dabney-web.new"
    echo "[dry-run] scp ${ssh_opts[*]} $(sh_quote "$hash_file") $remote:~/web/bin/hash.txt.new"
    echo "[dry-run] tar -C $(sh_quote "$site_dir") -czf - . | ssh ${ssh_opts[*]} $remote 'tar -xzf - -C ~/web/site.new'"
    echo "[dry-run] scp ${ssh_opts[*]} $(sh_quote "$watchdog_src") $remote:~/bin/dabney-web-watchdog.sh"
  fi
  echo "[dry-run] ssh ${ssh_opts[*]} $remote '<write config.env, devil www proxy $domain -> localhost:$port, install cron watchdog, restart>'"
  if [ -n "$serv00_ip" ]; then
    echo "[dry-run] TLS: devil ssl www add $serv00_ip le le $domain"
  fi
  exit 0
fi

ssh "${ssh_opts[@]}" "$remote" "$remote_setup"
if [ "$service" = "forge" ]; then
  scp "${ssh_opts[@]}" "$bin_path" "$remote:~/bin/dabney-forge.new"
  scp "${ssh_opts[@]}" "$watchdog_src" "$remote:~/bin/dabney-forge-watchdog.sh"
else
  scp "${ssh_opts[@]}" "$bin_path" "$remote:~/web/bin/dabney-web.new"
  scp "${ssh_opts[@]}" "$hash_file" "$remote:~/web/bin/hash.txt.new"
  tar -C "$site_dir" -czf - . | ssh "${ssh_opts[@]}" "$remote" 'tar -xzf - -C "$HOME/web/site.new"'
  scp "${ssh_opts[@]}" "$watchdog_src" "$remote:~/bin/dabney-web-watchdog.sh"
fi
ssh "${ssh_opts[@]}" "$remote" "$remote_activate"
echo "Deployed dabney-$service to https://$domain via $remote (localhost:$port)"
