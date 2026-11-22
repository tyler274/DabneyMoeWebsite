//! Verbatim memory organized by wing, room, and drawer.
//!
//! Drawers retain exact authored text alongside a SHA-256 integrity digest and
//! never paraphrase or summarize stored content. Products supply their own
//! durable [`PalaceBackend`] or mount [`InMemoryPalace`] for ephemeral runs,
//! and can expose the [`tools`] schemas directly in a harness recipe.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    str::FromStr,
    sync::RwLock,
};

use async_trait::async_trait;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::engine::Tool;

/// Tool name for storing a verbatim drawer.
pub const TOOL_STORE: &str = "palace_store";
/// Tool name for searching verbatim drawers within an optional wing/room scope.
pub const TOOL_SEARCH: &str = "palace_search";
/// Tool name for browsing wings, rooms within a wing, or drawers within a room.
pub const TOOL_BROWSE: &str = "palace_browse";

const DEFAULT_SEARCH_LIMIT: usize = 10;

/// Returns whether `name` is one of the palace memory tools.
pub fn is_tool(name: &str) -> bool {
    matches!(name, TOOL_STORE | TOOL_SEARCH | TOOL_BROWSE)
}

/// Which identifier kind failed boundary validation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdKind {
    /// Top-level memory partition.
    Wing,
    /// Named sub-partition within a wing.
    Room,
    /// Individual verbatim drawer record.
    Drawer,
}

impl fmt::Display for IdKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Wing => f.write_str("wing"),
            Self::Room => f.write_str("room"),
            Self::Drawer => f.write_str("drawer"),
        }
    }
}

/// Typed failures for verbatim palace validation, storage, search, and tool dispatch.
#[derive(Debug, thiserror::Error)]
pub enum PalaceError {
    /// An identifier failed boundary validation.
    #[error("invalid {kind} id: {reason}")]
    InvalidId {
        /// Identifier category that failed validation.
        kind: IdKind,
        /// Exact validation rule violated.
        reason: &'static str,
    },
    /// Drawer content must be non-empty.
    #[error("drawer content must not be empty")]
    EmptyContent,
    /// Search queries must contain at least one non-whitespace character.
    #[error("search query must not be empty")]
    EmptyQuery,
    /// A search or browse request supplied a room without its enclosing wing.
    #[error("room scope requires a wing id")]
    RoomWithoutWing,
    /// A drawer's recorded digest did not match its verbatim content.
    #[error("content hash mismatch for drawer {drawer_id}: expected {expected}, got {actual}")]
    ContentHashMismatch {
        /// Drawer whose integrity check failed.
        drawer_id: DrawerId,
        /// SHA-256 digest computed from `content`.
        expected: String,
        /// Digest carried on the drawer.
        actual: String,
    },
    /// A drawer with the same identity already exists in the palace.
    #[error("drawer {drawer_id} already exists")]
    DuplicateDrawer {
        /// Existing drawer identity.
        drawer_id: DrawerId,
    },
    /// Tool arguments failed typed JSON deserialization.
    #[error("invalid palace tool arguments: {source}")]
    InvalidArguments {
        /// Underlying JSON decoding error.
        #[source]
        source: serde_json::Error,
    },
    /// Tool dispatch received an unrecognized tool name.
    #[error("unknown palace tool: {name}")]
    UnknownTool {
        /// Unrecognized tool name.
        name: String,
    },
    /// The in-memory store lock was poisoned by a panic on another thread.
    #[error("in-memory palace lock poisoned")]
    LockPoisoned,
    /// A custom backend failed while serving a palace operation.
    #[error("palace backend error ({message}): {source}")]
    Backend {
        /// Operation context describing what failed.
        message: String,
        /// Original backend error cause.
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

impl PalaceError {
    /// Wrap a custom backend error while retaining its original source.
    pub fn backend(
        message: impl Into<String>,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self::Backend {
            message: message.into(),
            source: Box::new(source),
        }
    }
}

/// Single boundary validation rule shared by [`WingId`], [`RoomId`], and [`DrawerId`].
fn validate_id(kind: IdKind, raw: String) -> Result<String, PalaceError> {
    if raw.trim().is_empty() {
        return Err(PalaceError::InvalidId {
            kind,
            reason: "must not be empty or whitespace",
        });
    }
    if raw.trim() != raw {
        return Err(PalaceError::InvalidId {
            kind,
            reason: "must not have leading or trailing whitespace",
        });
    }
    if raw.chars().any(char::is_control) {
        return Err(PalaceError::InvalidId {
            kind,
            reason: "must not contain control characters",
        });
    }
    Ok(raw)
}

macro_rules! define_palace_id {
    ($name:ident, $kind:expr, $doc:literal) => {
        #[doc = $doc]
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Construct a validated identifier at the boundary.
            pub fn new(raw: impl Into<String>) -> Result<Self, PalaceError> {
                validate_id($kind, raw.into()).map(Self)
            }

            /// Borrow the validated identifier text.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl TryFrom<String> for $name {
            type Error = PalaceError;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl TryFrom<&str> for $name {
            type Error = PalaceError;

            fn try_from(value: &str) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl FromStr for $name {
            type Err = PalaceError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::new(value)
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                self.as_str()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let raw = String::deserialize(deserializer)?;
                Self::new(raw).map_err(serde::de::Error::custom)
            }
        }
    };
}

define_palace_id!(
    WingId,
    IdKind::Wing,
    "Validated non-empty identifier for a top-level memory wing."
);
define_palace_id!(
    RoomId,
    IdKind::Room,
    "Validated non-empty identifier for a room inside a wing."
);
define_palace_id!(
    DrawerId,
    IdKind::Drawer,
    "Validated non-empty identifier for a verbatim memory drawer."
);

/// Compute the lowercase hex SHA-256 digest of verbatim drawer content.
pub fn compute_content_hash(content: &str) -> String {
    let digest = Sha256::digest(content.as_bytes());
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = fmt::Write::write_fmt(&mut hex, format_args!("{byte:02x}"));
    }
    hex
}

/// One verbatim memory record located at `(wing, room, id)`.
///
/// The drawer preserves `content` byte-for-byte without paraphrasing or
/// summarizing, paired with its SHA-256 [`Self::content_hash`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Drawer {
    /// Unique drawer identity.
    pub id: DrawerId,
    /// Top-level wing partition.
    pub wing: WingId,
    /// Room partition inside the wing.
    pub room: RoomId,
    /// Exact verbatim content; never paraphrased or summarized.
    pub content: String,
    /// Lowercase hex SHA-256 digest of `content`.
    pub content_hash: String,
    /// Unix timestamp in seconds when this drawer was recorded.
    pub recorded_at_unix: i64,
    /// Optional provenance reference pointing back to the original source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_ref: Option<String>,
    /// Optional verbatim tags used as secondary search cues.
    #[serde(default)]
    pub tags: Vec<String>,
}

impl Drawer {
    /// Construct a verbatim drawer and compute its SHA-256 content digest.
    pub fn new(
        id: DrawerId,
        wing: WingId,
        room: RoomId,
        content: impl Into<String>,
        recorded_at_unix: i64,
        source_ref: Option<String>,
        tags: Vec<String>,
    ) -> Result<Self, PalaceError> {
        let content = content.into();
        if content.is_empty() {
            return Err(PalaceError::EmptyContent);
        }
        let content_hash = compute_content_hash(&content);
        Ok(Self {
            id,
            wing,
            room,
            content,
            content_hash,
            recorded_at_unix,
            source_ref,
            tags,
        })
    }

    /// Returns whether [`Self::content_hash`] matches [`Self::content`].
    pub fn verify_integrity(&self) -> bool {
        !self.content.is_empty() && self.content_hash == compute_content_hash(&self.content)
    }
}

/// Scope restricting a verbatim drawer search.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchScope {
    /// Search across every wing and room.
    All,
    /// Restrict search to one wing.
    Wing(WingId),
    /// Restrict search to a single room within a wing.
    Room {
        /// Target wing identifier.
        wing: WingId,
        /// Target room identifier.
        room: RoomId,
    },
}

impl SearchScope {
    /// Convenience constructor for [`Self::Wing`].
    pub fn wing(wing: WingId) -> Self {
        Self::Wing(wing)
    }

    /// Convenience constructor for [`Self::Room`].
    pub fn room(wing: WingId, room: RoomId) -> Self {
        Self::Room { wing, room }
    }

    /// Returns whether `drawer` falls inside this search scope.
    pub fn matches(&self, drawer: &Drawer) -> bool {
        match self {
            Self::All => true,
            Self::Wing(wing) => drawer.wing == *wing,
            Self::Room { wing, room } => drawer.wing == *wing && drawer.room == *room,
        }
    }
}

/// A verbatim drawer paired with its deterministic relevance score.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SearchHit {
    /// Matched verbatim drawer.
    pub drawer: Drawer,
    /// Deterministic relevance score (higher is more relevant).
    pub score: f64,
}

/// Persistence and retrieval contract for a verbatim memory palace.
#[async_trait]
pub trait PalaceBackend: Send + Sync {
    /// Store a verbatim drawer after verifying its content hash.
    async fn store_drawer(&self, drawer: Drawer) -> Result<Drawer, PalaceError>;

    /// Fetch a verbatim drawer by its exact identifier.
    async fn get_drawer(&self, id: &DrawerId) -> Result<Option<Drawer>, PalaceError>;

    /// List all wings currently present, in deterministic sorted order.
    async fn list_wings(&self) -> Result<Vec<WingId>, PalaceError>;

    /// List all rooms currently present in `wing`, in deterministic sorted order.
    async fn list_rooms(&self, wing: &WingId) -> Result<Vec<RoomId>, PalaceError>;

    /// List all drawers currently present in `(wing, room)`, in deterministic sorted order.
    async fn list_drawers(&self, wing: &WingId, room: &RoomId) -> Result<Vec<Drawer>, PalaceError>;

    /// Search verbatim drawers within `scope`, returning up to `limit` hits ranked by score.
    async fn search_drawers(
        &self,
        query: &str,
        scope: SearchScope,
        limit: usize,
    ) -> Result<Vec<SearchHit>, PalaceError>;
}

/// Thread-safe, pure-Rust in-memory implementation of [`PalaceBackend`].
///
/// Ranking combines deterministic token overlap over verbatim content and tags
/// with exact-phrase bonuses so literal quotes always rank highest.
#[derive(Debug, Default)]
pub struct InMemoryPalace {
    drawers: RwLock<BTreeMap<DrawerId, Drawer>>,
}

impl InMemoryPalace {
    /// Create an empty in-memory palace.
    pub fn new() -> Self {
        Self::default()
    }

    fn read_drawers(
        &self,
    ) -> Result<std::sync::RwLockReadGuard<'_, BTreeMap<DrawerId, Drawer>>, PalaceError> {
        match self.drawers.read() {
            Ok(guard) => Ok(guard),
            Err(_) => Err(PalaceError::LockPoisoned),
        }
    }

    fn write_drawers(
        &self,
    ) -> Result<std::sync::RwLockWriteGuard<'_, BTreeMap<DrawerId, Drawer>>, PalaceError> {
        match self.drawers.write() {
            Ok(guard) => Ok(guard),
            Err(_) => Err(PalaceError::LockPoisoned),
        }
    }
}

#[async_trait]
impl PalaceBackend for InMemoryPalace {
    async fn store_drawer(&self, drawer: Drawer) -> Result<Drawer, PalaceError> {
        if drawer.content.is_empty() {
            return Err(PalaceError::EmptyContent);
        }
        let expected = compute_content_hash(&drawer.content);
        if drawer.content_hash != expected {
            return Err(PalaceError::ContentHashMismatch {
                drawer_id: drawer.id,
                expected,
                actual: drawer.content_hash,
            });
        }
        let mut drawers = self.write_drawers()?;
        if drawers.contains_key(&drawer.id) {
            return Err(PalaceError::DuplicateDrawer {
                drawer_id: drawer.id,
            });
        }
        drawers.insert(drawer.id.clone(), drawer.clone());
        Ok(drawer)
    }

    async fn get_drawer(&self, id: &DrawerId) -> Result<Option<Drawer>, PalaceError> {
        let drawers = self.read_drawers()?;
        Ok(drawers.get(id).cloned())
    }

    async fn list_wings(&self) -> Result<Vec<WingId>, PalaceError> {
        let drawers = self.read_drawers()?;
        let wings: BTreeSet<WingId> = drawers.values().map(|d| d.wing.clone()).collect();
        Ok(wings.into_iter().collect())
    }

    async fn list_rooms(&self, wing: &WingId) -> Result<Vec<RoomId>, PalaceError> {
        let drawers = self.read_drawers()?;
        let rooms: BTreeSet<RoomId> = drawers
            .values()
            .filter(|d| d.wing == *wing)
            .map(|d| d.room.clone())
            .collect();
        Ok(rooms.into_iter().collect())
    }

    async fn list_drawers(&self, wing: &WingId, room: &RoomId) -> Result<Vec<Drawer>, PalaceError> {
        let drawers = self.read_drawers()?;
        Ok(drawers
            .values()
            .filter(|d| d.wing == *wing && d.room == *room)
            .cloned()
            .collect())
    }

    async fn search_drawers(
        &self,
        query: &str,
        scope: SearchScope,
        limit: usize,
    ) -> Result<Vec<SearchHit>, PalaceError> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return Err(PalaceError::EmptyQuery);
        }
        if limit == 0 {
            return Ok(Vec::new());
        }
        let query_lower = trimmed.to_lowercase();
        let query_tokens = tokenize(&query_lower);

        let drawers = self.read_drawers()?;
        let mut hits: Vec<SearchHit> = drawers
            .values()
            .filter(|drawer| scope.matches(drawer))
            .filter_map(|drawer| {
                let score = score_drawer(drawer, trimmed, &query_lower, &query_tokens);
                (score > 0.0).then(|| SearchHit {
                    drawer: drawer.clone(),
                    score,
                })
            })
            .collect();

        hits.sort_by(|left, right| {
            right
                .score
                .total_cmp(&left.score)
                .then_with(|| {
                    right
                        .drawer
                        .recorded_at_unix
                        .cmp(&left.drawer.recorded_at_unix)
                })
                .then_with(|| left.drawer.id.cmp(&right.drawer.id))
        });
        hits.truncate(limit);
        Ok(hits)
    }
}

fn tokenize(text: &str) -> BTreeSet<String> {
    text.split(|ch: char| !ch.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Deterministic token-overlap + exact-phrase relevance score over a verbatim drawer.
fn score_drawer(
    drawer: &Drawer,
    query_exact: &str,
    query_lower: &str,
    query_tokens: &BTreeSet<String>,
) -> f64 {
    let content_lower = drawer.content.to_lowercase();
    let mut score = 0.0;

    if !query_tokens.is_empty() {
        let content_tokens = tokenize(&content_lower);
        let content_matches = query_tokens
            .iter()
            .filter(|token| content_tokens.contains(*token))
            .count();
        score += content_matches as f64 / query_tokens.len() as f64;

        if !drawer.tags.is_empty() {
            let tag_tokens: BTreeSet<String> = drawer
                .tags
                .iter()
                .flat_map(|tag| tokenize(&tag.to_lowercase()))
                .collect();
            let tag_matches = query_tokens
                .iter()
                .filter(|token| tag_tokens.contains(*token))
                .count();
            score += (tag_matches as f64 / query_tokens.len() as f64) * 0.25;
        }
    }

    if content_lower.contains(query_lower) {
        score += 1.0;
        if drawer.content.contains(query_exact) {
            score += 0.25;
        }
    }

    score
}

/// Build the three harness [`Tool`] definitions (`palace_store`, `palace_search`, `palace_browse`).
pub fn tools() -> [Tool; 3] {
    [
        Tool {
            name: TOOL_STORE.into(),
            description: "Store a verbatim memory drawer under a wing and room without paraphrasing or summarizing. Returns the stored drawer with its SHA-256 content_hash.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "minLength": 1, "description": "Unique drawer identifier." },
                    "wing": { "type": "string", "minLength": 1, "description": "Top-level memory wing." },
                    "room": { "type": "string", "minLength": 1, "description": "Room inside the wing." },
                    "content": { "type": "string", "minLength": 1, "description": "Exact verbatim content to retain." },
                    "recorded_at_unix": { "type": "integer", "description": "Unix timestamp in seconds when recorded." },
                    "source_ref": { "type": ["string", "null"], "description": "Optional provenance reference." },
                    "tags": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Optional secondary search tags."
                    }
                },
                "required": ["id", "wing", "room", "content", "recorded_at_unix"],
                "additionalProperties": false
            }),
        },
        Tool {
            name: TOOL_SEARCH.into(),
            description: "Search verbatim drawers by text query, optionally scoped to a wing or a wing and room. Exact phrase matches outrank partial token matches.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "minLength": 1, "description": "Text or phrase to search for." },
                    "wing": { "type": ["string", "null"], "minLength": 1, "description": "Optional wing filter." },
                    "room": { "type": ["string", "null"], "minLength": 1, "description": "Optional room filter; requires wing." },
                    "limit": { "type": ["integer", "null"], "minimum": 1, "description": format!("Maximum hits to return. Null defaults to {DEFAULT_SEARCH_LIMIT}.") }
                },
                "required": ["query"],
                "additionalProperties": false
            }),
        },
        Tool {
            name: TOOL_BROWSE.into(),
            description: "Browse palace structure progressively: omit wing and room to list wings, supply wing to list rooms, or supply wing and room to list verbatim drawers.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "wing": { "type": ["string", "null"], "minLength": 1, "description": "Optional wing to inspect." },
                    "room": { "type": ["string", "null"], "minLength": 1, "description": "Optional room to inspect; requires wing." }
                },
                "required": [],
                "additionalProperties": false
            }),
        },
    ]
}

/// Return wire JSON function schemas for [`tools`], ready for a harness [`crate::engine::recipe::RequestRecipe`].
pub fn tool_schemas() -> [Value; 3] {
    tools().map(|tool| tool.function())
}

/// Typed arguments for [`TOOL_STORE`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoreArgs {
    /// Unique drawer identifier.
    pub id: DrawerId,
    /// Target wing identifier.
    pub wing: WingId,
    /// Target room identifier.
    pub room: RoomId,
    /// Exact verbatim text to store.
    pub content: String,
    /// Unix timestamp in seconds when recorded.
    pub recorded_at_unix: i64,
    /// Optional provenance reference.
    #[serde(default)]
    pub source_ref: Option<String>,
    /// Optional search tags.
    #[serde(default)]
    pub tags: Vec<String>,
}

/// Typed arguments for [`TOOL_SEARCH`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SearchArgs {
    /// Text or phrase query.
    pub query: String,
    /// Optional wing scope.
    #[serde(default)]
    pub wing: Option<WingId>,
    /// Optional room scope (requires `wing`).
    #[serde(default)]
    pub room: Option<RoomId>,
    /// Optional result cap.
    #[serde(default)]
    pub limit: Option<usize>,
}

impl SearchArgs {
    /// Convert the optional wing/room fields into a validated [`SearchScope`].
    pub fn scope(&self) -> Result<SearchScope, PalaceError> {
        match (&self.wing, &self.room) {
            (None, None) => Ok(SearchScope::All),
            (Some(wing), None) => Ok(SearchScope::wing(wing.clone())),
            (Some(wing), Some(room)) => Ok(SearchScope::room(wing.clone(), room.clone())),
            (None, Some(_)) => Err(PalaceError::RoomWithoutWing),
        }
    }
}

/// Typed arguments for [`TOOL_BROWSE`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowseArgs {
    /// Optional wing to inspect.
    #[serde(default)]
    pub wing: Option<WingId>,
    /// Optional room to inspect (requires `wing`).
    #[serde(default)]
    pub room: Option<RoomId>,
}

/// Validated target for a [`TOOL_BROWSE`] invocation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BrowseTarget {
    /// List all top-level wings.
    Wings,
    /// List all rooms inside `wing`.
    Rooms {
        /// Wing to inspect.
        wing: WingId,
    },
    /// List all drawers inside `(wing, room)`.
    Drawers {
        /// Wing to inspect.
        wing: WingId,
        /// Room to inspect.
        room: RoomId,
    },
}

impl BrowseArgs {
    /// Convert the optional wing/room fields into a validated [`BrowseTarget`].
    pub fn target(&self) -> Result<BrowseTarget, PalaceError> {
        match (&self.wing, &self.room) {
            (None, None) => Ok(BrowseTarget::Wings),
            (Some(wing), None) => Ok(BrowseTarget::Rooms { wing: wing.clone() }),
            (Some(wing), Some(room)) => Ok(BrowseTarget::Drawers {
                wing: wing.clone(),
                room: room.clone(),
            }),
            (None, Some(_)) => Err(PalaceError::RoomWithoutWing),
        }
    }
}

/// Parsed typed palace tool invocation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PalaceToolCall {
    /// Store a verbatim drawer.
    Store(StoreArgs),
    /// Search verbatim drawers.
    Search(SearchArgs),
    /// Browse wings, rooms, or drawers.
    Browse(BrowseArgs),
}

/// Typed result emitted by [`dispatch_tool`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PalaceToolOutput {
    /// Stored drawer receipt.
    Stored {
        /// Stored verbatim drawer with computed digest.
        drawer: Drawer,
    },
    /// Scoped search hits.
    SearchResults {
        /// Ranked search hits.
        hits: Vec<SearchHit>,
    },
    /// Listed top-level wings.
    Wings {
        /// Sorted wing identifiers.
        wings: Vec<WingId>,
    },
    /// Listed rooms in a wing.
    Rooms {
        /// Parent wing identifier.
        wing: WingId,
        /// Sorted room identifiers.
        rooms: Vec<RoomId>,
    },
    /// Listed verbatim drawers in a room.
    Drawers {
        /// Parent wing identifier.
        wing: WingId,
        /// Parent room identifier.
        room: RoomId,
        /// Drawers in the room.
        drawers: Vec<Drawer>,
    },
}

/// Parse a raw tool name and JSON arguments into a typed [`PalaceToolCall`].
pub fn parse_tool_call(name: &str, arguments: Value) -> Result<PalaceToolCall, PalaceError> {
    match name {
        TOOL_STORE => serde_json::from_value(arguments)
            .map(PalaceToolCall::Store)
            .map_err(|source| PalaceError::InvalidArguments { source }),
        TOOL_SEARCH => serde_json::from_value(arguments)
            .map(PalaceToolCall::Search)
            .map_err(|source| PalaceError::InvalidArguments { source }),
        TOOL_BROWSE => serde_json::from_value(arguments)
            .map(PalaceToolCall::Browse)
            .map_err(|source| PalaceError::InvalidArguments { source }),
        other => Err(PalaceError::UnknownTool {
            name: other.to_owned(),
        }),
    }
}

/// Execute a palace tool invocation against `backend` and return a typed [`PalaceToolOutput`].
pub async fn dispatch_tool(
    backend: &dyn PalaceBackend,
    name: &str,
    arguments: Value,
) -> Result<PalaceToolOutput, PalaceError> {
    match parse_tool_call(name, arguments)? {
        PalaceToolCall::Store(args) => {
            let drawer = Drawer::new(
                args.id,
                args.wing,
                args.room,
                args.content,
                args.recorded_at_unix,
                args.source_ref,
                args.tags,
            )?;
            let stored = backend.store_drawer(drawer).await?;
            Ok(PalaceToolOutput::Stored { drawer: stored })
        }
        PalaceToolCall::Search(args) => {
            let scope = args.scope()?;
            let limit = args.limit.unwrap_or(DEFAULT_SEARCH_LIMIT);
            let hits = backend.search_drawers(&args.query, scope, limit).await?;
            Ok(PalaceToolOutput::SearchResults { hits })
        }
        PalaceToolCall::Browse(args) => match args.target()? {
            BrowseTarget::Wings => {
                let wings = backend.list_wings().await?;
                Ok(PalaceToolOutput::Wings { wings })
            }
            BrowseTarget::Rooms { wing } => {
                let rooms = backend.list_rooms(&wing).await?;
                Ok(PalaceToolOutput::Rooms { wing, rooms })
            }
            BrowseTarget::Drawers { wing, room } => {
                let drawers = backend.list_drawers(&wing, &room).await?;
                Ok(PalaceToolOutput::Drawers {
                    wing,
                    room,
                    drawers,
                })
            }
        },
    }
}

/// Execute a palace tool invocation and encode its typed output as JSON for a harness tool result.
pub async fn dispatch_tool_json(
    backend: &dyn PalaceBackend,
    name: &str,
    arguments: Value,
) -> Result<Value, PalaceError> {
    let output = dispatch_tool(backend, name, arguments).await?;
    serde_json::to_value(output).map_err(|source| PalaceError::InvalidArguments { source })
}
