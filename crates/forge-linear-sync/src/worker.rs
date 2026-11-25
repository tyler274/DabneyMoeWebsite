//! Queue worker: parse, apply, retry with backoff, dead-letter.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Notify;

use crate::config::{RetryPolicy, SyncConfig};
use crate::event::parse_pull_request;
use crate::linear::LinearClient;
use crate::queue::{EventStore, QueuedEvent, StoreError};
use crate::sync::{apply, mark_sync_error, SyncOutcome};

/// What happened to one processed event.
#[derive(Debug)]
pub enum Processed {
    Done(SyncOutcome),
    Retrying { delay: Duration, error: String },
    DeadLettered { error: String },
}

/// Exponential backoff with a little jitter, never shorter than a rate-limit
/// reset Linear asked for.
pub fn backoff(policy: &RetryPolicy, attempt: u32, retry_after: Option<Duration>) -> Duration {
    let exp = policy
        .base_delay
        .saturating_mul(2u32.saturating_pow(attempt.min(20)))
        .min(policy.max_delay);
    let jitter_span = u64::try_from(exp.as_millis() / 4).unwrap_or(0);
    let nanos = u64::from(jiff::Timestamp::now().subsec_nanosecond().unsigned_abs());
    let jitter = if jitter_span == 0 {
        0
    } else {
        nanos % jitter_span
    };
    let delay = exp + Duration::from_millis(jitter);
    retry_after.map_or(delay, |reset| delay.max(reset))
}

fn chain(error: &dyn std::error::Error) -> String {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(s) = source {
        text.push_str(": ");
        text.push_str(&s.to_string());
        source = s.source();
    }
    text
}

#[derive(Clone)]
pub struct Worker {
    pub store: Arc<EventStore>,
    pub client: LinearClient,
    pub config: Arc<SyncConfig>,
    pub notify: Arc<Notify>,
}

impl Worker {
    pub fn new(store: Arc<EventStore>, config: Arc<SyncConfig>, notify: Arc<Notify>) -> Self {
        Worker {
            store,
            client: LinearClient::new(config.linear.clone()),
            config,
            notify,
        }
    }

    async fn process(&self, event: QueuedEvent) -> Result<Processed, StoreError> {
        let snapshot = match parse_pull_request(&event.payload, &self.config) {
            Ok(snapshot) => snapshot,
            Err(e) => {
                let error = chain(&e);
                self.store.dead_letter(event.id, &error)?;
                return Ok(Processed::DeadLettered { error });
            }
        };
        match apply(&self.client, &snapshot).await {
            Ok(outcome) => {
                self.store.mark_done(event.id)?;
                Ok(Processed::Done(outcome))
            }
            Err(e) if e.is_retryable() && event.attempts + 1 < self.config.retry.max_attempts => {
                let delay = backoff(&self.config.retry, event.attempts, e.retry_after());
                let error = chain(&e);
                self.store.reschedule(event.id, delay, &error)?;
                Ok(Processed::Retrying { delay, error })
            }
            Err(e) => {
                let error = chain(&e);
                self.store.dead_letter(event.id, &error)?;
                // Labels whichever issues resolve; an unknown key is skipped.
                let keys: Vec<_> = snapshot.issues.iter().cloned().collect();
                if let Err(label_err) = mark_sync_error(&self.client, &keys).await {
                    tracing::warn!(error = %chain(&label_err), "could not label dead-lettered issues");
                }
                Ok(Processed::DeadLettered { error })
            }
        }
    }

    /// Process every event that is due now. Returns what happened to each.
    pub async fn drain_due(&self) -> Result<Vec<Processed>, StoreError> {
        let mut out = Vec::new();
        while let Some(event) = self.store.next_due()? {
            out.push(self.process(event).await?);
        }
        Ok(out)
    }

    /// Run forever, waking on new deliveries or when a retry becomes due.
    pub async fn run(self) {
        loop {
            if let Err(e) = self.drain_due().await {
                tracing::error!(error = %chain(&e), "event store failed");
            }
            let wait = match self.store.next_wake() {
                Ok(Some(d)) => d.min(Duration::from_secs(30)),
                Ok(None) => Duration::from_secs(30),
                Err(e) => {
                    tracing::error!(error = %chain(&e), "event store failed");
                    Duration::from_secs(5)
                }
            };
            tokio::select! {
                () = self.notify.notified() => {}
                () = tokio::time::sleep(wait) => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_grows_and_honors_reset() {
        let p = RetryPolicy {
            max_attempts: 8,
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(10),
        };
        assert!(backoff(&p, 0, None) >= Duration::from_millis(100));
        assert!(backoff(&p, 3, None) >= Duration::from_millis(800));
        assert!(backoff(&p, 30, None) <= Duration::from_millis(12_500));
        assert!(backoff(&p, 0, Some(Duration::from_secs(5))) >= Duration::from_secs(5));
    }
}
