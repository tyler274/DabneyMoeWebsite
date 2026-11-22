//! Run lifecycle at the product's accepted-work owner. Attempt evidence has its
//! own sequence; late provider evidence cannot overwrite an accepted run outcome.
use super::{AttemptFailure, MAX_FAILED_ATTEMPTS};
use serde::{Deserialize, Serialize};

/// Why accepted work is held until the person explicitly continues it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HoldReason {
    /// The person pressed Stop.
    Stopped,
    /// A run backstop or the fresh-window refusal allowance was exhausted.
    Limit,
    /// The provider requires operator action.
    ProviderBlocked,
}

/// The one terminal outcome of an execution of accepted work.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum RunOutcome {
    /// The product accepted and published its explicit finish.
    Completed,
    /// Work is retained for an explicit continuation.
    Held {
        /// The reason governs the person's continuation action.
        reason: HoldReason,
    },
    /// Automatic recovery was exhausted.
    Failed,
    /// The work was withdrawn or replaced.
    Cancelled,
}
impl RunOutcome {
    /// A failed attempt ends its run only when recovery cannot continue.
    pub fn after_attempt(reason: AttemptFailure, failures: u32) -> Option<Self> {
        match reason {
            AttemptFailure::Limit => Some(Self::Held {
                reason: HoldReason::Limit,
            }),
            AttemptFailure::ProviderBlocked => Some(Self::Held {
                reason: HoldReason::ProviderBlocked,
            }),
            AttemptFailure::Failed | AttemptFailure::Abandoned
                if failures >= MAX_FAILED_ATTEMPTS =>
            {
                Some(Self::Failed)
            }
            AttemptFailure::Interrupted | AttemptFailure::Failed | AttemptFailure::Abandoned => {
                None
            }
        }
    }

    /// Terminal control projection; products use this vocabulary for both storage and UI.
    pub const fn status(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Held { .. } => "held",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

/// Immutable lifecycle facts saved atomically with the product transition.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RunEvent {
    /// A new execution of this accepted work, linked to its preceding execution.
    RunStarted {
        /// Stable execution identity within its work.
        run_id: String,
        /// Prior execution explicitly continued by this one.
        continues: Option<String>,
    },
    /// One accepted terminal disposition; late attempt evidence does not change it.
    RunEnded {
        /// The started execution this event ends.
        run_id: String,
        /// Product-accepted final disposition.
        outcome: RunOutcome,
    },
}

/// Derived view of the latest execution; never stored beside the events.
#[derive(Clone, Copy, Debug)]
pub struct RunState<'a> {
    /// Stable run identity, distinct from its attempts.
    pub id: &'a str,
    /// The preceding run of the same work, when explicitly continued.
    pub continues: Option<&'a str>,
    /// None while this run remains open across attempts.
    pub outcome: Option<RunOutcome>,
}

/// The product's lifecycle facts for one piece of work, not an attempt journal.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RunHistory(Vec<RunEvent>);

/// An invalid lifecycle transition is a protocol defect, not a held run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RunError {
    /// No execution identity was supplied.
    #[error("run identity is empty")]
    EmptyIdentity,
    /// An identity or predecessor conflicts with retained history.
    #[error("run identity was already used")]
    ReusedIdentity,
    /// The current execution has not ended.
    #[error("the preceding run has not ended")]
    StillRunning,
    /// No matching run exists for an ending.
    #[error("no run has started")]
    MissingRun,
    /// An accepted ending cannot be overwritten.
    #[error("the run already has a different outcome")]
    ConflictingOutcome,
}

impl RunHistory {
    /// Rebuild the lifecycle from the product's ordered neutral run events.
    pub fn from_events(events: impl IntoIterator<Item = RunEvent>) -> Result<Self, RunError> {
        let mut history = Self::default();
        for event in events {
            match event {
                RunEvent::RunStarted { run_id, continues } => {
                    if history.current().map(|run| run.id) != continues.as_deref() {
                        return Err(RunError::ReusedIdentity);
                    }
                    history.start(run_id)?;
                }
                RunEvent::RunEnded { run_id, outcome } => {
                    if history.current().is_none_or(|run| run.id != run_id) {
                        return Err(RunError::MissingRun);
                    }
                    history.end(outcome)?;
                }
            }
        }
        Ok(history)
    }

    /// The exact lifecycle facts the product persists and exposes for inspection.
    pub fn events(&self) -> &[RunEvent] {
        &self.0
    }

    /// Derive the latest run from its start and optional ending.
    pub fn current(&self) -> Option<RunState<'_>> {
        let mut current = None;
        for event in &self.0 {
            match event {
                RunEvent::RunStarted { run_id, continues } => {
                    current = Some(RunState {
                        id: run_id,
                        continues: continues.as_deref(),
                        outcome: None,
                    })
                }
                RunEvent::RunEnded { run_id, outcome } => {
                    if let Some(run) = &mut current
                        && run.id == run_id
                    {
                        run.outcome = Some(*outcome);
                    }
                }
            }
        }
        current
    }

    /// Start once when the product accepts initial work or an explicit continuation.
    /// Automatic recovery and steering retain the existing open run. The returned
    /// failure count starts this new run; products persist it with the lifecycle fact.
    pub fn start(&mut self, id: String) -> Result<u32, RunError> {
        if id.trim().is_empty() {
            return Err(RunError::EmptyIdentity);
        }
        if self
            .0
            .iter()
            .any(|event| matches!(event, RunEvent::RunStarted { run_id, .. } if run_id == &id))
        {
            return Err(RunError::ReusedIdentity);
        }
        let previous = self.current();
        if previous.is_some_and(|run| run.outcome.is_none()) {
            return Err(RunError::StillRunning);
        }
        let continues = previous.map(|run| run.id.to_owned());
        self.0.push(RunEvent::RunStarted {
            run_id: id,
            continues,
        });
        Ok(0)
    }

    /// End once in the transaction that accepts completion, a hold or cancellation.
    /// Repeating the same accepted outcome is idempotent.
    pub fn end(&mut self, outcome: RunOutcome) -> Result<(), RunError> {
        let current = self.current().ok_or(RunError::MissingRun)?;
        if let Some(saved) = current.outcome {
            return if saved == outcome {
                Ok(())
            } else {
                Err(RunError::ConflictingOutcome)
            };
        }
        let run_id = current.id.to_owned();
        self.0.push(RunEvent::RunEnded { run_id, outcome });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_an_explicit_continuation_starts_a_linked_run() {
        let mut history = RunHistory::default();
        history.start("first".into()).unwrap();
        assert_eq!(history.start("second".into()), Err(RunError::StillRunning));
        assert_eq!(RunOutcome::after_attempt(AttemptFailure::Failed, 1), None);
        let held = RunOutcome::Held {
            reason: HoldReason::Stopped,
        };
        history.end(held).unwrap();
        history.end(held).unwrap();
        assert_eq!(history.events().len(), 2);
        assert_eq!(
            history.end(RunOutcome::Completed),
            Err(RunError::ConflictingOutcome)
        );
        history.start("second".into()).unwrap();
        assert_eq!(history.current().unwrap().continues, Some("first"));
        assert_eq!(history.current().unwrap().outcome, None);
        history.end(RunOutcome::Completed).unwrap();
        let restored: RunHistory =
            serde_json::from_str(&serde_json::to_string(&history).unwrap()).unwrap();
        assert_eq!(restored, history);
        assert_eq!(
            restored
                .events()
                .iter()
                .filter(|event| matches!(event, RunEvent::RunEnded { .. }))
                .count(),
            2
        );
    }
}
