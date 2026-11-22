//! Physician-only, lossy projection invalidation. This is not a durable work queue.
//!
//! Consumers refetch once when they subscribe, then again on every `Resync`;
//! receiver lag, malformed wire data, and Redis reconnects all surface as one.
//! Workspace IDs are routing hints, not authorization: filter them against the
//! authenticated physician's current authority before sending anything to a client.
#![allow(
    clippy::map_err_ignore,
    reason = "converting to errors that keep their cause"
)]

use async_trait::async_trait;
use fred::{clients::SubscriberClient, prelude::*, types::RespVersion};
use serde::{Deserialize, Serialize};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{sync::broadcast, task::JoinHandle};

/// No patient data, messages, or authorization claims belong on this channel.
pub const CHANNEL: &str = "physician:workspace-events:v1";
const CAPACITY: usize = 1024;
const MAX_PAYLOAD: usize = 4096;

/// Connection timing. [`RedisBus::connect`] uses [`Timing::default`]; tests
/// pick their own through [`RedisBus::connect_with`] so a proof does not race
/// production's heartbeat or reconnect jitter.
#[doc(hidden)]
#[derive(Clone, Copy, Debug)]
pub struct Timing {
    /// Subscriber ping interval. A failed ping resyncs and forces a reconnect.
    pub heartbeat: Duration,
    /// Command and connection timeout, including the heartbeat ping.
    pub command_timeout: Duration,
    /// First reconnect delay; it doubles up to `reconnect_max`.
    pub reconnect_min: Duration,
    pub reconnect_max: Duration,
    /// Random delay added to each reconnect attempt.
    pub jitter: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            heartbeat: Duration::from_secs(2),
            command_timeout: Duration::from_secs(3),
            reconnect_min: Duration::from_millis(100),
            reconnect_max: Duration::from_secs(5),
            jitter: Duration::from_millis(100),
        }
    }
}

fn millis(duration: Duration) -> u32 {
    u32::try_from(duration.as_millis()).unwrap_or(u32::MAX)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Event {
    WorkspaceChanged { workspace_id: String },
    WorkspaceSettled { workspace_id: String },
    Resync,
}

impl Event {
    fn validate(&self) -> Result<(), String> {
        match self {
            Self::WorkspaceChanged { workspace_id } | Self::WorkspaceSettled { workspace_id }
                if workspace_id.is_empty()
                    || workspace_id.len() > 512
                    || workspace_id.chars().any(char::is_whitespace)
                    || workspace_id.chars().any(char::is_control) =>
            {
                Err("invalid physician workspace ID".into())
            }
            _ => Ok(()),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    version: u8,
    event: Event,
}

fn encode(event: Event) -> Result<String, String> {
    event.validate()?;
    serde_json::to_string(&Envelope { version: 1, event })
        .map_err(|_| "cannot encode physician bus event".into())
}

fn decode(payload: &[u8]) -> Result<Event, String> {
    if payload.len() > MAX_PAYLOAD {
        return Err("physician bus event too large".into());
    }
    let raw: serde_json::Value =
        serde_json::from_slice(payload).map_err(|_| "invalid physician bus event".to_string())?;
    // serde does not apply deny_unknown_fields to unit variants of an
    // internally tagged enum, so `resync` would accept smuggled fields.
    let allowed: &[&str] = match raw.pointer("/event/type").and_then(|t| t.as_str()) {
        Some("resync") => &["type"],
        _ => &["type", "workspace_id"],
    };
    if raw
        .get("event")
        .and_then(|event| event.as_object())
        .is_some_and(|event| event.keys().any(|key| !allowed.contains(&key.as_str())))
    {
        return Err("invalid physician bus event".into());
    }
    let envelope: Envelope =
        serde_json::from_value(raw).map_err(|_| "invalid physician bus event".to_string())?;
    if envelope.version != 1 {
        return Err("unsupported physician bus event version".into());
    }
    envelope.event.validate()?;
    Ok(envelope.event)
}

#[async_trait]
pub trait Bus: Send + Sync {
    /// Best-effort invalidation only. A successful publish is not durable delivery.
    async fn publish(&self, event: Event) -> Result<(), String>;
    /// Events published from now on. Whoever opens a subscription refetches
    /// once itself (it knows it just connected); afterwards every loss —
    /// receiver lag or a Redis reconnect — surfaces as `Resync`.
    fn subscribe(&self) -> Subscription;
    /// Whether the subscriber is connected and its subscription acknowledged.
    fn live(&self) -> bool;
}

/// One consumer's view of the bus. Lag surfaces as `Event::Resync`, so a
/// consumer only has to handle events, never a silent gap.
pub struct Subscription {
    events: broadcast::Receiver<Event>,
}

impl Subscription {
    fn new(events: &broadcast::Sender<Event>) -> Self {
        Self {
            events: events.subscribe(),
        }
    }

    /// The next event. `Err` only once the bus itself is gone.
    pub async fn recv(&mut self) -> Result<Event, broadcast::error::RecvError> {
        match self.events.recv().await {
            Err(broadcast::error::RecvError::Lagged(_)) => Ok(Event::Resync),
            other => other,
        }
    }

    /// A queued event without waiting, for draining.
    pub fn try_recv(&mut self) -> Result<Event, broadcast::error::TryRecvError> {
        match self.events.try_recv() {
            Err(broadcast::error::TryRecvError::Lagged(_)) => Ok(Event::Resync),
            other => other,
        }
    }
}

pub struct LocalBus {
    events: broadcast::Sender<Event>,
}

impl LocalBus {
    pub fn new(capacity: usize) -> Self {
        Self {
            events: broadcast::channel(capacity.max(1)).0,
        }
    }
}

#[async_trait]
impl Bus for LocalBus {
    async fn publish(&self, event: Event) -> Result<(), String> {
        event.validate()?;
        let _ = self.events.send(event);
        Ok(())
    }

    fn subscribe(&self) -> Subscription {
        Subscription::new(&self.events)
    }

    fn live(&self) -> bool {
        true
    }
}

#[derive(Default)]
struct Tasks(Vec<JoinHandle<()>>);

impl Drop for Tasks {
    fn drop(&mut self) {
        for task in &self.0 {
            task.abort();
        }
    }
}

pub struct RedisBus {
    publisher: Client,
    subscriber: SubscriberClient,
    events: broadcast::Sender<Event>,
    live: Arc<AtomicBool>,
    // Abort on drop, including cancellation/failure during connect. No detached
    // retry loops or sockets should outlive the owning bus.
    _tasks: Tasks,
}

fn config(url: &str, ca: Option<&[u8]>) -> Result<Config, String> {
    // Restrict to one ordinary Redis endpoint, never a cluster/sentinel URL.
    if !url.starts_with("redis://") && !url.starts_with("rediss://") {
        return Err("physician Redis URL must use redis or rediss".into());
    }
    // fred builds a default rustls config while parsing `rediss`. With both
    // ring and aws-lc-rs linked (feature unification), rustls has no process
    // default and panics, so pin one. An existing default is kept.
    let _ = fred::rustls::crypto::ring::default_provider().install_default();
    let mut config =
        Config::from_url(url).map_err(|_| "invalid physician Redis URL".to_string())?;
    // RESP3 allows a subscriber heartbeat without another subscriber connection.
    config.version = RespVersion::RESP3;
    if let Some(ca) = ca {
        if !url.starts_with("rediss://") {
            return Err("physician Redis CA requires rediss TLS".into());
        }
        use fred::rustls::{
            ClientConfig, RootCertStore,
            pki_types::{CertificateDer, pem::PemObject},
        };
        let mut roots = RootCertStore::empty();
        for cert in CertificateDer::pem_slice_iter(ca) {
            roots
                .add(cert.map_err(|_| "invalid physician Redis CA".to_string())?)
                .map_err(|_| "invalid physician Redis CA".to_string())?;
        }
        if roots.is_empty() {
            return Err("empty physician Redis CA bundle".into());
        }
        // Name the provider: feature unification can enable both ring and
        // aws-lc-rs in one binary, and then rustls has no process default.
        let provider = std::sync::Arc::new(fred::rustls::crypto::ring::default_provider());
        config.tls = Some(
            ClientConfig::builder_with_provider(provider)
                .with_safe_default_protocol_versions()
                .map_err(|_| "physician Redis TLS provider unavailable".to_string())?
                .with_root_certificates(roots)
                .with_no_client_auth()
                .into(),
        );
    }
    // AUTH (including ACL username) is parsed by fred. Never expose its errors,
    // URL, credentials, or payload in application logs or returned diagnostics.
    Ok(config)
}

impl RedisBus {
    /// Connect publisher and subscriber, authenticating from the URL. `rediss`
    /// verifies the server certificate and name using `ca` (PEM bundle bytes,
    /// e.g. `PHYSICIAN_REDIS_CA_PEM`) or system roots. Hosted configuration must use `rediss` and the Memorystore CA.
    /// Initial failure returns an error; established connections retry forever
    /// with bounded exponential backoff. No credentials appear in our errors.
    pub async fn connect(url: &str, ca: Option<&[u8]>) -> Result<Self, String> {
        Self::connect_with(url, ca, Timing::default()).await
    }

    /// [`RedisBus::connect`] with explicit timing, for tests.
    #[doc(hidden)]
    pub async fn connect_with(
        url: &str,
        ca: Option<&[u8]>,
        timing: Timing,
    ) -> Result<Self, String> {
        let config = config(url, ca)?;
        let mut builder = Builder::from_config(config);
        let mut policy = ReconnectPolicy::new_exponential(
            0,
            millis(timing.reconnect_min),
            millis(timing.reconnect_max),
            2,
        );
        policy.set_jitter(millis(timing.jitter));
        builder.set_policy(policy);
        builder.with_performance_config(|config| {
            config.default_command_timeout = timing.command_timeout
        });
        builder.with_connection_config(|config| config.connection_timeout = timing.command_timeout);
        let publisher = builder
            .build()
            .map_err(|_| "cannot build physician Redis publisher")?;
        let subscriber = builder
            .build_subscriber_client()
            .map_err(|_| "cannot build physician Redis subscriber")?;
        let events = broadcast::channel(CAPACITY).0;
        let live = Arc::new(AtomicBool::new(false));
        let mut tasks = Tasks::default();
        // Register notification receivers before connecting: even a very short
        // disconnect must invalidate projections, not disappear between polls.
        let mut reconnects = subscriber.reconnect_rx();
        let mut errors = subscriber.error_rx();
        let mut messages = subscriber.message_rx();
        let mut publisher_failed = publisher.error_rx();
        let mut subscriber_failed = subscriber.error_rx();
        for connection in [publisher.connect(), subscriber.connect()] {
            // Aborting a wrapper would detach its child. Store a task that owns
            // an abort-on-drop guard for the actual fred connection handle.
            struct AbortConnection(fred::types::ConnectHandle);
            impl Drop for AbortConnection {
                fn drop(&mut self) {
                    self.0.abort();
                }
            }
            let mut guard = AbortConnection(connection);
            tasks.0.push(tokio::spawn(async move {
                let _ = (&mut guard.0).await;
            }));
        }
        tokio::time::timeout(Duration::from_secs(10), async {
            connected(&publisher, &mut publisher_failed).await?;
            connected(&subscriber, &mut subscriber_failed).await?;
            subscriber.subscribe(CHANNEL).await.map_err(|_| ())
        })
        .await
        .map_err(|_| "physician Redis connection timed out")?
        .map_err(|()| "physician Redis connection or subscription failed")?;
        live.store(true, Ordering::Release);
        let listener = subscriber.clone();
        let output = events.clone();
        let healthy = live.clone();
        tasks.0.push(tokio::spawn(async move {
            let mut heartbeat = tokio::time::interval(timing.heartbeat);
            loop {
                tokio::select! {
                    result = reconnects.recv() => {
                        if matches!(result, Err(broadcast::error::RecvError::Closed)) { break; }
                        healthy.store(false, Ordering::Release);
                        let _ = output.send(Event::Resync);
                        // Restore explicitly, so live becomes true only after ACK.
                        if listener.subscribe(CHANNEL).await.is_ok() {
                            healthy.store(true, Ordering::Release);
                            let _ = output.send(Event::Resync);
                        }
                    }
                    result = errors.recv() => {
                        if matches!(result, Err(broadcast::error::RecvError::Closed)) { break; }
                        healthy.store(false, Ordering::Release);
                        let _ = output.send(Event::Resync);
                    }
                    result = messages.recv() => {
                        match result {
                            Ok(message) if message.channel == CHANNEL => {
                                let event = message.value.as_bytes().and_then(|value| decode(value).ok())
                                    .unwrap_or(Event::Resync);
                                let _ = output.send(event);
                            }
                            Ok(_) => {}
                            Err(broadcast::error::RecvError::Lagged(_)) => { let _ = output.send(Event::Resync); }
                            Err(broadcast::error::RecvError::Closed) => break,
                        }
                    }
                    _ = heartbeat.tick() => {
                        if listener.ping::<String>(None).await.is_err() {
                            healthy.store(false, Ordering::Release);
                            let _ = output.send(Event::Resync);
                            let _ = listener.force_reconnection().await;
                        } else if !healthy.load(Ordering::Acquire)
                            && listener.subscribe(CHANNEL).await.is_ok() {
                            healthy.store(true, Ordering::Release);
                            let _ = output.send(Event::Resync);
                        }
                    }
                }
            }
            healthy.store(false, Ordering::Release);
            let _ = output.send(Event::Resync);
        }));
        Ok(Self {
            publisher,
            subscriber,
            events,
            live,
            _tasks: tasks,
        })
    }
}

/// Resolves once `client` has connected, or fails on its first connection
/// error. `errors` must be subscribed before the client starts connecting.
///
/// Not `wait_for_connect`: fred reads the client state and only then
/// subscribes to the connect notice, so a connection that completes between
/// the two is never announced and the wait runs until the startup timeout.
/// The state is durable and the error receiver predates the connection, so
/// neither outcome can slip between checks here.
async fn connected<T: Clone>(
    client: &impl ClientLike,
    errors: &mut broadcast::Receiver<T>,
) -> Result<(), ()> {
    loop {
        if client.is_connected() {
            return Ok(());
        }
        tokio::select! {
            result = errors.recv() => {
                if !matches!(result, Err(broadcast::error::RecvError::Lagged(_))) {
                    return Err(());
                }
            }
            () = tokio::time::sleep(Duration::from_millis(10)) => {}
        }
    }
}

#[async_trait]
impl Bus for RedisBus {
    async fn publish(&self, event: Event) -> Result<(), String> {
        let payload = encode(event)?;
        if !self.publisher.is_connected() {
            return Err("physician Redis publisher disconnected".into());
        }
        self.publisher
            .publish::<i64, _, _>(CHANNEL, payload)
            .await
            .map(|_| ())
            .map_err(|_| "physician Redis publish failed".into())
    }

    fn subscribe(&self) -> Subscription {
        Subscription::new(&self.events)
    }

    fn live(&self) -> bool {
        // Both halves must be up: a reconnected subscriber with a publisher
        // still reconnecting would accept turns whose changes cannot fan out.
        self.live.load(Ordering::Acquire)
            && self.subscriber.is_connected()
            && self.publisher.is_connected()
    }
}

/// Validates bus environment configuration synchronously without opening a
/// network connection (`PHYSICIAN_REDIS_URL`, `PHYSICIAN_REDIS_CA_PEM`).
pub fn validate_env(get: impl Fn(&str) -> Option<String>, hosted: bool) -> Result<(), String> {
    let url = get("PHYSICIAN_REDIS_URL").filter(|url| !url.trim().is_empty());
    let ca = get("PHYSICIAN_REDIS_CA_PEM").filter(|ca| !ca.trim().is_empty());
    match url {
        None if ca.is_some() => Err("PHYSICIAN_REDIS_CA_PEM requires PHYSICIAN_REDIS_URL".into()),
        None => Ok(()),
        Some(url) => {
            if hosted && (!url.starts_with("rediss://") || ca.is_none()) {
                return Err(
                    "hosted physician Redis requires rediss and PHYSICIAN_REDIS_CA_PEM".into(),
                );
            }
            Ok(())
        }
    }
}

/// Selects the bus from configuration: `PHYSICIAN_REDIS_URL` picks
/// [`RedisBus`] (with optional `PHYSICIAN_REDIS_CA_PEM`), otherwise a
/// [`LocalBus`] serves one process. Hosted runs must use `rediss` with a CA:
/// a plaintext or system-root Redis would put AUTH on an unverified link.
pub async fn from_env(
    get: impl Fn(&str) -> Option<String>,
    hosted: bool,
) -> Result<Arc<dyn Bus>, String> {
    validate_env(&get, hosted)?;
    let url = get("PHYSICIAN_REDIS_URL").filter(|url| !url.trim().is_empty());
    let ca = get("PHYSICIAN_REDIS_CA_PEM").filter(|ca| !ca.trim().is_empty());
    match url {
        None => Ok(Arc::new(LocalBus::new(CAPACITY))),
        Some(url) => Ok(Arc::new(
            RedisBus::connect(&url, ca.as_deref().map(str::as_bytes)).await?,
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn selection_defaults_local_and_hosted_requires_verified_tls() {
        let none = |_: &str| None;
        assert!(from_env(none, true).await.unwrap().live());
        let only_ca = |name: &str| (name == "PHYSICIAN_REDIS_CA_PEM").then(|| "pem".to_owned());
        assert!(from_env(only_ca, false).await.is_err());
        for (url, ca) in [
            ("redis://default:x@10.0.0.1:6378", Some("pem")),
            ("rediss://default:x@10.0.0.1:6378", None),
        ] {
            let get = |name: &str| match name {
                "PHYSICIAN_REDIS_URL" => Some(url.to_owned()),
                "PHYSICIAN_REDIS_CA_PEM" => ca.map(str::to_owned),
                _ => None,
            };
            let error = from_env(get, true).await.err().unwrap();
            assert!(error.contains("hosted"), "{error}");
            assert!(!error.contains("default:x"));
        }
    }

    #[tokio::test]
    async fn local_fanout_and_lag_resync() {
        let bus: Arc<dyn Bus> = Arc::new(LocalBus::new(2));
        let mut first = bus.subscribe();
        let mut second = bus.subscribe();
        // Opening a subscription disturbs nobody.
        assert!(first.try_recv().is_err());
        assert!(second.try_recv().is_err());
        let event = Event::WorkspaceChanged {
            workspace_id: "workspace-1".into(),
        };
        bus.publish(event.clone()).await.unwrap();
        assert_eq!(first.recv().await.unwrap(), event);
        assert_eq!(second.recv().await.unwrap(), event);
        for _ in 0..4 {
            bus.publish(event.clone()).await.unwrap();
        }
        // Lag never drops silently: it is a resync.
        assert_eq!(first.recv().await.unwrap(), Event::Resync);
        assert!(bus.live());
    }

    #[test]
    fn strict_versioned_wire_contract() {
        for event in [
            Event::Resync,
            Event::WorkspaceChanged {
                workspace_id: "workspace-1".into(),
            },
            Event::WorkspaceSettled {
                workspace_id: "workspace-2".into(),
            },
        ] {
            assert_eq!(
                decode(encode(event.clone()).unwrap().as_bytes()).unwrap(),
                event
            );
        }
        for payload in [
            r#"{"version":2,"event":{"type":"resync"}}"#,
            r#"{"version":1,"event":{"type":"workspace_changed","workspace_id":""}}"#,
            r#"{"version":1,"event":{"type":"resync","clinical_text":"not allowed"}}"#,
            r#"{"version":1,"event":{"type":"unknown"}}"#,
            r#"{"version":1,"event":{"type":"resync"},"extra":true}"#,
        ] {
            assert!(decode(payload.as_bytes()).is_err(), "{payload}");
        }
        assert!(decode(&vec![b' '; MAX_PAYLOAD + 1]).is_err());
        assert!(
            encode(Event::WorkspaceChanged {
                workspace_id: "a\nb".into()
            })
            .is_err()
        );
    }

    #[test]
    fn rejects_unsafe_ca_and_redacts_url_errors() {
        let pem = b"-----BEGIN CERTIFICATE-----\nnot base64\n-----END CERTIFICATE-----\n";
        assert!(config("redis://localhost", Some(pem)).is_err());
        let error = config("not-a-url:secret", None).err().unwrap();
        assert!(!error.contains("secret"));
        assert!(config("rediss://localhost", Some(pem)).is_err());
        assert!(config("rediss://localhost", Some(b"")).is_err());
        let config = config(
            "redis://default:synthetic-test-password@localhost:6379",
            None,
        )
        .unwrap();
        assert_eq!(config.username.as_deref(), Some("default"));
        assert_eq!(config.password.as_deref(), Some("synthetic-test-password"));
    }
}
