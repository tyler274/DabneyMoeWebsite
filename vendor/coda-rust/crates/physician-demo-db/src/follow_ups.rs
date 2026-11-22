//! SQLite-backed patient `Follow-ups` listing, creation, editing, and lifecycle controls.

use crate::driver::DbError;
use crate::schema::{FollowUps, Patients};
use crate::store::{
    DEMO_NOW, DEMO_ZONE, DemoStore, build_answer_segments_and_sources_for_follow_up, next_seq,
    not_found,
};
use physician_wire::follow_ups::{
    CannotRestartResolutionWire, FollowUpMutationResponseWire, FollowUpOriginWire,
    FollowUpStandingWire, FollowUpWire, PatientFollowUpsResponseWire, UpdateFollowUpBody,
};
use physician_wire::{ChatSnapshot, FollowUp, PatientWorkspaceResponse, SnapshotFeedback};
use sea_query::{Expr, ExprTrait, OnConflict, Order, Query};

impl DemoStore {
    /// Lists follow-ups for `patient_workspace_id`, open items first.
    pub async fn patient_follow_ups(
        &self,
        patient_workspace_id: &str,
    ) -> Result<PatientFollowUpsResponseWire, DbError> {
        self.ensure_ready().await?;
        let _ = self.load_patient_name(patient_workspace_id)?;
        let q = Query::select()
            .column(FollowUps::PayloadJson)
            .from(FollowUps::Table)
            .and_where(Expr::col(FollowUps::WorkspaceId).eq(patient_workspace_id))
            .order_by(FollowUps::UpdatedAt, Order::Desc)
            .to_owned();
        let mut follow_ups: Vec<FollowUpWire> = self
            .driver
            .select(&q)?
            .iter()
            .map(|r| Ok(serde_json::from_str(&r.get_str("payload_json")?)?))
            .collect::<Result<Vec<_>, DbError>>()?;
        follow_ups.sort_by_key(|f| {
            (
                !f.standing.is_open(),
                std::cmp::Reverse(f.updated_at.clone()),
            )
        });
        Ok(PatientFollowUpsResponseWire {
            patient_workspace_id: patient_workspace_id.to_owned(),
            durable: true,
            follow_ups,
        })
    }

    /// Creates a new physician-authored follow-up and syncs workspace summary state.
    pub async fn create_follow_up(
        &self,
        patient_workspace_id: &str,
        summary: &str,
        due_date: &str,
        _idempotency_key: &str,
    ) -> Result<FollowUpMutationResponseWire, DbError> {
        self.ensure_ready().await?;
        let _ = self.load_patient_name(patient_workspace_id)?;
        let summary_clean = non_empty_trimmed(summary, "Follow-up summary cannot be empty.")?;
        let (due_at, due_date_input) = normalize_due_date(due_date)?;
        let follow_up = FollowUpWire {
            follow_up_id: format!("fu-{}-{}", patient_workspace_id, next_seq()),
            patient_workspace_id: patient_workspace_id.to_owned(),
            version: 1,
            summary: summary_clean,
            standing: FollowUpStandingWire::Scheduled {
                due_at,
                due_date_input,
            },
            origin: FollowUpOriginWire::PhysicianCreated {
                physician_id: self.whoami().physician_id,
            },
            latest_note: None,
            updated_at: DEMO_NOW.into(),
        };
        self.save_follow_up(&follow_up).await?;
        Ok(FollowUpMutationResponseWire {
            follow_up,
            review_started: false,
            discuss_chat_id: None,
        })
    }

    /// Updates a follow-up's summary, due date, or note.
    pub async fn update_follow_up(
        &self,
        patient_workspace_id: &str,
        follow_up_id: &str,
        body: &UpdateFollowUpBody,
    ) -> Result<FollowUpMutationResponseWire, DbError> {
        self.ensure_ready().await?;
        let mut fu = self.load_follow_up(patient_workspace_id, follow_up_id)?;
        if let Some(summary) = &body.summary {
            fu.summary = non_empty_trimmed(summary, "Follow-up summary cannot be empty.")?;
        }
        if let Some(due_date) = &body.due_date {
            let (due_at, due_date_input) = normalize_due_date(due_date)?;
            fu.standing = FollowUpStandingWire::Scheduled {
                due_at,
                due_date_input,
            };
        }
        if let Some(note) = body
            .note
            .as_deref()
            .map(str::trim)
            .filter(|n| !n.is_empty())
        {
            fu.latest_note = Some(note.to_owned());
        }
        fu.version += 1;
        fu.updated_at = DEMO_NOW.into();
        self.save_follow_up(&fu).await?;
        Ok(FollowUpMutationResponseWire {
            follow_up: fu,
            review_started: false,
            discuss_chat_id: None,
        })
    }

    /// Marks a follow-up `DueNow` for immediate chart review.
    pub async fn review_follow_up_now(
        &self,
        patient_workspace_id: &str,
        follow_up_id: &str,
        _idempotency_key: &str,
    ) -> Result<FollowUpMutationResponseWire, DbError> {
        self.ensure_ready().await?;
        let mut fu = self.load_follow_up(patient_workspace_id, follow_up_id)?;
        let due_date_input = current_due_date_input(&fu.standing);
        fu.standing = FollowUpStandingWire::DueNow {
            requested_at: DEMO_NOW.into(),
            due_date_input,
        };
        fu.version += 1;
        fu.updated_at = DEMO_NOW.into();
        self.save_follow_up(&fu).await?;
        Ok(FollowUpMutationResponseWire {
            follow_up: fu,
            review_started: true,
            discuss_chat_id: None,
        })
    }

    /// Pauses (`Held`) an active follow-up.
    pub async fn stop_follow_up(
        &self,
        patient_workspace_id: &str,
        follow_up_id: &str,
        reason: Option<String>,
        _idempotency_key: &str,
    ) -> Result<FollowUpMutationResponseWire, DbError> {
        self.ensure_ready().await?;
        let mut fu = self.load_follow_up(patient_workspace_id, follow_up_id)?;
        let clean_reason = reason
            .map(|r| r.trim().to_owned())
            .filter(|r| !r.is_empty());
        let due_date_input = current_due_date_input(&fu.standing);
        if let Some(ref r) = clean_reason {
            fu.latest_note = Some(r.clone());
        }
        fu.standing = FollowUpStandingWire::Held {
            paused_at: DEMO_NOW.into(),
            reason: clean_reason,
            due_date_input,
        };
        fu.version += 1;
        fu.updated_at = DEMO_NOW.into();
        self.save_follow_up(&fu).await?;
        Ok(FollowUpMutationResponseWire {
            follow_up: fu,
            review_started: false,
            discuss_chat_id: None,
        })
    }

    /// Pauses an active follow-up and opens a real SQLite discussion chat seeded with context.
    pub async fn stop_and_discuss_follow_up(
        &self,
        patient_workspace_id: &str,
        follow_up_id: &str,
        reason: Option<String>,
        _physician_time_zone: &str,
        idempotency_key: &str,
    ) -> Result<FollowUpMutationResponseWire, DbError> {
        let res = self
            .stop_follow_up(
                patient_workspace_id,
                follow_up_id,
                reason.clone(),
                idempotency_key,
            )
            .await?;
        let chat_id = format!("chat-fu-{}-{}", follow_up_id, next_seq());
        let records = self.load_patient_records(patient_workspace_id)?;
        let prompt = match reason.as_deref().map(str::trim).filter(|r| !r.is_empty()) {
            Some(note) => format!("Paused follow-up \"{}\": {note}", res.follow_up.summary),
            None => format!(
                "Paused follow-up \"{}\" to discuss next steps.",
                res.follow_up.summary
            ),
        };
        let (exchange, sources) = build_answer_segments_and_sources_for_follow_up(
            &res.follow_up.summary,
            &prompt,
            &records,
        );
        let snap = ChatSnapshot {
            transcript: vec![exchange],
            feedback: SnapshotFeedback::Loaded,
            sources,
            queued: Vec::new(),
            archived: false,
        };
        self.upsert_chat_snapshot(
            patient_workspace_id,
            &chat_id,
            &format!("Discuss: {}", res.follow_up.summary),
            DEMO_NOW,
            &snap,
        )?;
        Ok(FollowUpMutationResponseWire {
            follow_up: res.follow_up,
            review_started: false,
            discuss_chat_id: Some(chat_id),
        })
    }

    /// Resumes a `Held` follow-up back to `Scheduled`.
    pub async fn resume_follow_up(
        &self,
        patient_workspace_id: &str,
        follow_up_id: &str,
        _idempotency_key: &str,
    ) -> Result<FollowUpMutationResponseWire, DbError> {
        self.ensure_ready().await?;
        let mut fu = self.load_follow_up(patient_workspace_id, follow_up_id)?;
        let due_date_input = current_due_date_input(&fu.standing);
        let (due_at, due_date_input) = normalize_due_date(&due_date_input)?;
        fu.standing = FollowUpStandingWire::Scheduled {
            due_at,
            due_date_input,
        };
        fu.version += 1;
        fu.updated_at = DEMO_NOW.into();
        self.save_follow_up(&fu).await?;
        Ok(FollowUpMutationResponseWire {
            follow_up: fu,
            review_started: false,
            discuss_chat_id: None,
        })
    }

    /// Completes a follow-up with a clinical resolution note.
    pub async fn complete_follow_up(
        &self,
        patient_workspace_id: &str,
        follow_up_id: &str,
        resolution: &str,
        _idempotency_key: &str,
    ) -> Result<FollowUpMutationResponseWire, DbError> {
        self.ensure_ready().await?;
        let mut fu = self.load_follow_up(patient_workspace_id, follow_up_id)?;
        let note = non_empty_trimmed(resolution, "Completion note cannot be empty.")?;
        fu.latest_note = Some(note.clone());
        fu.standing = FollowUpStandingWire::Completed {
            completed_at: DEMO_NOW.into(),
            resolution: note,
        };
        fu.version += 1;
        fu.updated_at = DEMO_NOW.into();
        self.save_follow_up(&fu).await?;
        Ok(FollowUpMutationResponseWire {
            follow_up: fu,
            review_started: false,
            discuss_chat_id: None,
        })
    }

    /// Cancels a follow-up with a recorded reason.
    pub async fn cancel_follow_up(
        &self,
        patient_workspace_id: &str,
        follow_up_id: &str,
        reason: &str,
        _idempotency_key: &str,
    ) -> Result<FollowUpMutationResponseWire, DbError> {
        self.ensure_ready().await?;
        let mut fu = self.load_follow_up(patient_workspace_id, follow_up_id)?;
        let note = non_empty_trimmed(reason, "Cancellation reason cannot be empty.")?;
        fu.latest_note = Some(note.clone());
        fu.standing = FollowUpStandingWire::Cancelled {
            cancelled_at: DEMO_NOW.into(),
            reason: note,
        };
        fu.version += 1;
        fu.updated_at = DEMO_NOW.into();
        self.save_follow_up(&fu).await?;
        Ok(FollowUpMutationResponseWire {
            follow_up: fu,
            review_started: false,
            discuss_chat_id: None,
        })
    }

    /// Resolves a `CannotRestart` follow-up (`Complete`, `NewDate`, or `Cancel`).
    pub async fn resolve_cannot_restart_follow_up(
        &self,
        patient_workspace_id: &str,
        follow_up_id: &str,
        resolution: CannotRestartResolutionWire,
        idempotency_key: &str,
    ) -> Result<FollowUpMutationResponseWire, DbError> {
        match resolution {
            CannotRestartResolutionWire::Complete { resolution } => {
                self.complete_follow_up(
                    patient_workspace_id,
                    follow_up_id,
                    &resolution,
                    idempotency_key,
                )
                .await
            }
            CannotRestartResolutionWire::NewDate { due_date, note } => {
                self.update_follow_up(
                    patient_workspace_id,
                    follow_up_id,
                    &UpdateFollowUpBody {
                        summary: None,
                        due_date: Some(due_date),
                        note,
                        idempotency_key: idempotency_key.to_owned(),
                    },
                )
                .await
            }
            CannotRestartResolutionWire::Cancel { reason } => {
                self.cancel_follow_up(patient_workspace_id, follow_up_id, &reason, idempotency_key)
                    .await
            }
        }
    }

    fn load_follow_up(
        &self,
        patient_workspace_id: &str,
        follow_up_id: &str,
    ) -> Result<FollowUpWire, DbError> {
        let rows = self.driver.select_where(
            FollowUps::Table,
            &[FollowUps::PayloadJson],
            &[
                (FollowUps::WorkspaceId, patient_workspace_id),
                (FollowUps::FollowUpId, follow_up_id),
            ],
        )?;
        let row = rows
            .first()
            .ok_or_else(|| not_found("follow_up_not_found", follow_up_id))?;
        Ok(serde_json::from_str(&row.get_str("payload_json")?)?)
    }

    async fn save_follow_up(&self, fu: &FollowUpWire) -> Result<(), DbError> {
        {
            let insert = Query::insert()
                .into_table(FollowUps::Table)
                .columns(FollowUps::COLS.iter().copied())
                .values_panic([
                    fu.follow_up_id.as_str().into(),
                    fu.patient_workspace_id.as_str().into(),
                    fu.standing.badge_label().into(),
                    fu.updated_at.as_str().into(),
                    serde_json::to_string(fu)?.as_str().into(),
                ])
                .on_conflict(
                    OnConflict::column(FollowUps::FollowUpId)
                        .update_columns([
                            FollowUps::Standing,
                            FollowUps::UpdatedAt,
                            FollowUps::PayloadJson,
                        ])
                        .to_owned(),
                )
                .to_owned();
            self.driver.insert(&insert)?;
        }
        self.sync_workspace_active_follow_ups(&fu.patient_workspace_id)
            .await
    }

    async fn sync_workspace_active_follow_ups(&self, ws_id: &str) -> Result<(), DbError> {
        let list = self.patient_follow_ups(ws_id).await?;
        let active: Vec<FollowUp> = list
            .follow_ups
            .into_iter()
            .filter(|f| f.standing.is_open())
            .map(|f| FollowUp {
                follow_up_id: f.follow_up_id,
                description: Some(f.summary),
            })
            .collect();
        let rows = self.driver.select_where(
            Patients::Table,
            &[Patients::WorkspaceJson],
            &[(Patients::WorkspaceId, ws_id)],
        )?;
        if let Some(row) = rows.first() {
            let mut ws: PatientWorkspaceResponse =
                serde_json::from_str(&row.get_str("workspace_json")?)?;
            ws.active_follow_ups = active;
            self.driver.update_where(
                Patients::Table,
                Patients::WorkspaceJson,
                serde_json::to_string(&ws)?.into(),
                &[(Patients::WorkspaceId, ws_id)],
            )?;
        }
        Ok(())
    }
}

fn non_empty_trimmed(raw: &str, err: &'static str) -> Result<String, DbError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(DbError::InvalidCommand(err.into()));
    }
    Ok(trimmed.to_owned())
}

fn normalize_due_date(raw: &str) -> Result<(String, String), DbError> {
    let trimmed = non_empty_trimmed(raw, "Due date cannot be empty.")?;
    let date_part = trimmed.get(..10).unwrap_or(&trimmed);
    let _ = DEMO_ZONE;
    if trimmed.contains('T') {
        Ok((trimmed.clone(), date_part.to_owned()))
    } else {
        Ok((format!("{date_part}T13:00:00Z"), date_part.to_owned()))
    }
}

fn current_due_date_input(standing: &FollowUpStandingWire) -> String {
    match standing {
        FollowUpStandingWire::Scheduled { due_date_input, .. }
        | FollowUpStandingWire::DueNow { due_date_input, .. }
        | FollowUpStandingWire::Held { due_date_input, .. }
        | FollowUpStandingWire::CannotRestart { due_date_input, .. } => due_date_input.clone(),
        FollowUpStandingWire::Completed { .. } | FollowUpStandingWire::Cancelled { .. } => {
            "2026-09-03".into()
        }
    }
}
