//! California (`CA`) facility and clinical provider licensing adapter.
//!
//! Verified live public facility registries on the California Health and Human Services (CHHS)
//! Open Data Portal (`data.chhs.ca.gov`, CKAN DataStore API, unauthenticated, no CAPTCHA):
//! - DHCS Licensed SUD Recovery or Treatment Facilities (`a4cb74c2-15ec-4a34-8cec-a9f8a4a2d72e`)
//! - DHCS Licensed Narcotic Treatment Programs (`131c734e-a42a-42b3-9da5-635037bcdbe2`)
//! - DHCS Licensed MHRC and Psychiatric Health Facilities (`ef9f4daf-892c-45f6-b46a-3037c5a93b23`)
//! - DHCS Certified Residential Mental Health Programs (`c687007e-5b9a-4b5b-a92d-1ccd07008382`)
//! - CDPH Licensed and Certified Healthcare Facilities (`f0ae5731-fef8-417f-839d-54a0ed3a126e`)
//! - CDSS Community Care Licensing Adult Residential & Social Rehabilitation Facilities
//!   (`9f5d1d00-6b24-4f44-a158-9cbe4b43f117`)
//!
//! Clinical provider licensing in California is administered by the Department of Consumer
//! Affairs (DCA) across separate healing-arts boards, including a SEPARATE Osteopathic Medical
//! Board of California (DO) distinct from the Medical Board of California (MD). The interactive
//! search portal (`https://search.dca.ca.gov/`) is gated by Cloudflare Turnstile
//! (`captcha_or_login_gated: true`). We never bypass Turnstile; instead we model all six CA DCA
//! clinical boards as `BulkDownloadExport` pointing to DCA's official Public Information bulk
//! file page (`https://www.dca.ca.gov/consumers/public_info/index.shtml`) and support local
//! ingestion via `--import-file`.

use std::fs;

use serde_json::Value;

use crate::schema::{LicenseStatus, StaffLicense};
use crate::updater::cache::DiskCache;

use super::generic::{execute_generic_endpoint, import_local_file};
use super::types::{
    FacilityFieldMap, FacilityLicenseRecord, GenericEndpointSpec, LicensingAccessMode,
    LicensingDatasetFormat, LicensingScope, ProviderLicenseRecord, StateAdapterOutcome,
    StateBoardDescriptor, StateLicenseError, StateLicensingManifest, StateSyncRequest,
};

pub const CHHS_DATASTORE_SEARCH_URL: &str =
    "https://data.chhs.ca.gov/api/3/action/datastore_search";

pub const CA_DHCS_SUD_RESOURCE_ID: &str = "a4cb74c2-15ec-4a34-8cec-a9f8a4a2d72e";
pub const CA_DHCS_NTP_RESOURCE_ID: &str = "131c734e-a42a-42b3-9da5-635037bcdbe2";
pub const CA_DHCS_MHRC_PHF_RESOURCE_ID: &str = "ef9f4daf-892c-45f6-b46a-3037c5a93b23";
pub const CA_DHCS_RES_MH_RESOURCE_ID: &str = "c687007e-5b9a-4b5b-a92d-1ccd07008382";
pub const CA_CDPH_LOCATIONS_RESOURCE_ID: &str = "f0ae5731-fef8-417f-839d-54a0ed3a126e";
pub const CA_CDSS_ARF_RESOURCE_ID: &str = "9f5d1d00-6b24-4f44-a158-9cbe4b43f117";

pub const CA_DCA_SEARCH_PORTAL_URL: &str = "https://search.dca.ca.gov/";
pub const CA_DCA_PUBLIC_INFO_URL: &str = "https://www.dca.ca.gov/consumers/public_info/index.shtml";

const DHCS_SUD_FIELDS: FacilityFieldMap = FacilityFieldMap {
    license_number: &["Application_Number"],
    name: &["Facility_Name"],
    dba_or_site_name: &["Legal_Entity_Name"],
    license_type: &["Program_Code", "Type_of_Application"],
    status: &["Type_of_Application"],
    street: &["Facility_Address1"],
    city: &["Facility_City"],
    zip: &["Facility_Zip"],
    phone: &["Facility_Phone"],
    licensed_beds: &["Total_Capacity", "Treatment_Capacity"],
    expires_on: &["Expiration_Date"],
    detail_url: &[],
};

const DHCS_NTP_FIELDS: FacilityFieldMap = FacilityFieldMap {
    license_number: &["License__", "OTP_CA10"],
    name: &["Licensee"],
    dba_or_site_name: &["DBA"],
    license_type: &["OTP_CA10"],
    status: &[],
    street: &["Address"],
    city: &["City"],
    zip: &["Program_Zip"],
    phone: &["Phone_Number"],
    licensed_beds: &[],
    expires_on: &[],
    detail_url: &[],
};

const DHCS_MHRC_PHF_FIELDS: FacilityFieldMap = FacilityFieldMap {
    license_number: &["Record ID"],
    name: &["Facility Name", "Legal Name"],
    dba_or_site_name: &["Legal Name"],
    license_type: &["Service Type"],
    status: &["Type of Application"],
    street: &["Physical Address"],
    city: &["Physical City"],
    zip: &["Physical Zip"],
    phone: &["Facility Phone"],
    licensed_beds: &["Client/Patient Capacity"],
    expires_on: &["Expiration Date"],
    detail_url: &[],
};

const DHCS_RES_MH_FIELDS: FacilityFieldMap = FacilityFieldMap {
    license_number: &["CERTIFICATE NUMBER"],
    name: &["PROGRAM NAME", "LICENSEE"],
    dba_or_site_name: &["LICENSEE"],
    license_type: &["TYPE"],
    status: &["TYPE OF APPLICATION"],
    street: &["FACILITY ADDRESS"],
    city: &["CITY"],
    zip: &["ZIP CODE"],
    phone: &[],
    licensed_beds: &["BEDS"],
    expires_on: &[],
    detail_url: &[],
};

const CDPH_FACILITY_FIELDS: FacilityFieldMap = FacilityFieldMap {
    license_number: &["FINAL_LICENSE_NUMBER", "FACID"],
    name: &["FACNAME"],
    dba_or_site_name: &["BUSINESS_NAME"],
    license_type: &["FAC_FDR", "FAC_TYPE_CODE"],
    status: &["LICENSE_STATUS_DESCRIPTION", "FAC_STATUS_TYPE_CODE"],
    street: &["ADDRESS"],
    city: &["CITY"],
    zip: &["ZIP"],
    phone: &["CONTACT_PHONE_NUMBER"],
    licensed_beds: &["CAPACITY"],
    expires_on: &["LICENSE_EXPIRATION_DATE"],
    detail_url: &[],
};

const CDSS_ARF_FIELDS: FacilityFieldMap = FacilityFieldMap {
    license_number: &["facility_number"],
    name: &["facility_name"],
    // Omit `licensee` (multi-site corporate operator name) so confidential-address SAMHSA
    // entries do not absorb every campus of a large residential operator in the same city.
    dba_or_site_name: &[],
    license_type: &["facility_type"],
    status: &["facility_status"],
    street: &["facility_address"],
    city: &["facility_city"],
    zip: &["facility_zip"],
    phone: &["facility_telephone_number"],
    licensed_beds: &["facility_capacity"],
    expires_on: &[],
    detail_url: &[],
};

const CA_SYSTEMS: &[StateBoardDescriptor] = &[
    StateBoardDescriptor {
        id: "ca-dhcs-sud",
        state: "CA",
        agency_name: "CA DHCS (SUD Compliance Division)",
        system_name: "Licensed SUD Recovery or Treatment Facilities (CHHS CKAN)",
        scope: LicensingScope::Facility,
        access_mode: LicensingAccessMode::OpenDataApi,
        format: LicensingDatasetFormat::CkanDatastore,
        portal_url: "https://data.chhs.ca.gov/dataset/sud-recovery-treatment-facilities",
        api_or_download_url: Some(
            "https://data.chhs.ca.gov/api/3/action/datastore_search?resource_id=a4cb74c2-15ec-4a34-8cec-a9f8a4a2d72e",
        ),
        covers_licenses: &[],
        captcha_or_login_gated: false,
        generic_endpoint: Some(GenericEndpointSpec::CkanFacility {
            agency: "CA DHCS",
            datastore_search_url: CHHS_DATASTORE_SEARCH_URL,
            resource_id: CA_DHCS_SUD_RESOURCE_ID,
            page_size: 1000,
            max_records: 5000,
            verification_url_template: Some(
                "https://data.chhs.ca.gov/dataset/sud-recovery-treatment-facilities",
            ),
            fields: DHCS_SUD_FIELDS,
        }),
        notes: "Unauthenticated CHHS CKAN DataStore for DHCS-licensed and certified residential/outpatient SUD recovery and treatment programs (2,287 records verified).",
    },
    StateBoardDescriptor {
        id: "ca-dhcs-ntp",
        state: "CA",
        agency_name: "CA DHCS (Narcotic Treatment Programs)",
        system_name: "Licensed Narcotic Treatment Programs (CHHS CKAN)",
        scope: LicensingScope::Facility,
        access_mode: LicensingAccessMode::OpenDataApi,
        format: LicensingDatasetFormat::CkanDatastore,
        portal_url: "https://data.chhs.ca.gov/dataset/licensed-narcotic-treatment-programs",
        api_or_download_url: Some(
            "https://data.chhs.ca.gov/api/3/action/datastore_search?resource_id=131c734e-a42a-42b3-9da5-635037bcdbe2",
        ),
        covers_licenses: &[],
        captcha_or_login_gated: false,
        generic_endpoint: Some(GenericEndpointSpec::CkanFacility {
            agency: "CA DHCS NTP",
            datastore_search_url: CHHS_DATASTORE_SEARCH_URL,
            resource_id: CA_DHCS_NTP_RESOURCE_ID,
            page_size: 500,
            max_records: 1000,
            verification_url_template: Some(
                "https://data.chhs.ca.gov/dataset/licensed-narcotic-treatment-programs",
            ),
            fields: DHCS_NTP_FIELDS,
        }),
        notes: "Unauthenticated CHHS CKAN DataStore for DHCS-licensed opioid/narcotic treatment programs (183 records verified).",
    },
    StateBoardDescriptor {
        id: "ca-dhcs-mhrc-phf",
        state: "CA",
        agency_name: "CA DHCS (Mental Health Licensing)",
        system_name: "Licensed MHRC and Psychiatric Health Facilities (CHHS CKAN)",
        scope: LicensingScope::Facility,
        access_mode: LicensingAccessMode::OpenDataApi,
        format: LicensingDatasetFormat::CkanDatastore,
        portal_url: "https://data.chhs.ca.gov/dataset/licensed-mental-health-rehabilitation-centers-mhrc-and-psychiatric-health-facilities-phf",
        api_or_download_url: Some(
            "https://data.chhs.ca.gov/api/3/action/datastore_search?resource_id=ef9f4daf-892c-45f6-b46a-3037c5a93b23",
        ),
        covers_licenses: &[],
        captcha_or_login_gated: false,
        generic_endpoint: Some(GenericEndpointSpec::CkanFacility {
            agency: "CA DHCS MHRC/PHF",
            datastore_search_url: CHHS_DATASTORE_SEARCH_URL,
            resource_id: CA_DHCS_MHRC_PHF_RESOURCE_ID,
            page_size: 500,
            max_records: 1000,
            verification_url_template: Some(
                "https://data.chhs.ca.gov/dataset/licensed-mental-health-rehabilitation-centers-mhrc-and-psychiatric-health-facilities-phf",
            ),
            fields: DHCS_MHRC_PHF_FIELDS,
        }),
        notes: "Unauthenticated CHHS CKAN DataStore for DHCS-licensed Mental Health Rehabilitation Centers and Psychiatric Health Facilities (83 records verified).",
    },
    StateBoardDescriptor {
        id: "ca-dhcs-res-mh",
        state: "CA",
        agency_name: "CA DHCS (Mental Health Program Certification)",
        system_name: "Certified Residential Mental Health Programs (CHHS CKAN)",
        scope: LicensingScope::Facility,
        access_mode: LicensingAccessMode::OpenDataApi,
        format: LicensingDatasetFormat::CkanDatastore,
        portal_url: "https://data.chhs.ca.gov/dataset/certified-and-approved-residential-mental-health-programs",
        api_or_download_url: Some(
            "https://data.chhs.ca.gov/api/3/action/datastore_search?resource_id=c687007e-5b9a-4b5b-a92d-1ccd07008382",
        ),
        covers_licenses: &[],
        captcha_or_login_gated: false,
        generic_endpoint: Some(GenericEndpointSpec::CkanFacility {
            agency: "CA DHCS Residential MH",
            datastore_search_url: CHHS_DATASTORE_SEARCH_URL,
            resource_id: CA_DHCS_RES_MH_RESOURCE_ID,
            page_size: 1000,
            max_records: 2000,
            verification_url_template: Some(
                "https://data.chhs.ca.gov/dataset/certified-and-approved-residential-mental-health-programs",
            ),
            fields: DHCS_RES_MH_FIELDS,
        }),
        notes: "Unauthenticated CHHS CKAN DataStore for DHCS-certified Social Rehabilitation Programs, STRTPs, and residential mental health programs (688 records verified).",
    },
    StateBoardDescriptor {
        id: "ca-cdph",
        state: "CA",
        agency_name: "CA CDPH (Center for Health Care Quality)",
        system_name: "Licensed and Certified Healthcare Facility Locations (CHHS CKAN)",
        scope: LicensingScope::Facility,
        access_mode: LicensingAccessMode::OpenDataApi,
        format: LicensingDatasetFormat::CkanDatastore,
        portal_url: "https://data.chhs.ca.gov/dataset/healthcare-facility-locations",
        api_or_download_url: Some(
            "https://data.chhs.ca.gov/api/3/action/datastore_search?resource_id=f0ae5731-fef8-417f-839d-54a0ed3a126e",
        ),
        covers_licenses: &[],
        captcha_or_login_gated: false,
        generic_endpoint: Some(GenericEndpointSpec::CkanFacility {
            agency: "CA CDPH",
            datastore_search_url: CHHS_DATASTORE_SEARCH_URL,
            resource_id: CA_CDPH_LOCATIONS_RESOURCE_ID,
            page_size: 1000,
            max_records: 20_000,
            verification_url_template: Some(
                "https://data.chhs.ca.gov/dataset/healthcare-facility-locations",
            ),
            fields: CDPH_FACILITY_FIELDS,
        }),
        notes: "Unauthenticated CHHS CKAN DataStore for CDPH-licensed Acute Psychiatric Hospitals (APH), Chemical Dependency Recovery Hospitals (CDRH), Psychiatric Health Facilities (PHF), and Psychology Clinics (14,878 total records verified).",
    },
    StateBoardDescriptor {
        id: "ca-cdss-ccl",
        state: "CA",
        agency_name: "CA CDSS (Community Care Licensing Division)",
        system_name: "Adult Residential & Social Rehabilitation Facilities (CHHS CKAN)",
        scope: LicensingScope::Facility,
        access_mode: LicensingAccessMode::OpenDataApi,
        format: LicensingDatasetFormat::CkanDatastore,
        portal_url: "https://www.ccld.dss.ca.gov/carefacilitysearch/",
        api_or_download_url: Some(
            "https://data.chhs.ca.gov/api/3/action/datastore_search?resource_id=9f5d1d00-6b24-4f44-a158-9cbe4b43f117",
        ),
        covers_licenses: &[],
        captcha_or_login_gated: false,
        generic_endpoint: Some(GenericEndpointSpec::CkanFacility {
            agency: "CA CDSS CCL",
            datastore_search_url: CHHS_DATASTORE_SEARCH_URL,
            resource_id: CA_CDSS_ARF_RESOURCE_ID,
            page_size: 1000,
            max_records: 15_000,
            verification_url_template: Some("https://www.ccld.dss.ca.gov/carefacilitysearch/"),
            fields: CDSS_ARF_FIELDS,
        }),
        notes: "Unauthenticated CHHS CKAN DataStore for CDSS Community Care Licensing Adult Residential Facilities, Social Rehabilitation Facilities (527), Enhanced Behavioral Supports Homes (127), and Community Crisis Homes (38).",
    },
    StateBoardDescriptor {
        id: "ca-mbc",
        state: "CA",
        agency_name: "Medical Board of California (MBC)",
        system_name: "CA DCA BreEZe License Search & Public Info Bulk File (MD)",
        scope: LicensingScope::Provider,
        access_mode: LicensingAccessMode::BulkDownloadExport,
        format: LicensingDatasetFormat::LocalCsvImport,
        portal_url: CA_DCA_SEARCH_PORTAL_URL,
        api_or_download_url: Some(CA_DCA_PUBLIC_INFO_URL),
        covers_licenses: &[
            StaffLicense::MdPsychiatrist,
            StaffLicense::MdAddiction,
            StaffLicense::MdPhysician,
        ],
        captcha_or_login_gated: true,
        generic_endpoint: None,
        notes: "MD physicians only (DO physicians are regulated separately by OMBC). Interactive search.dca.ca.gov is gated by Cloudflare Turnstile; ingest official DCA Public Info bulk export via --import-file.",
    },
    StateBoardDescriptor {
        id: "ca-ombc",
        state: "CA",
        agency_name: "Osteopathic Medical Board of California (OMBC)",
        system_name: "CA DCA BreEZe License Search & Public Info Bulk File (DO)",
        scope: LicensingScope::Provider,
        access_mode: LicensingAccessMode::BulkDownloadExport,
        format: LicensingDatasetFormat::LocalCsvImport,
        portal_url: CA_DCA_SEARCH_PORTAL_URL,
        api_or_download_url: Some(CA_DCA_PUBLIC_INFO_URL),
        covers_licenses: &[
            StaffLicense::DoPsychiatrist,
            StaffLicense::DoAddiction,
            StaffLicense::DoPhysician,
        ],
        captcha_or_login_gated: true,
        generic_endpoint: None,
        notes: "Separate California osteopathic board for DO physicians only. Interactive search.dca.ca.gov is gated by Cloudflare Turnstile; ingest official DCA Public Info bulk export via --import-file.",
    },
    StateBoardDescriptor {
        id: "ca-brn",
        state: "CA",
        agency_name: "California Board of Registered Nursing (BRN)",
        system_name: "CA DCA BreEZe License Search & Public Info Bulk File (RN/NP/PMHNP)",
        scope: LicensingScope::Provider,
        access_mode: LicensingAccessMode::BulkDownloadExport,
        format: LicensingDatasetFormat::LocalCsvImport,
        portal_url: CA_DCA_SEARCH_PORTAL_URL,
        api_or_download_url: Some(CA_DCA_PUBLIC_INFO_URL),
        covers_licenses: &[StaffLicense::Pmhnp, StaffLicense::Np, StaffLicense::Rn],
        captcha_or_login_gated: true,
        generic_endpoint: None,
        notes: "Regulates RN, NP, and PMHNP licenses in California. Interactive search.dca.ca.gov is gated by Cloudflare Turnstile; ingest official DCA Public Info bulk export via --import-file.",
    },
    StateBoardDescriptor {
        id: "ca-bbs",
        state: "CA",
        agency_name: "California Board of Behavioral Sciences (BBS)",
        system_name: "CA DCA BreEZe License Search & Public Info Bulk File (LCSW/LMFT/LPCC)",
        scope: LicensingScope::Provider,
        access_mode: LicensingAccessMode::BulkDownloadExport,
        format: LicensingDatasetFormat::LocalCsvImport,
        portal_url: CA_DCA_SEARCH_PORTAL_URL,
        api_or_download_url: Some(CA_DCA_PUBLIC_INFO_URL),
        covers_licenses: &[
            StaffLicense::Lcsw,
            StaffLicense::Lmft,
            StaffLicense::LpcLmhc,
        ],
        captcha_or_login_gated: true,
        generic_endpoint: None,
        notes: "Regulates LCSW, LMFT, and LPCC licenses in California (associates ASW/AMFT/APCC are kept distinct). Interactive search.dca.ca.gov is gated by Cloudflare Turnstile; ingest official DCA Public Info bulk export via --import-file.",
    },
    StateBoardDescriptor {
        id: "ca-bop",
        state: "CA",
        agency_name: "California Board of Psychology (BOP)",
        system_name: "CA DCA BreEZe License Search & Public Info Bulk File (PhD/PsyD)",
        scope: LicensingScope::Provider,
        access_mode: LicensingAccessMode::BulkDownloadExport,
        format: LicensingDatasetFormat::LocalCsvImport,
        portal_url: CA_DCA_SEARCH_PORTAL_URL,
        api_or_download_url: Some(CA_DCA_PUBLIC_INFO_URL),
        covers_licenses: &[StaffLicense::PhdPsychologist, StaffLicense::PsydPsychologist],
        captcha_or_login_gated: true,
        generic_endpoint: None,
        notes: "Regulates doctoral psychologists in California (PhD and PsyD kept distinct when degree is stated). Interactive search.dca.ca.gov is gated by Cloudflare Turnstile; ingest official DCA Public Info bulk export via --import-file.",
    },
    StateBoardDescriptor {
        id: "ca-pab",
        state: "CA",
        agency_name: "Physician Assistant Board of California (PAB)",
        system_name: "CA DCA BreEZe License Search & Public Info Bulk File (PA)",
        scope: LicensingScope::Provider,
        access_mode: LicensingAccessMode::BulkDownloadExport,
        format: LicensingDatasetFormat::LocalCsvImport,
        portal_url: CA_DCA_SEARCH_PORTAL_URL,
        api_or_download_url: Some(CA_DCA_PUBLIC_INFO_URL),
        covers_licenses: &[StaffLicense::Pa],
        captcha_or_login_gated: true,
        generic_endpoint: None,
        notes: "Regulates Physician Assistants (PA) in California. Interactive search.dca.ca.gov is gated by Cloudflare Turnstile; ingest official DCA Public Info bulk export via --import-file.",
    },
];

/// Returns the complete California facility and provider licensing manifest.
pub fn manifest() -> StateLicensingManifest {
    StateLicensingManifest {
        state: "CA",
        state_name: "California",
        separate_osteopathic_board: true,
        systems: CA_SYSTEMS,
    }
}

/// Executes California's verified live CHHS CKAN facility registries and optionally imports
/// a local CA DCA Public Information bulk export file (`--import-file`).
pub async fn sync(
    client: &reqwest::Client,
    cache: &DiskCache,
    req: &StateSyncRequest,
) -> Result<StateAdapterOutcome, StateLicenseError> {
    let mut outcome = StateAdapterOutcome {
        state: "CA".to_string(),
        ..Default::default()
    };

    let ca_manifest = manifest();

    if let Some(ref import_path) = req.import_file {
        let matched_system = req.system_filter.as_deref().and_then(|filter| {
            ca_manifest
                .systems
                .iter()
                .find(|s| s.id.eq_ignore_ascii_case(filter))
        });

        let (agency_or_board, scope, system_id) = match matched_system {
            Some(sys) => (sys.agency_name, sys.scope, sys.id),
            None if req.skip_facilities && !req.skip_providers => {
                ("CA DCA", LicensingScope::Provider, "ca-dca-import")
            }
            None if req.skip_providers && !req.skip_facilities => (
                "CA Facility Import",
                LicensingScope::Facility,
                "ca-facility-import",
            ),
            None => ("CA DCA", LicensingScope::Both, "ca-import"),
        };

        let mut imported = if scope == LicensingScope::Provider
            && import_path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("csv"))
        {
            let raw = fs::read_to_string(import_path).map_err(|source| StateLicenseError::Io {
                path: import_path.clone(),
                source,
            })?;
            let provider_records = parse_dca_provider_csv(&raw, agency_or_board)?;
            StateAdapterOutcome {
                state: "CA".to_string(),
                provider_records,
                systems_executed: vec![system_id.to_string()],
                ..Default::default()
            }
        } else {
            import_local_file(import_path, "CA", agency_or_board, scope)?
        };

        if imported.systems_executed.is_empty() {
            imported.systems_executed.push(system_id.to_string());
        }
        outcome.merge(imported);
        return Ok(outcome);
    }

    for system in ca_manifest.systems {
        if let Some(ref filter) = req.system_filter {
            if !system.id.eq_ignore_ascii_case(filter) {
                continue;
            }
        }
        if req.skip_facilities && system.scope == LicensingScope::Facility {
            continue;
        }
        if req.skip_providers && system.scope == LicensingScope::Provider {
            continue;
        }

        match system.generic_endpoint {
            Some(ref spec) => {
                let mut part =
                    execute_generic_endpoint(client, cache, "CA", system.id, spec).await?;
                normalize_ca_facility_records(system.id, &mut part.facility_records);
                outcome.merge(part);
            }
            None => {
                if req.import_file.is_none() {
                    outcome.warnings.push(format!(
                        "{} ({}) is Turnstile/CAPTCHA-gated at {}; download official CA DCA Public Info bulk export from {} and pass --import-file",
                        system.id,
                        system.agency_name,
                        system.portal_url,
                        system.api_or_download_url.unwrap_or(CA_DCA_PUBLIC_INFO_URL),
                    ));
                }
            }
        }
    }

    Ok(outcome)
}

/// Applies California-specific post-normalization and behavioral-health relevance filtering
/// to facility records returned by CHHS CKAN endpoints.
fn normalize_ca_facility_records(system_id: &str, records: &mut Vec<FacilityLicenseRecord>) {
    // Keep only behavioral-health, psychiatric, and addiction-relevant CDPH license categories
    // so general medical clinics, surgical clinics, SNFs, hospices, and dialysis units sharing a
    // medical campus address are never attached to behavioral health listings.
    if system_id == "ca-cdph" {
        records.retain(|rec| {
            let lt = rec.license_type.trim().to_ascii_uppercase();
            lt == "APH"
                || lt == "CDRH"
                || lt == "PHF"
                || lt == "CMHC"
                || lt == "PSYCHC"
                || lt.contains("PSYCHIATRIC")
                || lt.contains("PSYCHOLOGY")
                || lt.contains("CHEMICAL DEPENDENCY")
                || lt.contains("MENTAL HEALTH")
                || lt.contains("BEHAVIORAL")
                || lt.contains("ADDICTION")
                || lt.contains("REHABILITATION")
        });
    }

    for rec in records.iter_mut() {
        if rec.licensed_beds == Some(0) {
            rec.licensed_beds = None;
        }
        if system_id == "ca-dhcs-ntp" {
            if rec.license_type.is_empty() || rec.license_type == rec.license_number {
                rec.license_type = "Narcotic Treatment Program (NTP)".to_string();
            }
            if rec.status == LicenseStatus::Unknown {
                rec.status = LicenseStatus::Active;
            }
        } else if system_id == "ca-dhcs-res-mh" && rec.status == LicenseStatus::Unknown {
            // DHCS Certified Residential Mental Health Programs marks active programs "Approved".
            rec.status = LicenseStatus::Active;
        }
    }
}

/// Normalizes California agency and DCA license status strings.
pub fn normalize_ca_status(raw: &str) -> LicenseStatus {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return LicenseStatus::Unknown;
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower == "approved" || lower == "licensed/certified" || lower == "clear" {
        return LicenseStatus::Active;
    }
    if lower.contains("pending") || lower.contains("provisional") {
        return LicenseStatus::Provisional;
    }
    if lower.contains("delinquent")
        || lower.contains("cancelled")
        || lower.contains("canceled")
        || lower.contains("closed")
        || lower.contains("expired")
        || lower.contains("inactive")
        || lower.contains("deceased")
    {
        return LicenseStatus::Inactive;
    }
    LicenseStatus::parse_loose(trimmed)
}

/// Strictly classifies a California clinical provider license without ever conflating
/// MD vs DO (separate Medical and Osteopathic boards), NP/PMHNP/PA vs physicians,
/// PhD vs PsyD, or BBS associate registrations vs independently licensed therapists.
pub fn classify_ca_provider_license(board: &str, credential_label: &str) -> Option<StaffLicense> {
    let b = board.trim().to_ascii_lowercase();
    let c = credential_label.trim().to_ascii_lowercase();

    let is_ombc = b.contains("osteopathic") || b == "ombc" || b == "ca-ombc";
    let is_mbc = !is_ombc && (b.contains("medical board") || b == "mbc" || b == "ca-mbc");
    let is_brn = b.contains("registered nursing") || b == "brn" || b == "ca-brn";
    let is_bbs = b.contains("behavioral sciences") || b == "bbs" || b == "ca-bbs";
    let is_bop = b.contains("psychology") || b == "bop" || b == "ca-bop";
    let is_pab = b.contains("physician assistant") || b == "pab" || b == "ca-pab";
    let is_bvnpt = b.contains("vocational nursing") || b == "bvnpt";

    // Physician Assistant Board (check BEFORE OMBC/MBC so PAs never map to DO or MD)
    if is_pab
        || c.contains("physician assistant")
        || c.contains("physician associate")
        || has_word(&c, "pa")
        || has_word(&c, "pa-c")
    {
        return Some(StaffLicense::Pa);
    }

    // Exclude non-physician allied licenses also regulated by MBC (midwives, sleep techs, etc.)
    let is_non_physician_mbc = c.contains("midwife")
        || c.contains("polysomnograph")
        || c.contains("research psychoanalyst")
        || c.contains("technologist")
        || c.contains("nurse");

    // Separate Osteopathic Medical Board of California (DO only)
    if !is_non_physician_mbc
        && (is_ombc || c.contains("osteopathic") || has_word(&c, "do") || c.contains("d.o."))
    {
        if c.contains("psychiatr") {
            return Some(StaffLicense::DoPsychiatrist);
        }
        if c.contains("addiction") {
            return Some(StaffLicense::DoAddiction);
        }
        return Some(StaffLicense::DoPhysician);
    }

    // Medical Board of California (MD only; require physician/MD credential, never allied roles)
    let cred_is_physician_or_md = has_word(&c, "md")
        || c.contains("m.d.")
        || c.contains("physician")
        || c.contains("surgeon")
        || c.contains("psychiatr")
        || c.contains("addiction medicine")
        || c.is_empty();
    if !is_non_physician_mbc
        && ((is_mbc && cred_is_physician_or_md)
            || has_word(&c, "md")
            || c.contains("m.d.")
            || c.contains("physician and surgeon"))
    {
        if c.contains("psychiatr") {
            return Some(StaffLicense::MdPsychiatrist);
        }
        if c.contains("addiction") {
            return Some(StaffLicense::MdAddiction);
        }
        return Some(StaffLicense::MdPhysician);
    }

    // Board of Registered Nursing (PMHNP vs NP vs RN)
    if is_brn
        || c.contains("nurse practitioner")
        || c.contains("registered nurse")
        || has_word(&c, "pmhnp")
        || has_word(&c, "np")
        || has_word(&c, "rn")
    {
        if c.contains("pmhnp")
            || (c.contains("psychiatric") && c.contains("nurse practitioner"))
            || (c.contains("mental health") && c.contains("nurse practitioner"))
        {
            return Some(StaffLicense::Pmhnp);
        }
        if c.contains("nurse practitioner") || has_word(&c, "np") || has_word(&c, "aprn") {
            return Some(StaffLicense::Np);
        }
        if c.contains("registered nurse") || has_word(&c, "rn") {
            return Some(StaffLicense::Rn);
        }
        return None;
    }

    // Board of Behavioral Sciences (strictly excludes Associate registrations ASW/AMFT/APCC)
    if is_bbs
        || c.contains("clinical social worker")
        || c.contains("marriage and family therapist")
        || c.contains("professional clinical counselor")
        || has_word(&c, "lcsw")
        || has_word(&c, "lmft")
        || has_word(&c, "lpcc")
    {
        if c.contains("associate")
            || has_word(&c, "asw")
            || has_word(&c, "amft")
            || has_word(&c, "apcc")
        {
            return None;
        }
        if has_word(&c, "lcsw") || c.contains("licensed clinical social worker") {
            return Some(StaffLicense::Lcsw);
        }
        if has_word(&c, "lmft") || c.contains("licensed marriage and family therapist") {
            return Some(StaffLicense::Lmft);
        }
        if has_word(&c, "lpcc") || c.contains("licensed professional clinical counselor") {
            return Some(StaffLicense::LpcLmhc);
        }
        return None;
    }

    // Board of Psychology (never guesses PhD vs PsyD when unspecified)
    if is_bop || c.contains("psychologist") || c.contains("psyd") || c.contains("phd") {
        if c.contains("psyd") || c.contains("psy.d") {
            return Some(StaffLicense::PsydPsychologist);
        }
        if c.contains("phd") || c.contains("ph.d") {
            return Some(StaffLicense::PhdPsychologist);
        }
        return None;
    }

    // Board of Vocational Nursing and Psychiatric Technicians
    if is_bvnpt || c.contains("vocational nurse") || has_word(&c, "lvn") || has_word(&c, "lpn") {
        return Some(StaffLicense::Lpn);
    }

    None
}

/// Parses a CHHS CKAN `datastore_search` JSON payload into normalized `FacilityLicenseRecord`s.
pub fn parse_chhs_ckan_facility_json(
    json_text: &str,
    agency: &str,
    fields: &FacilityFieldMap,
    portal_url: &str,
) -> Result<Vec<FacilityLicenseRecord>, StateLicenseError> {
    let root: Value =
        serde_json::from_str(json_text).map_err(|source| StateLicenseError::Json {
            source_label: format!("CHHS CKAN {agency}"),
            source,
        })?;

    let records_val = root
        .get("result")
        .and_then(|r| r.get("records"))
        .and_then(Value::as_array)
        .ok_or_else(|| {
            StateLicenseError::InvalidConfig(format!(
                "CHHS CKAN payload for {agency} missing result.records array"
            ))
        })?;

    let mut out = Vec::with_capacity(records_val.len());
    for row in records_val {
        let Some(obj) = row.as_object() else {
            continue;
        };

        let license_number = first_non_empty(obj, fields.license_number).unwrap_or_default();
        let facility_name = first_non_empty(obj, fields.name).unwrap_or_default();
        if license_number.is_empty() && facility_name.is_empty() {
            continue;
        }

        let dba_name = first_non_empty(obj, fields.dba_or_site_name)
            .filter(|v| !v.eq_ignore_ascii_case("n/a") && v != &facility_name);
        let license_type = first_non_empty(obj, fields.license_type)
            .unwrap_or_else(|| "Licensed Behavioral Health Facility".to_string());
        let status = first_non_empty(obj, fields.status)
            .map(|s| normalize_ca_status(&s))
            .unwrap_or(LicenseStatus::Unknown);
        let street = first_non_empty(obj, fields.street);
        let city = first_non_empty(obj, fields.city);
        let zip = first_non_empty(obj, fields.zip).map(|z| clean_zip5(&z));
        let phone = first_non_empty(obj, fields.phone);
        let licensed_beds =
            first_non_empty(obj, fields.licensed_beds).and_then(|s| parse_bed_count(&s));
        let expires_on = first_non_empty(obj, fields.expires_on).map(|d| clean_date(&d));
        let verification_url = first_non_empty(obj, fields.detail_url)
            .or_else(|| (!portal_url.is_empty()).then(|| portal_url.to_string()));

        out.push(FacilityLicenseRecord {
            state: "CA".to_string(),
            agency: agency.to_string(),
            license_number,
            facility_name,
            dba_name,
            license_type,
            status,
            street,
            city,
            zip,
            phone,
            licensed_beds,
            expires_on,
            disciplinary_actions: None,
            inspection_count: None,
            verification_url,
        });
    }

    Ok(out)
}

/// Parses a California DCA Public Information bulk CSV export into normalized
/// `ProviderLicenseRecord`s with strict California board and credential separation.
pub fn parse_dca_provider_csv(
    csv_text: &str,
    default_board: &str,
) -> Result<Vec<ProviderLicenseRecord>, StateLicenseError> {
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(csv_text.as_bytes());

    let headers: Vec<String> = reader
        .headers()
        .map_err(|source| StateLicenseError::Csv {
            source_label: format!("CA DCA CSV ({default_board})"),
            source,
        })?
        .iter()
        .map(|h| h.trim().to_ascii_lowercase())
        .collect();

    let mut out = Vec::new();
    for result in reader.records() {
        let record = result.map_err(|source| StateLicenseError::Csv {
            source_label: format!("CA DCA CSV ({default_board})"),
            source,
        })?;

        let get = |candidates: &[&str]| -> Option<String> {
            for cand in candidates {
                if let Some(idx) = headers.iter().position(|h| h == cand) {
                    if let Some(val) = record.get(idx) {
                        let trimmed = val.trim();
                        if !trimmed.is_empty() {
                            return Some(trimmed.to_string());
                        }
                    }
                }
            }
            None
        };

        let license_number =
            get(&["license number", "license_number", "lic_no", "license #"]).unwrap_or_default();
        let full_name =
            get(&["full name", "full_name", "name", "licensee name"]).unwrap_or_else(|| {
                let first = get(&["first name", "first_name"]).unwrap_or_default();
                let middle = get(&["middle name", "middle_name"]).unwrap_or_default();
                let last = get(&["last name", "last_name"]).unwrap_or_default();
                [first, middle, last]
                    .into_iter()
                    .filter(|p| !p.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ")
            });

        if license_number.is_empty() || full_name.is_empty() {
            continue;
        }

        let board = get(&["board", "board name", "board_name", "agency"])
            .unwrap_or_else(|| default_board.to_string());
        let credential_label = get(&[
            "license type",
            "license_type",
            "credential",
            "type",
            "license_class",
        ])
        .unwrap_or_default();
        let primary_status = get(&[
            "primary status",
            "primary_status",
            "status",
            "license status",
        ])
        .unwrap_or_default();
        let secondary_status = get(&["secondary status", "secondary_status"]).unwrap_or_default();

        let status = if !secondary_status.is_empty()
            && matches!(
                normalize_ca_status(&secondary_status),
                LicenseStatus::Probation | LicenseStatus::Suspended | LicenseStatus::Revoked
            ) {
            normalize_ca_status(&secondary_status)
        } else {
            normalize_ca_status(&primary_status)
        };

        let disciplinary_flag = get(&[
            "disciplinary action",
            "disciplinary_action",
            "discipline",
            "public_action",
        ])
        .is_some_and(|v| {
            let l = v.to_ascii_lowercase();
            l == "y" || l == "yes" || l == "true" || l == "1"
        }) || matches!(
            status,
            LicenseStatus::Probation | LicenseStatus::Suspended | LicenseStatus::Revoked
        );

        let license = classify_ca_provider_license(&board, &credential_label);

        out.push(ProviderLicenseRecord {
            state: "CA".to_string(),
            board,
            license_number,
            full_name,
            credential_label,
            license,
            status,
            city: get(&["city", "address_city"]),
            zip: get(&["zip", "zip code", "zip_code"]).map(|z| clean_zip5(&z)),
            expires_on: get(&["expiration date", "expiration_date", "expires_on"])
                .map(|d| clean_date(&d)),
            disciplinary_flag,
            verification_url: Some(CA_DCA_SEARCH_PORTAL_URL.to_string()),
        });
    }

    Ok(out)
}

fn first_non_empty(obj: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(val) = obj.get(*key) {
            match val {
                Value::String(s) => {
                    let t = s.trim();
                    if !t.is_empty() {
                        return Some(t.to_string());
                    }
                }
                Value::Number(n) => return Some(n.to_string()),
                _ => {}
            }
        }
    }
    None
}

fn parse_bed_count(raw: &str) -> Option<u32> {
    let cleaned: String = raw
        .trim()
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == ',')
        .filter(|c| c.is_ascii_digit())
        .collect();
    if cleaned.is_empty() {
        None
    } else {
        cleaned.parse::<u32>().ok()
    }
}

fn clean_zip5(raw: &str) -> String {
    let digits: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() >= 5 {
        digits[..5].to_string()
    } else {
        raw.trim().to_string()
    }
}

fn clean_date(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Some((date_part, _)) = trimmed.split_once('T') {
        date_part.to_string()
    } else {
        trimmed.to_string()
    }
}

fn has_word(haystack: &str, needle: &str) -> bool {
    haystack
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '-')
        .any(|token| token == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_has_separate_osteopathic_board_and_verified_endpoints() {
        let m = manifest();
        assert_eq!(m.state, "CA");
        assert!(m.separate_osteopathic_board);
        assert_eq!(m.systems.len(), 12);

        let mbc = m.systems.iter().find(|s| s.id == "ca-mbc").unwrap();
        let ombc = m.systems.iter().find(|s| s.id == "ca-ombc").unwrap();
        assert!(mbc.captcha_or_login_gated);
        assert!(ombc.captcha_or_login_gated);
        assert_eq!(mbc.access_mode, LicensingAccessMode::BulkDownloadExport);
        assert_eq!(ombc.access_mode, LicensingAccessMode::BulkDownloadExport);
        assert!(mbc.covers_licenses.contains(&StaffLicense::MdPsychiatrist));
        assert!(!mbc.covers_licenses.contains(&StaffLicense::DoPsychiatrist));
        assert!(ombc.covers_licenses.contains(&StaffLicense::DoPsychiatrist));
        assert!(!ombc.covers_licenses.contains(&StaffLicense::MdPsychiatrist));

        let dhcs = m.systems.iter().find(|s| s.id == "ca-dhcs-sud").unwrap();
        assert!(!dhcs.captcha_or_login_gated);
        assert!(dhcs.generic_endpoint.is_some());

        let cdph = m.systems.iter().find(|s| s.id == "ca-cdph").unwrap();
        assert!(!cdph.captcha_or_login_gated);
        assert!(cdph.generic_endpoint.is_some());

        for sys in m.systems {
            assert!(sys.portal_url.starts_with("https://"), "{}", sys.id);
        }
    }

    #[test]
    fn parses_live_dhcs_sud_ckan_fixture_with_beds_and_status() {
        // Exact field shape verified from CHHS CKAN resource a4cb74c2-15ec-4a34-8cec-a9f8a4a2d72e.
        let payload = r#"{
            "success": true,
            "result": {
                "total": 2287,
                "records": [
                    {
                        "_id": 1,
                        "Application_Number": "010001DN",
                        "Legal_Entity_Name": "HORIZON SERVICES, INCORPORATED",
                        "Facility_Name": "CHERRY HILL DETOXIFICATION SERVICES PROGRAM",
                        "Facility_Address1": "2035 FAIRMONT DRIVE",
                        "Facility_City": "SAN LEANDRO",
                        "Facility_State": "CA",
                        "Facility_Zip": "94578",
                        "Facility_Phone": "(510) 346-7833",
                        "Type_of_Application": "Licensed",
                        "Program_Code": "RES-DETOX",
                        "Treatment_Capacity": 32,
                        "Total_Capacity": 32,
                        "Expiration_Date": "1/31/2028"
                    }
                ]
            }
        }"#;

        let records = parse_chhs_ckan_facility_json(
            payload,
            "CA DHCS",
            &DHCS_SUD_FIELDS,
            "https://data.chhs.ca.gov/dataset/sud-recovery-treatment-facilities",
        )
        .unwrap();

        assert_eq!(records.len(), 1);
        let rec = &records[0];
        assert_eq!(rec.state, "CA");
        assert_eq!(rec.agency, "CA DHCS");
        assert_eq!(rec.license_number, "010001DN");
        assert_eq!(
            rec.facility_name,
            "CHERRY HILL DETOXIFICATION SERVICES PROGRAM"
        );
        assert_eq!(
            rec.dba_name.as_deref(),
            Some("HORIZON SERVICES, INCORPORATED")
        );
        assert_eq!(rec.license_type, "RES-DETOX");
        assert_eq!(rec.status, LicenseStatus::Active);
        assert_eq!(rec.licensed_beds, Some(32));
        assert_eq!(rec.city.as_deref(), Some("SAN LEANDRO"));
        assert_eq!(rec.zip.as_deref(), Some("94578"));
        assert_eq!(rec.expires_on.as_deref(), Some("1/31/2028"));
    }

    #[test]
    fn parses_live_cdph_ckan_fixture_with_fallback_facid_and_beds() {
        // Exact field shape verified from CHHS CKAN resource f0ae5731-fef8-417f-839d-54a0ed3a126e.
        let payload = r#"{
            "success": true,
            "result": {
                "total": 14878,
                "records": [
                    {
                        "FACID": "090000159",
                        "FINAL_LICENSE_NUMBER": 90000092,
                        "FACNAME": "SAN DIEGO COUNTY PSYCHIATRIC HOSPITAL",
                        "BUSINESS_NAME": "COUNTY OF SAN DIEGO",
                        "FAC_TYPE_CODE": "APH",
                        "FAC_FDR": "ACUTE PSYCHIATRIC HOSPITAL",
                        "CAPACITY": 109,
                        "LICENSE_STATUS_DESCRIPTION": "ACTIVE",
                        "FAC_STATUS_TYPE_CODE": "OPEN",
                        "ADDRESS": "3853 ROSECRANS ST",
                        "CITY": "SAN DIEGO",
                        "ZIP": 92110,
                        "CONTACT_PHONE_NUMBER": "(619) 692-8473",
                        "LICENSE_EXPIRATION_DATE": "2027-03-29T00:00:00"
                    },
                    {
                        "FACID": "050000667",
                        "FINAL_LICENSE_NUMBER": null,
                        "FACNAME": "SANTA BARBARA COUNTY PSYCHIATRIC HEALTH FACILITY",
                        "BUSINESS_NAME": null,
                        "FAC_TYPE_CODE": "PHF",
                        "FAC_FDR": "ACUTE PSYCHIATRIC HOSPITAL",
                        "CAPACITY": 16,
                        "LICENSE_STATUS_DESCRIPTION": null,
                        "FAC_STATUS_TYPE_CODE": "OPEN",
                        "ADDRESS": "315 CAMINO DEL REMEDIO",
                        "CITY": "SANTA BARBARA",
                        "ZIP": 93110,
                        "CONTACT_PHONE_NUMBER": "(805) 681-5244",
                        "LICENSE_EXPIRATION_DATE": null
                    }
                ]
            }
        }"#;

        let records = parse_chhs_ckan_facility_json(
            payload,
            "CA CDPH",
            &CDPH_FACILITY_FIELDS,
            "https://data.chhs.ca.gov/dataset/healthcare-facility-locations",
        )
        .unwrap();

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].license_number, "90000092");
        assert_eq!(records[0].licensed_beds, Some(109));
        assert_eq!(records[0].status, LicenseStatus::Active);
        assert_eq!(records[0].expires_on.as_deref(), Some("2027-03-29"));

        // Falls back to FACID and FAC_STATUS_TYPE_CODE ("OPEN" -> Active) when license fields are null.
        assert_eq!(records[1].license_number, "050000667");
        assert_eq!(records[1].licensed_beds, Some(16));
        assert_eq!(records[1].status, LicenseStatus::Active);
    }

    #[test]
    fn parses_live_dhcs_mhrc_and_cdss_srf_fixtures() {
        let mhrc_payload = r#"{
            "success": true,
            "result": {
                "records": [
                    {
                        "Record ID": "02 099 021",
                        "Legal Name": "7TH AVE CENTER, LLC",
                        "Facility Name": "7th Ave Care Center LLC",
                        "Service Type": "MHRC",
                        "Client/Patient Capacity": "99",
                        "Type of Application": "Licensed",
                        "Physical Address": "1171 Seventh Avenue",
                        "Physical City": "Santa Cruz",
                        "Physical Zip": "95062",
                        "Facility Phone": "(831) 476-1700"
                    }
                ]
            }
        }"#;
        let mhrc = parse_chhs_ckan_facility_json(
            mhrc_payload,
            "CA DHCS MHRC/PHF",
            &DHCS_MHRC_PHF_FIELDS,
            "https://data.chhs.ca.gov/",
        )
        .unwrap();
        assert_eq!(mhrc.len(), 1);
        assert_eq!(mhrc[0].license_number, "02 099 021");
        assert_eq!(mhrc[0].licensed_beds, Some(99));
        assert_eq!(mhrc[0].status, LicenseStatus::Active);

        let cdss_payload = r#"{
            "success": true,
            "result": {
                "records": [
                    {
                        "facility_number": "347001234",
                        "facility_name": "OAK PARK SOCIAL REHAB",
                        "licensee": "OAK PARK WELLNESS INC",
                        "facility_type": "SOCIAL REHABILITATION FACILITY",
                        "facility_status": "LICENSED",
                        "facility_capacity": "15",
                        "facility_address": "123 OAK AVE",
                        "facility_city": "SACRAMENTO",
                        "facility_zip": "95817",
                        "facility_telephone_number": "(916) 555-0101"
                    }
                ]
            }
        }"#;
        let cdss = parse_chhs_ckan_facility_json(
            cdss_payload,
            "CA CDSS CCL",
            &CDSS_ARF_FIELDS,
            "https://www.ccld.dss.ca.gov/carefacilitysearch/",
        )
        .unwrap();
        assert_eq!(cdss.len(), 1);
        assert_eq!(cdss[0].licensed_beds, Some(15));
        assert_eq!(cdss[0].license_type, "SOCIAL REHABILITATION FACILITY");
        assert_eq!(cdss[0].status, LicenseStatus::Active);
    }

    #[test]
    fn strictly_classifies_ca_md_vs_do_and_bbs_and_dca_csv() {
        assert_eq!(
            classify_ca_provider_license("Medical Board of California", "Physician and Surgeon"),
            Some(StaffLicense::MdPhysician)
        );
        assert_eq!(
            classify_ca_provider_license(
                "Osteopathic Medical Board of California",
                "Osteopathic Physician and Surgeon"
            ),
            Some(StaffLicense::DoPhysician)
        );
        assert_eq!(
            classify_ca_provider_license(
                "California Board of Registered Nursing",
                "Psychiatric Mental Health Nurse Practitioner (PMHNP)"
            ),
            Some(StaffLicense::Pmhnp)
        );
        assert_eq!(
            classify_ca_provider_license(
                "California Board of Behavioral Sciences",
                "Licensed Clinical Social Worker"
            ),
            Some(StaffLicense::Lcsw)
        );
        assert_eq!(
            classify_ca_provider_license(
                "California Board of Behavioral Sciences",
                "Licensed Marriage and Family Therapist"
            ),
            Some(StaffLicense::Lmft)
        );
        assert_eq!(
            classify_ca_provider_license(
                "California Board of Behavioral Sciences",
                "Licensed Professional Clinical Counselor"
            ),
            Some(StaffLicense::LpcLmhc)
        );
        // Associate BBS registrations must never be misclassified as independent LCSW/LMFT/LPCC.
        assert_eq!(
            classify_ca_provider_license(
                "California Board of Behavioral Sciences",
                "Associate Clinical Social Worker"
            ),
            None
        );
        // Board of Psychology never guesses PhD vs PsyD when unspecified.
        assert_eq!(
            classify_ca_provider_license("California Board of Psychology", "Psychologist"),
            None
        );
        assert_eq!(
            classify_ca_provider_license("California Board of Psychology", "Psychologist (PsyD)"),
            Some(StaffLicense::PsydPsychologist)
        );
        assert_eq!(
            classify_ca_provider_license(
                "Physician Assistant Board of California",
                "Physician Assistant"
            ),
            Some(StaffLicense::Pa)
        );

        let csv_fixture = "\
Board,License Type,License Number,Primary Status,Secondary Status,First Name,Last Name,City,Zip,Expiration Date,Disciplinary Action
Medical Board of California,Physician and Surgeon (Psychiatry),A123456,Current,,Elena,Reyes,San Francisco,94107,2027-08-31,N
Osteopathic Medical Board of California,Osteopathic Physician and Surgeon,20A98765,Current,Probation,Marcus,Chen,Los Angeles,90024,2026-12-31,Y
California Board of Behavioral Sciences,Licensed Clinical Social Worker,LCSW88412,Clear,,Priya,Patel,San Diego,92101,2028-02-28,N
California Board of Behavioral Sciences,Associate Clinical Social Worker,ASW10922,Current,,Jordan,Lee,Oakland,94612,2027-01-31,N
";
        let providers = parse_dca_provider_csv(csv_fixture, "CA DCA").unwrap();
        assert_eq!(providers.len(), 4);

        assert_eq!(providers[0].license, Some(StaffLicense::MdPsychiatrist));
        assert_eq!(providers[0].status, LicenseStatus::Active);
        assert!(!providers[0].disciplinary_flag);

        assert_eq!(providers[1].license, Some(StaffLicense::DoPhysician));
        assert_eq!(providers[1].status, LicenseStatus::Probation);
        assert!(providers[1].disciplinary_flag);

        assert_eq!(providers[2].license, Some(StaffLicense::Lcsw));
        assert_eq!(providers[2].status, LicenseStatus::Active);

        assert_eq!(providers[3].license, None);
    }

    #[test]
    fn normalizes_ca_license_statuses() {
        assert_eq!(normalize_ca_status("ACTIVE"), LicenseStatus::Active);
        assert_eq!(normalize_ca_status("OPEN"), LicenseStatus::Active);
        assert_eq!(normalize_ca_status("Licensed"), LicenseStatus::Active);
        assert_eq!(
            normalize_ca_status("Licensed/Certified"),
            LicenseStatus::Active
        );
        assert_eq!(normalize_ca_status("Approved"), LicenseStatus::Active);
        assert_eq!(normalize_ca_status("Clear"), LicenseStatus::Active);
        assert_eq!(normalize_ca_status("Probation"), LicenseStatus::Probation);
        assert_eq!(normalize_ca_status("Suspended"), LicenseStatus::Suspended);
        assert_eq!(normalize_ca_status("Revoked"), LicenseStatus::Revoked);
        assert_eq!(normalize_ca_status("Delinquent"), LicenseStatus::Inactive);
        assert_eq!(normalize_ca_status("Closed"), LicenseStatus::Inactive);
    }
}
