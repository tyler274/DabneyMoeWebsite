//! Public demo session configuration and bounded-memory turn budgets.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::Json;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use http::ingress::{ClientKey, DEFAULT_MAX_BODY_BYTES, IngressGuardConfig};
use serde_json::json;

/// Dedicated synthetic practice serving public demo visitors.
pub const DEMO_PRACTICE_ID: &str = "coda-demo-practice";

/// Display name shown on ephemeral demo visitor accounts.
pub const DEMO_PHYSICIAN_DISPLAY_NAME: &str = "Dr. Hannah Alvarez (Demo)";

/// Default fixed lifetime for each public demo bearer session (2 hours).
pub const DEMO_SESSION_SECONDS: u64 = 2 * 3600;

/// Environment variable overriding [`DEMO_SESSION_SECONDS`].
pub const DEMO_SESSION_SECONDS_ENV: &str = "CODA_DEMO_SESSION_SECONDS";

/// Default maximum model-invoking turns per ephemeral demo physician session.
pub const DEFAULT_DEMO_SESSION_MAX_TURNS: u32 = 20;

/// Environment variable overriding [`DEFAULT_DEMO_SESSION_MAX_TURNS`].
pub const DEMO_SESSION_MAX_TURNS_ENV: &str = "CODA_DEMO_SESSION_MAX_TURNS";

/// Default maximum model-invoking demo turns per client IP per hour (`3600s`).
pub const DEFAULT_DEMO_IP_HOURLY_MAX_TURNS: u32 = 30;

/// Environment variable overriding [`DEFAULT_DEMO_IP_HOURLY_MAX_TURNS`].
pub const DEMO_IP_HOURLY_MAX_TURNS_ENV: &str = "CODA_DEMO_IP_HOURLY_MAX_TURNS";

/// Default global daily cap across all public demo visitors (`86400s`).
pub const DEFAULT_DEMO_DAILY_MAX_TURNS: u32 = 500;

/// Environment variable overriding [`DEFAULT_DEMO_DAILY_MAX_TURNS`].
pub const DEMO_DAILY_MAX_TURNS_ENV: &str = "CODA_DEMO_DAILY_MAX_TURNS";

/// Default maximum new demo sessions minted per client IP per hour (`3600s`).
pub const DEFAULT_DEMO_IP_HOURLY_MAX_SESSIONS: u32 = 20;

/// Environment variable overriding [`DEFAULT_DEMO_IP_HOURLY_MAX_SESSIONS`].
pub const DEMO_IP_HOURLY_MAX_SESSIONS_ENV: &str = "CODA_DEMO_IP_HOURLY_MAX_SESSIONS";

const IP_WINDOW: Duration = Duration::from_secs(3600);
const DAILY_WINDOW: Duration = Duration::from_secs(86_400);
const MAX_TRACKED_SESSIONS: usize = 4_096;
const MAX_TRACKED_IPS: usize = 4_096;

/// Default maximum in-memory visitor sessions retained concurrently before LRU eviction.
pub const DEFAULT_MAX_SESSIONS: usize = 64;

/// Configuration for `POST /auth/demo-session`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DemoSessionConfig {
    /// Fixed session lifetime in seconds.
    pub session_seconds: u64,
    /// Maximum live isolated sessions retained in memory before LRU eviction.
    pub max_sessions: usize,
}

impl Default for DemoSessionConfig {
    fn default() -> Self {
        Self {
            session_seconds: DEMO_SESSION_SECONDS,
            max_sessions: DEFAULT_MAX_SESSIONS,
        }
    }
}

impl DemoSessionConfig {
    /// Reads demo session configuration from `get`, validating positive lifetimes.
    pub fn from_env(get: impl Fn(&str) -> Option<String>) -> Result<Self, String> {
        let session_seconds = match get(DEMO_SESSION_SECONDS_ENV)
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty())
        {
            Some(raw) => {
                let parsed = raw.parse::<u64>().map_err(|err| {
                    format!("{DEMO_SESSION_SECONDS_ENV} must be a positive integer: {err}")
                })?;
                if parsed == 0 {
                    return Err(format!("{DEMO_SESSION_SECONDS_ENV} must be greater than 0"));
                }
                parsed
            }
            None => DEMO_SESSION_SECONDS,
        };
        let max_sessions = match get("CODA_DEMO_MAX_SESSIONS")
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty())
        {
            Some(raw) => raw
                .parse::<usize>()
                .map_err(|err| format!("CODA_DEMO_MAX_SESSIONS must be a positive integer: {err}"))?
                .max(1),
            None => DEFAULT_MAX_SESSIONS,
        };
        Ok(Self {
            session_seconds,
            max_sessions,
        })
    }
}

/// Turn-budget configuration for public demo sessions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DemoTurnBudgetConfig {
    /// Maximum model-invoking turns allowed for one demo `physician_id`.
    pub session_max_turns: u32,
    /// Maximum model-invoking demo turns allowed per client IP per hour (`3600s`).
    pub ip_hourly_max_turns: u32,
    /// Global maximum model-invoking demo turns allowed per day (`86400s`).
    pub daily_max_turns: u32,
    /// Maximum new demo sessions minted per client IP per hour (`3600s`).
    pub ip_hourly_max_sessions: u32,
    /// Trusted proxy hop count for client IP extraction via [`ClientKey::from_request`].
    pub trusted_proxy_hops: usize,
    /// Positive `Retry-After` fallback reported when a per-session cap is reached.
    pub session_retry_after_seconds: u64,
}

impl Default for DemoTurnBudgetConfig {
    fn default() -> Self {
        Self {
            session_max_turns: DEFAULT_DEMO_SESSION_MAX_TURNS,
            ip_hourly_max_turns: DEFAULT_DEMO_IP_HOURLY_MAX_TURNS,
            daily_max_turns: DEFAULT_DEMO_DAILY_MAX_TURNS,
            ip_hourly_max_sessions: DEFAULT_DEMO_IP_HOURLY_MAX_SESSIONS,
            trusted_proxy_hops: http::ingress::DEFAULT_TRUSTED_PROXY_HOPS,
            session_retry_after_seconds: DEMO_SESSION_SECONDS,
        }
    }
}

impl DemoTurnBudgetConfig {
    /// Reads demo turn-budget limits from `get`.
    pub fn from_env(get: impl Fn(&str) -> Option<String>) -> Result<Self, String> {
        let parse_limit = |key: &str, default: u32| -> Result<u32, String> {
            match get(key)
                .map(|v| v.trim().to_owned())
                .filter(|v| !v.is_empty())
            {
                Some(raw) => raw
                    .parse::<u32>()
                    .map_err(|err| format!("{key} must be a non-negative integer: {err}")),
                None => Ok(default),
            }
        };
        let session_config = DemoSessionConfig::from_env(&get)?;
        let ingress = IngressGuardConfig::from_env(&get, DEFAULT_MAX_BODY_BYTES);
        Ok(Self {
            session_max_turns: parse_limit(
                DEMO_SESSION_MAX_TURNS_ENV,
                DEFAULT_DEMO_SESSION_MAX_TURNS,
            )?,
            ip_hourly_max_turns: parse_limit(
                DEMO_IP_HOURLY_MAX_TURNS_ENV,
                DEFAULT_DEMO_IP_HOURLY_MAX_TURNS,
            )?,
            daily_max_turns: parse_limit(DEMO_DAILY_MAX_TURNS_ENV, DEFAULT_DEMO_DAILY_MAX_TURNS)?,
            ip_hourly_max_sessions: parse_limit(
                DEMO_IP_HOURLY_MAX_SESSIONS_ENV,
                DEFAULT_DEMO_IP_HOURLY_MAX_SESSIONS,
            )?,
            trusted_proxy_hops: ingress.trusted_proxy_hops,
            session_retry_after_seconds: session_config.session_seconds.max(1),
        })
    }
}

/// Which quota caused [`DemoTurnBudget`] to reject a demo turn or session mint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DemoBudgetLimit {
    /// Per-demo-session lifetime turn cap.
    Session,
    /// Per-client-IP hourly window cap.
    IpHourly,
    /// Global daily window cap across all demo visitors.
    Daily,
    /// Per-client-IP hourly session creation cap.
    SessionMintIpHourly,
}

/// Rejection details when a demo turn budget is exhausted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DemoBudgetExhausted {
    /// Quota that triggered the rejection.
    pub limit: DemoBudgetLimit,
    /// Positive `Retry-After` interval in seconds.
    pub retry_after_seconds: u64,
}

impl DemoBudgetExhausted {
    fn detail(self) -> &'static str {
        match self.limit {
            DemoBudgetLimit::Session => {
                "This demo session has reached its turn limit. Start a new demo session to continue."
            }
            DemoBudgetLimit::IpHourly => {
                "Hourly public demo turn limit reached for this network address. Please try again later."
            }
            DemoBudgetLimit::Daily => {
                "The public demo has reached its daily capacity. Please try again later."
            }
            DemoBudgetLimit::SessionMintIpHourly => {
                "Too many public demo sessions created from this network address. Please try again later."
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct SessionBucket {
    count: u32,
    last_seen: Instant,
}

#[derive(Clone, Copy, Debug)]
struct WindowBucket {
    window_start: Instant,
    count: u32,
}

#[derive(Debug)]
struct BudgetState {
    sessions: HashMap<String, SessionBucket>,
    ips: HashMap<ClientKey, WindowBucket>,
    session_mints: HashMap<ClientKey, WindowBucket>,
    daily: WindowBucket,
}

/// Bounded-memory per-session, per-IP hourly, and global daily turn budget for
/// model-invoking commands.
#[derive(Clone, Debug)]
pub struct DemoTurnBudget {
    config: DemoTurnBudgetConfig,
    state: Arc<Mutex<BudgetState>>,
}

impl DemoTurnBudget {
    /// Builds a budget enforcer for `config`.
    pub fn new(config: DemoTurnBudgetConfig) -> Self {
        Self {
            config,
            state: Arc::new(Mutex::new(BudgetState {
                sessions: HashMap::new(),
                ips: HashMap::new(),
                session_mints: HashMap::new(),
                daily: WindowBucket {
                    window_start: Instant::now(),
                    count: 0,
                },
            })),
        }
    }

    /// Builds a budget enforcer configured from `get`.
    pub fn from_env(get: impl Fn(&str) -> Option<String>) -> Result<Self, String> {
        DemoTurnBudgetConfig::from_env(get).map(Self::new)
    }

    /// Checks and records one `POST /auth/demo-session` mint against per-IP hourly caps.
    pub fn check_and_record_session_mint(
        &self,
        headers: &HeaderMap,
        peer_addr: Option<SocketAddr>,
    ) -> Result<(), DemoBudgetExhausted> {
        let client = ClientKey::from_request(headers, peer_addr, self.config.trusted_proxy_hops);
        let now = Instant::now();
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        check_ip_window(
            &state.session_mints,
            client,
            DemoBudgetLimit::SessionMintIpHourly,
            self.config.ip_hourly_max_sessions,
            now,
        )?;
        record_ip_window(&mut state.session_mints, client, now);
        Ok(())
    }

    /// Checks and consumes one turn for `physician_id` and `headers`/`peer_addr`.
    pub fn check_and_record(
        &self,
        physician_id: &str,
        headers: &HeaderMap,
        peer_addr: Option<SocketAddr>,
    ) -> Result<(), DemoBudgetExhausted> {
        let client = ClientKey::from_request(headers, peer_addr, self.config.trusted_proxy_hops);
        self.consume_at(physician_id, client, Instant::now())
    }

    fn consume_at(
        &self,
        physician_id: &str,
        client: ClientKey,
        now: Instant,
    ) -> Result<(), DemoBudgetExhausted> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let daily_elapsed = now.saturating_duration_since(state.daily.window_start);
        if daily_elapsed >= DAILY_WINDOW {
            state.daily = WindowBucket {
                window_start: now,
                count: 0,
            };
        }
        if state.daily.count >= self.config.daily_max_turns {
            let remaining = DAILY_WINDOW.saturating_sub(daily_elapsed);
            return Err(DemoBudgetExhausted {
                limit: DemoBudgetLimit::Daily,
                retry_after_seconds: remaining.as_secs().max(1),
            });
        }

        check_ip_window(
            &state.ips,
            client,
            DemoBudgetLimit::IpHourly,
            self.config.ip_hourly_max_turns,
            now,
        )?;

        if let Some(bucket) = state.sessions.get(physician_id).copied()
            && bucket.count >= self.config.session_max_turns
        {
            return Err(DemoBudgetExhausted {
                limit: DemoBudgetLimit::Session,
                retry_after_seconds: self.config.session_retry_after_seconds.max(1),
            });
        }

        state.daily.count = state.daily.count.saturating_add(1);
        record_ip_window(&mut state.ips, client, now);

        if !state.sessions.contains_key(physician_id)
            && state.sessions.len() >= MAX_TRACKED_SESSIONS
            && let Some(oldest_id) = state
                .sessions
                .iter()
                .min_by_key(|(_, b)| b.last_seen)
                .map(|(k, _)| k.clone())
        {
            state.sessions.remove(&oldest_id);
        }
        let session = state
            .sessions
            .entry(physician_id.to_owned())
            .or_insert(SessionBucket {
                count: 0,
                last_seen: now,
            });
        session.count = session.count.saturating_add(1);
        session.last_seen = now;
        Ok(())
    }
}

fn check_ip_window(
    map: &HashMap<ClientKey, WindowBucket>,
    client: ClientKey,
    limit: DemoBudgetLimit,
    max: u32,
    now: Instant,
) -> Result<(), DemoBudgetExhausted> {
    if let Some(bucket) = map.get(&client).copied() {
        let elapsed = now.saturating_duration_since(bucket.window_start);
        if elapsed < IP_WINDOW && bucket.count >= max {
            let remaining = IP_WINDOW.saturating_sub(elapsed);
            return Err(DemoBudgetExhausted {
                limit,
                retry_after_seconds: remaining.as_secs().max(1),
            });
        }
    }
    Ok(())
}

fn record_ip_window(map: &mut HashMap<ClientKey, WindowBucket>, client: ClientKey, now: Instant) {
    let expired = map
        .get(&client)
        .is_some_and(|b| now.saturating_duration_since(b.window_start) >= IP_WINDOW);
    if !map.contains_key(&client) || expired {
        if !map.contains_key(&client) && map.len() >= MAX_TRACKED_IPS {
            map.retain(|_, b| now.saturating_duration_since(b.window_start) < IP_WINDOW);
            if map.len() >= MAX_TRACKED_IPS
                && let Some(oldest_key) = map
                    .iter()
                    .min_by_key(|(_, b)| b.window_start)
                    .map(|(k, _)| *k)
            {
                map.remove(&oldest_key);
            }
        }
        map.insert(
            client,
            WindowBucket {
                window_start: now,
                count: 1,
            },
        );
    } else if let Some(bucket) = map.get_mut(&client) {
        bucket.count = bucket.count.saturating_add(1);
    }
}

/// Renders the `429 Too Many Requests` response when [`DemoTurnBudget`] rejects a turn.
pub fn demo_budget_exhausted_response(exhausted: DemoBudgetExhausted) -> Response {
    let retry_after = exhausted.retry_after_seconds.max(1);
    let mut response = (
        StatusCode::TOO_MANY_REQUESTS,
        Json(json!({
            "code": "demo_turn_budget_exhausted",
            "detail": exhausted.detail(),
            "retry_after_seconds": retry_after,
        })),
    )
        .into_response();
    if let Ok(value) = HeaderValue::from_str(&retry_after.to_string()) {
        response.headers_mut().insert(header::RETRY_AFTER, value);
    }
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_enforces_session_ip_hourly_and_daily_caps() {
        let cfg = DemoTurnBudgetConfig {
            session_max_turns: 2,
            ip_hourly_max_turns: 3,
            daily_max_turns: 4,
            ..Default::default()
        };
        let budget = DemoTurnBudget::new(cfg);
        let peer = Some("127.0.0.1:42000".parse().unwrap());
        let headers = HeaderMap::new();

        assert!(budget.check_and_record("demo-s1", &headers, peer).is_ok());
        assert!(budget.check_and_record("demo-s1", &headers, peer).is_ok());
        let session_err = budget
            .check_and_record("demo-s1", &headers, peer)
            .expect_err("3rd turn on demo-s1 must hit session cap");
        assert_eq!(session_err.limit, DemoBudgetLimit::Session);

        // Fresh session from same IP still hits IP hourly cap (3 turns total).
        assert!(budget.check_and_record("demo-s2", &headers, peer).is_ok());
        let ip_err = budget
            .check_and_record("demo-s2", &headers, peer)
            .expect_err("4th turn from same IP must hit ip_hourly cap");
        assert_eq!(ip_err.limit, DemoBudgetLimit::IpHourly);

        // Different IP succeeds for the 4th recorded turn, then hits global daily cap (4 total turns).
        let peer2 = Some("10.0.0.5:42000".parse().unwrap());
        assert!(budget.check_and_record("demo-s3", &headers, peer2).is_ok());
        let daily_err = budget
            .check_and_record("demo-s3", &headers, peer2)
            .expect_err("5th recorded turn overall must hit daily cap");
        assert_eq!(daily_err.limit, DemoBudgetLimit::Daily);
    }

    #[tokio::test]
    async fn budget_exhausted_response_emits_429_json_and_retry_after() {
        let resp = demo_budget_exhausted_response(DemoBudgetExhausted {
            limit: DemoBudgetLimit::Session,
            retry_after_seconds: 120,
        });
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(resp.headers().get(header::RETRY_AFTER).unwrap(), "120");
        assert_eq!(
            resp.headers().get(header::CACHE_CONTROL).unwrap(),
            "no-store"
        );
        let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let val: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(val["code"], "demo_turn_budget_exhausted");
        assert_eq!(val["retry_after_seconds"], 120);
        assert!(val["detail"].as_str().is_some_and(|s| !s.is_empty()));
    }
}
