//! New York (`NY`) behavioral health facility and clinical provider licensing adapter.
//!
//! Verified live public data sources:
//! 1. **NYS OASAS** Substance Use Disorder Treatment Program Directory:
//!    Live unauthenticated CSV export via `POST https://webapps.oasas.ny.gov/providerDirectory/index.cfm`
//!    (`search_type=2&region_type=0&rstformat=1&btnSearch=Search`, redirecting to the dated CSV file,
//!    ~900 certified programs). Legacy `data.ny.gov` Socrata dataset `ngbt-9u94` returns HTTP 404.
//! 2. **NYS OMH** Local Mental Health Programs (`data.ny.gov` Socrata `6nvr-tbv8`):
//!    Live SODA JSON API (`https://data.ny.gov/resource/6nvr-tbv8.json`, 6,337 total rows; 1,620
//!    certified Article 31 programs with `operating_certificate_required = 'Y'`).
//! 3. **NYS DOH** Health Facility General Information (`health.data.ny.gov` Socrata `vn5v-hh5r`):
//!    Live SODA JSON API (`https://health.data.ny.gov/resource/vn5v-hh5r.json`, 5,978 total facilities;
//!    2,850 hospitals, hospital extension clinics, and diagnostic & treatment centers).
//! 4. **NYS DOH** Health Facility Certification Information (`health.data.ny.gov` Socrata `2g9y-7kqm`):
//!    Live SODA JSON API (`https://health.data.ny.gov/resource/2g9y-7kqm.json`, 45,258 total rows;
//!    151 certified Psychiatric and Chemical Dependence Detox/Rehab hospital bed records).
//! 5. **NYSED Office of the Professions** (`eservices.nysed.gov/professions/verification-search`):
//!    Unified licensing authority for NY physicians (both MD and DO under State Board for Medicine),
//!    NPs/PMHNPs, PAs, psychologists, LCSWs, LMHCs, LMFTs, RNs, and LPNs. Session/anti-bot gated SPA
//!    with no public bulk Open Data download; modeled honestly as `PortalLookupOnly` + `--import-file`.
//! 6. **NYS OASAS Credentialing Portal** (`credentialing.oasas.ny.gov/applicant-search`):
//!    Official verification portal for CASAC / CASAC-T addiction counselors and CRPA peer advocates.
//!    No public bulk Open Data dataset on `data.ny.gov`; modeled as `PortalLookupOnly` + `--import-file`.

use std::time::Duration;

use crate::schema::{LicenseStatus, StaffLicense};
use crate::updater::cache::{CacheMode, DiskCache};

use super::generic::{detect_captcha_or_block, execute_generic_endpoint, import_local_file};
use super::types::{
    FacilityFieldMap, FacilityLicenseRecord, GenericEndpointSpec, LicensingAccessMode,
    LicensingDatasetFormat, LicensingScope, StateAdapterOutcome, StateBoardDescriptor,
    StateLicenseError, StateLicensingManifest, StateSyncRequest,
};

const STATE_CODE: &str = "NY";
const CACHE_NAMESPACE: &str = "state-licensing";
const CACHE_TTL: Duration = Duration::from_secs(14 * 86_400);

const OASAS_DIRECTORY_URL: &str = "https://webapps.oasas.ny.gov/providerDirectory/index.cfm";
const OASAS_STATEWIDE_CSV_FORM_BODY: &str =
    "search_type=2&region_type=0&rstformat=1&btnSearch=Search";
const OASAS_CACHE_KEY: &str = "ny-oasas-treatment:statewide-csv:v1";

const OMH_SOCRATA_URL: &str = "https://data.ny.gov/resource/6nvr-tbv8.json";
const DOH_FACILITIES_SOCRATA_URL: &str = "https://health.data.ny.gov/resource/vn5v-hh5r.json";
const DOH_CERTIFICATIONS_SOCRATA_URL: &str = "https://health.data.ny.gov/resource/2g9y-7kqm.json";

const NYSED_OP_LICENSES: &[StaffLicense] = &[
    StaffLicense::MdPsychiatrist,
    StaffLicense::DoPsychiatrist,
    StaffLicense::MdAddiction,
    StaffLicense::DoAddiction,
    StaffLicense::MdPhysician,
    StaffLicense::DoPhysician,
    StaffLicense::Pmhnp,
    StaffLicense::Np,
    StaffLicense::Pa,
    StaffLicense::PhdPsychologist,
    StaffLicense::PsydPsychologist,
    StaffLicense::Lcsw,
    StaffLicense::LpcLmhc,
    StaffLicense::Lmft,
    StaffLicense::Rn,
    StaffLicense::Lpn,
];

const OASAS_CREDENTIAL_LICENSES: &[StaffLicense] =
    &[StaffLicense::AddictionCounselor, StaffLicense::PeerSupport];

static NY_SYSTEMS: &[StateBoardDescriptor] = &[
    StateBoardDescriptor {
        id: "ny-oasas-treatment",
        state: STATE_CODE,
        agency_name: "NYS Office of Addiction Services and Supports (OASAS)",
        system_name: "OASAS Substance Use Disorder Treatment Program Directory",
        scope: LicensingScope::Facility,
        access_mode: LicensingAccessMode::DirectSearchApi,
        format: LicensingDatasetFormat::CsvUrl,
        portal_url: OASAS_DIRECTORY_URL,
        api_or_download_url: Some(OASAS_DIRECTORY_URL),
        covers_licenses: &[],
        captcha_or_login_gated: false,
        generic_endpoint: None,
        notes: "Official OASAS directory of certified substance use disorder treatment programs (~900 rows). Legacy data.ny.gov dataset ngbt-9u94 returns 404; this adapter fetches the official statewide CSV export directly from webapps.oasas.ny.gov/providerDirectory/index.cfm.",
    },
    StateBoardDescriptor {
        id: "ny-omh-programs",
        state: STATE_CODE,
        agency_name: "NYS Office of Mental Health (OMH)",
        system_name: "OMH Local Mental Health Programs (data.ny.gov 6nvr-tbv8)",
        scope: LicensingScope::Facility,
        access_mode: LicensingAccessMode::OpenDataApi,
        format: LicensingDatasetFormat::SocrataJson,
        portal_url: "https://data.ny.gov/Human-Services/Local-Mental-Health-Programs/6nvr-tbv8",
        api_or_download_url: Some(OMH_SOCRATA_URL),
        covers_licenses: &[],
        captcha_or_login_gated: false,
        generic_endpoint: Some(GenericEndpointSpec::SocrataFacility {
            agency: "NYS Office of Mental Health (OMH)",
            endpoint_url: OMH_SOCRATA_URL,
            where_clause: Some("operating_certificate_required = 'Y'"),
            page_size: 1000,
            max_records: 5000,
            verification_url_template: Some("https://my.omh.ny.gov/bi/pd/saw.dll?PortalPages"),
            fields: FacilityFieldMap {
                license_number: &["program_code", "facility_code", "agency_code"],
                name: &["facility_name", "agency_name", "sponsor_name"],
                dba_or_site_name: &["program_name"],
                license_type: &[
                    "program_type_description",
                    "program_category_description",
                    "program_subcategory_description",
                ],
                status: &[],
                street: &["program_address_1"],
                city: &["program_city"],
                zip: &["program_zip"],
                phone: &["program_phone", "agency_phone"],
                licensed_beds: &[],
                expires_on: &[],
                detail_url: &[],
            },
        }),
        notes: "Verified live Socrata SODA endpoint on data.ny.gov (6nvr-tbv8, 6,337 total rows; filtered to 1,620 certified Article 31 outpatient, residential, inpatient, and crisis programs requiring an operating certificate).",
    },
    StateBoardDescriptor {
        id: "ny-doh-facilities",
        state: STATE_CODE,
        agency_name: "NYS Department of Health (NYS DOH)",
        system_name: "Health Facility General Information (health.data.ny.gov vn5v-hh5r)",
        scope: LicensingScope::Facility,
        access_mode: LicensingAccessMode::OpenDataApi,
        format: LicensingDatasetFormat::SocrataJson,
        portal_url: "https://health.data.ny.gov/Health/Health-Facility-General-Information/vn5v-hh5r",
        api_or_download_url: Some(DOH_FACILITIES_SOCRATA_URL),
        covers_licenses: &[],
        captcha_or_login_gated: false,
        generic_endpoint: Some(GenericEndpointSpec::SocrataFacility {
            agency: "NYS Department of Health (NYS DOH)",
            endpoint_url: DOH_FACILITIES_SOCRATA_URL,
            where_clause: Some("fac_desc_short IN ('HOSP','HOSP-EC','DTC','DTC-EC')"),
            page_size: 1000,
            max_records: 5000,
            verification_url_template: Some("https://profiles.health.ny.gov/"),
            fields: FacilityFieldMap {
                license_number: &["opcert_num", "fac_id"],
                name: &["facility_name", "operator_name"],
                dba_or_site_name: &["main_site_name", "operator_name"],
                license_type: &["description", "fac_desc_short"],
                status: &[],
                street: &["address1"],
                city: &["city"],
                zip: &["fac_zip"],
                phone: &["fac_phone"],
                licensed_beds: &[],
                expires_on: &[],
                detail_url: &[],
            },
        }),
        notes: "Verified live Socrata SODA endpoint on health.data.ny.gov (vn5v-hh5r, 5,978 total facilities; filtered to 2,850 hospitals, hospital extension clinics, and diagnostic & treatment centers including psychiatric hospital units).",
    },
    StateBoardDescriptor {
        id: "ny-doh-certifications",
        state: STATE_CODE,
        agency_name: "NYS Department of Health (NYS DOH)",
        system_name: "Health Facility Certification Information - Behavioral Health Beds (health.data.ny.gov 2g9y-7kqm)",
        scope: LicensingScope::Facility,
        access_mode: LicensingAccessMode::OpenDataApi,
        format: LicensingDatasetFormat::SocrataJson,
        portal_url: "https://health.data.ny.gov/Health/Health-Facility-Certification-Information/2g9y-7kqm",
        api_or_download_url: Some(DOH_CERTIFICATIONS_SOCRATA_URL),
        covers_licenses: &[],
        captcha_or_login_gated: false,
        generic_endpoint: Some(GenericEndpointSpec::SocrataFacility {
            agency: "NYS Department of Health (NYS DOH)",
            endpoint_url: DOH_CERTIFICATIONS_SOCRATA_URL,
            where_clause: Some(
                "attribute_type = 'Bed' AND (upper(attribute_value) LIKE '%PSYCH%' OR upper(attribute_value) LIKE '%CHEM%')",
            ),
            page_size: 1000,
            max_records: 2000,
            verification_url_template: Some("https://profiles.health.ny.gov/"),
            fields: FacilityFieldMap {
                license_number: &["fac_id"],
                name: &["primary_nam"],
                dba_or_site_name: &[],
                license_type: &["attribute_value", "description"],
                status: &["sub_type"],
                street: &[],
                city: &[],
                zip: &[],
                phone: &[],
                licensed_beds: &["measure_value"],
                expires_on: &[],
                detail_url: &[],
            },
        }),
        notes: "Verified live Socrata SODA endpoint on health.data.ny.gov (2g9y-7kqm, 45,258 total rows; filtered to 151 certified Psychiatric and Chemical Dependence Detox/Rehab bed certifications with bed counts in measure_value).",
    },
    StateBoardDescriptor {
        id: "ny-nysed-op",
        state: STATE_CODE,
        agency_name: "NYS Education Department Office of the Professions (NYSED OP)",
        system_name: "NYSED Office of the Professions Verification Search",
        scope: LicensingScope::Provider,
        access_mode: LicensingAccessMode::PortalLookupOnly,
        format: LicensingDatasetFormat::PortalWebOnly,
        portal_url: "https://eservices.nysed.gov/professions/verification-search",
        api_or_download_url: None,
        covers_licenses: NYSED_OP_LICENSES,
        captcha_or_login_gated: true,
        generic_endpoint: None,
        notes: "Unified NY licensing authority for physicians (both MD and DO under State Board for Medicine), NPs/PMHNPs, PAs, psychologists, LCSWs, LMHCs, LMFTs, RNs, and LPNs. Verification search at op.nysed.gov redirects to an anti-bot/session-gated SPA on eservices.nysed.gov with no public bulk Open Data download on data.ny.gov; use the official verification portal or --import-file.",
    },
    StateBoardDescriptor {
        id: "ny-oasas-casac",
        state: STATE_CODE,
        agency_name: "NYS Office of Addiction Services and Supports (OASAS)",
        system_name: "OASAS Credentialing Portal Applicant Search (CASAC / CRPA)",
        scope: LicensingScope::Provider,
        access_mode: LicensingAccessMode::PortalLookupOnly,
        format: LicensingDatasetFormat::PortalWebOnly,
        portal_url: "https://credentialing.oasas.ny.gov/applicant-search",
        api_or_download_url: None,
        covers_licenses: OASAS_CREDENTIAL_LICENSES,
        captcha_or_login_gated: false,
        generic_endpoint: None,
        notes: "Official OASAS verification portal for Credentialed Alcoholism and Substance Abuse Counselors (CASAC, CASAC-T), CPP/CPS, and Certified Recovery Peer Advocates (CRPA). No public bulk Open Data dataset exists on data.ny.gov; verify credentials online or import a local export via --import-file.",
    },
];

/// Returns the static New York facility and provider licensing manifest.
pub fn manifest() -> StateLicensingManifest {
    StateLicensingManifest {
        state: STATE_CODE,
        state_name: "New York",
        separate_osteopathic_board: false,
        systems: NY_SYSTEMS,
    }
}

/// Executes New York's verified live licensing endpoints or imports a local export file.
pub async fn sync(
    client: &reqwest::Client,
    cache: &DiskCache,
    req: &StateSyncRequest,
) -> Result<StateAdapterOutcome, StateLicenseError> {
    if let Some(path) = req.import_file.as_deref() {
        let target = select_import_target(req);
        let scope = if req.skip_facilities && !req.skip_providers {
            LicensingScope::Provider
        } else if req.skip_providers && !req.skip_facilities {
            LicensingScope::Facility
        } else {
            target.scope
        };
        return import_local_file(path, STATE_CODE, target.agency_name, scope);
    }

    let mut combined = StateAdapterOutcome {
        state: STATE_CODE.to_string(),
        ..StateAdapterOutcome::default()
    };

    let filter = req
        .system_filter
        .as_deref()
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty());

    for sys in NY_SYSTEMS {
        if let Some(ref wanted) = filter {
            if sys.id != wanted {
                continue;
            }
        }
        if req.skip_facilities && sys.scope == LicensingScope::Facility {
            continue;
        }
        if req.skip_providers && sys.scope == LicensingScope::Provider {
            continue;
        }

        if sys.id == "ny-oasas-treatment" {
            match sync_oasas_treatment_directory(client, cache).await {
                Ok(part) => combined.merge(part),
                Err(err @ StateLicenseError::CaptchaBlocked { .. }) => return Err(err),
                Err(err) if filter.is_some() => return Err(err),
                Err(err) => combined.warnings.push(format!("{}: {err}", sys.id)),
            }
            continue;
        }

        if let Some(ref spec) = sys.generic_endpoint {
            match execute_generic_endpoint(client, cache, STATE_CODE, sys.id, spec).await {
                Ok(mut part) => {
                    // OMH (operating_certificate_required='Y') and NYS DOH active directories
                    // omit a status column because every published row is an active operating
                    // certificate; default Unknown to Active rather than displaying Unknown.
                    if matches!(
                        sys.id,
                        "ny-omh-programs" | "ny-doh-facilities" | "ny-doh-certifications"
                    ) {
                        for rec in &mut part.facility_records {
                            if rec.status == LicenseStatus::Unknown {
                                rec.status = LicenseStatus::Active;
                            }
                        }
                    }
                    combined.merge(part);
                }
                Err(err @ StateLicenseError::CaptchaBlocked { .. }) => return Err(err),
                Err(err) if filter.is_some() => return Err(err),
                Err(err) => combined.warnings.push(format!("{}: {err}", sys.id)),
            }
            continue;
        }

        combined.warnings.push(format!(
            "{} ({}) is portal-lookup-only without a public bulk API; verify at {} or pass --import-file.",
            sys.id, sys.agency_name, sys.portal_url
        ));
    }

    // Join NYS DOH psychiatric / chemical-dependence bed certification counts (2g9y-7kqm,
    // which has no street/city/zip) onto matching NYS DOH facility rows (vn5v-hh5r) by
    // normalized facility name so hospital bed counts carry through to catalog matching.
    attach_doh_certified_beds(&mut combined.facility_records);

    Ok(combined)
}

fn attach_doh_certified_beds(records: &mut Vec<FacilityLicenseRecord>) {
    let mut beds_by_name: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    for rec in records.iter() {
        if rec.street.is_none() && rec.city.is_none() && rec.zip.is_none() {
            if let Some(beds) = rec.licensed_beds.filter(|&b| b > 0) {
                let key = rec.facility_name.trim().to_ascii_lowercase();
                if !key.is_empty() {
                    *beds_by_name.entry(key).or_insert(0) += beds;
                }
            }
        }
    }
    if beds_by_name.is_empty() {
        return;
    }
    for rec in records.iter_mut() {
        if rec.agency == "NYS Department of Health (NYS DOH)"
            && (rec.street.is_some() || rec.zip.is_some())
            && rec.licensed_beds.is_none()
        {
            let key = rec.facility_name.trim().to_ascii_lowercase();
            if let Some(&beds) = beds_by_name.get(&key) {
                rec.licensed_beds = Some(beds);
            }
        }
    }
    // Drop addressless bed-only rows once folded onto their parent DOH facilities.
    records.retain(|rec| {
        !(rec.agency == "NYS Department of Health (NYS DOH)"
            && rec.street.is_none()
            && rec.city.is_none()
            && rec.zip.is_none())
    });
}

fn select_import_target(req: &StateSyncRequest) -> &'static StateBoardDescriptor {
    if let Some(ref filter) = req.system_filter {
        let wanted = filter.trim().to_ascii_lowercase();
        if let Some(found) = NY_SYSTEMS.iter().find(|s| s.id == wanted) {
            return found;
        }
    }
    if req.skip_facilities && !req.skip_providers {
        NY_SYSTEMS
            .iter()
            .find(|s| s.id == "ny-nysed-op")
            .unwrap_or(&NY_SYSTEMS[0])
    } else {
        &NY_SYSTEMS[0]
    }
}

async fn sync_oasas_treatment_directory(
    client: &reqwest::Client,
    cache: &DiskCache,
) -> Result<StateAdapterOutcome, StateLicenseError> {
    let (csv_text, from_cache) = fetch_oasas_csv_cached(client, cache).await?;
    let records = parse_oasas_treatment_csv(&csv_text)?;

    Ok(StateAdapterOutcome {
        state: STATE_CODE.to_string(),
        facility_records: records,
        provider_records: Vec::new(),
        cache_hits: usize::from(from_cache),
        fetched: usize::from(!from_cache),
        systems_executed: vec!["ny-oasas-treatment".to_string()],
        warnings: Vec::new(),
    })
}

async fn fetch_oasas_csv_cached(
    client: &reqwest::Client,
    cache: &DiskCache,
) -> Result<(String, bool), StateLicenseError> {
    if let Some(env) = cache.get::<String>(CACHE_NAMESPACE, OASAS_CACHE_KEY, CACHE_TTL) {
        return Ok((env.value, true));
    }

    if cache.mode() == CacheMode::Offline {
        return Err(StateLicenseError::InvalidConfig(format!(
            "offline cache miss for {OASAS_DIRECTORY_URL}"
        )));
    }

    let response = client
        .post(OASAS_DIRECTORY_URL)
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        )
        .body(OASAS_STATEWIDE_CSV_FORM_BODY)
        .send()
        .await;

    match response {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let final_url = resp.url().to_string();
            let body = resp
                .text()
                .await
                .map_err(|source| StateLicenseError::Http {
                    url: final_url.clone(),
                    source,
                })?;

            if detect_captcha_or_block(status, &body) {
                return Err(StateLicenseError::CaptchaBlocked { url: final_url });
            }

            if !(200..300).contains(&status) {
                if let Some(stale) = cache.get_stale::<String>(CACHE_NAMESPACE, OASAS_CACHE_KEY) {
                    return Ok((stale.value, true));
                }
                return Err(StateLicenseError::HttpStatus {
                    url: final_url,
                    status,
                });
            }

            let _ = cache.put(CACHE_NAMESPACE, OASAS_CACHE_KEY, &body, None, None);
            Ok((body, false))
        }
        Err(source) => {
            if let Some(stale) = cache.get_stale::<String>(CACHE_NAMESPACE, OASAS_CACHE_KEY) {
                return Ok((stale.value, true));
            }
            Err(StateLicenseError::Http {
                url: OASAS_DIRECTORY_URL.to_string(),
                source,
            })
        }
    }
}

/// Parses the official OASAS Treatment Program Directory CSV export into normalized records.
pub fn parse_oasas_treatment_csv(
    csv_text: &str,
) -> Result<Vec<FacilityLicenseRecord>, StateLicenseError> {
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(csv_text.as_bytes());

    let headers = reader
        .headers()
        .map_err(|source| StateLicenseError::Csv {
            source_label: "ny-oasas-treatment".to_string(),
            source,
        })?
        .clone();

    let idx = |name: &str| -> Option<usize> {
        headers
            .iter()
            .position(|h| h.trim().eq_ignore_ascii_case(name))
    };

    let col_provider_name = idx("PROVIDER_NAME");
    let col_provider_num = idx("PROVIDER_NUMBER");
    let col_provider_street = idx("PROVIDER_STREET");
    let col_provider_city = idx("PROVIDER_CITY");
    let col_provider_zip = idx("PROVIDER_ZIP_CODE");
    let col_program_name = idx("PROGRAM_NAME");
    let col_program_num = idx("PROGRAM_NUMBER");
    let col_program_street = idx("PROGRAM_STREET_ADDRESS");
    let col_program_city = idx("PROGRAM_CITY");
    let col_program_zip = idx("PROGRAM_ZIP_CODE");
    let col_program_type = idx("PROGRAM_TYPE");
    let col_service_type = idx("SERVICE_TYPE");
    let col_admission_phone = idx("ADMISSION_PHONE");
    let col_director_phone = idx("PROGRAM_DIRECTOR_TELEPHONE");

    let mut out = Vec::new();
    for row in reader.records() {
        let record = row.map_err(|source| StateLicenseError::Csv {
            source_label: "ny-oasas-treatment".to_string(),
            source,
        })?;

        let field = |col: Option<usize>| -> Option<String> {
            col.and_then(|i| record.get(i))
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        };

        let license_number = field(col_program_num).or_else(|| field(col_provider_num));
        let provider_name = field(col_provider_name);
        let program_name = field(col_program_name);

        let Some(license_number) = license_number else {
            continue;
        };
        let Some(facility_name) = provider_name.clone().or_else(|| program_name.clone()) else {
            continue;
        };

        let dba_name = program_name.filter(|p| !p.eq_ignore_ascii_case(&facility_name));

        let license_type = match (field(col_program_type), field(col_service_type)) {
            (Some(pt), Some(st)) if !pt.eq_ignore_ascii_case(&st) => format!("{pt} - {st}"),
            (Some(pt), _) => pt,
            (None, Some(st)) => st,
            (None, None) => "OASAS Certified SUD Treatment Program".to_string(),
        };

        let street = field(col_program_street).or_else(|| field(col_provider_street));
        let city = field(col_program_city).or_else(|| field(col_provider_city));
        let zip = field(col_program_zip).or_else(|| field(col_provider_zip));
        let phone = field(col_admission_phone).or_else(|| field(col_director_phone));

        out.push(FacilityLicenseRecord {
            state: STATE_CODE.to_string(),
            agency: "NYS Office of Addiction Services and Supports (OASAS)".to_string(),
            license_number,
            facility_name,
            dba_name,
            license_type,
            status: LicenseStatus::Active,
            street,
            city,
            zip,
            phone,
            licensed_beds: None,
            expires_on: None,
            disciplinary_actions: None,
            inspection_count: None,
            verification_url: Some(OASAS_DIRECTORY_URL.to_string()),
        });
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const OASAS_FIXTURE_CSV: &str = r#""PROVIDER_NAME","PROVIDER_NUMBER","PROVIDER_STREET","PROVIDER_CITY","PROVIDER_COUNTY","PROVIDER_STATE","PROVIDER_ZIP_CODE","PROVIDER_ZIP_CODE_PLUS_4","PROGRAM_NAME","PROGRAM_NUMBER","PROGRAM_COUNTY","PROGRAM_STREET_ADDRESS","PROGRAM_CITY","PROGRAM_STATE","PROGRAM_ZIP_CODE","PROGRAM_ZIP_CODE_PLUS_4","PROGRAM_DIRECTOR","PROGRAM_DIRECTOR_EMAIL","PROGRAM_DIRECTOR_TELEPHONE","PROGRAM_DIRECTOR_PHONE_EXTENSION","PROGRAM_TYPE","SERVICE_TYPE","ADMISSION_PHONE","ADMISSION_PHONE_EXT","TELEHEALTH_SERVICES","ADOLESCENT_DESIGNATION","PROBLEM_GAMBLING","INTEGRATED_LICENSE_SERVICE","PARENTS_WITH_CHILDREN","LGBTQ_ENDORSEMNT","ADDITIONAL_LOCATION_STREET","ADDITIONAL_LOCATION_CITY","ADDITIONAL_LOCATION_ZIP","ADDITIONAL_LOCATION_COUNTYNAME"
"820 River St., Inc.","35210","950 New Loudon Road Suite 220","Latham","Albany","NY","12110","","Eleanor R.Young Memorial Clinc OP","51423","Albany","134 Franklin St","Albany","NY","12202","","Mr. Gary Lunderman CASAC","Gary.Lunderman@pyhit.org","5184658034","","Outpatient Services","Part 822 Outpatient Program","5184658034","","Y","","","","","","","","",""
"Addictions Care Center of Albany, Inc.","35090","73 Troy Road","East Greenbush","Albany","NY","12061","","McCarty Avenue Men's Stabl/Rehab SRh","53459","Albany","90 McCarty Ave","Albany","NY","12202","","Yusef Benjamin CASAC","ybenjamin@theacca.net","5184342367","1901","Residential Services","820 Stabil and Rehab","","","Y","","","","","","","","",""
"#;

    const OMH_FIXTURE_JSON: &str = r#"[
      {
        "sponsor_name": "Project Renewal Inc.",
        "sponsor_code": "528099",
        "agency_name": "Project Renewal Inc.",
        "agency_code": "50470",
        "facility_name": "Project Renewal Inc.",
        "facility_code": "6179",
        "program_name": "Fletcher Residence",
        "program_code": "014",
        "populations_served": "Adults",
        "agency_phone": "(212)620-0340",
        "program_phone": "Not Available",
        "program_state": "NY",
        "operating_certificate_required": "Y",
        "operating_certificate_duration": "36",
        "program_county": "Bronx",
        "program_region": "New York City",
        "program_type_description": "SRO Community Residence",
        "program_category_description": "Residential",
        "program_subcategory_description": "Treatment Program"
      }
    ]"#;

    const DOH_GENERAL_FIXTURE_JSON: &str = r#"[
      {
        "fac_id": "9700",
        "facility_name": "Northwell Greenwich Village Hospital",
        "fac_desc_short": "HOSP",
        "description": "Hospital",
        "address1": "30 Seventh Avenue",
        "city": "New York",
        "state": "New York",
        "fac_zip": "10011",
        "fac_phone": "5164658018",
        "main_site_name": "Lenox Hill Hospital",
        "opcert_num": "7002017H",
        "operator_name": "Lenox Hill Hospital"
      }
    ]"#;

    const DOH_CERT_FIXTURE_JSON: &str = r#"[
      {
        "fac_id": "1",
        "primary_nam": "Albany Medical Center Hospital",
        "description": "Hospital",
        "fac_desc_short": "HOSP",
        "attribute_type": "Bed",
        "attribute_value": "Psychiatric",
        "measure_value": "26",
        "sub_type": "Permanent",
        "county": "Albany"
      }
    ]"#;

    #[test]
    fn manifest_declares_ny_facility_and_provider_systems() {
        let m = manifest();
        assert_eq!(m.state, "NY");
        assert_eq!(m.state_name, "New York");
        assert!(!m.separate_osteopathic_board);
        assert_eq!(m.systems.len(), 6);

        for sys in m.systems {
            assert_eq!(sys.state, "NY");
            assert!(sys.portal_url.starts_with("https://"));
        }

        let nysed = m
            .systems
            .iter()
            .find(|s| s.id == "ny-nysed-op")
            .expect("ny-nysed-op present");
        assert!(nysed.captcha_or_login_gated);
        assert_eq!(nysed.access_mode, LicensingAccessMode::PortalLookupOnly);
        assert!(nysed
            .covers_licenses
            .contains(&StaffLicense::MdPsychiatrist));
        assert!(nysed
            .covers_licenses
            .contains(&StaffLicense::DoPsychiatrist));
        assert!(nysed.covers_licenses.contains(&StaffLicense::Pmhnp));
        assert!(nysed.covers_licenses.contains(&StaffLicense::Lcsw));
        assert!(nysed.covers_licenses.contains(&StaffLicense::LpcLmhc));

        let casac = m
            .systems
            .iter()
            .find(|s| s.id == "ny-oasas-casac")
            .expect("ny-oasas-casac present");
        assert!(casac
            .covers_licenses
            .contains(&StaffLicense::AddictionCounselor));
        assert!(casac.covers_licenses.contains(&StaffLicense::PeerSupport));
    }

    #[test]
    fn parse_oasas_csv_extracts_certified_programs() {
        let records = parse_oasas_treatment_csv(OASAS_FIXTURE_CSV).expect("valid CSV");
        assert_eq!(records.len(), 2);

        assert_eq!(records[0].license_number, "51423");
        assert_eq!(records[0].facility_name, "820 River St., Inc.");
        assert_eq!(
            records[0].dba_name.as_deref(),
            Some("Eleanor R.Young Memorial Clinc OP")
        );
        assert_eq!(
            records[0].license_type,
            "Outpatient Services - Part 822 Outpatient Program"
        );
        assert_eq!(records[0].street.as_deref(), Some("134 Franklin St"));
        assert_eq!(records[0].city.as_deref(), Some("Albany"));
        assert_eq!(records[0].zip.as_deref(), Some("12202"));
        assert_eq!(records[0].phone.as_deref(), Some("5184658034"));
        assert_eq!(records[0].status, LicenseStatus::Active);

        // Second row has empty ADMISSION_PHONE and falls back to PROGRAM_DIRECTOR_TELEPHONE.
        assert_eq!(records[1].license_number, "53459");
        assert_eq!(records[1].phone.as_deref(), Some("5184342367"));
        assert_eq!(
            records[1].license_type,
            "Residential Services - 820 Stabil and Rehab"
        );
    }

    #[test]
    fn socrata_field_maps_match_live_omh_and_doh_fixtures() {
        let omh_rows: Vec<serde_json::Value> =
            serde_json::from_str(OMH_FIXTURE_JSON).expect("valid OMH JSON");
        let doh_gen_rows: Vec<serde_json::Value> =
            serde_json::from_str(DOH_GENERAL_FIXTURE_JSON).expect("valid DOH general JSON");
        let doh_cert_rows: Vec<serde_json::Value> =
            serde_json::from_str(DOH_CERT_FIXTURE_JSON).expect("valid DOH cert JSON");

        let omh_sys = NY_SYSTEMS
            .iter()
            .find(|s| s.id == "ny-omh-programs")
            .unwrap();
        let Some(GenericEndpointSpec::SocrataFacility { fields: omh_f, .. }) =
            &omh_sys.generic_endpoint
        else {
            panic!("expected SocrataFacility for ny-omh-programs");
        };
        assert_eq!(omh_rows[0][omh_f.license_number[0]].as_str(), Some("014"));
        assert_eq!(
            omh_rows[0][omh_f.name[0]].as_str(),
            Some("Project Renewal Inc.")
        );
        assert_eq!(
            omh_rows[0][omh_f.dba_or_site_name[0]].as_str(),
            Some("Fletcher Residence")
        );

        let doh_gen_sys = NY_SYSTEMS
            .iter()
            .find(|s| s.id == "ny-doh-facilities")
            .unwrap();
        let Some(GenericEndpointSpec::SocrataFacility {
            fields: doh_gen_f, ..
        }) = &doh_gen_sys.generic_endpoint
        else {
            panic!("expected SocrataFacility for ny-doh-facilities");
        };
        assert_eq!(
            doh_gen_rows[0][doh_gen_f.license_number[0]].as_str(),
            Some("7002017H")
        );
        assert_eq!(
            doh_gen_rows[0][doh_gen_f.name[0]].as_str(),
            Some("Northwell Greenwich Village Hospital")
        );
        assert_eq!(doh_gen_rows[0][doh_gen_f.zip[0]].as_str(), Some("10011"));

        let doh_cert_sys = NY_SYSTEMS
            .iter()
            .find(|s| s.id == "ny-doh-certifications")
            .unwrap();
        let Some(GenericEndpointSpec::SocrataFacility {
            fields: doh_cert_f, ..
        }) = &doh_cert_sys.generic_endpoint
        else {
            panic!("expected SocrataFacility for ny-doh-certifications");
        };
        assert_eq!(
            doh_cert_rows[0][doh_cert_f.license_number[0]].as_str(),
            Some("1")
        );
        assert_eq!(
            doh_cert_rows[0][doh_cert_f.name[0]].as_str(),
            Some("Albany Medical Center Hospital")
        );
        assert_eq!(
            doh_cert_rows[0][doh_cert_f.licensed_beds[0]].as_str(),
            Some("26")
        );
    }
}
