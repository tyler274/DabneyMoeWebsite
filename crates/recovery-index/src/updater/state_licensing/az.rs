//! Arizona (`AZ`) facility and clinical provider licensing adapter.
//!
//! Live public facility source (verified unauthenticated, CAPTCHA-free):
//! - Arizona Department of Health Services (ADHS) Division of Licensing ArcGIS FeatureServer
//!   (`All_State_Licensed_Facilities_in_Arizona/FeatureServer/18`), backing AZCareCheck
//!   (`https://azcarecheck.azdhs.gov/s/`). Covers Behavioral Health Residential Facilities,
//!   Behavioral Health Inpatient Facilities, Outpatient Treatment Centers, Psychiatric Hospitals,
//!   Substance Abuse Transitional Facilities, Community Mental Health Centers, Counseling,
//!   DUI/DVTX, and Behavioral Health Therapeutic/Respite/Group Homes.
//!
//! Provider licensing boards (Arizona maintains a separate Board of Osteopathic Examiners,
//! keeping MD and DO credentials strictly separated):
//! - Arizona Medical Board (`azmd.gov`, MD and PA)
//! - Arizona Board of Osteopathic Examiners in Medicine and Surgery (`azdo.gov`, DO)
//! - Arizona State Board of Nursing (`azbn.gov` / Nursys QuickConfirm, PMHNP, NP, RN, LPN)
//! - Arizona Board of Behavioral Health Examiners (`bbhe.az.gov`, LCSW, LPC, LMFT, LISAC/LADC)
//! - Arizona Board of Psychologist Examiners (`psychboard.az.gov`, PhD, PsyD)
//!
//! All five Arizona provider portals are protected by Cloudflare/anti-bot checks and do not
//! expose an unauthenticated bulk API. Per adapter policy, we never bypass anti-bot gates;
//! those boards are modeled honestly as `BulkDownloadExport` with official lookup URLs and
//! support local public-records CSV/JSON ingestion via `--import-file`.

use std::collections::HashSet;

use serde_json::Value;
use url::Url;

use crate::schema::{LicenseStatus, StaffLicense};
use crate::updater::cache::{CacheMode, DiskCache};

use super::generic::{classify_state_provider_license, fetch_cached_text, import_local_file};
use super::types::{
    FacilityFieldMap, FacilityLicenseRecord, GenericEndpointSpec, LicensingAccessMode,
    LicensingDatasetFormat, LicensingScope, StateAdapterOutcome, StateBoardDescriptor,
    StateLicenseError, StateLicensingManifest, StateSyncRequest,
};

pub const AZ_ADHS_SYSTEM_ID: &str = "az-azdhs-facilities";
pub const AZ_MEDICAL_BOARD_ID: &str = "az-azmd";
pub const AZ_OSTEOPATHIC_BOARD_ID: &str = "az-azdo";
pub const AZ_NURSING_BOARD_ID: &str = "az-azbn";
pub const AZ_BBHE_BOARD_ID: &str = "az-azbbhe";
pub const AZ_PSYCH_BOARD_ID: &str = "az-azbpe";

pub const ADHS_AGENCY_NAME: &str = "Arizona Department of Health Services (ADHS)";
pub const ADHS_PORTAL_URL: &str = "https://azcarecheck.azdhs.gov/s/";
pub const ADHS_ARCGIS_LAYER_QUERY_URL: &str = "https://services1.arcgis.com/mpVYz37anSdrK4d8/arcgis/rest/services/All_State_Licensed_Facilities_in_Arizona/FeatureServer/18/query";

/// SQL WHERE clause filtering ADHS Layer 18 to behavioral health, substance use, and
/// psychiatric hospital facilities (verified returning 3,827 records out of 11,555 total).
pub const ADHS_BEHAVIORAL_HEALTH_WHERE: &str = concat!(
    "TYPE IN (",
    "'BEHAVIORAL HEALTH RESIDENTIAL FACILITY',",
    "'BEHAVIORAL HEALTH INPATIENT FACILITY',",
    "'OUTPATIENT TREATMENT CENTER',",
    "'SUBSTANCE ABUSE TRANSITIONAL',",
    "'COMMUNITY MENTAL HEALTH CENTERS',",
    "'COUNSELING',",
    "'DUI/DVTX',",
    "'ADULT BEHAVIORAL HEALTH THERAPEUTIC HOME',",
    "'BEHAVIORAL HEALTH RESPITE HOME',",
    "'BEHAVIORAL HEALTH SUPPORTED GROUP HOMES',",
    "'BH SPECIALIZED TRANSITIONAL FACILITY',",
    "'RECOVERY CARE CENTER'",
    ") OR SUBTYPE='PSYCHIATRIC'"
);

const ADHS_PAGE_SIZE: usize = 1000;
const ADHS_MAX_RECORDS: usize = 10_000;

const ADHS_OUT_FIELDS: &str = "FACID,FACILITY_NAME,LICENSE_NUMBER,TYPE,SUBTYPE,LICENSE_TYPE,Capacity,ADDRESS,CITY,ZIP,Telephone,OPERATION_STATUS,license_expiration,Bureau";

const ADHS_FACILITY_FIELDS: FacilityFieldMap = FacilityFieldMap {
    license_number: &["LICENSE_NUMBER", "FACID"],
    name: &["FACILITY_NAME"],
    dba_or_site_name: &[],
    license_type: &["TYPE", "LICENSE_TYPE"],
    status: &["OPERATION_STATUS"],
    street: &["ADDRESS"],
    city: &["CITY"],
    zip: &["ZIP"],
    phone: &["Telephone", "TELEPHONE"],
    licensed_beds: &["Capacity", "CAPACITY"],
    expires_on: &["license_expiration", "LICENSE_EXPIRATION"],
    detail_url: &[],
};

const AZ_SYSTEMS: &[StateBoardDescriptor] = &[
    StateBoardDescriptor {
        id: AZ_ADHS_SYSTEM_ID,
        state: "AZ",
        agency_name: ADHS_AGENCY_NAME,
        system_name: "AZCareCheck & ADHS Licensed Healthcare Facilities (ArcGIS Open Data)",
        scope: LicensingScope::Facility,
        access_mode: LicensingAccessMode::OpenDataApi,
        format: LicensingDatasetFormat::ArcGisFeatureLayer,
        portal_url: ADHS_PORTAL_URL,
        api_or_download_url: Some(ADHS_ARCGIS_LAYER_QUERY_URL),
        covers_licenses: &[],
        captcha_or_login_gated: false,
        generic_endpoint: Some(GenericEndpointSpec::ArcGisFacility {
            agency: ADHS_AGENCY_NAME,
            layer_query_url: ADHS_ARCGIS_LAYER_QUERY_URL,
            where_clause: ADHS_BEHAVIORAL_HEALTH_WHERE,
            page_size: ADHS_PAGE_SIZE,
            max_records: ADHS_MAX_RECORDS,
            verification_url_template: None,
            fields: ADHS_FACILITY_FIELDS,
        }),
        notes: "Public unauthenticated ADHS ArcGIS FeatureServer layer 18 updated monthly from the ADHS Division of Licensing database; covers behavioral health residential, inpatient, outpatient, psychiatric hospitals, substance abuse transitional, counseling, and DUI/DVTX facilities.",
    },
    StateBoardDescriptor {
        id: AZ_MEDICAL_BOARD_ID,
        state: "AZ",
        agency_name: "Arizona Medical Board",
        system_name: "AZMD Doctor Search (MD & PA)",
        scope: LicensingScope::Provider,
        access_mode: LicensingAccessMode::BulkDownloadExport,
        format: LicensingDatasetFormat::LocalCsvImport,
        portal_url: "https://www.azmd.gov/DoctorSearch/DoctorSearch",
        api_or_download_url: None,
        covers_licenses: &[
            StaffLicense::MdPsychiatrist,
            StaffLicense::MdAddiction,
            StaffLicense::MdPhysician,
            StaffLicense::Pa,
        ],
        captcha_or_login_gated: true,
        generic_endpoint: None,
        notes: "Regulates allopathic physicians (MD) separately from DOs (also hosts Arizona Regulatory Board of Physician Assistants lookups). Portal is Cloudflare-protected (HTTP 403 to automated clients); ingest official public-records rosters via --import-file.",
    },
    StateBoardDescriptor {
        id: AZ_OSTEOPATHIC_BOARD_ID,
        state: "AZ",
        agency_name: "Arizona Board of Osteopathic Examiners in Medicine and Surgery",
        system_name: "AZDO Physician Search (DO)",
        scope: LicensingScope::Provider,
        access_mode: LicensingAccessMode::BulkDownloadExport,
        format: LicensingDatasetFormat::LocalCsvImport,
        portal_url: "https://azdo.gov/find-your-do/physician-search",
        api_or_download_url: None,
        covers_licenses: &[
            StaffLicense::DoPsychiatrist,
            StaffLicense::DoAddiction,
            StaffLicense::DoPhysician,
        ],
        captcha_or_login_gated: true,
        generic_endpoint: None,
        notes: "Separate statutory board regulating Doctors of Osteopathic Medicine (DO) in Arizona; strictly separated from the Arizona Medical Board (MD). Both azdo.gov and its Thentia portal return HTTP 403 to automated clients; ingest official rosters via --import-file.",
    },
    StateBoardDescriptor {
        id: AZ_NURSING_BOARD_ID,
        state: "AZ",
        agency_name: "Arizona State Board of Nursing",
        system_name: "AZBN / Nursys QuickConfirm (PMHNP, NP, RN, LPN)",
        scope: LicensingScope::Provider,
        access_mode: LicensingAccessMode::BulkDownloadExport,
        format: LicensingDatasetFormat::LocalCsvImport,
        portal_url: "https://www.nursys.com/LQC/LQCSearch.aspx",
        api_or_download_url: None,
        covers_licenses: &[
            StaffLicense::Pmhnp,
            StaffLicense::Np,
            StaffLicense::Rn,
            StaffLicense::Lpn,
        ],
        captcha_or_login_gated: true,
        generic_endpoint: None,
        notes: "AZBN delegates primary source verification for RN, LPN, and APRN (including NP and PMHNP) licenses to Nursys QuickConfirm; azbn.gov is Cloudflare-protected and Nursys is interactive/bot-gated. Ingest official rosters via --import-file.",
    },
    StateBoardDescriptor {
        id: AZ_BBHE_BOARD_ID,
        state: "AZ",
        agency_name: "Arizona Board of Behavioral Health Examiners",
        system_name: "AZBBHE Directory & Verifications Search (LCSW, LPC, LMFT, LISAC)",
        scope: LicensingScope::Provider,
        access_mode: LicensingAccessMode::BulkDownloadExport,
        format: LicensingDatasetFormat::LocalCsvImport,
        portal_url: "https://bbhe.az.gov/directory-and-verifications-search",
        api_or_download_url: None,
        covers_licenses: &[
            StaffLicense::Lcsw,
            StaffLicense::LpcLmhc,
            StaffLicense::Lmft,
            StaffLicense::AddictionCounselor,
        ],
        captcha_or_login_gated: true,
        generic_endpoint: None,
        notes: "Regulates clinical social workers (LCSW), professional counselors (LPC), marriage and family therapists (LMFT), and substance abuse counselors (LISAC/LASAC). Portal is Cloudflare-protected (HTTP 403); ingest official rosters via --import-file.",
    },
    StateBoardDescriptor {
        id: AZ_PSYCH_BOARD_ID,
        state: "AZ",
        agency_name: "Arizona Board of Psychologist Examiners",
        system_name: "AZBPE Licensee Search Directory (PhD, PsyD)",
        scope: LicensingScope::Provider,
        access_mode: LicensingAccessMode::BulkDownloadExport,
        format: LicensingDatasetFormat::LocalCsvImport,
        portal_url: "https://psychboard.az.gov/directory-licensee-search",
        api_or_download_url: None,
        covers_licenses: &[StaffLicense::PhdPsychologist, StaffLicense::PsydPsychologist],
        captcha_or_login_gated: true,
        generic_endpoint: None,
        notes: "Regulates licensed psychologists (PhD and PsyD) via a Thentia-backed directory on psychboard.az.gov. Portal is Cloudflare-protected (HTTP 403); ingest official rosters via --import-file.",
    },
];

/// Returns the complete licensing manifest for Arizona (`AZ`).
pub fn manifest() -> StateLicensingManifest {
    StateLicensingManifest {
        state: "AZ",
        state_name: "Arizona",
        separate_osteopathic_board: true,
        systems: AZ_SYSTEMS,
    }
}

/// Executes Arizona's live facility licensing adapter (ADHS ArcGIS Open Data) and optional
/// local bulk file import (`--import-file`) for any Arizona facility or provider board.
pub async fn sync(
    client: &reqwest::Client,
    cache: &DiskCache,
    req: &StateSyncRequest,
) -> Result<StateAdapterOutcome, StateLicenseError> {
    let mut outcome = StateAdapterOutcome {
        state: "AZ".to_string(),
        ..Default::default()
    };

    if let Some(import_path) = req.import_file.as_deref() {
        let (system_id, agency_or_board, scope) = resolve_import_target(req);
        let mut imported = import_local_file(import_path, "AZ", agency_or_board, scope)?;
        // Enforce strict AZ board-aware credential classification (especially MD vs DO).
        for rec in &mut imported.provider_records {
            if rec.license.is_none() {
                rec.license = classify_az_provider_license(&rec.board, &rec.credential_label);
            }
        }
        if !imported.systems_executed.iter().any(|s| s == system_id) {
            imported.systems_executed.push(system_id.to_string());
        }
        outcome.merge(imported);
        return Ok(outcome);
    }

    let run_facilities = !req.skip_facilities
        && req
            .system_filter
            .as_deref()
            .is_none_or(|f| f.eq_ignore_ascii_case(AZ_ADHS_SYSTEM_ID));

    if run_facilities {
        let adhs = sync_adhs_arcgis_facilities(client, cache).await?;
        outcome.merge(adhs);
    }

    if !req.skip_providers {
        let matching_provider_boards: Vec<&StateBoardDescriptor> = AZ_SYSTEMS
            .iter()
            .filter(|s| s.scope == LicensingScope::Provider)
            .filter(|s| {
                req.system_filter
                    .as_deref()
                    .is_none_or(|f| f.eq_ignore_ascii_case(s.id))
            })
            .collect();
        if !matching_provider_boards.is_empty() {
            let ids: Vec<&str> = matching_provider_boards.iter().map(|s| s.id).collect();
            outcome.warnings.push(format!(
                "AZ provider boards ({}) are Cloudflare/bot-gated with no unauthenticated bulk API; supply official rosters with --import-file and --system <id>.",
                ids.join(", ")
            ));
        }
    }

    Ok(outcome)
}

fn resolve_import_target(req: &StateSyncRequest) -> (&'static str, &'static str, LicensingScope) {
    if let Some(filter) = req.system_filter.as_deref() {
        if let Some(sys) = AZ_SYSTEMS
            .iter()
            .find(|s| s.id.eq_ignore_ascii_case(filter))
        {
            return (sys.id, sys.agency_name, sys.scope);
        }
    }
    if req.skip_facilities && !req.skip_providers {
        return (
            AZ_MEDICAL_BOARD_ID,
            "Arizona Provider Licensing Import",
            LicensingScope::Provider,
        );
    }
    if req.skip_providers && !req.skip_facilities {
        return (
            AZ_ADHS_SYSTEM_ID,
            ADHS_AGENCY_NAME,
            LicensingScope::Facility,
        );
    }
    (AZ_ADHS_SYSTEM_ID, ADHS_AGENCY_NAME, LicensingScope::Both)
}

async fn sync_adhs_arcgis_facilities(
    client: &reqwest::Client,
    cache: &DiskCache,
) -> Result<StateAdapterOutcome, StateLicenseError> {
    let mut outcome = StateAdapterOutcome {
        state: "AZ".to_string(),
        ..Default::default()
    };
    let mut seen_licenses: HashSet<String> = HashSet::new();
    let mut offset: usize = 0;

    while outcome.facility_records.len() < ADHS_MAX_RECORDS {
        let url = build_adhs_query_url(offset, ADHS_PAGE_SIZE)?;
        let (body, from_cache) = match fetch_cached_text(client, cache, &url).await {
            Ok(pair) => pair,
            Err(err) if cache.mode() == CacheMode::Offline => {
                outcome.warnings.push(format!(
                    "offline mode: cached ADHS ArcGIS page unavailable at offset {offset} ({err})"
                ));
                break;
            }
            Err(err) => return Err(err),
        };

        if from_cache {
            outcome.cache_hits += 1;
        } else {
            outcome.fetched += 1;
        }

        let page = parse_adhs_arcgis_page(&body)?;
        let returned_count = page.records.len();

        for rec in page.records {
            let dedup_key = format!(
                "{}|{}",
                rec.license_number.to_ascii_uppercase(),
                rec.facility_name.to_ascii_uppercase()
            );
            if seen_licenses.insert(dedup_key) {
                outcome.facility_records.push(rec);
                if outcome.facility_records.len() >= ADHS_MAX_RECORDS {
                    break;
                }
            }
        }

        if returned_count == 0 || (!page.exceeded_transfer_limit && returned_count < ADHS_PAGE_SIZE)
        {
            break;
        }
        offset += returned_count;
    }

    outcome.systems_executed.push(AZ_ADHS_SYSTEM_ID.to_string());
    Ok(outcome)
}

fn build_adhs_query_url(offset: usize, page_size: usize) -> Result<String, StateLicenseError> {
    let offset_str = offset.to_string();
    let count_str = page_size.to_string();
    let url = Url::parse_with_params(
        ADHS_ARCGIS_LAYER_QUERY_URL,
        &[
            ("where", ADHS_BEHAVIORAL_HEALTH_WHERE),
            ("outFields", ADHS_OUT_FIELDS),
            ("returnGeometry", "false"),
            ("orderByFields", "OBJECTID ASC"),
            ("resultOffset", offset_str.as_str()),
            ("resultRecordCount", count_str.as_str()),
            ("f", "json"),
        ],
    )
    .map_err(|err| StateLicenseError::InvalidConfig(format!("invalid ADHS ArcGIS URL: {err}")))?;
    Ok(url.into())
}

struct ParsedArcGisPage {
    records: Vec<FacilityLicenseRecord>,
    exceeded_transfer_limit: bool,
}

fn parse_adhs_arcgis_page(body: &str) -> Result<ParsedArcGisPage, StateLicenseError> {
    let root: Value = serde_json::from_str(body).map_err(|source| StateLicenseError::Json {
        source_label: AZ_ADHS_SYSTEM_ID.to_string(),
        source,
    })?;

    if let Some(err_obj) = root.get("error") {
        let msg = err_obj
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("ArcGIS query error");
        return Err(StateLicenseError::InvalidConfig(format!(
            "ADHS ArcGIS error: {msg}"
        )));
    }

    let exceeded_transfer_limit = root
        .get("exceededTransferLimit")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let features = root
        .get("features")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            StateLicenseError::InvalidConfig(
                "ADHS ArcGIS response missing 'features' array".to_string(),
            )
        })?;

    let mut records = Vec::with_capacity(features.len());
    for feature in features {
        let Some(attrs) = feature.get("attributes").and_then(Value::as_object) else {
            continue;
        };

        let facility_name = attr_str(attrs, "FACILITY_NAME").unwrap_or_default();
        if facility_name.is_empty() {
            continue;
        }

        let license_number = attr_str(attrs, "LICENSE_NUMBER")
            .or_else(|| attr_str(attrs, "FACID"))
            .unwrap_or_default();
        if license_number.is_empty() {
            continue;
        }

        let primary_type = attr_str(attrs, "TYPE")
            .or_else(|| attr_str(attrs, "LICENSE_TYPE"))
            .unwrap_or_else(|| "BEHAVIORAL HEALTH FACILITY".to_string());
        let subtype = attr_str(attrs, "SUBTYPE");
        let license_type = format_adhs_license_type(&primary_type, subtype.as_deref());

        let status_raw = attr_str(attrs, "OPERATION_STATUS").unwrap_or_default();
        let status = map_adhs_status(&status_raw);

        let licensed_beds = attrs.get("Capacity").and_then(parse_adhs_capacity);
        let zip = attrs.get("ZIP").and_then(parse_adhs_zip);
        let expires_on = attrs
            .get("license_expiration")
            .and_then(parse_arcgis_epoch_date);

        records.push(FacilityLicenseRecord {
            state: "AZ".to_string(),
            agency: ADHS_AGENCY_NAME.to_string(),
            license_number,
            facility_name,
            dba_name: None,
            license_type,
            status,
            street: attr_str(attrs, "ADDRESS"),
            city: attr_str(attrs, "CITY"),
            zip,
            phone: attr_str(attrs, "Telephone"),
            licensed_beds,
            expires_on,
            disciplinary_actions: None,
            inspection_count: None,
            verification_url: Some(ADHS_PORTAL_URL.to_string()),
        });
    }

    Ok(ParsedArcGisPage {
        records,
        exceeded_transfer_limit,
    })
}

fn attr_str(attrs: &serde_json::Map<String, Value>, key: &str) -> Option<String> {
    let val = attrs.get(key)?;
    match val {
        Value::String(s) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn format_adhs_license_type(primary_type: &str, subtype: Option<&str>) -> String {
    match subtype {
        Some(sub)
            if !sub.is_empty()
                && !sub.eq_ignore_ascii_case(primary_type)
                && !sub.eq_ignore_ascii_case("UNKNOWN")
                && !sub.eq_ignore_ascii_case("UNCLASSIFIED") =>
        {
            format!("{primary_type} ({sub})")
        }
        _ => primary_type.to_string(),
    }
}

/// Parses ADHS `Capacity` values such as `"10.0"`, `"120.0"`, `"0.0"`, or numeric `16`.
/// Returns `None` when capacity is missing, unparseable, or zero (e.g. outpatient clinics).
pub fn parse_adhs_capacity(value: &Value) -> Option<u32> {
    let num = match value {
        Value::Number(n) => n.as_f64()?,
        Value::String(s) => s.trim().parse::<f64>().ok()?,
        _ => return None,
    };
    if !num.is_finite() || num <= 0.0 {
        return None;
    }
    let rounded = num.round();
    if rounded > u32::MAX as f64 {
        return None;
    }
    Some(rounded as u32)
}

fn parse_adhs_zip(value: &Value) -> Option<String> {
    let digits: String = match value {
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.chars().filter(char::is_ascii_digit).collect(),
        _ => return None,
    };
    if digits.len() >= 5 {
        Some(digits[..5].to_string())
    } else {
        None
    }
}

fn parse_arcgis_epoch_date(value: &Value) -> Option<String> {
    match value {
        Value::Number(n) => {
            let millis = n.as_i64()?;
            let dt = chrono::DateTime::from_timestamp_millis(millis)?;
            Some(dt.format("%Y-%m-%d").to_string())
        }
        Value::String(s) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        _ => None,
    }
}

/// Maps ADHS `OPERATION_STATUS` strings to `LicenseStatus`.
pub fn map_adhs_status(raw: &str) -> LicenseStatus {
    LicenseStatus::parse_loose(raw)
}

/// Strictly classifies Arizona provider credentials using both the issuing board name and
/// credential label, preserving Arizona's separate MD (`az-azmd`) vs DO (`az-azdo`) boards.
pub fn classify_az_provider_license(board: &str, credential_label: &str) -> Option<StaffLicense> {
    let board_lower = board.trim().to_ascii_lowercase();
    let cred_lower = credential_label.trim().to_ascii_lowercase();

    // Arizona Board of Osteopathic Examiners strictly licenses DOs, never MDs.
    if board_lower.contains("osteopathic") || board_lower == AZ_OSTEOPATHIC_BOARD_ID {
        if cred_lower.contains("psychiatr") {
            return Some(StaffLicense::DoPsychiatrist);
        }
        if cred_lower.contains("addiction") {
            return Some(StaffLicense::DoAddiction);
        }
        if cred_lower.is_empty()
            || cred_lower.contains("do")
            || cred_lower.contains("osteopath")
            || cred_lower.contains("physician")
        {
            return Some(StaffLicense::DoPhysician);
        }
    }

    // Arizona Medical Board licenses MDs (and PAs under its sister PA board), never DOs.
    if (board_lower.contains("medical board") || board_lower == AZ_MEDICAL_BOARD_ID)
        && !cred_lower.contains("assistant")
        && !cred_lower.contains("pa-c")
    {
        if cred_lower.contains("psychiatr") {
            return Some(StaffLicense::MdPsychiatrist);
        }
        if cred_lower.contains("addiction") {
            return Some(StaffLicense::MdAddiction);
        }
        if cred_lower.contains("md")
            || cred_lower.contains("m.d.")
            || cred_lower.contains("allopathic")
            || cred_lower.contains("physician")
        {
            return Some(StaffLicense::MdPhysician);
        }
    }

    // Arizona Board of Behavioral Health Examiners specific credential codes (LISAC/LASAC).
    if cred_lower.contains("lisac") || cred_lower.contains("lasac") {
        return Some(StaffLicense::AddictionCounselor);
    }

    classify_state_provider_license(board, credential_label)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verified live fixture snippet sampled from ADHS ArcGIS FeatureServer layer 18.
    const ADHS_LIVE_FIXTURE: &str = r#"{
        "exceededTransferLimit": false,
        "features": [
            {
                "attributes": {
                    "FACID": "BH10180",
                    "FACILITY_NAME": "SILVER SANDS RECOVERY LLC",
                    "LICENSE_NUMBER": "BH10180",
                    "TYPE": "BEHAVIORAL HEALTH RESIDENTIAL FACILITY",
                    "SUBTYPE": "BH RESIDENTIAL FACILITY - ADULT",
                    "LICENSE_TYPE": "BEHAVIORAL HEALTH RESIDENTIAL FACILITY",
                    "Capacity": "10.0",
                    "ADDRESS": "545 SOUTH LAKEVIEW DRIVE",
                    "CITY": "PRESCOTT",
                    "ZIP": 86301,
                    "Telephone": "(833)693-1386",
                    "OPERATION_STATUS": "ACTIVE",
                    "license_expiration": null,
                    "Bureau": "BHL"
                }
            },
            {
                "attributes": {
                    "FACID": "BH1453",
                    "FACILITY_NAME": "EAST VALLEY ADDICTION RECOVERY CENTER",
                    "LICENSE_NUMBER": "IFBH6337",
                    "TYPE": "BEHAVIORAL HEALTH INPATIENT FACILITY",
                    "SUBTYPE": "BH INPATIENT FACILITY - SUBACUTE",
                    "LICENSE_TYPE": "BEHAVIORAL HEALTH INPATIENT FACILITY",
                    "Capacity": "16.0",
                    "ADDRESS": "560 SOUTH BELLVIEW, ROOMS B & ",
                    "CITY": "MESA",
                    "ZIP": 85204,
                    "Telephone": "(480)831-7566",
                    "OPERATION_STATUS": "ACTIVE",
                    "license_expiration": 1798675200000,
                    "Bureau": "MED"
                }
            },
            {
                "attributes": {
                    "FACID": "BH10181",
                    "FACILITY_NAME": "HORIZON RECOVERY OUTPATIENT ARROWHEAD",
                    "LICENSE_NUMBER": "OTC13561",
                    "TYPE": "OUTPATIENT TREATMENT CENTER",
                    "SUBTYPE": "OUTPATIENT TREATMENT CENTER",
                    "LICENSE_TYPE": "OUTPATIENT TREATMENT CENTER",
                    "Capacity": "0.0",
                    "ADDRESS": "5940 WEST UNION HILLS DRIVE",
                    "CITY": "GLENDALE",
                    "ZIP": 85308,
                    "Telephone": "(623)693-2198",
                    "OPERATION_STATUS": "ACTIVE",
                    "license_expiration": null,
                    "Bureau": "MED"
                }
            },
            {
                "attributes": {
                    "FACID": "BH7347",
                    "FACILITY_NAME": "VIA LINDA BEHAVIORAL HOSPITAL",
                    "LICENSE_NUMBER": "SH11520",
                    "TYPE": "HOSPITAL",
                    "SUBTYPE": "PSYCHIATRIC",
                    "LICENSE_TYPE": "HOSPITAL",
                    "Capacity": "120.0",
                    "ADDRESS": "9160 EAST HORSESHOE RD",
                    "CITY": "SCOTTSDALE",
                    "ZIP": 85258,
                    "Telephone": "(480)476-7000",
                    "OPERATION_STATUS": "PROBATION",
                    "license_expiration": null,
                    "Bureau": "MED"
                }
            }
        ]
    }"#;

    #[test]
    fn manifest_has_separate_osteopathic_board_and_valid_https_urls() {
        let m = manifest();
        assert_eq!(m.state, "AZ");
        assert_eq!(m.state_name, "Arizona");
        assert!(m.separate_osteopathic_board);
        assert_eq!(m.systems.len(), 6);

        let adhs = m
            .systems
            .iter()
            .find(|s| s.id == AZ_ADHS_SYSTEM_ID)
            .expect("ADHS facility system must exist");
        assert_eq!(adhs.scope, LicensingScope::Facility);
        assert_eq!(adhs.access_mode, LicensingAccessMode::OpenDataApi);
        assert!(!adhs.captcha_or_login_gated);
        assert!(adhs.generic_endpoint.is_some());

        for sys in m.systems {
            assert!(
                sys.portal_url.starts_with("https://"),
                "portal_url must be https: {}",
                sys.portal_url
            );
            if let Some(api_url) = sys.api_or_download_url {
                assert!(
                    api_url.starts_with("https://"),
                    "api_or_download_url must be https: {api_url}"
                );
            }
            if sys.scope == LicensingScope::Provider {
                assert!(sys.captcha_or_login_gated);
                assert!(!sys.covers_licenses.is_empty());
            }
        }
    }

    #[test]
    fn parses_live_adhs_arcgis_fixture_and_extracts_beds_and_subtypes() {
        let page = parse_adhs_arcgis_page(ADHS_LIVE_FIXTURE).expect("fixture should parse");
        assert!(!page.exceeded_transfer_limit);
        assert_eq!(page.records.len(), 4);

        let bhrf = &page.records[0];
        assert_eq!(bhrf.state, "AZ");
        assert_eq!(bhrf.license_number, "BH10180");
        assert_eq!(bhrf.facility_name, "SILVER SANDS RECOVERY LLC");
        assert_eq!(
            bhrf.license_type,
            "BEHAVIORAL HEALTH RESIDENTIAL FACILITY (BH RESIDENTIAL FACILITY - ADULT)"
        );
        assert_eq!(bhrf.licensed_beds, Some(10));
        assert_eq!(bhrf.city.as_deref(), Some("PRESCOTT"));
        assert_eq!(bhrf.zip.as_deref(), Some("86301"));
        assert_eq!(bhrf.status, LicenseStatus::Active);

        let inpatient = &page.records[1];
        assert_eq!(inpatient.license_number, "IFBH6337");
        assert_eq!(inpatient.licensed_beds, Some(16));
        assert_eq!(inpatient.expires_on.as_deref(), Some("2026-12-31"));

        let outpatient = &page.records[2];
        assert_eq!(outpatient.license_number, "OTC13561");
        assert_eq!(outpatient.license_type, "OUTPATIENT TREATMENT CENTER");
        assert_eq!(outpatient.licensed_beds, None);

        let psych_hospital = &page.records[3];
        assert_eq!(psych_hospital.license_number, "SH11520");
        assert_eq!(psych_hospital.license_type, "HOSPITAL (PSYCHIATRIC)");
        assert_eq!(psych_hospital.licensed_beds, Some(120));
        assert_eq!(psych_hospital.status, LicenseStatus::Probation);
    }

    #[test]
    fn maps_adhs_status_strings() {
        assert_eq!(map_adhs_status("ACTIVE"), LicenseStatus::Active);
        assert_eq!(map_adhs_status("Provisional"), LicenseStatus::Provisional);
        assert_eq!(map_adhs_status("Probation"), LicenseStatus::Probation);
        assert_eq!(map_adhs_status("Suspended"), LicenseStatus::Suspended);
        assert_eq!(map_adhs_status("Revoked"), LicenseStatus::Revoked);
        assert_eq!(map_adhs_status("Closed"), LicenseStatus::Inactive);
        assert_eq!(map_adhs_status(""), LicenseStatus::Unknown);
    }

    #[test]
    fn parses_adhs_capacity_edge_cases() {
        assert_eq!(parse_adhs_capacity(&Value::String("10.0".into())), Some(10));
        assert_eq!(parse_adhs_capacity(&Value::String("0.0".into())), None);
        assert_eq!(parse_adhs_capacity(&Value::String("".into())), None);
        assert_eq!(parse_adhs_capacity(&serde_json::json!(24)), Some(24));
        assert_eq!(parse_adhs_capacity(&Value::Null), None);
    }

    #[test]
    fn strictly_separates_az_md_and_do_boards() {
        assert_eq!(
            classify_az_provider_license("Arizona Medical Board", "MD"),
            Some(StaffLicense::MdPhysician)
        );
        assert_eq!(
            classify_az_provider_license("Arizona Medical Board", "MD - Psychiatry"),
            Some(StaffLicense::MdPsychiatrist)
        );
        assert_eq!(
            classify_az_provider_license(
                "Arizona Board of Osteopathic Examiners in Medicine and Surgery",
                "DO"
            ),
            Some(StaffLicense::DoPhysician)
        );
        assert_eq!(
            classify_az_provider_license(
                "Arizona Board of Osteopathic Examiners in Medicine and Surgery",
                "DO - Psychiatry"
            ),
            Some(StaffLicense::DoPsychiatrist)
        );
        assert_eq!(
            classify_az_provider_license("Arizona Board of Behavioral Health Examiners", "LISAC"),
            Some(StaffLicense::AddictionCounselor)
        );
        assert_ne!(
            classify_az_provider_license("Arizona Medical Board", "Physician"),
            classify_az_provider_license(
                "Arizona Board of Osteopathic Examiners in Medicine and Surgery",
                "Physician"
            )
        );
    }

    #[test]
    fn builds_adhs_query_url_with_params() {
        let url = build_adhs_query_url(1000, 1000).expect("URL should build");
        assert!(url.starts_with(ADHS_ARCGIS_LAYER_QUERY_URL));
        assert!(url.contains("resultOffset=1000"));
        assert!(url.contains("resultRecordCount=1000"));
        assert!(url.contains("f=json"));
    }
}
