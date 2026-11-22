//! Typed wire contracts for Spruce unmatched message routing (`Messages to match`),
//! classified non-patient mail (`Filtered`), unreadable channel deliveries, and
//! patient-scoped Spruce reply cards, private drafts, revisions, copy attributions,
//! and conversation moves (`Wrong patient?` / `Undo`).

use serde::{Deserialize, Serialize};

use crate::ReplyRevisionRun;
use crate::today_updates::ItemCitationWire;

/// Urgency standing for an unmatched patient message in `Messages to match`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum UnmatchedUrgencyWire {
    /// Routine patient mail.
    Ordinary,
    /// Urgent clinical symptoms leading `Today` as `Now` before patient linking.
    Urgent {
        /// Plain-language clinical reason this unmatched message needs prompt attention.
        reason: String,
        /// RFC 3339 timestamp when urgency was flagged.
        flagged_at: String,
    },
}

/// Authorized patient identity candidate shown when Coda cannot narrow to a single match.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CandidatePatientWire {
    /// Authorized patient workspace identifier.
    pub patient_workspace_id: String,
    /// Patient display name.
    pub patient_display_name: String,
    /// Date of birth when supplied by demographics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub birth_date: Option<String>,
    /// Primary medical record number when supplied by demographics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mrn: Option<String>,
}

/// Closed reason why Coda needs a physician's explicit patient choice instead of suggesting one.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NeedsChoiceReasonWire {
    /// Multiple authorized patients match the permitted identity evidence.
    AmbiguousCandidates,
    /// Identity evidence points to conflicting patient records.
    ConflictingEvidence,
    /// No registered patient matched the permitted identity evidence.
    NoMatchFound,
    /// Coda has not yet completed classification before the waiting deadline.
    ClassificationTimedOut,
}

/// Access-filtered patient match suggestion for a message in `Messages to match`.
///
/// Carries **zero** numeric confidence scores (`intent/physician.md` Rule 8).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MatchSuggestionWire {
    /// Single authorized panel patient suggested from permitted identity evidence.
    SinglePatient {
        /// Authorized patient workspace identifier.
        patient_workspace_id: String,
        /// Patient display name.
        patient_display_name: String,
        /// Date of birth when supplied by demographics.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        birth_date: Option<String>,
        /// Primary medical record number when supplied by demographics.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mrn: Option<String>,
        /// Plain-language permitted identity basis (name, DOB, MRN, or confirmed sender).
        evidence_summary: String,
    },
    /// No single authorized suggestion; the physician chooses a patient or leaves unlinked.
    NeedsPhysicianChoice {
        /// Why Coda did not narrow to one suggested patient.
        reason: NeedsChoiceReasonWire,
        /// Plain-language identity summary without disclosing unauthorized patient identities.
        evidence_summary: String,
        /// Authorized candidate patients when multiple panel patients matched.
        #[serde(default)]
        candidate_patients: Vec<CandidatePatientWire>,
    },
}

/// Attachment summary shown alongside a Spruce message.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SpruceAttachmentWire {
    /// Stable attachment identifier.
    pub attachment_id: String,
    /// Attachment file name when provided.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    /// MIME content type.
    pub mime_type: String,
}

/// One message waiting for a physician's patient match in `Messages to match`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UnmatchedMessageWire {
    /// Stable Spruce message identifier.
    pub message_id: String,
    /// Stable Spruce conversation identifier.
    pub conversation_id: String,
    /// Channel sender display name (`author_name` or sender address fallback).
    pub sender_display_name: String,
    /// Normalized sender phone or email address when supplied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sender_address: Option<String>,
    /// Message text body.
    pub body_text: String,
    /// Attachment metadata summaries.
    #[serde(default)]
    pub attachments: Vec<SpruceAttachmentWire>,
    /// Direct Spruce application link when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spruce_app_url: Option<String>,
    /// RFC 3339 timestamp when the message entered `Messages to match`.
    pub waiting_since: String,
    /// Urgency standing (`ordinary` vs `urgent` leading Today as `Now`).
    pub urgency: UnmatchedUrgencyWire,
    /// Access-filtered patient suggestion (`Membership::Allowed` only, zero confidence scores).
    pub suggestion: MatchSuggestionWire,
}

/// One unreadable Spruce delivery surfaced for manual physician handling in Spruce.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UnreadableSpruceMessageWire {
    /// Stable Spruce message identifier.
    pub message_id: String,
    /// Stable Spruce conversation identifier.
    pub conversation_id: String,
    /// Reason the delivery could not be read.
    pub failure_reason: String,
    /// Direct link to open the message in Spruce.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spruce_app_url: Option<String>,
    /// RFC 3339 timestamp when the unreadable item was recorded.
    pub kept_at: String,
}

/// Practice-level Spruce channel health banner shown on Today and `Messages to match`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SpruceChannelBannerWire {
    /// Spruce is not configured for this practice.
    NotConfigured,
    /// Spruce is connected and reading deliveries normally.
    ConnectedReading {
        /// RFC 3339 timestamp of the latest successful read when present.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        last_read_at: Option<String>,
    },
    /// Spruce encountered a read failure after a known good read.
    ReadFailed {
        /// RFC 3339 timestamp of the last successful read, if any.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        last_read_at: Option<String>,
        /// Failure reason.
        reason: String,
        /// RFC 3339 timestamp when the failure was recorded.
        failed_at: String,
    },
    /// Practice credentials lost access and require reconnection.
    ReconnectRequired {
        /// RFC 3339 timestamp of the last successful read, if any.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        last_read_at: Option<String>,
        /// Reason access was lost.
        reason: String,
        /// RFC 3339 timestamp when access was lost.
        lost_at: String,
    },
    /// Practice disconnected the Spruce channel.
    Disconnected {
        /// Reason the channel was disconnected.
        reason: String,
        /// RFC 3339 timestamp when disconnected.
        disconnected_at: String,
    },
}

/// Response body for `GET /spruce-matching/messages`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MessagesToMatchResponseWire {
    /// Practice-level Spruce channel standing.
    pub channel_banner: SpruceChannelBannerWire,
    /// Ordered messages waiting for physician matching (urgent `Now` rows first).
    #[serde(default)]
    pub messages: Vec<UnmatchedMessageWire>,
    /// Unreadable Spruce deliveries awaiting manual check and `Done`.
    #[serde(default)]
    pub unreadable_messages: Vec<UnreadableSpruceMessageWire>,
    /// Count of classified non-patient messages currently in `Filtered`.
    #[serde(default)]
    pub filtered_count: usize,
    /// True when one or more patients could not have panel access verified and were skipped.
    #[serde(default)]
    pub may_be_missing_someone: bool,
}

/// Whether confirming a patient link also records the sender address as confirmed for future mail.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SenderConfirmationPolicyWire {
    /// Link only this conversation without confirming the sender address for future conversations.
    ConversationOnly,
    /// Link this conversation and confirm the sender address for this patient.
    AlsoConfirmSenderForPatient,
}

/// Request body for `POST /spruce-matching/messages/{message_id}/confirm`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConfirmMessageLinkBody {
    /// Target authorized patient workspace identifier.
    pub patient_workspace_id: String,
    /// Whether to also confirm the sender address for future messages from this sender.
    pub sender_policy: SenderConfirmationPolicyWire,
    /// Client idempotency key.
    pub idempotency_key: String,
}

/// Response body for `POST /spruce-matching/messages/{message_id}/confirm`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConfirmMessageLinkResponseWire {
    /// Linked message identifier.
    pub message_id: String,
    /// Linked Spruce conversation identifier.
    pub conversation_id: String,
    /// Linked patient workspace identifier.
    pub patient_workspace_id: String,
    /// Linked patient display name.
    pub patient_display_name: String,
    /// Message identifiers on the same conversation linked alongside this confirmation.
    #[serde(default)]
    pub linked_message_ids: Vec<String>,
}

/// Request body for `POST /spruce-matching/messages/{message_id}/leave-unlinked`
/// and `POST /spruce-matching/unreadable/{message_id}/done`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MatchingActionCommandBody {
    /// Client idempotency key.
    pub idempotency_key: String,
}

/// Closed saved standing returned after leaving a message unlinked or marking an unreadable item done.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MatchedMessageStandingWire {
    /// Left unlinked on the channel (`Leave unlinked`).
    Unlinked,
    /// Unreadable delivery acknowledged manually (`Done`).
    Done,
}

/// Response body after leaving a message unlinked or marking an unreadable item done.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MatchingMessageStatusResponseWire {
    /// Target Spruce message identifier.
    pub message_id: String,
    /// Saved standing (`unlinked` or `done`).
    pub standing: MatchedMessageStandingWire,
}

/// Closed non-patient mail classification category in `Filtered`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NonPatientCategoryWire {
    /// Vendor pitch, marketing, or spam.
    VendorOrSpam,
    /// Practice-level administrative or operational notice.
    AdministrativeNotice,
    /// Non-patient external outreach.
    NonPatientOutreach,
}

impl NonPatientCategoryWire {
    /// Physician-facing category label.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::VendorOrSpam => "Vendor or spam",
            Self::AdministrativeNotice => "Administrative notice",
            Self::NonPatientOutreach => "External outreach",
        }
    }
}

/// One classified non-patient message shown in `Filtered`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FilteredMessageWire {
    /// Stable Spruce message identifier.
    pub message_id: String,
    /// Stable Spruce conversation identifier.
    pub conversation_id: String,
    /// Channel sender display name.
    pub sender_display_name: String,
    /// Normalized sender phone or email address when supplied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sender_address: Option<String>,
    /// Message text body.
    pub body_text: String,
    /// Direct Spruce application link when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spruce_app_url: Option<String>,
    /// Classified non-patient category.
    pub category: NonPatientCategoryWire,
    /// Plain-language rationale explaining why Coda filtered this message.
    pub rationale: String,
    /// RFC 3339 timestamp when the message was filtered.
    pub filtered_at: String,
}

/// Response body for `GET /spruce-matching/filtered`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FilteredMessagesResponseWire {
    /// Classified non-patient messages, newest first.
    #[serde(default)]
    pub messages: Vec<FilteredMessageWire>,
}

/// Routing result returned when a physician clicks `"This is a patient"` on a filtered message.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ThisIsAPatientOutcomeWire {
    /// Message immediately linked to an existing patient conversation or unique channel tie.
    Linked {
        /// Spruce message identifier.
        message_id: String,
        /// Spruce conversation identifier.
        conversation_id: String,
        /// Authorized patient workspace identifier when visible to the viewer.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        patient_workspace_id: Option<String>,
        /// Patient display name when visible to the viewer.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        patient_display_name: Option<String>,
    },
    /// Message moved into `Messages to match` for explicit patient selection.
    WaitingInMessagesToMatch {
        /// Spruce message identifier.
        message_id: String,
        /// Spruce conversation identifier.
        conversation_id: String,
    },
}

/// Whether a channel sender is the patient themselves or a caregiver/other sender.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChannelSenderRoleWire {
    /// The linked patient sent the message.
    Patient,
    /// A caregiver, parent, or other non-patient sender wrote about the patient.
    NonPatient,
}

/// One linked patient-side channel message displayed on the reply card.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReplyPatientMessageWire {
    /// Stable message identifier.
    pub message_id: String,
    /// Monotonic sequence number within the conversation.
    pub sequence: u64,
    /// Channel sender display name (never replaced with the linked patient's name).
    pub sender_display_name: String,
    /// Whether this sender is the patient or a non-patient sender.
    pub sender_role: ChannelSenderRoleWire,
    /// Message text body.
    pub text: String,
    /// RFC 3339 timestamp when sent.
    pub sent_at: String,
    /// Whether this message arrived after the proposal currently shown on the card.
    #[serde(default)]
    pub arrived_after_proposal: bool,
}

/// Kind of practice-sent message observed on the Spruce channel.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PracticeMessageKindWire {
    /// Human reply sent by practice staff or a physician in Spruce.
    HumanReply,
    /// Automated Spruce acknowledgement or office-hours message.
    AutomatedAcknowledgement,
}

/// One practice-sent channel message observed from Spruce.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReplyPracticeMessageWire {
    /// Stable practice message identifier.
    pub practice_message_id: String,
    /// Monotonic sequence number.
    pub sequence: u64,
    /// Kind of practice message.
    pub kind: PracticeMessageKindWire,
    /// Message text body.
    pub text: String,
    /// RFC 3339 timestamp when sent.
    pub sent_at: String,
}

/// How the physician's private draft was last produced.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PhysicianDraftKindWire {
    /// Direct text edited and saved by this physician.
    YourText,
    /// Revised by Coda on this physician's instruction, retaining prior wording for `Restore my version`.
    CodaRevision {
        /// Exact physician wording replaced by the latest Coda revision.
        pre_revision_your_text: String,
    },
}

/// Versioned reply text currently shown to the signed-in physician.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum DisplayedReplyTextWire {
    /// Unedited shared Coda proposal at `proposal_version`.
    CodaProposal {
        /// Proposal version number.
        proposal_version: u64,
        /// Drafted reply text.
        text: String,
    },
    /// This physician's private saved draft at `revision`.
    PhysicianDraft {
        /// Private draft revision number.
        revision: u64,
        /// Saved private draft text.
        text: String,
        /// Whether this draft is direct physician wording or a Coda revision.
        kind: PhysicianDraftKindWire,
    },
    /// Reply text withheld while Coda re-reviews a conversation moved to another patient.
    Withheld {
        /// Last proposal version before withholding.
        proposal_version: u64,
        /// Plain-language reason the prior proposal is withheld.
        reason: String,
    },
}

/// Expected version fence sent with physician draft edits, `Copy reply`, and `No reply needed`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExpectedDraftBaseWire {
    /// Physician was viewing Coda's unedited proposal at `version`.
    ProposalVersion {
        /// Proposal version displayed on screen.
        version: u64,
    },
    /// Physician was viewing their own private saved draft at `revision`.
    PhysicianDraftRevision {
        /// Private draft revision displayed on screen.
        revision: u64,
    },
}

/// Non-destructive offer when a newer Coda proposal arrives while the physician has private edits.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CodaUpdateOfferWire {
    /// Newer proposal version number.
    pub offered_proposal_version: u64,
    /// Newer proposed reply text.
    pub text: String,
    /// Basis and limitations for the newer proposal.
    pub basis_and_limitations: String,
    /// Verified chart citations backing the newer proposal.
    #[serde(default)]
    pub citations: Vec<ItemCitationWire>,
}

/// Copy attribution shown to other physicians until a practice message is observed on the channel.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CopyAttributionWire {
    /// Copy record identifier.
    pub copy_id: String,
    /// Display name or identifier of the physician who copied a reply.
    pub copied_by_physician: String,
    /// RFC 3339 timestamp when the reply was copied.
    pub copied_at: String,
}

/// Whether Coda is currently reviewing new activity on the linked conversation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum CardReviewStateWire {
    /// No active review is running.
    Idle,
    /// Coda is actively reviewing new messages or source changes while keeping messages visible.
    Reviewing {
        /// RFC 3339 timestamp when the review started or queued.
        started_at: String,
    },
}

/// Channel connectivity state on an open reply card.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum CardChannelBannerWire {
    /// Spruce channel is connected.
    Connected,
    /// Spruce channel was disconnected while this reply remained open.
    Disconnected {
        /// Reason the channel was disconnected.
        reason: String,
        /// RFC 3339 timestamp when disconnected.
        disconnected_at: String,
        /// Direct link to open the conversation in Spruce.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        spruce_app_url: Option<String>,
    },
}

/// Destination named by a pending `Wrong patient?` move during its `Undo` window.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PendingMoveTargetDisplayWire {
    /// Moving to another patient workspace.
    AnotherPatient {
        /// Destination patient workspace identifier when authorized for the viewer.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        patient_workspace_id: Option<String>,
        /// Destination patient display name when authorized for the viewer.
        patient_label: String,
    },
    /// Moving out of patient care (`Not a patient`).
    NotAPatient,
}

/// Active `Undo` window for a recent `Wrong patient?` conversation move.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PendingSpruceMoveWire {
    /// Destination of the pending move.
    pub destination: PendingMoveTargetDisplayWire,
    /// Physician who initiated the move.
    pub moved_by_physician: String,
    /// RFC 3339 timestamp when the `Undo` window expires and the move settles.
    pub expires_at: String,
}

/// One-line arrival or replacement notice shown on an open reply card.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CardArrivalNoticeWire {
    /// Coda's newer reply replaced an earlier proposal version on the card.
    NewerReplyReplacedPrevious {
        /// Replaced proposal version.
        previous_version: u64,
        /// Current proposal version.
        current_version: u64,
    },
    /// A new message arrived after the displayed proposal was drafted.
    PatientWroteAgain,
    /// An older message was read late; never states that the patient wrote again.
    OlderMessageReadLate,
}

/// Full open reply card projected for a single conversation and signed-in physician.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReplyCardWire {
    /// Stable Spruce conversation identifier.
    pub conversation_id: String,
    /// Authorized patient workspace identifier.
    pub patient_workspace_id: String,
    /// Linked patient display name.
    pub patient_display_name: String,
    /// Channel sender display name leading the reply card (`Rule`: names the channel sender, never substituting the linked patient).
    pub primary_sender_name: String,
    /// Whether the primary channel sender is the patient or a non-patient sender.
    pub primary_sender_role: ChannelSenderRoleWire,
    /// Additional non-patient sender names participating on the conversation, shown alongside.
    #[serde(default)]
    pub non_patient_sender_names: Vec<String>,
    /// Direct link to open this conversation in Spruce.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spruce_app_url: Option<String>,
    /// Open reply identifier (`reply_id`).
    pub reply_id: u64,
    /// Conversation version fence (`conversation_version`).
    pub conversation_version: u64,
    /// Latest shared Coda proposal version (`proposal_version`).
    pub proposal_version: u64,
    /// Whether an open clinical concern is associated with this conversation (`Flagged` on Today).
    #[serde(default)]
    pub has_open_concern: bool,
    /// Open patient messages from one consistent conversation snapshot.
    #[serde(default)]
    pub patient_messages: Vec<ReplyPatientMessageWire>,
    /// Observed practice messages on this conversation.
    #[serde(default)]
    pub practice_messages: Vec<ReplyPracticeMessageWire>,
    /// Reply text currently displayed for this physician (`CodaProposal`, `PhysicianDraft`, or `Withheld`).
    pub displayed_reply: DisplayedReplyTextWire,
    /// Live or settled status of this physician's latest Coda revision run, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision_run: Option<ReplyRevisionRun>,
    /// Newer Coda proposal offered without overwriting this physician's private edits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coda_update_offer: Option<CodaUpdateOfferWire>,
    /// Ordered one-line arrival or replacement notices on this card.
    #[serde(default)]
    pub notices: Vec<CardArrivalNoticeWire>,
    /// Basis and limitations for the proposal backing this card.
    pub basis_and_limitations: String,
    /// Verified chart citations backing the proposal (`CitationChip` -> `ReaderDialog`).
    #[serde(default)]
    pub citations: Vec<ItemCitationWire>,
    /// Copy attributions from other physicians, cleared once a practice message is observed.
    #[serde(default)]
    pub other_physician_copies: Vec<CopyAttributionWire>,
    /// Whether this signed-in physician has already copied the reply since the last practice message.
    #[serde(default)]
    pub copied_by_me: bool,
    /// Active review state (`Idle` vs `Reviewing`).
    pub review_state: CardReviewStateWire,
    /// Channel connection banner (`Connected` vs `Disconnected` with `Done`).
    pub channel_banner: CardChannelBannerWire,
    /// Active `Wrong patient?` move `Undo` banner when a move is pending settlement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_move: Option<PendingSpruceMoveWire>,
}

/// Closed reply surface state for one linked Spruce conversation on a patient page.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "surface", rename_all = "snake_case")]
pub enum PatientReplyConversationWire {
    /// An open reply card is ready (or withheld during move re-review).
    OpenCard(Box<ReplyCardWire>),
    /// Physician closed the reply as `No reply needed`; `Undo` remains available until new channel activity.
    ClosedUndoBanner {
        /// Spruce conversation identifier.
        conversation_id: String,
        /// Linked patient workspace identifier.
        patient_workspace_id: String,
        /// Channel sender display name.
        primary_sender_name: String,
        /// Channel sender role.
        primary_sender_role: ChannelSenderRoleWire,
        /// Direct link to Spruce when available.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        spruce_app_url: Option<String>,
        /// Physician who closed the reply.
        closed_by_physician: String,
        /// Pending move `Undo` banner if any.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pending_move: Option<PendingSpruceMoveWire>,
    },
    /// Linked conversation has no open reply card; messages remain readable and `Ask Coda to reply` may be offered.
    NoOpenCard {
        /// Spruce conversation identifier.
        conversation_id: String,
        /// Linked patient workspace identifier.
        patient_workspace_id: String,
        /// Channel sender display name.
        primary_sender_name: String,
        /// Channel sender role.
        primary_sender_role: ChannelSenderRoleWire,
        /// Non-patient sender names on the conversation.
        #[serde(default)]
        non_patient_sender_names: Vec<String>,
        /// Direct link to Spruce when available.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        spruce_app_url: Option<String>,
        /// Patient messages on the conversation.
        #[serde(default)]
        patient_messages: Vec<ReplyPatientMessageWire>,
        /// Practice messages observed on the conversation.
        #[serde(default)]
        practice_messages: Vec<ReplyPracticeMessageWire>,
        /// Active review state (`Idle` vs `Reviewing`).
        review_state: CardReviewStateWire,
        /// Whether the physician can trigger `Ask Coda to reply`.
        can_ask_coda_to_reply: bool,
        /// Pending move `Undo` banner if any.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pending_move: Option<PendingSpruceMoveWire>,
    },
}

/// Response body for `GET /patient-workspaces/{id}/spruce-replies`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PatientReplyListResponseWire {
    /// Patient workspace identifier.
    pub patient_workspace_id: String,
    /// Ordered linked Spruce conversations for this patient.
    #[serde(default)]
    pub conversations: Vec<PatientReplyConversationWire>,
}

/// Request body for `PUT /patient-workspaces/{id}/spruce-replies/{conversation_id}/draft`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SaveReplyProposalDraftBody {
    /// Displayed base version the physician edited from.
    pub expected_base: ExpectedDraftBaseWire,
    /// Complete edited reply text.
    pub text: String,
}

/// Request body for `POST /patient-workspaces/{id}/spruce-replies/{conversation_id}/revise`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RequestReplyProposalRevisionBody {
    /// Displayed base version the physician asked Coda to revise (`ProposalVersion` or `PhysicianDraftRevision`).
    pub expected_base: ExpectedDraftBaseWire,
    ///Displayed reply text when starting from `ProposalVersion` so a private draft can be seeded atomically if needed.
    pub current_text: String,
    /// Physician's revision instruction.
    pub instruction: String,
    /// Physician's IANA time zone.
    pub physician_time_zone: String,
    /// Client idempotency key.
    pub idempotency_key: String,
}

/// Request body for `POST /patient-workspaces/{id}/spruce-replies/{conversation_id}/restore-my-version`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RestoreMyReplyVersionBody {
    /// Current `CodaRevision` draft revision shown to the physician.
    pub expected_draft_revision: u64,
    /// Client idempotency key.
    pub idempotency_key: String,
}

/// Request body for `POST /patient-workspaces/{id}/spruce-replies/{conversation_id}/choose-update`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChooseCodaReplyUpdateBody {
    /// Current private draft revision shown to the physician.
    pub expected_draft_revision: u64,
    /// Offered proposal version the physician chose to adopt.
    pub expected_proposal_version: u64,
    /// Client idempotency key.
    pub idempotency_key: String,
}

/// Request body for `POST /patient-workspaces/{id}/spruce-replies/{conversation_id}/copy`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CopyReplyBody {
    /// Displayed version copied to the physician's clipboard.
    pub expected_base: ExpectedDraftBaseWire,
    /// Client idempotency key.
    pub idempotency_key: String,
}

/// Response body for `POST /patient-workspaces/{id}/spruce-replies/{conversation_id}/copy`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CopyReplyResponseWire {
    /// Exact text recorded as copied, while keeping the reply card open.
    pub copied_text: String,
    /// Updated conversation surface (still `OpenCard`).
    pub surface: PatientReplyConversationWire,
}

/// Request body for `POST /patient-workspaces/{id}/spruce-replies/{conversation_id}/no-reply-needed`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NoReplyNeededBody {
    /// Latest proposal version displayed when the physician chose `No reply needed`.
    pub expected_proposal_version: u64,
    /// Client idempotency key.
    pub idempotency_key: String,
}

/// Request body for `POST /patient-workspaces/{id}/spruce-replies/{conversation_id}/undo-no-reply-needed`
/// and `POST /patient-workspaces/{id}/spruce-replies/{conversation_id}/ask-coda`
/// and `POST /patient-workspaces/{id}/spruce-replies/{conversation_id}/undo-move`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReplyCommandKeyBody {
    /// Client idempotency key.
    pub idempotency_key: String,
}

/// Destination chosen when the physician clicks `Wrong patient?` on a linked conversation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MoveDestinationWire {
    /// Re-link the conversation to another registered patient workspace.
    AnotherPatient {
        /// Target patient workspace identifier.
        patient_workspace_id: String,
    },
    /// Remove the conversation from patient care (`Not a patient`).
    NotAPatient,
}

/// Request body for `POST /patient-workspaces/{id}/spruce-replies/{conversation_id}/move`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MoveSpruceConversationBody {
    /// Destination (`AnotherPatient` or `NotAPatient`).
    pub destination: MoveDestinationWire,
    /// Optional override for the `Undo` window duration in seconds (defaults to server standard).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub undo_window_seconds: Option<i64>,
    /// Client idempotency key.
    pub idempotency_key: String,
}

/// Expected card standing when marking a disconnected reply `Done`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "standing", rename_all = "snake_case")]
pub enum DisconnectedCardStandingWire {
    /// Card displayed a ready reply proposal at `proposal_version`.
    Ready {
        /// Proposal version displayed on the card.
        proposal_version: u64,
    },
    /// Card displayed a withheld reply after a conversation move.
    Withheld,
}

/// Request body for `POST /patient-workspaces/{id}/spruce-replies/{conversation_id}/disconnected-done`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DisconnectedReplyDoneBody {
    /// Open reply identifier displayed on the card.
    pub expected_reply_id: u64,
    /// Card standing (`Ready { proposal_version }` vs `Withheld`).
    pub standing: DisconnectedCardStandingWire,
    /// Client idempotency key.
    pub idempotency_key: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unmatched_wire_contains_zero_confidence_fields_and_roundtrips() {
        let response = MessagesToMatchResponseWire {
            channel_banner: SpruceChannelBannerWire::ConnectedReading {
                last_read_at: Some("2026-04-15T14:00:00Z".into()),
            },
            messages: vec![UnmatchedMessageWire {
                message_id: "msg-1".into(),
                conversation_id: "conv-1".into(),
                sender_display_name: " Jordan Diaz ".into(),
                sender_address: Some("+14155550101".into()),
                body_text: "My home blood pressure was 168/102 with a headache.".into(),
                attachments: vec![],
                spruce_app_url: Some("https://app.sprucehealth.com/conversations/conv-1".into()),
                waiting_since: "2026-04-15T14:01:00Z".into(),
                urgency: UnmatchedUrgencyWire::Urgent {
                    reason: "Symptomatic severe hypertension".into(),
                    flagged_at: "2026-04-15T14:01:05Z".into(),
                },
                suggestion: MatchSuggestionWire::SinglePatient {
                    patient_workspace_id: "ws-elena".into(),
                    patient_display_name: "Elena Diaz".into(),
                    birth_date: Some("1958-09-14".into()),
                    mrn: Some("MRN-10042".into()),
                    evidence_summary: "Matched caregiver phone and patient surname Diaz.".into(),
                },
            }],
            unreadable_messages: vec![],
            filtered_count: 1,
            may_be_missing_someone: false,
        };

        let encoded = serde_json::to_string(&response).expect("serialize");
        assert!(
            !encoded.to_ascii_lowercase().contains("confidence"),
            "serialized wire payload must never expose confidence scores: {encoded}"
        );
        let decoded: MessagesToMatchResponseWire =
            serde_json::from_str(&encoded).expect("deserialize");
        assert_eq!(decoded, response);
    }

    #[test]
    fn reply_card_wire_roundtrips_withheld_and_coda_revision_states() {
        let card = PatientReplyConversationWire::OpenCard(Box::new(ReplyCardWire {
            conversation_id: "conv-7".into(),
            patient_workspace_id: "ws-1".into(),
            patient_display_name: "Elena Morales".into(),
            primary_sender_name: "Carlos Morales (Son)".into(),
            primary_sender_role: ChannelSenderRoleWire::NonPatient,
            non_patient_sender_names: vec!["Carlos Morales (Son)".into()],
            spruce_app_url: Some("https://app.sprucehealth.com/conversations/conv-7".into()),
            reply_id: 2,
            conversation_version: 5,
            proposal_version: 3,
            has_open_concern: true,
            patient_messages: vec![ReplyPatientMessageWire {
                message_id: "m-1".into(),
                sequence: 1,
                sender_display_name: "Carlos Morales (Son)".into(),
                sender_role: ChannelSenderRoleWire::NonPatient,
                text: "Mom's ankle swelling increased.".into(),
                sent_at: "2026-04-15T15:00:00Z".into(),
                arrived_after_proposal: false,
            }],
            practice_messages: vec![],
            displayed_reply: DisplayedReplyTextWire::PhysicianDraft {
                revision: 2,
                text: "Shortened draft.".into(),
                kind: PhysicianDraftKindWire::CodaRevision {
                    pre_revision_your_text: "My earlier wording.".into(),
                },
            },
            revision_run: None,
            coda_update_offer: None,
            notices: vec![CardArrivalNoticeWire::NewerReplyReplacedPrevious {
                previous_version: 2,
                current_version: 3,
            }],
            basis_and_limitations: "Based on outpatient BMP from 2026-04-10.".into(),
            citations: vec![],
            other_physician_copies: vec![],
            copied_by_me: false,
            review_state: CardReviewStateWire::Idle,
            channel_banner: CardChannelBannerWire::Connected,
            pending_move: None,
        }));

        let json = serde_json::to_string(&card).expect("serialize");
        let parsed: PatientReplyConversationWire = serde_json::from_str(&json).expect("parse");
        assert_eq!(parsed, card);

        let status = MatchingMessageStatusResponseWire {
            message_id: "msg-done-1".into(),
            standing: MatchedMessageStandingWire::Done,
        };
        let status_json = serde_json::to_string(&status).expect("serialize status");
        assert!(status_json.contains("\"standing\":\"done\""));
        let parsed_status: MatchingMessageStatusResponseWire =
            serde_json::from_str(&status_json).expect("parse status");
        assert_eq!(parsed_status, status);
    }
}
