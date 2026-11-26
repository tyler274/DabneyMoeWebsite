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
    dispatch_roi_request, render_roi_cover_text, render_roi_form_html, validate_roi_request,
    RoiBatchDispatchReport, RoiDispatchItemOutcome, RoiRequest, RoiStatus,
};
use serde::{Deserialize, Serialize};
use tower_http::limit::RequestBodyLimitLayer;
use url::Url;

const MAX_DISCOVERY_BYTES: usize = 128 * 1024;
const MAX_ROI_DISPATCH_BODY: usize = 256 * 1024;

/// Optional operator configuration for automated server-side ROI dispatch
/// (for example an e-fax / HIM email relay webhook).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoiWebhookConfig {
    webhook_url: Url,
    bearer_token: Option<String>,
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

fn is_forbidden_host(host: &str) -> bool {
    let clean = host.trim_start_matches('[').trim_end_matches(']');
    if clean.eq_ignore_ascii_case("localhost")
        || clean.ends_with(".local")
        || clean.ends_with(".internal")
        || clean.eq_ignore_ascii_case("metadata.google.internal")
    {
        return true;
    }
    if let Ok(ip) = clean.parse::<IpAddr>() {
        return match ip {
            IpAddr::V4(v4) => {
                v4.is_private()
                    || v4.is_loopback()
                    || v4.is_link_local()
                    || v4.is_broadcast()
                    || v4.is_unspecified()
                    || (v4.octets()[0] == 100 && (v4.octets()[1] & 0b1100_0000) == 0b0100_0000)
            }
            IpAddr::V6(v6) => {
                v6.is_loopback()
                    || v6.is_unspecified()
                    || v6.is_unique_local()
                    || v6.is_unicast_link_local()
            }
        };
    }
    false
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

/// Builds the live SMART discovery & automated ROI dispatch sub-router mounted
/// under `/clinical/api`.
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
