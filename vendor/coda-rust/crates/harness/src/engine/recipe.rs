//! A product freezes its job recipe. The harness constructs each materialized provider request.
pub use super::Cancellation;
use super::*;
pub use crate::engine::{Item, Turn};
impl From<Invocation<Value>> for Item {
    fn from(call: Invocation<Value>) -> Self {
        Self::Call(crate::engine::Invocation {
            id: call.id,
            name: call.name,
            arguments: call.arguments.to_string(),
        })
    }
}

impl crate::engine::Invocation {
    /// Check the argument object before admitting this invocation to a product tool.
    pub fn parsed(&self) -> Result<Invocation<Value>, &'static str> {
        Ok(Invocation {
            id: self.id.clone(),
            name: self.name.clone(),
            arguments: self.parse_arguments()?,
        })
    }
}

/// A provider request materialized by the harness from the frozen recipe and current window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    /// Product-owned conversation or job partition, retained across tool turns.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_key: Option<crate::PromptCacheKey>,
    /// Model binding, never selected by the engine.
    pub model: String,
    /// Product-selected working window, measured with the shared estimate.
    pub context_window_tokens: u64,
    /// Provider input ceiling, checked against the estimated dispatch size.
    pub max_input_tokens: u64,
    /// Provider-specific inference options.
    pub options: Value,
    /// Product instructions, preserved across exchanges.
    pub instructions: String,
    /// Tool schemas authorized by the product for this work.
    pub tools: Vec<Value>,
    /// Exact context; the engine never truncates or summarizes it.
    pub items: Vec<Item>,
}

/// Immutable provider binding and instructions saved once by the product for
/// accepted work. Every dispatch references this same recipe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RequestRecipe {
    /// Product-selected model.
    pub model: String,
    /// Product-selected working window.
    pub context_window_tokens: u64,
    /// Provider input limit.
    pub max_input_tokens: u64,
    /// Exact provider inference options.
    pub options: Value,
    /// Exact resident instructions.
    pub instructions: String,
    /// Exact admitted wire schemas.
    pub tools: Vec<Value>,
}

impl RequestRecipe {
    /// Bind shared policy to the cache scope accepted for this run.
    pub fn bind(&self, cache_key: Option<crate::PromptCacheKey>) -> RequestBinding {
        RequestBinding {
            recipe: self.clone(),
            cache_key,
        }
    }
}

/// The shared policy and scope accepted for one run. Cache identity is a fact
/// about this binding, never part of the reusable recipe.
#[derive(Debug, Clone, PartialEq)]
pub struct RequestBinding {
    /// Reusable provider policy, independent of the run's scope.
    pub recipe: RequestRecipe,
    /// Exact conversation or job cache scope accepted for this run.
    pub cache_key: Option<crate::PromptCacheKey>,
}

impl RequestBinding {
    /// Materialize the accepted binding with exact saved or newly prepared items.
    pub fn request(&self, items: Vec<Item>) -> Request {
        Request {
            cache_key: self.cache_key.clone(),
            model: self.recipe.model.clone(),
            context_window_tokens: self.recipe.context_window_tokens,
            max_input_tokens: self.recipe.max_input_tokens,
            options: self.recipe.options.clone(),
            instructions: self.recipe.instructions.clone(),
            tools: self.recipe.tools.clone(),
            items,
        }
    }
}

impl Request {
    pub(super) fn for_provider(&self) -> Self {
        let mut request = self.clone();
        for item in &mut request.items {
            *item = match item {
                Item::ProductMessage(message) => Item::Message {
                    role: message.role.as_str().into(),
                    text: message.text.clone(),
                },
                Item::Feedback { text } => Item::Message {
                    role: "developer".into(),
                    text: text.clone(),
                },
                Item::FinishRefused(refusal) => Item::Message {
                    role: "developer".into(),
                    text: refusal.feedback().to_string(),
                },
                Item::OutputCutoff
                | Item::Message { .. }
                | Item::Call(_)
                | Item::Result { .. }
                | Item::MaterializedResult { .. }
                | Item::Opaque(_) => continue,
            };
        }
        request
    }
    /// The immutable ingredient owned by the product's accepted work.
    pub fn recipe(&self) -> RequestRecipe {
        RequestRecipe {
            model: self.model.clone(),
            context_window_tokens: self.context_window_tokens,
            max_input_tokens: self.max_input_tokens,
            options: self.options.clone(),
            instructions: self.instructions.clone(),
            tools: self.tools.clone(),
        }
    }

    /// Accepted policy and cache scope, independent of the current window items.
    pub fn binding(&self) -> RequestBinding {
        self.recipe().bind(self.cache_key.clone())
    }
}

/// Product permission to use a saved event as continuing model context.
#[derive(Clone, Copy)]
pub enum ReplayEvent<'a> {
    /// Its content can supply direction and completed work.
    Admitted(&'a Event),
    /// It still settles the request, but its output cannot supply direction.
    EvidenceOnly(&'a Event),
}

impl<'a> ReplayEvent<'a> {
    pub(super) fn event(self) -> &'a Event {
        match self {
            Self::Admitted(event) | Self::EvidenceOnly(event) => event,
        }
    }

    /// Content usable by recovery after the shared replay rules are applied.
    pub(crate) fn admitted(self) -> Option<&'a Event> {
        match self {
            Self::Admitted(event) => Some(event),
            Self::EvidenceOnly(_) => None,
        }
    }
}

impl<'a> From<&'a (Event, bool)> for ReplayEvent<'a> {
    fn from((event, admitted): &'a (Event, bool)) -> Self {
        if *admitted {
            Self::Admitted(event)
        } else {
            Self::EvidenceOnly(event)
        }
    }
}

/// A result from an admitted call remains usable when it finishes after a steer.
/// Late model output still cannot supply new direction or executable calls.
pub(crate) fn replay_events<'a>(
    events: impl IntoIterator<Item = impl Into<ReplayEvent<'a>>>,
) -> impl Iterator<Item = ReplayEvent<'a>> {
    let mut started = std::collections::HashSet::new();
    events.into_iter().map(move |record| {
        let record = record.into();
        let event = record.event();
        match &event.kind {
            EventKind::ToolStarted(call) if record.admitted().is_some() => {
                started.insert((event.attempt_id.as_str(), call.id.as_str()));
            }
            EventKind::ToolReturned { call_id, .. }
                if started.contains(&(event.attempt_id.as_str(), call_id.as_str())) =>
            {
                return ReplayEvent::Admitted(event);
            }
            EventKind::ToolStarted(_)
            | EventKind::ToolReturned { .. }
            | EventKind::ProviderStarted { .. }
            | EventKind::ProviderObservation { .. }
            | EventKind::CallBegan { .. }
            | EventKind::ProviderFailed { .. }
            | EventKind::TransportRetry { .. }
            | EventKind::ProviderReturned(_)
            | EventKind::ProviderBlocked { .. }
            | EventKind::ContextRejected
            | EventKind::Candidate { .. }
            | EventKind::WindowOpened { .. } => (),
        }
        record
    })
}

/// Whether a dispatched exchange still needs its exact request retried.
#[derive(Debug, Clone)]
pub(crate) enum ExchangeState {
    /// No admitted answer survived; an eligible retry uses the immutable dispatch.
    Pending,
    /// A completed, product-admitted output can continue without another call.
    Returned(Turn),
    /// The exchange ended, but has no output admitted for continuing this work.
    Ended,
    /// The provider refused this window; recovery starts from the product seed.
    ContextRejected,
}

/// Retained protocol facts and product-admitted output for a successor.
#[derive(Clone, Debug)]
pub(crate) struct Recovery {
    /// Original product attempt that dispatched this exchange.
    pub attempt_id: String,
    /// Original provider request ordinal within that work record.
    pub ordinal: u64,
    /// Exact latest dispatched context and frozen recipe.
    pub request: Request,
    /// Terminal evidence distinguishes an unfinished dispatch from ended work.
    pub exchange: ExchangeState,
    /// The saved request refers to a message that is no longer current.
    pub sources_changed: bool,
    pub(super) meter: context::Meter,
    pub(super) context: DispatchContext,
    pub(super) tool_events: Vec<Event>,
}

/// Reconstruct one dispatch from ordered durable facts, without deciding
/// whether the product may execute, finish, or continue it.
pub(crate) fn recovery<'a>(
    binding: &RequestBinding,
    events: impl IntoIterator<Item = impl Into<ReplayEvent<'a>>>,
    messages: &[ProductMessage],
) -> Result<Option<Recovery>, Failure> {
    let mut current: Option<Recovery> = None;
    let mut meter = context::Meter::default();
    let mut parts = request_parts::Parts::new(messages);
    for recorded in replay_events(events) {
        let event = recorded.event();
        let admitted = recorded.admitted().is_some();
        if admitted {
            parts.observe(event)?;
        }
        match &event.kind {
            EventKind::WindowOpened { .. } if admitted => meter = context::Meter::default(),
            EventKind::ProviderStarted {
                request, context, ..
            } if admitted => {
                let (items, sources_changed) = parts.restore_current(request)?;
                current = Some(Recovery {
                    sources_changed,
                    attempt_id: event.attempt_id.clone(),
                    ordinal: event.ordinal,
                    request: binding.request(items),
                    exchange: ExchangeState::Pending,
                    meter: meter.clone(),
                    context: context.clone(),
                    tool_events: Vec::new(),
                });
            }
            EventKind::ToolStarted(_) | EventKind::ToolReturned { .. } if admitted => {
                if let Some(retained) = &mut current
                    && event.ordinal == retained.ordinal
                {
                    retained.tool_events.push(event.clone());
                }
            }
            EventKind::ProviderReturned(turn) => {
                if let Some(retained) = &mut current
                    && event.attempt_id == retained.attempt_id
                    && event.ordinal == retained.ordinal
                {
                    retained.exchange = if admitted {
                        meter.observe(&parts.for_provider(&retained.request)?, turn)?;
                        retained.meter = meter.clone();
                        ExchangeState::Returned(turn.clone())
                    } else {
                        ExchangeState::Ended
                    };
                }
            }
            EventKind::ProviderFailed { .. } | EventKind::TransportRetry { .. } => {
                if let Some(retained) = &mut current
                    && event.attempt_id == retained.attempt_id
                    && event.ordinal == retained.ordinal
                    && !admitted
                {
                    retained.exchange = ExchangeState::Ended;
                }
            }
            EventKind::ProviderBlocked { .. } => {
                if let Some(retained) = &mut current
                    && event.attempt_id == retained.attempt_id
                    && event.ordinal == retained.ordinal
                {
                    match retained.exchange {
                        ExchangeState::Pending => retained.exchange = ExchangeState::Ended,
                        ExchangeState::Returned(_)
                        | ExchangeState::Ended
                        | ExchangeState::ContextRejected => (),
                    }
                }
            }
            EventKind::ContextRejected => {
                if let Some(retained) = &mut current
                    && event.attempt_id == retained.attempt_id
                    && event.ordinal == retained.ordinal
                {
                    retained.exchange = ExchangeState::ContextRejected;
                }
            }
            EventKind::WindowOpened { .. }
            | EventKind::ProviderStarted { .. }
            | EventKind::ToolStarted(_)
            | EventKind::ToolReturned { .. }
            | EventKind::ProviderObservation { .. }
            | EventKind::CallBegan { .. }
            | EventKind::Candidate { .. } => (),
        }
    }
    Ok(current)
}

/// Measured usage. Unknown token counts stay unknown, never become zero.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    /// Measured input tokens, when supplied by the provider.
    pub input_tokens: Option<u64>,
    /// Reported input served from the provider cache.
    pub cached_input_tokens: Option<u64>,
    /// Measured output tokens, when supplied by the provider.
    pub output_tokens: Option<u64>,
}

/// Events are provisional; they confer no publication authority.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum StreamEvent {
    /// Incremental authored text, with the provider location when available.
    Text(crate::engine::TextDelta),
    /// Provider usage observation.
    Usage(Usage),
    /// The provider announced a function call, named `name`, before its
    /// arguments are complete. Its completed item stays the truth for the turn.
    CallBegan {
        /// The name the announced call gives.
        name: String,
    },
    /// Provider response identity observed before termination. Persist it for
    /// interrupted-call reconciliation; it is not a completion receipt.
    Receipt {
        /// Provider identity and provenance.
        identity: Value,
        /// Token counts observed in this receipt.
        usage: Usage,
    },
}

/// Provider transport. Poll `cancel` during long operations and call `emit` for
/// provisional streaming. Returning an error from `emit` must stop streaming.
pub trait Provider {
    /// Wait for the product quota before recording or dispatching a request.
    fn ready(&mut self) -> impl Future<Output = Result<(), Cause>> + Send {
        async { Ok(()) }
    }
    /// Executes exactly one exchange. No automatic retry or tool execution.
    fn exchange(
        &mut self,
        request: &Request,
        cancel: &Cancellation,
        limits: &RunLimits,
        observations: &mut dyn Observer,
    ) -> impl Future<Output = Result<Turn, Cause>> + Send;
}

/// How a terminal exchange settled.
#[derive(Debug, Clone, PartialEq)]
pub enum Finish {
    /// The product committed the completion.
    Committed,
    /// The product declined to commit. The value is returned to the model as
    /// feedback for this completion candidate, and the run continues.
    Continue(Value),
    /// A typed refusal of the completion, with the same shape as a refused tool.
    Refused(Refusal),
}

/// One call's reported usage and fallback estimates, reconstructed from its events.
#[derive(Default)]
pub(super) struct CallUsage {
    pub(super) usage: Usage,
    estimated_input: u64,
    accepted: bool,
    returned_output: Option<u64>,
}

impl CallUsage {
    pub(super) fn new(estimated_input: u64) -> Self {
        Self {
            estimated_input,
            ..Self::default()
        }
    }

    pub(super) fn observe(&mut self, event: &EventKind) {
        match event {
            EventKind::ProviderStarted { context, .. } => {
                self.estimated_input = context.usage.input_tokens;
            }
            EventKind::ProviderObservation { usage, receipt } => {
                self.accepted |= !receipt.is_null()
                    || usage.input_tokens.is_some()
                    || usage.output_tokens.is_some()
                    || usage.cached_input_tokens.is_some();
                self.usage.merge(usage);
            }
            EventKind::ProviderReturned(turn) => {
                self.accepted = true;
                self.usage.merge(&turn.usage);
                self.returned_output = context::estimate_items(&turn.items);
            }
            EventKind::CallBegan { .. }
            | EventKind::ProviderFailed { .. }
            | EventKind::TransportRetry { .. }
            | EventKind::ToolStarted(_)
            | EventKind::ToolReturned { .. }
            | EventKind::ProviderBlocked { .. }
            | EventKind::ContextRejected
            | EventKind::Candidate { .. }
            | EventKind::WindowOpened { .. } => (),
        }
    }

    fn tokens(&self) -> u64 {
        if !self.accepted {
            return 0;
        }
        let input = self.usage.input_tokens.unwrap_or(self.estimated_input);
        let output = self
            .usage
            .output_tokens
            .or(self.returned_output)
            .unwrap_or_default();
        // Only reported cache hits are excluded. Missing counts remain unknown
        // in Usage; estimates apply solely to the runaway backstop.
        input
            .saturating_sub(self.usage.cached_input_tokens.unwrap_or(0))
            .saturating_add(output)
    }
}

/// Restored counters from the product execution record, not a new run ledger.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Progress {
    /// Calls already dispatched (including failures).
    pub calls: u64,
    /// Dispatched transports known to have failed before visible output.
    pub transport_retries: u64,
    /// Sum of reported input and output tokens, including cached input and late responses.
    pub known_tokens: u64,
    /// Exchanges with at least one unknown input or output count.
    pub unknown_usage: u64,
    /// Output plus uncached input, using the shared estimate for missing counts.
    pub budget_tokens: u64,
}

impl Progress {
    /// Model steps counted against the request ceiling. Every physical dispatch
    /// retains its own identity and token observations.
    pub(crate) fn model_steps(&self) -> u64 {
        self.calls.saturating_sub(self.transport_retries)
    }
    /// Restore call and token accounting from neutral events in durable order.
    pub fn from_events<'a>(events: impl IntoIterator<Item = &'a crate::engine::Event>) -> Self {
        let mut calls = std::collections::BTreeMap::<(&str, u64), (CallUsage, bool)>::new();
        for event in events {
            match &event.kind {
                EventKind::ProviderStarted { .. }
                | EventKind::ProviderObservation { .. }
                | EventKind::ProviderReturned(_)
                | EventKind::TransportRetry { .. } => (),
                EventKind::CallBegan { .. }
                | EventKind::ProviderFailed { .. }
                | EventKind::ToolStarted(_)
                | EventKind::ToolReturned { .. }
                | EventKind::ProviderBlocked { .. }
                | EventKind::ContextRejected
                | EventKind::Candidate { .. }
                | EventKind::WindowOpened { .. } => continue,
            }
            let (usage, retry) = calls
                .entry((event.attempt_id.as_str(), event.ordinal))
                .or_default();
            usage.observe(&event.kind);
            if matches!(event.kind, EventKind::TransportRetry { .. }) {
                *retry = true;
            }
        }
        // Distinct attempts may restart ordinals; a suffix can start above one.
        let high_water = calls.keys().map(|(_, ordinal)| *ordinal).max().unwrap_or(0);
        let mut progress = Self {
            calls: high_water.max(calls.len() as u64),
            ..Self::default()
        };
        for (usage, retry) in calls.values() {
            progress.transport_retries += u64::from(*retry);
            progress.account(usage);
        }
        progress
    }

    pub(super) fn from_events_after<'a>(
        events: impl IntoIterator<Item = &'a crate::engine::Event>,
        allowance_after: u64,
    ) -> Self {
        let events: Vec<_> = events
            .into_iter()
            .filter(|event| event.ordinal > allowance_after)
            .collect();
        let actual = events
            .iter()
            .filter(|event| matches!(event.kind, EventKind::ProviderStarted { .. }))
            .map(|event| (event.attempt_id.as_str(), event.ordinal))
            .collect::<std::collections::BTreeSet<_>>()
            .len() as u64;
        let mut progress = Self::from_events(events.iter().copied());
        let relative_high_water = events
            .iter()
            .map(|event| event.ordinal.saturating_sub(allowance_after))
            .max()
            .unwrap_or(0);
        progress.calls = relative_high_water.max(actual);
        progress
    }

    pub(super) fn account(&mut self, call: &CallUsage) {
        self.budget_tokens = self.budget_tokens.saturating_add(call.tokens());
        self.known_tokens = self
            .known_tokens
            .saturating_add(call.usage.input_tokens.unwrap_or(0))
            .saturating_add(call.usage.output_tokens.unwrap_or(0));
        if call.usage.input_tokens.is_none() || call.usage.output_tokens.is_none() {
            self.unknown_usage += 1;
        }
    }
}

/// Receives at most one in-flight transport observation. A successful return
/// acknowledges live text delivery or durable retention of other facts.
#[async_trait]
pub trait Observer: Send {
    /// Deliver a provisional observation; this never publishes an answer.
    async fn emit(&mut self, event: StreamEvent) -> Result<(), Cause>;
}

#[async_trait]
impl<F: FnMut(StreamEvent) -> Result<(), Cause> + Send> Observer for F {
    async fn emit(&mut self, event: StreamEvent) -> Result<(), Cause> {
        self(event)
    }
}
/// Immutable model binding and instructions for one admitted job.
pub(crate) struct Recipe {
    model: String,
    pub(super) context_window_tokens: u64,
    max_input_tokens: u64,
    options: Value,
    instructions: String,
    tools: Vec<Tool>,
}
/// The admitted recipe and the exact items with which to enter the loop.
pub struct Window {
    pub(super) recipe: Recipe,
    pub(super) cache_key: Option<crate::PromptCacheKey>,
    pub(super) items: Vec<Item>,
    /// Items already sent in this window keep their exact representation.
    pub(super) sent_items: usize,
    pub(super) handoff_pending: bool,
    pub(super) stage: WindowStage,
    pub(super) meter: context::Meter,
}
impl Request {
    /// Separate the immutable recipe from the items the harness will extend.
    pub fn freeze(self) -> Result<Work, Failure> {
        Ok(Work {
            window: self.into_window()?,
            saved: None,
            tool_failures: Default::default(),
            notes: Vec::new(),
            parts: Default::default(),
            allowance_after: 0,
            partial_message: None,
            pending_input: Vec::new(),
            source_feedback: None,
        })
    }

    pub(super) fn into_window(mut self) -> Result<Window, Failure> {
        self.items = window_items(std::mem::take(&mut self.items))?;
        context::input_limit(&self)?;
        let Self {
            cache_key,
            model,
            context_window_tokens,
            max_input_tokens,
            options,
            instructions,
            tools,
            items,
        } = self;
        let tools = tools
            .into_iter()
            .map(|definition| {
                let tool: Tool =
                    serde_json::from_value(definition.clone()).map_err(Failure::Encoding)?;
                if tool.function() != definition {
                    return Err(Failure::Protocol(
                        "unsupported tool definition in frozen recipe".into(),
                    ));
                }
                Ok(tool)
            })
            .collect::<Result<_, Failure>>()?;
        Ok(Window {
            cache_key,
            recipe: Recipe {
                model,
                context_window_tokens,
                max_input_tokens,
                options,
                instructions,
                tools,
            },
            items,
            sent_items: 0,
            stage: WindowStage::Accumulated,
            handoff_pending: false,
            meter: context::Meter::default(),
        })
    }
}
impl Recipe {
    pub(super) fn working_windows(&self) -> bool {
        self.tools
            .iter()
            .any(|tool| tool.name == super::windows::SAVE)
    }

    pub(super) fn offers(&self, name: &str) -> bool {
        self.tools.iter().any(|tool| tool.name == name)
    }

    /// The names of the tools this job offers, in recipe order.
    pub(super) fn tool_names(&self) -> impl Iterator<Item = &str> {
        self.tools.iter().map(|tool| tool.name.as_str())
    }

    fn request(&self, cache_key: Option<crate::PromptCacheKey>, items: Vec<Item>) -> Request {
        Request {
            cache_key,
            model: self.model.clone(),
            context_window_tokens: self.context_window_tokens,
            max_input_tokens: self.max_input_tokens,
            options: self.options.clone(),
            instructions: self.instructions.clone(),
            tools: self.tools.iter().map(Tool::function).collect(),
            items,
        }
    }
}

impl Window {
    pub(super) fn take_request(&mut self) -> Request {
        let items = std::mem::take(&mut self.items);
        self.request(items)
    }

    pub(super) fn request(&self, items: Vec<Item>) -> Request {
        self.recipe.request(self.cache_key.clone(), items)
    }
}

/// The product's disposition of a tool invocation under its current authority.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolOutcome {
    /// The product confirmed the invocation, including a previously saved receipt.
    Returned(String),
    /// Structured history output, encoded only at the provider boundary.
    History(history::Page<Value>),
    /// An existing typed receipt, reopened without copying its referenced sources.
    Retained(results::ToolResult),
    /// The product knows the requested operation did not run or was rolled back.
    Refused(Refusal),
}

/// Corrective feedback for an operation that did not execute, or that failed
/// with an outcome nobody knows. The run record keeps this typed value. Only
/// the harness serializes the feedback envelope the model receives.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Refusal {
    reason: String,
    ran: Ran,
}

/// Whether the operation behind a refusal ran.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Ran {
    /// It did not run.
    No,
    /// It failed, and it may have changed something.
    Unknown,
}

impl Refusal {
    /// Refuse an operation that did not run.
    pub fn new(reason: impl std::fmt::Display) -> Self {
        Self {
            reason: reason.to_string(),
            ran: Ran::No,
        }
    }

    /// A call that failed and may have changed something. The model is told
    /// the outcome is unknown, never that the call did not run.
    pub(crate) fn unknown_outcome(reason: impl std::fmt::Display) -> Self {
        Self {
            ran: Ran::Unknown,
            ..Self::new(reason)
        }
    }

    /// The correction supplied by the owner of the refused operation.
    pub fn reason(&self) -> &str {
        &self.reason
    }

    /// The feedback envelope the model receives for this refusal.
    pub(crate) fn feedback(&self) -> Value {
        serde_json::to_value(self).expect("refusal contains only JSON values")
    }
}

impl serde::Serialize for Refusal {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut feedback = serializer.serialize_map(Some(2))?;
        match self.ran {
            Ran::No => feedback.serialize_entry("executed", &false)?,
            Ran::Unknown => feedback.serialize_entry("outcome", "unknown")?,
        }
        feedback.serialize_entry("error", &self.reason)?;
        feedback.end()
    }
}

impl<'de> serde::Deserialize<'de> for Refusal {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Envelope {
            executed: Option<bool>,
            outcome: Option<String>,
            error: String,
        }
        let envelope = Envelope::deserialize(deserializer)?;
        let ran = match (envelope.executed, envelope.outcome.as_deref()) {
            (Some(false), None) => Ran::No,
            (None, Some("unknown")) => Ran::Unknown,
            _ => {
                return Err(serde::de::Error::custom(
                    "a refusal never executed, or its outcome is unknown",
                ));
            }
        };
        Ok(Self {
            reason: envelope.error,
            ran,
        })
    }
}

impl ToolOutcome {
    /// Refuse an invocation without claiming that its operation ran.
    pub fn refused(reason: impl std::fmt::Display) -> Self {
        Self::Refused(Refusal::new(reason))
    }
    /// Exact textual product payload, when this outcome is text rather than a
    /// typed projection or history page.
    pub fn text(&self) -> Option<&str> {
        match self {
            Self::Returned(output) => Some(output),
            Self::History(_) | Self::Refused(_) => None,
            Self::Retained(result) => result.original(),
        }
    }
}

/// An owned operation, independent of the callback's borrow. The product may
/// resolve an in-memory result immediately; asynchronous I/O stays in this future.
/// Dropping it does not roll back an effect. Recovery reconciles the original ID.
pub type ToolFuture<T = ToolOutcome> =
    std::pin::Pin<Box<dyn Future<Output = Result<T, super::ProductError>> + Send>>;

/// Whether the product actually exposed this live fragment to a person.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextDelivery {
    /// Private work or a product that waits for the completed message.
    Hidden,
    /// The product delivered text through its live presentation channel.
    Visible,
}

/// Authority for one admitted job. All tool permissions and writes are fenced at the product.
#[async_trait]
pub trait Product: Send {
    /// Read whether work is still allowed and its inputs remain current before
    /// reading or acting. When the work no longer wants this attempt, answer
    /// [`super::ProductError::NotCurrent`] with the reason.
    async fn assert_current(&mut self) -> Result<(), super::ProductError>;
    /// Read authored-message sources after the harness checks currentness.
    /// This read does not acquire a write fence or check attempt ownership.
    /// Return None only when this product has no mutable message sources.
    async fn message_sources(
        &mut self,
    ) -> Result<Option<Vec<super::ProductMessage>>, super::ProductError>;
    /// Check the accepted policy and cache scope at the work owner before any
    /// dispatch. Neither part of the binding may change during recovery.
    async fn admit_binding(&mut self, binding: &RequestBinding) -> Result<(), super::ProductError>;
    /// Acknowledge the ordered fact in the product's durable record. Exact replay
    /// must be idempotent. Only transient storage failures may be wrapped in
    /// [`RetryablePersistence`]; authority, fence and semantic errors stay permanent.
    async fn persist(&mut self, event: Event) -> Result<(), super::ProductError>;
    /// Deliver one ephemeral fragment, outside the durable event record. The
    /// provider ordinal identifies its exchange. Return Visible only when shown.
    async fn live_text(
        &mut self,
        _ordinal: u64,
        _delta: TextDelta,
    ) -> Result<TextDelivery, super::ProductError> {
        Ok(TextDelivery::Hidden)
    }
    /// Whether this turn requests the product's explicit completion tool.
    fn terminal(&self, _turn: &Turn) -> bool {
        false
    }
    /// Product tools safe to read together. Unknown tools remain barriers.
    fn read_only(&self, _name: &str) -> bool {
        false
    }
    /// Prepare an owned invocation after authority and ToolStarted acknowledgment.
    /// Read-only invocations may be polled together; all others are barriers.
    /// Reconcile the original identity and fence any effect in the operation.
    fn execute(&mut self, call: &Invocation, arguments: Value) -> ToolFuture;
    /// Read a product receipt whose harness outcome was not acknowledged.
    /// This must never execute the call. Products with no separate effect store
    /// have no additional evidence and return None.
    fn saved_outcome(&mut self, _call: &Invocation) -> ToolFuture<Option<ToolOutcome>> {
        Box::pin(async { Ok(None) })
    }
    /// Read the confirmed receipt in this work under the current product authority.
    fn read_result(&mut self, call_id: &str) -> ToolFuture<Option<String>>;
    /// Publish under current authority or return corrective feedback to the same run.
    async fn finish(&mut self, turn: &Turn) -> Result<Finish, super::ProductError>;

    /// Retain the best answer from the reserved terminal request without
    /// representing the accepted work as complete. The product owns how the
    /// partial answer is exposed and how an explicit Try again rearms work.
    async fn finish_partial(
        &mut self,
        turn: &Turn,
        backstops: &[LimitBackstop],
    ) -> Result<(), super::ProductError>;

    /// Exact accepted assignment, original request, accepted steering and
    /// stable references needed to enter a fresh provider window. The harness
    /// appends the acknowledged note to this product-owned seed.
    async fn window_seed(&mut self) -> Result<crate::engine::WindowSeed, super::ProductError>;
}

impl Usage {
    pub(super) fn merge(&mut self, newer: &Self) {
        self.input_tokens = newer.input_tokens.or(self.input_tokens);
        self.cached_input_tokens = newer.cached_input_tokens.or(self.cached_input_tokens);
        self.output_tokens = newer.output_tokens.or(self.output_tokens);
    }
}

/// Reconstruct the continuation of a product-declined finish on recovery.
pub(super) fn continued(mut request: Request, turn: &Turn, feedback: &Value) -> Request {
    request.items.extend(turn.context_items().cloned());
    request.items.extend(turn.feedback(feedback));
    request
}

/// Start an explicitly rearmed allowance from the accepted frozen recipe while
/// retaining the latest exact context and the prior disposition.
pub(super) fn rearmed(
    frozen: &Request,
    mut latest: Request,
    partial: Option<ProductMessage>,
    feedback: &Value,
) -> Result<Request, Failure> {
    if let Some(message) = partial {
        latest.items.push(Item::ProductMessage(message));
    }
    latest.items.push(Item::Feedback {
        text: feedback.to_string(),
    });
    latest.items.retain(|item| *item != Item::OutputCutoff);
    Ok(Request {
        items: window_items(latest.items)?,
        ..frozen.clone()
    })
}
