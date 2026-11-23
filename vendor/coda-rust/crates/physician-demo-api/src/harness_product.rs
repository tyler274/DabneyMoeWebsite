//! `crates/harness` [`Product`] implementation over per-session `turso_core` [`DemoStore`].

use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use harness::engine::recipe::{Finish, RequestBinding, RequestRecipe, TextDelivery, ToolFuture};
use harness::engine::recovery::{Inputs, Messages, Records};
use harness::engine::results::ToolResult;
use harness::engine::runs::{HoldReason, RunOutcome};
use harness::engine::{
    Cancellation, Event, EventKind, Failure, Invocation, Item, LimitBackstop, NotCurrent, Product,
    ProductError, ProductMessage, RunLimits, TextDelta, Tool, ToolOutcome, Turn, WindowSeed,
};
use harness::skills::Package;
use physician_bus::{Bus, Event as BusEvent};
use physician_demo_db::driver::DbError;
use physician_demo_db::store::{
    DEFAULT_DEMO_BASE_UNIX, DEFAULT_DEMO_DATE, build_cited_source_wire, extract_verbatim_quote,
    next_demo_seq, parse_record_group_wire, search_terms_for_question,
};
use physician_demo_db::{
    DemoSourceRecord, LingoCgmSummary, LingoParseError, LingoReading, MentalHealthCheckIn,
    parse_lingo_csv,
};
use physician_wire::follow_ups::UpdateFollowUpBody;
use physician_wire::{
    AnswerState, AttemptId, ChatSnapshot, CitationTargetKind, CitedSourceWire, Exchange,
    ExchangeSteps, Found, MessageKey, MessageSegment, PhysicianMessage, RecordAbout, RecordDate,
    RecordRead, ResponseId, SavedAt, SearchPage, SourceVersion, Step, StepState, WorkItemId,
    WorkResumeTarget, WorkStopTarget,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::provider::ProviderBackend;
use crate::sessions::{ActiveRun, VisitorSession, unix_now};

pub(crate) const TOOL_CHART_SEARCH: &str = "chart_search";
pub(crate) const TOOL_CHART_READ: &str = "chart_read";
pub(crate) const TOOL_MEMORY_READ: &str = "memory_read";
pub(crate) const TOOL_MEMORY_SAVE: &str = "memory_save";
pub(crate) const TOOL_FOLLOW_UP_LIST: &str = "follow_up_list";
pub(crate) const TOOL_FOLLOW_UP_CREATE: &str = "follow_up_create";
pub(crate) const TOOL_BRIEFING_READ: &str = "briefing_read";
pub(crate) const TOOL_MENTAL_HEALTH_LOG: &str = "mental_health_log";
pub(crate) const TOOL_LINGO_CGM_INGEST: &str = "lingo_cgm_ingest";

const DEFAULT_DEMO_FOLLOW_UP_DUE_DATE: &str = "2026-09-03";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DemoTool {
    ChartSearch,
    ChartRead,
    MemoryRead,
    MemorySave,
    FollowUpList,
    FollowUpCreate,
    BriefingRead,
    MentalHealthLog,
    LingoCgmIngest,
    SkillView,
}

impl DemoTool {
    fn from_name(name: &str) -> Option<Self> {
        match name {
            TOOL_CHART_SEARCH => Some(Self::ChartSearch),
            TOOL_CHART_READ => Some(Self::ChartRead),
            TOOL_MEMORY_READ => Some(Self::MemoryRead),
            TOOL_MEMORY_SAVE => Some(Self::MemorySave),
            TOOL_FOLLOW_UP_LIST => Some(Self::FollowUpList),
            TOOL_FOLLOW_UP_CREATE => Some(Self::FollowUpCreate),
            TOOL_BRIEFING_READ => Some(Self::BriefingRead),
            TOOL_MENTAL_HEALTH_LOG => Some(Self::MentalHealthLog),
            TOOL_LINGO_CGM_INGEST => Some(Self::LingoCgmIngest),
            harness::skills::SKILL_VIEW => Some(Self::SkillView),
            _ => None,
        }
    }

    fn is_read_only(self) -> bool {
        match self {
            Self::ChartSearch
            | Self::ChartRead
            | Self::MemoryRead
            | Self::FollowUpList
            | Self::BriefingRead
            | Self::SkillView => true,
            Self::MemorySave
            | Self::FollowUpCreate
            | Self::MentalHealthLog
            | Self::LingoCgmIngest => false,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
struct ChartSearchArgs {
    #[serde(default)]
    query: Option<String>,
    #[serde(default, rename = "record_type")]
    _record_type: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct ChartReadArgs {
    #[serde(default)]
    candidate_ids: Vec<String>,
    #[serde(default)]
    candidate_id: Option<String>,
}

impl ChartReadArgs {
    fn into_ids(self) -> Vec<String> {
        let mut ids: Vec<String> = self
            .candidate_ids
            .into_iter()
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .collect();
        if ids.is_empty()
            && let Some(single) = self
                .candidate_id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
        {
            ids.push(single.to_owned());
        }
        ids
    }
}

#[derive(Debug, Deserialize)]
struct MemorySaveArgs {
    note: String,
}

#[derive(Debug, Deserialize)]
struct FollowUpCreateArgs {
    title: String,
    instructions: String,
    #[serde(default)]
    due_at: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct BriefingReadArgs {
    #[serde(default)]
    briefing_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct MentalHealthCheckInPayload {
    #[serde(default)]
    pub(crate) recorded_at: Option<String>,
    pub(crate) mood_1_to_10: u8,
    pub(crate) anxiety_1_to_10: u8,
    pub(crate) energy_1_to_10: u8,
    pub(crate) sleep_hours: f32,
    #[serde(default)]
    pub(crate) phq9_score: Option<u8>,
    #[serde(default)]
    pub(crate) gad7_score: Option<u8>,
    #[serde(default)]
    pub(crate) note: Option<String>,
}

impl MentalHealthCheckInPayload {
    pub(crate) fn into_checkin(self) -> MentalHealthCheckIn {
        let recorded_at = self
            .recorded_at
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(DEFAULT_DEMO_DATE)
            .to_owned();
        MentalHealthCheckIn {
            recorded_at,
            mood_1_to_10: self.mood_1_to_10,
            anxiety_1_to_10: self.anxiety_1_to_10,
            energy_1_to_10: self.energy_1_to_10,
            sleep_hours: self.sleep_hours,
            phq9_score: self.phq9_score,
            gad7_score: self.gad7_score,
            note: self.note.unwrap_or_default(),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct LingoCgmIngestPayload {
    #[serde(default)]
    pub(crate) summary_title: Option<String>,
    #[serde(default)]
    pub(crate) csv: Option<String>,
    #[serde(default)]
    pub(crate) readings: Option<Vec<LingoReading>>,
}

impl LingoCgmIngestPayload {
    pub(crate) fn resolve_readings(self) -> Result<(String, Vec<LingoReading>), LingoParseError> {
        let title = self
            .summary_title
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("Abbott Lingo CGM Summary")
            .to_owned();
        match (self.csv, self.readings) {
            (Some(csv), _) if !csv.trim().is_empty() => {
                let parsed = parse_lingo_csv(&csv)?;
                Ok((title, parsed))
            }
            (_, Some(readings)) => {
                if readings.is_empty() {
                    return Err(LingoParseError::EmptyInput);
                }
                for (idx, reading) in readings.iter().enumerate() {
                    reading.validate(idx + 1)?;
                }
                Ok((title, readings))
            }
            _ => Err(LingoParseError::EmptyInput),
        }
    }
}

pub(crate) fn mental_health_record_response(
    rec: &DemoSourceRecord,
    checkin: &MentalHealthCheckIn,
) -> Value {
    json!({
        "record_id": rec.id,
        "workspace_id": rec.workspace_id,
        "title": rec.title,
        "recorded_at": rec.recorded_at,
        "source_label": rec.source_label,
        "body": rec.body,
        "fields": rec.fields,
        "checkin": checkin,
    })
}

pub(crate) fn lingo_cgm_record_response(
    rec: &DemoSourceRecord,
    summary: &LingoCgmSummary,
) -> Value {
    json!({
        "record_id": rec.id,
        "workspace_id": rec.workspace_id,
        "title": rec.title,
        "recorded_at": rec.recorded_at,
        "source_label": rec.source_label,
        "body": rec.body,
        "fields": rec.fields,
        "summary": summary,
    })
}

/// Shared runtime dependencies needed to spawn `crates/harness` runs for a visitor session.
#[derive(Clone)]
pub(crate) struct HarnessRunner {
    pub(crate) bus: Arc<dyn Bus>,
    pub(crate) provider_backend: ProviderBackend,
    pub(crate) skills: Arc<Package>,
}

impl HarnessRunner {
    #[expect(
        clippy::too_many_arguments,
        reason = "internal async task handoff of session, workspace/chat identifiers, and prior history"
    )]
    pub(crate) fn spawn_exchange(
        &self,
        session: VisitorSession,
        ws_id: String,
        chat_id: String,
        work_item_id: String,
        run_id: String,
        question: String,
        prior_history: Vec<(String, String)>,
    ) {
        let runner = self.clone();
        let cancel = Cancellation::default();
        session.register_run(
            &ws_id,
            &work_item_id,
            ActiveRun {
                run_id: run_id.clone(),
                cancel: cancel.clone(),
            },
        );
        tokio::spawn(async move {
            runner
                .run_loop(
                    session,
                    ws_id,
                    chat_id,
                    work_item_id,
                    run_id,
                    question,
                    prior_history,
                    cancel,
                )
                .await;
        });
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "internal async task handoff of session, workspace/chat identifiers, and cancellation token"
    )]
    fn run_loop(
        self,
        session: VisitorSession,
        ws_id: String,
        chat_id: String,
        work_item_id: String,
        run_id: String,
        question: String,
        prior_history: Vec<(String, String)>,
        cancel: Cancellation,
    ) -> Pin<Box<dyn Future<Output = ()> + Send>> {
        Box::pin(async move {
            let outcome = self
                .execute_once(
                    &session,
                    &ws_id,
                    &chat_id,
                    &work_item_id,
                    &run_id,
                    &question,
                    &prior_history,
                    &cancel,
                )
                .await;
            if !session.is_current_run(&ws_id, &work_item_id, &run_id) {
                return;
            }

            match outcome {
                RunOutcome::Completed => {
                    session.clear_run_if_matches(&ws_id, &work_item_id, &run_id);
                    let maybe_next = pop_next_queued_turn(&session, &ws_id, &chat_id).await;
                    if let Ok(Some(next_turn)) = maybe_next {
                        let _ = self
                            .bus
                            .publish(BusEvent::WorkspaceChanged {
                                workspace_id: ws_id.clone(),
                            })
                            .await;
                        self.spawn_exchange(
                            session,
                            ws_id,
                            chat_id,
                            next_turn.work_item_id,
                            next_turn.run_id,
                            next_turn.question,
                            next_turn.prior_history,
                        );
                    }
                }
                RunOutcome::Held {
                    reason: HoldReason::Stopped,
                } => {
                    mark_exchange_stopped(&session, &ws_id, &chat_id, &work_item_id, &run_id).await;
                    session.clear_run_if_matches(&ws_id, &work_item_id, &run_id);
                    let _ = self
                        .bus
                        .publish(BusEvent::WorkspaceChanged {
                            workspace_id: ws_id,
                        })
                        .await;
                }
                RunOutcome::Failed
                | RunOutcome::Cancelled
                | RunOutcome::Held {
                    reason: HoldReason::Limit | HoldReason::ProviderBlocked,
                } => {
                    mark_exchange_failed(&session, &ws_id, &chat_id, &work_item_id, &run_id).await;
                    session.clear_run_if_matches(&ws_id, &work_item_id, &run_id);
                    let _ = self
                        .bus
                        .publish(BusEvent::WorkspaceChanged {
                            workspace_id: ws_id,
                        })
                        .await;
                }
            }
        })
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "carries owned identifiers for one harness exchange"
    )]
    async fn execute_once(
        &self,
        session: &VisitorSession,
        ws_id: &str,
        chat_id: &str,
        work_item_id: &str,
        run_id: &str,
        question: &str,
        prior_history: &[(String, String)],
        cancel: &Cancellation,
    ) -> RunOutcome {
        let mut provider = match self.provider_backend.instantiate() {
            Ok(p) => p,
            Err(err) => {
                tracing::warn!(%err, "failed instantiating demo provider");
                return RunOutcome::Failed;
            }
        };
        let recipe = chart_recipe(self.provider_backend.model_name(), &self.skills);
        let binding = recipe.bind(Some(harness::PromptCacheKey::scoped(
            "physician-demo-api",
            &[&session.physician_id, ws_id, chat_id],
        )));
        let mut items = Vec::new();
        for (q, a) in prior_history {
            if !q.trim().is_empty() {
                items.push(Item::Message {
                    role: "user".to_owned(),
                    text: q.clone(),
                });
            }
            if !a.trim().is_empty() {
                items.push(Item::assistant(a.clone()));
            }
        }
        items.push(Item::Message {
            role: "user".to_owned(),
            text: question.to_owned(),
        });
        let request = binding.request(items);
        let work = match request.freeze() {
            Ok(w) => w,
            Err(err) => {
                tracing::warn!(?err, "failed freezing harness request");
                return RunOutcome::Failed;
            }
        };
        let recovered = match work.recover(
            "attempt-1",
            Records {
                events: Vec::new(),
                allowance_after: 0,
            },
            Messages {
                sources: &[],
                partial: None,
            },
            Inputs::default(),
        ) {
            Ok(r) => r,
            Err(err) => {
                tracing::warn!(?err, "failed initializing harness work");
                return RunOutcome::Failed;
            }
        };

        let mut product = DemoHarnessProduct {
            session: session.clone(),
            bus: Arc::clone(&self.bus),
            skills: Arc::clone(&self.skills),
            expected_binding: binding,
            ws_id: ws_id.to_owned(),
            chat_id: chat_id.to_owned(),
            work_item_id: work_item_id.to_owned(),
            run_id: run_id.to_owned(),
            question: question.to_owned(),
            cancel: cancel.clone(),
            read_records: Vec::new(),
            assigned_citation_ids: Arc::new(std::sync::Mutex::new(Vec::new())),
            pending_calls: HashMap::new(),
            last_search_terms: search_terms_for_question(question),
            last_search_more: false,
            refused_unbound_citations: 0,
            live_writing_notified: false,
        };

        match recovered
            .run(&mut provider, &mut product, RunLimits::default(), cancel)
            .await
        {
            Ok(_) => RunOutcome::Completed,
            Err(Failure::Cancelled | Failure::NotCurrent(_)) => RunOutcome::Held {
                reason: HoldReason::Stopped,
            },
            Err(Failure::Limit(_) | Failure::ContextRefused) => RunOutcome::Held {
                reason: HoldReason::Limit,
            },
            Err(err) => {
                tracing::warn!(?err, %ws_id, %chat_id, %work_item_id, "harness run failed");
                RunOutcome::Failed
            }
        }
    }
}

pub(crate) fn chart_recipe(model: &str, skills: &Package) -> RequestRecipe {
    let mut instructions = String::from(
        "You are Coda, a clinical chart assistant helping a physician manage a synthetic patient workspace.\n\
         For clinical chart questions, first search the chart with `chart_search`, then read all relevant records with `chart_read` before answering.\n\
         When multiple records are read, cite EVERY relevant `citation_number` inline ([1], [2], [3], ...) beside each clinical claim.\n\
         Do not cite numbers not returned by `chart_read`, and do not fabricate clinical values absent from read records.\n\
         Use `memory_read` / `memory_save` to read or append durable physician notes in Patient Memory, `follow_up_list` / `follow_up_create` to inspect or schedule patient follow-ups, `briefing_read` to read pre-visit briefings, `mental_health_log` to record structured mental health check-ins (mood, anxiety, energy, sleep, PHQ-9, GAD-7), and `lingo_cgm_ingest` to ingest Abbott Lingo CGM glucose readings or CSV summaries.",
    );
    let catalog = skills.catalog_instructions();
    if !catalog.is_empty() {
        instructions.push('\n');
        instructions.push_str(&catalog);
    }
    RequestRecipe {
        model: model.to_owned(),
        context_window_tokens: 32_768,
        max_input_tokens: 24_576,
        options: Value::Null,
        instructions,
        tools: vec![
            chart_search_tool().function(),
            chart_read_tool().function(),
            memory_read_tool().function(),
            memory_save_tool().function(),
            follow_up_list_tool().function(),
            follow_up_create_tool().function(),
            briefing_read_tool().function(),
            mental_health_log_tool().function(),
            lingo_cgm_ingest_tool().function(),
            harness::skills::tool().function(),
        ],
    }
}

fn chart_search_tool() -> Tool {
    Tool {
        name: TOOL_CHART_SEARCH.to_owned(),
        description: "Search the active patient's SQLite chart records by clinical keywords and optional record group.".to_owned(),
        parameters: json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Clinical keywords to search for." },
                "record_type": { "type": "string", "description": "Optional record type filter." }
            },
            "required": ["query"],
            "additionalProperties": false
        }),
    }
}

fn chart_read_tool() -> Tool {
    Tool {
        name: TOOL_CHART_READ.to_owned(),
        description: "Read full clinical text for specific `candidate_id`s returned by `chart_search`. Each returned record carries a 1-based `citation_number` for inline [N] citations.".to_owned(),
        parameters: json!({
            "type": "object",
            "properties": {
                "candidate_ids": {
                    "type": "array",
                    "items": { "type": "string" },
                    "minItems": 1
                }
            },
            "required": ["candidate_ids"],
            "additionalProperties": false
        }),
    }
}

fn memory_read_tool() -> Tool {
    Tool {
        name: TOOL_MEMORY_READ.to_owned(),
        description: "Read the active patient's current durable Patient Memory (`summary` and physician `direction` notes).".to_owned(),
        parameters: json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        }),
    }
}

fn memory_save_tool() -> Tool {
    Tool {
        name: TOOL_MEMORY_SAVE.to_owned(),
        description: "Append a concise physician note to the active patient's durable Patient Memory without overwriting prior notes.".to_owned(),
        parameters: json!({
            "type": "object",
            "properties": {
                "note": {
                    "type": "string",
                    "description": "Non-empty physician clinical note or preference to persist in Patient Memory."
                }
            },
            "required": ["note"],
            "additionalProperties": false
        }),
    }
}

fn follow_up_list_tool() -> Tool {
    Tool {
        name: TOOL_FOLLOW_UP_LIST.to_owned(),
        description: "List the active patient's scheduled, open, and completed follow-up items."
            .to_owned(),
        parameters: json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        }),
    }
}

fn follow_up_create_tool() -> Tool {
    Tool {
        name: TOOL_FOLLOW_UP_CREATE.to_owned(),
        description: "Create a new scheduled follow-up item for the active patient with a concise title, clinical instructions, and optional due date.".to_owned(),
        parameters: json!({
            "type": "object",
            "properties": {
                "title": {
                    "type": "string",
                    "description": "Concise non-empty summary title for the follow-up."
                },
                "instructions": {
                    "type": "string",
                    "description": "Non-empty clinical instructions or monitoring plan for the follow-up."
                },
                "due_at": {
                    "type": "string",
                    "description": "Optional due date in YYYY-MM-DD or RFC 3339 format."
                }
            },
            "required": ["title", "instructions"],
            "additionalProperties": false
        }),
    }
}

fn briefing_read_tool() -> Tool {
    Tool {
        name: TOOL_BRIEFING_READ.to_owned(),
        description: "Read the active patient's saved pre-visit briefings (or a specific briefing by `briefing_id`).".to_owned(),
        parameters: json!({
            "type": "object",
            "properties": {
                "briefing_id": {
                    "type": "string",
                    "description": "Optional briefing identifier to read one briefing directly."
                }
            },
            "additionalProperties": false
        }),
    }
}

fn mental_health_log_tool() -> Tool {
    Tool {
        name: TOOL_MENTAL_HEALTH_LOG.to_owned(),
        description: "Record a structured mental health check-in for the active patient (mood, anxiety, energy on 1..=10, prior-night sleep hours, and optional PHQ-9 / GAD-7 scores) as a cited chart source record.".to_owned(),
        parameters: json!({
            "type": "object",
            "properties": {
                "recorded_at": {
                    "type": "string",
                    "description": "Optional ISO date or timestamp when the check-in occurred."
                },
                "mood_1_to_10": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 10,
                    "description": "Self-reported mood rating from 1 (lowest) to 10 (brightest)."
                },
                "anxiety_1_to_10": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 10,
                    "description": "Self-reported anxiety intensity from 1 (calm) to 10 (severe)."
                },
                "energy_1_to_10": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 10,
                    "description": "Self-reported daytime energy rating from 1 (exhausted) to 10 (energized)."
                },
                "sleep_hours": {
                    "type": "number",
                    "minimum": 0.0,
                    "maximum": 24.0,
                    "description": "Prior-night sleep duration in hours (0.0..=24.0)."
                },
                "phq9_score": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 27,
                    "description": "Optional PHQ-9 depression total score (0..=27)."
                },
                "gad7_score": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 21,
                    "description": "Optional GAD-7 anxiety total score (0..=21)."
                },
                "note": {
                    "type": "string",
                    "description": "Optional symptom, trigger, or context note."
                }
            },
            "required": ["mood_1_to_10", "anxiety_1_to_10", "energy_1_to_10", "sleep_hours"],
            "additionalProperties": false
        }),
    }
}

fn lingo_cgm_ingest_tool() -> Tool {
    Tool {
        name: TOOL_LINGO_CGM_INGEST.to_owned(),
        description: "Ingest Abbott Lingo continuous glucose monitoring (CGM) CSV rows or structured readings for the active patient, compute glycemic variability and Time in Range metrics, and file a cited chart source record.".to_owned(),
        parameters: json!({
            "type": "object",
            "properties": {
                "summary_title": {
                    "type": "string",
                    "description": "Optional title for the filed Abbott Lingo CGM summary record."
                },
                "csv": {
                    "type": "string",
                    "description": "Abbott Lingo CSV text (`timestamp,glucose_mg_dl[,lingo_count[,note]]`)."
                },
                "readings": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "timestamp": { "type": "string" },
                            "glucose_mg_dl": { "type": "number" },
                            "lingo_count": { "type": "number" },
                            "note": { "type": "string" }
                        },
                        "required": ["timestamp", "glucose_mg_dl"],
                        "additionalProperties": false
                    },
                    "description": "Structured Abbott Lingo glucose readings when CSV text is not supplied."
                }
            },
            "additionalProperties": false
        }),
    }
}

struct DemoHarnessProduct {
    session: VisitorSession,
    bus: Arc<dyn Bus>,
    skills: Arc<Package>,
    expected_binding: RequestBinding,
    ws_id: String,
    chat_id: String,
    work_item_id: String,
    run_id: String,
    question: String,
    cancel: Cancellation,
    read_records: Vec<DemoSourceRecord>,
    assigned_citation_ids: Arc<std::sync::Mutex<Vec<String>>>,
    pending_calls: HashMap<String, DemoTool>,
    last_search_terms: Vec<String>,
    last_search_more: bool,
    refused_unbound_citations: usize,
    live_writing_notified: bool,
}

impl DemoHarnessProduct {
    async fn with_exchange_mut(
        &self,
        f: impl FnOnce(&mut ChatSnapshot, usize) -> bool,
    ) -> Result<(), ProductError> {
        let _guard = self.session.store_lock.lock().await;
        let Some((title, created_at, mut snap)) = self
            .session
            .store
            .load_chat_snapshot_row(&self.ws_id, &self.chat_id)
            .map_err(|e| ProductError::Failed(e.to_string().into()))?
        else {
            return Err(NotCurrent::Withdrawn.into());
        };
        let Some(idx) = snap
            .transcript
            .iter()
            .position(|ex| ex.work_item_id.0 == self.work_item_id)
        else {
            return Err(NotCurrent::Withdrawn.into());
        };
        if matches!(snap.transcript[idx].answer, AnswerState::Stopped { .. })
            || self.cancel.is_cancelled()
            || !self
                .session
                .is_current_run(&self.ws_id, &self.work_item_id, &self.run_id)
        {
            return Err(NotCurrent::Withdrawn.into());
        }
        if f(&mut snap, idx) {
            self.session
                .store
                .save_chat_snapshot_row(&self.ws_id, &self.chat_id, &title, &created_at, &snap)
                .map_err(|e| ProductError::Failed(e.to_string().into()))?;
            drop(_guard);
            let _ = self
                .bus
                .publish(BusEvent::WorkspaceChanged {
                    workspace_id: self.ws_id.clone(),
                })
                .await;
        }
        Ok(())
    }

    async fn handle_tool_started(&mut self, call: &Invocation) -> Result<(), ProductError> {
        let Some(tool) = DemoTool::from_name(&call.name) else {
            return Ok(());
        };
        self.pending_calls.insert(call.id.clone(), tool);
        let args: Value = serde_json::from_str(&call.arguments).unwrap_or_else(|_| json!({}));
        match tool {
            DemoTool::ChartSearch => {
                let parsed: ChartSearchArgs = serde_json::from_value(args).unwrap_or_default();
                let query = parsed
                    .query
                    .filter(|q| !q.trim().is_empty())
                    .unwrap_or_else(|| self.question.clone());
                let terms = search_terms_for_question(&query);
                if !terms.is_empty() {
                    self.last_search_terms = terms;
                }
                self.with_exchange_mut(|snap, idx| {
                    let ExchangeSteps::Read(steps) = &mut snap.transcript[idx].steps else {
                        return false;
                    };
                    steps.push(Step::SearchedChart {
                        query,
                        group: None,
                        page: SearchPage::First,
                        state: StepState::Running,
                    });
                    true
                })
                .await?;
            }
            DemoTool::ChartRead => {
                let parsed: ChartReadArgs = serde_json::from_value(args).unwrap_or_default();
                let requested = parsed.into_ids().len().max(1);
                self.with_exchange_mut(|snap, idx| {
                    let ExchangeSteps::Read(steps) = &mut snap.transcript[idx].steps else {
                        return false;
                    };
                    steps.push(Step::ReadRecords {
                        requested,
                        state: StepState::Running,
                    });
                    true
                })
                .await?;
            }
            DemoTool::MemorySave => {
                self.with_exchange_mut(|snap, idx| {
                    let ExchangeSteps::Read(steps) = &mut snap.transcript[idx].steps else {
                        return false;
                    };
                    steps.push(Step::SavedMemoryNote {
                        state: StepState::Running,
                    });
                    true
                })
                .await?;
            }
            DemoTool::FollowUpCreate => {
                self.with_exchange_mut(|snap, idx| {
                    let ExchangeSteps::Read(steps) = &mut snap.transcript[idx].steps else {
                        return false;
                    };
                    steps.push(Step::CreatedFollowUp {
                        state: StepState::Running,
                    });
                    true
                })
                .await?;
            }
            DemoTool::MemoryRead
            | DemoTool::FollowUpList
            | DemoTool::BriefingRead
            | DemoTool::MentalHealthLog
            | DemoTool::LingoCgmIngest
            | DemoTool::SkillView => {}
        }
        Ok(())
    }

    async fn handle_tool_returned(
        &mut self,
        call_id: &str,
        result: &ToolResult,
    ) -> Result<(), ProductError> {
        let Some(tool) = self.pending_calls.remove(call_id) else {
            return Ok(());
        };
        if matches!(result, ToolResult::Refused(_)) {
            self.mark_running_step_failed(tool).await?;
            return Ok(());
        }
        let Some(output) = result.original() else {
            return Ok(());
        };
        match tool {
            DemoTool::ChartSearch => {
                let parsed: Value = serde_json::from_str(output).unwrap_or(Value::Null);
                let count = parsed
                    .get("candidates")
                    .and_then(Value::as_array)
                    .map_or(0, Vec::len);
                let more = self.last_search_more;
                self.with_exchange_mut(|snap, idx| {
                    let ExchangeSteps::Read(steps) = &mut snap.transcript[idx].steps else {
                        return false;
                    };
                    for step in steps.iter_mut().rev() {
                        if let Step::SearchedChart { state, .. } = step
                            && matches!(state, StepState::Running)
                        {
                            *state = StepState::Returned {
                                result: Found {
                                    records: count,
                                    more,
                                },
                            };
                            return true;
                        }
                    }
                    false
                })
                .await?;
            }
            DemoTool::ChartRead => {
                let parsed: Value = serde_json::from_str(output).unwrap_or(Value::Null);
                let Some(recs) = parsed.get("records").and_then(Value::as_array) else {
                    return Ok(());
                };
                let mut ids = Vec::new();
                for r in recs {
                    if let Some(id) = r.get("candidate_id").and_then(Value::as_str) {
                        ids.push(id.to_owned());
                    }
                }
                let fetched = {
                    let _guard = self.session.store_lock.lock().await;
                    self.session
                        .store
                        .read_records_by_ids(&self.ws_id, &ids)
                        .map_err(|e| ProductError::Failed(Box::new(e)))?
                };
                for rec in fetched {
                    if !self
                        .read_records
                        .iter()
                        .any(|existing| existing.id == rec.id)
                    {
                        self.read_records.push(rec);
                    }
                }
                let assigned = self
                    .assigned_citation_ids
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .clone();
                self.read_records.sort_by_key(|rec| {
                    assigned
                        .iter()
                        .position(|id| id == &rec.id)
                        .unwrap_or(usize::MAX)
                });
                let read_wires: Vec<RecordRead> = self
                    .read_records
                    .iter()
                    .map(|r| RecordRead::Whole {
                        version: SourceVersion {
                            candidate_id: r.id.clone(),
                            revision: "1".into(),
                        },
                        about: RecordAbout {
                            group: Some(parse_record_group_wire(&r.group)),
                            title: Some(r.title.clone()),
                            dates: vec![RecordDate {
                                role: Some(physician_wire::DateRole::Recorded),
                                value: r.recorded_at.clone(),
                            }],
                        },
                    })
                    .collect();
                self.with_exchange_mut(|snap, idx| {
                    let ExchangeSteps::Read(steps) = &mut snap.transcript[idx].steps else {
                        return false;
                    };
                    for step in steps.iter_mut().rev() {
                        if let Step::ReadRecords { state, .. } = step
                            && matches!(state, StepState::Running)
                        {
                            *state = StepState::Returned {
                                result: read_wires.clone(),
                            };
                            return true;
                        }
                    }
                    false
                })
                .await?;
            }
            DemoTool::MemorySave => {
                self.with_exchange_mut(|snap, idx| {
                    let ExchangeSteps::Read(steps) = &mut snap.transcript[idx].steps else {
                        return false;
                    };
                    for step in steps.iter_mut().rev() {
                        if let Step::SavedMemoryNote { state } = step
                            && matches!(state, StepState::Running)
                        {
                            *state = StepState::Returned { result: () };
                            return true;
                        }
                    }
                    false
                })
                .await?;
            }
            DemoTool::FollowUpCreate => {
                self.with_exchange_mut(|snap, idx| {
                    let ExchangeSteps::Read(steps) = &mut snap.transcript[idx].steps else {
                        return false;
                    };
                    for step in steps.iter_mut().rev() {
                        if let Step::CreatedFollowUp { state } = step
                            && matches!(state, StepState::Running)
                        {
                            *state = StepState::Returned { result: () };
                            return true;
                        }
                    }
                    false
                })
                .await?;
            }
            DemoTool::MemoryRead
            | DemoTool::FollowUpList
            | DemoTool::BriefingRead
            | DemoTool::MentalHealthLog
            | DemoTool::LingoCgmIngest
            | DemoTool::SkillView => {}
        }
        Ok(())
    }

    async fn mark_running_step_failed(&self, tool: DemoTool) -> Result<(), ProductError> {
        self.with_exchange_mut(|snap, idx| {
            let ExchangeSteps::Read(steps) = &mut snap.transcript[idx].steps else {
                return false;
            };
            for step in steps.iter_mut().rev() {
                match (tool, step) {
                    (DemoTool::ChartSearch, Step::SearchedChart { state, .. })
                        if matches!(state, StepState::Running) =>
                    {
                        *state = StepState::Failed;
                        return true;
                    }
                    (DemoTool::ChartRead, Step::ReadRecords { state, .. })
                        if matches!(state, StepState::Running) =>
                    {
                        *state = StepState::Failed;
                        return true;
                    }
                    (DemoTool::MemorySave, Step::SavedMemoryNote { state })
                        if matches!(state, StepState::Running) =>
                    {
                        *state = StepState::Failed;
                        return true;
                    }
                    (DemoTool::FollowUpCreate, Step::CreatedFollowUp { state })
                        if matches!(state, StepState::Running) =>
                    {
                        *state = StepState::Failed;
                        return true;
                    }
                    _ => {}
                }
            }
            false
        })
        .await
    }
}

#[async_trait]
impl Product for DemoHarnessProduct {
    async fn assert_current(&mut self) -> Result<(), ProductError> {
        if self.cancel.is_cancelled() {
            return Err(NotCurrent::Withdrawn.into());
        }
        let _guard = self.session.store_lock.lock().await;
        let Some((_, _, snap)) = self
            .session
            .store
            .load_chat_snapshot_row(&self.ws_id, &self.chat_id)
            .map_err(|e| ProductError::Failed(e.to_string().into()))?
        else {
            return Err(NotCurrent::Withdrawn.into());
        };
        let Some(ex) = snap
            .transcript
            .iter()
            .find(|ex| ex.work_item_id.0 == self.work_item_id)
        else {
            return Err(NotCurrent::Withdrawn.into());
        };
        if !matches!(ex.answer, AnswerState::Working { .. }) {
            return Err(NotCurrent::Withdrawn.into());
        }
        Ok(())
    }

    async fn message_sources(&mut self) -> Result<Option<Vec<ProductMessage>>, ProductError> {
        Ok(None)
    }

    async fn admit_binding(&mut self, binding: &RequestBinding) -> Result<(), ProductError> {
        if *binding != self.expected_binding {
            return Err(ProductError::Failed(
                "harness request binding drifted from accepted demo recipe".into(),
            ));
        }
        Ok(())
    }

    async fn persist(&mut self, event: Event) -> Result<(), ProductError> {
        match &event.kind {
            EventKind::ToolStarted(call) => self.handle_tool_started(call).await?,
            EventKind::ToolReturned { call_id, result } => {
                self.handle_tool_returned(call_id, result).await?;
            }
            _ => {}
        }
        Ok(())
    }

    async fn live_text(
        &mut self,
        _ordinal: u64,
        delta: TextDelta,
    ) -> Result<TextDelivery, ProductError> {
        if delta.text.is_empty() {
            return Ok(TextDelivery::Hidden);
        }
        if !self.live_writing_notified {
            self.live_writing_notified = true;
            self.with_exchange_mut(|snap, idx| {
                if let AnswerState::Working { stop, writing } = &snap.transcript[idx].answer
                    && !*writing
                {
                    snap.transcript[idx].answer = AnswerState::Working {
                        stop: stop.clone(),
                        writing: true,
                    };
                    return true;
                }
                false
            })
            .await?;
        }
        Ok(TextDelivery::Visible)
    }

    fn read_only(&self, name: &str) -> bool {
        DemoTool::from_name(name).is_some_and(DemoTool::is_read_only)
    }

    fn execute(&mut self, call: &Invocation, arguments: Value) -> ToolFuture {
        let name = call.name.clone();
        let session = self.session.clone();
        let bus = Arc::clone(&self.bus);
        let ws_id = self.ws_id.clone();
        let skills = Arc::clone(&self.skills);
        let default_query = self.question.clone();
        let assigned = Arc::clone(&self.assigned_citation_ids);
        Box::pin(async move {
            let Some(tool) = DemoTool::from_name(&name) else {
                return Ok(ToolOutcome::refused(format!(
                    "tool `{name}` is not authorized"
                )));
            };
            match tool {
                DemoTool::ChartSearch => {
                    let args: ChartSearchArgs = match serde_json::from_value(arguments) {
                        Ok(a) => a,
                        Err(err) => {
                            return Ok(ToolOutcome::refused(format!(
                                "invalid chart_search arguments: {err}"
                            )));
                        }
                    };
                    let query = args
                        .query
                        .as_deref()
                        .map(str::trim)
                        .filter(|q| !q.is_empty())
                        .unwrap_or(&default_query);
                    let terms = search_terms_for_question(query);
                    let _guard = session.store_lock.lock().await;
                    let found = session
                        .store
                        .search_records(&ws_id, &terms, 6)
                        .map_err(|e| ProductError::Failed(Box::new(e)))?;
                    let candidates: Vec<Value> = found
                        .iter()
                        .map(|r| {
                            let snippet: String = r.body.chars().take(180).collect();
                            json!({
                                "candidate_id": r.id,
                                "title": r.title,
                                "group": r.group,
                                "recorded_at": r.recorded_at,
                                "snippet": snippet,
                            })
                        })
                        .collect();
                    Ok(ToolOutcome::Returned(
                        json!({ "candidates": candidates }).to_string(),
                    ))
                }
                DemoTool::ChartRead => {
                    let args: ChartReadArgs = match serde_json::from_value(arguments) {
                        Ok(a) => a,
                        Err(err) => {
                            return Ok(ToolOutcome::refused(format!(
                                "invalid chart_read arguments: {err}"
                            )));
                        }
                    };
                    let ids = args.into_ids();
                    if ids.is_empty() {
                        return Ok(ToolOutcome::refused(
                            "chart_read requires at least one candidate_id",
                        ));
                    }
                    let records = {
                        let _guard = session.store_lock.lock().await;
                        session
                            .store
                            .read_records_by_ids(&ws_id, &ids)
                            .map_err(|e| ProductError::Failed(Box::new(e)))?
                    };
                    if records.is_empty() {
                        return Ok(ToolOutcome::refused(
                            "no matching chart records found for the requested candidate_ids",
                        ));
                    }
                    let encoded_records: Vec<Value> = {
                        let mut list = assigned
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        records
                            .iter()
                            .map(|r| {
                                let citation_number = match list.iter().position(|id| id == &r.id) {
                                    Some(idx) => idx + 1,
                                    None => {
                                        list.push(r.id.clone());
                                        list.len()
                                    }
                                };
                                json!({
                                    "citation_number": citation_number,
                                    "candidate_id": r.id,
                                    "title": r.title,
                                    "group": r.group,
                                    "recorded_at": r.recorded_at,
                                    "source_label": r.source_label,
                                    "body": r.body,
                                })
                            })
                            .collect()
                    };
                    Ok(ToolOutcome::Returned(
                        json!({ "records": encoded_records }).to_string(),
                    ))
                }
                DemoTool::MemoryRead => {
                    let _guard = session.store_lock.lock().await;
                    let memory = match session.store.patient_memory(&ws_id).await {
                        Ok(m) => m,
                        Err(DbError::NotFound { kind, id }) => {
                            return Ok(ToolOutcome::refused(format!("{kind} `{id}` not found")));
                        }
                        Err(err) => return Err(ProductError::Failed(Box::new(err))),
                    };
                    let encoded = serde_json::to_string(&memory)
                        .map_err(|e| ProductError::Failed(Box::new(e)))?;
                    Ok(ToolOutcome::Returned(encoded))
                }
                DemoTool::MemorySave => {
                    let args: MemorySaveArgs = match serde_json::from_value(arguments) {
                        Ok(a) => a,
                        Err(err) => {
                            return Ok(ToolOutcome::refused(format!(
                                "invalid memory_save arguments: {err}"
                            )));
                        }
                    };
                    let note = args.note.trim();
                    if note.is_empty() {
                        return Ok(ToolOutcome::refused(
                            "memory_save requires a non-empty `note`",
                        ));
                    }
                    let saved = {
                        let _guard = session.store_lock.lock().await;
                        match session.store.save_patient_memory(&ws_id, note).await {
                            Ok(s) => s,
                            Err(DbError::InvalidCommand(msg)) => {
                                return Ok(ToolOutcome::refused(msg));
                            }
                            Err(DbError::NotFound { kind, id }) => {
                                return Ok(ToolOutcome::refused(format!(
                                    "{kind} `{id}` not found"
                                )));
                            }
                            Err(err) => return Err(ProductError::Failed(Box::new(err))),
                        }
                    };
                    let _ = bus
                        .publish(BusEvent::WorkspaceChanged {
                            workspace_id: ws_id.clone(),
                        })
                        .await;
                    Ok(ToolOutcome::Returned(
                        json!({
                            "saved": true,
                            "note_number": saved.note_number,
                            "log_path": saved.log_path,
                            "saved_at": saved.saved_at,
                            "note": note,
                        })
                        .to_string(),
                    ))
                }
                DemoTool::FollowUpList => {
                    let _guard = session.store_lock.lock().await;
                    let follow_ups = match session.store.patient_follow_ups(&ws_id).await {
                        Ok(f) => f,
                        Err(DbError::NotFound { kind, id }) => {
                            return Ok(ToolOutcome::refused(format!("{kind} `{id}` not found")));
                        }
                        Err(err) => return Err(ProductError::Failed(Box::new(err))),
                    };
                    let encoded = serde_json::to_string(&follow_ups)
                        .map_err(|e| ProductError::Failed(Box::new(e)))?;
                    Ok(ToolOutcome::Returned(encoded))
                }
                DemoTool::FollowUpCreate => {
                    let args: FollowUpCreateArgs = match serde_json::from_value(arguments) {
                        Ok(a) => a,
                        Err(err) => {
                            return Ok(ToolOutcome::refused(format!(
                                "invalid follow_up_create arguments: {err}"
                            )));
                        }
                    };
                    let title = args.title.trim();
                    let instructions = args.instructions.trim();
                    if title.is_empty() || instructions.is_empty() {
                        return Ok(ToolOutcome::refused(
                            "follow_up_create requires non-empty `title` and `instructions`",
                        ));
                    }
                    let due_date = args
                        .due_at
                        .as_deref()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .unwrap_or(DEFAULT_DEMO_FOLLOW_UP_DUE_DATE);
                    let created = {
                        let _guard = session.store_lock.lock().await;
                        let idem = format!("tool-fu-{}", next_demo_seq());
                        let initial = match session
                            .store
                            .create_follow_up(&ws_id, title, due_date, &idem)
                            .await
                        {
                            Ok(c) => c,
                            Err(DbError::InvalidCommand(msg)) => {
                                return Ok(ToolOutcome::refused(msg));
                            }
                            Err(DbError::NotFound { kind, id }) => {
                                return Ok(ToolOutcome::refused(format!(
                                    "{kind} `{id}` not found"
                                )));
                            }
                            Err(err) => return Err(ProductError::Failed(Box::new(err))),
                        };
                        match session
                            .store
                            .update_follow_up(
                                &ws_id,
                                &initial.follow_up.follow_up_id,
                                &UpdateFollowUpBody {
                                    summary: None,
                                    due_date: None,
                                    note: Some(instructions.to_owned()),
                                    idempotency_key: format!("tool-fu-note-{}", next_demo_seq()),
                                },
                            )
                            .await
                        {
                            Ok(updated) => updated,
                            Err(err) => return Err(ProductError::Failed(Box::new(err))),
                        }
                    };
                    let _ = bus
                        .publish(BusEvent::WorkspaceChanged {
                            workspace_id: ws_id.clone(),
                        })
                        .await;
                    let encoded = serde_json::to_string(&created.follow_up)
                        .map_err(|e| ProductError::Failed(Box::new(e)))?;
                    Ok(ToolOutcome::Returned(encoded))
                }
                DemoTool::BriefingRead => {
                    let args: BriefingReadArgs = match serde_json::from_value(arguments) {
                        Ok(a) => a,
                        Err(err) => {
                            return Ok(ToolOutcome::refused(format!(
                                "invalid briefing_read arguments: {err}"
                            )));
                        }
                    };
                    let _guard = session.store_lock.lock().await;
                    if let Some(id) = args
                        .briefing_id
                        .as_deref()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                    {
                        let briefing = match session.store.visit_briefing(&ws_id, id).await {
                            Ok(b) => b,
                            Err(DbError::NotFound { kind, id }) => {
                                return Ok(ToolOutcome::refused(format!(
                                    "{kind} `{id}` not found"
                                )));
                            }
                            Err(err) => return Err(ProductError::Failed(Box::new(err))),
                        };
                        let encoded = serde_json::to_string(&briefing)
                            .map_err(|e| ProductError::Failed(Box::new(e)))?;
                        return Ok(ToolOutcome::Returned(encoded));
                    }
                    let summary = match session.store.workspace_briefings(&ws_id).await {
                        Ok(s) => s,
                        Err(DbError::NotFound { kind, id }) => {
                            return Ok(ToolOutcome::refused(format!("{kind} `{id}` not found")));
                        }
                        Err(err) => return Err(ProductError::Failed(Box::new(err))),
                    };
                    let mut details = Vec::with_capacity(summary.briefings.len());
                    for item in &summary.briefings {
                        if let Ok(full) = session
                            .store
                            .visit_briefing(&ws_id, &item.briefing_id)
                            .await
                        {
                            details.push(full.briefing);
                        }
                    }
                    Ok(ToolOutcome::Returned(
                        json!({
                            "briefings": summary.briefings,
                            "details": details,
                        })
                        .to_string(),
                    ))
                }
                DemoTool::MentalHealthLog => {
                    let payload: MentalHealthCheckInPayload =
                        match serde_json::from_value(arguments) {
                            Ok(p) => p,
                            Err(err) => {
                                return Ok(ToolOutcome::refused(format!(
                                    "invalid mental_health_log arguments: {err}"
                                )));
                            }
                        };
                    let checkin = payload.into_checkin();
                    if let Err(err) = checkin.validate() {
                        return Ok(ToolOutcome::refused(err.to_string()));
                    }
                    let rec = {
                        let _guard = session.store_lock.lock().await;
                        match session
                            .store
                            .log_mental_health_checkin(&ws_id, &checkin)
                            .await
                        {
                            Ok(r) => r,
                            Err(DbError::InvalidCommand(msg)) => {
                                return Ok(ToolOutcome::refused(msg));
                            }
                            Err(DbError::NotFound { kind, id }) => {
                                return Ok(ToolOutcome::refused(format!(
                                    "{kind} `{id}` not found"
                                )));
                            }
                            Err(err) => return Err(ProductError::Failed(Box::new(err))),
                        }
                    };
                    let _ = bus
                        .publish(BusEvent::WorkspaceChanged {
                            workspace_id: ws_id.clone(),
                        })
                        .await;
                    Ok(ToolOutcome::Returned(
                        mental_health_record_response(&rec, &checkin).to_string(),
                    ))
                }
                DemoTool::LingoCgmIngest => {
                    let payload: LingoCgmIngestPayload = match serde_json::from_value(arguments) {
                        Ok(p) => p,
                        Err(err) => {
                            return Ok(ToolOutcome::refused(format!(
                                "invalid lingo_cgm_ingest arguments: {err}"
                            )));
                        }
                    };
                    let (title, readings) = match payload.resolve_readings() {
                        Ok(resolved) => resolved,
                        Err(err) => return Ok(ToolOutcome::refused(err.to_string())),
                    };
                    let (rec, summary) = {
                        let _guard = session.store_lock.lock().await;
                        match session
                            .store
                            .ingest_lingo_cgm(&ws_id, &title, &readings)
                            .await
                        {
                            Ok(res) => res,
                            Err(DbError::InvalidCommand(msg)) => {
                                return Ok(ToolOutcome::refused(msg));
                            }
                            Err(DbError::NotFound { kind, id }) => {
                                return Ok(ToolOutcome::refused(format!(
                                    "{kind} `{id}` not found"
                                )));
                            }
                            Err(err) => return Err(ProductError::Failed(Box::new(err))),
                        }
                    };
                    let _ = bus
                        .publish(BusEvent::WorkspaceChanged {
                            workspace_id: ws_id.clone(),
                        })
                        .await;
                    Ok(ToolOutcome::Returned(
                        lingo_cgm_record_response(&rec, &summary).to_string(),
                    ))
                }
                DemoTool::SkillView => match skills.execute_skill_view(&arguments) {
                    Ok(view) => Ok(ToolOutcome::Returned(view.to_json())),
                    Err(err) => Ok(ToolOutcome::refused(err.to_string())),
                },
            }
        })
    }

    fn read_result(&mut self, _call_id: &str) -> ToolFuture<Option<String>> {
        Box::pin(async { Ok(None) })
    }

    async fn finish(&mut self, turn: &Turn) -> Result<Finish, ProductError> {
        let raw_text = turn.text();
        let trimmed = raw_text.trim();
        if trimmed.is_empty() {
            return Ok(Finish::Continue(json!({
                "error": "Answer text cannot be empty. Provide a concise grounded response."
            })));
        }

        let max_valid = self.read_records.len();
        let markers = parse_citation_markers(trimmed);
        let has_unbound = markers
            .iter()
            .any(|&(_, _, num)| num == 0 || num > max_valid);
        if has_unbound && self.refused_unbound_citations < 1 {
            self.refused_unbound_citations += 1;
            return Ok(Finish::Continue(json!({
                "error": format!(
                    "Only citation numbers 1..={max_valid} correspond to records read in this run. Remove invalid [N] markers."
                )
            })));
        }

        let (segments, basis, cited_sources) =
            build_segments_and_citations(trimmed, &self.read_records, &self.last_search_terms);

        let _guard = self.session.store_lock.lock().await;
        let Some((title, created_at, mut snap)) = self
            .session
            .store
            .load_chat_snapshot_row(&self.ws_id, &self.chat_id)
            .map_err(|e| ProductError::Failed(e.to_string().into()))?
        else {
            return Err(NotCurrent::Withdrawn.into());
        };
        let Some(idx) = snap
            .transcript
            .iter()
            .position(|ex| ex.work_item_id.0 == self.work_item_id)
        else {
            return Err(NotCurrent::Withdrawn.into());
        };
        if matches!(snap.transcript[idx].answer, AnswerState::Stopped { .. })
            || self.cancel.is_cancelled()
            || !self
                .session
                .is_current_run(&self.ws_id, &self.work_item_id, &self.run_id)
        {
            return Err(NotCurrent::Withdrawn.into());
        }

        for (k, v) in cited_sources {
            snap.sources.insert(k, v);
        }
        let seq = next_demo_seq();
        let ex = &mut snap.transcript[idx];
        ex.answer = AnswerState::Finished {
            response_id: ResponseId(format!("resp-live-{seq}")),
            attempt_id: AttemptId(self.run_id.clone()),
            finished_at: SavedAt(i64::try_from(unix_now()).unwrap_or(DEFAULT_DEMO_BASE_UNIX)),
            text: trimmed.to_owned(),
            feedback: None,
            segments,
            basis: (!basis.is_empty()).then_some(basis),
            newer_records: false,
        };
        if idx > 0
            && let ExchangeSteps::Read(steps) = &mut ex.steps
            && !steps
                .iter()
                .any(|s| matches!(s, Step::ReadConversation { .. }))
        {
            steps.push(Step::ReadConversation {
                state: StepState::Returned { result: () },
            });
        }

        self.session
            .store
            .save_chat_snapshot_row(&self.ws_id, &self.chat_id, &title, &created_at, &snap)
            .map_err(|e| ProductError::Failed(e.to_string().into()))?;
        drop(_guard);

        let _ = self
            .bus
            .publish(BusEvent::WorkspaceChanged {
                workspace_id: self.ws_id.clone(),
            })
            .await;
        let _ = self
            .bus
            .publish(BusEvent::WorkspaceSettled {
                workspace_id: self.ws_id.clone(),
            })
            .await;
        Ok(Finish::Committed)
    }

    async fn finish_partial(
        &mut self,
        _turn: &Turn,
        _backstops: &[LimitBackstop],
    ) -> Result<(), ProductError> {
        Ok(())
    }

    async fn window_seed(&mut self) -> Result<WindowSeed, ProductError> {
        Ok(WindowSeed {
            items: Vec::new(),
            note_references: Vec::new(),
        })
    }
}

fn parse_citation_markers(text: &str) -> Vec<(usize, usize, usize)> {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'[' {
            let mut j = i + 1;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            if j > i + 1
                && j < bytes.len()
                && bytes[j] == b']'
                && let Ok(num) = text[i + 1..j].parse::<usize>()
            {
                found.push((i, j + 1, num));
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
    found
}

fn build_segments_and_citations(
    text: &str,
    read_records: &[DemoSourceRecord],
    terms: &[String],
) -> (
    Vec<MessageSegment>,
    Vec<MessageSegment>,
    BTreeMap<String, CitedSourceWire>,
) {
    if read_records.is_empty() {
        return (
            vec![MessageSegment::Text {
                text: text.to_owned(),
            }],
            Vec::new(),
            BTreeMap::new(),
        );
    }

    let markers = parse_citation_markers(text);
    let mut segments = Vec::new();
    let mut sources = BTreeMap::new();
    let mut basis_cited = Vec::new();
    let mut key_for_number = BTreeMap::<usize, (String, String, String, CitationTargetKind)>::new();
    let mut cursor = 0usize;

    for (start, end, num) in markers {
        if start > cursor {
            segments.push(MessageSegment::Text {
                text: text[cursor..start].to_owned(),
            });
        }
        if num >= 1 && num <= read_records.len() {
            let rec = &read_records[num - 1];
            let (key, label, quote, target) = key_for_number
                .entry(num)
                .or_insert_with(|| {
                    let key = format!("live-{}-{num}", next_demo_seq());
                    let label = format!("{} ({})", rec.title, rec.recorded_at);
                    let quote = extract_verbatim_quote(&rec.body, terms);
                    let target = if quote.is_empty() {
                        CitationTargetKind::WholeRecord
                    } else {
                        CitationTargetKind::Passage
                    };
                    let cited = build_cited_source_wire(&key, num, rec, quote.clone());
                    sources.insert(key.clone(), cited);
                    basis_cited.push((key.clone(), num, label.clone(), quote.clone(), target));
                    (key, label, quote, target)
                })
                .clone();
            segments.push(MessageSegment::Citation {
                source_key: key,
                number: num,
                label,
                quote,
                target,
            });
        }
        cursor = end;
    }
    if cursor < text.len() {
        segments.push(MessageSegment::Text {
            text: text[cursor..].to_owned(),
        });
    }
    if segments.is_empty() {
        segments.push(MessageSegment::Text {
            text: text.to_owned(),
        });
    }

    let mut basis = Vec::new();
    if !basis_cited.is_empty() {
        basis.push(MessageSegment::Text {
            text: format!(
                "Basis and limitations: grounded in {} retrieved SQLite chart record(s) (",
                basis_cited.len()
            ),
        });
        for (idx, (key, number, label, quote, target)) in basis_cited.into_iter().enumerate() {
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
            text: "). Covers synthetic EHR records retrieved during this run only.".into(),
        });
    }
    (segments, basis, sources)
}

pub(crate) fn extract_prior_history(snap: &ChatSnapshot) -> Vec<(String, String)> {
    let mut history = Vec::new();
    for ex in &snap.transcript {
        if let AnswerState::Finished { text, .. } = &ex.answer {
            let q = ex
                .messages
                .iter()
                .map(|m| m.text.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            history.push((q, text.clone()));
        }
    }
    history
}

pub(crate) fn create_working_exchange(
    work_item_id: String,
    msg_id: String,
    question: String,
    ts: i64,
) -> Exchange {
    Exchange {
        work_item_id: WorkItemId(work_item_id.clone()),
        messages: vec![PhysicianMessage {
            id: MessageKey(msg_id),
            text: question,
            saved_at: SavedAt(ts),
        }],
        steps: ExchangeSteps::Read(Vec::new()),
        kept: Vec::new(),
        answer: AnswerState::Working {
            stop: WorkStopTarget {
                work_item_id: WorkItemId(work_item_id),
            },
            writing: false,
        },
    }
}

struct NextQueuedTurn {
    work_item_id: String,
    run_id: String,
    question: String,
    prior_history: Vec<(String, String)>,
}

async fn pop_next_queued_turn(
    session: &VisitorSession,
    ws_id: &str,
    chat_id: &str,
) -> Result<Option<NextQueuedTurn>, String> {
    let _guard = session.store_lock.lock().await;
    let Some((title, created_at, mut snap)) = session
        .store
        .load_chat_snapshot_row(ws_id, chat_id)
        .map_err(|e| e.to_string())?
    else {
        return Ok(None);
    };
    if snap.queued.is_empty() {
        return Ok(None);
    }
    if snap
        .transcript
        .iter()
        .any(|ex| matches!(ex.answer, AnswerState::Working { .. }))
    {
        return Ok(None);
    }
    let next_msg = snap.queued.remove(0);
    let prior_history = extract_prior_history(&snap);
    let seq = next_demo_seq();
    let work_item_id = format!("wi-live-{seq}");
    let run_id = format!("run-live-{seq}");
    snap.transcript.push(create_working_exchange(
        work_item_id.clone(),
        next_msg.id.0,
        next_msg.text.clone(),
        next_msg.saved_at.0,
    ));
    session
        .store
        .save_chat_snapshot_row(ws_id, chat_id, &title, &created_at, &snap)
        .map_err(|e| e.to_string())?;
    Ok(Some(NextQueuedTurn {
        work_item_id,
        run_id,
        question: next_msg.text,
        prior_history,
    }))
}

pub(crate) fn close_running_steps(steps: &mut ExchangeSteps, stopped: bool) {
    let ExchangeSteps::Read(items) = steps else {
        return;
    };
    let terminal = if stopped {
        StepState::Stopped
    } else {
        StepState::Failed
    };
    for step in items {
        match step {
            Step::SearchedChart { state, .. } if matches!(state, StepState::Running) => {
                *state = if stopped {
                    StepState::Stopped
                } else {
                    StepState::Failed
                };
            }
            Step::ReadRecords { state, .. } if matches!(state, StepState::Running) => {
                *state = if stopped {
                    StepState::Stopped
                } else {
                    StepState::Failed
                };
            }
            Step::ReadConversation { state }
            | Step::SavedMemoryNote { state }
            | Step::EditedMemory { state }
            | Step::CreatedFollowUp { state }
                if matches!(state, StepState::Running) =>
            {
                *state = terminal.clone();
            }
            Step::SearchedChart { .. }
            | Step::ReadRecords { .. }
            | Step::ReadConversation { .. }
            | Step::SavedMemoryNote { .. }
            | Step::EditedMemory { .. }
            | Step::CreatedFollowUp { .. } => {}
        }
    }
}

async fn mark_exchange_stopped(
    session: &VisitorSession,
    ws_id: &str,
    chat_id: &str,
    work_item_id: &str,
    run_id: &str,
) {
    update_terminal_hold(session, ws_id, chat_id, work_item_id, run_id, true).await;
}

async fn mark_exchange_failed(
    session: &VisitorSession,
    ws_id: &str,
    chat_id: &str,
    work_item_id: &str,
    run_id: &str,
) {
    update_terminal_hold(session, ws_id, chat_id, work_item_id, run_id, false).await;
}

async fn update_terminal_hold(
    session: &VisitorSession,
    ws_id: &str,
    chat_id: &str,
    work_item_id: &str,
    run_id: &str,
    stopped: bool,
) {
    let _guard = session.store_lock.lock().await;
    let Ok(Some((title, created_at, mut snap))) =
        session.store.load_chat_snapshot_row(ws_id, chat_id)
    else {
        return;
    };
    let Some(ex) = snap
        .transcript
        .iter_mut()
        .find(|ex| ex.work_item_id.0 == work_item_id)
    else {
        return;
    };
    if !matches!(ex.answer, AnswerState::Working { .. }) {
        return;
    }
    close_running_steps(&mut ex.steps, stopped);
    let resume = Some(WorkResumeTarget {
        work_item_id: WorkItemId(work_item_id.to_owned()),
        expected_run_id: run_id.to_owned(),
    });
    ex.answer = if stopped {
        AnswerState::Stopped { resume }
    } else {
        AnswerState::CouldntFinish { try_again: resume }
    };
    let _ = session
        .store
        .save_chat_snapshot_row(ws_id, chat_id, &title, &created_at, &snap);
}

/// Runs a single provider completion to revise a patient reply draft using `DemoStore` context.
pub(crate) async fn run_reply_revision_with_provider(
    backend: &ProviderBackend,
    session: &VisitorSession,
    ws_id: &str,
    reply_work_id: &str,
    instruction: &str,
    expected_rev: u64,
) -> Result<u64, String> {
    let existing = {
        let _guard = session.store_lock.lock().await;
        session
            .store
            .read_reply_draft(ws_id, reply_work_id)
            .await
            .map_err(|e| e.to_string())?
    };
    let mut provider = backend.instantiate()?;
    let prompt = format!(
        "Revise the following draft physician reply according to the physician's instruction.\n\
         Output ONLY the revised reply text.\n\n\
         Current draft:\n{}\n\nInstruction:\n{}",
        existing.text,
        instruction.trim()
    );
    let recipe = RequestRecipe {
        model: backend.model_name().to_owned(),
        context_window_tokens: 16_384,
        max_input_tokens: 12_288,
        options: Value::Null,
        instructions: "You revise clinical patient reply drafts concisely and accurately."
            .to_owned(),
        tools: Vec::new(),
    };
    let binding = recipe.bind(None);
    let request = binding.request(vec![Item::Message {
        role: "user".to_owned(),
        text: prompt,
    }]);
    let mut noop_obs = |_: harness::engine::recipe::StreamEvent| Ok(());
    let turn = harness::engine::recipe::Provider::exchange(
        &mut provider,
        &request,
        &Cancellation::default(),
        &RunLimits::default(),
        &mut noop_obs,
    )
    .await
    .map_err(|e| e.to_string())?;
    let revised = turn.text().trim().to_owned();
    let final_text = if revised.is_empty() {
        format!(
            "{}\n\n[Updated per physician note: {}]",
            existing.text,
            instruction.trim()
        )
    } else {
        revised
    };
    let _guard = session.store_lock.lock().await;
    session
        .store
        .apply_reply_revision(ws_id, reply_work_id, expected_rev, final_text)
        .await
        .map_err(|e| e.to_string())
}

pub(crate) fn default_demo_timestamp(seq: u64) -> i64 {
    let now = i64::try_from(unix_now()).unwrap_or(DEFAULT_DEMO_BASE_UNIX);
    if now > DEFAULT_DEMO_BASE_UNIX {
        now
    } else {
        DEFAULT_DEMO_BASE_UNIX + i64::try_from(seq).unwrap_or(1)
    }
}

pub(crate) fn fallback_title(question: &str) -> String {
    let trimmed = question.trim();
    if trimmed.chars().count() <= 48 {
        trimmed.to_owned()
    } else {
        format!("{}…", trimmed.chars().take(47).collect::<String>())
    }
}
