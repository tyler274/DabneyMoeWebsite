//! Reconstruct accepted work from product-owned facts, before entering the loop.
use super::{
    AcceptedInput, Cancellation, Event, EventKind, Failure, Product, ProductMessage, RunLimits,
    Turn, Work, WorkProgress,
    recipe::{Provider, ReplayEvent},
};
use serde_json::Value;
use std::collections::HashSet;

/// Ordered execution evidence for one accepted work, with its explicit retry boundary.
pub struct Records<'a> {
    /// Products decide admissibility; the harness interprets the event content.
    pub events: Vec<ReplayEvent<'a>>,
    /// Last dispatch before the current explicitly started run.
    pub allowance_after: u64,
}

/// Current authored sources and the identity of the product's retained partial.
pub struct Messages<'a> {
    /// Deleted product messages are absent from this collection.
    pub sources: &'a [ProductMessage],
    /// Identity of this work's latest retained partial.
    pub partial: Option<String>,
}

/// Accepted direction and current product evidence supplied without event interpretation.
#[derive(Default)]
pub struct Inputs {
    /// All accepted inputs belonging to this work, in acceptance order.
    pub accepted: Vec<AcceptedInput>,
    /// Product-owned source changes requiring a candidate to be reconsidered.
    pub source_feedback: Option<Value>,
}

/// Complete continuation assembled by the harness, including its mechanical progress.
pub struct Recovered {
    attempt: String,
    work: Work,
    progress: WorkProgress,
}

impl Work {
    /// Reconstruct one continuation from records, current messages and accepted inputs.
    pub fn recover(
        self,
        attempt: &str,
        records: Records<'_>,
        messages: Messages<'_>,
        inputs: Inputs,
    ) -> Result<Recovered, Failure> {
        let pending = pending_input_ids(
            records.events.iter().copied(),
            inputs.accepted.iter().map(|input| input.id.clone()),
        );
        let progress = WorkProgress::from_events_after(
            records.events.iter().map(|record| record.event()),
            attempt,
            records.allowance_after,
        );
        let mut work =
            self.with_history(records.events, records.allowance_after, messages.sources)?;
        work.partial_message = messages.partial;
        work.pending_input = inputs
            .accepted
            .into_iter()
            .filter(|input| pending.contains(&input.id))
            .collect();
        work.source_feedback = inputs.source_feedback;
        Ok(Recovered {
            attempt: attempt.into(),
            work,
            progress,
        })
    }
}

impl Recovered {
    /// Execute this continuation without product reconstruction of counters or context.
    pub async fn run<P: Provider + ?Sized>(
        mut self,
        provider: &mut P,
        product: &mut dyn Product,
        limits: RunLimits,
        cancel: &Cancellation,
    ) -> Result<Turn, Failure> {
        super::run_work(
            provider,
            product,
            &self.attempt,
            self.work,
            &mut self.progress,
            limits,
            cancel,
        )
        .await
    }
}

/// Which accepted input identities have not reached an acknowledged dispatch.
/// Publication and pending-final checks consume the same rule as recovery.
pub fn pending_input_ids<'a>(
    records: impl IntoIterator<Item = impl Into<ReplayEvent<'a>>>,
    inputs: impl IntoIterator<Item = String>,
) -> Vec<String> {
    let carried: HashSet<_> = records
        .into_iter()
        .filter_map(|record| record.into().admitted())
        .filter_map(|event| match &event.kind {
            EventKind::ProviderStarted { context, .. } => {
                Some(context.carried_inputs.iter().map(String::as_str))
            }
            EventKind::ProviderObservation { .. }
            | EventKind::CallBegan { .. }
            | EventKind::ProviderFailed { .. }
            | EventKind::TransportRetry { .. }
            | EventKind::ProviderReturned(_)
            | EventKind::ToolStarted(_)
            | EventKind::ToolReturned { .. }
            | EventKind::ProviderBlocked { .. }
            | EventKind::ContextRejected
            | EventKind::Candidate { .. }
            | EventKind::WindowOpened { .. } => None,
        })
        .flatten()
        .collect();
    inputs
        .into_iter()
        .filter(|id| !carried.contains(id.as_str()))
        .collect()
}

/// The last provider identity, retained when an explicit retry starts a fresh allowance.
pub fn resume_boundary<'a>(events: impl IntoIterator<Item = &'a Event>) -> u64 {
    events
        .into_iter()
        .filter(|event| matches!(event.kind, EventKind::ProviderStarted { .. }))
        .map(|event| event.ordinal)
        .max()
        .unwrap_or(0)
}
