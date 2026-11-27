//! KipuEMR behavioral health & Substance Use Disorder (SUD) integration module:
//! - Pure-Rust RFC 2104 HMAC-SHA1, HMAC-SHA256, and RFC 1321 MD5 implementing
//!   Kipu Health API v3 `APIAuth` / `APIAuth-HMAC-SHA256` and API v4 Bearer
//!   request envelopes (zero external crypto crates; runs identically on SSR,
//!   WASM, and Tauri).
//! - Defensive, partial-failure-tolerant Kipu JSON bundle importer preserving
//!   `casefile_id` provenance and explicit **42 CFR Part 2** confidentiality
//!   tagging (`ConfidentialityClass::Part2Sud`).
//! - Zero-PHI fictional Kipu sandbox payload for interactive testing.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::ehr_portal::{
    base64_encode_standard, date_prefix, sanitize_id_slug, validate_public_https_url, AllergyEntry,
    FhirImportResult, FhirResourceWarning,
};
use crate::{
    ChartRecord, ConfidentialityClass, Medication, PatientChart, Problem, RecordKind,
    PART2_REDISCLOSURE_NOTICE_SHORT,
};

/// Default Kipu Health API base URL (tenant-provisioned via Kipu Super Admin).
pub const KIPU_DEFAULT_BASE_URL: &str = "https://api.kipuworks.com";

/// Required `Accept` media-type header for Kipu Health API v3.
pub const KIPU_API_V3_ACCEPT: &str = "application/vnd.kipusystems+json; version=3";

/// Required `Accept` media-type header for Kipu Health API v4 (OAuth 2.0 Bearer).
pub const KIPU_API_V4_ACCEPT: &str = "application/vnd.kipusystems+json; version=4";

/// Authentication strategy for Kipu Health API requests.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KipuAuthScheme {
    /// Kipu API v3 `APIAuth <access_id>:<base64(HMAC-SHA1)>` (Ruby `api-auth`).
    #[default]
    ApiAuthHmacSha1,
    /// Kipu API v3 `APIAuth-HMAC-SHA256 <access_id>:<base64(HMAC-SHA256)>`.
    ApiAuthHmacSha256,
    /// Kipu API v4 `Bearer <oauth2_access_token>` (`Accept: ...; version=4`).
    OAuthBearerV4,
}

impl KipuAuthScheme {
    pub const fn label(self) -> &'static str {
        match self {
            Self::ApiAuthHmacSha1 => "Kipu API v3 (APIAuth HMAC-SHA1)",
            Self::ApiAuthHmacSha256 => "Kipu API v3 (APIAuth HMAC-SHA256)",
            Self::OAuthBearerV4 => "Kipu API v4 (OAuth 2.0 Bearer)",
        }
    }

    pub const fn accept_header(self) -> &'static str {
        match self {
            Self::ApiAuthHmacSha1 | Self::ApiAuthHmacSha256 => KIPU_API_V3_ACCEPT,
            Self::OAuthBearerV4 => KIPU_API_V4_ACCEPT,
        }
    }
}

/// Verified Kipu Health API v3 endpoint paths.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KipuEndpoint {
    PatientsCensus,
    Patients,
    CensusLatest,
    Admissions,
    Episodes,
    PatientEvaluations,
    VitalSigns,
    Allergies,
    EpisodeOrders,
    Appointments,
    ConsentForms,
}

impl KipuEndpoint {
    pub const fn path(self) -> &'static str {
        match self {
            Self::PatientsCensus => "/api/patients/census",
            Self::Patients => "/api/patients",
            Self::CensusLatest => "/api/v3/census/latest",
            Self::Admissions => "/api/v3/admissions",
            Self::Episodes => "/api/v3/episodes",
            Self::PatientEvaluations => "/api/v3/patient_evaluations",
            Self::VitalSigns => "/api/v3/vital_signs",
            Self::Allergies => "/api/v3/allergies",
            Self::EpisodeOrders => "/api/v3/episode_orders",
            Self::Appointments => "/api/v3/appointments",
            Self::ConsentForms => "/api/v3/consent_form_records",
        }
    }
}

/// Signed Kipu API HTTP request envelope ready for transmission or inspection.
///
/// Implements redacted [`std::fmt::Debug`] so the HMAC signature / bearer token
/// is never leaked via debug formatting; use [`Self::redacted_authorization`]
/// for operator-safe diagnostics.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KipuSignedRequest {
    pub method: String,
    pub base_url: String,
    pub request_uri: String,
    pub full_url: String,
    pub auth_scheme: KipuAuthScheme,
    pub accept: String,
    pub date: String,
    pub content_type: String,
    pub content_md5: String,
    pub canonical_string: String,
    pub authorization: String,
    pub part2_protected: bool,
}

impl std::fmt::Debug for KipuSignedRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KipuSignedRequest")
            .field("method", &self.method)
            .field("base_url", &self.base_url)
            .field("request_uri", &self.request_uri)
            .field("auth_scheme", &self.auth_scheme)
            .field("accept", &self.accept)
            .field("date", &self.date)
            .field("content_type", &self.content_type)
            .field("content_md5", &self.content_md5)
            .field("authorization", &self.redacted_authorization())
            .field("part2_protected", &self.part2_protected)
            .finish()
    }
}

impl KipuSignedRequest {
    /// Returns an operator-safe redacted preview of the `Authorization` header
    /// that omits the HMAC digest or bearer secret.
    #[must_use]
    pub fn redacted_authorization(&self) -> String {
        match self.auth_scheme {
            KipuAuthScheme::ApiAuthHmacSha1 | KipuAuthScheme::ApiAuthHmacSha256 => {
                if let Some((prefix, _sig)) = self.authorization.rsplit_once(':') {
                    format!("{prefix}:[REDACTED]")
                } else {
                    "[REDACTED]".into()
                }
            }
            KipuAuthScheme::OAuthBearerV4 => "Bearer [REDACTED]".into(),
        }
    }
}

/// Builds and signs a Kipu Health API v3 (`APIAuth` / `APIAuth-HMAC-SHA256`) or
/// API v4 (`Bearer`) request envelope.
///
/// Exact Kipu API v3 canonical string (strict comma-separated, no spaces):
/// `{Content-Type},{Content-MD5},{Request-URI},{Date}`
/// where `Request-URI` is path (+ optional query), excluding scheme and host.
#[allow(clippy::too_many_arguments)]
pub fn sign_kipu_request(
    base_url: &str,
    method: &str,
    request_uri: &str,
    date_rfc1123: &str,
    content_type: Option<&str>,
    body: Option<&[u8]>,
    auth_scheme: KipuAuthScheme,
    access_id: &str,
    secret_or_token: &str,
    allow_loopback_http: bool,
) -> Result<KipuSignedRequest, String> {
    let normalized_base = if allow_loopback_http {
        crate::ehr_portal::validate_smart_endpoint_url(base_url)?
    } else {
        validate_public_https_url(base_url)?
    };

    let verb = method.trim().to_ascii_uppercase();
    if !matches!(
        verb.as_str(),
        "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD"
    ) {
        return Err(format!("Unsupported HTTP method for Kipu API: `{verb}`."));
    }

    let uri = request_uri.trim();
    if !uri.starts_with('/') || uri.starts_with("//") || uri.chars().any(char::is_whitespace) {
        return Err(
            "Kipu Request-URI must start with a single `/` (path + optional query, excluding scheme and host) with no whitespace."
                .into(),
        );
    }

    let date = date_rfc1123.trim();
    if date.is_empty() || date.contains('\r') || date.contains('\n') {
        return Err("Kipu `Date` header must be a valid RFC 1123 HTTP-date in GMT.".into());
    }

    let secret = secret_or_token.trim();
    if secret.is_empty() {
        return Err("Kipu secret key or OAuth bearer token must not be empty.".into());
    }

    let has_body = body.is_some_and(|b| !b.is_empty());
    let (ct, md5_b64) = if has_body {
        let ct_val = content_type
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("application/json");
        if ct_val.contains(',') || ct_val.contains('\r') || ct_val.contains('\n') {
            return Err("Invalid Content-Type header for Kipu signing.".into());
        }
        (ct_val.to_string(), content_md5_base64(body.unwrap_or(&[])))
    } else {
        // Kipu / api-auth uses empty Content-Type and Content-MD5 for bodyless GET/DELETE.
        let ct_val = content_type
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("");
        (ct_val.to_string(), String::new())
    };

    let canonical_string = format!("{ct},{md5_b64},{uri},{date}");

    let authorization = match auth_scheme {
        KipuAuthScheme::ApiAuthHmacSha1 => {
            let aid = access_id.trim();
            if aid.is_empty() || aid.contains(':') || aid.chars().any(char::is_whitespace) {
                return Err(
                    "Kipu `access_id` must be non-empty and contain no colons or whitespace."
                        .into(),
                );
            }
            let mac = hmac_sha1(secret.as_bytes(), canonical_string.as_bytes());
            format!("APIAuth {aid}:{}", base64_encode_standard(&mac))
        }
        KipuAuthScheme::ApiAuthHmacSha256 => {
            let aid = access_id.trim();
            if aid.is_empty() || aid.contains(':') || aid.chars().any(char::is_whitespace) {
                return Err(
                    "Kipu `access_id` must be non-empty and contain no colons or whitespace."
                        .into(),
                );
            }
            let mac = hmac_sha256(secret.as_bytes(), canonical_string.as_bytes());
            format!("APIAuth-HMAC-SHA256 {aid}:{}", base64_encode_standard(&mac))
        }
        KipuAuthScheme::OAuthBearerV4 => format!("Bearer {secret}"),
    };

    Ok(KipuSignedRequest {
        method: verb,
        full_url: format!("{normalized_base}{uri}"),
        base_url: normalized_base,
        request_uri: uri.to_string(),
        auth_scheme,
        accept: auth_scheme.accept_header().to_string(),
        date: date.to_string(),
        content_type: ct,
        content_md5: md5_b64,
        canonical_string,
        authorization,
        part2_protected: true,
    })
}

/// Computes `base64(MD5(body))` for the HTTP `Content-MD5` header used by Kipu API v3.
#[must_use]
pub fn content_md5_base64(body: &[u8]) -> String {
    base64_encode_standard(&md5_bytes(body))
}

/// Pure-Rust FIPS 180-4 SHA-1 digest (used exclusively for Kipu API v3 legacy
/// `APIAuth` HMAC-SHA1 compatibility).
#[must_use]
pub fn sha1_bytes(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0];

    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut padded = Vec::with_capacity(data.len() + 72);
    padded.extend_from_slice(data);
    padded.push(0x80);
    while (padded.len() % 64) != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in padded.chunks_exact(64) {
        let mut w = [0u32; 80];
        for (i, word) in w.iter_mut().take(16).enumerate() {
            let b = i * 4;
            *word = u32::from_be_bytes([chunk[b], chunk[b + 1], chunk[b + 2], chunk[b + 3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }

        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (i, &wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A827999u32),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1u32),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDCu32),
                _ => (b ^ c ^ d, 0xCA62C1D6u32),
            };
            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
    }

    let mut out = [0u8; 20];
    for (i, word) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

/// Pure-Rust RFC 2104 HMAC-SHA1.
#[must_use]
pub fn hmac_sha1(key: &[u8], message: &[u8]) -> [u8; 20] {
    let mut key_block = [0u8; 64];
    if key.len() > 64 {
        let hashed = sha1_bytes(key);
        key_block[..20].copy_from_slice(&hashed);
    } else {
        key_block[..key.len()].copy_from_slice(key);
    }

    let mut inner = Vec::with_capacity(64 + message.len());
    for &b in &key_block {
        inner.push(b ^ 0x36);
    }
    inner.extend_from_slice(message);
    let inner_hash = sha1_bytes(&inner);

    let mut outer = Vec::with_capacity(64 + 20);
    for &b in &key_block {
        outer.push(b ^ 0x5c);
    }
    outer.extend_from_slice(&inner_hash);
    sha1_bytes(&outer)
}

/// Pure-Rust RFC 2104 HMAC-SHA256 built atop [`crate::ehr_portal::sha256_bytes`].
#[must_use]
pub fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    let mut key_block = [0u8; 64];
    if key.len() > 64 {
        let hashed = crate::ehr_portal::sha256_bytes(key);
        key_block[..32].copy_from_slice(&hashed);
    } else {
        key_block[..key.len()].copy_from_slice(key);
    }

    let mut inner = Vec::with_capacity(64 + message.len());
    for &b in &key_block {
        inner.push(b ^ 0x36);
    }
    inner.extend_from_slice(message);
    let inner_hash = crate::ehr_portal::sha256_bytes(&inner);

    let mut outer = Vec::with_capacity(64 + 32);
    for &b in &key_block {
        outer.push(b ^ 0x5c);
    }
    outer.extend_from_slice(&inner_hash);
    crate::ehr_portal::sha256_bytes(&outer)
}

/// Pure-Rust RFC 1321 MD5 digest (used exclusively for Kipu API v3 HTTP
/// `Content-MD5` request signing compatibility).
#[must_use]
pub fn md5_bytes(data: &[u8]) -> [u8; 16] {
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5,
        9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10,
        15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    const K: [u32; 64] = [
        0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613,
        0xfd469501, 0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193,
        0xa679438e, 0x49b40821, 0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d,
        0x02441453, 0xd8a1e681, 0xe7d3fbc8, 0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed,
        0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a, 0xfffa3942, 0x8771f681, 0x6d9d6122,
        0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70, 0x289b7ec6, 0xeaa127fa,
        0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665, 0xf4292244,
        0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
        0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb,
        0xeb86d391,
    ];

    let mut a0: u32 = 0x67452301;
    let mut b0: u32 = 0xefcdab89;
    let mut c0: u32 = 0x98badcfe;
    let mut d0: u32 = 0x10325476;

    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut padded = Vec::with_capacity(data.len() + 72);
    padded.extend_from_slice(data);
    padded.push(0x80);
    while (padded.len() % 64) != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_le_bytes());

    for chunk in padded.chunks_exact(64) {
        let mut m = [0u32; 16];
        for (i, word) in m.iter_mut().enumerate() {
            let b = i * 4;
            *word = u32::from_le_bytes([chunk[b], chunk[b + 1], chunk[b + 2], chunk[b + 3]]);
        }

        let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);
        for i in 0..64 {
            let (f, g) = match i {
                0..=15 => ((b & c) | ((!b) & d), i),
                16..=31 => ((d & b) | ((!d) & c), (5 * i + 1) % 16),
                32..=47 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | (!d)), (7 * i) % 16),
            };
            let f_sum = f.wrapping_add(a).wrapping_add(K[i]).wrapping_add(m[g]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(f_sum.rotate_left(S[i]));
        }

        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }

    let mut out = [0u8; 16];
    out[0..4].copy_from_slice(&a0.to_le_bytes());
    out[4..8].copy_from_slice(&b0.to_le_bytes());
    out[8..12].copy_from_slice(&c0.to_le_bytes());
    out[12..16].copy_from_slice(&d0.to_le_bytes());
    out
}

/// Parses a Kipu Health API v3/v4 JSON bundle or export payload covering
/// patients/census, evaluations/assessments, vital signs, allergies,
/// medications/orders, labs, diagnoses, appointments, and consents into a
/// normalized [`FhirImportResult`].
///
/// Every imported record, medication, problem, and allergy defaults to
/// [`ConfidentialityClass::Part2Sud`] so Substance Use Disorder (SUD) records
/// retain their **42 CFR Part 2** statutory protections and never silently blend
/// into general exports. Malformed entries record non-fatal warnings without
/// blocking sibling valid entries.
pub fn ingest_kipu_payload(
    source_label: &str,
    portal_id: &str,
    raw_json: &str,
) -> Result<FhirImportResult, String> {
    let parsed: Value =
        serde_json::from_str(raw_json).map_err(|err| format!("Invalid KipuEMR JSON: {err}"))?;
    let root = parsed
        .as_object()
        .ok_or_else(|| "KipuEMR payload must be a JSON object.".to_string())?;

    let prefix = sanitize_id_slug(portal_id);
    let mut result = FhirImportResult {
        source_label: source_label.trim().to_string(),
        portal_id: portal_id.trim().to_string(),
        patient_name: None,
        patient_birth_date: None,
        records: Vec::new(),
        medications: Vec::new(),
        problems: Vec::new(),
        allergies: Vec::new(),
        total_entries_seen: 0,
        warnings: Vec::new(),
        confidentiality: ConfidentialityClass::Part2Sud,
    };

    // Resolve top-level or nested patient / census demographics & casefile_id.
    let patient_obj = root
        .get("patient")
        .and_then(Value::as_object)
        .or_else(|| {
            root.get("patients")
                .or_else(|| root.get("census"))
                .and_then(Value::as_array)
                .and_then(|arr| arr.first())
                .and_then(Value::as_object)
        })
        .unwrap_or(root);

    let casefile_id = str_field(patient_obj, &["casefile_id", "casefileId", "episode_id"])
        .or_else(|| str_field(root, &["casefile_id", "casefileId"]))
        .map(str::to_string);

    if let Some(name) = extract_kipu_patient_name(patient_obj) {
        result.patient_name = Some(name);
    }
    if let Some(dob) = str_field(
        patient_obj,
        &["dob", "birth_date", "birthDate", "date_of_birth"],
    ) {
        result.patient_birth_date = Some(date_prefix(Some(dob)));
    }
    if let Some(admission_status) = str_field(patient_obj, &["admission_status", "status"]) {
        if let Some(cf) = &casefile_id {
            result.total_entries_seen += 1;
            let program = str_field(patient_obj, &["program", "location_name", "facility"])
                .unwrap_or("KipuEMR Behavioral Health Program");
            result.records.push(ChartRecord {
                id: format!("{prefix}-kipu-census-{}", sanitize_id_slug(cf)),
                kind: RecordKind::ProgressNote,
                title: format!("KipuEMR Episode / Census Summary ({cf})"),
                recorded_at: date_prefix(str_field(
                    patient_obj,
                    &["admission_date", "admitted_at", "updated_at"],
                )),
                source_label: source_label.to_string(),
                body: format!(
                    "Casefile {cf} at {program} — status: {admission_status}. {PART2_REDISCLOSURE_NOTICE_SHORT}"
                ),
                key_values: vec![
                    ("Casefile ID".into(), cf.clone()),
                    ("Admission status".into(), admission_status.to_string()),
                    ("Confidentiality".into(), "42 CFR Part 2 (SUD)".into()),
                ],
                confidentiality: ConfidentialityClass::Part2Sud,
            });
        }
    }

    // Evaluations / Assessments (`evaluations`, `patient_evaluations`, `assessments`).
    if let Some(items) = array_field(root, &["patient_evaluations", "evaluations", "assessments"]) {
        for (idx, item) in items.iter().enumerate() {
            result.total_entries_seen += 1;
            match parse_kipu_evaluation(item, idx, &prefix, source_label, casefile_id.as_deref()) {
                Ok(rec) => result.records.push(rec),
                Err(reason) => result.warnings.push(FhirResourceWarning {
                    index: idx,
                    resource_type: "KipuEvaluation".into(),
                    resource_id: item_id(item),
                    reason,
                }),
            }
        }
    }

    // Vital Signs (`vital_signs`, `vitals`, `orthostatic_vital_signs`).
    if let Some(items) = array_field(root, &["vital_signs", "vitals", "orthostatic_vital_signs"]) {
        for (idx, item) in items.iter().enumerate() {
            result.total_entries_seen += 1;
            match parse_kipu_vital(item, idx, &prefix, source_label, casefile_id.as_deref()) {
                Ok(rec) => result.records.push(rec),
                Err(reason) => result.warnings.push(FhirResourceWarning {
                    index: idx,
                    resource_type: "KipuVitalSign".into(),
                    resource_id: item_id(item),
                    reason,
                }),
            }
        }
    }

    // Allergies (`allergies`).
    if let Some(items) = array_field(root, &["allergies"]) {
        for (idx, item) in items.iter().enumerate() {
            result.total_entries_seen += 1;
            match parse_kipu_allergy(item, idx, &prefix, source_label) {
                Ok((allergy, rec)) => {
                    result.allergies.push(allergy);
                    result.records.push(rec);
                }
                Err(reason) => result.warnings.push(FhirResourceWarning {
                    index: idx,
                    resource_type: "KipuAllergy".into(),
                    resource_id: item_id(item),
                    reason,
                }),
            }
        }
    }

    // Medications & Episode Orders (`medications`, `episode_orders`, `patient_orders`).
    if let Some(items) = array_field(root, &["medications", "episode_orders", "patient_orders"]) {
        for (idx, item) in items.iter().enumerate() {
            result.total_entries_seen += 1;
            match parse_kipu_medication(item) {
                Ok(med) => result.medications.push(med),
                Err(reason) => result.warnings.push(FhirResourceWarning {
                    index: idx,
                    resource_type: "KipuMedicationOrder".into(),
                    resource_id: item_id(item),
                    reason,
                }),
            }
        }
    }

    // Diagnoses / Problems (`diagnoses`, `problems`).
    if let Some(items) = array_field(root, &["diagnoses", "problems"]) {
        for (idx, item) in items.iter().enumerate() {
            result.total_entries_seen += 1;
            match parse_kipu_diagnosis(item) {
                Ok(prob) => result.problems.push(prob),
                Err(reason) => result.warnings.push(FhirResourceWarning {
                    index: idx,
                    resource_type: "KipuDiagnosis".into(),
                    resource_id: item_id(item),
                    reason,
                }),
            }
        }
    }

    // Labs / Toxicology / Results (`labs`, `lab_results`, `glucose_logs`).
    if let Some(items) = array_field(root, &["labs", "lab_results", "glucose_logs"]) {
        for (idx, item) in items.iter().enumerate() {
            result.total_entries_seen += 1;
            match parse_kipu_lab(item, idx, &prefix, source_label) {
                Ok(rec) => result.records.push(rec),
                Err(reason) => result.warnings.push(FhirResourceWarning {
                    index: idx,
                    resource_type: "KipuLab".into(),
                    resource_id: item_id(item),
                    reason,
                }),
            }
        }
    }

    // Appointments / Schedules (`appointments`).
    if let Some(items) = array_field(root, &["appointments"]) {
        for (idx, item) in items.iter().enumerate() {
            result.total_entries_seen += 1;
            match parse_kipu_appointment(item, idx, &prefix, source_label) {
                Ok(rec) => result.records.push(rec),
                Err(reason) => result.warnings.push(FhirResourceWarning {
                    index: idx,
                    resource_type: "KipuAppointment".into(),
                    resource_id: item_id(item),
                    reason,
                }),
            }
        }
    }

    // Consents (`consents`, `consent_forms`, `consent_form_records`).
    if let Some(items) = array_field(root, &["consents", "consent_forms", "consent_form_records"]) {
        for (idx, item) in items.iter().enumerate() {
            result.total_entries_seen += 1;
            match parse_kipu_consent(item, idx, &prefix, source_label) {
                Ok(rec) => result.records.push(rec),
                Err(reason) => result.warnings.push(FhirResourceWarning {
                    index: idx,
                    resource_type: "KipuConsent".into(),
                    resource_id: item_id(item),
                    reason,
                }),
            }
        }
    }

    if result.records.is_empty()
        && result.medications.is_empty()
        && result.problems.is_empty()
        && result.allergies.is_empty()
        && result.patient_name.is_none()
    {
        return Err("No supported KipuEMR clinical records found in JSON payload.".into());
    }

    Ok(result)
}

/// Generates a strictly zero-PHI fictional KipuEMR API v3 behavioral-health
/// sandbox payload for interactive demo and unit testing.
#[must_use]
pub fn sandbox_kipu_payload_for_patient(patient: &PatientChart) -> String {
    let mut name_parts = patient.display_name.split_whitespace();
    let first = name_parts.next().unwrap_or("Sample");
    let last = name_parts.collect::<Vec<_>>().join(" ");
    let last_name = if last.is_empty() { "Patient" } else { &last };

    let payload = json!({
        "kipu_api_version": "3",
        "confidentiality": "42_cfr_part_2",
        "patient": {
            "casefile_id": format!("901:{}", sanitize_id_slug(&patient.mrn)),
            "first_name": first,
            "last_name": last_name,
            "dob": patient.birth_date,
            "admission_status": "Outpatient Recovery Maintenance (Synthetic Demo)",
            "program": "Harborview Behavioral & Recovery Clinic (Zero-PHI Sandbox)",
            "admission_date": "2026-10-12"
        },
        "patient_evaluations": [
            {
                "id": "eval-ciwa-01",
                "name": "CIWA-Ar & Recovery Stability Check-In",
                "status": "completed",
                "completed_at": "2026-11-19",
                "score": "2 (Minimal / Stable)",
                "summary": "Synthetic demo assessment: sleep steady at 7.5 h/night, zero cravings reported over 14 days, continuing weekly group support."
            }
        ],
        "vital_signs": [
            {
                "id": "vit-01",
                "recorded_at": "2026-11-19",
                "blood_pressure": "120/76 mmHg",
                "pulse": "68 bpm",
                "temperature": "98.4 F",
                "spo2": "99%"
            }
        ],
        "allergies": [
            {
                "id": "kalg-01",
                "substance": "Tramadol",
                "reaction": "Severe nausea and dizziness (documented intolerance)",
                "severity": "Moderate",
                "recorded_at": "2026-10-12"
            }
        ],
        "episode_orders": [
            {
                "id": "ord-01",
                "medication": "Naltrexone HCl",
                "dose": "50 mg",
                "schedule": "Once daily by mouth",
                "indication": "Recovery support (42 CFR Part 2 synthetic demo)"
            }
        ],
        "diagnoses": [
            {
                "code": "F10.21",
                "description": "Alcohol use disorder, moderate, in remission (Synthetic Demo)",
                "status": "Active · in sustained remission"
            }
        ],
        "labs": [
            {
                "id": "klab-01",
                "name": "Hepatic Function Panel (AST/ALT)",
                "value": "AST 22 U/L, ALT 19 U/L",
                "status": "final",
                "collected_at": "2026-11-14",
                "notes": "Baseline hepatic transaminases within normal range on maintenance regimen."
            }
        ],
        "appointments": [
            {
                "id": "kappt-01",
                "starts_at": "2026-12-03T10:00:00Z",
                "reason": "Monthly Outpatient Recovery & Medication Follow-Up",
                "provider": "Dr. Jordan Ellis (Demo)",
                "status": "Booked"
            }
        ],
        "consent_form_records": [
            {
                "id": "kcon-01",
                "title": "42 CFR Part 2 Written Consent — Care Coordination",
                "status": "Signed (Synthetic Demo)",
                "signed_at": "2026-10-12",
                "expires_at": "2027-10-12",
                "scope": "Primary care coordination; excludes separate SUD counseling notes."
            }
        ]
    });

    serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".into())
}

fn str_field<'a>(obj: &'a serde_json::Map<String, Value>, keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|k| {
        obj.get(*k)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
    })
}

fn array_field<'a>(obj: &'a serde_json::Map<String, Value>, keys: &[&str]) -> Option<&'a [Value]> {
    keys.iter()
        .find_map(|k| obj.get(*k).and_then(Value::as_array).map(Vec::as_slice))
}

fn item_id(val: &Value) -> Option<String> {
    val.as_object().and_then(|o| {
        o.get("id").and_then(|v| {
            v.as_str()
                .map(str::to_string)
                .or_else(|| v.as_i64().map(|n| n.to_string()))
        })
    })
}

fn extract_kipu_patient_name(obj: &serde_json::Map<String, Value>) -> Option<String> {
    if let Some(full) = str_field(obj, &["full_name", "name", "display_name"]) {
        return Some(full.to_string());
    }
    let first = str_field(obj, &["first_name", "firstName"]).unwrap_or("");
    let last = str_field(obj, &["last_name", "lastName"]).unwrap_or("");
    let joined = format!("{first} {last}").trim().to_string();
    (!joined.is_empty()).then_some(joined)
}

fn stable_kipu_id(prefix: &str, kind: &str, item: &Value, idx: usize) -> String {
    match item_id(item) {
        Some(raw) => format!("{prefix}-kipu-{kind}-{}", sanitize_id_slug(&raw)),
        None => format!("{prefix}-kipu-{kind}-{idx}"),
    }
}

fn parse_kipu_evaluation(
    item: &Value,
    idx: usize,
    prefix: &str,
    source_label: &str,
    casefile_id: Option<&str>,
) -> Result<ChartRecord, String> {
    let obj = item
        .as_object()
        .ok_or_else(|| "Kipu evaluation entry must be a JSON object.".to_string())?;
    let title = str_field(obj, &["name", "title", "evaluation_name", "template_name"])
        .ok_or_else(|| "Kipu evaluation missing `name` or `title`.".to_string())?;
    let recorded_at = date_prefix(str_field(
        obj,
        &["completed_at", "updated_at", "created_at", "date"],
    ));
    let summary = str_field(obj, &["summary", "notes", "body", "result"])
        .unwrap_or("Behavioral health evaluation imported from KipuEMR.");
    let mut kvs = Vec::new();
    if let Some(score) = str_field(obj, &["score", "total_score"]) {
        kvs.push(("Score".into(), score.to_string()));
    }
    if let Some(status) = str_field(obj, &["status"]) {
        kvs.push(("Status".into(), status.to_string()));
    }
    if let Some(cf) = casefile_id {
        kvs.push(("Casefile ID".into(), cf.to_string()));
    }
    kvs.push(("Confidentiality".into(), "42 CFR Part 2 (SUD)".into()));

    Ok(ChartRecord {
        id: stable_kipu_id(prefix, "eval", item, idx),
        kind: RecordKind::ProgressNote,
        title: title.to_string(),
        recorded_at,
        source_label: source_label.to_string(),
        body: format!("{summary} ({PART2_REDISCLOSURE_NOTICE_SHORT})"),
        key_values: kvs,
        confidentiality: ConfidentialityClass::Part2Sud,
    })
}

fn parse_kipu_vital(
    item: &Value,
    idx: usize,
    prefix: &str,
    source_label: &str,
    casefile_id: Option<&str>,
) -> Result<ChartRecord, String> {
    let obj = item
        .as_object()
        .ok_or_else(|| "Kipu vital sign entry must be a JSON object.".to_string())?;
    let mut kvs = Vec::new();
    let mut parts = Vec::new();
    for (field, label) in [
        ("blood_pressure", "BP"),
        ("bp", "BP"),
        ("pulse", "Pulse"),
        ("heart_rate", "Heart Rate"),
        ("temperature", "Temp"),
        ("respirations", "RR"),
        ("spo2", "SpO2"),
        ("o2_saturation", "SpO2"),
        ("weight", "Weight"),
    ] {
        if let Some(v) = obj.get(field) {
            let val_str = if let Some(s) = v.as_str() {
                s.trim().to_string()
            } else if v.is_number() {
                v.to_string()
            } else {
                String::new()
            };
            if !val_str.is_empty() {
                kvs.push((label.to_string(), val_str.clone()));
                parts.push(format!("{label}: {val_str}"));
            }
        }
    }
    if parts.is_empty() {
        return Err("Kipu vital sign entry contains no recognized vital sign measurements.".into());
    }
    if let Some(cf) = casefile_id {
        kvs.push(("Casefile ID".into(), cf.to_string()));
    }
    let recorded_at = date_prefix(str_field(obj, &["recorded_at", "created_at", "date"]));
    Ok(ChartRecord {
        id: stable_kipu_id(prefix, "vital", item, idx),
        kind: RecordKind::LabResult,
        title: "KipuEMR Vital Signs".into(),
        recorded_at,
        source_label: source_label.to_string(),
        body: format!(
            "Recorded vitals: {}. ({PART2_REDISCLOSURE_NOTICE_SHORT})",
            parts.join(", ")
        ),
        key_values: kvs,
        confidentiality: ConfidentialityClass::Part2Sud,
    })
}

fn parse_kipu_allergy(
    item: &Value,
    idx: usize,
    prefix: &str,
    source_label: &str,
) -> Result<(AllergyEntry, ChartRecord), String> {
    let obj = item
        .as_object()
        .ok_or_else(|| "Kipu allergy entry must be a JSON object.".to_string())?;
    let substance = str_field(obj, &["substance", "allergen", "name"])
        .ok_or_else(|| "Kipu allergy entry missing `substance` / `allergen`.".to_string())?;
    let reaction = str_field(obj, &["reaction", "manifestation", "notes"])
        .unwrap_or("Documented allergy/intolerance");
    let severity = str_field(obj, &["severity", "criticality"]).unwrap_or("Unspecified");
    let recorded_at = date_prefix(str_field(obj, &["recorded_at", "created_at", "date"]));
    let id = stable_kipu_id(prefix, "allergy", item, idx);

    let entry = AllergyEntry {
        id: id.clone(),
        substance: substance.to_string(),
        reaction: reaction.to_string(),
        criticality: severity.to_string(),
        recorded_at: recorded_at.clone(),
        source_label: source_label.to_string(),
        confidentiality: ConfidentialityClass::Part2Sud,
    };
    let rec = ChartRecord {
        id,
        kind: RecordKind::ProgressNote,
        title: format!("Allergy / Intolerance: {substance}"),
        recorded_at,
        source_label: source_label.to_string(),
        body: format!(
            "Documented reaction: {reaction} (severity: {severity}). {PART2_REDISCLOSURE_NOTICE_SHORT}"
        ),
        key_values: vec![
            ("Allergy".into(), substance.to_string()),
            ("Severity".into(), severity.to_string()),
            ("Confidentiality".into(), "42 CFR Part 2 (SUD)".into()),
        ],
        confidentiality: ConfidentialityClass::Part2Sud,
    };
    Ok((entry, rec))
}

fn parse_kipu_medication(item: &Value) -> Result<Medication, String> {
    let obj = item
        .as_object()
        .ok_or_else(|| "Kipu medication/order entry must be a JSON object.".to_string())?;
    let name = str_field(obj, &["medication", "drug_name", "name", "order_name"])
        .ok_or_else(|| "Kipu medication order missing `medication` / `name`.".to_string())?;
    let dose = str_field(obj, &["dose", "dosage", "strength"]).unwrap_or("See Kipu order");
    let schedule = str_field(obj, &["schedule", "frequency", "sig", "status"]).unwrap_or("Active");
    let indication = str_field(obj, &["indication", "reason", "notes"])
        .unwrap_or("Behavioral health / SUD order (42 CFR Part 2)");
    Ok(Medication {
        name: name.to_string(),
        dose: dose.to_string(),
        schedule: schedule.to_string(),
        indication: indication.to_string(),
        confidentiality: ConfidentialityClass::Part2Sud,
    })
}

fn parse_kipu_diagnosis(item: &Value) -> Result<Problem, String> {
    let obj = item
        .as_object()
        .ok_or_else(|| "Kipu diagnosis entry must be a JSON object.".to_string())?;
    let label = str_field(obj, &["description", "label", "name"])
        .ok_or_else(|| "Kipu diagnosis missing `description` / `label`.".to_string())?;
    let code = str_field(obj, &["code", "icd10", "icd_10"]).unwrap_or("KIPU");
    let status = str_field(obj, &["status"]).unwrap_or("Active");
    Ok(Problem {
        code: code.to_string(),
        label: label.to_string(),
        status: status.to_string(),
        confidentiality: ConfidentialityClass::Part2Sud,
    })
}

fn parse_kipu_lab(
    item: &Value,
    idx: usize,
    prefix: &str,
    source_label: &str,
) -> Result<ChartRecord, String> {
    let obj = item
        .as_object()
        .ok_or_else(|| "Kipu lab entry must be a JSON object.".to_string())?;
    let name = str_field(obj, &["name", "test_name", "panel"])
        .ok_or_else(|| "Kipu lab entry missing `name`.".to_string())?;
    let val = str_field(obj, &["value", "result", "summary"])
        .ok_or_else(|| "Kipu lab entry missing `value` or `result`.".to_string())?;
    let notes = str_field(obj, &["notes", "comment"]).unwrap_or("");
    let recorded_at = date_prefix(str_field(
        obj,
        &["collected_at", "resulted_at", "recorded_at", "date"],
    ));
    let body = if notes.is_empty() {
        format!("{name}: {val}. ({PART2_REDISCLOSURE_NOTICE_SHORT})")
    } else {
        format!("{name}: {val}. {notes} ({PART2_REDISCLOSURE_NOTICE_SHORT})")
    };
    Ok(ChartRecord {
        id: stable_kipu_id(prefix, "lab", item, idx),
        kind: RecordKind::LabResult,
        title: name.to_string(),
        recorded_at,
        source_label: source_label.to_string(),
        body,
        key_values: vec![
            (name.to_string(), val.to_string()),
            ("Confidentiality".into(), "42 CFR Part 2 (SUD)".into()),
        ],
        confidentiality: ConfidentialityClass::Part2Sud,
    })
}

fn parse_kipu_appointment(
    item: &Value,
    idx: usize,
    prefix: &str,
    source_label: &str,
) -> Result<ChartRecord, String> {
    let obj = item
        .as_object()
        .ok_or_else(|| "Kipu appointment entry must be a JSON object.".to_string())?;
    let reason = str_field(obj, &["reason", "title", "appointment_type", "name"])
        .ok_or_else(|| "Kipu appointment missing `reason` / `title`.".to_string())?;
    let starts_at = date_prefix(str_field(obj, &["starts_at", "start_time", "date"]));
    let provider = str_field(obj, &["provider", "clinician"]).unwrap_or("Kipu Care Team");
    let status = str_field(obj, &["status"]).unwrap_or("Scheduled");
    Ok(ChartRecord {
        id: stable_kipu_id(prefix, "appt", item, idx),
        kind: RecordKind::ProgressNote,
        title: format!("Appointment: {reason}"),
        recorded_at: starts_at.clone(),
        source_label: source_label.to_string(),
        body: format!(
            "Scheduled with {provider} ({status}) on {starts_at}. ({PART2_REDISCLOSURE_NOTICE_SHORT})"
        ),
        key_values: vec![
            ("Provider".into(), provider.to_string()),
            ("Status".into(), status.to_string()),
            ("Confidentiality".into(), "42 CFR Part 2 (SUD)".into()),
        ],
        confidentiality: ConfidentialityClass::Part2Sud,
    })
}

fn parse_kipu_consent(
    item: &Value,
    idx: usize,
    prefix: &str,
    source_label: &str,
) -> Result<ChartRecord, String> {
    let obj = item
        .as_object()
        .ok_or_else(|| "Kipu consent entry must be a JSON object.".to_string())?;
    let title = str_field(obj, &["title", "name", "consent_type"])
        .ok_or_else(|| "Kipu consent entry missing `title` / `name`.".to_string())?;
    let status = str_field(obj, &["status"]).unwrap_or("Recorded");
    let signed_at = date_prefix(str_field(obj, &["signed_at", "date", "created_at"]));
    let expires_at = str_field(obj, &["expires_at", "expiration_date"]).unwrap_or("Unspecified");
    let scope = str_field(obj, &["scope", "description", "notes"])
        .unwrap_or("42 CFR Part 2 consent on file.");
    Ok(ChartRecord {
        id: stable_kipu_id(prefix, "consent", item, idx),
        kind: RecordKind::ProgressNote,
        title: title.to_string(),
        recorded_at: signed_at,
        source_label: source_label.to_string(),
        body: format!(
            "Consent status: {status} (expires: {expires_at}). Scope: {scope} ({PART2_REDISCLOSURE_NOTICE_SHORT})"
        ),
        key_values: vec![
            ("Status".into(), status.to_string()),
            ("Expires".into(), expires_at.to_string()),
            ("Confidentiality".into(), "42 CFR Part 2 (SUD)".into()),
        ],
        confidentiality: ConfidentialityClass::Part2Sud,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::find_patient;

    fn hex_lower(bytes: &[u8]) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut out = String::with_capacity(bytes.len() * 2);
        for &b in bytes {
            out.push(HEX[(b >> 4) as usize] as char);
            out.push(HEX[(b & 0x0f) as usize] as char);
        }
        out
    }

    #[test]
    fn sha1_hmac_sha1_hmac_sha256_and_md5_match_official_rfc_vectors() {
        // FIPS 180-4 SHA-1 vectors
        assert_eq!(
            hex_lower(&sha1_bytes(b"")),
            "da39a3ee5e6b4b0d3255bfef95601890afd80709"
        );
        assert_eq!(
            hex_lower(&sha1_bytes(b"abc")),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );

        // RFC 2202 HMAC-SHA1 Test Case 2: key = "Jefe", data = "what do ya want for nothing?"
        assert_eq!(
            hex_lower(&hmac_sha1(b"Jefe", b"what do ya want for nothing?")),
            "effcdf6ae5eb2fa2d27416d5f184df9c259a7c79"
        );

        // RFC 4231 HMAC-SHA256 Test Case 2: key = "Jefe", data = "what do ya want for nothing?"
        assert_eq!(
            hex_lower(&hmac_sha256(b"Jefe", b"what do ya want for nothing?")),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );

        // RFC 1321 MD5 test vectors
        assert_eq!(
            hex_lower(&md5_bytes(b"")),
            "d41d8cd98f00b204e9800998ecf8427e"
        );
        assert_eq!(
            hex_lower(&md5_bytes(b"abc")),
            "900150983cd24fb0d6963f7d28e17f72"
        );
        assert_eq!(
            hex_lower(&md5_bytes(b"message digest")),
            "f96b697d7cb7938d525a2f31aaf161d0"
        );
    }

    #[test]
    fn kipu_v3_request_signing_builds_exact_canonical_string_and_headers() {
        // Bodyless GET canonical string: ",,/api/patients/census?app_id=demo,Wed, 06 Nov 2019 15:38:34 GMT"
        let get_req = sign_kipu_request(
            KIPU_DEFAULT_BASE_URL,
            "GET",
            "/api/patients/census?app_id=demo&phi_level=low",
            "Wed, 06 Nov 2019 15:38:34 GMT",
            None,
            None,
            KipuAuthScheme::ApiAuthHmacSha1,
            "access-123",
            "secret-xyz",
            false,
        )
        .expect("signed GET");
        assert_eq!(
            get_req.canonical_string,
            ",,/api/patients/census?app_id=demo&phi_level=low,Wed, 06 Nov 2019 15:38:34 GMT"
        );
        assert_eq!(get_req.accept, KIPU_API_V3_ACCEPT);
        assert!(get_req.authorization.starts_with("APIAuth access-123:"));
        assert_eq!(
            get_req.redacted_authorization(),
            "APIAuth access-123:[REDACTED]"
        );

        // POST with JSON body computes Content-MD5 and APIAuth-HMAC-SHA256
        let body = br#"{"status":"active"}"#;
        let post_req = sign_kipu_request(
            KIPU_DEFAULT_BASE_URL,
            "POST",
            "/api/v3/vital_signs",
            "Wed, 26 Nov 2026 04:30:00 GMT",
            Some("application/json"),
            Some(body),
            KipuAuthScheme::ApiAuthHmacSha256,
            "access-123",
            "secret-xyz",
            false,
        )
        .expect("signed POST");
        assert!(!post_req.content_md5.is_empty());
        assert!(post_req.canonical_string.starts_with(&format!(
            "application/json,{},/api/v3/vital_signs,",
            post_req.content_md5
        )));
        assert!(post_req
            .authorization
            .starts_with("APIAuth-HMAC-SHA256 access-123:"));

        // Rejects non-HTTPS / private hosts when allow_loopback_http = false
        assert!(sign_kipu_request(
            "http://127.0.0.1:8080",
            "GET",
            "/api/patients/census",
            "Wed, 26 Nov 2026 04:30:00 GMT",
            None,
            None,
            KipuAuthScheme::ApiAuthHmacSha1,
            "a",
            "b",
            false,
        )
        .is_err());
    }

    #[test]
    fn kipu_payload_imports_all_domains_with_part2_confidentiality_and_tolerates_partial_errors() {
        let nadia = find_patient("pt-nadia-kulkarni").expect("nadia");
        let raw = sandbox_kipu_payload_for_patient(&nadia);
        let mut parsed: Value = serde_json::from_str(&raw).unwrap();
        parsed["vital_signs"]
            .as_array_mut()
            .unwrap()
            .push(json!({ "id": "bad-vital" }));

        let import = ingest_kipu_payload(
            "Harborview Behavioral (KipuEMR)",
            "org-kipu-sandbox",
            &serde_json::to_string(&parsed).unwrap(),
        )
        .expect("kipu import succeeds");

        assert_eq!(import.confidentiality, ConfidentialityClass::Part2Sud);
        assert_eq!(import.patient_name.as_deref(), Some("Nadia Kulkarni"));
        assert_eq!(import.medications.len(), 1);
        assert_eq!(import.problems.len(), 1);
        assert_eq!(import.allergies.len(), 1);
        // census + eval + vital + allergy record + lab + appointment + consent = 7
        assert_eq!(import.records.len(), 7);
        assert_eq!(import.warnings.len(), 1);
        assert!(import
            .records
            .iter()
            .all(|r| r.confidentiality == ConfidentialityClass::Part2Sud));
        assert!(import
            .medications
            .iter()
            .all(|m| m.confidentiality == ConfidentialityClass::Part2Sud));
        assert!(import
            .problems
            .iter()
            .all(|p| p.confidentiality == ConfidentialityClass::Part2Sud));
        assert!(import
            .allergies
            .iter()
            .all(|a| a.confidentiality == ConfidentialityClass::Part2Sud));
    }
}
