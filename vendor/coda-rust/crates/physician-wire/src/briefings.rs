//! Wire contracts for pre-visit briefings, briefing history, and schedule-source errors.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{CitedSourceWire, MessageSegment};

/// Calendar encounter identity carried on a schedule visit row.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScheduleEncounterWire {
    /// Calendar source code (`source_id`).
    #[serde(default)]
    pub source_code: String,
    /// Native calendar appointment identifier.
    #[serde(default)]
    pub native_event_id: String,
    /// Calendar source's event revision when provided.
    #[serde(default)]
    pub source_event_revision: Option<String>,
}

/// Latest schedule refresh error attached to [`crate::PracticeSourceState`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PracticeSourceErrorWire {
    /// RFC-3339 timestamp when the failed schedule read occurred.
    pub failed_at: String,
    /// Human-readable explanation of why the schedule refresh failed.
    pub reason: String,
}

/// Whether a saved visit briefing still matches its scheduled slot or is kept in history.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "standing", rename_all = "snake_case")]
pub enum BriefingVisitStandingWire {
    /// Appointment remains scheduled at the prepared start time.
    Current,
    /// Appointment moved to a different start time; this briefing is preserved for the earlier slot.
    Moved {
        /// New scheduled start timestamp (RFC-3339).
        new_starts_at: String,
        /// Timestamp when the schedule move was recorded (RFC-3339).
        noted_at: String,
    },
    /// Appointment was cancelled or withdrawn; this briefing is preserved in history.
    Cancelled {
        /// Timestamp when the cancellation was recorded (RFC-3339).
        noted_at: String,
    },
    /// Appointment slot has ended or completed; this briefing is preserved in history.
    Ended {
        /// Scheduled end timestamp of the completed visit slot (RFC-3339).
        ended_at: String,
    },
}

/// Freshness standing of one connected chart source when a visit briefing was saved.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum BriefingSourceStatusWire {
    /// Connected source had a current pull with no newer refresh failure.
    Updated {
        /// Stable source identifier.
        source_id: String,
        /// Physician-facing source label.
        source_label: String,
        /// Timestamp of the last good pull (RFC-3339).
        last_good_at: String,
    },
    /// Connected source failed its latest refresh; an earlier pull may still be held.
    RefreshFailed {
        /// Stable source identifier.
        source_id: String,
        /// Physician-facing source label.
        source_label: String,
        /// Timestamp of the retained good pull when one exists (RFC-3339).
        #[serde(default)]
        last_good_at: Option<String>,
        /// Timestamp of the failed refresh check (RFC-3339).
        failed_at: String,
        /// Failure detail when reported.
        #[serde(default)]
        reason: Option<String>,
    },
    /// Connected source had not completed any good pull when the briefing was saved.
    Unavailable {
        /// Stable source identifier.
        source_id: String,
        /// Physician-facing source label.
        source_label: String,
    },
}

/// Kind of chart gap or limitation recorded on a saved briefing.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BriefingLimitKindWire {
    /// Expected record or document was not present in the connected sources.
    Missing,
    /// Connected source could not be reached.
    Unavailable,
    /// Record was present in a source but could not be read.
    Unreadable,
}

impl BriefingLimitKindWire {
    /// Short physician-facing label for this limitation kind.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Missing => "Missing records",
            Self::Unavailable => "Source unavailable",
            Self::Unreadable => "Unreadable record",
        }
    }
}

/// Explicit record limitation carried on a saved visit briefing.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BriefingLimitWire {
    /// Category of missing, late, unreadable, or unavailable evidence.
    pub kind: BriefingLimitKindWire,
    /// Grounded explanation shown to the physician.
    pub detail: String,
}

/// One cited point in Trajectory, Meaningful changes, or Unresolved issues.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BriefingClaimWire {
    /// Prose and numbered inline citation chips for this point.
    pub segments: Vec<MessageSegment>,
    /// Inline uncertainty or conflicting-record qualification when present.
    #[serde(default)]
    pub uncertainty: Option<String>,
}

/// One open clinical decision or question surfaced for physician consideration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BriefingDecisionWire {
    /// Open decision or question headline.
    pub decision: String,
    /// Cited clinical rationale for considering this decision during the visit.
    pub rationale_segments: Vec<MessageSegment>,
    /// Concrete options or questions to discuss with the patient.
    #[serde(default)]
    pub options_or_questions: Vec<String>,
    /// Inline uncertainty qualification when present.
    #[serde(default)]
    pub uncertainty: Option<String>,
}

/// Full structured pre-visit briefing returned by `GET /patient-workspaces/{id}/briefings/{briefing_id}`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VisitBriefingWire {
    /// Durable briefing identifier.
    pub briefing_id: String,
    /// Patient workspace identifier.
    pub patient_workspace_id: String,
    /// Patient display name.
    pub patient_display_name: String,
    /// Calendar source identifier (`source_id`).
    pub source_id: String,
    /// Calendar appointment identifier.
    pub appointment_id: String,
    /// Visit label from the practice schedule.
    pub visit_label: String,
    /// Scheduled visit start timestamp for which this briefing was prepared (RFC-3339).
    pub starts_at: String,
    /// Scheduled visit end timestamp (RFC-3339).
    pub ends_at: String,
    /// Timestamp when this briefing was saved (RFC-3339).
    pub prepared_at: String,
    /// Current vs moved vs cancelled standing of the scheduled slot.
    pub visit_standing: BriefingVisitStandingWire,
    /// Concise visit orientation shown at the top of the briefing and on the ready visit card.
    pub visit_card_orientation: String,
    /// Connected chart source freshness when the briefing was prepared.
    #[serde(default)]
    pub source_freshness: Vec<BriefingSourceStatusWire>,
    /// Missing, late, unreadable, or unavailable source disclosures.
    #[serde(default)]
    pub limits: Vec<BriefingLimitWire>,
    /// Clinical trajectory points with inline citations.
    #[serde(default)]
    pub trajectory: Vec<BriefingClaimWire>,
    /// Meaningful changes since the last visit with inline citations.
    #[serde(default)]
    pub meaningful_changes: Vec<BriefingClaimWire>,
    /// Unresolved clinical issues with inline citations.
    #[serde(default)]
    pub unresolved_issues: Vec<BriefingClaimWire>,
    /// Open decisions and questions for physician consideration during the visit.
    #[serde(default)]
    pub visit_decisions: Vec<BriefingDecisionWire>,
    /// Basis and limitations prose with inline citation chips.
    #[serde(default)]
    pub basis_segments: Vec<MessageSegment>,
    /// Cited records keyed by `source_key` (`b1`, `b2`, ...) for side-panel inspection and the Sources list.
    #[serde(default)]
    pub sources: BTreeMap<String, CitedSourceWire>,
}

/// Response envelope for `GET /patient-workspaces/{id}/briefings/{briefing_id}`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VisitBriefingResponse {
    /// Saved visit briefing.
    pub briefing: VisitBriefingWire,
}

/// Compact summary of one saved visit briefing in a patient workspace's briefing list.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BriefingSummaryItemWire {
    /// Durable briefing identifier.
    pub briefing_id: String,
    /// Calendar appointment identifier.
    pub appointment_id: String,
    /// Visit label from the practice schedule when known.
    pub visit_label: String,
    /// Scheduled visit start timestamp (RFC-3339).
    pub starts_at: String,
    /// Scheduled visit end timestamp (RFC-3339).
    pub ends_at: String,
    /// Timestamp when this briefing was saved (RFC-3339).
    pub prepared_at: String,
    /// Concise orientation preview.
    pub visit_card_orientation: String,
    /// Current vs moved vs cancelled standing of the scheduled slot.
    pub visit_standing: BriefingVisitStandingWire,
}

/// Response envelope for `GET /patient-workspaces/{id}/briefings`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceBriefingsResponse {
    /// Saved briefings for this patient workspace, newest scheduled slot first.
    #[serde(default)]
    pub briefings: Vec<BriefingSummaryItemWire>,
}

/// Per-physician panel collapse preferences stored on the server
/// (`GET /physician-state/panels`, `PUT /physician-state/panels`).
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PanelPreferencesWire {
    /// Whether the navigation sidebar is collapsed for this physician.
    #[serde(default)]
    pub sidebar_collapsed: bool,
    /// Whether the schedule rail is collapsed for this physician.
    #[serde(default)]
    pub schedule_collapsed: bool,
}
