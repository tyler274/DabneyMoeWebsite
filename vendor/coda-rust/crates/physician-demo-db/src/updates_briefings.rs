//! SQLite-backed Today open/done, Updates standing, Visit Briefings, and Schedule refresh.

use crate::driver::DbError;
use crate::schema::{Briefings, Chats, TodayItems, UpdatesFeed};
use crate::store::{DEMO_NOW, DemoStore, build_item_citation, not_found};
use physician_wire::briefings::{
    BriefingSummaryItemWire, VisitBriefingResponse, VisitBriefingWire, WorkspaceBriefingsResponse,
};
use physician_wire::today_updates::{
    ItemWorkEvidenceWire, SeenStandingWire, TodayDoneOutcomeWire, TodayDoneResponseWire,
    UpdateItemKindWire, UpdateItemWire,
};
use physician_wire::{PracticeDayResponse, TodayRow};
use sea_query::{Expr, ExprTrait, Order, Query};

impl DemoStore {
    /// Opens a Today row or Updates entry, records it as `Seen`, and returns chart evidence.
    pub async fn open_today_update_item(
        &self,
        patient_workspace_id: &str,
        item_id: &str,
        _idempotency_key: &str,
    ) -> Result<ItemWorkEvidenceWire, DbError> {
        self.ensure_ready().await?;
        let display_name = self.load_patient_name(patient_workspace_id)?;

        let today_rows = self.driver.select_where(
            TodayItems::Table,
            &[TodayItems::RowJson],
            &[
                (TodayItems::WorkspaceId, patient_workspace_id),
                (TodayItems::ItemId, item_id),
            ],
        )?;
        let update_rows = self.driver.select_where(
            UpdatesFeed::Table,
            &[UpdatesFeed::PayloadJson],
            &[
                (UpdatesFeed::WorkspaceId, patient_workspace_id),
                (UpdatesFeed::Id, item_id),
            ],
        )?;

        let (version_id, kind_label, headline, narrative) = if let Some(row) = today_rows.first() {
            let parsed: TodayRow = serde_json::from_str(&row.get_str("row_json")?)?;
            self.mark_today_item_seen(patient_workspace_id, item_id)?;
            let kind_label = match parsed.kind.as_str() {
                "reply" | "drafted_reply" => "Patient reply",
                "preparation" => "Visit preparation",
                _ if parsed.urgency == "now" => "Urgent concern",
                _ => "Clinical concern",
            }
            .to_owned();
            let headline = parsed
                .title
                .clone()
                .or_else(|| parsed.reason.clone())
                .unwrap_or_else(|| "Patient attention item".into());
            let narrative = parsed.reason.clone().unwrap_or_else(|| headline.clone());
            (parsed.version_id, kind_label, headline, narrative)
        } else if let Some(row) = update_rows.first() {
            let mut item: UpdateItemWire = serde_json::from_str(&row.get_str("payload_json")?)?;
            item.standing = SeenStandingWire::Seen {
                seen_at: DEMO_NOW.into(),
            };
            self.driver.update_where(
                UpdatesFeed::Table,
                UpdatesFeed::PayloadJson,
                serde_json::to_string(&item)?.into(),
                &[
                    (UpdatesFeed::WorkspaceId, patient_workspace_id),
                    (UpdatesFeed::Id, item_id),
                ],
            )?;
            let kind_label = match item.kind {
                UpdateItemKindWire::ConcernRaised => "Clinical concern",
                UpdateItemKindWire::ConcernRetired => "Retired concern",
                UpdateItemKindWire::QuietReview => "Quiet chart review",
                UpdateItemKindWire::AssessedReview => "Chart review",
                UpdateItemKindWire::VisitBriefing => "Visit briefing",
                UpdateItemKindWire::PatientSummary => "Patient summary",
                UpdateItemKindWire::EndedNothingToDo => "Skipped review",
                UpdateItemKindWire::FollowUpChanged => "Follow-up",
            }
            .to_owned();
            (item.version_id, kind_label, item.headline, item.detail)
        } else {
            return Err(not_found("today or update item", item_id));
        };

        let records = self.load_patient_records(patient_workspace_id)?;
        let citations = records.iter().take(2).map(build_item_citation).collect();
        let default_chat = format!("chat-{patient_workspace_id}-1");
        let linked_chat_id = if self
            .driver
            .select_where(
                Chats::Table,
                &[Chats::ChatId],
                &[
                    (Chats::WorkspaceId, patient_workspace_id),
                    (Chats::ChatId, default_chat.as_str()),
                ],
            )?
            .is_empty()
        {
            None
        } else {
            Some(default_chat)
        };

        Ok(ItemWorkEvidenceWire {
            item_id: item_id.to_owned(),
            version_id,
            patient_workspace_id: patient_workspace_id.to_owned(),
            patient_display_name: display_name,
            kind_label,
            headline,
            narrative,
            basis_and_limitations: Some(
                "Grounded in the patient's chart records stored in the browser SQLite database."
                    .into(),
            ),
            citations,
            linked_chat_id,
            standing: SeenStandingWire::Seen {
                seen_at: DEMO_NOW.into(),
            },
        })
    }

    /// Clears a Today row as `Done` in SQLite.
    pub async fn mark_today_item_done(
        &self,
        patient_workspace_id: &str,
        item_id: &str,
        version_id: &str,
        _idempotency_key: &str,
    ) -> Result<TodayDoneResponseWire, DbError> {
        self.ensure_ready().await?;
        let rows = self.driver.select_where(
            TodayItems::Table,
            &[TodayItems::RowJson, TodayItems::Done],
            &[
                (TodayItems::WorkspaceId, patient_workspace_id),
                (TodayItems::ItemId, item_id),
            ],
        )?;
        let Some(row) = rows.first() else {
            return Ok(TodayDoneResponseWire {
                outcome: TodayDoneOutcomeWire::Rejected,
                item_id: item_id.to_owned(),
                message: Some("That item could not be found in SQLite.".into()),
            });
        };
        let parsed: TodayRow = serde_json::from_str(&row.get_str("row_json")?)?;
        if !version_id.is_empty() && parsed.version_id != version_id {
            return Ok(TodayDoneResponseWire {
                outcome: TodayDoneOutcomeWire::Rejected,
                item_id: item_id.to_owned(),
                message: Some("This item changed since it was loaded; refresh Today.".into()),
            });
        }
        self.driver.update_where(
            TodayItems::Table,
            TodayItems::Done,
            1.into(),
            &[
                (TodayItems::WorkspaceId, patient_workspace_id),
                (TodayItems::ItemId, item_id),
            ],
        )?;
        Ok(TodayDoneResponseWire {
            outcome: TodayDoneOutcomeWire::Accepted,
            item_id: item_id.to_owned(),
            message: None,
        })
    }

    /// Lists saved visit briefings for `workspace_id`.
    pub async fn workspace_briefings(
        &self,
        workspace_id: &str,
    ) -> Result<WorkspaceBriefingsResponse, DbError> {
        self.ensure_ready().await?;
        let _ = self.load_patient_name(workspace_id)?;
        let q = Query::select()
            .column(Briefings::PayloadJson)
            .from(Briefings::Table)
            .and_where(Expr::col(Briefings::WorkspaceId).eq(workspace_id))
            .order_by(Briefings::BriefingId, Order::Desc)
            .to_owned();
        let mut briefings: Vec<BriefingSummaryItemWire> = self
            .driver
            .select(&q)?
            .iter()
            .map(|r| {
                let b: VisitBriefingWire = serde_json::from_str(&r.get_str("payload_json")?)?;
                Ok(BriefingSummaryItemWire {
                    briefing_id: b.briefing_id,
                    appointment_id: b.appointment_id,
                    visit_label: b.visit_label,
                    starts_at: b.starts_at,
                    ends_at: b.ends_at,
                    prepared_at: b.prepared_at,
                    visit_card_orientation: b.visit_card_orientation,
                    visit_standing: b.visit_standing,
                })
            })
            .collect::<Result<Vec<_>, DbError>>()?;
        briefings.sort_by(|a, b| b.starts_at.cmp(&a.starts_at));
        Ok(WorkspaceBriefingsResponse { briefings })
    }

    /// Reads one structured pre-visit briefing from SQLite.
    pub async fn visit_briefing(
        &self,
        workspace_id: &str,
        briefing_id: &str,
    ) -> Result<VisitBriefingResponse, DbError> {
        self.ensure_ready().await?;
        let rows = self.driver.select_where(
            Briefings::Table,
            &[Briefings::PayloadJson],
            &[
                (Briefings::WorkspaceId, workspace_id),
                (Briefings::BriefingId, briefing_id),
            ],
        )?;
        let row = rows
            .first()
            .ok_or_else(|| not_found("visit briefing", briefing_id))?;
        let briefing: VisitBriefingWire = serde_json::from_str(&row.get_str("payload_json")?)?;
        Ok(VisitBriefingResponse { briefing })
    }

    /// Refreshes the practice day schedule directly from SQLite.
    pub async fn refresh_practice_day(&self) -> Result<PracticeDayResponse, DbError> {
        self.practice_day().await
    }
}
