//! Generic declarative execution engine for state facility and provider licensing endpoints,
//! local bulk file imports (`--import-file`), CAPTCHA fail-closed detection, and strict
//! clinical credential classification.
//!
//! Supported live formats:
//! - Socrata SODA JSON API (facilities and providers)
//! - ArcGIS FeatureServer / MapServer `/query` JSON layers
//! - CKAN DataStore `datastore_search` JSON API
//! - Direct public CSV downloads (facilities and providers)
//!
//! All network responses are cached on disk under `.cache/recovery-index/state-licensing/`
//! with a 14-day TTL and conditional ETag / Last-Modified revalidation. Any CAPTCHA or
//! anti-bot challenge fails closed with [`StateLicenseError::CaptchaBlocked`].

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::Duration;

use serde_json::{Map, Value};

use crate::schema::{percent_encode_query, LicenseStatus, StaffLicense};
use crate::updater::cache::{CacheMode, DiskCache};

use super::types::{
    FacilityFieldMap, FacilityLicenseRecord, GenericEndpointSpec, LicensingScope, ProviderFieldMap,
    ProviderLicenseRecord, StateAdapterOutcome, StateLicenseError,
};

/// Disk cache namespace for all state licensing HTTP responses.
pub const STATE_LICENSING_CACHE_NAMESPACE: &str = "state-licensing";

/// Default disk cache TTL for state licensing responses (14 days).
pub const STATE_LICENSING_TTL: Duration = Duration::from_secs(14 * 24 * 60 * 60);

const DEFAULT_PAGE_SIZE: usize = 1_000;
const DEFAULT_MAX_RECORDS: usize = 50_000;

/// Performs a cached HTTP GET returning `(body, from_cache)` using the `"state-licensing"`
/// namespace and 14-day TTL.
///
/// Fails closed with [`StateLicenseError::CaptchaBlocked`] whenever a CAPTCHA or anti-bot
/// challenge is detected (never caches challenge HTML and never attempts to bypass it).
/// Falls back to a stale cached copy on transient network or 5xx/429 upstream errors.
pub async fn fetch_cached_text(
    client: &reqwest::Client,
    cache: &DiskCache,
    url: &str,
) -> Result<(String, bool), StateLicenseError> {
    if let Some(hit) =
        cache.get::<String>(STATE_LICENSING_CACHE_NAMESPACE, url, STATE_LICENSING_TTL)
    {
        if detect_captcha_or_block(200, &hit.value) {
            return Err(StateLicenseError::CaptchaBlocked {
                url: url.to_string(),
            });
        }
        return Ok((hit.value, true));
    }

    if cache.mode() == CacheMode::Offline {
        return Err(StateLicenseError::InvalidConfig(format!(
            "offline mode: no cached state licensing response for {url}"
        )));
    }

    let stale = cache.get_stale::<String>(STATE_LICENSING_CACHE_NAMESPACE, url);

    const MAX_ATTEMPTS: usize = 4;
    const RETRY_BACKOFF_MS: [u64; 3] = [1_000, 2_500, 5_000];

    let mut last_err: Option<StateLicenseError> = None;

    for attempt in 0..MAX_ATTEMPTS {
        if attempt > 0 {
            let delay_ms = RETRY_BACKOFF_MS[(attempt - 1).min(RETRY_BACKOFF_MS.len() - 1)];
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
        }

        let mut req = client.get(url);
        if let Some(ref env) = stale {
            if let Some(ref etag) = env.etag {
                req = req.header(reqwest::header::IF_NONE_MATCH, etag.as_str());
            }
            if let Some(ref last_mod) = env.last_modified {
                req = req.header(reqwest::header::IF_MODIFIED_SINCE, last_mod.as_str());
            }
        }

        let resp = match req.send().await {
            Ok(r) => r,
            Err(source) => {
                last_err = Some(StateLicenseError::Http {
                    url: url.to_string(),
                    source,
                });
                continue;
            }
        };

        let status = resp.status().as_u16();
        if status == 304 {
            if let Some(env) = stale {
                let _ = cache.put(
                    STATE_LICENSING_CACHE_NAMESPACE,
                    url,
                    &env.value,
                    env.etag.clone(),
                    env.last_modified.clone(),
                );
                return Ok((env.value, true));
            }
        }

        let etag = resp
            .headers()
            .get(reqwest::header::ETAG)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let last_modified = resp
            .headers()
            .get(reqwest::header::LAST_MODIFIED)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);

        let body = match resp.text().await {
            Ok(text) => text,
            Err(source) => {
                last_err = Some(StateLicenseError::Http {
                    url: url.to_string(),
                    source,
                });
                continue;
            }
        };

        if detect_captcha_or_block(status, &body) {
            return Err(StateLicenseError::CaptchaBlocked {
                url: url.to_string(),
            });
        }

        if !(200..300).contains(&status) {
            let retryable = matches!(status, 429 | 500 | 502 | 503 | 504);
            last_err = Some(StateLicenseError::HttpStatus {
                url: url.to_string(),
                status,
            });
            if retryable && attempt + 1 < MAX_ATTEMPTS {
                continue;
            }
            if retryable {
                if let Some(env) = stale {
                    if !detect_captcha_or_block(200, &env.value) {
                        return Ok((env.value, true));
                    }
                }
            }
            return Err(last_err.expect("last_err set"));
        }

        let _ = cache.put(
            STATE_LICENSING_CACHE_NAMESPACE,
            url,
            &body,
            etag,
            last_modified,
        );
        return Ok((body, false));
    }

    if let Some(env) = stale {
        if !detect_captcha_or_block(200, &env.value) {
            return Ok((env.value, true));
        }
    }
    Err(last_err.unwrap_or_else(|| StateLicenseError::HttpStatus {
        url: url.to_string(),
        status: 503,
    }))
}

/// Detects CAPTCHA, Turnstile, hCaptcha, reCAPTCHA, Incapsula, PerimeterX, or similar
/// anti-bot HTML challenge pages so adapters fail closed instead of parsing error HTML.
pub fn detect_captcha_or_block(status: u16, body: &str) -> bool {
    let lower = body.to_ascii_lowercase();

    // Unambiguous anti-bot script or widget signatures regardless of HTTP status.
    const HARD_SIGNATURES: &[&str] = &[
        "cf-turnstile",
        "challenges.cloudflare.com",
        "__cf_chl_",
        "cf-browser-verification",
        "attention required! | cloudflare",
        "g-recaptcha",
        "www.google.com/recaptcha",
        "recaptcha/api.js",
        "h-captcha",
        "hcaptcha.com/1/api.js",
        "data-hcaptcha",
        "_incapsula_resource",
        "incapsula incident id",
        "request unsuccessful. incapsula",
        "px-captcha",
        "datadome.co/captcha",
        "arkoselabs",
        "funcaptcha",
    ];
    if HARD_SIGNATURES.iter().any(|sig| lower.contains(sig)) {
        return true;
    }

    let trimmed = lower.trim_start();
    let looks_like_html = trimmed.starts_with("<!doctype")
        || trimmed.starts_with("<html")
        || lower.contains("<html")
        || lower.contains("<title")
        || lower.contains("<script");

    if looks_like_html {
        const HTML_CHALLENGE_PHRASES: &[&str] = &[
            "verify you are human",
            "prove you are human",
            "are you a robot",
            "are you human",
            "complete the captcha",
            "please solve the captcha",
            "bot protection",
            "checking your browser before accessing",
            "enable javascript and cookies to continue",
        ];
        if HTML_CHALLENGE_PHRASES.iter().any(|p| lower.contains(p)) {
            return true;
        }
        if lower.contains("just a moment") && lower.contains("cloudflare") {
            return true;
        }
        if matches!(status, 403 | 429 | 503)
            && (lower.contains("captcha")
                || lower.contains("cloudflare")
                || lower.contains("incapsula")
                || lower.contains("perimeterx")
                || lower.contains("access denied"))
        {
            return true;
        }
    }

    false
}

/// Builds an official public state verification URL from an optional template (supporting
/// `{license}`, `{license_number}`, or `{id}` placeholders) or falls back to `portal_fallback`.
pub fn build_verification_url(
    template: Option<&str>,
    license_number: &str,
    portal_fallback: &str,
) -> Option<String> {
    let clean_num = license_number.trim();
    if let Some(tmpl) = template.map(str::trim).filter(|s| !s.is_empty()) {
        let has_placeholder = tmpl.contains("{license}")
            || tmpl.contains("{license_number}")
            || tmpl.contains("{id}");
        if has_placeholder {
            if !clean_num.is_empty() {
                let encoded = percent_encode_query(clean_num);
                let url = tmpl
                    .replace("{license_number}", &encoded)
                    .replace("{license}", &encoded)
                    .replace("{id}", &encoded);
                if is_http_url(&url) {
                    return Some(url);
                }
            }
        } else if is_http_url(tmpl) {
            return Some(tmpl.to_string());
        }
    }

    let fallback = portal_fallback.trim();
    if is_http_url(fallback) {
        Some(fallback.to_string())
    } else {
        None
    }
}

/// Executes any declarative [`GenericEndpointSpec`] variant (Socrata, ArcGIS, CKAN, or CSV)
/// with disk caching, pagination, and CAPTCHA fail-closed handling.
pub async fn execute_generic_endpoint(
    client: &reqwest::Client,
    cache: &DiskCache,
    state: &str,
    system_id: &str,
    spec: &GenericEndpointSpec,
) -> Result<StateAdapterOutcome, StateLicenseError> {
    let state_upper = state.trim().to_ascii_uppercase();
    let mut outcome = StateAdapterOutcome {
        state: state_upper.clone(),
        systems_executed: vec![system_id.to_string()],
        ..StateAdapterOutcome::default()
    };

    match spec {
        GenericEndpointSpec::SocrataFacility {
            agency,
            endpoint_url,
            where_clause,
            page_size,
            max_records,
            verification_url_template,
            fields,
        } => {
            let page_sz = effective_page_size(*page_size);
            let cap = effective_max_records(*max_records);
            let mut offset = 0usize;
            let mut total_seen = 0usize;

            while total_seen < cap {
                let limit = page_sz.min(cap - total_seen);
                let mut params = vec![
                    ("$limit", limit.to_string()),
                    ("$offset", offset.to_string()),
                ];
                if let Some(where_str) = where_clause.map(str::trim).filter(|s| !s.is_empty()) {
                    params.push(("$where", where_str.to_string()));
                }
                let url = build_url_with_params(endpoint_url, &params)?;
                let (body, from_cache) = fetch_cached_text(client, cache, url.as_str()).await?;
                record_fetch_stat(&mut outcome, from_cache);

                let (records, raw_count) = parse_socrata_facility_page(
                    &body,
                    &state_upper,
                    agency,
                    *verification_url_template,
                    fields,
                    system_id,
                )?;
                outcome.facility_records.extend(records);
                total_seen += raw_count;
                offset += raw_count;

                if raw_count < limit {
                    break;
                }
            }
        }
        GenericEndpointSpec::SocrataProvider {
            board,
            endpoint_url,
            where_clause,
            page_size,
            max_records,
            verification_url_template,
            fields,
        } => {
            let page_sz = effective_page_size(*page_size);
            let cap = effective_max_records(*max_records);
            let mut offset = 0usize;
            let mut total_seen = 0usize;

            while total_seen < cap {
                let limit = page_sz.min(cap - total_seen);
                let mut params = vec![
                    ("$limit", limit.to_string()),
                    ("$offset", offset.to_string()),
                ];
                if let Some(where_str) = where_clause.map(str::trim).filter(|s| !s.is_empty()) {
                    params.push(("$where", where_str.to_string()));
                }
                let url = build_url_with_params(endpoint_url, &params)?;
                let (body, from_cache) = fetch_cached_text(client, cache, url.as_str()).await?;
                record_fetch_stat(&mut outcome, from_cache);

                let (records, raw_count) = parse_socrata_provider_page(
                    &body,
                    &state_upper,
                    board,
                    *verification_url_template,
                    fields,
                    system_id,
                )?;
                outcome.provider_records.extend(records);
                total_seen += raw_count;
                offset += raw_count;

                if raw_count < limit {
                    break;
                }
            }
        }
        GenericEndpointSpec::ArcGisFacility {
            agency,
            layer_query_url,
            where_clause,
            page_size,
            max_records,
            verification_url_template,
            fields,
        } => {
            let page_sz = effective_page_size(*page_size);
            let cap = effective_max_records(*max_records);
            let where_expr = if where_clause.trim().is_empty() {
                "1=1"
            } else {
                where_clause.trim()
            };
            let mut offset = 0usize;
            let mut total_seen = 0usize;

            while total_seen < cap {
                let limit = page_sz.min(cap - total_seen);
                let params = vec![
                    ("where", where_expr.to_string()),
                    ("outFields", "*".to_string()),
                    ("returnGeometry", "false".to_string()),
                    ("f", "json".to_string()),
                    ("resultOffset", offset.to_string()),
                    ("resultRecordCount", limit.to_string()),
                ];
                let url = build_url_with_params(layer_query_url, &params)?;
                let (body, from_cache) = fetch_cached_text(client, cache, url.as_str()).await?;
                record_fetch_stat(&mut outcome, from_cache);

                let page_res = parse_arcgis_facility_page(
                    &body,
                    &state_upper,
                    agency,
                    *verification_url_template,
                    fields,
                    system_id,
                );
                let (records, raw_count, exceeded_limit, unpaginated_fallback) = match page_res {
                    Ok((r, c, e)) => (r, c, e, false),
                    Err(StateLicenseError::InvalidConfig(ref msg))
                        if offset == 0 && msg.contains("ArcGIS error") =>
                    {
                        // Older ArcGIS MapServer layers with supportsPagination=false reject
                        // resultOffset/resultRecordCount; retry once without pagination params.
                        let fallback_params = vec![
                            ("where", where_expr.to_string()),
                            ("outFields", "*".to_string()),
                            ("returnGeometry", "false".to_string()),
                            ("f", "json".to_string()),
                        ];
                        let fallback_url =
                            build_url_with_params(layer_query_url, &fallback_params)?;
                        let (fb_body, fb_cached) =
                            fetch_cached_text(client, cache, fallback_url.as_str()).await?;
                        record_fetch_stat(&mut outcome, fb_cached);
                        let (r, c, e) = parse_arcgis_facility_page(
                            &fb_body,
                            &state_upper,
                            agency,
                            *verification_url_template,
                            fields,
                            system_id,
                        )?;
                        (r, c, e, true)
                    }
                    Err(err) => return Err(err),
                };
                outcome.facility_records.extend(records);
                total_seen += raw_count;
                offset += raw_count;

                if unpaginated_fallback || raw_count == 0 || (raw_count < limit && !exceeded_limit)
                {
                    break;
                }
            }
        }
        GenericEndpointSpec::CkanFacility {
            agency,
            datastore_search_url,
            resource_id,
            page_size,
            max_records,
            verification_url_template,
            fields,
        } => {
            let page_sz = effective_page_size(*page_size);
            let cap = effective_max_records(*max_records);
            let mut offset = 0usize;
            let mut total_seen = 0usize;

            while total_seen < cap {
                let limit = page_sz.min(cap - total_seen);
                let params = vec![
                    ("resource_id", (*resource_id).to_string()),
                    ("limit", limit.to_string()),
                    ("offset", offset.to_string()),
                ];
                let url = build_url_with_params(datastore_search_url, &params)?;
                let (body, from_cache) = fetch_cached_text(client, cache, url.as_str()).await?;
                record_fetch_stat(&mut outcome, from_cache);

                let (records, raw_count, total_available) = parse_ckan_facility_page(
                    &body,
                    &state_upper,
                    agency,
                    *verification_url_template,
                    fields,
                    system_id,
                )?;
                outcome.facility_records.extend(records);
                total_seen += raw_count;
                offset += raw_count;

                if raw_count == 0 || raw_count < limit {
                    break;
                }
                if let Some(total) = total_available {
                    if offset >= total {
                        break;
                    }
                }
            }
        }
        GenericEndpointSpec::CsvFacility {
            agency,
            csv_url,
            filter_keywords,
            verification_url_template,
            fields,
        } => {
            let (body, from_cache) = fetch_cached_text(client, cache, csv_url).await?;
            record_fetch_stat(&mut outcome, from_cache);

            let records = parse_csv_facilities(
                &body,
                &state_upper,
                agency,
                filter_keywords,
                *verification_url_template,
                fields,
                system_id,
            )?;
            outcome.facility_records.extend(records);
        }
        GenericEndpointSpec::CsvProvider {
            board,
            csv_url,
            verification_url_template,
            fields,
        } => {
            let (body, from_cache) = fetch_cached_text(client, cache, csv_url).await?;
            record_fetch_stat(&mut outcome, from_cache);

            let records = parse_csv_providers(
                &body,
                &state_upper,
                board,
                *verification_url_template,
                fields,
                system_id,
            )?;
            outcome.provider_records.extend(records);
        }
    }

    Ok(outcome)
}

/// Imports a local CSV, TSV, or JSON bulk download (`--import-file`) with resilient header
/// matching so operators can ingest official exports from CAPTCHA-gated state portals.
pub fn import_local_file(
    path: &Path,
    state: &str,
    agency_or_board: &str,
    scope: LicensingScope,
) -> Result<StateAdapterOutcome, StateLicenseError> {
    let raw = fs::read_to_string(path).map_err(|source| StateLicenseError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let source_label = path.display().to_string();
    let state_upper = state.trim().to_ascii_uppercase();
    let file_tag = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("import");

    let mut outcome = StateAdapterOutcome {
        state: state_upper.clone(),
        systems_executed: vec![format!("local-import:{file_tag}")],
        ..StateAdapterOutcome::default()
    };

    let ext_is_json = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "json" | "geojson"));
    let trimmed = raw.trim_start();
    let looks_json = ext_is_json || trimmed.starts_with('{') || trimmed.starts_with('[');

    if looks_json {
        let rows = parse_json_export_rows(&raw, &source_label)?;
        match scope {
            LicensingScope::Facility => {
                for row in &rows {
                    if let Some(rec) = facility_from_json_map(
                        row,
                        &state_upper,
                        agency_or_board,
                        None,
                        &DEFAULT_FACILITY_FIELDS,
                    ) {
                        outcome.facility_records.push(rec);
                    }
                }
            }
            LicensingScope::Provider => {
                for row in &rows {
                    if let Some(rec) = provider_from_json_map(
                        row,
                        &state_upper,
                        agency_or_board,
                        None,
                        &DEFAULT_PROVIDER_FIELDS,
                    ) {
                        outcome.provider_records.push(rec);
                    }
                }
            }
            LicensingScope::Both => {
                for row in &rows {
                    let has_provider_hint =
                        lookup_json_str(row, DEFAULT_PROVIDER_FIELDS.first_name).is_some()
                            || lookup_json_str(row, DEFAULT_PROVIDER_FIELDS.last_name).is_some()
                            || lookup_json_str(row, DEFAULT_PROVIDER_FIELDS.credential_or_type)
                                .is_some();
                    let has_facility_hint = lookup_json_str(row, DEFAULT_FACILITY_FIELDS.name)
                        .is_some()
                        || lookup_json_str(row, DEFAULT_FACILITY_FIELDS.licensed_beds).is_some();

                    if has_facility_hint {
                        if let Some(rec) = facility_from_json_map(
                            row,
                            &state_upper,
                            agency_or_board,
                            None,
                            &DEFAULT_FACILITY_FIELDS,
                        ) {
                            outcome.facility_records.push(rec);
                        }
                    }
                    if has_provider_hint || !has_facility_hint {
                        if let Some(rec) = provider_from_json_map(
                            row,
                            &state_upper,
                            agency_or_board,
                            None,
                            &DEFAULT_PROVIDER_FIELDS,
                        ) {
                            outcome.provider_records.push(rec);
                        }
                    }
                }
            }
        }
    } else {
        match scope {
            LicensingScope::Facility => {
                outcome.facility_records = parse_csv_facilities(
                    &raw,
                    &state_upper,
                    agency_or_board,
                    &[],
                    None,
                    &DEFAULT_FACILITY_FIELDS,
                    &source_label,
                )?;
            }
            LicensingScope::Provider => {
                outcome.provider_records = parse_csv_providers(
                    &raw,
                    &state_upper,
                    agency_or_board,
                    None,
                    &DEFAULT_PROVIDER_FIELDS,
                    &source_label,
                )?;
            }
            LicensingScope::Both => {
                let first_line = raw.lines().next().unwrap_or_default().to_ascii_lowercase();
                let looks_provider_csv = first_line.contains("first_name")
                    || first_line.contains("first name")
                    || first_line.contains("last_name")
                    || first_line.contains("last name")
                    || first_line.contains("credential")
                    || first_line.contains("practitioner")
                    || first_line.contains("provider_name");
                let looks_facility_csv = first_line.contains("facility")
                    || first_line.contains("bed")
                    || first_line.contains("dba")
                    || first_line.contains("program_name")
                    || first_line.contains("agency_name");

                if looks_facility_csv || !looks_provider_csv {
                    outcome.facility_records = parse_csv_facilities(
                        &raw,
                        &state_upper,
                        agency_or_board,
                        &[],
                        None,
                        &DEFAULT_FACILITY_FIELDS,
                        &source_label,
                    )?;
                }
                if looks_provider_csv {
                    outcome.provider_records = parse_csv_providers(
                        &raw,
                        &state_upper,
                        agency_or_board,
                        None,
                        &DEFAULT_PROVIDER_FIELDS,
                        &source_label,
                    )?;
                }
            }
        }
    }

    Ok(outcome)
}

/// Strictly classifies a state licensing board name + credential/profession label into a
/// non-conflated [`StaffLicense`].
///
/// Key invariants:
/// - MD is never conflated with DO, and ambiguous physician labels with no explicit MD/DO
///   degree and no single-degree Medical/Osteopathic board return `None`.
/// - Osteopathic boards imply DO for physician labels; dedicated Medical (allopathic) boards
///   imply MD for physician labels; combined Medical & Osteopathic boards remain ambiguous.
/// - Physician Assistants (`Pa`) and Nurse Practitioners (`Pmhnp` / `Np`) never classify as
///   Physicians or Psychiatrists, even when regulated under a state Medical Board.
/// - Doctoral Psychologists strictly separate `PhdPsychologist` vs `PsydPsychologist` and
///   return `None` when the degree is not explicit.
/// - `Lcsw`, `LpcLmhc`, and `Lmft` remain distinct; `Rn` and `Lpn` remain distinct.
pub fn classify_state_provider_license(
    board: &str,
    credential_label: &str,
) -> Option<StaffLicense> {
    let board_lower = board.trim().to_ascii_lowercase();
    let cred_lower = credential_label.trim().to_ascii_lowercase();
    if board_lower.is_empty() && cred_lower.is_empty() {
        return None;
    }

    let tokens = credential_tokens(credential_label);
    let has_tok = |target: &str| tokens.iter().any(|t| t == target);

    // 1. Advanced Practice Providers first so a PA under a Medical Board or an NP under a
    //    combined Healing Arts board is never misclassified as an MD/DO physician.
    if cred_lower.contains("physician assistant")
        || cred_lower.contains("physician associate")
        || has_tok("PA")
        || has_tok("PAC")
        || has_tok("RPA")
        || has_tok("MPAS")
        || (board_lower.contains("physician assistant") && !cred_lower.contains("nurse"))
    {
        return Some(StaffLicense::Pa);
    }

    let is_np_signal = cred_lower.contains("nurse practitioner")
        || cred_lower.contains("advanced practice registered nurse")
        || cred_lower.contains("advanced practice nurse")
        || has_tok("PMHNP")
        || has_tok("PMHNPBC")
        || has_tok("NPP")
        || has_tok("NP")
        || has_tok("APRN")
        || has_tok("ARNP")
        || has_tok("CRNP")
        || has_tok("FNP")
        || has_tok("FNPC")
        || has_tok("AGNP")
        || has_tok("CNP")
        || has_tok("DNP");
    let is_psych_mh_phrase = cred_lower.contains("psychiatr")
        || cred_lower.contains("mental health")
        || cred_lower.contains("behavioral health");

    if has_tok("PMHNP")
        || has_tok("PMHNPBC")
        || has_tok("NPP")
        || (is_np_signal && is_psych_mh_phrase)
    {
        return Some(StaffLicense::Pmhnp);
    }
    if is_np_signal {
        return Some(StaffLicense::Np);
    }

    // 2. Doctoral Psychologists: strictly separate PhD vs PsyD; never guess when degree is absent.
    if has_tok("PSYD") || cred_lower.contains("doctor of psychology") {
        return Some(StaffLicense::PsydPsychologist);
    }
    if has_tok("PHD") && (cred_lower.contains("psycholog") || board_lower.contains("psycholog")) {
        return Some(StaffLicense::PhdPsychologist);
    }

    // 3. Master's Therapists & Clinical Social Workers (strictly distinct; exclude pre-licensed
    // associates/interns such as ASW, AMFT, APCC, and Associate Clinical Social Workers).
    let is_prelicensed_associate = has_tok("ASW")
        || has_tok("AMFT")
        || has_tok("APCC")
        || has_tok("ACSW")
        || has_tok("LPC-A")
        || has_tok("LPCA")
        || cred_lower.contains("associate ")
        || cred_lower.contains("intern")
        || cred_lower.contains("trainee")
        || cred_lower.contains("resident in counseling");

    if !is_prelicensed_associate {
        if has_tok("LCSW")
            || has_tok("LICSW")
            || has_tok("LISW")
            || has_tok("LCSWC")
            || has_tok("LCSWR")
            || has_tok("LSCSW")
            || has_tok("CISW")
            || cred_lower.contains("clinical social worker")
        {
            return Some(StaffLicense::Lcsw);
        }
        if has_tok("LMFT")
            || has_tok("MFT")
            || has_tok("LCMFT")
            || cred_lower.contains("marriage and family therapist")
            || cred_lower.contains("marriage & family therapist")
        {
            return Some(StaffLicense::Lmft);
        }
        if has_tok("LPC")
            || has_tok("LPCC")
            || has_tok("LMHC")
            || has_tok("LCPC")
            || has_tok("LCMHC")
            || has_tok("LPCMH")
            || has_tok("MHSP")
            || cred_lower.contains("professional counselor")
            || cred_lower.contains("mental health counselor")
        {
            return Some(StaffLicense::LpcLmhc);
        }
    }

    // 4. Addiction Counselors, Peer Support, and Nursing (RN vs LPN distinct).
    if has_tok("LADC")
        || has_tok("CADC")
        || has_tok("LCDC")
        || has_tok("CASAC")
        || has_tok("LAC")
        || has_tok("CAC")
        || has_tok("LASAC")
        || has_tok("LISAC")
        || has_tok("SUDP")
        || has_tok("CDCA")
        || has_tok("LICDC")
        || cred_lower.contains("addiction counselor")
        || cred_lower.contains("alcohol and drug counselor")
        || cred_lower.contains("chemical dependency counselor")
        || cred_lower.contains("substance abuse counselor")
        || cred_lower.contains("substance use disorder professional")
    {
        return Some(StaffLicense::AddictionCounselor);
    }
    if has_tok("CPRS")
        || has_tok("CPSS")
        || has_tok("CRPS")
        || has_tok("CRPA")
        || has_tok("PSS")
        || cred_lower.contains("peer support")
        || cred_lower.contains("peer recovery")
        || cred_lower.contains("peer specialist")
    {
        return Some(StaffLicense::PeerSupport);
    }
    if has_tok("LPN")
        || has_tok("LVN")
        || cred_lower.contains("licensed practical nurse")
        || cred_lower.contains("licensed vocational nurse")
        || cred_lower.contains("practical nurse")
        || cred_lower.contains("vocational nurse")
    {
        return Some(StaffLicense::Lpn);
    }
    if has_tok("RN") || has_tok("BSN") || cred_lower.contains("registered nurse") {
        return Some(StaffLicense::Rn);
    }

    // 5. Physicians: strictly separate MD vs DO; never guess when degree is ambiguous.
    let cred_is_md = has_tok("MD")
        || has_tok("MBBS")
        || cred_lower.contains("doctor of medicine")
        || cred_lower.contains("medical doctor")
        || cred_lower.contains("allopathic");
    let cred_is_do = has_tok("DO")
        || cred_lower.contains("osteopath")
        || cred_lower.contains("doctor of osteopathy")
        || cred_lower.contains("doctor of osteopathic");

    if cred_is_md && cred_is_do {
        return None;
    }

    let is_psych_specialty = cred_lower.contains("psychiatr");
    let is_addiction_specialty =
        cred_lower.contains("addiction medicine") || cred_lower.contains("addictionologist");

    if cred_is_md {
        return Some(if is_psych_specialty {
            StaffLicense::MdPsychiatrist
        } else if is_addiction_specialty {
            StaffLicense::MdAddiction
        } else {
            StaffLicense::MdPhysician
        });
    }
    if cred_is_do {
        return Some(if is_psych_specialty {
            StaffLicense::DoPsychiatrist
        } else if is_addiction_specialty {
            StaffLicense::DoAddiction
        } else {
            StaffLicense::DoPhysician
        });
    }

    // Board-level MD/DO resolution only when the credential clearly describes a physician
    // and the board is unambiguously single-degree (not combined Medical & Osteopathic).
    let cred_indicates_physician = cred_lower.contains("physician")
        || cred_lower.contains("surgeon")
        || is_psych_specialty
        || is_addiction_specialty
        || cred_lower.contains("medical license");
    if !cred_indicates_physician {
        return None;
    }

    let board_mentions_osteo = board_lower.contains("osteopath");
    let board_is_explicit_allopathic_only = board_lower.contains("allopathic")
        || board_lower.contains("medical board of california")
        || board_lower.contains("arizona medical board")
        || board_lower.contains("florida board of medicine")
        || board_lower.contains("michigan board of medicine")
        || board_lower.contains("pennsylvania state board of medicine")
        || board_lower.contains("maine board of licensure in medicine")
        || board_lower.contains("vermont board of medical practice")
        || board_lower.contains("west virginia board of medicine")
        || board_lower.contains("nevada state board of medical examiners")
        || board_lower.contains("oklahoma state board of medical licensure")
        || board_lower.contains("utah physicians and surgeons")
        || board_lower.contains("washington medical commission")
        || board_lower.contains("tennessee board of medical examiners");
    let board_is_combined = (board_mentions_osteo && board_is_explicit_allopathic_only)
        || board_lower.contains("healing arts")
        || board_lower.contains("medical and osteopathic")
        || board_lower.contains("medical & osteopathic");

    if board_is_combined {
        return None;
    }

    if board_mentions_osteo {
        return Some(if is_psych_specialty {
            StaffLicense::DoPsychiatrist
        } else if is_addiction_specialty {
            StaffLicense::DoAddiction
        } else {
            StaffLicense::DoPhysician
        });
    }
    if board_is_explicit_allopathic_only {
        return Some(if is_psych_specialty {
            StaffLicense::MdPsychiatrist
        } else if is_addiction_specialty {
            StaffLicense::MdAddiction
        } else {
            StaffLicense::MdPhysician
        });
    }

    None
}

// ---------------------------------------------------------------------------
// Internal parsing & normalization helpers
// ---------------------------------------------------------------------------

fn effective_page_size(page_size: usize) -> usize {
    if page_size == 0 {
        DEFAULT_PAGE_SIZE
    } else {
        page_size
    }
}

fn effective_max_records(max_records: usize) -> usize {
    if max_records == 0 {
        DEFAULT_MAX_RECORDS
    } else {
        max_records
    }
}

fn record_fetch_stat(outcome: &mut StateAdapterOutcome, from_cache: bool) {
    if from_cache {
        outcome.cache_hits += 1;
    } else {
        outcome.fetched += 1;
    }
}

fn is_http_url(raw: &str) -> bool {
    raw.starts_with("https://") || raw.starts_with("http://")
}

fn build_url_with_params(
    base: &str,
    params: &[(&str, String)],
) -> Result<url::Url, StateLicenseError> {
    url::Url::parse_with_params(base, params)
        .map_err(|err| StateLicenseError::InvalidConfig(format!("invalid URL {base}: {err}")))
}

fn parse_socrata_facility_page(
    body: &str,
    state: &str,
    agency: &str,
    verification_url_template: Option<&str>,
    fields: &FacilityFieldMap,
    source_label: &str,
) -> Result<(Vec<FacilityLicenseRecord>, usize), StateLicenseError> {
    let rows: Vec<Map<String, Value>> =
        serde_json::from_str(body).map_err(|source| StateLicenseError::Json {
            source_label: source_label.to_string(),
            source,
        })?;
    let raw_count = rows.len();
    let records = rows
        .iter()
        .filter_map(|row| {
            facility_from_json_map(row, state, agency, verification_url_template, fields)
        })
        .collect();
    Ok((records, raw_count))
}

fn parse_socrata_provider_page(
    body: &str,
    state: &str,
    board: &str,
    verification_url_template: Option<&str>,
    fields: &ProviderFieldMap,
    source_label: &str,
) -> Result<(Vec<ProviderLicenseRecord>, usize), StateLicenseError> {
    let rows: Vec<Map<String, Value>> =
        serde_json::from_str(body).map_err(|source| StateLicenseError::Json {
            source_label: source_label.to_string(),
            source,
        })?;
    let raw_count = rows.len();
    let records = rows
        .iter()
        .filter_map(|row| {
            provider_from_json_map(row, state, board, verification_url_template, fields)
        })
        .collect();
    Ok((records, raw_count))
}

fn parse_arcgis_facility_page(
    body: &str,
    state: &str,
    agency: &str,
    verification_url_template: Option<&str>,
    fields: &FacilityFieldMap,
    source_label: &str,
) -> Result<(Vec<FacilityLicenseRecord>, usize, bool), StateLicenseError> {
    let root: Value = serde_json::from_str(body).map_err(|source| StateLicenseError::Json {
        source_label: source_label.to_string(),
        source,
    })?;

    if let Some(err_obj) = root.get("error") {
        let msg = err_obj
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("ArcGIS query returned an error");
        return Err(StateLicenseError::InvalidConfig(format!(
            "ArcGIS error for {source_label}: {msg}"
        )));
    }

    let exceeded = root
        .get("exceededTransferLimit")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let features = root
        .get("features")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            StateLicenseError::InvalidConfig(format!(
                "ArcGIS response for {source_label} missing 'features' array"
            ))
        })?;

    let raw_count = features.len();
    let mut records = Vec::with_capacity(raw_count);
    for feat in features {
        if let Some(attrs) = feat.get("attributes").and_then(Value::as_object) {
            if let Some(rec) =
                facility_from_json_map(attrs, state, agency, verification_url_template, fields)
            {
                records.push(rec);
            }
        }
    }
    Ok((records, raw_count, exceeded))
}

fn parse_ckan_facility_page(
    body: &str,
    state: &str,
    agency: &str,
    verification_url_template: Option<&str>,
    fields: &FacilityFieldMap,
    source_label: &str,
) -> Result<(Vec<FacilityLicenseRecord>, usize, Option<usize>), StateLicenseError> {
    let root: Value = serde_json::from_str(body).map_err(|source| StateLicenseError::Json {
        source_label: source_label.to_string(),
        source,
    })?;

    if root.get("success").and_then(Value::as_bool) == Some(false) {
        let msg = root
            .get("error")
            .and_then(|e| e.get("message").or(Some(e)))
            .map(Value::to_string)
            .unwrap_or_else(|| "CKAN datastore_search returned success=false".to_string());
        return Err(StateLicenseError::InvalidConfig(format!(
            "CKAN error for {source_label}: {msg}"
        )));
    }

    let result = root.get("result").ok_or_else(|| {
        StateLicenseError::InvalidConfig(format!(
            "CKAN response for {source_label} missing 'result' object"
        ))
    })?;
    let total = result
        .get("total")
        .and_then(Value::as_u64)
        .and_then(|n| usize::try_from(n).ok());
    let rows = result
        .get("records")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            StateLicenseError::InvalidConfig(format!(
                "CKAN response for {source_label} missing 'result.records' array"
            ))
        })?;

    let raw_count = rows.len();
    let mut records = Vec::with_capacity(raw_count);
    for item in rows {
        if let Some(map) = item.as_object() {
            if let Some(rec) =
                facility_from_json_map(map, state, agency, verification_url_template, fields)
            {
                records.push(rec);
            }
        }
    }
    Ok((records, raw_count, total))
}

fn parse_csv_facilities(
    body: &str,
    state: &str,
    agency: &str,
    filter_keywords: &[&str],
    verification_url_template: Option<&str>,
    fields: &FacilityFieldMap,
    source_label: &str,
) -> Result<Vec<FacilityLicenseRecord>, StateLicenseError> {
    let delimiter = detect_csv_delimiter(body);
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .flexible(true)
        .from_reader(body.as_bytes());

    let headers = reader
        .headers()
        .map_err(|source| StateLicenseError::Csv {
            source_label: source_label.to_string(),
            source,
        })?
        .clone();
    let header_idx = HeaderIndex::from_headers(&headers);

    let lower_keywords: Vec<String> = filter_keywords
        .iter()
        .map(|k| k.trim().to_ascii_lowercase())
        .filter(|k| !k.is_empty())
        .collect();

    let mut out = Vec::new();
    for row_res in reader.records() {
        let row = row_res.map_err(|source| StateLicenseError::Csv {
            source_label: source_label.to_string(),
            source,
        })?;
        let facility_name = header_idx.get_str(&row, fields.name).unwrap_or_default();
        let license_number = header_idx
            .get_str(&row, fields.license_number)
            .unwrap_or_default();
        if facility_name.is_empty() && license_number.is_empty() {
            continue;
        }
        let dba_name = header_idx.get_str(&row, fields.dba_or_site_name);
        let license_type = header_idx
            .get_str(&row, fields.license_type)
            .unwrap_or_default();

        if !lower_keywords.is_empty() {
            let haystack = format!(
                "{} {} {}",
                license_type.to_ascii_lowercase(),
                facility_name.to_ascii_lowercase(),
                dba_name.as_deref().unwrap_or_default().to_ascii_lowercase()
            );
            if !lower_keywords.iter().any(|kw| haystack.contains(kw)) {
                continue;
            }
        }

        let status_raw = header_idx.get_str(&row, fields.status).unwrap_or_default();
        let detail_url = header_idx
            .get_str(&row, fields.detail_url)
            .filter(|u| is_http_url(u));
        let verification_url = detail_url
            .or_else(|| build_verification_url(verification_url_template, &license_number, ""));

        let mut street = header_idx.get_str(&row, fields.street);
        let mut city = header_idx.get_str(&row, fields.city);
        let mut zip = header_idx
            .get_str(&row, fields.zip)
            .and_then(|z| normalize_zip_opt(&z));
        if city.is_none() || zip.is_none() {
            if let Some(ref raw_street) = street {
                let (parsed_street, parsed_city, parsed_zip) =
                    parse_combined_us_address(raw_street, state);
                if parsed_city.is_some() || parsed_zip.is_some() {
                    if let Some(clean_street) = parsed_street {
                        street = Some(clean_street);
                    }
                    if city.is_none() {
                        city = parsed_city;
                    }
                    if zip.is_none() {
                        zip = parsed_zip;
                    }
                }
            }
        }

        out.push(FacilityLicenseRecord {
            state: state.to_string(),
            agency: agency.to_string(),
            license_number,
            facility_name,
            dba_name,
            license_type,
            status: LicenseStatus::parse_loose(&status_raw),
            street,
            city,
            zip,
            phone: header_idx.get_str(&row, fields.phone),
            licensed_beds: header_idx
                .get_str(&row, fields.licensed_beds)
                .and_then(|v| parse_u32_loose(&v)),
            expires_on: header_idx.get_str(&row, fields.expires_on),
            disciplinary_actions: None,
            inspection_count: None,
            verification_url,
        });
    }
    Ok(out)
}

fn parse_csv_providers(
    body: &str,
    state: &str,
    default_board: &str,
    verification_url_template: Option<&str>,
    fields: &ProviderFieldMap,
    source_label: &str,
) -> Result<Vec<ProviderLicenseRecord>, StateLicenseError> {
    let delimiter = detect_csv_delimiter(body);
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .flexible(true)
        .from_reader(body.as_bytes());

    let headers = reader
        .headers()
        .map_err(|source| StateLicenseError::Csv {
            source_label: source_label.to_string(),
            source,
        })?
        .clone();
    let header_idx = HeaderIndex::from_headers(&headers);

    let mut out = Vec::new();
    for row_res in reader.records() {
        let row = row_res.map_err(|source| StateLicenseError::Csv {
            source_label: source_label.to_string(),
            source,
        })?;
        let license_number = header_idx
            .get_str(&row, fields.license_number)
            .unwrap_or_default();
        let full_name = header_idx
            .get_str(&row, fields.full_name)
            .or_else(|| {
                combine_first_last(
                    header_idx.get_str(&row, fields.first_name).as_deref(),
                    header_idx.get_str(&row, fields.last_name).as_deref(),
                )
            })
            .unwrap_or_default();
        if full_name.is_empty() && license_number.is_empty() {
            continue;
        }

        let board = header_idx
            .get_str(&row, fields.board_name)
            .unwrap_or_else(|| default_board.to_string());
        let credential_label = header_idx.get_combined_str(&row, fields.credential_or_type);
        let license = classify_state_provider_license(&board, &credential_label);
        let status_raw = header_idx.get_str(&row, fields.status).unwrap_or_default();
        let disciplinary_flag = header_idx
            .get_str(&row, fields.disciplinary_flag)
            .is_some_and(|v| parse_disciplinary_str(&v));
        let verification_url =
            build_verification_url(verification_url_template, &license_number, "");

        out.push(ProviderLicenseRecord {
            state: state.to_string(),
            board,
            license_number,
            full_name,
            credential_label,
            license,
            status: LicenseStatus::parse_loose(&status_raw),
            city: header_idx.get_str(&row, fields.city),
            zip: header_idx
                .get_str(&row, fields.zip)
                .and_then(|z| normalize_zip_opt(&z)),
            expires_on: header_idx.get_str(&row, fields.expires_on),
            disciplinary_flag,
            verification_url,
        });
    }
    Ok(out)
}

fn facility_from_json_map(
    map: &Map<String, Value>,
    state: &str,
    agency: &str,
    verification_url_template: Option<&str>,
    fields: &FacilityFieldMap,
) -> Option<FacilityLicenseRecord> {
    let facility_name = lookup_json_str(map, fields.name).unwrap_or_default();
    let license_number = lookup_json_str(map, fields.license_number).unwrap_or_default();
    if facility_name.is_empty() && license_number.is_empty() {
        return None;
    }

    let dba_name = lookup_json_str(map, fields.dba_or_site_name);
    let license_type = lookup_json_str(map, fields.license_type).unwrap_or_default();
    let status_raw = lookup_json_str(map, fields.status).unwrap_or_default();
    let detail_url = lookup_json_str(map, fields.detail_url).filter(|u| is_http_url(u));
    let verification_url = detail_url
        .or_else(|| build_verification_url(verification_url_template, &license_number, ""));

    let mut street = lookup_json_str(map, fields.street);
    let mut city = lookup_json_str(map, fields.city);
    let mut zip = lookup_json_str(map, fields.zip).and_then(|z| normalize_zip_opt(&z));

    if street.is_none() || city.is_none() || zip.is_none() {
        let (loc_street, loc_city, loc_zip) = extract_socrata_human_address(map);
        if street.is_none() {
            street = loc_street;
        }
        if city.is_none() {
            city = loc_city;
        }
        if zip.is_none() {
            zip = loc_zip;
        }
    }

    if city.is_none() || zip.is_none() {
        if let Some(ref raw_street) = street {
            let (parsed_street, parsed_city, parsed_zip) =
                parse_combined_us_address(raw_street, state);
            if parsed_city.is_some() || parsed_zip.is_some() {
                if let Some(clean_street) = parsed_street {
                    street = Some(clean_street);
                }
                if city.is_none() {
                    city = parsed_city;
                }
                if zip.is_none() {
                    zip = parsed_zip;
                }
            }
        }
    }

    Some(FacilityLicenseRecord {
        state: state.to_string(),
        agency: agency.to_string(),
        license_number,
        facility_name,
        dba_name,
        license_type,
        status: LicenseStatus::parse_loose(&status_raw),
        street,
        city,
        zip,
        phone: lookup_json_str(map, fields.phone),
        licensed_beds: lookup_json_u32(map, fields.licensed_beds),
        expires_on: lookup_json_str(map, fields.expires_on),
        disciplinary_actions: None,
        inspection_count: None,
        verification_url,
    })
}

fn provider_from_json_map(
    map: &Map<String, Value>,
    state: &str,
    default_board: &str,
    verification_url_template: Option<&str>,
    fields: &ProviderFieldMap,
) -> Option<ProviderLicenseRecord> {
    let license_number = lookup_json_str(map, fields.license_number).unwrap_or_default();
    let full_name = lookup_json_str(map, fields.full_name)
        .or_else(|| {
            combine_first_last(
                lookup_json_str(map, fields.first_name).as_deref(),
                lookup_json_str(map, fields.last_name).as_deref(),
            )
        })
        .unwrap_or_default();
    if full_name.is_empty() && license_number.is_empty() {
        return None;
    }

    let board =
        lookup_json_str(map, fields.board_name).unwrap_or_else(|| default_board.to_string());
    let credential_label = combine_json_str_fields(map, fields.credential_or_type);
    let license = classify_state_provider_license(&board, &credential_label);
    let status_raw = lookup_json_str(map, fields.status).unwrap_or_default();
    let disciplinary_flag = lookup_json_disciplinary(map, fields.disciplinary_flag);
    let verification_url = build_verification_url(verification_url_template, &license_number, "");

    Some(ProviderLicenseRecord {
        state: state.to_string(),
        board,
        license_number,
        full_name,
        credential_label,
        license,
        status: LicenseStatus::parse_loose(&status_raw),
        city: lookup_json_str(map, fields.city),
        zip: lookup_json_str(map, fields.zip).and_then(|z| normalize_zip_opt(&z)),
        expires_on: lookup_json_str(map, fields.expires_on),
        disciplinary_flag,
        verification_url,
    })
}

fn parse_json_export_rows(
    raw: &str,
    source_label: &str,
) -> Result<Vec<Map<String, Value>>, StateLicenseError> {
    let root: Value = serde_json::from_str(raw).map_err(|source| StateLicenseError::Json {
        source_label: source_label.to_string(),
        source,
    })?;

    match root {
        Value::Array(items) => Ok(items
            .into_iter()
            .filter_map(|v| match v {
                Value::Object(m) => Some(m),
                _ => None,
            })
            .collect()),
        Value::Object(obj) => {
            if let Some(features) = obj.get("features").and_then(Value::as_array) {
                let rows = features
                    .iter()
                    .filter_map(|f| {
                        f.get("attributes")
                            .or_else(|| f.get("properties"))
                            .and_then(Value::as_object)
                            .cloned()
                    })
                    .collect();
                return Ok(rows);
            }
            if let Some(ckan_rows) = obj
                .get("result")
                .and_then(|r| r.get("records"))
                .and_then(Value::as_array)
            {
                return Ok(ckan_rows
                    .iter()
                    .filter_map(|v| v.as_object().cloned())
                    .collect());
            }
            for wrapper_key in [
                "records",
                "data",
                "results",
                "items",
                "facilities",
                "providers",
                "licenses",
            ] {
                if let Some(arr) = obj.get(wrapper_key).and_then(Value::as_array) {
                    return Ok(arr.iter().filter_map(|v| v.as_object().cloned()).collect());
                }
            }
            Ok(vec![obj])
        }
        _ => Ok(Vec::new()),
    }
}

fn detect_csv_delimiter(body: &str) -> u8 {
    let first_line = body.lines().next().unwrap_or_default();
    if first_line.contains('\t') && !first_line.contains(',') {
        b'\t'
    } else {
        b','
    }
}

struct HeaderIndex {
    exact_lower: HashMap<String, usize>,
    normalized: HashMap<String, usize>,
}

impl HeaderIndex {
    fn from_headers(headers: &csv::StringRecord) -> Self {
        let mut exact_lower = HashMap::new();
        let mut normalized = HashMap::new();
        for (idx, h) in headers.iter().enumerate() {
            let trimmed = h.trim().trim_start_matches('\u{feff}');
            let lower = trimmed.to_ascii_lowercase();
            exact_lower.entry(lower).or_insert(idx);
            let norm = normalize_key(trimmed);
            if !norm.is_empty() {
                normalized.entry(norm).or_insert(idx);
            }
        }
        Self {
            exact_lower,
            normalized,
        }
    }

    fn find(&self, candidates: &[&str]) -> Option<usize> {
        for cand in candidates {
            let lower = cand.trim().to_ascii_lowercase();
            if let Some(&idx) = self.exact_lower.get(&lower) {
                return Some(idx);
            }
        }
        for cand in candidates {
            let norm = normalize_key(cand);
            if let Some(&idx) = self.normalized.get(&norm) {
                return Some(idx);
            }
        }
        None
    }

    fn get_str(&self, row: &csv::StringRecord, candidates: &[&str]) -> Option<String> {
        let idx = self.find(candidates)?;
        let val = row.get(idx)?.trim();
        if val.is_empty() {
            None
        } else {
            Some(val.to_string())
        }
    }

    /// Collects all distinct non-empty values across `candidates` in order so secondary fields
    /// (such as `license_type` alongside `degree`) are never shadowed.
    fn get_combined_str(&self, row: &csv::StringRecord, candidates: &[&str]) -> String {
        let mut parts: Vec<String> = Vec::new();
        for cand in candidates {
            if let Some(val) = self.get_str(row, &[*cand]) {
                push_distinct_label_part(&mut parts, val);
            }
        }
        parts.join(" - ")
    }
}

fn lookup_json_value<'a>(map: &'a Map<String, Value>, candidates: &[&str]) -> Option<&'a Value> {
    for cand in candidates {
        if let Some(v) = map.get(*cand) {
            if !v.is_null() {
                return Some(v);
            }
        }
    }
    for cand in candidates {
        let target = normalize_key(cand);
        if target.is_empty() {
            continue;
        }
        for (k, v) in map {
            if !v.is_null() && normalize_key(k) == target {
                return Some(v);
            }
        }
    }
    None
}

fn lookup_json_str(map: &Map<String, Value>, candidates: &[&str]) -> Option<String> {
    let val = lookup_json_value(map, candidates)?;
    json_value_to_str(val)
}

fn json_value_to_str(val: &Value) -> Option<String> {
    match val {
        Value::String(s) => {
            let t = s.trim();
            (!t.is_empty()).then(|| t.to_string())
        }
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// Collects all distinct non-empty string values across `candidates` in order so a degree
/// column like `"MPAS"` never shadows `license_type = "Physician Assistant License"`.
fn combine_json_str_fields(map: &Map<String, Value>, candidates: &[&str]) -> String {
    let mut parts: Vec<String> = Vec::new();
    for cand in candidates {
        if let Some(val) = lookup_json_str(map, &[*cand]) {
            push_distinct_label_part(&mut parts, val);
        }
    }
    parts.join(" - ")
}

fn push_distinct_label_part(parts: &mut Vec<String>, candidate: String) {
    let cand_lower = candidate.to_ascii_lowercase();
    if parts
        .iter()
        .any(|existing| existing.to_ascii_lowercase().contains(&cand_lower))
    {
        return;
    }
    parts.push(candidate);
}

/// Extracts `(street, city, zip5)` from Socrata nested location objects such as `location_1`
/// or `location` whose `human_address` field holds a JSON-encoded address object (used by PA
/// DDAP `eswt-bam9.json` and similar Socrata datasets).
fn extract_socrata_human_address(
    map: &Map<String, Value>,
) -> (Option<String>, Option<String>, Option<String>) {
    for key in ["location_1", "location", "geocoded_column"] {
        let Some(obj) = map.get(key).and_then(Value::as_object) else {
            continue;
        };
        let Some(human_raw) = obj.get("human_address").and_then(Value::as_str) else {
            continue;
        };
        let Ok(parsed) = serde_json::from_str::<Map<String, Value>>(human_raw) else {
            continue;
        };
        let street = lookup_json_str(&parsed, &["address", "street"]);
        let city = lookup_json_str(&parsed, &["city"]);
        let zip =
            lookup_json_str(&parsed, &["zip", "postal_code"]).and_then(|z| normalize_zip_opt(&z));
        if street.is_some() || city.is_some() || zip.is_some() {
            return (street, city, zip);
        }
    }
    (None, None, None)
}

/// Parses a single-line US address such as:
/// - `"1100 THIRD STREET, TILLAMOOK, OR   97141"` (OR OHA `USER_FULL_ADDRESS`)
/// - `"1100 3rd St, Tillamook, Oregon, 97141"` (OR OHA `Match_addr`)
/// - `"425 2nd Street NW, 2 North   Washington, DC 20001"` (DC DBH `ADDRESS`)
/// - `"1930 Martin Luther King Jr., Avenue SEWashington, DC 20020"` (DC DBH concatenated)
///
/// Returns `(cleaned_street, city, zip5)`.
fn parse_combined_us_address(
    raw: &str,
    expected_state: &str,
) -> (Option<String>, Option<String>, Option<String>) {
    let normalized = raw.replace('\u{00a0}', " ");
    let trimmed = normalized.trim();
    if trimmed.is_empty() {
        return (None, None, None);
    }

    // 1. Extract trailing 5-digit ZIP (optionally with -4 extension).
    let (before_zip, zip5) = split_trailing_zip(trimmed);
    let Some(zip5) = zip5 else {
        return (None, None, None);
    };

    // 2. Strip trailing comma/whitespace and state code or full state name before the ZIP.
    let before_zip = before_zip.trim_end_matches(|c: char| c.is_whitespace() || c == ',');
    let state_upper = expected_state.trim().to_ascii_uppercase();
    let before_state = strip_trailing_state(before_zip, &state_upper);
    let before_state = before_state.trim_end_matches(|c: char| c.is_whitespace() || c == ',');

    if before_state.is_empty() {
        return (None, None, Some(zip5));
    }

    // 3. Special handling for DC where "Washington" is occasionally concatenated directly onto
    //    the street quadrant (e.g. "Avenue SEWashington") or separated by multiple spaces.
    if state_upper == "DC" {
        let lower = before_state.to_ascii_lowercase();
        if lower.ends_with("washington") {
            let street_part = before_state[..before_state.len() - "washington".len()]
                .trim_end_matches(|c: char| c.is_whitespace() || c == ',');
            let street = (!street_part.is_empty()).then(|| street_part.to_string());
            return (street, Some("Washington".to_string()), Some(zip5));
        }
    }

    // 4. Comma-delimited: "... , CITY" (with guard for "2 North   Washington" style multi-space).
    if let Some(comma_idx) = before_state.rfind(',') {
        let left = before_state[..comma_idx].trim();
        let right = before_state[comma_idx + 1..].trim();
        if !right.is_empty() {
            if let Some((extra_street, city_part)) = split_last_double_space(right) {
                let combined_street = if left.is_empty() {
                    extra_street.to_string()
                } else {
                    format!("{left}, {extra_street}")
                };
                return (
                    Some(combined_street),
                    Some(city_part.to_string()),
                    Some(zip5),
                );
            }
            let street = (!left.is_empty()).then(|| left.to_string());
            return (street, Some(right.to_string()), Some(zip5));
        }
    }

    // 5. No comma before state/ZIP: split on the last run of 2+ consecutive spaces.
    if let Some((street_part, city_part)) = split_last_double_space(before_state) {
        return (
            Some(street_part.to_string()),
            Some(city_part.to_string()),
            Some(zip5),
        );
    }

    (Some(before_state.to_string()), None, Some(zip5))
}

fn split_trailing_zip(s: &str) -> (&str, Option<String>) {
    let bytes = s.as_bytes();
    let mut end = bytes.len();
    while end > 0 && bytes[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    let slice = &s[..end];
    // Match optional "-1234" suffix first.
    let base = if slice.len() >= 10 {
        let tail = &slice[slice.len() - 5..];
        if tail.starts_with('-') && tail[1..].bytes().all(|b| b.is_ascii_digit()) {
            &slice[..slice.len() - 5]
        } else {
            slice
        }
    } else {
        slice
    };
    if base.len() < 5 {
        return (s, None);
    }
    let zip_cand = &base[base.len() - 5..];
    if !zip_cand.bytes().all(|b| b.is_ascii_digit()) {
        return (s, None);
    }
    let prefix = &base[..base.len() - 5];
    if let Some(prev) = prefix.chars().next_back() {
        if prev.is_ascii_alphanumeric() {
            return (s, None);
        }
    }
    (prefix, Some(zip_cand.to_string()))
}

fn strip_trailing_state<'a>(s: &'a str, expected_state: &str) -> &'a str {
    let trimmed = s.trim_end();
    if expected_state.len() == 2 {
        if let Some(tail) = trimmed.get(trimmed.len().saturating_sub(2)..) {
            if tail.eq_ignore_ascii_case(expected_state) {
                let prefix = &trimmed[..trimmed.len() - 2];
                if prefix
                    .chars()
                    .next_back()
                    .is_none_or(|c| !c.is_ascii_alphabetic())
                {
                    return prefix;
                }
            }
        }
    }
    if let Some(full_name) = state_full_name(expected_state) {
        if trimmed.len() >= full_name.len() {
            let tail = &trimmed[trimmed.len() - full_name.len()..];
            if tail.eq_ignore_ascii_case(full_name) {
                let prefix = &trimmed[..trimmed.len() - full_name.len()];
                if prefix
                    .chars()
                    .next_back()
                    .is_none_or(|c| !c.is_ascii_alphabetic())
                {
                    return prefix;
                }
            }
        }
    }
    trimmed
}

fn split_last_double_space(s: &str) -> Option<(&str, &str)> {
    let idx = s.rfind("  ")?;
    let left = s[..idx].trim_end();
    let right = s[idx..].trim_start();
    if left.is_empty() || right.is_empty() {
        None
    } else {
        Some((left, right))
    }
}

fn state_full_name(code: &str) -> Option<&'static str> {
    Some(match code {
        "AL" => "Alabama",
        "AK" => "Alaska",
        "AZ" => "Arizona",
        "AR" => "Arkansas",
        "CA" => "California",
        "CO" => "Colorado",
        "CT" => "Connecticut",
        "DE" => "Delaware",
        "DC" => "District of Columbia",
        "FL" => "Florida",
        "GA" => "Georgia",
        "HI" => "Hawaii",
        "ID" => "Idaho",
        "IL" => "Illinois",
        "IN" => "Indiana",
        "IA" => "Iowa",
        "KS" => "Kansas",
        "KY" => "Kentucky",
        "LA" => "Louisiana",
        "ME" => "Maine",
        "MD" => "Maryland",
        "MA" => "Massachusetts",
        "MI" => "Michigan",
        "MN" => "Minnesota",
        "MS" => "Mississippi",
        "MO" => "Missouri",
        "MT" => "Montana",
        "NE" => "Nebraska",
        "NV" => "Nevada",
        "NH" => "New Hampshire",
        "NJ" => "New Jersey",
        "NM" => "New Mexico",
        "NY" => "New York",
        "NC" => "North Carolina",
        "ND" => "North Dakota",
        "OH" => "Ohio",
        "OK" => "Oklahoma",
        "OR" => "Oregon",
        "PA" => "Pennsylvania",
        "RI" => "Rhode Island",
        "SC" => "South Carolina",
        "SD" => "South Dakota",
        "TN" => "Tennessee",
        "TX" => "Texas",
        "UT" => "Utah",
        "VT" => "Vermont",
        "VA" => "Virginia",
        "WA" => "Washington",
        "WV" => "West Virginia",
        "WI" => "Wisconsin",
        "WY" => "Wyoming",
        _ => return None,
    })
}

fn lookup_json_u32(map: &Map<String, Value>, candidates: &[&str]) -> Option<u32> {
    let val = lookup_json_value(map, candidates)?;
    let parsed = match val {
        Value::Number(n) => n
            .as_u64()
            .and_then(|v| u32::try_from(v).ok())
            .or_else(|| n.as_f64().filter(|f| *f >= 0.0).map(|f| f.round() as u32)),
        Value::String(s) => parse_u32_loose(s),
        _ => None,
    };
    parsed.filter(|&n| n > 0)
}

fn lookup_json_disciplinary(map: &Map<String, Value>, candidates: &[&str]) -> bool {
    let Some(val) = lookup_json_value(map, candidates) else {
        return false;
    };
    match val {
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_u64().is_some_and(|v| v > 0),
        Value::String(s) => parse_disciplinary_str(s),
        _ => false,
    }
}

fn parse_disciplinary_str(raw: &str) -> bool {
    let s = raw.trim().to_ascii_lowercase();
    if s.is_empty()
        || matches!(s.as_str(), "n" | "no" | "false" | "0" | "none" | "n/a")
        || s.starts_with("no ")
    {
        return false;
    }
    matches!(s.as_str(), "y" | "yes" | "true" | "1")
        || s.contains("disciplin")
        || s.contains("action")
        || s.contains("sanction")
        || s.contains("revok")
        || s.contains("suspend")
        || s.contains("probat")
        || s.parse::<u32>().is_ok_and(|n| n > 0)
}

fn parse_u32_loose(raw: &str) -> Option<u32> {
    let cleaned: String = raw
        .trim()
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    if cleaned.is_empty() {
        return None;
    }
    if let Ok(v) = cleaned.parse::<u32>() {
        return (v > 0).then_some(v);
    }
    cleaned
        .parse::<f64>()
        .ok()
        .filter(|f| *f > 0.0 && *f <= f64::from(u32::MAX))
        .map(|f| f.round() as u32)
        .filter(|&n| n > 0)
}

fn normalize_zip_opt(raw: &str) -> Option<String> {
    let digits: String = raw.chars().filter(|c| c.is_ascii_digit()).take(5).collect();
    if digits.len() == 5 {
        Some(digits)
    } else {
        let trimmed = raw.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    }
}

fn combine_first_last(first: Option<&str>, last: Option<&str>) -> Option<String> {
    let f = first.map(str::trim).filter(|s| !s.is_empty());
    let l = last.map(str::trim).filter(|s| !s.is_empty());
    match (f, l) {
        (Some(f), Some(l)) => Some(format!("{f} {l}")),
        (Some(f), None) => Some(f.to_string()),
        (None, Some(l)) => Some(l.to_string()),
        (None, None) => None,
    }
}

fn normalize_key(raw: &str) -> String {
    raw.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Splits a credential or profession string into uppercase alphanumeric tokens.
fn credential_tokens(raw: &str) -> Vec<String> {
    let mut out = Vec::new();
    for part in raw.split([',', '/', ';', ' ', '\t', '(', ')']) {
        let cleaned: String = part
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .map(|c| c.to_ascii_uppercase())
            .collect();
        if !cleaned.is_empty() {
            out.push(cleaned);
        }
        if part.contains('-') {
            for sub in part.split('-') {
                let sub_clean: String = sub
                    .chars()
                    .filter(|c| c.is_ascii_alphanumeric())
                    .map(|c| c.to_ascii_uppercase())
                    .collect();
                if !sub_clean.is_empty() && !out.contains(&sub_clean) {
                    out.push(sub_clean);
                }
            }
        }
    }
    out
}

const DEFAULT_FACILITY_FIELDS: FacilityFieldMap = FacilityFieldMap {
    license_number: &[
        "license_number",
        "license_no",
        "lic_no",
        "lic_num",
        "license_id",
        "license",
        "facility_id",
        "program_number",
        "program_id",
        "certificate_number",
        "permit_number",
        "id",
    ],
    name: &[
        "facility_name",
        "program_name",
        "provider_name",
        "legal_name",
        "business_name",
        "agency_name",
        "name",
    ],
    dba_or_site_name: &[
        "dba",
        "dba_name",
        "doing_business_as",
        "site_name",
        "operating_name",
        "alt_name",
    ],
    license_type: &[
        "license_type",
        "facility_type",
        "facility_category",
        "program_type",
        "service_type",
        "category",
        "subtype",
        "type",
    ],
    status: &[
        "status",
        "license_status",
        "facility_status",
        "standing",
        "lic_status",
    ],
    street: &[
        "street",
        "address",
        "address_1",
        "address1",
        "street_address",
        "physical_address",
        "location_address",
    ],
    city: &["city", "physical_city", "location_city", "municipality"],
    zip: &[
        "zip",
        "zip_code",
        "zipcode",
        "postal_code",
        "physical_zip",
        "location_zip",
    ],
    phone: &[
        "phone",
        "telephone",
        "phone_number",
        "telephone_number",
        "contact_phone",
    ],
    licensed_beds: &[
        "licensed_beds",
        "bed_count",
        "beds",
        "capacity",
        "licensed_capacity",
        "total_beds",
    ],
    expires_on: &[
        "expires_on",
        "expiration_date",
        "expire_date",
        "license_expiration",
        "exp_date",
    ],
    detail_url: &["detail_url", "verification_url", "profile_url", "url"],
};

const DEFAULT_PROVIDER_FIELDS: ProviderFieldMap = ProviderFieldMap {
    license_number: &[
        "license_number",
        "license_no",
        "lic_no",
        "lic_num",
        "license_id",
        "license",
        "credential_number",
        "registration_number",
    ],
    full_name: &[
        "full_name",
        "provider_name",
        "practitioner_name",
        "licensee_name",
        "name",
    ],
    first_name: &["first_name", "fname", "given_name"],
    last_name: &["last_name", "lname", "surname", "family_name"],
    credential_or_type: &[
        "credential",
        "credential_label",
        "license_type",
        "profession",
        "profession_name",
        "title",
        "degree",
        "specialty",
        "type",
    ],
    board_name: &[
        "board",
        "board_name",
        "licensing_board",
        "agency",
        "regulatory_board",
    ],
    status: &[
        "status",
        "license_status",
        "standing",
        "lic_status",
        "credential_status",
    ],
    city: &["city", "practice_city", "business_city"],
    zip: &["zip", "zip_code", "zipcode", "postal_code", "practice_zip"],
    expires_on: &[
        "expires_on",
        "expiration_date",
        "expire_date",
        "exp_date",
        "license_expiration",
    ],
    disciplinary_flag: &[
        "disciplinary_flag",
        "discipline",
        "disciplinary_action",
        "has_discipline",
        "board_action",
        "sanctions",
    ],
};

#[cfg(test)]
mod tests {
    use super::*;

    fn test_facility_fields() -> FacilityFieldMap {
        FacilityFieldMap {
            license_number: &["lic_num", "license_number"],
            name: &["fac_name", "facility_name"],
            dba_or_site_name: &["dba"],
            license_type: &["fac_type", "license_type"],
            status: &["status"],
            street: &["address"],
            city: &["city"],
            zip: &["zip"],
            phone: &["phone"],
            licensed_beds: &["beds", "capacity"],
            expires_on: &["exp_date"],
            detail_url: &["detail_url"],
        }
    }

    fn test_provider_fields() -> ProviderFieldMap {
        ProviderFieldMap {
            license_number: &["lic_no"],
            full_name: &["full_name"],
            first_name: &["first_name"],
            last_name: &["last_name"],
            credential_or_type: &["license_type"],
            board_name: &["board"],
            status: &["status"],
            city: &["city"],
            zip: &["zip"],
            expires_on: &["exp_date"],
            disciplinary_flag: &["discipline"],
        }
    }

    #[test]
    fn parses_socrata_facility_json() {
        let json = r#"[
            {
                "lic_num": "BH-4021",
                "fac_name": "Desert Hope Behavioral Health",
                "dba": "Desert Hope Scottsdale",
                "fac_type": "Behavioral Health Residential Facility",
                "status": "Active",
                "address": "1234 E Camelback Rd",
                "city": "Scottsdale",
                "zip": "85251-1234",
                "phone": "(480) 555-0199",
                "beds": "24",
                "exp_date": "2027-06-30"
            }
        ]"#;
        let fields = test_facility_fields();
        let (records, raw_count) = parse_socrata_facility_page(
            json,
            "AZ",
            "AZDHS",
            Some("https://azcarecheck.azdhs.gov/s/?lic={license}"),
            &fields,
            "test-socrata",
        )
        .expect("socrata facility parse");

        assert_eq!(raw_count, 1);
        assert_eq!(records.len(), 1);
        let r = &records[0];
        assert_eq!(r.state, "AZ");
        assert_eq!(r.agency, "AZDHS");
        assert_eq!(r.license_number, "BH-4021");
        assert_eq!(r.facility_name, "Desert Hope Behavioral Health");
        assert_eq!(r.dba_name.as_deref(), Some("Desert Hope Scottsdale"));
        assert_eq!(r.status, LicenseStatus::Active);
        assert_eq!(r.zip.as_deref(), Some("85251"));
        assert_eq!(r.licensed_beds, Some(24));
        assert_eq!(
            r.verification_url.as_deref(),
            Some("https://azcarecheck.azdhs.gov/s/?lic=BH-4021")
        );
    }

    #[test]
    fn parses_socrata_provider_json_with_strict_classification() {
        let json = r#"[
            {
                "lic_no": "DO-8812",
                "first_name": "Maya",
                "last_name": "Patel",
                "license_type": "Osteopathic Physician - Psychiatry",
                "board": "AZ Board of Osteopathic Examiners",
                "status": "Active",
                "city": "Phoenix",
                "zip": "85004",
                "discipline": "No"
            },
            {
                "lic_no": "MD-9910",
                "full_name": "Daniel Kim",
                "license_type": "MD Psychiatrist",
                "board": "AZ Medical Board",
                "status": "Probation",
                "discipline": "Yes"
            }
        ]"#;
        let fields = test_provider_fields();
        let (records, raw_count) = parse_socrata_provider_page(
            json,
            "AZ",
            "AZ State Board",
            Some("https://verify.az.gov/?id={id}"),
            &fields,
            "test-socrata-prov",
        )
        .expect("socrata provider parse");

        assert_eq!(raw_count, 2);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].full_name, "Maya Patel");
        assert_eq!(records[0].license, Some(StaffLicense::DoPsychiatrist));
        assert!(!records[0].disciplinary_flag);
        assert_eq!(records[1].license, Some(StaffLicense::MdPsychiatrist));
        assert_eq!(records[1].status, LicenseStatus::Probation);
        assert!(records[1].disciplinary_flag);
    }

    #[test]
    fn parses_arcgis_features() {
        let json = r#"{
            "exceededTransferLimit": true,
            "features": [
                {
                    "attributes": {
                        "FAC_NAME": "Mountain View Recovery",
                        "LIC_NUM": "CO-BHE-101",
                        "FAC_TYPE": "Behavioral Health Entity",
                        "STATUS": "Licensed",
                        "ADDRESS": "500 Broadway",
                        "CITY": "Denver",
                        "ZIP": "80203",
                        "CAPACITY": 32
                    }
                }
            ]
        }"#;
        let fields = test_facility_fields();
        let (records, raw_count, exceeded) =
            parse_arcgis_facility_page(json, "CO", "CO BHA", None, &fields, "test-arcgis")
                .expect("arcgis parse");

        assert_eq!(raw_count, 1);
        assert!(exceeded);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].facility_name, "Mountain View Recovery");
        assert_eq!(records[0].licensed_beds, Some(32));
        assert_eq!(records[0].status, LicenseStatus::Active);
    }

    #[test]
    fn parses_ckan_datastore_response() {
        let json = r#"{
            "success": true,
            "result": {
                "total": 1,
                "records": [
                    {
                        "license_number": "300012345",
                        "facility_name": "Pacific Coast Treatment",
                        "license_type": "SUD Residential",
                        "status": "Active",
                        "address": "710 Harbor Blvd",
                        "city": "Costa Mesa",
                        "zip": "92627",
                        "beds": "18"
                    }
                ]
            }
        }"#;
        let fields = test_facility_fields();
        let (records, raw_count, total) = parse_ckan_facility_page(
            json,
            "CA",
            "CA DHCS",
            Some("https://data.chhs.ca.gov/lookup?id={license_number}"),
            &fields,
            "test-ckan",
        )
        .expect("ckan parse");

        assert_eq!(raw_count, 1);
        assert_eq!(total, Some(1));
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].license_number, "300012345");
        assert_eq!(records[0].licensed_beds, Some(18));
        assert_eq!(
            records[0].verification_url.as_deref(),
            Some("https://data.chhs.ca.gov/lookup?id=300012345")
        );
    }

    #[test]
    fn parses_csv_facilities_with_keyword_filter() {
        let csv_data = "\
License Number,Facility Name,License Type,Status,Address,City,Zip,Phone,Beds
BH-100,Sunrise Behavioral Health,Behavioral Health Residential,Active,100 Main St,Phoenix,85004,602-555-0100,16
SNF-200,Oakwood Nursing Home,Skilled Nursing Facility,Active,200 Oak Ave,Phoenix,85004,602-555-0200,80
";
        let records = parse_csv_facilities(
            csv_data,
            "AZ",
            "AZDHS",
            &["behavioral", "substance"],
            None,
            &DEFAULT_FACILITY_FIELDS,
            "test-csv",
        )
        .expect("csv parse");

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].license_number, "BH-100");
        assert_eq!(records[0].licensed_beds, Some(16));
    }

    #[test]
    fn imports_local_csv_and_json_files() {
        let dir = std::env::temp_dir().join(format!("recovery-lic-test-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);

        let csv_path = dir.join("providers.csv");
        fs::write(
            &csv_path,
            "License No,First Name,Last Name,Credential,Status,Discipline\n\
             LIC-1,Elena,Vance,PMHNP-BC,Active,No\n\
             LIC-2,Marcus,Cole,Psy.D.,Active,No\n",
        )
        .expect("write csv");

        let csv_out = import_local_file(&csv_path, "CO", "CO DORA", LicensingScope::Provider)
            .expect("import csv");
        assert_eq!(csv_out.provider_records.len(), 2);
        assert_eq!(csv_out.provider_records[0].full_name, "Elena Vance");
        assert_eq!(
            csv_out.provider_records[0].license,
            Some(StaffLicense::Pmhnp)
        );
        assert_eq!(
            csv_out.provider_records[1].license,
            Some(StaffLicense::PsydPsychologist)
        );

        let json_path = dir.join("facilities.json");
        fs::write(
            &json_path,
            r#"{"records":[{"license_number":"NY-900","facility_name":"Hudson Valley OASAS Clinic","license_type":"Outpatient SUD","status":"Active","city":"Albany","zip":"12207"}]}"#,
        )
        .expect("write json");

        let json_out = import_local_file(&json_path, "NY", "NY OASAS", LicensingScope::Facility)
            .expect("import json");
        assert_eq!(json_out.facility_records.len(), 1);
        assert_eq!(json_out.facility_records[0].license_number, "NY-900");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn detects_captcha_and_anti_bot_challenges() {
        assert!(detect_captcha_or_block(
            403,
            "<html><head><title>Just a moment...</title></head><body>Cloudflare Turnstile cf-turnstile</body></html>"
        ));
        assert!(detect_captcha_or_block(
            200,
            "<div class=\"g-recaptcha\" data-sitekey=\"abc\"></div>"
        ));
        assert!(detect_captcha_or_block(
            200,
            "<script src=\"https://hcaptcha.com/1/api.js\"></script>"
        ));
        assert!(detect_captcha_or_block(
            200,
            "<iframe src=\"/_Incapsula_Resource?SWKMTFSR=1\"></iframe>"
        ));
        assert!(detect_captcha_or_block(
            403,
            "<!DOCTYPE html><html><body>Access Denied - please complete captcha</body></html>"
        ));
        assert!(!detect_captcha_or_block(
            200,
            r#"[{"facility_name":"Hope Recovery Center","status":"Active"}]"#
        ));
    }

    #[test]
    fn strictly_classifies_provider_credentials_without_conflation() {
        // MD vs DO strictly separated
        assert_eq!(
            classify_state_provider_license("AZ Medical Board", "MD - Psychiatry"),
            Some(StaffLicense::MdPsychiatrist)
        );
        assert_eq!(
            classify_state_provider_license("AZ Board of Osteopathic Examiners", "DO - Psychiatry"),
            Some(StaffLicense::DoPsychiatrist)
        );
        assert_eq!(
            classify_state_provider_license("AZ Board of Osteopathic Examiners", "Physician"),
            Some(StaffLicense::DoPhysician)
        );
        assert_eq!(
            classify_state_provider_license("Medical Board of California", "Physician and Surgeon"),
            Some(StaffLicense::MdPhysician)
        );
        // Combined board or blank board with generic "Physician" or "Psychiatrist" must NOT guess MD vs DO
        assert_eq!(
            classify_state_provider_license(
                "Board of Medical and Osteopathic Examiners",
                "Physician"
            ),
            None
        );
        assert_eq!(
            classify_state_provider_license("State Board of Healing Arts", "Psychiatrist"),
            None
        );
        assert_eq!(classify_state_provider_license("", "Psychiatrist"), None);

        // PA under Medical Board must classify as PA, never MD
        assert_eq!(
            classify_state_provider_license("Texas Medical Board", "Physician Assistant (PA-C)"),
            Some(StaffLicense::Pa)
        );

        // PMHNP vs NP vs PA strictly separated from physicians
        assert_eq!(
            classify_state_provider_license("AZ Board of Nursing", "PMHNP-BC"),
            Some(StaffLicense::Pmhnp)
        );
        assert_eq!(
            classify_state_provider_license(
                "AZ Board of Nursing",
                "Psychiatric Mental Health Nurse Practitioner"
            ),
            Some(StaffLicense::Pmhnp)
        );
        assert_eq!(
            classify_state_provider_license(
                "AZ Board of Nursing",
                "Family Nurse Practitioner (FNP)"
            ),
            Some(StaffLicense::Np)
        );

        // PhD vs PsyD strictly separated; generic Psychologist returns None
        assert_eq!(
            classify_state_provider_license(
                "Board of Psychologist Examiners",
                "Licensed Psychologist, Ph.D."
            ),
            Some(StaffLicense::PhdPsychologist)
        );
        assert_eq!(
            classify_state_provider_license(
                "Board of Psychologist Examiners",
                "Licensed Psychologist, Psy.D."
            ),
            Some(StaffLicense::PsydPsychologist)
        );
        assert_eq!(
            classify_state_provider_license(
                "Board of Psychologist Examiners",
                "Licensed Psychologist"
            ),
            None
        );

        // Master's therapists, addiction counselors, nursing, peer support
        assert_eq!(
            classify_state_provider_license("Board of Behavioral Health", "LCSW"),
            Some(StaffLicense::Lcsw)
        );
        assert_eq!(
            classify_state_provider_license("Board of Behavioral Health", "LPC"),
            Some(StaffLicense::LpcLmhc)
        );
        assert_eq!(
            classify_state_provider_license("Board of Behavioral Health", "LMFT"),
            Some(StaffLicense::Lmft)
        );
        assert_eq!(
            classify_state_provider_license("Board of Nursing", "Registered Nurse (RN)"),
            Some(StaffLicense::Rn)
        );
        assert_eq!(
            classify_state_provider_license("Board of Nursing", "Licensed Practical Nurse (LPN)"),
            Some(StaffLicense::Lpn)
        );
        assert_eq!(
            classify_state_provider_license("OASAS", "CASAC"),
            Some(StaffLicense::AddictionCounselor)
        );
        assert_eq!(
            classify_state_provider_license(
                "State Behavioral Health",
                "Certified Peer Support Specialist"
            ),
            Some(StaffLicense::PeerSupport)
        );
    }

    #[test]
    fn builds_verification_urls_safely() {
        assert_eq!(
            build_verification_url(
                Some("https://portal.state.gov/verify?lic={license}"),
                "BH 123/4",
                "https://portal.state.gov/"
            ),
            Some("https://portal.state.gov/verify?lic=BH+123%2F4".to_string())
        );
        assert_eq!(
            build_verification_url(None, "BH-100", "https://portal.state.gov/search"),
            Some("https://portal.state.gov/search".to_string())
        );
        assert_eq!(
            build_verification_url(Some("javascript:alert(1)"), "123", ""),
            None
        );
    }

    #[test]
    fn parses_combined_us_addresses_for_or_and_dc() {
        let (st, city, zip) =
            parse_combined_us_address("1100 THIRD STREET, TILLAMOOK, OR   97141", "OR");
        assert_eq!(st.as_deref(), Some("1100 THIRD STREET"));
        assert_eq!(city.as_deref(), Some("TILLAMOOK"));
        assert_eq!(zip.as_deref(), Some("97141"));

        let (st2, city2, zip2) =
            parse_combined_us_address("12350 NW Main St, Ste 112, Banks, Oregon, 97106", "OR");
        assert_eq!(st2.as_deref(), Some("12350 NW Main St, Ste 112"));
        assert_eq!(city2.as_deref(), Some("Banks"));
        assert_eq!(zip2.as_deref(), Some("97106"));

        let (st3, city3, zip3) =
            parse_combined_us_address("425 2nd Street NW, 2 North   Washington, DC 20001", "DC");
        assert_eq!(st3.as_deref(), Some("425 2nd Street NW, 2 North"));
        assert_eq!(city3.as_deref(), Some("Washington"));
        assert_eq!(zip3.as_deref(), Some("20001"));

        let (st4, city4, zip4) = parse_combined_us_address(
            "2041 Martin Luther King Jr. Avenue SE   Suite 109   Washington,\u{00a0}DC 20020",
            "DC",
        );
        assert_eq!(
            st4.as_deref(),
            Some("2041 Martin Luther King Jr. Avenue SE   Suite 109")
        );
        assert_eq!(city4.as_deref(), Some("Washington"));
        assert_eq!(zip4.as_deref(), Some("20020"));

        let (st5, city5, zip5) = parse_combined_us_address(
            "1930 Martin Luther King Jr., Avenue SEWashington, DC 20020",
            "DC",
        );
        assert_eq!(
            st5.as_deref(),
            Some("1930 Martin Luther King Jr., Avenue SE")
        );
        assert_eq!(city5.as_deref(), Some("Washington"));
        assert_eq!(zip5.as_deref(), Some("20020"));
    }

    #[test]
    fn extracts_pa_socrata_human_address_city_and_zip() {
        let json = r#"[{
            "type_of_facility": "DDAP-Licensed Treatment Facility",
            "name_of_facility": "Choices Recovery Program",
            "street_address": "Rear 307 Laird Street",
            "phone_number": "570-408-9320",
            "location_1": {
                "latitude": "41.2563695",
                "longitude": "-75.8278712",
                "human_address": "{\"address\": \"\", \"city\": \"Wilkes Barre\", \"state\": \"Pennsylvania\", \"zip\": \"18702\"}"
            }
        }]"#;
        let fields = FacilityFieldMap {
            license_number: &[],
            name: &["name_of_facility"],
            dba_or_site_name: &[],
            license_type: &["type_of_facility"],
            status: &[],
            street: &["street_address"],
            city: &[],
            zip: &[],
            phone: &["phone_number"],
            licensed_beds: &[],
            expires_on: &[],
            detail_url: &[],
        };
        let (records, raw_count) =
            parse_socrata_facility_page(json, "PA", "PA DDAP", None, &fields, "pa-ddap-test")
                .expect("pa parse");
        assert_eq!(raw_count, 1);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].street.as_deref(), Some("Rear 307 Laird Street"));
        assert_eq!(records[0].city.as_deref(), Some("Wilkes Barre"));
        assert_eq!(records[0].zip.as_deref(), Some("18702"));
    }

    #[test]
    fn tx_tmb_degree_does_not_shadow_physician_assistant_license_type() {
        let json = r#"[
            {
                "license_number": "PA10001",
                "first_name": "Alex",
                "last_name": "Rivera",
                "degree": "MPAS",
                "license_type": "Physician Assistant License",
                "registration_status": "ACTIVE"
            },
            {
                "license_number": "M20002",
                "first_name": "Priya",
                "last_name": "Nair",
                "degree": "MD",
                "license_type": "Physician License",
                "registration_status": "ACTIVE"
            },
            {
                "license_number": "D30003",
                "first_name": "Sam",
                "last_name": "Carter",
                "degree": "DO",
                "license_type": "Physician License",
                "registration_status": "ACTIVE"
            }
        ]"#;
        let fields = ProviderFieldMap {
            license_number: &["license_number"],
            full_name: &[],
            first_name: &["first_name"],
            last_name: &["last_name"],
            credential_or_type: &["degree", "license_type"],
            board_name: &[],
            status: &["registration_status"],
            city: &[],
            zip: &[],
            expires_on: &[],
            disciplinary_flag: &[],
        };
        let (records, raw_count) = parse_socrata_provider_page(
            json,
            "TX",
            "Texas Medical Board",
            None,
            &fields,
            "tx-tmb-test",
        )
        .expect("tx parse");
        assert_eq!(raw_count, 3);
        assert_eq!(records[0].license, Some(StaffLicense::Pa));
        assert_eq!(records[1].license, Some(StaffLicense::MdPhysician));
        assert_eq!(records[2].license, Some(StaffLicense::DoPhysician));
    }
}
