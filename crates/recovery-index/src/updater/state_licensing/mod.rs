//! 50-state (+ DC and territories) facility and provider licensing adapter framework.
//!
//! Covers all 55 US jurisdictions present in `public/recovery/index.json`:
//! - Priority dedicated adapters (`AZ`, `CA`, `NY`, `CO`) with live verified Open Data /
//!   ArcGIS / CKAN / Socrata / official CSV pipelines and state-specific credential rules.
//! - Declarative verified Open Data endpoints across 14 additional states/DC (`CT`, `DC`,
//!   `IA`, `MD`, `MI`, `MN`, `MO`, `NJ`, `OR`, `PA`, `TX`, `UT`, `WA`, `WI`) executed
//!   through [`generic::execute_generic_endpoint`].
//! - Complete official agency and board manifests for all 55 jurisdictions, explicitly
//!   modeling states with separate Osteopathic boards (`separate_osteopathic_board = true`)
//!   and supporting local bulk file import (`--import-file`) for CAPTCHA-gated portals
//!   without ever bypassing anti-bot checks.

pub mod az;
pub mod ca;
pub mod co;
pub mod generic;
pub mod matcher;
pub mod ny;
pub mod registry_south_northeast;
pub mod registry_west_midwest;
pub mod types;

use crate::updater::cache::DiskCache;

pub use matcher::{apply_state_licensing_to_catalog, MatchSummary};
pub use types::{
    FacilityFieldMap, FacilityLicenseRecord, GenericEndpointSpec, LicensingAccessMode,
    LicensingDatasetFormat, LicensingScope, ProviderFieldMap, ProviderLicenseRecord,
    StateAdapterOutcome, StateBoardDescriptor, StateLicenseError, StateLicensingManifest,
    StateSyncRequest,
};

/// Returns the complete sorted list of official facility and provider licensing manifests
/// covering all 55 US jurisdictions (50 states, DC, and PR/GU/VI/MP).
pub fn all_state_manifests() -> Vec<StateLicensingManifest> {
    let mut manifests = Vec::with_capacity(55);
    manifests.push(az::manifest());
    manifests.push(ca::manifest());
    manifests.push(co::manifest());
    manifests.push(ny::manifest());
    manifests.extend(registry_west_midwest::manifests().iter().cloned());
    manifests.extend(registry_south_northeast::manifests().iter().cloned());
    manifests.sort_by(|a, b| a.state.cmp(b.state));
    manifests
}

/// Looks up the licensing manifest for a single 2-letter US jurisdiction code.
pub fn manifest_for_state(state: &str) -> Option<StateLicensingManifest> {
    let code = state.trim().to_ascii_uppercase();
    match code.as_str() {
        "AZ" => Some(az::manifest()),
        "CA" => Some(ca::manifest()),
        "CO" => Some(co::manifest()),
        "NY" => Some(ny::manifest()),
        _ => registry_west_midwest::manifests()
            .iter()
            .chain(registry_south_northeast::manifests().iter())
            .find(|m| m.state == code)
            .cloned(),
    }
}

/// Returns true if a state has at least one live unauthenticated Open Data / direct API
/// adapter configured (either a custom state adapter or a declarative `GenericEndpointSpec`).
pub fn has_live_adapter(manifest: &StateLicensingManifest) -> bool {
    manifest.systems.iter().any(|s| {
        s.generic_endpoint.is_some()
            || matches!(
                s.access_mode,
                LicensingAccessMode::OpenDataApi | LicensingAccessMode::DirectSearchApi
            )
    })
}

/// Runs the appropriate facility and/or provider licensing adapter(s) for `state`.
///
/// Priority states (`AZ`, `CA`, `NY`, `CO`) run their dedicated state modules; all other
/// jurisdictions run any verified `GenericEndpointSpec` defined in their manifest plus
/// optional local bulk export ingestion via `req.import_file`.
pub async fn sync_state_licensing(
    client: &reqwest::Client,
    cache: &DiskCache,
    state: &str,
    req: &StateSyncRequest,
) -> Result<StateAdapterOutcome, StateLicenseError> {
    let code = state.trim().to_ascii_uppercase();
    match code.as_str() {
        "AZ" => az::sync(client, cache, req).await,
        "CA" => ca::sync(client, cache, req).await,
        "NY" => ny::sync(client, cache, req).await,
        "CO" => co::sync(client, cache, req).await,
        _ => sync_generic_manifest(client, cache, &code, req).await,
    }
}

async fn sync_generic_manifest(
    client: &reqwest::Client,
    cache: &DiskCache,
    state: &str,
    req: &StateSyncRequest,
) -> Result<StateAdapterOutcome, StateLicenseError> {
    let Some(manifest) = manifest_for_state(state) else {
        return Err(StateLicenseError::InvalidConfig(format!(
            "No state licensing manifest registered for `{state}`."
        )));
    };

    let mut outcome = StateAdapterOutcome {
        state: state.to_string(),
        ..StateAdapterOutcome::default()
    };

    if let Some(import_path) = &req.import_file {
        let selected_system = req
            .system_filter
            .as_deref()
            .and_then(|wanted| {
                manifest
                    .systems
                    .iter()
                    .find(|s| s.id.eq_ignore_ascii_case(wanted))
            })
            .or_else(|| manifest.systems.first());

        let (agency, scope) = match selected_system {
            Some(sys) => {
                let effective_scope = if req.skip_providers && !req.skip_facilities {
                    LicensingScope::Facility
                } else if req.skip_facilities && !req.skip_providers {
                    LicensingScope::Provider
                } else {
                    sys.scope
                };
                (sys.agency_name, effective_scope)
            }
            None => ("State Licensing Import", LicensingScope::Both),
        };
        let imported = generic::import_local_file(import_path, state, agency, scope)?;
        outcome.merge(imported);
        return Ok(outcome);
    }

    for sys in manifest.systems {
        if let Some(filter) = req.system_filter.as_deref() {
            if !sys.id.eq_ignore_ascii_case(filter) {
                continue;
            }
        }
        if req.skip_facilities && sys.scope == LicensingScope::Facility {
            continue;
        }
        if req.skip_providers && sys.scope == LicensingScope::Provider {
            continue;
        }
        let Some(spec) = &sys.generic_endpoint else {
            continue;
        };
        match generic::execute_generic_endpoint(client, cache, state, sys.id, spec).await {
            Ok(part) => outcome.merge(part),
            Err(StateLicenseError::CaptchaBlocked { url }) => {
                outcome.warnings.push(format!(
                    "{}: blocked by CAPTCHA/anti-bot gate at {url}; use --import-file or official portal {}",
                    sys.id, sys.portal_url
                ));
            }
            Err(err) => {
                outcome
                    .warnings
                    .push(format!("{}: sync warning: {err}", sys.id));
            }
        }
    }

    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn all_55_jurisdictions_have_unique_verified_licensing_manifests() {
        let manifests = all_state_manifests();
        assert_eq!(
            manifests.len(),
            55,
            "expected 50 states + DC + 4 territories"
        );

        let mut seen = BTreeSet::new();
        let mut live_count = 0usize;
        let mut separate_do_states = Vec::new();

        for m in &manifests {
            assert!(
                seen.insert(m.state),
                "duplicate state manifest for {}",
                m.state
            );
            assert!(
                !m.systems.is_empty(),
                "state {} must register at least one licensing system",
                m.state
            );
            for sys in m.systems {
                assert_eq!(sys.state, m.state);
                assert!(
                    sys.portal_url.starts_with("https://"),
                    "system {} portal URL must use https: {}",
                    sys.id,
                    sys.portal_url
                );
            }
            if has_live_adapter(m) {
                live_count += 1;
            }
            if m.separate_osteopathic_board {
                separate_do_states.push(m.state);
            }
        }

        // Priority 4 states + 14 open-data states/DC = 18 live jurisdictions out of the box.
        assert!(
            live_count >= 18,
            "expected at least 18 live open-data jurisdictions, found {live_count}"
        );
        for priority in ["AZ", "CA", "CO", "NY"] {
            let m = manifest_for_state(priority).expect("priority state manifest");
            assert!(has_live_adapter(&m), "{priority} must have live adapter");
        }
        // Verify key states with separate DO boards are modeled accurately.
        for do_state in [
            "AZ", "CA", "FL", "ME", "MI", "NV", "OK", "PA", "TN", "UT", "VT", "WA", "WV",
        ] {
            assert!(
                separate_do_states.contains(&do_state),
                "expected {do_state} to have separate_osteopathic_board = true"
            );
        }
    }
}
