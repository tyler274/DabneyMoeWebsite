//! Ephemeral visitor session registry with per-session isolated `turso_core` SQLite stores.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use harness::engine::Cancellation;
use physician_demo_db::{DbError, DemoStore};
use physician_wire::briefings::PanelPreferencesWire;
use physician_wire::{PracticeAccountSummary, WhoamiResponse};

use crate::budget::{DEFAULT_MAX_SESSIONS, DEMO_PHYSICIAN_DISPLAY_NAME, DEMO_PRACTICE_ID};

/// One active harness run inside a visitor session.
#[derive(Clone, Debug)]
pub struct ActiveRun {
    /// Expected run identifier matched by `work.resume`.
    pub run_id: String,
    /// Cooperative token flipped by `work.stop`.
    pub cancel: Cancellation,
}

/// Isolated state owned by one public demo visitor session.
#[derive(Clone)]
pub struct VisitorSession {
    /// Bearer token presented in `Authorization: Bearer <token>`.
    pub token: String,
    /// Ephemeral physician identifier for this session.
    pub physician_id: String,
    /// Synthetic practice identifier.
    pub practice_id: String,
    /// Fixed expiry timestamp in Unix seconds.
    pub ends_at: u64,
    /// Isolated in-memory `turso_core` SQLite store cloned from seed bytes.
    pub store: DemoStore,
    /// Serializes concurrent transcript mutations for this visitor.
    pub store_lock: Arc<tokio::sync::Mutex<()>>,
    /// Active harness runs keyed by `(workspace_id, work_item_id)`.
    pub active_runs: Arc<Mutex<HashMap<(String, String), ActiveRun>>>,
    /// Cached workspace IDs present in this visitor's database.
    pub workspaces: Arc<Vec<String>>,
    /// Per-visitor panel sidebar/schedule collapse preferences.
    pub panel_preferences: Arc<Mutex<PanelPreferencesWire>>,
    /// Monotonic last-access order for bounded LRU eviction.
    last_used_seq: u64,
}

impl VisitorSession {
    /// Returns whether this session includes `workspace_id`.
    pub fn allows_workspace(&self, workspace_id: &str) -> bool {
        self.workspaces.iter().any(|w| w == workspace_id)
    }

    /// Returns a [`WhoamiResponse`] scoped to this visitor session.
    pub fn whoami(&self) -> WhoamiResponse {
        let email = format!("{}@demo.codahealth.test", self.physician_id);
        let acct = PracticeAccountSummary {
            physician_id: self.physician_id.clone(),
            display_name: DEMO_PHYSICIAN_DISPLAY_NAME.to_owned(),
            email: email.clone(),
            admin: false,
        };
        WhoamiResponse {
            physician_id: self.physician_id.clone(),
            practice_id: self.practice_id.clone(),
            email,
            display_name: DEMO_PHYSICIAN_DISPLAY_NAME.to_owned(),
            admin: false,
            practice_accounts: vec![acct],
        }
    }

    /// Registers an active harness run for `(workspace_id, work_item_id)`.
    pub fn register_run(&self, workspace_id: &str, work_item_id: &str, run: ActiveRun) {
        self.active_runs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert((workspace_id.to_owned(), work_item_id.to_owned()), run);
    }

    /// Looks up and cancels any active run matching `(workspace_id, work_item_id)`.
    pub fn cancel_run(&self, workspace_id: &str, work_item_id: &str) -> Option<String> {
        let active = self
            .active_runs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&(workspace_id.to_owned(), work_item_id.to_owned()))?;
        active.cancel.cancel();
        Some(active.run_id)
    }

    /// Returns whether `run_id` is still the active run for `(workspace_id, work_item_id)`.
    pub fn is_current_run(&self, workspace_id: &str, work_item_id: &str, run_id: &str) -> bool {
        let guard = self
            .active_runs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard
            .get(&(workspace_id.to_owned(), work_item_id.to_owned()))
            .is_some_and(|r| r.run_id == run_id)
    }

    /// Removes the active run marker when `run_id` finishes.
    pub fn clear_run_if_matches(&self, workspace_id: &str, work_item_id: &str, run_id: &str) {
        let mut guard = self
            .active_runs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let key = (workspace_id.to_owned(), work_item_id.to_owned());
        if guard.get(&key).is_some_and(|r| r.run_id == run_id) {
            guard.remove(&key);
        }
    }
}

/// Bounded-capacity registry of per-visitor isolated [`DemoStore`] instances.
#[derive(Clone)]
pub struct SessionRegistry {
    seed_bytes: Arc<Vec<u8>>,
    session_seconds: u64,
    max_sessions: usize,
    inner: Arc<Mutex<RegistryInner>>,
}

struct RegistryInner {
    sessions: HashMap<String, VisitorSession>,
    seq: u64,
}

impl SessionRegistry {
    /// Creates a registry minting isolated sessions from `seed_bytes`.
    pub fn new(seed_bytes: Vec<u8>, session_seconds: u64, max_sessions: Option<usize>) -> Self {
        Self {
            seed_bytes: Arc::new(seed_bytes),
            session_seconds: session_seconds.max(1),
            max_sessions: max_sessions.unwrap_or(DEFAULT_MAX_SESSIONS).max(1),
            inner: Arc::new(Mutex::new(RegistryInner {
                sessions: HashMap::new(),
                seq: 0,
            })),
        }
    }

    /// Mints a new isolated visitor session backed by its own `turso_core` DB instance.
    pub fn mint(&self) -> Result<VisitorSession, DbError> {
        let now = unix_now();
        let store = DemoStore::from_bytes(self.seed_bytes.as_ref().clone())?;
        let workspaces = Arc::new(store.list_workspace_ids()?);
        let token = format!("demo-tok-{}", uuid::Uuid::new_v4().simple());
        let physician_id = format!("demo-{}", uuid::Uuid::new_v4().simple());
        let ends_at = now.saturating_add(self.session_seconds);

        let mut guard = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.sessions.retain(|_, s| s.ends_at > now);
        while guard.sessions.len() >= self.max_sessions {
            let Some(oldest) = guard
                .sessions
                .iter()
                .min_by_key(|(_, s)| s.last_used_seq)
                .map(|(k, _)| k.clone())
            else {
                break;
            };
            guard.sessions.remove(&oldest);
        }
        guard.seq = guard.seq.saturating_add(1);
        let session = VisitorSession {
            token: token.clone(),
            physician_id,
            practice_id: DEMO_PRACTICE_ID.to_owned(),
            ends_at,
            store,
            store_lock: Arc::new(tokio::sync::Mutex::new(())),
            active_runs: Arc::new(Mutex::new(HashMap::new())),
            workspaces,
            panel_preferences: Arc::new(Mutex::new(PanelPreferencesWire::default())),
            last_used_seq: guard.seq,
        };
        guard.sessions.insert(token, session.clone());
        Ok(session)
    }

    /// Removes a bearer session on sign-out.
    pub fn revoke(&self, token: &str) {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .sessions
            .remove(token);
    }

    /// Resolves a valid, unexpired bearer token and updates LRU recency.
    pub fn authenticate(&self, token: &str) -> Option<VisitorSession> {
        let now = unix_now();
        let mut guard = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.seq = guard.seq.saturating_add(1);
        let seq = guard.seq;
        let session = guard.sessions.get_mut(token)?;
        if session.ends_at <= now {
            guard.sessions.remove(token);
            return None;
        }
        session.last_used_seq = seq;
        Some(session.clone())
    }
}

pub(crate) fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}
