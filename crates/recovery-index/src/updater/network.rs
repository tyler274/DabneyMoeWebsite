//! Cross-facility shared staff & visiting provider network builder.
//!
//! Single source of truth for assembling `StateCatalog::staff_network` and computing each
//! facility's `StaffingProfile` (dedicated clinicians vs shared/visiting clinicians, dedicated
//! MD/DO physicians vs shared prescribers, visiting wellness specialists, and distinct peer
//! facility overlap count).

use std::collections::{BTreeMap, BTreeSet};

use crate::schema::{
    DataSource, NetworkProvider, ProviderRole, StaffArrangement, StaffingProfile, StateCatalog,
};

/// One observed link between a person/contractor and a facility (from NPPES, website/LLM, or
/// existing shard network).
#[derive(Clone, Debug)]
pub struct ProviderObservation {
    pub id: String,
    pub name: String,
    pub organization: Option<String>,
    pub npi: Option<String>,
    pub role: ProviderRole,
    pub arrangement_hint: StaffArrangement,
    pub facility_id: String,
    pub source: DataSource,
    pub notes: Option<String>,
}

/// Controls how [`rebuild_state_staff_network`] reconciles prior shard network data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetworkRebuildMode {
    /// Full-state NPPES observations were just collected for every facility in the state:
    /// replace stale automated NPPES-only network entries while preserving LLM/Website and
    /// curated entries, and recompute every facility's `StaffingProfile` from scratch.
    FullNppesRefresh,
    /// Partial update (`add-provider`, `rebuild`, or `enrich --skip-nppes` / `--limit N`):
    /// preserve existing NPPES/LLM/Website network entries and per-facility dedicated clinician
    /// counts while folding in new observations and the full curated `[[providers]]` list.
    IncrementalMerge,
}

/// Rebuilds `catalog.staff_network` and every facility's `staffing_profile` consistently from:
/// - `observations`: per-facility clinician/specialist observations (NPPES + LLM)
/// - `curated`: human-verified/curated [`NetworkProvider`] entries from `overrides/<ST>.toml`
/// - `mode`: whether this pass has complete NPPES coverage or is an incremental update
///
/// Only providers that are shared across 2+ facilities, explicitly `Visiting`, or curated/verified
/// are stored in `catalog.staff_network` (keeping shards compact while still counting every
/// single-site clinician in each facility's `dedicated_clinicians` / `dedicated_physicians`).
pub fn rebuild_state_staff_network(
    catalog: &mut StateCatalog,
    observations: &[ProviderObservation],
    curated: &[NetworkProvider],
    mode: NetworkRebuildMode,
) {
    let valid_ids: BTreeSet<String> = catalog.facilities.iter().map(|f| f.stable_id()).collect();

    // Preserve prior non-stale providers already on the catalog before folding in fresh
    // observations & curated entries. Verified-only entries are always reloaded from `curated`
    // so removing a provider from `overrides/<ST>.toml` removes it cleanly on rebuild.
    let mut merged: BTreeMap<String, NetworkProvider> = BTreeMap::new();
    let mut is_curated: BTreeSet<String> = BTreeSet::new();

    for existing in &catalog.staff_network {
        let keep_existing = match mode {
            NetworkRebuildMode::FullNppesRefresh => existing
                .sources
                .iter()
                .any(|s| matches!(s, DataSource::Llm | DataSource::Website)),
            NetworkRebuildMode::IncrementalMerge => existing
                .sources
                .iter()
                .any(|s| matches!(s, DataSource::Nppes | DataSource::Llm | DataSource::Website)),
        };
        if keep_existing {
            let mut copy = existing.clone();
            copy.facility_ids.retain(|fid| valid_ids.contains(fid));
            // Drop stale Verified marker unless still present in `curated` below.
            copy.sources.retain(|s| *s != DataSource::Verified);
            if !copy.facility_ids.is_empty() {
                merged.insert(copy.id.clone(), copy);
            }
        }
    }

    for obs in observations {
        if !valid_ids.contains(&obs.facility_id) {
            continue;
        }
        let entry = merged
            .entry(obs.id.clone())
            .or_insert_with(|| NetworkProvider {
                id: obs.id.clone(),
                name: obs.name.clone(),
                organization: obs.organization.clone(),
                npi: obs.npi.clone(),
                role: obs.role,
                arrangement: obs.arrangement_hint,
                facility_ids: Vec::new(),
                sources: Vec::new(),
                notes: obs.notes.clone(),
            });
        // Prefer a real person name over a placeholder "NPI ..." label.
        if entry.name.starts_with("NPI ") && !obs.name.starts_with("NPI ") {
            entry.name = obs.name.clone();
        }
        if entry.organization.is_none() {
            entry.organization.clone_from(&obs.organization);
        }
        if entry.npi.is_none() {
            entry.npi.clone_from(&obs.npi);
        }
        if entry.notes.is_none() {
            entry.notes.clone_from(&obs.notes);
        }
        if obs.arrangement_hint == StaffArrangement::Visiting {
            entry.arrangement = StaffArrangement::Visiting;
        }
        if !entry.facility_ids.contains(&obs.facility_id) {
            entry.facility_ids.push(obs.facility_id.clone());
        }
        if !entry.sources.contains(&obs.source) {
            entry.sources.push(obs.source);
        }
    }

    // Layer curated providers last so verified facts always win.
    for cur in curated {
        is_curated.insert(cur.id.clone());
        let mut entry = cur.clone();
        entry.facility_ids.retain(|fid| valid_ids.contains(fid));
        entry.facility_ids.sort();
        entry.facility_ids.dedup();
        if !entry.sources.contains(&DataSource::Verified) {
            entry.sources.push(DataSource::Verified);
        }
        entry.sources.sort_unstable();
        if let Some(prev) = merged.get(&entry.id) {
            for fid in &prev.facility_ids {
                if !entry.facility_ids.contains(fid) {
                    entry.facility_ids.push(fid.clone());
                }
            }
            entry.facility_ids.sort();
            entry.facility_ids.dedup();
        }
        if !entry.facility_ids.is_empty() {
            merged.insert(entry.id.clone(), entry);
        }
    }

    // Classify every provider's effective arrangement and accumulate per-facility profiles.
    let mut profiles: BTreeMap<String, StaffingProfile> = BTreeMap::new();
    let mut peers_by_facility: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut published_network: Vec<NetworkProvider> = Vec::new();

    for mut prov in merged.into_values() {
        prov.facility_ids.sort();
        prov.facility_ids.dedup();
        prov.sources.sort_unstable();
        prov.sources.dedup();

        let site_count = prov.facility_ids.len();
        if site_count == 0 {
            continue;
        }
        if site_count >= 2 && prov.arrangement == StaffArrangement::Dedicated {
            prov.arrangement = StaffArrangement::Shared;
        }

        let is_shared_or_visiting =
            site_count >= 2 || prov.arrangement == StaffArrangement::Visiting;
        let publish_in_network = is_shared_or_visiting || is_curated.contains(&prov.id);

        for fid in &prov.facility_ids {
            let prof = profiles.entry(fid.clone()).or_default();
            match prov.role {
                ProviderRole::Clinical(lic) => {
                    if is_shared_or_visiting {
                        prof.shared_clinicians += 1;
                        if lic.is_prescriber() {
                            prof.shared_prescribers += 1;
                        }
                    } else {
                        prof.dedicated_clinicians += 1;
                        if lic.is_physician() {
                            prof.dedicated_physicians += 1;
                        }
                    }
                }
                ProviderRole::Wellness(_) => {
                    if is_shared_or_visiting {
                        prof.visiting_specialists += 1;
                    }
                }
            }

            if publish_in_network {
                prof.network_provider_ids.push(prov.id.clone());
            }

            if site_count >= 2 {
                let peers = peers_by_facility.entry(fid.clone()).or_default();
                for other in &prov.facility_ids {
                    if other != fid {
                        peers.insert(other.clone());
                    }
                }
            }
        }

        if publish_in_network {
            published_network.push(prov);
        }
    }

    // Sort published network providers: curated/verified & visiting wellness/specialists first,
    // then multi-site prescribers & clinicians spanning the most facilities, and cap automated
    // NPPES-only entries per state to keep browser shards lean (< 1 MB) while retaining exact
    // dedicated/shared counts on every facility.
    const MAX_AUTO_NPPES_NETWORK_PER_STATE: usize = 200;

    let fac_org_city: BTreeMap<String, (String, String)> = catalog
        .facilities
        .iter()
        .map(|f| {
            (
                f.stable_id(),
                (
                    f.name.trim().to_ascii_lowercase(),
                    f.address.city.trim().to_ascii_lowercase(),
                ),
            )
        })
        .collect();

    let distinct_org_count = |p: &NetworkProvider| -> usize {
        p.facility_ids
            .iter()
            .filter_map(|fid| fac_org_city.get(fid))
            .map(|(name, city)| (name.as_str(), city.as_str()))
            .collect::<BTreeSet<_>>()
            .len()
    };

    let (mut priority_entries, mut auto_nppes_entries): (
        Vec<NetworkProvider>,
        Vec<NetworkProvider>,
    ) = published_network.into_iter().partition(|p| {
        is_curated.contains(&p.id)
            || p.arrangement == StaffArrangement::Visiting
            || p.sources.iter().any(|s| {
                matches!(
                    s,
                    DataSource::Verified | DataSource::Llm | DataSource::Website
                )
            })
    });

    auto_nppes_entries.sort_by(|a, b| {
        let a_named = !a.name.starts_with("NPI ");
        let b_named = !b.name.starts_with("NPI ");
        b.facility_ids
            .len()
            .cmp(&a.facility_ids.len())
            .then_with(|| distinct_org_count(b).cmp(&distinct_org_count(a)))
            .then_with(|| b.role.is_prescriber().cmp(&a.role.is_prescriber()))
            .then_with(|| b_named.cmp(&a_named))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    auto_nppes_entries.truncate(MAX_AUTO_NPPES_NETWORK_PER_STATE);

    priority_entries.extend(auto_nppes_entries);
    let mut published_network = priority_entries;
    published_network.sort_by(|a, b| {
        b.facility_ids
            .len()
            .cmp(&a.facility_ids.len())
            .then_with(|| b.role.is_prescriber().cmp(&a.role.is_prescriber()))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    let kept_provider_ids: BTreeSet<String> =
        published_network.iter().map(|p| p.id.clone()).collect();

    // Build quick lookup from facility_id -> curated clinical licenses to ensure curated
    // clinical providers also show up in `facility.staff_licenses`.
    let mut curated_clinical_by_fac: BTreeMap<String, Vec<crate::schema::StaffLicense>> =
        BTreeMap::new();
    for prov in &published_network {
        if let ProviderRole::Clinical(lic) = prov.role {
            for fid in &prov.facility_ids {
                curated_clinical_by_fac
                    .entry(fid.clone())
                    .or_default()
                    .push(lic);
            }
        }
    }

    for fac in &mut catalog.facilities {
        let fid = fac.stable_id();
        if let Some(extra_lics) = curated_clinical_by_fac.get(&fid) {
            for &lic in extra_lics {
                if !fac.staff_licenses.contains(&lic) {
                    fac.staff_licenses.push(lic);
                }
            }
            fac.staff_licenses.sort_unstable();
            fac.staff_licenses.dedup();
        }
        let next_prof = profiles.remove(&fid).map(|mut prof| {
            prof.network_provider_ids
                .retain(|id| kept_provider_ids.contains(id));
            prof.network_provider_ids.sort();
            prof.network_provider_ids.dedup();
            prof.shared_with_facilities = peers_by_facility
                .get(&fid)
                .map(|s| u32::try_from(s.len()).unwrap_or(u32::MAX))
                .unwrap_or(0);
            prof
        });

        match mode {
            NetworkRebuildMode::FullNppesRefresh => {
                fac.staffing_profile = next_prof.filter(|p| !p.is_empty());
            }
            NetworkRebuildMode::IncrementalMerge => {
                let mut merged_prof = next_prof.unwrap_or_default();
                if let Some(prev) = &fac.staffing_profile {
                    // Single-site dedicated clinicians & uncapped full-state shared counts live
                    // only on the facility's StaffingProfile (not in the capped `staff_network`).
                    merged_prof.dedicated_clinicians = merged_prof
                        .dedicated_clinicians
                        .max(prev.dedicated_clinicians);
                    merged_prof.dedicated_physicians = merged_prof
                        .dedicated_physicians
                        .max(prev.dedicated_physicians);
                    merged_prof.shared_clinicians =
                        merged_prof.shared_clinicians.max(prev.shared_clinicians);
                    merged_prof.shared_prescribers =
                        merged_prof.shared_prescribers.max(prev.shared_prescribers);
                    merged_prof.shared_with_facilities = merged_prof
                        .shared_with_facilities
                        .max(prev.shared_with_facilities);
                }
                fac.staffing_profile = (!merged_prof.is_empty()).then_some(merged_prof);
            }
        }
    }

    catalog.staff_network = published_network;
}

/// Builds a deterministic slug ID for a named non-NPI staff member or visiting specialist.
pub fn provider_slug_id(state: &str, name: &str, organization: Option<&str>) -> String {
    let combined = match organization.map(str::trim).filter(|s| !s.is_empty()) {
        Some(org) => format!("{name} {org}"),
        None => name.to_string(),
    };
    let mut slug = String::new();
    let mut prev_dash = false;
    for ch in combined.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash && !slug.is_empty() {
            slug.push('-');
            prev_dash = true;
        }
        if slug.len() >= 40 {
            break;
        }
    }
    let slug = slug.trim_end_matches('-');
    format!("staff-{}-{slug}", state.trim().to_ascii_lowercase())
}
