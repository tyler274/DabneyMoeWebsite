//! Typed `sea-query` repository backing all physician UI reads and interactive demo commands.

use crate::driver::{DbError, SqlRow, SqliteDriver};
use crate::schema::{
    Chats, Drafts, Patients, PracticeVisits, SourceRecords, SpruceMatchQueue, TodayItems,
    UpdatesFeed, schema_ddl,
};
use physician_wire::{
    AnswerState, AttemptId, ChatSnapshot, ChatSnapshotResponse, CitationOrigin, CitationTargetKind,
    CitedFieldWire, CitedSourceWire, CitedTextPartWire, DateRole, Exchange, ExchangeSteps, Found,
    HealthCapabilities, HealthResponse, HistoryWorkOutcome, MemoryEditTarget, MessageKey,
    MessageSegment, PatientSearchResponse, PatientSearchRow, PatientWorkspaceResponse,
    PatientWorkspaceSummary, PatientWorkspacesResponse, PhysicianDraft, PhysicianMessage,
    PracticeAccountSummary, PracticeDayResponse, PracticeSourceState, RecentConversation,
    RecentConversationsResponse, RecordAbout, RecordDate, RecordGroup, RecordRead, ReplyDraftWire,
    ResponseFeedback, ResponseId, SaveFeedback, SavePhysicianMemoryEditRequest, SavedAt,
    SearchPage, SnapshotFeedback, SourceVersion, Step, StepState, TodayConnection,
    TodayListResponse, TodayRow, UpdatesResponse, WhoamiResponse, WorkItemId, WorkResumeTarget,
    WorkspaceChat, WorkspacePlanItem,
    today_updates::{EarlierDayUpdatesWire, ItemCitationWire, SeenStandingWire, UpdateItemWire},
};
use sea_query::{Cond, Expr, ExprTrait, OnConflict, Order, Query, SelectStatement};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};

pub(crate) const DEMO_DATE: &str = "2026-08-27";
pub(crate) const DEMO_NOW: &str = "2026-08-27T19:00:00.000Z";
pub(crate) const DEMO_ZONE: &str = "America/New_York";
pub(crate) const DEMO_BASE_UNIX: i64 = 1_787_857_200;

/// Synthetic calendar date used by the seeded demo database (`YYYY-MM-DD`).
#[cfg(not(target_arch = "wasm32"))]
pub const DEFAULT_DEMO_DATE: &str = DEMO_DATE;
/// Synthetic ISO-8601 current timestamp used when saving demo chat updates.
#[cfg(not(target_arch = "wasm32"))]
pub const DEFAULT_DEMO_NOW: &str = DEMO_NOW;
/// Synthetic Unix timestamp baseline used by demo transcript messages.
#[cfg(not(target_arch = "wasm32"))]
pub const DEFAULT_DEMO_BASE_UNIX: i64 = DEMO_BASE_UNIX;
static SEQUENCE: AtomicU64 = AtomicU64::new(1);

pub(crate) fn next_seq() -> u64 {
    SEQUENCE.fetch_add(1, Ordering::Relaxed)
}

/// Returns the next process-unique demo sequence number.
#[cfg(not(target_arch = "wasm32"))]
pub fn next_demo_seq() -> u64 {
    next_seq()
}

/// Location of an existing chat work item loaded from [`DemoStore`].
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Clone)]
pub struct ChatWorkLocation {
    /// Conversation identifier holding the work item.
    pub chat_id: String,
    /// Conversation title.
    pub title: String,
    /// Conversation creation timestamp string.
    pub created_at: String,
    /// Full decoded conversation snapshot.
    pub snapshot: ChatSnapshot,
    /// Index of the target exchange inside `snapshot.transcript`.
    pub exchange_idx: usize,
}

/// One source record retrieved from SQLite for chart inspection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DemoSourceRecord {
    /// Record identifier within the patient workspace.
    pub id: String,
    /// Patient workspace owning the record.
    pub workspace_id: String,
    /// Shared record envelope group label.
    pub group: String,
    /// Clinical record title.
    pub title: String,
    /// Recorded date string.
    pub recorded_at: String,
    /// Full clinical record body text.
    pub body: String,
    /// Source label shown in inspection provenance.
    pub source_label: String,
    /// Structured readable sections in reading order.
    pub readable_parts: Vec<CitedTextPartWire>,
    /// Structured clinical fields (labs, vitals, medications, conditions).
    pub fields: Vec<CitedFieldWire>,
    /// Clinical verification status (e.g. `"final"`).
    pub record_status: String,
    /// Pretty-printed raw fixture JSON for Technical details disclosure.
    pub raw_content: Option<String>,
}

/// Result of attempting an optimistic-concurrency composer draft save.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DraftSaveOutcome {
    /// Draft saved at the returned revision.
    Saved(PhysicianDraft),
    /// A newer revision already existed; nothing was overwritten.
    Conflict(PhysicianDraft),
}

struct ChatWorkMatch {
    chat_id: String,
    title: String,
    created_at: String,
    snap: ChatSnapshot,
    exchange_idx: usize,
}

/// SQLite-backed store powering the interactive browser demo and native host tests.
#[derive(Debug, Clone)]
pub struct DemoStore {
    pub(crate) driver: SqliteDriver,
}

impl DemoStore {
    /// Wraps a configured SQLite driver.
    pub fn new(driver: SqliteDriver) -> Self {
        Self { driver }
    }

    /// Opens an isolated in-memory [`DemoStore`] populated from snapshot `bytes`.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, DbError> {
        Ok(Self {
            driver: SqliteDriver::from_bytes(bytes)?,
        })
    }

    /// Initializes the SQLite tables on a newly opened native database.
    pub fn init_schema(&self) -> Result<(), DbError> {
        self.driver.exec_ddl(&schema_ddl())
    }

    /// Exposes the underlying driver for seeding helpers.
    pub fn driver(&self) -> &SqliteDriver {
        &self.driver
    }

    /// Ensures the browser SQLite engine has loaded the seed snapshot.
    pub async fn ensure_ready(&self) -> Result<(), DbError> {
        self.driver.ensure_ready().await
    }

    /// Restores the demo database to its pristine seeded snapshot.
    pub async fn reset(&self) -> Result<(), DbError> {
        self.driver.reset().await
    }

    fn map_select<T>(
        &self,
        q: &SelectStatement,
        f: impl Fn(&SqlRow) -> Result<T, DbError>,
    ) -> Result<Vec<T>, DbError> {
        self.driver.select(q)?.iter().map(f).collect()
    }

    /// Returns static demo health metadata.
    pub fn health(&self) -> HealthResponse {
        HealthResponse {
            status: "ok".into(),
            retrying: Vec::new(),
            held_by_newer_release: Vec::new(),
            capabilities: Some(HealthCapabilities {
                connection_accounts: false,
                practice_sources: true,
            }),
        }
    }

    /// Returns the synthetic signed-in demo physician.
    pub fn whoami(&self) -> WhoamiResponse {
        let acct = PracticeAccountSummary {
            physician_id: "phys-demo-hannah".into(),
            display_name: "Dr. Hannah Alvarez".into(),
            email: "hannah@codahealth.example".into(),
            admin: true,
        };
        WhoamiResponse {
            physician_id: acct.physician_id.clone(),
            practice_id: "prac-coda-demo".into(),
            email: acct.email.clone(),
            display_name: acct.display_name.clone(),
            admin: true,
            practice_accounts: vec![acct],
        }
    }

    /// Queries active items for the Today list.
    pub async fn today(&self) -> Result<TodayListResponse, DbError> {
        self.today_filtered(None).await
    }

    /// Queries active Today items for one patient workspace.
    pub async fn today_for(&self, workspace_id: &str) -> Result<TodayListResponse, DbError> {
        self.today_filtered(Some(workspace_id)).await
    }

    async fn today_filtered(&self, ws: Option<&str>) -> Result<TodayListResponse, DbError> {
        self.ensure_ready().await?;
        let mut q = Query::select();
        q.columns([TodayItems::RowJson, TodayItems::Seen])
            .from(TodayItems::Table)
            .and_where(Expr::col(TodayItems::Done).eq(0));
        if let Some(id) = ws {
            q.and_where(Expr::col(TodayItems::WorkspaceId).eq(id));
        }
        q.order_by(TodayItems::Urgency, Order::Asc)
            .order_by(TodayItems::WaitingSince, Order::Desc);
        let decoded: Vec<(TodayRow, bool)> = self.map_select(&q, |r| {
            let row: TodayRow = serde_json::from_str(&r.get_str("row_json")?)?;
            let seen_flag = r.get_i64("seen")? != 0 || row.seen;
            Ok((row, seen_flag))
        })?;
        let mut rows = Vec::with_capacity(decoded.len());
        let mut seen = Vec::new();
        for (mut row, is_seen) in decoded {
            if is_seen {
                row.seen = true;
                if row.standing.is_none() || matches!(row.standing, Some(SeenStandingWire::Unseen))
                {
                    row.standing = Some(SeenStandingWire::Seen {
                        seen_at: DEMO_NOW.into(),
                    });
                }
                seen.push(row.item_id.clone());
            }
            rows.push(row);
        }
        let unmatched_q = Query::select()
            .column(SpruceMatchQueue::MessageId)
            .from(SpruceMatchQueue::Table)
            .and_where(Expr::col(SpruceMatchQueue::Bucket).eq("unmatched"))
            .to_owned();
        let messages_to_match_count = self.driver.select(&unmatched_q)?.len();

        let quiet_q = Query::select()
            .column(UpdatesFeed::Id)
            .from(UpdatesFeed::Table)
            .and_where(Expr::col(UpdatesFeed::Bucket).eq("quiet"))
            .to_owned();
        let also_reviewed = self.driver.select(&quiet_q)?.len();

        let connections = ["spruce", "gmail"]
            .into_iter()
            .map(|c| TodayConnection {
                condition: "connected".into(),
                channel: Some(c.into()),
                source_id: None,
                source_label: None,
                reason: None,
                last_successful_read_at: None,
                recovery: None,
            })
            .collect();
        Ok(TodayListResponse {
            date: DEMO_DATE.into(),
            time_zone: Some(DEMO_ZONE.into()),
            rows,
            seen,
            as_of: 1,
            connections,
            incomplete: Some(false),
            also_reviewed,
            practice_outages: Vec::new(),
            failed_parts: Vec::new(),
            messages_to_match_count,
            may_be_missing_someone: false,
        })
    }

    /// Lists all patient workspaces in the demo database.
    pub async fn workspaces(&self) -> Result<PatientWorkspacesResponse, DbError> {
        self.ensure_ready().await?;
        let q = Query::select()
            .columns([Patients::WorkspaceId, Patients::DisplayName])
            .from(Patients::Table)
            .order_by(Patients::DisplayName, Order::Asc)
            .to_owned();
        let patients = self.map_select(&q, |r| {
            Ok(PatientWorkspaceSummary {
                patient_workspace_id: r.get_str("workspace_id")?,
                display_name: r.get_str("display_name")?,
                cursor: 1,
            })
        })?;
        Ok(PatientWorkspacesResponse {
            patients,
            incomplete: Some(false),
            may_be_missing_someone: false,
        })
    }

    /// Queries the scheduled clinic visits for the Schedule rail.
    pub async fn practice_day(&self) -> Result<PracticeDayResponse, DbError> {
        self.ensure_ready().await?;
        let q = Query::select()
            .column(PracticeVisits::PayloadJson)
            .from(PracticeVisits::Table)
            .order_by(PracticeVisits::StartsAt, Order::Asc)
            .to_owned();
        let visits = self.map_select(&q, |r| {
            Ok(serde_json::from_str(&r.get_str("payload_json")?)?)
        })?;
        Ok(PracticeDayResponse {
            current_time: DEMO_NOW.into(),
            time_zone: DEMO_ZONE.into(),
            source_state: PracticeSourceState {
                state: "usable".into(),
                source_label: Some("Demo Clinic Calendar".into()),
                snapshot_as_of: Some(DEMO_NOW.into()),
                synchronized_at: Some(DEMO_NOW.into()),
                freshness: Some("current".into()),
                last_error: None,
            },
            visits,
            may_be_missing_someone: false,
        })
    }

    /// Queries recent conversations across seeded patients.
    pub async fn recent_conversations(&self) -> Result<RecentConversationsResponse, DbError> {
        self.ensure_ready().await?;
        let join = Expr::col((Chats::Table, Chats::WorkspaceId))
            .equals((Patients::Table, Patients::WorkspaceId));
        let q = Query::select()
            .column((Chats::Table, Chats::ChatId))
            .column((Chats::Table, Chats::WorkspaceId))
            .column((Chats::Table, Chats::Title))
            .column((Chats::Table, Chats::CreatedAt))
            .column((Chats::Table, Chats::SnapshotJson))
            .column((Patients::Table, Patients::DisplayName))
            .from(Chats::Table)
            .inner_join(Patients::Table, join)
            .order_by((Chats::Table, Chats::CreatedAt), Order::Desc)
            .limit(12)
            .to_owned();
        let conversations = self.map_select(&q, |r| {
            let snap: ChatSnapshot = serde_json::from_str(&r.get_str("snapshot_json")?)?;
            let (summary, outcome) = summarize_snapshot(&snap);
            Ok(RecentConversation {
                chat_id: Some(r.get_str("chat_id")?),
                patient_workspace_id: Some(r.get_str("workspace_id")?),
                title: Some(r.get_str("title")?),
                summary,
                outcome,
                patient_display_name: Some(r.get_str("display_name")?),
                updated_at: Some(r.get_str("created_at")?),
                archived: snap.archived,
            })
        })?;
        Ok(RecentConversationsResponse {
            conversations,
            may_be_missing_someone: false,
        })
    }

    /// Queries the Updates feed.
    pub async fn updates(&self) -> Result<UpdatesResponse, DbError> {
        self.ensure_ready().await?;
        let q = Query::select()
            .columns([UpdatesFeed::Bucket, UpdatesFeed::PayloadJson])
            .from(UpdatesFeed::Table)
            .order_by(UpdatesFeed::CreatedAt, Order::Desc)
            .to_owned();
        let entries: Vec<(String, serde_json::Value)> = self.map_select(&q, |r| {
            Ok((
                r.get_str("bucket")?,
                serde_json::from_str(&r.get_str("payload_json")?)?,
            ))
        })?;
        let mut today_items = Vec::new();
        let mut reviewed_nothing_for_you = Vec::new();
        let mut yesterday_items = Vec::new();
        let mut legacy_rows = Vec::new();
        for (bucket, payload) in entries {
            if let Ok(item) = serde_json::from_value::<UpdateItemWire>(payload.clone()) {
                match bucket.as_str() {
                    "quiet" => reviewed_nothing_for_you.push(item),
                    "yesterday" => yesterday_items.push(item),
                    _ => today_items.push(item),
                }
            } else {
                legacy_rows.push(payload);
            }
        }
        let earlier_days = if yesterday_items.is_empty() {
            Vec::new()
        } else {
            vec![EarlierDayUpdatesWire {
                date: "2026-08-26".into(),
                label: "Yesterday".into(),
                items: yesterday_items,
            }]
        };
        Ok(UpdatesResponse {
            active_runs: Vec::new(),
            rows: legacy_rows,
            today_items,
            reviewed_nothing_for_you,
            earlier_days,
            failed_parts: Vec::new(),
            may_be_missing_someone: false,
        })
    }

    /// Searches patients by name, MRN, or birth date using SQL `LIKE`.
    pub async fn search(&self, query_text: &str) -> Result<PatientSearchResponse, DbError> {
        self.ensure_ready().await?;
        let trimmed = query_text.trim();
        let pat = format!("%{trimmed}%");
        let mut stmt = Query::select();
        stmt.columns([
            Patients::WorkspaceId,
            Patients::DisplayName,
            Patients::BirthDate,
            Patients::Mrn,
        ])
        .from(Patients::Table);
        if !trimmed.is_empty() {
            stmt.cond_where(
                Cond::any()
                    .add(Expr::col(Patients::DisplayName).like(&pat))
                    .add(Expr::col(Patients::Mrn).like(&pat))
                    .add(Expr::col(Patients::BirthDate).like(&pat)),
            );
        }
        stmt.order_by(Patients::DisplayName, Order::Asc).limit(25);
        let patients = self.map_select(&stmt, |r| {
            Ok(PatientSearchRow {
                patient_workspace_id: r.get_str("workspace_id")?,
                patient_display_name: r.get_str("display_name")?,
                birth_date: Some(r.get_str("birth_date")?),
                mrn: Some(r.get_str("mrn")?),
                duet_linked: true,
                sharing_consented: true,
                connection_status: physician_wire::ConnectionStatusWire::Connected,
                cadence: Some("daily_when_changed".into()),
            })
        })?;
        Ok(PatientSearchResponse {
            patients,
            next_after: None,
            may_be_missing_someone: false,
            match_mode: Default::default(),
        })
    }

    /// Reads the complete workspace projection for `workspace_id`.
    pub async fn workspace(&self, workspace_id: &str) -> Result<PatientWorkspaceResponse, DbError> {
        self.ensure_ready().await?;
        let rows = self.driver.select_where(
            Patients::Table,
            &[Patients::WorkspaceJson],
            &[(Patients::WorkspaceId, workspace_id)],
        )?;
        let row = rows
            .first()
            .ok_or_else(|| not_found("patient workspace", workspace_id))?;
        let mut ws: PatientWorkspaceResponse =
            serde_json::from_str(&row.get_str("workspace_json")?)?;
        ws.chats = self.load_workspace_chats(workspace_id)?;
        Ok(ws)
    }

    async fn mutate_workspace<T>(
        &self,
        payload: &serde_json::Value,
        f: impl FnOnce(&mut PatientWorkspaceResponse) -> Result<T, DbError>,
    ) -> Result<(String, T), DbError> {
        let ws_id = required_str(payload, "patient_workspace_id")?;
        let mut ws = self.workspace(&ws_id).await?;
        let out = f(&mut ws)?;
        let json = serde_json::to_string(&ws)?;
        self.driver.update_where(
            Patients::Table,
            Patients::WorkspaceJson,
            json.into(),
            &[(Patients::WorkspaceId, ws_id.as_str())],
        )?;
        Ok((ws_id, out))
    }

    /// Queries one conversation transcript snapshot.
    pub async fn chat_snapshot(
        &self,
        ws_id: &str,
        chat_id: &str,
    ) -> Result<ChatSnapshotResponse, DbError> {
        self.ensure_ready().await?;
        let rows = self.driver.select_where(
            Chats::Table,
            &[Chats::SnapshotJson],
            &[(Chats::WorkspaceId, ws_id), (Chats::ChatId, chat_id)],
        )?;
        let snapshot = match rows.first() {
            Some(row) => serde_json::from_str(&row.get_str("snapshot_json")?)?,
            None => ChatSnapshot {
                sources: Default::default(),
                transcript: Vec::new(),
                feedback: SnapshotFeedback::Loaded,
                queued: Vec::new(),
                archived: false,
            },
        };
        Ok(ChatSnapshotResponse { snapshot })
    }

    /// Reads the saved composer draft for `(workspace_id, chat_id)`.
    pub async fn read_draft(&self, ws_id: &str, chat_id: &str) -> Result<PhysicianDraft, DbError> {
        self.ensure_ready().await?;
        let rows = self.driver.select_where(
            Drafts::Table,
            &[Drafts::Revision, Drafts::Text],
            &[(Drafts::WorkspaceId, ws_id), (Drafts::ChatId, chat_id)],
        )?;
        let Some(row) = rows.first() else {
            return Ok(PhysicianDraft {
                text: String::new(),
                revision: 0,
            });
        };
        Ok(PhysicianDraft {
            text: row.get_str("text")?,
            revision: row.get_i64("revision")?,
        })
    }

    /// Saves a composer draft if `expected_revision` matches the stored revision.
    pub async fn save_draft(
        &self,
        ws_id: &str,
        chat_id: &str,
        expected_revision: i64,
        text: &str,
    ) -> Result<DraftSaveOutcome, DbError> {
        self.ensure_ready().await?;
        let current = self.read_draft(ws_id, chat_id).await?;
        if current.revision != expected_revision {
            return Ok(DraftSaveOutcome::Conflict(current));
        }
        let next = expected_revision + 1;
        let conflict = OnConflict::columns([Drafts::WorkspaceId, Drafts::ChatId])
            .update_columns([Drafts::Revision, Drafts::Text])
            .to_owned();
        let insert = Query::insert()
            .into_table(Drafts::Table)
            .columns(Drafts::COLS.iter().copied())
            .values_panic([ws_id.into(), chat_id.into(), next.into(), text.into()])
            .on_conflict(conflict)
            .to_owned();
        self.driver.insert(&insert)?;
        Ok(DraftSaveOutcome::Saved(PhysicianDraft {
            text: text.into(),
            revision: next,
        }))
    }

    /// Persists physician feedback on a response inside a chat snapshot.
    pub async fn save_feedback(
        &self,
        ws_id: &str,
        chat_id: &str,
        response: &ResponseId,
        save: &SaveFeedback,
    ) -> Result<ResponseFeedback, DbError> {
        let mut snap = self.chat_snapshot(ws_id, chat_id).await?.snapshot;
        let mut saved = None;
        for ex in &mut snap.transcript {
            if let AnswerState::Finished {
                response_id,
                feedback,
                ..
            } = &mut ex.answer
                && response_id == response
            {
                let next = ResponseFeedback {
                    revision: feedback.as_ref().map_or(1, |p| p.revision + 1),
                    rating: save.rating,
                    details: save.details.clone(),
                };
                *feedback = Some(next.clone());
                saved = Some(next);
            }
        }
        let out = saved.ok_or_else(|| not_found("response", response.as_str()))?;
        self.driver.update_where(
            Chats::Table,
            Chats::SnapshotJson,
            serde_json::to_string(&snap)?.into(),
            &[(Chats::WorkspaceId, ws_id), (Chats::ChatId, chat_id)],
        )?;
        Ok(out)
    }

    /// Reads one reply draft with its current revision.
    pub async fn read_reply_draft(
        &self,
        ws_id: &str,
        reply_id: &str,
    ) -> Result<ReplyDraftWire, DbError> {
        let ws = self.workspace(ws_id).await?;
        let reply = ws
            .replies
            .iter()
            .find(|r| r.reply_work_id == reply_id)
            .ok_or_else(|| not_found("reply draft", reply_id))?;
        Ok(ReplyDraftWire {
            revision: reply.draft_revision.unwrap_or(1),
            text: reply.draft_text.clone(),
            restore_text: None,
            revision_run: None,
        })
    }

    /// Returns one full chart source record for citation inspection.
    pub async fn source_record(
        &self,
        ws_id: &str,
        record_id: &str,
    ) -> Result<DemoSourceRecord, DbError> {
        self.ensure_ready().await?;
        let rows = self.driver.select_where(
            SourceRecords::Table,
            SourceRecords::COLS,
            &[
                (SourceRecords::WorkspaceId, ws_id),
                (SourceRecords::Id, record_id),
            ],
        )?;
        let row = rows
            .first()
            .ok_or_else(|| not_found("source record", record_id))?;
        decode_source_record(row)
    }

    /// Executes an interactive physician command against the browser SQLite store.
    pub async fn command(&self, body: &serde_json::Value) -> Result<serde_json::Value, DbError> {
        self.ensure_ready().await?;
        let kind = required_str(body, "type")?;
        let payload = body.get("payload").unwrap_or(body);
        match kind.as_str() {
            "message.submit" => self.handle_message_submit(body, payload).await,
            "message.queue_edit" => self.handle_queue_edit(payload).await,
            "message.withdraw" => self.handle_queue_withdraw(payload).await,
            "message.queue_send_now" => self.handle_queue_send_now(body, payload).await,
            "conversation.archive" => self.handle_conversation_archive(payload, true).await,
            "conversation.restore" => self.handle_conversation_archive(payload, false).await,
            "discussion.open" => self.handle_discussion_open(payload).await,
            "item.open" => self.handle_item_open(payload),
            "plan.decide_recommendation" => self.handle_decide_recommendation(payload).await,
            "reply.close" => self.handle_reply_close(payload).await,
            "reply.revise" => self.handle_reply_revise(payload).await,
            "concern.done" | "work.done" => self.handle_item_done(payload),
            "work.stop" => self.handle_work_stop(payload).await,
            "work.resume" => self.handle_work_resume(payload).await,
            _ => Err(DbError::InvalidCommand(format!(
                "unsupported demo command `{kind}`"
            ))),
        }
    }

    async fn handle_message_submit(
        &self,
        envelope: &serde_json::Value,
        payload: &serde_json::Value,
    ) -> Result<serde_json::Value, DbError> {
        let ws_id = required_str(payload, "patient_workspace_id")?;
        let chat_id = required_str(payload, "chat_id")?;
        let question = required_str(payload, "text")?;
        let key = envelope
            .get("idempotency_key")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| format!("msg-{}", next_seq()));
        let mut snap = self.chat_snapshot(&ws_id, &chat_id).await?.snapshot;
        let is_new = snap.transcript.is_empty();
        let seq = next_seq();
        let ts = DEMO_BASE_UNIX + i64::try_from(seq).unwrap_or(1);
        if payload.get("delivery").and_then(serde_json::Value::as_str) == Some("send_next") {
            snap.queued.push(physician_wire::QueuedMessageWire {
                id: MessageKey(key),
                text: question.clone(),
                saved_at: SavedAt(ts),
            });
            let title = self
                .existing_chat_title(&ws_id, &chat_id)?
                .unwrap_or_else(|| truncate_title(&question));
            self.upsert_chat_snapshot(&ws_id, &chat_id, &title, DEMO_NOW, &snap)?;
            return Ok(serde_json::json!({ "ok": true, "chat_id": chat_id }));
        }
        self.append_chart_exchange(&ws_id, &chat_id, &mut snap, is_new, (key, question, ts))
            .await
    }

    async fn append_chart_exchange(
        &self,
        ws_id: &str,
        chat_id: &str,
        snap: &mut ChatSnapshot,
        is_new: bool,
        msg: (String, String, i64),
    ) -> Result<serde_json::Value, DbError> {
        let (key, question, ts) = msg;
        let terms = extract_search_terms(&question);
        let matched = self.search_patient_records(ws_id, &terms)?;
        let saved_memory_note = self
            .record_memory_note_if_requested(ws_id, &question)
            .await?;
        let seq = next_seq();
        let reads = chart_reads_from(&matched);
        let (answer_text, segments, basis, cited_map) =
            synthesize_chart_answer_with_citations(&question, &matched, "c");
        for (k, v) in cited_map {
            snap.sources.insert(k, v);
        }
        let mut ex = build_exchange_with_segments(
            "demo",
            &seq.to_string(),
            (key, question.clone(), ts),
            &terms.join(" "),
            &reads,
            false,
            (answer_text, segments, basis),
        );
        append_follow_up_and_memory_steps(&mut ex.steps, !is_new, saved_memory_note);
        snap.transcript.push(ex);
        let title = if is_new {
            truncate_title(&question)
        } else {
            self.existing_chat_title(ws_id, chat_id)?
                .unwrap_or_else(|| truncate_title(&question))
        };
        self.upsert_chat_snapshot(ws_id, chat_id, &title, DEMO_NOW, snap)?;
        Ok(serde_json::json!({ "ok": true, "chat_id": chat_id }))
    }

    async fn record_memory_note_if_requested(
        &self,
        ws_id: &str,
        question: &str,
    ) -> Result<bool, DbError> {
        if !question_requests_memory_note(question) {
            return Ok(false);
        }
        self.save_memory_direction_note(ws_id, question).await
    }

    /// Appends a physician direction note into `PatientMemory` for `ws_id` if non-empty.
    pub async fn save_memory_direction_note(
        &self,
        ws_id: &str,
        note: &str,
    ) -> Result<bool, DbError> {
        let note = note.trim();
        if note.is_empty() {
            return Ok(false);
        }
        let current = self.patient_memory(ws_id).await?;
        let prior = current
            .direction
            .as_ref()
            .map(|doc| doc.markdown.trim())
            .filter(|text| !text.is_empty())
            .unwrap_or("");
        let updated = if prior.is_empty() {
            note.to_owned()
        } else if prior.contains(note) {
            prior.to_owned()
        } else {
            format!("{prior}\n\n- {note}")
        };
        self.save_patient_memory_edit(
            ws_id,
            &SavePhysicianMemoryEditRequest {
                target: MemoryEditTarget::Direction,
                text: updated,
                idempotency_key: format!("mem-note-{}", next_seq()),
            },
        )
        .await?;
        Ok(true)
    }

    async fn handle_work_stop(
        &self,
        payload: &serde_json::Value,
    ) -> Result<serde_json::Value, DbError> {
        let ws_id = required_str(payload, "patient_workspace_id")?;
        let chat_id = required_str(payload, "chat_id")?;
        let requested_work = payload
            .get("work_item_id")
            .and_then(serde_json::Value::as_str)
            .filter(|s| !s.trim().is_empty());
        let Some((title, created_at, mut snap)) = self.load_chat_row(&ws_id, &chat_id)? else {
            return Ok(command_rejected("Coda couldn’t stop that work."));
        };
        let target_idx = match requested_work {
            Some(work_id) => snap
                .transcript
                .iter()
                .position(|ex| ex.work_item_id.0 == work_id),
            None => snap
                .transcript
                .iter()
                .rposition(|ex| matches!(ex.answer, AnswerState::Working { .. }))
                .or_else(|| snap.transcript.len().checked_sub(1)),
        };
        let Some(idx) = target_idx else {
            return Ok(command_rejected("Coda couldn’t stop that work."));
        };
        let ex = &mut snap.transcript[idx];
        if !matches!(
            ex.answer,
            AnswerState::Working { .. } | AnswerState::Finished { .. }
        ) {
            return Ok(command_rejected("Coda couldn’t stop that work."));
        }
        mark_running_steps_stopped(&mut ex.steps);
        let run_id = format!("run-stopped-{}", next_seq());
        let work_item_id = ex.work_item_id.clone();
        ex.answer = AnswerState::Stopped {
            resume: Some(WorkResumeTarget {
                work_item_id: work_item_id.clone(),
                expected_run_id: run_id.clone(),
            }),
        };
        let returned_messages: Vec<String> = snap.queued.drain(..).map(|q| q.text).collect();
        self.upsert_chat_snapshot(&ws_id, &chat_id, &title, &created_at, &snap)?;
        Ok(serde_json::json!({
            "outcome": "accepted",
            "patient_workspace_id": ws_id,
            "chat_id": chat_id,
            "work_item_id": work_item_id,
            "expected_run_id": run_id,
            "returned_messages": returned_messages,
        }))
    }

    async fn handle_work_resume(
        &self,
        payload: &serde_json::Value,
    ) -> Result<serde_json::Value, DbError> {
        let ws_id = required_str(payload, "patient_workspace_id")?;
        let work_item_id = required_str(payload, "work_item_id")?;
        let expected_run_id = payload
            .get("expected_run_id")
            .and_then(serde_json::Value::as_str);
        let requested_chat = payload
            .get("chat_id")
            .and_then(serde_json::Value::as_str)
            .filter(|s| !s.trim().is_empty());
        let Some(ChatWorkMatch {
            chat_id,
            title,
            created_at,
            mut snap,
            exchange_idx: idx,
        }) = self.find_chat_work_exchange(&ws_id, requested_chat, &work_item_id)?
        else {
            return Ok(command_rejected("Coda couldn’t resume that work."));
        };
        let resume_target = match &snap.transcript[idx].answer {
            AnswerState::Stopped { resume } => resume.as_ref(),
            AnswerState::CutOff { try_again } | AnswerState::CouldntFinish { try_again } => {
                try_again.as_ref()
            }
            AnswerState::Working { .. } | AnswerState::Finished { .. } => None,
        };
        let Some(target) = resume_target else {
            return Ok(command_rejected("Coda couldn’t resume that work."));
        };
        if let Some(expected) = expected_run_id
            && target.expected_run_id != expected
        {
            return Ok(command_rejected("Coda couldn’t resume that work."));
        }
        let question = snap.transcript[idx]
            .messages
            .iter()
            .map(|m| m.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let terms = extract_search_terms(&question);
        let matched = self.search_patient_records(&ws_id, &terms)?;
        let saved_memory_note = self
            .record_memory_note_if_requested(&ws_id, &question)
            .await?;
        let reads = chart_reads_from(&matched);
        let (answer_text, segments, basis, cited_map) =
            synthesize_chart_answer_with_citations(&question, &matched, "res");
        for (k, v) in cited_map {
            snap.sources.insert(k, v);
        }
        let seq = next_seq();
        let finished_at = SavedAt(
            snap.transcript[idx]
                .messages
                .last()
                .map_or(DEMO_BASE_UNIX, |m| m.saved_at.0)
                + 2,
        );
        let rebuilt = build_exchange_with_segments(
            "res",
            &seq.to_string(),
            (String::new(), question, finished_at.0 - 2),
            &terms.join(" "),
            &reads,
            false,
            (answer_text, segments, basis),
        );
        let ex = &mut snap.transcript[idx];
        ex.steps = rebuilt.steps;
        append_follow_up_and_memory_steps(&mut ex.steps, idx > 0, saved_memory_note);
        ex.answer = rebuilt.answer;
        self.upsert_chat_snapshot(&ws_id, &chat_id, &title, &created_at, &snap)?;
        Ok(serde_json::json!({
            "outcome": "accepted",
            "patient_workspace_id": ws_id,
            "chat_id": chat_id,
            "work_item_id": work_item_id,
        }))
    }

    fn load_chat_row(
        &self,
        ws_id: &str,
        chat_id: &str,
    ) -> Result<Option<(String, String, ChatSnapshot)>, DbError> {
        let rows = self.driver.select_where(
            Chats::Table,
            &[Chats::Title, Chats::CreatedAt, Chats::SnapshotJson],
            &[(Chats::WorkspaceId, ws_id), (Chats::ChatId, chat_id)],
        )?;
        let Some(row) = rows.first() else {
            return Ok(None);
        };
        Ok(Some((
            row.get_str("title")?,
            row.get_str("created_at")?,
            serde_json::from_str(&row.get_str("snapshot_json")?)?,
        )))
    }

    fn find_chat_work_exchange(
        &self,
        ws_id: &str,
        chat_id: Option<&str>,
        work_item_id: &str,
    ) -> Result<Option<ChatWorkMatch>, DbError> {
        if let Some(cid) = chat_id {
            let Some((title, created_at, snap)) = self.load_chat_row(ws_id, cid)? else {
                return Ok(None);
            };
            let Some(exchange_idx) = snap
                .transcript
                .iter()
                .position(|ex| ex.work_item_id.0 == work_item_id)
            else {
                return Ok(None);
            };
            return Ok(Some(ChatWorkMatch {
                chat_id: cid.to_owned(),
                title,
                created_at,
                snap,
                exchange_idx,
            }));
        }
        let q = Query::select()
            .columns([
                Chats::ChatId,
                Chats::Title,
                Chats::CreatedAt,
                Chats::SnapshotJson,
            ])
            .from(Chats::Table)
            .and_where(Expr::col(Chats::WorkspaceId).eq(ws_id))
            .order_by(Chats::CreatedAt, Order::Desc)
            .to_owned();
        for row in self.driver.select(&q)? {
            let cid = row.get_str("chat_id")?;
            let title = row.get_str("title")?;
            let created_at = row.get_str("created_at")?;
            let snap: ChatSnapshot = serde_json::from_str(&row.get_str("snapshot_json")?)?;
            if let Some(exchange_idx) = snap
                .transcript
                .iter()
                .position(|ex| ex.work_item_id.0 == work_item_id)
            {
                return Ok(Some(ChatWorkMatch {
                    chat_id: cid,
                    title,
                    created_at,
                    snap,
                    exchange_idx,
                }));
            }
        }
        Ok(None)
    }

    async fn handle_queue_edit(
        &self,
        payload: &serde_json::Value,
    ) -> Result<serde_json::Value, DbError> {
        let ws_id = required_str(payload, "patient_workspace_id")?;
        let chat_id = required_str(payload, "chat_id")?;
        let msg_id = required_str(payload, "message_id")?;
        let text = required_str(payload, "text")?;
        let mut snap = self.chat_snapshot(&ws_id, &chat_id).await?.snapshot;
        let Some(item) = snap.queued.iter_mut().find(|q| q.id.0 == msg_id) else {
            return Ok(serde_json::json!({
                "outcome": "rejected",
                "message": "Coda couldn’t update that queued message.",
            }));
        };
        item.text = text;
        let title = self
            .existing_chat_title(&ws_id, &chat_id)?
            .unwrap_or_else(|| "Conversation".into());
        self.upsert_chat_snapshot(&ws_id, &chat_id, &title, DEMO_NOW, &snap)?;
        Ok(serde_json::json!({
            "outcome": "accepted",
            "patient_workspace_id": ws_id,
            "chat_id": chat_id,
            "message_id": msg_id,
        }))
    }

    async fn handle_queue_withdraw(
        &self,
        payload: &serde_json::Value,
    ) -> Result<serde_json::Value, DbError> {
        let ws_id = required_str(payload, "patient_workspace_id")?;
        let chat_id = required_str(payload, "chat_id")?;
        let msg_id = required_str(payload, "message_id")?;
        let mut snap = self.chat_snapshot(&ws_id, &chat_id).await?.snapshot;
        let before = snap.queued.len();
        snap.queued.retain(|q| q.id.0 != msg_id);
        if snap.queued.len() == before {
            return Ok(serde_json::json!({
                "outcome": "rejected",
                "message": "Coda couldn’t remove that queued message.",
            }));
        }
        let title = self
            .existing_chat_title(&ws_id, &chat_id)?
            .unwrap_or_else(|| "Conversation".into());
        self.upsert_chat_snapshot(&ws_id, &chat_id, &title, DEMO_NOW, &snap)?;
        Ok(serde_json::json!({
            "outcome": "accepted",
            "patient_workspace_id": ws_id,
            "chat_id": chat_id,
            "message_id": msg_id,
        }))
    }

    async fn handle_queue_send_now(
        &self,
        _envelope: &serde_json::Value,
        payload: &serde_json::Value,
    ) -> Result<serde_json::Value, DbError> {
        let ws_id = required_str(payload, "patient_workspace_id")?;
        let chat_id = required_str(payload, "chat_id")?;
        let msg_id = required_str(payload, "message_id")?;
        let mut snap = self.chat_snapshot(&ws_id, &chat_id).await?.snapshot;
        let Some(pos) = snap.queued.iter().position(|q| q.id.0 == msg_id) else {
            return Ok(serde_json::json!({
                "outcome": "rejected",
                "message": "Coda couldn’t send that queued message.",
            }));
        };
        let queued = snap.queued.remove(pos);
        let is_new = snap.transcript.is_empty();
        self.append_chart_exchange(
            &ws_id,
            &chat_id,
            &mut snap,
            is_new,
            (queued.id.0, queued.text, queued.saved_at.0),
        )
        .await?;
        Ok(serde_json::json!({
            "outcome": "accepted",
            "patient_workspace_id": ws_id,
            "chat_id": chat_id,
            "message_id": msg_id,
        }))
    }

    async fn handle_conversation_archive(
        &self,
        payload: &serde_json::Value,
        archived: bool,
    ) -> Result<serde_json::Value, DbError> {
        let ws_id = required_str(payload, "patient_workspace_id")?;
        let chat_id = required_str(payload, "chat_id")?;
        let rows = self.driver.select_where(
            Chats::Table,
            &[Chats::Title, Chats::CreatedAt, Chats::SnapshotJson],
            &[
                (Chats::WorkspaceId, ws_id.as_str()),
                (Chats::ChatId, chat_id.as_str()),
            ],
        )?;
        let Some(row) = rows.first() else {
            return Err(not_found("conversation", &chat_id));
        };
        let mut snap: ChatSnapshot = serde_json::from_str(&row.get_str("snapshot_json")?)?;
        snap.archived = archived;
        self.upsert_chat_snapshot(
            &ws_id,
            &chat_id,
            &row.get_str("title")?,
            &row.get_str("created_at")?,
            &snap,
        )?;
        Ok(serde_json::json!({
            "outcome": "accepted",
            "patient_workspace_id": ws_id,
            "chat_id": chat_id,
        }))
    }

    async fn handle_discussion_open(
        &self,
        payload: &serde_json::Value,
    ) -> Result<serde_json::Value, DbError> {
        let ws_id = required_str(payload, "patient_workspace_id")?;
        let chat_id = required_str(payload, "chat_id")?;
        let _ = self.load_patient_name(&ws_id)?;
        let existing = self.driver.select_where(
            Chats::Table,
            &[Chats::ChatId],
            &[
                (Chats::WorkspaceId, ws_id.as_str()),
                (Chats::ChatId, chat_id.as_str()),
            ],
        )?;
        if existing.is_empty() {
            let snap = ChatSnapshot {
                sources: Default::default(),
                transcript: Vec::new(),
                feedback: SnapshotFeedback::Loaded,
                queued: Vec::new(),
                archived: false,
            };
            self.upsert_chat_snapshot(&ws_id, &chat_id, "Clinical discussion", DEMO_NOW, &snap)?;
        }
        Ok(serde_json::json!({
            "outcome": "accepted",
            "patient_workspace_id": ws_id,
            "chat_id": chat_id,
            "returned_messages": [],
        }))
    }

    fn handle_item_open(&self, payload: &serde_json::Value) -> Result<serde_json::Value, DbError> {
        let ws_id = required_str(payload, "patient_workspace_id")?;
        let item_id = required_str(payload, "item_id")?;
        self.mark_today_item_seen(&ws_id, &item_id)?;
        Ok(serde_json::json!({
            "outcome": "accepted",
            "patient_workspace_id": ws_id,
            "item_id": item_id,
        }))
    }

    fn search_patient_records(
        &self,
        ws_id: &str,
        terms: &[String],
    ) -> Result<Vec<DemoSourceRecord>, DbError> {
        self.search_records(ws_id, terms, 4)
    }

    /// Searches patient chart records in SQLite for `ws_id`, matching expanded clinical `terms`.
    pub fn search_records(
        &self,
        ws_id: &str,
        terms: &[String],
        limit: u64,
    ) -> Result<Vec<DemoSourceRecord>, DbError> {
        let expanded = expand_search_terms(terms);
        for active in [expanded.as_slice(), &[][..]] {
            let mut stmt = Query::select();
            stmt.columns(SourceRecords::COLS.iter().copied())
                .from(SourceRecords::Table)
                .and_where(Expr::col(SourceRecords::WorkspaceId).eq(ws_id));
            if !active.is_empty() {
                let mut any_term = Cond::any();
                for term in active {
                    let pat = format!("%{term}%");
                    any_term = any_term
                        .add(Expr::col(SourceRecords::Title).like(&pat))
                        .add(Expr::col(SourceRecords::Body).like(&pat))
                        .add(Expr::col(SourceRecords::RawContentJson).like(&pat))
                        .add(Expr::col(SourceRecords::RecordGroup).like(&pat));
                }
                stmt.cond_where(any_term);
            }
            stmt.order_by(SourceRecords::RecordedAt, Order::Desc)
                .limit(Ord::max(limit, 1));
            let rows = self.driver.select(&stmt)?;
            if !rows.is_empty() || active.is_empty() {
                return rows.iter().map(decode_source_record).collect();
            }
        }
        Ok(Vec::new())
    }

    /// Reads exact chart records for `ws_id` matching `ids`, preserving requested ID order.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn read_records_by_ids(
        &self,
        ws_id: &str,
        ids: &[String],
    ) -> Result<Vec<DemoSourceRecord>, DbError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let stmt = Query::select()
            .columns(SourceRecords::COLS.iter().copied())
            .from(SourceRecords::Table)
            .and_where(Expr::col(SourceRecords::WorkspaceId).eq(ws_id))
            .and_where(Expr::col(SourceRecords::Id).is_in(ids.iter().map(String::as_str)))
            .to_owned();
        let fetched: Vec<DemoSourceRecord> = self
            .driver
            .select(&stmt)?
            .iter()
            .map(decode_source_record)
            .collect::<Result<_, _>>()?;
        let mut by_id: BTreeMap<String, DemoSourceRecord> = fetched
            .into_iter()
            .map(|rec| (rec.id.clone(), rec))
            .collect();
        let mut ordered = Vec::new();
        for id in ids {
            if let Some(rec) = by_id.remove(id) {
                ordered.push(rec);
            }
        }
        Ok(ordered)
    }

    /// Lists all seeded patient workspace identifiers in the demo database.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn list_workspace_ids(&self) -> Result<Vec<String>, DbError> {
        let q = Query::select()
            .column(Patients::WorkspaceId)
            .from(Patients::Table)
            .order_by(Patients::WorkspaceId, Order::Asc)
            .to_owned();
        self.driver
            .select(&q)?
            .iter()
            .map(|r| r.get_str("workspace_id"))
            .collect()
    }

    /// Loads all chart records for `ws_id` in reverse chronological order.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn load_records(&self, ws_id: &str) -> Result<Vec<DemoSourceRecord>, DbError> {
        self.load_patient_records(ws_id)
    }

    /// Loads the patient display name for `ws_id`.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn load_patient_display_name(&self, ws_id: &str) -> Result<String, DbError> {
        self.load_patient_name(ws_id)
    }

    /// Loads a stored chat row `(title, created_at, snapshot)` for `(ws_id, chat_id)`.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn load_chat_snapshot_row(
        &self,
        ws_id: &str,
        chat_id: &str,
    ) -> Result<Option<(String, String, ChatSnapshot)>, DbError> {
        self.load_chat_row(ws_id, chat_id)
    }

    /// Saves a chat snapshot row into `Chats`.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn save_chat_snapshot_row(
        &self,
        ws_id: &str,
        chat_id: &str,
        title: &str,
        created_at: &str,
        snap: &ChatSnapshot,
    ) -> Result<(), DbError> {
        self.upsert_chat_snapshot(ws_id, chat_id, title, created_at, snap)
    }

    /// Finds the conversation and exchange index owning `work_item_id`.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn locate_chat_work_exchange(
        &self,
        ws_id: &str,
        chat_id: Option<&str>,
        work_item_id: &str,
    ) -> Result<Option<ChatWorkLocation>, DbError> {
        Ok(self
            .find_chat_work_exchange(ws_id, chat_id, work_item_id)?
            .map(|found| ChatWorkLocation {
                chat_id: found.chat_id,
                title: found.title,
                created_at: found.created_at,
                snapshot: found.snap,
                exchange_idx: found.exchange_idx,
            }))
    }

    /// Updates a patient reply draft after checking optimistic revision `expected_rev`.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn apply_reply_revision(
        &self,
        ws_id: &str,
        reply_work_id: &str,
        expected_rev: u64,
        revised_text: String,
    ) -> Result<u64, DbError> {
        let payload = serde_json::json!({ "patient_workspace_id": ws_id });
        let (_, next_rev) = self
            .mutate_workspace(&payload, |ws| {
                let reply = ws
                    .replies
                    .iter_mut()
                    .find(|r| r.reply_work_id == reply_work_id)
                    .ok_or_else(|| not_found("reply draft", reply_work_id))?;
                let rev = reply.draft_revision.unwrap_or(1);
                if rev != expected_rev {
                    return Err(DbError::InvalidCommand(
                        "reply draft revision changed".into(),
                    ));
                }
                let next = rev + 1;
                reply.draft_text = revised_text;
                reply.draft_revision = Some(next);
                Ok(next)
            })
            .await?;
        Ok(next_rev)
    }

    async fn handle_decide_recommendation(
        &self,
        payload: &serde_json::Value,
    ) -> Result<serde_json::Value, DbError> {
        let rec_id = required_str(payload, "recommendation_id")?;
        self.mutate_workspace(payload, |ws| {
            let pos = ws
                .undecided_recommendations
                .iter()
                .position(|r| r.recommendation_id == rec_id)
                .ok_or_else(|| not_found("recommendation", &rec_id))?;
            let chosen = ws.undecided_recommendations.remove(pos);
            ws.active_plan_items.push(WorkspacePlanItem {
                item_id: format!("plan-{}", next_seq()),
                patient_workspace_id: ws.patient_workspace_id.clone(),
                title: chosen.medication,
                detail: chosen.rationale,
                decided_at: DEMO_NOW.into(),
                source_recommendation_id: Some(chosen.recommendation_id),
            });
            Ok(())
        })
        .await?;
        Ok(serde_json::json!({ "ok": true }))
    }

    async fn handle_reply_close(
        &self,
        payload: &serde_json::Value,
    ) -> Result<serde_json::Value, DbError> {
        let reply_id = required_str(payload, "reply_work_id")?;
        let (ws_id, ()) = self
            .mutate_workspace(payload, |ws| {
                ws.replies.retain(|r| r.reply_work_id != reply_id);
                Ok(())
            })
            .await?;
        self.driver.update_where(
            TodayItems::Table,
            TodayItems::Done,
            1.into(),
            &[
                (TodayItems::WorkspaceId, &ws_id),
                (TodayItems::Kind, "reply"),
            ],
        )?;
        Ok(serde_json::json!({ "ok": true }))
    }

    async fn handle_reply_revise(
        &self,
        payload: &serde_json::Value,
    ) -> Result<serde_json::Value, DbError> {
        let reply_id = required_str(payload, "reply_work_id")?;
        let instruction = required_str(payload, "instruction")?;
        let expected = payload
            .get("expected_draft_revision")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(1);
        let (_, next_rev) = self
            .mutate_workspace(payload, |ws| {
                let reply = ws
                    .replies
                    .iter_mut()
                    .find(|r| r.reply_work_id == reply_id)
                    .ok_or_else(|| not_found("reply draft", &reply_id))?;
                let rev = reply.draft_revision.unwrap_or(1);
                if rev != expected {
                    return Err(DbError::InvalidCommand(
                        "reply draft revision changed".into(),
                    ));
                }
                let next = rev + 1;
                reply.draft_text = format!(
                    "{}\n\n[Updated per physician note: {}]",
                    reply.draft_text,
                    instruction.trim()
                );
                reply.draft_revision = Some(next);
                Ok(next)
            })
            .await?;
        Ok(serde_json::json!({ "ok": true, "draft_revision": next_rev }))
    }

    fn handle_item_done(&self, payload: &serde_json::Value) -> Result<serde_json::Value, DbError> {
        let item_id = required_str(payload, "item_id")?;
        self.driver.update_where(
            TodayItems::Table,
            TodayItems::Done,
            1.into(),
            &[(TodayItems::ItemId, &item_id)],
        )?;
        Ok(serde_json::json!({ "ok": true }))
    }

    fn existing_chat_title(&self, ws_id: &str, chat_id: &str) -> Result<Option<String>, DbError> {
        let rows = self.driver.select_where(
            Chats::Table,
            &[Chats::Title],
            &[(Chats::WorkspaceId, ws_id), (Chats::ChatId, chat_id)],
        )?;
        rows.first().map(|r| r.get_str("title")).transpose()
    }

    fn load_workspace_chats(&self, ws_id: &str) -> Result<Vec<WorkspaceChat>, DbError> {
        let q = Query::select()
            .columns([
                Chats::ChatId,
                Chats::Title,
                Chats::CreatedAt,
                Chats::SnapshotJson,
            ])
            .from(Chats::Table)
            .and_where(Expr::col(Chats::WorkspaceId).eq(ws_id))
            .order_by(Chats::CreatedAt, Order::Desc)
            .to_owned();
        self.driver
            .select(&q)?
            .iter()
            .map(|r| {
                let snap: ChatSnapshot = serde_json::from_str(&r.get_str("snapshot_json")?)?;
                let (summary, outcome) = summarize_snapshot(&snap);
                Ok(WorkspaceChat {
                    chat_id: r.get_str("chat_id")?,
                    title: Some(r.get_str("title")?),
                    summary,
                    outcome,
                    created_at: r.get_str("created_at")?,
                    archived: snap.archived,
                })
            })
            .collect()
    }

    pub(crate) fn load_patient_name(&self, ws_id: &str) -> Result<String, DbError> {
        let rows = self.driver.select_where(
            Patients::Table,
            &[Patients::DisplayName],
            &[(Patients::WorkspaceId, ws_id)],
        )?;
        let row = rows
            .first()
            .ok_or_else(|| not_found("patient workspace", ws_id))?;
        row.get_str("display_name")
    }

    pub(crate) fn load_patient_records(
        &self,
        ws_id: &str,
    ) -> Result<Vec<DemoSourceRecord>, DbError> {
        let q = Query::select()
            .columns(SourceRecords::COLS.iter().copied())
            .from(SourceRecords::Table)
            .and_where(Expr::col(SourceRecords::WorkspaceId).eq(ws_id))
            .order_by(SourceRecords::RecordedAt, Order::Desc)
            .to_owned();
        self.driver
            .select(&q)?
            .iter()
            .map(decode_source_record)
            .collect()
    }

    pub(crate) fn upsert_chat_snapshot(
        &self,
        ws_id: &str,
        chat_id: &str,
        title: &str,
        created_at: &str,
        snap: &ChatSnapshot,
    ) -> Result<(), DbError> {
        let insert = Query::insert()
            .into_table(Chats::Table)
            .columns(Chats::COLS.iter().copied())
            .values_panic([
                chat_id.into(),
                ws_id.into(),
                title.into(),
                created_at.into(),
                serde_json::to_string(snap)?.as_str().into(),
            ])
            .on_conflict(
                OnConflict::column(Chats::ChatId)
                    .update_columns([Chats::Title, Chats::SnapshotJson])
                    .to_owned(),
            )
            .to_owned();
        self.driver.insert(&insert)
    }

    pub(crate) fn mark_today_item_seen(&self, ws_id: &str, item_id: &str) -> Result<(), DbError> {
        let rows = self.driver.select_where(
            TodayItems::Table,
            &[TodayItems::RowJson],
            &[
                (TodayItems::WorkspaceId, ws_id),
                (TodayItems::ItemId, item_id),
            ],
        )?;
        if let Some(row) = rows.first() {
            self.driver.update_where(
                TodayItems::Table,
                TodayItems::Seen,
                1.into(),
                &[
                    (TodayItems::WorkspaceId, ws_id),
                    (TodayItems::ItemId, item_id),
                ],
            )?;
            let mut parsed: TodayRow = serde_json::from_str(&row.get_str("row_json")?)?;
            parsed.seen = true;
            parsed.standing = Some(SeenStandingWire::Seen {
                seen_at: DEMO_NOW.into(),
            });
            self.driver.update_where(
                TodayItems::Table,
                TodayItems::RowJson,
                serde_json::to_string(&parsed)?.into(),
                &[
                    (TodayItems::WorkspaceId, ws_id),
                    (TodayItems::ItemId, item_id),
                ],
            )?;
        }
        Ok(())
    }
}

pub(crate) type ChartRead = (String, RecordGroup, String, String);

pub(crate) fn chart_reads_from(records: &[DemoSourceRecord]) -> Vec<ChartRead> {
    records
        .iter()
        .map(|r| {
            (
                r.id.clone(),
                parse_record_group(&r.group),
                r.title.clone(),
                r.recorded_at.clone(),
            )
        })
        .collect()
}

fn command_rejected(message: &str) -> serde_json::Value {
    serde_json::json!({
        "outcome": "rejected",
        "message": message,
    })
}

fn question_requests_memory_note(question: &str) -> bool {
    let lower = question.to_ascii_lowercase();
    [
        "remember",
        "memory note",
        "note to chart",
        "save a note",
        "save note",
        "add note",
        "keep in mind",
        "document that",
    ]
    .iter()
    .any(|cue| lower.contains(cue))
}

fn append_follow_up_and_memory_steps(
    steps: &mut ExchangeSteps,
    read_conversation: bool,
    saved_memory_note: bool,
) {
    let ExchangeSteps::Read(items) = steps else {
        return;
    };
    if read_conversation {
        items.push(Step::ReadConversation {
            state: StepState::Returned { result: () },
        });
    }
    if saved_memory_note {
        items.push(Step::SavedMemoryNote {
            state: StepState::Returned { result: () },
        });
    }
}

fn mark_running_steps_stopped(steps: &mut ExchangeSteps) {
    let ExchangeSteps::Read(items) = steps else {
        return;
    };
    for step in items {
        match step {
            Step::SearchedChart { state, .. } => {
                if matches!(state, StepState::Running) {
                    *state = StepState::Stopped;
                }
            }
            Step::ReadRecords { state, .. } => {
                if matches!(state, StepState::Running) {
                    *state = StepState::Stopped;
                }
            }
            Step::ReadConversation { state }
            | Step::SavedMemoryNote { state }
            | Step::EditedMemory { state } => {
                if matches!(state, StepState::Running) {
                    *state = StepState::Stopped;
                }
            }
        }
    }
}

pub(crate) fn build_exchange_with_segments(
    prefix: &str,
    suffix: &str,
    msg: (String, String, i64),
    query: &str,
    reads: &[ChartRead],
    more: bool,
    answer: (String, Vec<MessageSegment>, Vec<MessageSegment>),
) -> Exchange {
    let (key, question, ts) = msg;
    let (answer_text, segments, basis) = answer;
    let steps = if reads.is_empty() && query.is_empty() {
        Vec::new()
    } else {
        let read_records = reads
            .iter()
            .map(|(id, group, title, dt)| RecordRead::Whole {
                version: SourceVersion {
                    candidate_id: id.clone(),
                    revision: "1".into(),
                },
                about: RecordAbout {
                    group: Some(*group),
                    title: Some(title.clone()),
                    dates: vec![RecordDate {
                        role: Some(DateRole::Recorded),
                        value: dt.clone(),
                    }],
                },
            })
            .collect();
        vec![
            Step::SearchedChart {
                query: query.into(),
                group: None,
                page: SearchPage::First,
                state: StepState::Returned {
                    result: Found {
                        records: reads.len(),
                        more,
                    },
                },
            },
            Step::ReadRecords {
                requested: reads.len(),
                state: StepState::Returned {
                    result: read_records,
                },
            },
        ]
    };
    Exchange {
        work_item_id: WorkItemId(format!("wi-{prefix}-{suffix}")),
        messages: vec![PhysicianMessage {
            id: MessageKey(key),
            text: question,
            saved_at: SavedAt(ts),
        }],
        steps: ExchangeSteps::Read(steps),
        kept: Vec::new(),
        answer: AnswerState::Finished {
            response_id: ResponseId(format!("resp-{prefix}-{suffix}")),
            attempt_id: AttemptId(format!("att-{prefix}-{suffix}")),
            finished_at: SavedAt(ts + 2),
            text: answer_text,
            segments,
            basis: (!basis.is_empty()).then_some(basis),
            newer_records: false,
            feedback: None,
        },
    }
}

/// Picks the most relevant verbatim contiguous slice (`&body[start..end]`) of `body`
/// for `terms`, falling back to the first clinically substantive line/sentence.
/// Never crosses newlines so every returned quote matches both `rec.body` and any
/// matching single-section `CitedTextPartWire` verbatim.
pub(crate) fn extract_verbatim_passage(body: &str, terms: &[String]) -> String {
    let candidates = collect_passage_candidates(body);
    let Some(best) = candidates
        .iter()
        .enumerate()
        .max_by_key(|(idx, slice)| {
            let hits = term_match_score(slice, terms);
            let substantive = usize::from(slice.chars().count() >= 12);
            (hits, substantive, usize::MAX - *idx)
        })
        .map(|(_, slice)| *slice)
    else {
        return first_trimmed_line(body).to_owned();
    };
    best.to_owned()
}

fn collect_passage_candidates(body: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut line_start = 0usize;
    for (idx, ch) in body.char_indices() {
        if ch == '\n' {
            push_line_sentences(&body[line_start..idx], &mut out);
            line_start = idx + 1;
        }
    }
    if line_start < body.len() {
        push_line_sentences(&body[line_start..], &mut out);
    }
    out
}

fn push_line_sentences<'a>(raw_line: &'a str, out: &mut Vec<&'a str>) {
    let mut trimmed = raw_line.trim();
    if trimmed.is_empty() {
        return;
    }
    if let Some(rest) = trimmed.strip_prefix("Conclusion:") {
        let after = rest.trim_start();
        if !after.is_empty() {
            trimmed = after;
        }
    }
    let mut seg_start = 0usize;
    let mut prev_punct = false;
    for (idx, ch) in trimmed.char_indices() {
        if prev_punct && ch.is_whitespace() {
            push_bounded_span(trimmed[seg_start..idx].trim(), out);
            seg_start = idx + ch.len_utf8();
        }
        prev_punct = matches!(ch, '.' | '!' | '?');
    }
    if seg_start < trimmed.len() {
        push_bounded_span(trimmed[seg_start..].trim(), out);
    }
}

fn push_bounded_span<'a>(span: &'a str, out: &mut Vec<&'a str>) {
    if span.is_empty() {
        return;
    }
    const MAX_PASSAGE_BYTES: usize = 220;
    if span.len() <= MAX_PASSAGE_BYTES {
        out.push(span);
        return;
    }
    let mut cut = 0usize;
    for (idx, ch) in span.char_indices() {
        if idx > MAX_PASSAGE_BYTES {
            break;
        }
        if ch.is_whitespace() && idx >= 40 {
            cut = idx;
        }
    }
    let clipped = if cut > 0 {
        span[..cut].trim_end()
    } else {
        let end = span
            .char_indices()
            .take_while(|(idx, _)| *idx <= MAX_PASSAGE_BYTES)
            .last()
            .map(|(idx, ch)| idx + ch.len_utf8())
            .unwrap_or(span.len());
        span[..end].trim_end()
    };
    if !clipped.is_empty() {
        out.push(clipped);
    }
}

fn first_trimmed_line(body: &str) -> &str {
    body.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("")
}

fn term_match_score(candidate: &str, terms: &[String]) -> usize {
    if terms.is_empty() {
        return 0;
    }
    let lower = candidate.to_ascii_lowercase();
    terms
        .iter()
        .filter(|term| {
            let t = term.as_str();
            lower.contains(t)
                || match t {
                    "ldl" => lower.contains("low density lipoprotein"),
                    "hdl" => lower.contains("high density lipoprotein"),
                    "a1c" => lower.contains("hemoglobin a1c"),
                    "blood" | "pressure" | "bp" => lower.contains("bp "),
                    "heart" | "hr" => lower.contains("hr "),
                    "egfr" => lower.contains("glomerular filtration"),
                    _ => false,
                }
        })
        .count()
}

pub(crate) fn build_cited_source(
    key: &str,
    number: usize,
    rec: &DemoSourceRecord,
) -> CitedSourceWire {
    let quote = extract_verbatim_passage(&rec.body, &[]);
    build_cited_source_with_quote(key, number, rec, quote)
}

fn pretty_raw_json(raw: &str) -> String {
    serde_json::from_str::<serde_json::Value>(raw)
        .and_then(|val| serde_json::to_string_pretty(&val))
        .unwrap_or_else(|_| raw.to_owned())
}

pub(crate) fn build_cited_source_with_quote(
    key: &str,
    number: usize,
    rec: &DemoSourceRecord,
    quote: String,
) -> CitedSourceWire {
    CitedSourceWire {
        source_key: key.to_owned(),
        number,
        source_label: rec.source_label.clone(),
        title: rec.title.clone(),
        record_type: Some(rec.group.clone()),
        recorded_at: Some(rec.recorded_at.clone()),
        record_status: (!rec.record_status.trim().is_empty()).then(|| rec.record_status.clone()),
        quote,
        body: rec.body.clone(),
        readable_parts: rec.readable_parts.clone(),
        fields: rec.fields.clone(),
        raw_content: rec.raw_content.as_deref().map(pretty_raw_json),
        external_url: None,
        origin: CitationOrigin::SourceRecord,
    }
}

pub(crate) fn build_item_citation(rec: &DemoSourceRecord) -> ItemCitationWire {
    let excerpt = extract_verbatim_passage(&rec.body, &[]);
    ItemCitationWire {
        candidate_id: rec.id.clone(),
        revision: "1".into(),
        source_id: "synthetic-ehr".into(),
        source_label: rec.source_label.clone(),
        label: rec.title.clone(),
        as_of: Some(rec.recorded_at.clone()),
        excerpt,
        body: rec.body.clone(),
        native_uri: None,
    }
}

pub(crate) fn synthesize_chart_answer_with_citations(
    question: &str,
    records: &[DemoSourceRecord],
    key_prefix: &str,
) -> (
    String,
    Vec<MessageSegment>,
    Vec<MessageSegment>,
    BTreeMap<String, CitedSourceWire>,
) {
    if records.is_empty() {
        let text = format!(
            "No matching chart records were found in SQLite for \"{question}\". Try asking about medications, labs, blood pressure, or recent visits."
        );
        return (
            text.clone(),
            vec![MessageSegment::Text { text }],
            Vec::new(),
            BTreeMap::new(),
        );
    }
    let terms = extract_search_terms(question);
    let mut text = format!(
        "From SQLite chart search across {} record(s):\n\n",
        records.len()
    );
    let mut segments = vec![MessageSegment::Text {
        text: format!(
            "From SQLite chart search across {} record(s): ",
            records.len()
        ),
    }];
    let mut sources = BTreeMap::new();
    let mut basis_cites = Vec::new();
    for (idx, rec) in records.iter().enumerate() {
        let number = idx + 1;
        let key = format!("{key_prefix}{}-{}", next_seq(), number);
        let quote = extract_verbatim_passage(&rec.body, &terms);
        let target = if quote.is_empty() {
            CitationTargetKind::WholeRecord
        } else {
            CitationTargetKind::Passage
        };
        let label = format!("{} ({})", rec.title, rec.recorded_at);
        text.push_str(&format!("[{number}] {label} — \"{quote}\"\n\n"));
        if idx > 0 {
            segments.push(MessageSegment::Text { text: " ".into() });
        }
        segments.push(MessageSegment::Text {
            text: format!("{label}: \"{quote}\""),
        });
        let cited = build_cited_source_with_quote(&key, number, rec, quote.clone());
        segments.push(MessageSegment::Citation {
            source_key: key.clone(),
            number,
            label: label.clone(),
            quote: cited.quote.clone(),
            target,
        });
        basis_cites.push((
            key.clone(),
            number,
            label,
            cited.quote.clone(),
            target,
            rec.recorded_at.as_str(),
        ));
        sources.insert(key, cited);
    }
    let latest_date = basis_cites
        .iter()
        .map(|(_, _, _, _, _, dt)| *dt)
        .max()
        .unwrap_or(DEMO_DATE);
    let mut basis = vec![MessageSegment::Text {
        text: format!(
            "Grounded directly in {} SQLite chart record(s) (",
            records.len()
        ),
    }];
    for (idx, (key, number, label, quote, target, _)) in basis_cites.into_iter().enumerate() {
        if idx > 0 {
            basis.push(MessageSegment::Text { text: ", ".into() });
        }
        basis.push(MessageSegment::Text {
            text: format!("{label} "),
        });
        basis.push(MessageSegment::Citation {
            source_key: key,
            number,
            label,
            quote,
            target,
        });
    }
    basis.push(MessageSegment::Text {
        text: format!(
            "). Currency & scope: reflects synthetic EHR fixtures documented through {latest_date}; excludes unrecorded clinical changes or portal messages outside the seeded chart."
        ),
    });
    (text.trim_end().into(), segments, basis, sources)
}

pub(crate) fn build_answer_segments_and_sources_for_follow_up(
    summary: &str,
    prompt: &str,
    records: &[DemoSourceRecord],
) -> (Exchange, BTreeMap<String, CitedSourceWire>) {
    let seq = next_seq();
    let reads: Vec<_> = records
        .iter()
        .take(2)
        .map(|r| {
            (
                r.id.clone(),
                parse_record_group(&r.group),
                r.title.clone(),
                r.recorded_at.clone(),
            )
        })
        .collect();
    let (_, segments, basis, sources) =
        synthesize_chart_answer_with_citations(summary, records.get(..2).unwrap_or(records), "fu");
    let answer_text = format!(
        "Paused follow-up \"{summary}\" for review. Ask below to update the plan or restart monitoring."
    );
    let ex = build_exchange_with_segments(
        "fu",
        &seq.to_string(),
        (
            format!("msg-fu-{seq}"),
            prompt.to_owned(),
            DEMO_BASE_UNIX + 10,
        ),
        summary,
        &reads,
        false,
        (answer_text, segments, basis),
    );
    (ex, sources)
}

fn summarize_snapshot(snap: &ChatSnapshot) -> (Option<String>, Option<HistoryWorkOutcome>) {
    for ex in snap.transcript.iter().rev() {
        if let AnswerState::Finished { text, .. } = &ex.answer {
            let preview = text.lines().next().unwrap_or("").trim();
            let short = if preview.chars().count() > 110 {
                format!("{}…", preview.chars().take(109).collect::<String>())
            } else {
                preview.to_owned()
            };
            return (Some(short), Some(HistoryWorkOutcome::Answered));
        }
    }
    (None, None)
}

pub(crate) fn not_found(kind: &'static str, id: &str) -> DbError {
    DbError::NotFound {
        kind,
        id: id.into(),
    }
}

pub(crate) fn required_str(
    value: &serde_json::Value,
    key: &'static str,
) -> Result<String, DbError> {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| DbError::InvalidCommand(format!("missing `{key}`")))
}

fn decode_source_record(row: &SqlRow) -> Result<DemoSourceRecord, DbError> {
    let group = row.get_str("record_group")?;
    let body = row.get_str("body")?;
    let readable_parts_json = row.get_str("readable_parts_json")?;
    let stored_parts: Vec<CitedTextPartWire> = if readable_parts_json.trim().is_empty() {
        Vec::new()
    } else {
        serde_json::from_str(&readable_parts_json)?
    };
    let readable_parts = if stored_parts.is_empty() {
        derive_readable_parts(&group, &body)
    } else {
        stored_parts
    };
    let fields_json = row.get_str("fields_json")?;
    let record_status = row.get_str("record_status")?;
    let raw_str = row.get_str("raw_content_json")?;
    let raw_content = (!raw_str.trim().is_empty()).then_some(raw_str);
    let id = row.get_str("id")?;
    let fields = if fields_json.trim().is_empty() {
        raw_content
            .as_deref()
            .map(|raw| derive_fields_from_raw(&id, &group, raw))
            .unwrap_or_default()
    } else {
        serde_json::from_str(&fields_json)?
    };
    Ok(DemoSourceRecord {
        id,
        workspace_id: row.get_str("workspace_id")?,
        group,
        title: row.get_str("title")?,
        recorded_at: row.get_str("recorded_at")?,
        body,
        source_label: row.get_str("source_label")?,
        readable_parts,
        fields,
        record_status,
        raw_content,
    })
}

pub(crate) fn format_lab_range(
    range: Option<&serde_json::Value>,
) -> (Option<String>, Option<f64>, Option<f64>) {
    use serde_json::Value;
    let Some(val) = range else {
        return (None, None, None);
    };
    if let Some(s) = val.as_str().map(str::trim).filter(|s| !s.is_empty()) {
        return (Some(s.to_owned()), None, None);
    }
    let Some(obj) = val.as_object() else {
        return (None, None, None);
    };
    let low = obj.get("low").and_then(Value::as_f64);
    let high = obj.get("high").and_then(Value::as_f64);
    let formatted = match (low, high) {
        (Some(lo), Some(hi)) => Some(format!("{lo}–{hi}")),
        (None, Some(hi)) => Some(format!("<= {hi}")),
        (Some(lo), None) => Some(format!(">= {lo}")),
        (None, None) => None,
    };
    (formatted, low, high)
}

pub(crate) fn format_json_scalar(val: Option<&serde_json::Value>) -> Option<(String, Option<f64>)> {
    use serde_json::Value;
    let v = val?;
    match v {
        Value::Null => None,
        Value::Number(n) => Some((n.to_string(), n.as_f64())),
        Value::Bool(b) => Some((b.to_string(), None)),
        Value::String(s) => {
            let trimmed = s.trim();
            (!trimmed.is_empty()).then(|| (trimmed.to_owned(), trimmed.parse::<f64>().ok()))
        }
        _ => None,
    }
}

pub(crate) fn lab_abnormality_status(
    numeric: Option<f64>,
    low: Option<f64>,
    high: Option<f64>,
    text_val: Option<&str>,
) -> Option<String> {
    if let Some(n) = numeric {
        if let Some(hi) = high
            && n > hi
        {
            return Some("High".into());
        }
        if let Some(lo) = low
            && n < lo
        {
            return Some("Low".into());
        }
        if low.is_some() || high.is_some() {
            return Some("Normal".into());
        }
    }
    let raw = text_val?.trim();
    if raw.eq_ignore_ascii_case("elevated")
        || raw.eq_ignore_ascii_case("high")
        || raw.eq_ignore_ascii_case("low")
        || raw.eq_ignore_ascii_case("positive")
        || raw.eq_ignore_ascii_case("reactive")
    {
        return Some(raw.to_owned());
    }
    None
}

fn jstr_val<'a>(val: &'a serde_json::Value, key: &str) -> &'a str {
    val.get(key)
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
}

fn jnonempty_val<'a>(val: &'a serde_json::Value, key: &str, fb: &'a str) -> &'a str {
    match jstr_val(val, key).trim() {
        "" => fb,
        s => s,
    }
}

pub(crate) fn condition_fields(prefix: &str, conds: &[serde_json::Value]) -> Vec<CitedFieldWire> {
    conds
        .iter()
        .enumerate()
        .map(|(ci, c)| {
            let label = jstr_val(c, "label");
            let action = jnonempty_val(c, "action", "recorded");
            let note = jstr_val(c, "note").trim();
            let icd10 = jstr_val(c, "icd10").trim();
            let onset = jstr_val(c, "onset").trim();
            let value = if !note.is_empty() {
                note.to_owned()
            } else if !onset.is_empty() {
                format!("Onset {onset}")
            } else {
                action.to_owned()
            };
            let status = if icd10.is_empty() {
                action.to_owned()
            } else {
                format!("{action} · ICD-10 {icd10}")
            };
            CitedFieldWire {
                key: format!("{prefix}-{ci}"),
                name: label.to_owned(),
                value,
                unit: None,
                range: None,
                status: Some(status),
            }
        })
        .collect()
}

pub(crate) fn medication_fields(prefix: &str, meds: &[serde_json::Value]) -> Vec<CitedFieldWire> {
    meds.iter()
        .enumerate()
        .map(|(mi, m)| {
            let drug = jnonempty_val(m, "drug", jstr_val(m, "drug_text"));
            let action = jnonempty_val(m, "action", "active");
            let sig = jstr_val(m, "sig").trim();
            let med_reason = jstr_val(m, "reason").trim();
            let dose_summary = m
                .get("dose")
                .and_then(serde_json::Value::as_object)
                .and_then(|d| {
                    let val = format_json_scalar(d.get("value")).map(|(s, _)| s)?;
                    let unit = d
                        .get("unit")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("")
                        .trim();
                    Some(if unit.is_empty() {
                        val
                    } else {
                        format!("{val} {unit}")
                    })
                });
            let value = if !sig.is_empty() {
                sig.to_owned()
            } else {
                dose_summary.unwrap_or_else(|| action.to_owned())
            };
            let status = if med_reason.is_empty() {
                action.to_owned()
            } else {
                format!("{action} · {med_reason}")
            };
            CitedFieldWire {
                key: format!("{prefix}-{mi}"),
                name: drug.to_owned(),
                value,
                unit: None,
                range: None,
                status: Some(status),
            }
        })
        .collect()
}

pub(crate) fn lab_fields(prefix: &str, results: &[serde_json::Value]) -> Vec<CitedFieldWire> {
    results
        .iter()
        .enumerate()
        .map(|(ri, r)| {
            let test = jnonempty_val(r, "test", "Lab test");
            let text_val = r
                .get("text")
                .and_then(serde_json::Value::as_str)
                .map(str::trim);
            let (display_val, numeric_val) = format_json_scalar(r.get("value"))
                .or_else(|| {
                    text_val
                        .filter(|s| !s.is_empty())
                        .map(|s| (s.to_owned(), None))
                })
                .unwrap_or_else(|| ("Not supplied".into(), None));
            let unit = r
                .get("unit")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned);
            let (range_str, low, high) = format_lab_range(r.get("range"));
            let status = lab_abnormality_status(numeric_val, low, high, text_val);
            CitedFieldWire {
                key: format!("{prefix}-{ri}"),
                name: test.to_owned(),
                value: display_val,
                unit,
                range: range_str,
                status,
            }
        })
        .collect()
}

pub(crate) fn vitals_fields(prefix: &str, v: &serde_json::Value) -> Vec<CitedFieldWire> {
    use serde_json::Value;
    let Some(bp) = v.get("bp").and_then(Value::as_array) else {
        return Vec::new();
    };
    if bp.len() < 2 {
        return Vec::new();
    }
    let sys = bp[0].as_i64();
    let dia = bp[1].as_i64();
    let mut fields = vec![CitedFieldWire {
        key: format!("{prefix}-bp"),
        name: "Blood pressure".into(),
        value: format!("{}/{}", bp[0], bp[1]),
        unit: Some("mmHg".into()),
        range: Some("< 120/80".into()),
        status: match (sys, dia) {
            (Some(s), Some(d)) if s >= 130 || d >= 80 => Some("Elevated".into()),
            (Some(_), Some(_)) => Some("Normal".into()),
            _ => None,
        },
    }];
    if let Some(hr) = v.get("hr").and_then(Value::as_i64) {
        fields.push(CitedFieldWire {
            key: format!("{prefix}-hr"),
            name: "Heart rate".into(),
            value: hr.to_string(),
            unit: Some("bpm".into()),
            range: Some("60–100".into()),
            status: Some(
                if !(60..=100).contains(&hr) {
                    if hr > 100 { "High" } else { "Low" }
                } else {
                    "Normal"
                }
                .into(),
            ),
        });
    }
    for (json_key, field_key, label, unit) in [
        ("temp_c", "temp", "Temperature", "°C"),
        ("spo2", "spo2", "Oxygen saturation", "%"),
        ("rr", "rr", "Respiratory rate", "/min"),
        ("weight_kg", "wt", "Weight", "kg"),
        ("height_cm", "ht", "Height", "cm"),
    ] {
        if let Some((val, _)) = format_json_scalar(v.get(json_key)) {
            fields.push(CitedFieldWire {
                key: format!("{prefix}-{field_key}"),
                name: label.into(),
                value: val,
                unit: Some(unit.into()),
                range: None,
                status: None,
            });
        }
    }
    fields
}

pub(crate) fn derive_fields_from_raw(id: &str, group: &str, raw_json: &str) -> Vec<CitedFieldWire> {
    let Ok(val) = serde_json::from_str::<serde_json::Value>(raw_json) else {
        return Vec::new();
    };
    match group {
        "problems" => val
            .get("conditions")
            .and_then(serde_json::Value::as_array)
            .map(|conds| condition_fields(id, conds))
            .unwrap_or_default(),
        "medications" => val
            .get("medications")
            .and_then(serde_json::Value::as_array)
            .map(|meds| medication_fields(id, meds))
            .unwrap_or_default(),
        "results" => val
            .get("results")
            .and_then(serde_json::Value::as_array)
            .map(|results| lab_fields(id, results))
            .unwrap_or_default(),
        "vitals" => vitals_fields(id, &val),
        _ => Vec::new(),
    }
}

fn derive_readable_parts(group: &str, body: &str) -> Vec<CitedTextPartWire> {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    if group == "notes" || group == "documents" {
        return trimmed
            .split("\n\n")
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(|para| CitedTextPartWire {
                label: None,
                text: para.to_owned(),
            })
            .collect();
    }
    if group == "results"
        && let Some((findings, conc)) = trimmed.split_once("\n\nConclusion:")
    {
        let mut parts = Vec::new();
        if !findings.trim().is_empty() {
            parts.push(CitedTextPartWire {
                label: Some("Findings".into()),
                text: findings.trim().to_owned(),
            });
        }
        if !conc.trim().is_empty() {
            parts.push(CitedTextPartWire {
                label: Some("Conclusion".into()),
                text: conc.trim().to_owned(),
            });
        }
        if !parts.is_empty() {
            return parts;
        }
    }
    let label = match group {
        "problems" => Some("Diagnoses".into()),
        "medications" => Some("Medication list".into()),
        "results" => Some("Results".into()),
        "vitals" => Some("Vital signs".into()),
        _ => None,
    };
    vec![CitedTextPartWire {
        label,
        text: trimmed.to_owned(),
    }]
}

fn extract_search_terms(question: &str) -> Vec<String> {
    const STOP: &[&str] = &[
        "the", "and", "for", "with", "what", "when", "where", "which", "this", "that", "have",
        "has", "had", "does", "his", "her", "their", "about", "from", "into", "are", "was", "were",
        "been", "how", "any", "all", "can", "show", "tell", "check", "latest", "recent", "patient",
        "chart", "please",
    ];
    const SHORT_CLINICAL: &[&str] = &["bp", "hr", "rr"];
    let mut terms = Vec::new();
    for raw in question.split(|c: char| !c.is_alphanumeric() && c != '-') {
        let w = raw.trim().to_ascii_lowercase();
        let keep =
            (w.len() >= 3 && !STOP.contains(&w.as_str())) || SHORT_CLINICAL.contains(&w.as_str());
        if keep && !terms.contains(&w) {
            terms.push(w);
        }
        if terms.len() >= 6 {
            break;
        }
    }
    terms
}

fn expand_search_terms(terms: &[String]) -> Vec<String> {
    let mut out = terms.to_vec();
    for t in terms {
        let alias = match t.as_str() {
            "ldl" => Some("low density lipoprotein"),
            "hdl" => Some("high density lipoprotein"),
            "a1c" => Some("hemoglobin a1c"),
            "egfr" => Some("glomerular filtration"),
            "pressure" => Some("bp"),
            "heart" => Some("hr"),
            _ => None,
        };
        if let Some(a) = alias
            && !out.iter().any(|existing| existing == a)
        {
            out.push(a.to_owned());
        }
    }
    out
}

fn truncate_title(question: &str) -> String {
    let t = question.trim();
    if t.chars().count() <= 44 {
        t.into()
    } else {
        format!("{}…", t.chars().take(43).collect::<String>())
    }
}

/// Extracts search terms from a physician's clinical prompt.
#[cfg(not(target_arch = "wasm32"))]
pub fn search_terms_for_question(question: &str) -> Vec<String> {
    extract_search_terms(question)
}

/// Extracts a verbatim single-line passage from `body` matching `terms`.
#[cfg(not(target_arch = "wasm32"))]
pub fn extract_verbatim_quote(body: &str, terms: &[String]) -> String {
    extract_verbatim_passage(body, terms)
}

/// Builds a [`CitedSourceWire`] from a retrieved [`DemoSourceRecord`] and verbatim `quote`.
#[cfg(not(target_arch = "wasm32"))]
pub fn build_cited_source_wire(
    key: &str,
    number: usize,
    rec: &DemoSourceRecord,
    quote: String,
) -> CitedSourceWire {
    build_cited_source_with_quote(key, number, rec, quote)
}

/// Parses a stored record group string into [`RecordGroup`].
#[cfg(not(target_arch = "wasm32"))]
pub fn parse_record_group_wire(raw: &str) -> RecordGroup {
    parse_record_group(raw)
}

pub(crate) fn parse_record_group(raw: &str) -> RecordGroup {
    serde_json::from_value(serde_json::Value::String(raw.into())).unwrap_or(RecordGroup::Other)
}
