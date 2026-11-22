//! One bounded execution loop. Hosts own durable truth and every effect.
//!
//! Transient provider requests retry only before visible output. Dropping a tool
//! future does not establish that its effect was rolled back. After any failure
//! following `ToolStarted`, the host reconciles the original operation before
//! deciding whether to resume. Persisted events are execution evidence, not a
//! product lifecycle. A candidate must still pass the product's commit fence.

use std::{
    collections::HashSet,
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::{sync::watch, time::Instant};

/// Model-window measurement, kept separate from cumulative token budgets and transport bounds.
pub mod context;
mod exchange;
/// Shared cursor protocol for exact product-owned conversation reads.
pub mod history;
mod limits;
/// Frozen recipes, materialized requests, token usage and the provider contract.
pub mod recipe;
/// Reconstruct continuations from product-owned execution and message facts.
pub mod recovery;
mod request_parts;
/// Bounded access to the authorized receipts of this work.
pub mod results;
/// Shared run lifecycle, persisted at each product's accepted-work owner.
pub mod runs;
mod windows;
mod work;
pub use limits::{LimitBackstop, ReachedBackstops, RunLimits};
use recipe::Window;
pub use recipe::{Product, ToolOutcome};
pub use request_parts::{RequestItem, SavedRequest};
pub use windows::{FINAL_REQUEST, LIMIT_WARNING, handoff_ask, handoff_ending, offer_working_notes};
pub use windows::{
    NoteReference, ReadWorkingNote, SaveWorkingNote, WindowSeed, WorkingNoteReceipt,
};
pub use work::{AcceptedInput, PendingFinal, Work, pending_final, tool_is_read_only};
pub(crate) use work::{WorkProgress, run_work};

/// A provider can author assistant output and calls. Only a product supplies results.
pub(crate) fn validate_output(items: &[Item]) -> Result<(), &'static str> {
    if items.contains(&Item::OutputCutoff) && items.len() != 1 {
        return Err("incomplete output cannot include executable items");
    }
    for item in items {
        match item {
            Item::Result { .. } | Item::MaterializedResult { .. } => {
                return Err("provider authored a tool result");
            }
            Item::Feedback { .. } | Item::FinishRefused(_) => {
                return Err("provider authored harness feedback");
            }
            Item::ProductMessage(_) => return Err("provider authored a product message identity"),
            Item::Message { role, .. } if role != "assistant" => {
                return Err("provider authored a non-assistant message");
            }
            Item::OutputCutoff | Item::Message { .. } | Item::Call(_) | Item::Opaque(_) => (),
        }
    }
    Ok(())
}

/// SQLSTATEs for transient acknowledgment failures. Products classify their
/// driver and pool errors and keep the original cause when requesting a retry.
pub fn retryable_sqlstate(code: Option<&str>) -> bool {
    matches!(
        code,
        Some(
            "08000"
                | "08001"
                | "08003"
                | "08006"
                | "08007"
                | "40001"
                | "40P01"
                | "55P03"
                | "53300"
                | "57P01"
                | "57P02"
                | "57P03"
        )
    )
}

/// A cooperative stop signal, independent of any product terminal state.
/// Hosting cancels through it; the running loop marks through it each step it
/// reaches, so hosting can tell a run that stopped moving.
#[derive(Clone, Debug)]
pub struct Cancellation {
    run: watch::Sender<bool>,
    request: Option<watch::Receiver<bool>>,
    liveness: Liveness,
}

impl Default for Cancellation {
    fn default() -> Self {
        Self {
            run: watch::channel(false).0,
            request: None,
            liveness: Liveness::default(),
        }
    }
}

/// When the running loop last reached a step, shared by every clone of one
/// [`Cancellation`]. A step is a saved event, a piece of the model's stream or
/// a currency check, which runs every second while the loop waits. Creating
/// the signal counts as the first step.
#[derive(Clone, Debug)]
struct Liveness {
    origin: Instant,
    /// Milliseconds from `origin` to the last step.
    last: Arc<AtomicU64>,
}

impl Default for Liveness {
    fn default() -> Self {
        Self {
            origin: Instant::now(),
            last: Arc::default(),
        }
    }
}

impl Cancellation {
    /// Requests cancellation. An already committed effect is not undone.
    pub fn cancel(&self) {
        self.run.send_replace(true);
    }

    /// Whether this run has been cancelled.
    pub fn is_cancelled(&self) -> bool {
        *self.run.borrow() || self.request.as_ref().is_some_and(|signal| *signal.borrow())
    }

    /// Records that the running loop reached a step now.
    pub(crate) fn mark(&self) {
        let since = Instant::now().saturating_duration_since(self.liveness.origin);
        let millis = u64::try_from(since.as_millis()).unwrap_or(u64::MAX);
        self.liveness.last.fetch_max(millis, Ordering::Relaxed);
    }

    /// How long ago the running loop last reached a step. Hosting reads it to
    /// tell a run that stopped moving.
    pub fn since_last_step(&self) -> Duration {
        let last = self.liveness.origin
            + Duration::from_millis(self.liveness.last.load(Ordering::Relaxed));
        Instant::now().saturating_duration_since(last)
    }

    /// Refuses a dispatch after cancellation.
    pub fn check(&self) -> Result<(), Failure> {
        if self.is_cancelled() {
            Err(Failure::Cancelled)
        } else {
            Ok(())
        }
    }

    // Losing a product fence stops this exchange. Only the product can cancel
    // the entire run, whose cancellation handle can outlive the exchange.
    fn for_request(&self) -> (Self, watch::Sender<bool>) {
        let (signal, request) = watch::channel(false);
        (
            Self {
                run: self.run.clone(),
                request: Some(request),
                liveness: self.liveness.clone(),
            },
            signal,
        )
    }
    pub(crate) async fn cancelled(&self) {
        let mut run = self.run.subscribe();
        let mut request = self.request.clone();
        tokio::select! {
            _ = run.wait_for(|flag|*flag) => (),
            () = async {
                if let Some(signal) = &mut request { let _ = signal.wait_for(|flag|*flag).await; }
                else { std::future::pending().await }
            } => (),
        }
    }
}

/// Current authorized text resolved by the product's canonical message store.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductMessage {
    /// Stable message identity within this product conversation or work.
    pub id: String,
    /// Authorship supplied by the product.
    pub role: history::Role,
    /// Exact current text; never copied into a dispatch record.
    pub text: String,
}

/// Provider-neutral conversation item. Opaque items retain provider continuation
/// data, never authority to execute a tool.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Item {
    /// Authored product message; dispatch records retain only its identity.
    ProductMessage(ProductMessage),
    /// Harness-owned continuation feedback, retained at the end of a fresh window.
    /// The provider boundary sends it as developer guidance.
    Feedback {
        /// Exact continuation content.
        text: String,
    },
    /// A product refused a finish proposed without calls. The harness renders
    /// it as developer feedback only at the provider boundary.
    FinishRefused(recipe::Refusal),
    /// Provider hit its output limit; the receipt retains the incomplete output.
    OutputCutoff,
    /// Authored message; input roles are user, assistant or developer.
    Message {
        /// Wire role.
        role: String,
        /// Authored content.
        text: String,
    },
    /// Provider-requested tool call.
    Call(Invocation),
    /// Exactly one result for a call.
    Result {
        /// Matching call identity.
        call_id: String,
        /// Typed model-visible projection of an exact retained result.
        output: results::ResultView,
    },
    /// Provider-wire form of a typed result. The harness creates this only after
    /// it has durably recorded the request's [`results::ResultView`].
    MaterializedResult {
        /// Matching call identity.
        call_id: String,
        /// Exact provider-facing bytes encoded from the typed projection.
        output: String,
    },
    /// Provider-specific continuation item, for example reasoning metadata.
    Opaque(Value),
}

impl Item {
    /// The provider-requested call this item holds, if it is one.
    pub fn call(&self) -> Option<&Invocation> {
        match self {
            Self::Call(call) => Some(call),
            Self::ProductMessage(_)
            | Self::Feedback { .. }
            | Self::FinishRefused(_)
            | Self::OutputCutoff
            | Self::Message { .. }
            | Self::Result { .. }
            | Self::MaterializedResult { .. }
            | Self::Opaque(_) => None,
        }
    }

    /// A message authored by the assistant.
    pub fn assistant(text: impl Into<String>) -> Self {
        Self::Message {
            role: "assistant".into(),
            text: text.into(),
        }
    }
}

/// An invocation identity is unique within an execution attempt. Products supply
/// their own durable effect identity; a provider call ID is not an idempotency key.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Invocation<A = String> {
    /// Provider call identity.
    pub id: String,
    /// Tool name.
    pub name: String,
    /// Raw provider text on the wire, or a validated object at the tool boundary.
    pub arguments: A,
}

impl Invocation {
    pub(crate) fn parse_arguments(&self) -> Result<Value, &'static str> {
        serde_json::from_str::<Value>(&self.arguments)
            .ok()
            .filter(Value::is_object)
            .ok_or("arguments must be a JSON object")
    }
}

/// A tool offered by the product, not inferred from a skill or provider response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Tool {
    /// Unique callable name.
    pub name: String,
    /// Model-facing description.
    pub description: String,
    /// JSON schema. The host validates product-specific argument semantics.
    pub parameters: Value,
}

impl Tool {
    /// Declares this product tool to the Responses protocol.
    pub fn function(&self) -> Value {
        serde_json::json!({"type":"function", "name":self.name, "description":self.description, "parameters":self.parameters})
    }
}

/// A completed provider exchange; neither text nor tool calls are committed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Turn {
    /// Ordered output, including opaque continuation items.
    #[serde(deserialize_with = "read_output")]
    pub items: Vec<Item>,
    /// Provider-owned usage/response evidence for the host's record.
    pub receipt: Value,
    /// Provider-reported token counts. A missing observation stays unknown.
    #[serde(default)]
    pub usage: recipe::Usage,
}

fn read_output<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<Item>, D::Error> {
    let items = Vec::deserialize(d)?;
    validate_output(&items).map_err(serde::de::Error::custom)?;
    Ok(items)
}

impl Turn {
    /// Authored prose in provider order.
    pub fn text(&self) -> String {
        self.items
            .iter()
            .filter_map(|item| match item {
                Item::Message { text, .. } => Some(text.as_str()),
                Item::ProductMessage(_)
                | Item::Feedback { .. }
                | Item::FinishRefused(_)
                | Item::OutputCutoff
                | Item::Call(_)
                | Item::Result { .. }
                | Item::MaterializedResult { .. }
                | Item::Opaque(_) => None,
            })
            .collect()
    }

    /// Keep the authored text and billing evidence of a reserved final. Its
    /// raw calls remain in the run record and confer no execution authority.
    fn handoff(mut self) -> Self {
        self.items
            .retain(|item| matches!(item, Item::Message { .. }));
        self
    }

    /// A completed exchange with no calls or substantive assistant text.
    pub fn is_empty(&self) -> bool {
        !self.items.iter().any(|item| match item {
            Item::Call(_) => true,
            Item::Message { text, .. } => !text.trim().is_empty(),
            Item::ProductMessage(_)
            | Item::Feedback { .. }
            | Item::FinishRefused(_)
            | Item::OutputCutoff
            | Item::Result { .. }
            | Item::MaterializedResult { .. }
            | Item::Opaque(_) => false,
        })
    }

    // Only valid provider turns reach this point. Preserve the turn itself and
    // pair every rejected call before the next dispatch.
    fn correction(&self) -> Option<Vec<Item>> {
        if self.items == [Item::OutputCutoff] {
            Some(vec![Item::Feedback { text: "Your output was cut off at the provider limit. Re-issue any tool call with complete arguments or provide a complete answer.".into() }])
        } else if self.is_empty() {
            Some(vec![Item::Feedback { text: "Your last turn contained no answer or tool call. Provide an answer or use an offered tool.".into() }])
        } else {
            None
        }
    }

    fn feedback(&self, value: &Value) -> Vec<Item> {
        let output = value.to_string();
        if self.calls().next().is_none() {
            return vec![Item::Feedback { text: output }];
        }
        self.calls()
            .map(|call| Item::Result {
                call_id: call.id.clone(),
                output: results::ResultView::inline(output.clone()),
            })
            .collect()
    }

    /// A refused finish stays typed under each call, or as one feedback item.
    fn refused(&self, refusal: &recipe::Refusal) -> Vec<Item> {
        if self.calls().next().is_none() {
            return vec![Item::FinishRefused(refusal.clone())];
        }
        self.calls()
            .map(|call| Item::Result {
                call_id: call.id.clone(),
                output: results::ResultView::finish_refused(refusal.clone()),
            })
            .collect()
    }

    /// Calls in provider order, including calls whose arguments need correction.
    pub fn calls(&self) -> impl Iterator<Item = &Invocation> {
        self.items.iter().filter_map(Item::call)
    }

    /// The items that can enter context, in provider order. The cut-off
    /// marker stays in the record only; `window_items` refuses it.
    fn context_items(&self) -> impl Iterator<Item = &Item> {
        self.items
            .iter()
            .filter(|item| **item != Item::OutputCutoff)
    }
}

/// A text fragment and the provider location when the transport supplied one.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextDelta {
    /// Exact authored fragment.
    pub text: String,
    /// Provider identity and location. Some transports expose only the text.
    #[serde(flatten)]
    pub location: Option<DeltaLocation>,
}

impl From<String> for TextDelta {
    fn from(text: String) -> Self {
        Self {
            text,
            location: None,
        }
    }
}

impl From<&str> for TextDelta {
    fn from(text: &str) -> Self {
        text.to_owned().into()
    }
}

/// Coordinates supplied with an identified streamed fragment.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeltaLocation {
    /// Provider response identity.
    pub response_id: String,
    /// Provider output item identity.
    pub item_id: String,
    /// Provider event sequence.
    pub sequence: u64,
    /// Output item position.
    pub output_index: usize,
    /// Content position.
    pub content_index: usize,
}

/// Execution evidence awaited before dependent operations. Sequence numbers are
/// scoped to the owner-provided attempt, not a new durable state machine.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Event {
    /// Owner-provided attempt identity.
    pub attempt_id: String,
    /// Monotonic, one-based event ordinal within this invocation of `run`.
    pub sequence: u64,
    /// Provider request this fact belongs to, including calls restored across attempts.
    pub ordinal: u64,
    /// Evidence to persist through the product owner.
    pub kind: EventKind,
}

/// Why the harness prepared this request.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RequestPurpose {
    /// An ordinary model step.
    #[default]
    Work,
    /// The model can answer or save a note for its next window.
    Handoff,
    /// Retain authored text and execute no calls.
    Final {
        /// The backstops that reserved this dispatch.
        backstops: ReachedBackstops,
    },
}

/// Transient instructions applied to one provider request. The event keeps
/// its exact wire-facing items and enough information to continue cleanly.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DispatchContext {
    /// Measured and estimated context at this exact dispatch.
    pub usage: context::ContextUsage,
    /// Newly accepted product input included in this dispatch, in delivery order.
    pub carried_inputs: Vec<String>,
    /// A final or handoff request has one appended developer instruction.
    pub purpose: RequestPurpose,
    /// This request first added the near-limit warning to the growing context.
    pub warning_added: bool,
    /// Saved with the dispatch so a restart cannot renew the refusal allowance.
    pub window_stage: WindowStage,
}

/// Where this request stands in the bounded fresh-window refusal sequence.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum WindowStage {
    /// Ordinary accumulated work has not yet been reopened for this request.
    #[default]
    Accumulated,
    /// The exact fresh seed and pending results have not been accepted yet.
    Fresh,
    /// The one retry uses previews and shorter complete pages.
    Reduced,
}

/// Neutral event payloads. The product decides retention and redaction.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum EventKind {
    /// Must be acknowledged before dispatch; owner may reserve durable budget.
    ProviderStarted {
        /// Unix-millisecond admission time, retained unchanged on persistence retries.
        dispatched_at_unix_ms: u64,
        /// Typed projection of this dispatch's temporary instructions.
        context: DispatchContext,
        /// Exact context references; the recipe lives once with the accepted work.
        request: SavedRequest,
    },
    /// Identity or usage available before a completed exchange can be retained.
    /// A terminal receipt precedes validation of the completed output.
    /// A buffered success retains the receipt directly in `ProviderReturned`.
    ProviderObservation {
        /// Provider identity, status, and any reported usage.
        receipt: Value,
        /// Cumulative token counts available at this observation.
        usage: recipe::Usage,
    },
    /// The provider announced a call of this request, named `name`, before
    /// its arguments were complete. It is not the call: the returned turn holds
    /// the calls the run makes.
    CallBegan {
        /// The name the announced call gives.
        name: String,
    },
    /// Transport failed after dispatch. Later receipts still belong to the same request.
    ProviderFailed {
        /// Failure reported by the transport.
        reason: String,
    },
    /// No visible output was produced. The provider can retry this model step.
    TransportRetry {
        /// Provider failure retained for accounting and inspection.
        reason: String,
    },
    /// Must be acknowledged before interpreting calls or returning a candidate.
    ProviderReturned(Turn),
    /// Must be acknowledged before an effect; not proof that it happened.
    ToolStarted(Invocation),
    /// Includes rejected calls and results whose delivery was refused by bounds.
    ToolReturned {
        /// Original call identity.
        call_id: String,
        /// Exact original, a typed projection of an earlier retained result,
        /// or the typed refusal of an operation that did not run.
        result: results::ToolResult,
    },
    /// Provider requires operator intervention; the product atomically holds
    /// automatic recovery when acknowledging this fact under its current fence.
    ProviderBlocked {
        /// Actionable provider failure, with no secret response body.
        reason: String,
    },
    /// This request was too long. Continue the same work in a fresh window.
    ContextRejected,
    /// Candidate evidence only; never a product completion event.
    Candidate {
        /// Candidate authored text from the terminal turn.
        text: String,
    },
    /// A fresh window opened. It carries the newest note saved in this work.
    WindowOpened {
        /// Wall-clock time when the fresh window opened.
        opened_at_unix_ms: u64,
        /// Why this boundary was opened.
        reason: WindowReason,
        /// Product-confirmed note saved for this change, when the model asked.
        handoff: Option<NoteReference>,
    },
}

impl EventKind {
    /// Receipts remain inspectable after currency is lost; new work and
    /// candidate direction require a current attempt.
    pub fn survives_loss_of_currency(&self) -> bool {
        match self {
            Self::ProviderObservation { .. }
            | Self::CallBegan { .. }
            | Self::ProviderReturned(_)
            | Self::ProviderFailed { .. }
            | Self::TransportRetry { .. }
            | Self::ToolReturned { .. }
            | Self::ProviderBlocked { .. }
            | Self::ContextRejected => true,
            Self::ProviderStarted { .. }
            | Self::ToolStarted(_)
            | Self::Candidate { .. }
            | Self::WindowOpened { .. } => false,
        }
    }
}

/// The cause of a fresh working window, preserved with its opening.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowReason {
    /// The model saved a note and requested a window.
    ModelAsked,
    /// The harness asked for a handoff near the window limit.
    NearEnd,
    /// Local measurement found the window full.
    Full,
    /// The provider refused the request as too long.
    Refused,
}

/// Original error supplied by a product or provider adapter.
pub type Cause = Box<dyn std::error::Error + Send + Sync>;

/// Why the work no longer wants this attempt. The product's currency owner
/// gives it, and the harness matches on it. It is not an [`std::error::Error`],
/// so it cannot be boxed into a [`Cause`] and lost in an error chain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotCurrent {
    /// New input replaced the direction. Tool calls already running finish
    /// within their deadline; no new call starts.
    Superseded,
    /// The work no longer wants this attempt: the person stopped it, or the
    /// work ended, even when newer input waits. The loop does not wait for
    /// running tool calls; each stays a call that started with no recorded
    /// outcome.
    Withdrawn,
    /// Another attempt holds the work, or this attempt's lease ran out. An
    /// operational failure.
    Lost,
}

impl std::fmt::Display for NotCurrent {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Superseded => "accepted work or inputs no longer current",
            Self::Withdrawn => "accepted work no longer wants this attempt",
            Self::Lost => "attempt ownership lost",
        })
    }
}

/// The error of every [`Product`] callback: the work no longer wants this
/// attempt, or the callback failed. It is not an [`std::error::Error`], so the
/// currency answer cannot hide in a chain; a caller that needs an error
/// converts it into a [`Failure`].
#[derive(Debug)]
pub enum ProductError {
    /// The work no longer wants this attempt.
    NotCurrent(NotCurrent),
    /// The callback failed; durable writes may need reconciliation.
    Failed(Cause),
}

impl From<NotCurrent> for ProductError {
    fn from(answer: NotCurrent) -> Self {
        Self::NotCurrent(answer)
    }
}

impl<E: Into<Cause>> From<E> for ProductError {
    fn from(error: E) -> Self {
        Self::Failed(error.into())
    }
}

impl std::fmt::Display for ProductError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotCurrent(answer) => answer.fmt(formatter),
            Self::Failed(error) => error.fmt(formatter),
        }
    }
}

impl From<ProductError> for Failure {
    fn from(error: ProductError) -> Self {
        match error {
            ProductError::NotCurrent(answer) => Self::NotCurrent(answer),
            ProductError::Failed(cause) => Self::Host(cause),
        }
    }
}

/// A transient ordered-event write failure, classified by the product storage owner.
/// The identical event may be saved again: products must acknowledge exact replay
/// without repeating any related effects, including an unknown commit outcome.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct RetryablePersistence(#[source] Cause);
impl RetryablePersistence {
    /// Preserve the original storage failure as the source.
    pub fn new(cause: impl Into<Cause>) -> Self {
        Self(cause.into())
    }
}

/// Why execution stopped, without deciding a product terminal state.
#[derive(Debug, thiserror::Error)]
pub enum Failure {
    /// Cooperative cancellation.
    #[error("execution cancelled")]
    Cancelled,
    /// One of the two runaway backstops was reached.
    #[error("{0}")]
    Limit(String),
    /// A final request or the reduced fresh window was refused by the provider.
    #[error("provider refused the last permitted context request")]
    ContextRefused,
    /// The reserved final dispatch failed before it yielded a handoff.
    #[error("{0}")]
    Final(#[source] Box<Failure>),
    /// Provider or context protocol integrity failed.
    #[error("{0}")]
    Protocol(String),
    /// A provider failure; may have dispatched and consumed provider usage.
    #[error("{0}")]
    Provider(#[source] Cause),
    /// Transport failed after acknowledged visible output; never replay this request.
    #[error("{0}")]
    AfterOutput(#[source] Cause),
    /// The work no longer wants this attempt, as its product answered.
    #[error("{0}")]
    NotCurrent(NotCurrent),
    /// Product callback failure; durable writes may need reconciliation.
    #[error("{0}")]
    Host(#[source] Cause),
    /// A materialized request or response could not be encoded.
    #[error("{0}")]
    Encoding(#[source] serde_json::Error),
}

impl Failure {
    fn transport_retry(&self) -> bool {
        matches!(self, Self::Provider(cause) if cause.is::<crate::responses::Transient>())
    }
}

/// Automatic recovery stops after five actual failed attempts. Explicit Try
/// again can resume the retained work, as specified in `intent/harness.md`.
pub const MAX_FAILED_ATTEMPTS: u32 = 5;

/// Shared classification of an attempt that did not complete. Products retain
/// authority checks and map this reason to their own durable lifecycle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttemptFailure {
    /// Cancellation, superseding input or withdrawn work, not a failed attempt.
    Interrupted,
    /// A reached bound, including an unsuccessful reserved final request.
    Limit,
    /// An operational failure after in-run transport recovery.
    Failed,
    /// An active execution disappeared before hosting could settle it.
    Abandoned,
    /// The provider requires operator action before another attempt.
    ProviderBlocked,
}

impl AttemptFailure {
    /// The kind of ending a run's failure is. Cancellation takes precedence.
    /// Hosting supplies the same cancellation signal the run used; display
    /// text is never read.
    pub fn of(failure: &Failure, cancel: &Cancellation) -> Self {
        if cancel.is_cancelled() {
            return Self::Interrupted;
        }
        let blocked = crate::responses::provider_blocked(failure);
        match failure {
            Failure::Cancelled
            | Failure::NotCurrent(NotCurrent::Superseded | NotCurrent::Withdrawn) => {
                Self::Interrupted
            }
            Failure::NotCurrent(NotCurrent::Lost) => Self::Failed,
            _ if blocked => Self::ProviderBlocked,
            Failure::Limit(_) | Failure::Final(_) | Failure::ContextRefused => Self::Limit,
            Failure::Protocol(_)
            | Failure::Provider(_)
            | Failure::AfterOutput(_)
            | Failure::Host(_)
            | Failure::Encoding(_) => Self::Failed,
        }
    }

    /// Only actual failures consume the shared automatic recovery allowance.
    pub const fn counts_as_failure(self) -> bool {
        match self {
            Self::Failed | Self::Abandoned => true,
            Self::Interrupted | Self::Limit | Self::ProviderBlocked => false,
        }
    }
}

fn checked_calls(turn: &Turn) -> Result<Vec<Invocation>, Failure> {
    validate_output(&turn.items).map_err(|reason| Failure::Protocol(reason.into()))?;
    Ok(turn.calls().cloned().collect())
}

/// Give every call in a new response an ID no other call in the run has, as
/// OpenClaw and Hermes do. A call keeps a nonempty ID that is new to the run
/// and to this response. An empty ID or a repeated one gets a new ID. The new
/// ID depends only on the response and the IDs already taken, so a retried
/// save is the same. This runs once, before the response is saved, so the
/// record, the result and every later request carry the same ID.
fn assign_call_ids<'a>(turn: &mut Turn, taken: impl IntoIterator<Item = &'a str>) {
    let mut taken: HashSet<String> = taken.into_iter().map(str::to_owned).collect();
    // A new ID never takes the ID a later call in this response brought.
    let mut reserved: HashSet<String> = turn
        .calls()
        .map(|call| call.id.clone())
        .filter(|id| !id.trim().is_empty() && !taken.contains(id))
        .collect();
    let mut position = 0;
    for item in &mut turn.items {
        let Item::Call(call) = item else {
            continue;
        };
        position += 1;
        if !call.id.trim().is_empty() && taken.insert(call.id.clone()) {
            reserved.remove(&call.id);
            continue;
        }
        let base = if call.id.trim().is_empty() {
            format!("call_{position}")
        } else {
            call.id.clone()
        };
        let mut id = base.clone();
        let mut suffix = 2;
        while taken.contains(&id) || reserved.contains(&id) {
            id = format!("{base}_d{suffix}");
            suffix += 1;
        }
        taken.insert(id.clone());
        call.id = id;
    }
}

fn work_response_bound(turn: &Turn, max: usize) -> Result<(), Failure> {
    let bytes = serde_json::to_vec(turn).map_err(Failure::Encoding)?;
    if bytes.len() > max {
        return Err(Failure::Protocol(
            "execution context byte bound reached".into(),
        ));
    }
    Ok(())
}

/// Items enter a window only through here: the product's items on a fresh
/// start, a saved request on recovery, a rearmed allowance, accepted input and
/// a fresh window's seed. Checks call identities, message roles and that only
/// retained forms enter the context, then matches every call with one result.
/// Window handoff, result sizing, the size estimate, the saved request and
/// dispatch all read these same items.
pub(crate) fn window_items(items: Vec<Item>) -> Result<Vec<Item>, Failure> {
    let mut seen = HashSet::new();
    for item in &items {
        match item {
            Item::OutputCutoff => {
                return Err(Failure::Protocol(
                    "incomplete turn cannot enter dispatched context".into(),
                ));
            }
            Item::Call(call) => {
                if call.id.trim().is_empty() || !seen.insert(call.id.as_str()) {
                    return Err(Failure::Protocol(
                        "duplicate/empty context call identity".into(),
                    ));
                }
            }
            Item::MaterializedResult { .. } => {
                return Err(Failure::Protocol(
                    "provider-wire result entered retained context".into(),
                ));
            }
            Item::Message { role, .. } => {
                if !matches!(role.as_str(), "user" | "assistant" | "developer" | "system") {
                    return Err(Failure::Protocol("invalid message role".into()));
                }
            }
            Item::Feedback { .. }
            | Item::ProductMessage(_)
            | Item::Result { .. }
            | Item::FinishRefused(_)
            | Item::Opaque(_) => (),
        }
    }
    Ok(request_parts::paired(items))
}
