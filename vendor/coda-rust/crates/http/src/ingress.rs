//! Shared HTTP ingress abuse protection: per-client rate limiting, concurrency
//! caps, and request body-size limits for Coda's public-facing backends.
//!
//! Designed for Cloud Run / GCP External ALB deployments (`coda.dabney.moe`):
//! - Extracts the client IP from the rightmost untrusted `X-Forwarded-For` hop
//!   (never the attacker-spoofable leftmost entry), falling back to socket peer
//!   `ConnectInfo<SocketAddr>`.
//! - Bounds in-memory client tracking to `max_tracked_clients` so distributed
//!   IP floods cannot OOM a container.
//! - Exempts health probes (`/health`, `/healthz`, `/readyz`, `/api/health`)
//!   from rate and concurrency limits so Cloud Run liveness checks stay healthy
//!   under attack.
//! - Rejects oversized `Content-Length` headers immediately with `413 Payload
//!   Too Large` and installs `DefaultBodyLimit` for chunked transfers.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::Router;
use axum::extract::{ConnectInfo, DefaultBodyLimit, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use serde_json::json;
use tokio::sync::Semaphore;

/// Default general per-client request quota per 60-second window.
pub const DEFAULT_GENERAL_RPM: u32 = 240;

/// Default strict per-client request quota per 60-second window for unauthenticated
/// auth, magic-link, OTP, invitation, and sync endpoints.
pub const DEFAULT_AUTH_RPM: u32 = 20;

/// Default hard cap on distinct `(tier, client)` buckets kept in memory.
pub const DEFAULT_MAX_TRACKED_CLIENTS: usize = 4_096;

/// Default concurrent in-flight request limit per instance.
pub const DEFAULT_MAX_CONCURRENT_REQUESTS: usize = 64;

/// Default maximum request body size (`2 MiB`) when a server does not specify one.
pub const DEFAULT_MAX_BODY_BYTES: usize = 2 * 1024 * 1024;

/// Default number of trusted reverse-proxy hops appending to `X-Forwarded-For`
/// (Cloud Run / GCP External ALB appends the verified client IP as the rightmost hop).
pub const DEFAULT_TRUSTED_PROXY_HOPS: usize = 1;

/// Closed classification of an incoming HTTP route for rate limiting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RateLimitTier {
    /// Cloud Run / ALB health and readiness probes; never rate-limited.
    HealthProbe,
    /// Unauthenticated sign-in, magic-link, OTP, invite, rehearsal, or sync ingress.
    AuthSensitive,
    /// All other routes.
    Public,
}

impl RateLimitTier {
    /// Classifies `path` once at the HTTP ingress boundary.
    pub fn classify(path: &str) -> Self {
        let normalized = path.trim_end_matches('/');
        if matches!(
            normalized,
            "/health" | "/healthz" | "/readyz" | "/api/health"
        ) {
            return Self::HealthProbe;
        }
        if normalized == "/auth"
            || normalized.starts_with("/auth/")
            || normalized == "/physician/auth"
            || normalized.starts_with("/physician/auth/")
            || normalized == "/api/auth/email"
            || normalized.starts_with("/api/auth/email/")
            || normalized == "/api/auth/email-change"
            || normalized.starts_with("/api/auth/email-change/")
            || normalized == "/api/connected-care/sync"
            || normalized == "/api/connected-care/invitations"
            || normalized.starts_with("/api/connected-care/invitations/")
            || normalized == "/front-door/sign-in"
            || normalized.starts_with("/front-door/sign-in/")
            || normalized == "/front-door/invitations"
            || normalized.starts_with("/front-door/invitations/")
        {
            return Self::AuthSensitive;
        }
        Self::Public
    }

    /// Stable low-cardinality label for telemetry logs.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::HealthProbe => "health_probe",
            Self::AuthSensitive => "auth_sensitive",
            Self::Public => "public",
        }
    }
}

/// Fixed-window per-client request quota.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimitQuota {
    /// Maximum requests allowed per `window`.
    pub requests: u32,
    /// Duration of one quota window.
    pub window: Duration,
}

impl RateLimitQuota {
    /// Constructs a quota, clamping `requests` to at least `1` and `window` to at least 1s.
    pub fn per_window(requests: u32, window: Duration) -> Self {
        Self {
            requests: requests.max(1),
            window: window.max(Duration::from_secs(1)),
        }
    }

    /// Convenience constructor for a 60-second per-minute quota.
    pub fn per_minute(requests: u32) -> Self {
        Self::per_window(requests, Duration::from_secs(60))
    }
}

/// Whether direct loopback (`127.0.0.1` / `::1` without non-loopback `X-Forwarded-For`)
/// is exempt from per-IP rate limiting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopbackPolicy {
    /// Exempt direct loopback traffic (avoids flaking local multi-test browser suites).
    Exempt,
    /// Enforce quotas on every client including loopback.
    Enforced,
}

/// Typed rate-limiting policy for [`IngressGuard`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateLimitPolicy {
    /// Rate limiting disabled (only body-size and concurrency bounds apply).
    Disabled,
    /// Enforce per-client quotas with bounded memory.
    Enforced {
        /// Quota for [`RateLimitTier::Public`] requests.
        general: RateLimitQuota,
        /// Stricter quota for [`RateLimitTier::AuthSensitive`] requests.
        auth_sensitive: RateLimitQuota,
        /// Hard cap on tracked `(tier, client)` buckets.
        max_tracked_clients: usize,
        /// Loopback exemption policy.
        loopback: LoopbackPolicy,
    },
}

/// Configuration for [`IngressGuard`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IngressGuardConfig {
    /// Per-client rate-limit policy.
    pub rate_limit: RateLimitPolicy,
    /// Maximum allowed HTTP request body size in bytes (`413 Payload Too Large` above this).
    pub max_body_bytes: usize,
    /// Maximum concurrent in-flight non-probe requests (`503 Service Unavailable` above this).
    pub max_concurrent_requests: usize,
    /// Number of trusted reverse-proxy hops appending client IPs to `X-Forwarded-For`.
    pub trusted_proxy_hops: usize,
}

impl IngressGuardConfig {
    /// Standard production-safe defaults using `max_body_bytes`, honoring environment overrides:
    /// - `CODA_RATE_LIMIT_RPM` (default `240`; `0` disables rate limiting)
    /// - `CODA_AUTH_RATE_LIMIT_RPM` (default `20`)
    /// - `CODA_MAX_REQUEST_BODY_BYTES` (default `max_body_bytes`)
    /// - `CODA_MAX_CONCURRENT_REQUESTS` (default `64`)
    /// - `CODA_TRUSTED_PROXY_HOPS` (default `1`)
    /// - `CODA_RATE_LIMIT_ENFORCE_LOOPBACK=1` (or hosted `CODA_ENVIRONMENT`, `APP_ENV`,
    ///   `K_SERVICE`, or `CODA_HOSTED` non-empty) enforces on loopback too.
    pub fn from_env(
        lookup: impl Fn(&str) -> Option<String>,
        default_max_body_bytes: usize,
    ) -> Self {
        let general_rpm = parse_u32(&lookup, "CODA_RATE_LIMIT_RPM").unwrap_or(DEFAULT_GENERAL_RPM);
        let auth_rpm = parse_u32(&lookup, "CODA_AUTH_RATE_LIMIT_RPM")
            .unwrap_or(DEFAULT_AUTH_RPM)
            .max(1);
        let max_body_bytes = parse_usize(&lookup, "CODA_MAX_REQUEST_BODY_BYTES")
            .filter(|&n| n > 0)
            .unwrap_or(default_max_body_bytes.max(1024));
        let max_concurrent_requests = parse_usize(&lookup, "CODA_MAX_CONCURRENT_REQUESTS")
            .filter(|&n| n > 0)
            .unwrap_or(DEFAULT_MAX_CONCURRENT_REQUESTS);
        let trusted_proxy_hops =
            parse_usize(&lookup, "CODA_TRUSTED_PROXY_HOPS").unwrap_or(DEFAULT_TRUSTED_PROXY_HOPS);
        let hosted_env = ["CODA_ENVIRONMENT", "APP_ENV", "K_SERVICE", "CODA_HOSTED"]
            .into_iter()
            .any(|key| lookup(key).as_deref().is_some_and(|v| !v.trim().is_empty()));
        let enforce_loopback = matches!(
            lookup("CODA_RATE_LIMIT_ENFORCE_LOOPBACK").as_deref(),
            Some("1" | "true")
        ) || hosted_env;

        let rate_limit = if general_rpm == 0 {
            RateLimitPolicy::Disabled
        } else {
            RateLimitPolicy::Enforced {
                general: RateLimitQuota::per_minute(general_rpm),
                auth_sensitive: RateLimitQuota::per_minute(auth_rpm),
                max_tracked_clients: DEFAULT_MAX_TRACKED_CLIENTS,
                loopback: if enforce_loopback {
                    LoopbackPolicy::Enforced
                } else {
                    LoopbackPolicy::Exempt
                },
            }
        };

        Self {
            rate_limit,
            max_body_bytes,
            max_concurrent_requests,
            trusted_proxy_hops,
        }
    }

    /// Strict configuration enforcing rate limits on all clients including loopback.
    pub fn strict(
        general: RateLimitQuota,
        auth_sensitive: RateLimitQuota,
        max_body_bytes: usize,
        max_concurrent_requests: usize,
    ) -> Self {
        Self {
            rate_limit: RateLimitPolicy::Enforced {
                general,
                auth_sensitive,
                max_tracked_clients: DEFAULT_MAX_TRACKED_CLIENTS,
                loopback: LoopbackPolicy::Enforced,
            },
            max_body_bytes: max_body_bytes.max(1),
            max_concurrent_requests: max_concurrent_requests.max(1),
            trusted_proxy_hops: DEFAULT_TRUSTED_PROXY_HOPS,
        }
    }
}

fn parse_u32(lookup: &impl Fn(&str) -> Option<String>, key: &str) -> Option<u32> {
    lookup(key)?.trim().parse::<u32>().ok()
}

fn parse_usize(lookup: &impl Fn(&str) -> Option<String>, key: &str) -> Option<usize> {
    lookup(key)?.trim().parse::<usize>().ok()
}

/// Identified client key used to scope rate-limit buckets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClientKey {
    /// Canonical client IP address.
    Ip(IpAddr),
    /// Neither a valid `X-Forwarded-For` hop nor `ConnectInfo<SocketAddr>` was present.
    Unknown,
}

impl ClientKey {
    /// True only when the client resolved to a loopback IP (`127.0.0.1` or `::1`).
    pub fn is_loopback(self) -> bool {
        matches!(self, Self::Ip(ip) if ip.is_loopback())
    }

    /// Extracts the client key from `X-Forwarded-For` (selecting the `trusted_proxy_hops`-th
    /// entry from the right so attackers cannot spoof leftmost entries) or `peer_addr`.
    pub fn from_request(
        headers: &HeaderMap,
        peer_addr: Option<SocketAddr>,
        trusted_proxy_hops: usize,
    ) -> Self {
        if trusted_proxy_hops > 0 {
            let mut ips = Vec::new();
            for value in headers.get_all("x-forwarded-for") {
                if let Ok(forwarded) = value.to_str() {
                    ips.extend(
                        forwarded
                            .split(',')
                            .filter_map(|raw| raw.trim().parse::<IpAddr>().ok())
                            .map(|ip| ip.to_canonical()),
                    );
                }
            }
            if !ips.is_empty() {
                let index = ips.len().saturating_sub(trusted_proxy_hops);
                return Self::Ip(ips[index]);
            }
        }
        if let Some(addr) = peer_addr {
            return Self::Ip(addr.ip().to_canonical());
        }
        Self::Unknown
    }
}

/// Outcome of evaluating a request against [`IngressGuard`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IngressDecision {
    /// Request may proceed.
    Allowed,
    /// Per-client rate limit exceeded (`429 Too Many Requests`).
    RateLimited {
        /// Which tier rejected the request.
        tier: RateLimitTier,
        /// Whole seconds until the current window resets (always `>= 1`).
        retry_after_secs: u64,
    },
    /// Declared `Content-Length` exceeds `max_body_bytes` (`413 Payload Too Large`).
    PayloadTooLarge {
        /// Configured ceiling in bytes.
        max_bytes: usize,
    },
}

#[derive(Debug, Clone, Copy)]
struct WindowBucket {
    window_start: Instant,
    count: u32,
}

/// Shared, thread-safe HTTP ingress abuse-protection owner.
#[derive(Clone)]
pub struct IngressGuard {
    config: IngressGuardConfig,
    buckets: Arc<Mutex<HashMap<(RateLimitTier, ClientKey), WindowBucket>>>,
    permits: Arc<Semaphore>,
}

impl std::fmt::Debug for IngressGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IngressGuard")
            .field("config", &self.config)
            .finish()
    }
}

impl IngressGuard {
    /// Creates a new guard enforcing `config`.
    pub fn new(config: IngressGuardConfig) -> Self {
        Self {
            permits: Arc::new(Semaphore::new(config.max_concurrent_requests.max(1))),
            buckets: Arc::new(Mutex::new(HashMap::new())),
            config,
        }
    }

    /// Creates a guard configured from process environment variables.
    pub fn from_env(default_max_body_bytes: usize) -> Self {
        Self::new(IngressGuardConfig::from_env(
            |key| std::env::var(key).ok(),
            default_max_body_bytes,
        ))
    }

    /// Returns the active [`IngressGuardConfig`].
    pub const fn config(&self) -> IngressGuardConfig {
        self.config
    }

    /// Evaluates body-size and rate-limit rules for one request at `now`.
    pub fn check_at(
        &self,
        path: &str,
        headers: &HeaderMap,
        peer_addr: Option<SocketAddr>,
        now: Instant,
    ) -> IngressDecision {
        if let Some(len) = headers
            .get(header::CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.trim().parse::<usize>().ok())
            && len > self.config.max_body_bytes
        {
            return IngressDecision::PayloadTooLarge {
                max_bytes: self.config.max_body_bytes,
            };
        }

        let tier = RateLimitTier::classify(path);
        if tier == RateLimitTier::HealthProbe {
            return IngressDecision::Allowed;
        }

        let RateLimitPolicy::Enforced {
            general,
            auth_sensitive,
            max_tracked_clients,
            loopback,
        } = self.config.rate_limit
        else {
            return IngressDecision::Allowed;
        };

        let client = ClientKey::from_request(headers, peer_addr, self.config.trusted_proxy_hops);
        if loopback == LoopbackPolicy::Exempt && client.is_loopback() {
            return IngressDecision::Allowed;
        }

        let quota = match tier {
            RateLimitTier::AuthSensitive => auth_sensitive,
            RateLimitTier::Public => general,
            RateLimitTier::HealthProbe => return IngressDecision::Allowed,
        };

        let Ok(mut map) = self.buckets.lock() else {
            return IngressDecision::Allowed;
        };

        let key = (tier, client);
        if let Some(bucket) = map.get_mut(&key) {
            let elapsed = now.saturating_duration_since(bucket.window_start);
            if elapsed >= quota.window {
                bucket.window_start = now;
                bucket.count = 1;
                return IngressDecision::Allowed;
            }
            if bucket.count >= quota.requests {
                let remaining = quota.window.saturating_sub(elapsed);
                let retry_after_secs = remaining.as_secs().max(1);
                return IngressDecision::RateLimited {
                    tier,
                    retry_after_secs,
                };
            }
            bucket.count = bucket.count.saturating_add(1);
            return IngressDecision::Allowed;
        }

        let cap = max_tracked_clients.max(1);
        if map.len() >= cap {
            let max_window = general.window.max(auth_sensitive.window);
            map.retain(|_, bucket| now.saturating_duration_since(bucket.window_start) < max_window);
        }
        if map.len() >= cap
            && let Some(oldest_key) = map
                .iter()
                .min_by_key(|(_, bucket)| bucket.window_start)
                .map(|(k, _)| *k)
        {
            map.remove(&oldest_key);
        }

        map.insert(
            key,
            WindowBucket {
                window_start: now,
                count: 1,
            },
        );
        IngressDecision::Allowed
    }

    /// Evaluates body-size and rate-limit rules at `Instant::now()`.
    pub fn check(
        &self,
        path: &str,
        headers: &HeaderMap,
        peer_addr: Option<SocketAddr>,
    ) -> IngressDecision {
        self.check_at(path, headers, peer_addr, Instant::now())
    }

    /// Wraps `router` with both the ingress rate/concurrency/content-length guard
    /// and Axum's [`DefaultBodyLimit`] ceiling.
    pub fn protect(self, router: Router) -> Router {
        let max_body = self.config.max_body_bytes;
        router
            .layer(DefaultBodyLimit::max(max_body))
            .layer(middleware::from_fn_with_state(self, enforce_ingress))
    }
}

/// Renders a standard `429 Too Many Requests` response with `Retry-After` and `Cache-Control: no-store`.
pub fn rate_limited_response(tier: RateLimitTier, retry_after_secs: u64) -> Response {
    let retry = retry_after_secs.max(1);
    tracing::warn!(
        event = "http.ingress.rate_limited",
        tier = tier.as_str(),
        retry_after_secs = retry,
        "HTTP ingress rate limit exceeded"
    );
    let mut response = (
        StatusCode::TOO_MANY_REQUESTS,
        axum::Json(json!({
            "error": "rate_limited",
            "retry_after_seconds": retry,
        })),
    )
        .into_response();
    if let Ok(value) = HeaderValue::from_str(&retry.to_string()) {
        response.headers_mut().insert(header::RETRY_AFTER, value);
    }
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// Renders a standard `413 Payload Too Large` response with `Cache-Control: no-store`.
pub fn payload_too_large_response(max_bytes: usize) -> Response {
    tracing::warn!(
        event = "http.ingress.payload_too_large",
        max_bytes,
        "HTTP ingress rejected oversized request body"
    );
    let mut response = (
        StatusCode::PAYLOAD_TOO_LARGE,
        axum::Json(json!({
            "error": "payload_too_large",
            "max_bytes": max_bytes,
        })),
    )
        .into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// Renders a standard `503 Service Unavailable` response when concurrency permits are exhausted.
pub fn overloaded_response() -> Response {
    let mut response = (
        StatusCode::SERVICE_UNAVAILABLE,
        axum::Json(json!({
            "error": "server_overloaded",
            "retry_after_seconds": 1,
        })),
    )
        .into_response();
    response
        .headers_mut()
        .insert(header::RETRY_AFTER, HeaderValue::from_static("1"));
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

async fn enforce_ingress(
    State(guard): State<IngressGuard>,
    request: Request,
    next: Next,
) -> Response {
    let path = request.uri().path();
    let peer = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(addr)| *addr);

    match guard.check(path, request.headers(), peer) {
        IngressDecision::Allowed => {}
        IngressDecision::RateLimited {
            tier,
            retry_after_secs,
        } => return rate_limited_response(tier, retry_after_secs),
        IngressDecision::PayloadTooLarge { max_bytes } => {
            return payload_too_large_response(max_bytes);
        }
    }

    if RateLimitTier::classify(path) == RateLimitTier::HealthProbe {
        return next.run(request).await;
    }

    let Ok(_permit) = guard.permits.clone().try_acquire_owned() else {
        return overloaded_response();
    };
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::routing::{get, post};
    use std::net::{Ipv4Addr, Ipv6Addr};
    use tower::ServiceExt;

    #[test]
    fn route_classification_separates_health_auth_and_public() {
        assert_eq!(
            RateLimitTier::classify("/health"),
            RateLimitTier::HealthProbe
        );
        assert_eq!(
            RateLimitTier::classify("/readyz"),
            RateLimitTier::HealthProbe
        );
        assert_eq!(
            RateLimitTier::classify("/api/health"),
            RateLimitTier::HealthProbe
        );
        assert_eq!(
            RateLimitTier::classify("/auth/sign-in"),
            RateLimitTier::AuthSensitive
        );
        assert_eq!(
            RateLimitTier::classify("/physician/auth/verify"),
            RateLimitTier::AuthSensitive
        );
        assert_eq!(
            RateLimitTier::classify("/auth/demo-session"),
            RateLimitTier::AuthSensitive
        );
        assert_eq!(
            RateLimitTier::classify("/physician/auth/demo-session"),
            RateLimitTier::AuthSensitive
        );
        assert_eq!(
            RateLimitTier::classify("/api/auth/email/start"),
            RateLimitTier::AuthSensitive
        );
        assert_eq!(
            RateLimitTier::classify("/api/auth/email-change/start"),
            RateLimitTier::AuthSensitive
        );
        assert_eq!(
            RateLimitTier::classify("/api/auth/email-change/confirm"),
            RateLimitTier::AuthSensitive
        );
        assert_eq!(
            RateLimitTier::classify("/api/connected-care/sync"),
            RateLimitTier::AuthSensitive
        );
        assert_eq!(
            RateLimitTier::classify("/api/connected-care/invitations/inspect"),
            RateLimitTier::AuthSensitive
        );
        assert_eq!(
            RateLimitTier::classify("/api/connected-care/invitations/verify-dob"),
            RateLimitTier::AuthSensitive
        );
        assert_eq!(
            RateLimitTier::classify("/api/connected-care/invitations/share"),
            RateLimitTier::AuthSensitive
        );
        assert_eq!(
            RateLimitTier::classify("/front-door/invitations/redeem"),
            RateLimitTier::AuthSensitive
        );
        assert_eq!(RateLimitTier::classify("/today"), RateLimitTier::Public);
        assert_eq!(RateLimitTier::classify("/duet"), RateLimitTier::Public);
    }

    #[test]
    fn forwarded_for_ignores_spoofed_leftmost_ip_and_canonicalizes_v4_mapped_v6() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("9.9.9.9, 1.2.3.4, ::ffff:203.0.113.5"),
        );
        let key = ClientKey::from_request(&headers, None, 1);
        assert_eq!(
            key,
            ClientKey::Ip(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 5)))
        );

        let two_hops = ClientKey::from_request(&headers, None, 2);
        assert_eq!(
            two_hops,
            ClientKey::Ip(IpAddr::V4(Ipv4Addr::new(1, 2, 3, 4)))
        );

        // Multiple separate X-Forwarded-For headers must combine in order so an
        // attacker-supplied first header cannot shadow the rightmost proxy header.
        let mut multi = HeaderMap::new();
        multi.append("x-forwarded-for", HeaderValue::from_static("127.0.0.1"));
        multi.append(
            "x-forwarded-for",
            HeaderValue::from_static("198.51.100.42, 203.0.113.9"),
        );
        assert_eq!(
            ClientKey::from_request(&multi, None, 1),
            ClientKey::Ip(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 9)))
        );
        assert_eq!(
            ClientKey::from_request(&multi, None, 2),
            ClientKey::Ip(IpAddr::V4(Ipv4Addr::new(198, 51, 100, 42)))
        );

        let peer = SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), 4444);
        assert!(ClientKey::from_request(&HeaderMap::new(), Some(peer), 1).is_loopback());
    }

    #[test]
    fn hosted_environment_markers_enforce_loopback_quotas() {
        let local = IngressGuardConfig::from_env(|_| None, DEFAULT_MAX_BODY_BYTES);
        assert!(matches!(
            local.rate_limit,
            RateLimitPolicy::Enforced {
                loopback: LoopbackPolicy::Exempt,
                ..
            }
        ));
        for var in [
            "CODA_ENVIRONMENT",
            "APP_ENV",
            "K_SERVICE",
            "CODA_HOSTED",
            "CODA_RATE_LIMIT_ENFORCE_LOOPBACK",
        ] {
            let cfg = IngressGuardConfig::from_env(
                |key| {
                    if key == var {
                        Some(if var == "CODA_RATE_LIMIT_ENFORCE_LOOPBACK" {
                            "1".into()
                        } else {
                            "staging".into()
                        })
                    } else {
                        None
                    }
                },
                DEFAULT_MAX_BODY_BYTES,
            );
            assert!(
                matches!(
                    cfg.rate_limit,
                    RateLimitPolicy::Enforced {
                        loopback: LoopbackPolicy::Enforced,
                        ..
                    }
                ),
                "{var} must enforce loopback quotas"
            );
        }
    }

    #[test]
    fn strict_rate_limit_enforces_tier_quotas_recovers_after_window_and_bounds_memory() {
        let mut config = IngressGuardConfig::strict(
            RateLimitQuota::per_window(3, Duration::from_secs(60)),
            RateLimitQuota::per_window(2, Duration::from_secs(60)),
            1024,
            8,
        );
        if let RateLimitPolicy::Enforced {
            ref mut max_tracked_clients,
            ..
        } = config.rate_limit
        {
            *max_tracked_clients = 4;
        }
        let guard = IngressGuard::new(config);
        let t0 = Instant::now();
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", HeaderValue::from_static("198.51.100.10"));

        // Auth-sensitive allows 2 then returns 429 with Retry-After.
        assert_eq!(
            guard.check_at("/api/auth/email/start", &headers, None, t0),
            IngressDecision::Allowed
        );
        assert_eq!(
            guard.check_at("/api/auth/email/start", &headers, None, t0),
            IngressDecision::Allowed
        );
        assert!(matches!(
            guard.check_at(
                "/api/auth/email/start",
                &headers,
                None,
                t0 + Duration::from_secs(5)
            ),
            IngressDecision::RateLimited {
                tier: RateLimitTier::AuthSensitive,
                retry_after_secs: 55
            }
        ));

        // Health probes stay allowed even while the client is rate-limited.
        assert_eq!(
            guard.check_at("/readyz", &headers, None, t0 + Duration::from_secs(5)),
            IngressDecision::Allowed
        );

        // Window expiry resets the quota cleanly.
        assert_eq!(
            guard.check_at(
                "/api/auth/email/start",
                &headers,
                None,
                t0 + Duration::from_secs(60)
            ),
            IngressDecision::Allowed
        );

        // Distributed IP flood stays bounded to `max_tracked_clients`.
        for octet in 1..=20u8 {
            let mut flood = HeaderMap::new();
            flood.insert(
                "x-forwarded-for",
                HeaderValue::from_str(&format!("198.51.100.{octet}")).unwrap(),
            );
            let _ = guard.check_at("/today", &flood, None, t0 + Duration::from_secs(10));
        }
        assert!(guard.buckets.lock().unwrap().len() <= 4);
    }

    #[tokio::test]
    async fn middleware_returns_429_retry_after_and_413_payload_too_large() {
        let guard = IngressGuard::new(IngressGuardConfig::strict(
            RateLimitQuota::per_minute(5),
            RateLimitQuota::per_minute(1),
            64,
            2,
        ));
        let app = guard.protect(
            Router::new()
                .route("/health", get(|| async { "ok" }))
                .route("/auth/sign-in", post(|body: String| async move { body }))
                .route("/echo", post(|body: String| async move { body })),
        );

        // First auth request succeeds; second hits 429 with Retry-After and no-store.
        let req1 = Request::builder()
            .method("POST")
            .uri("/auth/sign-in")
            .header("x-forwarded-for", "203.0.113.7")
            .body(Body::from("{\"email\":\"a@b.test\"}"))
            .unwrap();
        let resp1 = app.clone().oneshot(req1).await.unwrap();
        assert_eq!(resp1.status(), StatusCode::OK);

        let req2 = Request::builder()
            .method("POST")
            .uri("/auth/sign-in")
            .header("x-forwarded-for", "203.0.113.7")
            .body(Body::from("{\"email\":\"a@b.test\"}"))
            .unwrap();
        let resp2 = app.clone().oneshot(req2).await.unwrap();
        assert_eq!(resp2.status(), StatusCode::TOO_MANY_REQUESTS);
        assert!(resp2.headers().contains_key(header::RETRY_AFTER));
        assert_eq!(resp2.headers()[header::CACHE_CONTROL], "no-store");

        // Oversized body fails fast with 413 Payload Too Large.
        let big = "x".repeat(128);
        let req3 = Request::builder()
            .method("POST")
            .uri("/echo")
            .header("x-forwarded-for", "203.0.113.8")
            .header(header::CONTENT_LENGTH, big.len().to_string())
            .body(Body::from(big))
            .unwrap();
        let resp3 = app.clone().oneshot(req3).await.unwrap();
        assert_eq!(resp3.status(), StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(resp3.headers()[header::CACHE_CONTROL], "no-store");
    }
}
