use async_trait::async_trait;
use harness::{engine::Cause, provider::QuotaGate};
use std::{sync::Mutex, time::Duration};
use tokio::time::Instant;

#[derive(Default)]
pub struct Gate(Mutex<Option<Instant>>);
#[async_trait]
impl QuotaGate for Gate {
    async fn remaining(&self) -> Result<Duration, Cause> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .map(|at| at.saturating_duration_since(Instant::now()))
            .unwrap_or_default())
    }
    async fn defer(&self, delay: Duration) -> Result<(), Cause> {
        let until = Instant::now() + delay;
        let mut saved = self.0.lock().unwrap();
        *saved = Some(saved.map_or(until, |previous| previous.max(until)));
        Ok(())
    }
}
