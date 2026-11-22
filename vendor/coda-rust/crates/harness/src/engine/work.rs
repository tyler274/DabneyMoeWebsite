//! Model, tool and completion protocol for a product's accepted work.
use super::*;
use recipe::{Finish, Progress, Provider, Request};
use tokio::task::JoinSet;

// Observe steering and revoked authority promptly, without checking each text fragment.
// Stop and steering are observed within one second while work is waiting or streaming.
pub(super) const CURRENCY_INTERVAL: Duration = Duration::from_secs(1);
// The shared specification permits eight concurrent read-only calls.
const MAX_PARALLEL_READS: usize = 8;

// Start storage recovery quickly, then avoid hammering a recovering database.
const PERSIST_RETRY_START: Duration = Duration::from_millis(100);
const PERSIST_RETRY_MAX: Duration = Duration::from_secs(2);
// Quiet provider failures retry with backoff.
const PROVIDER_RETRY_START: Duration = Duration::from_millis(250);
const PROVIDER_RETRY_MAX: Duration = Duration::from_secs(30);
// A retryable provider failure is retried five times in a row; when the
// fifth retry fails too, the attempt fails. This follows Codex's
// `stream_max_retries` default of 5.
const PROVIDER_RETRIES: u32 = 5;
// One deadline around every tool call, as Hermes puts one around every tool
// dispatch. Codex and Hermes give an MCP tool call the same 300 seconds.
const TOOL_DEADLINE: Duration = Duration::from_secs(300);
// What the model is told when a tool fails unexpectedly.
const READ_FAILED: &str = "This call failed. You can try it again.";
const EFFECT_FAILED: &str =
    "This call failed and its outcome is unknown. Check the current state before you repeat it.";
// What the model is told when a tool call reaches its deadline.
const READ_TIMED_OUT: &str = "This call timed out before it finished. You can try it again.";
const EFFECT_TIMED_OUT: &str = "This call timed out before it finished and its outcome is unknown. Check the current state before you repeat it.";
// Three identical refusals establish a loop worth drawing to the model's attention.
const REPEATED_FAILURE_THRESHOLD: u8 = 3;

/// Why a tool call that ran gave no outcome.
enum NoOutcome {
    /// The product's operation answered with an error.
    Error(ProductError),
    /// The call was still running at [`TOOL_DEADLINE`]. Its operation was
    /// dropped, so an effect it started may or may not have happened.
    TimedOut,
}

/// The shared batching rule, also used by product owners to fence writes.
/// Unknown tools remain barriers; shared tool behavior has one owner.
pub fn tool_is_read_only(product: &dyn Product, name: &str) -> bool {
    match name {
        crate::skills::SKILL_VIEW
        | results::READ
        | windows::READ
        | history::LIST
        | history::READ
        | history::SEARCH => true,
        windows::SAVE => false,
        _ => product.read_only(name),
    }
}

/// Exact retained terminal exchange that can only be reconciled as a partial.
/// Products use this before ordinary exhaustion settlement so recovery never
/// needs another provider dispatch merely to retain an acknowledged final turn.
#[derive(Clone, Debug, PartialEq)]
pub struct PendingFinal {
    /// Attempt that recorded the reserved terminal request.
    pub attempt_id: String,
    /// Durable provider request identity.
    pub ordinal: u64,
    /// Exact final request identified by its dispatch purpose.
    pub request: Request,
    /// Exact acknowledged response awaiting the product's partial disposition.
    pub turn: Turn,
    /// Mechanical limits that reserved this terminal request.
    pub backstops: ReachedBackstops,
}

/// Recognize an acknowledged reserved terminal exchange from ordered durable
/// events. Only the latest saved dispatch can own this handoff.
pub fn pending_final<'a>(
    frozen: &recipe::RequestBinding,
    events: impl IntoIterator<Item = impl Into<recipe::ReplayEvent<'a>>>,
    allowance_after: u64,
    messages: &[ProductMessage],
) -> Result<Option<PendingFinal>, Failure> {
    let Some(saved) = recipe::recovery(frozen, events, messages)? else {
        return Ok(None);
    };
    if saved.sources_changed || saved.ordinal <= allowance_after {
        return Ok(None);
    }
    let RequestPurpose::Final { backstops } = saved.context.purpose else {
        return Ok(None);
    };
    let recipe::ExchangeState::Returned(turn) = saved.exchange else {
        return Ok(None);
    };
    Ok(Some(PendingFinal {
        attempt_id: saved.attempt_id,
        ordinal: saved.ordinal,
        request: saved.request,
        turn: turn.handoff(),
        backstops,
    }))
}

/// One accepted product input. Its ordered items enter the wire request together
/// and its identity enters the acknowledged dispatch record.
pub struct AcceptedInput {
    /// Durable command or steer identity in the product's work scope.
    pub id: String,
    /// The authored input and any product-selected context for it.
    pub items: Vec<Item>,
}

/// One product-admitted recipe and the facts authorized for its next step.
pub struct Work {
    pub(super) window: Window,
    pub(super) saved: Option<recipe::Recovery>,
    pub(super) tool_failures: ToolFailures,
    pub(super) notes: Vec<windows::Handoff>,
    pub(super) parts: request_parts::Parts,
    pub(super) allowance_after: u64,
    /// Identity of the product's retained partial answer after explicit retry.
    pub(crate) partial_message: Option<String>,
    /// Accepted input that has not yet been carried by a model dispatch.
    pub(crate) pending_input: Vec<AcceptedInput>,
    /// Product evidence that invalidates a candidate, for example changed sources.
    pub(crate) source_feedback: Option<Value>,
}

impl Work {
    /// The product marks which events may supply direction. Late evidence still
    /// settles an exchange, so it cannot masquerade as an unfinished dispatch.
    pub(crate) fn with_history<'a>(
        mut self,
        events: impl IntoIterator<Item = impl Into<recipe::ReplayEvent<'a>>>,
        allowance_after: u64,
        messages: &[ProductMessage],
    ) -> Result<Self, Failure> {
        let events: Vec<_> = recipe::replay_events(events).collect();
        let frozen = self.window.request(Vec::new()).binding();
        self.saved = recipe::recovery(&frozen, events.iter().copied(), messages)?;
        self.parts = request_parts::Parts::from_events(
            events.iter().filter_map(|event| event.admitted()),
            messages,
        )?;
        self.tool_failures =
            ToolFailures::from_events(events.iter().filter_map(|event| event.admitted()));
        self.notes = windows::saved_notes(
            &events
                .iter()
                .filter_map(|event| event.admitted())
                .collect::<Vec<_>>(),
        );
        self.allowance_after = allowance_after;
        Ok(self)
    }
}

/// Derived from the existing tool receipts, including through recovery. A replayed
/// call keeps its original guidance and never advances the consecutive count.
#[derive(Default)]
pub(super) struct ToolFailures {
    seen: std::collections::HashMap<String, ObservedTool>,
    last: Option<(FailedCall, u8)>,
}

enum ObservedTool {
    Returned,
    Refused {
        output: recipe::Refusal,
        warned: bool,
    },
}

#[derive(PartialEq)]
struct FailedCall {
    name: String,
    arguments: Result<Value, String>,
    output: recipe::Refusal,
}

enum ToolDisposition<'a> {
    Returned,
    Refused(&'a recipe::Refusal),
}

impl ToolFailures {
    fn from_events<'a>(events: impl IntoIterator<Item = &'a Event>) -> Self {
        let mut state = Self::default();
        let mut calls = std::collections::HashMap::new();
        for event in events {
            match &event.kind {
                EventKind::ProviderReturned(turn) => {
                    calls.extend(turn.calls().map(|call| (call.id.as_str(), call)));
                }
                EventKind::ToolStarted(call) => {
                    calls.insert(call.id.as_str(), call);
                }
                EventKind::ToolReturned { call_id, result } => {
                    if let Some(call) = calls.get(call_id.as_str()) {
                        let disposition = match result {
                            results::ToolResult::Refused(refusal) => {
                                ToolDisposition::Refused(refusal)
                            }
                            results::ToolResult::Original(_)
                            | results::ToolResult::History(_)
                            | results::ToolResult::View(_) => ToolDisposition::Returned,
                        };
                        state.observe(call, disposition);
                    }
                }
                EventKind::ProviderStarted { .. }
                | EventKind::ProviderObservation { .. }
                | EventKind::CallBegan { .. }
                | EventKind::ProviderFailed { .. }
                | EventKind::TransportRetry { .. }
                | EventKind::ProviderBlocked { .. }
                | EventKind::ContextRejected
                | EventKind::Candidate { .. }
                | EventKind::WindowOpened { .. } => (),
            }
        }
        state
    }

    fn observe(&mut self, call: &Invocation, disposition: ToolDisposition<'_>) -> bool {
        if let Some(prior) = self.seen.get(&call.id) {
            match (prior, &disposition) {
                (ObservedTool::Returned, ToolDisposition::Returned) => return false,
                (
                    ObservedTool::Refused {
                        output: previous,
                        warned,
                    },
                    ToolDisposition::Refused(output),
                ) if previous == *output => return *warned,
                // Products may retry a refusal after the resource changes.
                // That changed outcome replaces its old advice and streak.
                (
                    ObservedTool::Returned | ObservedTool::Refused { .. },
                    ToolDisposition::Refused(_),
                )
                | (ObservedTool::Refused { .. }, ToolDisposition::Returned) => self.last = None,
            }
        }
        let (observed, warned) = match disposition {
            ToolDisposition::Returned => {
                self.last = None;
                (ObservedTool::Returned, false)
            }
            ToolDisposition::Refused(output) => {
                let signature = FailedCall {
                    name: call.name.clone(),
                    arguments: call.parse_arguments().map_err(|_| call.arguments.clone()),
                    output: output.clone(),
                };
                let count = self
                    .last
                    .as_ref()
                    .filter(|(prior, _)| *prior == signature)
                    .map_or(1, |(_, count)| count.saturating_add(1));
                self.last = Some((signature, count));
                let warned = count >= REPEATED_FAILURE_THRESHOLD;
                (
                    ObservedTool::Refused {
                        output: output.clone(),
                        warned,
                    },
                    warned,
                )
            }
        };
        self.seen.insert(call.id.clone(), observed);
        warned
    }
}

/// Restored provider usage and the current attempt's event sequence.
#[derive(Default)]
pub(crate) struct WorkProgress {
    /// Accounted across all attempts belonging to this work.
    pub(crate) usage: Progress,
    /// Events are numbered from one within a product-supplied attempt.
    pub(crate) sequence: u64,
    /// Durable provider request identity. It never resets when a product
    /// explicitly rearms a fresh mechanical allowance for the same work.
    pub(crate) provider_ordinal: u64,
    /// The one near-limit warning reached a request the provider accepted.
    pub(crate) warned: bool,
}

impl WorkProgress {
    /// Restore one fresh mechanical allowance without resetting durable provider identity.
    pub(crate) fn from_events_after<'a>(
        events: impl IntoIterator<Item = &'a Event>,
        attempt: &str,
        allowance_after: u64,
    ) -> Self {
        let events: Vec<_> = events.into_iter().collect();
        Self {
            usage: Progress::from_events_after(events.iter().copied(), allowance_after),
            sequence: events
                .iter()
                .filter(|event| event.attempt_id == attempt)
                .map(|event| event.sequence)
                .max()
                .unwrap_or(0),
            provider_ordinal: events
                .iter()
                .filter(|event| matches!(event.kind, EventKind::ProviderStarted { .. }))
                .map(|event| event.ordinal)
                .max()
                .unwrap_or(0),
            warned: results::warning_received(events.iter().copied(), allowance_after),
        }
    }
}

pub(super) struct Run<'a> {
    pub(super) product: &'a mut dyn Product,
    pub(super) attempt: &'a str,
    pub(super) progress: &'a mut WorkProgress,
    pub(super) limits: RunLimits,
    pub(super) cancel: &'a Cancellation,
    final_step: Option<ReachedBackstops>,
    tool_failures: ToolFailures,
    parts: request_parts::Parts,
    notes: Vec<windows::Handoff>,
}

impl<'a> Run<'a> {
    fn new(
        product: &'a mut dyn Product,
        attempt: &'a str,
        progress: &'a mut WorkProgress,
        limits: RunLimits,
        cancel: &'a Cancellation,
        work: &mut Work,
    ) -> Result<Self, Failure> {
        if attempt.trim().is_empty() {
            return Err(Failure::Protocol("empty attempt identity".into()));
        }
        let limits = limits.bounded();
        progress.provider_ordinal = progress.provider_ordinal.max(progress.usage.calls);
        Ok(Self {
            product,
            attempt,
            progress,
            limits,
            cancel,
            final_step: None,
            tool_failures: std::mem::take(&mut work.tool_failures),
            parts: std::mem::take(&mut work.parts),
            notes: std::mem::take(&mut work.notes),
        })
    }
    pub(super) async fn record(&mut self, kind: EventKind) -> Result<(), Failure> {
        let sequence = self
            .progress
            .sequence
            .checked_add(1)
            .ok_or_else(|| Failure::Protocol("event ordinal overflow".into()))?;
        let event = Event {
            attempt_id: self.attempt.to_owned(),
            sequence,
            ordinal: self.progress.provider_ordinal,
            kind,
        };
        let mut backoff = PERSIST_RETRY_START;
        loop {
            match self.product.persist(event.clone()).await {
                Ok(()) => break,
                Err(ProductError::Failed(error)) if error.is::<RetryablePersistence>() => {
                    // Keep the exact event and any paid result in memory until the
                    // owner acknowledges it. Never request a replacement model turn.
                    // Stop and shutdown end the wait; the save itself is never cut.
                    tokio::select! {
                        biased;
                        () = self.cancel.cancelled() => return Err(Failure::Cancelled),
                        () = tokio::time::sleep(backoff) => (),
                    }
                    backoff = (backoff * 2).min(PERSIST_RETRY_MAX);
                }
                Err(error) => return Err(error.into()),
            }
        }
        self.progress.sequence = sequence;
        self.cancel.mark();
        self.parts.observe(&event)?;
        Ok(())
    }

    /// The exact request sent to the provider, with results in their wire form.
    pub(super) fn for_provider(&self, request: &Request) -> Result<Request, Failure> {
        self.parts.for_provider(request)
    }
    // Provider readiness and retry backoff have no product callback in flight.
    // Use the same currency cadence as transport dispatch to observe Stop,
    // steering and revoked authority while waiting.
    async fn wait<T>(
        &mut self,
        future: impl Future<Output = Result<T, Failure>>,
    ) -> Result<T, Failure> {
        tokio::pin!(future);
        let mut tick =
            tokio::time::interval_at(Instant::now() + CURRENCY_INTERVAL, CURRENCY_INTERVAL);
        loop {
            tokio::select! {
                biased;
                () = self.cancel.cancelled() => return Err(Failure::Cancelled),
                result = &mut future => return result,
                _ = tick.tick() => while_current(self.cancel, self.check()).await?,
            }
        }
    }
    async fn check(&mut self) -> Result<(), Failure> {
        self.cancel.mark();
        self.cancel.check()?;
        self.product.assert_current().await.map_err(Failure::from)
    }
    async fn candidate(&mut self, turn: &Turn) -> Result<Finish, Failure> {
        self.check().await?;
        self.record(EventKind::Candidate { text: turn.text() })
            .await?;
        self.check().await?;
        self.product.finish(turn).await.map_err(Failure::from)
    }
    async fn partial_candidate(
        &mut self,
        turn: &Turn,
        backstops: &[LimitBackstop],
    ) -> Result<(), Failure> {
        self.check().await?;
        self.record(EventKind::Candidate { text: turn.text() })
            .await?;
        self.check().await?;
        self.product
            .finish_partial(turn, backstops)
            .await
            .map_err(Failure::from)
    }
    /// Called once for each new response, before it is saved.
    pub(super) fn assign_call_ids(&self, turn: &mut Turn) {
        assign_call_ids(turn, self.parts.call_ids());
    }

    async fn tools(&mut self, window: &mut Window, calls: Vec<Invocation>) -> Result<(), Failure> {
        let mut calls = calls.as_slice();
        while let Some(first) = calls.first() {
            let size = if tool_is_read_only(self.product, &first.name) {
                calls
                    .iter()
                    .take_while(|call| tool_is_read_only(self.product, &call.name))
                    .count()
            } else {
                1
            };
            let (batch, rest) = calls.split_at(size);
            let mut pending = JoinSet::new();
            let mut next = 0;
            let mut results = vec![None; batch.len()];
            let mut failure = None;
            let mut tick =
                tokio::time::interval_at(Instant::now() + CURRENCY_INTERVAL, CURRENCY_INTERVAL);
            loop {
                // Admit only calls that can start now. A steer closes admission,
                // but every operation already handed to the product is drained.
                while failure.is_none() && next < batch.len() && pending.len() < MAX_PARALLEL_READS
                {
                    // Check cancellation before admission. Once persistence begins,
                    // its acknowledgement owns the sequence, including during Stop.
                    if let Some(replaced) = self.currency_while_calls_run().await? {
                        failure.get_or_insert(replaced);
                        break;
                    }
                    match self.tool(window, &batch[next]).await {
                        Ok(operation) => {
                            let index = next;
                            next += 1;
                            pending.spawn(async move {
                                let outcome =
                                    match tokio::time::timeout(TOOL_DEADLINE, operation).await {
                                        Ok(outcome) => outcome.map_err(NoOutcome::Error),
                                        Err(_) => Err(NoOutcome::TimedOut),
                                    };
                                (index, outcome)
                            });
                        }
                        Err(error) => failure = Some(error),
                    }
                }
                // While calls run, check currency as `wait` does. New input lets
                // them finish within their deadline. Stop, withdrawn work or lost
                // ownership ends the wait now: dropping `pending` drops the
                // running operations, and each stays a call that started with no
                // recorded outcome, which recovery reports to the model.
                let completed = tokio::select! {
                    biased;
                    () = self.cancel.cancelled() => return Err(Failure::Cancelled),
                    completed = pending.join_next() => completed,
                    _ = tick.tick() => {
                        if let Some(replaced) = self.currency_while_calls_run().await? {
                            failure.get_or_insert(replaced);
                        }
                        continue;
                    }
                };
                let Some(completed) = completed else {
                    break;
                };
                let (index, outcome) = match completed {
                    Ok((index, Ok(outcome))) => (index, outcome),
                    // A failure already ends the batch, so this call's own
                    // error changes nothing.
                    Ok((_, Err(_))) if failure.is_some() => continue,
                    Ok((index, Err(no_outcome))) => {
                        match self.tool_failed(&batch[index], no_outcome).await {
                            Ok(outcome) => (index, outcome),
                            Err(error) => {
                                failure.get_or_insert(error);
                                continue;
                            }
                        }
                    }
                    Err(error) => {
                        failure.get_or_insert(Failure::Host(error.into()));
                        continue;
                    }
                };
                match self.deliver(window, &batch[index], outcome).await {
                    Ok(result) => results[index] = Some(result),
                    Err(error) => {
                        // An unacknowledged event owns the next sequence, so no
                        // later result can be recorded. Only read-only calls
                        // share a batch, so any call still running is a read.
                        // Dropping `pending` drops it, and it stays a call that
                        // started with no recorded outcome.
                        return Err(error);
                    }
                }
            }
            if let Some(failure) = failure {
                return Err(failure);
            }
            window.items.extend(results.into_iter().flatten());
            calls = rest;
        }
        Ok(())
    }

    /// While calls run, whether the attempt is still wanted. `Ok(None)`: it
    /// is. `Ok(Some(failure))`: new input replaced the direction, so admission
    /// closes and running calls finish within their deadline. `Err`: Stop,
    /// withdrawn work or lost ownership ends the wait now.
    async fn currency_while_calls_run(&mut self) -> Result<Option<Failure>, Failure> {
        match while_current(self.cancel, self.check()).await {
            Ok(()) => Ok(None),
            Err(failure @ Failure::NotCurrent(NotCurrent::Superseded)) => Ok(Some(failure)),
            Err(failure) => Err(failure),
        }
    }

    /// A tool that fails unexpectedly gives the model an error result while
    /// the attempt is still current, as Codex, Hermes, OpenClaw and Pi do. The
    /// text is fixed; the error itself never reaches the model. A call that
    /// reached its deadline takes this path too, and its text says it timed
    /// out. A tool whose product answered that the work no longer wants this
    /// attempt ends the attempt with that answer. When the attempt is no
    /// longer current, the currency check's failure ends it, so newer input
    /// interrupts it.
    async fn tool_failed(
        &mut self,
        call: &Invocation,
        no_outcome: NoOutcome,
    ) -> Result<ToolOutcome, Failure> {
        let timed_out = match no_outcome {
            NoOutcome::Error(error @ ProductError::NotCurrent(_)) => return Err(error.into()),
            NoOutcome::Error(ProductError::Failed(_)) => false,
            NoOutcome::TimedOut => true,
        };
        while_current(self.cancel, self.check()).await?;
        let read_only = tool_is_read_only(self.product, &call.name);
        Ok(ToolOutcome::Refused(match (read_only, timed_out) {
            (true, false) => recipe::Refusal::new(READ_FAILED),
            (true, true) => recipe::Refusal::new(READ_TIMED_OUT),
            (false, false) => recipe::Refusal::unknown_outcome(EFFECT_FAILED),
            (false, true) => recipe::Refusal::unknown_outcome(EFFECT_TIMED_OUT),
        }))
    }

    /// Reconcile every retained outcome before admitting any unstarted call.
    async fn recover_tools(
        &mut self,
        window: &mut Window,
        turn: Turn,
        events: &[Event],
        steered: bool,
    ) -> Result<(), Failure> {
        work_response_bound(&turn, self.limits.max_response_bytes)?;
        let calls = checked_calls(&turn)?;
        window.items.extend(
            turn.items
                .into_iter()
                .filter(|item| !matches!(item, Item::Message { role, .. } if role == "assistant")),
        );
        let mut pending = Vec::new();
        for call in calls {
            let returned = events.iter().rev().find(|event| {
                matches!(
                    &event.kind, EventKind::ToolReturned { call_id, .. } if call_id == &call.id
                )
            });
            let output = if let Some(event) = returned {
                let EventKind::ToolReturned { result, .. } = &event.kind else {
                    unreachable!()
                };
                Some(self.present_result(
                    window,
                    &call,
                    result.clone(),
                    &event.attempt_id,
                    event.sequence,
                )?)
            } else if events.iter().any(|event| {
                matches!(
                    &event.kind, EventKind::ToolStarted(started) if started.id == call.id
                )
            }) {
                self.check().await?;
                let receipt = self.product.saved_outcome(&call);
                let saved = self
                    .wait(async { receipt.await.map_err(Failure::from) })
                    .await?;
                Some(match saved {
                    Some(outcome) => self.deliver(window, &call, outcome).await?,
                    None => Item::Result {
                        call_id: call.id.clone(),
                        output: results::ResultView::inline(serde_json::json!({
                            "outcome": "unknown",
                            "instruction": "This call started but has no known outcome. Check current state before repeating a call that may have had an effect."
                        }).to_string()),
                    },
                })
            } else if steered {
                Some(Item::Result {
                    call_id: call.id.clone(),
                    output: results::ResultView::inline(serde_json::json!({
                        "outcome": "not_run",
                        "instruction": "This call did not start before the new input. Decide whether it is still needed."
                    }).to_string()),
                })
            } else {
                None
            };
            if let Some(output) = output {
                window.items.push(output);
            } else {
                pending.push(call);
            }
        }
        self.tools(window, pending).await
    }

    /// Apply a retained exchange to this run and to the window. The result
    /// either settles the work or says how the live loop starts.
    async fn resume(
        &mut self,
        mut window: Window,
        mut saved: recipe::Recovery,
        mut source_feedback: Option<Value>,
        allowance_after: u64,
        new_direction: bool,
        partial_message: Option<String>,
    ) -> Result<Resumed, Failure> {
        if saved.sources_changed && source_feedback.is_none() {
            source_feedback = Some(changed_sources_feedback());
        }
        let frozen = window.take_request();
        let rearmed = saved.ordinal <= allowance_after;
        let entry = match &saved.exchange {
            recipe::ExchangeState::Pending
                if !rearmed && !new_direction && source_feedback.is_none() =>
            {
                // Retain the exact request as a replay candidate. The shared
                // dispatch decision still checks whether ordinary work is allowed.
                Entry::Replay(Box::new((saved.request.clone(), saved.context.clone())))
            }
            recipe::ExchangeState::ContextRejected => Entry::ContextRejected,
            recipe::ExchangeState::Pending
            | recipe::ExchangeState::Returned(_)
            | recipe::ExchangeState::Ended => Entry::Fresh,
        };
        saved.context.restore(&mut saved.request.items)?;
        let handoff_pending = matches!(saved.context.purpose, RequestPurpose::Handoff);
        let final_step = match &saved.context.purpose {
            RequestPurpose::Final { backstops } => Some(backstops.clone()),
            RequestPurpose::Work | RequestPurpose::Handoff => None,
        };
        let returned = match saved.exchange {
            recipe::ExchangeState::Returned(turn) => Some(turn),
            recipe::ExchangeState::Pending
            | recipe::ExchangeState::Ended
            | recipe::ExchangeState::ContextRejected => None,
        };
        let exchange_returned = returned.is_some();
        let mut turn = returned.map(|turn| {
            if final_step.is_none() {
                turn
            } else {
                turn.handoff()
            }
        });
        let mut request = saved.request.clone();
        if let Some(feedback) = source_feedback {
            if let Some(returned) =
                turn.take_if(|turn| turn.calls().next().is_none() || self.product.terminal(turn))
            {
                request = recipe::continued(request, &returned, &feedback);
            } else {
                request.items.push(Item::Feedback {
                    text: feedback.to_string(),
                });
            }
        }
        // New direction supersedes a terminal answer. On explicit rearm, only
        // the product's published segment can stand for visible work.
        if (new_direction || rearmed)
            && turn
                .as_ref()
                .is_some_and(|t| t.calls().next().is_none() || self.product.terminal(t))
        {
            turn = None;
        }
        if new_direction && !rearmed && final_step.is_some() {
            // Accepted input superseded the in-flight reserved step. Redo that
            // final with the new direction; the rest of the allowance stays spent.
            self.final_step = final_step.clone();
        }
        let ending = final_step.is_some() && !rearmed && !new_direction;
        window = request.into_window()?;
        window.sent_items = saved.request.items.len();
        window.handoff_pending = handoff_pending;
        window.meter = saved.meter;
        // As in the live loop, a returned exchange leaves the window
        // accumulated, whichever branch below consumes its turn.
        window.stage = if exchange_returned {
            WindowStage::Accumulated
        } else {
            saved.context.window_stage
        };
        if turn
            .as_ref()
            .is_some_and(|turn| turn.calls().next().is_some() && !self.product.terminal(turn))
        {
            let returned = turn.take().expect("tool turn exists");
            self.recover_tools(&mut window, returned, &saved.tool_events, new_direction)
                .await?;
        }
        if matches!(entry, Entry::ContextRejected) && !rearmed && final_step.is_some() {
            return Err(Failure::ContextRefused);
        }
        // A paid reserved final is settled with publication authority only;
        // no provider work is started.
        if ending && let Some(turn) = turn.take() {
            self.final_step = final_step;
            let settled = self.apply_turn(&mut window, turn).await?;
            if let Some(answer) = settled {
                return Ok(Resumed::Settled(answer));
            }
            return Err(Failure::Protocol("reserved final did not settle".into()));
        }
        if rearmed {
            let partial = partial_message
                .as_deref()
                .and_then(|id| self.parts.message(id))
                .cloned();
            let feedback = serde_json::json!({
                "instruction": "Continue the same accepted work from retained evidence. Do not repeat visible partial work or any completed tool effect."
            });
            let restored = recipe::rearmed(&frozen, window.take_request(), partial, &feedback)?;
            window.items = restored.items;
            window.handoff_pending = false;
            window.stage = WindowStage::Accumulated;
        }
        if let Some(turn) = turn {
            let settled = self.apply_turn(&mut window, turn).await?;
            if let Some(answer) = settled {
                return Ok(Resumed::Settled(answer));
            }
        }
        window.recipe = frozen.into_window()?.recipe;
        Ok(Resumed::Continue { window, entry })
    }

    async fn tool(
        &mut self,
        window: &Window,
        call: &Invocation,
    ) -> Result<recipe::ToolFuture, Failure> {
        let refused = |reason: &str| -> recipe::ToolFuture {
            let result = ToolOutcome::refused(reason);
            Box::pin(async move { Ok(result) })
        };
        if call.name.trim().is_empty() {
            return Ok(refused(
                "The tool name was empty. Call a tool by a name from the offered tools, or answer in plain text.",
            ));
        }
        if !window.recipe.offers(&call.name) {
            // Name the tools the job has, as Hermes does.
            let offered: Vec<_> = window.recipe.tool_names().collect();
            return Ok(refused(&format!(
                "Tool '{}' does not exist. Available tools: {}.",
                call.name,
                offered.join(", ")
            )));
        }
        let args = match call.parse_arguments() {
            Ok(args) => args,
            Err(reason) => return Ok(refused(reason)),
        };
        self.record(EventKind::ToolStarted(call.clone())).await?;
        self.check().await?;
        if call.name == results::READ {
            let page = match serde_json::from_value::<results::PageRequest>(args) {
                Ok(page) => page,
                Err(_) => {
                    return Ok(refused(
                        "Use the original call_id and next_offset as offset.",
                    ));
                }
            };
            let original = self.product.read_result(&page.call_id);
            let presentation =
                results::Presentation::for_window(window.recipe.context_window_tokens);
            return Ok(Box::pin(async move {
                let original = original.await?;
                Ok(page.read(original.as_deref(), presentation))
            }));
        }
        Ok(self.product.execute(call, args))
    }

    async fn deliver(
        &mut self,
        window: &Window,
        call: &Invocation,
        outcome: ToolOutcome,
    ) -> Result<Item, Failure> {
        let result = match outcome {
            ToolOutcome::Returned(payload) => results::ToolResult::Original(payload),
            ToolOutcome::History(page) => results::ToolResult::History(page.into()),
            ToolOutcome::Retained(result) => result,
            ToolOutcome::Refused(refusal) => results::ToolResult::Refused(refusal),
        };
        self.record(EventKind::ToolReturned {
            call_id: call.id.clone(),
            result: result.clone(),
        })
        .await?;
        if let Some(note) = result
            .original()
            .and_then(|output| windows::saved_note(call, output))
        {
            self.notes.push(note);
        }
        self.present_result(window, call, result, self.attempt, self.progress.sequence)
    }

    fn present_result(
        &mut self,
        window: &Window,
        call: &Invocation,
        result: results::ToolResult,
        attempt: &str,
        sequence: u64,
    ) -> Result<Item, Failure> {
        let disposition = match &result {
            results::ToolResult::Refused(refusal) => ToolDisposition::Refused(refusal),
            results::ToolResult::Original(_)
            | results::ToolResult::History(_)
            | results::ToolResult::View(_) => ToolDisposition::Returned,
        };
        let repeated = self.tool_failures.observe(call, disposition);
        let mut output = match result {
            results::ToolResult::View(view) => {
                if self.parts.original_for(&view).is_some() {
                    view
                } else {
                    results::ResultView::unavailable()
                }
            }
            results::ToolResult::History(page) => match self.parts.resolve_result(&call.id) {
                Some(encoded) if window.recipe.offers(results::READ) => {
                    results::Presentation::for_window(window.recipe.context_window_tokens)
                        .show(&call.id, &encoded)
                }
                Some(_) => results::ResultView::retained_whole(&call.id, page.total_bytes),
                None => results::ResultView::unavailable(),
            },
            results::ToolResult::Refused(refusal) => {
                results::ResultView::refused(attempt, sequence, &refusal)
            }
            results::ToolResult::Original(original) => {
                if window.recipe.offers(results::READ) {
                    results::Presentation::for_window(window.recipe.context_window_tokens)
                        .show(&call.id, &original)
                } else {
                    results::ResultView::whole(&call.id, &original)
                }
            }
        };
        if results::is_page_read(&call.name) {
            let presentation =
                results::Presentation::for_window(window.recipe.context_window_tokens);
            if let Some(original) = self.parts.original_for(&output) {
                let _ =
                    results::shorten_read(&call.name, &mut output, &original, presentation.inline);
            }
        }
        if repeated {
            output.annotate(
                "repeated_failure_note",
                format!(
                    "This call has failed the same way {REPEATED_FAILURE_THRESHOLD} or more times in a row. Do not repeat this call unchanged. Reassess the task and choose a different argument, tool or approach."
                ),
            );
        }
        Ok(Item::Result {
            call_id: call.id.clone(),
            output,
        })
    }

    fn reached_backstops(&self) -> Option<ReachedBackstops> {
        self.limits.reached_backstops(&self.progress.usage)
    }

    async fn prepare(
        &mut self,
        window: &mut Window,
        starting: bool,
    ) -> Result<(Request, DispatchContext), Failure> {
        if self.final_step.is_none() && self.progress.usage.model_steps() >= self.limits.max_calls {
            return Err(Failure::Limit("model calls".into()));
        }
        if window.recipe.working_windows()
            && let Some((handoff, items)) = windows::rotate(
                self.product,
                &window.recipe,
                &window.items,
                &self.notes,
                &self.parts,
            )
            .await?
        {
            self.record(EventKind::WindowOpened {
                opened_at_unix_ms: unix_ms()?,
                reason: if window.handoff_pending {
                    WindowReason::NearEnd
                } else {
                    WindowReason::ModelAsked
                },
                handoff: Some(handoff.note),
            })
            .await?;
            window.items = items;
            window.sent_items = 0;
            window.handoff_pending = false;
            window.stage = WindowStage::Fresh;
            window.meter = context::Meter::default();
        }
        if self.final_step.is_none() {
            self.final_step = self.reached_backstops();
        }
        let should_warn = !self.progress.warned && self.limits.near_limit(&self.progress.usage);
        let mut reopened = window.stage != WindowStage::Accumulated;
        loop {
            let mut request = window.request(window.items.clone());
            let input_limit = context::input_limit(&request)?;
            let warning_added = if should_warn {
                windows::append_warning(&mut request.items[window.sent_items..])
            } else {
                false
            };
            if window.recipe.offers(results::READ) {
                let room = result_room(window, &request, &self.parts)?;
                results::page_batch(&mut request, &self.parts, window.sent_items, room)?;
            }
            let measured = self.parts.for_provider(&request)?;
            let ask = handoff_ask(window, &measured)?;
            let needs_handoff =
                window.recipe.working_windows() && window.meter.needs_handoff(&measured, &ask)?;
            if starting && !reopened && needs_handoff {
                self.reopen(window, WindowReason::NearEnd).await?;
                reopened = true;
                continue;
            }
            let purpose = self.request_purpose(window, &mut request, needs_handoff, ask);
            let count = window.items.len();
            let usage = window.meter.measure(&self.parts.for_provider(&request)?)?;
            if usage.input_tokens > input_limit {
                if !window.recipe.working_windows() {
                    return Err(Failure::Protocol(
                        "model context tokens; working windows unavailable".into(),
                    ));
                }
                if !reopened {
                    self.reopen(window, WindowReason::Full).await?;
                    reopened = true;
                    continue;
                }
                // The fresh seed is exact accepted work. An estimate never
                // truncates it or fails the run; the provider can refuse it and
                // ask for another window through the same continuation path.
            }
            window.handoff_pending = purpose == RequestPurpose::Handoff;
            window.items.clone_from_slice(&request.items[..count]);
            return Ok((
                request,
                DispatchContext {
                    usage,
                    purpose,
                    carried_inputs: Vec::new(),
                    warning_added,
                    window_stage: window.stage,
                },
            ));
        }
    }

    fn request_purpose(
        &self,
        window: &Window,
        request: &mut Request,
        needs_handoff: bool,
        ask: String,
    ) -> RequestPurpose {
        let purpose = if let Some(backstops) = &self.final_step {
            RequestPurpose::Final {
                backstops: backstops.clone(),
            }
        } else if window.recipe.working_windows() && (window.handoff_pending || needs_handoff) {
            RequestPurpose::Handoff
        } else {
            RequestPurpose::Work
        };
        let instruction = match &purpose {
            RequestPurpose::Final { .. } => Some(windows::FINAL_REQUEST.into()),
            RequestPurpose::Handoff => Some(ask),
            RequestPurpose::Work => None,
        };
        if let Some(text) = instruction {
            request.items.push(Item::Message {
                role: "developer".into(),
                text,
            });
        }
        purpose
    }

    /// At the start of a run, name the notes earlier work in this conversation
    /// saved. The product lists them for a fresh window, so the same list is
    /// read here. The message stays in the window. If the run opens as a fresh
    /// window instead, that window's own message names the notes.
    async fn name_earlier_notes(&mut self, window: &mut Window) -> Result<(), Failure> {
        if !window.recipe.working_windows() || !window.recipe.offers(windows::READ) {
            return Ok(());
        }
        self.check().await?;
        let seed = self.product.window_seed().await.map_err(Failure::from)?;
        window
            .items
            .extend(windows::earlier_notes(&seed.note_references)?);
        Ok(())
    }

    async fn reopen(&mut self, window: &mut Window, reason: WindowReason) -> Result<(), Failure> {
        self.check().await?;
        let pending = windows::pending_results(&window.items)?;
        // The window closes without a note for this change; the newest note
        // saved in this work still carries the work across.
        let carried = match self.notes.last() {
            Some(note) => windows::Carried::Earlier(note),
            None => windows::Carried::Nothing,
        };
        let items = windows::seed(
            self.product,
            &window.recipe,
            carried,
            &self.notes,
            pending,
            &self.parts,
        )
        .await?;
        self.record(EventKind::WindowOpened {
            opened_at_unix_ms: unix_ms()?,
            reason,
            handoff: None,
        })
        .await?;
        window.items = items;
        window.sent_items = 0;
        window.handoff_pending = false;
        window.stage = WindowStage::Fresh;
        window.meter = context::Meter::default();
        Ok(())
    }

    async fn refused(&mut self, window: &mut Window) -> Result<(), Failure> {
        let previous = window.stage;
        if previous == WindowStage::Reduced {
            return Err(Failure::ContextRefused);
        }
        if previous == WindowStage::Fresh {
            let mut request = window.take_request();
            results::reduce_pending(&mut request, &self.parts)?;
            window.items = request.items;
            window.sent_items = 0;
            window.meter = context::Meter::default();
            window.stage = WindowStage::Reduced;
        } else {
            self.reopen(window, WindowReason::Refused).await?;
        }
        Ok(())
    }

    async fn admit(
        &mut self,
        provider: &mut (impl Provider + ?Sized),
        request: &Request,
        context: &DispatchContext,
    ) -> Result<(), Failure> {
        self.wait(async { provider.ready().await.map_err(Failure::Provider) })
            .await?;
        self.check().await?;
        self.progress.usage.calls += 1;
        self.progress.provider_ordinal = self
            .progress
            .provider_ordinal
            .checked_add(1)
            .ok_or_else(|| Failure::Protocol("provider request ordinal overflow".into()))?;
        let sequence = self
            .progress
            .sequence
            .checked_add(1)
            .ok_or_else(|| Failure::Protocol("event ordinal overflow".into()))?;
        let saved = self.parts.save(self.attempt, sequence, &request.items)?;
        self.record(EventKind::ProviderStarted {
            dispatched_at_unix_ms: unix_ms()?,
            context: context.clone(),
            request: saved,
        })
        .await?;
        self.check().await
    }

    async fn apply_turn(
        &mut self,
        window: &mut Window,
        turn: Turn,
    ) -> Result<Option<Turn>, Failure> {
        while_current(self.cancel, self.check()).await?;
        let turn = if self.final_step.is_some() {
            turn.handoff()
        } else {
            turn
        };
        work_response_bound(&turn, self.limits.max_response_bytes)?;
        if let Some(backstops) = self.final_step.clone() {
            while_current(self.cancel, self.partial_candidate(&turn, &backstops)).await?;
            return Ok(Some(turn));
        }
        let calls = checked_calls(&turn)?;
        window.items.extend(turn.context_items().cloned());
        if let Some(feedback) = turn.correction() {
            window.items.extend(feedback);
        } else if calls.is_empty() || self.product.terminal(&turn) {
            match while_current(self.cancel, self.candidate(&turn)).await? {
                Finish::Committed => return Ok(Some(turn)),
                Finish::Continue(feedback) => window.items.extend(turn.feedback(&feedback)),
                Finish::Refused(refusal) => window.items.extend(turn.refused(&refusal)),
            }
        } else {
            self.tools(window, calls).await?;
        }
        Ok(None)
    }
}

/// The ask this request carries near the end, with the input tokens it leaves.
/// It is measured with the widest count, so the count never overstates what is left.
fn handoff_ask(window: &Window, request: &Request) -> Result<String, Failure> {
    let widest = windows::handoff_ask(context::input_limit(request)?);
    let left = window.meter.tokens_left(request, &widest)?;
    Ok(windows::ask(window.handoff_pending, left))
}

/// Characters this turn's new results can use: the room under the input limit
/// after the items already sent, the new items that are not results and the ask.
fn result_room(
    window: &Window,
    request: &Request,
    parts: &request_parts::Parts,
) -> Result<usize, Failure> {
    let mut others = request.clone();
    let mut index = 0;
    others.items.retain(|item| {
        let sent = index < window.sent_items;
        index += 1;
        sent || !matches!(item, Item::Result { .. })
    });
    let results = request.items.len() - others.items.len();
    let widest = windows::ask(window.handoff_pending, context::input_limit(request)?);
    window
        .meter
        .result_room(&parts.for_provider(&others)?, &widest, results)
}

fn changed_sources_feedback() -> Value {
    serde_json::json!({"changed_sources":"A previously referenced message was deleted or changed. Continue from current sources."})
}

/// What applying a retained exchange leaves for the work loop.
enum Resumed {
    /// The retained exchange settled the work.
    Settled(Turn),
    /// The live loop continues from this window.
    Continue { window: Window, entry: Entry },
}

/// How the live loop starts after a retained exchange is applied.
enum Entry {
    /// Send the retained request again, unchanged. The shared dispatch
    /// decision still checks whether ordinary work is allowed.
    Replay(Box<(Request, DispatchContext)>),
    /// The provider rejected the retained request's context.
    ContextRejected,
    /// Prepare a new request from the window.
    Fresh,
}

/// Retryable provider failures in a row, and the backoff before the next try.
/// A completed response, or a request that changed, starts both again.
struct ProviderRetries {
    failures: u32,
    backoff: Duration,
}
impl ProviderRetries {
    const fn new() -> Self {
        Self {
            failures: 0,
            backoff: PROVIDER_RETRY_START,
        }
    }
    /// Count one more failure. False when no retry is left.
    fn another(&mut self) -> bool {
        self.failures += 1;
        self.failures <= PROVIDER_RETRIES
    }
    /// The wait before the next try.
    fn wait(&mut self) -> Duration {
        let wait = self.backoff;
        self.backoff = (self.backoff * 2).min(PROVIDER_RETRY_MAX);
        wait
    }
}

/// Start or continue one product-authorized work item. The same loop handles
/// a live response and a returned exchange retained across a process restart.
pub(crate) async fn run_work<P: Provider + ?Sized>(
    provider: &mut P,
    product: &mut dyn Product,
    attempt: &str,
    mut work: Work,
    progress: &mut WorkProgress,
    limits: RunLimits,
    cancel: &Cancellation,
) -> Result<Turn, Failure> {
    let mut run = Run::new(product, attempt, progress, limits, cancel, &mut work)?;
    let mut window = work.window;
    while_current(cancel, async {
        run.product
            .admit_binding(&window.request(Vec::new()).binding())
            .await
            .map_err(Failure::from)
    })
    .await?;
    let mut starting = work.saved.is_none();
    let mut entry = Entry::Fresh;
    if let Some(saved) = work.saved {
        match run
            .resume(
                window,
                saved,
                work.source_feedback,
                work.allowance_after,
                !work.pending_input.is_empty(),
                work.partial_message,
            )
            .await?
        {
            Resumed::Settled(answer) => return Ok(answer),
            Resumed::Continue {
                window: resumed,
                entry: resumed_entry,
            } => {
                window = resumed;
                entry = resumed_entry;
            }
        }
    }
    if starting {
        while_current(cancel, run.name_earlier_notes(&mut window)).await?;
    }
    let mut carried_inputs = Vec::with_capacity(work.pending_input.len());
    for input in work.pending_input {
        window.items.extend(input.items);
        carried_inputs.push(input.id);
    }
    window.items = window_items(std::mem::take(&mut window.items))?;
    let mut retry = match entry {
        Entry::Replay(replay) => Some(*replay),
        Entry::ContextRejected => {
            if window.recipe.working_windows() {
                while_current(cancel, run.refused(&mut window)).await?;
            }
            None
        }
        Entry::Fresh => None,
    };
    let mut retries = ProviderRetries::new();
    loop {
        // Live retries and recovered requests enter this same dispatch decision.
        // Keep a permitted final exact; a reached backstop replaces ordinary work.
        let sources = while_current(cancel, async {
            run.check().await?;
            run.product.message_sources().await.map_err(Failure::from)
        })
        .await?;
        let sources_changed = match sources {
            Some(messages) => run.parts.refresh_messages(messages, &mut window.items)?,
            None => false,
        };
        if sources_changed {
            window.items.push(Item::Feedback {
                text: changed_sources_feedback().to_string(),
            });
            retry = None;
            retries = ProviderRetries::new();
        }
        if retry
            .as_ref()
            .is_some_and(|(_, context)| match context.purpose {
                RequestPurpose::Final { .. } => false,
                RequestPurpose::Work | RequestPurpose::Handoff => run.reached_backstops().is_some(),
            })
        {
            retry = None;
            retries = ProviderRetries::new();
        }
        let (request, mut context) = match retry.take() {
            Some(saved) => {
                run.final_step = match &saved.1.purpose {
                    RequestPurpose::Final { backstops } => Some(backstops.clone()),
                    RequestPurpose::Work | RequestPurpose::Handoff => None,
                };
                saved
            }
            None => while_current(cancel, run.prepare(&mut window, starting)).await?,
        };
        starting = false;
        context.carried_inputs = std::mem::take(&mut carried_inputs);
        while_current(cancel, run.admit(provider, &request, &context)).await?;
        window.sent_items = window.items.len();
        let turn = match exchange::dispatch(&mut run, provider, &request, &context).await {
            Ok(turn) => turn,
            Err(error)
                if crate::responses::context_rejected(&error)
                    && window.recipe.working_windows() =>
            {
                if run.final_step.is_some() {
                    return Err(Failure::Final(Box::new(error)));
                }
                while_current(cancel, run.refused(&mut window)).await?;
                retries = ProviderRetries::new();
                continue;
            }
            // After the last retry, the provider error ends the attempt.
            Err(error) if error.transport_retry() && retries.another() => {
                let wait = retries.wait();
                run.wait(async {
                    tokio::time::sleep(wait).await;
                    Ok(())
                })
                .await?;
                retry = Some((request, context));
                continue;
            }
            Err(error @ (Failure::Provider(_) | Failure::AfterOutput(_)))
                if run.final_step.is_some() =>
            {
                return Err(Failure::Final(Box::new(error)));
            }
            Err(error) => return Err(error),
        };
        retries = ProviderRetries::new();
        window.stage = WindowStage::Accumulated;
        window
            .meter
            .observe(&run.parts.for_provider(&request)?, &turn)?;
        let settled = run.apply_turn(&mut window, turn).await?;
        if let Some(committed) = settled {
            return Ok(committed);
        }
    }
}

/// Once cancellation is known, no new product callback may start.
/// A dropped in-flight write is reconciled by its product on recovery.
async fn while_current<T>(
    cancel: &Cancellation,
    future: impl Future<Output = Result<T, Failure>>,
) -> Result<T, Failure> {
    tokio::select! {
        biased;
        () = cancel.cancelled() => Err(Failure::Cancelled),
        result = future => result,
    }
}

fn unix_ms() -> Result<u64, Failure> {
    Ok(std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .map_err(|error| Failure::Host(error.into()))?
        .as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(id: &str) -> Invocation {
        Invocation {
            id: id.into(),
            name: "read".into(),
            arguments: "{}".into(),
        }
    }

    struct Owner;
    #[async_trait]
    impl Product for Owner {
        fn read_result(
            &mut self,
            _call_id: &str,
        ) -> crate::engine::recipe::ToolFuture<Option<String>> {
            Box::pin(async { Err("result paging is unavailable for this fixture".into()) })
        }
        async fn finish_partial(
            &mut self,
            _turn: &crate::engine::Turn,
            _backstops: &[crate::engine::LimitBackstop],
        ) -> Result<(), crate::engine::ProductError> {
            Err("this fixture has no partial-result disposition".into())
        }
        async fn window_seed(
            &mut self,
        ) -> Result<crate::engine::WindowSeed, crate::engine::ProductError> {
            Err("working windows are unavailable for this fixture".into())
        }

        async fn assert_current(&mut self) -> Result<(), ProductError> {
            Ok(())
        }
        async fn message_sources(&mut self) -> Result<Option<Vec<ProductMessage>>, ProductError> {
            Ok(None)
        }
        async fn admit_binding(&mut self, _: &recipe::RequestBinding) -> Result<(), ProductError> {
            Ok(())
        }
        async fn persist(&mut self, _: Event) -> Result<(), ProductError> {
            Ok(())
        }
        fn execute(&mut self, _: &Invocation, _: Value) -> recipe::ToolFuture {
            Box::pin(async { Ok(ToolOutcome::Returned("evidence".into())) })
        }
        async fn finish(&mut self, _: &Turn) -> Result<Finish, ProductError> {
            Ok(Finish::Committed)
        }
    }
    fn request() -> Request {
        Request {
            cache_key: None,
            model: "scripted".into(),
            context_window_tokens: 100_000,
            max_input_tokens: 100_000,
            options: Value::Null,
            instructions: "Complete the work".into(),
            items: vec![],
            tools: vec![
                Tool {
                    name: "read".into(),
                    description: "Read evidence".into(),
                    parameters: serde_json::json!({"type":"object"}),
                }
                .function(),
            ],
        }
    }

    /// Answers each request in order, where `None` is a retryable failure, and
    /// completes every request after the script.
    struct Replies {
        script: std::collections::VecDeque<Option<Item>>,
        sent: usize,
    }
    impl Replies {
        fn new(script: impl IntoIterator<Item = Option<Item>>) -> Self {
            Self {
                script: script.into_iter().collect(),
                sent: 0,
            }
        }
    }
    impl Provider for Replies {
        async fn exchange(
            &mut self,
            _: &Request,
            _: &Cancellation,
            _: &RunLimits,
            _: &mut dyn recipe::Observer,
        ) -> Result<Turn, Cause> {
            self.sent += 1;
            let item = match self.script.pop_front() {
                Some(None) => {
                    return Err(crate::responses::Transient::transport(std::io::Error::from(
                        std::io::ErrorKind::ConnectionReset,
                    ))
                    .into());
                }
                Some(Some(item)) => item,
                None => Item::assistant("Done"),
            };
            Ok(Turn {
                items: vec![item],
                usage: recipe::Usage::default(),
                receipt: Value::Null,
            })
        }
    }

    async fn run(provider: &mut Replies) -> Result<Turn, Failure> {
        run_work(
            provider,
            &mut Owner,
            "retries",
            request().freeze().unwrap(),
            &mut WorkProgress::default(),
            RunLimits::default(),
            &Cancellation::default(),
        )
        .await
    }

    #[tokio::test(start_paused = true)]
    async fn a_failed_fifth_retry_fails_the_attempt_without_a_seventh_request() {
        let mut provider = Replies::new(std::iter::repeat_n(None, 6));
        let error = run(&mut provider).await.unwrap_err();
        assert!(
            error.transport_retry(),
            "the provider error ends it: {error}"
        );
        assert_eq!(
            AttemptFailure::of(&error, &Cancellation::default()),
            AttemptFailure::Failed
        );
        assert_eq!(provider.sent, 6, "a try and five retries, no seventh");
    }

    #[tokio::test(start_paused = true)]
    async fn a_completed_response_between_failures_resets_the_count() {
        let mut provider = Replies::new(
            std::iter::repeat_n(None, 5)
                .chain([Some(Item::Call(call("read")))])
                .chain(std::iter::repeat_n(None, 6)),
        );
        let error = run(&mut provider).await.unwrap_err();
        assert_eq!(
            AttemptFailure::of(&error, &Cancellation::default()),
            AttemptFailure::Failed
        );
        assert_eq!(provider.sent, 12, "five, a response, then six in a row");
    }

    #[tokio::test(start_paused = true)]
    async fn a_successful_exchange_resets_transport_retry_backoff() {
        #[derive(Default)]
        struct TimedProvider(Vec<Instant>);
        impl Provider for TimedProvider {
            async fn exchange(
                &mut self,
                _: &Request,
                _: &Cancellation,
                _: &RunLimits,
                _: &mut dyn recipe::Observer,
            ) -> Result<Turn, Cause> {
                self.0.push(Instant::now());
                match self.0.len() {
                    1 | 2 | 4 => Err(crate::responses::Transient::transport(std::io::Error::from(
                        std::io::ErrorKind::ConnectionReset,
                    ))
                    .into()),
                    3 => Ok(Turn {
                        items: vec![Item::Call(call("read"))],
                        usage: recipe::Usage::default(),
                        receipt: Value::Null,
                    }),
                    5 => Ok(Turn {
                        items: vec![Item::assistant("Done")],
                        usage: recipe::Usage::default(),
                        receipt: Value::Null,
                    }),
                    _ => panic!("unexpected request"),
                }
            }
        }
        let mut provider = TimedProvider::default();
        run_work(
            &mut provider,
            &mut Owner,
            "backoff",
            request().freeze().unwrap(),
            &mut WorkProgress::default(),
            RunLimits::default(),
            &Cancellation::default(),
        )
        .await
        .unwrap();
        let gaps: Vec<_> = provider
            .0
            .windows(2)
            .map(|pair| pair[1] - pair[0])
            .collect();
        assert_eq!(
            gaps,
            vec![
                Duration::from_millis(250),
                Duration::from_millis(500),
                Duration::ZERO,
                Duration::from_millis(250)
            ],
            "a successful exchange must restart transport backoff at 250 ms"
        );
    }
}
