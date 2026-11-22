//! Wire contracts for saved patient reviews and their controls on the patient page.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{CitedSourceWire, MessageSegment};

/// Authoritative state version of one saved patient review.
///
/// Every control request (`Stop`, `Resume`, `Try again`) sends both the review
/// identifier and this version so the server refuses stale control commands.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ReviewVersion(pub String);

impl ReviewVersion {
    /// Returns the version token string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Which control action a physician can request on a patient review.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewControlActionWire {
    /// Hold a pending or actively running review (`Stop`).
    Stop,
    /// Continue a review intentionally held by `Stop` (`Resume`).
    Resume,
    /// Retry a review that reached a limit or exhausted its attempt allowance (`Try again`).
    TryAgain,
}

impl ReviewControlActionWire {
    /// Physician-facing button label for this control.
    #[must_use]
    pub const fn button_label(self) -> &'static str {
        match self {
            Self::Stop => "Stop",
            Self::Resume => "Resume",
            Self::TryAgain => "Try again",
        }
    }

    /// Pending status sentence shown while waiting for the server's answer.
    #[must_use]
    pub const fn pending_message(self) -> &'static str {
        match self {
            Self::Stop => "Stopping review…",
            Self::Resume => "Resuming review…",
            Self::TryAgain => "Retrying review…",
        }
    }

    /// Confirmation sentence shown once the server accepts the control.
    #[must_use]
    pub const fn done_message(self) -> &'static str {
        match self {
            Self::Stop => "Review stopped.",
            Self::Resume => "Review resumed.",
            Self::TryAgain => "Review started again.",
        }
    }
}

/// Why a held patient review awaits physician action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewHoldReasonWire {
    /// The physician stopped the review before it finished; `Resume` continues it.
    Stopped,
    /// The review reached a turn or budget limit; `Try again` continues it.
    Limit,
    /// The model provider refused the request (for example exhausted billing or invalid auth); `Try again` continues it.
    ProviderBlocked {
        /// Provider-facing refusal summary preserved for the physician.
        reason: String,
    },
}

/// Why a patient review was started.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReviewTriggerWire {
    /// Started because one or more patient chart records arrived, changed, or were withdrawn.
    ChangedRecords {
        /// Workspace source version observed when triggered.
        workspace_version: u64,
        /// Human-readable titles or identifiers of changed records when available.
        #[serde(default)]
        changed_records: Vec<String>,
    },
    /// Started because the patient's periodic review window elapsed.
    Periodic {
        /// Workspace source version observed when triggered.
        workspace_version: u64,
        /// RFC 3339 timestamp of the prior completed review when present.
        #[serde(default)]
        last_reviewed_at: Option<String>,
    },
    /// Started at a physician's explicit request.
    PhysicianRequest {
        /// Requesting physician identifier.
        physician_id: String,
        /// Clinical focus or question supplied by the physician.
        request_text: String,
        /// Workspace source version observed when requested.
        workspace_version: u64,
    },
}

impl ReviewTriggerWire {
    /// Short physician-facing headline for the trigger.
    #[must_use]
    pub fn headline(&self) -> &'static str {
        match self {
            Self::ChangedRecords { .. } => "New records",
            Self::Periodic { .. } => "Periodic review",
            Self::PhysicianRequest { .. } => "Physician request",
        }
    }
}

/// Follow-up direction accepted onto a review before completion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewAcceptedInputWire {
    /// Stable input identifier.
    pub id: String,
    /// Accepted follow-up text.
    pub text: String,
    /// RFC 3339 timestamp when accepted.
    pub recorded_at: String,
}

/// Where one recorded tool or model step of a review stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewStepStateWire {
    /// Currently running.
    Running,
    /// Completed normally.
    Done,
    /// Ended with an error.
    Failed,
    /// Stopped before finishing.
    Stopped,
}

/// One inspectable step from the review's run trace (shown in working and stopped disclosures).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewStepWire {
    /// Short human-readable step description (for example `"Read BMP panel"`).
    pub label: String,
    /// Optional supporting detail (for example source name or count).
    #[serde(default)]
    pub detail: Option<String>,
    /// Current state of this step.
    pub state: ReviewStepStateWire,
}

/// Completed clinical disposition of a finished patient review.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "disposition", rename_all = "snake_case")]
pub enum PatientReviewOutcomeWire {
    /// Completed quietly with `"nothing to do"` — nothing required physician attention.
    Quiet {
        /// Concise clinical explanation of what Coda verified.
        explanation: String,
        /// Prose and inline citation segments for the explanation.
        #[serde(default)]
        segments: Vec<MessageSegment>,
        /// Basis and limitations prose and inline citation segments.
        #[serde(default)]
        basis: Vec<MessageSegment>,
    },
    /// Completed with clinical findings, concern updates, or longitudinal revisions.
    Assessed {
        /// Clinical synthesis of what changed and what was decided.
        summary: String,
        /// Prose and inline citation segments for the summary.
        #[serde(default)]
        segments: Vec<MessageSegment>,
        /// Basis and limitations prose and inline citation segments.
        #[serde(default)]
        basis: Vec<MessageSegment>,
    },
}

/// Why an incomplete review was superseded without finishing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReviewSupersededReasonWire {
    /// Practice patient access was withdrawn before completion.
    AccessLost {
        /// Diagnostic detail.
        detail: String,
    },
    /// A newer review or input superseded this review.
    NewInput {
        /// Replacement review identifier when one took over.
        #[serde(default)]
        replacement_review_id: Option<String>,
    },
}

/// Authoritative lifecycle state of a saved patient review.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PatientReviewStateWire {
    /// Accepted and waiting for a background worker to claim it.
    Pending,
    /// Actively executing on a background worker.
    Running {
        /// Active attempt identifier.
        attempt_id: String,
        /// RFC 3339 timestamp when execution began.
        started_at: String,
    },
    /// Paused awaiting explicit physician continuation (`Resume` or `Try again`).
    Held {
        /// Why the review is held.
        reason: ReviewHoldReasonWire,
        /// RFC 3339 timestamp when held.
        held_at: String,
        /// Partial output retained before the hold, when present.
        #[serde(default)]
        partial_text: Option<String>,
    },
    /// Finished and durably committed.
    Completed {
        /// Assessed or quiet outcome.
        outcome: PatientReviewOutcomeWire,
        /// RFC 3339 timestamp when committed.
        completed_at: String,
    },
    /// Exhausted retry allowance or blocked by provider failure; recoverable via `Try again`.
    Failed {
        /// Failure summary.
        reason: String,
        /// RFC 3339 timestamp when settled as failed.
        failed_at: String,
    },
    /// Superseded without completing.
    Superseded {
        /// Why the review was superseded.
        reason: ReviewSupersededReasonWire,
        /// RFC 3339 timestamp when superseded.
        superseded_at: String,
    },
}

impl PatientReviewStateWire {
    /// Single-owner mapping from lifecycle state to the control offered on the patient page.
    #[must_use]
    pub const fn available_control(&self) -> Option<ReviewControlActionWire> {
        match self {
            Self::Pending | Self::Running { .. } => Some(ReviewControlActionWire::Stop),
            Self::Held {
                reason: ReviewHoldReasonWire::Stopped,
                ..
            } => Some(ReviewControlActionWire::Resume),
            Self::Held {
                reason: ReviewHoldReasonWire::Limit | ReviewHoldReasonWire::ProviderBlocked { .. },
                ..
            }
            | Self::Failed { .. } => Some(ReviewControlActionWire::TryAgain),
            Self::Completed { .. } | Self::Superseded { .. } => None,
        }
    }

    /// Whether the review is currently queued or actively executing (`Reviewing`).
    #[must_use]
    pub const fn is_working(&self) -> bool {
        matches!(self, Self::Pending | Self::Running { .. })
    }

    /// Short physician-facing badge label for the review row.
    #[must_use]
    pub const fn badge_label(&self) -> &'static str {
        match self {
            Self::Pending | Self::Running { .. } => "Reviewing",
            Self::Held {
                reason: ReviewHoldReasonWire::Stopped,
                ..
            } => "Stopped",
            Self::Held {
                reason: ReviewHoldReasonWire::Limit | ReviewHoldReasonWire::ProviderBlocked { .. },
                ..
            }
            | Self::Failed { .. } => "Couldn’t finish",
            Self::Completed {
                outcome: PatientReviewOutcomeWire::Quiet { .. },
                ..
            } => "Reviewed, nothing to do",
            Self::Completed {
                outcome: PatientReviewOutcomeWire::Assessed { .. },
                ..
            } => "Reviewed",
            Self::Superseded { .. } => "Superseded",
        }
    }
}

/// One evidence record referenced or read by a patient review.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewEvidenceRefWire {
    /// Key into [`PatientReviewWire::sources`] so clicking opens the citation reader.
    pub source_key: String,
    /// 1-based reference number within the review.
    pub number: usize,
    /// Opaque candidate record identifier.
    pub candidate_id: String,
    /// Immutable source revision read by Coda.
    pub revision: String,
    /// Physician-facing source name.
    pub source_label: String,
    /// Record display title.
    pub display_label: String,
    /// Primary date on the record (`YYYY-MM-DD`), when present.
    #[serde(default)]
    pub as_of: Option<String>,
    /// Verified excerpt or summary preview.
    pub excerpt: String,
}

/// Durable action committed during a patient review.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewActionWire {
    /// Short physician-facing description of the action taken.
    pub summary: String,
}

/// Kind of evidence limit blocking resolution of an unfinished-work item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewLimitKindWire {
    /// Expected external record was not present.
    Missing,
    /// Record or attachment could not be parsed or read.
    Unreadable,
    /// Connected source was temporarily unavailable.
    Unavailable,
}

impl ReviewLimitKindWire {
    /// Short physician-facing label for the limit kind.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Missing => "Missing record",
            Self::Unreadable => "Unreadable record",
            Self::Unavailable => "Source unavailable",
        }
    }
}

/// Unfinished follow-up item or unresolved evidence gap recorded on a patient review.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewUnfinishedWorkWire {
    /// Short clinical topic naming the unresolved question or pending record.
    pub topic: String,
    /// Specific follow-up check or missing evidence needed on future reassessment.
    pub detail: String,
    /// Optional structured evidence limit kind (`missing`, `unreadable`, or `unavailable`).
    #[serde(default)]
    pub limit_kind: Option<ReviewLimitKindWire>,
    /// Optional detail describing the missing or unavailable source.
    #[serde(default)]
    pub limit_detail: Option<String>,
}

/// One saved patient review returned for the patient page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatientReviewWire {
    /// Stable review identifier within the patient workspace.
    pub review_id: String,
    /// Authoritative version sent back with every control command.
    pub version: ReviewVersion,
    /// Underlying run-store work item identifier.
    pub work_id: String,
    /// Chronological triggers that started or expanded this review.
    pub triggers: Vec<ReviewTriggerWire>,
    /// Follow-up inputs accepted before completion.
    #[serde(default)]
    pub accepted_inputs: Vec<ReviewAcceptedInputWire>,
    /// Current lifecycle state and completed outcome when finished.
    pub state: PatientReviewStateWire,
    /// Inspectable run trace steps for active and stopped disclosures.
    #[serde(default)]
    pub steps: Vec<ReviewStepWire>,
    /// Evidence references read or cited by this review.
    #[serde(default)]
    pub evidence: Vec<ReviewEvidenceRefWire>,
    /// Resolved cited sources keyed by `source_key` for opening in the reader dialog.
    #[serde(default)]
    pub sources: BTreeMap<String, CitedSourceWire>,
    /// Durable actions recorded under this review's work.
    #[serde(default)]
    pub actions: Vec<ReviewActionWire>,
    /// Unfinished work recorded for subsequent reviews.
    #[serde(default)]
    pub unfinished_work: Vec<ReviewUnfinishedWorkWire>,
    /// RFC 3339 timestamp when queued.
    pub queued_at: String,
    /// RFC 3339 timestamp of the latest state transition.
    pub updated_at: String,
}

/// Response returned by `GET /patient-workspaces/{id}/reviews`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatientReviewsResponse {
    /// Saved patient reviews for this workspace, newest update first.
    #[serde(default)]
    pub reviews: Vec<PatientReviewWire>,
}

/// Request sent to `POST /patient-workspaces/{id}/reviews/{review_id}/control`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewControlRequest {
    /// Target review identifier.
    pub review_id: String,
    /// Exact version of the review shown when the physician chose the control.
    pub version: ReviewVersion,
    /// Requested control transition (`stop`, `resume`, or `try_again`).
    pub action: ReviewControlActionWire,
    /// Client-generated idempotency key reused on lost-receipt retries.
    pub idempotency_key: String,
}

/// Structured reason why the server refused a review control command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewControlRefusalReason {
    /// The review moved to a newer version before the command arrived.
    StaleVersion,
    /// The requested control is invalid for the review's current lifecycle state.
    InvalidState,
    /// The idempotency key was already used with a different command payload.
    IdempotencyCollision,
    /// The request fields failed validation.
    InvalidRequest,
}

/// Server response to `POST /patient-workspaces/{id}/reviews/{review_id}/control`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ReviewControlResponse {
    /// The control took effect (or matched an earlier accepted receipt) and returned the updated review.
    Accepted {
        /// Authoritative updated review state.
        review: Box<PatientReviewWire>,
    },
    /// The server refused the control without mutating the review.
    Refused {
        /// Structured refusal category.
        reason: ReviewControlRefusalReason,
        /// Physician-facing explanation of why the command was refused.
        message: String,
        /// Current authoritative review state when available so the client can reconcile.
        #[serde(default)]
        review: Option<Box<PatientReviewWire>>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn available_control_matches_each_review_lifecycle_state() {
        assert_eq!(
            PatientReviewStateWire::Pending.available_control(),
            Some(ReviewControlActionWire::Stop)
        );
        assert_eq!(
            PatientReviewStateWire::Running {
                attempt_id: "att-1".into(),
                started_at: "2026-08-27T12:00:00Z".into(),
            }
            .available_control(),
            Some(ReviewControlActionWire::Stop)
        );
        assert_eq!(
            PatientReviewStateWire::Held {
                reason: ReviewHoldReasonWire::Stopped,
                held_at: "2026-08-27T12:01:00Z".into(),
                partial_text: None,
            }
            .available_control(),
            Some(ReviewControlActionWire::Resume)
        );
        assert_eq!(
            PatientReviewStateWire::Held {
                reason: ReviewHoldReasonWire::Limit,
                held_at: "2026-08-27T12:01:00Z".into(),
                partial_text: Some("Partial synthesis".into()),
            }
            .available_control(),
            Some(ReviewControlActionWire::TryAgain)
        );
        let blocked = PatientReviewStateWire::Held {
            reason: ReviewHoldReasonWire::ProviderBlocked {
                reason: "Provider credits exhausted.".into(),
            },
            held_at: "2026-08-27T12:01:30Z".into(),
            partial_text: None,
        };
        assert_eq!(
            blocked.available_control(),
            Some(ReviewControlActionWire::TryAgain)
        );
        assert_eq!(blocked.badge_label(), "Couldn’t finish");
        let encoded = serde_json::to_string(&blocked).unwrap();
        let decoded: PatientReviewStateWire = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, blocked);
        assert_eq!(
            PatientReviewStateWire::Failed {
                reason: "provider timeout".into(),
                failed_at: "2026-08-27T12:02:00Z".into(),
            }
            .available_control(),
            Some(ReviewControlActionWire::TryAgain)
        );
        assert_eq!(
            PatientReviewStateWire::Completed {
                outcome: PatientReviewOutcomeWire::Quiet {
                    explanation: "Routine labs stable.".into(),
                    segments: Vec::new(),
                    basis: Vec::new(),
                },
                completed_at: "2026-08-27T12:03:00Z".into(),
            }
            .available_control(),
            None
        );
        assert_eq!(
            PatientReviewStateWire::Superseded {
                reason: ReviewSupersededReasonWire::NewInput {
                    replacement_review_id: Some("rev-2".into()),
                },
                superseded_at: "2026-08-27T12:04:00Z".into(),
            }
            .available_control(),
            None
        );
    }

    #[test]
    fn control_request_and_response_round_trip_with_review_id_and_version() {
        let req = ReviewControlRequest {
            review_id: "rev-123".into(),
            version: ReviewVersion("rv-abc123".into()),
            action: ReviewControlActionWire::Stop,
            idempotency_key: "ctl-1".into(),
        };
        let json = serde_json::to_value(&req).expect("serialize request");
        assert_eq!(json["review_id"], "rev-123");
        assert_eq!(json["version"], "rv-abc123");
        assert_eq!(json["action"], "stop");
        assert_eq!(json["idempotency_key"], "ctl-1");
        let parsed: ReviewControlRequest = serde_json::from_value(json).expect("deserialize");
        assert_eq!(parsed, req);
    }
}
