//! Pure-Rust SQLite execution driver backed by `turso_core` across native and `wasm32` targets.

use sea_query::{
    DeleteStatement, Expr, ExprTrait, Iden, InsertStatement, Query, SelectStatement, SimpleExpr,
    SqliteQueryBuilder, UpdateStatement, Value, Values,
};
use serde_json::Value as JsonValue;
use std::collections::{BTreeMap, HashMap};
use std::num::NonZero;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
#[cfg(not(target_arch = "wasm32"))]
use turso_core::io::clock::DefaultClock;
use turso_core::io::{
    Buffer, Clock, Completion, File, FileId, FileSyncType, IO, OpenFlags,
    clock::{MonotonicInstant, WallClockInstant},
};
use turso_core::{
    CheckpointMode, CompletionError, Connection, Database, DatabaseOpts, LimboError, Numeric,
    OpenOptions, SqliteDialect, Value as TursoValue,
};

static NEXT_DB_ID: AtomicU64 = AtomicU64::new(1);

/// Error returned by the demo SQLite driver or typed store.
#[derive(Debug, thiserror::Error)]
pub enum DbError {
    /// Failed to decode JSON payloads stored in or returned from SQLite.
    #[error("demo SQLite JSON error")]
    Json(#[from] serde_json::Error),
    /// Pure-Rust SQLite engine (`turso_core`) failure.
    #[error("SQLite engine error")]
    Sqlite(#[from] LimboError),
    /// Native snapshot file I/O failure.
    #[cfg(not(target_arch = "wasm32"))]
    #[error("SQLite file I/O error at `{path}`")]
    Io {
        /// Snapshot file path whose read or write failed.
        path: std::path::PathBuf,
        /// Underlying OS I/O error.
        #[source]
        source: std::io::Error,
    },
    /// Browser asset fetch or IndexedDB persistence bridge failure.
    #[cfg(target_arch = "wasm32")]
    #[error("browser SQLite bridge error: {0}")]
    Browser(String),
    /// Missing expected column in query result row.
    #[error("missing SQLite column `{column}`")]
    MissingColumn {
        /// Column name expected by the typed store.
        column: &'static str,
    },
    /// Requested entity does not exist in the demo database.
    #[error("{kind} `{id}` not found")]
    NotFound {
        /// Domain entity label.
        kind: &'static str,
        /// Entity identifier.
        id: String,
    },
    /// Invalid command payload sent to the demo store.
    #[error("{0}")]
    InvalidCommand(String),
    /// Optimistic version or state conflict preventing mutation.
    #[error("{0}")]
    Conflict(String),
}

/// Typed SQL parameter converted from `sea_query::Value`.
#[derive(Debug, Clone)]
pub enum SqlParam {
    /// SQL `NULL`.
    Null,
    /// SQL integer.
    Int(i64),
    /// SQL floating-point number.
    Real(f64),
    /// SQL UTF-8 text.
    Text(String),
}

impl From<&Value> for SqlParam {
    fn from(value: &Value) -> Self {
        match value {
            Value::Bool(Some(b)) => Self::Int(i64::from(*b)),
            Value::TinyInt(Some(n)) => Self::Int(i64::from(*n)),
            Value::SmallInt(Some(n)) => Self::Int(i64::from(*n)),
            Value::Int(Some(n)) => Self::Int(i64::from(*n)),
            Value::BigInt(Some(n)) => Self::Int(*n),
            Value::TinyUnsigned(Some(n)) => Self::Int(i64::from(*n)),
            Value::SmallUnsigned(Some(n)) => Self::Int(i64::from(*n)),
            Value::Unsigned(Some(n)) => Self::Int(i64::from(*n)),
            Value::BigUnsigned(Some(n)) => Self::Int(i64::try_from(*n).unwrap_or(i64::MAX)),
            Value::Float(Some(f)) => Self::Real(f64::from(*f)),
            Value::Double(Some(f)) => Self::Real(*f),
            Value::String(Some(s)) => Self::Text(s.to_string()),
            Value::Char(Some(c)) => Self::Text(c.to_string()),
            _ => Self::Null,
        }
    }
}

impl SqlParam {
    fn to_turso_value(&self) -> TursoValue {
        match self {
            Self::Null => TursoValue::Null,
            Self::Int(n) => TursoValue::from_i64(*n),
            Self::Real(f) => TursoValue::from_f64(*f),
            Self::Text(s) => TursoValue::build_text(s.clone()),
        }
    }
}

fn bind_values(values: Values) -> Vec<SqlParam> {
    values.0.iter().map(SqlParam::from).collect()
}

/// One row returned from SQLite with column names mapped to JSON values.
#[derive(Debug, Clone)]
pub struct SqlRow(BTreeMap<String, JsonValue>);

impl SqlRow {
    /// Reads a required UTF-8 string column.
    pub fn get_str(&self, column: &'static str) -> Result<String, DbError> {
        match self.0.get(column) {
            Some(JsonValue::String(s)) => Ok(s.clone()),
            Some(JsonValue::Number(n)) => Ok(n.to_string()),
            _ => Err(DbError::MissingColumn { column }),
        }
    }

    /// Reads a required integer column.
    pub fn get_i64(&self, column: &'static str) -> Result<i64, DbError> {
        match self.0.get(column) {
            Some(JsonValue::Number(n)) => n.as_i64().ok_or(DbError::MissingColumn { column }),
            _ => Err(DbError::MissingColumn { column }),
        }
    }
}

fn turso_value_to_json(value: &TursoValue) -> JsonValue {
    match value {
        TursoValue::Null => JsonValue::Null,
        TursoValue::Numeric(Numeric::Integer(n)) => JsonValue::from(*n),
        TursoValue::Numeric(Numeric::Float(f)) => {
            serde_json::Number::from_f64(f64::from(*f)).map_or(JsonValue::Null, JsonValue::Number)
        }
        TursoValue::Text(text) => JsonValue::String(text.as_str().to_string()),
        TursoValue::Blob(bytes) => {
            JsonValue::Array(bytes.iter().copied().map(JsonValue::from).collect())
        }
    }
}

struct BufferFile {
    data: RwLock<Vec<u8>>,
}

impl BufferFile {
    fn from_bytes(bytes: Vec<u8>) -> Self {
        Self {
            data: RwLock::new(bytes),
        }
    }

    fn bytes(&self) -> Vec<u8> {
        self.data.read().expect("buffer file read lock").clone()
    }

    fn write_locked(buf: &mut Vec<u8>, pos: u64, src: &[u8]) {
        let start = usize::try_from(pos).unwrap_or(usize::MAX);
        let end = start.saturating_add(src.len());
        if buf.len() < end {
            buf.resize(end, 0);
        }
        buf[start..end].copy_from_slice(src);
    }
}

impl File for BufferFile {
    fn lock_file(&self, _exclusive: bool) -> turso_core::Result<()> {
        Ok(())
    }

    fn unlock_file(&self) -> turso_core::Result<()> {
        Ok(())
    }

    fn pread(&self, pos: u64, c: Completion) -> turso_core::Result<Completion> {
        let buf = c.as_read().buf();
        let dst = buf.as_mut_slice();
        let guard = self.data.read().expect("buffer file read lock");
        let start = usize::try_from(pos).unwrap_or(usize::MAX);
        let read_len = if start < guard.len() {
            dst.len().min(guard.len() - start)
        } else {
            0
        };
        if read_len > 0 {
            dst[..read_len].copy_from_slice(&guard[start..start + read_len]);
        }
        if read_len < dst.len() {
            dst[read_len..].fill(0);
        }
        drop(guard);
        c.complete(read_len as i32);
        Ok(c)
    }

    fn pwrite(
        &self,
        pos: u64,
        buffer: Arc<Buffer>,
        c: Completion,
    ) -> turso_core::Result<Completion> {
        let src = buffer.as_slice();
        let mut guard = self.data.write().expect("buffer file write lock");
        Self::write_locked(&mut guard, pos, src);
        drop(guard);
        c.complete(src.len() as i32);
        Ok(c)
    }

    fn sync(&self, c: Completion, _sync_type: FileSyncType) -> turso_core::Result<Completion> {
        c.complete(0);
        Ok(c)
    }

    fn pwritev(
        &self,
        pos: u64,
        buffers: Vec<Arc<Buffer>>,
        c: Completion,
    ) -> turso_core::Result<Completion> {
        let mut offset = pos;
        let mut total = 0usize;
        let mut guard = self.data.write().expect("buffer file write lock");
        for buf in &buffers {
            let src = buf.as_slice();
            Self::write_locked(&mut guard, offset, src);
            offset += src.len() as u64;
            total += src.len();
        }
        drop(guard);
        c.complete(total as i32);
        Ok(c)
    }

    fn size(&self) -> turso_core::Result<u64> {
        Ok(self.data.read().expect("buffer file read lock").len() as u64)
    }

    fn truncate(&self, len: u64, c: Completion) -> turso_core::Result<Completion> {
        let new_len = usize::try_from(len).unwrap_or(usize::MAX);
        self.data
            .write()
            .expect("buffer file write lock")
            .resize(new_len, 0);
        c.complete(0);
        Ok(c)
    }
}

struct BufferIO {
    files: Mutex<HashMap<String, Arc<BufferFile>>>,
}

impl BufferIO {
    fn empty() -> Self {
        Self {
            files: Mutex::new(HashMap::new()),
        }
    }

    fn with_main_bytes(path: &str, bytes: Vec<u8>) -> Self {
        let mut files = HashMap::new();
        files.insert(path.to_string(), Arc::new(BufferFile::from_bytes(bytes)));
        Self {
            files: Mutex::new(files),
        }
    }

    fn read_file_bytes(&self, path: &str) -> Option<Vec<u8>> {
        self.files
            .lock()
            .expect("buffer io lock")
            .get(path)
            .map(|f| f.bytes())
    }
}

impl Clock for BufferIO {
    #[cfg(not(target_arch = "wasm32"))]
    fn current_time_monotonic(&self) -> MonotonicInstant {
        DefaultClock.current_time_monotonic()
    }

    #[cfg(target_arch = "wasm32")]
    fn current_time_monotonic(&self) -> MonotonicInstant {
        let nanos = (js_sys::Date::now().max(0.0) * 1_000_000.0) as u128;
        MonotonicInstant::from_nanos(nanos)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn current_time_wall_clock(&self) -> WallClockInstant {
        DefaultClock.current_time_wall_clock()
    }

    #[cfg(target_arch = "wasm32")]
    fn current_time_wall_clock(&self) -> WallClockInstant {
        let ms = js_sys::Date::now().max(0.0);
        let secs = (ms / 1000.0).floor() as i64;
        let micros = ((ms % 1000.0) * 1000.0) as u32;
        WallClockInstant { secs, micros }
    }
}

impl IO for BufferIO {
    fn open_file(
        &self,
        path: &str,
        flags: OpenFlags,
        _direct: bool,
    ) -> turso_core::Result<Arc<dyn File>> {
        let mut files = self.files.lock().expect("buffer io lock");
        if let Some(existing) = files.get(path) {
            return Ok(existing.clone());
        }
        if !flags.contains(OpenFlags::Create) {
            return Err(CompletionError::IOError(std::io::ErrorKind::NotFound, "open").into());
        }
        let file = Arc::new(BufferFile::from_bytes(Vec::new()));
        files.insert(path.to_string(), file.clone());
        Ok(file)
    }

    fn remove_file(&self, path: &str) -> turso_core::Result<()> {
        self.files.lock().expect("buffer io lock").remove(path);
        Ok(())
    }

    fn file_id(&self, path: &str) -> turso_core::Result<FileId> {
        Ok(FileId::from_path_hash(path))
    }

    fn supports_shared_wal_coordination(&self) -> bool {
        false
    }
}

struct TursoEngine {
    io: Arc<BufferIO>,
    _db: Arc<Database>,
    conn: Arc<Connection>,
    db_path: String,
}

impl TursoEngine {
    fn open_with_bytes(bytes: Vec<u8>) -> Result<Self, DbError> {
        let id = NEXT_DB_ID.fetch_add(1, Ordering::Relaxed);
        let db_path = format!("/coda-demo-{id}.sqlite");
        let io = if bytes.is_empty() {
            Arc::new(BufferIO::empty())
        } else {
            Arc::new(BufferIO::with_main_bytes(&db_path, bytes))
        };
        let options = OpenOptions::new(Arc::new(SqliteDialect))
            .flags(OpenFlags::Create)
            .db_opts(DatabaseOpts::new().with_vacuum(true));
        let io_dyn: Arc<dyn IO> = io.clone();
        let db = Database::open(io_dyn, &db_path, options)?;
        let conn = db.connect()?;
        Ok(Self {
            io,
            _db: db,
            conn,
            db_path,
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn empty() -> Result<Self, DbError> {
        Self::open_with_bytes(Vec::new())
    }

    fn exec_raw(&self, sql: &str, params: &[SqlParam]) -> Result<(), DbError> {
        if params.is_empty() {
            self.conn.execute(sql)?;
            return Ok(());
        }
        let mut stmt = self.conn.prepare(sql)?;
        for (idx, param) in params.iter().enumerate() {
            let pos = NonZero::new(idx + 1).expect("1-based parameter index");
            stmt.bind_at(pos, param.to_turso_value())?;
        }
        stmt.run_ignore_rows()?;
        Ok(())
    }

    fn query_raw(&self, sql: &str, params: &[SqlParam]) -> Result<Vec<SqlRow>, DbError> {
        let mut stmt = self.conn.prepare(sql)?;
        for (idx, param) in params.iter().enumerate() {
            let pos = NonZero::new(idx + 1).expect("1-based parameter index");
            stmt.bind_at(pos, param.to_turso_value())?;
        }
        let col_names: Vec<String> = (0..stmt.num_columns())
            .map(|i| stmt.get_column_name(i).into_owned())
            .collect();
        let rows = stmt.run_collect_rows()?;
        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let mut map = BTreeMap::new();
            for (idx, val) in row.iter().enumerate() {
                if let Some(name) = col_names.get(idx) {
                    map.insert(name.clone(), turso_value_to_json(val));
                }
            }
            out.push(SqlRow(map));
        }
        Ok(out)
    }

    fn export_bytes(&self) -> Result<Vec<u8>, DbError> {
        self.conn.checkpoint(CheckpointMode::Truncate {
            upper_bound_inclusive: None,
        })?;
        Ok(self.io.read_file_bytes(&self.db_path).unwrap_or_default())
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::AtomicBool;

    fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), DbError> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).map_err(|source| DbError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let seq = NEXT_DB_ID.fetch_add(1, Ordering::Relaxed);
        let tmp = path.with_extension(format!("sqlite.tmp-{}-{seq}", std::process::id()));
        std::fs::write(&tmp, bytes).map_err(|source| DbError::Io {
            path: tmp.clone(),
            source,
        })?;
        std::fs::rename(&tmp, path).map_err(|source| DbError::Io {
            path: path.to_path_buf(),
            source,
        })
    }

    struct NativeInner {
        engine: Mutex<TursoEngine>,
        disk_path: Option<PathBuf>,
        reset_paths: Mutex<Option<(PathBuf, PathBuf)>>,
        dirty: AtomicBool,
    }

    impl Drop for NativeInner {
        fn drop(&mut self) {
            if !self.dirty.load(Ordering::Relaxed) {
                return;
            }
            let Some(path) = &self.disk_path else {
                return;
            };
            if let Ok(guard) = self.engine.lock()
                && let Ok(bytes) = guard.export_bytes()
            {
                let _ = write_atomic(path, &bytes);
            }
        }
    }

    #[derive(Clone)]
    pub struct SqliteBackend {
        inner: Arc<NativeInner>,
    }

    impl std::fmt::Debug for SqliteBackend {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("SqliteBackend").finish_non_exhaustive()
        }
    }

    impl SqliteBackend {
        pub fn open_in_memory() -> Result<Self, DbError> {
            Ok(Self {
                inner: Arc::new(NativeInner {
                    engine: Mutex::new(TursoEngine::empty()?),
                    disk_path: None,
                    reset_paths: Mutex::new(None),
                    dirty: AtomicBool::new(false),
                }),
            })
        }

        pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, DbError> {
            Ok(Self {
                inner: Arc::new(NativeInner {
                    engine: Mutex::new(TursoEngine::open_with_bytes(bytes)?),
                    disk_path: None,
                    reset_paths: Mutex::new(None),
                    dirty: AtomicBool::new(false),
                }),
            })
        }

        pub fn open_file(path: &Path) -> Result<Self, DbError> {
            let bytes = match std::fs::read(path) {
                Ok(b) => b,
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => Vec::new(),
                Err(source) => {
                    return Err(DbError::Io {
                        path: path.to_path_buf(),
                        source,
                    });
                }
            };
            Ok(Self {
                inner: Arc::new(NativeInner {
                    engine: Mutex::new(TursoEngine::open_with_bytes(bytes)?),
                    disk_path: Some(path.to_path_buf()),
                    reset_paths: Mutex::new(None),
                    dirty: AtomicBool::new(false),
                }),
            })
        }

        pub fn bind_reset_files(&self, seed_path: PathBuf, active_path: PathBuf) {
            *self.inner.reset_paths.lock().expect("reset lock") = Some((seed_path, active_path));
        }

        pub fn exec_raw(&self, sql: &str, params: &[SqlParam]) -> Result<(), DbError> {
            let guard = self.inner.engine.lock().expect("sqlite lock");
            guard.exec_raw(sql, params)?;
            if self.inner.disk_path.is_some() {
                let is_vacuum = sql
                    .trim_start()
                    .get(..6)
                    .is_some_and(|prefix| prefix.eq_ignore_ascii_case("vacuum"));
                if is_vacuum && let Some(path) = &self.inner.disk_path {
                    let bytes = guard.export_bytes()?;
                    write_atomic(path, &bytes)?;
                    self.inner.dirty.store(false, Ordering::Relaxed);
                    return Ok(());
                }
                self.inner.dirty.store(true, Ordering::Relaxed);
            }
            Ok(())
        }

        pub fn query_raw(&self, sql: &str, params: &[SqlParam]) -> Result<Vec<SqlRow>, DbError> {
            self.inner
                .engine
                .lock()
                .expect("sqlite lock")
                .query_raw(sql, params)
        }

        pub async fn reset(&self) -> Result<(), DbError> {
            let Some((seed, active)) = self.inner.reset_paths.lock().expect("reset lock").clone()
            else {
                return Ok(());
            };
            let bytes =
                std::fs::read(&seed).map_err(|source| DbError::Io { path: seed, source })?;
            let fresh = TursoEngine::open_with_bytes(bytes.clone())?;
            *self.inner.engine.lock().expect("sqlite lock") = fresh;
            write_atomic(&active, &bytes)?;
            self.inner.dirty.store(false, Ordering::Relaxed);
            Ok(())
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod imp {
    use super::*;
    use js_sys::Uint8Array;
    use std::cell::RefCell;
    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen(module = "/assets/sqlite_bridge.js")]
    extern "C" {
        #[wasm_bindgen(catch, js_name = "initDemoDb")]
        async fn js_init_demo_db(db_url: &str, export_bytes: &JsValue) -> Result<JsValue, JsValue>;
        #[wasm_bindgen(js_name = "schedulePersist")]
        fn js_schedule_persist();
        #[wasm_bindgen(catch, js_name = "resetDemoDb")]
        async fn js_reset_demo_db() -> Result<JsValue, JsValue>;
    }

    type ExporterClosure = Closure<dyn FnMut() -> Result<Uint8Array, JsValue>>;

    thread_local! {
        static ACTIVE_EXPORTER: RefCell<Option<ExporterClosure>> = const { RefCell::new(None) };
    }

    fn js_error(err: JsValue) -> DbError {
        DbError::Browser(err.as_string().unwrap_or_else(|| format!("{err:?}")))
    }

    #[derive(Clone)]
    pub struct SqliteBackend {
        db_url: String,
        engine: Arc<Mutex<Option<TursoEngine>>>,
    }

    impl std::fmt::Debug for SqliteBackend {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("SqliteBackend")
                .field("db_url", &self.db_url)
                .finish_non_exhaustive()
        }
    }

    impl SqliteBackend {
        pub fn new(db_url: String) -> Self {
            Self {
                db_url,
                engine: Arc::new(Mutex::new(None)),
            }
        }

        pub async fn ensure_ready(&self) -> Result<(), DbError> {
            if self.engine.lock().expect("sqlite lock").is_some() {
                return Ok(());
            }
            let engine_ref = self.engine.clone();
            let exporter = Closure::wrap(Box::new(move || -> Result<Uint8Array, JsValue> {
                let guard = engine_ref.lock().map_err(|err| {
                    JsValue::from_str(&format!("demo database lock poisoned: {err}"))
                })?;
                let engine = guard
                    .as_ref()
                    .ok_or_else(|| JsValue::from_str("demo database is not initialized"))?;
                let bytes = engine
                    .export_bytes()
                    .map_err(|err| JsValue::from_str(&err.to_string()))?;
                Ok(Uint8Array::from(bytes.as_slice()))
            })
                as Box<dyn FnMut() -> Result<Uint8Array, JsValue>>);
            let exporter_js = exporter.as_ref().clone();
            ACTIVE_EXPORTER.with(|slot| {
                *slot.borrow_mut() = Some(exporter);
            });
            let init_val = js_init_demo_db(&self.db_url, &exporter_js)
                .await
                .map_err(js_error)?;
            let bytes = Uint8Array::new(&init_val).to_vec();
            let engine = TursoEngine::open_with_bytes(bytes)?;
            let mut guard = self.engine.lock().expect("sqlite lock");
            if guard.is_none() {
                *guard = Some(engine);
            }
            Ok(())
        }

        pub fn exec_raw(&self, sql: &str, params: &[SqlParam]) -> Result<(), DbError> {
            {
                let guard = self.engine.lock().expect("sqlite lock");
                let engine = guard.as_ref().ok_or_else(|| {
                    DbError::Browser("demo database must be initialized before writes".into())
                })?;
                engine.exec_raw(sql, params)?;
            }
            js_schedule_persist();
            Ok(())
        }

        pub fn query_raw(&self, sql: &str, params: &[SqlParam]) -> Result<Vec<SqlRow>, DbError> {
            let guard = self.engine.lock().expect("sqlite lock");
            let engine = guard.as_ref().ok_or_else(|| {
                DbError::Browser("demo database must be initialized before reads".into())
            })?;
            engine.query_raw(sql, params)
        }

        pub async fn reset(&self) -> Result<(), DbError> {
            let seed_val = js_reset_demo_db().await.map_err(js_error)?;
            let seed_bytes = Uint8Array::new(&seed_val).to_vec();
            let fresh = TursoEngine::open_with_bytes(seed_bytes)?;
            *self.engine.lock().expect("sqlite lock") = Some(fresh);
            Ok(())
        }
    }
}

/// Platform-unified pure-Rust SQLite driver executing `sea-query` statements with bound parameters.
#[derive(Debug, Clone)]
pub struct SqliteDriver {
    backend: imp::SqliteBackend,
}

impl SqliteDriver {
    /// Opens an in-memory SQLite database (native targets and tests).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open_in_memory() -> Result<Self, DbError> {
        Ok(Self {
            backend: imp::SqliteBackend::open_in_memory()?,
        })
    }

    /// Opens an isolated in-memory SQLite database initialized from snapshot `bytes`.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, DbError> {
        Ok(Self {
            backend: imp::SqliteBackend::from_bytes(bytes)?,
        })
    }

    /// Opens or creates a SQLite file at `path` (native seeding binary and file tests).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open_file(path: &std::path::Path) -> Result<Self, DbError> {
        Ok(Self {
            backend: imp::SqliteBackend::open_file(path)?,
        })
    }

    /// Creates a browser SQLite driver fetching seed bytes from `db_url` and running `turso_core`.
    #[cfg(target_arch = "wasm32")]
    pub fn from_browser_url(db_url: impl Into<String>) -> Self {
        Self {
            backend: imp::SqliteBackend::new(db_url.into()),
        }
    }

    /// Ensures the browser SQLite runtime and seed database have loaded.
    pub async fn ensure_ready(&self) -> Result<(), DbError> {
        #[cfg(target_arch = "wasm32")]
        {
            self.backend.ensure_ready().await
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            Ok(())
        }
    }

    /// Executes a `sea-query` `SELECT` statement and returns owned rows.
    pub fn select(&self, query: &SelectStatement) -> Result<Vec<SqlRow>, DbError> {
        let (sql, values) = query.build(SqliteQueryBuilder);
        self.backend.query_raw(&sql, &bind_values(values))
    }

    /// Executes a `sea-query` `INSERT` statement.
    pub fn insert(&self, query: &InsertStatement) -> Result<(), DbError> {
        let (sql, values) = query.build(SqliteQueryBuilder);
        self.backend.exec_raw(&sql, &bind_values(values))
    }

    /// Inserts one row into `table` for `cols` and `vals`.
    pub fn insert_row<T: Iden + 'static, C: Iden + Copy + 'static>(
        &self,
        table: T,
        cols: &[C],
        vals: impl IntoIterator<Item = SimpleExpr>,
    ) -> Result<(), DbError> {
        let mut stmt = Query::insert();
        stmt.into_table(table)
            .columns(cols.iter().copied())
            .values_panic(vals);
        self.insert(&stmt)
    }

    /// Executes a `sea-query` `UPDATE` statement.
    pub fn update(&self, query: &UpdateStatement) -> Result<(), DbError> {
        let (sql, values) = query.build(SqliteQueryBuilder);
        self.backend.exec_raw(&sql, &bind_values(values))
    }

    /// Executes a `sea-query` `DELETE` statement.
    pub fn delete(&self, query: &DeleteStatement) -> Result<(), DbError> {
        let (sql, values) = query.build(SqliteQueryBuilder);
        self.backend.exec_raw(&sql, &bind_values(values))
    }

    /// Deletes rows from `table` matching all equality `conds`.
    pub fn delete_where<T: Iden + 'static, K: Iden + Copy + 'static>(
        &self,
        table: T,
        conds: &[(K, &str)],
    ) -> Result<(), DbError> {
        let mut q = Query::delete();
        q.from_table(table);
        for (k, v) in conds {
            q.and_where(Expr::col(*k).eq(*v));
        }
        self.delete(&q)
    }

    /// Selects `cols` from `table` matching all equality `conds`.
    pub fn select_where<T: Iden + 'static, C: Iden + Copy + 'static, K: Iden + Copy + 'static>(
        &self,
        table: T,
        cols: &[C],
        conds: &[(K, &str)],
    ) -> Result<Vec<SqlRow>, DbError> {
        let mut q = Query::select();
        q.columns(cols.iter().copied()).from(table);
        for (col, val) in conds {
            q.and_where(Expr::col(*col).eq(*val));
        }
        self.select(&q)
    }

    /// Updates `col = val` in `table` matching all equality `conds`.
    pub fn update_where<T: Iden + 'static, C: Iden + 'static, K: Iden + Copy + 'static>(
        &self,
        table: T,
        col: C,
        val: SimpleExpr,
        conds: &[(K, &str)],
    ) -> Result<(), DbError> {
        let mut q = Query::update();
        q.table(table).value(col, val);
        for (k, v) in conds {
            q.and_where(Expr::col(*k).eq(*v));
        }
        self.update(&q)
    }

    /// Executes DDL schema initialization statements.
    pub fn exec_ddl(&self, ddl: &[String]) -> Result<(), DbError> {
        for stmt in ddl {
            self.backend.exec_raw(stmt, &[])?;
        }
        Ok(())
    }

    /// Binds a seed file and active file path so native tests can reset cleanly.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn bind_reset_files(&self, seed_path: std::path::PathBuf, active_path: std::path::PathBuf) {
        self.backend.bind_reset_files(seed_path, active_path);
    }

    /// Resets the demo database back to its initial baseline snapshot.
    pub async fn reset(&self) -> Result<(), DbError> {
        self.backend.reset().await
    }
}
