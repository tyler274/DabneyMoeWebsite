//! Human-editable, checked-in per-state TOML overrides (`data/recovery/overrides/<ST>.toml`).
//!
//! Manual / phone-verified entries always take precedence over automated scraping or NPPES
//! matching, and survive SAMHSA re-imports cleanly because they key on [`Facility::stable_id`].

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::schema::{
    Amenity, DataSource, DietaryOption, ElectronicsPolicy, EnrichmentMeta, Facility, FoodSituation,
    NetworkProvider, SmokingPolicy, StaffLicense, VapingPolicy,
};

#[derive(Debug, thiserror::Error)]
pub enum OverrideError {
    #[error("could not read override file {}", path.display())]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not parse override TOML {}", path.display())]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error("could not serialize override TOML {}", path.display())]
    Serialize {
        path: PathBuf,
        #[source]
        source: toml::ser::Error,
    },
    #[error("could not write override file {}", path.display())]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateOverridesFile {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub facilities: Vec<FacilityOverride>,
    /// Curated shared clinicians and visiting/contracted specialists (e.g. traveling PMHNPs,
    /// local yoga instructors serving multiple clinics in the state).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub providers: Vec<NetworkProvider>,
}

/// Curated per-facility override stored in `data/recovery/overrides/<ST>.toml`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FacilityOverride {
    /// Matches [`Facility::stable_id`].
    pub id: String,
    /// Optional human-readable facility name (for easy reviewing in git diffs).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub amenities: Vec<Amenity>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub remove_amenities: Vec<Amenity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub food: Option<FoodSituation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dietary: Vec<DietaryOption>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub remove_dietary: Vec<DietaryOption>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub electronics: Option<ElectronicsPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub smoking: Option<SmokingPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vaping: Option<VapingPolicy>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub staff_licenses: Vec<StaffLicense>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub remove_staff_licenses: Vec<StaffLicense>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verified_at: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence_urls: Vec<String>,
}

impl FacilityOverride {
    /// Applies this curated override onto `facility`, preserving provenance.
    pub fn apply_to(&self, facility: &mut Facility) {
        for &a in &self.amenities {
            if !facility.lifestyle.amenities.contains(&a) {
                facility.lifestyle.amenities.push(a);
            }
        }
        facility
            .lifestyle
            .amenities
            .retain(|a| !self.remove_amenities.contains(a));

        if let Some(food) = self.food {
            facility.lifestyle.food = Some(food);
        }
        for &d in &self.dietary {
            if !facility.lifestyle.dietary.contains(&d) {
                facility.lifestyle.dietary.push(d);
            }
        }
        facility
            .lifestyle
            .dietary
            .retain(|d| !self.remove_dietary.contains(d));

        if let Some(electronics) = self.electronics {
            facility.lifestyle.electronics = Some(electronics);
        }
        if let Some(smoking) = self.smoking {
            facility.lifestyle.smoking = Some(smoking);
        }
        if let Some(vaping) = self.vaping {
            facility.lifestyle.vaping = Some(vaping);
        }
        if let Some(notes) = &self.notes {
            let trimmed = notes.trim();
            if !trimmed.is_empty() {
                facility.lifestyle.notes = Some(trimmed.to_string());
            }
        }
        facility.lifestyle.normalize();

        for &lic in &self.staff_licenses {
            if !facility.staff_licenses.contains(&lic) {
                facility.staff_licenses.push(lic);
            }
        }
        facility
            .staff_licenses
            .retain(|lic| !self.remove_staff_licenses.contains(lic));
        facility.staff_licenses.sort_unstable();
        facility.staff_licenses.dedup();

        let meta = facility
            .enrichment
            .get_or_insert_with(EnrichmentMeta::default);
        meta.add_source(DataSource::Verified);
        if let Some(date) = &self.verified_at {
            meta.verified_at = Some(date.clone());
        }
        for url in &self.evidence_urls {
            meta.add_evidence_url(url.clone());
        }
    }
}

pub fn override_path(overrides_dir: &Path, state: &str) -> PathBuf {
    overrides_dir.join(format!("{}.toml", state.to_ascii_uppercase()))
}

pub fn load_state_overrides_file(
    overrides_dir: &Path,
    state: &str,
) -> Result<StateOverridesFile, OverrideError> {
    let path = override_path(overrides_dir, state);
    if !path.exists() {
        return Ok(StateOverridesFile::default());
    }
    let raw = fs::read_to_string(&path).map_err(|source| OverrideError::Read {
        path: path.clone(),
        source,
    })?;
    toml::from_str(&raw).map_err(|source| OverrideError::Parse { path, source })
}

pub fn load_state_overrides(
    overrides_dir: &Path,
    state: &str,
) -> Result<BTreeMap<String, FacilityOverride>, OverrideError> {
    let parsed = load_state_overrides_file(overrides_dir, state)?;
    let mut map = BTreeMap::new();
    for entry in parsed.facilities {
        map.insert(entry.id.clone(), entry);
    }
    Ok(map)
}

pub fn load_state_curated_providers(
    overrides_dir: &Path,
    state: &str,
) -> Result<Vec<NetworkProvider>, OverrideError> {
    Ok(load_state_overrides_file(overrides_dir, state)?.providers)
}

pub fn save_state_overrides(
    overrides_dir: &Path,
    state: &str,
    entries: &BTreeMap<String, FacilityOverride>,
) -> Result<PathBuf, OverrideError> {
    let existing = load_state_overrides_file(overrides_dir, state)?;
    save_state_overrides_file(
        overrides_dir,
        state,
        &StateOverridesFile {
            facilities: entries.values().cloned().collect(),
            providers: existing.providers,
        },
    )
}

pub fn save_state_overrides_file(
    overrides_dir: &Path,
    state: &str,
    doc: &StateOverridesFile,
) -> Result<PathBuf, OverrideError> {
    fs::create_dir_all(overrides_dir).map_err(|source| OverrideError::Write {
        path: overrides_dir.to_path_buf(),
        source,
    })?;
    let path = override_path(overrides_dir, state);
    let header = format!(
        "# Curated & verified non-care amenities, staff licenses, and shared-staff network overrides for {}.\n\
         # Edit directly or update via `cargo run -p recovery-index --features cli -- set|add-provider --state {} ...`.\n\n",
        state.to_ascii_uppercase(),
        state.to_ascii_uppercase(),
    );
    let body = toml::to_string_pretty(doc).map_err(|source| OverrideError::Serialize {
        path: path.clone(),
        source,
    })?;
    fs::write(&path, format!("{header}{body}")).map_err(|source| OverrideError::Write {
        path: path.clone(),
        source,
    })?;
    Ok(path)
}
