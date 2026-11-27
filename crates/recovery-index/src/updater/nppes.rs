//! Layer 2: Cached CMS NPPES NPI Registry API v2.1 staff license enricher.
//!
//! Queries individual practitioners (`NPI-1`) by `(state, zip)` once per ZIP code, caches the
//! result on disk, matches practitioners to a facility by street address or phone number, and
//! classifies each practitioner into a strict, non-conflated [`StaffLicense`]:
//! - An MD is never conflated with a DO (`MdPsychiatrist` vs `DoPsychiatrist`, etc.)
//! - A Psychiatrist is never conflated with a Nurse Practitioner (`Pmhnp` / `Np`) or `Pa`
//! - A PhD Psychologist is distinct from a PsyD Psychologist.

use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use futures_util::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};

use crate::schema::{
    DataSource, EnrichmentMeta, Facility, ProviderRole, StaffArrangement, StaffLicense,
};
use crate::updater::cache::{CacheMode, DiskCache};
use crate::updater::network::ProviderObservation;

const NPPES_API_URL: &str = "https://npiregistry.cms.hhs.gov/api/";
const NPPES_TTL: Duration = Duration::from_secs(30 * 24 * 60 * 60);
const NPPES_PAGE_LIMIT: usize = 200;
const NPPES_MAX_PAGES_PER_ZIP: usize = 5; // Up to 1,000 individual providers per ZIP
const NPPES_CONCURRENCY: usize = 3;
const NPPES_PACE_DELAY: Duration = Duration::from_millis(150);

/// Compact cached projection of an individual NPPES practitioner record.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CachedNppesProvider {
    pub npi: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub full_name: Option<String>,
    pub credential: String,
    pub taxonomies: Vec<String>,
    pub taxonomy_descs: Vec<String>,
    pub street_norm: Option<(String, String)>,
    pub phone_digits: Option<String>,
    /// Additional secondary practice addresses from NPPES `practiceLocations` / `addresses`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_streets: Vec<(String, String)>,
    /// Additional practice phone numbers from NPPES `practiceLocations` / `addresses`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_phones: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct NppesResponse {
    #[serde(default)]
    result_count: usize,
    #[serde(default)]
    results: Vec<NppesResult>,
}

#[derive(Debug, Deserialize)]
struct NppesResult {
    #[serde(default)]
    number: String,
    #[serde(default)]
    basic: NppesBasic,
    #[serde(default)]
    addresses: Vec<NppesAddress>,
    #[serde(default, rename = "practiceLocations")]
    practice_locations: Vec<NppesAddress>,
    #[serde(default)]
    taxonomies: Vec<NppesTaxonomy>,
}

#[derive(Debug, Default, Deserialize)]
struct NppesBasic {
    #[serde(default)]
    first_name: Option<String>,
    #[serde(default)]
    last_name: Option<String>,
    #[serde(default)]
    credential: Option<String>,
    #[serde(default)]
    status: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct NppesAddress {
    #[serde(default)]
    address_purpose: Option<String>,
    #[serde(default)]
    address_1: Option<String>,
    #[serde(default)]
    telephone_number: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct NppesTaxonomy {
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    desc: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct NppesMatchSummary {
    pub licenses: Vec<StaffLicense>,
    pub matched_providers: u32,
    pub matched_records: Vec<(String, Option<String>, String, StaffLicense)>,
}

/// Enriches a slice of facilities using cached ZIP-level NPPES queries and returns
/// `(cache_hits, network_fetches, facilities_enriched, provider_observations)`.
///
/// For any NPI matched across 2+ facilities whose display name wasn't present in an older ZIP cache,
/// performs a polite cached NPI detail lookup (`?version=2.1&number=<NPI>`) to resolve the
/// clinician's full name and secondary `practiceLocations` across the state.
pub async fn enrich_facilities_with_nppes(
    client: &reqwest::Client,
    cache: &DiskCache,
    facilities: &mut [Facility],
) -> (usize, usize, usize, Vec<ProviderObservation>) {
    // Deduplicate `(state, zip)` pairs across all facilities so each ZIP is queried at most once.
    let mut zip_providers: HashMap<(String, String), Vec<CachedNppesProvider>> = HashMap::new();
    let mut unique_zips: Vec<(String, String)> = Vec::new();
    for f in facilities.iter() {
        let key = (f.address.state.clone(), f.address.zip.clone());
        if key.1.len() == 5 && !zip_providers.contains_key(&key) {
            zip_providers.insert(key.clone(), Vec::new());
            unique_zips.push(key);
        }
    }

    let mut cache_hits = 0usize;
    let mut to_fetch: Vec<(String, String)> = Vec::new();

    for (state, zip) in unique_zips {
        let cache_key = format!("npi1-{state}-{zip}");
        if let Some(hit) = cache.get::<Vec<CachedNppesProvider>>("nppes", &cache_key, NPPES_TTL) {
            zip_providers.insert((state, zip), hit.value);
            cache_hits += 1;
        } else if cache.mode() != CacheMode::Offline {
            to_fetch.push((state, zip));
        }
    }

    let total_fetch = to_fetch.len();
    let done_counter = AtomicUsize::new(0);
    let net_fetches = AtomicUsize::new(0);

    let fetched_results: Vec<((String, String), Vec<CachedNppesProvider>)> = stream::iter(to_fetch)
        .map(|(state, zip)| {
            let done_ref = &done_counter;
            let net_ref = &net_fetches;
            async move {
                let cache_key = format!("npi1-{state}-{zip}");
                tokio::time::sleep(NPPES_PACE_DELAY).await;
                let providers = match fetch_zip_providers(client, &state, &zip).await {
                    Ok(providers) => {
                        let _ = cache.put("nppes", &cache_key, &providers, None, None);
                        net_ref.fetch_add(1, Ordering::Relaxed);
                        providers
                    }
                    Err(err) => {
                        eprintln!("recovery-index: NPPES fetch warning for {state} {zip}: {err}");
                        cache
                            .get_stale::<Vec<CachedNppesProvider>>("nppes", &cache_key)
                            .map(|e| e.value)
                            .unwrap_or_default()
                    }
                };
                let done = done_ref.fetch_add(1, Ordering::Relaxed) + 1;
                if total_fetch >= 50 && (done.is_multiple_of(50) || done == total_fetch) {
                    eprintln!("recovery-index: NPPES progress {done}/{total_fetch} ZIPs");
                }
                ((state, zip), providers)
            }
        })
        .buffer_unordered(NPPES_CONCURRENCY)
        .collect()
        .await;

    for (key, providers) in fetched_results {
        zip_providers.insert(key, providers);
    }
    let net_fetches = net_fetches.load(Ordering::Relaxed);

    let mut enriched_count = 0usize;
    let mut observations: Vec<ProviderObservation> = Vec::new();
    let mut npi_facility_counts: HashMap<String, (BTreeSet<String>, StaffLicense)> = HashMap::new();
    let mut npi_missing_name: BTreeSet<String> = BTreeSet::new();

    for f in facilities.iter_mut() {
        let key = (f.address.state.clone(), f.address.zip.clone());
        let Some(providers) = zip_providers.get(&key) else {
            continue;
        };
        let fid = f.stable_id();
        let summary = match_facility_providers(f, providers);
        if summary.matched_providers > 0 && !summary.licenses.is_empty() {
            for lic in summary.licenses {
                if !f.staff_licenses.contains(&lic) {
                    f.staff_licenses.push(lic);
                }
            }
            f.staff_licenses.sort_unstable();
            f.staff_licenses.dedup();
            let meta = f.enrichment.get_or_insert_with(EnrichmentMeta::default);
            meta.add_source(DataSource::Nppes);
            meta.nppes_provider_count = Some(summary.matched_providers);
            enriched_count += 1;

            for (npi, full_name, cred, lic) in summary.matched_records {
                npi_facility_counts
                    .entry(npi.clone())
                    .or_insert_with(|| (BTreeSet::new(), lic))
                    .0
                    .insert(fid.clone());
                if full_name.is_none() {
                    npi_missing_name.insert(npi.clone());
                }
                let display_name = match full_name {
                    Some(name) if !cred.trim().is_empty() => format!("{name}, {}", cred.trim()),
                    Some(name) => name,
                    None => format!("NPI {npi}"),
                };
                observations.push(ProviderObservation {
                    id: format!("npi-{npi}"),
                    name: display_name,
                    organization: None,
                    npi: Some(npi),
                    role: ProviderRole::Clinical(lic),
                    arrangement_hint: StaffArrangement::Dedicated,
                    facility_id: fid.clone(),
                    source: DataSource::Nppes,
                    notes: None,
                });
            }
        }
    }

    // Prioritize resolving human-readable display names for the highest-impact shared prescribers
    // and multi-site clinicians first (most sites served, prescribers ahead of non-prescribers).
    let mut shared_ranked: Vec<(usize, bool, String)> = npi_facility_counts
        .iter()
        .filter(|(npi, (fids, _))| fids.len() >= 2 && npi_missing_name.contains(*npi))
        .map(|(npi, (fids, lic))| (fids.len(), lic.is_prescriber(), npi.clone()))
        .collect();
    shared_ranked.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| b.1.cmp(&a.1))
            .then_with(|| a.2.cmp(&b.2))
    });
    let shared_needing_names: Vec<String> =
        shared_ranked.into_iter().map(|(_, _, npi)| npi).collect();

    if !shared_needing_names.is_empty() {
        let resolved_names = resolve_shared_npi_details(client, cache, &shared_needing_names).await;
        for obs in &mut observations {
            if let Some(npi) = &obs.npi {
                if let Some((name, cred)) = resolved_names.get(npi) {
                    obs.name = if cred.trim().is_empty() {
                        name.clone()
                    } else {
                        format!("{name}, {}", cred.trim())
                    };
                }
            }
        }
    }

    (cache_hits, net_fetches, enriched_count, observations)
}

async fn resolve_shared_npi_details(
    client: &reqwest::Client,
    cache: &DiskCache,
    npis: &[String],
) -> HashMap<String, (String, String)> {
    let mut out = HashMap::new();
    let mut to_fetch = Vec::new();
    for npi in npis {
        let key = format!("npi-detail-{npi}");
        if let Some(hit) = cache.get::<(String, String)>("nppes", &key, NPPES_TTL) {
            out.insert(npi.clone(), hit.value);
        } else if cache.mode() != CacheMode::Offline {
            to_fetch.push(npi.clone());
        }
    }
    // Keep name resolution fast & polite even on large states.
    const MAX_DETAIL_LOOKUPS_PER_RUN: usize = 120;
    to_fetch.truncate(MAX_DETAIL_LOOKUPS_PER_RUN);

    let fetched: Vec<(String, Option<(String, String)>)> = stream::iter(to_fetch)
        .map(|npi| async move {
            tokio::time::sleep(NPPES_PACE_DELAY).await;
            let key = format!("npi-detail-{npi}");
            let res = fetch_single_npi_name(client, &npi).await.ok().flatten();
            if let Some(pair) = &res {
                let _ = cache.put("nppes", &key, pair, None, None);
            }
            (npi, res)
        })
        .buffer_unordered(NPPES_CONCURRENCY)
        .collect()
        .await;

    for (npi, pair) in fetched {
        if let Some(pair) = pair {
            out.insert(npi, pair);
        }
    }
    out
}

async fn fetch_single_npi_name(
    client: &reqwest::Client,
    npi: &str,
) -> Result<Option<(String, String)>, String> {
    let url = url::Url::parse_with_params(NPPES_API_URL, &[("version", "2.1"), ("number", npi)])
        .map_err(|e| e.to_string())?;
    let resp = client
        .get(url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|e| e.to_string())?;
    let body = resp.bytes().await.map_err(|e| e.to_string())?;
    let parsed: NppesResponse = serde_json::from_slice(&body).map_err(|e| e.to_string())?;
    let Some(first) = parsed.results.into_iter().next() else {
        return Ok(None);
    };
    let Some(name) = format_person_name(&first.basic) else {
        return Ok(None);
    };
    Ok(Some((name, first.basic.credential.unwrap_or_default())))
}

fn format_person_name(basic: &NppesBasic) -> Option<String> {
    let first = basic
        .first_name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())?;
    let last = basic
        .last_name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())?;
    Some(format!("{} {}", title_case(first), title_case(last)))
}

fn title_case(word: &str) -> String {
    word.split_whitespace()
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => {
                    first.to_uppercase().collect::<String>() + &chars.as_str().to_ascii_lowercase()
                }
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

async fn fetch_zip_providers(
    client: &reqwest::Client,
    state: &str,
    zip: &str,
) -> Result<Vec<CachedNppesProvider>, String> {
    let mut all = Vec::new();
    for page in 0..NPPES_MAX_PAGES_PER_ZIP {
        let skip = page * NPPES_PAGE_LIMIT;
        let url = url::Url::parse_with_params(
            NPPES_API_URL,
            &[
                ("version", "2.1".to_string()),
                ("enumeration_type", "NPI-1".to_string()),
                ("state", state.to_string()),
                ("postal_code", zip.to_string()),
                ("limit", NPPES_PAGE_LIMIT.to_string()),
                ("skip", skip.to_string()),
            ],
        )
        .map_err(|e| e.to_string())?;
        let resp = client
            .get(url)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|e| e.to_string())?;

        let body = resp.bytes().await.map_err(|e| e.to_string())?;
        let parsed: NppesResponse = serde_json::from_slice(&body).map_err(|e| e.to_string())?;
        let count = parsed.results.len();

        for r in parsed.results {
            // Skip deactivated NPIs.
            if r.basic.status.as_deref() == Some("D") {
                continue;
            }
            let location = r
                .addresses
                .iter()
                .find(|a| a.address_purpose.as_deref() == Some("LOCATION"))
                .or_else(|| r.addresses.first());
            let street_norm = location
                .and_then(|a| a.address_1.as_deref())
                .and_then(normalize_street_key);
            let phone_digits = location
                .and_then(|a| a.telephone_number.as_deref())
                .and_then(normalize_phone10);
            let mut extra_streets = Vec::new();
            let mut extra_phones = Vec::new();
            for addr in r.addresses.iter().chain(r.practice_locations.iter()) {
                if let Some(st) = addr.address_1.as_deref().and_then(normalize_street_key) {
                    if Some(&st) != street_norm.as_ref() && !extra_streets.contains(&st) {
                        extra_streets.push(st);
                    }
                }
                if let Some(ph) = addr.telephone_number.as_deref().and_then(normalize_phone10) {
                    if Some(&ph) != phone_digits.as_ref() && !extra_phones.contains(&ph) {
                        extra_phones.push(ph);
                    }
                }
            }
            let taxonomies: Vec<String> =
                r.taxonomies.iter().filter_map(|t| t.code.clone()).collect();
            let taxonomy_descs: Vec<String> =
                r.taxonomies.iter().filter_map(|t| t.desc.clone()).collect();

            all.push(CachedNppesProvider {
                npi: r.number,
                full_name: format_person_name(&r.basic),
                credential: r.basic.credential.unwrap_or_default(),
                taxonomies,
                taxonomy_descs,
                street_norm,
                phone_digits,
                extra_streets,
                extra_phones,
            });
        }

        if count < NPPES_PAGE_LIMIT || parsed.result_count < NPPES_PAGE_LIMIT {
            break;
        }
    }
    Ok(all)
}

/// Matches cached individual NPPES providers to `facility` by practice street address or phone.
pub fn match_facility_providers(
    facility: &Facility,
    providers: &[CachedNppesProvider],
) -> NppesMatchSummary {
    let fac_street = facility
        .address
        .street
        .as_deref()
        .and_then(normalize_street_key);
    let fac_phone = facility.phone.as_deref().and_then(normalize_phone10);

    if fac_street.is_none() && fac_phone.is_none() {
        return NppesMatchSummary::default();
    }

    let mut matched_npis = BTreeSet::new();
    let mut licenses = BTreeSet::new();
    let mut matched_records = Vec::new();

    for prov in providers {
        let street_match = fac_street.as_ref().is_some_and(|fs| {
            prov.street_norm.as_ref() == Some(fs) || prov.extra_streets.contains(fs)
        });
        let phone_match = fac_phone.as_ref().is_some_and(|fp| {
            prov.phone_digits.as_ref() == Some(fp) || prov.extra_phones.contains(fp)
        });
        if !street_match && !phone_match {
            continue;
        }
        if let Some(lic) =
            classify_nppes_provider(&prov.credential, &prov.taxonomies, &prov.taxonomy_descs)
        {
            if matched_npis.insert(prov.npi.as_str()) {
                matched_records.push((
                    prov.npi.clone(),
                    prov.full_name.clone(),
                    prov.credential.clone(),
                    lic,
                ));
            }
            licenses.insert(lic);
        }
    }

    NppesMatchSummary {
        licenses: licenses.into_iter().collect(),
        matched_providers: u32::try_from(matched_npis.len()).unwrap_or(u32::MAX),
        matched_records,
    }
}

/// Strictly classifies an individual NPPES practitioner's credential + NUCC taxonomies into a
/// single exact [`StaffLicense`].
///
/// Key invariants:
/// - `MD` never classifies as `DO`, and `DO` never classifies as `MD`.
/// - A `2084P*` taxonomy with no explicit MD or DO credential returns `None` rather than guessing.
/// - `PMHNP` / `NP` / `APRN` and `PA` never classify as a Psychiatrist or Physician.
pub fn classify_nppes_provider(
    credential_raw: &str,
    taxonomies: &[String],
    taxonomy_descs: &[String],
) -> Option<StaffLicense> {
    let tokens = credential_tokens(credential_raw);
    let has_tok = |target: &str| tokens.iter().any(|t| t == target);
    let has_tax_prefix = |prefix: &str| taxonomies.iter().any(|code| code.starts_with(prefix));
    let has_tax_exact = |exact: &str| taxonomies.iter().any(|code| code == exact);
    let desc_has = |needle: &str| {
        taxonomy_descs
            .iter()
            .any(|d| d.to_ascii_lowercase().contains(needle))
    };

    let is_md = has_tok("MD") || has_tok("MBBS");
    let is_do = has_tok("DO");

    let is_psych_tax = has_tax_prefix("2084P")
        || has_tax_exact("2084F0202X")
        || has_tax_exact("2084B0040X")
        || has_tax_exact("2084A2900X")
        || desc_has("psychiatry");
    let is_addiction_tax = has_tax_exact("207QA0401X")
        || has_tax_exact("2083A0300X")
        || has_tax_exact("2084A0401X")
        || has_tax_exact("207LA0401X")
        || has_tax_exact("207RA0401X")
        || desc_has("addiction medicine");

    // 1. Physicians: strictly separate MD vs DO, never guess degree if missing.
    if is_md && !is_do {
        if is_psych_tax {
            return Some(StaffLicense::MdPsychiatrist);
        }
        if is_addiction_tax {
            return Some(StaffLicense::MdAddiction);
        }
        if has_tax_prefix("20") || tokens.iter().any(|t| t == "MD") {
            return Some(StaffLicense::MdPhysician);
        }
    }
    if is_do && !is_md {
        if is_psych_tax {
            return Some(StaffLicense::DoPsychiatrist);
        }
        if is_addiction_tax {
            return Some(StaffLicense::DoAddiction);
        }
        if has_tax_prefix("20") || tokens.iter().any(|t| t == "DO") {
            return Some(StaffLicense::DoPhysician);
        }
    }

    // 2. Advanced Practice Providers (strictly separate from MD/DO psychiatrists/physicians).
    if has_tax_exact("363LP0808X")
        || has_tok("PMHNP")
        || has_tok("PMHNPBC")
        || has_tok("NPP")
        || (has_tax_prefix("363L") && desc_has("psych"))
    {
        return Some(StaffLicense::Pmhnp);
    }
    if has_tax_prefix("363L")
        || has_tok("NP")
        || has_tok("FNP")
        || has_tok("FNPC")
        || has_tok("AGNP")
        || has_tok("APRN")
        || has_tok("ARNP")
        || has_tok("CRNP")
        || has_tok("DNP")
    {
        return Some(StaffLicense::Np);
    }
    if has_tax_prefix("363A") || has_tok("PA") || has_tok("PAC") {
        return Some(StaffLicense::Pa);
    }

    // 3. Doctoral Psychologists (PhD vs PsyD strictly separated).
    if has_tax_prefix("103T") {
        if has_tok("PSYD") {
            return Some(StaffLicense::PsydPsychologist);
        }
        if has_tok("PHD") {
            return Some(StaffLicense::PhdPsychologist);
        }
    }
    if has_tok("PSYD") {
        return Some(StaffLicense::PsydPsychologist);
    }

    // 4. Master's Therapists & Clinical Social Workers.
    if has_tax_exact("1041C0700X")
        || has_tok("LCSW")
        || has_tok("LICSW")
        || has_tok("LISW")
        || has_tok("LCSWC")
    {
        return Some(StaffLicense::Lcsw);
    }
    if has_tax_exact("106H00000X") || has_tok("LMFT") || has_tok("MFT") {
        return Some(StaffLicense::Lmft);
    }
    if has_tax_exact("101YP2500X")
        || has_tax_exact("101YM0800X")
        || has_tok("LPC")
        || has_tok("LPCC")
        || has_tok("LMHC")
        || has_tok("LCPC")
        || has_tok("LCMHC")
    {
        return Some(StaffLicense::LpcLmhc);
    }

    // 5. Addiction Counselors, Peer Support, and Nursing.
    if has_tax_exact("101YA0400X")
        || has_tok("LADC")
        || has_tok("CADC")
        || has_tok("LCDC")
        || has_tok("CASAC")
        || has_tok("LAC")
        || has_tok("CAC")
    {
        return Some(StaffLicense::AddictionCounselor);
    }
    if has_tax_exact("175T00000X") || has_tok("CPRS") || has_tok("CPSS") {
        return Some(StaffLicense::PeerSupport);
    }
    if has_tax_prefix("163W") || has_tok("RN") || has_tok("BSN") {
        return Some(StaffLicense::Rn);
    }
    if has_tax_prefix("164W") || has_tok("LPN") || has_tok("LVN") {
        return Some(StaffLicense::Lpn);
    }

    None
}

/// Splits a raw NPPES credential like `"M.D., Ph.D."` or `"PMHNP-BC"` into uppercase alphanumeric tokens.
fn credential_tokens(raw: &str) -> Vec<String> {
    let mut out = Vec::new();
    for part in raw.split([',', '/', ';', ' ', '\t', '(', ')']) {
        let cleaned: String = part
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .map(|c| c.to_ascii_uppercase())
            .collect();
        if !cleaned.is_empty() {
            out.push(cleaned);
        }
        // Also split hyphenated credentials like "PA-C" -> ["PAC", "PA", "C"] and "PMHNP-BC" -> ["PMHNPBC", "PMHNP"].
        if part.contains('-') {
            for sub in part.split('-') {
                let sub_clean: String = sub
                    .chars()
                    .filter(|c| c.is_ascii_alphanumeric())
                    .map(|c| c.to_ascii_uppercase())
                    .collect();
                if !sub_clean.is_empty() && !out.contains(&sub_clean) {
                    out.push(sub_clean);
                }
            }
        }
    }
    out
}

/// Normalizes a street address to `(street_number, primary_street_word)` so `"501 South Burma Avenue"`
/// and `"501 S BURMA AVE"` match reliably without false positives.
pub fn normalize_street_key(street: &str) -> Option<(String, String)> {
    let tokens: Vec<String> = street
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(str::to_ascii_lowercase)
        .collect();
    let number = tokens
        .first()
        .filter(|t| t.bytes().all(|b| b.is_ascii_digit()))?
        .clone();
    let primary = tokens
        .iter()
        .skip(1)
        .find(|t| {
            !matches!(
                t.as_str(),
                "n" | "s"
                    | "e"
                    | "w"
                    | "ne"
                    | "nw"
                    | "se"
                    | "sw"
                    | "north"
                    | "south"
                    | "east"
                    | "west"
                    | "st"
                    | "street"
                    | "ave"
                    | "avenue"
                    | "blvd"
                    | "boulevard"
                    | "dr"
                    | "drive"
                    | "rd"
                    | "road"
                    | "ln"
                    | "lane"
                    | "ct"
                    | "court"
                    | "cir"
                    | "circle"
                    | "pkwy"
                    | "parkway"
                    | "way"
                    | "pl"
                    | "place"
                    | "hwy"
                    | "highway"
                    | "ste"
                    | "suite"
                    | "unit"
                    | "apt"
                    | "bldg"
            )
        })?
        .clone();
    Some((number, primary))
}

/// Normalizes a US phone string to its 10 core digits (ignoring extensions and leading country `1`).
pub fn normalize_phone10(raw: &str) -> Option<String> {
    let main = raw.split(['x', 'X']).next().unwrap_or(raw);
    let mut digits: String = main.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() == 11 && digits.starts_with('1') {
        digits.remove(0);
    }
    (digits.len() == 10).then_some(digits)
}
