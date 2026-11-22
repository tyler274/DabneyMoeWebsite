//! Typed Today and Updates wire contracts for per-physician seen state,
//! partial-load producer failure diagnostics, practice-wide source outages,
//! structured Updates groups, work-and-evidence inspection, and `Done` clearance.

use serde::{Deserialize, Serialize};

/// Per-physician seen standing for a Today row or Updates item.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SeenStandingWire {
    /// Never opened by the signed-in physician.
    #[default]
    Unseen,
    /// Opened at the current version by the signed-in physician.
    Seen {
        /// RFC 3339 timestamp when the physician last opened this item.
        seen_at: String,
    },
    /// Opened earlier, then revised with new clinical content or urgency.
    ChangedSinceSeen {
        /// RFC 3339 timestamp when the physician earlier opened this item.
        seen_at: String,
        /// Human-readable summary of what changed since the physician last opened it.
        what_changed: String,
    },
}

impl SeenStandingWire {
    /// Whether this item has not yet been seen in its current state (`Unseen` or `ChangedSinceSeen`).
    #[must_use]
    pub const fn needs_physician_eye(&self) -> bool {
        matches!(self, Self::Unseen | Self::ChangedSinceSeen { .. })
    }

    /// Returns the `what_changed` summary when the item changed after being seen.
    #[must_use]
    pub fn what_changed(&self) -> Option<&str> {
        match self {
            Self::ChangedSinceSeen { what_changed, .. } => Some(what_changed.as_str()),
            Self::Unseen | Self::Seen { .. } => None,
        }
    }
}

/// Closed identifier for a Today or Updates read-model producer slice.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProducerSliceWire {
    /// Raised or retired clinical concerns.
    Concerns,
    /// Scheduled and event-triggered patient chart reviews.
    PatientReviews,
    /// Upcoming visit preparation and visit briefings.
    VisitPreparation,
    /// Initial patient understanding and clinical summary upkeep.
    PatientUnderstanding,
    /// Currently running Coda chat turns.
    RunningConversations,
    /// Connected chart source health and pull standings.
    SourceStatuses,
    /// Spruce channel matching and reply proposals.
    SpruceMessages,
    /// Patient follow-ups.
    FollowUps,
}

/// One producer slice that failed while assembling Today or Updates.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FailedProducerWire {
    /// Typed producer slice identifier.
    pub producer: ProducerSliceWire,
    /// Physician-facing label naming the unavailable slice.
    pub label: String,
    /// Diagnostic detail describing why the producer failed.
    pub message: String,
}

/// One collapsed practice-wide source outage line on Today.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PracticeOutageWire {
    /// Stable connected source identifier.
    pub source_id: String,
    /// Physician-facing source name.
    pub source_label: String,
    /// Count of authorized panel patients affected by this outage.
    pub affected_workspace_count: usize,
    /// RFC 3339 timestamp when the outage was observed.
    pub since: String,
    /// Single-line failure explanation.
    pub failure_detail: String,
}

/// Closed work category for an item shown in Updates.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UpdateItemKindWire {
    /// Open clinical concern raised by Coda.
    ConcernRaised,
    /// Earlier clinical concern retired with a recorded explanation.
    ConcernRetired,
    /// Chart review completed with nothing needing physician action.
    QuietReview,
    /// Chart review completed with an assessment for the physician.
    AssessedReview,
    /// Visit briefing prepared ahead of an upcoming appointment.
    VisitBriefing,
    /// Initial or refreshed patient summary saved to the chart.
    PatientSummary,
    /// Scheduled review skipped cleanly with a recorded clinical reason.
    EndedNothingToDo,
    /// Patient follow-up created, rescheduled, paused, resumed, completed, or cancelled.
    FollowUpChanged,
}

/// One typed entry in Updates (`today_items`, `reviewed_nothing_for_you`, or `earlier_days`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpdateItemWire {
    /// Seen-state item key (`concern:...`, `review:...`, `briefing:...`, `summary:...`).
    pub item_id: String,
    /// Current version token of this item.
    pub version_id: String,
    /// Authorized patient workspace identifier.
    pub patient_workspace_id: String,
    /// Patient display name.
    pub patient_display_name: String,
    /// Closed work category of this update.
    pub kind: UpdateItemKindWire,
    /// Concise physician-facing headline.
    pub headline: String,
    /// Supporting clinical detail or one-line retirement/skip reason.
    pub detail: String,
    /// RFC 3339 timestamp when this update occurred.
    pub occurred_at: String,
    /// Per-physician seen standing.
    #[serde(default)]
    pub standing: SeenStandingWire,
}

/// One earlier calendar day group in Updates.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EarlierDayUpdatesWire {
    /// ISO calendar date (`YYYY-MM-DD`).
    pub date: String,
    /// Physician-facing day heading (`Yesterday` or `YYYY-MM-DD`).
    pub label: String,
    /// Ordered update items from this day, newest first.
    #[serde(default)]
    pub items: Vec<UpdateItemWire>,
}

/// One verified chart citation backing an opened Today row or Update item.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ItemCitationWire {
    /// Source candidate record identifier.
    pub candidate_id: String,
    /// Pinned source record revision.
    pub revision: String,
    /// Source system identifier.
    pub source_id: String,
    /// Physician-facing source system label.
    pub source_label: String,
    /// Record label (`Hemoglobin A1c`, `Laboratory result`, etc.).
    pub label: String,
    /// Effective date of the cited record, when supplied.
    #[serde(default)]
    pub as_of: Option<String>,
    /// Bounded preview excerpt from the saved chart record.
    pub excerpt: String,
    /// Full formatted readable chart record opened when inspecting the citation.
    #[serde(default)]
    pub body: String,
    /// Optional deep link into the external EHR.
    #[serde(default)]
    pub native_uri: Option<String>,
}

/// Work and cited chart evidence returned when a physician opens a Today row or Update item.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ItemWorkEvidenceWire {
    /// Seen-state item key.
    pub item_id: String,
    /// Current version token recorded in the physician's seen state.
    pub version_id: String,
    /// Authorized patient workspace identifier.
    pub patient_workspace_id: String,
    /// Patient display name.
    pub patient_display_name: String,
    /// Physician-facing work category (`Urgent concern`, `Visit briefing`, `Quiet chart review`, etc.).
    pub kind_label: String,
    /// Item headline.
    pub headline: String,
    /// Full clinical narrative or explanation of what Coda found or did.
    pub narrative: String,
    /// Basis and limitations for finished Coda work, when applicable.
    #[serde(default)]
    pub basis_and_limitations: Option<String>,
    /// Verified chart citations backing this work.
    #[serde(default)]
    pub citations: Vec<ItemCitationWire>,
    /// Linked conversation identifier when opening an active chat response.
    #[serde(default)]
    pub linked_chat_id: Option<String>,
    /// Updated seen standing (`Seen`) recorded for this physician.
    pub standing: SeenStandingWire,
}

/// Request body for `POST /today-updates/open`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OpenTodayUpdateItemBody {
    /// Authorized patient workspace identifier.
    pub patient_workspace_id: String,
    /// Item key to open and mark seen for the signed-in physician.
    pub item_id: String,
    /// Client idempotency key.
    pub idempotency_key: String,
}

/// Request body for `POST /today-updates/done`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MarkTodayDoneBody {
    /// Authorized patient workspace identifier.
    pub patient_workspace_id: String,
    /// Today row item key to clear with `Done`.
    pub item_id: String,
    /// Version token shown to the physician when `Done` was clicked.
    pub version_id: String,
    /// Client idempotency key.
    pub idempotency_key: String,
}

/// Outcome tag returned by `POST /today-updates/done`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TodayDoneOutcomeWire {
    /// Item was cleared (or was already cleared idempotently).
    Accepted,
    /// Item could not be cleared (e.g., stale version or unsupported item kind).
    Rejected,
}

/// Response body returned by `POST /today-updates/done`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TodayDoneResponseWire {
    /// Server outcome (`accepted` or `rejected`).
    pub outcome: TodayDoneOutcomeWire,
    /// Item key targeted by the command.
    pub item_id: String,
    /// Optional refusal explanation when `outcome == Rejected`.
    #[serde(default)]
    pub message: Option<String>,
}
