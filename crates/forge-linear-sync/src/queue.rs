//! Durable SQLite event queue with delivery deduplication and a dead-letter
//! state. Only verified payloads are enqueued.

use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use bytes::Bytes;
use rusqlite::{params, Connection, OptionalExtension};

use crate::domain::{Forge, RepoPath};
use crate::verify::VerifiedPayload;

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
    Done,
    Dead,
}

impl EventStatus {
    fn as_str(self) -> &'static str {
        match self {
            EventStatus::Pending => "pending",
            EventStatus::Done => "done",
            EventStatus::Dead => "dead",
        }
    }

    fn parse(raw: &str) -> Option<Self> {
        [EventStatus::Pending, EventStatus::Done, EventStatus::Dead]
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

pub struct EventStore {
    conn: Mutex<Connection>,
}

fn now_ms() -> i64 {
    jiff::Timestamp::now().as_millisecond()
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
    status TEXT NOT NULL CHECK (status IN ('pending', 'done', 'dead')),
    attempts INTEGER NOT NULL DEFAULT 0,
    next_at INTEGER NOT NULL,
    last_error TEXT,
    created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS events_due ON events (status, next_at);
";

impl EventStore {
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        Self::from_connection(Connection::open(path)?)
    }

    pub fn in_memory() -> Result<Self, StoreError> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(conn: Connection) -> Result<Self, StoreError> {
        conn.execute_batch(SCHEMA)?;
        Ok(EventStore {
            conn: Mutex::new(conn),
        })
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
        let now = now_ms();
        let inserted = tx.execute(
            "INSERT OR IGNORE INTO deliveries (forge, delivery_id, received_at) VALUES (?1, ?2, ?3)",
            params![payload.forge().as_str(), payload.delivery_id(), now],
        )?;
        if inserted == 0 {
            return Ok(Enqueued::Duplicate);
        }
        tx.execute(
            "INSERT INTO events (forge, repo, delivery_id, body, status, next_at, created_at)
             VALUES (?1, ?2, ?3, ?4, 'pending', ?5, ?5)",
            params![
                payload.forge().as_str(),
                payload.repo().as_str(),
                payload.delivery_id(),
                payload.body().as_ref(),
                now
            ],
        )?;
        let id = tx.last_insert_rowid();
        tx.commit()?;
        Ok(Enqueued::New(id))
    }

    /// The oldest pending event that is due.
    pub fn next_due(&self) -> Result<Option<QueuedEvent>, StoreError> {
        let conn = self.conn();
        let row = conn
            .query_row(
                "SELECT id, forge, repo, delivery_id, body, attempts FROM events
                 WHERE status = 'pending' AND next_at <= ?1 ORDER BY id LIMIT 1",
                params![now_ms()],
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
        let Some((id, forge, repo, delivery_id, body, attempts)) = row else {
            return Ok(None);
        };
        let forge: Forge = forge
            .parse()
            .map_err(|_| StoreError::Corrupt { field: "forge" })?;
        let repo = RepoPath::parse(&repo).map_err(|_| StoreError::Corrupt { field: "repo" })?;
        Ok(Some(QueuedEvent {
            id,
            attempts,
            payload: VerifiedPayload::from_trusted_store(
                forge,
                repo,
                delivery_id,
                Bytes::from(body),
            ),
        }))
    }

    /// Milliseconds until the next pending event is due, if any.
    pub fn next_wake(&self) -> Result<Option<Duration>, StoreError> {
        let conn = self.conn();
        let next: Option<i64> = conn.query_row(
            "SELECT MIN(next_at) FROM events WHERE status = 'pending'",
            [],
            |r| r.get(0),
        )?;
        Ok(next.map(|at| Duration::from_millis(u64::try_from(at - now_ms()).unwrap_or(0))))
    }

    pub fn mark_done(&self, id: i64) -> Result<(), StoreError> {
        self.conn().execute(
            "UPDATE events SET status = 'done', attempts = attempts + 1, last_error = NULL WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    pub fn reschedule(&self, id: i64, delay: Duration, error: &str) -> Result<(), StoreError> {
        let next_at = now_ms() + i64::try_from(delay.as_millis()).unwrap_or(i64::MAX / 2);
        self.conn().execute(
            "UPDATE events SET attempts = attempts + 1, next_at = ?2, last_error = ?3 WHERE id = ?1",
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
            params![now_ms()],
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
