# `recovery-index`

Shared schema, pure on-device search engine, and 4-layer cached in-repo updater for the Recovery & Mental Health Facility Finder at [`dabney.moe/tools/recovery`](https://dabney.moe/tools/recovery).

## What the index covers

1. **Base care listings** (SAMHSA FindTreatment.gov): care focus, service settings, insurance/sliding-scale payments, specialized programs, smoking/vaping policy (`SMP`/`FVP`), housing/transportation/childcare (`RSS`/`AS`), and languages (`SL`).
2. **Non-care amenities & living setup** ("everything aside from the care"):
   - Water & wellness: swimming pool, hot tub/jacuzzi, sauna, fitness center/gym, yoga studio, spa/massage
   - Rooms & living: private rooms, shared rooms, pet-friendly, on-site laundry, Wi-Fi, workspace
   - Campus & recreation: outdoor grounds/trails, sports courts, equine therapy, art/music studio, lounge/game room
   - Food & dining situation (`chef-prepared`, `dietitian-catered`, `cafeteria`, `self-catered`) + dietary options (`vegan-vegetarian`, `gluten-free`, `kosher`, `halal`, `allergy-accommodated`, `snacks-coffee`)
   - Electronic device allowance policy (`unrestricted`, `scheduled`, `phones-only`, `work-approved`, `prohibited`)
   - Smoking & vaping policies
3. **Exact clinical staff license types** (never conflated):
   - Physicians: `md-psychiatrist` ≠ `do-psychiatrist`, `md-addiction` ≠ `do-addiction`, `md-physician` ≠ `do-physician`
   - Advanced practice (distinct from physicians/psychiatrists): `pmhnp`, `np`, `pa`
   - Doctoral psychologists: `phd-psychologist` ≠ `psyd-psychologist`
   - Master's therapists & clinical social workers: `lcsw`, `lpc-lmhc`, `lmft`
   - Nursing & addiction support: `rn`, `lpn`, `addiction-counselor`, `peer-support`

## 4-layer cached pipeline

All network responses are cached on disk under `.cache/recovery-index/` (gitignored):

| Layer | Source | Disk Cache TTL |
| --- | --- | --- |
| 1 | SAMHSA FindTreatment.gov v2 export pages | 7 days |
| 2 | CMS NPPES NPI Registry v2.1 (`NPI-1` per ZIP) | 30 days |
| 2b | Official 50-state (+ DC/territories) facility & provider licensing adapters (ArcGIS, Socrata, CKAN, CSV, and `--import-file` bulk exports) | 14 days |
| 3 | Public facility websites (homepage + up to 4 amenity/FAQ/team subpages, ETag/Last-Modified revalidation) | 14 days |
| 3b *(optional)* | LLM structured extraction (`--llm codex\|claude\|openai\|ollama`) over cached site text, content-addressed by text SHA-256 | 90 days |
| 4 | Curated checked-in TOML overrides (`data/recovery/overrides/<ST>.toml`) | Always wins |

State shards are written to `public/recovery/<ST>.json` with content SHA-256 digests in `public/recovery/index.json` so browsers cache shards in Cache Storage (`dabney-recovery-shards-v1`) and automatically update when a shard changes.

## Usage

Run from the repository root (inside `nix develop`):

```bash
# List all valid amenity, food, electronics, smoking/vaping, and staff license codes
cargo run -p recovery-index --features cli -- codes

# Enrich specific states (residential/inpatient facilities prioritized; cached on disk)
cargo run -p recovery-index --features cli -- enrich --states AZ,CA,NY,CO --residential-first

# Optionally layer Codex (or Claude / OpenAI / Ollama) structured extraction over cached website text
cargo run -p recovery-index --features cli -- enrich --states CO,AZ,CA,NY --residential-first --limit 25 --skip-nppes --llm codex
cargo run -p recovery-index --features cli -- enrich --states CO --limit 25 --skip-nppes --llm claude
cargo run -p recovery-index --features cli -- enrich --states CO --limit 25 --skip-nppes --llm ollama --llm-model qwen2.5:14b

# Re-run purely from local disk cache (zero network I/O)
cargo run -p recovery-index --features cli -- enrich --states CO,WY --offline

# Refresh base listings from SAMHSA FindTreatment.gov (preserving prior enrichments + overrides)
cargo run -p recovery-index --features cli -- import --states CO

# Look up a facility's stable ID
cargo run -p recovery-index --features cli -- find --state CO --query "Denver"

# Add or update a curated/verified facility override and rebuild that state's shard + index.json
cargo run -p recovery-index --features cli -- set \
  --state CO \
  --id <facility-id> \
  --add-amenity pool,hot-tub,private-rooms \
  --food chef-prepared \
  --electronics scheduled \
  --add-staff do-psychiatrist,pmhnp \
  --notes "Heated pool and hot tub; phones allowed 1 hr/evening after 7-day blackout." \
  --verify

# Offline rebuild after hand-editing data/recovery/overrides/<ST>.toml
cargo run -p recovery-index --features cli -- rebuild

# Map a shared clinician or visiting/contracted specialist (e.g. a visiting PMHNP or local yoga instructor)
cargo run -p recovery-index --features cli -- add-provider \
  --state AZ \
  --name "Jacob" \
  --org "Scottsdale Yoga Studio" \
  --wellness yoga-instructor \
  --arrangement visiting \
  --facilities <facility-id-1>,<facility-id-2> \
  --notes "Provides weekly yoga classes across Phoenix & Scottsdale residential programs."

# Inspect the cross-facility shared-staff & visiting-provider network for a state
cargo run -p recovery-index --features cli -- network --state AZ --top 25

# Inspect all 55 US jurisdiction facility & provider licensing adapters
cargo run -p recovery-index --features cli -- state-adapters
cargo run -p recovery-index --features cli -- state-adapters --live-only
cargo run -p recovery-index --features cli -- state-adapters --state AZ

# Sync official state facility & provider licensing registries onto shards (cached on disk)
cargo run -p recovery-index --features cli -- state-sync --states AZ,CA,NY,CO
cargo run -p recovery-index --features cli -- state-sync --states AZ --import-file ./az-board-export.csv

# Check enrichment coverage across states
cargo run -p recovery-index --features cli -- stats
```
