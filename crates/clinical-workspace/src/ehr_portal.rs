//! Patient EHR portal link-through: searchable health-system directory,
//! standards-compliant SMART on FHIR R4 OAuth 2.0 + PKCE (`S256`) session
//! builder, and partial-failure-tolerant HL7 FHIR R4 Bundle ingestion.
//!
//! Pure Rust with zero external crypto or HTTP crates so the exact same code
//! runs identically in native SSR, browser WebAssembly, Android/Desktop Tauri,
//! and static-musl/FreeBSD binaries.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{ChartRecord, ConfidentialityClass, Medication, PatientChart, Problem, RecordKind};

/// Default read-only SMART App Launch v2 (CRUDS) scopes requested when a
/// patient links their own doctor's portal. Dabney.moe never requests write scopes.
pub const SMART_READ_ONLY_SCOPES: &str = "openid fhirUser offline_access \
    patient/Patient.rs patient/Observation.rs patient/Condition.rs \
    patient/MedicationRequest.rs patient/DiagnosticReport.rs \
    patient/DocumentReference.rs patient/AllergyIntolerance.rs";

/// Read-only SMART App Launch v1 explicit-resource scopes (used for EHRs that
/// require SMART v1 `.read` scopes or disallow wildcard `patient/*.read`, such
/// as Oracle Health / Cerner and MEDITECH Expanse).
pub const SMART_V1_READ_ONLY_SCOPES: &str = "openid fhirUser offline_access \
    patient/Patient.read patient/Observation.read patient/Condition.read \
    patient/MedicationRequest.read patient/DiagnosticReport.read \
    patient/DocumentReference.read patient/AllergyIntolerance.read";

/// Read-only SMART v2 scopes tailored for CMS Medicare Blue Button 2.0
/// (claims-only FHIR profile: `Patient`, `Coverage`, `ExplanationOfBenefit`).
pub const MEDICARE_BLUE_BUTTON_SCOPES: &str =
    "openid profile patient/Patient.rs patient/Coverage.rs patient/ExplanationOfBenefit.rs";

/// SMART App Launch scope grammar version supported by a vendor profile.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SmartScopeVersion {
    /// SMART v1: `patient/<Resource>.read`.
    V1,
    /// SMART v2 CRUDS (HTI-1 / ONC): `patient/<Resource>.rs`.
    #[default]
    V2,
}

impl SmartScopeVersion {
    pub const fn label(self) -> &'static str {
        match self {
            Self::V1 => "SMART v1 (.read)",
            Self::V2 => "SMART v2 (.rs)",
        }
    }
}

/// Typed integration capability supported by an EHR vendor or organization.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntegrationCapability {
    SmartOnFhirR4V2Pkce,
    SmartOnFhirR4V1Pkce,
    SmartHealthLink,
    KipuApiV3Hmac,
    VendorRestApi,
    CcdaXmlImport,
    Hl7V2Import,
    AutomatedHipaaRoi,
    Part2SudProtected,
}

impl IntegrationCapability {
    pub const fn badge_label(self) -> &'static str {
        match self {
            Self::SmartOnFhirR4V2Pkce => "SMART FHIR v2",
            Self::SmartOnFhirR4V1Pkce => "SMART FHIR v1",
            Self::SmartHealthLink => "SHLink",
            Self::KipuApiV3Hmac => "Kipu API v3",
            Self::VendorRestApi => "Vendor REST",
            Self::CcdaXmlImport => "C-CDA XML",
            Self::Hl7V2Import => "HL7 v2",
            Self::AutomatedHipaaRoi => "ROI Fallback",
            Self::Part2SudProtected => "42 CFR Part 2",
        }
    }
}

/// Explicit transport & protocol capability profile for an EHR organization so
/// callers and UI never guess whether a vendor uses SMART on FHIR, Kipu API v3,
/// C-CDA/HL7 v2 upload, or Automated ROI.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EhrTransportProfile {
    pub primary_capability: IntegrationCapability,
    pub capabilities: Vec<IntegrationCapability>,
    pub smart_scope_version: Option<SmartScopeVersion>,
    pub requires_explicit_resource_scopes: bool,
    pub requires_aud_param: bool,
    pub claims_only: bool,
    pub part2_sensitive: bool,
    pub notes: String,
}

impl Default for EhrTransportProfile {
    fn default() -> Self {
        PortalVendor::CustomSmartFhir.default_transport_profile()
    }
}

impl EhrTransportProfile {
    #[must_use]
    pub fn supports_smart_on_fhir(&self) -> bool {
        self.capabilities.iter().any(|c| {
            matches!(
                c,
                IntegrationCapability::SmartOnFhirR4V2Pkce
                    | IntegrationCapability::SmartOnFhirR4V1Pkce
            )
        })
    }

    #[must_use]
    pub fn supports_kipu_api(&self) -> bool {
        self.capabilities
            .contains(&IntegrationCapability::KipuApiV3Hmac)
    }

    #[must_use]
    pub fn default_smart_scopes(&self) -> &'static str {
        if self.claims_only {
            MEDICARE_BLUE_BUTTON_SCOPES
        } else if self.smart_scope_version == Some(SmartScopeVersion::V1) {
            SMART_V1_READ_ONLY_SCOPES
        } else {
            SMART_READ_ONLY_SCOPES
        }
    }
}

/// Supported patient-portal, specialty, and behavioral-health EHR vendors.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PortalVendor {
    EpicMyChart,
    OracleHealth,
    AthenaHealth,
    Healow,
    Meditech,
    NextGen,
    Veradigm,
    ElationHealth,
    VaHealth,
    MedicareBlueButton,
    KipuEmr,
    HintHealth,
    SimplePractice,
    DrChrono,
    ModMed,
    CustomSmartFhir,
}

impl PortalVendor {
    pub const fn label(self) -> &'static str {
        match self {
            Self::EpicMyChart => "Epic / MyChart",
            Self::OracleHealth => "Oracle Health (Cerner)",
            Self::AthenaHealth => "athenahealth",
            Self::Healow => "healow / eClinicalWorks",
            Self::Meditech => "MEDITECH Expanse",
            Self::NextGen => "NextGen Healthcare",
            Self::Veradigm => "Veradigm (Allscripts)",
            Self::ElationHealth => "Elation Health",
            Self::VaHealth => "VA Health (My HealtheVet)",
            Self::MedicareBlueButton => "Medicare Blue Button 2.0",
            Self::KipuEmr => "KipuEMR Behavioral Health",
            Self::HintHealth => "Hint Health (DPC)",
            Self::SimplePractice => "SimplePractice",
            Self::DrChrono => "DrChrono",
            Self::ModMed => "ModMed (EMA / gGastro)",
            Self::CustomSmartFhir => "Custom SMART on FHIR R4",
        }
    }

    pub const fn short_badge(self) -> &'static str {
        match self {
            Self::EpicMyChart => "Epic MyChart",
            Self::OracleHealth => "Oracle Health",
            Self::AthenaHealth => "athenahealth",
            Self::Healow => "healow",
            Self::Meditech => "MEDITECH",
            Self::NextGen => "NextGen",
            Self::Veradigm => "Veradigm",
            Self::ElationHealth => "Elation",
            Self::VaHealth => "VA FHIR",
            Self::MedicareBlueButton => "Medicare",
            Self::KipuEmr => "KipuEMR",
            Self::HintHealth => "Hint Health",
            Self::SimplePractice => "SimplePractice",
            Self::DrChrono => "DrChrono",
            Self::ModMed => "ModMed",
            Self::CustomSmartFhir => "SMART FHIR",
        }
    }

    pub const fn is_part2_sensitive(self) -> bool {
        matches!(self, Self::KipuEmr)
    }

    #[must_use]
    pub fn default_transport_profile(self) -> EhrTransportProfile {
        use IntegrationCapability::*;
        match self {
            Self::EpicMyChart => EhrTransportProfile {
                primary_capability: SmartOnFhirR4V2Pkce,
                capabilities: vec![
                    SmartOnFhirR4V2Pkce,
                    SmartOnFhirR4V1Pkce,
                    SmartHealthLink,
                    CcdaXmlImport,
                    AutomatedHipaaRoi,
                ],
                smart_scope_version: Some(SmartScopeVersion::V2),
                requires_explicit_resource_scopes: false,
                requires_aud_param: true,
                claims_only: false,
                part2_sensitive: false,
                notes: "Supports SMART v1 & v2 PKCE S256 with strict exact `aud` matching.".into(),
            },
            Self::OracleHealth => EhrTransportProfile {
                primary_capability: SmartOnFhirR4V1Pkce,
                capabilities: vec![
                    SmartOnFhirR4V1Pkce,
                    SmartOnFhirR4V2Pkce,
                    CcdaXmlImport,
                    AutomatedHipaaRoi,
                ],
                smart_scope_version: Some(SmartScopeVersion::V1),
                requires_explicit_resource_scopes: true,
                requires_aud_param: true,
                claims_only: false,
                part2_sensitive: false,
                notes: "Explicit resource scopes required (wildcard `*` rejected by Cerner/Oracle Health).".into(),
            },
            Self::AthenaHealth => EhrTransportProfile {
                primary_capability: SmartOnFhirR4V2Pkce,
                capabilities: vec![
                    SmartOnFhirR4V2Pkce,
                    CcdaXmlImport,
                    AutomatedHipaaRoi,
                ],
                smart_scope_version: Some(SmartScopeVersion::V2),
                requires_explicit_resource_scopes: true,
                requires_aud_param: true,
                claims_only: false,
                part2_sensitive: false,
                notes: "SMART v2 CRUDS with PKCE S256 and US Core granular scopes.".into(),
            },
            Self::Healow => EhrTransportProfile {
                primary_capability: SmartOnFhirR4V2Pkce,
                capabilities: vec![
                    SmartOnFhirR4V2Pkce,
                    SmartOnFhirR4V1Pkce,
                    CcdaXmlImport,
                    AutomatedHipaaRoi,
                ],
                smart_scope_version: Some(SmartScopeVersion::V2),
                requires_explicit_resource_scopes: false,
                requires_aud_param: true,
                claims_only: false,
                part2_sensitive: false,
                notes: "Patient-facing healow / eClinicalWorks R4 with per-practice tenant `aud`.".into(),
            },
            Self::Meditech => EhrTransportProfile {
                primary_capability: SmartOnFhirR4V1Pkce,
                capabilities: vec![
                    SmartOnFhirR4V1Pkce,
                    SmartOnFhirR4V2Pkce,
                    CcdaXmlImport,
                    Hl7V2Import,
                    AutomatedHipaaRoi,
                ],
                smart_scope_version: Some(SmartScopeVersion::V1),
                requires_explicit_resource_scopes: true,
                requires_aud_param: true,
                claims_only: false,
                part2_sensitive: false,
                notes: "MEDITECH Expanse / Greenfield: confidential clients may require both PKCE S256 and client_secret.".into(),
            },
            Self::NextGen => EhrTransportProfile {
                primary_capability: SmartOnFhirR4V1Pkce,
                capabilities: vec![
                    SmartOnFhirR4V1Pkce,
                    CcdaXmlImport,
                    AutomatedHipaaRoi,
                ],
                smart_scope_version: Some(SmartScopeVersion::V1),
                requires_explicit_resource_scopes: true,
                requires_aud_param: true,
                claims_only: false,
                part2_sensitive: false,
                notes: "NextGen US Core R4 with mandatory PKCE S256 and strict redirect_uri check.".into(),
            },
            Self::Veradigm => EhrTransportProfile {
                primary_capability: SmartOnFhirR4V1Pkce,
                capabilities: vec![
                    SmartOnFhirR4V1Pkce,
                    CcdaXmlImport,
                    AutomatedHipaaRoi,
                ],
                smart_scope_version: Some(SmartScopeVersion::V1),
                requires_explicit_resource_scopes: true,
                requires_aud_param: true,
                claims_only: false,
                part2_sensitive: false,
                notes: "Veradigm / Allscripts TouchWorks & EHR SMART R4 per-product tenants.".into(),
            },
            Self::ElationHealth => EhrTransportProfile {
                primary_capability: SmartOnFhirR4V1Pkce,
                capabilities: vec![
                    SmartOnFhirR4V1Pkce,
                    VendorRestApi,
                    CcdaXmlImport,
                    AutomatedHipaaRoi,
                ],
                smart_scope_version: Some(SmartScopeVersion::V1),
                requires_explicit_resource_scopes: true,
                requires_aud_param: true,
                claims_only: false,
                part2_sensitive: false,
                notes: "Elation Health SMART on FHIR R4 standalone + EHR launch with mandatory PKCE S256.".into(),
            },
            Self::VaHealth => EhrTransportProfile {
                primary_capability: SmartOnFhirR4V2Pkce,
                capabilities: vec![
                    SmartOnFhirR4V2Pkce,
                    SmartOnFhirR4V1Pkce,
                    AutomatedHipaaRoi,
                ],
                smart_scope_version: Some(SmartScopeVersion::V2),
                requires_explicit_resource_scopes: true,
                requires_aud_param: true,
                claims_only: false,
                part2_sensitive: false,
                notes: "VA Lighthouse Patient Health FHIR R4 with PKCE S256 (v1 .read & v2 .rs).".into(),
            },
            Self::MedicareBlueButton => EhrTransportProfile {
                primary_capability: SmartOnFhirR4V2Pkce,
                capabilities: vec![SmartOnFhirR4V2Pkce],
                smart_scope_version: Some(SmartScopeVersion::V2),
                requires_explicit_resource_scopes: true,
                requires_aud_param: true,
                claims_only: true,
                part2_sensitive: false,
                notes: "CMS Blue Button 2.0 claims-only FHIR R4 (Patient, Coverage, ExplanationOfBenefit).".into(),
            },
            Self::KipuEmr => EhrTransportProfile {
                primary_capability: KipuApiV3Hmac,
                capabilities: vec![
                    KipuApiV3Hmac,
                    Part2SudProtected,
                    CcdaXmlImport,
                    AutomatedHipaaRoi,
                ],
                smart_scope_version: None,
                requires_explicit_resource_scopes: false,
                requires_aud_param: false,
                claims_only: false,
                part2_sensitive: true,
                notes: "Kipu Health API v3 (`APIAuth` HMAC-SHA1/SHA256) / v4 Bearer. Records carry 42 CFR Part 2 SUD protections.".into(),
            },
            Self::HintHealth => EhrTransportProfile {
                primary_capability: VendorRestApi,
                capabilities: vec![VendorRestApi, CcdaXmlImport, AutomatedHipaaRoi],
                smart_scope_version: None,
                requires_explicit_resource_scopes: false,
                requires_aud_param: false,
                claims_only: false,
                part2_sensitive: false,
                notes: "Direct Primary Care admin/membership REST API (not clinical FHIR R4); use C-CDA import or Automated ROI for clinical notes.".into(),
            },
            Self::SimplePractice => EhrTransportProfile {
                primary_capability: AutomatedHipaaRoi,
                capabilities: vec![CcdaXmlImport, AutomatedHipaaRoi],
                smart_scope_version: None,
                requires_explicit_resource_scopes: false,
                requires_aud_param: false,
                claims_only: false,
                part2_sensitive: false,
                notes: "No open patient FHIR API; import exported C-CDA / clinical files or send an Automated HIPAA ROI request.".into(),
            },
            Self::DrChrono => EhrTransportProfile {
                primary_capability: VendorRestApi,
                capabilities: vec![
                    VendorRestApi,
                    SmartOnFhirR4V1Pkce,
                    CcdaXmlImport,
                    AutomatedHipaaRoi,
                ],
                smart_scope_version: Some(SmartScopeVersion::V1),
                requires_explicit_resource_scopes: true,
                requires_aud_param: true,
                claims_only: false,
                part2_sensitive: false,
                notes: "Dual-track: OAuth2 REST API v4 + C-CDA export + review-gated EverHealth FHIR R4.".into(),
            },
            Self::ModMed => EhrTransportProfile {
                primary_capability: SmartOnFhirR4V2Pkce,
                capabilities: vec![
                    SmartOnFhirR4V2Pkce,
                    VendorRestApi,
                    CcdaXmlImport,
                    AutomatedHipaaRoi,
                ],
                smart_scope_version: Some(SmartScopeVersion::V2),
                requires_explicit_resource_scopes: true,
                requires_aud_param: true,
                claims_only: false,
                part2_sensitive: false,
                notes: "Partner-gated certified US Core FHIR R4 (`sso.ema.md` OAuth) + C-CDA exchange.".into(),
            },
            Self::CustomSmartFhir => EhrTransportProfile {
                primary_capability: SmartOnFhirR4V2Pkce,
                capabilities: vec![
                    SmartOnFhirR4V2Pkce,
                    SmartOnFhirR4V1Pkce,
                    SmartHealthLink,
                    CcdaXmlImport,
                    Hl7V2Import,
                    AutomatedHipaaRoi,
                ],
                smart_scope_version: Some(SmartScopeVersion::V2),
                requires_explicit_resource_scopes: false,
                requires_aud_param: true,
                claims_only: false,
                part2_sensitive: false,
                notes: "Custom HTTPS SMART on FHIR R4 endpoint with PKCE S256.".into(),
            },
        }
    }

    pub const fn all() -> &'static [Self] {
        &[
            Self::EpicMyChart,
            Self::OracleHealth,
            Self::AthenaHealth,
            Self::Healow,
            Self::Meditech,
            Self::NextGen,
            Self::Veradigm,
            Self::ElationHealth,
            Self::VaHealth,
            Self::MedicareBlueButton,
            Self::KipuEmr,
            Self::HintHealth,
            Self::SimplePractice,
            Self::DrChrono,
            Self::ModMed,
            Self::CustomSmartFhir,
        ]
    }
}

/// Directory entry describing a patient portal's transport capabilities,
/// endpoints, and Health Information Management (HIM) medical-records contact
/// details (also reusable as an ROI target).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortalOrganization {
    pub id: String,
    pub name: String,
    pub city_state: String,
    pub vendor: PortalVendor,
    pub fhir_base_url: String,
    pub authorize_url: String,
    pub token_url: String,
    pub him_email: Option<String>,
    pub him_fax: Option<String>,
    pub him_address: Option<String>,
    pub is_sandbox: bool,
    #[serde(default)]
    pub transport: EhrTransportProfile,
}

impl PortalOrganization {
    #[must_use]
    pub fn supports_smart_on_fhir(&self) -> bool {
        self.transport.supports_smart_on_fhir()
            && !self.fhir_base_url.trim().is_empty()
            && !self.authorize_url.trim().is_empty()
            && !self.token_url.trim().is_empty()
    }
}

/// Patient-reported or portal-imported allergy / intolerance entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AllergyEntry {
    pub id: String,
    pub substance: String,
    pub reaction: String,
    pub criticality: String,
    pub recorded_at: String,
    pub source_label: String,
    #[serde(default, skip_serializing_if = "ConfidentialityClass::is_standard")]
    pub confidentiality: ConfidentialityClass,
}

/// How a portal connection obtained its records.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PortalConnectionMode {
    LiveSmartFhir,
    ImportedFhirBundle,
    InteractiveSandboxDemo,
    KipuApiV3,
    ImportedCcdaXml,
    ImportedHl7V2,
    SmartHealthLink,
}

impl PortalConnectionMode {
    pub const fn label(self) -> &'static str {
        match self {
            Self::LiveSmartFhir => "Live SMART on FHIR",
            Self::ImportedFhirBundle => "Imported FHIR R4 file",
            Self::InteractiveSandboxDemo => "Interactive Sandbox (Zero PHI)",
            Self::KipuApiV3 => "KipuEMR API v3 (42 CFR Part 2)",
            Self::ImportedCcdaXml => "Imported C-CDA R2.1 XML",
            Self::ImportedHl7V2 => "Imported HL7 v2.x Message",
            Self::SmartHealthLink => "SMART Health Link (shlink:/)",
        }
    }
}

/// Explicit lifecycle state of one linked patient portal.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum PortalConnectionStatus {
    PendingAuth {
        oauth_state: String,
        authorize_url: String,
    },
    Connected {
        connected_at: String,
        last_synced_at: String,
        mode: PortalConnectionMode,
        imported_record_count: usize,
        imported_medication_count: usize,
        imported_problem_count: usize,
        imported_allergy_count: usize,
    },
    NeedsReauth {
        last_synced_at: String,
        reason: String,
    },
    Disconnected {
        disconnected_at: String,
        retained_record_count: usize,
    },
}

impl PortalConnectionStatus {
    pub const fn badge_label(&self) -> &'static str {
        match self {
            Self::PendingAuth { .. } => "Awaiting portal sign-in",
            Self::Connected { .. } => "Connected · Read-only",
            Self::NeedsReauth { .. } => "Reconnect needed",
            Self::Disconnected { .. } => "Disconnected (history kept)",
        }
    }

    pub const fn is_connected(&self) -> bool {
        matches!(self, Self::Connected { .. })
    }
}

/// One doctor/hospital portal linked to the patient's local workspace.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkedPortalConnection {
    pub organization_id: String,
    pub organization_name: String,
    pub vendor: PortalVendor,
    pub fhir_base_url: String,
    pub status: PortalConnectionStatus,
}

/// Complete PKCE `S256` SMART on FHIR R4 authorization request prepared for a
/// patient portal sign-in.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SmartPkceSession {
    pub organization_id: String,
    pub organization_name: String,
    pub vendor: PortalVendor,
    pub fhir_base_url: String,
    pub authorize_endpoint: String,
    pub token_endpoint: String,
    pub redirect_uri: String,
    pub client_id: String,
    pub scopes: String,
    pub state: String,
    pub code_verifier: String,
    pub code_challenge: String,
    pub code_challenge_method: String,
    pub authorize_url: String,
    pub read_only: bool,
}

/// Non-fatal parse warning for one individual FHIR resource inside a Bundle.
/// Sibling valid resources still import cleanly.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FhirResourceWarning {
    pub index: usize,
    pub resource_type: String,
    pub resource_id: Option<String>,
    pub reason: String,
}

/// Structured result produced by clinical document importers (FHIR R4, KipuEMR,
/// C-CDA R2.1 XML, and HL7 v2.x).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FhirImportResult {
    pub source_label: String,
    pub portal_id: String,
    pub patient_name: Option<String>,
    pub patient_birth_date: Option<String>,
    pub records: Vec<ChartRecord>,
    pub medications: Vec<Medication>,
    pub problems: Vec<Problem>,
    pub allergies: Vec<AllergyEntry>,
    pub total_entries_seen: usize,
    pub warnings: Vec<FhirResourceWarning>,
    #[serde(default, skip_serializing_if = "ConfidentialityClass::is_standard")]
    pub confidentiality: ConfidentialityClass,
}

/// Counts of newly merged items vs deduplicated repeats when merging a portal
/// FHIR import into a patient chart.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeCounts {
    pub added_records: usize,
    pub added_medications: usize,
    pub added_problems: usize,
    pub added_allergies: usize,
    pub skipped_duplicates: usize,
}

/// Curated searchable US health-system, SMART on FHIR R4, behavioral-health,
/// and non-FHIR/partial-FHIR EHR directory.
#[must_use]
pub fn portal_directory() -> Vec<PortalOrganization> {
    vec![
        PortalOrganization {
            id: "org-epic-sandbox".into(),
            name: "Epic MyChart FHIR R4 Sandbox (Demo)".into(),
            city_state: "Verona, WI · Sandbox".into(),
            vendor: PortalVendor::EpicMyChart,
            fhir_base_url: "https://fhir.epic.com/interconnect-fhir-oauth/api/FHIR/R4".into(),
            authorize_url: "https://fhir.epic.com/interconnect-fhir-oauth/oauth2/authorize".into(),
            token_url: "https://fhir.epic.com/interconnect-fhir-oauth/oauth2/token".into(),
            him_email: Some("him-sandbox@dabney-demo.invalid".into()),
            him_fax: Some("+1-555-010-1001".into()),
            him_address: Some("1979 Milky Way, Verona, WI 53593 (Sandbox)".into()),
            is_sandbox: true,
            transport: PortalVendor::EpicMyChart.default_transport_profile(),
        },
        PortalOrganization {
            id: "org-oracle-sandbox".into(),
            name: "Oracle Health / Cerner R4 Sandbox (Demo)".into(),
            city_state: "Kansas City, MO · Sandbox".into(),
            vendor: PortalVendor::OracleHealth,
            fhir_base_url: "https://fhir-myrecord.cerner.com/r4/ec2458f2-1e24-41c8-b71b-0e701af7583d".into(),
            authorize_url: "https://authorization.cerner.com/tenants/ec2458f2-1e24-41c8-b71b-0e701af7583d/protocols/oauth2/profiles/smart-v1/personas/patient/authorize".into(),
            token_url: "https://authorization.cerner.com/tenants/ec2458f2-1e24-41c8-b71b-0e701af7583d/protocols/oauth2/profiles/smart-v1/token".into(),
            him_email: Some("records-oracle-sandbox@dabney-demo.invalid".into()),
            him_fax: Some("+1-555-010-1002".into()),
            him_address: Some("2800 Rockcreek Pkwy, North Kansas City, MO 64117 (Sandbox)".into()),
            is_sandbox: true,
            transport: PortalVendor::OracleHealth.default_transport_profile(),
        },
        PortalOrganization {
            id: "org-stanford-mychart".into(),
            name: "Stanford Health Care (MyHealth / Epic)".into(),
            city_state: "Palo Alto, CA".into(),
            vendor: PortalVendor::EpicMyChart,
            fhir_base_url: "https://sfd.stanfordmed.org/FHIR/api/FHIR/R4".into(),
            authorize_url: "https://sfd.stanfordmed.org/FHIR/oauth2/authorize".into(),
            token_url: "https://sfd.stanfordmed.org/FHIR/oauth2/token".into(),
            him_email: Some("ROI@stanfordhealthcare.org".into()),
            him_fax: Some("+1-650-725-9821".into()),
            him_address: Some("300 Pasteur Dr, Stanford, CA 94305".into()),
            is_sandbox: false,
            transport: PortalVendor::EpicMyChart.default_transport_profile(),
        },
        PortalOrganization {
            id: "org-ucsf-mychart".into(),
            name: "UCSF Health (MyChart / Epic)".into(),
            city_state: "San Francisco, CA".into(),
            vendor: PortalVendor::EpicMyChart,
            fhir_base_url: "https://unified-api.ucsf.edu/clinical/apex/api/FHIR/R4".into(),
            authorize_url: "https://unified-api.ucsf.edu/clinical/apex/oauth2/authorize".into(),
            token_url: "https://unified-api.ucsf.edu/clinical/apex/oauth2/token".into(),
            him_email: Some("medicalrecords@ucsf.edu".into()),
            him_fax: Some("+1-415-353-2189".into()),
            him_address: Some("400 Parnassus Ave, Room A88, San Francisco, CA 94143".into()),
            is_sandbox: false,
            transport: PortalVendor::EpicMyChart.default_transport_profile(),
        },
        PortalOrganization {
            id: "org-cedars-mychart".into(),
            name: "Cedars-Sinai Medical Center (My CS-Link)".into(),
            city_state: "Los Angeles, CA".into(),
            vendor: PortalVendor::EpicMyChart,
            fhir_base_url: "https://cslinkmobile.csmc.edu/fhirproxy/api/FHIR/R4".into(),
            authorize_url: "https://cslinkmobile.csmc.edu/fhirproxy/oauth2/authorize".into(),
            token_url: "https://cslinkmobile.csmc.edu/fhirproxy/oauth2/token".into(),
            him_email: Some("releaseofinfo@cshs.org".into()),
            him_fax: Some("+1-310-423-0113".into()),
            him_address: Some("8700 Beverly Blvd, Los Angeles, CA 90048".into()),
            is_sandbox: false,
            transport: PortalVendor::EpicMyChart.default_transport_profile(),
        },
        PortalOrganization {
            id: "org-mayo-mychart".into(),
            name: "Mayo Clinic (Patient Online Services)".into(),
            city_state: "Rochester, MN".into(),
            vendor: PortalVendor::EpicMyChart,
            fhir_base_url: "https://pep.api.mayo.edu/epic-fhir/api/FHIR/R4".into(),
            authorize_url: "https://pep.api.mayo.edu/epic-fhir/oauth2/authorize".into(),
            token_url: "https://pep.api.mayo.edu/epic-fhir/oauth2/token".into(),
            him_email: Some("himroi@mayo.edu".into()),
            him_fax: Some("+1-507-284-0161".into()),
            him_address: Some("200 First St SW, Rochester, MN 55905".into()),
            is_sandbox: false,
            transport: PortalVendor::EpicMyChart.default_transport_profile(),
        },
        PortalOrganization {
            id: "org-kaiser-ca".into(),
            name: "Kaiser Permanente (Northern & Southern CA)".into(),
            city_state: "Oakland & Pasadena, CA".into(),
            vendor: PortalVendor::EpicMyChart,
            fhir_base_url: "https://api.kp.org/fhir/r4".into(),
            authorize_url: "https://api.kp.org/oauth2/authorize".into(),
            token_url: "https://api.kp.org/oauth2/token".into(),
            him_email: Some("roi-ca@kp.org".into()),
            him_fax: Some("+1-855-822-2652".into()),
            him_address: Some("One Kaiser Plaza, Oakland, CA 94612".into()),
            is_sandbox: false,
            transport: PortalVendor::EpicMyChart.default_transport_profile(),
        },
        PortalOrganization {
            id: "org-athena-clinic".into(),
            name: "Pacific Primary & Metabolic Care (athenahealth)".into(),
            city_state: "Pasadena, CA · Sandbox".into(),
            vendor: PortalVendor::AthenaHealth,
            fhir_base_url: "https://api.platform.athenahealth.com/fhir/r4".into(),
            authorize_url: "https://api.platform.athenahealth.com/oauth2/v1/authorize".into(),
            token_url: "https://api.platform.athenahealth.com/oauth2/v1/token".into(),
            him_email: Some("records@pacific-metabolic-demo.invalid".into()),
            him_fax: Some("+1-555-014-2200".into()),
            him_address: Some("100 E California Blvd, Pasadena, CA 91105 (Demo)".into()),
            is_sandbox: true,
            transport: PortalVendor::AthenaHealth.default_transport_profile(),
        },
        PortalOrganization {
            id: "org-healow-sandbox".into(),
            name: "eClinicalWorks / healow R4 Sandbox (Demo)".into(),
            city_state: "Westborough, MA · Sandbox".into(),
            vendor: PortalVendor::Healow,
            fhir_base_url: "https://staging-fhir.ecwcloud.com/fhir/r4/FFBJCD".into(),
            authorize_url: "https://staging-oauthserver.ecwcloud.com/oauth/oauth2/authorize".into(),
            token_url: "https://staging-oauthserver.ecwcloud.com/oauth/oauth2/token".into(),
            him_email: Some("him-healow-demo@dabney-demo.invalid".into()),
            him_fax: Some("+1-555-014-3100".into()),
            him_address: Some("2 Technology Dr, Westborough, MA 01581 (Sandbox)".into()),
            is_sandbox: true,
            transport: PortalVendor::Healow.default_transport_profile(),
        },
        PortalOrganization {
            id: "org-meditech-expanse".into(),
            name: "MEDITECH Expanse Community Hospital (Greenfield Profile)".into(),
            city_state: "Canton, MA · Facility SMART".into(),
            vendor: PortalVendor::Meditech,
            fhir_base_url: "https://fhir.meditech.com/fhir/r4".into(),
            authorize_url: "https://fhir.meditech.com/oauth2/authorize".into(),
            token_url: "https://fhir.meditech.com/oauth2/token".into(),
            him_email: Some("him-meditech-demo@dabney-demo.invalid".into()),
            him_fax: Some("+1-555-014-4100".into()),
            him_address: Some("7 Blue Hill River Rd, Canton, MA 02021 (Demo)".into()),
            is_sandbox: true,
            transport: PortalVendor::Meditech.default_transport_profile(),
        },
        PortalOrganization {
            id: "org-nextgen-clinic".into(),
            name: "NextGen Healthcare Outpatient Group".into(),
            city_state: "Atlanta, GA · US Core R4".into(),
            vendor: PortalVendor::NextGen,
            fhir_base_url: "https://fhir.nextgen.com/nge/prod/fhir-api-r4/fhir/r4".into(),
            authorize_url: "https://fhir.nextgen.com/nge/prod/patient-oauth/authorize".into(),
            token_url: "https://fhir.nextgen.com/nge/prod/patient-oauth/token".into(),
            him_email: Some("records-nextgen-demo@dabney-demo.invalid".into()),
            him_fax: Some("+1-555-014-5100".into()),
            him_address: Some("3525 Piedmont Rd NE, Atlanta, GA 30305 (Demo)".into()),
            is_sandbox: true,
            transport: PortalVendor::NextGen.default_transport_profile(),
        },
        PortalOrganization {
            id: "org-veradigm-sandbox".into(),
            name: "Veradigm / Allscripts Open R4 Sandbox (Demo)".into(),
            city_state: "Chicago, IL · Sandbox".into(),
            vendor: PortalVendor::Veradigm,
            fhir_base_url: "https://fhir.fhirpoint.open.allscripts.com/fhirroute/open/Sandbox".into(),
            authorize_url: "https://fhir.fhirpoint.open.allscripts.com/fhirroute/authorization/authorize".into(),
            token_url: "https://fhir.fhirpoint.open.allscripts.com/fhirroute/authorization/token".into(),
            him_email: Some("him-veradigm-demo@dabney-demo.invalid".into()),
            him_fax: Some("+1-555-014-6100".into()),
            him_address: Some("222 W Merchandise Mart Plaza, Chicago, IL 60654 (Sandbox)".into()),
            is_sandbox: true,
            transport: PortalVendor::Veradigm.default_transport_profile(),
        },
        PortalOrganization {
            id: "org-elation-sandbox".into(),
            name: "Elation Health Primary Care Sandbox (Demo)".into(),
            city_state: "San Francisco, CA · Sandbox".into(),
            vendor: PortalVendor::ElationHealth,
            fhir_base_url: "https://sandbox.fhir.elationemr.com/fhir".into(),
            authorize_url: "https://sandbox.fhir.elationemr.com/oauth2/authorize".into(),
            token_url: "https://sandbox.fhir.elationemr.com/oauth2/token".into(),
            him_email: Some("records-elation-demo@dabney-demo.invalid".into()),
            him_fax: Some("+1-555-014-7100".into()),
            him_address: Some("548 Market St, San Francisco, CA 94104 (Sandbox)".into()),
            is_sandbox: true,
            transport: PortalVendor::ElationHealth.default_transport_profile(),
        },
        PortalOrganization {
            id: "org-va-health".into(),
            name: "U.S. Department of Veterans Affairs (VA Health)".into(),
            city_state: "Nationwide, US · Sandbox".into(),
            vendor: PortalVendor::VaHealth,
            fhir_base_url: "https://sandbox-api.va.gov/services/fhir/v0/r4".into(),
            authorize_url: "https://sandbox-api.va.gov/oauth2/patient-health/v0/authorization".into(),
            token_url: "https://sandbox-api.va.gov/oauth2/patient-health/v0/token".into(),
            him_email: None,
            him_fax: None,
            him_address: Some("Local VA Medical Center Release of Information Office".into()),
            is_sandbox: true,
            transport: PortalVendor::VaHealth.default_transport_profile(),
        },
        PortalOrganization {
            id: "org-medicare-bb".into(),
            name: "Medicare Blue Button 2.0 (CMS Claims & Meds)".into(),
            city_state: "Nationwide, US · Claims FHIR".into(),
            vendor: PortalVendor::MedicareBlueButton,
            fhir_base_url: "https://sandbox.bluebutton.cms.gov/v2/fhir".into(),
            authorize_url: "https://sandbox.bluebutton.cms.gov/v2/o/authorize/".into(),
            token_url: "https://sandbox.bluebutton.cms.gov/v2/o/token/".into(),
            him_email: None,
            him_fax: None,
            him_address: Some("Centers for Medicare & Medicaid Services, Baltimore, MD".into()),
            is_sandbox: true,
            transport: PortalVendor::MedicareBlueButton.default_transport_profile(),
        },
        PortalOrganization {
            id: "org-kipu-sandbox".into(),
            name: "Harborview Behavioral & Recovery (KipuEMR · 42 CFR Part 2)".into(),
            city_state: "Behavioral Health · Kipu API v3 Sandbox".into(),
            vendor: PortalVendor::KipuEmr,
            fhir_base_url: "https://api.kipuworks.com".into(),
            authorize_url: String::new(),
            token_url: String::new(),
            him_email: Some("part2-records@harborview-demo.invalid".into()),
            him_fax: Some("+1-555-019-4200".into()),
            him_address: Some("240 Harbor Recovery Way, Portland, OR 97205 (Demo)".into()),
            is_sandbox: true,
            transport: PortalVendor::KipuEmr.default_transport_profile(),
        },
        PortalOrganization {
            id: "org-hint-dpc".into(),
            name: "Oakridge Direct Primary Care (Hint Health)".into(),
            city_state: "Denver, CO · DPC REST + ROI".into(),
            vendor: PortalVendor::HintHealth,
            fhir_base_url: "https://api.hint.com/api/provider".into(),
            authorize_url: String::new(),
            token_url: String::new(),
            him_email: Some("records@oakridge-dpc-demo.invalid".into()),
            him_fax: Some("+1-555-019-5300".into()),
            him_address: Some("1600 Stout St, Denver, CO 80202 (Demo)".into()),
            is_sandbox: true,
            transport: PortalVendor::HintHealth.default_transport_profile(),
        },
        PortalOrganization {
            id: "org-simplepractice-clinic".into(),
            name: "Willow Therapy & Counseling (SimplePractice)".into(),
            city_state: "Seattle, WA · C-CDA / Automated ROI".into(),
            vendor: PortalVendor::SimplePractice,
            fhir_base_url: String::new(),
            authorize_url: String::new(),
            token_url: String::new(),
            him_email: Some("roi@willow-therapy-demo.invalid".into()),
            him_fax: Some("+1-555-019-6400".into()),
            him_address: Some("1201 Third Ave, Seattle, WA 98101 (Demo)".into()),
            is_sandbox: true,
            transport: PortalVendor::SimplePractice.default_transport_profile(),
        },
        PortalOrganization {
            id: "org-drchrono-clinic".into(),
            name: "Bayside Multispecialty Clinic (DrChrono)".into(),
            city_state: "San Mateo, CA · REST v4 + FHIR R4".into(),
            vendor: PortalVendor::DrChrono,
            fhir_base_url: "https://drchrono-fhirpresentation.everhealthsoftware.com/fhir/drchrono/demo-practice/r4".into(),
            authorize_url: "https://drchrono.com/o/authorize/".into(),
            token_url: "https://drchrono.com/o/token/".into(),
            him_email: Some("him@bayside-drchrono-demo.invalid".into()),
            him_fax: Some("+1-555-019-7500".into()),
            him_address: Some("500 S El Camino Real, San Mateo, CA 94402 (Demo)".into()),
            is_sandbox: true,
            transport: PortalVendor::DrChrono.default_transport_profile(),
        },
        PortalOrganization {
            id: "org-modmed-specialty".into(),
            name: "Coastal Dermatology & GI (ModMed EMA / gGastro)".into(),
            city_state: "Boca Raton, FL · Partner-Gated FHIR R4".into(),
            vendor: PortalVendor::ModMed,
            fhir_base_url: "https://fhirmp.mmi.prod.fhir.ema-api.com/fhir/r4".into(),
            authorize_url: "https://sso.ema.md/oauth2/authorize".into(),
            token_url: "https://sso.ema.md/oauth2/token".into(),
            him_email: Some("records@coastal-modmed-demo.invalid".into()),
            him_fax: Some("+1-555-019-8600".into()),
            him_address: Some("4850 T-Rex Ave, Boca Raton, FL 33431 (Demo)".into()),
            is_sandbox: true,
            transport: PortalVendor::ModMed.default_transport_profile(),
        },
    ]
}

/// Look up a single portal organization by stable ID.
#[must_use]
pub fn find_portal_organization(id: &str) -> Option<PortalOrganization> {
    portal_directory().into_iter().find(|o| o.id == id)
}

/// Filter the portal directory by free-text query (name, city/state, vendor) and
/// optional vendor filter.
#[must_use]
pub fn search_portal_directory(
    query: &str,
    vendor_filter: Option<PortalVendor>,
) -> Vec<PortalOrganization> {
    let q = query.trim().to_ascii_lowercase();
    let terms: Vec<&str> = q.split_whitespace().filter(|t| !t.is_empty()).collect();
    portal_directory()
        .into_iter()
        .filter(|org| vendor_filter.is_none_or(|v| org.vendor == v))
        .filter(|org| {
            if terms.is_empty() {
                return true;
            }
            let hay = format!(
                "{} {} {} {}",
                org.name,
                org.city_state,
                org.vendor.label(),
                org.fhir_base_url
            )
            .to_ascii_lowercase();
            terms.iter().all(|term| hay.contains(term))
        })
        .collect()
}

/// Validates that an outbound remote URL is strictly HTTPS and SSRF-safe:
/// rejects non-`https://` schemes, whitespace, embedded credentials (`@`),
/// fragments (`#`), loopback/private/link-local/metadata/CGNAT IPv4/IPv6
/// literals, and `.local`/`.internal` hosts.
pub fn validate_public_https_url(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("Endpoint URL must not be empty.".into());
    }
    if trimmed.chars().any(char::is_whitespace) {
        return Err("Endpoint URL must not contain whitespace.".into());
    }
    if trimmed.contains('#') {
        return Err("Endpoint URL must not contain a fragment.".into());
    }
    let lower = trimmed.to_ascii_lowercase();
    if !lower.starts_with("https://") {
        return Err("Only HTTPS endpoints are permitted.".into());
    }
    let after_scheme = &trimmed[8..];
    let authority = after_scheme.split(['/', '?']).next().unwrap_or_default();
    if authority.is_empty() || authority.contains('@') {
        return Err("Endpoint URL must contain a valid host without embedded credentials.".into());
    }
    let (host, port) = extract_authority_host_port(authority)?;
    if let Some(p) = port {
        if p != 443 {
            return Err(format!(
                "Only standard HTTPS port 443 is permitted (got port {p})."
            ));
        }
    }
    if is_private_or_local_host(host) {
        return Err(
            "Private, loopback, link-local, or cloud metadata hosts are not permitted.".into(),
        );
    }
    Ok(trimmed.trim_end_matches('/').to_string())
}

fn extract_authority_host_port(authority: &str) -> Result<(&str, Option<u16>), String> {
    if let Some(rest) = authority.strip_prefix('[') {
        let Some(end) = rest.find(']') else {
            return Err("Invalid bracketed IPv6 host in URL.".into());
        };
        let host = &rest[..end];
        let after = &rest[end + 1..];
        let port = if after.is_empty() {
            None
        } else if let Some(p_str) = after.strip_prefix(':') {
            Some(
                p_str
                    .parse::<u16>()
                    .map_err(|_| "Invalid port number in URL.".to_string())?,
            )
        } else {
            return Err("Malformed IPv6 authority in URL.".into());
        };
        Ok((host, port))
    } else if let Some((host, p_str)) = authority.rsplit_once(':') {
        let port = p_str
            .parse::<u16>()
            .map_err(|_| "Invalid port number in URL.".to_string())?;
        Ok((host, Some(port)))
    } else {
        Ok((authority, None))
    }
}

fn is_forbidden_ip_addr(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => {
            let o = v4.octets();
            v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_unspecified()
                || v4.is_multicast()
                || o[0] == 0
                || o[0] >= 240
                || (o[0] == 100 && (o[1] & 0b1100_0000) == 0b0100_0000)
                || (o[0] == 192 && o[1] == 0 && o[2] == 0)
                || (o[0] == 198 && (o[1] == 18 || o[1] == 19))
        }
        std::net::IpAddr::V6(v6) => {
            let segs = v6.segments();
            let is_nat64 = (segs[0] == 0x0064 && segs[1] == 0xff9b && segs[2] == 0 && segs[3] == 0)
                || (segs[0] == 0x0064 && segs[1] == 0xff9b && segs[2] == 0x0001);
            let is_ipv4_compat = segs[0..6] == [0, 0, 0, 0, 0, 0] && (segs[6] != 0 || segs[7] > 1);
            let embedded_v4 = v6.to_ipv4_mapped().or_else(|| {
                (is_ipv4_compat || is_nat64).then(|| {
                    std::net::Ipv4Addr::new(
                        (segs[6] >> 8) as u8,
                        (segs[6] & 0xff) as u8,
                        (segs[7] >> 8) as u8,
                        (segs[7] & 0xff) as u8,
                    )
                })
            });
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_unique_local()
                || v6.is_unicast_link_local()
                || v6.is_multicast()
                || is_nat64
                || is_ipv4_compat
                || embedded_v4.is_some_and(|v4| is_forbidden_ip_addr(std::net::IpAddr::V4(v4)))
        }
    }
}

fn looks_like_noncanonical_ipv4(clean: &str) -> bool {
    let labels: Vec<&str> = clean.split('.').collect();
    if labels.is_empty() || labels.len() > 4 {
        return false;
    }
    labels.iter().all(|lbl| {
        !lbl.is_empty()
            && (lbl.bytes().all(|b| b.is_ascii_digit())
                || lbl.strip_prefix("0x").is_some_and(|hex| {
                    !hex.is_empty() && hex.bytes().all(|b| b.is_ascii_hexdigit())
                }))
    })
}

fn is_private_or_local_host(host: &str) -> bool {
    let clean = host.trim_end_matches('.').to_ascii_lowercase();
    if clean.is_empty()
        || clean == "localhost"
        || clean.ends_with(".localhost")
        || clean.ends_with(".local")
        || clean.ends_with(".internal")
        || clean == "metadata.google.internal"
    {
        return true;
    }
    if let Ok(ip) = clean.parse::<std::net::IpAddr>() {
        return is_forbidden_ip_addr(ip);
    }
    // Reject octal/hex/short-form/integer IPv4 obfuscations (`127.1`, `0177.0.0.1`, `0x7f.0.0.1`, `2130706433`).
    if looks_like_noncanonical_ipv4(&clean) {
        return true;
    }
    false
}

/// Validates that a SMART on FHIR endpoint URL is safe (`https://`, or
/// loopback `http://127.0.0.1` / `http://localhost` for local sandboxes) with
/// no embedded credentials or whitespace.
pub fn validate_smart_endpoint_url(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("FHIR endpoint URL must not be empty.".into());
    }
    if trimmed.chars().any(char::is_whitespace) {
        return Err("FHIR endpoint URL must not contain whitespace.".into());
    }
    let lower = trimmed.to_ascii_lowercase();
    let is_https = lower.starts_with("https://");
    let is_local_http =
        lower.starts_with("http://127.0.0.1") || lower.starts_with("http://localhost");
    if !is_https && !is_local_http {
        return Err(
            "SMART on FHIR endpoints must use HTTPS (only localhost/127.0.0.1 allows HTTP).".into(),
        );
    }
    let after_scheme = trimmed
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or_default();
    let authority = after_scheme.split('/').next().unwrap_or_default();
    if authority.is_empty() || authority.contains('@') {
        return Err(
            "FHIR endpoint URL must contain a valid host without embedded credentials.".into(),
        );
    }
    Ok(trimmed.trim_end_matches('/').to_string())
}

/// Typed SMART Health Link (`shlink:/`) capability flags per the SMART Health
/// Links specification.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SmartHealthLinkFlag {
    /// `L`: Long-term link whose manifest contents may update over time.
    LongTerm,
    /// `P`: Passcode required in the manifest `POST` body.
    PasscodeRequired,
    /// `U`: Direct single-file URL (use `GET <url>?recipient=...`, never combined with `P`).
    DirectFileUrl,
}

impl SmartHealthLinkFlag {
    pub const fn code(self) -> char {
        match self {
            Self::LongTerm => 'L',
            Self::PasscodeRequired => 'P',
            Self::DirectFileUrl => 'U',
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::LongTerm => "Long-term (L)",
            Self::PasscodeRequired => "Passcode protected (P)",
            Self::DirectFileUrl => "Direct file URL (U)",
        }
    }
}

/// Parsed, validated SMART Health Link (`shlink:/<base64url(JSON)>`) payload.
///
/// Implements redacted [`std::fmt::Debug`] so the 32-byte AES-256 `key` is
/// never exposed in debug logs.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SmartHealthLinkPayload {
    pub url: String,
    pub key: String,
    pub exp: Option<u64>,
    pub flag: Option<String>,
    pub flags: Vec<SmartHealthLinkFlag>,
    pub label: Option<String>,
    pub v: u32,
    pub viewer_origin: Option<String>,
}

impl std::fmt::Debug for SmartHealthLinkPayload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SmartHealthLinkPayload")
            .field("url", &self.url)
            .field("key", &"[REDACTED]")
            .field("exp", &self.exp)
            .field("flag", &self.flag)
            .field("flags", &self.flags)
            .field("label", &self.label)
            .field("v", &self.v)
            .field("viewer_origin", &self.viewer_origin)
            .finish()
    }
}

impl SmartHealthLinkPayload {
    #[must_use]
    pub fn requires_passcode(&self) -> bool {
        self.flags.contains(&SmartHealthLinkFlag::PasscodeRequired)
    }

    #[must_use]
    pub fn is_direct_file_url(&self) -> bool {
        self.flags.contains(&SmartHealthLinkFlag::DirectFileUrl)
    }

    #[must_use]
    pub fn is_long_term(&self) -> bool {
        self.flags.contains(&SmartHealthLinkFlag::LongTerm)
    }

    #[must_use]
    pub fn is_expired(&self, now_epoch_secs: u64) -> bool {
        self.exp.is_some_and(|exp| now_epoch_secs > exp)
    }

    #[must_use]
    pub fn host(&self) -> String {
        self.url
            .strip_prefix("https://")
            .unwrap_or(&self.url)
            .split(['/', '?'])
            .next()
            .unwrap_or("unknown")
            .to_string()
    }
}

const MAX_SHLINK_URI_BYTES: usize = 8192;

/// Parses and strictly validates a SMART Health Link (`shlink:/...` or
/// viewer-wrapped `https://viewer.example/#shlink:/...`) URI in pure Rust:
/// - Enforces HTTPS-only SSRF-safe `url`
/// - Enforces exact 32-byte (43-char unpadded base64url) AES-256 `key`
/// - Enforces `v == 1` when specified
/// - Rejects unknown flags and invalid `P`+`U` flag combinations
/// - Enforces `exp` when `now_epoch_secs` is provided
pub fn parse_smart_health_link(
    raw_uri: &str,
    now_epoch_secs: Option<u64>,
) -> Result<SmartHealthLinkPayload, String> {
    let trimmed = raw_uri.trim();
    if trimmed.is_empty() {
        return Err("SMART Health Link URI must not be empty.".into());
    }
    if trimmed.len() > MAX_SHLINK_URI_BYTES {
        return Err("SMART Health Link URI exceeds the 8 KiB safety cap.".into());
    }
    if trimmed.chars().any(char::is_whitespace) {
        return Err("SMART Health Link URI must not contain whitespace.".into());
    }

    let (viewer_origin, b64_part) = if let Some(rest) = trimmed.strip_prefix("shlink:/") {
        (None, rest)
    } else if let Some((prefix, rest)) = trimmed.split_once("#shlink:/") {
        let viewer = validate_public_https_url(prefix)?;
        (Some(viewer), rest)
    } else {
        return Err(
            "SMART Health Link must start with `shlink:/` or wrap `#shlink:/` in an HTTPS viewer URL."
                .into(),
        );
    };

    if b64_part.is_empty() {
        return Err("SMART Health Link payload segment after `shlink:/` is empty.".into());
    }
    let decoded = base64url_decode_strict(b64_part)
        .ok_or_else(|| "SMART Health Link payload is not valid unpadded base64url.".to_string())?;
    let json_text = String::from_utf8(decoded)
        .map_err(|_| "SMART Health Link payload is not valid UTF-8 JSON.".to_string())?;
    let val: Value = serde_json::from_str(&json_text)
        .map_err(|err| format!("Invalid SMART Health Link JSON payload: {err}"))?;
    let obj = val
        .as_object()
        .ok_or_else(|| "SMART Health Link payload must decode to a JSON object.".to_string())?;

    let version = match obj.get("v") {
        None | Some(Value::Null) => 1u32,
        Some(v) => v
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| "SMART Health Link `v` must be an integer version.".to_string())?,
    };
    if version != 1 {
        return Err(format!(
            "Unsupported SMART Health Link version `v={version}` (only v=1 is supported)."
        ));
    }

    let raw_url = obj
        .get("url")
        .and_then(Value::as_str)
        .ok_or_else(|| "SMART Health Link payload missing required `url` string.".to_string())?;
    let url = validate_public_https_url(raw_url)?;

    let key = obj
        .get("key")
        .and_then(Value::as_str)
        .ok_or_else(|| "SMART Health Link payload missing required `key` string.".to_string())?;
    if key.len() != 43 {
        return Err(format!(
            "SMART Health Link `key` must be a 43-character unpadded base64url 32-byte AES-256 key (got {} chars).",
            key.len()
        ));
    }
    let key_bytes = base64url_decode_strict(key)
        .ok_or_else(|| "SMART Health Link `key` is not valid unpadded base64url.".to_string())?;
    if key_bytes.len() != 32 {
        return Err(format!(
            "SMART Health Link `key` must decode to exactly 32 bytes (got {}).",
            key_bytes.len()
        ));
    }

    let exp =
        match obj.get("exp") {
            None | Some(Value::Null) => None,
            Some(v) => Some(v.as_u64().ok_or_else(|| {
                "SMART Health Link `exp` must be a UNIX epoch integer.".to_string()
            })?),
        };
    if let (Some(now), Some(exp_ts)) = (now_epoch_secs, exp) {
        if now > exp_ts {
            return Err(format!(
                "SMART Health Link expired at UNIX timestamp {exp_ts}."
            ));
        }
    }

    let flag_raw = obj
        .get("flag")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let mut flags = Vec::new();
    if let Some(ref fl) = flag_raw {
        for ch in fl.chars() {
            let parsed = match ch {
                'L' => SmartHealthLinkFlag::LongTerm,
                'P' => SmartHealthLinkFlag::PasscodeRequired,
                'U' => SmartHealthLinkFlag::DirectFileUrl,
                other => {
                    return Err(format!("Unsupported SMART Health Link flag `{other}`."));
                }
            };
            if flags.contains(&parsed) {
                return Err(format!("Duplicate SMART Health Link flag `{ch}`."));
            }
            flags.push(parsed);
        }
        if flags.contains(&SmartHealthLinkFlag::PasscodeRequired)
            && flags.contains(&SmartHealthLinkFlag::DirectFileUrl)
        {
            return Err(
                "Invalid SMART Health Link flags: `P` (passcode) and `U` (direct file URL) cannot be combined."
                    .into(),
            );
        }
    }

    let label = obj
        .get("label")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.chars().take(160).collect());

    Ok(SmartHealthLinkPayload {
        url,
        key: key.to_string(),
        exp,
        flag: flag_raw,
        flags,
        label,
        v: version,
        viewer_origin,
    })
}

/// Encodes a validated [`SmartHealthLinkPayload`] into a `shlink:/...` URI.
pub fn build_shlink_uri(payload: &SmartHealthLinkPayload) -> Result<String, String> {
    let mut obj = serde_json::Map::new();
    obj.insert("url".into(), Value::String(payload.url.clone()));
    obj.insert("key".into(), Value::String(payload.key.clone()));
    if let Some(exp) = payload.exp {
        obj.insert("exp".into(), json!(exp));
    }
    if let Some(flag) = &payload.flag {
        obj.insert("flag".into(), Value::String(flag.clone()));
    }
    if let Some(label) = &payload.label {
        obj.insert("label".into(), Value::String(label.clone()));
    }
    obj.insert("v".into(), json!(payload.v));
    let raw_json = serde_json::to_string(&Value::Object(obj))
        .map_err(|err| format!("Failed to serialize SMART Health Link payload: {err}"))?;
    let uri = format!("shlink:/{}", base64url_encode(raw_json.as_bytes()));
    // Re-validate round-trip so callers can never emit an invalid shlink URI.
    let _ = parse_smart_health_link(&uri, None)?;
    Ok(uri)
}

/// Builds the JSON request body for a SMART Health Link manifest `POST`.
pub fn build_shlink_manifest_request_body(
    payload: &SmartHealthLinkPayload,
    recipient: &str,
    passcode: Option<&str>,
    embedded_length_max: Option<usize>,
) -> Result<Value, String> {
    if payload.is_direct_file_url() {
        return Err(
            "SMART Health Link has flag `U` (direct file URL); use GET with `?recipient=...` instead of a manifest POST."
                .into(),
        );
    }
    let rec = recipient.trim();
    if rec.is_empty() {
        return Err("SMART Health Link manifest request requires a non-empty `recipient`.".into());
    }
    let mut body = serde_json::Map::new();
    body.insert("recipient".into(), Value::String(rec.to_string()));
    if payload.requires_passcode() {
        let code = passcode
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                "SMART Health Link requires a `passcode` (flag `P` is set).".to_string()
            })?;
        body.insert("passcode".into(), Value::String(code.to_string()));
    } else if let Some(code) = passcode.map(str::trim).filter(|s| !s.is_empty()) {
        body.insert("passcode".into(), Value::String(code.to_string()));
    }
    if let Some(max_len) = embedded_length_max {
        body.insert("embeddedLengthMax".into(), json!(max_len));
    }
    Ok(Value::Object(body))
}

/// Returns a deterministic zero-PHI example `shlink:/` URI for interactive
/// SMART Health Link inspection demos.
#[must_use]
pub fn sandbox_smart_health_link_uri(patient: &PatientChart) -> String {
    let key_bytes = sha256_bytes(format!("dabney-zero-phi-shlink-demo-{}", patient.id).as_bytes());
    let key = base64url_encode(&key_bytes);
    let payload = SmartHealthLinkPayload {
        url: format!(
            "https://shl-sandbox.dabney-demo.invalid/manifests/{}",
            sanitize_id_slug(&patient.id)
        ),
        key,
        exp: Some(1_893_456_000), // 2030-01-01T00:00:00Z
        flag: Some("LP".into()),
        flags: vec![
            SmartHealthLinkFlag::LongTerm,
            SmartHealthLinkFlag::PasscodeRequired,
        ],
        label: Some(format!(
            "{} — Portable IPS Summary (Zero-PHI Demo)",
            patient.display_name
        )),
        v: 1,
        viewer_origin: None,
    };
    build_shlink_uri(&payload).unwrap_or_default()
}

/// Checks whether a SMART scope string requests only read/search permissions
/// across both SMART v1 (`.read`) and SMART v2 (`.r`, `.s`, `.rs`) grammars.
#[must_use]
pub fn smart_scopes_are_read_only(scopes: &str) -> bool {
    for token in scopes.split_whitespace() {
        if let Some((_, perm)) = token.rsplit_once('.') {
            let perm_base = perm.split('?').next().unwrap_or(perm);
            let is_resource_scope = token.starts_with("patient/")
                || token.starts_with("user/")
                || token.starts_with("system/");
            if !is_resource_scope {
                continue;
            }
            let allowed = matches!(perm_base, "read" | "r" | "s" | "rs");
            if !allowed {
                return false;
            }
        }
    }
    true
}

/// Constructs a standards-compliant SMART on FHIR R4 OAuth 2.0 + PKCE (`S256`)
/// sign-in session and authorize URL for `org`.
///
/// Selects SMART v1 vs v2 scopes from `org.transport` when `scopes` is `None`,
/// and fails closed for non-SMART vendors (such as KipuEMR, Hint Health, or
/// SimplePractice) with an actionable explanation of their supported flow.
pub fn build_smart_authorize_url(
    org: &PortalOrganization,
    client_id: &str,
    redirect_uri: &str,
    scopes: Option<&str>,
    state: &str,
    code_verifier: &str,
) -> Result<SmartPkceSession, String> {
    if !org.supports_smart_on_fhir() {
        return Err(format!(
            "{} ({}) does not expose a live SMART on FHIR R4 launch endpoint. Primary workflow: {} — {}",
            org.name,
            org.vendor.label(),
            org.transport.primary_capability.badge_label(),
            org.transport.notes
        ));
    }

    let fhir_base = validate_smart_endpoint_url(&org.fhir_base_url)?;
    let auth_ep = validate_smart_endpoint_url(&org.authorize_url)?;
    let token_ep = validate_smart_endpoint_url(&org.token_url)?;
    let redirect = validate_smart_endpoint_url(redirect_uri)?;

    let client = client_id.trim();
    if client.is_empty() {
        return Err("SMART client_id must not be empty.".into());
    }
    if state.trim().len() < 8 {
        return Err("OAuth state token must be at least 8 characters.".into());
    }
    if code_verifier.trim().len() < 43 || code_verifier.trim().len() > 128 {
        return Err("PKCE code_verifier must be 43–128 characters per RFC 7636.".into());
    }

    let default_scopes = org.transport.default_smart_scopes();
    let chosen_scopes = scopes
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(default_scopes);

    let challenge = pkce_s256_challenge(code_verifier.trim());
    let sep = if auth_ep.contains('?') { '&' } else { '?' };
    let authorize_url = format!(
        "{auth_ep}{sep}response_type=code&client_id={}&redirect_uri={}&scope={}&state={}&aud={}&code_challenge={}&code_challenge_method=S256",
        percent_encode(client),
        percent_encode(&redirect),
        percent_encode(chosen_scopes),
        percent_encode(state.trim()),
        percent_encode(&fhir_base),
        percent_encode(&challenge),
    );

    Ok(SmartPkceSession {
        organization_id: org.id.clone(),
        organization_name: org.name.clone(),
        vendor: org.vendor,
        fhir_base_url: fhir_base,
        authorize_endpoint: auth_ep,
        token_endpoint: token_ep,
        redirect_uri: redirect,
        client_id: client.to_string(),
        scopes: chosen_scopes.to_string(),
        state: state.trim().to_string(),
        code_verifier: code_verifier.trim().to_string(),
        code_challenge: challenge,
        code_challenge_method: "S256".to_string(),
        authorize_url,
        read_only: smart_scopes_are_read_only(chosen_scopes),
    })
}

/// Computes an RFC 7636 PKCE `S256` challenge (`BASE64URL(SHA256(verifier))`).
#[must_use]
pub fn pkce_s256_challenge(verifier: &str) -> String {
    let digest = sha256_bytes(verifier.as_bytes());
    base64url_encode(&digest)
}

/// Deterministic zero-dependency FIPS 180-4 SHA-256 implementation.
#[must_use]
pub fn sha256_bytes(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut padded = Vec::with_capacity(data.len() + 72);
    padded.extend_from_slice(data);
    padded.push(0x80);
    while (padded.len() % 64) != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in padded.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (i, word) in w.iter_mut().take(16).enumerate() {
            let b = i * 4;
            *word = u32::from_be_bytes([chunk[b], chunk[b + 1], chunk[b + 2], chunk[b + 3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    let mut out = [0u8; 32];
    for (i, word) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

/// Lowercase hex-encoded SHA-256 digest.
#[must_use]
pub fn sha256_hex(data: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = sha256_bytes(data);
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

/// RFC 4648 base64url encoding without `=` padding (used for PKCE `S256`).
#[must_use]
pub fn base64url_encode(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    let mut i = 0;
    while i + 3 <= input.len() {
        let n =
            (u32::from(input[i]) << 16) | (u32::from(input[i + 1]) << 8) | u32::from(input[i + 2]);
        out.push(TABLE[((n >> 18) & 0x3f) as usize] as char);
        out.push(TABLE[((n >> 12) & 0x3f) as usize] as char);
        out.push(TABLE[((n >> 6) & 0x3f) as usize] as char);
        out.push(TABLE[(n & 0x3f) as usize] as char);
        i += 3;
    }
    let rem = input.len() - i;
    if rem == 1 {
        let n = u32::from(input[i]) << 16;
        out.push(TABLE[((n >> 18) & 0x3f) as usize] as char);
        out.push(TABLE[((n >> 12) & 0x3f) as usize] as char);
    } else if rem == 2 {
        let n = (u32::from(input[i]) << 16) | (u32::from(input[i + 1]) << 8);
        out.push(TABLE[((n >> 18) & 0x3f) as usize] as char);
        out.push(TABLE[((n >> 12) & 0x3f) as usize] as char);
        out.push(TABLE[((n >> 6) & 0x3f) as usize] as char);
    }
    out
}

/// Strict RFC 4648 unpadded base64url decoder (rejects `+`, `/`, `=`, and
/// whitespace) used for SMART Health Link (`shlink:/`) URIs and 32-byte keys.
#[must_use]
pub fn base64url_decode_strict(input: &str) -> Option<Vec<u8>> {
    if input.is_empty() || input.len() % 4 == 1 {
        return None;
    }
    let mut buf = Vec::with_capacity((input.len() * 3) / 4);
    let mut acc: u32 = 0;
    let mut bits: u8 = 0;
    for b in input.bytes() {
        let val = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            _ => return None,
        };
        acc = (acc << 6) | u32::from(val);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            buf.push(((acc >> bits) & 0xff) as u8);
        }
    }
    // Reject non-zero trailing unused bits per RFC 4648 §3.5.
    if bits > 0 && (acc & ((1u32 << bits) - 1)) != 0 {
        return None;
    }
    Some(buf)
}

/// Standard base64 decoder (ignores ASCII whitespace) for decoding inline
/// FHIR `DocumentReference.content[].attachment.data` text attachments.
#[must_use]
pub fn base64_decode(input: &str) -> Option<Vec<u8>> {
    let mut buf = Vec::with_capacity((input.len() / 4) * 3);
    let mut acc: u32 = 0;
    let mut bits: u8 = 0;
    for b in input.bytes() {
        if b.is_ascii_whitespace() || b == b'=' {
            continue;
        }
        let val = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        };
        acc = (acc << 6) | u32::from(val);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            buf.push(((acc >> bits) & 0xff) as u8);
        }
    }
    Some(buf)
}

/// RFC 3986 percent-encoding for URL query values, `mailto:` fields, and `data:` URIs.
#[must_use]
pub fn percent_encode(input: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(input.len() + input.len() / 4);
    for &b in input.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push('%');
            out.push(HEX[(b >> 4) as usize] as char);
            out.push(HEX[(b & 0x0f) as usize] as char);
        }
    }
    out
}

/// Escapes untrusted text for safe insertion into standalone printable HTML
/// health passports and HIPAA ROI forms.
#[must_use]
pub fn html_escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 16);
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

/// Builds a client-side `data:` download URI for JSON, FHIR, HTML, or Markdown files.
#[must_use]
pub fn data_download_uri(mime: &str, content: &str) -> String {
    format!("data:{mime};charset=utf-8,{}", percent_encode(content))
}

/// Detected clinical interchange format handled by [`ingest_clinical_document`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClinicalDocumentFormat {
    FhirR4Json,
    KipuJson,
    CcdaR21Xml,
    Hl7V2Pipe,
    SmartHealthLinkUri,
}

impl ClinicalDocumentFormat {
    pub const fn label(self) -> &'static str {
        match self {
            Self::FhirR4Json => "HL7 FHIR R4 JSON",
            Self::KipuJson => "KipuEMR API JSON (42 CFR Part 2)",
            Self::CcdaR21Xml => "HL7 C-CDA R2.1 XML",
            Self::Hl7V2Pipe => "HL7 v2.x Pipe-Delimited Message",
            Self::SmartHealthLinkUri => "SMART Health Link (shlink:/)",
        }
    }
}

/// Unified outcome of [`ingest_clinical_document`]: either an imported clinical
/// payload (`import`) or a parsed/validated SMART Health Link (`smart_health_link`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClinicalDocumentIngest {
    pub format: ClinicalDocumentFormat,
    pub import: Option<FhirImportResult>,
    pub smart_health_link: Option<SmartHealthLinkPayload>,
}

/// Auto-detecting unified clinical document ingestion entry point dispatching:
/// - `shlink:/...` or `https://...#shlink:/...` → [`parse_smart_health_link`]
/// - `MSH<punct>...` → [`crate::hl7v2::ingest_hl7v2_message`]
/// - `<...` (XML) → [`crate::ccda::ingest_ccda_xml`]
/// - JSON with `resourceType` → [`ingest_fhir_r4_bundle`]
/// - Kipu JSON envelope (`casefile_id`, `patient_evaluations`, `vital_signs`,
///   `episode_orders`, `consent_form_records`, `kipu_api_version`, etc.) →
///   [`crate::kipu::ingest_kipu_payload`]
pub fn ingest_clinical_document(
    source_label: &str,
    portal_id: &str,
    raw_input: &str,
) -> Result<ClinicalDocumentIngest, String> {
    let trimmed = raw_input.trim_start_matches('\u{feff}').trim();
    if trimmed.is_empty() {
        return Err("Clinical document input is empty.".into());
    }

    if trimmed.starts_with("shlink:/") || trimmed.contains("#shlink:/") {
        let shl = parse_smart_health_link(trimmed, None)?;
        return Ok(ClinicalDocumentIngest {
            format: ClinicalDocumentFormat::SmartHealthLinkUri,
            import: None,
            smart_health_link: Some(shl),
        });
    }

    if trimmed.starts_with("MSH")
        && trimmed
            .chars()
            .nth(3)
            .is_some_and(|c| c.is_ascii_punctuation())
    {
        let imported = crate::hl7v2::ingest_hl7v2_message(source_label, portal_id, trimmed)?;
        return Ok(ClinicalDocumentIngest {
            format: ClinicalDocumentFormat::Hl7V2Pipe,
            import: Some(imported),
            smart_health_link: None,
        });
    }

    if trimmed.starts_with('<') {
        let imported = crate::ccda::ingest_ccda_xml(source_label, portal_id, trimmed)?;
        return Ok(ClinicalDocumentIngest {
            format: ClinicalDocumentFormat::CcdaR21Xml,
            import: Some(imported),
            smart_health_link: None,
        });
    }

    if trimmed.starts_with('{') {
        let parsed: Value = serde_json::from_str(trimmed)
            .map_err(|err| format!("Invalid JSON clinical document: {err}"))?;
        let obj = parsed
            .as_object()
            .ok_or_else(|| "JSON clinical document must be an object.".to_string())?;
        if obj.get("resourceType").and_then(Value::as_str).is_some() {
            let imported = ingest_fhir_r4_bundle(source_label, portal_id, trimmed)?;
            return Ok(ClinicalDocumentIngest {
                format: ClinicalDocumentFormat::FhirR4Json,
                import: Some(imported),
                smart_health_link: None,
            });
        }
        let is_kipu = obj.contains_key("kipu_api_version")
            || obj.contains_key("casefile_id")
            || obj.contains_key("patient_evaluations")
            || obj.contains_key("vital_signs")
            || obj.contains_key("episode_orders")
            || obj.contains_key("patient_orders")
            || obj.contains_key("consent_form_records")
            || obj.contains_key("consent_forms")
            || obj.get("patient").and_then(Value::as_object).is_some()
            || obj.get("census").and_then(Value::as_array).is_some();
        if is_kipu {
            let imported = crate::kipu::ingest_kipu_payload(source_label, portal_id, trimmed)?;
            return Ok(ClinicalDocumentIngest {
                format: ClinicalDocumentFormat::KipuJson,
                import: Some(imported),
                smart_health_link: None,
            });
        }
        return Err(
            "Unrecognized JSON clinical payload: expected FHIR R4 (`resourceType`) or KipuEMR JSON envelope."
                .into(),
        );
    }

    Err(
        "Unrecognized clinical document format: supply FHIR R4 JSON, KipuEMR JSON, C-CDA R2.1 XML, HL7 v2 pipe-delimited text (`MSH|...`), or a `shlink:/` SMART Health Link URI."
            .into(),
    )
}

/// Parses an HL7 FHIR R4 `Bundle` JSON (or single FHIR R4 resource JSON) into
/// normalized Dabney.moe [`ChartRecord`], [`Medication`], [`Problem`], and
/// [`AllergyEntry`] collections.
///
/// Malformed or unsupported individual entries are recorded in `warnings`
/// without failing sibling valid resources.
pub fn ingest_fhir_r4_bundle(
    source_label: &str,
    portal_id: &str,
    raw_json: &str,
) -> Result<FhirImportResult, String> {
    let parsed: Value =
        serde_json::from_str(raw_json).map_err(|err| format!("Invalid FHIR R4 JSON: {err}"))?;
    let root = parsed
        .as_object()
        .ok_or_else(|| "FHIR R4 payload must be a JSON object.".to_string())?;
    let root_type = root
        .get("resourceType")
        .and_then(Value::as_str)
        .ok_or_else(|| "Missing FHIR `resourceType` field.".to_string())?;

    let bundle_confidentiality = detect_fhir_security_confidentiality(root);

    let resources: Vec<&Value> = if root_type == "Bundle" {
        root.get("entry")
            .and_then(Value::as_array)
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(|e| e.get("resource"))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    } else {
        vec![&parsed]
    };

    let mut result = FhirImportResult {
        source_label: source_label.trim().to_string(),
        portal_id: portal_id.trim().to_string(),
        patient_name: None,
        patient_birth_date: None,
        records: Vec::new(),
        medications: Vec::new(),
        problems: Vec::new(),
        allergies: Vec::new(),
        total_entries_seen: resources.len(),
        warnings: Vec::new(),
        confidentiality: bundle_confidentiality,
    };

    let prefix = sanitize_id_slug(portal_id);

    for (idx, res) in resources.iter().enumerate() {
        let Some(obj) = res.as_object() else {
            result.warnings.push(FhirResourceWarning {
                index: idx,
                resource_type: "unknown".into(),
                resource_id: None,
                reason: "Bundle entry resource is not a JSON object.".into(),
            });
            continue;
        };
        let entry_conf = if bundle_confidentiality.is_part2_sud() {
            ConfidentialityClass::Part2Sud
        } else {
            detect_fhir_security_confidentiality(obj)
        };
        if entry_conf.is_part2_sud() {
            result.confidentiality = ConfidentialityClass::Part2Sud;
        }
        let rtype = obj
            .get("resourceType")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let rid = obj.get("id").and_then(Value::as_str).map(str::to_string);
        let fallback_id = format!("{prefix}-{}-{idx}", rtype.to_ascii_lowercase());
        let stable_id = rid
            .as_deref()
            .map(|id| {
                let slug = sanitize_id_slug(id);
                let expected_prefix = format!("{prefix}-");
                if slug == prefix || slug.starts_with(&expected_prefix) {
                    slug
                } else {
                    format!("{prefix}-{slug}")
                }
            })
            .unwrap_or(fallback_id);

        match rtype {
            "Patient" => {
                if let Some(name) = extract_patient_name(obj) {
                    result.patient_name = Some(name);
                }
                if let Some(dob) = obj.get("birthDate").and_then(Value::as_str) {
                    result.patient_birth_date = Some(dob.to_string());
                }
            }
            "Observation" => match parse_observation(obj, &stable_id, source_label, entry_conf) {
                Ok(rec) => result.records.push(rec),
                Err(reason) => result.warnings.push(FhirResourceWarning {
                    index: idx,
                    resource_type: rtype.into(),
                    resource_id: rid,
                    reason,
                }),
            },
            "Condition" => match parse_condition(obj, entry_conf) {
                Ok(prob) => result.problems.push(prob),
                Err(reason) => result.warnings.push(FhirResourceWarning {
                    index: idx,
                    resource_type: rtype.into(),
                    resource_id: rid,
                    reason,
                }),
            },
            "MedicationRequest" | "MedicationStatement" => {
                match parse_medication(obj, entry_conf) {
                    Ok(med) => result.medications.push(med),
                    Err(reason) => result.warnings.push(FhirResourceWarning {
                        index: idx,
                        resource_type: rtype.into(),
                        resource_id: rid,
                        reason,
                    }),
                }
            }
            "DiagnosticReport" => {
                match parse_diagnostic_report(obj, &stable_id, source_label, entry_conf) {
                    Ok(rec) => result.records.push(rec),
                    Err(reason) => result.warnings.push(FhirResourceWarning {
                        index: idx,
                        resource_type: rtype.into(),
                        resource_id: rid,
                        reason,
                    }),
                }
            }
            "DocumentReference" => {
                match parse_document_reference(obj, &stable_id, source_label, entry_conf) {
                    Ok(rec) => result.records.push(rec),
                    Err(reason) => result.warnings.push(FhirResourceWarning {
                        index: idx,
                        resource_type: rtype.into(),
                        resource_id: rid,
                        reason,
                    }),
                }
            }
            "AllergyIntolerance" => {
                match parse_allergy(obj, &stable_id, source_label, entry_conf) {
                    Ok((allergy, rec)) => {
                        result.allergies.push(allergy);
                        result.records.push(rec);
                    }
                    Err(reason) => result.warnings.push(FhirResourceWarning {
                        index: idx,
                        resource_type: rtype.into(),
                        resource_id: rid,
                        reason,
                    }),
                }
            }
            "Composition" => {
                // Document composition header (e.g., IPS bundle) is structural metadata.
            }
            other => {
                result.warnings.push(FhirResourceWarning {
                    index: idx,
                    resource_type: other.into(),
                    resource_id: rid,
                    reason: format!(
                        "Resource type `{other}` retained as unsupported in local summary."
                    ),
                });
            }
        }
    }

    if result.records.is_empty()
        && result.medications.is_empty()
        && result.problems.is_empty()
        && result.allergies.is_empty()
        && result.patient_name.is_none()
    {
        return Err("No supported FHIR R4 clinical resources found in payload.".into());
    }

    Ok(result)
}

fn detect_fhir_security_confidentiality(
    obj: &serde_json::Map<String, Value>,
) -> ConfidentialityClass {
    let has_part2_tag = obj
        .get("meta")
        .and_then(|m| m.get("security"))
        .and_then(Value::as_array)
        .is_some_and(|sec| {
            sec.iter().any(|coding| {
                coding
                    .get("code")
                    .and_then(Value::as_str)
                    .is_some_and(|code| {
                        code.eq_ignore_ascii_case("ETHUD")
                            || code.eq_ignore_ascii_case("42CFRPart2")
                            || code.eq_ignore_ascii_case("42CFR2")
                            || code.eq_ignore_ascii_case("SUD")
                    })
            })
        });
    if has_part2_tag {
        ConfidentialityClass::Part2Sud
    } else {
        ConfidentialityClass::StandardHipaa
    }
}

/// Merges a parsed [`FhirImportResult`] into a live [`PatientChart`] and
/// allergy list, deduplicating cleanly by stable record ID, medication
/// name+dose, problem code/label, and allergy substance.
pub fn merge_fhir_import_into_chart(
    chart: &mut PatientChart,
    allergies: &mut Vec<AllergyEntry>,
    import: &FhirImportResult,
) -> MergeCounts {
    let mut counts = MergeCounts::default();

    for rec in &import.records {
        if chart.records.iter().any(|existing| existing.id == rec.id) {
            counts.skipped_duplicates += 1;
        } else {
            chart.records.insert(0, rec.clone());
            counts.added_records += 1;
        }
    }

    for med in &import.medications {
        let dup = chart.medications.iter().any(|m| {
            m.name.eq_ignore_ascii_case(&med.name) && m.dose.eq_ignore_ascii_case(&med.dose)
        });
        if dup {
            counts.skipped_duplicates += 1;
        } else {
            chart.medications.push(med.clone());
            counts.added_medications += 1;
        }
    }

    for prob in &import.problems {
        let dup = chart.problems.iter().any(|p| {
            (!prob.code.is_empty() && p.code.eq_ignore_ascii_case(&prob.code))
                || p.label.eq_ignore_ascii_case(&prob.label)
        });
        if dup {
            counts.skipped_duplicates += 1;
        } else {
            chart.problems.push(prob.clone());
            counts.added_problems += 1;
        }
    }

    for allergy in &import.allergies {
        let dup = allergies
            .iter()
            .any(|a| a.substance.eq_ignore_ascii_case(&allergy.substance));
        if dup {
            counts.skipped_duplicates += 1;
        } else {
            allergies.push(allergy.clone());
            counts.added_allergies += 1;
        }
    }

    counts
}

/// Default synthetic documented allergies for each sample patient profile.
#[must_use]
pub fn default_allergies_for_patient(patient_id: &str) -> Vec<AllergyEntry> {
    match patient_id {
        "pt-nadia-kulkarni" => vec![AllergyEntry {
            id: "nk-alg-01".into(),
            substance: "Sulfamethoxazole-trimethoprim".into(),
            reaction: "Maculopapular rash (2019)".into(),
            criticality: "Low".into(),
            recorded_at: "2019-06-14".into(),
            source_label: "Dabney.moe Synthetic Clinic".into(),
            confidentiality: ConfidentialityClass::StandardHipaa,
        }],
        "pt-carlos-mendez" => vec![AllergyEntry {
            id: "cm-alg-01".into(),
            substance: "Penicillin".into(),
            reaction: "Hives (childhood)".into(),
            criticality: "High".into(),
            recorded_at: "2014-03-02".into(),
            source_label: "Dabney.moe Synthetic Clinic".into(),
            confidentiality: ConfidentialityClass::StandardHipaa,
        }],
        "pt-david-okafor" => vec![AllergyEntry {
            id: "do-alg-01".into(),
            substance: "Ibuprofen / NSAIDs".into(),
            reaction: "Avoid with apixaban anticoagulation (bleeding precaution)".into(),
            criticality: "High".into(),
            recorded_at: "2025-09-10".into(),
            source_label: "Dabney.moe Synthetic Clinic".into(),
            confidentiality: ConfidentialityClass::StandardHipaa,
        }],
        _ => Vec::new(),
    }
}

/// Generates a realistic, 100% synthetic zero-PHI HL7 FHIR R4 `Bundle` JSON
/// tailored to `org_id` and `patient` so interactive portal sign-in exercises
/// the exact same [`ingest_fhir_r4_bundle`] parser used for live EHR files.
#[must_use]
pub fn sandbox_fhir_bundle_for_org(org_id: &str, patient: &PatientChart) -> String {
    let org_slug = sanitize_id_slug(org_id);
    let (lab_code, lab_name, lab_val, lab_unit, med_name, med_dose, consult_title, consult_text, allergy_sub, allergy_rxn) =
        match org_id {
            "org-oracle-sandbox" | "org-cedars-mychart" => (
                "2093-3",
                "Fasting Lipid Panel — LDL Cholesterol",
                "94",
                "mg/dL",
                "Vitamin D3",
                "2000 IU once daily with meal",
                "Preventive Cardiology & Lipid Consult Note",
                "External specialist review: lipid panel shows LDL 94 mg/dL, HDL 58 mg/dL, triglycerides 118 mg/dL. No statin therapy initiated at this visit; continue lifestyle focus and share results with primary clinician.",
                "Latex adhesive",
                "Mild contact erythema",
            ),
            "org-athena-clinic" | "org-kaiser-ca" => (
                "3016-3",
                "Thyroid Stimulating Hormone (TSH)",
                "1.84",
                "mIU/L",
                "Omega-3 EPA/DHA",
                "1000 mg daily",
                "Annual Primary Care Continuity Summary",
                "Continuity visit note: TSH 1.84 mIU/L (euthyroid), Vitamin B12 540 pg/mL, blood pressure 122/76 mmHg seated. Patient requested portable electronic copy for specialist coordination.",
                "Shellfish contrast dye",
                "No prior anaphylaxis; mild flushing noted",
            ),
            _ => (
                "14959-1",
                "Urine Albumin-to-Creatinine Ratio (uACR)",
                "11.4",
                "mg/g",
                "Magnesium Glycinate",
                "200 mg at bedtime",
                "Endocrinology Portal Continuity Note",
                "Portal continuity summary: Urine albumin-to-creatinine ratio 11.4 mg/g (normal <30 mg/g), retinal screening clear with no diabetic retinopathy, foot monofilament exam intact bilaterally.",
                "Codeine",
                "Nausea / GI upset (intolerance)",
            ),
        };

    let note_b64 = base64_encode_standard(consult_text.as_bytes());
    let bundle = serde_json::json!({
        "resourceType": "Bundle",
        "type": "collection",
        "timestamp": "2026-11-26T04:00:00Z",
        "entry": [
            {
                "resource": {
                    "resourceType": "Patient",
                    "id": format!("{org_slug}-pat"),
                    "name": [{ "text": patient.display_name }],
                    "birthDate": patient.birth_date
                }
            },
            {
                "resource": {
                    "resourceType": "Observation",
                    "id": format!("{org_slug}-obs-01"),
                    "status": "final",
                    "category": [{ "coding": [{ "code": "laboratory", "display": "Laboratory" }] }],
                    "code": {
                        "coding": [{ "system": "http://loinc.org", "code": lab_code, "display": lab_name }],
                        "text": lab_name
                    },
                    "effectiveDateTime": "2026-11-15",
                    "valueQuantity": { "value": lab_val.parse::<f64>().unwrap_or(0.0), "unit": lab_unit }
                }
            },
            {
                "resource": {
                    "resourceType": "Observation",
                    "id": format!("{org_slug}-bp-01"),
                    "status": "final",
                    "category": [{ "coding": [{ "code": "vital-signs", "display": "Vital Signs" }] }],
                    "code": { "text": "Clinic Blood Pressure Panel" },
                    "effectiveDateTime": "2026-11-15",
                    "component": [
                        { "code": { "text": "Systolic BP" }, "valueQuantity": { "value": 124, "unit": "mmHg" } },
                        { "code": { "text": "Diastolic BP" }, "valueQuantity": { "value": 78, "unit": "mmHg" } }
                    ]
                }
            },
            {
                "resource": {
                    "resourceType": "DocumentReference",
                    "id": format!("{org_slug}-doc-01"),
                    "status": "current",
                    "description": consult_title,
                    "date": "2026-11-15",
                    "content": [{
                        "attachment": {
                            "contentType": "text/plain",
                            "data": note_b64
                        }
                    }]
                }
            },
            {
                "resource": {
                    "resourceType": "MedicationRequest",
                    "id": format!("{org_slug}-med-01"),
                    "status": "active",
                    "medicationCodeableConcept": { "text": med_name },
                    "dosageInstruction": [{ "text": med_dose }],
                    "reasonCode": [{ "text": "Imported via connected patient portal" }]
                }
            },
            {
                "resource": {
                    "resourceType": "AllergyIntolerance",
                    "id": format!("{org_slug}-alg-01"),
                    "criticality": "low",
                    "recordedDate": "2025-05-12",
                    "code": { "text": allergy_sub },
                    "reaction": [{ "manifestation": [{ "text": allergy_rxn }] }]
                }
            }
        ]
    });

    serde_json::to_string_pretty(&bundle).unwrap_or_else(|_| "{}".to_string())
}

/// Standard RFC 4648 base64 encoder with `=` padding (used for FHIR
/// attachments and Kipu API v3 `Content-MD5` / HMAC signatures).
#[must_use]
pub fn base64_encode_standard(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    let mut i = 0;
    while i + 3 <= input.len() {
        let n =
            (u32::from(input[i]) << 16) | (u32::from(input[i + 1]) << 8) | u32::from(input[i + 2]);
        out.push(TABLE[((n >> 18) & 0x3f) as usize] as char);
        out.push(TABLE[((n >> 12) & 0x3f) as usize] as char);
        out.push(TABLE[((n >> 6) & 0x3f) as usize] as char);
        out.push(TABLE[(n & 0x3f) as usize] as char);
        i += 3;
    }
    let rem = input.len() - i;
    if rem == 1 {
        let n = u32::from(input[i]) << 16;
        out.push(TABLE[((n >> 18) & 0x3f) as usize] as char);
        out.push(TABLE[((n >> 12) & 0x3f) as usize] as char);
        out.push('=');
        out.push('=');
    } else if rem == 2 {
        let n = (u32::from(input[i]) << 16) | (u32::from(input[i + 1]) << 8);
        out.push(TABLE[((n >> 18) & 0x3f) as usize] as char);
        out.push(TABLE[((n >> 12) & 0x3f) as usize] as char);
        out.push(TABLE[((n >> 6) & 0x3f) as usize] as char);
        out.push('=');
    }
    out
}

pub(crate) fn sanitize_id_slug(raw: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    let trimmed = out.trim_end_matches('-');
    if trimmed.is_empty() {
        "portal".into()
    } else {
        trimmed.chars().take(48).collect()
    }
}

fn extract_codeable_text(val: Option<&Value>) -> Option<String> {
    let obj = val?.as_object()?;
    if let Some(t) = obj
        .get("text")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return Some(t.to_string());
    }
    if let Some(codings) = obj.get("coding").and_then(Value::as_array) {
        for c in codings {
            if let Some(d) = c
                .get("display")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                return Some(d.to_string());
            }
            if let Some(code) = c
                .get("code")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                return Some(code.to_string());
            }
        }
    }
    None
}

fn extract_coding_code(val: Option<&Value>) -> Option<String> {
    val?.as_object()?
        .get("coding")?
        .as_array()?
        .iter()
        .find_map(|c| {
            c.get("code")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        })
}

fn extract_patient_name(obj: &serde_json::Map<String, Value>) -> Option<String> {
    let names = obj.get("name")?.as_array()?;
    let first = names.first()?.as_object()?;
    if let Some(t) = first
        .get("text")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return Some(t.to_string());
    }
    let given: Vec<&str> = first
        .get("given")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let family = first.get("family").and_then(Value::as_str).unwrap_or("");
    let full = format!("{} {}", given.join(" "), family).trim().to_string();
    (!full.is_empty()).then_some(full)
}

fn format_quantity(val: &Value) -> Option<String> {
    let obj = val.as_object()?;
    let num = obj.get("value")?;
    let num_str = if let Some(i) = num.as_i64() {
        i.to_string()
    } else if let Some(f) = num.as_f64() {
        if (f.fract()).abs() < 1e-9 {
            format!("{f:.0}")
        } else {
            format!("{f}")
        }
    } else {
        num.as_str()?.to_string()
    };
    let unit = obj
        .get("unit")
        .or_else(|| obj.get("code"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());
    Some(match unit {
        Some(u) => format!("{num_str} {u}"),
        None => num_str,
    })
}

pub(crate) fn date_prefix(raw: Option<&str>) -> String {
    raw.map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.chars().take(10).collect())
        .unwrap_or_else(|| "Unknown date".into())
}

fn parse_observation(
    obj: &serde_json::Map<String, Value>,
    stable_id: &str,
    source_label: &str,
    confidentiality: ConfidentialityClass,
) -> Result<ChartRecord, String> {
    let title = extract_codeable_text(obj.get("code"))
        .ok_or_else(|| "Observation missing `code.text` or `code.coding[].display`.".to_string())?;
    let recorded_at = date_prefix(
        obj.get("effectiveDateTime")
            .or_else(|| obj.get("issued"))
            .and_then(Value::as_str),
    );
    let status = obj.get("status").and_then(Value::as_str).unwrap_or("final");

    let mut key_values = Vec::new();
    let mut parts = Vec::new();

    if let Some(q) = obj.get("valueQuantity").and_then(format_quantity) {
        key_values.push((title.clone(), q.clone()));
        parts.push(format!("{title}: {q}"));
    } else if let Some(s) = obj.get("valueString").and_then(Value::as_str) {
        key_values.push((title.clone(), s.to_string()));
        parts.push(format!("{title}: {s}"));
    } else if let Some(cc) = extract_codeable_text(obj.get("valueCodeableConcept")) {
        key_values.push((title.clone(), cc.clone()));
        parts.push(format!("{title}: {cc}"));
    }

    if let Some(components) = obj.get("component").and_then(Value::as_array) {
        for comp in components {
            let label =
                extract_codeable_text(comp.get("code")).unwrap_or_else(|| "Component".into());
            if let Some(q) = comp.get("valueQuantity").and_then(format_quantity) {
                key_values.push((label.clone(), q.clone()));
                parts.push(format!("{label}: {q}"));
            }
        }
    }

    if parts.is_empty() {
        return Err("Observation has no valueQuantity, valueString, valueCodeableConcept, or component values.".into());
    }

    let title_lower = title.to_ascii_lowercase();
    let kind = if title_lower.contains("cgm") || title_lower.contains("continuous glucose") {
        RecordKind::CgmSummary
    } else {
        RecordKind::LabResult
    };

    let body = format!(
        "{} (FHIR status: {status}, imported from {source_label}).",
        parts.join("; ")
    );

    Ok(ChartRecord {
        id: stable_id.to_string(),
        kind,
        title,
        recorded_at,
        source_label: source_label.to_string(),
        body,
        key_values,
        confidentiality,
    })
}

fn parse_condition(
    obj: &serde_json::Map<String, Value>,
    confidentiality: ConfidentialityClass,
) -> Result<Problem, String> {
    let label = extract_codeable_text(obj.get("code"))
        .ok_or_else(|| "Condition missing `code` display/text.".to_string())?;
    let code = extract_coding_code(obj.get("code")).unwrap_or_else(|| "FHIR".into());
    let status =
        extract_codeable_text(obj.get("clinicalStatus")).unwrap_or_else(|| "Active".into());
    Ok(Problem {
        code,
        label,
        status,
        confidentiality,
    })
}

fn parse_medication(
    obj: &serde_json::Map<String, Value>,
    confidentiality: ConfidentialityClass,
) -> Result<Medication, String> {
    let name = extract_codeable_text(obj.get("medicationCodeableConcept"))
        .or_else(|| {
            obj.get("medicationReference")
                .and_then(|r| r.get("display"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .ok_or_else(|| "Medication resource missing medication name.".to_string())?;

    let dosage_text = obj
        .get("dosageInstruction")
        .or_else(|| obj.get("dosage"))
        .and_then(Value::as_array)
        .and_then(|arr| arr.first())
        .and_then(|d| d.get("text"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("See portal directions");

    let indication = obj
        .get("reasonCode")
        .and_then(Value::as_array)
        .and_then(|arr| arr.first())
        .and_then(|r| extract_codeable_text(Some(r)))
        .unwrap_or_else(|| "Imported from connected EHR portal".into());

    let status = obj
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("active");

    Ok(Medication {
        name,
        dose: dosage_text.to_string(),
        schedule: format!("Status: {status}"),
        indication,
        confidentiality,
    })
}

fn parse_diagnostic_report(
    obj: &serde_json::Map<String, Value>,
    stable_id: &str,
    source_label: &str,
    confidentiality: ConfidentialityClass,
) -> Result<ChartRecord, String> {
    let title = extract_codeable_text(obj.get("code"))
        .ok_or_else(|| "DiagnosticReport missing `code`.".to_string())?;
    let recorded_at = date_prefix(
        obj.get("effectiveDateTime")
            .or_else(|| obj.get("issued"))
            .and_then(Value::as_str),
    );
    let conclusion = obj
        .get("conclusion")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("Diagnostic report imported from connected EHR portal.");

    let cat_text = obj
        .get("category")
        .and_then(Value::as_array)
        .and_then(|arr| arr.first())
        .and_then(|c| extract_codeable_text(Some(c)))
        .unwrap_or_default()
        .to_ascii_lowercase();
    let kind = if cat_text.contains("imag")
        || cat_text.contains("rad")
        || title.to_ascii_lowercase().contains("echo")
        || title.to_ascii_lowercase().contains("mri")
        || title.to_ascii_lowercase().contains("ct ")
        || title.to_ascii_lowercase().contains("x-ray")
    {
        RecordKind::Imaging
    } else {
        RecordKind::LabResult
    };

    Ok(ChartRecord {
        id: stable_id.to_string(),
        kind,
        title,
        recorded_at,
        source_label: source_label.to_string(),
        body: conclusion.to_string(),
        key_values: Vec::new(),
        confidentiality,
    })
}

fn parse_document_reference(
    obj: &serde_json::Map<String, Value>,
    stable_id: &str,
    source_label: &str,
    confidentiality: ConfidentialityClass,
) -> Result<ChartRecord, String> {
    let title = obj
        .get("description")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| extract_codeable_text(obj.get("type")))
        .ok_or_else(|| "DocumentReference missing `description` and `type`.".to_string())?;
    let recorded_at = date_prefix(obj.get("date").and_then(Value::as_str));

    let decoded_body = obj
        .get("content")
        .and_then(Value::as_array)
        .and_then(|arr| arr.first())
        .and_then(|c| c.get("attachment"))
        .and_then(|att| {
            let ct = att
                .get("contentType")
                .and_then(Value::as_str)
                .unwrap_or("text/plain");
            if !ct.starts_with("text/") {
                return None;
            }
            let b64 = att.get("data").and_then(Value::as_str)?;
            let bytes = base64_decode(b64)?;
            String::from_utf8(bytes).ok()
        })
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("Clinical document imported from {source_label}."));

    Ok(ChartRecord {
        id: stable_id.to_string(),
        kind: RecordKind::ProgressNote,
        title,
        recorded_at,
        source_label: source_label.to_string(),
        body: decoded_body,
        key_values: Vec::new(),
        confidentiality,
    })
}

fn parse_allergy(
    obj: &serde_json::Map<String, Value>,
    stable_id: &str,
    source_label: &str,
    confidentiality: ConfidentialityClass,
) -> Result<(AllergyEntry, ChartRecord), String> {
    let substance = extract_codeable_text(obj.get("code"))
        .ok_or_else(|| "AllergyIntolerance missing `code` substance.".to_string())?;
    let criticality = obj
        .get("criticality")
        .and_then(Value::as_str)
        .unwrap_or("unspecified")
        .to_string();
    let recorded_at = date_prefix(obj.get("recordedDate").and_then(Value::as_str));
    let reaction = obj
        .get("reaction")
        .and_then(Value::as_array)
        .and_then(|arr| arr.first())
        .and_then(|r| r.get("manifestation"))
        .and_then(Value::as_array)
        .and_then(|arr| arr.first())
        .and_then(|m| extract_codeable_text(Some(m)))
        .unwrap_or_else(|| "Documented intolerance / allergy".into());

    let entry = AllergyEntry {
        id: stable_id.to_string(),
        substance: substance.clone(),
        reaction: reaction.clone(),
        criticality: criticality.clone(),
        recorded_at: recorded_at.clone(),
        source_label: source_label.to_string(),
        confidentiality,
    };
    let rec = ChartRecord {
        id: stable_id.to_string(),
        kind: RecordKind::ProgressNote,
        title: format!("Allergy / Intolerance: {substance}"),
        recorded_at,
        source_label: source_label.to_string(),
        body: format!("Documented reaction: {reaction} (criticality: {criticality})."),
        key_values: vec![
            ("Allergy".into(), substance),
            ("Criticality".into(), criticality),
        ],
        confidentiality,
    };
    Ok((entry, rec))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::find_patient;

    #[test]
    fn sha256_and_pkce_match_rfc_test_vectors() {
        // FIPS 180-4 empty & "abc" vectors.
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );

        // RFC 7636 Appendix B official PKCE S256 test vector!
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        assert_eq!(
            pkce_s256_challenge(verifier),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn smart_authorize_url_builds_pkce_s256_and_rejects_insecure_urls() {
        let org = find_portal_organization("org-epic-sandbox").expect("epic sandbox");
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let session = build_smart_authorize_url(
            &org,
            "dabney-patient-demo",
            "https://dabney.moe/clinical/patient",
            None,
            "state-12345678",
            verifier,
        )
        .expect("valid smart session");

        assert!(session.read_only);
        assert_eq!(session.code_challenge_method, "S256");
        assert_eq!(
            session.code_challenge,
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        assert!(session.authorize_url.contains("code_challenge_method=S256"));
        assert!(session.authorize_url.contains("aud=https%3A%2F%2F"));
        assert!(session.authorize_url.contains("state=state-12345678"));

        // Non-HTTPS remote endpoints fail closed.
        let mut bad_org = org.clone();
        bad_org.fhir_base_url = "http://evil.example/fhir".into();
        assert!(build_smart_authorize_url(
            &bad_org,
            "dabney-patient-demo",
            "https://dabney.moe/clinical/patient",
            None,
            "state-12345678",
            verifier
        )
        .is_err());
    }

    #[test]
    fn fhir_r4_bundle_ingests_records_meds_allergies_and_tolerates_partial_errors() {
        let nadia = find_patient("pt-nadia-kulkarni").expect("nadia");
        let raw = sandbox_fhir_bundle_for_org("org-epic-sandbox", &nadia);
        let mut parsed_val: Value = serde_json::from_str(&raw).unwrap();
        // Inject one malformed observation to confirm sibling entries still import cleanly.
        parsed_val["entry"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "resource": {
                    "resourceType": "Observation",
                    "id": "bad-obs"
                }
            }));
        let modified = serde_json::to_string(&parsed_val).unwrap();

        let import = ingest_fhir_r4_bundle("Epic MyChart Sandbox", "org-epic-sandbox", &modified)
            .expect("partial import should succeed");
        assert_eq!(import.patient_name.as_deref(), Some("Nadia Kulkarni"));
        assert_eq!(import.records.len(), 4); // lab + BP + decoded consult note + allergy record
        assert_eq!(import.medications.len(), 1);
        assert_eq!(import.allergies.len(), 1);
        assert_eq!(import.warnings.len(), 1);
        assert!(import
            .records
            .iter()
            .any(|r| r.body.contains("retinal screening clear")));
        assert!(import
            .records
            .iter()
            .all(|r| !r.id.contains("org-epic-sandbox-org-epic-sandbox")));
        assert!(import
            .records
            .iter()
            .any(|r| r.id == "org-epic-sandbox-obs-01"));

        let mut chart = nadia.clone();
        let mut allergies = default_allergies_for_patient(&nadia.id);
        let first = merge_fhir_import_into_chart(&mut chart, &mut allergies, &import);
        assert_eq!(first.added_records, 4);
        assert_eq!(first.added_medications, 1);
        assert_eq!(first.added_allergies, 1);

        // Merging the exact same bundle a second time deduplicates cleanly.
        let second = merge_fhir_import_into_chart(&mut chart, &mut allergies, &import);
        assert_eq!(second.added_records, 0);
        assert_eq!(second.added_medications, 0);
        assert_eq!(second.added_allergies, 0);
        assert!(second.skipped_duplicates >= 6);
    }

    #[test]
    fn directory_search_filters_by_text_and_vendor() {
        let epics = search_portal_directory("stanford", Some(PortalVendor::EpicMyChart));
        assert_eq!(epics.len(), 1);
        assert!(epics[0].name.contains("Stanford"));

        let empty = search_portal_directory("stanford", Some(PortalVendor::OracleHealth));
        assert!(empty.is_empty());

        // Verify expanded SMART + behavioral health + non-FHIR directory coverage
        for id in [
            "org-healow-sandbox",
            "org-meditech-expanse",
            "org-nextgen-clinic",
            "org-veradigm-sandbox",
            "org-elation-sandbox",
            "org-kipu-sandbox",
            "org-hint-dpc",
            "org-simplepractice-clinic",
            "org-drchrono-clinic",
            "org-modmed-specialty",
        ] {
            assert!(
                find_portal_organization(id).is_some(),
                "missing expanded directory entry {id}"
            );
        }
    }

    #[test]
    fn vendor_transport_selects_smart_v1_vs_v2_and_blocks_non_smart_vendors() {
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";

        // Oracle Health selects explicit SMART v1 `.read` scopes (no wildcards).
        let oracle = find_portal_organization("org-oracle-sandbox").unwrap();
        let oracle_sess = build_smart_authorize_url(
            &oracle,
            "dabney-patient",
            "https://dabney.moe/clinical/patient",
            None,
            "state-oracle-1234",
            verifier,
        )
        .unwrap();
        assert!(oracle_sess.scopes.contains("patient/Patient.read"));
        assert!(!oracle_sess.scopes.contains('*'));
        assert!(oracle_sess.read_only);

        // Medicare Blue Button selects claims-only scopes.
        let medicare = find_portal_organization("org-medicare-bb").unwrap();
        let med_sess = build_smart_authorize_url(
            &medicare,
            "dabney-patient",
            "https://dabney.moe/clinical/patient",
            None,
            "state-cms-1234",
            verifier,
        )
        .unwrap();
        assert!(med_sess.scopes.contains("ExplanationOfBenefit.rs"));

        // Non-SMART vendors (KipuEMR, SimplePractice, Hint Health) fail closed with guidance.
        for non_smart_id in [
            "org-kipu-sandbox",
            "org-simplepractice-clinic",
            "org-hint-dpc",
        ] {
            let org = find_portal_organization(non_smart_id).unwrap();
            assert!(!org.supports_smart_on_fhir());
            let err = build_smart_authorize_url(
                &org,
                "dabney-patient",
                "https://dabney.moe/clinical/patient",
                None,
                "state-non-smart-1",
                verifier,
            )
            .unwrap_err();
            assert!(err.contains("does not expose a live SMART on FHIR R4 launch endpoint"));
        }
    }

    #[test]
    fn smart_health_link_parser_validates_key_ssrf_flags_version_and_expiry() {
        let nadia = find_patient("pt-nadia-kulkarni").unwrap();
        let uri = sandbox_smart_health_link_uri(&nadia);
        assert!(uri.starts_with("shlink:/"));

        let parsed = parse_smart_health_link(&uri, Some(1_700_000_000)).expect("valid shlink");
        assert_eq!(parsed.v, 1);
        assert!(parsed.requires_passcode());
        assert!(parsed.is_long_term());
        assert!(!parsed.is_direct_file_url());
        assert_eq!(parsed.key.len(), 43);

        // Viewer-wrapped URI works too.
        let wrapped = format!("https://viewer.example.org/shl#{uri}");
        let parsed_wrapped =
            parse_smart_health_link(&wrapped, Some(1_700_000_000)).expect("wrapped shlink");
        assert_eq!(
            parsed_wrapped.viewer_origin.as_deref(),
            Some("https://viewer.example.org/shl")
        );

        // Expired link rejected.
        assert!(parse_smart_health_link(&uri, Some(1_900_000_000)).is_err());

        // Manifest POST requires passcode when P is set.
        assert!(build_shlink_manifest_request_body(&parsed, "Dabney Viewer", None, None).is_err());
        let body =
            build_shlink_manifest_request_body(&parsed, "Dabney Viewer", Some("1234"), Some(65536))
                .expect("manifest body");
        assert_eq!(body["passcode"], "1234");

        // Invalid `PU` combination rejected.
        let valid_key = parsed.key.clone();
        let pu_json = json!({
            "url": "https://example.org/shl/abc",
            "key": valid_key,
            "flag": "PU",
            "v": 1
        })
        .to_string();
        let pu_uri = format!("shlink:/{}", base64url_encode(pu_json.as_bytes()));
        assert!(parse_smart_health_link(&pu_uri, None).is_err());

        // SSRF private metadata IP rejected.
        let ssrf_json = json!({
            "url": "https://169.254.169.254/latest/meta-data",
            "key": parsed.key,
            "v": 1
        })
        .to_string();
        let ssrf_uri = format!("shlink:/{}", base64url_encode(ssrf_json.as_bytes()));
        assert!(parse_smart_health_link(&ssrf_uri, None).is_err());

        // Short key (<32 bytes) rejected.
        let short_key_json = json!({
            "url": "https://example.org/shl/abc",
            "key": base64url_encode(b"too-short"),
            "v": 1
        })
        .to_string();
        let short_uri = format!("shlink:/{}", base64url_encode(short_key_json.as_bytes()));
        assert!(parse_smart_health_link(&short_uri, None).is_err());

        // Non-canonical IPv4, NAT64 IPv6, reserved IPv4, and non-443 port SSRF checks.
        for bad_url in [
            "https://127.1/shl",
            "https://0177.0.0.1/shl",
            "https://0x7f.0.0.1/shl",
            "https://2130706433/shl",
            "https://[::127.0.0.1]/shl",
            "https://[64:ff9b::10.0.0.1]/shl",
            "https://240.0.0.1/shl",
            "https://example.org:8443/shl",
        ] {
            assert!(
                validate_public_https_url(bad_url).is_err(),
                "expected {bad_url} to be rejected by SSRF guard"
            );
        }

        // Debug output redacts the 32-byte AES-256 key.
        let dbg = format!("{parsed:?}");
        assert!(dbg.contains("[REDACTED]"));
        assert!(!dbg.contains(&parsed.key));
    }

    #[test]
    fn unified_ingest_clinical_document_dispatches_all_five_formats() {
        let nadia = find_patient("pt-nadia-kulkarni").unwrap();

        let fhir = ingest_clinical_document(
            "Epic",
            "org-epic-sandbox",
            &sandbox_fhir_bundle_for_org("org-epic-sandbox", &nadia),
        )
        .unwrap();
        assert_eq!(fhir.format, ClinicalDocumentFormat::FhirR4Json);
        assert!(fhir.import.is_some());

        let kipu = ingest_clinical_document(
            "KipuEMR",
            "org-kipu-sandbox",
            &crate::kipu::sandbox_kipu_payload_for_patient(&nadia),
        )
        .unwrap();
        assert_eq!(kipu.format, ClinicalDocumentFormat::KipuJson);
        assert_eq!(
            kipu.import.as_ref().unwrap().confidentiality,
            ConfidentialityClass::Part2Sud
        );

        let ccda = ingest_clinical_document(
            "SimplePractice",
            "org-simplepractice",
            &crate::ccda::sandbox_ccda_xml_for_patient(&nadia),
        )
        .unwrap();
        assert_eq!(ccda.format, ClinicalDocumentFormat::CcdaR21Xml);
        assert!(ccda.import.is_some());

        let hl7 = ingest_clinical_document(
            "Regional Lab",
            "org-hl7-lab",
            &crate::hl7v2::sandbox_hl7v2_message_for_patient(&nadia),
        )
        .unwrap();
        assert_eq!(hl7.format, ClinicalDocumentFormat::Hl7V2Pipe);
        assert!(hl7.import.is_some());

        let shl =
            ingest_clinical_document("SHLink", "shlink", &sandbox_smart_health_link_uri(&nadia))
                .unwrap();
        assert_eq!(shl.format, ClinicalDocumentFormat::SmartHealthLinkUri);
        assert!(shl.smart_health_link.is_some());
    }
}
