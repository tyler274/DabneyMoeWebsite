//! Layer 1: Cached SAMHSA FindTreatment.gov v2 importer.
//!
//! Pulls (and caches on disk) SAMHSA export pages, merges dual SA/MH listings, and extracts
//! both care metadata (`SET`, `PAY`/`PYAS`, `SG`, `typeFacility`) and SAMHSA's non-care
//! lifestyle/policy fields (`SMP` smoking policy, `FVP` vaping policy, `RSS`/`AS` housing,
//! transportation & childcare, and `SL` languages).

use std::collections::BTreeMap;
use std::fs;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use url::Url;

use crate::schema::{
    Address, Amenity, CareType, Coordinates, DataSource, EnrichmentMeta, Facility,
    FacilityLifestyle, Group, Payment, Setting, SmokingPolicy, StateCatalog, VapingPolicy,
};
use crate::updater::cache::{CacheMode, DiskCache};

const EXPORT_URL: &str = "https://findtreatment.gov/locator/exportsAsJson/v2";
const PAGE_SIZE: u32 = 2000;
const SAMHSA_TTL: Duration = Duration::from_secs(7 * 24 * 60 * 60);

#[derive(Debug, thiserror::Error)]
pub enum SamhsaError {
    #[error("offline mode enabled and SAMHSA page {page} is not cached")]
    OfflineMiss { page: u32 },
    #[error("could not fetch SAMHSA page {page}")]
    Fetch {
        page: u32,
        #[source]
        source: reqwest::Error,
    },
    #[error("could not read raw SAMHSA file {path}")]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("SAMHSA page {origin} did not parse as expected JSON")]
    Parse {
        origin: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("SAMHSA export reported {expected} rows but pages held {fetched}")]
    RowCount { expected: u32, fetched: usize },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawPage {
    pub total_pages: u32,
    pub record_count: u32,
    pub rows: Vec<RawRow>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawRow {
    #[serde(rename = "_irow", default)]
    pub irow: u64,
    pub name1: String,
    #[serde(default)]
    pub name2: Option<String>,
    #[serde(default)]
    pub street1: Option<String>,
    #[serde(default)]
    pub street2: Option<String>,
    pub city: String,
    pub state: String,
    pub zip: String,
    #[serde(default)]
    pub phone: Option<String>,
    #[serde(default)]
    pub website: Option<String>,
    #[serde(default)]
    pub latitude: Option<String>,
    #[serde(default)]
    pub longitude: Option<String>,
    pub type_facility: String,
    #[serde(default)]
    pub services: Vec<RawService>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RawService {
    pub f2: String,
    pub f3: String,
}

pub fn read_pages_from_files(paths: &[std::path::PathBuf]) -> Result<Vec<RawPage>, SamhsaError> {
    paths
        .iter()
        .map(|path| {
            let text = fs::read_to_string(path).map_err(|source| SamhsaError::Read {
                path: path.display().to_string(),
                source,
            })?;
            serde_json::from_str(&text).map_err(|source| SamhsaError::Parse {
                origin: path.display().to_string(),
                source,
            })
        })
        .collect()
}

pub async fn fetch_pages_cached(
    client: &reqwest::Client,
    cache: &DiskCache,
) -> Result<Vec<RawPage>, SamhsaError> {
    let first = fetch_one_page(client, cache, 1).await?;
    let total_pages = first.total_pages;
    let expected = first.record_count;
    let mut pages = Vec::with_capacity(total_pages as usize);
    pages.push(first);
    for page in 2..=total_pages {
        pages.push(fetch_one_page(client, cache, page).await?);
    }
    let fetched: usize = pages.iter().map(|p| p.rows.len()).sum();
    if usize::try_from(expected).ok() != Some(fetched) {
        return Err(SamhsaError::RowCount { expected, fetched });
    }
    Ok(pages)
}

async fn fetch_one_page(
    client: &reqwest::Client,
    cache: &DiskCache,
    page: u32,
) -> Result<RawPage, SamhsaError> {
    let key = format!("v2-both-size{PAGE_SIZE}-page{page}");
    if let Some(hit) = cache.get::<RawPage>("samhsa", &key, SAMHSA_TTL) {
        return Ok(hit.value);
    }
    if cache.mode() == CacheMode::Offline {
        return Err(SamhsaError::OfflineMiss { page });
    }
    eprintln!("recovery-index: fetching SAMHSA page {page}...");
    let url = Url::parse_with_params(
        EXPORT_URL,
        &[
            ("sType", "both".to_string()),
            ("pageSize", PAGE_SIZE.to_string()),
            ("page", page.to_string()),
        ],
    )
    .expect("valid SAMHSA export URL");
    let resp = client
        .get(url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|source| SamhsaError::Fetch { page, source })?;
    let bytes = resp
        .bytes()
        .await
        .map_err(|source| SamhsaError::Fetch { page, source })?;
    let parsed: RawPage = serde_json::from_slice(&bytes).map_err(|source| SamhsaError::Parse {
        origin: format!("page {page}"),
        source,
    })?;
    let _ = cache.put("samhsa", &key, &parsed, None, None);
    Ok(parsed)
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct MergeKey([String; 4]);

impl MergeKey {
    fn of(f: &Facility) -> Self {
        let norm = |s: Option<&str>| {
            s.unwrap_or_default()
                .split_whitespace()
                .map(str::to_lowercase)
                .collect::<Vec<_>>()
                .join(" ")
        };
        Self([
            norm(Some(&f.name)),
            norm(f.site_name.as_deref()),
            norm(f.address.street.as_deref()),
            f.address.zip.clone(),
        ])
    }
}

/// Converts raw SAMHSA pages into per-state [`StateCatalog`]s (filtering to `state_filter` if non-empty).
pub fn build_catalogs_from_pages(
    pages: Vec<RawPage>,
    retrieved: &str,
    state_filter: &[String],
) -> Vec<StateCatalog> {
    let mut by_state: BTreeMap<String, BTreeMap<MergeKey, Facility>> = BTreeMap::new();

    for row in pages.into_iter().flat_map(|p| p.rows) {
        let state = row.state.trim().to_ascii_uppercase();
        if state.len() != 2 || !state.bytes().all(|b| b.is_ascii_uppercase()) {
            continue;
        }
        if !state_filter.is_empty() && !state_filter.iter().any(|s| s == &state) {
            continue;
        }
        let Some(facility) = map_row(row, &state) else {
            continue;
        };
        by_state
            .entry(state)
            .or_default()
            .entry(MergeKey::of(&facility))
            .and_modify(|kept| merge_facilities(kept, &facility))
            .or_insert(facility);
    }

    by_state
        .into_iter()
        .map(|(state, facilities_map)| {
            let mut facilities: Vec<Facility> = facilities_map.into_values().collect();
            facilities.sort_by(|a, b| {
                a.name
                    .to_lowercase()
                    .cmp(&b.name.to_lowercase())
                    .then_with(|| a.address.zip.cmp(&b.address.zip))
                    .then_with(|| a.site_name.cmp(&b.site_name))
            });
            for f in &mut facilities {
                f.id = Some(f.stable_id());
            }
            StateCatalog {
                state,
                retrieved: retrieved.to_string(),
                facilities,
            }
        })
        .collect()
}

fn map_row(row: RawRow, state: &str) -> Option<Facility> {
    let care_type = CareType::from_samhsa_type(&row.type_facility)?;
    let zip: String = row
        .zip
        .trim()
        .chars()
        .filter(|c| c.is_ascii_digit())
        .take(5)
        .collect();
    if zip.len() != 5 {
        return None;
    }

    let mut settings = Vec::new();
    let mut payments = Vec::new();
    let mut groups = Vec::new();
    let mut lifestyle = FacilityLifestyle::default();

    for svc in &row.services {
        let labels = svc.f3.split("; ").map(str::trim).filter(|l| !l.is_empty());
        match svc.f2.as_str() {
            "SET" => {
                for l in labels {
                    if let Some(v) = Setting::from_samhsa_label(l) {
                        settings.push(v);
                    }
                }
            }
            "PAY" | "PYAS" => {
                for l in labels {
                    if let Some(v) = Payment::from_samhsa_label(l) {
                        payments.push(v);
                    }
                }
            }
            "SG" => {
                for l in labels {
                    if let Some(v) = Group::from_samhsa_label(l) {
                        groups.push(v);
                    }
                }
            }
            "SMP" => {
                for l in labels {
                    if let Some(v) = SmokingPolicy::from_samhsa_label(l) {
                        lifestyle.smoking = Some(v);
                    }
                }
            }
            "FVP" => {
                for l in labels {
                    if let Some(v) = VapingPolicy::from_samhsa_label(l) {
                        lifestyle.vaping = Some(v);
                    }
                }
            }
            "RSS" | "AS" => {
                for l in labels {
                    let lower = l.to_ascii_lowercase();
                    if lower.contains("housing") {
                        lifestyle.amenities.push(Amenity::HousingAssistance);
                    }
                    if lower.contains("transportation") {
                        lifestyle.amenities.push(Amenity::Transportation);
                    }
                    if lower.contains("child care") || lower.contains("childcare") {
                        lifestyle.amenities.push(Amenity::Childcare);
                    }
                }
            }
            "SL" => {
                for l in labels {
                    if !l.is_empty() && l.len() <= 64 {
                        lifestyle.languages.push(l.to_string());
                    }
                }
            }
            _ => {}
        }
    }

    settings.sort_unstable();
    settings.dedup();
    payments.sort_unstable();
    payments.dedup();
    groups.sort_unstable();
    groups.dedup();
    lifestyle.normalize();

    let location = parse_coords(row.latitude.as_deref(), row.longitude.as_deref());
    let mut enrichment = EnrichmentMeta::default();
    if !lifestyle.is_empty() {
        enrichment.add_source(DataSource::Samhsa);
    }

    let mut facility = Facility {
        id: None,
        name: row.name1.trim().to_string(),
        site_name: clean_opt(row.name2),
        address: Address {
            street: clean_opt(row.street1),
            street2: clean_opt(row.street2),
            city: row.city.trim().to_string(),
            state: state.to_string(),
            zip,
        },
        phone: clean_opt(row.phone)
            .filter(|p| p.chars().filter(|c| c.is_ascii_digit()).count() >= 10),
        website: clean_opt(row.website).and_then(|u| clean_website(&u)),
        location,
        care: vec![care_type],
        settings,
        payments,
        groups,
        lifestyle,
        staff_licenses: Vec::new(),
        enrichment: (!enrichment.is_empty()).then_some(enrichment),
    };
    facility.id = Some(facility.stable_id());
    Some(facility)
}

fn merge_facilities(kept: &mut Facility, other: &Facility) {
    fn union<T: Ord + Copy>(into: &mut Vec<T>, from: &[T]) {
        into.extend_from_slice(from);
        into.sort_unstable();
        into.dedup();
    }
    union(&mut kept.care, &other.care);
    union(&mut kept.settings, &other.settings);
    union(&mut kept.payments, &other.payments);
    union(&mut kept.groups, &other.groups);
    union(&mut kept.lifestyle.amenities, &other.lifestyle.amenities);
    union(&mut kept.lifestyle.dietary, &other.lifestyle.dietary);
    kept.lifestyle
        .languages
        .extend(other.lifestyle.languages.iter().cloned());
    if kept.lifestyle.smoking.is_none() {
        kept.lifestyle.smoking = other.lifestyle.smoking;
    }
    if kept.lifestyle.vaping.is_none() {
        kept.lifestyle.vaping = other.lifestyle.vaping;
    }
    kept.lifestyle.normalize();
    if kept.address.street2.is_none() {
        kept.address.street2.clone_from(&other.address.street2);
    }
    if kept.phone.is_none() {
        kept.phone.clone_from(&other.phone);
    }
    if kept.website.is_none() {
        kept.website.clone_from(&other.website);
    }
    if kept.location.is_none() {
        kept.location = other.location;
    }
    if !kept.lifestyle.is_empty() {
        kept.enrichment
            .get_or_insert_with(EnrichmentMeta::default)
            .add_source(DataSource::Samhsa);
    }
}

fn clean_opt(val: Option<String>) -> Option<String> {
    val.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn clean_website(raw: &str) -> Option<String> {
    let url = Url::parse(raw).ok()?;
    if matches!(url.scheme(), "http" | "https") && url.host_str().is_some_and(|h| !h.is_empty()) {
        Some(url.into())
    } else {
        None
    }
}

fn parse_coords(lat: Option<&str>, lng: Option<&str>) -> Option<Coordinates> {
    let lat: f64 = lat?.trim().parse().ok()?;
    let lng: f64 = lng?.trim().parse().ok()?;
    if lat.is_finite()
        && lng.is_finite()
        && (-90.0..=90.0).contains(&lat)
        && (-180.0..=180.0).contains(&lng)
    {
        Some(Coordinates { lat, lng })
    } else {
        None
    }
}
