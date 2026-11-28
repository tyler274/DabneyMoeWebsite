//! Colorado (CO) facility and clinical provider licensing adapter.
//!
//! Verified live unauthenticated public data sources:
//! 1. CDPHE Health Facilities ArcGIS FeatureServer (`co-cdphe-facilities`):
//!    Psychiatric hospitals (`HOSP-PSY`, `LICPSY`), Acute Treatment Units (`ATU`),
//!    Psychiatric Residential Treatment Facilities (`PRTF`), residential care (`RCF-DD`),
//!    and Assisted Living Residences (`ALRONLY`, `ALR/ACF`, `ALR/BISL`).
//! 2. Colorado BHA / CDHS LADDERS ArcGIS MapServer (`co-bha-ladders`):
//!    Substance use disorder and mental health licensed/designated treatment facilities.
//! 3. Colorado DORA Division of Professions and Occupations Socrata dataset `7s5z-vewr`
//!    (`co-dora-providers`): Individual clinical practitioner licenses, expiration dates,
//!    disciplinary actions, and direct license verification URLs.
//!
//! Colorado regulates both MDs and DOs under a single unified Colorado Medical Board
//! (`separate_osteopathic_board = false`) using license prefix `DR`. MD vs DO distinction
//! is preserved strictly from the DORA `degrees` field (`~Medical Doctor` vs
//! `~Doctor of Osteopathy`); when `degrees` is absent or ambiguous, the adapter never guesses.

use std::fs;
use std::path::Path;
use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;

use crate::schema::{LicenseStatus, StaffLicense};
use crate::updater::cache::{CacheMode, DiskCache};

use super::types::{
    FacilityFieldMap, FacilityLicenseRecord, GenericEndpointSpec, LicensingAccessMode,
    LicensingDatasetFormat, LicensingScope, ProviderFieldMap, ProviderLicenseRecord,
    StateAdapterOutcome, StateBoardDescriptor, StateLicenseError, StateLicensingManifest,
    StateSyncRequest,
};

pub const CO_CDPHE_SYSTEM_ID: &str = "co-cdphe-facilities";
pub const CO_BHA_LADDERS_SYSTEM_ID: &str = "co-bha-ladders";
pub const CO_DORA_SYSTEM_ID: &str = "co-dora-providers";

pub const CO_CDPHE_QUERY_URL: &str = "https://services3.arcgis.com/66aUo8zsujfVXRIT/arcgis/rest/services/CDPHE_Health_Facilities/FeatureServer/0/query";
pub const CO_CDPHE_WHERE_CLAUSE: &str = "Facility_Type_Abbreviation IN ('HOSP-PSY','LICPSY','ATU','PRTF','RCF-DD','ALRONLY','ALR/ACF','ALR/BISL')";
pub const CO_CDPHE_PORTAL_URL: &str = "https://cdphe.colorado.gov/find-and-compare-facilities";

pub const CO_BHA_LADDERS_QUERY_URL: &str = "https://www.cohealthmaps.dphe.state.co.us/arcgis/rest/services/OPEN_DATA/cdphe_cdhs_colorado_ladders/MapServer/0/query";
pub const CO_BHA_LADDERS_WHERE_CLAUSE: &str = "1=1";
pub const CO_BHA_PORTAL_URL: &str = "https://bha.colorado.gov/";

pub const CO_DORA_SOCRATA_URL: &str = "https://data.colorado.gov/resource/7s5z-vewr.json";
pub const CO_DORA_WHERE_CLAUSE: &str = "licensetype in('DR','APN','C-APN','PA','PSY','CSW','LPC','MFT','ACD','ACC','ACB','ACA','ADDC','RN','PN') AND licensestatusdescription = 'Active'";
pub const CO_DORA_PORTAL_URL: &str =
    "https://apps.colorado.gov/dora/licensing/Lookup/LicenseLookup.aspx";

const CACHE_NAMESPACE: &str = "state-licensing/co";
const CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);
const ARCGIS_PAGE_SIZE: usize = 500;
const ARCGIS_MAX_RECORDS: usize = 5_000;
const DORA_PAGE_SIZE: usize = 1_000;
const DORA_MAX_RECORDS: usize = 20_000;

const CO_DORA_COVERS: &[StaffLicense] = &[
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
    StaffLicense::AddictionCounselor,
];

const CO_SYSTEMS: &[StateBoardDescriptor] = &[
    StateBoardDescriptor {
        id: CO_CDPHE_SYSTEM_ID,
        state: "CO",
        agency_name: "Colorado Department of Public Health and Environment (CDPHE)",
        system_name: "CDPHE Health Facilities Registry (ArcGIS FeatureServer)",
        scope: LicensingScope::Facility,
        access_mode: LicensingAccessMode::OpenDataApi,
        format: LicensingDatasetFormat::ArcGisFeatureLayer,
        portal_url: CO_CDPHE_PORTAL_URL,
        api_or_download_url: Some(CO_CDPHE_QUERY_URL),
        covers_licenses: &[],
        captcha_or_login_gated: false,
        generic_endpoint: Some(GenericEndpointSpec::ArcGisFacility {
            agency: "CDPHE Health Facilities & EMS Division",
            layer_query_url: CO_CDPHE_QUERY_URL,
            where_clause: CO_CDPHE_WHERE_CLAUSE,
            page_size: ARCGIS_PAGE_SIZE,
            max_records: ARCGIS_MAX_RECORDS,
            verification_url_template: None,
            fields: FacilityFieldMap {
                license_number: &["Facility_ID"],
                name: &["Facility_Name"],
                dba_or_site_name: &[],
                license_type: &[
                    "Facility_Type_Detail",
                    "Facility_Type_Abbreviation",
                    "Facility_Type",
                ],
                status: &["Operating_Status", "CDPHE_HFEMS_Licensed"],
                street: &["Address_Full"],
                city: &[],
                zip: &[],
                phone: &["Telephone"],
                licensed_beds: &["Licensed_Beds_Total"],
                expires_on: &[],
                detail_url: &[],
            },
        }),
        notes: "Public unauthenticated CDPHE ArcGIS FeatureServer layer covering psychiatric hospitals (HOSP-PSY, LICPSY), Acute Treatment Units (ATU), PRTFs, residential care (RCF-DD), and assisted living residences.",
    },
    StateBoardDescriptor {
        id: CO_BHA_LADDERS_SYSTEM_ID,
        state: "CO",
        agency_name: "Colorado Behavioral Health Administration (BHA / CDHS)",
        system_name: "CDHS LADDERS Behavioral Health & SUD Directory (ArcGIS MapServer)",
        scope: LicensingScope::Facility,
        access_mode: LicensingAccessMode::OpenDataApi,
        format: LicensingDatasetFormat::ArcGisFeatureLayer,
        portal_url: CO_BHA_PORTAL_URL,
        api_or_download_url: Some(CO_BHA_LADDERS_QUERY_URL),
        covers_licenses: &[],
        captcha_or_login_gated: false,
        generic_endpoint: Some(GenericEndpointSpec::ArcGisFacility {
            agency: "Colorado BHA / CDHS LADDERS",
            layer_query_url: CO_BHA_LADDERS_QUERY_URL,
            where_clause: CO_BHA_LADDERS_WHERE_CLAUSE,
            page_size: ARCGIS_PAGE_SIZE,
            max_records: ARCGIS_MAX_RECORDS,
            verification_url_template: None,
            fields: FacilityFieldMap {
                license_number: &["OBJECTID"],
                name: &["Account_Name"],
                dba_or_site_name: &[],
                license_type: &[
                    "Services_Offered",
                    "Substance_Use_Services",
                    "Mental_Health_Settings",
                ],
                status: &[],
                street: &["Address"],
                city: &["City"],
                zip: &["Zip"],
                phone: &["Phone"],
                licensed_beds: &[],
                expires_on: &[],
                detail_url: &[],
            },
        }),
        notes: "Public unauthenticated ArcGIS MapServer layer mirroring the Colorado LADDERS database of licensed substance use disorder and mental health facilities.",
    },
    StateBoardDescriptor {
        id: CO_DORA_SYSTEM_ID,
        state: "CO",
        agency_name: "Colorado Department of Regulatory Agencies (DORA)",
        system_name: "Division of Professions and Occupations (Socrata 7s5z-vewr)",
        scope: LicensingScope::Provider,
        access_mode: LicensingAccessMode::OpenDataApi,
        format: LicensingDatasetFormat::SocrataJson,
        portal_url: CO_DORA_PORTAL_URL,
        api_or_download_url: Some(CO_DORA_SOCRATA_URL),
        covers_licenses: CO_DORA_COVERS,
        captcha_or_login_gated: false,
        generic_endpoint: Some(GenericEndpointSpec::SocrataProvider {
            board: "Colorado DORA Division of Professions and Occupations",
            endpoint_url: CO_DORA_SOCRATA_URL,
            where_clause: Some(CO_DORA_WHERE_CLAUSE),
            page_size: DORA_PAGE_SIZE,
            max_records: DORA_MAX_RECORDS,
            verification_url_template: None,
            fields: ProviderFieldMap {
                license_number: &["licensenumber"],
                full_name: &["entityname"],
                first_name: &["firstname"],
                last_name: &["lastname"],
                credential_or_type: &["licensetype", "degrees", "subcategory", "specialty"],
                board_name: &["licensetype"],
                status: &["licensestatusdescription"],
                city: &["city"],
                zip: &["mailzipcode"],
                expires_on: &["licenseexpirationdate"],
                disciplinary_flag: &["programaction", "casenumber"],
            },
        }),
        notes: "Public unauthenticated Socrata SODA API on data.colorado.gov (dataset 7s5z-vewr). Unified Colorado Medical Board issues DR for both MD and DO, distinguished strictly via the degrees column.",
    },
];

pub fn manifest() -> StateLicensingManifest {
    StateLicensingManifest {
        state: "CO",
        state_name: "Colorado",
        separate_osteopathic_board: false,
        systems: CO_SYSTEMS,
    }
}

pub async fn sync(
    client: &reqwest::Client,
    cache: &DiskCache,
    req: &StateSyncRequest,
) -> Result<StateAdapterOutcome, StateLicenseError> {
    let mut outcome = StateAdapterOutcome {
        state: "CO".to_string(),
        ..StateAdapterOutcome::default()
    };

    if let Some(path) = req.import_file.as_deref() {
        let imported = import_local_file(path, req)?;
        outcome.merge(imported);
        return Ok(outcome);
    }

    if !req.skip_facilities && matches_filter(req.system_filter.as_deref(), CO_CDPHE_SYSTEM_ID) {
        let part = sync_cdphe_facilities(client, cache).await?;
        outcome.merge(part);
    }

    if !req.skip_facilities
        && matches_filter(req.system_filter.as_deref(), CO_BHA_LADDERS_SYSTEM_ID)
    {
        let part = sync_bha_ladders(client, cache).await?;
        outcome.merge(part);
    }

    if !req.skip_providers && matches_filter(req.system_filter.as_deref(), CO_DORA_SYSTEM_ID) {
        let part = sync_dora_providers(client, cache).await?;
        outcome.merge(part);
    }

    Ok(outcome)
}

fn matches_filter(filter: Option<&str>, system_id: &str) -> bool {
    match filter {
        None => true,
        Some(raw) => {
            let f = raw.trim().to_ascii_lowercase();
            f.is_empty()
                || f == system_id
                || system_id.contains(&f)
                || (f == "co-cdphe" && system_id == CO_CDPHE_SYSTEM_ID)
                || (f == "co-bha" && system_id == CO_BHA_LADDERS_SYSTEM_ID)
                || (f == "co-dora" && system_id == CO_DORA_SYSTEM_ID)
        }
    }
}

async fn fetch_cached_text(
    client: &reqwest::Client,
    cache: &DiskCache,
    url: &str,
    outcome: &mut StateAdapterOutcome,
) -> Result<Option<String>, StateLicenseError> {
    if let Some(envelope) = cache.get::<String>(CACHE_NAMESPACE, url, CACHE_TTL) {
        outcome.cache_hits += 1;
        return Ok(Some(envelope.value));
    }

    if cache.mode() == CacheMode::Offline {
        outcome
            .warnings
            .push(format!("offline cache miss for {url}"));
        return Ok(None);
    }

    let stale = cache.get_stale::<String>(CACHE_NAMESPACE, url);
    let mut request = client.get(url);
    if let Some(ref env) = stale {
        if let Some(ref etag) = env.etag {
            request = request.header(reqwest::header::IF_NONE_MATCH, etag);
        }
        if let Some(ref last_modified) = env.last_modified {
            request = request.header(reqwest::header::IF_MODIFIED_SINCE, last_modified);
        }
    }

    let response = request
        .send()
        .await
        .map_err(|source| StateLicenseError::Http {
            url: url.to_string(),
            source,
        })?;

    let status = response.status();
    if status == reqwest::StatusCode::NOT_MODIFIED {
        if let Some(env) = stale {
            outcome.cache_hits += 1;
            let _ = cache.put(
                CACHE_NAMESPACE,
                url,
                &env.value,
                env.etag,
                env.last_modified,
            );
            return Ok(Some(env.value));
        }
    }

    if status == reqwest::StatusCode::FORBIDDEN || status == reqwest::StatusCode::TOO_MANY_REQUESTS
    {
        return Err(StateLicenseError::CaptchaBlocked {
            url: url.to_string(),
        });
    }

    if !status.is_success() {
        return Err(StateLicenseError::HttpStatus {
            url: url.to_string(),
            status: status.as_u16(),
        });
    }

    let etag = response
        .headers()
        .get(reqwest::header::ETAG)
        .and_then(|v| v.to_str().ok())
        .map(ToString::to_string);
    let last_modified = response
        .headers()
        .get(reqwest::header::LAST_MODIFIED)
        .and_then(|v| v.to_str().ok())
        .map(ToString::to_string);

    let body = response
        .text()
        .await
        .map_err(|source| StateLicenseError::Http {
            url: url.to_string(),
            source,
        })?;

    if looks_like_captcha_html(&body) {
        return Err(StateLicenseError::CaptchaBlocked {
            url: url.to_string(),
        });
    }

    outcome.fetched += 1;
    let _ = cache.put(CACHE_NAMESPACE, url, &body, etag, last_modified);
    Ok(Some(body))
}

fn looks_like_captcha_html(body: &str) -> bool {
    let trimmed = body.trim_start();
    if !trimmed.starts_with('<') {
        return false;
    }
    let mut end = trimmed.len().min(4096);
    while end > 0 && !trimmed.is_char_boundary(end) {
        end -= 1;
    }
    let lower = trimmed[..end].to_ascii_lowercase();
    lower.contains("cf-turnstile")
        || lower.contains("g-recaptcha")
        || lower.contains("hcaptcha")
        || lower.contains("captcha")
        || lower.contains("just a moment")
}

#[derive(Debug, Deserialize)]
struct ArcGisResponse {
    #[serde(default)]
    features: Vec<ArcGisFeature>,
    #[serde(default, rename = "exceededTransferLimit")]
    exceeded_transfer_limit: bool,
}

#[derive(Debug, Deserialize)]
struct ArcGisFeature {
    #[serde(default)]
    attributes: serde_json::Map<String, Value>,
}

async fn sync_cdphe_facilities(
    client: &reqwest::Client,
    cache: &DiskCache,
) -> Result<StateAdapterOutcome, StateLicenseError> {
    let mut outcome = StateAdapterOutcome {
        state: "CO".to_string(),
        systems_executed: vec![CO_CDPHE_SYSTEM_ID.to_string()],
        ..StateAdapterOutcome::default()
    };

    let mut offset = 0usize;
    while offset < ARCGIS_MAX_RECORDS {
        let offset_str = offset.to_string();
        let count_str = ARCGIS_PAGE_SIZE.to_string();
        let url = url::Url::parse_with_params(
            CO_CDPHE_QUERY_URL,
            &[
                ("where", CO_CDPHE_WHERE_CLAUSE),
                ("outFields", "*"),
                ("returnGeometry", "false"),
                ("orderByFields", "OBJECTID ASC"),
                ("resultOffset", offset_str.as_str()),
                ("resultRecordCount", count_str.as_str()),
                ("f", "json"),
            ],
        )
        .map_err(|err| StateLicenseError::InvalidConfig(err.to_string()))?;

        let Some(body) = fetch_cached_text(client, cache, url.as_str(), &mut outcome).await? else {
            break;
        };

        let parsed: ArcGisResponse =
            serde_json::from_str(&body).map_err(|source| StateLicenseError::Json {
                source_label: url.to_string(),
                source,
            })?;

        let batch_len = parsed.features.len();
        for feature in parsed.features {
            if let Some(rec) = parse_cdphe_attributes(&feature.attributes) {
                outcome.facility_records.push(rec);
            }
        }

        if batch_len == 0 || (!parsed.exceeded_transfer_limit && batch_len < ARCGIS_PAGE_SIZE) {
            break;
        }
        offset += batch_len;
    }

    Ok(outcome)
}

async fn sync_bha_ladders(
    client: &reqwest::Client,
    cache: &DiskCache,
) -> Result<StateAdapterOutcome, StateLicenseError> {
    let mut outcome = StateAdapterOutcome {
        state: "CO".to_string(),
        systems_executed: vec![CO_BHA_LADDERS_SYSTEM_ID.to_string()],
        ..StateAdapterOutcome::default()
    };

    let mut offset = 0usize;
    while offset < ARCGIS_MAX_RECORDS {
        let offset_str = offset.to_string();
        let count_str = ARCGIS_PAGE_SIZE.to_string();
        let url = url::Url::parse_with_params(
            CO_BHA_LADDERS_QUERY_URL,
            &[
                ("where", CO_BHA_LADDERS_WHERE_CLAUSE),
                ("outFields", "*"),
                ("returnGeometry", "false"),
                ("orderByFields", "OBJECTID ASC"),
                ("resultOffset", offset_str.as_str()),
                ("resultRecordCount", count_str.as_str()),
                ("f", "json"),
            ],
        )
        .map_err(|err| StateLicenseError::InvalidConfig(err.to_string()))?;

        let Some(body) = fetch_cached_text(client, cache, url.as_str(), &mut outcome).await? else {
            break;
        };

        let parsed: ArcGisResponse =
            serde_json::from_str(&body).map_err(|source| StateLicenseError::Json {
                source_label: url.to_string(),
                source,
            })?;

        let batch_len = parsed.features.len();
        for feature in parsed.features {
            if let Some(rec) = parse_ladders_attributes(&feature.attributes) {
                outcome.facility_records.push(rec);
            }
        }

        if batch_len == 0 || (!parsed.exceeded_transfer_limit && batch_len < ARCGIS_PAGE_SIZE) {
            break;
        }
        offset += batch_len;
    }

    Ok(outcome)
}

async fn sync_dora_providers(
    client: &reqwest::Client,
    cache: &DiskCache,
) -> Result<StateAdapterOutcome, StateLicenseError> {
    let mut outcome = StateAdapterOutcome {
        state: "CO".to_string(),
        systems_executed: vec![CO_DORA_SYSTEM_ID.to_string()],
        ..StateAdapterOutcome::default()
    };

    let mut offset = 0usize;
    while offset < DORA_MAX_RECORDS {
        let limit_str = DORA_PAGE_SIZE.to_string();
        let offset_str = offset.to_string();
        let url = url::Url::parse_with_params(
            CO_DORA_SOCRATA_URL,
            &[
                ("$where", CO_DORA_WHERE_CLAUSE),
                ("$order", "licensenumber ASC"),
                ("$limit", limit_str.as_str()),
                ("$offset", offset_str.as_str()),
            ],
        )
        .map_err(|err| StateLicenseError::InvalidConfig(err.to_string()))?;

        let Some(body) = fetch_cached_text(client, cache, url.as_str(), &mut outcome).await? else {
            break;
        };

        let rows: Vec<serde_json::Map<String, Value>> =
            serde_json::from_str(&body).map_err(|source| StateLicenseError::Json {
                source_label: url.to_string(),
                source,
            })?;

        let batch_len = rows.len();
        for row in rows {
            if let Some(rec) = parse_dora_row(&row) {
                outcome.provider_records.push(rec);
            }
        }

        if batch_len < DORA_PAGE_SIZE {
            break;
        }
        offset += batch_len;
    }

    Ok(outcome)
}

pub fn parse_cdphe_attributes(
    attrs: &serde_json::Map<String, Value>,
) -> Option<FacilityLicenseRecord> {
    let facility_name = first_non_empty_str(attrs, &["Facility_Name", "facility_name"])?;
    let license_number = first_non_empty_str(attrs, &["Facility_ID", "facility_id", "OBJECTID"])
        .unwrap_or_else(|| format!("CDPHE-{}", slug_key(&facility_name)));

    let detail = first_non_empty_str(attrs, &["Facility_Type_Detail", "facility_type_detail"]);
    let abbrev = first_non_empty_str(
        attrs,
        &["Facility_Type_Abbreviation", "facility_type_abbreviation"],
    );
    let base_type = first_non_empty_str(attrs, &["Facility_Type", "facility_type"]);

    let license_type = match (base_type, detail, abbrev) {
        (Some(t), Some(d), Some(a)) => format!("{t} - {d} ({a})"),
        (Some(t), Some(d), None) => format!("{t} - {d}"),
        (None, Some(d), Some(a)) => format!("{d} ({a})"),
        (None, Some(d), None) => d,
        (Some(t), None, Some(a)) => format!("{t} ({a})"),
        (Some(t), None, None) => t,
        (None, None, Some(a)) => a,
        (None, None, None) => "CDPHE Licensed Health Facility".to_string(),
    };

    let status_raw = first_non_empty_str(
        attrs,
        &[
            "Operating_Status",
            "operating_status",
            "CDPHE_HFEMS_Licensed",
        ],
    )
    .unwrap_or_default();
    let status = LicenseStatus::parse_loose(&status_raw);

    let full_addr = first_non_empty_str(attrs, &["Address_Full", "address_full"]);
    let (street, city, zip) = full_addr
        .as_deref()
        .map(parse_co_address_full)
        .unwrap_or((None, None, None));

    let phone = first_non_empty_str(attrs, &["Telephone", "telephone", "Phone"]);
    let licensed_beds = first_u32(attrs, &["Licensed_Beds_Total", "licensed_beds_total"]);

    Some(FacilityLicenseRecord {
        state: "CO".to_string(),
        agency: "CDPHE Health Facilities & EMS Division".to_string(),
        license_number,
        facility_name,
        dba_name: None,
        license_type,
        status,
        street,
        city,
        zip,
        phone,
        licensed_beds,
        expires_on: None,
        disciplinary_actions: None,
        inspection_count: None,
        verification_url: Some(CO_CDPHE_PORTAL_URL.to_string()),
    })
}

pub fn parse_ladders_attributes(
    attrs: &serde_json::Map<String, Value>,
) -> Option<FacilityLicenseRecord> {
    let raw_name = first_non_empty_str(attrs, &["Account_Name", "account_name"])?;
    let (facility_name, dba_name) = split_dba_name(&raw_name);
    let license_number = first_non_empty_str(attrs, &["OBJECTID", "objectid"])
        .map(|id| format!("LADDERS-{id}"))
        .unwrap_or_else(|| format!("LADDERS-{}", slug_key(&facility_name)));

    let services = first_non_empty_str(attrs, &["Services_Offered", "services_offered"]);
    let sud = first_non_empty_str(attrs, &["Substance_Use_Services", "substance_use_services"]);
    let mh = first_non_empty_str(attrs, &["Mental_Health_Settings", "mental_health_settings"]);
    let license_type = [services, sud, mh]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("; ");
    let license_type = if license_type.is_empty() {
        "BHA / CDHS LADDERS Behavioral Health Facility".to_string()
    } else {
        license_type
    };

    let street = first_non_empty_str(attrs, &["Address", "address"]);
    let city = first_non_empty_str(attrs, &["City", "city"]);
    let zip = first_non_empty_str(attrs, &["Zip", "zip"]).map(|z| normalize_zip(&z));
    let phone = first_non_empty_str(attrs, &["Phone", "phone"]);

    Some(FacilityLicenseRecord {
        state: "CO".to_string(),
        agency: "Colorado BHA / CDHS LADDERS".to_string(),
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
        verification_url: Some(CO_BHA_PORTAL_URL.to_string()),
    })
}

pub fn parse_dora_row(row: &serde_json::Map<String, Value>) -> Option<ProviderLicenseRecord> {
    let license_type = first_non_empty_str(row, &["licensetype", "licensePrefix"])?;
    let license_number_raw = first_non_empty_str(row, &["licensenumber", "licenseNumber"])?;
    let license_number = format!("{license_type}.{license_number_raw}");

    let full_name = build_provider_name(row)?;
    let subcategory = first_non_empty_str(row, &["subcategory", "subCategory"]);
    let specialty = first_non_empty_str(row, &["specialty"]);
    let degrees = first_non_empty_str(row, &["degrees"]);

    let license = classify_dora_license(
        &license_type,
        subcategory.as_deref(),
        specialty.as_deref(),
        degrees.as_deref(),
    );

    let credential_label = build_dora_credential_label(
        &license_type,
        subcategory.as_deref(),
        specialty.as_deref(),
        degrees.as_deref(),
    );

    let status_raw = first_non_empty_str(
        row,
        &["licensestatusdescription", "licenseStatusDescription"],
    )
    .unwrap_or_default();
    let status = LicenseStatus::parse_loose(&status_raw);

    let city = first_non_empty_str(row, &["city"]);
    let zip = first_non_empty_str(row, &["mailzipcode", "mailZipCode"]).map(|z| normalize_zip(&z));
    let expires_on = first_non_empty_str(row, &["licenseexpirationdate", "licenseExpirationDate"])
        .map(|d| normalize_iso_date(&d));

    let disciplinary_flag = first_non_empty_str(row, &["programaction", "programAction"]).is_some()
        || first_non_empty_str(row, &["casenumber", "caseNumber"]).is_some()
        || first_non_empty_str(row, &["disciplineeffectivedate", "disciplineEffectiveDate"])
            .is_some();

    let verification_url =
        extract_socrata_url(row, &["linktoverifylicense", "linkToVerifyLicense"])
            .or_else(|| Some(CO_DORA_PORTAL_URL.to_string()));

    let board = dora_board_name(&license_type).to_string();

    Some(ProviderLicenseRecord {
        state: "CO".to_string(),
        board,
        license_number,
        full_name,
        credential_label,
        license,
        status,
        city,
        zip,
        expires_on,
        disciplinary_flag,
        verification_url,
    })
}

/// Strictly maps Colorado DORA `licensetype`, `subcategory`, `specialty`, and `degrees`
/// into a `StaffLicense` without ever guessing MD vs DO, PhD vs PsyD, or conflating NP/PA
/// with physicians.
pub fn classify_dora_license(
    license_type: &str,
    subcategory: Option<&str>,
    specialty: Option<&str>,
    degrees: Option<&str>,
) -> Option<StaffLicense> {
    let code = license_type.trim().to_ascii_uppercase();
    let sub = subcategory.unwrap_or("").trim().to_ascii_uppercase();
    let spec = specialty.unwrap_or("").trim().to_ascii_lowercase();
    let deg = degrees.unwrap_or("").trim().to_ascii_lowercase();

    match code.as_str() {
        "DR" | "T-DR" | "MSDR" => {
            let has_do = deg.contains("doctor of osteopathy")
                || deg.contains("osteopathic")
                || has_degree_token(&deg, "do");
            let has_md = deg.contains("medical doctor")
                || deg.contains("doctor in medicine")
                || has_degree_token(&deg, "md");

            if has_do && !has_md {
                if spec.contains("addict") {
                    Some(StaffLicense::DoAddiction)
                } else if spec.contains("psych") {
                    Some(StaffLicense::DoPsychiatrist)
                } else {
                    Some(StaffLicense::DoPhysician)
                }
            } else if has_md && !has_do {
                if spec.contains("addict") {
                    Some(StaffLicense::MdAddiction)
                } else if spec.contains("psych") {
                    Some(StaffLicense::MdPsychiatrist)
                } else {
                    Some(StaffLicense::MdPhysician)
                }
            } else {
                // Colorado Medical Board uses DR for both MD and DO; never guess when
                // degrees is absent or ambiguous.
                None
            }
        }
        "APN" | "C-APN" | "T-APN" | "MSAPN" | "DOCAPN" => {
            if sub.contains("CRNA") || sub.contains("CNM") {
                return None;
            }
            if spec.contains("psych") || spec.contains("mental health") {
                Some(StaffLicense::Pmhnp)
            } else if sub.is_empty() || sub == "NP" || sub == "C-NP" || sub.contains("CNS") {
                Some(StaffLicense::Np)
            } else {
                None
            }
        }
        "PA" | "T-PA" | "MSPA" | "TPA" => Some(StaffLicense::Pa),
        "PSY" | "MSPSY" | "PSYRX" => {
            let has_psyd = deg.contains("doctorate of psychology")
                || deg.contains("doctor of psychology")
                || has_degree_token(&deg, "psyd");
            let has_phd = deg.contains("doctorate of philosophy")
                || deg.contains("doctor of philosophy")
                || has_degree_token(&deg, "phd");

            if has_psyd && !has_phd {
                Some(StaffLicense::PsydPsychologist)
            } else if has_phd && !has_psyd {
                Some(StaffLicense::PhdPsychologist)
            } else {
                // Never guess PhD vs PsyD when degrees is missing or dual/ambiguous.
                None
            }
        }
        "CSW" | "MSCSW" => Some(StaffLicense::Lcsw),
        "LPC" | "MSLPC" => Some(StaffLicense::LpcLmhc),
        "MFT" | "MSMFT" => Some(StaffLicense::Lmft),
        "ACD" | "ACC" | "ACB" | "ACA" | "ADDC" => Some(StaffLicense::AddictionCounselor),
        "RN" | "T-RN" | "MSRN" | "DOC-RN" => Some(StaffLicense::Rn),
        "PN" | "T-PN" | "MSPN" | "DOC-PN" => Some(StaffLicense::Lpn),
        _ => None,
    }
}

fn dora_board_name(license_type: &str) -> &'static str {
    match license_type.trim().to_ascii_uppercase().as_str() {
        "DR" | "T-DR" | "MSDR" | "PA" | "T-PA" | "MSPA" | "TPA" => "Colorado Medical Board (DORA)",
        "APN" | "C-APN" | "T-APN" | "MSAPN" | "DOCAPN" | "RN" | "T-RN" | "MSRN" | "DOC-RN"
        | "PN" | "T-PN" | "MSPN" | "DOC-PN" => "Colorado State Board of Nursing (DORA)",
        "PSY" | "MSPSY" | "PSYRX" => "Colorado State Board of Psychologist Examiners (DORA)",
        "CSW" | "MSCSW" | "LSW" | "MSLSW" | "SWC" => {
            "Colorado State Board of Social Work Examiners (DORA)"
        }
        "LPC" | "MSLPC" | "LPCC" => {
            "Colorado State Board of Licensed Professional Counselor Examiners (DORA)"
        }
        "MFT" | "MSMFT" | "MFTC" => {
            "Colorado State Board of Marriage and Family Therapist Examiners (DORA)"
        }
        "ACD" | "ACC" | "ACB" | "ACA" | "ADDC" => {
            "Colorado State Board of Addiction Counselor Examiners (DORA)"
        }
        _ => "Colorado DORA Division of Professions and Occupations",
    }
}

fn build_dora_credential_label(
    license_type: &str,
    subcategory: Option<&str>,
    specialty: Option<&str>,
    degrees: Option<&str>,
) -> String {
    let mut parts = vec![license_type.trim().to_string()];
    if let Some(sub) = subcategory {
        let s = sub.trim();
        if !s.is_empty() {
            parts.push(s.to_string());
        }
    }
    if let Some(deg) = degrees {
        let cleaned = deg
            .split('~')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(", ");
        if !cleaned.is_empty() {
            parts.push(cleaned);
        }
    }
    if let Some(spec) = specialty {
        let s = spec.trim();
        if !s.is_empty() {
            parts.push(s.to_string());
        }
    }
    parts.join(" | ")
}

fn import_local_file(
    path: &Path,
    req: &StateSyncRequest,
) -> Result<StateAdapterOutcome, StateLicenseError> {
    let raw = fs::read_to_string(path).map_err(|source| StateLicenseError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let label = path.display().to_string();
    let trimmed = raw.trim_start();

    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        import_json_str(trimmed, &label, req)
    } else {
        import_csv_str(&raw, &label, req)
    }
}

fn import_json_str(
    raw: &str,
    source_label: &str,
    req: &StateSyncRequest,
) -> Result<StateAdapterOutcome, StateLicenseError> {
    let mut outcome = StateAdapterOutcome {
        state: "CO".to_string(),
        systems_executed: vec!["co-import-json".to_string()],
        ..StateAdapterOutcome::default()
    };

    let value: Value = serde_json::from_str(raw).map_err(|source| StateLicenseError::Json {
        source_label: source_label.to_string(),
        source,
    })?;

    if let Some(features) = value.get("features").and_then(Value::as_array) {
        if !req.skip_facilities {
            for feature in features {
                let Some(attrs) = feature.get("attributes").and_then(Value::as_object) else {
                    continue;
                };
                if attrs.contains_key("Facility_Name") || attrs.contains_key("facility_name") {
                    if let Some(rec) = parse_cdphe_attributes(attrs) {
                        outcome.facility_records.push(rec);
                    }
                } else if attrs.contains_key("Account_Name") || attrs.contains_key("account_name") {
                    if let Some(rec) = parse_ladders_attributes(attrs) {
                        outcome.facility_records.push(rec);
                    }
                }
            }
        }
        return Ok(outcome);
    }

    if let Some(rows) = value.as_array() {
        for item in rows {
            let Some(obj) = item.as_object() else {
                continue;
            };
            if obj.contains_key("licensetype") || obj.contains_key("licensePrefix") {
                if !req.skip_providers {
                    if let Some(rec) = parse_dora_row(obj) {
                        outcome.provider_records.push(rec);
                    }
                }
            } else if obj.contains_key("Facility_Name") || obj.contains_key("facility_name") {
                if !req.skip_facilities {
                    if let Some(rec) = parse_cdphe_attributes(obj) {
                        outcome.facility_records.push(rec);
                    }
                }
            } else if (obj.contains_key("Account_Name") || obj.contains_key("account_name"))
                && !req.skip_facilities
            {
                if let Some(rec) = parse_ladders_attributes(obj) {
                    outcome.facility_records.push(rec);
                }
            }
        }
        return Ok(outcome);
    }

    Err(StateLicenseError::InvalidConfig(format!(
        "unsupported JSON structure in {source_label}; expected ArcGIS FeatureSet or JSON array"
    )))
}

fn import_csv_str(
    raw: &str,
    source_label: &str,
    req: &StateSyncRequest,
) -> Result<StateAdapterOutcome, StateLicenseError> {
    let mut outcome = StateAdapterOutcome {
        state: "CO".to_string(),
        systems_executed: vec!["co-import-csv".to_string()],
        ..StateAdapterOutcome::default()
    };

    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .from_reader(raw.as_bytes());
    let headers = reader
        .headers()
        .map_err(|source| StateLicenseError::Csv {
            source_label: source_label.to_string(),
            source,
        })?
        .clone();

    let header_names: Vec<String> = headers.iter().map(|h| h.trim().to_string()).collect();

    for record in reader.records() {
        let record = record.map_err(|source| StateLicenseError::Csv {
            source_label: source_label.to_string(),
            source,
        })?;
        let mut map = serde_json::Map::new();
        for (idx, key) in header_names.iter().enumerate() {
            if let Some(val) = record.get(idx) {
                let v = val.trim();
                if !v.is_empty() {
                    map.insert(key.clone(), Value::String(v.to_string()));
                }
            }
        }

        if map.contains_key("licensetype") || map.contains_key("licensePrefix") {
            if !req.skip_providers {
                if let Some(rec) = parse_dora_row(&map) {
                    outcome.provider_records.push(rec);
                }
            }
        } else if map.contains_key("Facility_Name") || map.contains_key("facility_name") {
            if !req.skip_facilities {
                if let Some(rec) = parse_cdphe_attributes(&map) {
                    outcome.facility_records.push(rec);
                }
            }
        } else if (map.contains_key("Account_Name") || map.contains_key("account_name"))
            && !req.skip_facilities
        {
            if let Some(rec) = parse_ladders_attributes(&map) {
                outcome.facility_records.push(rec);
            }
        }
    }

    Ok(outcome)
}

fn first_non_empty_str(map: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<String> {
    for &key in keys {
        if let Some(value) = map.get(key) {
            match value {
                Value::String(s) => {
                    let trimmed = s.trim();
                    if !trimmed.is_empty() {
                        return Some(trimmed.to_string());
                    }
                }
                Value::Number(n) => return Some(n.to_string()),
                _ => {}
            }
        }
    }
    None
}

fn first_u32(map: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<u32> {
    for &key in keys {
        if let Some(value) = map.get(key) {
            match value {
                Value::Number(n) => {
                    if let Some(u) = n.as_u64() {
                        return u32::try_from(u).ok();
                    }
                    if let Some(f) = n.as_f64() {
                        if f.is_finite() && f >= 0.0 {
                            return Some(f.round() as u32);
                        }
                    }
                }
                Value::String(s) => {
                    let trimmed = s.trim();
                    if let Ok(u) = trimmed.parse::<u32>() {
                        return Some(u);
                    }
                    if let Ok(f) = trimmed.parse::<f64>() {
                        if f.is_finite() && f >= 0.0 {
                            return Some(f.round() as u32);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    None
}

fn extract_socrata_url(map: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<String> {
    for &key in keys {
        if let Some(value) = map.get(key) {
            match value {
                Value::String(s) => {
                    let trimmed = s.trim();
                    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
                        return Some(trimmed.to_string());
                    }
                }
                Value::Object(obj) => {
                    if let Some(Value::String(u)) = obj.get("url") {
                        let trimmed = u.trim();
                        if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
                            return Some(trimmed.to_string());
                        }
                    }
                }
                _ => {}
            }
        }
    }
    None
}

fn build_provider_name(row: &serde_json::Map<String, Value>) -> Option<String> {
    let first = first_non_empty_str(row, &["firstname", "firstName"]);
    let middle = first_non_empty_str(row, &["middlename", "middleName"]);
    let last = first_non_empty_str(row, &["lastname", "lastName"]);
    let suffix = first_non_empty_str(row, &["suffix"]);

    let mut parts = Vec::new();
    if let Some(f) = first {
        parts.push(f);
    }
    if let Some(m) = middle {
        parts.push(m);
    }
    if let Some(l) = last {
        parts.push(l);
    }
    if let Some(s) = suffix {
        parts.push(s);
    }

    if !parts.is_empty() {
        return Some(parts.join(" "));
    }

    first_non_empty_str(row, &["entityname", "entityName"])
}

fn has_degree_token(lower: &str, token: &str) -> bool {
    lower
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|part| part == token)
}

fn parse_co_address_full(raw: &str) -> (Option<String>, Option<String>, Option<String>) {
    let parts: Vec<&str> = raw
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    match parts.as_slice() {
        [street, city, state_zip] => {
            let zip = state_zip
                .split_whitespace()
                .last()
                .filter(|tok| tok.chars().next().is_some_and(|c| c.is_ascii_digit()))
                .map(normalize_zip);
            (Some((*street).to_string()), Some((*city).to_string()), zip)
        }
        [street, rest] => (Some((*street).to_string()), Some((*rest).to_string()), None),
        [only] => (Some((*only).to_string()), None, None),
        _ => (Some(raw.trim().to_string()), None, None),
    }
}

fn split_dba_name(raw: &str) -> (String, Option<String>) {
    let upper = raw.to_ascii_uppercase();
    if let Some(idx) = upper.find(" DBA ") {
        let legal = raw[..idx].trim();
        let dba = raw[idx + 5..].trim();
        if !legal.is_empty() && !dba.is_empty() {
            return (legal.to_string(), Some(dba.to_string()));
        }
    }
    (raw.trim().to_string(), None)
}

fn normalize_zip(raw: &str) -> String {
    let digits: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() >= 5 {
        digits[..5].to_string()
    } else {
        raw.trim().to_string()
    }
}

fn normalize_iso_date(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Some(date_part) = trimmed.split('T').next() {
        if date_part.len() == 10 {
            return date_part.to_string();
        }
    }
    trimmed.to_string()
}

fn slug_key(raw: &str) -> String {
    raw.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(24)
        .collect::<String>()
        .to_ascii_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_describes_all_verified_co_systems() {
        let m = manifest();
        assert_eq!(m.state, "CO");
        assert_eq!(m.state_name, "Colorado");
        assert!(!m.separate_osteopathic_board);
        assert_eq!(m.systems.len(), 3);

        for sys in m.systems {
            assert_eq!(sys.state, "CO");
            assert!(sys.portal_url.starts_with("https://"));
            assert!(sys.api_or_download_url.is_some());
            assert!(sys.generic_endpoint.is_some());
            assert!(!sys.captcha_or_login_gated);
        }
    }

    #[test]
    fn parses_live_verified_cdphe_psychiatric_hospital_fixture() {
        let fixture = r#"{
            "features": [
                {
                    "attributes": {
                        "OBJECTID": 16,
                        "Facility_ID": "010304",
                        "Facility_Name": "CENTENNIAL PEAKS HOSPITAL",
                        "Facility_Type_Code": "012",
                        "Facility_Type": "Hospital",
                        "Facility_Type_Detail": "PSYCHIATRIC",
                        "Facility_Type_Abbreviation": "HOSP-PSY",
                        "Licensed_Beds_Total": 104,
                        "Telephone": "(303) 673-9990",
                        "County": "BOULDER",
                        "Address_Full": "2255 S 88TH ST, LOUISVILLE, CO 80027",
                        "CDPHE_HFEMS_Licensed": "Licensed",
                        "Operating_Status": "Active"
                    }
                }
            ]
        }"#;

        let outcome =
            import_json_str(fixture, "cdphe-fixture.json", &StateSyncRequest::default()).unwrap();
        assert_eq!(outcome.facility_records.len(), 1);
        let rec = &outcome.facility_records[0];
        assert_eq!(rec.state, "CO");
        assert_eq!(rec.license_number, "010304");
        assert_eq!(rec.facility_name, "CENTENNIAL PEAKS HOSPITAL");
        assert_eq!(rec.status, LicenseStatus::Active);
        assert_eq!(rec.licensed_beds, Some(104));
        assert_eq!(rec.street.as_deref(), Some("2255 S 88TH ST"));
        assert_eq!(rec.city.as_deref(), Some("LOUISVILLE"));
        assert_eq!(rec.zip.as_deref(), Some("80027"));
        assert_eq!(rec.phone.as_deref(), Some("(303) 673-9990"));
        assert!(rec.license_type.contains("HOSP-PSY"));
    }

    #[test]
    fn parses_live_verified_bha_ladders_fixture() {
        let fixture = r#"{
            "features": [
                {
                    "attributes": {
                        "OBJECTID": 803,
                        "Account_Name": "Reyes Corporation DBA Creative Counseling Services - Fort Collins",
                        "Address": "3000 S College Ave #201",
                        "City": "Fort Collins",
                        "State": "CO",
                        "Zip": "80525",
                        "Phone": "(970) 221-4057",
                        "Services_Offered": "Behavioral health: both mental health (MH) and substance use disorder (SUD) treatment services",
                        "Substance_Use_Services": "",
                        "Mental_Health_Settings": ""
                    }
                }
            ]
        }"#;

        let outcome = import_json_str(
            fixture,
            "ladders-fixture.json",
            &StateSyncRequest::default(),
        )
        .unwrap();
        assert_eq!(outcome.facility_records.len(), 1);
        let rec = &outcome.facility_records[0];
        assert_eq!(rec.license_number, "LADDERS-803");
        assert_eq!(rec.facility_name, "Reyes Corporation");
        assert_eq!(
            rec.dba_name.as_deref(),
            Some("Creative Counseling Services - Fort Collins")
        );
        assert_eq!(rec.city.as_deref(), Some("Fort Collins"));
        assert_eq!(rec.zip.as_deref(), Some("80525"));
        assert_eq!(rec.status, LicenseStatus::Active);
    }

    #[test]
    fn strictly_distinguishes_co_dora_md_do_apn_pa_psy_lcsw_and_discipline() {
        let fixture = r#"[
            {
                "lastname": "Grebe",
                "firstname": "John",
                "middlename": "Harry",
                "city": "Highlands Ranch",
                "state": "CO",
                "mailzipcode": "80126",
                "licensetype": "DR",
                "licensenumber": "55676",
                "licenseexpirationdate": "2027-04-30T00:00:00.000",
                "licensestatusdescription": "Active",
                "specialty": "Psychiatry",
                "degrees": "~Medical Doctor",
                "linktoverifylicense": {"url": "https://www.colorado.gov/dora/licensing/Lookup/PrintLicenseDetails.aspx?cred=1039211&contact=1115552"}
            },
            {
                "lastname": "Hampton",
                "firstname": "Kyle",
                "middlename": "Nathan",
                "city": "Fort Collins",
                "state": "CO",
                "mailzipcode": "80528",
                "licensetype": "DR",
                "licensenumber": "56040",
                "licenseexpirationdate": "2027-04-30T00:00:00.000",
                "licensestatusdescription": "Active",
                "specialty": "Family Medicine",
                "degrees": "~Doctor of Osteopathy",
                "linktoverifylicense": {"url": "https://www.colorado.gov/dora/licensing/Lookup/PrintLicenseDetails.aspx?cred=1065832&contact=1147744"}
            },
            {
                "lastname": "Eppler",
                "firstname": "Stephen",
                "city": "Denver",
                "state": "CO",
                "mailzipcode": "80206",
                "licensetype": "DR",
                "licensenumber": "23920",
                "licenseexpirationdate": "2011-05-31T00:00:00.000",
                "licensestatusdescription": "Surrendered",
                "casenumber": "M2008-3129",
                "programaction": "Agreement"
            },
            {
                "lastname": "Rivera",
                "firstname": "Elena",
                "city": "Denver",
                "state": "CO",
                "mailzipcode": "80202",
                "licensetype": "C-APN",
                "subcategory": "C-NP",
                "licensenumber": "99100",
                "licenseexpirationdate": "2027-09-30T00:00:00.000",
                "licensestatusdescription": "Active",
                "specialty": "Psychiatric/Mental Health"
            },
            {
                "lastname": "Haka",
                "firstname": "Kimberly",
                "city": "Highlands Ranch",
                "state": "CO",
                "mailzipcode": "80126",
                "licensetype": "CSW",
                "licensenumber": "998081",
                "licenseexpirationdate": "2027-08-31T00:00:00.000",
                "licensestatusdescription": "Active",
                "degrees": "~Masters of Social Work"
            }
        ]"#;

        let outcome =
            import_json_str(fixture, "dora-fixture.json", &StateSyncRequest::default()).unwrap();
        assert_eq!(outcome.provider_records.len(), 5);

        // MD Psychiatrist
        let md = &outcome.provider_records[0];
        assert_eq!(md.license, Some(StaffLicense::MdPsychiatrist));
        assert_eq!(md.license_number, "DR.55676");
        assert_eq!(md.expires_on.as_deref(), Some("2027-04-30"));
        assert!(!md.disciplinary_flag);
        assert!(md
            .verification_url
            .as_deref()
            .unwrap()
            .contains("PrintLicenseDetails.aspx"));

        // DO Physician (strictly distinct from MD)
        let do_doc = &outcome.provider_records[1];
        assert_eq!(do_doc.license, Some(StaffLicense::DoPhysician));
        assert_eq!(do_doc.license_number, "DR.56040");
        assert!(!do_doc.disciplinary_flag);

        // DR without degrees: never guesses MD vs DO, preserves disciplinary flag & status
        let unknown_dr = &outcome.provider_records[2];
        assert_eq!(unknown_dr.license, None);
        assert_eq!(unknown_dr.status, LicenseStatus::Revoked);
        assert!(unknown_dr.disciplinary_flag);

        // C-APN Psychiatric/Mental Health -> PMHNP (strictly distinct from MD/DO)
        let pmhnp = &outcome.provider_records[3];
        assert_eq!(pmhnp.license, Some(StaffLicense::Pmhnp));
        assert!(!pmhnp.license.unwrap().is_physician());

        // CSW -> LCSW
        let lcsw = &outcome.provider_records[4];
        assert_eq!(lcsw.license, Some(StaffLicense::Lcsw));
    }

    #[test]
    fn classify_dora_license_covers_all_behavioral_health_codes() {
        assert_eq!(
            classify_dora_license(
                "DR",
                None,
                Some("Addiction/Psychiatry Fellowship"),
                Some("~Medical Doctor")
            ),
            Some(StaffLicense::MdAddiction)
        );
        assert_eq!(
            classify_dora_license(
                "DR",
                None,
                Some("Psychiatry"),
                Some("~Doctor of Osteopathy")
            ),
            Some(StaffLicense::DoPsychiatrist)
        );
        assert_eq!(
            classify_dora_license("DR", None, Some("Psychiatry"), None),
            None
        );
        assert_eq!(
            classify_dora_license("APN", Some("NP"), Some("Family"), None),
            Some(StaffLicense::Np)
        );
        assert_eq!(classify_dora_license("APN", Some("CRNA"), None, None), None);
        assert_eq!(
            classify_dora_license("PA", None, None, None),
            Some(StaffLicense::Pa)
        );
        assert_eq!(
            classify_dora_license("PSY", None, None, Some("~Doctorate of Philosophy")),
            Some(StaffLicense::PhdPsychologist)
        );
        assert_eq!(
            classify_dora_license("PSY", None, None, Some("~Doctorate of Psychology")),
            Some(StaffLicense::PsydPsychologist)
        );
        assert_eq!(classify_dora_license("PSY", None, None, None), None);
        assert_eq!(
            classify_dora_license("LPC", None, None, None),
            Some(StaffLicense::LpcLmhc)
        );
        assert_eq!(
            classify_dora_license("MFT", None, None, None),
            Some(StaffLicense::Lmft)
        );
        assert_eq!(
            classify_dora_license("ACD", None, None, None),
            Some(StaffLicense::AddictionCounselor)
        );
        assert_eq!(
            classify_dora_license("ACC", None, None, None),
            Some(StaffLicense::AddictionCounselor)
        );
        assert_eq!(
            classify_dora_license("RN", None, None, None),
            Some(StaffLicense::Rn)
        );
        assert_eq!(
            classify_dora_license("PN", None, None, None),
            Some(StaffLicense::Lpn)
        );
    }
}
