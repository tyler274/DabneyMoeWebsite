//! Conservative matcher that joins normalized state facility and provider licensing records
//! onto an existing [`StateCatalog`].
//!
//! Matching invariants:
//! - Facilities match only by:
//!   1. Exact normalized street number + primary street word + 5-digit ZIP, OR
//!   2. Exact normalized 10-digit phone + same state, OR
//!   3. High token overlap on normalized facility name (or DBA) + same 5-digit ZIP (or same
//!      city when the name has at least 2 meaningful tokens), only when no exact location/phone
//!      match exists.
//! - Providers match only by normalized person name (stripping titles and credential suffixes
//!   and handling both `"First Last"` and `"Last, First"`) AND strictly compatible clinical
//!   role (never attaching a DO license to an MD provider, an NP/PA license to a psychiatrist,
//!   or a PsyD license to a PhD psychologist).

use std::collections::BTreeSet;

use crate::schema::{
    DataSource, EnrichmentMeta, Facility, NetworkProvider, ProviderRole, StaffLicense,
    StateCatalog, StateFacilityLicense, StateProviderLicense,
};
use crate::updater::nppes::{normalize_phone10, normalize_street_key};

use super::types::{FacilityLicenseRecord, StateAdapterOutcome};

/// Counts of matched entities and newly attached state licenses for a single merge pass.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MatchSummary {
    pub facilities_matched: usize,
    pub facility_licenses_attached: usize,
    pub providers_matched: usize,
    pub provider_licenses_attached: usize,
}

/// Applies normalized facility and provider state licensing records to `catalog` in place.
pub fn apply_state_licensing_to_catalog(
    catalog: &mut StateCatalog,
    outcome: &StateAdapterOutcome,
    retrieved_date: &str,
) -> MatchSummary {
    let mut summary = MatchSummary::default();
    let retrieved_at = {
        let trimmed = retrieved_date.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    };

    // Precompute normalized facility keys once for fast, deterministic matching.
    let fac_keys: Vec<FacilityMatchKey> = catalog
        .facilities
        .iter()
        .map(FacilityMatchKey::from_facility)
        .collect();

    let mut matched_facility_indices: BTreeSet<usize> = BTreeSet::new();

    for rec in &outcome.facility_records {
        let rec_key = RecordFacilityKey::from_record(rec);
        let target_indices = find_matching_facilities(&fac_keys, &rec_key);
        if target_indices.is_empty() {
            continue;
        }

        let license_entry = StateFacilityLicense {
            state: rec.state.trim().to_ascii_uppercase(),
            agency: rec.agency.trim().to_string(),
            license_number: rec.license_number.trim().to_string(),
            license_type: rec.license_type.trim().to_string(),
            status: rec.status,
            licensed_beds: rec.licensed_beds,
            expires_on: rec.expires_on.clone(),
            disciplinary_actions: rec.disciplinary_actions,
            inspection_count: rec.inspection_count,
            verification_url: rec.verification_url.clone(),
            retrieved_at: retrieved_at.clone(),
        };

        for idx in target_indices {
            let fac = &mut catalog.facilities[idx];
            if attach_facility_license(fac, license_entry.clone()) {
                summary.facility_licenses_attached += 1;
            }
            let meta = fac.enrichment.get_or_insert_with(EnrichmentMeta::default);
            meta.add_source(DataSource::StateLicense);
            if let Some(url) = license_entry
                .verification_url
                .as_deref()
                .map(str::trim)
                .filter(|u| u.starts_with("https://") || u.starts_with("http://"))
            {
                meta.add_evidence_url(url.to_string());
            }
            matched_facility_indices.insert(idx);
        }
    }
    summary.facilities_matched = matched_facility_indices.len();

    // Precompute normalized provider names once.
    let prov_names: Vec<Option<NormalizedPersonName>> = catalog
        .staff_network
        .iter()
        .map(|p| normalize_person_name(&p.name))
        .collect();

    let mut matched_provider_indices: BTreeSet<usize> = BTreeSet::new();

    for rec in &outcome.provider_records {
        let Some(rec_name) = normalize_person_name(&rec.full_name) else {
            continue;
        };
        let Some(rec_lic) = rec.license else {
            continue;
        };

        let license_entry = StateProviderLicense {
            state: rec.state.trim().to_ascii_uppercase(),
            board: rec.board.trim().to_string(),
            license_number: rec.license_number.trim().to_string(),
            credential_label: rec.credential_label.trim().to_string(),
            license: Some(rec_lic),
            status: rec.status,
            expires_on: rec.expires_on.clone(),
            disciplinary_flag: rec.disciplinary_flag,
            verification_url: rec.verification_url.clone(),
        };

        for (idx, provider) in catalog.staff_network.iter_mut().enumerate() {
            let Some(ref prov_name) = prov_names[idx] else {
                continue;
            };
            if !prov_name.matches(&rec_name) {
                continue;
            }
            if !is_role_compatible(provider.role, rec_lic) {
                continue;
            }

            if attach_provider_license(provider, license_entry.clone()) {
                summary.provider_licenses_attached += 1;
            }
            if !provider.sources.contains(&DataSource::StateLicense) {
                provider.sources.push(DataSource::StateLicense);
                provider.sources.sort_unstable();
            }
            matched_provider_indices.insert(idx);
        }
    }
    summary.providers_matched = matched_provider_indices.len();

    summary
}

// ---------------------------------------------------------------------------
// Facility matching helpers
// ---------------------------------------------------------------------------

struct FacilityMatchKey {
    state: String,
    zip5: Option<String>,
    city_norm: String,
    street: Option<(String, String)>,
    phone10: Option<String>,
    exact_names: Vec<String>,
    token_sets: Vec<BTreeSet<String>>,
}

impl FacilityMatchKey {
    fn from_facility(f: &Facility) -> Self {
        let mut exact_names = Vec::new();
        let mut token_sets = Vec::new();
        push_name_variants(&f.name, &mut exact_names, &mut token_sets);
        if let Some(site) = f.site_name.as_deref() {
            push_name_variants(site, &mut exact_names, &mut token_sets);
            let combined = format!("{} {}", f.name, site);
            push_name_variants(&combined, &mut exact_names, &mut token_sets);
        }
        Self {
            state: f.address.state.trim().to_ascii_uppercase(),
            zip5: normalize_zip5(&f.address.zip),
            city_norm: normalize_city(&f.address.city),
            street: f.address.street.as_deref().and_then(normalize_street_key),
            phone10: f.phone.as_deref().and_then(normalize_phone10),
            exact_names,
            token_sets,
        }
    }
}

struct RecordFacilityKey {
    state: String,
    zip5: Option<String>,
    city_norm: String,
    street: Option<(String, String)>,
    phone10: Option<String>,
    is_inactive: bool,
    exact_names: Vec<String>,
    token_sets: Vec<BTreeSet<String>>,
}

impl RecordFacilityKey {
    fn from_record(r: &FacilityLicenseRecord) -> Self {
        let mut exact_names = Vec::new();
        let mut token_sets = Vec::new();
        push_name_variants(&r.facility_name, &mut exact_names, &mut token_sets);
        if let Some(dba) = r.dba_name.as_deref() {
            push_name_variants(dba, &mut exact_names, &mut token_sets);
            let combined = format!("{} {}", r.facility_name, dba);
            push_name_variants(&combined, &mut exact_names, &mut token_sets);
        }
        Self {
            state: r.state.trim().to_ascii_uppercase(),
            zip5: r.zip.as_deref().and_then(normalize_zip5),
            city_norm: r.city.as_deref().map(normalize_city).unwrap_or_default(),
            street: r.street.as_deref().and_then(normalize_street_key),
            phone10: r.phone.as_deref().and_then(normalize_phone10),
            is_inactive: r.status == crate::schema::LicenseStatus::Inactive,
            exact_names,
            token_sets,
        }
    }
}

fn find_matching_facilities(fac_keys: &[FacilityMatchKey], rec: &RecordFacilityKey) -> Vec<usize> {
    // Rule 1 (highest priority): Exact normalized street number + street name + 5-digit ZIP.
    let mut street_matches = Vec::new();
    for (idx, fac) in fac_keys.iter().enumerate() {
        if !states_compatible(&fac.state, &rec.state) {
            continue;
        }
        let street_zip_match = fac.street.is_some()
            && fac.street == rec.street
            && fac.zip5.is_some()
            && fac.zip5 == rec.zip5;
        if street_zip_match {
            street_matches.push(idx);
        }
    }
    if !street_matches.is_empty() {
        return street_matches;
    }

    // Rule 2: Exact normalized 10-digit phone + same state, guarded against central corporate
    // intake numbers shared by multi-site health systems (do not cross-attach Site A's license
    // onto Site B in another city/ZIP just because both list the same intake phone).
    if let Some(rec_phone) = rec.phone10.as_deref() {
        let mut phone_matches = Vec::new();
        for (idx, fac) in fac_keys.iter().enumerate() {
            if !states_compatible(&fac.state, &rec.state) {
                continue;
            }
            if fac.phone10.as_deref() != Some(rec_phone) {
                continue;
            }
            // Reject when both sides have streets or ZIPs that explicitly disagree.
            let conflicting_zip = fac.zip5.is_some() && rec.zip5.is_some() && fac.zip5 != rec.zip5;
            let conflicting_street =
                fac.street.is_some() && rec.street.is_some() && fac.street != rec.street;
            if conflicting_zip || conflicting_street {
                continue;
            }
            phone_matches.push(idx);
        }
        // If 3+ facilities in the same ZIP still share one phone without a street match,
        // narrow by name similarity rather than broadcasting one site's license to all of them.
        if phone_matches.len() > 2 {
            let narrowed: Vec<usize> = phone_matches
                .iter()
                .copied()
                .filter(|&idx| names_match_conservatively(&fac_keys[idx], rec, true))
                .collect();
            if !narrowed.is_empty() {
                return narrowed;
            }
        }
        if !phone_matches.is_empty() && phone_matches.len() <= 2 {
            return phone_matches;
        }
    }

    // Rule 3: High token overlap on normalized facility name (or DBA) + same 5-digit ZIP
    // (or same city when name is distinctive with >= 2 meaningful tokens), rejecting records
    // whose street explicitly disagrees with the facility's street or whose status is Inactive.
    if rec.is_inactive {
        return Vec::new();
    }
    let mut name_matches = Vec::new();
    for (idx, fac) in fac_keys.iter().enumerate() {
        if !states_compatible(&fac.state, &rec.state) {
            continue;
        }
        let conflicting_street =
            fac.street.is_some() && rec.street.is_some() && fac.street != rec.street;
        if conflicting_street {
            continue;
        }
        let same_zip = fac.zip5.is_some() && fac.zip5 == rec.zip5;
        let same_city = !fac.city_norm.is_empty() && fac.city_norm == rec.city_norm;
        if !same_zip && !same_city {
            continue;
        }
        if names_match_conservatively(fac, rec, same_zip) {
            name_matches.push(idx);
        }
    }
    name_matches
}

fn states_compatible(fac_state: &str, rec_state: &str) -> bool {
    fac_state.is_empty() || rec_state.is_empty() || fac_state == rec_state
}

fn names_match_conservatively(
    fac: &FacilityMatchKey,
    rec: &RecordFacilityKey,
    same_zip: bool,
) -> bool {
    // Exact normalized name match (legal entity suffixes stripped).
    for f_exact in &fac.exact_names {
        for r_exact in &rec.exact_names {
            if f_exact == r_exact {
                let word_count = f_exact.split_whitespace().count();
                if same_zip || word_count >= 2 {
                    return true;
                }
            }
        }
    }

    // High meaningful-token overlap (requires >= 2 distinctive tokens on both sides).
    for f_set in &fac.token_sets {
        if f_set.len() < 2 {
            continue;
        }
        for r_set in &rec.token_sets {
            if r_set.len() < 2 {
                continue;
            }
            let overlap = f_set.intersection(r_set).count();
            if overlap < 2 {
                continue;
            }
            let union = f_set.len() + r_set.len() - overlap;
            let min_len = f_set.len().min(r_set.len());
            let max_len = f_set.len().max(r_set.len());

            if same_zip {
                // Jaccard >= 0.60, or full containment of a >=2-token name within <=2 extra tokens.
                if overlap * 5 >= union * 3 || (overlap == min_len && max_len <= min_len + 2) {
                    return true;
                }
            } else {
                // City-only fallback requires stricter overlap (Jaccard >= 0.67).
                if overlap * 3 >= union * 2 {
                    return true;
                }
            }
        }
    }

    false
}

fn push_name_variants(
    raw: &str,
    exact_names: &mut Vec<String>,
    token_sets: &mut Vec<BTreeSet<String>>,
) {
    let exact = normalize_facility_exact_name(raw);
    if !exact.is_empty() && !exact_names.contains(&exact) {
        exact_names.push(exact);
    }
    let tokens = meaningful_facility_tokens(raw);
    if !tokens.is_empty() && !token_sets.contains(&tokens) {
        token_sets.push(tokens);
    }
}

fn normalize_facility_exact_name(raw: &str) -> String {
    let words: Vec<String> = raw
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(str::to_ascii_lowercase)
        .filter(|w| {
            !matches!(
                w.as_str(),
                "llc"
                    | "inc"
                    | "corp"
                    | "corporation"
                    | "co"
                    | "ltd"
                    | "lp"
                    | "pllc"
                    | "pc"
                    | "dba"
            )
        })
        .collect();
    words.join(" ")
}

fn meaningful_facility_tokens(raw: &str) -> BTreeSet<String> {
    raw.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(str::to_ascii_lowercase)
        .filter(|w| w.len() >= 2 && !is_generic_facility_word(w))
        .collect()
}

fn is_generic_facility_word(word: &str) -> bool {
    matches!(
        word,
        "the"
            | "of"
            | "and"
            | "at"
            | "in"
            | "for"
            | "to"
            | "by"
            | "llc"
            | "inc"
            | "corp"
            | "corporation"
            | "co"
            | "ltd"
            | "lp"
            | "pllc"
            | "pc"
            | "dba"
            | "center"
            | "centers"
            | "clinic"
            | "clinics"
            | "health"
            | "healthcare"
            | "behavioral"
            | "mental"
            | "recovery"
            | "treatment"
            | "service"
            | "services"
            | "program"
            | "programs"
            | "facility"
            | "facilities"
            | "care"
            | "medical"
            | "group"
            | "associates"
            | "institute"
            | "foundation"
            | "hospital"
            | "system"
            | "systems"
            | "network"
            | "wellness"
            | "counseling"
            | "therapy"
            | "rehab"
            | "rehabilitation"
            | "residential"
            | "outpatient"
            | "inpatient"
            | "community"
            | "substance"
            | "addiction"
            | "psychiatry"
            | "psychiatric"
    )
}

fn normalize_zip5(raw: &str) -> Option<String> {
    let digits: String = raw.chars().filter(|c| c.is_ascii_digit()).take(5).collect();
    (digits.len() == 5).then_some(digits)
}

fn normalize_city(raw: &str) -> String {
    raw.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Attaches or updates a deduplicated [`StateFacilityLicense`]. Returns `true` if newly added.
fn attach_facility_license(fac: &mut Facility, entry: StateFacilityLicense) -> bool {
    if let Some(existing) = fac.state_licenses.iter_mut().find(|e| {
        e.state.eq_ignore_ascii_case(&entry.state)
            && e.agency.eq_ignore_ascii_case(&entry.agency)
            && e.license_number.eq_ignore_ascii_case(&entry.license_number)
    }) {
        *existing = entry;
        return false;
    }
    fac.state_licenses.push(entry);
    fac.state_licenses.sort_by(|a, b| {
        a.agency
            .cmp(&b.agency)
            .then_with(|| a.license_number.cmp(&b.license_number))
            .then_with(|| a.license_type.cmp(&b.license_type))
    });
    true
}

// ---------------------------------------------------------------------------
// Provider name normalization & strict role compatibility
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
struct NormalizedPersonName {
    first: String,
    middle_initial: Option<char>,
    last: String,
}

impl NormalizedPersonName {
    fn matches(&self, other: &Self) -> bool {
        if self.first != other.first || self.last != other.last {
            return false;
        }
        match (self.middle_initial, other.middle_initial) {
            (Some(a), Some(b)) => a == b,
            _ => true,
        }
    }
}

fn normalize_person_name(raw: &str) -> Option<NormalizedPersonName> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Handle comma-separated forms:
    // - "Jane A. Smith, MD, PhD" -> credentials after comma are stripped.
    // - "Smith, Jane A., MD" -> first segment is last name, second segment is first/middle.
    let segments: Vec<&str> = trimmed
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    let ordered_tokens: Vec<String> = if segments.len() >= 2 {
        let first_seg_tokens = clean_name_tokens(segments[0]);
        let second_seg_tokens = clean_name_tokens(segments[1]);
        let second_is_credentials = !second_seg_tokens.is_empty()
            && second_seg_tokens.iter().all(|t| is_credential_or_suffix(t));

        if !second_is_credentials && first_seg_tokens.len() <= 2 && !second_seg_tokens.is_empty() {
            // "LAST, FIRST [MIDDLE]" format.
            let mut combined = Vec::new();
            for tok in second_seg_tokens {
                if !is_title(&tok) && !is_credential_or_suffix(&tok) {
                    combined.push(tok);
                }
            }
            for tok in first_seg_tokens {
                if !is_title(&tok) && !is_credential_or_suffix(&tok) {
                    combined.push(tok);
                }
            }
            combined
        } else {
            // "FIRST [MIDDLE] LAST, CREDENTIALS..." format.
            first_seg_tokens
                .into_iter()
                .filter(|t| !is_title(t) && !is_credential_or_suffix(t))
                .collect()
        }
    } else {
        clean_name_tokens(trimmed)
            .into_iter()
            .filter(|t| !is_title(t) && !is_credential_or_suffix(t))
            .collect()
    };

    if ordered_tokens.len() < 2 {
        return None;
    }

    let first = ordered_tokens.first()?.clone();
    let last = ordered_tokens.last()?.clone();
    if first.len() < 2 || last.len() < 2 {
        return None;
    }

    let middle_initial = if ordered_tokens.len() >= 3 {
        ordered_tokens[1].chars().next()
    } else {
        None
    };

    Some(NormalizedPersonName {
        first,
        middle_initial,
        last,
    })
}

fn clean_name_tokens(segment: &str) -> Vec<String> {
    let mut out = Vec::new();
    for part in segment.split_whitespace() {
        // Check hyphenated credential suffix like "PMHNP-BC" or "PA-C" before splitting.
        let compact: String = part
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .map(|c| c.to_ascii_lowercase())
            .collect();
        if compact.is_empty() {
            continue;
        }
        if is_credential_or_suffix(&compact) || is_title(&compact) {
            out.push(compact);
            continue;
        }
        let alpha: String = part
            .chars()
            .filter(|c| c.is_ascii_alphabetic())
            .map(|c| c.to_ascii_lowercase())
            .collect();
        if !alpha.is_empty() {
            out.push(alpha);
        }
    }
    out
}

fn is_title(tok: &str) -> bool {
    matches!(tok, "dr" | "doctor" | "mr" | "mrs" | "ms" | "mx" | "prof")
}

fn is_credential_or_suffix(tok: &str) -> bool {
    matches!(
        tok,
        "md" | "do"
            | "mbbs"
            | "phd"
            | "psyd"
            | "edd"
            | "dnp"
            | "pmhnp"
            | "pmhnpbc"
            | "np"
            | "npc"
            | "aprn"
            | "arnp"
            | "crnp"
            | "fnp"
            | "fnpc"
            | "agnp"
            | "cnp"
            | "pa"
            | "pac"
            | "rpa"
            | "lcsw"
            | "lcswc"
            | "lcswr"
            | "licsw"
            | "lisw"
            | "lscsw"
            | "lpc"
            | "lpcc"
            | "lmhc"
            | "lcpc"
            | "lcmhc"
            | "lmft"
            | "mft"
            | "ladc"
            | "cadc"
            | "lcdc"
            | "casac"
            | "lac"
            | "cac"
            | "lasac"
            | "lisac"
            | "rn"
            | "bsn"
            | "msn"
            | "lpn"
            | "lvn"
            | "cprs"
            | "cpss"
            | "bc"
            | "fapa"
            | "dfapa"
            | "facp"
            | "fasam"
            | "jr"
            | "sr"
            | "ii"
            | "iii"
            | "iv"
    )
}

/// Strictly validates whether a state board provider license is compatible with a catalog
/// provider's role.
///
/// Guarantees:
/// - MD licenses never attach to DO providers, and DO licenses never attach to MD providers.
/// - NP / PMHNP / PA licenses never attach to MD / DO psychiatrists or physicians.
/// - PhD and PsyD psychologists never cross-attach.
/// - LCSW, LPC/LMHC, and LMFT never cross-attach.
/// - General state MD/DO physician licenses may attach to MD/DO psychiatrists or addiction
///   specialists of the SAME degree, and general NP licenses may attach to PMHNPs.
fn is_role_compatible(provider_role: ProviderRole, record_license: StaffLicense) -> bool {
    let ProviderRole::Clinical(prov_lic) = provider_role else {
        return false;
    };

    if prov_lic == record_license {
        return true;
    }

    match (prov_lic, record_license) {
        // Same-degree MD physician license from a state Medical Board matching an MD specialist.
        (StaffLicense::MdPsychiatrist | StaffLicense::MdAddiction, StaffLicense::MdPhysician)
        | (StaffLicense::MdPhysician, StaffLicense::MdPsychiatrist | StaffLicense::MdAddiction) => {
            true
        }
        // Same-degree DO physician license from an Osteopathic Board matching a DO specialist.
        (StaffLicense::DoPsychiatrist | StaffLicense::DoAddiction, StaffLicense::DoPhysician)
        | (StaffLicense::DoPhysician, StaffLicense::DoPsychiatrist | StaffLicense::DoAddiction) => {
            true
        }
        // State nursing board NP/APRN license matching a PMHNP (still strictly non-physician).
        (StaffLicense::Pmhnp, StaffLicense::Np) | (StaffLicense::Np, StaffLicense::Pmhnp) => true,
        _ => false,
    }
}

/// Attaches or updates a deduplicated [`StateProviderLicense`]. Returns `true` if newly added.
fn attach_provider_license(provider: &mut NetworkProvider, entry: StateProviderLicense) -> bool {
    if let Some(existing) = provider.state_licenses.iter_mut().find(|e| {
        e.state.eq_ignore_ascii_case(&entry.state)
            && e.board.eq_ignore_ascii_case(&entry.board)
            && e.license_number.eq_ignore_ascii_case(&entry.license_number)
    }) {
        *existing = entry;
        return false;
    }
    provider.state_licenses.push(entry);
    provider.state_licenses.sort_by(|a, b| {
        a.board
            .cmp(&b.board)
            .then_with(|| a.license_number.cmp(&b.license_number))
    });
    true
}

#[cfg(test)]
mod tests {
    use super::super::types::ProviderLicenseRecord;
    use super::*;
    use crate::schema::{
        Address, FacilityLifestyle, LicenseStatus, StaffArrangement, WellnessRole,
    };

    fn make_facility(
        name: &str,
        street: Option<&str>,
        city: &str,
        state: &str,
        zip: &str,
        phone: Option<&str>,
    ) -> Facility {
        Facility {
            id: None,
            name: name.to_string(),
            site_name: None,
            address: Address {
                street: street.map(str::to_string),
                street2: None,
                city: city.to_string(),
                state: state.to_string(),
                zip: zip.to_string(),
            },
            phone: phone.map(str::to_string),
            website: None,
            location: None,
            care: Vec::new(),
            settings: Vec::new(),
            payments: Vec::new(),
            groups: Vec::new(),
            lifestyle: FacilityLifestyle::default(),
            staff_licenses: Vec::new(),
            staffing_profile: None,
            state_licenses: Vec::new(),
            enrichment: None,
        }
    }

    fn make_provider(id: &str, name: &str, role: ProviderRole) -> NetworkProvider {
        NetworkProvider {
            id: id.to_string(),
            name: name.to_string(),
            organization: None,
            npi: None,
            role,
            arrangement: StaffArrangement::Shared,
            facility_ids: vec!["f1".to_string()],
            sources: vec![DataSource::Nppes],
            state_licenses: Vec::new(),
            notes: None,
        }
    }

    #[test]
    fn matches_facilities_by_street_zip_phone_and_distinctive_name() {
        let mut catalog = StateCatalog {
            state: "AZ".to_string(),
            retrieved: "2026-11-28".to_string(),
            facilities: vec![
                make_facility(
                    "Desert Hope Center",
                    Some("501 South Burma Avenue"),
                    "Phoenix",
                    "AZ",
                    "85004",
                    Some("(602) 555-0101"),
                ),
                make_facility(
                    "Canyon Vista Wellness",
                    Some("900 N Oak St"),
                    "Tucson",
                    "AZ",
                    "85701",
                    Some("520-555-0202"),
                ),
                make_facility(
                    "Sierra Sunrise Behavioral Health",
                    None,
                    "Scottsdale",
                    "AZ",
                    "85251",
                    None,
                ),
                // Unrelated facility in same city/ZIP that must NOT match.
                make_facility(
                    "Scottsdale Recovery Clinic",
                    Some("777 E Main St"),
                    "Scottsdale",
                    "AZ",
                    "85251",
                    Some("480-555-9999"),
                ),
            ],
            staff_network: Vec::new(),
        };

        let outcome = StateAdapterOutcome {
            state: "AZ".to_string(),
            facility_records: vec![
                // Match 1: normalized street + ZIP (different name)
                FacilityLicenseRecord {
                    state: "AZ".to_string(),
                    agency: "AZDHS".to_string(),
                    license_number: "BH-1".to_string(),
                    facility_name: "Desert Hope Operating LLC".to_string(),
                    dba_name: None,
                    license_type: "BHRF".to_string(),
                    status: LicenseStatus::Active,
                    street: Some("501 S BURMA AVE".to_string()),
                    city: Some("Phoenix".to_string()),
                    zip: Some("85004".to_string()),
                    phone: None,
                    licensed_beds: Some(16),
                    expires_on: Some("2027-01-01".to_string()),
                    disciplinary_actions: None,
                    inspection_count: None,
                    verification_url: Some("https://azcarecheck.azdhs.gov/s/?lic=BH-1".to_string()),
                },
                // Match 2: normalized 10-digit phone + same state
                FacilityLicenseRecord {
                    state: "AZ".to_string(),
                    agency: "AZDHS".to_string(),
                    license_number: "BH-2".to_string(),
                    facility_name: "Canyon Vista".to_string(),
                    dba_name: None,
                    license_type: "Outpatient".to_string(),
                    status: LicenseStatus::Active,
                    street: None,
                    city: Some("Tucson".to_string()),
                    zip: None,
                    phone: Some("+1 (520) 555-0202 x100".to_string()),
                    licensed_beds: None,
                    expires_on: None,
                    disciplinary_actions: None,
                    inspection_count: None,
                    verification_url: Some("https://azcarecheck.azdhs.gov/s/?lic=BH-2".to_string()),
                },
                // Match 3: distinctive 2+ token name overlap + same ZIP
                FacilityLicenseRecord {
                    state: "AZ".to_string(),
                    agency: "AZDHS".to_string(),
                    license_number: "BH-3".to_string(),
                    facility_name: "Sierra Sunrise Treatment Center LLC".to_string(),
                    dba_name: None,
                    license_type: "BHRF".to_string(),
                    status: LicenseStatus::Active,
                    street: None,
                    city: Some("Scottsdale".to_string()),
                    zip: Some("85251".to_string()),
                    phone: None,
                    licensed_beds: Some(10),
                    expires_on: None,
                    disciplinary_actions: None,
                    inspection_count: None,
                    verification_url: None,
                },
                // Non-match: only generic words + city name "Scottsdale"
                FacilityLicenseRecord {
                    state: "AZ".to_string(),
                    agency: "AZDHS".to_string(),
                    license_number: "BH-999".to_string(),
                    facility_name: "Scottsdale Behavioral Health Center".to_string(),
                    dba_name: None,
                    license_type: "Outpatient".to_string(),
                    status: LicenseStatus::Active,
                    street: None,
                    city: Some("Scottsdale".to_string()),
                    zip: Some("85251".to_string()),
                    phone: None,
                    licensed_beds: None,
                    expires_on: None,
                    disciplinary_actions: None,
                    inspection_count: None,
                    verification_url: None,
                },
            ],
            ..StateAdapterOutcome::default()
        };

        let summary = apply_state_licensing_to_catalog(&mut catalog, &outcome, "2026-11-28");
        assert_eq!(summary.facilities_matched, 3);
        assert_eq!(summary.facility_licenses_attached, 3);

        assert_eq!(catalog.facilities[0].state_licenses.len(), 1);
        assert_eq!(
            catalog.facilities[0].state_licenses[0].license_number,
            "BH-1"
        );
        assert_eq!(
            catalog.facilities[0].state_licenses[0]
                .retrieved_at
                .as_deref(),
            Some("2026-11-28")
        );
        let meta0 = catalog.facilities[0]
            .enrichment
            .as_ref()
            .expect("enrichment");
        assert!(meta0.sources.contains(&DataSource::StateLicense));
        assert!(meta0
            .evidence_urls
            .contains(&"https://azcarecheck.azdhs.gov/s/?lic=BH-1".to_string()));

        assert_eq!(catalog.facilities[1].state_licenses.len(), 1);
        assert_eq!(
            catalog.facilities[1].state_licenses[0].license_number,
            "BH-2"
        );

        assert_eq!(catalog.facilities[2].state_licenses.len(), 1);
        assert_eq!(
            catalog.facilities[2].state_licenses[0].license_number,
            "BH-3"
        );

        // Unrelated facility did not receive a false-positive match.
        assert!(catalog.facilities[3].state_licenses.is_empty());

        // Re-applying the same outcome is idempotent (deduplicated).
        let second = apply_state_licensing_to_catalog(&mut catalog, &outcome, "2026-11-28");
        assert_eq!(second.facility_licenses_attached, 0);
        assert_eq!(catalog.facilities[0].state_licenses.len(), 1);
    }

    #[test]
    fn matches_providers_strictly_without_md_do_or_np_conflation() {
        let mut catalog = StateCatalog {
            state: "AZ".to_string(),
            retrieved: "2026-11-28".to_string(),
            facilities: Vec::new(),
            staff_network: vec![
                make_provider(
                    "p1",
                    "Dr. Elena Vance, MD",
                    ProviderRole::Clinical(StaffLicense::MdPsychiatrist),
                ),
                make_provider(
                    "p2",
                    "Marcus Cole, DO",
                    ProviderRole::Clinical(StaffLicense::DoPsychiatrist),
                ),
                make_provider(
                    "p3",
                    "Sarah Chen, PMHNP-BC",
                    ProviderRole::Clinical(StaffLicense::Pmhnp),
                ),
                make_provider(
                    "p4",
                    "Jordan Lee, PhD",
                    ProviderRole::Clinical(StaffLicense::PhdPsychologist),
                ),
                make_provider(
                    "p5",
                    "Taylor Brooks",
                    ProviderRole::Wellness(WellnessRole::YogaInstructor),
                ),
            ],
        };

        let outcome = StateAdapterOutcome {
            state: "AZ".to_string(),
            provider_records: vec![
                // Valid: state Medical Board MD license matches MD Psychiatrist ("VANCE, ELENA").
                ProviderLicenseRecord {
                    state: "AZ".to_string(),
                    board: "AZ Medical Board".to_string(),
                    license_number: "MD-1001".to_string(),
                    full_name: "VANCE, ELENA".to_string(),
                    credential_label: "MD".to_string(),
                    license: Some(StaffLicense::MdPhysician),
                    status: LicenseStatus::Active,
                    city: Some("Phoenix".to_string()),
                    zip: Some("85004".to_string()),
                    expires_on: None,
                    disciplinary_flag: false,
                    verification_url: Some("https://azmd.gov/1001".to_string()),
                },
                // MUST NOT MATCH: DO license with Elena Vance's name must NEVER attach to MD provider.
                ProviderLicenseRecord {
                    state: "AZ".to_string(),
                    board: "AZ Board of Osteopathic Examiners".to_string(),
                    license_number: "DO-9999".to_string(),
                    full_name: "Elena Vance, DO".to_string(),
                    credential_label: "DO".to_string(),
                    license: Some(StaffLicense::DoPhysician),
                    status: LicenseStatus::Active,
                    city: None,
                    zip: None,
                    expires_on: None,
                    disciplinary_flag: false,
                    verification_url: None,
                },
                // MUST NOT MATCH: NP license with Marcus Cole's name must NEVER attach to DO Psychiatrist.
                ProviderLicenseRecord {
                    state: "AZ".to_string(),
                    board: "AZ Board of Nursing".to_string(),
                    license_number: "NP-5555".to_string(),
                    full_name: "Marcus Cole, NP".to_string(),
                    credential_label: "Nurse Practitioner".to_string(),
                    license: Some(StaffLicense::Np),
                    status: LicenseStatus::Active,
                    city: None,
                    zip: None,
                    expires_on: None,
                    disciplinary_flag: false,
                    verification_url: None,
                },
                // Valid: state Nursing Board NP license matches PMHNP provider.
                ProviderLicenseRecord {
                    state: "AZ".to_string(),
                    board: "AZ Board of Nursing".to_string(),
                    license_number: "APRN-3002".to_string(),
                    full_name: "Sarah Chen".to_string(),
                    credential_label: "APRN".to_string(),
                    license: Some(StaffLicense::Np),
                    status: LicenseStatus::Active,
                    city: None,
                    zip: None,
                    expires_on: None,
                    disciplinary_flag: false,
                    verification_url: None,
                },
                // MUST NOT MATCH: PsyD license must NEVER attach to PhD psychologist.
                ProviderLicenseRecord {
                    state: "AZ".to_string(),
                    board: "AZ Board of Psychologist Examiners".to_string(),
                    license_number: "PSY-404".to_string(),
                    full_name: "Jordan Lee, PsyD".to_string(),
                    credential_label: "PsyD".to_string(),
                    license: Some(StaffLicense::PsydPsychologist),
                    status: LicenseStatus::Active,
                    city: None,
                    zip: None,
                    expires_on: None,
                    disciplinary_flag: false,
                    verification_url: None,
                },
                // MUST NOT MATCH: Clinical license must NEVER attach to Wellness role.
                ProviderLicenseRecord {
                    state: "AZ".to_string(),
                    board: "AZ Board of Behavioral Health".to_string(),
                    license_number: "LCSW-777".to_string(),
                    full_name: "Taylor Brooks, LCSW".to_string(),
                    credential_label: "LCSW".to_string(),
                    license: Some(StaffLicense::Lcsw),
                    status: LicenseStatus::Active,
                    city: None,
                    zip: None,
                    expires_on: None,
                    disciplinary_flag: false,
                    verification_url: None,
                },
            ],
            ..StateAdapterOutcome::default()
        };

        let summary = apply_state_licensing_to_catalog(&mut catalog, &outcome, "2026-11-28");
        assert_eq!(summary.providers_matched, 2);
        assert_eq!(summary.provider_licenses_attached, 2);

        // Elena Vance (MD Psychiatrist) only got MD-1001, NOT DO-9999.
        assert_eq!(catalog.staff_network[0].state_licenses.len(), 1);
        assert_eq!(
            catalog.staff_network[0].state_licenses[0].license_number,
            "MD-1001"
        );
        assert!(catalog.staff_network[0]
            .sources
            .contains(&DataSource::StateLicense));

        // Marcus Cole (DO Psychiatrist) did NOT get NP-5555.
        assert!(catalog.staff_network[1].state_licenses.is_empty());

        // Sarah Chen (PMHNP) got APRN-3002.
        assert_eq!(catalog.staff_network[2].state_licenses.len(), 1);
        assert_eq!(
            catalog.staff_network[2].state_licenses[0].license_number,
            "APRN-3002"
        );

        // Jordan Lee (PhD) did NOT get PsyD license.
        assert!(catalog.staff_network[3].state_licenses.is_empty());

        // Taylor Brooks (Wellness) did NOT get LCSW license.
        assert!(catalog.staff_network[4].state_licenses.is_empty());
    }
}
