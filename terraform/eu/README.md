# `terraform/eu`: dabney.moe on non-US providers

This OpenTofu root moves dabney.moe's DNS and hosting to EU providers:

- **deSEC** (Berlin, non-profit) creates and owns the `dabney.moe` zone, signs it with DNSSEC, and publishes CAA (Let's Encrypt only, no wildcards) and the A/AAAA records for the VPS hosts: the apex, `www`, `git`, `cache` and `auth` (plus `curie` when `curie_on_vps = true`).
- **Tier 1: Hetzner Cloud** (Helsinki `hel1`, or Falkenstein `fsn1` / Nuremberg `nbg1`): one protected CX23 VPS with backups, a firewall for SSH, HTTP, HTTPS, HTTP/3, forge SSH (2222) and Radicle (8776), and the deploy SSH key. It runs [`selfhost/eu`](../../selfhost/eu/README.md).
- **Tier 0: Scaleway** (Paris): a private Container Registry namespace and, when `serverless_image` is set, a scale-to-zero Serverless Container for `dabney-web` bound to `serverless_host`. A CNAME cannot sit at the apex, so Tier 0 serves a subdomain; the apex needs Tier 1.

> [!IMPORTANT]
> Public, demo and non-PHI surfaces only. Apply only from a reviewed saved plan (`tofu plan -out=eu.tfplan`, review, `tofu apply eu.tfplan`); DNS changes to `dabney.moe` need the owner's explicit authorization.

## Zone ownership with dabney-moe

dabney-moe's `infra/eu` adds records to this zone without creating it: `app` and `api` (pointed at this VPS through its `server_ipv4`/`server_ipv6` inputs), `curie` as a Codeberg git-pages CNAME with its `_git-pages-repository.curie` TXT record, the Scaleway TEM mail records, and the apex TXT (SPF). Each name has one owner: keep the two roots' host lists disjoint, never manage the apex TXT here, and set `curie_on_vps = true` only after removing `curie` from dabney-moe's `git_pages_hosts`.

## Apply

```sh
cd terraform/eu
cp terraform.tfvars.example terraform.tfvars   # non-secret inputs
# Secrets: TF_VAR_hcloud_token, TF_VAR_desec_token, SCW_ACCESS_KEY, SCW_SECRET_KEY,
# SCW_DEFAULT_PROJECT_ID, for example from a SOPS file:
sops exec-env ../../secrets/eu.sops.env 'tofu init && tofu plan -out=eu.tfplan'
# review eu.tfplan, then with authorization:
sops exec-env ../../secrets/eu.sops.env 'tofu apply eu.tfplan'
```

After the first apply, delegate the domain at the registrar to the `nameservers` output and enter the `dnssec_ds` records there; until then deSEC serves the zone but resolvers do not use it.

## Checks

```sh
tofu init -backend=false && tofu fmt -check -recursive && tofu validate && tofu test
```

[`tests/eu.tftest.hcl`](tests/eu.tftest.hcl) plans with mocked providers: Tier 1 hosts and CAA, `curie_on_vps`, Tier 0 on a subdomain, and refusal of an apex Serverless host.
