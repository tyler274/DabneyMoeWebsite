//! Same-origin streaming reverse proxy for `/clinical/api/*`.
//!
//! When `CLINICAL_API_URL` is configured, requests under `/clinical/api` are
//! forwarded to the external clinical API backend, preserving
//! `Authorization: Bearer`, `X-Forwarded-For`, and unbuffered
//! `text/event-stream` responses while stripping cookies and `x-goog-*`
//! headers, enforcing `Origin` + `Sec-Fetch-Site` CSRF checks on non-safe
//! methods, and forcing `private, no-store, max-age=0` + `vary: *` cache
//! headers.
//!
//! When `CLINICAL_API_URL` is unset, `web::mount_clinical_api_routes` mounts
//! the embedded `clinical_workspace::api::router()` directly at
//! `/clinical/api`.

use axum::body::{to_bytes, Body};
use axum::extract::{Request, State};
use axum::http::{header, HeaderMap, HeaderName, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use axum::Router;
use futures_util::stream::unfold;
use reqwest::Client;
use std::collections::HashSet;
use std::fmt;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;
use tower_http::limit::RequestBodyLimitLayer;
use url::Url;

/// URL prefix mounted on `dabney.moe` for clinical workspace API + SSE traffic.
pub const CLINICAL_API_PREFIX: &str = "/clinical/api";

/// Maximum request body size (`8 MiB`) accepted on `/clinical/api/*`.
pub const CLINICAL_API_BODY_LIMIT: usize = 8 * 1024 * 1024;

/// Configuration validation failure for [`ClinicalApiProxyConfig`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClinicalProxyConfigError(&'static str);

impl fmt::Display for ClinicalProxyConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for ClinicalProxyConfigError {}

/// Validated startup configuration for the `/clinical/api` proxy mount.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClinicalApiProxyConfig {
    upstream: Url,
    expected_origin: Option<String>,
}

impl ClinicalApiProxyConfig {
    /// Builds a validated proxy configuration from explicit upstream and public-origin strings.
    pub fn new(
        upstream: &str,
        expected_origin: Option<&str>,
        deployed: bool,
    ) -> Result<Self, ClinicalProxyConfigError> {
        let upstream = validate_origin(upstream, deployed)?;
        let expected_origin = match expected_origin.map(str::trim).filter(|s| !s.is_empty()) {
            Some(origin) => Some(
                validate_origin(origin, deployed)?
                    .origin()
                    .ascii_serialization(),
            ),
            None if deployed => Some("https://dabney.moe".to_string()),
            None => None,
        };
        Ok(Self {
            upstream,
            expected_origin,
        })
    }

    /// Reads `CLINICAL_API_URL` and optional `CLINICAL_PUBLIC_ORIGIN` from `lookup`.
    /// Returns `Ok(None)` when `CLINICAL_API_URL` is unset or blank.
    pub fn from_env(
        lookup: impl Fn(&str) -> Option<String>,
    ) -> Result<Option<Self>, ClinicalProxyConfigError> {
        let Some(raw_upstream) = lookup("CLINICAL_API_URL") else {
            return Ok(None);
        };
        let trimmed = raw_upstream.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }
        let deployed = lookup("K_SERVICE").is_some_and(|v| !v.trim().is_empty())
            || lookup("LEPTOS_ENV").is_some_and(|v| v.trim().eq_ignore_ascii_case("PROD"));
        let public_origin = lookup("CLINICAL_PUBLIC_ORIGIN");
        Self::new(trimmed, public_origin.as_deref(), deployed).map(Some)
    }

    /// Validated upstream base origin URL.
    pub fn upstream(&self) -> &Url {
        &self.upstream
    }

    /// Expected `Origin` header for non-safe requests, if enforced.
    pub fn expected_origin(&self) -> Option<&str> {
        self.expected_origin.as_deref()
    }
}

fn validate_origin(raw: &str, deployed: bool) -> Result<Url, ClinicalProxyConfigError> {
    let parsed = Url::parse(raw)
        .map_err(|_| ClinicalProxyConfigError("proxy URL must be a valid absolute origin"))?;
    let is_loopback = parsed.host_str().is_some_and(|host| {
        host.eq_ignore_ascii_case("localhost")
            || host
                .trim_matches(['[', ']'])
                .parse::<IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
    });
    let scheme_ok =
        parsed.scheme() == "https" || (!deployed && is_loopback && parsed.scheme() == "http");
    if raw.trim() != raw
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || parsed.path() != "/"
        || parsed.host_str().is_none()
        || !scheme_ok
    {
        return Err(ClinicalProxyConfigError(
            "proxy URL must be a credential-free HTTPS origin (or loopback HTTP locally)",
        ));
    }
    Ok(parsed)
}

#[derive(Clone)]
struct ClinicalProxyState {
    config: ClinicalApiProxyConfig,
    client: Client,
}

/// Builds a router serving `/clinical/api`, `/clinical/api/`, and
/// `/clinical/api/{*rest}` with its own `8 MiB` body-size limit.
pub fn clinical_api_proxy_router<S>(config: ClinicalApiProxyConfig) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    let client = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(5))
        .build()
        .expect("reqwest rustls client");
    let state = Arc::new(ClinicalProxyState { config, client });
    Router::new()
        .route(CLINICAL_API_PREFIX, any(proxy_request))
        .route("/clinical/api/", any(proxy_request))
        .route("/clinical/api/{*rest}", any(proxy_request))
        .layer(RequestBodyLimitLayer::new(CLINICAL_API_BODY_LIMIT))
        .with_state(state)
}

async fn proxy_request(State(state): State<Arc<ClinicalProxyState>>, req: Request) -> Response {
    let method = req.method().clone();
    if method == Method::CONNECT || method == Method::TRACE {
        return empty_status(StatusCode::METHOD_NOT_ALLOWED);
    }
    if !matches!(method, Method::GET | Method::HEAD | Method::OPTIONS)
        && !origin_allowed(req.headers(), state.config.expected_origin.as_deref())
    {
        return empty_status(StatusCode::FORBIDDEN);
    }

    let Some(target_url) = build_upstream_url(&state.config.upstream, req.uri()) else {
        return empty_status(StatusCode::BAD_REQUEST);
    };

    let outbound_headers = filter_request_headers(req.headers());
    let body_bytes = match to_bytes(req.into_body(), CLINICAL_API_BODY_LIMIT).await {
        Ok(bytes) => bytes,
        Err(_) => return empty_status(StatusCode::PAYLOAD_TOO_LARGE),
    };

    let reqwest_method = match reqwest::Method::from_bytes(method.as_str().as_bytes()) {
        Ok(m) => m,
        Err(_) => return empty_status(StatusCode::BAD_REQUEST),
    };

    let mut upstream_req = state.client.request(reqwest_method, target_url);
    for (name, value) in &outbound_headers {
        upstream_req = upstream_req.header(name.as_str(), value.as_bytes());
    }
    if !body_bytes.is_empty() {
        upstream_req = upstream_req.body(body_bytes);
    }

    let upstream_resp = match upstream_req.send().await {
        Ok(resp) => resp,
        Err(err) => {
            tracing::warn!(error = %err, "clinical api upstream request failed");
            return empty_status(StatusCode::BAD_GATEWAY);
        }
    };

    let status =
        StatusCode::from_u16(upstream_resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let resp_headers = filter_response_headers(upstream_resp.headers());
    let stream = unfold(upstream_resp, |mut resp| async move {
        match resp.chunk().await {
            Ok(Some(chunk)) => Some((Ok::<_, std::io::Error>(chunk), resp)),
            Ok(None) => None,
            Err(err) => Some((Err(std::io::Error::other(err)), resp)),
        }
    });

    let mut response = Response::new(Body::from_stream(stream));
    *response.status_mut() = status;
    *response.headers_mut() = resp_headers;
    response
}

fn origin_allowed(headers: &HeaderMap, expected_origin: Option<&str>) -> bool {
    if headers
        .get("sec-fetch-site")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.eq_ignore_ascii_case("cross-site"))
    {
        return false;
    }
    let Some(expected) = expected_origin else {
        return true;
    };
    let mut origins = headers.get_all(header::ORIGIN).iter();
    let Some(first) = origins.next() else {
        return false;
    };
    if origins.next().is_some() {
        return false;
    }
    first.to_str().ok() == Some(expected)
}

fn build_upstream_url(base: &Url, uri: &axum::http::Uri) -> Option<Url> {
    let raw_path = uri.path();
    let rest = raw_path.strip_prefix(CLINICAL_API_PREFIX)?;
    if !rest.is_empty() && !rest.starts_with('/') {
        return None;
    }
    if rest.contains('\\') || has_dot_segment(rest) {
        return None;
    }
    let mut out = base.clone();
    out.set_path(if rest.is_empty() { "/" } else { rest });
    out.set_query(uri.query());
    Some(out)
}

fn has_dot_segment(path: &str) -> bool {
    path.split('/').any(|seg| {
        let decoded = decode_ascii_percent(seg);
        seg == "."
            || seg == ".."
            || decoded == "."
            || decoded == ".."
            || decoded.contains('/')
            || decoded.contains('\\')
    })
}

fn decode_ascii_percent(seg: &str) -> String {
    let bytes = seg.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(hex) = std::str::from_utf8(&bytes[i + 1..i + 3]) {
                if let Ok(b) = u8::from_str_radix(hex, 16) {
                    out.push(b);
                    i += 3;
                    continue;
                }
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn connection_listed_headers<'a>(values: impl Iterator<Item = &'a str>) -> HashSet<String> {
    let mut listed = HashSet::new();
    for raw in values {
        for token in raw.split(',') {
            let trimmed = token.trim();
            if !trimmed.is_empty() {
                listed.insert(trimmed.to_ascii_lowercase());
            }
        }
    }
    listed
}

fn is_hop_by_hop_or_stripped_req(name: &str, connection_tokens: &HashSet<String>) -> bool {
    matches!(
        name,
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
            | "host"
            | "cookie"
            | "content-length"
    ) || name.starts_with("x-goog-")
        || connection_tokens.contains(name)
}

fn filter_request_headers(headers: &HeaderMap) -> HeaderMap {
    let connection_tokens = connection_listed_headers(
        headers
            .get_all(header::CONNECTION)
            .iter()
            .filter_map(|v| v.to_str().ok()),
    );
    let mut out = HeaderMap::new();
    for (name, value) in headers {
        let lower = name.as_str().to_ascii_lowercase();
        if !is_hop_by_hop_or_stripped_req(&lower, &connection_tokens) {
            out.append(name.clone(), value.clone());
        }
    }
    out
}

fn filter_response_headers(headers: &reqwest::header::HeaderMap) -> HeaderMap {
    let connection_tokens = connection_listed_headers(
        headers
            .get_all(reqwest::header::CONNECTION)
            .iter()
            .filter_map(|v| v.to_str().ok()),
    );
    let mut out = HeaderMap::new();
    for (name, value) in headers {
        let lower = name.as_str().to_ascii_lowercase();
        if matches!(
            lower.as_str(),
            "connection"
                | "keep-alive"
                | "proxy-authenticate"
                | "proxy-authorization"
                | "te"
                | "trailer"
                | "transfer-encoding"
                | "upgrade"
                | "set-cookie"
                | "cache-control"
                | "vary"
        ) || connection_tokens.contains(&lower)
        {
            continue;
        }
        if let (Ok(hn), Ok(hv)) = (
            HeaderName::from_bytes(name.as_str().as_bytes()),
            HeaderValue::from_bytes(value.as_bytes()),
        ) {
            out.append(hn, hv);
        }
    }
    out.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store, max-age=0"),
    );
    out.insert(header::VARY, HeaderValue::from_static("*"));
    out
}

fn empty_status(status: StatusCode) -> Response {
    (
        status,
        [
            (
                header::CACHE_CONTROL,
                HeaderValue::from_static("private, no-store, max-age=0"),
            ),
            (header::VARY, HeaderValue::from_static("*")),
        ],
    )
        .into_response()
}
