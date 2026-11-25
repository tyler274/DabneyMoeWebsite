//! Durable SQLite event queue with delivery deduplication, exclusive
//! claim leases, and a dead-letter state. Only verified payloads are enqueued.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};

use crate::domain::{Forge, RepoPath};
use crate::verify::VerifiedPayload;

/// Default lease duration while a worker is processing a claimed event.
pub const DEFAULT_LEASE: Duration = Duration::from_secs(60);

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("event store database error")]
    Sqlite(#[from] rusqlite::Error),
    #[error("stored event has an invalid {field}")]
    Corrupt { field: &'static str },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventStatus {
    Pending,
    Processing,
    Done,
    Dead,
}

impl EventStatus {
    fn as_str(self) -> &'static str {
        match self {
            EventStatus::Pending => "pending",
            EventStatus::Processing => "processing",
            EventStatus::Done => "done",
            EventStatus::Dead => "dead",
        }
    }

    fn parse(raw: &str) -> Option<Self> {
        [
            EventStatus::Pending,
            EventStatus::Processing,
            EventStatus::Done,
            EventStatus::Dead,
        ]
        .into_iter()
        .find(|s| s.as_str() == raw)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Enqueued {
    New(i64),
    Duplicate,
}

#[derive(Debug, Clone)]
pub struct QueuedEvent {
    pub id: i64,
    pub attempts: u32,
    pub payload: VerifiedPayload<Bytes>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventRecord {
    pub id: i64,
    pub forge: String,
    pub delivery_id: String,
    pub status: EventStatus,
    pub attempts: u32,
    pub last_error: Option<String>,
}

/// Clock source returning UTC milliseconds since Unix epoch.
pub type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;

pub struct EventStore {
    conn: Mutex<Connection>,
    clock: Clock,
}

fn system_clock() -> Clock {
    Arc::new(|| jiff::Timestamp::now().as_millisecond())
}

fn encode_repos(repos: &[RepoPath]) -> String {
    let raw: Vec<&str> = repos.iter().map(RepoPath::as_str).collect();
    serde_json::to_string(&raw).expect("repo path strings serialize")
}

fn decode_repos(raw: &str) -> Result<Vec<RepoPath>, StoreError> {
    let list: Vec<String> = if raw.starts_with('[') {
        serde_json::from_str(raw).map_err(|_| StoreError::Corrupt { field: "repo" })?
    } else {
        vec![raw.to_owned()]
    };
    if list.is_empty() {
        return Err(StoreError::Corrupt { field: "repo" });
    }
    list.iter()
        .map(|r| RepoPath::parse(r).map_err(|_| StoreError::Corrupt { field: "repo" }))
        .collect()
}

const SCHEMA: &str = "
PRAGMA journal_mode = WAL;
PRAGMA busy_timeout = 5000;
CREATE TABLE IF NOT EXISTS deliveries (
    forge TEXT NOT NULL,
    delivery_id TEXT NOT NULL,
    received_at INTEGER NOT NULL,
    PRIMARY KEY (forge, delivery_id)
);
CREATE TABLE IF NOT EXISTS events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    forge TEXT NOT NULL,
    repo TEXT NOT NULL,
    delivery_id TEXT NOT NULL,
    body BLOB NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('pending', 'processing', 'done', 'dead')),
    attempts INTEGER NOT NULL DEFAULT 0,
    next_at INTEGER NOT NULL,
    last_error TEXT,
    created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS events_due ON events (status, next_at);
";

impl EventStore {
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        Self::from_connection(Connection::open(path)?, system_clock())
    }

    pub fn in_memory() -> Result<Self, StoreError> {
        Self::from_connection(Connection::open_in_memory()?, system_clock())
    }

    /// In-memory store driven by a caller-supplied millisecond clock, for
    /// deterministic retry and lease tests without real-time sleeps.
    pub fn in_memory_with_clock(clock: Clock) -> Result<Self, StoreError> {
        Self::from_connection(Connection::open_in_memory()?, clock)
    }

    fn from_connection(conn: Connection, clock: Clock) -> Result<Self, StoreError> {
        conn.execute_batch(SCHEMA)?;
        Ok(EventStore {
            conn: Mutex::new(conn),
            clock,
        })
    }

    fn now_ms(&self) -> i64 {
        (self.clock)()
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        // A panic while holding the lock leaves SQLite itself consistent.
        self.conn
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    /// Record the delivery and queue its body, atomically. A delivery seen
    /// before is not queued again.
    pub fn enqueue(&self, payload: &VerifiedPayload<Bytes>) -> Result<Enqueued, StoreError> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let now = self.now_ms();
        let inserted = tx.execute(
            "INSERT OR IGNORE INTO deliveries (forge, delivery_id, received_at) VALUES (?1, ?2, ?3)",
            params![payload.forge().as_str(), payload.delivery_id(), now],
        )?;
        if inserted == 0 {
            return Ok(Enqueued::Duplicate);
        }
        let repos = encode_repos(payload.verified_repos());
        tx.execute(
            "INSERT INTO events (forge, repo, delivery_id, body, status, next_at, created_at)
             VALUES (?1, ?2, ?3, ?4, 'pending', ?5, ?5)",
            params![
                payload.forge().as_str(),
                repos,
                payload.delivery_id(),
                payload.body().as_ref(),
                now
            ],
        )?;
        let id = tx.last_insert_rowid();
        tx.commit()?;
        Ok(Enqueued::New(id))
    }

    /// Claim the oldest due event under a lease so concurrent workers (such
    /// as `serve` and `replay-dlq`) cannot process the same event twice.
    /// Corrupt rows are dead-lettered and skipped instead of blocking the
    /// queue.
    pub fn claim_due(&self, lease: Duration) -> Result<Option<QueuedEvent>, StoreError> {
        let lease_ms = i64::try_from(lease.as_millis()).unwrap_or(i64::MAX / 2);
        loop {
            let mut conn = self.conn();
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let now = self.now_ms();
            let row = tx
                .query_row(
                    "SELECT id, forge, repo, delivery_id, body, attempts FROM events
                     WHERE status IN ('pending', 'processing') AND next_at <= ?1
                     ORDER BY id LIMIT 1",
                    params![now],
                    |r| {
                        Ok((
                            r.get::<_, i64>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, String>(2)?,
                            r.get::<_, String>(3)?,
                            r.get::<_, Vec<u8>>(4)?,
                            r.get::<_, u32>(5)?,
                        ))
                    },
                )
                .optional()?;
            let Some((id, forge_raw, repo_raw, delivery_id, body, attempts)) = row else {
                return Ok(None);
            };
            let parsed = forge_raw
                .parse::<Forge>()
                .map_err(|_| StoreError::Corrupt { field: "forge" })
                .and_then(|forge| Ok((forge, decode_repos(&repo_raw)?)));
            match parsed {
                Ok((forge, verified_repos)) => {
                    let lease_until = now.saturating_add(lease_ms);
                    tx.execute(
                        "UPDATE events SET status = 'processing', next_at = ?2 WHERE id = ?1",
                        params![id, lease_until],
                    )?;
                    tx.commit()?;
                    return Ok(Some(QueuedEvent {
                        id,
                        attempts,
                        payload: VerifiedPayload::from_trusted_store(
                            forge,
                            verified_repos,
                            delivery_id,
                            Bytes::from(body),
                        ),
                    }));
                }
                Err(err) => {
                    let msg = err.to_string();
                    tracing::error!(event_id = id, error = %msg, "dead-lettering corrupt queued event");
                    tx.execute(
                        "UPDATE events SET status = 'dead', attempts = attempts + 1, last_error = ?2 WHERE id = ?1",
                        params![id, msg],
                    )?;
                    tx.commit()?;
                }
            }
        }
    }

    /// Claim the oldest due event using [`DEFAULT_LEASE`].
    pub fn next_due(&self) -> Result<Option<QueuedEvent>, StoreError> {
        self.claim_due(DEFAULT_LEASE)
    }

    /// Milliseconds until the next pending or leased event is due, if any.
    pub fn next_wake(&self) -> Result<Option<Duration>, StoreError> {
        let conn = self.conn();
        let next: Option<i64> = conn.query_row(
            "SELECT MIN(next_at) FROM events WHERE status IN ('pending', 'processing')",
            [],
            |r| r.get(0),
        )?;
        Ok(next.map(|at| Duration::from_millis(u64::try_from(at - self.now_ms()).unwrap_or(0))))
    }

    pub fn mark_done(&self, id: i64) -> Result<(), StoreError> {
        self.conn().execute(
            "UPDATE events SET status = 'done', attempts = attempts + 1, last_error = NULL WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    pub fn reschedule(&self, id: i64, delay: Duration, error: &str) -> Result<(), StoreError> {
        let next_at = self
            .now_ms()
            .saturating_add(i64::try_from(delay.as_millis()).unwrap_or(i64::MAX / 2));
        self.conn().execute(
            "UPDATE events SET status = 'pending', attempts = attempts + 1, next_at = ?2, last_error = ?3 WHERE id = ?1",
            params![id, next_at, error],
        )?;
        Ok(())
    }

    pub fn dead_letter(&self, id: i64, error: &str) -> Result<(), StoreError> {
        self.conn().execute(
            "UPDATE events SET status = 'dead', attempts = attempts + 1, last_error = ?2 WHERE id = ?1",
            params![id, error],
        )?;
        Ok(())
    }

    /// Move every dead-lettered event back to pending with fresh attempts.
    pub fn replay_dead(&self) -> Result<usize, StoreError> {
        Ok(self.conn().execute(
            "UPDATE events SET status = 'pending', attempts = 0, next_at = ?1 WHERE status = 'dead'",
            params![self.now_ms()],
        )?)
    }

    pub fn events(&self, status: Option<EventStatus>) -> Result<Vec<EventRecord>, StoreError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, forge, delivery_id, status, attempts, last_error FROM events
             WHERE ?1 IS NULL OR status = ?1 ORDER BY id",
        )?;
        let rows = stmt.query_map(params![status.map(EventStatus::as_str)], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, u32>(4)?,
                r.get::<_, Option<String>>(5)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, forge, delivery_id, status, attempts, last_error) = row?;
            out.push(EventRecord {
                id,
                forge,
                delivery_id,
                status: EventStatus::parse(&status)
                    .ok_or(StoreError::Corrupt { field: "status" })?,
                attempts,
                last_error,
            });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claim_leases_prevent_double_claim_and_corrupt_rows_do_not_block() {
        use std::sync::atomic::{AtomicI64, Ordering};

        let now = Arc::new(AtomicI64::new(10_000));
        let clock_ref = now.clone();
        let store =
            EventStore::in_memory_with_clock(Arc::new(move || clock_ref.load(Ordering::SeqCst)))
                .unwrap();
        // Insert a corrupt row ahead of a valid row directly via SQL.
        {
            let conn = store.conn();
            conn.execute(
                "INSERT INTO events (forge, repo, delivery_id, body, status, next_at, created_at)
                 VALUES ('unknown-forge', 'tyler/site', 'bad-1', X'7b7d', 'pending', 0, 0)",
                [],
            )
            .unwrap();
        }
        let valid = VerifiedPayload::from_trusted_store(
            Forge::Dabney,
            vec![
                RepoPath::parse("tyler/site").unwrap(),
                RepoPath::parse("tyler/other").unwrap(),
            ],
            "good-1".into(),
            Bytes::from_static(b"{}"),
        );
        assert!(matches!(store.enqueue(&valid).unwrap(), Enqueued::New(_)));

        // Claiming skips and dead-letters the corrupt row, returning the valid one.
        let claimed = store.claim_due(Duration::from_secs(60)).unwrap().unwrap();
        assert_eq!(claimed.payload.delivery_id(), "good-1");
        assert_eq!(claimed.payload.verified_repos().len(), 2);

        // A second claim while leased sees nothing due.
        assert!(store.claim_due(Duration::from_secs(60)).unwrap().is_none());

        // Once the lease expires without completion, another worker can reclaim it.
        now.fetch_add(60_000, Ordering::SeqCst);
        let reclaimed = store.claim_due(Duration::from_secs(60)).unwrap().unwrap();
        assert_eq!(reclaimed.id, claimed.id);

        let dead = store.events(Some(EventStatus::Dead)).unwrap();
        assert_eq!(dead.len(), 1);
        assert_eq!(dead[0].delivery_id, "bad-1");
    }
}
