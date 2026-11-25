# Deploying `dabney-forge` & `dabney-web` on Serv00 (Free EU FreeBSD)

[Serv00](https://www.serv00.com) provides zero-cost, no-credit-card FreeBSD (`x86_64`) shell hosting in Poland (EU) with 3 GB SSD, 512 MB RAM, PostgreSQL/SQLite, custom domains, and Let's Encrypt TLS. A single Serv00 account can natively run both:

- **`dabney-forge` (`git.dabney.moe`)** — the pure-Rust Gitoxide forge (`~14 MB` FreeBSD binary) plus embedded `forge-linear-sync` webhook handling.
- **`dabney-web` (`dabney.moe`)** — the Leptos SSR + WASM web server (`~21 MB` FreeBSD binary + `target/site/` static/WASM bundle + `hash.txt`) with the in-process synthetic `clinical-workspace` demo and `/ownership.json`.

> **HIPAA boundary.** Never host protected health information (PHI) on shared free hosting. Serv00 hosts only public Git repositories, PR metadata, non-PHI webhooks, and the public/synthetic demo site.

## 1. Cross-Compile FreeBSD `x86_64` Binaries Locally

Because Serv00 free accounts have a 512 MB RAM limit, cross-compile locally from Linux or macOS using the dedicated Nix FreeBSD shell (`zig` + `cargo-zigbuild`):

```sh
# Pure-Rust Gitoxide forge (git.dabney.moe)
nix develop .#freebsd --command \
  cargo zigbuild --release --target x86_64-unknown-freebsd -p forge --bin dabney-forge

# Leptos SSR web server (dabney.moe)
nix develop .#freebsd --command \
  cargo zigbuild --release --target x86_64-unknown-freebsd -p web --features ssr --bin web

# Leptos hashed WASM/CSS/public asset bundle + hash.txt (built once in the default dev shell)
nix develop --command cargo leptos build --release
```

Verify both FreeBSD `x86-64` ELF executables:

```sh
file target/x86_64-unknown-freebsd/release/dabney-forge target/x86_64-unknown-freebsd/release/web
```

## 2. Reserve Ports and Inspect Your Serv00 Web IP

Log into your Serv00 account via SSH (replace `<user>` and `sN.serv00.com` with the credentials from your Serv00 welcome email):

```sh
ssh <user>@sN.serv00.com
devil binexec on
devil port add tcp random   # port for dabney-forge (e.g. 45123)
devil port add tcp random   # port for dabney-web   (e.g. 45124)
devil port list             # list allocated TCP ports
devil vhost list            # note your assigned web IP address for DNS & TLS
```

Log out and back in once after `devil binexec on` if this is the first time enabling custom binary execution.

## 3. Deploy with `scripts/deploy-serv00.sh`

Preview each deployment first (dry-run by default), then pass `--push` (add `--serv00-ip <ip>` once your custom domain `A` record points at Serv00 so `devil ssl www add` can issue a Let's Encrypt certificate):

```sh
# 1. Deploy dabney-forge (git.dabney.moe)
bash scripts/deploy-serv00.sh \
  --service forge \
  --host sN.serv00.com \
  --user <user> \
  --domain git.dabney.moe \
  --port 45123 \
  --serv00-ip <serv00-web-ip> \
  --push

# 2. Deploy dabney-web (dabney.moe)
bash scripts/deploy-serv00.sh \
  --service web \
  --host sN.serv00.com \
  --user <user> \
  --domain dabney.moe \
  --port 45124 \
  --site-dir target/site \
  --hash-file target/release/hash.txt \
  --serv00-ip <serv00-web-ip> \
  --push
```

`scripts/deploy-serv00.sh` uploads each binary atomically (plus `hash.txt` next to `~/web/bin/dabney-web` and an atomic `~/web/site` asset swap for `--service web`), installs [`watchdog.sh`](watchdog.sh) with idempotent `@reboot` and `*/5` cron entries, configures Serv00's Nginx reverse proxy (`devil www add <domain> proxy localhost <port>`), and restarts the service.

## 4. Initialize Repositories, PATs, Mirrors, and Linear Sync

On the Serv00 host:

```sh
~/bin/dabney-forge --data-dir ~/forge-data create-repo tyler274/dabney-moe
~/bin/dabney-forge --data-dir ~/forge-data set-policy tyler274/dabney-moe --approvals 1
~/bin/dabney-forge --data-dir ~/forge-data issue-token tyler274 --scopes read,write,status,admin --name primary
```

Store optional secrets (`DABNEY_FORGE_WEBHOOK_SECRET`, `CODEBERG_WEBHOOK_SECRET`, `GITEA_WEBHOOK_SECRET`, `GITLAB_WEBHOOK_TOKEN`, `LINEAR_API_KEY`, mirror push tokens) in `~/.config/dabney-forge/secrets.env` with mode `0600` (`watchdog.sh` refuses to load group- or world-readable secret files). To enable embedded `forge-linear-sync` webhook handling (`POST /hooks/forge/{forge}`), copy [`sync.json.example`](sync.json.example) to `~/.config/dabney-forge/sync.json` and run `~/bin/dabney-forge-watchdog.sh restart`.

## 5. DNS and Ongoing Maintenance

- **DNS (deSEC):** Add `A` records for `git.dabney.moe` and `dabney.moe` pointing to the IP returned by `devil vhost list`, then run `devil ssl www add <ip> le le <domain>` (or pass `--serv00-ip <ip>` to `scripts/deploy-serv00.sh`).
- **90-day activity rule:** Serv00 requires logging into the DevilWEB panel or SSH at least once every 90 days to keep a free account active.
- **Offline verification:** Run `bash scripts/test-deploy-serv00.sh` locally to validate script syntax, argument guards, `sync.json.example`, and watchdog lock/permission/`LEPTOS_*` behavior for both `forge` and `web` without contacting Serv00.
