# Self-hosting dabney.moe with HTTPS (Caddy + Let's Encrypt)

The managed cloud targets under [`../terraform`](../terraform) (Cloud Run / App
Runner / Container Apps) terminate TLS with their own certificates, so they need
nothing here. This directory is for running the site on **your own host/VPS**,
where you bring the TLS: [Caddy](https://caddyserver.com) reverse-proxies the SSR
server and obtains + auto-renews a [Let's Encrypt](https://letsencrypt.org)
certificate over ACME.

```
internet ──▶ :443 HTTPS  ┌─────────┐  :8080  ┌──────────────┐
             :80  →  443  │  caddy  ├────────▶│ web (SSR app)│
                          └────┬────┘         └──────────────┘
                               │ ACME http-01 / tls-alpn-01
                               ▼
                         Let's Encrypt
```

Caddy gives us the security best practices out of the box: HTTP→HTTPS redirect,
automatic issuance/renewal at randomized times, modern TLS defaults, and it
follows the ACME `Link` headers (so the intermediate and ToS URLs are never
hardcoded). Port 80 stays open for the `http-01` challenge and the redirect.

## Prerequisites

- A host with a public IP and inbound **80 + 443** open (80 is required for the
  ACME `http-01` challenge and the HTTP→HTTPS redirect).
- `dabney.moe`'s `A` (and `AAAA`) DNS record pointing at that host.
- Docker + the Compose plugin.

## Run

```bash
cd selfhost
cp .env.example .env          # set ACME_EMAIL and (if needed) SITE_DOMAIN

# Build the hardened SSR image with Nix and load it into the local daemon.
# (Or set WEB_IMAGE in .env to a registry ref to pull instead.)
nix -C .. build .#serverImage
docker load < ../result

docker compose up -d
docker compose logs -f caddy   # watch the cert get issued
```

Open `https://dabney.moe`. Plain `http://` redirects to HTTPS.

## Testing without burning rate limits

Let's Encrypt's production CA has
[rate limits](https://letsencrypt.org/docs/rate-limits/). While iterating, point
Caddy at the staging CA by uncommenting the `acme_ca` line in the
[`Caddyfile`](Caddyfile); staging certs aren't publicly trusted (the browser
warns) but exercise the full flow. Remove it (and `docker compose restart caddy`)
for a real cert.

## HSTS

The `Caddyfile` sets `Strict-Transport-Security` with a 60-day `max-age` to start
(per the Let's Encrypt integration guidance; a too-early long max-age can lock
users out if a cert problem appears). Once HTTPS is stable, bump it to
`max-age=31536000; includeSubDomains; preload`.
