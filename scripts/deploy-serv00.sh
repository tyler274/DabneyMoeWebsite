#!/usr/bin/env bash
# Deploy `dabney-forge` (`git.dabney.moe`) to a zero-cost EU Serv00 (FreeBSD)
# shell account. Dry-run by default; pass --push to execute remote SSH/SCP steps.
#
# Usage:
#   scripts/deploy-serv00.sh --host <sN.serv00.com> --user <login> \
#     --domain <git.dabney.moe|login.serv00.net> --port <reserved-tcp-port> \
#     --bin <path-to-freebsd-dabney-forge> [options]
#
# Options:
#   --host <host>              Serv00 SSH host (e.g. s14.serv00.com)
#   --user <user>              Serv00 account login
#   --domain <host>            Public hostname (e.g. git.dabney.moe or <user>.serv00.net)
#   --port <1024-65535>        TCP port reserved via `devil port add tcp random`
#   --bin <path>               Path to compiled x86_64-unknown-freebsd `dabney-forge` binary
#   --serv00-ip <ip>           Optional Serv00 web IP (`devil vhost list`) to request Let's Encrypt TLS
#   --ssh-key <path>           Optional SSH private key path
#   --ssh-known-hosts <path>   Optional pinned known_hosts file (enables StrictHostKeyChecking=yes)
#   --allow-any-bin            Skip the FreeBSD ELF check on --bin
#   --push                     Execute remote deploy (dry-run without --push)
#   -h, --help                 Show this help
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

host="${SERV00_HOST:-}"
user="${SERV00_USER:-}"
domain="${SERV00_DOMAIN:-}"
port="${SERV00_PORT:-}"
bin_path="${SERV00_FORGE_BIN:-$root/target/x86_64-unknown-freebsd/release/dabney-forge}"
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
    --host) host="${2:?missing value for --host}"; shift 2 ;;
    --user) user="${2:?missing value for --user}"; shift 2 ;;
    --domain) domain="${2:?missing value for --domain}"; shift 2 ;;
    --port) port="${2:?missing value for --port}"; shift 2 ;;
    --bin) bin_path="${2:?missing value for --bin}"; shift 2 ;;
    --serv00-ip) serv00_ip="${2:?missing value for --serv00-ip}"; shift 2 ;;
    --ssh-key) ssh_key="${2:?missing value for --ssh-key}"; shift 2 ;;
    --ssh-known-hosts) known_hosts="${2:?missing value for --ssh-known-hosts}"; shift 2 ;;
    --allow-any-bin) allow_any_bin=1; shift ;;
    --push) do_push=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) die "unknown argument: $1" ;;
  esac
done

[ -n "$host" ] || die "--host <sN.serv00.com> is required"
[ -n "$user" ] || die "--user <login> is required"
[ -n "$domain" ] || die "--domain <git.dabney.moe> is required"
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
  [ -f "$bin_path" ] || die "missing compiled binary: $bin_path (build with: nix develop .#freebsd --command cargo zigbuild --release --target x86_64-unknown-freebsd -p forge --bin dabney-forge)" 1
  if [ "$allow_any_bin" -eq 0 ] && command -v file >/dev/null 2>&1; then
    bin_desc=$(file -b "$bin_path" || true)
    case "$bin_desc" in
      *FreeBSD*x86-64*|*FreeBSD*x86_64*) ;;
      *) die "--bin '$bin_path' does not look like a FreeBSD x86_64 ELF binary ($bin_desc); pass --allow-any-bin to override" 1 ;;
    esac
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

if [ "$do_push" -eq 0 ]; then
  echo "[dry-run] Target: $remote (domain=https://$domain, localhost:$port)"
  echo "[dry-run] ssh ${ssh_opts[*]} $remote '<setup binexec & directories>'"
  echo "[dry-run] scp ${ssh_opts[*]} $(sh_quote "$bin_path") $remote:~/bin/dabney-forge.new"
  echo "[dry-run] scp ${ssh_opts[*]} $(sh_quote "$watchdog_src") $remote:~/bin/dabney-forge-watchdog.sh"
  echo "[dry-run] ssh ${ssh_opts[*]} $remote '<write config.env, devil www proxy $domain -> localhost:$port, install cron watchdog, restart>'"
  if [ -n "$serv00_ip" ]; then
    echo "[dry-run] TLS: devil ssl www add $serv00_ip le le $domain"
  fi
  exit 0
fi

ssh "${ssh_opts[@]}" "$remote" "$remote_setup"
scp "${ssh_opts[@]}" "$bin_path" "$remote:~/bin/dabney-forge.new"
scp "${ssh_opts[@]}" "$watchdog_src" "$remote:~/bin/dabney-forge-watchdog.sh"
ssh "${ssh_opts[@]}" "$remote" "$remote_activate"
echo "Deployed dabney-forge to https://$domain via $remote (localhost:$port)"
