//! `crates/harness` [`Product`] implementation over per-session `turso_core` [`DemoStore`].

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use harness::engine::recipe::{Finish, RequestBinding, RequestRecipe, TextDelivery, ToolFuture};
use harness::engine::recovery::{Inputs, Messages, Records};
use harness::engine::runs::{HoldReason, RunOutcome};
use harness::engine::{
    Cancellation, Event, EventKind, Failure, Invocation, Item, LimitBackstop, NotCurrent, Product,
    ProductError, ProductMessage, RunLimits, TextDelta, Tool, ToolOutcome, Turn, WindowSeed,
};
use harness::skills::Package;
use physician_bus::{Bus, Event as BusEvent};
use physician_demo_db::DemoSourceRecord;
use physician_demo_db::store::{
    DEFAULT_DEMO_BASE_UNIX, build_cited_source_wire, extract_verbatim_quote, next_demo_seq,
    parse_record_group_wire, search_terms_for_question,
};
use physician_wire::{
    AnswerState, AttemptId, ChatSnapshot, CitationTargetKind, CitedSourceWire, Exchange,
    ExchangeSteps, Found, MessageKey, MessageSegment, PhysicianMessage, RecordAbout, RecordDate,
    RecordRead, ResponseId, SavedAt, SearchPage, SourceVersion, Step, StepState, WorkItemId,
    WorkResumeTarget, WorkStopTarget,
};
use serde_json::{Value, json};

use crate::provider::ProviderBackend;
use crate::sessions::{ActiveRun, VisitorSession, unix_now};

pub(crate) const TOOL_CHART_SEARCH: &str = "chart_search";
pub(crate) const TOOL_CHART_READ: &str = "chart_read";

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
        "You are Coda, a clinical chart assistant helping a physician review a synthetic patient chart.\n\
         First search the chart with `chart_search`, then read the relevant records with `chart_read` before answering.\n\
         Cite every retrieved clinical claim inline using [1], [2], etc. corresponding to the `citation_number` returned by `chart_read`.\n\
         Do not cite numbers not returned by `chart_read`, and do not fabricate clinical values absent from read records.",
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
                "record_type": { "type": ["string", "null"], "description": "Optional record type filter." }
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
        let args: Value = serde_json::from_str(&call.arguments).unwrap_or_else(|_| json!({}));
        match call.name.as_str() {
            TOOL_CHART_SEARCH => {
                let query = args
                    .get("query")
                    .and_then(Value::as_str)
                    .unwrap_or(&self.question)
                    .to_owned();
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
            TOOL_CHART_READ => {
                let requested = extract_candidate_ids(&args).len().max(1);
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
            _ => {}
        }
        Ok(())
    }

    async fn handle_tool_returned(&mut self, output: &str) -> Result<(), ProductError> {
        let parsed: Value = serde_json::from_str(output).unwrap_or(Value::Null);
        if let Some(cands) = parsed.get("candidates").and_then(Value::as_array) {
            let count = cands.len();
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
        } else if let Some(recs) = parsed.get("records").and_then(Value::as_array) {
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
                    .map_err(|e| ProductError::Failed(e.to_string().into()))?
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
        Ok(())
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
            EventKind::ToolReturned { result, .. } => {
                if let Some(original) = result.original() {
                    self.handle_tool_returned(original).await?;
                }
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
        matches!(
            name,
            TOOL_CHART_SEARCH | TOOL_CHART_READ | harness::skills::SKILL_VIEW
        )
    }

    fn execute(&mut self, call: &Invocation, arguments: Value) -> ToolFuture {
        let name = call.name.clone();
        let session = self.session.clone();
        let ws_id = self.ws_id.clone();
        let skills = Arc::clone(&self.skills);
        let default_query = self.question.clone();
        let assigned = Arc::clone(&self.assigned_citation_ids);
        Box::pin(async move {
            match name.as_str() {
                TOOL_CHART_SEARCH => {
                    let query = arguments
                        .get("query")
                        .and_then(Value::as_str)
                        .unwrap_or(&default_query);
                    let terms = search_terms_for_question(query);
                    let _guard = session.store_lock.lock().await;
                    let found = session
                        .store
                        .search_records(&ws_id, &terms, 6)
                        .map_err(|e| ProductError::Failed(e.to_string().into()))?;
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
                TOOL_CHART_READ => {
                    let ids = extract_candidate_ids(&arguments);
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
                            .map_err(|e| ProductError::Failed(e.to_string().into()))?
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
                harness::skills::SKILL_VIEW => match skills.execute_skill_view(&arguments) {
                    Ok(view) => Ok(ToolOutcome::Returned(view.to_json())),
                    Err(err) => Ok(ToolOutcome::refused(err.to_string())),
                },
                _ => Ok(ToolOutcome::refused(format!(
                    "tool `{name}` is not authorized"
                ))),
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

fn extract_candidate_ids(arguments: &Value) -> Vec<String> {
    if let Some(arr) = arguments.get("candidate_ids").and_then(Value::as_array) {
        return arr
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();
    }
    if let Some(single) = arguments
        .get("candidate_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return vec![single.to_owned()];
    }
    Vec::new()
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
    if let ExchangeSteps::Read(steps) = &mut ex.steps {
        for step in steps {
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
                    if matches!(state, StepState::Running) =>
                {
                    *state = if stopped {
                        StepState::Stopped
                    } else {
                        StepState::Failed
                    };
                }
                _ => {}
            }
        }
    }
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
