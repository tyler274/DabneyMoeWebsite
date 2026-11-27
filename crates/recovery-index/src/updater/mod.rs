//! 4-layer cached recovery directory updater & CLI pipeline:
//! 1. SAMHSA FindTreatment.gov (base listings + smoking/vaping/support/languages, cached on disk)
//! 2. CMS NPPES NPI Registry v2.1 (ZIP-cached individual provider MD/DO/PMHNP/NP/PA/PhD/LCSW matching)
//! 3. Facility public websites (ETag/TTL-cached amenities, food, electronics, and staff credentials)
//! 4. Checked-in human-editable TOML overrides (`data/recovery/overrides/<ST>.toml`)

pub mod cache;
pub mod llm;
pub mod network;
pub mod nppes;
pub mod overrides;
pub mod samhsa;
pub mod website;

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use futures_util::stream::{self, StreamExt};

use crate::schema::{
    Amenity, DataSource, DietaryOption, ElectronicsPolicy, Facility, FoodSituation,
    NetworkProvider, ProviderRole, SmokingPolicy, StaffArrangement, StaffLicense, StateCatalog,
    StateIndex, StateIndexEntry, VapingPolicy,
};
use crate::updater::cache::{sha256_hex, today_utc_iso, CacheMode, DiskCache};
use crate::updater::overrides::{
    load_state_curated_providers, load_state_overrides, load_state_overrides_file,
    save_state_overrides, save_state_overrides_file, FacilityOverride,
};

const WEB_CONCURRENCY: usize = 20;

#[derive(Debug, thiserror::Error)]
pub enum UpdaterError {
    #[error("could not build HTTP client")]
    HttpClient(#[source] reqwest::Error),
    #[error("SAMHSA import failed")]
    Samhsa(#[from] samhsa::SamhsaError),
    #[error("override error")]
    Override(#[from] overrides::OverrideError),
    #[error("could not read {}", path.display())]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not parse shard {}", path.display())]
    ParseShard {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("could not write {}", path.display())]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not serialize JSON")]
    Serialize(#[source] serde_json::Error),
    #[error("{0}")]
    Usage(String),
}

pub fn build_http_client() -> Result<reqwest::Client, UpdaterError> {
    reqwest::Client::builder()
        .user_agent("dabney-recovery-index/1.0 (+https://dabney.moe/tools/recovery)")
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(UpdaterError::HttpClient)
}

#[derive(Clone, Debug)]
pub struct PathsConfig {
    pub out_dir: PathBuf,
    pub overrides_dir: PathBuf,
    pub cache_dir: PathBuf,
}

impl Default for PathsConfig {
    fn default() -> Self {
        Self {
            out_dir: PathBuf::from("public/recovery"),
            overrides_dir: PathBuf::from("data/recovery/overrides"),
            cache_dir: PathBuf::from(".cache/recovery-index"),
        }
    }
}

/// Loads all existing state shards from `out_dir` into a sorted map keyed by 2-letter state code.
pub fn load_existing_shards(
    out_dir: &Path,
) -> Result<BTreeMap<String, StateCatalog>, UpdaterError> {
    let mut map = BTreeMap::new();
    if !out_dir.exists() {
        return Ok(map);
    }
    let entries = fs::read_dir(out_dir).map_err(|source| UpdaterError::Read {
        path: out_dir.to_path_buf(),
        source,
    })?;
    for entry in entries {
        let path = entry
            .map_err(|source| UpdaterError::Read {
                path: out_dir.to_path_buf(),
                source,
            })?
            .path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some(stem) = name.strip_suffix(".json") else {
            continue;
        };
        if stem.len() != 2 || !stem.bytes().all(|b| b.is_ascii_uppercase()) {
            continue;
        }
        let raw = fs::read_to_string(&path).map_err(|source| UpdaterError::Read {
            path: path.clone(),
            source,
        })?;
        let mut catalog: StateCatalog =
            serde_json::from_str(&raw).map_err(|source| UpdaterError::ParseShard {
                path: path.clone(),
                source,
            })?;
        for f in &mut catalog.facilities {
            if f.id.is_none() {
                f.id = Some(f.stable_id());
            }
        }
        map.insert(stem.to_string(), catalog);
    }
    Ok(map)
}

/// Writes `catalog` compactly to `<out_dir>/<ST>.json` and returns its SHA-256 hex digest.
pub fn write_state_shard(out_dir: &Path, catalog: &StateCatalog) -> Result<String, UpdaterError> {
    fs::create_dir_all(out_dir).map_err(|source| UpdaterError::Write {
        path: out_dir.to_path_buf(),
        source,
    })?;
    let mut encoded = serde_json::to_string(catalog).map_err(UpdaterError::Serialize)?;
    encoded.push('\n');
    let digest = sha256_hex(encoded.as_bytes());
    let path = out_dir.join(format!("{}.json", catalog.state));
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, encoded.as_bytes()).map_err(|source| UpdaterError::Write {
        path: tmp.clone(),
        source,
    })?;
    fs::rename(&tmp, &path).map_err(|source| UpdaterError::Write { path, source })?;
    Ok(digest)
}

/// Recomputes and writes `<out_dir>/index.json` with SHA-256 hashes and enrichment counts
/// for every `<ST>.json` shard present in `out_dir`.
pub fn rebuild_index_json(out_dir: &Path) -> Result<StateIndex, UpdaterError> {
    let shards = load_existing_shards(out_dir)?;
    let mut states = Vec::with_capacity(shards.len());
    for (code, catalog) in shards {
        let path = out_dir.join(format!("{code}.json"));
        let bytes = fs::read(&path).map_err(|source| UpdaterError::Read { path, source })?;
        let sha256 = sha256_hex(&bytes);
        let enriched_facilities = catalog
            .facilities
            .iter()
            .filter(|f| f.is_enriched())
            .count();
        states.push(StateIndexEntry {
            state: code,
            facilities: catalog.facilities.len(),
            retrieved: catalog.retrieved,
            sha256: Some(sha256),
            enriched_facilities,
        });
    }
    let index = StateIndex { states };
    let mut json = serde_json::to_string_pretty(&index).map_err(UpdaterError::Serialize)?;
    json.push('\n');
    let index_path = out_dir.join("index.json");
    let tmp = index_path.with_extension("json.tmp");
    fs::write(&tmp, json.as_bytes()).map_err(|source| UpdaterError::Write {
        path: tmp.clone(),
        source,
    })?;
    fs::rename(&tmp, &index_path).map_err(|source| UpdaterError::Write {
        path: index_path,
        source,
    })?;
    Ok(index)
}

/// Applies all checked-in TOML overrides for `catalog.state` onto `catalog`.
pub fn apply_overrides_to_catalog(
    overrides_dir: &Path,
    catalog: &mut StateCatalog,
) -> Result<usize, UpdaterError> {
    let map = load_state_overrides(overrides_dir, &catalog.state)?;
    if map.is_empty() {
        return Ok(0);
    }
    let mut applied = 0usize;
    for f in &mut catalog.facilities {
        let id = f.stable_id();
        f.id = Some(id.clone());
        if let Some(ov) = map.get(&id) {
            ov.apply_to(f);
            applied += 1;
        }
    }
    Ok(applied)
}

#[derive(Clone, Debug)]
pub struct ImportOptions {
    pub paths: PathsConfig,
    pub states: Vec<String>,
    pub from_files: Vec<PathBuf>,
    pub retrieved: Option<String>,
    pub cache_mode: CacheMode,
}

/// Runs Layer 1 SAMHSA import (preserving existing enrichments if re-importing), applies overrides,
/// writes state shards, and rebuilds `index.json` with SHA-256 digests.
pub async fn run_import(opts: ImportOptions) -> Result<(), UpdaterError> {
    let cache = DiskCache::new(opts.paths.cache_dir.clone(), opts.cache_mode);
    let retrieved = opts.retrieved.unwrap_or_else(today_utc_iso);
    let pages = if opts.from_files.is_empty() {
        let client = build_http_client()?;
        samhsa::fetch_pages_cached(&client, &cache).await?
    } else {
        samhsa::read_pages_from_files(&opts.from_files)?
    };

    let existing = load_existing_shards(&opts.paths.out_dir)?;
    let mut catalogs = samhsa::build_catalogs_from_pages(pages, &retrieved, &opts.states);

    let mut total_facilities = 0usize;
    let mut total_overrides = 0usize;
    for catalog in &mut catalogs {
        // Carry forward prior NPPES/website/network enrichment if already present on matching stable_ids.
        if let Some(prev_cat) = existing.get(&catalog.state) {
            let prev_by_id: BTreeMap<String, &Facility> = prev_cat
                .facilities
                .iter()
                .map(|f| (f.stable_id(), f))
                .collect();
            for f in &mut catalog.facilities {
                let id = f.stable_id();
                if let Some(prev) = prev_by_id.get(&id) {
                    merge_preserved_enrichment(f, prev);
                }
            }
            catalog.staff_network = prev_cat.staff_network.clone();
        }
        total_overrides += apply_overrides_to_catalog(&opts.paths.overrides_dir, catalog)?;
        let curated_providers =
            load_state_curated_providers(&opts.paths.overrides_dir, &catalog.state)?;
        if !catalog.staff_network.is_empty() || !curated_providers.is_empty() {
            network::rebuild_state_staff_network(
                catalog,
                &[],
                &curated_providers,
                network::NetworkRebuildMode::IncrementalMerge,
            );
        }
        write_state_shard(&opts.paths.out_dir, catalog)?;
        total_facilities += catalog.facilities.len();
    }

    let index = rebuild_index_json(&opts.paths.out_dir)?;
    println!(
        "recovery-index import: updated {} states ({total_facilities} facilities, {total_overrides} overrides applied); index has {} states.",
        catalogs.len(),
        index.states.len(),
    );
    Ok(())
}

fn merge_preserved_enrichment(target: &mut Facility, prev: &Facility) {
    for &a in &prev.lifestyle.amenities {
        if !target.lifestyle.amenities.contains(&a) {
            target.lifestyle.amenities.push(a);
        }
    }
    if target.lifestyle.food.is_none() {
        target.lifestyle.food = prev.lifestyle.food;
    }
    for &d in &prev.lifestyle.dietary {
        if !target.lifestyle.dietary.contains(&d) {
            target.lifestyle.dietary.push(d);
        }
    }
    if target.lifestyle.electronics.is_none() {
        target.lifestyle.electronics = prev.lifestyle.electronics;
    }
    if target.lifestyle.smoking.is_none() {
        target.lifestyle.smoking = prev.lifestyle.smoking;
    }
    if target.lifestyle.vaping.is_none() {
        target.lifestyle.vaping = prev.lifestyle.vaping;
    }
    if target.lifestyle.notes.is_none() {
        target.lifestyle.notes.clone_from(&prev.lifestyle.notes);
    }
    target.lifestyle.normalize();

    for &lic in &prev.staff_licenses {
        if !target.staff_licenses.contains(&lic) {
            target.staff_licenses.push(lic);
        }
    }
    target.staff_licenses.sort_unstable();
    target.staff_licenses.dedup();
    if target.staffing_profile.is_none() {
        target.staffing_profile.clone_from(&prev.staffing_profile);
    }

    if let Some(prev_meta) = &prev.enrichment {
        let meta = target
            .enrichment
            .get_or_insert_with(crate::schema::EnrichmentMeta::default);
        for &src in &prev_meta.sources {
            meta.add_source(src);
        }
        if meta.nppes_provider_count.is_none() {
            meta.nppes_provider_count = prev_meta.nppes_provider_count;
        }
        if meta.website_checked.is_none() {
            meta.website_checked.clone_from(&prev_meta.website_checked);
        }
        if meta.verified_at.is_none() {
            meta.verified_at.clone_from(&prev_meta.verified_at);
        }
        for url in &prev_meta.evidence_urls {
            meta.add_evidence_url(url.clone());
        }
    }
}

#[derive(Clone, Debug)]
pub struct EnrichOptions {
    pub paths: PathsConfig,
    pub states: Vec<String>,
    pub limit: Option<usize>,
    pub residential_first: bool,
    pub skip_nppes: bool,
    pub skip_web: bool,
    pub llm: Option<llm::LlmConfig>,
    pub cache_mode: CacheMode,
}

/// Runs Layers 2 (CMS NPPES) + 3 (facility website scraper) + 4 (TOML overrides) over existing
/// shards, caching every external response on disk.
pub async fn run_enrich(opts: EnrichOptions) -> Result<(), UpdaterError> {
    let client = build_http_client()?;
    let cache = DiskCache::new(opts.paths.cache_dir.clone(), opts.cache_mode);
    let mut shards = load_existing_shards(&opts.paths.out_dir)?;

    let target_states: Vec<String> = if opts.states.is_empty() {
        shards.keys().cloned().collect()
    } else {
        opts.states
            .iter()
            .map(|s| s.to_ascii_uppercase())
            .filter(|s| shards.contains_key(s))
            .collect()
    };

    if target_states.is_empty() {
        return Err(UpdaterError::Usage(
            "No matching state shards found in output directory.".to_string(),
        ));
    }

    for state in &target_states {
        let Some(catalog) = shards.get_mut(state) else {
            continue;
        };

        // Pick indices to enrich (optionally prioritizing residential/inpatient sites).
        let mut indices: Vec<usize> = (0..catalog.facilities.len()).collect();
        if opts.residential_first {
            indices.sort_by_key(|&i| {
                let f = &catalog.facilities[i];
                let res = f.settings.iter().any(|s| s.is_residential_or_inpatient());
                let has_web = f.safe_website().is_some();
                (!res, !has_web, i)
            });
        }
        if let Some(limit) = opts.limit {
            indices.truncate(limit);
        }

        // Layer 2: CMS NPPES (ZIP-cached)
        let mut nppes_hits = 0usize;
        let mut nppes_net = 0usize;
        let mut nppes_enriched = 0usize;
        let mut all_observations: Vec<network::ProviderObservation> = Vec::new();
        if !opts.skip_nppes {
            let mut subset: Vec<Facility> = indices
                .iter()
                .map(|&i| catalog.facilities[i].clone())
                .collect();
            let (hits, net, enriched, nppes_obs) =
                nppes::enrich_facilities_with_nppes(&client, &cache, &mut subset).await;
            for (&idx, updated) in indices.iter().zip(subset) {
                catalog.facilities[idx] = updated;
            }
            nppes_hits = hits;
            nppes_net = net;
            nppes_enriched = enriched;
            all_observations.extend(nppes_obs);
        }

        // Layer 3 (+ optional 3b LLM): Public facility websites (deduplicated per unique URL, cached)
        let mut web_hits = 0usize;
        let mut web_net = 0usize;
        let mut web_enriched = 0usize;
        let mut llm_hits = 0usize;
        let mut llm_calls = 0usize;
        let mut llm_enriched = 0usize;
        if !opts.skip_web || opts.llm.is_some() {
            let client_ref = &client;
            let cache_ref = &cache;
            let mut unique_urls: Vec<String> = Vec::new();
            let mut seen_urls = std::collections::BTreeSet::new();
            for &idx in &indices {
                if let Some(url) = catalog.facilities[idx].safe_website() {
                    if seen_urls.insert(url.to_string()) {
                        unique_urls.push(url.to_string());
                    }
                }
            }

            let total_urls = unique_urls.len();
            let done_urls = AtomicUsize::new(0);
            let done_ref = &done_urls;

            let fetched: Vec<(String, usize, usize, Vec<website::CachedWebPage>)> =
                stream::iter(unique_urls)
                    .map(|url| async move {
                        let (h, n, pages) =
                            website::fetch_website_pages(client_ref, cache_ref, &url).await;
                        let done = done_ref.fetch_add(1, Ordering::Relaxed) + 1;
                        if total_urls >= 50 && (done.is_multiple_of(50) || done == total_urls) {
                            eprintln!(
                                "recovery-index: Web [{state}] progress {done}/{total_urls} sites"
                            );
                        }
                        (url, h, n, pages)
                    })
                    .buffer_unordered(WEB_CONCURRENCY)
                    .collect()
                    .await;

            let mut pages_by_url: HashMap<String, Vec<website::CachedWebPage>> =
                HashMap::with_capacity(fetched.len());
            let mut signals_by_url: HashMap<String, website::WebsiteSignals> =
                HashMap::with_capacity(fetched.len());
            for (url, h, n, pages) in fetched {
                web_hits += h;
                web_net += n;
                if !opts.skip_web {
                    signals_by_url.insert(url.clone(), website::extract_signals_from_pages(&pages));
                }
                pages_by_url.insert(url, pages);
            }

            if !opts.skip_web {
                for &idx in &indices {
                    if let Some(url) = catalog.facilities[idx].safe_website().map(str::to_string) {
                        if let Some(signals) = signals_by_url.get(&url) {
                            if website::apply_website_signals(&mut catalog.facilities[idx], signals)
                            {
                                web_enriched += 1;
                            }
                        }
                    }
                }
            }

            // Optional Layer 3b: Cached LLM structured extraction over the already-cached site text
            if let Some(llm_cfg) = &opts.llm {
                let llm_concurrency = llm_cfg.concurrency.max(1);
                let llm_work: Vec<(String, Vec<website::CachedWebPage>)> = pages_by_url
                    .into_iter()
                    .filter(|(_, pages)| !pages.is_empty())
                    .collect();
                let llm_total = llm_work.len();
                let llm_done = AtomicUsize::new(0);
                let llm_done_ref = &llm_done;

                let llm_results: Vec<(String, Result<llm::LlmOutcome, llm::LlmError>)> =
                    stream::iter(llm_work)
                        .map(|(url, pages)| async move {
                            let res =
                                llm::extract_from_pages_cached(client_ref, cache_ref, llm_cfg, &pages)
                                    .await;
                            let done = llm_done_ref.fetch_add(1, Ordering::Relaxed) + 1;
                            if llm_total >= 10 && (done.is_multiple_of(10) || done == llm_total) {
                                eprintln!(
                                    "recovery-index: LLM ({}) [{state}] progress {done}/{llm_total} sites",
                                    llm_cfg.backend.as_str()
                                );
                            }
                            (url, res)
                        })
                        .buffer_unordered(llm_concurrency)
                        .collect()
                        .await;

                let mut llm_by_url: HashMap<String, llm::LlmExtraction> = HashMap::new();
                for (url, res) in llm_results {
                    match res {
                        Ok(llm::LlmOutcome::CacheHit(ext)) => {
                            llm_hits += 1;
                            llm_by_url.insert(url, ext);
                        }
                        Ok(llm::LlmOutcome::Extracted(ext)) => {
                            llm_calls += 1;
                            llm_by_url.insert(url, ext);
                        }
                        Ok(llm::LlmOutcome::Skipped) => {}
                        Err(err) => {
                            eprintln!("recovery-index: LLM warning for {url}: {err}");
                        }
                    }
                }

                for &idx in &indices {
                    if let Some(url) = catalog.facilities[idx].safe_website().map(str::to_string) {
                        if let Some(ext) = llm_by_url.get(&url) {
                            if llm::apply_llm_extraction(&mut catalog.facilities[idx], ext) {
                                llm_enriched += 1;
                            }
                            let fid = catalog.facilities[idx].stable_id();
                            all_observations.extend(llm::observations_from_llm(state, &fid, ext));
                        }
                    }
                }
            }
        }

        // Layer 4: Curated in-repo TOML overrides + Cross-facility shared-staff network
        let ov_count = apply_overrides_to_catalog(&opts.paths.overrides_dir, catalog)?;
        let curated_providers = load_state_curated_providers(&opts.paths.overrides_dir, state)?;
        let network_mode = if !opts.skip_nppes && opts.limit.is_none() {
            network::NetworkRebuildMode::FullNppesRefresh
        } else {
            network::NetworkRebuildMode::IncrementalMerge
        };
        if !all_observations.is_empty()
            || !curated_providers.is_empty()
            || network_mode == network::NetworkRebuildMode::FullNppesRefresh
        {
            network::rebuild_state_staff_network(
                catalog,
                &all_observations,
                &curated_providers,
                network_mode,
            );
        }
        for f in &mut catalog.facilities {
            f.id = Some(f.stable_id());
        }
        let digest = write_state_shard(&opts.paths.out_dir, catalog)?;
        // Persist index.json after each state completes so partial multi-state runs stay durable.
        rebuild_index_json(&opts.paths.out_dir)?;
        let enriched_total = catalog
            .facilities
            .iter()
            .filter(|f| f.is_enriched())
            .count();
        let llm_summary = if opts.llm.is_some() {
            format!(" | LLM: {llm_enriched} matched (cache {llm_hits} hits / {llm_calls} called)")
        } else {
            String::new()
        };
        println!(
            "recovery-index enrich [{state}]: {} facilities ({enriched_total} enriched) | \
             NPPES: {nppes_enriched} matched (cache {nppes_hits} hits / {nppes_net} fetched) | \
             Web: {web_enriched} matched (cache {web_hits} hits / {web_net} fetched){llm_summary} | \
             Network: {} shared/visiting providers | Overrides: {ov_count} | sha256: {}...",
            catalog.facilities.len(),
            catalog.staff_network.len(),
            &digest[..12],
        );
    }

    rebuild_index_json(&opts.paths.out_dir)?;
    Ok(())
}

/// Pure offline rebuild: ensures every facility has a stable `id`, reapplies TOML overrides,
/// writes shards, and recomputes `public/recovery/index.json` with SHA-256 digests.
pub fn run_rebuild(paths: &PathsConfig, states: &[String]) -> Result<(), UpdaterError> {
    let mut shards = load_existing_shards(&paths.out_dir)?;
    let filter: Vec<String> = states.iter().map(|s| s.to_ascii_uppercase()).collect();
    let mut updated_states = 0usize;
    let mut total_overrides = 0usize;

    for (code, catalog) in &mut shards {
        if !filter.is_empty() && !filter.contains(code) {
            continue;
        }
        for f in &mut catalog.facilities {
            f.id = Some(f.stable_id());
        }
        total_overrides += apply_overrides_to_catalog(&paths.overrides_dir, catalog)?;
        let curated_providers = load_state_curated_providers(&paths.overrides_dir, code)?;
        if !curated_providers.is_empty() || !catalog.staff_network.is_empty() {
            network::rebuild_state_staff_network(
                catalog,
                &[],
                &curated_providers,
                network::NetworkRebuildMode::IncrementalMerge,
            );
        }
        write_state_shard(&paths.out_dir, catalog)?;
        updated_states += 1;
    }

    let index = rebuild_index_json(&paths.out_dir)?;
    let enriched_sum: usize = index.states.iter().map(|s| s.enriched_facilities).sum();
    println!(
        "recovery-index rebuild: {updated_states} states written ({total_overrides} overrides applied), \
         index.json updated ({} states, {enriched_sum} enriched facilities).",
        index.states.len(),
    );
    Ok(())
}

#[derive(Clone, Debug, Default)]
pub struct SetOverrideOptions {
    pub paths: PathsConfig,
    pub state: String,
    pub id: String,
    pub add_amenities: Vec<Amenity>,
    pub remove_amenities: Vec<Amenity>,
    pub food: Option<FoodSituation>,
    pub add_dietary: Vec<DietaryOption>,
    pub remove_dietary: Vec<DietaryOption>,
    pub electronics: Option<ElectronicsPolicy>,
    pub smoking: Option<SmokingPolicy>,
    pub vaping: Option<VapingPolicy>,
    pub add_staff: Vec<StaffLicense>,
    pub remove_staff: Vec<StaffLicense>,
    pub notes: Option<String>,
    pub evidence_urls: Vec<String>,
    pub verify: bool,
}

/// Edits or creates an override in `data/recovery/overrides/<ST>.toml` and immediately rebuilds
/// that state's shard + `index.json`.
pub fn run_set_override(opts: SetOverrideOptions) -> Result<(), UpdaterError> {
    let state = opts.state.to_ascii_uppercase();
    let mut shards = load_existing_shards(&opts.paths.out_dir)?;
    let catalog = shards.get_mut(&state).ok_or_else(|| {
        UpdaterError::Usage(format!(
            "State shard `{state}` not found in {}",
            opts.paths.out_dir.display()
        ))
    })?;

    let matched_name = catalog
        .facilities
        .iter()
        .find(|f| f.stable_id() == opts.id)
        .map(|f| f.name.clone())
        .ok_or_else(|| {
            UpdaterError::Usage(format!(
                "Facility id `{}` not found in {state}. Run `recovery-index find --state {state} --query ...` to look up IDs.",
                opts.id
            ))
        })?;

    let mut overrides = load_state_overrides(&opts.paths.overrides_dir, &state)?;
    let entry = overrides
        .entry(opts.id.clone())
        .or_insert_with(|| FacilityOverride {
            id: opts.id.clone(),
            name: Some(matched_name.clone()),
            ..FacilityOverride::default()
        });
    if entry.name.is_none() {
        entry.name = Some(matched_name.clone());
    }

    for a in opts.add_amenities {
        if !entry.amenities.contains(&a) {
            entry.amenities.push(a);
        }
        entry.remove_amenities.retain(|x| *x != a);
    }
    for a in opts.remove_amenities {
        if !entry.remove_amenities.contains(&a) {
            entry.remove_amenities.push(a);
        }
        entry.amenities.retain(|x| *x != a);
    }
    if let Some(food) = opts.food {
        entry.food = Some(food);
    }
    for d in opts.add_dietary {
        if !entry.dietary.contains(&d) {
            entry.dietary.push(d);
        }
        entry.remove_dietary.retain(|x| *x != d);
    }
    for d in opts.remove_dietary {
        if !entry.remove_dietary.contains(&d) {
            entry.remove_dietary.push(d);
        }
        entry.dietary.retain(|x| *x != d);
    }
    if let Some(e) = opts.electronics {
        entry.electronics = Some(e);
    }
    if let Some(s) = opts.smoking {
        entry.smoking = Some(s);
    }
    if let Some(v) = opts.vaping {
        entry.vaping = Some(v);
    }
    for lic in opts.add_staff {
        if !entry.staff_licenses.contains(&lic) {
            entry.staff_licenses.push(lic);
        }
        entry.remove_staff_licenses.retain(|x| *x != lic);
    }
    for lic in opts.remove_staff {
        if !entry.remove_staff_licenses.contains(&lic) {
            entry.remove_staff_licenses.push(lic);
        }
        entry.staff_licenses.retain(|x| *x != lic);
    }
    if let Some(notes) = opts.notes {
        entry.notes = Some(notes);
    }
    for url in opts.evidence_urls {
        if !entry.evidence_urls.contains(&url) {
            entry.evidence_urls.push(url);
        }
    }
    if opts.verify || entry.verified_at.is_none() {
        entry.verified_at = Some(today_utc_iso());
    }

    entry.amenities.sort_unstable();
    entry.dietary.sort_unstable();
    entry.staff_licenses.sort_unstable();

    let saved_path = save_state_overrides(&opts.paths.overrides_dir, &state, &overrides)?;
    apply_overrides_to_catalog(&opts.paths.overrides_dir, catalog)?;
    write_state_shard(&opts.paths.out_dir, catalog)?;
    rebuild_index_json(&opts.paths.out_dir)?;

    println!(
        "Updated override for `{matched_name}` ({}) in {} and rebuilt {state}.json + index.json.",
        opts.id,
        saved_path.display()
    );
    Ok(())
}

/// Searches a state shard from the CLI and prints facility IDs + current enrichment summary.
pub fn run_find(
    paths: &PathsConfig,
    state: &str,
    query_text: &str,
    limit: usize,
) -> Result<(), UpdaterError> {
    let state = state.to_ascii_uppercase();
    let shards = load_existing_shards(&paths.out_dir)?;
    let catalog = shards
        .get(&state)
        .ok_or_else(|| UpdaterError::Usage(format!("State shard `{state}` not found.")))?;

    let needle = query_text.to_ascii_lowercase();
    let mut shown = 0usize;
    for f in &catalog.facilities {
        let hay = format!(
            "{} {} {} {}",
            f.name,
            f.site_name.as_deref().unwrap_or(""),
            f.address.city,
            f.address.zip
        )
        .to_ascii_lowercase();
        if !needle.is_empty() && !hay.contains(&needle) {
            continue;
        }
        println!("id: {}", f.stable_id());
        println!(
            "  name: {}{}",
            f.name,
            f.site_name
                .as_deref()
                .map(|s| format!(" ({s})"))
                .unwrap_or_default()
        );
        println!("  addr: {}", f.address.display_line());
        if let Some(web) = f.safe_website() {
            println!("  web:  {web}");
        }
        if !f.lifestyle.amenities.is_empty() {
            let codes: Vec<&str> = f.lifestyle.amenities.iter().map(|a| a.code()).collect();
            println!("  amenities: {}", codes.join(", "));
        }
        if let Some(food) = f.lifestyle.food {
            println!("  food: {}", food.label());
        }
        if let Some(elec) = f.lifestyle.electronics {
            println!("  electronics: {}", elec.label());
        }
        if !f.staff_licenses.is_empty() {
            let codes: Vec<&str> = f.staff_licenses.iter().map(|l| l.short_label()).collect();
            println!("  staff: {}", codes.join(", "));
        }
        shown += 1;
        if shown >= limit {
            break;
        }
    }
    println!("Shown {shown} facilities in {state}.");
    Ok(())
}

/// Prints enrichment coverage statistics across states.
pub fn run_stats(paths: &PathsConfig, state_filter: Option<&str>) -> Result<(), UpdaterError> {
    let shards = load_existing_shards(&paths.out_dir)?;
    let mut total_fac = 0usize;
    let mut total_enriched = 0usize;
    let mut total_amenities = 0usize;
    let mut total_food = 0usize;
    let mut total_elec = 0usize;
    let mut total_staff = 0usize;

    for (code, catalog) in &shards {
        if let Some(wanted) = state_filter {
            if !code.eq_ignore_ascii_case(wanted) {
                continue;
            }
        }
        let fac = catalog.facilities.len();
        let enriched = catalog
            .facilities
            .iter()
            .filter(|f| f.is_enriched())
            .count();
        let amenities = catalog
            .facilities
            .iter()
            .filter(|f| !f.lifestyle.amenities.is_empty())
            .count();
        let food = catalog
            .facilities
            .iter()
            .filter(|f| f.lifestyle.food.is_some())
            .count();
        let elec = catalog
            .facilities
            .iter()
            .filter(|f| f.lifestyle.electronics.is_some())
            .count();
        let staff = catalog
            .facilities
            .iter()
            .filter(|f| !f.staff_licenses.is_empty())
            .count();

        total_fac += fac;
        total_enriched += enriched;
        total_amenities += amenities;
        total_food += food;
        total_elec += elec;
        total_staff += staff;

        if enriched > 0 || state_filter.is_some() {
            println!(
                "{code}: {fac:4} total | {enriched:4} enriched | amenities:{amenities:3} food:{food:3} devices:{elec:3} staff:{staff:3} | shared-network:{} providers",
                catalog.staff_network.len()
            );
        }
    }

    println!(
        "TOTAL: {total_fac} facilities | {total_enriched} enriched | amenities:{total_amenities} food:{total_food} devices:{total_elec} staff:{total_staff}"
    );
    Ok(())
}

#[derive(Clone, Debug)]
pub struct AddProviderOptions {
    pub paths: PathsConfig,
    pub state: String,
    pub id: Option<String>,
    pub name: String,
    pub organization: Option<String>,
    pub npi: Option<String>,
    pub role: ProviderRole,
    pub arrangement: StaffArrangement,
    pub facility_ids: Vec<String>,
    pub notes: Option<String>,
}

/// Adds or updates a curated shared clinician or visiting/contracted specialist (e.g. a visiting
/// PMHNP or local yoga instructor serving multiple rehabs) in `data/recovery/overrides/<ST>.toml`
/// and immediately rebuilds that state's shard + `index.json`.
pub fn run_add_provider(opts: AddProviderOptions) -> Result<(), UpdaterError> {
    let state = opts.state.to_ascii_uppercase();
    let mut shards = load_existing_shards(&opts.paths.out_dir)?;
    let catalog = shards.get_mut(&state).ok_or_else(|| {
        UpdaterError::Usage(format!(
            "State shard `{state}` not found in {}",
            opts.paths.out_dir.display()
        ))
    })?;

    let valid_ids: std::collections::BTreeSet<String> =
        catalog.facilities.iter().map(|f| f.stable_id()).collect();
    for fid in &opts.facility_ids {
        if !valid_ids.contains(fid) {
            return Err(UpdaterError::Usage(format!(
                "Facility id `{fid}` not found in {state}. Run `recovery-index find --state {state} --query ...` to look up IDs."
            )));
        }
    }

    let id = opts.id.unwrap_or_else(|| {
        network::provider_slug_id(&state, &opts.name, opts.organization.as_deref())
    });
    let mut doc = load_state_overrides_file(&opts.paths.overrides_dir, &state)?;
    let mut facility_ids = opts.facility_ids;
    facility_ids.sort();
    facility_ids.dedup();

    if let Some(existing) = doc.providers.iter_mut().find(|p| p.id == id) {
        existing.name = opts.name.clone();
        if opts.organization.is_some() {
            existing.organization.clone_from(&opts.organization);
        }
        if opts.npi.is_some() {
            existing.npi.clone_from(&opts.npi);
        }
        existing.role = opts.role;
        existing.arrangement = opts.arrangement;
        for fid in facility_ids {
            if !existing.facility_ids.contains(&fid) {
                existing.facility_ids.push(fid);
            }
        }
        existing.facility_ids.sort();
        existing.facility_ids.dedup();
        if opts.notes.is_some() {
            existing.notes.clone_from(&opts.notes);
        }
        if !existing.sources.contains(&DataSource::Verified) {
            existing.sources.push(DataSource::Verified);
        }
    } else {
        doc.providers.push(NetworkProvider {
            id: id.clone(),
            name: opts.name.clone(),
            organization: opts.organization,
            npi: opts.npi,
            role: opts.role,
            arrangement: opts.arrangement,
            facility_ids,
            sources: vec![DataSource::Verified],
            notes: opts.notes,
        });
    }

    let saved = save_state_overrides_file(&opts.paths.overrides_dir, &state, &doc)?;
    network::rebuild_state_staff_network(
        catalog,
        &[],
        &doc.providers,
        network::NetworkRebuildMode::IncrementalMerge,
    );
    write_state_shard(&opts.paths.out_dir, catalog)?;
    rebuild_index_json(&opts.paths.out_dir)?;

    println!(
        "Saved provider `{}` ({id}) in {} and rebuilt {state}.json ({} network providers).",
        opts.name,
        saved.display(),
        catalog.staff_network.len()
    );
    Ok(())
}

/// Prints the top shared clinicians and visiting specialists in a state's cross-facility network.
pub fn run_network(
    paths: &PathsConfig,
    state: &str,
    top: usize,
    role_filter: Option<&str>,
) -> Result<(), UpdaterError> {
    let state = state.to_ascii_uppercase();
    let shards = load_existing_shards(&paths.out_dir)?;
    let catalog = shards
        .get(&state)
        .ok_or_else(|| UpdaterError::Usage(format!("State shard `{state}` not found.")))?;
    let name_by_id: HashMap<String, String> = catalog
        .facilities
        .iter()
        .map(|f| (f.stable_id(), format!("{} ({})", f.name, f.address.city)))
        .collect();

    println!(
        "Shared/visiting staff network for {state}: {} providers mapped",
        catalog.staff_network.len()
    );
    let mut shown = 0usize;
    for prov in &catalog.staff_network {
        if let Some(rf) = role_filter {
            let matches_role = match prov.role {
                ProviderRole::Clinical(lic) => lic.code().eq_ignore_ascii_case(rf),
                ProviderRole::Wellness(w) => w.code().eq_ignore_ascii_case(rf),
            };
            if !matches_role {
                continue;
            }
        }
        let org = prov
            .organization
            .as_deref()
            .map(|o| format!(" · {o}"))
            .unwrap_or_default();
        println!(
            "- [{}] {}{} | {} | {} ({} sites)",
            prov.id,
            prov.name,
            org,
            prov.role.short_label(),
            prov.arrangement.label(),
            prov.facility_ids.len()
        );
        for fid in prov.facility_ids.iter().take(6) {
            if let Some(fname) = name_by_id.get(fid) {
                println!("    • {fname}");
            }
        }
        if prov.facility_ids.len() > 6 {
            println!("    • (+{} more facilities)", prov.facility_ids.len() - 6);
        }
        shown += 1;
        if shown >= top {
            break;
        }
    }
    Ok(())
}
