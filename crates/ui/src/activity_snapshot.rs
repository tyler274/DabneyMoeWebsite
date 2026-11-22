//! Version 1 producer contract. Times are UTC Unix seconds; intervals are [start, end).
//! These input types are never serialized into the public page. Publishers must write
//! both files atomically outside the static site root; authorization is a separate,
//! current read of the consent authority, not a copy inferred from snapshot membership.
use serde::{Deserialize, Serialize};

pub const SNAPSHOT_MAX_AGE: i64 = 300;
pub const AUTHORIZATION_MAX_AGE: i64 = 60;
pub const PUBLICATION_DELAY: i64 = 3600;
pub const RETENTION: i64 = 7 * 24 * 3600;
pub const MIN_COHORT: usize = 5;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "version", deny_unknown_fields)]
pub enum Snapshot {
    #[serde(rename = "1")]
    V1(SnapshotV1),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotV1 {
    pub generated_at: i64,
    /// Stable operator identity, even when their optional public series is absent.
    /// It must not change across snapshots of already-published hours.
    pub operator_subject: String,
    pub health: Vec<HealthInterval>,
    pub hourly_messages: Vec<MessageHour>,
    pub operator: Option<MessageSeries>,
    pub presence: Vec<PresenceSeries>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "version", deny_unknown_fields)]
pub enum Authorization {
    #[serde(rename = "1")]
    V1 { checked_at: i64, grants: Vec<Grant> },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Grant {
    pub subject: String,
    pub scope: Scope,
    pub effective_at: i64,
    pub expires_at: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    AggregateMessages,
    IdentifiedMessages,
    PresenceRetention,
    PublicPresence,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MessageHour {
    // Published hours are immutable: on withdrawal omit the entire affected hour
    // from every subsequent snapshot, never republish a decremented total. This
    // producer-side obligation cannot be verified by this stateless reader.
    pub start: i64,
    pub contributions: Vec<Contribution>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Contribution {
    pub subject: String,
    pub count: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MessageSeries {
    pub subject: String,
    pub label: String,
    pub hours: Vec<CountHour>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CountHour {
    pub start: i64,
    pub count: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PresenceSeries {
    pub subject: String,
    pub label: String,
    pub transitions: Vec<PresenceTransition>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PresenceTransition {
    pub at: i64,
    pub status: DiscordStatus,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscordStatus {
    Online,
    Idle,
    DoNotDisturb,
    OfflineOrInvisible,
    Unknown,
}

impl DiscordStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Online => "online",
            Self::Idle => "idle",
            Self::DoNotDisturb => "do not disturb",
            Self::OfflineOrInvisible => "offline or invisible",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HealthInterval {
    pub start: i64,
    pub end: i64,
    pub process: Health,
    pub gateway: Health,
    pub collection: Health,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Health {
    Healthy,
    Unavailable,
    Unknown,
}

impl Health {
    pub fn label(self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::Unavailable => "unavailable",
            Self::Unknown => "unknown",
        }
    }
}
