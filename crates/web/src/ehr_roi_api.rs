//! Live server-side SMART on FHIR `.well-known/smart-configuration` discovery
//! and operator-configured automated ROI dispatch gateway (`ssr` only).
//!
//! Security & privacy invariants:
//! - Strictly fail-closed on non-HTTPS, credentialed, or private/link-local/loopback
//!   target URLs (preventing SSRF on SMART discovery and ROI webhook relay).
//! - Every response sets `Cache-Control: private, no-store, max-age=0` and `Vary: *`.
//! - When `ROI_DISPATCH_WEBHOOK_URL` is unset (default public zero-PHI demo mode),
//!   `/roi/dispatch-live` executes deterministic local sandbox dispatch and never
//!   makes outbound network calls or logs request payloads.

use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Query, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use clinical_workspace::{
    build_shlink_manifest_request_body, dispatch_roi_request, parse_smart_health_link,
    render_roi_cover_text, render_roi_form_html, sign_kipu_request, validate_public_https_url,
    validate_roi_request, KipuAuthScheme, RoiBatchDispatchReport, RoiDispatchItemOutcome,
    RoiRequest, RoiStatus, KIPU_DEFAULT_BASE_URL,
};
use serde::{Deserialize, Serialize};
use tower_http::limit::RequestBodyLimitLayer;
use url::Url;

const MAX_DISCOVERY_BYTES: usize = 128 * 1024;
const MAX_ROI_DISPATCH_BODY: usize = 256 * 1024;

/// Optional operator configuration for automated server-side ROI dispatch
/// (for example an e-fax / HIM email relay webhook).
///
/// Implements redacted [`std::fmt::Debug`] so `bearer_token` is never exposed.
#[derive(Clone, PartialEq, Eq)]
pub struct RoiWebhookConfig {
    webhook_url: Url,
    bearer_token: Option<String>,
}

impl std::fmt::Debug for RoiWebhookConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RoiWebhookConfig")
            .field("webhook_url", &self.webhook_url)
            .field(
                "bearer_token",
                &self.bearer_token.as_ref().map(|_| "[REDACTED]"),
            )
            .finish()
    }
}

impl RoiWebhookConfig {
    /// Reads `ROI_DISPATCH_WEBHOOK_URL` and optional `ROI_DISPATCH_BEARER_TOKEN`.
    /// Returns `Ok(None)` when unset so default deployments stay strictly local/sandbox.
    pub fn from_env(read: impl Fn(&str) -> Option<String>) -> Result<Option<Self>, String> {
        let raw = match read("ROI_DISPATCH_WEBHOOK_URL") {
            Some(v) if !v.trim().is_empty() => v,
            _ => return Ok(None),
        };
        let is_prod = read("K_SERVICE").is_some()
            || read("LEPTOS_ENV")
                .as_deref()
                .is_some_and(|v| v.eq_ignore_ascii_case("PROD"));
        let allow_loopback = !is_prod;
        let url = validate_outbound_url(&raw, allow_loopback)?;
        let bearer_token = read("ROI_DISPATCH_BEARER_TOKEN")
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        Ok(Some(Self {
            webhook_url: url,
            bearer_token,
        }))
    }

    pub fn webhook_url(&self) -> &Url {
        &self.webhook_url
    }
}

/// Validates an outbound HTTPS URL (rejecting embedded credentials, fragments,
/// non-HTTPS schemes, and private/link-local/metadata IP literals).
pub fn validate_outbound_url(raw: &str, allow_loopback_http: bool) -> Result<Url, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.chars().any(char::is_whitespace) {
        return Err("URL must be non-empty and contain no whitespace.".into());
    }
    let parsed = Url::parse(trimmed).map_err(|err| format!("Invalid URL: {err}"))?;
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("URL must not include embedded credentials.".into());
    }
    if parsed.fragment().is_some() {
        return Err("URL must not include a fragment.".into());
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| "URL must include a hostname.".to_string())?;
    let is_loopback_host = host.eq_ignore_ascii_case("localhost")
        || host == "127.0.0.1"
        || host == "::1"
        || host == "[::1]";

    match parsed.scheme() {
        "https" => {
            if !allow_loopback_http && is_forbidden_host(host) {
                return Err("Private, loopback, or link-local hosts are not permitted.".into());
            }
        }
        "http" if allow_loopback_http && is_loopback_host => {}
        _ => {
            return Err("Only HTTPS endpoints are permitted.".into());
        }
    }

    if !is_loopback_host && is_forbidden_host(host) {
        return Err("Private or link-local hosts are not permitted.".into());
    }

    Ok(parsed)
}

fn is_forbidden_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
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
        IpAddr::V6(v6) => {
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
                || embedded_v4.is_some_and(|v4| is_forbidden_ip(IpAddr::V4(v4)))
        }
    }
}

fn is_forbidden_host(host: &str) -> bool {
    let clean = host.trim_start_matches('[').trim_end_matches(']');
    if clean.eq_ignore_ascii_case("localhost")
        || clean.ends_with(".localhost")
        || clean.ends_with(".local")
        || clean.ends_with(".internal")
        || clean.eq_ignore_ascii_case("metadata.google.internal")
    {
        return true;
    }
    if let Ok(ip) = clean.parse::<IpAddr>() {
        return is_forbidden_ip(ip);
    }
    false
}

/// Resolves DNS for `url` before making an outbound HTTP request and rejects
/// any private, loopback, link-local, or cloud-metadata resolved address when
/// `allow_loopback` is `false` (DNS rebinding SSRF defense).
async fn verify_dns_ssrf_safe(url: &Url, allow_loopback: bool) -> Result<(), String> {
    let Some(host) = url.host_str() else {
        return Err("Outbound URL missing hostname.".into());
    };
    let port = url.port_or_known_default().unwrap_or(443);
    if allow_loopback
        && (host.eq_ignore_ascii_case("localhost")
            || host == "127.0.0.1"
            || host == "::1"
            || host == "[::1]")
    {
        return Ok(());
    }
    let clean_host = host.trim_start_matches('[').trim_end_matches(']');
    if let Ok(ip) = clean_host.parse::<IpAddr>() {
        if is_forbidden_ip(ip) {
            return Err("Private, loopback, or link-local IP addresses are not permitted.".into());
        }
        return Ok(());
    }
    let addrs = tokio::net::lookup_host((clean_host, port))
        .await
        .map_err(|err| format!("DNS resolution failed for outbound host: {err}"))?;
    let mut saw_any = false;
    for sock in addrs {
        saw_any = true;
        if is_forbidden_ip(sock.ip()) {
            return Err(
                "Resolved destination address falls in a forbidden private/loopback/link-local range."
                    .into(),
            );
        }
    }
    if !saw_any {
        return Err("DNS resolution returned no addresses for outbound host.".into());
    }
    Ok(())
}

#[derive(Clone)]
struct EhrRoiState {
    http: reqwest::Client,
    webhook: Option<RoiWebhookConfig>,
    allow_loopback_discovery: bool,
}

#[derive(Deserialize)]
struct SmartDiscoverQuery {
    iss: String,
}

#[derive(Serialize)]
struct SmartDiscoverResponse {
    fhir_base_url: String,
    well_known_url: String,
    authorization_endpoint: String,
    token_endpoint: String,
    capabilities: Vec<String>,
}

#[derive(Deserialize)]
struct LiveRoiDispatchRequest {
    requests: Vec<RoiRequest>,
    #[serde(default)]
    dispatched_at: Option<String>,
    #[serde(default)]
    due_by_30d: Option<String>,
}

#[derive(Deserialize)]
struct ShlinkInspectRequest {
    uri: String,
    #[serde(default)]
    recipient: Option<String>,
    #[serde(default)]
    passcode: Option<String>,
    #[serde(default)]
    fetch_manifest: bool,
    #[serde(default)]
    now_epoch_secs: Option<u64>,
}

#[derive(Deserialize)]
struct KipuVerifyRequest {
    #[serde(default)]
    base_url: Option<String>,
    #[serde(default)]
    method: Option<String>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    access_id: Option<String>,
    #[serde(default)]
    secret_key: Option<String>,
    #[serde(default)]
    date: Option<String>,
    #[serde(default)]
    auth_scheme: KipuAuthScheme,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    live_probe: bool,
}

/// Builds the live SMART discovery, SHLink inspection, Kipu API v3 verification,
/// and automated ROI dispatch sub-router mounted under `/clinical/api`.
pub fn router(webhook: Option<RoiWebhookConfig>, allow_loopback_discovery: bool) -> Router {
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(12))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("dabney-moe-clinical/1.0 (+https://dabney.moe)")
        .build()
        .expect("build reqwest client for SMART/ROI");

    let state = Arc::new(EhrRoiState {
        http,
        webhook,
        allow_loopback_discovery,
    });

    Router::new()
        .route("/portals/smart-discover", get(smart_discover))
        .route("/ehr/shlink/inspect", post(shlink_inspect))
        .route("/ehr/kipu/verify", post(kipu_verify))
        .route("/roi/dispatch-live", post(roi_dispatch_live))
        .layer(RequestBodyLimitLayer::new(MAX_ROI_DISPATCH_BODY))
        .with_state(state)
}

fn json_no_store<T: Serialize>(status: StatusCode, body: &T) -> Response {
    let mut resp = (status, Json(body)).into_response();
    resp.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store, max-age=0"),
    );
    resp.headers_mut()
        .insert(header::VARY, HeaderValue::from_static("*"));
    resp
}

async fn smart_discover(
    State(state): State<Arc<EhrRoiState>>,
    Query(query): Query<SmartDiscoverQuery>,
) -> Response {
    let base = match validate_outbound_url(&query.iss, state.allow_loopback_discovery) {
        Ok(url) => url,
        Err(err) => {
            return json_no_store(
                StatusCode::BAD_REQUEST,
                &serde_json::json!({ "error": err }),
            )
        }
    };

    if let Err(err) = verify_dns_ssrf_safe(&base, state.allow_loopback_discovery).await {
        return json_no_store(
            StatusCode::BAD_REQUEST,
            &serde_json::json!({ "error": err }),
        );
    }

    let well_known_str = format!(
        "{}/.well-known/smart-configuration",
        base.as_str().trim_end_matches('/')
    );
    let resp = match state
        .http
        .get(&well_known_str)
        .header(header::ACCEPT, "application/json")
        .send()
        .await
    {
        Ok(r) => r,
        Err(err) => {
            return json_no_store(
                StatusCode::BAD_GATEWAY,
                &serde_json::json!({
                    "error": format!("Could not reach SMART discovery endpoint: {err}")
                }),
            );
        }
    };

    if !resp.status().is_success() {
        return json_no_store(
            StatusCode::BAD_GATEWAY,
            &serde_json::json!({
                "error": format!("SMART discovery endpoint returned HTTP {}", resp.status().as_u16())
            }),
        );
    }

    let bytes = match resp.bytes().await {
        Ok(b) if b.len() <= MAX_DISCOVERY_BYTES => b,
        Ok(_) => {
            return json_no_store(
                StatusCode::BAD_GATEWAY,
                &serde_json::json!({ "error": "SMART discovery document exceeded 128 KiB safety cap." }),
            );
        }
        Err(err) => {
            return json_no_store(
                StatusCode::BAD_GATEWAY,
                &serde_json::json!({ "error": format!("Failed reading SMART discovery body: {err}") }),
            );
        }
    };

    let doc: serde_json::Value = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(err) => {
            return json_no_store(
                StatusCode::BAD_GATEWAY,
                &serde_json::json!({ "error": format!("Invalid SMART discovery JSON: {err}") }),
            );
        }
    };

    let auth_ep = match doc
        .get("authorization_endpoint")
        .and_then(serde_json::Value::as_str)
        .map(|u| validate_outbound_url(u, state.allow_loopback_discovery))
    {
        Some(Ok(u)) => u.to_string(),
        _ => {
            return json_no_store(
                StatusCode::BAD_GATEWAY,
                &serde_json::json!({ "error": "SMART discovery JSON missing valid HTTPS authorization_endpoint." }),
            );
        }
    };

    let token_ep = match doc
        .get("token_endpoint")
        .and_then(serde_json::Value::as_str)
        .map(|u| validate_outbound_url(u, state.allow_loopback_discovery))
    {
        Some(Ok(u)) => u.to_string(),
        _ => {
            return json_no_store(
                StatusCode::BAD_GATEWAY,
                &serde_json::json!({ "error": "SMART discovery JSON missing valid HTTPS token_endpoint." }),
            );
        }
    };

    let capabilities = doc
        .get("capabilities")
        .and_then(serde_json::Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(serde_json::Value::as_str)
                .take(24)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();

    json_no_store(
        StatusCode::OK,
        &SmartDiscoverResponse {
            fhir_base_url: base.as_str().trim_end_matches('/').to_string(),
            well_known_url: well_known_str,
            authorization_endpoint: auth_ep,
            token_endpoint: token_ep,
            capabilities,
        },
    )
}

async fn roi_dispatch_live(
    State(state): State<Arc<EhrRoiState>>,
    Json(mut payload): Json<LiveRoiDispatchRequest>,
) -> Response {
    let dispatched_at = payload
        .dispatched_at
        .as_deref()
        .unwrap_or("2026-11-26T04:30:00Z");
    let due_by_30d = payload.due_by_30d.as_deref().unwrap_or("2026-12-26");

    let mut report = RoiBatchDispatchReport {
        total_queued: 0,
        dispatched_count: 0,
        skipped_or_failed_count: 0,
        items: Vec::new(),
    };

    let mode = if state.webhook.is_some() {
        "webhook_gateway"
    } else {
        "local_sandbox"
    };

    for req in &mut payload.requests {
        if !matches!(req.status, RoiStatus::SignedReadyToSend { .. }) {
            continue;
        }
        report.total_queued += 1;

        if let Err(err) = validate_roi_request(req) {
            report.skipped_or_failed_count += 1;
            report.items.push(RoiDispatchItemOutcome {
                request_id: req.id.clone(),
                clinic_name: req.clinic_name.clone(),
                dispatched: false,
                reference_id: None,
                detail: err,
            });
            continue;
        }

        if let Some(cfg) = &state.webhook {
            // Render the complete signed authorization packet and deliver to the
            // operator's configured email/e-fax relay webhook. Failure NEVER
            // marks the request as dispatched.
            let mut candidate = req.clone();
            let ref_id = match dispatch_roi_request(
                &mut candidate,
                dispatched_at,
                due_by_30d,
                "Dispatched via configured HIM automation webhook",
            ) {
                Ok(id) => id,
                Err(err) => {
                    report.skipped_or_failed_count += 1;
                    report.items.push(RoiDispatchItemOutcome {
                        request_id: req.id.clone(),
                        clinic_name: req.clinic_name.clone(),
                        dispatched: false,
                        reference_id: None,
                        detail: err,
                    });
                    continue;
                }
            };

            let envelope = serde_json::json!({
                "reference_id": ref_id,
                "due_by_30d": due_by_30d,
                "request": candidate,
                "cover_text": render_roi_cover_text(&candidate),
                "form_html": render_roi_form_html(&candidate),
            });

            let body = match serde_json::to_vec(&envelope) {
                Ok(b) => b,
                Err(err) => {
                    report.skipped_or_failed_count += 1;
                    report.items.push(RoiDispatchItemOutcome {
                        request_id: req.id.clone(),
                        clinic_name: req.clinic_name.clone(),
                        dispatched: false,
                        reference_id: None,
                        detail: format!("Failed to encode ROI dispatch payload: {err}"),
                    });
                    continue;
                }
            };

            let mut http_req = state
                .http
                .post(cfg.webhook_url().clone())
                .header(header::CONTENT_TYPE, "application/json")
                .body(body);
            if let Some(tok) = &cfg.bearer_token {
                http_req = http_req.bearer_auth(tok);
            }

            match http_req.send().await {
                Ok(resp) if resp.status().is_success() => {
                    *req = candidate;
                    report.dispatched_count += 1;
                    report.items.push(RoiDispatchItemOutcome {
                        request_id: req.id.clone(),
                        clinic_name: req.clinic_name.clone(),
                        dispatched: true,
                        reference_id: Some(ref_id.clone()),
                        detail: format!(
                            "Delivered to HIM automation gateway (HTTP {} · Ref {ref_id} · 30d due {due_by_30d}).",
                            resp.status().as_u16()
                        ),
                    });
                }
                Ok(resp) => {
                    report.skipped_or_failed_count += 1;
                    report.items.push(RoiDispatchItemOutcome {
                        request_id: req.id.clone(),
                        clinic_name: req.clinic_name.clone(),
                        dispatched: false,
                        reference_id: None,
                        detail: format!(
                            "HIM automation gateway rejected dispatch (HTTP {}); request remains signed & queued for retry.",
                            resp.status().as_u16()
                        ),
                    });
                }
                Err(err) => {
                    report.skipped_or_failed_count += 1;
                    report.items.push(RoiDispatchItemOutcome {
                        request_id: req.id.clone(),
                        clinic_name: req.clinic_name.clone(),
                        dispatched: false,
                        reference_id: None,
                        detail: format!(
                            "HIM automation gateway unreachable ({err}); request remains signed & queued for retry."
                        ),
                    });
                }
            }
        } else {
            match dispatch_roi_request(
                req,
                dispatched_at,
                due_by_30d,
                "Local sandbox dispatch (configure ROI_DISPATCH_WEBHOOK_URL for live e-fax/email relay)",
            ) {
                Ok(ref_id) => {
                    report.dispatched_count += 1;
                    report.items.push(RoiDispatchItemOutcome {
                        request_id: req.id.clone(),
                        clinic_name: req.clinic_name.clone(),
                        dispatched: true,
                        reference_id: Some(ref_id.clone()),
                        detail: format!(
                            "Sandbox dispatched via {} (Ref {ref_id} · 30-day statutory due date {due_by_30d}).",
                            req.delivery_channel.label()
                        ),
                    });
                }
                Err(err) => {
                    report.skipped_or_failed_count += 1;
                    report.items.push(RoiDispatchItemOutcome {
                        request_id: req.id.clone(),
                        clinic_name: req.clinic_name.clone(),
                        dispatched: false,
                        reference_id: None,
                        detail: err,
                    });
                }
            }
        }
    }

    json_no_store(
        StatusCode::OK,
        &serde_json::json!({
            "mode": mode,
            "report": report,
            "requests": payload.requests,
        }),
    )
}

/// `POST /clinical/api/ehr/shlink/inspect`
///
/// Validates a SMART Health Link (`shlink:/...` or viewer-wrapped `#shlink:/...`)
/// URI with strict HTTPS-only SSRF enforcement and exact 32-byte key check.
/// Never echoes back the decryption `key` or `passcode`. When `fetch_manifest`
/// is `true`, performs an SSRF-verified manifest `POST` and validates every
/// returned `files[].location` URL before summarizing.
async fn shlink_inspect(
    State(state): State<Arc<EhrRoiState>>,
    Json(req): Json<ShlinkInspectRequest>,
) -> Response {
    let payload = match parse_smart_health_link(&req.uri, req.now_epoch_secs) {
        Ok(p) => p,
        Err(err) => {
            return json_no_store(
                StatusCode::BAD_REQUEST,
                &serde_json::json!({ "error": err }),
            );
        }
    };

    // Re-validate manifest URL with server-side SSRF rules (strictly HTTPS-only;
    // SHLink never permits plain HTTP or private/loopback targets).
    let manifest_url = match validate_outbound_url(&payload.url, false) {
        Ok(u) => u,
        Err(err) => {
            return json_no_store(
                StatusCode::BAD_REQUEST,
                &serde_json::json!({ "error": err }),
            );
        }
    };

    let mut response = serde_json::json!({
        "valid": true,
        "version": payload.v,
        "manifest_host": payload.host(),
        "manifest_url": payload.url,
        "key_bytes": 32,
        "key_valid": true,
        "exp": payload.exp,
        "flag": payload.flag,
        "flags": payload.flags,
        "label": payload.label,
        "viewer_origin": payload.viewer_origin,
        "passcode_required": payload.requires_passcode(),
        "direct_file_url": payload.is_direct_file_url(),
        "long_term": payload.is_long_term(),
        "mode": if req.fetch_manifest { "live_manifest_fetch" } else { "offline_inspect" },
    });

    if !req.fetch_manifest {
        return json_no_store(StatusCode::OK, &response);
    }

    let recipient = req
        .recipient
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("Dabney.moe Clinical Viewer");
    let post_body = match build_shlink_manifest_request_body(
        &payload,
        recipient,
        req.passcode.as_deref(),
        Some(65_536),
    ) {
        Ok(b) => b,
        Err(err) => {
            return json_no_store(
                StatusCode::BAD_REQUEST,
                &serde_json::json!({ "error": err }),
            );
        }
    };

    if let Err(err) = verify_dns_ssrf_safe(&manifest_url, false).await {
        return json_no_store(
            StatusCode::BAD_REQUEST,
            &serde_json::json!({ "error": err }),
        );
    }

    let body_bytes = match serde_json::to_vec(&post_body) {
        Ok(b) => b,
        Err(err) => {
            return json_no_store(
                StatusCode::BAD_REQUEST,
                &serde_json::json!({ "error": format!("Failed to encode manifest request: {err}") }),
            );
        }
    };

    let resp = match state
        .http
        .post(manifest_url)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, "application/json")
        .body(body_bytes)
        .send()
        .await
    {
        Ok(r) => r,
        Err(err) => {
            return json_no_store(
                StatusCode::BAD_GATEWAY,
                &serde_json::json!({
                    "error": format!("Could not reach SMART Health Link manifest server: {err}")
                }),
            );
        }
    };

    if resp.status() == StatusCode::UNAUTHORIZED {
        return json_no_store(
            StatusCode::UNAUTHORIZED,
            &serde_json::json!({ "error": "SMART Health Link manifest rejected passcode (HTTP 401)." }),
        );
    }
    if !resp.status().is_success() {
        return json_no_store(
            StatusCode::BAD_GATEWAY,
            &serde_json::json!({
                "error": format!("SMART Health Link manifest server returned HTTP {}.", resp.status().as_u16())
            }),
        );
    }

    let bytes = match resp.bytes().await {
        Ok(b) if b.len() <= MAX_DISCOVERY_BYTES => b,
        Ok(_) => {
            return json_no_store(
                StatusCode::BAD_GATEWAY,
                &serde_json::json!({ "error": "SMART Health Link manifest exceeded 128 KiB safety cap." }),
            );
        }
        Err(err) => {
            return json_no_store(
                StatusCode::BAD_GATEWAY,
                &serde_json::json!({ "error": format!("Failed reading SMART Health Link manifest: {err}") }),
            );
        }
    };

    let manifest_doc: serde_json::Value = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(err) => {
            return json_no_store(
                StatusCode::BAD_GATEWAY,
                &serde_json::json!({ "error": format!("Invalid SMART Health Link manifest JSON: {err}") }),
            );
        }
    };

    let status = manifest_doc
        .get("status")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown");
    let mut sanitized_files = Vec::new();
    if let Some(files) = manifest_doc
        .get("files")
        .and_then(serde_json::Value::as_array)
    {
        for f in files.iter().take(32) {
            let content_type = f
                .get("contentType")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("application/octet-stream");
            let has_embedded = f
                .get("embedded")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|s| !s.is_empty());
            let location_host = if let Some(loc) =
                f.get("location").and_then(serde_json::Value::as_str)
            {
                // Enforce HTTPS-only, port-443-only, and full SSRF guard on every manifest file URL.
                match validate_public_https_url(loc)
                    .and_then(|clean| validate_outbound_url(&clean, false))
                {
                    Ok(valid_loc) => Some(valid_loc.host_str().unwrap_or("unknown").to_string()),
                    Err(err) => {
                        return json_no_store(
                            StatusCode::BAD_GATEWAY,
                            &serde_json::json!({
                                "error": format!("Manifest contained an unsafe file location URL: {err}")
                            }),
                        );
                    }
                }
            } else {
                None
            };
            sanitized_files.push(serde_json::json!({
                "content_type": content_type,
                "has_embedded": has_embedded,
                "location_host": location_host,
            }));
        }
    }

    response["manifest"] = serde_json::json!({
        "status": status,
        "file_count": sanitized_files.len(),
        "files": sanitized_files,
    });
    json_no_store(StatusCode::OK, &response)
}

/// `POST /clinical/api/ehr/kipu/verify`
///
/// Builds and verifies a Kipu Health API v3 (`APIAuth` HMAC-SHA1 / HMAC-SHA256)
/// or API v4 (`Bearer`) signed request envelope with strict SSRF protection
/// (fail-closed on private/loopback hosts in production).
///
/// When `live_probe` is `false` or credentials are omitted, operates in
/// deterministic zero-PHI sandbox envelope mode with zero outbound network calls
/// and redacted secret output.
async fn kipu_verify(
    State(state): State<Arc<EhrRoiState>>,
    Json(req): Json<KipuVerifyRequest>,
) -> Response {
    let base_raw = req
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(KIPU_DEFAULT_BASE_URL);

    let validated_base = match validate_outbound_url(base_raw, state.allow_loopback_discovery) {
        Ok(u) => u,
        Err(err) => {
            return json_no_store(
                StatusCode::BAD_REQUEST,
                &serde_json::json!({ "error": err }),
            );
        }
    };

    let method = req
        .method
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("GET");
    let path = req
        .path
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("/api/patients/census?app_id=dabney-demo&phi_level=low");
    let date = req
        .date
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("Wed, 26 Nov 2026 04:30:00 GMT");

    let has_user_secret = req
        .secret_key
        .as_deref()
        .map(str::trim)
        .is_some_and(|s| !s.is_empty());
    let access_id = req
        .access_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("dabney-zero-phi-sandbox");
    let secret = req
        .secret_key
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("dabney-zero-phi-sandbox-key");

    let signed = match sign_kipu_request(
        validated_base.as_str(),
        method,
        path,
        date,
        req.body.as_ref().map(|_| "application/json"),
        req.body.as_deref().map(str::as_bytes),
        req.auth_scheme,
        access_id,
        secret,
        state.allow_loopback_discovery,
    ) {
        Ok(s) => s,
        Err(err) => {
            return json_no_store(
                StatusCode::BAD_REQUEST,
                &serde_json::json!({ "error": err }),
            );
        }
    };

    let mut summary = serde_json::json!({
        "mode": if req.live_probe && has_user_secret { "live_probe" } else { "sandbox_envelope" },
        "method": signed.method,
        "base_url": signed.base_url,
        "request_uri": signed.request_uri,
        "auth_scheme": signed.auth_scheme,
        "accept": signed.accept,
        "date": signed.date,
        "content_type": signed.content_type,
        "content_md5": signed.content_md5,
        "canonical_string": signed.canonical_string,
        "authorization_redacted": signed.redacted_authorization(),
        "part2_protected": signed.part2_protected,
    });

    if !req.live_probe || !has_user_secret {
        return json_no_store(StatusCode::OK, &summary);
    }

    // Security invariant: never act as an unauthenticated arbitrary-method outbound
    // HTTP proxy. Live probes are strictly read-only (`GET` / `HEAD`, no request body)
    // and restricted to the explicit Kipu host allowlist (`api.kipuworks.com` /
    // `*.kipuworks.com` on HTTPS port 443), with loopback permitted only in local
    // non-production dev/test mode (`allow_loopback_discovery == true`).
    if !matches!(signed.method.as_str(), "GET" | "HEAD") {
        return json_no_store(
            StatusCode::BAD_REQUEST,
            &serde_json::json!({
                "error": "Live Kipu probes are restricted to read-only GET or HEAD requests."
            }),
        );
    }
    if req.body.as_deref().is_some_and(|b| !b.is_empty()) {
        return json_no_store(
            StatusCode::BAD_REQUEST,
            &serde_json::json!({
                "error": "Live Kipu probes do not accept a request body."
            }),
        );
    }

    let target_url = match Url::parse(&signed.full_url) {
        Ok(u) => u,
        Err(err) => {
            return json_no_store(
                StatusCode::BAD_REQUEST,
                &serde_json::json!({ "error": format!("Invalid Kipu target URL: {err}") }),
            );
        }
    };
    if !is_allowed_kipu_live_probe_target(&target_url, state.allow_loopback_discovery) {
        return json_no_store(
            StatusCode::BAD_REQUEST,
            &serde_json::json!({
                "error": "Live Kipu probes are restricted to HTTPS port 443 on `api.kipuworks.com` / `*.kipuworks.com`."
            }),
        );
    }
    if let Err(err) = verify_dns_ssrf_safe(&target_url, state.allow_loopback_discovery).await {
        return json_no_store(
            StatusCode::BAD_REQUEST,
            &serde_json::json!({ "error": err }),
        );
    }

    let req_method = match signed.method.as_str() {
        "HEAD" => reqwest::Method::HEAD,
        _ => reqwest::Method::GET,
    };
    let mut http_req = state
        .http
        .request(req_method, target_url)
        .header(header::ACCEPT, &signed.accept)
        .header(header::DATE, &signed.date)
        .header(header::AUTHORIZATION, &signed.authorization);
    if !signed.content_type.is_empty() {
        http_req = http_req.header(header::CONTENT_TYPE, &signed.content_type);
    }
    if !signed.content_md5.is_empty() {
        http_req = http_req.header("content-md5", &signed.content_md5);
    }

    match http_req.send().await {
        Ok(resp) => {
            summary["probe_status"] = serde_json::json!(resp.status().as_u16());
            summary["probe_ok"] = serde_json::json!(resp.status().is_success());
            json_no_store(StatusCode::OK, &summary)
        }
        Err(err) => json_no_store(
            StatusCode::BAD_GATEWAY,
            &serde_json::json!({
                "error": format!("Could not reach Kipu API endpoint: {err}")
            }),
        ),
    }
}

fn is_allowed_kipu_live_probe_target(url: &Url, allow_loopback: bool) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    let lower = host.to_ascii_lowercase();
    if allow_loopback
        && (lower == "127.0.0.1" || lower == "localhost" || lower == "::1" || lower == "[::1]")
    {
        return matches!(url.scheme(), "http" | "https");
    }
    url.scheme() == "https"
        && url.port_or_known_default() == Some(443)
        && (lower == "api.kipuworks.com" || lower.ends_with(".kipuworks.com"))
}
