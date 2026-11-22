//! SQLite-backed Patient Reviews, Past work, Cited source reader, and Patient Memory.

use crate::driver::DbError;
use crate::schema::{Chats, PatientMemory, PatientReviews};
use crate::store::{DEMO_DATE, DEMO_NOW, DemoStore, build_cited_source, next_seq, not_found};
use physician_wire::reviews::{
    PatientReviewStateWire, PatientReviewWire, PatientReviewsResponse, ReviewControlActionWire,
    ReviewControlRefusalReason, ReviewControlRequest, ReviewControlResponse, ReviewHoldReasonWire,
    ReviewVersion,
};
use physician_wire::{
    AnswerState, ChatSnapshot, CitedSourceResponse, HistoryWorkOutcome, MemoryDocumentWire,
    MemoryEditTarget, PastWorkItem, PastWorkResponse, PastWorkTarget, PatientMemoryResponse,
    SavePhysicianMemoryEditRequest, SavePhysicianMemoryEditResponse,
};
use sea_query::{Expr, ExprTrait, OnConflict, Order, Query};

impl DemoStore {
    /// Lists saved patient reviews for `workspace_id`, newest updated first.
    pub async fn patient_reviews(
        &self,
        workspace_id: &str,
    ) -> Result<PatientReviewsResponse, DbError> {
        self.ensure_ready().await?;
        let _ = self.load_patient_name(workspace_id)?;
        let q = Query::select()
            .column(PatientReviews::PayloadJson)
            .from(PatientReviews::Table)
            .and_where(Expr::col(PatientReviews::WorkspaceId).eq(workspace_id))
            .order_by(PatientReviews::UpdatedAt, Order::Desc)
            .to_owned();
        let reviews = self
            .driver
            .select(&q)?
            .iter()
            .map(|r| Ok(serde_json::from_str(&r.get_str("payload_json")?)?))
            .collect::<Result<Vec<PatientReviewWire>, DbError>>()?;
        Ok(PatientReviewsResponse { reviews })
    }

    /// Reads a single saved patient review by identifier.
    pub async fn patient_review(
        &self,
        workspace_id: &str,
        review_id: &str,
    ) -> Result<PatientReviewWire, DbError> {
        self.ensure_ready().await?;
        let rows = self.driver.select_where(
            PatientReviews::Table,
            &[PatientReviews::PayloadJson],
            &[
                (PatientReviews::WorkspaceId, workspace_id),
                (PatientReviews::ReviewId, review_id),
            ],
        )?;
        let row = rows
            .first()
            .ok_or_else(|| not_found("patient review", review_id))?;
        Ok(serde_json::from_str(&row.get_str("payload_json")?)?)
    }

    /// Applies an optimistic-versioned control action (`Stop`, `Resume`, `TryAgain`) to a patient review.
    pub async fn control_patient_review(
        &self,
        workspace_id: &str,
        review_id: &str,
        request: &ReviewControlRequest,
    ) -> Result<ReviewControlResponse, DbError> {
        self.ensure_ready().await?;
        let mut review = self.patient_review(workspace_id, review_id).await?;
        if review.version != request.version {
            return Ok(ReviewControlResponse::Refused {
                reason: ReviewControlRefusalReason::StaleVersion,
                message: "This review moved to a newer version before your command arrived.".into(),
                review: Some(Box::new(review)),
            });
        }
        if review.state.available_control() != Some(request.action) {
            return Ok(ReviewControlResponse::Refused {
                reason: ReviewControlRefusalReason::InvalidState,
                message: format!(
                    "Cannot `{}` a review in `{}` state.",
                    request.action.button_label(),
                    review.state.badge_label()
                ),
                review: Some(Box::new(review)),
            });
        }
        review.state = match request.action {
            ReviewControlActionWire::Stop => PatientReviewStateWire::Held {
                reason: ReviewHoldReasonWire::Stopped,
                held_at: DEMO_NOW.into(),
                partial_text: Some("Reviewed interval notes and active medications.".into()),
            },
            ReviewControlActionWire::Resume | ReviewControlActionWire::TryAgain => {
                PatientReviewStateWire::Running {
                    attempt_id: format!("att-{}", next_seq()),
                    started_at: DEMO_NOW.into(),
                }
            }
        };
        let next_num = review
            .version
            .as_str()
            .trim_start_matches("rv-")
            .parse::<u64>()
            .unwrap_or(1)
            + 1;
        review.version = ReviewVersion(format!("rv-{next_num}"));
        review.updated_at = DEMO_NOW.into();
        let insert = Query::insert()
            .into_table(PatientReviews::Table)
            .columns(PatientReviews::COLS.iter().copied())
            .values_panic([
                review.review_id.as_str().into(),
                workspace_id.into(),
                review.updated_at.as_str().into(),
                serde_json::to_string(&review)?.as_str().into(),
            ])
            .on_conflict(
                OnConflict::column(PatientReviews::ReviewId)
                    .update_columns([PatientReviews::UpdatedAt, PatientReviews::PayloadJson])
                    .to_owned(),
            )
            .to_owned();
        self.driver.insert(&insert)?;
        Ok(ReviewControlResponse::Accepted {
            review: Box::new(review),
        })
    }

    /// Reads saved conversations and past work for `workspace_id`.
    pub async fn past_work(&self, workspace_id: &str) -> Result<PastWorkResponse, DbError> {
        self.ensure_ready().await?;
        let _ = self.load_patient_name(workspace_id)?;
        let q = Query::select()
            .columns([
                Chats::ChatId,
                Chats::Title,
                Chats::CreatedAt,
                Chats::SnapshotJson,
            ])
            .from(Chats::Table)
            .and_where(Expr::col(Chats::WorkspaceId).eq(workspace_id))
            .order_by(Chats::CreatedAt, Order::Desc)
            .to_owned();
        let mut items = Vec::new();
        for row in self.driver.select(&q)? {
            let chat_id = row.get_str("chat_id")?;
            let title = row.get_str("title")?;
            let created_at = row.get_str("created_at")?;
            let snap: ChatSnapshot = serde_json::from_str(&row.get_str("snapshot_json")?)?;
            let summary = snap
                .transcript
                .iter()
                .rev()
                .find_map(|ex| match &ex.answer {
                    AnswerState::Finished { text, .. } => {
                        Some(text.lines().next().unwrap_or("").trim().to_owned())
                    }
                    _ => None,
                })
                .unwrap_or_else(|| "Saved conversation".into());
            items.push(PastWorkItem {
                id: format!("pw-{chat_id}"),
                target: PastWorkTarget::Conversation { chat_id },
                title,
                summary,
                outcome: HistoryWorkOutcome::Answered,
                updated_at: created_at,
            });
        }
        Ok(PastWorkResponse { items })
    }

    /// Resolves one cited source record on demand for an open conversation.
    pub async fn cited_source(
        &self,
        workspace_id: &str,
        chat_id: &str,
        source_key: &str,
    ) -> Result<CitedSourceResponse, DbError> {
        self.ensure_ready().await?;
        let snap = self.chat_snapshot(workspace_id, chat_id).await?.snapshot;
        if let Some(source) = snap.sources.get(source_key) {
            return Ok(CitedSourceResponse {
                source: source.clone(),
            });
        }
        if let Ok(rec) = self.source_record(workspace_id, source_key).await {
            return Ok(CitedSourceResponse {
                source: build_cited_source(source_key, 1, &rec),
            });
        }
        Err(not_found("cited source", source_key))
    }

    /// Reads `summary.md` and `direction.md` for `workspace_id`.
    pub async fn patient_memory(
        &self,
        workspace_id: &str,
    ) -> Result<PatientMemoryResponse, DbError> {
        self.ensure_ready().await?;
        let display_name = self.load_patient_name(workspace_id)?;
        let rows = self.driver.select_where(
            PatientMemory::Table,
            &[PatientMemory::PayloadJson],
            &[(PatientMemory::WorkspaceId, workspace_id)],
        )?;
        if let Some(row) = rows.first() {
            return Ok(serde_json::from_str(&row.get_str("payload_json")?)?);
        }
        Ok(PatientMemoryResponse {
            patient_workspace_id: workspace_id.to_owned(),
            durable: true,
            summary: Some(MemoryDocumentWire {
                version: 1,
                saved_at: DEMO_DATE.into(),
                markdown: format!(
                    "# {display_name}\n\nLongitudinal chart synthesis prepared from demo EHR records."
                ),
            }),
            direction: Some(MemoryDocumentWire {
                version: 1,
                saved_at: DEMO_DATE.into(),
                markdown: "Continue current surveillance and monitor interval labs/vitals.".into(),
            }),
        })
    }

    /// Appends a non-empty physician note to `direction.md` for `workspace_id` without
    /// overwriting prior notes.
    pub async fn save_patient_memory(
        &self,
        workspace_id: &str,
        note: &str,
    ) -> Result<SavePhysicianMemoryEditResponse, DbError> {
        let trimmed = note.trim();
        if trimmed.is_empty() {
            return Err(DbError::InvalidCommand(
                "Memory note text cannot be empty.".into(),
            ));
        }
        let current = self.patient_memory(workspace_id).await?;
        let prior = current
            .direction
            .as_ref()
            .map(|doc| doc.markdown.trim())
            .filter(|text| !text.is_empty())
            .unwrap_or("");
        let updated = if prior.is_empty() {
            trimmed.to_owned()
        } else if prior.contains(trimmed) {
            prior.to_owned()
        } else {
            format!("{prior}\n\n- {trimmed}")
        };
        self.save_patient_memory_edit(
            workspace_id,
            &SavePhysicianMemoryEditRequest {
                target: MemoryEditTarget::Direction,
                text: updated,
                idempotency_key: format!("mem-note-{}", next_seq()),
            },
        )
        .await
    }

    /// Saves a physician's edit to `summary.md` or `direction.md` and appends a log note.
    pub async fn save_patient_memory_edit(
        &self,
        workspace_id: &str,
        request: &SavePhysicianMemoryEditRequest,
    ) -> Result<SavePhysicianMemoryEditResponse, DbError> {
        self.ensure_ready().await?;
        let text = request.text.trim();
        if text.is_empty() {
            return Err(DbError::InvalidCommand(
                "Memory note text cannot be empty.".into(),
            ));
        }
        let mut current = self.patient_memory(workspace_id).await?;
        let rows = self.driver.select_where(
            PatientMemory::Table,
            &[PatientMemory::NoteSeq],
            &[(PatientMemory::WorkspaceId, workspace_id)],
        )?;
        let next_note = rows
            .first()
            .and_then(|r| r.get_i64("note_seq").ok())
            .unwrap_or(0)
            + 1;
        let doc_slot = match request.target {
            MemoryEditTarget::Summary => &mut current.summary,
            MemoryEditTarget::Direction => &mut current.direction,
        };
        let next_ver = doc_slot.as_ref().map_or(1, |d| d.version + 1);
        *doc_slot = Some(MemoryDocumentWire {
            version: next_ver,
            saved_at: DEMO_DATE.into(),
            markdown: text.to_owned(),
        });
        current.durable = true;
        let insert = Query::insert()
            .into_table(PatientMemory::Table)
            .columns(PatientMemory::COLS.iter().copied())
            .values_panic([
                workspace_id.into(),
                next_note.into(),
                serde_json::to_string(&current)?.as_str().into(),
            ])
            .on_conflict(
                OnConflict::column(PatientMemory::WorkspaceId)
                    .update_columns([PatientMemory::NoteSeq, PatientMemory::PayloadJson])
                    .to_owned(),
            )
            .to_owned();
        self.driver.insert(&insert)?;
        Ok(SavePhysicianMemoryEditResponse {
            note_number: format!("N{next_note}"),
            log_path: format!("log/{DEMO_DATE}.md"),
            saved_at: DEMO_NOW.into(),
        })
    }
}
