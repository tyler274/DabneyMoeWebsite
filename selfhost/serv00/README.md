# Deploying `dabney-forge` (`git.dabney.moe`) on Serv00 (Free EU FreeBSD)

[Serv00](https://www.serv00.com) provides zero-cost, no-credit-card FreeBSD (`x86_64`) shell hosting in Poland (EU) with 3 GB SSD, 512 MB RAM, PostgreSQL/SQLite, custom domains, and Let's Encrypt TLS. It is a zero-cost non-US home for the pure-Rust Gitoxide forge (`dabney-forge`) and embedded `forge-linear-sync` webhook bridge for public, non-PHI repositories.

> **HIPAA boundary.** Never host protected health information (PHI) on shared free hosting. Serv00 hosts only public Git repositories, PR metadata, and non-PHI webhooks.

## 1. Cross-Compile `dabney-forge` for FreeBSD `x86_64`

Because Serv00 free accounts have a 512 MB RAM limit, cross-compile `dabney-forge` locally from Linux or macOS using the dedicated Nix FreeBSD shell (`zig` + `cargo-zigbuild`):

```sh
nix develop .#freebsd --command \
  cargo zigbuild --release --target x86_64-unknown-freebsd -p forge --bin dabney-forge
```

Verify the resulting binary is a FreeBSD `x86-64` ELF executable:

```sh
file target/x86_64-unknown-freebsd/release/dabney-forge
```

## 2. Reserve a Port and Inspect Your Serv00 Web IP

Log into your Serv00 account via SSH (replace `<user>` and `sN.serv00.com` with the credentials from your Serv00 welcome email):

```sh
ssh <user>@sN.serv00.com
devil binexec on
devil port add tcp random
devil port list        # note the allocated TCP port, e.g. 45123
devil vhost list       # note your assigned web IP address for DNS & TLS
```

Log out and back in once after `devil binexec on` if this is the first time enabling custom binary execution.

## 3. Deploy with `scripts/deploy-serv00.sh`

Preview the deployment first (dry-run by default):

```sh
bash scripts/deploy-serv00.sh \
  --host sN.serv00.com \
  --user <user> \
  --domain <user>.serv00.net \
  --port 45123
```

When ready, pass `--push` (add `--serv00-ip <ip>` once your custom domain `A` record points at Serv00 so `devil ssl www add` can issue a Let's Encrypt certificate):

```sh
bash scripts/deploy-serv00.sh \
  --host sN.serv00.com \
  --user <user> \
  --domain git.dabney.moe \
  --port 45123 \
  --serv00-ip <serv00-web-ip> \
  --push
```

`scripts/deploy-serv00.sh` uploads the binary atomically to `~/bin/dabney-forge`, installs [`watchdog.sh`](watchdog.sh) with idempotent `@reboot` and `*/5` cron entries, configures Serv00's Nginx reverse proxy (`devil www add <domain> proxy localhost <port>`), and starts `dabney-forge`.

## 4. Initialize Repositories, PATs, Mirrors, and Linear Sync

On the Serv00 host:

```sh
~/bin/dabney-forge --data-dir ~/forge-data create-repo tyler274/dabney-moe
~/bin/dabney-forge --data-dir ~/forge-data set-policy tyler274/dabney-moe --approvals 1
~/bin/dabney-forge --data-dir ~/forge-data issue-token tyler274 --scopes read,write,status,admin --name primary
```

Store optional secrets (`DABNEY_FORGE_WEBHOOK_SECRET`, `LINEAR_API_KEY`, mirror push tokens) in `~/.config/dabney-forge/secrets.env` with mode `0600` (`watchdog.sh` refuses to load group- or world-readable secret files). If `~/.config/dabney-forge/sync.toml` is present, `watchdog.sh` automatically enables embedded `forge-linear-sync` webhook handling (`POST /hooks/forge/{forge}`).

## 5. DNS and Ongoing Maintenance

- **DNS (deSEC):** Add an `A` record for `git.dabney.moe` pointing to the IP returned by `devil vhost list`, then run `devil ssl www add <ip> le le git.dabney.moe`.
- **90-day activity rule:** Serv00 requires logging into the DevilWEB panel or SSH at least once every 90 days to keep a free account active.
- **Offline verification:** Run `bash scripts/test-deploy-serv00.sh` locally to validate script syntax, argument guards, and watchdog lock/permission behavior without contacting Serv00.
