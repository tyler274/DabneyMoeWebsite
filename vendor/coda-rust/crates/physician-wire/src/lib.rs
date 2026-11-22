//! Physician reads the Dioxus site uses.
//!
//! The physician server publishes TypeScript types, not OpenAPI, so these structs
//! follow `src/server/apiTypes.ts` and `TodayListProjection` for the fields the
//! screens show. Extra JSON fields are ignored, which keeps a newer server readable.
//! Book: `book/src/physician-site.md`.
#![deny(missing_docs)]

pub mod briefings;
pub use briefings::PanelPreferencesWire;
pub mod follow_ups;
pub mod matching_reply;
pub mod source_setup;
pub mod today_updates;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub mod reviews;
pub use reviews::*;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
/// The `HealthResponse` value exchanged by this crate.
pub struct HealthResponse {
    /// The `status` value.
    pub status: String,
    #[serde(default)]
    /// The `retrying` value.
    pub retrying: Vec<Value>,
    #[serde(default)]
    /// The `held_by_newer_release` value.
    pub held_by_newer_release: Vec<Value>,
    #[serde(default)]
    /// Whether the physician can manage connection accounts.
    pub capabilities: Option<HealthCapabilities>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
/// The capability flags published with the health check.
pub struct HealthCapabilities {
    #[serde(default)]
    /// True when the server can open the Connections accounts dialog.
    pub connection_accounts: bool,
    #[serde(default)]
    /// True when the server exposes practice-facing source setup and recovery (`/connections`).
    pub practice_sources: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
/// The `TodayListResponse` value exchanged by this crate.
pub struct TodayListResponse {
    /// The `date` value.
    pub date: String,
    #[serde(default)]
    /// Saved IANA time zone of the signed-in practice, when provided.
    pub time_zone: Option<String>,
    /// The `rows` value.
    pub rows: Vec<TodayRow>,
    /// The `as_of` value.
    pub as_of: i64,
    #[serde(default)]
    /// The `connections` value.
    pub connections: Vec<TodayConnection>,
    #[serde(default)]
    /// The `incomplete` value.
    pub incomplete: Option<bool>,
    #[serde(default)]
    /// Item identifiers already opened once by the signed-in physician.
    pub seen: Vec<String>,
    #[serde(default)]
    /// Count of additional patients quietly reviewed today with nothing needing the physician.
    pub also_reviewed: usize,
    #[serde(default)]
    /// Collapsed practice-wide source outage lines when an entire source is down across the practice.
    pub practice_outages: Vec<today_updates::PracticeOutageWire>,
    #[serde(default)]
    /// Individual producer slices that failed during a partial Today load.
    pub failed_parts: Vec<today_updates::FailedProducerWire>,
    #[serde(default)]
    /// Count of patient messages currently waiting in `Messages to match`.
    pub messages_to_match_count: usize,
    #[serde(default)]
    /// True when one or more patients could not have panel access verified and were skipped.
    pub may_be_missing_someone: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// A decided item in a patient's active care plan.
pub struct WorkspacePlanItem {
    /// Plan item identifier.
    pub item_id: String,
    /// Patient workspace identifier.
    pub patient_workspace_id: String,
    /// Short plan item title.
    pub title: String,
    /// Clinical detail for the plan item.
    pub detail: String,
    /// Timestamp when the item was decided.
    pub decided_at: String,
    #[serde(default)]
    /// Recommendation identifier if promoted from a recommendation.
    pub source_recommendation_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// A medication recommendation awaiting or reflecting a physician decision.
pub struct WorkspaceMedicationRecommendation {
    /// Recommendation identifier.
    pub recommendation_id: String,
    /// Patient workspace identifier.
    pub patient_workspace_id: String,
    /// Medication name and dose.
    pub medication: String,
    /// Clinical rationale or context.
    pub rationale: String,
    /// Source note or review label.
    pub source_label: String,
    /// Current decision state (`undecided`, `decided`, `declined`).
    pub decision_state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// A drafted patient reply on a workspace projection.
pub struct WorkspaceReply {
    /// Reply work identifier.
    pub reply_work_id: String,
    /// Patient workspace identifier.
    pub patient_workspace_id: String,
    /// Patient display name.
    pub patient_display_name: String,
    /// Outbound channel (`spruce` or `email`).
    pub channel: String,
    /// Summary headline.
    pub headline: String,
    /// Drafted message text.
    pub draft_text: String,
    /// Urgency tier (`urgent_ed` or `routine`).
    pub urgency: String,
    /// Timestamp when the reply was drafted.
    pub created_at: String,
    /// Lifecycle state (`ready`, `superseded_by_correction`, `closed_by_physician`).
    pub state: String,
    #[serde(default)]
    /// Input key that superseded this reply, if any.
    pub superseded_by_input: Option<String>,
    #[serde(default)]
    /// Reason this reply was superseded, if any.
    pub superseded_reason: Option<String>,
    #[serde(default)]
    /// The saved reply draft's exact revision, when a hosted owner holds it.
    /// A revision request names this revision; absent on recorded replies.
    pub draft_revision: Option<u64>,
    #[serde(default)]
    /// The latest revision run on the saved draft, when one was accepted.
    pub revision_run: Option<ReplyRevisionRun>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// The latest accepted revision of a saved reply draft and how it stands.
pub struct ReplyRevisionRun {
    /// Stable execution identity.
    pub run_id: String,
    /// `pending`, `active`, `completed`, `held`, `failed` or `cancelled`.
    pub status: String,
    /// Present only while the saved draft permits this run to continue.
    pub resume_target: Option<WorkResumeTarget>,
}

impl ReplyRevisionRun {
    /// Whether the run is still waiting or working.
    pub fn running(&self) -> bool {
        matches!(self.status.as_str(), "pending" | "active")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// The reply-draft route's answer: saved text at an exact revision.
pub struct ReplyDraftWire {
    /// Saved revision.
    pub revision: u64,
    /// Saved text.
    pub text: String,
    #[serde(default)]
    /// The text the latest save or revision replaced.
    pub restore_text: Option<String>,
    #[serde(default)]
    /// The latest accepted revision run, if any.
    pub revision_run: Option<ReplyRevisionRun>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// The `TodayConnection` value exchanged by this crate.
pub struct TodayConnection {
    /// The `condition` value.
    pub condition: String,
    #[serde(default)]
    /// The `channel` value.
    pub channel: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_supported_source")]
    /// Typed source identifier when this Today notice comes from practice source setup.
    pub source_id: Option<source_setup::SupportedSourceWire>,
    #[serde(default)]
    /// Physician-facing source name when reported.
    pub source_label: Option<String>,
    #[serde(default)]
    /// Specific failure or access-loss reason reported by the source.
    pub reason: Option<String>,
    #[serde(default)]
    /// RFC-3339 timestamp of the last successful read before the failure or loss of access.
    pub last_successful_read_at: Option<String>,
    #[serde(default)]
    /// Closed recovery classification (`read_failed` vs `reconnect_required`), when structured.
    pub recovery: Option<source_setup::TodayConnectionRecoveryWire>,
}

fn deserialize_optional_supported_source<'de, D>(
    deserializer: D,
) -> Result<Option<source_setup::SupportedSourceWire>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = Option::<String>::deserialize(deserializer)?;
    Ok(raw
        .as_deref()
        .and_then(source_setup::SupportedSourceWire::from_slug))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
/// The `TodayRow` value exchanged by this crate.
pub struct TodayRow {
    /// The `patient_workspace_id` value.
    pub patient_workspace_id: String,
    /// The `patient_display_name` value.
    pub patient_display_name: String,
    /// The `item_id` value.
    pub item_id: String,
    /// The `version_id` value.
    pub version_id: String,
    /// The `waiting_since` value.
    pub waiting_since: String,
    /// The `urgency` value.
    pub urgency: String,
    /// The `kind` value.
    pub kind: String,
    #[serde(default)]
    /// The `reason` value.
    pub reason: Option<String>,
    #[serde(default)]
    /// The `work_kind` value.
    pub work_kind: Option<String>,
    #[serde(default)]
    /// The `resume_target` value.
    pub resume_target: Option<WorkResumeTarget>,
    #[serde(default)]
    /// Whether the signed-in physician has already opened this row once.
    pub seen: bool,
    #[serde(default)]
    /// Concise row title when distinct from the supporting detail.
    pub title: Option<String>,
    #[serde(default)]
    /// Typed per-physician seen standing (`unseen`, `seen`, or `changed_since_seen`).
    pub standing: Option<today_updates::SeenStandingWire>,
    #[serde(default)]
    /// Whether this row can be cleared with `Done`.
    pub can_mark_done: bool,
    #[serde(default)]
    /// Linked conversation identifier when this row tracks an active physician chat response.
    pub linked_chat_id: Option<String>,
    #[serde(default)]
    /// Linked Spruce conversation identifier when this row opens a patient reply card (`Review reply`).
    pub spruce_conversation_id: Option<String>,
    #[serde(default)]
    /// Unmatched Spruce message identifier when this row opens `Messages to match`.
    pub matching_message_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// The `WorkResumeTarget` value exchanged by this crate.
pub struct WorkResumeTarget {
    /// The `work_item_id` value.
    pub work_item_id: WorkItemId,
    /// The `expected_run_id` value.
    pub expected_run_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// The work a running chat turn's Stop command names.
pub struct WorkStopTarget {
    /// The `work_item_id` value.
    pub work_item_id: WorkItemId,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
/// The `PatientWorkspacesResponse` value exchanged by this crate.
pub struct PatientWorkspacesResponse {
    /// The `patients` value.
    pub patients: Vec<PatientWorkspaceSummary>,
    #[serde(default)]
    /// The `incomplete` value.
    pub incomplete: Option<bool>,
    #[serde(default)]
    /// True when one or more patients could not have panel access verified and were skipped.
    pub may_be_missing_someone: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
/// The `PatientWorkspaceSummary` value exchanged by this crate.
pub struct PatientWorkspaceSummary {
    /// The `patient_workspace_id` value.
    pub patient_workspace_id: String,
    /// The `display_name` value.
    pub display_name: String,
    /// The `cursor` value.
    pub cursor: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
/// The `WorkspaceChat` value exchanged by this crate.
pub struct WorkspaceChat {
    /// The `chat_id` value.
    pub chat_id: String,
    #[serde(default)]
    /// The `title` value.
    pub title: Option<String>,
    #[serde(default)]
    /// Content-distinguishing answer or status preview.
    pub summary: Option<String>,
    #[serde(default)]
    /// Current outcome of this conversation.
    pub outcome: Option<HistoryWorkOutcome>,
    /// The `created_at` value.
    pub created_at: String,
    #[serde(default, skip_serializing_if = "is_false")]
    /// Whether this conversation is archived.
    pub archived: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
/// The `PracticeDayResponse` value exchanged by this crate.
pub struct PracticeDayResponse {
    /// The `current_time` value.
    pub current_time: String,
    /// The `time_zone` value.
    pub time_zone: String,
    /// The `source_state` value.
    pub source_state: PracticeSourceState,
    #[serde(default)]
    /// The `visits` value.
    pub visits: Vec<PracticeVisit>,
    #[serde(default)]
    /// True when one or more patients could not have panel access verified and were skipped.
    pub may_be_missing_someone: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// The `PracticeSourceState` value exchanged by this crate.
pub struct PracticeSourceState {
    /// The `state` value.
    pub state: String,
    #[serde(default)]
    /// Physician-facing calendar source label when reported.
    pub source_label: Option<String>,
    #[serde(default)]
    /// Timestamp of the retained schedule snapshot (RFC-3339).
    pub snapshot_as_of: Option<String>,
    #[serde(default)]
    /// Timestamp when the schedule snapshot was synchronized (RFC-3339).
    pub synchronized_at: Option<String>,
    #[serde(default)]
    /// Schedule snapshot freshness (`current`, `partial`, or `refresh_failed`).
    pub freshness: Option<String>,
    #[serde(default)]
    /// Latest schedule refresh error when a refresh failed.
    pub last_error: Option<briefings::PracticeSourceErrorWire>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// The `PracticeVisit` value exchanged by this crate.
pub struct PracticeVisit {
    #[serde(default)]
    /// Calendar encounter identity when supplied.
    pub encounter: Option<briefings::ScheduleEncounterWire>,
    #[serde(default)]
    /// The `patient_display_name` value.
    pub patient_display_name: Option<String>,
    #[serde(default)]
    /// The `patient_workspace_id` value.
    pub patient_workspace_id: Option<String>,
    /// The `label` value.
    pub label: String,
    /// The `starts_at` value.
    pub starts_at: String,
    #[serde(default)]
    /// The `ends_at` value.
    pub ends_at: Option<String>,
    /// The `status` value.
    pub status: String,
    #[serde(default)]
    /// The `patient_time_zone` value.
    pub patient_time_zone: Option<String>,
    #[serde(default)]
    /// The `preparation` value.
    pub preparation: Option<VisitPreparation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// The `VisitPreparation` value exchanged by this crate.
pub struct VisitPreparation {
    /// The `state` value.
    pub state: String,
    #[serde(default)]
    /// Saved briefing identifier when `state == "prepared"`.
    pub briefing_id: Option<String>,
    #[serde(default)]
    /// Concise visit-card orientation when `state == "prepared"`.
    pub visit_card_orientation: Option<String>,
    #[serde(default)]
    /// Timestamp when the briefing was saved (RFC-3339).
    pub prepared_at: Option<String>,
    #[serde(default)]
    /// Failure or skip explanation when reported.
    pub reason: Option<String>,
    #[serde(default)]
    /// Historical briefing identifier preserved for a cancelled or ended visit slot.
    pub historical_briefing_id: Option<String>,
    #[serde(default)]
    /// The `last_attempt` value.
    pub last_attempt: Option<PreparationAttempt>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// The `PreparationAttempt` value exchanged by this crate.
pub struct PreparationAttempt {
    /// The `outcome` value.
    pub outcome: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
/// How one saved conversation or workspace work item ended or currently stands.
pub enum HistoryWorkOutcome {
    /// A finished response was published.
    Answered,
    /// Work is accepted or actively running.
    Running,
    /// Work is held until the physician resumes it.
    Held,
    /// Recovery gave up; Try again can continue it.
    Failed,
    /// A saved patient-reply draft is ready for review.
    DraftReady,
    /// A patient-reply revision failed; Try again can continue it.
    RevisionFailed,
}

impl HistoryWorkOutcome {
    /// Short physician-facing outcome label.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Answered => "Answered",
            Self::Running => "In progress",
            Self::Held => "Paused",
            Self::Failed => "Couldn't finish",
            Self::DraftReady => "Reply ready",
            Self::RevisionFailed => "Revision couldn't finish",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
/// Which saved work item a Past work entry reopens.
pub enum PastWorkTarget {
    /// A physician conversation with Coda.
    Conversation {
        /// Durable chat identifier.
        chat_id: String,
    },
    /// A patient-message reply draft and its archived revisions.
    ReplyDraft {
        /// Durable reply work identifier.
        reply_id: String,
        #[serde(default)]
        /// Number of archived earlier revisions retained for this draft.
        archived_revisions: usize,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// One saved entry in a patient workspace's Past work list.
pub struct PastWorkItem {
    /// Stable entry identifier.
    pub id: String,
    /// Reopen target.
    pub target: PastWorkTarget,
    /// Content-distinguishing title (question, instruction, or review subject).
    pub title: String,
    /// Content-distinguishing summary of the answer, draft, or finding.
    pub summary: String,
    /// Current outcome of the saved work.
    pub outcome: HistoryWorkOutcome,
    /// Timestamp when the item last changed.
    pub updated_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
/// Saved conversations and work items for one patient workspace.
pub struct PastWorkResponse {
    #[serde(default)]
    /// Ordered history items, latest activity first.
    pub items: Vec<PastWorkItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
/// The `RecentConversationsResponse` value exchanged by this crate.
pub struct RecentConversationsResponse {
    #[serde(default)]
    /// The `conversations` value.
    pub conversations: Vec<RecentConversation>,
    #[serde(default)]
    /// True when one or more patients could not have panel access verified and were skipped.
    pub may_be_missing_someone: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// The `RecentConversation` value exchanged by this crate.
pub struct RecentConversation {
    #[serde(default)]
    /// Durable conversation identifier used when reopening the conversation.
    pub chat_id: Option<String>,
    #[serde(default)]
    /// The `patient_workspace_id` value.
    pub patient_workspace_id: Option<String>,
    #[serde(default)]
    /// The `title` value.
    pub title: Option<String>,
    #[serde(default)]
    /// Content-distinguishing answer preview or status line.
    pub summary: Option<String>,
    #[serde(default)]
    /// Current outcome of the conversation.
    pub outcome: Option<HistoryWorkOutcome>,
    #[serde(default)]
    /// The `patient_display_name` value.
    pub patient_display_name: Option<String>,
    #[serde(default)]
    /// Timestamp of latest activity on the conversation.
    pub updated_at: Option<String>,
    #[serde(default)]
    /// Whether this conversation is archived.
    pub archived: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// One running review or background work item surfaced in Updates and the orb preview.
pub struct ActiveReviewRun {
    /// Stable run identifier.
    pub run_id: String,
    /// Patient workspace identifier.
    pub patient_workspace_id: String,
    /// Patient display name.
    pub patient_display_name: String,
    /// Physician-facing activity line, such as `"Coda is reviewing"`.
    pub headline: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
/// The `UpdatesResponse` value exchanged by this crate.
pub struct UpdatesResponse {
    #[serde(default)]
    /// Running reviews currently in progress across the physician's panel.
    pub active_runs: Vec<ActiveReviewRun>,
    #[serde(default)]
    /// The `rows` value.
    pub rows: Vec<Value>,
    #[serde(default)]
    /// Typed Updates entries recorded today (excluding quiet reviews).
    pub today_items: Vec<today_updates::UpdateItemWire>,
    #[serde(default)]
    /// Quiet completed reviews today grouped under `"Reviewed, nothing for you"`.
    pub reviewed_nothing_for_you: Vec<today_updates::UpdateItemWire>,
    #[serde(default)]
    /// Completed updates from earlier calendar days (`Yesterday`, etc.).
    pub earlier_days: Vec<today_updates::EarlierDayUpdatesWire>,
    #[serde(default)]
    /// Individual producer slices that failed during a partial Updates load.
    pub failed_parts: Vec<today_updates::FailedProducerWire>,
    #[serde(default)]
    /// True when one or more patients could not have panel access verified and were skipped.
    pub may_be_missing_someone: bool,
}

/// How a `Find patient` query matched rows, so the UI surfaces an explicit
/// notice when exact or prefix matching fell back to keyword matching.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SearchMatchMode {
    /// Matched by exact identifier, name prefix, or substring.
    #[default]
    ExactOrPrefix,
    /// No exact/prefix match succeeded, so results came from keyword fallback.
    KeywordFallback,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// The `PatientSearchResponse` value exchanged by this crate.
pub struct PatientSearchResponse {
    #[serde(default)]
    /// The `patients` value.
    pub patients: Vec<PatientSearchRow>,
    #[serde(default)]
    /// Cursor token for the next name-ordered page when more patients remain.
    pub next_after: Option<String>,
    #[serde(default)]
    /// True when one or more patients could not have panel access verified and were skipped.
    pub may_be_missing_someone: bool,
    #[serde(default)]
    /// How rows were matched for the submitted query.
    pub match_mode: SearchMatchMode,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
/// The practice-patient connection standing between Duet and this practice.
pub enum ConnectionStatusWire {
    /// No active connection or open invitation exists yet.
    #[default]
    NotInvited,
    /// An invitation has been sent and awaits redemption by the patient in Duet.
    Invited,
    /// The patient's Duet account is connected with this practice.
    Connected,
    /// A prior connection ended and can be re-invited by the practice.
    Ended,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// The `PatientSearchRow` value exchanged by this crate.
pub struct PatientSearchRow {
    /// The `patient_workspace_id` value.
    pub patient_workspace_id: String,
    /// The `patient_display_name` value.
    pub patient_display_name: String,
    #[serde(default)]
    /// The `birth_date` value.
    pub birth_date: Option<String>,
    #[serde(default)]
    /// Primary medical record number supplied by the practice roster, if any.
    pub mrn: Option<String>,
    #[serde(default)]
    /// Whether this roster patient has accepted the explicit practice invitation in Duet.
    pub duet_linked: bool,
    #[serde(default)]
    /// Whether this roster patient has active `"share with my physician"` consent.
    pub sharing_consented: bool,
    #[serde(default)]
    /// Canonical connection standing (`not_invited`, `invited`, `connected`, or `ended`).
    pub connection_status: ConnectionStatusWire,
    #[serde(default)]
    /// The per-patient review cadence exposed to Agents (`"weekly"` or `"daily_when_changed"`).
    pub cadence: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
/// The caller's membership level for a patient workspace.
pub enum WorkspaceMembership {
    /// The patient is on the physician's panel (`Membership::Allowed`).
    #[default]
    Allowed,
    /// The patient is in the same clinic, but not on the physician's panel.
    ClinicDemographic,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
/// The `PatientWorkspaceResponse` value exchanged by this crate.
pub struct PatientWorkspaceResponse {
    /// The `patient_workspace_id` value.
    pub patient_workspace_id: String,
    /// The `display_name` value.
    pub display_name: String,
    #[serde(default)]
    /// The caller's membership level (`allowed` or `clinic_demographic`).
    pub membership: WorkspaceMembership,
    #[serde(default)]
    /// The `time_zone` value.
    pub time_zone: Option<String>,
    #[serde(default)]
    /// The `chats` value.
    pub chats: Vec<WorkspaceChat>,
    #[serde(default)]
    /// The `identity` value.
    pub identity: Option<PatientIdentity>,
    #[serde(default)]
    /// The `patient_synopsis` value.
    pub patient_synopsis: Option<PatientSynopsis>,
    #[serde(default)]
    /// The `automation_run` value.
    pub automation_run: Option<AutomationRun>,
    #[serde(default)]
    /// The `active_follow_ups` value.
    pub active_follow_ups: Vec<FollowUp>,
    #[serde(default)]
    /// Whether this workspace has past work. The recording sets the flag and does not include history items.
    pub has_past_work: bool,
    #[serde(default)]
    /// When Coda last completed a review of the patient's whole care.
    pub last_reviewed_at: Option<String>,
    #[serde(default)]
    /// Connected record sources, including each last good pull.
    pub connected_sources: Vec<ConnectedSource>,
    #[serde(default)]
    /// Active ready patient replies on the workspace.
    pub replies: Vec<WorkspaceReply>,
    #[serde(default)]
    /// Replies superseded by corrected laboratory results.
    pub superseded_replies: Vec<WorkspaceReply>,
    #[serde(default)]
    /// Decided active care-plan items for the patient.
    pub active_plan_items: Vec<WorkspacePlanItem>,
    #[serde(default)]
    /// Undecided medication recommendations held out of the active plan.
    pub undecided_recommendations: Vec<WorkspaceMedicationRecommendation>,
    #[serde(default)]
    /// Canonical connection standing (`not_invited`, `invited`, `connected`, or `ended`).
    pub connection_status: ConnectionStatusWire,
    #[serde(default)]
    /// The patient front-door link and versioned consent state.
    pub patient_link: Option<PatientLinkWire>,
}

impl PatientWorkspaceResponse {
    /// Whether this response represents a same-clinic non-panel demographic card.
    pub fn is_clinic_demographic(&self) -> bool {
        self.membership == WorkspaceMembership::ClinicDemographic
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// The `PatientIdentity` value exchanged by this crate.
pub struct PatientIdentity {
    #[serde(default)]
    /// The `display_name` value.
    pub display_name: Option<String>,
    #[serde(default)]
    /// The `birth_date` value.
    pub birth_date: Option<String>,
    #[serde(default)]
    /// The `age_years` value.
    pub age_years: Option<i64>,
    #[serde(default)]
    /// The `sex_or_gender` value.
    pub sex_or_gender: Option<SexOrGender>,
    #[serde(default)]
    /// Medical record numbers the chart supplied.
    pub medical_record_numbers: Vec<MedicalRecordNumber>,
    #[serde(default)]
    /// Chart email address, when present in demographics (`COD-47`).
    pub email: Option<String>,
    #[serde(default)]
    /// The identity state (`available`, `missing`, etc.).
    pub state: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// One medical record number on the patient identity.
pub struct MedicalRecordNumber {
    #[serde(default)]
    /// The record number the chart supplied.
    pub value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// The `SexOrGender` value exchanged by this crate.
pub struct SexOrGender {
    #[serde(default)]
    /// The `value` value.
    pub value: Option<String>,
    #[serde(default)]
    /// The `label` value.
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// The `PatientSynopsis` value exchanged by this crate.
pub struct PatientSynopsis {
    /// The `version_id` value.
    pub version_id: String,
    #[serde(default)]
    /// The `authored_text` value.
    pub authored_text: Option<String>,
    #[serde(default)]
    /// Typed text and citation segments. The page renders these, not marker strings.
    pub segments: Vec<ClinicalSegment>,
    #[serde(default)]
    /// When this summary version was written.
    pub authored_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// One connected record source on the workspace.
pub struct ConnectedSource {
    #[serde(default)]
    /// Stable source identifier when supplied.
    pub source_id: Option<String>,
    #[serde(default)]
    /// Physician-facing source name when supplied.
    pub source_label: Option<String>,
    #[serde(default)]
    /// The source's current pull state.
    pub state: SourceConnectionState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
/// The pull state of one connected source.
pub struct SourceConnectionState {
    #[serde(default)]
    /// `usable`, `refreshing`, `failed`, or another server state.
    pub state: String,
    #[serde(default)]
    /// When this state last pulled records.
    pub synchronized_at: Option<String>,
    #[serde(default)]
    /// The last good pull, kept when a later pull failed.
    pub usable_snapshot: Option<SourceSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// A retained good pull.
pub struct SourceSnapshot {
    #[serde(default)]
    /// When that pull finished.
    pub synchronized_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
/// One piece of authored clinical text.
// The Citation variant carries six option fields against Text's one String,
// which trips `large_endian_variants`; boxing the wire struct would fight the
// generated serde shape, so the lint stays off for this enum.
#[allow(clippy::large_enum_variant)]
pub enum ClinicalSegment {
    /// Prose between citations.
    Text {
        /// The prose.
        text: String,
    },
    /// A source citation. Extra server fields stay unread.
    Citation {
        #[serde(default)]
        /// `resolved` or `unavailable`.
        state: String,
        #[serde(default)]
        /// The source title the chip names.
        display_label: Option<String>,
        #[serde(default)]
        /// Which catalog the citation belongs to.
        source: Option<String>,
        #[serde(default)]
        /// The source identity.
        source_id: Option<String>,
        #[serde(default)]
        /// The record handle inside that source.
        handle: Option<String>,
        #[serde(default)]
        /// The lead's short preview of this source.
        source_presentation: Option<SourcePresentation>,
        #[serde(default)]
        /// Where the original record can be opened.
        destination: Option<CitationDestination>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
/// The preview stored on a citation.
pub struct SourcePresentation {
    #[serde(default)]
    /// The preview body and its as-of date.
    pub content: Option<SourcePreviewContent>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
/// The text a source chip opens.
pub struct SourcePreviewContent {
    #[serde(default)]
    /// The preview prose.
    pub text: String,
    #[serde(default)]
    /// The as-of date, already written for display.
    pub as_of: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
/// A link from a citation back to its record.
pub struct CitationDestination {
    #[serde(default)]
    /// `source_native` opens the connected chart.
    pub kind: String,
    #[serde(default)]
    /// The chart name, such as Demo chart.
    pub source_label: Option<String>,
    #[serde(default)]
    /// The destination URI as the server recorded it.
    pub uri: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// The `AutomationRun` value exchanged by this crate.
pub struct AutomationRun {
    /// The `kind` value.
    pub kind: String,
    /// The `state` value.
    pub state: String,
    #[serde(default)]
    /// The `ended_cause` value.
    pub ended_cause: Option<String>,
    #[serde(default)]
    /// The `purpose` value.
    pub purpose: Option<WorkPurpose>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// The `WorkPurpose` value exchanged by this crate.
pub struct WorkPurpose {
    #[serde(default)]
    /// The `headline` value.
    pub headline: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// The `FollowUp` value exchanged by this crate.
pub struct FollowUp {
    /// The `follow_up_id` value.
    pub follow_up_id: String,
    #[serde(default)]
    /// The `description` value.
    pub description: Option<String>,
}

/// A conversation as the page reads it: one exchange per piece of accepted
/// work, in the order the physician started them.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChatSnapshotResponse {
    /// The `snapshot` value.
    pub snapshot: ChatSnapshot,
}

/// Where a cited source's displayed text came from.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CitationOrigin {
    /// Copied or decoded from the source record itself.
    #[default]
    SourceRecord,
    /// Coda's own short description because the source had nothing of its own to show.
    PreparedByCoda,
}

/// Where a citation points in its source record.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CitationTargetKind {
    /// Names the record as a whole rather than a specific passage.
    #[default]
    WholeRecord,
    /// Names one or more cited passages inside the record.
    Passage,
    /// Named a passage that could not be matched inside this record's text.
    PassageNotFound,
}

/// One labeled or unlabeled readable section of a cited source record.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CitedTextPartWire {
    #[serde(default)]
    /// Section heading if different from the record title.
    pub label: Option<String>,
    /// Human-readable prose for this section.
    pub text: String,
}

/// One structured clinical field inside a cited record.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CitedFieldWire {
    /// Stable key identifying this field inside the record.
    pub key: String,
    /// Physician-facing field name.
    pub name: String,
    /// Recorded value.
    pub value: String,
    #[serde(default)]
    /// Measurement unit, when supplied.
    pub unit: Option<String>,
    #[serde(default)]
    /// Reference range, when supplied.
    pub range: Option<String>,
    #[serde(default)]
    /// Field status or abnormality flag, when supplied.
    pub status: Option<String>,
}

/// Authoritative response for reading a single cited source record on demand.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CitedSourceResponse {
    /// The requested cited source record.
    pub source: CitedSourceWire,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// One cited record backing claims in a conversation snapshot.
pub struct CitedSourceWire {
    /// Lookup key referenced by [`MessageSegment::Citation`].
    pub source_key: String,
    /// 1-based citation number within its response.
    pub number: usize,
    /// Physician-facing source name.
    pub source_label: String,
    /// Record title or kind label.
    pub title: String,
    #[serde(default)]
    /// Source's own record type.
    pub record_type: Option<String>,
    #[serde(default)]
    /// Date recorded on the source record, when supplied.
    pub recorded_at: Option<String>,
    #[serde(default)]
    /// Source's own verification or clinical status (e.g. `"preliminary"`, `"final"`).
    pub record_status: Option<String>,
    /// Quoted excerpt supporting the citation preview.
    pub quote: String,
    /// Readable record body opened in the side panel.
    pub body: String,
    #[serde(default)]
    /// Structured readable sections of the source record, in reading order.
    pub readable_parts: Vec<CitedTextPartWire>,
    #[serde(default)]
    /// Structured fields decoded from the record, when available.
    pub fields: Vec<CitedFieldWire>,
    #[serde(default)]
    /// Exact source-native record representation for Technical details.
    pub raw_content: Option<String>,
    #[serde(default)]
    /// Direct link into the connected chart when available.
    pub external_url: Option<String>,
    #[serde(default)]
    /// Whether the body is from the source record or Coda's fallback description.
    pub origin: CitationOrigin,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
/// One span of text or inline citation in a finished answer or its Basis.
pub enum MessageSegment {
    /// Plain prose span.
    Text {
        /// Prose text.
        text: String,
    },
    /// Numbered inline citation linked to [`ChatSnapshot::sources`].
    Citation {
        /// Key into [`ChatSnapshot::sources`].
        source_key: String,
        /// 1-based citation number.
        number: usize,
        /// Short preview label.
        label: String,
        /// Quoted supporting passage.
        quote: String,
        #[serde(default)]
        /// Whether this mark points at a specific passage, an unmatched passage, or the whole record.
        target: CitationTargetKind,
    },
}

/// Request sent to `POST /dictation` carrying recorded audio and/or metadata.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DictationRequest {
    #[serde(default)]
    /// Base64-encoded captured audio bytes (WebM/Opus, WAV/PCM, or sample audio).
    pub audio_base64: String,
    #[serde(default)]
    /// MIME type of the captured audio stream.
    pub mime_type: String,
    #[serde(default)]
    /// Sample rate in Hz when provided by the audio recorder.
    pub sample_rate_hz: Option<u32>,
}

/// Transcribed plain text returned by `POST /dictation`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DictationResponse {
    /// Recognized plain text ready for insertion into the focused field.
    pub transcript: String,
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// How a submitted message should be delivered relative to active work.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SubmitDelivery {
    /// Send immediately (starts a turn when idle/stopped/failed, or steers active work).
    #[default]
    SendNow,
    /// Queue while Coda is working; sent together once the answer finishes.
    SendNext,
}

/// One physician message queued on the server waiting for the current answer to finish.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QueuedMessageWire {
    /// The idempotency key the page sent the message with.
    pub id: MessageKey,
    /// The message as the physician wrote or edited it.
    pub text: String,
    /// When the server saved it.
    pub saved_at: SavedAt,
}

/// The conversation's transcript and whether its feedback could be read.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChatSnapshot {
    /// Each piece of accepted work with its messages, steps and answer.
    pub transcript: Vec<Exchange>,
    /// Whether the signed-in physician's feedback on the answers could be read.
    pub feedback: SnapshotFeedback,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    /// Verified cited sources referenced by [`AnswerState::Finished`] segments and Basis.
    pub sources: std::collections::BTreeMap<String, CitedSourceWire>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    /// Physician messages queued behind active/cut-off/failed work.
    pub queued: Vec<QueuedMessageWire>,
    #[serde(default, skip_serializing_if = "is_false")]
    /// Whether this conversation is currently archived.
    pub archived: bool,
}

/// One piece of accepted work: the messages that started or steered it, the
/// steps Coda took for it and how its answer stands.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Exchange {
    /// The work these messages belong to.
    pub work_item_id: WorkItemId,
    /// Every physician message that started or steered the work, in order.
    pub messages: Vec<PhysicianMessage>,
    /// What Coda did for the answer, or that its saved record could not be read.
    pub steps: ExchangeSteps,
    /// What Coda had written each time a limit cut this answer off, in the
    /// order it was saved. Try again continues after it, so it stays with
    /// the exchange whatever the answer's state.
    pub kept: Vec<String>,
    /// How the answer stands.
    pub answer: AnswerState,
}

/// The steps of one exchange, or that they could not be read. Like
/// [`SnapshotFeedback`], an unreadable part is marked, never shown as empty
/// or as a partial list, and the rest of the exchange still comes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", content = "steps", rename_all = "snake_case")]
pub enum ExchangeSteps {
    /// Each step, in the order Coda started it.
    Read(Vec<Step>),
    /// A saved call of this work could not be read as a step, so none are
    /// shown and none can be told apart from "took no steps".
    Unavailable,
}

/// One message the physician sent.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PhysicianMessage {
    /// The idempotency key the page sent the message with.
    pub id: MessageKey,
    /// The message as the physician wrote it.
    pub text: String,
    /// When the server saved it.
    pub saved_at: SavedAt,
}

/// When the server saved a message or finished an answer: its database
/// clock's reading, in whole seconds since the Unix epoch.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub struct SavedAt(pub i64);

/// How one exchange's answer stands. Only the conversation's current work
/// offers Resume or Try again; an answer whose work a later message replaced
/// keeps the ending it had, without either.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum AnswerState {
    /// Coda is working on the answer. No part of its text is shown yet.
    Working {
        /// The work Stop names.
        stop: WorkStopTarget,
        /// Coda has begun handing in the answer and is writing it: the model
        /// began the finish in its latest request, and no request has started
        /// or failed since. The finish's return does not end it.
        writing: bool,
    },
    /// Coda finished the answer.
    Finished {
        /// The answer's durable name, which feedback names.
        response_id: ResponseId,
        /// The attempt that published the answer, so an operator can match
        /// the reply to its telemetry by attempt id.
        attempt_id: AttemptId,
        /// When the server saved the finished answer.
        finished_at: SavedAt,
        /// The answer as Coda published it. Text a limit kept before it is
        /// in the exchange's `kept`.
        text: String,
        /// The physician's saved feedback on it, when they rated it and the
        /// snapshot could read it.
        feedback: Option<ResponseFeedback>,
        /// Structured prose and citation segments for the answer's body.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        segments: Vec<MessageSegment>,
        /// Collapsed Basis and limitations segments on a finished answer.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        basis: Option<Vec<MessageSegment>>,
        /// Whether newer source records exist in the workspace after this
        /// answer's observed record state.
        #[serde(default, skip_serializing_if = "is_false")]
        newer_records: bool,
    },
    /// The physician stopped the work.
    Stopped {
        /// The run Resume continues, while this is the current work.
        resume: Option<WorkResumeTarget>,
    },
    /// Coda reached a limit before it finished. What it kept is in the
    /// exchange's `kept`.
    CutOff {
        /// The run Try again continues, while this is the current work.
        try_again: Option<WorkResumeTarget>,
    },
    /// Coda's recovery was used up, or the model service refused the work.
    CouldntFinish {
        /// The run Try again continues, while this is the current work.
        try_again: Option<WorkResumeTarget>,
    },
}

/// One thing Coda did for an answer, as typed facts. The page words it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "did", rename_all = "snake_case")]
pub enum Step {
    /// Coda searched the patient's chart.
    SearchedChart {
        /// The search text, as Coda gave it; empty when it browsed.
        query: String,
        /// The group the search was limited to, if any.
        group: Option<RecordGroup>,
        /// Whether Coda began the search or went on to its next page.
        page: SearchPage,
        /// How the search stands.
        state: StepState<Found>,
    },
    /// Coda opened records from the patient's chart.
    ReadRecords {
        /// How many records it asked for.
        requested: usize,
        /// How the read stands, with what each record gave once it returned.
        state: StepState<Vec<RecordRead>>,
    },
    /// Coda looked at earlier messages of the conversation. It may have found nothing.
    ReadConversation {
        /// How the look stands.
        state: StepState<()>,
    },
    /// Coda saved a note to the patient's memory log.
    SavedMemoryNote {
        /// How the save stands.
        state: StepState<()>,
    },
    /// Coda edited a patient memory file.
    EditedMemory {
        /// How the edit stands.
        state: StepState<()>,
    },
    /// Coda created a patient follow-up item.
    CreatedFollowUp {
        /// How the follow-up creation stands.
        state: StepState<()>,
    },
}

/// Which page of its results a chart search got.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SearchPage {
    /// Its first page: Coda began the search.
    First,
    /// The page after one Coda got before, for the same search.
    More,
}

/// How one step stands.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum StepState<T> {
    /// It started and has not returned.
    Running,
    /// It returned this result.
    Returned {
        /// What it gave.
        result: T,
    },
    /// It returned no output: it was refused, failed or timed out.
    Failed,
    /// The physician stopped the work before it returned. How it ended is
    /// not known.
    Stopped,
    /// The work's attempt ended before it returned, for a reason other than
    /// Stop, such as a restart. How it ended is not known.
    Interrupted,
}

/// What a chart search found.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct Found {
    /// Records on this page of results.
    pub records: usize,
    /// More results follow this page.
    pub more: bool,
}

/// What one record of a chart read gave.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum RecordRead {
    /// The whole record.
    Whole {
        /// The version of the record that was read.
        version: SourceVersion,
        /// What the record says about itself.
        about: RecordAbout,
    },
    /// One passage of a large record.
    Passage {
        /// The version of the record the passage is part of.
        version: SourceVersion,
    },
    /// The record is not in the chart, or its revision changed.
    Unavailable,
    /// The record did not fit the read's size budget.
    TooLarge,
}

/// One version of a chart record, as a read names it: the record's id in the patient's chart and
/// the revision of it that was read. Every read of that version, whole or in part, names it alike.
/// It is compared, never shown.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct SourceVersion {
    /// The record's id in the patient's chart.
    pub candidate_id: String,
    /// The revision of the record that was read.
    pub revision: String,
}

/// The group Coda files a record under: the sixteen groups of the shared record envelope.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RecordGroup {
    /// Patient demographics and identifiers.
    Patient,
    /// Active and historical problems or diagnoses.
    Problems,
    /// Prescriptions, medication statements, dispenses, and administrations.
    Medications,
    /// Allergies and intolerances.
    Allergies,
    /// Laboratory, imaging, and diagnostic report results.
    Results,
    /// Vital signs.
    Vitals,
    /// Clinical notes.
    Notes,
    /// Non-note clinical documents and attachments.
    Documents,
    /// Encounters and appointments.
    Visits,
    /// Procedures.
    Procedures,
    /// Immunizations.
    Immunizations,
    /// Service requests and orders.
    Orders,
    /// Social and family history.
    History,
    /// Questionnaires and survey observations.
    Forms,
    /// Patient and care-team communications.
    Messages,
    /// Source records whose kind has no more specific group.
    Other,
}

/// What a record says about itself, from its envelope.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecordAbout {
    /// The group it is filed under. None for a record filed without an envelope, which a
    /// read's receipt still allows.
    pub group: Option<RecordGroup>,
    /// Its title, when the source gave one.
    pub title: Option<String>,
    /// Its dates in the source's words, in the order the adapter's table declares them.
    pub dates: Vec<RecordDate>,
}

/// One date on a record, as the source wrote it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecordDate {
    /// What the date marks, as the source's adapter declares it. None for a
    /// date the record carries without a declared role.
    pub role: Option<DateRole>,
    /// The value as the source gave it.
    pub value: String,
}

/// What a date on a record marks, as the source's adapter declares it.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DateRole {
    /// Patient date of birth.
    BirthDate,
    /// Condition or allergy onset.
    Onset,
    /// Condition abatement or resolution.
    Abatement,
    /// When the record was recorded or documented.
    Recorded,
    /// When an order, request, or response was authored.
    Authored,
    /// Clinical effective time of an observation, medication, or report.
    Effective,
    /// When a report or observation was issued.
    Issued,
    /// When a procedure was performed.
    Performed,
    /// When an immunization or dispense occurred.
    Occurred,
    /// Start of a clinical period or appointment.
    Start,
    /// End of a clinical period or appointment.
    End,
    /// When a communication was sent.
    Sent,
    /// When a communication was received.
    Received,
}

/// Whether a chat snapshot could read the physician's feedback. The
/// conversation comes either way; only the ratings are missing when it
/// couldn't.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotFeedback {
    /// Each rated response carries its saved feedback; one without is unrated.
    #[default]
    Loaded,
    /// The feedback read failed, so no response carries feedback and none
    /// can be told apart from unrated.
    Unavailable,
}

/// The durable name of one of Coda's published replies, which stays the same
/// across restarts and reloads.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub struct ResponseId(pub String);

impl ResponseId {
    /// The response id as the server sent it.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The id of one piece of accepted work, which Stop, Resume and Try again name.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub struct WorkItemId(pub String);

impl WorkItemId {
    /// The work item id as the server sent it.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The idempotency key the page sent one physician message with.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub struct MessageKey(pub String);

impl MessageKey {
    /// The message key as the server sent it.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The id of one attempt at a piece of work, which its telemetry carries.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub struct AttemptId(pub String);

impl AttemptId {
    /// The attempt id as the server sent it.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A physician's rating of one of Coda's responses.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FeedbackRating {
    /// A good response.
    Up,
    /// A poor response.
    Down,
}

/// A reason picked in a rating's details.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum FeedbackReason {
    /// "Incorrect information".
    IncorrectInformation,
    /// "Missing context".
    MissingContext,
    /// "Hard to follow".
    HardToFollow,
    /// "Not useful".
    NotUseful,
}

/// The optional details on a rating.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct FeedbackDetails {
    /// The reasons picked.
    pub reasons: Vec<FeedbackReason>,
    /// The physician's words.
    pub text: String,
}

/// A feedback save's body: the rating and the details together, replacing
/// both, so details never travel without a rating.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SaveFeedback {
    /// The rating chosen.
    pub rating: FeedbackRating,
    #[serde(default)]
    /// The details, or `None` to save the rating alone.
    pub details: Option<FeedbackDetails>,
}

/// The physician's saved feedback on one response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResponseFeedback {
    /// 1 on the first save, one more on each later save: the highest
    /// revision seen is the newest feedback.
    pub revision: u64,
    /// The saved rating.
    pub rating: FeedbackRating,
    #[serde(default)]
    /// The saved details, if any.
    pub details: Option<FeedbackDetails>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// The `PhysicianDraft` value exchanged by this crate.
pub struct PhysicianDraft {
    /// The `text` value.
    pub text: String,
    /// The `revision` value.
    pub revision: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// The sign-in route's answer: a token to verify, or the no-account deferral.
pub struct SignInResponse {
    /// The `status` value: `token_issued` when proof delivery returns a code,
    /// or `unavailable` for the indistinguishable "link is on its way" line.
    /// Hosted mail uses `unavailable` and omits the code.
    pub status: String,
    #[serde(default)]
    /// The sign-in token when the server issued one (proof delivery).
    pub token: Option<String>,
    #[serde(default)]
    /// The human line for `unavailable`, such as the no-account deferral
    /// and the hosted "link is on its way" response.
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// The verify route's answer: the bearer session and who holds it.
pub struct VerifyResponse {
    /// The bearer session to present on every physician read.
    pub session: String,
    /// The `physician_id` value.
    pub physician_id: String,
    /// The `practice_id` value.
    pub practice_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// The demo session route's answer: the live harness bearer session and metadata.
pub struct DemoSessionResponse {
    /// The bearer session to present on every live demo read and command.
    pub session: String,
    /// The `physician_id` value.
    pub physician_id: String,
    /// The `practice_id` value.
    pub practice_id: String,
    /// Optional unix timestamp when the live demo session expires.
    #[serde(default)]
    pub ends_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// One active physician account in the caller's practice, for panel assignment.
pub struct PracticeAccountSummary {
    /// The `physician_id` value.
    pub physician_id: String,
    /// The physician's display name.
    pub display_name: String,
    #[serde(default)]
    /// The physician's sign-in email.
    pub email: String,
    #[serde(default)]
    /// Whether this account holds clinic admin / panel-assignment permission.
    pub admin: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// The whoami route's answer about the session's physician.
pub struct WhoamiResponse {
    /// The `physician_id` value.
    pub physician_id: String,
    /// The `practice_id` value.
    pub practice_id: String,
    #[serde(default)]
    /// The account's email; the expired-session prompt mails its link here.
    pub email: String,
    /// The physician's display name.
    pub display_name: String,
    #[serde(default)]
    /// Whether this account holds clinic admin / panel-assignment permission.
    pub admin: bool,
    #[serde(default)]
    /// Active accounts in the caller's practice when `admin` is true.
    pub practice_accounts: Vec<PracticeAccountSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// The panel assignment route's answer after updating one panel membership.
pub struct PanelAssignmentResponse {
    /// The `practice_id` value.
    pub practice_id: String,
    /// The `physician_id` value.
    pub physician_id: String,
    /// The `patient_workspace_id` value.
    pub patient_workspace_id: String,
    /// Whether the patient workspace is now on the physician's panel.
    pub allowed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// The wire representation of a roster patient's front-door link and consent state.
pub struct PatientLinkWire {
    /// The practice that owns the roster patient.
    pub practice_id: String,
    /// The roster patient's workspace id.
    pub patient_workspace_id: String,
    #[serde(default)]
    /// The linked Duet account id, once the invitation is accepted.
    pub duet_account_id: Option<String>,
    #[serde(default)]
    /// Whether the patient accepted the practice invitation.
    pub linked: bool,
    #[serde(default)]
    /// Whether `"share with my physician"` consent is currently active.
    pub sharing_consented: bool,
    #[serde(default)]
    /// Canonical connection standing (`not_invited`, `invited`, `connected`, or `ended`).
    pub connection_status: ConnectionStatusWire,
    #[serde(default)]
    /// The accepted consent version (such as `"v1"`).
    pub consent_version: Option<String>,
    #[serde(default)]
    /// The Unix second consent was granted.
    pub consented_at: Option<u64>,
    #[serde(default)]
    /// The Unix second consent was revoked, if revoked.
    pub revoked_at: Option<u64>,
    #[serde(default)]
    /// Email address of the currently open invitation, when `connection_status == Invited`.
    pub invited_email: Option<String>,
    #[serde(default)]
    /// Wrong birth-date answers recorded against the open invitation.
    pub wrong_dob_attempts: u8,
    #[serde(default)]
    /// Unix second the open invitation locked after 5 wrong birth-date attempts (`COD-48`).
    pub locked_at: Option<u64>,
    #[serde(default)]
    /// Unix second outbound invitation mail last failed, if any (`Rule 8`).
    pub delivery_failed_at: Option<u64>,
    #[serde(default)]
    /// Diagnostic detail for the last invitation mail delivery failure.
    pub delivery_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// Request body for `POST /auth/invitations` (patient Duet invitation from patient's page).
pub struct InvitePatientRequest {
    /// Practice patient workspace to invite.
    pub patient_workspace_id: String,
    /// Invitee's email address.
    pub email: String,
    /// Invitee's display name.
    pub display_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// Request body for `POST /auth/invitations/resend` and `POST /auth/invitations/cancel`.
pub struct PatientInvitationActionRequest {
    /// Practice patient workspace targeted by the invitation action.
    pub patient_workspace_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// Response body for `POST /auth/invitations/cancel`.
pub struct CancelInvitationResponse {
    /// Practice owning the roster patient.
    pub practice_id: String,
    /// Roster patient workspace identifier.
    pub patient_workspace_id: String,
    /// Resulting standing (`"not_invited"`).
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// Response body for `POST /auth/invitations` and `POST /auth/invitations/resend`.
pub struct PatientInvitationResponse {
    /// Practice owning the roster patient.
    pub practice_id: String,
    /// Roster patient workspace identifier.
    pub patient_workspace_id: String,
    /// Invited email address.
    pub email: String,
    /// Invited display name.
    pub display_name: String,
    /// Status (`"invited"`).
    pub status: String,
    #[serde(default)]
    /// Proof-mode single-use invitation token, omitted in hosted mail delivery.
    pub token: Option<String>,
    #[serde(default)]
    /// Proof-mode human-enterable short code (`XXXX-XXXX`), omitted in hosted mail delivery.
    pub short_code: Option<String>,
    #[serde(default)]
    /// Whether outbound mail delivery failed (`Rule 8`; invitation remains `Invited`).
    pub delivery_failed: bool,
    #[serde(default)]
    /// Reason mail delivery failed, when `delivery_failed` is true.
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// Request body for `POST /auth/colleagues/invitations` (inviting a colleague physician).
pub struct InviteColleagueRequest {
    /// Colleague's work email address.
    pub email: String,
    /// Colleague's display name.
    pub display_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// Response body for `POST /auth/colleagues/invitations`.
pub struct ColleagueInvitationResponse {
    /// Practice the colleague was invited into.
    pub practice_id: String,
    /// Colleague's email address.
    pub email: String,
    /// Colleague's display name.
    pub display_name: String,
    /// Assigned role (`"physician"`).
    pub role: String,
    /// Status (`"invited"`).
    pub status: String,
    /// Delivery channel (`"stored"` or `"mailed"`).
    pub delivery: String,
    #[serde(default)]
    /// Proof-mode sign-in token, omitted in hosted mode.
    pub token: Option<String>,
    #[serde(default)]
    /// Proof-mode sign-in path.
    pub sign_in_path: Option<String>,
}

/// One physician-facing patient memory document (`summary.md` or `direction.md`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryDocumentWire {
    /// The current version number.
    pub version: u64,
    /// The UTC day this version was saved (`YYYY-MM-DD`).
    pub saved_at: String,
    /// The markdown body.
    pub markdown: String,
}

/// The physician-facing patient memory view (`GET /patient-workspaces/{id}/memory`).
/// Carries only the summary and the direction (`Memory.Code.20`); pages and
/// the log are Coda's working files and are never exposed on this surface.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PatientMemoryResponse {
    /// Patient workspace identifier.
    pub patient_workspace_id: String,
    /// Whether this server is backed by durable Postgres memory storage.
    #[serde(default)]
    pub durable: bool,
    /// Current `summary.md`, when present.
    pub summary: Option<MemoryDocumentWire>,
    /// Current `direction.md`, when present.
    pub direction: Option<MemoryDocumentWire>,
}

/// Which physician-facing memory document the physician is editing (`Memory.Code.22`).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryEditTarget {
    /// `summary.md`.
    Summary,
    /// `direction.md`.
    Direction,
}

/// Request body for `POST /patient-workspaces/{id}/memory`: saves the
/// physician's edit as a note in their words so patient rounds fold it in.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SavePhysicianMemoryEditRequest {
    /// Which document (`summary` or `direction`) the edit addresses.
    pub target: MemoryEditTarget,
    /// The physician's own words.
    pub text: String,
    /// Client idempotency key so retries save one note.
    pub idempotency_key: String,
}

/// Response body for `POST /patient-workspaces/{id}/memory`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SavePhysicianMemoryEditResponse {
    /// Assigned note number (for example `"N1"`).
    pub note_number: String,
    /// Log path holding the saved note (for example `"log/2026-11-19.md"`).
    pub log_path: String,
    /// RFC-3339 timestamp when the note was saved.
    pub saved_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_today_list() {
        let list: TodayListResponse = serde_json::from_str(
            r#"{
            "date": "2026-10-09",
            "rows": [{
                "patient_workspace_id": "p1",
                "patient_display_name": "Ada",
                "item_id": "i1",
                "version_id": "v1",
                "waiting_since": "2026-10-09T12:00:00Z",
                "urgency": "today",
                "kind": "concern",
                "reason": "New note"
            }],
            "connections": [],
            "cursors": {},
            "as_of": 3,
            "seen": []
        }"#,
        )
        .unwrap();
        assert_eq!(list.rows[0].patient_display_name, "Ada");
        assert_eq!(list.as_of, 3);
    }

    /// The page reads every answer state and step the site writes, and
    /// refuses the conversation shape older sites wrote.
    #[test]
    fn reads_a_transcript_and_refuses_the_older_shape() {
        let page: ChatSnapshotResponse = serde_json::from_str(
            r#"{"snapshot": {"feedback": "loaded", "transcript": [{
                "work_item_id": "w1",
                "messages": [{"id": "k1", "text": "Hello", "saved_at": 1794600000},
                             {"id": "k2", "text": "And sodium?", "saved_at": 1794600030}],
                "steps": {"state": "read", "steps": [
                    {"did": "searched_chart", "query": "sodium", "group": null, "page": "first",
                     "state": {"state": "returned", "result": {"records": 2, "more": false}}},
                    {"did": "read_records", "requested": 2, "state": {"state": "returned", "result": [
                        {"outcome": "whole",
                         "version": {"candidate_id": "ehr/Observation/sodium", "revision": "1"},
                         "about": {"group": "results", "title": "Sodium",
                         "dates": [{"role": "effective", "value": "2026-10-01"}]}},
                        {"outcome": "unavailable"}]}},
                    {"did": "read_conversation", "state": {"state": "stopped"}}
                ]},
                "kept": ["Sodium was 139"],
                "answer": {"state": "stopped",
                           "resume": {"work_item_id": "w1", "expected_run_id": "r1"}}
            }]}}"#,
        )
        .unwrap();
        let exchange = &page.snapshot.transcript[0];
        assert_eq!(exchange.messages[1].text, "And sodium?");
        assert_eq!(exchange.kept, ["Sodium was 139"]);
        assert_eq!(
            exchange.answer,
            AnswerState::Stopped {
                resume: Some(WorkResumeTarget {
                    work_item_id: WorkItemId("w1".into()),
                    expected_run_id: "r1".into(),
                }),
            }
        );
        let ExchangeSteps::Read(steps) = &exchange.steps else {
            panic!("the steps were read: {:?}", exchange.steps);
        };
        assert_eq!(
            steps[2],
            Step::ReadConversation {
                state: StepState::Stopped
            }
        );
        let written = serde_json::to_value(&page).unwrap();
        assert_eq!(
            serde_json::from_value::<ChatSnapshotResponse>(written).unwrap(),
            page
        );
        let mut unreadable = page.clone();
        unreadable.snapshot.transcript[0].steps = ExchangeSteps::Unavailable;
        let written = serde_json::to_value(&unreadable).unwrap();
        assert_eq!(
            written["snapshot"]["transcript"][0]["steps"],
            serde_json::json!({"state": "unavailable"})
        );
        assert_eq!(
            serde_json::from_value::<ChatSnapshotResponse>(written).unwrap(),
            unreadable
        );
        assert!(
            serde_json::from_str::<ExchangeSteps>(r#"[]"#).is_err(),
            "a bare step list is not read as readable steps"
        );
        assert!(
            serde_json::from_str::<ChatSnapshotResponse>(
                r#"{"snapshot": {"conversation": [], "runs": []}}"#
            )
            .is_err(),
            "the older conversation shape is not read"
        );
    }

    /// A search step says whether Coda began the search or went on to its
    /// next page. A search step that does not say is not read.
    #[test]
    fn a_search_step_says_which_page_it_got() {
        for (page, name) in [(SearchPage::First, "first"), (SearchPage::More, "more")] {
            let step = Step::SearchedChart {
                query: "a1c".into(),
                group: None,
                page,
                state: StepState::Returned {
                    result: Found {
                        records: 4,
                        more: false,
                    },
                },
            };
            let written = serde_json::to_value(&step).unwrap();
            assert_eq!(written["page"], name);
            assert_eq!(serde_json::from_value::<Step>(written).unwrap(), step);
        }
        assert!(
            serde_json::from_value::<Step>(serde_json::json!({
                "did": "searched_chart", "query": "a1c", "group": null,
                "state": {"state": "running"}
            }))
            .is_err(),
            "a search step without its page is read"
        );
    }

    /// A step names a record by its filing group and title. A step with a
    /// source's type name in their place is not read.
    #[test]
    fn a_step_names_a_record_by_its_group_and_title() {
        let steps = vec![
            Step::SearchedChart {
                query: "a1c".into(),
                group: Some(RecordGroup::Results),
                page: SearchPage::First,
                state: StepState::Returned {
                    result: Found {
                        records: 1,
                        more: false,
                    },
                },
            },
            Step::ReadRecords {
                requested: 3,
                state: StepState::Returned {
                    result: vec![
                        RecordRead::Whole {
                            version: SourceVersion {
                                candidate_id: "ehr/Observation/a1c".into(),
                                revision: "3".into(),
                            },
                            about: RecordAbout {
                                group: Some(RecordGroup::History),
                                title: Some("Hemoglobin A1c".into()),
                                dates: vec![RecordDate {
                                    role: Some(DateRole::Effective),
                                    value: "2025-01-21".into(),
                                }],
                            },
                        },
                        RecordRead::Whole {
                            version: SourceVersion {
                                candidate_id: "ehr/Binary/scan".into(),
                                revision: "1".into(),
                            },
                            about: RecordAbout {
                                group: None,
                                title: None,
                                dates: Vec::new(),
                            },
                        },
                        RecordRead::Passage {
                            version: SourceVersion {
                                candidate_id: "ehr/DocumentReference/note".into(),
                                revision: "2".into(),
                            },
                        },
                    ],
                },
            },
        ];
        let written = serde_json::to_value(&steps).unwrap();
        assert_eq!(written[0]["group"], "results");
        assert_eq!(
            written[1]["state"]["result"][0]["about"]["group"],
            "history"
        );
        assert_eq!(
            written[1]["state"]["result"][0]["about"]["title"],
            "Hemoglobin A1c"
        );
        assert_eq!(
            written[1]["state"]["result"][2],
            serde_json::json!({"outcome": "passage", "version": {
                "candidate_id": "ehr/DocumentReference/note", "revision": "2"}})
        );
        assert_eq!(serde_json::from_value::<Vec<Step>>(written).unwrap(), steps);
        let groups = [
            "patient",
            "problems",
            "medications",
            "allergies",
            "results",
            "vitals",
            "notes",
            "documents",
            "visits",
            "procedures",
            "immunizations",
            "orders",
            "history",
            "forms",
            "messages",
            "other",
        ];
        for name in groups {
            let group: RecordGroup = serde_json::from_value(serde_json::json!(name)).unwrap();
            assert_eq!(serde_json::to_value(group).unwrap(), name);
        }
        assert!(
            serde_json::from_str::<RecordRead>(
                r#"{"outcome": "whole", "record_type": "Observation", "dates": []}"#
            )
            .is_err(),
            "a read that names a source's type is not read"
        );
    }

    #[test]
    fn reads_a_snapshot_whose_feedback_was_unavailable() {
        let page: ChatSnapshotResponse = serde_json::from_str(
            r#"{"snapshot": {"feedback": "unavailable", "transcript": [{
                "work_item_id": "w1", "messages": [], "steps": {"state": "read", "steps": []}, "kept": [],
                "answer": {"state": "finished", "response_id": "a1:reply", "attempt_id": "a1",
                           "finished_at": 1794600000,
                           "text": "Sodium is stable.", "feedback": null}
            }]}}"#,
        )
        .unwrap();
        assert_eq!(page.snapshot.feedback, SnapshotFeedback::Unavailable);
        assert!(
            serde_json::from_str::<SnapshotFeedback>(r#""partial""#).is_err(),
            "an unknown feedback state is refused, not read as loaded"
        );
    }

    #[test]
    fn memory_steps_and_patient_memory_wire_round_trip() {
        let steps = vec![
            Step::SavedMemoryNote {
                state: StepState::Returned { result: () },
            },
            Step::EditedMemory {
                state: StepState::Running,
            },
            Step::CreatedFollowUp {
                state: StepState::Returned { result: () },
            },
        ];
        let json = serde_json::to_value(&steps).unwrap();
        assert_eq!(json[0]["did"], "saved_memory_note");
        assert_eq!(json[1]["did"], "edited_memory");
        assert_eq!(json[2]["did"], "created_follow_up");
        assert_eq!(serde_json::from_value::<Vec<Step>>(json).unwrap(), steps);

        let memory = PatientMemoryResponse {
            patient_workspace_id: "pw-1".into(),
            durable: true,
            summary: Some(MemoryDocumentWire {
                version: 2,
                saved_at: "2026-11-19".into(),
                markdown: "# Summary\nCKD stage 3b.".into(),
            }),
            direction: Some(MemoryDocumentWire {
                version: 1,
                saved_at: "2026-11-19".into(),
                markdown: "Hold lisinopril.".into(),
            }),
        };
        let encoded = serde_json::to_value(&memory).unwrap();
        assert_eq!(
            serde_json::from_value::<PatientMemoryResponse>(encoded).unwrap(),
            memory
        );

        let req = SavePhysicianMemoryEditRequest {
            target: MemoryEditTarget::Direction,
            text: "Keep potassium above 4.0.".into(),
            idempotency_key: "key-1".into(),
        };
        let encoded = serde_json::to_value(&req).unwrap();
        assert_eq!(encoded["target"], "direction");
        assert_eq!(
            serde_json::from_value::<SavePhysicianMemoryEditRequest>(encoded).unwrap(),
            req
        );

        let resp = SavePhysicianMemoryEditResponse {
            note_number: "N2".into(),
            log_path: "log/2026-11-19.md".into(),
            saved_at: "2026-11-19T20:00:00Z".into(),
        };
        let encoded = serde_json::to_value(&resp).unwrap();
        assert_eq!(
            serde_json::from_value::<SavePhysicianMemoryEditResponse>(encoded).unwrap(),
            resp
        );
    }
}
