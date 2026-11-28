//! Shared types and adapter contract for all 50-state (+ DC/territories) facility and
//! provider licensing search adapters.
//!
//! Design principles:
//! - Never bypass CAPTCHAs, Turnstile, login walls, or anti-bot challenges. Portals gated
//!   by anti-bot checks are modeled honestly as `BulkDownloadExport` (supporting official
//!   bulk CSV/JSON files via `--import-file`) or `PortalLookupOnly` (emitting official
//!   state verification URLs).
//! - Strictly preserve credential distinctions (MD vs DO, Psychiatrist vs PMHNP/NP/PA,
//!   PhD vs PsyD, LCSW vs LPC/LMHC vs LMFT), especially in states with separate Medical
//!   and Osteopathic boards.
//! - Cache every upstream API/file response on disk under `.cache/recovery-index/state-licensing/`.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::schema::{LicenseStatus, StaffLicense};

/// Whether a state licensing system tracks facilities, individual practitioners, or both.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LicensingScope {
    Facility,
    Provider,
    Both,
}

impl LicensingScope {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Facility => "Facility",
            Self::Provider => "Provider",
            Self::Both => "Facility & Provider",
        }
    }
}

/// How an adapter can legally and reliably ingest records from a state licensing system.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LicensingAccessMode {
    /// Public, unauthenticated Open Data API (Socrata, ArcGIS REST, CKAN, or direct CSV/JSON).
    OpenDataApi,
    /// Public, unauthenticated JSON/REST/HTML search API without CAPTCHA gating.
    DirectSearchApi,
    /// Official public bulk CSV/TSV/XLSX/ZIP download or local export file (`--import-file`).
    BulkDownloadExport,
    /// Interactive web portal (often CAPTCHA-gated); adapter exposes official verification URLs
    /// and accepts local exports without attempting any anti-bot bypass.
    PortalLookupOnly,
}

impl LicensingAccessMode {
    pub const fn label(self) -> &'static str {
        match self {
            Self::OpenDataApi => "Open Data API",
            Self::DirectSearchApi => "Direct Search API",
            Self::BulkDownloadExport => "Bulk Export / File Import",
            Self::PortalLookupOnly => "Official Portal Lookup",
        }
    }
}

/// Concrete wire format for an executable or file-import licensing adapter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LicensingDatasetFormat {
    SocrataJson,
    ArcGisFeatureLayer,
    CkanDatastore,
    CsvUrl,
    JsonUrl,
    CustomApi,
    LocalCsvImport,
    PortalWebOnly,
}

/// Field-name mapping for declarative Open Data / CSV / ArcGIS / CKAN facility datasets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FacilityFieldMap {
    pub license_number: &'static [&'static str],
    pub name: &'static [&'static str],
    pub dba_or_site_name: &'static [&'static str],
    pub license_type: &'static [&'static str],
    pub status: &'static [&'static str],
    pub street: &'static [&'static str],
    pub city: &'static [&'static str],
    pub zip: &'static [&'static str],
    pub phone: &'static [&'static str],
    pub licensed_beds: &'static [&'static str],
    pub expires_on: &'static [&'static str],
    pub detail_url: &'static [&'static str],
}

/// Field-name mapping for declarative Open Data / CSV / ArcGIS / CKAN provider datasets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderFieldMap {
    pub license_number: &'static [&'static str],
    pub full_name: &'static [&'static str],
    pub first_name: &'static [&'static str],
    pub last_name: &'static [&'static str],
    pub credential_or_type: &'static [&'static str],
    pub board_name: &'static [&'static str],
    pub status: &'static [&'static str],
    pub city: &'static [&'static str],
    pub zip: &'static [&'static str],
    pub expires_on: &'static [&'static str],
    pub disciplinary_flag: &'static [&'static str],
}

/// Declarative endpoint specification executed by the generic Open Data / CSV / ArcGIS engine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GenericEndpointSpec {
    /// Socrata SODA JSON endpoint (`https://<host>/resource/<dataset-id>.json`).
    SocrataFacility {
        agency: &'static str,
        endpoint_url: &'static str,
        where_clause: Option<&'static str>,
        page_size: usize,
        max_records: usize,
        verification_url_template: Option<&'static str>,
        fields: FacilityFieldMap,
    },
    /// Socrata SODA JSON endpoint for individual clinical practitioners.
    SocrataProvider {
        board: &'static str,
        endpoint_url: &'static str,
        where_clause: Option<&'static str>,
        page_size: usize,
        max_records: usize,
        verification_url_template: Option<&'static str>,
        fields: ProviderFieldMap,
    },
    /// ArcGIS FeatureServer / MapServer `/query` REST layer returning JSON features.
    ArcGisFacility {
        agency: &'static str,
        layer_query_url: &'static str,
        where_clause: &'static str,
        page_size: usize,
        max_records: usize,
        verification_url_template: Option<&'static str>,
        fields: FacilityFieldMap,
    },
    /// CKAN DataStore `datastore_search` JSON API (`resource_id=...`).
    CkanFacility {
        agency: &'static str,
        datastore_search_url: &'static str,
        resource_id: &'static str,
        page_size: usize,
        max_records: usize,
        verification_url_template: Option<&'static str>,
        fields: FacilityFieldMap,
    },
    /// Direct public CSV download URL (streamed/cached on disk, parsed with `csv::Reader`).
    CsvFacility {
        agency: &'static str,
        csv_url: &'static str,
        filter_keywords: &'static [&'static str],
        verification_url_template: Option<&'static str>,
        fields: FacilityFieldMap,
    },
    /// Direct public CSV download URL for individual clinical practitioners.
    CsvProvider {
        board: &'static str,
        csv_url: &'static str,
        verification_url_template: Option<&'static str>,
        fields: ProviderFieldMap,
    },
}

/// Metadata describing one official state facility or provider licensing system.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateBoardDescriptor {
    pub id: &'static str,
    pub state: &'static str,
    pub agency_name: &'static str,
    pub system_name: &'static str,
    pub scope: LicensingScope,
    pub access_mode: LicensingAccessMode,
    pub format: LicensingDatasetFormat,
    /// Official public search / license verification portal URL for patients and families.
    pub portal_url: &'static str,
    /// Direct API or bulk-download URL when publicly exposed without CAPTCHA.
    pub api_or_download_url: Option<&'static str>,
    /// Clinical license categories verified by this board (empty for facility-only agencies).
    pub covers_licenses: &'static [StaffLicense],
    /// Whether the interactive web portal is known to be CAPTCHA or login gated.
    pub captcha_or_login_gated: bool,
    /// Optional declarative live adapter spec executed by `generic.rs`.
    pub generic_endpoint: Option<GenericEndpointSpec>,
    pub notes: &'static str,
}

/// Complete licensing manifest for one US state, DC, or territory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateLicensingManifest {
    pub state: &'static str,
    pub state_name: &'static str,
    /// True when the state regulates DOs under a separate Board of Osteopathic Medicine
    /// (critical for keeping MD vs DO strictly separated).
    pub separate_osteopathic_board: bool,
    pub systems: &'static [StateBoardDescriptor],
}

/// Normalized facility licensing record returned by any state adapter before catalog matching.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FacilityLicenseRecord {
    pub state: String,
    pub agency: String,
    pub license_number: String,
    pub facility_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dba_name: Option<String>,
    pub license_type: String,
    pub status: LicenseStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub street: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub city: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zip: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phone: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub licensed_beds: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_on: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disciplinary_actions: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inspection_count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_url: Option<String>,
}

/// Normalized individual provider licensing record returned by any state adapter.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderLicenseRecord {
    pub state: String,
    pub board: String,
    pub license_number: String,
    pub full_name: String,
    pub credential_label: String,
    /// Strictly mapped `StaffLicense` (never guesses MD vs DO or conflates NP/PA with MD).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<StaffLicense>,
    pub status: LicenseStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub city: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zip: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_on: Option<String>,
    #[serde(default)]
    pub disciplinary_flag: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_url: Option<String>,
}

/// Output of running a state's licensing adapter(s).
#[derive(Clone, Debug, Default)]
pub struct StateAdapterOutcome {
    pub state: String,
    pub facility_records: Vec<FacilityLicenseRecord>,
    pub provider_records: Vec<ProviderLicenseRecord>,
    pub cache_hits: usize,
    pub fetched: usize,
    pub systems_executed: Vec<String>,
    pub warnings: Vec<String>,
}

impl StateAdapterOutcome {
    pub fn merge(&mut self, other: Self) {
        self.facility_records.extend(other.facility_records);
        self.provider_records.extend(other.provider_records);
        self.cache_hits += other.cache_hits;
        self.fetched += other.fetched;
        self.systems_executed.extend(other.systems_executed);
        self.warnings.extend(other.warnings);
    }
}

/// Options passed into state licensing adapter execution.
#[derive(Clone, Debug, Default)]
pub struct StateSyncRequest {
    /// Optional local CSV/JSON bulk export file supplied via `--import-file`.
    pub import_file: Option<PathBuf>,
    /// Optional agency/board filter (e.g. `"az-azdhs"` or `"co-dora"`).
    pub system_filter: Option<String>,
    /// Skip provider boards (only run facility registries).
    pub skip_providers: bool,
    /// Skip facility registries (only run provider boards).
    pub skip_facilities: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum StateLicenseError {
    #[error("HTTP request failed for {url}")]
    Http {
        url: String,
        #[source]
        source: reqwest::Error,
    },
    #[error("HTTP status {status} for {url}")]
    HttpStatus { url: String, status: u16 },
    #[error("CAPTCHA or bot-challenge gate encountered at {url}; use --import-file or official portal lookup")]
    CaptchaBlocked { url: String },
    #[error("could not parse JSON from {source_label}")]
    Json {
        source_label: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("could not parse CSV from {source_label}")]
    Csv {
        source_label: String,
        #[source]
        source: csv::Error,
    },
    #[error("could not read {path}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{0}")]
    InvalidConfig(String),
}
