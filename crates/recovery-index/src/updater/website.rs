//! Layer 3: Polite, disk-cached public facility website extractor.
//!
//! Fetches a facility's homepage plus up to 4 high-signal same-origin subpages (amenities,
//! accommodations, dining, what-to-bring, FAQ, staff/team) with disk caching (TTL 14 days +
//! ETag/Last-Modified revalidation), strips HTML to clean visible text, and extracts:
//! - Non-care amenities (pool, hot tub, sauna, gym, yoga, spa, private/shared rooms, pets, etc.)
//! - Food situation (chef-prepared, dietitian-catered, cafeteria, self-catered) + dietary options
//! - Electronic device policy (with negation handling so "no cell phones" never reads as allowed!)
//! - Strict staff license credentials from team/staff bios (MD vs DO, Psychiatrist vs PMHNP/NP vs PA).

use std::collections::BTreeSet;
use std::sync::OnceLock;
use std::time::Duration;

use regex::Regex;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::schema::{
    Amenity, DataSource, DietaryOption, ElectronicsPolicy, EnrichmentMeta, Facility, FoodSituation,
    StaffLicense,
};
use crate::updater::cache::{today_utc_iso, CacheMode, DiskCache};

const WEB_TTL: Duration = Duration::from_secs(14 * 24 * 60 * 60);
const MAX_HTML_BYTES: usize = 512 * 1024;
const MAX_SUBPAGES: usize = 4;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CachedWebPage {
    pub url: String,
    pub status: u16,
    pub text: String,
    pub candidate_links: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WebsiteSignals {
    pub amenities: Vec<Amenity>,
    pub food: Option<FoodSituation>,
    pub dietary: Vec<DietaryOption>,
    pub electronics: Option<ElectronicsPolicy>,
    pub staff_licenses: Vec<StaffLicense>,
    pub evidence_urls: Vec<String>,
}

impl WebsiteSignals {
    pub fn is_empty(&self) -> bool {
        self.amenities.is_empty()
            && self.food.is_none()
            && self.dietary.is_empty()
            && self.electronics.is_none()
            && self.staff_licenses.is_empty()
    }
}

/// Fetches (or reads from disk cache) a facility's homepage + up to 4 high-signal subpages,
/// returning `(cache_hits, net_fetches, pages)`.
pub async fn fetch_website_pages(
    client: &reqwest::Client,
    cache: &DiskCache,
    home_url: &str,
) -> (usize, usize, Vec<CachedWebPage>) {
    let mut cache_hits = 0usize;
    let mut net_fetches = 0usize;
    let mut pages = Vec::new();

    match fetch_page_cached(client, cache, home_url).await {
        PageFetch::Hit(page) => {
            cache_hits += 1;
            pages.push(page);
        }
        PageFetch::Fetched(page) => {
            net_fetches += 1;
            pages.push(page);
        }
        PageFetch::Skipped | PageFetch::Failed => {
            return (cache_hits, net_fetches, pages);
        }
    }

    let subpage_urls: Vec<String> = pages[0]
        .candidate_links
        .iter()
        .take(MAX_SUBPAGES)
        .cloned()
        .collect();

    for url in subpage_urls {
        match fetch_page_cached(client, cache, &url).await {
            PageFetch::Hit(page) => {
                cache_hits += 1;
                pages.push(page);
            }
            PageFetch::Fetched(page) => {
                net_fetches += 1;
                pages.push(page);
            }
            PageFetch::Skipped | PageFetch::Failed => {}
        }
    }

    (cache_hits, net_fetches, pages)
}

/// Fetches and extracts regex signals for a single website URL once (homepage + up to 4 high-signal
/// subpages), returning `(cache_hits, net_fetches, signals)`.
pub async fn fetch_website_signals(
    client: &reqwest::Client,
    cache: &DiskCache,
    home_url: &str,
) -> (usize, usize, WebsiteSignals) {
    let (cache_hits, net_fetches, pages) = fetch_website_pages(client, cache, home_url).await;
    let signals = extract_signals_from_pages(&pages);
    (cache_hits, net_fetches, signals)
}

/// Applies extracted [`WebsiteSignals`] onto `facility`, returning `true` if any signal was added.
pub fn apply_website_signals(facility: &mut Facility, signals: &WebsiteSignals) -> bool {
    if signals.is_empty() {
        return false;
    }

    for &a in &signals.amenities {
        if !facility.lifestyle.amenities.contains(&a) {
            facility.lifestyle.amenities.push(a);
        }
    }
    if facility.lifestyle.food.is_none() {
        facility.lifestyle.food = signals.food;
    }
    for &d in &signals.dietary {
        if !facility.lifestyle.dietary.contains(&d) {
            facility.lifestyle.dietary.push(d);
        }
    }
    if facility.lifestyle.electronics.is_none() {
        facility.lifestyle.electronics = signals.electronics;
    }
    facility.lifestyle.normalize();

    for &lic in &signals.staff_licenses {
        if !facility.staff_licenses.contains(&lic) {
            facility.staff_licenses.push(lic);
        }
    }
    facility.staff_licenses.sort_unstable();
    facility.staff_licenses.dedup();

    let meta = facility
        .enrichment
        .get_or_insert_with(EnrichmentMeta::default);
    meta.add_source(DataSource::Website);
    meta.website_checked = Some(today_utc_iso());
    for url in &signals.evidence_urls {
        meta.add_evidence_url(url.clone());
    }
    true
}

/// Fetches and extracts signals for a single facility, returning `(cache_hits, net_fetches, enriched)`.
pub async fn enrich_facility_from_website(
    client: &reqwest::Client,
    cache: &DiskCache,
    facility: &mut Facility,
) -> (usize, usize, bool) {
    let Some(home_url) = facility.safe_website().map(str::to_string) else {
        return (0, 0, false);
    };
    let (cache_hits, net_fetches, signals) = fetch_website_signals(client, cache, &home_url).await;
    let applied = apply_website_signals(facility, &signals);
    (cache_hits, net_fetches, applied)
}

enum PageFetch {
    Hit(CachedWebPage),
    Fetched(CachedWebPage),
    Skipped,
    Failed,
}

async fn fetch_page_cached(client: &reqwest::Client, cache: &DiskCache, url: &str) -> PageFetch {
    if let Some(hit) = cache.get::<CachedWebPage>("web", url, WEB_TTL) {
        return if hit.value.status >= 200 && hit.value.status < 400 && !hit.value.text.is_empty() {
            PageFetch::Hit(hit.value)
        } else {
            PageFetch::Skipped
        };
    }
    if cache.mode() == CacheMode::Offline {
        return PageFetch::Skipped;
    }

    let stale = cache.get_stale::<CachedWebPage>("web", url);
    let mut req = client
        .get(url)
        .timeout(Duration::from_secs(10))
        .header("Accept", "text/html,application/xhtml+xml;q=0.9,*/*;q=0.5");

    if let Some(prev) = &stale {
        if let Some(etag) = &prev.etag {
            req = req.header("If-None-Match", etag);
        }
        if let Some(lm) = &prev.last_modified {
            req = req.header("If-Modified-Since", lm);
        }
    }

    let resp = match req.send().await {
        Ok(r) => r,
        Err(_) => {
            // Cache negative fetch briefly as status 0? No, fall back to stale if available.
            if let Some(prev) = stale {
                return PageFetch::Hit(prev.value);
            }
            let failed_stub = CachedWebPage {
                url: url.to_string(),
                status: 0,
                text: String::new(),
                candidate_links: Vec::new(),
            };
            let _ = cache.put("web", url, &failed_stub, None, None);
            return PageFetch::Failed;
        }
    };

    let status = resp.status().as_u16();
    if status == 304 {
        if let Some(prev) = stale {
            let _ = cache.put("web", url, &prev.value, prev.etag, prev.last_modified);
            return PageFetch::Hit(prev.value);
        }
    }

    let etag = resp
        .headers()
        .get("etag")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let last_modified = resp
        .headers()
        .get("last-modified")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let final_url = resp.url().clone();
    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();

    if !resp.status().is_success() || (!content_type.is_empty() && !content_type.contains("html")) {
        let stub = CachedWebPage {
            url: final_url.to_string(),
            status,
            text: String::new(),
            candidate_links: Vec::new(),
        };
        let _ = cache.put("web", url, &stub, etag, last_modified);
        return PageFetch::Skipped;
    }

    let bytes = match resp.bytes().await {
        Ok(b) => b,
        Err(_) => return PageFetch::Failed,
    };
    let slice = &bytes[..bytes.len().min(MAX_HTML_BYTES)];
    let html = String::from_utf8_lossy(slice);
    let text = html_to_visible_text(&html);
    let candidate_links = extract_high_signal_links(&html, &final_url);

    let page = CachedWebPage {
        url: final_url.to_string(),
        status,
        text,
        candidate_links,
    };
    let _ = cache.put("web", url, &page, etag, last_modified);
    PageFetch::Fetched(page)
}

/// Extracts all structured non-care amenities, food, electronic device policy, and strict staff
/// credentials from a collection of fetched facility pages.
pub fn extract_signals_from_pages(pages: &[CachedWebPage]) -> WebsiteSignals {
    let mut amenities = BTreeSet::new();
    let mut dietary = BTreeSet::new();
    let mut staff = BTreeSet::new();
    let mut food: Option<FoodSituation> = None;
    let mut electronics: Option<ElectronicsPolicy> = None;
    let mut evidence_urls = Vec::new();

    for page in pages {
        if page.text.is_empty() {
            continue;
        }
        let page_sig = extract_signals_from_text(&page.text);
        if !page_sig.is_empty() && !evidence_urls.contains(&page.url) {
            evidence_urls.push(page.url.clone());
        }
        amenities.extend(page_sig.amenities);
        dietary.extend(page_sig.dietary);
        staff.extend(page_sig.staff_licenses);

        // Prefer more specific food situation (ChefPrepared > DietitianCatered > Cafeteria > SelfCatered).
        if let Some(f) = page_sig.food {
            food = Some(match food {
                Some(prev) => prev.min(f),
                None => f,
            });
        }
        // Prefer explicit electronics policies (Prohibited / Scheduled / PhonesOnly / WorkApproved / Unrestricted).
        if let Some(e) = page_sig.electronics {
            if electronics.is_none() {
                electronics = Some(e);
            }
        }
    }

    WebsiteSignals {
        amenities: amenities.into_iter().collect(),
        food,
        dietary: dietary.into_iter().collect(),
        electronics,
        staff_licenses: staff.into_iter().collect(),
        evidence_urls,
    }
}

/// Pure text extractor (unit-tested directly without network).
pub fn extract_signals_from_text(text: &str) -> WebsiteSignals {
    let lower = text.to_ascii_lowercase();
    let mut amenities = BTreeSet::new();
    let mut dietary = BTreeSet::new();

    // Water & wellness amenities
    if re_pool().is_match(&lower) {
        amenities.insert(Amenity::Pool);
    }
    if re_hot_tub().is_match(&lower) {
        amenities.insert(Amenity::HotTub);
    }
    if re_sauna().is_match(&lower) {
        amenities.insert(Amenity::Sauna);
    }
    if re_gym().is_match(&lower) {
        amenities.insert(Amenity::Gym);
    }
    if re_yoga().is_match(&lower) {
        amenities.insert(Amenity::YogaStudio);
    }
    if re_spa().is_match(&lower) {
        amenities.insert(Amenity::SpaMassage);
    }

    // Rooms & living
    if re_private_rooms().is_match(&lower) {
        amenities.insert(Amenity::PrivateRooms);
    }
    if re_shared_rooms().is_match(&lower) {
        amenities.insert(Amenity::SharedRooms);
    }
    if re_pet_friendly().is_match(&lower) && !re_no_pets().is_match(&lower) {
        amenities.insert(Amenity::PetFriendly);
    }
    if re_laundry().is_match(&lower) {
        amenities.insert(Amenity::Laundry);
    }
    if re_wifi().is_match(&lower) {
        amenities.insert(Amenity::Wifi);
    }
    if re_workspace().is_match(&lower) {
        amenities.insert(Amenity::Workspace);
    }

    // Campus & recreation
    if re_outdoor_grounds().is_match(&lower) {
        amenities.insert(Amenity::OutdoorGrounds);
    }
    if re_rec_courts().is_match(&lower) {
        amenities.insert(Amenity::RecreationCourts);
    }
    if re_equine().is_match(&lower) {
        amenities.insert(Amenity::EquineTherapy);
    }
    if re_art_music().is_match(&lower) {
        amenities.insert(Amenity::ArtMusicStudio);
    }
    if re_lounge().is_match(&lower) {
        amenities.insert(Amenity::LoungeGameRoom);
    }
    if re_family_visitation().is_match(&lower) {
        amenities.insert(Amenity::FamilyVisitation);
    }
    if re_offsite_outings().is_match(&lower) {
        amenities.insert(Amenity::OffsiteOutings);
    }

    // Food situation
    let food = if re_chef_prepared().is_match(&lower) {
        Some(FoodSituation::ChefPrepared)
    } else if re_dietitian_catered().is_match(&lower) {
        Some(FoodSituation::DietitianCatered)
    } else if re_cafeteria().is_match(&lower) {
        Some(FoodSituation::Cafeteria)
    } else if re_self_catered().is_match(&lower) {
        Some(FoodSituation::SelfCatered)
    } else {
        None
    };

    // Dietary options
    if re_vegan_veg().is_match(&lower) {
        dietary.insert(DietaryOption::VeganVegetarian);
    }
    if re_gluten_free().is_match(&lower) {
        dietary.insert(DietaryOption::GlutenFree);
    }
    if re_kosher().is_match(&lower) {
        dietary.insert(DietaryOption::Kosher);
    }
    if re_halal().is_match(&lower) {
        dietary.insert(DietaryOption::Halal);
    }
    if re_allergy().is_match(&lower) {
        dietary.insert(DietaryOption::AllergyAccommodated);
    }
    if re_snacks().is_match(&lower) {
        dietary.insert(DietaryOption::SnacksCoffeeBar);
    }

    // Electronic device policy (order matters: check explicit restrictions/policies before unrestricted!)
    let electronics = if re_elec_prohibited().is_match(&lower) {
        Some(ElectronicsPolicy::Prohibited)
    } else if re_elec_phones_only().is_match(&lower) {
        Some(ElectronicsPolicy::PhonesOnly)
    } else if re_elec_work_approved().is_match(&lower) {
        Some(ElectronicsPolicy::WorkApproved)
    } else if re_elec_scheduled().is_match(&lower) {
        Some(ElectronicsPolicy::Scheduled)
    } else if re_elec_unrestricted().is_match(&lower) {
        Some(ElectronicsPolicy::Unrestricted)
    } else {
        None
    };

    // Staff licenses (case-sensitive suffix matching on original text to prevent "do" / "MD" state false positives!)
    let staff_licenses = extract_staff_credentials_from_text(text);

    WebsiteSignals {
        amenities: amenities.into_iter().collect(),
        food,
        dietary: dietary.into_iter().collect(),
        electronics,
        staff_licenses,
        evidence_urls: Vec::new(),
    }
}

/// Extracts strict clinical staff license types from bio/team text without conflating MD vs DO
/// or Psychiatrist vs Nurse Practitioner / PA.
pub fn extract_staff_credentials_from_text(text: &str) -> Vec<StaffLicense> {
    let mut found = BTreeSet::new();

    // Check MD credential suffixes (e.g. "Jane Smith, M.D." or "Dr. Jane Smith, MD")
    // while ignoring Maryland postal addresses (", MD 21201") and non-clinical mentions.
    for m in re_cred_md().find_iter(text) {
        if looks_like_zip_after(text, m.end()) {
            continue;
        }
        let matched = m.as_str();
        let broad = context_window(text, m.start(), m.end(), 140).to_ascii_lowercase();
        let is_dotted = matched.contains("M.D.");
        if !is_dotted && !has_clinical_context(&broad) {
            continue;
        }
        match classify_physician_specialty(text, m.start(), m.end()) {
            PhysicianSpecialty::Psychiatry => found.insert(StaffLicense::MdPsychiatrist),
            PhysicianSpecialty::Addiction => found.insert(StaffLicense::MdAddiction),
            PhysicianSpecialty::General => found.insert(StaffLicense::MdPhysician),
        };
    }

    // Check DO credential suffixes (e.g. "John Doe, D.O." or "John Doe, DO", strictly separate from MD!)
    for m in re_cred_do().find_iter(text) {
        let matched = m.as_str();
        let broad = context_window(text, m.start(), m.end(), 140).to_ascii_lowercase();
        let is_dotted = matched.contains("D.O.");
        if !is_dotted && !has_clinical_context(&broad) {
            continue;
        }
        match classify_physician_specialty(text, m.start(), m.end()) {
            PhysicianSpecialty::Psychiatry => found.insert(StaffLicense::DoPsychiatrist),
            PhysicianSpecialty::Addiction => found.insert(StaffLicense::DoAddiction),
            PhysicianSpecialty::General => found.insert(StaffLicense::DoPhysician),
        };
    }

    if re_cred_pmhnp().is_match(text) {
        found.insert(StaffLicense::Pmhnp);
    }
    if re_cred_np().is_match(text) {
        found.insert(StaffLicense::Np);
    }
    if re_cred_pa().is_match(text) {
        found.insert(StaffLicense::Pa);
    }
    if re_cred_phd_psych().is_match(text) {
        found.insert(StaffLicense::PhdPsychologist);
    }
    if re_cred_psyd().is_match(text) {
        found.insert(StaffLicense::PsydPsychologist);
    }
    if re_cred_lcsw().is_match(text) {
        found.insert(StaffLicense::Lcsw);
    }
    if re_cred_lpc().is_match(text) {
        found.insert(StaffLicense::LpcLmhc);
    }
    if re_cred_lmft().is_match(text) {
        found.insert(StaffLicense::Lmft);
    }
    if re_cred_ladc().is_match(text) {
        found.insert(StaffLicense::AddictionCounselor);
    }
    if re_cred_rn().is_match(text) {
        found.insert(StaffLicense::Rn);
    }
    if re_cred_lpn().is_match(text) {
        found.insert(StaffLicense::Lpn);
    }
    if re_cred_peer().is_match(text) {
        found.insert(StaffLicense::PeerSupport);
    }

    found.into_iter().collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PhysicianSpecialty {
    Psychiatry,
    Addiction,
    General,
}

/// Classifies a matched MD/DO credential by looking at its own local bio phrase (preferring text
/// immediately after the credential, then immediately before it, and picking the closest specialty
/// keyword so neighboring staff bios do not bleed across clinicians).
fn classify_physician_specialty(text: &str, start: usize, end: usize) -> PhysicianSpecialty {
    let mut after_hi = (end + 80).min(text.len());
    while after_hi < text.len() && !text.is_char_boundary(after_hi) {
        after_hi += 1;
    }
    let after_raw = &text[end..after_hi];
    // Trim at the end of the current sentence if present.
    let after = after_raw
        .split(". ")
        .next()
        .unwrap_or(after_raw)
        .to_ascii_lowercase();

    let after_psych = after.find("psychiatr");
    let after_addict = after
        .find("addiction medicine")
        .or_else(|| after.find("addiction specialist"));

    match (after_psych, after_addict) {
        (Some(p), Some(a)) => {
            if p <= a {
                return PhysicianSpecialty::Psychiatry;
            }
            return PhysicianSpecialty::Addiction;
        }
        (Some(_), None) => return PhysicianSpecialty::Psychiatry,
        (None, Some(_)) => return PhysicianSpecialty::Addiction,
        (None, None) => {}
    }

    // Fallback: look immediately before the name (e.g. "Staff Psychiatrist Dr. Jane Smith, MD")
    let mut before_lo = start.saturating_sub(50);
    while before_lo > 0 && !text.is_char_boundary(before_lo) {
        before_lo -= 1;
    }
    let before = text[before_lo..start]
        .rsplit(". ")
        .next()
        .unwrap_or(&text[before_lo..start])
        .to_ascii_lowercase();

    let before_psych = before.rfind("psychiatr");
    let before_addict = before
        .rfind("addiction medicine")
        .or_else(|| before.rfind("addiction specialist"));

    match (before_psych, before_addict) {
        (Some(p), Some(a)) => {
            if p >= a {
                PhysicianSpecialty::Psychiatry
            } else {
                PhysicianSpecialty::Addiction
            }
        }
        (Some(_), None) => PhysicianSpecialty::Psychiatry,
        (None, Some(_)) => PhysicianSpecialty::Addiction,
        (None, None) => PhysicianSpecialty::General,
    }
}

fn looks_like_zip_after(text: &str, end: usize) -> bool {
    let rest = text[end..].trim_start_matches([' ', '\t', ',']);
    rest.len() >= 5 && rest.as_bytes()[..5].iter().all(u8::is_ascii_digit)
}

fn has_clinical_context(lower_window: &str) -> bool {
    [
        "dr.",
        "doctor",
        "physician",
        "psychiatr",
        "medical director",
        "chief medical",
        "board-certified",
        "board certified",
        "residency",
        "fellowship",
        "addiction medicine",
        "clinician",
        "osteopathic",
    ]
    .iter()
    .any(|kw| lower_window.contains(kw))
}

fn context_window(text: &str, start: usize, end: usize, radius: usize) -> &str {
    let mut lo = start.saturating_sub(radius);
    while lo > 0 && !text.is_char_boundary(lo) {
        lo -= 1;
    }
    let mut hi = (end + radius).min(text.len());
    while hi < text.len() && !text.is_char_boundary(hi) {
        hi += 1;
    }
    &text[lo..hi]
}

/// Strips `<script>`, `<style>`, `<noscript>`, and HTML tags to compact visible text.
pub fn html_to_visible_text(html: &str) -> String {
    let without_scripts = re_strip_blocks().replace_all(html, " ");
    let without_tags = re_strip_tags().replace_all(&without_scripts, " ");
    let decoded = without_tags
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">");
    decoded.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Finds up to `MAX_SUBPAGES * 2` high-signal same-origin links (amenities, dining, FAQ, staff/team).
pub fn extract_high_signal_links(html: &str, base: &Url) -> Vec<String> {
    let Some(base_host) = base.host_str() else {
        return Vec::new();
    };
    let mut scored: Vec<(u8, String)> = Vec::new();
    let mut seen = BTreeSet::new();
    seen.insert(base.as_str().trim_end_matches('/').to_string());

    for cap in re_href().captures_iter(html) {
        let Some(raw_href) = cap.get(1).map(|m| m.as_str().trim()) else {
            continue;
        };
        let href = raw_href.replace("&amp;", "&");
        if href.is_empty()
            || href.starts_with('#')
            || href.starts_with("mailto:")
            || href.starts_with("tel:")
            || href.starts_with("javascript:")
        {
            continue;
        }
        let Ok(mut joined) = base.join(&href) else {
            continue;
        };
        joined.set_fragment(None);
        if joined.host_str() != Some(base_host) {
            continue;
        }
        let path_lower = joined.path().to_ascii_lowercase();
        if path_lower.ends_with(".pdf")
            || path_lower.ends_with(".jpg")
            || path_lower.ends_with(".png")
            || path_lower.ends_with(".svg")
            || path_lower.ends_with(".css")
            || path_lower.ends_with(".js")
        {
            continue;
        }
        let priority = if path_lower.contains("amenit")
            || path_lower.contains("accommodat")
            || path_lower.contains("what-to-bring")
            || path_lower.contains("packing")
            || path_lower.contains("dining")
            || path_lower.contains("campus")
            || path_lower.contains("facility")
            || path_lower.contains("facilities")
            || path_lower.contains("housing")
        {
            3
        } else if path_lower.contains("staff")
            || path_lower.contains("team")
            || path_lower.contains("provider")
            || path_lower.contains("clinician")
            || path_lower.contains("leadership")
            || path_lower.contains("doctor")
            || path_lower.contains("psychiatr")
        {
            2
        } else if path_lower.contains("faq")
            || path_lower.contains("admission")
            || path_lower.contains("about")
            || path_lower.contains("expect")
        {
            1
        } else {
            0
        };
        if priority == 0 {
            continue;
        }
        let normalized = joined.as_str().trim_end_matches('/').to_string();
        if seen.insert(normalized) {
            scored.push((priority, joined.to_string()));
        }
    }

    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    scored
        .into_iter()
        .take(MAX_SUBPAGES * 2)
        .map(|(_, u)| u)
        .collect()
}

macro_rules! cached_regex {
    ($fn_name:ident, $pattern:literal) => {
        fn $fn_name() -> &'static Regex {
            static RE: OnceLock<Regex> = OnceLock::new();
            RE.get_or_init(|| Regex::new($pattern).expect("valid static regex"))
        }
    };
}

cached_regex!(
    re_strip_blocks,
    r"(?is)<(script|style|noscript|svg|head)\b[^>]*>.*?</(script|style|noscript|svg|head)>"
);
cached_regex!(re_strip_tags, r"(?s)<[^>]+>");
cached_regex!(re_href, r#"(?i)\bhref\s*=\s*["']([^"']+)["']"#);

// Amenities
cached_regex!(
    re_pool,
    r"\b(swimming pool|heated pool|outdoor pool|indoor pool|lap pool|poolside|resort-style pool|saltwater pool|resort pool|plunge pool|pool area)\b"
);
cached_regex!(
    re_hot_tub,
    r"\b(hot tubs?|hot-tubs?|jacuzzis?|whirlpool tub|hydrotherapy tub|spa tub|cold plunge)\b"
);
cached_regex!(re_sauna, r"\b(sauna|infrared sauna|steam room)\b");
cached_regex!(
    re_gym,
    r"\b(fitness center|on-site gym|onsite gym|weight room|cardio room|workout room|fitness room|exercise room|gymnasium)\b"
);
cached_regex!(
    re_yoga,
    r"\b(yoga studio|yoga classes|meditation room|meditation studio|mindfulness studio)\b"
);
cached_regex!(
    re_spa,
    r"\b(massage therapy|on-site spa|spa services|acupuncture and massage)\b"
);
cached_regex!(
    re_private_rooms,
    r"\b(private rooms?|private bedrooms?|private suites?|single-occupancy rooms?)\b"
);
cached_regex!(
    re_shared_rooms,
    r"\b(semi-private rooms?|shared rooms?|shared bedrooms?|double-occupancy rooms?)\b"
);
cached_regex!(
    re_pet_friendly,
    r"\b(pet-friendly|pet friendly|bring your pet|pets are welcome|pets allowed|dog-friendly)\b"
);
cached_regex!(
    re_no_pets,
    r"\b(no pets|pets are not allowed|pets are not permitted|cannot accommodate pets)\b"
);
cached_regex!(
    re_laundry,
    r"\b(on-site laundry|onsite laundry|laundry facilities|washer and dryer|washers and dryers|laundry room)\b"
);
cached_regex!(
    re_wifi,
    r"\b(wi-fi|wifi|high-speed internet|wireless internet)\b"
);
cached_regex!(
    re_workspace,
    r"\b(executive program|private workspace|business center|workstations?|work from rehab|dedicated desk)\b"
);
cached_regex!(
    re_outdoor_grounds,
    r"\b(hiking trails?|walking trails?|nature trails?|courtyard|outdoor grounds|scenic campus|mountain views|garden grounds)\b"
);
cached_regex!(
    re_rec_courts,
    r"\b(basketball court|volleyball court|tennis court|pickleball court|sports court|recreation court)\b"
);
cached_regex!(
    re_equine,
    r"\b(equine therapy|equine-assisted|horseback riding|animal-assisted therapy)\b"
);
cached_regex!(
    re_art_music,
    r"\b(art studio|music room|music studio|recording studio|creative arts studio)\b"
);
cached_regex!(
    re_lounge,
    r"\b(game room|recreation room|billiards|pool table|ping pong|community lounge|tv lounge)\b"
);
cached_regex!(
    re_family_visitation,
    r"\b(family visitation|visiting hours|family weekend|family visiting)\b"
);
cached_regex!(
    re_offsite_outings,
    r"\b(off-site outings|offsite outings|weekend outings|recreational outings|adventure outings|day passes)\b"
);

// Food & dietary
cached_regex!(
    re_chef_prepared,
    r"\b(private chef|executive chef|gourmet meals?|chef-prepared|chef prepared|chef-crafted|in-house chef|culinary team|restaurant-style dining|freshly prepared meals?)\b"
);
cached_regex!(
    re_dietitian_catered,
    r"\b(registered dietitian|dietitian-planned|nutritionist-planned|nutritious meals prepared|three nutritious meals|daily nutritious meals|chef and dietitian|catered meals?)\b"
);
cached_regex!(
    re_cafeteria,
    r"\b(cafeteria|dining hall|communal dining room|three meals a day)\b"
);
cached_regex!(
    re_self_catered,
    r"\b(resident kitchen|communal kitchen|prepare your own meals|cook your own meals|self-cater)\b"
);
cached_regex!(re_vegan_veg, r"\b(vegan|vegetarian|plant-based)\b");
cached_regex!(re_gluten_free, r"\b(gluten-free|gluten free|celiac)\b");
cached_regex!(re_kosher, r"\b(kosher)\b");
cached_regex!(re_halal, r"\b(halal)\b");
cached_regex!(
    re_allergy,
    r"\b(food allergies|dietary restrictions|dietary accommodations|special dietary needs)\b"
);
cached_regex!(
    re_snacks,
    r"\b(snacks? available|coffee bar|espresso bar|stocked kitchen|24/7 snacks|healthy snacks)\b"
);

// Electronic device policies
cached_regex!(
    re_elec_prohibited,
    r"\b(no cell phones?|no personal cell phones?|cell phones? are (not allowed|not permitted|prohibited|collected|locked|stored)|phones? and laptops? are not (allowed|permitted)|electronics are (not allowed|not permitted|prohibited)|no personal electronics|phone-free environment|device-free|digital detox|electronics? prohibited)\b"
);
cached_regex!(
    re_elec_phones_only,
    r"\b(cell phones? (are )?(allowed|permitted)[^.]{0,60}(no laptops|without laptops|laptops are not)|laptops? are not (allowed|permitted)|phones only)\b"
);
cached_regex!(
    re_elec_work_approved,
    r"\b(laptops? and (cell )?phones? (for|to) (continue )?work|executive rehab[^.]{0,80}(laptop|phone)|stay connected (to|with) work|work-approved (devices?|laptops?)|laptops? (are )?(allowed|permitted) for work|remote work)\b"
);
cached_regex!(
    re_elec_scheduled,
    r"\b((cell )?phones? (are )?(allowed|permitted) (during|after|on|at) (designated|scheduled|certain|specific|evening|weekend)|blackout period|limited (cell )?phone (access|use|time)|designated phone (hours|times)|scheduled phone (hours|times|access)|phone privileges|phone use (is )?(limited|restricted))\b"
);
cached_regex!(
    re_elec_unrestricted,
    r"\b(keep your (cell )?phone|keep (your )?(phone|laptop)s? with you|unrestricted (phone|electronics|device)|phones? and laptops? (are )?(welcome|allowed|permitted)|bring your (cell )?phone|laptops? (are )?(allowed|permitted|welcome))\b"
);

// Strict case-sensitive staff credential suffixes (never match generic English "do" or state "MD")
cached_regex!(
    re_cred_md,
    r"(?:[A-Z][a-z]{1,24},\s*(?:M\.D\.|MD)|\bM\.D\.)(?:\b|[,\s;])"
);
cached_regex!(
    re_cred_do,
    r"(?:[A-Z][a-z]{1,24},\s*(?:D\.O\.|DO)|\bD\.O\.)(?:\b|[,\s;])"
);
cached_regex!(
    re_cred_pmhnp,
    r"\b(PMHNP(?:-BC)?|Psychiatric[- ]Mental Health Nurse Practitioner|Psychiatric Nurse Practitioner)\b"
);
cached_regex!(
    re_cred_np,
    r"(?:,\s*|\b)(?:FNP(?:-BC|-C)?|AGNP(?:-BC|-C)?|APRN|ARNP|CRNP|Family Nurse Practitioner|Adult[- ]Gerontology Nurse Practitioner|Primary Care Nurse Practitioner)\b|[A-Z][a-z]{1,24},\s*NP(?:-C|-BC)?\b"
);
cached_regex!(re_cred_pa, r"(?:,\s*|\b)(?:PA-C|Physician Assistant)\b");
cached_regex!(
    re_cred_phd_psych,
    r"(?i)(?:Ph\.?D\.?[^.\n]{0,40}psychologist|psychologist[^.\n]{0,40}Ph\.?D\.?)"
);
cached_regex!(re_cred_psyd, r"\bPsy\.?D\.?\b");
cached_regex!(re_cred_lcsw, r"\b(LCSW(?:-C|-R)?|LICSW|LISW(?:-CP|-S)?)\b");
cached_regex!(re_cred_lpc, r"\b(LPC(?:-S|-MHSP)?|LPCC|LMHC|LCPC|LCMHC)\b");
cached_regex!(re_cred_lmft, r"\bLMFT\b");
cached_regex!(
    re_cred_ladc,
    r"\b(LADC|CADC|LCDC|CASAC|LAC|CAC\s*(?:II|III))\b"
);
cached_regex!(
    re_cred_rn,
    r"(?:,\s*|\b)(?:RN(?:-BC)?|BSN,\s*RN|Registered Nurse)\b"
);
cached_regex!(re_cred_lpn, r"\b(LPN|LVN|Licensed Practical Nurse)\b");
cached_regex!(
    re_cred_peer,
    r"\b(Certified Peer Support|Peer Support Specialist|CPRS|CPSS)\b"
);
