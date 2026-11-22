//! SQLite-backed Spruce `Messages to match`, `Filtered`, and patient `Spruce` reply cards.

use crate::driver::DbError;
use crate::schema::{PracticeConnections, SpruceMatchQueue, SpruceReplies, TodayItems};
use crate::store::{DEMO_NOW, DemoStore, build_item_citation, next_seq, not_found};
use physician_wire::ReplyRevisionRun;
use physician_wire::matching_reply::{
    CardChannelBannerWire, CardReviewStateWire, ChannelSenderRoleWire,
    ConfirmMessageLinkResponseWire, CopyAttributionWire, CopyReplyResponseWire,
    DisconnectedReplyDoneBody, DisplayedReplyTextWire, ExpectedDraftBaseWire, FilteredMessageWire,
    FilteredMessagesResponseWire, MatchSuggestionWire, MatchedMessageStandingWire,
    MatchingMessageStatusResponseWire, MessagesToMatchResponseWire, MoveDestinationWire,
    PatientReplyConversationWire, PatientReplyListResponseWire, PendingMoveTargetDisplayWire,
    PendingSpruceMoveWire, PhysicianDraftKindWire, ReplyCardWire, ReplyPatientMessageWire,
    RequestReplyProposalRevisionBody, SenderConfirmationPolicyWire, SpruceChannelBannerWire,
    ThisIsAPatientOutcomeWire, UnmatchedMessageWire, UnmatchedUrgencyWire,
    UnreadableSpruceMessageWire,
};
use physician_wire::source_setup::{PracticeSourceWire, SourceStandingWire, SupportedSourceWire};
use sea_query::{Expr, ExprTrait, OnConflict, Order, Query};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum StoredMatchItem {
    Unmatched(UnmatchedMessageWire),
    Unreadable(UnreadableSpruceMessageWire),
    Filtered(FilteredMessageWire),
    Resolved { standing: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct StoredSpruceReply {
    pub(crate) surface: PatientReplyConversationWire,
    #[serde(default)]
    pub(crate) prior_open_card: Option<ReplyCardWire>,
    #[serde(default)]
    pub(crate) prior_move_source_ws: Option<String>,
}

impl DemoStore {
    /// Reads `Messages to match` and unreadable Spruce channel deliveries.
    pub async fn messages_to_match(&self) -> Result<MessagesToMatchResponseWire, DbError> {
        self.ensure_ready().await?;
        let q = Query::select()
            .columns([SpruceMatchQueue::Bucket, SpruceMatchQueue::PayloadJson])
            .from(SpruceMatchQueue::Table)
            .order_by(SpruceMatchQueue::WaitingSince, Order::Asc)
            .to_owned();
        let mut messages = Vec::new();
        let mut unreadable_messages = Vec::new();
        let mut filtered_count = 0usize;
        for row in self.driver.select(&q)? {
            let bucket = row.get_str("bucket")?;
            let item: StoredMatchItem = serde_json::from_str(&row.get_str("payload_json")?)?;
            match (bucket.as_str(), item) {
                ("unmatched", StoredMatchItem::Unmatched(m)) => messages.push(m),
                ("unreadable", StoredMatchItem::Unreadable(u)) => unreadable_messages.push(u),
                ("filtered", StoredMatchItem::Filtered(_)) => filtered_count += 1,
                _ => {}
            }
        }
        messages.sort_by_key(|m| {
            (
                !matches!(m.urgency, UnmatchedUrgencyWire::Urgent { .. }),
                m.waiting_since.clone(),
            )
        });
        let channel_banner = self.spruce_channel_banner()?;
        Ok(MessagesToMatchResponseWire {
            channel_banner,
            messages,
            unreadable_messages,
            filtered_count,
            may_be_missing_someone: false,
        })
    }

    /// Reads classified non-patient mail in `Filtered`.
    pub async fn filtered_spruce_messages(&self) -> Result<FilteredMessagesResponseWire, DbError> {
        self.ensure_ready().await?;
        let q = Query::select()
            .column(SpruceMatchQueue::PayloadJson)
            .from(SpruceMatchQueue::Table)
            .and_where(Expr::col(SpruceMatchQueue::Bucket).eq("filtered"))
            .order_by(SpruceMatchQueue::WaitingSince, Order::Desc)
            .to_owned();
        let mut messages = Vec::new();
        for row in self.driver.select(&q)? {
            if let StoredMatchItem::Filtered(msg) =
                serde_json::from_str(&row.get_str("payload_json")?)?
            {
                messages.push(msg);
            }
        }
        Ok(FilteredMessagesResponseWire { messages })
    }

    /// Confirms a patient link for an unmatched Spruce message and creates a reply surface.
    pub async fn confirm_spruce_message_link(
        &self,
        message_id: &str,
        patient_workspace_id: &str,
        _sender_policy: SenderConfirmationPolicyWire,
        _idempotency_key: &str,
    ) -> Result<ConfirmMessageLinkResponseWire, DbError> {
        self.ensure_ready().await?;
        let patient_display_name = self.load_patient_name(patient_workspace_id)?;
        let StoredMatchItem::Unmatched(msg) = self.load_match_item(message_id)? else {
            return Err(DbError::InvalidCommand(
                "Message is not waiting in Messages to match.".into(),
            ));
        };
        let resolved = StoredMatchItem::Resolved {
            standing: "linked".into(),
        };
        self.save_match_item(
            message_id,
            &msg.conversation_id,
            "resolved",
            &msg.waiting_since,
            &resolved,
        )?;
        let records = self.load_patient_records(patient_workspace_id)?;
        let citations = records.iter().take(1).map(build_item_citation).collect();
        let reply = StoredSpruceReply {
            surface: PatientReplyConversationWire::OpenCard(Box::new(ReplyCardWire {
                conversation_id: msg.conversation_id.clone(),
                patient_workspace_id: patient_workspace_id.to_owned(),
                patient_display_name: patient_display_name.clone(),
                primary_sender_name: msg.sender_display_name.clone(),
                primary_sender_role: ChannelSenderRoleWire::Patient,
                non_patient_sender_names: Vec::new(),
                spruce_app_url: msg.spruce_app_url.clone(),
                reply_id: next_seq(),
                conversation_version: 1,
                proposal_version: 1,
                has_open_concern: matches!(msg.urgency, UnmatchedUrgencyWire::Urgent { .. }),
                patient_messages: vec![ReplyPatientMessageWire {
                    message_id: msg.message_id.clone(),
                    sequence: 1,
                    sender_display_name: msg.sender_display_name.clone(),
                    sender_role: ChannelSenderRoleWire::Patient,
                    text: msg.body_text.clone(),
                    sent_at: msg.waiting_since.clone(),
                    arrived_after_proposal: false,
                }],
                practice_messages: Vec::new(),
                displayed_reply: DisplayedReplyTextWire::CodaProposal {
                    proposal_version: 1,
                    text: format!(
                        "Hi {}, thank you for reaching out. Our care team has reviewed your message and connected it to your chart.",
                        msg.sender_display_name
                    ),
                },
                revision_run: None,
                coda_update_offer: None,
                notices: Vec::new(),
                basis_and_limitations:
                    "Drafted from newly linked Spruce message and patient SQLite records.".into(),
                citations,
                other_physician_copies: Vec::new(),
                copied_by_me: false,
                review_state: CardReviewStateWire::Idle,
                channel_banner: CardChannelBannerWire::Connected,
                pending_move: None,
            })),
            prior_open_card: None,
            prior_move_source_ws: None,
        };
        self.save_stored_reply(
            &msg.conversation_id,
            patient_workspace_id,
            &msg.waiting_since,
            &reply,
        )?;
        Ok(ConfirmMessageLinkResponseWire {
            message_id: message_id.to_owned(),
            conversation_id: msg.conversation_id,
            patient_workspace_id: patient_workspace_id.to_owned(),
            patient_display_name,
            linked_message_ids: vec![message_id.to_owned()],
        })
    }

    /// Leaves a waiting message unlinked.
    pub async fn leave_spruce_message_unlinked(
        &self,
        message_id: &str,
        _idempotency_key: &str,
    ) -> Result<MatchingMessageStatusResponseWire, DbError> {
        self.ensure_ready().await?;
        let StoredMatchItem::Unmatched(msg) = self.load_match_item(message_id)? else {
            return Err(not_found("unmatched spruce message", message_id));
        };
        self.save_match_item(
            message_id,
            &msg.conversation_id,
            "resolved",
            &msg.waiting_since,
            &StoredMatchItem::Resolved {
                standing: "unlinked".into(),
            },
        )?;
        Ok(MatchingMessageStatusResponseWire {
            message_id: message_id.to_owned(),
            standing: MatchedMessageStandingWire::Unlinked,
        })
    }

    /// Moves a filtered message into `Messages to match`.
    pub async fn mark_spruce_message_as_patient(
        &self,
        message_id: &str,
        _idempotency_key: &str,
    ) -> Result<ThisIsAPatientOutcomeWire, DbError> {
        self.ensure_ready().await?;
        let StoredMatchItem::Filtered(msg) = self.load_match_item(message_id)? else {
            return Err(not_found("filtered spruce message", message_id));
        };
        let unmatched = UnmatchedMessageWire {
            message_id: msg.message_id.clone(),
            conversation_id: msg.conversation_id.clone(),
            sender_display_name: msg.sender_display_name,
            sender_address: msg.sender_address,
            body_text: msg.body_text,
            attachments: Vec::new(),
            spruce_app_url: msg.spruce_app_url,
            waiting_since: msg.filtered_at.clone(),
            urgency: UnmatchedUrgencyWire::Ordinary,
            suggestion: MatchSuggestionWire::NeedsPhysicianChoice {
                reason: physician_wire::matching_reply::NeedsChoiceReasonWire::NoMatchFound,
                evidence_summary: "Physician reclassified filtered message as a patient inquiry."
                    .into(),
                candidate_patients: Vec::new(),
            },
        };
        self.save_match_item(
            message_id,
            &msg.conversation_id,
            "unmatched",
            &msg.filtered_at,
            &StoredMatchItem::Unmatched(unmatched),
        )?;
        Ok(ThisIsAPatientOutcomeWire::WaitingInMessagesToMatch {
            message_id: message_id.to_owned(),
            conversation_id: msg.conversation_id,
        })
    }

    /// Marks an unreadable Spruce delivery `Done`.
    pub async fn mark_unreadable_spruce_message_done(
        &self,
        message_id: &str,
        _idempotency_key: &str,
    ) -> Result<MatchingMessageStatusResponseWire, DbError> {
        self.ensure_ready().await?;
        let StoredMatchItem::Unreadable(msg) = self.load_match_item(message_id)? else {
            return Err(not_found("unreadable spruce message", message_id));
        };
        self.save_match_item(
            message_id,
            &msg.conversation_id,
            "resolved",
            &msg.kept_at,
            &StoredMatchItem::Resolved {
                standing: "done".into(),
            },
        )?;
        Ok(MatchingMessageStatusResponseWire {
            message_id: message_id.to_owned(),
            standing: MatchedMessageStandingWire::Done,
        })
    }

    /// Lists all linked Spruce reply conversations on `patient_workspace_id`.
    pub async fn patient_spruce_replies(
        &self,
        patient_workspace_id: &str,
    ) -> Result<PatientReplyListResponseWire, DbError> {
        self.ensure_ready().await?;
        let _ = self.load_patient_name(patient_workspace_id)?;
        let q = Query::select()
            .column(SpruceReplies::PayloadJson)
            .from(SpruceReplies::Table)
            .and_where(Expr::col(SpruceReplies::WorkspaceId).eq(patient_workspace_id))
            .order_by(SpruceReplies::WaitingSince, Order::Desc)
            .to_owned();
        let conversations = self
            .driver
            .select(&q)?
            .iter()
            .map(|r| {
                let stored: StoredSpruceReply = serde_json::from_str(&r.get_str("payload_json")?)?;
                Ok(stored.surface)
            })
            .collect::<Result<Vec<_>, DbError>>()?;
        Ok(PatientReplyListResponseWire {
            patient_workspace_id: patient_workspace_id.to_owned(),
            conversations,
        })
    }

    /// Reads one linked Spruce reply conversation surface.
    pub async fn patient_spruce_reply(
        &self,
        patient_workspace_id: &str,
        conversation_id: &str,
    ) -> Result<PatientReplyConversationWire, DbError> {
        self.ensure_ready().await?;
        Ok(self
            .load_stored_reply(patient_workspace_id, conversation_id)?
            .surface)
    }

    /// Saves a physician's private draft edit against `expected_base`.
    pub async fn save_reply_proposal_draft(
        &self,
        patient_workspace_id: &str,
        conversation_id: &str,
        expected_base: ExpectedDraftBaseWire,
        text: &str,
    ) -> Result<PatientReplyConversationWire, DbError> {
        self.ensure_ready().await?;
        let mut stored = self.load_stored_reply(patient_workspace_id, conversation_id)?;
        let card = open_card_mut(&mut stored.surface)?;
        verify_expected_base(&card.displayed_reply, expected_base)?;
        let next_rev = next_draft_revision(&card.displayed_reply);
        card.displayed_reply = DisplayedReplyTextWire::PhysicianDraft {
            revision: next_rev,
            text: text.to_owned(),
            kind: PhysicianDraftKindWire::YourText,
        };
        card.conversation_version += 1;
        self.save_stored_reply(conversation_id, patient_workspace_id, DEMO_NOW, &stored)?;
        Ok(stored.surface)
    }

    /// Applies an inline Coda revision to the physician's reply draft.
    pub async fn request_reply_proposal_revision(
        &self,
        patient_workspace_id: &str,
        conversation_id: &str,
        body: &RequestReplyProposalRevisionBody,
    ) -> Result<PatientReplyConversationWire, DbError> {
        self.ensure_ready().await?;
        let mut stored = self.load_stored_reply(patient_workspace_id, conversation_id)?;
        let card = open_card_mut(&mut stored.surface)?;
        verify_expected_base(&card.displayed_reply, body.expected_base)?;
        let pre_text = match &card.displayed_reply {
            DisplayedReplyTextWire::PhysicianDraft { text, .. } => text.clone(),
            _ => body.current_text.clone(),
        };
        let next_rev = next_draft_revision(&card.displayed_reply);
        let revised = format!(
            "{}\n\n[Revised per instruction: {}]",
            pre_text.trim(),
            body.instruction.trim()
        );
        card.displayed_reply = DisplayedReplyTextWire::PhysicianDraft {
            revision: next_rev,
            text: revised,
            kind: PhysicianDraftKindWire::CodaRevision {
                pre_revision_your_text: pre_text,
            },
        };
        card.revision_run = Some(ReplyRevisionRun {
            run_id: format!("revrun-{}", next_seq()),
            status: "completed".into(),
            resume_target: None,
        });
        card.conversation_version += 1;
        self.save_stored_reply(conversation_id, patient_workspace_id, DEMO_NOW, &stored)?;
        Ok(stored.surface)
    }

    /// Restores the physician's pre-revision wording (`Restore my version`).
    pub async fn restore_my_reply_version(
        &self,
        patient_workspace_id: &str,
        conversation_id: &str,
        expected_draft_revision: u64,
        _idempotency_key: &str,
    ) -> Result<PatientReplyConversationWire, DbError> {
        self.ensure_ready().await?;
        let mut stored = self.load_stored_reply(patient_workspace_id, conversation_id)?;
        let card = open_card_mut(&mut stored.surface)?;
        let pre_text = match &card.displayed_reply {
            DisplayedReplyTextWire::PhysicianDraft {
                revision,
                kind:
                    PhysicianDraftKindWire::CodaRevision {
                        pre_revision_your_text,
                    },
                ..
            } if *revision == expected_draft_revision => pre_revision_your_text.clone(),
            _ => {
                return Err(DbError::Conflict(
                    "Draft changed before Restore my version was applied.".into(),
                ));
            }
        };
        card.displayed_reply = DisplayedReplyTextWire::PhysicianDraft {
            revision: expected_draft_revision + 1,
            text: pre_text,
            kind: PhysicianDraftKindWire::YourText,
        };
        card.revision_run = None;
        card.conversation_version += 1;
        self.save_stored_reply(conversation_id, patient_workspace_id, DEMO_NOW, &stored)?;
        Ok(stored.surface)
    }

    /// Adopts a newer Coda proposal (`Choose update`) over the physician's private draft.
    pub async fn choose_coda_reply_update(
        &self,
        patient_workspace_id: &str,
        conversation_id: &str,
        expected_draft_revision: u64,
        expected_proposal_version: u64,
        _idempotency_key: &str,
    ) -> Result<PatientReplyConversationWire, DbError> {
        self.ensure_ready().await?;
        let mut stored = self.load_stored_reply(patient_workspace_id, conversation_id)?;
        let card = open_card_mut(&mut stored.surface)?;
        if let DisplayedReplyTextWire::PhysicianDraft { revision, .. } = &card.displayed_reply
            && *revision != expected_draft_revision
        {
            return Err(DbError::Conflict("Draft revision changed.".into()));
        }
        let Some(offer) = card.coda_update_offer.take() else {
            return Err(DbError::Conflict(
                "No newer Coda update offer present.".into(),
            ));
        };
        if offer.offered_proposal_version != expected_proposal_version {
            card.coda_update_offer = Some(offer);
            return Err(DbError::Conflict(
                "Offered proposal version changed.".into(),
            ));
        }
        card.proposal_version = offer.offered_proposal_version;
        card.displayed_reply = DisplayedReplyTextWire::CodaProposal {
            proposal_version: offer.offered_proposal_version,
            text: offer.text,
        };
        card.basis_and_limitations = offer.basis_and_limitations;
        card.citations = offer.citations;
        card.conversation_version += 1;
        self.save_stored_reply(conversation_id, patient_workspace_id, DEMO_NOW, &stored)?;
        Ok(stored.surface)
    }

    /// Records `Copy reply` on the displayed version without closing the reply card.
    pub async fn copy_spruce_reply(
        &self,
        patient_workspace_id: &str,
        conversation_id: &str,
        expected_base: ExpectedDraftBaseWire,
        _idempotency_key: &str,
    ) -> Result<CopyReplyResponseWire, DbError> {
        self.ensure_ready().await?;
        let mut stored = self.load_stored_reply(patient_workspace_id, conversation_id)?;
        let physician = self.whoami().display_name;
        let card = open_card_mut(&mut stored.surface)?;
        verify_expected_base(&card.displayed_reply, expected_base)?;
        let copied_text = match &card.displayed_reply {
            DisplayedReplyTextWire::CodaProposal { text, .. }
            | DisplayedReplyTextWire::PhysicianDraft { text, .. } => text.clone(),
            DisplayedReplyTextWire::Withheld { .. } => {
                return Err(DbError::InvalidCommand(
                    "Cannot copy withheld reply.".into(),
                ));
            }
        };
        card.copied_by_me = true;
        card.other_physician_copies.push(CopyAttributionWire {
            copy_id: format!("copy-{}", next_seq()),
            copied_by_physician: physician,
            copied_at: DEMO_NOW.into(),
        });
        card.conversation_version += 1;
        self.save_stored_reply(conversation_id, patient_workspace_id, DEMO_NOW, &stored)?;
        Ok(CopyReplyResponseWire {
            copied_text,
            surface: stored.surface,
        })
    }

    /// Closes an open Spruce reply card as `No reply needed` and clears its Today row.
    pub async fn close_reply_no_reply_needed(
        &self,
        patient_workspace_id: &str,
        conversation_id: &str,
        expected_proposal_version: u64,
        _idempotency_key: &str,
    ) -> Result<PatientReplyConversationWire, DbError> {
        self.ensure_ready().await?;
        let mut stored = self.load_stored_reply(patient_workspace_id, conversation_id)?;
        let card = match &stored.surface {
            PatientReplyConversationWire::OpenCard(card) => card.as_ref().clone(),
            _ => return Err(DbError::Conflict("Reply card is not open.".into())),
        };
        if card.proposal_version != expected_proposal_version {
            return Err(DbError::Conflict("Reply proposal version changed.".into()));
        }
        stored.prior_open_card = Some(card.clone());
        stored.surface = PatientReplyConversationWire::ClosedUndoBanner {
            conversation_id: card.conversation_id,
            patient_workspace_id: card.patient_workspace_id,
            primary_sender_name: card.primary_sender_name,
            primary_sender_role: card.primary_sender_role,
            spruce_app_url: card.spruce_app_url,
            closed_by_physician: self.whoami().display_name,
            pending_move: card.pending_move,
        };
        self.save_stored_reply(conversation_id, patient_workspace_id, DEMO_NOW, &stored)?;
        self.set_spruce_today_done(patient_workspace_id, conversation_id, true)?;
        Ok(stored.surface)
    }

    /// Re-opens a conversation closed via `No reply needed` and restores its Today row.
    pub async fn undo_close_no_reply_needed(
        &self,
        patient_workspace_id: &str,
        conversation_id: &str,
        _idempotency_key: &str,
    ) -> Result<PatientReplyConversationWire, DbError> {
        self.ensure_ready().await?;
        let mut stored = self.load_stored_reply(patient_workspace_id, conversation_id)?;
        let card = stored.prior_open_card.take().ok_or_else(|| {
            DbError::Conflict("No closed reply snapshot available to undo.".into())
        })?;
        stored.surface = PatientReplyConversationWire::OpenCard(Box::new(card));
        self.save_stored_reply(conversation_id, patient_workspace_id, DEMO_NOW, &stored)?;
        self.set_spruce_today_done(patient_workspace_id, conversation_id, false)?;
        Ok(stored.surface)
    }

    /// Generates an open reply proposal card for a `NoOpenCard` Spruce conversation.
    pub async fn ask_coda_to_reply(
        &self,
        patient_workspace_id: &str,
        conversation_id: &str,
        _idempotency_key: &str,
    ) -> Result<PatientReplyConversationWire, DbError> {
        self.ensure_ready().await?;
        let mut stored = self.load_stored_reply(patient_workspace_id, conversation_id)?;
        let patient_display_name = self.load_patient_name(patient_workspace_id)?;
        let records = self.load_patient_records(patient_workspace_id)?;
        let citations = records.iter().take(1).map(build_item_citation).collect();
        let card = match &stored.surface {
            PatientReplyConversationWire::NoOpenCard {
                conversation_id,
                patient_workspace_id,
                primary_sender_name,
                primary_sender_role,
                non_patient_sender_names,
                spruce_app_url,
                patient_messages,
                practice_messages,
                pending_move,
                ..
            } => ReplyCardWire {
                conversation_id: conversation_id.clone(),
                patient_workspace_id: patient_workspace_id.clone(),
                patient_display_name,
                primary_sender_name: primary_sender_name.clone(),
                primary_sender_role: *primary_sender_role,
                non_patient_sender_names: non_patient_sender_names.clone(),
                spruce_app_url: spruce_app_url.clone(),
                reply_id: next_seq(),
                conversation_version: 1,
                proposal_version: 1,
                has_open_concern: false,
                patient_messages: patient_messages.clone(),
                practice_messages: practice_messages.clone(),
                displayed_reply: DisplayedReplyTextWire::CodaProposal {
                    proposal_version: 1,
                    text: format!(
                        "Hi {primary_sender_name}, thank you for sending this update. Please continue your current regimen and reach out if anything changes before our visit."
                    ),
                },
                revision_run: None,
                coda_update_offer: None,
                notices: Vec::new(),
                basis_and_limitations:
                    "Generated on physician request from browser SQLite chart records.".into(),
                citations,
                other_physician_copies: Vec::new(),
                copied_by_me: false,
                review_state: CardReviewStateWire::Idle,
                channel_banner: CardChannelBannerWire::Connected,
                pending_move: pending_move.clone(),
            },
            PatientReplyConversationWire::OpenCard(card) => card.as_ref().clone(),
            PatientReplyConversationWire::ClosedUndoBanner { .. } => {
                return Err(DbError::Conflict(
                    "Undo close before asking Coda to reply.".into(),
                ));
            }
        };
        stored.surface = PatientReplyConversationWire::OpenCard(Box::new(card));
        self.save_stored_reply(conversation_id, patient_workspace_id, DEMO_NOW, &stored)?;
        Ok(stored.surface)
    }

    /// Moves a Spruce conversation to another patient or out of patient care (`Undo` banner active).
    pub async fn move_spruce_conversation(
        &self,
        patient_workspace_id: &str,
        conversation_id: &str,
        destination: MoveDestinationWire,
        _undo_window_seconds: Option<i64>,
        _idempotency_key: &str,
    ) -> Result<PatientReplyConversationWire, DbError> {
        self.ensure_ready().await?;
        let mut stored = self.load_stored_reply(patient_workspace_id, conversation_id)?;
        let physician = self.whoami().display_name;
        let pending_dest = match &destination {
            MoveDestinationWire::AnotherPatient {
                patient_workspace_id: target_ws,
            } => {
                let label = self.load_patient_name(target_ws)?;
                PendingMoveTargetDisplayWire::AnotherPatient {
                    patient_workspace_id: Some(target_ws.clone()),
                    patient_label: label,
                }
            }
            MoveDestinationWire::NotAPatient => PendingMoveTargetDisplayWire::NotAPatient,
        };
        let pending = Some(PendingSpruceMoveWire {
            destination: pending_dest,
            moved_by_physician: physician,
            expires_at: "2026-08-27T19:05:00.000Z".into(),
        });
        let target_ws = match &destination {
            MoveDestinationWire::AnotherPatient {
                patient_workspace_id: dst,
            } => {
                stored.prior_move_source_ws = Some(patient_workspace_id.to_owned());
                dst.as_str()
            }
            MoveDestinationWire::NotAPatient => patient_workspace_id,
        };
        match &mut stored.surface {
            PatientReplyConversationWire::OpenCard(card) => {
                card.pending_move = pending;
                if let MoveDestinationWire::AnotherPatient {
                    patient_workspace_id: dst,
                } = &destination
                {
                    card.patient_workspace_id = dst.clone();
                    card.patient_display_name = self.load_patient_name(dst)?;
                    card.displayed_reply = DisplayedReplyTextWire::Withheld {
                        proposal_version: card.proposal_version,
                        reason: "Re-reviewing against the destination patient's chart.".into(),
                    };
                }
            }
            PatientReplyConversationWire::ClosedUndoBanner { pending_move, .. }
            | PatientReplyConversationWire::NoOpenCard { pending_move, .. } => {
                *pending_move = pending;
            }
        }
        self.save_stored_reply(conversation_id, target_ws, DEMO_NOW, &stored)?;
        Ok(stored.surface)
    }

    /// Cancels a pending `Wrong patient?` move during the `Undo` window.
    pub async fn undo_spruce_conversation_move(
        &self,
        patient_workspace_id: &str,
        conversation_id: &str,
        _idempotency_key: &str,
    ) -> Result<PatientReplyConversationWire, DbError> {
        self.ensure_ready().await?;
        let mut stored = self.load_stored_reply_any_ws(conversation_id)?;
        let restore_ws = stored
            .prior_move_source_ws
            .take()
            .unwrap_or_else(|| patient_workspace_id.to_owned());
        let restore_name = self.load_patient_name(&restore_ws)?;
        match &mut stored.surface {
            PatientReplyConversationWire::OpenCard(card) => {
                card.pending_move = None;
                card.patient_workspace_id = restore_ws.clone();
                card.patient_display_name = restore_name;
                if let DisplayedReplyTextWire::Withheld {
                    proposal_version, ..
                } = &card.displayed_reply
                {
                    card.displayed_reply = DisplayedReplyTextWire::CodaProposal {
                        proposal_version: *proposal_version,
                        text:
                            "Thank you for reaching out — our care team has reviewed your message."
                                .into(),
                    };
                }
            }
            PatientReplyConversationWire::ClosedUndoBanner { pending_move, .. }
            | PatientReplyConversationWire::NoOpenCard { pending_move, .. } => {
                *pending_move = None;
            }
        }
        self.save_stored_reply(conversation_id, &restore_ws, DEMO_NOW, &stored)?;
        Ok(stored.surface)
    }

    /// Marks an open reply card `Done` while Spruce is disconnected.
    pub async fn mark_disconnected_reply_done(
        &self,
        patient_workspace_id: &str,
        conversation_id: &str,
        body: &DisconnectedReplyDoneBody,
    ) -> Result<PatientReplyConversationWire, DbError> {
        self.ensure_ready().await?;
        let mut stored = self.load_stored_reply(patient_workspace_id, conversation_id)?;
        let card = match &stored.surface {
            PatientReplyConversationWire::OpenCard(card) => card.as_ref().clone(),
            _ => return Err(DbError::Conflict("Reply card is not open.".into())),
        };
        if card.reply_id != body.expected_reply_id {
            return Err(DbError::Conflict("Reply identifier changed.".into()));
        }
        stored.surface = PatientReplyConversationWire::NoOpenCard {
            conversation_id: card.conversation_id,
            patient_workspace_id: card.patient_workspace_id,
            primary_sender_name: card.primary_sender_name,
            primary_sender_role: card.primary_sender_role,
            non_patient_sender_names: card.non_patient_sender_names,
            spruce_app_url: card.spruce_app_url,
            patient_messages: card.patient_messages,
            practice_messages: card.practice_messages,
            review_state: CardReviewStateWire::Idle,
            can_ask_coda_to_reply: true,
            pending_move: card.pending_move,
        };
        self.save_stored_reply(conversation_id, patient_workspace_id, DEMO_NOW, &stored)?;
        self.set_spruce_today_done(patient_workspace_id, conversation_id, true)?;
        Ok(stored.surface)
    }

    fn spruce_channel_banner(&self) -> Result<SpruceChannelBannerWire, DbError> {
        let rows = self.driver.select_where(
            PracticeConnections::Table,
            &[PracticeConnections::PayloadJson],
            &[(
                PracticeConnections::SourceId,
                SupportedSourceWire::Spruce.slug(),
            )],
        )?;
        let Some(row) = rows.first() else {
            return Ok(SpruceChannelBannerWire::ConnectedReading {
                last_read_at: Some(DEMO_NOW.into()),
            });
        };
        let src: PracticeSourceWire = serde_json::from_str(&row.get_str("payload_json")?)?;
        Ok(match src.standing {
            SourceStandingWire::Disconnected | SourceStandingWire::CodaAssistedRequested { .. } => {
                SpruceChannelBannerWire::Disconnected {
                    reason: "Spruce channel disconnected in demo Connections.".into(),
                    disconnected_at: DEMO_NOW.into(),
                }
            }
            SourceStandingWire::AccessLost {
                last_successful_read_at,
                reason,
                lost_at,
                ..
            } => SpruceChannelBannerWire::ReconnectRequired {
                last_read_at: last_successful_read_at,
                reason,
                lost_at,
            },
            SourceStandingWire::ReadFailed {
                last_successful_read_at,
                reason,
                failed_at,
                ..
            } => SpruceChannelBannerWire::ReadFailed {
                last_read_at: last_successful_read_at,
                reason,
                failed_at,
            },
            SourceStandingWire::Connected { connected_at, .. } => {
                SpruceChannelBannerWire::ConnectedReading {
                    last_read_at: Some(connected_at),
                }
            }
            SourceStandingWire::RecordsImported {
                last_imported_at, ..
            } => SpruceChannelBannerWire::ConnectedReading {
                last_read_at: Some(last_imported_at),
            },
            SourceStandingWire::UpToDate { last_read_at, .. } => {
                SpruceChannelBannerWire::ConnectedReading {
                    last_read_at: Some(last_read_at),
                }
            }
            SourceStandingWire::RollbackHalted {
                last_successful_read_at,
                ..
            } => SpruceChannelBannerWire::ConnectedReading {
                last_read_at: last_successful_read_at,
            },
        })
    }

    fn load_match_item(&self, message_id: &str) -> Result<StoredMatchItem, DbError> {
        let rows = self.driver.select_where(
            SpruceMatchQueue::Table,
            &[SpruceMatchQueue::PayloadJson],
            &[(SpruceMatchQueue::MessageId, message_id)],
        )?;
        let row = rows
            .first()
            .ok_or_else(|| not_found("spruce matching message", message_id))?;
        Ok(serde_json::from_str(&row.get_str("payload_json")?)?)
    }

    fn save_match_item(
        &self,
        message_id: &str,
        conversation_id: &str,
        bucket: &str,
        waiting_since: &str,
        item: &StoredMatchItem,
    ) -> Result<(), DbError> {
        let insert = Query::insert()
            .into_table(SpruceMatchQueue::Table)
            .columns(SpruceMatchQueue::COLS.iter().copied())
            .values_panic([
                message_id.into(),
                conversation_id.into(),
                bucket.into(),
                waiting_since.into(),
                serde_json::to_string(item)?.as_str().into(),
            ])
            .on_conflict(
                OnConflict::column(SpruceMatchQueue::MessageId)
                    .update_columns([
                        SpruceMatchQueue::Bucket,
                        SpruceMatchQueue::WaitingSince,
                        SpruceMatchQueue::PayloadJson,
                    ])
                    .to_owned(),
            )
            .to_owned();
        self.driver.insert(&insert)
    }

    fn load_stored_reply(
        &self,
        ws_id: &str,
        conversation_id: &str,
    ) -> Result<StoredSpruceReply, DbError> {
        let rows = self.driver.select_where(
            SpruceReplies::Table,
            &[SpruceReplies::PayloadJson],
            &[
                (SpruceReplies::WorkspaceId, ws_id),
                (SpruceReplies::ConversationId, conversation_id),
            ],
        )?;
        let row = rows
            .first()
            .ok_or_else(|| not_found("spruce conversation", conversation_id))?;
        Ok(serde_json::from_str(&row.get_str("payload_json")?)?)
    }

    fn load_stored_reply_any_ws(
        &self,
        conversation_id: &str,
    ) -> Result<StoredSpruceReply, DbError> {
        let rows = self.driver.select_where(
            SpruceReplies::Table,
            &[SpruceReplies::PayloadJson],
            &[(SpruceReplies::ConversationId, conversation_id)],
        )?;
        let row = rows
            .first()
            .ok_or_else(|| not_found("spruce conversation", conversation_id))?;
        Ok(serde_json::from_str(&row.get_str("payload_json")?)?)
    }

    pub(crate) fn save_stored_reply(
        &self,
        conversation_id: &str,
        ws_id: &str,
        waiting_since: &str,
        reply: &StoredSpruceReply,
    ) -> Result<(), DbError> {
        let insert = Query::insert()
            .into_table(SpruceReplies::Table)
            .columns(SpruceReplies::COLS.iter().copied())
            .values_panic([
                conversation_id.into(),
                ws_id.into(),
                waiting_since.into(),
                serde_json::to_string(reply)?.as_str().into(),
            ])
            .on_conflict(
                OnConflict::column(SpruceReplies::ConversationId)
                    .update_columns([
                        SpruceReplies::WorkspaceId,
                        SpruceReplies::WaitingSince,
                        SpruceReplies::PayloadJson,
                    ])
                    .to_owned(),
            )
            .to_owned();
        self.driver.insert(&insert)
    }

    fn set_spruce_today_done(
        &self,
        ws_id: &str,
        conversation_id: &str,
        done: bool,
    ) -> Result<(), DbError> {
        let item_id = format!("spruce-reply:{conversation_id}");
        self.driver.update_where(
            TodayItems::Table,
            TodayItems::Done,
            i64::from(done).into(),
            &[
                (TodayItems::WorkspaceId, ws_id),
                (TodayItems::ItemId, item_id.as_str()),
            ],
        )
    }
}

fn open_card_mut(
    surface: &mut PatientReplyConversationWire,
) -> Result<&mut ReplyCardWire, DbError> {
    match surface {
        PatientReplyConversationWire::OpenCard(card) => Ok(card.as_mut()),
        _ => Err(DbError::Conflict(
            "Reply card is not currently open.".into(),
        )),
    }
}

fn verify_expected_base(
    displayed: &DisplayedReplyTextWire,
    expected: ExpectedDraftBaseWire,
) -> Result<(), DbError> {
    match (displayed, expected) {
        (
            DisplayedReplyTextWire::CodaProposal {
                proposal_version, ..
            },
            ExpectedDraftBaseWire::ProposalVersion { version },
        ) if *proposal_version == version => Ok(()),
        (
            DisplayedReplyTextWire::PhysicianDraft { revision, .. },
            ExpectedDraftBaseWire::PhysicianDraftRevision { revision: exp },
        ) if *revision == exp => Ok(()),
        _ => Err(DbError::Conflict(
            "Reply draft changed since it was loaded; refresh to view the latest draft.".into(),
        )),
    }
}

fn next_draft_revision(displayed: &DisplayedReplyTextWire) -> u64 {
    match displayed {
        DisplayedReplyTextWire::PhysicianDraft { revision, .. } => revision + 1,
        DisplayedReplyTextWire::CodaProposal { .. } | DisplayedReplyTextWire::Withheld { .. } => 1,
    }
}
