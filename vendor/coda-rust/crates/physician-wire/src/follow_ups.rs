//! Typed wire contracts for the patient-page `Follow-ups` section:
//! listing, creating, editing, `Review now`, `Stop`, `Stop and discuss`,
//! `Resume`, `Complete`, `Cancel`, and `"Can't restart"` resolution.

use serde::{Deserialize, Serialize};

/// Origin of a patient follow-up item.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FollowUpOriginWire {
    /// Created directly by a physician on the patient page.
    PhysicianCreated {
        /// Physician identifier who created the follow-up.
        physician_id: String,
    },
    /// Created by an automated Coda review run.
    CodaCreated {
        /// Review run identifier.
        run_id: String,
    },
}

/// Closed lifecycle standing of a patient follow-up.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum FollowUpStandingWire {
    /// Active and scheduled for future review on or after `due_at`.
    Scheduled {
        /// RFC 3339 timestamp or ISO date label when due.
        due_at: String,
        /// Raw `YYYY-MM-DD` date string pre-populated in date inputs when editing.
        due_date_input: String,
    },
    /// Marked `Review now` and due for immediate patient chart review.
    DueNow {
        /// RFC 3339 timestamp when immediate review was requested.
        requested_at: String,
        /// Previous scheduled `YYYY-MM-DD` date string for inline editing.
        due_date_input: String,
    },
    /// Paused (`Stop` or `Stop and discuss`); runs no scheduled reviews until `Resume`.
    Held {
        /// RFC 3339 timestamp when held.
        paused_at: String,
        /// Optional pause note.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
        /// Retained `YYYY-MM-DD` due date string.
        due_date_input: String,
    },
    /// Automated review attempts exhausted (`"Can't restart"` on Today and patient page);
    /// requires `Complete`, `New date`, or `Cancel`.
    CannotRestart {
        /// Plain-language failure summary.
        reason: String,
        /// RFC 3339 timestamp when `Can't restart` was recorded.
        recorded_at: String,
        /// Retained `YYYY-MM-DD` due date string.
        due_date_input: String,
    },
    /// Terminal completed standing.
    Completed {
        /// RFC 3339 timestamp when completed.
        completed_at: String,
        /// Recorded completion note.
        resolution: String,
    },
    /// Terminal cancelled standing.
    Cancelled {
        /// RFC 3339 timestamp when cancelled.
        cancelled_at: String,
        /// Recorded cancellation reason.
        reason: String,
    },
}

impl FollowUpStandingWire {
    /// Physician-facing standing label.
    #[must_use]
    pub const fn badge_label(&self) -> &'static str {
        match self {
            Self::Scheduled { .. } => "Scheduled",
            Self::DueNow { .. } => "Reviewing now",
            Self::Held { .. } => "Held",
            Self::CannotRestart { .. } => "Can't restart",
            Self::Completed { .. } => "Completed",
            Self::Cancelled { .. } => "Cancelled",
        }
    }

    /// Whether the follow-up is still open (non-terminal).
    #[must_use]
    pub const fn is_open(&self) -> bool {
        matches!(
            self,
            Self::Scheduled { .. }
                | Self::DueNow { .. }
                | Self::Held { .. }
                | Self::CannotRestart { .. }
        )
    }
}

/// One durable patient follow-up returned for the patient page.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FollowUpWire {
    /// Stable follow-up identifier.
    pub follow_up_id: String,
    /// Authorized patient workspace identifier.
    pub patient_workspace_id: String,
    /// Monotonic version number incremented on every change.
    pub version: u64,
    /// Concise clinical summary naming what to check.
    pub summary: String,
    /// Current lifecycle standing.
    pub standing: FollowUpStandingWire,
    /// Who created this follow-up.
    pub origin: FollowUpOriginWire,
    /// Latest single-line change note, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_note: Option<String>,
    /// RFC 3339 timestamp when last updated.
    pub updated_at: String,
}

/// Response body for `GET /patient-workspaces/{id}/follow-ups`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PatientFollowUpsResponseWire {
    /// Authorized patient workspace identifier.
    pub patient_workspace_id: String,
    /// Whether durable follow-up creation and lifecycle controls are backed by the store.
    #[serde(default)]
    pub durable: bool,
    /// Ordered follow-ups (open items first, then settled items).
    #[serde(default)]
    pub follow_ups: Vec<FollowUpWire>,
}

/// Request body for `POST /patient-workspaces/{id}/follow-ups`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CreateFollowUpBody {
    /// Clinical summary of what Coda should check.
    pub summary: String,
    /// Target due date (`YYYY-MM-DD` or RFC 3339 timestamp).
    pub due_date: String,
    /// Client idempotency key.
    pub idempotency_key: String,
}

/// Request body for `PUT /patient-workspaces/{id}/follow-ups/{follow_up_id}`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpdateFollowUpBody {
    /// Updated clinical summary, when changed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// Updated due date (`YYYY-MM-DD` or RFC 3339 timestamp), when changed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due_date: Option<String>,
    /// Optional single-line note explaining the edit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Client idempotency key.
    pub idempotency_key: String,
}

/// Request body for `POST /patient-workspaces/{id}/follow-ups/{follow_up_id}/stop`
/// and `POST /patient-workspaces/{id}/follow-ups/{follow_up_id}/stop-and-discuss`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StopFollowUpBody {
    /// Optional single-line pause note.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Physician's IANA time zone (used when seeding a discussion turn).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub physician_time_zone: Option<String>,
    /// Client idempotency key.
    pub idempotency_key: String,
}

/// Request body for `POST /patient-workspaces/{id}/follow-ups/{follow_up_id}/resume`
/// and `POST /patient-workspaces/{id}/follow-ups/{follow_up_id}/review-now`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FollowUpActionCommandBody {
    /// Client idempotency key.
    pub idempotency_key: String,
}

/// Request body for `POST /patient-workspaces/{id}/follow-ups/{follow_up_id}/complete`
/// and `POST /patient-workspaces/{id}/follow-ups/{follow_up_id}/cancel`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SettleFollowUpBody {
    /// Single-line completion resolution or cancellation reason.
    pub note: String,
    /// Client idempotency key.
    pub idempotency_key: String,
}

/// Closed resolution choice for a follow-up in `"Can't restart"` standing.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "resolution_kind", rename_all = "snake_case")]
pub enum CannotRestartResolutionWire {
    /// Mark the follow-up completed with a single-line resolution note.
    Complete {
        /// Completion note.
        resolution: String,
    },
    /// Schedule a new review date with an optional single-line note.
    NewDate {
        /// New due date (`YYYY-MM-DD` or RFC 3339 timestamp).
        due_date: String,
        /// Optional single-line note.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
    },
    /// Cancel the follow-up with a single-line reason.
    Cancel {
        /// Cancellation reason.
        reason: String,
    },
}

/// Request body for `POST /patient-workspaces/{id}/follow-ups/{follow_up_id}/resolve-cannot-restart`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResolveCannotRestartBody {
    /// Chosen resolution (`Complete`, `NewDate`, or `Cancel`).
    pub resolution: CannotRestartResolutionWire,
    /// Client idempotency key.
    pub idempotency_key: String,
}

/// Response body for single-follow-up mutations.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FollowUpMutationResponseWire {
    /// Saved follow-up after the command.
    pub follow_up: FollowUpWire,
    /// Whether a background patient chart review was queued (`Review now`).
    #[serde(default)]
    pub review_started: bool,
    /// Durable patient conversation identifier opened and seeded by `Stop and discuss`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub discuss_chat_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follow_up_wire_roundtrips_all_standings_and_cannot_restart_resolutions() {
        let item = FollowUpWire {
            follow_up_id: "fu-1".into(),
            patient_workspace_id: "ws-1".into(),
            version: 3,
            summary: "Recheck BMP potassium after lisinopril increase".into(),
            standing: FollowUpStandingWire::CannotRestart {
                reason: "Three consecutive review runs failed on transient lab timeout".into(),
                recorded_at: "2026-04-15T12:00:00Z".into(),
                due_date_input: "2026-04-20".into(),
            },
            origin: FollowUpOriginWire::PhysicianCreated {
                physician_id: "phys-1".into(),
            },
            latest_note: Some("Moved out 2 days".into()),
            updated_at: "2026-04-15T12:00:00Z".into(),
        };
        let encoded = serde_json::to_string(&item).expect("serialize");
        let decoded: FollowUpWire = serde_json::from_str(&encoded).expect("deserialize");
        assert_eq!(decoded, item);

        let resolve = ResolveCannotRestartBody {
            resolution: CannotRestartResolutionWire::NewDate {
                due_date: "2026-04-25".into(),
                note: Some("Retry next week".into()),
            },
            idempotency_key: "cmd-1".into(),
        };
        let resolve_json = serde_json::to_string(&resolve).expect("serialize resolve");
        let parsed: ResolveCannotRestartBody =
            serde_json::from_str(&resolve_json).expect("parse resolve");
        assert_eq!(parsed, resolve);
    }
}
