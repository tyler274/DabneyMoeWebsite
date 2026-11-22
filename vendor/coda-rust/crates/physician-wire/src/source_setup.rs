//! Practice-facing clinical source connection, backlog acceptance, and recovery wire types.

use serde::{Deserialize, Serialize};

/// Supported practice-facing clinical and messaging sources.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupportedSourceWire {
    /// Hint Health membership and clinical chart connector (Chrome browser session).
    Hint,
    /// Elation / FHIR clinical record connection.
    ElationFhir,
    /// Spruce Health patient messaging, attachment, and fax channel.
    Spruce,
}

impl SupportedSourceWire {
    /// Stable URL path segment for `/connections/{source_id}/...`.
    pub fn slug(self) -> &'static str {
        match self {
            Self::Hint => "hint",
            Self::ElationFhir => "elation_fhir",
            Self::Spruce => "spruce",
        }
    }

    /// Parses a URL path segment into its closed [`SupportedSourceWire`] variant.
    pub fn from_slug(slug: &str) -> Option<Self> {
        match slug.trim() {
            "hint" => Some(Self::Hint),
            "elation_fhir" | "elation-fhir" => Some(Self::ElationFhir),
            "spruce" => Some(Self::Spruce),
            _ => None,
        }
    }

    /// Physician-facing source name.
    pub fn label(self) -> &'static str {
        match self {
            Self::Hint => "Hint Health",
            Self::ElationFhir => "Elation / FHIR",
            Self::Spruce => "Spruce Health",
        }
    }

    /// All supported practice sources in display order.
    pub const ALL: [Self; 3] = [Self::Hint, Self::ElationFhir, Self::Spruce];
}

/// How a practice connects a source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceConnectionMethodWire {
    /// Direct credential or API grant verification.
    CredentialSignIn,
    /// Authenticated Chrome connector browser session (Hint Health).
    BrowserSession,
    /// Simulated practice access for staging walkthroughs and rehearsals.
    SimulatedPractice,
    /// Request Coda staff to assist with practice source onboarding.
    CodaAssisted,
}

impl SourceConnectionMethodWire {
    /// Physician-facing label for the connection method.
    pub fn label(self) -> &'static str {
        match self {
            Self::CredentialSignIn => "Sign in / API credentials",
            Self::BrowserSession => "Chrome connector session",
            Self::SimulatedPractice => "Simulated practice walkthrough",
            Self::CodaAssisted => "Coda-assisted setup",
        }
    }
}

/// Why a source lost its authorization grant (`Rule 10`: only these causes ask for `Reconnect`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccessLossCauseWire {
    /// The practice's OAuth / API / browser session grant was revoked or expired permanently.
    RevokedGrant,
    /// A practice or workspace administrator blocked Coda's access.
    AdministratorBlock,
}

impl AccessLossCauseWire {
    /// Short physician-facing summary of the access loss cause.
    pub fn label(self) -> &'static str {
        match self {
            Self::RevokedGrant => "Access revoked",
            Self::AdministratorBlock => "Blocked by administrator",
        }
    }
}

/// Closed algebraic standing of a practice source (`Connected`, `Records imported`, and
/// `Up to date` are distinct variants so a credential check never implies records were imported).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SourceStandingWire {
    /// Not connected.
    Disconnected,
    /// Practice requested Coda-assisted onboarding; awaiting credential completion.
    CodaAssistedRequested {
        /// RFC-3339 timestamp when assistance was requested.
        requested_at: String,
        /// Practice contact email for Coda onboarding staff.
        contact_email: String,
        /// Optional practice notes or preferred window.
        #[serde(default)]
        notes: Option<String>,
    },
    /// Credentials or browser session verified; zero records imported yet.
    Connected {
        /// RFC-3339 timestamp when credentials or session were verified.
        connected_at: String,
        /// How the connection was established.
        method: SourceConnectionMethodWire,
        /// Account, workspace, or endpoint label verified during connection.
        account_label: String,
    },
    /// Initial records have been imported, before a subsequent refresh confirms `UpToDate`.
    RecordsImported {
        /// RFC-3339 timestamp when the source was connected.
        connected_at: String,
        /// How the connection was established.
        method: SourceConnectionMethodWire,
        /// Account, workspace, or endpoint label.
        account_label: String,
        /// Count of patient chart records currently saved from this source.
        imported_records: usize,
        /// RFC-3339 timestamp when initial records finished importing.
        last_imported_at: String,
    },
    /// Source has completed a clean refresh and its saved records are current.
    UpToDate {
        /// RFC-3339 timestamp when the source was connected.
        connected_at: String,
        /// How the connection was established.
        method: SourceConnectionMethodWire,
        /// Account, workspace, or endpoint label.
        account_label: String,
        /// Count of patient chart records currently saved from this source.
        imported_records: usize,
        /// RFC-3339 timestamp of the latest successful read.
        last_read_at: String,
    },
    /// Transient read failure: retains the failure reason and last successful read time,
    /// keeps previously saved records readable, and never asks for `Reconnect`.
    ReadFailed {
        /// RFC-3339 timestamp when the source was connected.
        connected_at: String,
        /// How the connection was established.
        method: SourceConnectionMethodWire,
        /// Account, workspace, or endpoint label.
        account_label: String,
        /// Count of previously saved patient chart records still readable.
        imported_records: usize,
        /// RFC-3339 timestamp of the last successful read, if one completed before the failure.
        #[serde(default)]
        last_successful_read_at: Option<String>,
        /// RFC-3339 timestamp when the read failed.
        failed_at: String,
        /// Specific physician-facing reason the read could not finish.
        reason: String,
    },
    /// Permanent authorization loss (`RevokedGrant` or `AdministratorBlock`): retains the specific
    /// reason and last successful read time, keeps previously saved records readable, and offers
    /// `Reconnect`.
    AccessLost {
        /// RFC-3339 timestamp when the source was connected.
        connected_at: String,
        /// How the connection was established.
        method: SourceConnectionMethodWire,
        /// Account, workspace, or endpoint label.
        account_label: String,
        /// Count of previously saved patient chart records still readable.
        imported_records: usize,
        /// RFC-3339 timestamp of the last successful read, if one completed before access was lost.
        #[serde(default)]
        last_successful_read_at: Option<String>,
        /// RFC-3339 timestamp when access was lost.
        lost_at: String,
        /// Why authorization was lost.
        cause: AccessLossCauseWire,
        /// Specific physician-facing recovery guidance and failure reason.
        reason: String,
    },
    /// Spruce filing was halted after detecting an older running release behind a newer
    /// release that already touched the practice channel; persists until operator resolution.
    RollbackHalted {
        /// RFC-3339 timestamp when the source was connected.
        connected_at: String,
        /// How the connection was established.
        method: SourceConnectionMethodWire,
        /// Account, workspace, or endpoint label.
        account_label: String,
        /// Count of saved records/messages still readable.
        imported_records: usize,
        /// RFC-3339 timestamp of the last successful read before the halt.
        #[serde(default)]
        last_successful_read_at: Option<String>,
        /// RFC-3339 timestamp when the rollback halt fired.
        halted_at: String,
        /// Release label of the older binary that hit the fence.
        running_release: String,
        /// Highest release label previously recorded on the practice channel.
        highest_release: String,
        /// Operator-facing explanation of why filing is halted.
        reason: String,
    },
}

impl SourceStandingWire {
    /// Short physician-facing badge label for this exact saved standing.
    pub fn pill_label(&self) -> &'static str {
        match self {
            Self::Disconnected => "Disconnected",
            Self::CodaAssistedRequested { .. } => "Coda-assisted setup requested",
            Self::Connected { .. } => "Connected",
            Self::RecordsImported { .. } => "Records imported",
            Self::UpToDate { .. } => "Up to date",
            Self::ReadFailed { .. } => "Records couldn’t be updated",
            Self::AccessLost { .. } => "Access lost",
            Self::RollbackHalted { .. } => "Filing halted",
        }
    }

    /// Single owner of `Rule 10`: whether this standing asks the physician to `Reconnect`.
    /// Transient `ReadFailed` never asks to `Reconnect`; only `AccessLost` does.
    pub fn requires_reconnect(&self) -> bool {
        matches!(self, Self::AccessLost { .. })
    }

    /// Whether the source is currently connected (including degraded read or lost-access states
    /// that can be disconnected).
    pub fn can_disconnect(&self) -> bool {
        !matches!(self, Self::Disconnected)
    }
}

/// Standing of pre-connection historical Spruce backlog (`Rule 14`: read only after acceptance).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SpruceBacklogWire {
    /// Source is not Spruce.
    NotApplicable,
    /// No historical backlog is waiting for review.
    NonePending,
    /// Historical messages predate connection and require explicit acceptance before reading.
    Offered {
        /// Number of historical threads/messages held outside clinical reading until accepted.
        message_count: usize,
        /// RFC-3339 timestamp of the earliest held historical message.
        earliest_message_at: String,
    },
    /// Physician explicitly accepted backlog review; historical messages were imported.
    Accepted {
        /// RFC-3339 timestamp when backlog review was accepted.
        accepted_at: String,
        /// Number of historical messages imported upon acceptance.
        imported_message_count: usize,
    },
}

/// Structured Today recovery classification for a source needing attention.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TodayConnectionRecoveryWire {
    /// Transient read failure: show reason + last read time and offer `Open Connections` (never `Reconnect`).
    ReadFailed,
    /// Revoked grant or administrator block: show reason + last read time and offer `Reconnect`.
    ReconnectRequired,
    /// Release rollback fence halted filing until explicitly resolved by an operator.
    OperatorActionRequired,
}

/// Full projection for one practice source on `GET /connections` and mutation responses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PracticeSourceWire {
    /// Source identifier.
    pub source_id: SupportedSourceWire,
    /// Physician-facing source name.
    pub label: String,
    /// Short summary of what records this source supplies.
    pub summary: String,
    /// Setup guidance shown before and during connection.
    pub guidance: String,
    /// Concrete prerequisites the practice needs before connecting.
    pub prerequisites: Vec<String>,
    /// Connection routes supported by this source.
    pub supported_methods: Vec<SourceConnectionMethodWire>,
    /// Current saved standing (`Connected`, `Records imported`, `Up to date`, etc.).
    pub standing: SourceStandingWire,
    /// Historical backlog prompt standing (Spruce only; `NotApplicable` elsewhere).
    pub backlog: SpruceBacklogWire,
    /// Count of never-linked inbound Spruce messages currently held in staging (`0` after disconnect).
    pub unlinked_spruce_messages: usize,
    /// Count of linked clinical records retained under the practice record policy.
    pub retained_linked_records: usize,
    /// Count of physician reply drafts retained under the practice record policy.
    pub retained_reply_drafts: usize,
}

/// Response from `GET /connections`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PracticeConnectionsWire {
    /// Practice identifier owning these connections.
    pub practice_id: String,
    /// Ordered list of supported sources and their current saved standing.
    pub sources: Vec<PracticeSourceWire>,
}

/// Request body for `POST /connections/{source_id}/connect` and `/reconnect`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectSourceBody {
    /// Chosen connection method (`credential_sign_in`, `browser_session`, or `simulated_practice`).
    pub method: SourceConnectionMethodWire,
    /// Account email, FHIR endpoint URL, or connector profile label.
    pub account_label: String,
    /// Credential secret, API token, or browser session verification code (optional in `simulated_practice`).
    #[serde(default)]
    pub secret_or_code: Option<String>,
    /// Confirmed practice prerequisites (required for Hint Chrome connector session setup).
    #[serde(default)]
    pub prerequisites_confirmed: bool,
}

/// Request body for `POST /connections/{source_id}/assist`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestAssistedSetupBody {
    /// Practice contact email for Coda onboarding staff.
    pub contact_email: String,
    /// Optional notes about clinic EHR administrator or preferred setup window.
    #[serde(default)]
    pub notes: Option<String>,
}

/// Response from any single-source mutation on `/connections/{source_id}/*`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceOperationResponseWire {
    /// Practice identifier.
    pub practice_id: String,
    /// Updated saved projection of the mutated source.
    pub source: PracticeSourceWire,
}

/// Staging/test simulation event on `POST /connections/{source_id}/simulate` to exercise
/// quiet catch-up/retry, transient read failure, and revoked/blocked access recovery.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum SimulateSourceEventBody {
    /// Background Spruce catch-up in progress (must remain completely silent in Today & UI).
    QuietCatchUp,
    /// Background transient retry in progress (must remain completely silent in Today & UI).
    QuietTransientRetry {
        /// Current retry attempt number.
        attempt: u32,
    },
    /// Clear background catch-up / retry activity back to idle.
    ClearQuietActivity,
    /// Record a surfaced read failure while preserving `last_successful_read_at` and saved records.
    TransientReadFailure {
        /// Specific reason the read failed.
        reason: String,
    },
    /// Record a revoked OAuth/API/session grant (`Rule 10`: asks for `Reconnect`).
    RevokedGrant {
        /// Specific reason access was lost.
        reason: String,
    },
    /// Record an administrator block (`Rule 10`: asks for `Reconnect`).
    AdministratorBlock {
        /// Specific administrator block guidance.
        reason: String,
    },
}
