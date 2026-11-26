//! Patient EHR portal link-through: searchable health-system directory,
//! standards-compliant SMART on FHIR R4 OAuth 2.0 + PKCE (`S256`) session
//! builder, and partial-failure-tolerant HL7 FHIR R4 Bundle ingestion.
//!
//! Pure Rust with zero external crypto or HTTP crates so the exact same code
//! runs identically in native SSR, browser WebAssembly, Android/Desktop Tauri,
//! and static-musl/FreeBSD binaries.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{ChartRecord, Medication, PatientChart, Problem, RecordKind};

/// Default read-only SMART App Launch v2 scopes requested when a patient links
/// their own doctor's portal. Dabney.moe never requests write scopes.
pub const SMART_READ_ONLY_SCOPES: &str = "openid fhirUser offline_access \
    patient/Patient.rs patient/Observation.rs patient/Condition.rs \
    patient/MedicationRequest.rs patient/DiagnosticReport.rs \
    patient/DocumentReference.rs patient/AllergyIntolerance.rs";

/// Supported patient-portal EHR vendors.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PortalVendor {
    EpicMyChart,
    OracleHealth,
    AthenaHealth,
    Healow,
    VaHealth,
    MedicareBlueButton,
    CustomSmartFhir,
}

impl PortalVendor {
    pub const fn label(self) -> &'static str {
        match self {
            Self::EpicMyChart => "Epic / MyChart",
            Self::OracleHealth => "Oracle Health (Cerner)",
            Self::AthenaHealth => "athenahealth",
            Self::Healow => "healow / eClinicalWorks",
            Self::VaHealth => "VA Health (My HealtheVet)",
            Self::MedicareBlueButton => "Medicare Blue Button 2.0",
            Self::CustomSmartFhir => "Custom SMART on FHIR R4",
        }
    }

    pub const fn short_badge(self) -> &'static str {
        match self {
            Self::EpicMyChart => "Epic MyChart",
            Self::OracleHealth => "Oracle Health",
            Self::AthenaHealth => "athenahealth",
            Self::Healow => "healow",
            Self::VaHealth => "VA FHIR",
            Self::MedicareBlueButton => "Medicare",
            Self::CustomSmartFhir => "SMART FHIR",
        }
    }

    pub const fn all() -> &'static [Self] {
        &[
            Self::EpicMyChart,
            Self::OracleHealth,
            Self::AthenaHealth,
            Self::Healow,
            Self::VaHealth,
            Self::MedicareBlueButton,
            Self::CustomSmartFhir,
        ]
    }
}

/// Directory entry describing a patient portal's SMART on FHIR R4 endpoints and
/// Health Information Management (HIM) medical-records contact details (also
/// reusable as an ROI target).
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
}

/// How a portal connection obtained its records.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PortalConnectionMode {
    LiveSmartFhir,
    ImportedFhirBundle,
    InteractiveSandboxDemo,
}

impl PortalConnectionMode {
    pub const fn label(self) -> &'static str {
        match self {
            Self::LiveSmartFhir => "Live SMART on FHIR",
            Self::ImportedFhirBundle => "Imported FHIR R4 file",
            Self::InteractiveSandboxDemo => "Interactive Sandbox (Zero PHI)",
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

/// Structured result produced by [`ingest_fhir_r4_bundle`].
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

/// Curated searchable US health-system & SMART on FHIR R4 sandbox directory.
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
        },
        PortalOrganization {
            id: "org-athena-clinic".into(),
            name: "Pacific Primary & Metabolic Care (athenahealth)".into(),
            city_state: "Pasadena, CA · Demo Clinic".into(),
            vendor: PortalVendor::AthenaHealth,
            fhir_base_url: "https://api.preview.platform.athenahealth.com/fhir/r4".into(),
            authorize_url: "https://api.preview.platform.athenahealth.com/oauth2/v1/authorize".into(),
            token_url: "https://api.preview.platform.athenahealth.com/oauth2/v1/token".into(),
            him_email: Some("records@pacific-metabolic-demo.invalid".into()),
            him_fax: Some("+1-555-014-2200".into()),
            him_address: Some("100 E California Blvd, Pasadena, CA 91105 (Demo)".into()),
            is_sandbox: true,
        },
        PortalOrganization {
            id: "org-va-health".into(),
            name: "U.S. Department of Veterans Affairs (VA Health)".into(),
            city_state: "Nationwide, US".into(),
            vendor: PortalVendor::VaHealth,
            fhir_base_url: "https://sandbox-api.va.gov/services/fhir/v0/r4".into(),
            authorize_url: "https://sandbox-api.va.gov/oauth2/health/v1/authorization".into(),
            token_url: "https://sandbox-api.va.gov/oauth2/health/v1/token".into(),
            him_email: None,
            him_fax: None,
            him_address: Some("Local VA Medical Center Release of Information Office".into()),
            is_sandbox: true,
        },
        PortalOrganization {
            id: "org-medicare-bb".into(),
            name: "Medicare Blue Button 2.0 (CMS Claims & Meds)".into(),
            city_state: "Nationwide, US".into(),
            vendor: PortalVendor::MedicareBlueButton,
            fhir_base_url: "https://sandbox.bluebutton.cms.gov/v2/fhir".into(),
            authorize_url: "https://sandbox.bluebutton.cms.gov/v2/o/authorize/".into(),
            token_url: "https://sandbox.bluebutton.cms.gov/v2/o/token/".into(),
            him_email: None,
            him_fax: None,
            him_address: Some("Centers for Medicare & Medicaid Services, Baltimore, MD".into()),
            is_sandbox: true,
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

/// Constructs a standards-compliant SMART on FHIR R4 OAuth 2.0 + PKCE (`S256`)
/// sign-in session and authorize URL for `org`.
pub fn build_smart_authorize_url(
    org: &PortalOrganization,
    client_id: &str,
    redirect_uri: &str,
    scopes: Option<&str>,
    state: &str,
    code_verifier: &str,
) -> Result<SmartPkceSession, String> {
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

    let chosen_scopes = scopes
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(SMART_READ_ONLY_SCOPES);

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
        read_only: !chosen_scopes.contains(".write") && !chosen_scopes.contains(".u"),
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
            "Observation" => match parse_observation(obj, &stable_id, source_label) {
                Ok(rec) => result.records.push(rec),
                Err(reason) => result.warnings.push(FhirResourceWarning {
                    index: idx,
                    resource_type: rtype.into(),
                    resource_id: rid,
                    reason,
                }),
            },
            "Condition" => match parse_condition(obj) {
                Ok(prob) => result.problems.push(prob),
                Err(reason) => result.warnings.push(FhirResourceWarning {
                    index: idx,
                    resource_type: rtype.into(),
                    resource_id: rid,
                    reason,
                }),
            },
            "MedicationRequest" | "MedicationStatement" => match parse_medication(obj) {
                Ok(med) => result.medications.push(med),
                Err(reason) => result.warnings.push(FhirResourceWarning {
                    index: idx,
                    resource_type: rtype.into(),
                    resource_id: rid,
                    reason,
                }),
            },
            "DiagnosticReport" => match parse_diagnostic_report(obj, &stable_id, source_label) {
                Ok(rec) => result.records.push(rec),
                Err(reason) => result.warnings.push(FhirResourceWarning {
                    index: idx,
                    resource_type: rtype.into(),
                    resource_id: rid,
                    reason,
                }),
            },
            "DocumentReference" => match parse_document_reference(obj, &stable_id, source_label) {
                Ok(rec) => result.records.push(rec),
                Err(reason) => result.warnings.push(FhirResourceWarning {
                    index: idx,
                    resource_type: rtype.into(),
                    resource_id: rid,
                    reason,
                }),
            },
            "AllergyIntolerance" => match parse_allergy(obj, &stable_id, source_label) {
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
            },
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
        }],
        "pt-carlos-mendez" => vec![AllergyEntry {
            id: "cm-alg-01".into(),
            substance: "Penicillin".into(),
            reaction: "Hives (childhood)".into(),
            criticality: "High".into(),
            recorded_at: "2014-03-02".into(),
            source_label: "Dabney.moe Synthetic Clinic".into(),
        }],
        "pt-david-okafor" => vec![AllergyEntry {
            id: "do-alg-01".into(),
            substance: "Ibuprofen / NSAIDs".into(),
            reaction: "Avoid with apixaban anticoagulation (bleeding precaution)".into(),
            criticality: "High".into(),
            recorded_at: "2025-09-10".into(),
            source_label: "Dabney.moe Synthetic Clinic".into(),
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

fn base64_encode_standard(input: &[u8]) -> String {
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

fn sanitize_id_slug(raw: &str) -> String {
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

fn date_prefix(raw: Option<&str>) -> String {
    raw.map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.chars().take(10).collect())
        .unwrap_or_else(|| "Unknown date".into())
}

fn parse_observation(
    obj: &serde_json::Map<String, Value>,
    stable_id: &str,
    source_label: &str,
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
    })
}

fn parse_condition(obj: &serde_json::Map<String, Value>) -> Result<Problem, String> {
    let label = extract_codeable_text(obj.get("code"))
        .ok_or_else(|| "Condition missing `code` display/text.".to_string())?;
    let code = extract_coding_code(obj.get("code")).unwrap_or_else(|| "FHIR".into());
    let status =
        extract_codeable_text(obj.get("clinicalStatus")).unwrap_or_else(|| "Active".into());
    Ok(Problem {
        code,
        label,
        status,
    })
}

fn parse_medication(obj: &serde_json::Map<String, Value>) -> Result<Medication, String> {
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
    })
}

fn parse_diagnostic_report(
    obj: &serde_json::Map<String, Value>,
    stable_id: &str,
    source_label: &str,
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
    })
}

fn parse_document_reference(
    obj: &serde_json::Map<String, Value>,
    stable_id: &str,
    source_label: &str,
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
    })
}

fn parse_allergy(
    obj: &serde_json::Map<String, Value>,
    stable_id: &str,
    source_label: &str,
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
    }
}
