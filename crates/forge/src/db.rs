//! SQLite (WAL) storage for repositories, tokens, pull requests, reviews,
//! inline comments, check statuses, mirrors and webhook deliveries.

use std::path::Path;
use std::sync::Mutex;

use gix::ObjectId;
use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ToSqlOutput, ValueRef};
use rusqlite::{params, Connection, OptionalExtension, Row, ToSql};
use serde::Serialize;

use crate::error::ForgeError;
use crate::repo::RepoName;

pub fn now() -> i64 {
    jiff::Timestamp::now().as_second()
}

/// Enums stored as their lowercase label.
macro_rules! text_enum {
    ($name:ident { $($variant:ident => $label:literal),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name { $($variant),+ }

        impl $name {
            pub const ALL: &'static [$name] = &[$($name::$variant),+];

            pub fn as_str(self) -> &'static str {
                match self { $($name::$variant => $label),+ }
            }

            pub fn parse(raw: &str) -> Option<Self> {
                Self::ALL.iter().copied().find(|v| v.as_str() == raw)
            }
        }

        impl ToSql for $name {
            fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
                Ok(ToSqlOutput::from(self.as_str()))
            }
        }

        impl FromSql for $name {
            fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
                let text = value.as_str()?;
                $name::parse(text).ok_or_else(|| FromSqlError::Other(format!("invalid {} {text:?}", stringify!($name)).into()))
            }
        }
    };
}

text_enum!(Verdict { Approve => "approve", RequestChanges => "request_changes", Comment => "comment" });
text_enum!(CheckState { Pending => "pending", Success => "success", Failure => "failure", Error => "error" });
text_enum!(JobStatus { Queued => "queued", Running => "running", Succeeded => "succeeded", Failed => "failed" });
text_enum!(DeliveryStatus { Pending => "pending", Delivered => "delivered", Failed => "failed" });
text_enum!(Side { Old => "old", New => "new" });

/// Object ids are stored as hex text.
struct Oid(ObjectId);

impl FromSql for Oid {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let text = value.as_str()?;
        ObjectId::from_hex(text.as_bytes())
            .map(Oid)
            .map_err(|e| FromSqlError::Other(Box::new(e)))
    }
}

fn oid(row: &Row<'_>, idx: usize) -> rusqlite::Result<ObjectId> {
    Ok(row.get::<_, Oid>(idx)?.0)
}

#[derive(Debug, Clone, Serialize)]
pub struct RepoRecord {
    pub id: i64,
    pub name: RepoName,
    pub private: bool,
    pub default_branch: String,
    pub required_approvals: u32,
    pub required_checks: Vec<String>,
}

/// Where a pull request stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrState {
    Open,
    Merged { commit: ObjectId },
    Closed,
}

impl PrState {
    pub fn label(&self) -> &'static str {
        match self {
            PrState::Open => "open",
            PrState::Merged { .. } => "merged",
            PrState::Closed => "closed",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PullRequest {
    pub id: i64,
    pub repo_id: i64,
    pub number: u64,
    pub title: String,
    pub body: String,
    pub author: String,
    pub source_branch: String,
    pub target_branch: String,
    /// Target tip when the PR was opened.
    pub base_oid: ObjectId,
    pub head_oid: ObjectId,
    pub state: PrState,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct Review {
    pub id: i64,
    pub author: String,
    pub verdict: Verdict,
    pub head_oid: ObjectId,
    pub body: String,
    pub created_at: i64,
}

/// Where an inline comment points: one line of one side of the diff of
/// `path`, as the pull request stood at `head_oid`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Anchor {
    pub head_oid: ObjectId,
    pub path: String,
    pub side: Side,
    /// 1-based line number on `side`.
    pub line: u32,
}

#[derive(Debug, Clone)]
pub struct InlineComment {
    pub id: i64,
    pub author: String,
    pub anchor: Anchor,
    pub body: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct CheckStatus {
    pub context: String,
    pub state: CheckState,
    pub target_url: Option<String>,
    pub description: Option<String>,
    pub creator: String,
    pub created_at: i64,
}

#[derive(Debug, Clone)]
pub struct Mirror {
    pub id: i64,
    pub name: String,
    pub url: String,
    pub username: Option<String>,
    /// Environment variable holding the push token; never the token itself.
    pub token_env: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MirrorJob {
    pub id: i64,
    pub repo_id: i64,
    pub mirror: Mirror,
    pub trigger: String,
    pub status: JobStatus,
    pub attempts: u32,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct WebhookDelivery {
    pub id: i64,
    pub pr_id: i64,
    pub delivery_id: String,
    pub action: String,
    pub target: String,
    pub status: DeliveryStatus,
    pub http_status: Option<u16>,
    pub attempts: u32,
    pub last_error: Option<String>,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct TokenRecord {
    pub id: String,
    pub user: String,
    pub scopes: String,
    pub hash: String,
}

const SCHEMA: &str = "
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;
CREATE TABLE IF NOT EXISTS repos (
    id INTEGER PRIMARY KEY,
    owner TEXT NOT NULL,
    name TEXT NOT NULL,
    private INTEGER NOT NULL DEFAULT 0,
    default_branch TEXT NOT NULL,
    required_approvals INTEGER NOT NULL DEFAULT 1,
    required_checks TEXT NOT NULL DEFAULT '[]',
    created_at INTEGER NOT NULL,
    UNIQUE (owner, name)
);
CREATE TABLE IF NOT EXISTS collaborators (
    repo_id INTEGER NOT NULL REFERENCES repos(id),
    user TEXT NOT NULL,
    PRIMARY KEY (repo_id, user)
);
CREATE TABLE IF NOT EXISTS tokens (
    id TEXT PRIMARY KEY,
    user TEXT NOT NULL,
    name TEXT NOT NULL,
    scopes TEXT NOT NULL,
    hash TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    revoked INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS pull_requests (
    id INTEGER PRIMARY KEY,
    repo_id INTEGER NOT NULL REFERENCES repos(id),
    number INTEGER NOT NULL,
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    author TEXT NOT NULL,
    source_branch TEXT NOT NULL,
    target_branch TEXT NOT NULL,
    base_oid TEXT NOT NULL,
    head_oid TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('open', 'merged', 'closed')),
    merged_oid TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE (repo_id, number),
    CHECK ((state = 'merged') = (merged_oid IS NOT NULL))
);
CREATE TABLE IF NOT EXISTS reviews (
    id INTEGER PRIMARY KEY,
    pr_id INTEGER NOT NULL REFERENCES pull_requests(id),
    author TEXT NOT NULL,
    verdict TEXT NOT NULL CHECK (verdict IN ('approve', 'request_changes', 'comment')),
    head_oid TEXT NOT NULL,
    body TEXT NOT NULL,
    created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS comments (
    id INTEGER PRIMARY KEY,
    pr_id INTEGER NOT NULL REFERENCES pull_requests(id),
    author TEXT NOT NULL,
    head_oid TEXT NOT NULL,
    path TEXT NOT NULL,
    side TEXT NOT NULL CHECK (side IN ('old', 'new')),
    line INTEGER NOT NULL CHECK (line > 0),
    body TEXT NOT NULL,
    created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS statuses (
    id INTEGER PRIMARY KEY,
    repo_id INTEGER NOT NULL REFERENCES repos(id),
    oid TEXT NOT NULL,
    context TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('pending', 'success', 'failure', 'error')),
    target_url TEXT,
    description TEXT,
    creator TEXT NOT NULL,
    created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS statuses_oid ON statuses (repo_id, oid);
CREATE TABLE IF NOT EXISTS mirrors (
    id INTEGER PRIMARY KEY,
    repo_id INTEGER NOT NULL REFERENCES repos(id),
    name TEXT NOT NULL,
    url TEXT NOT NULL,
    username TEXT,
    token_env TEXT,
    UNIQUE (repo_id, name)
);
CREATE TABLE IF NOT EXISTS mirror_jobs (
    id INTEGER PRIMARY KEY,
    repo_id INTEGER NOT NULL REFERENCES repos(id),
    mirror_id INTEGER NOT NULL REFERENCES mirrors(id),
    trigger TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('queued', 'running', 'succeeded', 'failed')),
    attempts INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS webhook_deliveries (
    id INTEGER PRIMARY KEY,
    pr_id INTEGER NOT NULL REFERENCES pull_requests(id),
    delivery_id TEXT NOT NULL UNIQUE,
    action TEXT NOT NULL,
    target TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('pending', 'delivered', 'failed')),
    http_status INTEGER,
    attempts INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    payload BLOB NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
";

pub struct Db {
    conn: Mutex<Connection>,
}

const PR_COLUMNS: &str = "id, repo_id, number, title, body, author, source_branch, target_branch, base_oid, head_oid, state, merged_oid, created_at, updated_at";

fn pr_row(row: &Row<'_>) -> rusqlite::Result<PullRequest> {
    let state: String = row.get(10)?;
    let merged: Option<Oid> = row.get(11)?;
    let state = match (state.as_str(), merged) {
        ("open", None) => PrState::Open,
        ("closed", None) => PrState::Closed,
        ("merged", Some(Oid(commit))) => PrState::Merged { commit },
        (other, _) => {
            return Err(rusqlite::Error::FromSqlConversionFailure(
                10,
                rusqlite::types::Type::Text,
                format!("inconsistent pull request state {other:?}").into(),
            ))
        }
    };
    Ok(PullRequest {
        id: row.get(0)?,
        repo_id: row.get(1)?,
        number: row.get(2)?,
        title: row.get(3)?,
        body: row.get(4)?,
        author: row.get(5)?,
        source_branch: row.get(6)?,
        target_branch: row.get(7)?,
        base_oid: oid(row, 8)?,
        head_oid: oid(row, 9)?,
        state,
        created_at: row.get(12)?,
        updated_at: row.get(13)?,
    })
}

fn repo_row(row: &Row<'_>) -> rusqlite::Result<(i64, String, String, bool, String, u32, String)> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
    ))
}

fn mirror_row(row: &Row<'_>, offset: usize) -> rusqlite::Result<Mirror> {
    Ok(Mirror {
        id: row.get(offset)?,
        name: row.get(offset + 1)?,
        url: row.get(offset + 2)?,
        username: row.get(offset + 3)?,
        token_env: row.get(offset + 4)?,
    })
}

impl Db {
    pub fn open(path: &Path) -> Result<Self, ForgeError> {
        Self::init(Connection::open(path)?)
    }

    pub fn in_memory() -> Result<Self, ForgeError> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self, ForgeError> {
        conn.execute_batch(SCHEMA)?;
        Ok(Db {
            conn: Mutex::new(conn),
        })
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    // ---- repositories -------------------------------------------------

    pub fn insert_repo(
        &self,
        name: &RepoName,
        private: bool,
        default_branch: &str,
    ) -> Result<i64, ForgeError> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO repos (owner, name, private, default_branch, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![name.owner.as_str(), name.name.as_str(), private, default_branch, now()],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn set_repo_policy(
        &self,
        repo_id: i64,
        required_approvals: u32,
        required_checks: &[String],
    ) -> Result<(), ForgeError> {
        let checks = serde_json::to_string(required_checks).expect("strings serialize");
        self.conn().execute(
            "UPDATE repos SET required_approvals = ?2, required_checks = ?3 WHERE id = ?1",
            params![repo_id, required_approvals, checks],
        )?;
        Ok(())
    }

    fn build_repo(
        raw: (i64, String, String, bool, String, u32, String),
    ) -> Result<RepoRecord, ForgeError> {
        let (id, owner, name, private, default_branch, required_approvals, checks) = raw;
        Ok(RepoRecord {
            id,
            name: RepoName::new(&owner, &name)?,
            private,
            default_branch,
            required_approvals,
            required_checks: serde_json::from_str(&checks).map_err(|e| {
                ForgeError::BadRequest(format!("stored required_checks are invalid: {e}"))
            })?,
        })
    }

    pub fn repo(&self, name: &RepoName) -> Result<Option<RepoRecord>, ForgeError> {
        let raw = self
            .conn()
            .query_row(
                "SELECT id, owner, name, private, default_branch, required_approvals, required_checks
                 FROM repos WHERE owner = ?1 AND name = ?2",
                params![name.owner.as_str(), name.name.as_str()],
                repo_row,
            )
            .optional()?;
        raw.map(Self::build_repo).transpose()
    }

    pub fn repo_by_id(&self, id: i64) -> Result<RepoRecord, ForgeError> {
        let raw = self.conn().query_row(
            "SELECT id, owner, name, private, default_branch, required_approvals, required_checks
             FROM repos WHERE id = ?1",
            params![id],
            repo_row,
        )?;
        Self::build_repo(raw)
    }

    pub fn repos(&self) -> Result<Vec<RepoRecord>, ForgeError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, owner, name, private, default_branch, required_approvals, required_checks
             FROM repos ORDER BY owner, name",
        )?;
        let rows = stmt
            .query_map([], repo_row)?
            .collect::<Result<Vec<_>, _>>()?;
        drop(stmt);
        drop(conn);
        rows.into_iter().map(Self::build_repo).collect()
    }

    pub fn add_collaborator(&self, repo_id: i64, user: &str) -> Result<(), ForgeError> {
        self.conn().execute(
            "INSERT OR IGNORE INTO collaborators (repo_id, user) VALUES (?1, ?2)",
            params![repo_id, user],
        )?;
        Ok(())
    }

    pub fn is_collaborator(&self, repo_id: i64, user: &str) -> Result<bool, ForgeError> {
        Ok(self
            .conn()
            .query_row(
                "SELECT 1 FROM collaborators WHERE repo_id = ?1 AND user = ?2",
                params![repo_id, user],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    // ---- tokens -------------------------------------------------------

    pub fn insert_token(
        &self,
        id: &str,
        user: &str,
        name: &str,
        scopes: &str,
        hash: &str,
    ) -> Result<(), ForgeError> {
        self.conn().execute(
            "INSERT INTO tokens (id, user, name, scopes, hash, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, user, name, scopes, hash, now()],
        )?;
        Ok(())
    }

    pub fn token(&self, id: &str) -> Result<Option<TokenRecord>, ForgeError> {
        Ok(self
            .conn()
            .query_row(
                "SELECT id, user, scopes, hash FROM tokens WHERE id = ?1 AND revoked = 0",
                params![id],
                |r| {
                    Ok(TokenRecord {
                        id: r.get(0)?,
                        user: r.get(1)?,
                        scopes: r.get(2)?,
                        hash: r.get(3)?,
                    })
                },
            )
            .optional()?)
    }

    // ---- pull requests ------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    pub fn insert_pr(
        &self,
        repo_id: i64,
        title: &str,
        body: &str,
        author: &str,
        source_branch: &str,
        target_branch: &str,
        base_oid: ObjectId,
        head_oid: ObjectId,
    ) -> Result<PullRequest, ForgeError> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let number: u64 = tx.query_row(
            "SELECT COALESCE(MAX(number), 0) + 1 FROM pull_requests WHERE repo_id = ?1",
            params![repo_id],
            |r| r.get(0),
        )?;
        let ts = now();
        tx.execute(
            "INSERT INTO pull_requests (repo_id, number, title, body, author, source_branch, target_branch,
                 base_oid, head_oid, state, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'open', ?10, ?10)",
            params![
                repo_id,
                number,
                title,
                body,
                author,
                source_branch,
                target_branch,
                base_oid.to_string(),
                head_oid.to_string(),
                ts
            ],
        )?;
        let id = tx.last_insert_rowid();
        let pr = tx.query_row(
            &format!("SELECT {PR_COLUMNS} FROM pull_requests WHERE id = ?1"),
            params![id],
            pr_row,
        )?;
        tx.commit()?;
        Ok(pr)
    }

    pub fn pr(&self, repo_id: i64, number: u64) -> Result<Option<PullRequest>, ForgeError> {
        Ok(self
            .conn()
            .query_row(
                &format!(
                    "SELECT {PR_COLUMNS} FROM pull_requests WHERE repo_id = ?1 AND number = ?2"
                ),
                params![repo_id, number],
                pr_row,
            )
            .optional()?)
    }

    pub fn pr_by_id(&self, id: i64) -> Result<PullRequest, ForgeError> {
        Ok(self.conn().query_row(
            &format!("SELECT {PR_COLUMNS} FROM pull_requests WHERE id = ?1"),
            params![id],
            pr_row,
        )?)
    }

    pub fn prs(&self, repo_id: i64) -> Result<Vec<PullRequest>, ForgeError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT {PR_COLUMNS} FROM pull_requests WHERE repo_id = ?1 ORDER BY number DESC"
        ))?;
        let rows = stmt
            .query_map(params![repo_id], pr_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn open_prs_from_branch(
        &self,
        repo_id: i64,
        branch: &str,
    ) -> Result<Vec<PullRequest>, ForgeError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT {PR_COLUMNS} FROM pull_requests WHERE repo_id = ?1 AND source_branch = ?2 AND state = 'open'"
        ))?;
        let rows = stmt
            .query_map(params![repo_id, branch], pr_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn set_pr_head(&self, pr_id: i64, head: ObjectId) -> Result<(), ForgeError> {
        self.conn().execute(
            "UPDATE pull_requests SET head_oid = ?2, updated_at = ?3 WHERE id = ?1",
            params![pr_id, head.to_string(), now()],
        )?;
        Ok(())
    }

    /// Record the merge, only if the PR is still open at the merged head.
    pub fn mark_merged(
        &self,
        pr_id: i64,
        head: ObjectId,
        commit: ObjectId,
    ) -> Result<bool, ForgeError> {
        let changed = self.conn().execute(
            "UPDATE pull_requests SET state = 'merged', merged_oid = ?3, updated_at = ?4
             WHERE id = ?1 AND state = 'open' AND head_oid = ?2",
            params![pr_id, head.to_string(), commit.to_string(), now()],
        )?;
        Ok(changed == 1)
    }

    pub fn mark_closed(&self, pr_id: i64) -> Result<bool, ForgeError> {
        let changed = self.conn().execute(
            "UPDATE pull_requests SET state = 'closed', updated_at = ?2 WHERE id = ?1 AND state = 'open'",
            params![pr_id, now()],
        )?;
        Ok(changed == 1)
    }

    // ---- reviews and comments ----------------------------------------

    pub fn insert_review(
        &self,
        pr_id: i64,
        author: &str,
        verdict: Verdict,
        head: ObjectId,
        body: &str,
    ) -> Result<i64, ForgeError> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO reviews (pr_id, author, verdict, head_oid, body, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![pr_id, author, verdict, head.to_string(), body, now()],
        )?;
        conn.execute(
            "UPDATE pull_requests SET updated_at = ?2 WHERE id = ?1",
            params![pr_id, now()],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn reviews(&self, pr_id: i64) -> Result<Vec<Review>, ForgeError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, author, verdict, head_oid, body, created_at FROM reviews WHERE pr_id = ?1 ORDER BY id",
        )?;
        let rows = stmt
            .query_map(params![pr_id], |r| {
                Ok(Review {
                    id: r.get(0)?,
                    author: r.get(1)?,
                    verdict: r.get(2)?,
                    head_oid: oid(r, 3)?,
                    body: r.get(4)?,
                    created_at: r.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn insert_comment(
        &self,
        pr_id: i64,
        author: &str,
        anchor: &Anchor,
        body: &str,
    ) -> Result<i64, ForgeError> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO comments (pr_id, author, head_oid, path, side, line, body, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                pr_id,
                author,
                anchor.head_oid.to_string(),
                anchor.path,
                anchor.side,
                anchor.line,
                body,
                now()
            ],
        )?;
        conn.execute(
            "UPDATE pull_requests SET updated_at = ?2 WHERE id = ?1",
            params![pr_id, now()],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn comments(&self, pr_id: i64) -> Result<Vec<InlineComment>, ForgeError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, author, head_oid, path, side, line, body, created_at FROM comments WHERE pr_id = ?1 ORDER BY id",
        )?;
        let rows = stmt
            .query_map(params![pr_id], |r| {
                Ok(InlineComment {
                    id: r.get(0)?,
                    author: r.get(1)?,
                    anchor: Anchor {
                        head_oid: oid(r, 2)?,
                        path: r.get(3)?,
                        side: r.get(4)?,
                        line: r.get(5)?,
                    },
                    body: r.get(6)?,
                    created_at: r.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    // ---- statuses -------------------------------------------------------

    pub fn insert_status(
        &self,
        repo_id: i64,
        commit: ObjectId,
        status: &CheckStatus,
    ) -> Result<(), ForgeError> {
        self.conn().execute(
            "INSERT INTO statuses (repo_id, oid, context, state, target_url, description, creator, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                repo_id,
                commit.to_string(),
                status.context,
                status.state,
                status.target_url,
                status.description,
                status.creator,
                status.created_at
            ],
        )?;
        Ok(())
    }

    /// The latest status per context on `commit`.
    pub fn latest_statuses(
        &self,
        repo_id: i64,
        commit: ObjectId,
    ) -> Result<Vec<CheckStatus>, ForgeError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT context, state, target_url, description, creator, created_at FROM statuses s
             WHERE repo_id = ?1 AND oid = ?2
               AND id = (SELECT MAX(id) FROM statuses t WHERE t.repo_id = s.repo_id AND t.oid = s.oid AND t.context = s.context)
             ORDER BY context",
        )?;
        let rows = stmt
            .query_map(params![repo_id, commit.to_string()], |r| {
                Ok(CheckStatus {
                    context: r.get(0)?,
                    state: r.get(1)?,
                    target_url: r.get(2)?,
                    description: r.get(3)?,
                    creator: r.get(4)?,
                    created_at: r.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    // ---- mirrors ---------------------------------------------------------

    pub fn add_mirror(
        &self,
        repo_id: i64,
        name: &str,
        url: &str,
        username: Option<&str>,
        token_env: Option<&str>,
    ) -> Result<i64, ForgeError> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO mirrors (repo_id, name, url, username, token_env) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![repo_id, name, url, username, token_env],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Queue one job per configured mirror. Returns how many were queued.
    pub fn queue_mirror_jobs(&self, repo_id: i64, trigger: &str) -> Result<usize, ForgeError> {
        let ts = now();
        Ok(self.conn().execute(
            "INSERT INTO mirror_jobs (repo_id, mirror_id, trigger, status, created_at, updated_at)
             SELECT repo_id, id, ?2, 'queued', ?3, ?3 FROM mirrors WHERE repo_id = ?1",
            params![repo_id, trigger, ts],
        )?)
    }

    const JOB_SELECT: &'static str =
        "SELECT j.id, j.repo_id, j.trigger, j.status, j.attempts, j.last_error,
            m.id, m.name, m.url, m.username, m.token_env
         FROM mirror_jobs j JOIN mirrors m ON m.id = j.mirror_id";

    fn job_row(r: &Row<'_>) -> rusqlite::Result<MirrorJob> {
        Ok(MirrorJob {
            id: r.get(0)?,
            repo_id: r.get(1)?,
            trigger: r.get(2)?,
            status: r.get(3)?,
            attempts: r.get(4)?,
            last_error: r.get(5)?,
            mirror: mirror_row(r, 6)?,
        })
    }

    /// Claim the oldest queued job.
    pub fn claim_mirror_job(&self) -> Result<Option<MirrorJob>, ForgeError> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let job = tx
            .query_row(
                &format!(
                    "{} WHERE j.status = 'queued' ORDER BY j.id LIMIT 1",
                    Self::JOB_SELECT
                ),
                [],
                Self::job_row,
            )
            .optional()?;
        if let Some(job) = &job {
            tx.execute(
                "UPDATE mirror_jobs SET status = 'running', attempts = attempts + 1, updated_at = ?2 WHERE id = ?1",
                params![job.id, now()],
            )?;
        }
        tx.commit()?;
        Ok(job)
    }

    pub fn finish_mirror_job(
        &self,
        id: i64,
        status: JobStatus,
        error: Option<&str>,
    ) -> Result<(), ForgeError> {
        self.conn().execute(
            "UPDATE mirror_jobs SET status = ?2, last_error = ?3, updated_at = ?4 WHERE id = ?1",
            params![id, status, error, now()],
        )?;
        Ok(())
    }

    /// The latest job per mirror of a repository.
    pub fn mirror_status(&self, repo_id: i64) -> Result<Vec<MirrorJob>, ForgeError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!(
            "{} WHERE j.repo_id = ?1 AND j.id = (SELECT MAX(id) FROM mirror_jobs k WHERE k.mirror_id = j.mirror_id)
             ORDER BY m.name",
            Self::JOB_SELECT
        ))?;
        let rows = stmt
            .query_map(params![repo_id], Self::job_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    // ---- webhook deliveries ----------------------------------------------

    pub fn insert_delivery(
        &self,
        pr_id: i64,
        delivery_id: &str,
        action: &str,
        target: &str,
        payload: &[u8],
    ) -> Result<i64, ForgeError> {
        let conn = self.conn();
        let ts = now();
        conn.execute(
            "INSERT INTO webhook_deliveries (pr_id, delivery_id, action, target, status, payload, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, 'pending', ?5, ?6, ?6)",
            params![pr_id, delivery_id, action, target, payload, ts],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn finish_delivery(
        &self,
        id: i64,
        status: DeliveryStatus,
        http_status: Option<u16>,
        attempts: u32,
        error: Option<&str>,
    ) -> Result<(), ForgeError> {
        self.conn().execute(
            "UPDATE webhook_deliveries SET status = ?2, http_status = ?3, attempts = ?4, last_error = ?5, updated_at = ?6
             WHERE id = ?1",
            params![id, status, http_status, attempts, error, now()],
        )?;
        Ok(())
    }

    const DELIVERY_SELECT: &'static str =
        "SELECT id, pr_id, delivery_id, action, target, status, http_status, attempts, last_error, payload
         FROM webhook_deliveries";

    fn delivery_row(r: &Row<'_>) -> rusqlite::Result<WebhookDelivery> {
        Ok(WebhookDelivery {
            id: r.get(0)?,
            pr_id: r.get(1)?,
            delivery_id: r.get(2)?,
            action: r.get(3)?,
            target: r.get(4)?,
            status: r.get(5)?,
            http_status: r.get(6)?,
            attempts: r.get(7)?,
            last_error: r.get(8)?,
            payload: r.get(9)?,
        })
    }

    pub fn deliveries(&self, pr_id: i64) -> Result<Vec<WebhookDelivery>, ForgeError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!(
            "{} WHERE pr_id = ?1 ORDER BY id",
            Self::DELIVERY_SELECT
        ))?;
        let rows = stmt
            .query_map(params![pr_id], Self::delivery_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// The oldest pending delivery, so deliveries leave in emission order.
    pub fn next_pending_delivery(&self) -> Result<Option<WebhookDelivery>, ForgeError> {
        Ok(self
            .conn()
            .query_row(
                &format!(
                    "{} WHERE status = 'pending' ORDER BY id LIMIT 1",
                    Self::DELIVERY_SELECT
                ),
                [],
                Self::delivery_row,
            )
            .optional()?)
    }

    /// Jobs a stopped process left running go back to the queue.
    pub fn requeue_running_mirror_jobs(&self) -> Result<usize, ForgeError> {
        Ok(self.conn().execute(
            "UPDATE mirror_jobs SET status = 'queued', updated_at = ?1 WHERE status = 'running'",
            params![now()],
        )?)
    }
}
