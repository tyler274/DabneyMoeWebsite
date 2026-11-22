//! Embeddable pure-Rust physician demo API server backed by `crates/harness`,
//! per-visitor `turso_core` SQLite snapshots, `physician-bus` SSE invalidations,
//! and dual-mode Cloud API + quantized pure-Rust `candle` GGUF inference.
#![deny(missing_docs)]

pub mod budget;
pub mod candle_qwen;
mod harness_product;
pub mod provider;
pub mod routes;
pub mod sessions;

use std::path::Path;
use std::sync::Arc;

use axum::Router;
use harness::skills::Package;
use physician_bus::Bus;
use physician_demo_db::{DemoStore, SqliteDriver};

use crate::budget::{DemoSessionConfig, DemoTurnBudget, DemoTurnBudgetConfig};
use crate::harness_product::HarnessRunner;
use crate::provider::ProviderBackend;
use crate::sessions::SessionRegistry;

const DEFAULT_DEMO_SEED_PATIENT_LIMIT: usize = 4;

/// Configuration for building an embeddable [`DemoApiState`] router.
#[derive(Clone)]
pub struct DemoApiConfig {
    /// Raw SQLite bytes cloned per visitor into isolated in-memory `turso_core` databases.
    pub seed_sqlite_bytes: Vec<u8>,
    /// Visitor session lifetime and capacity settings.
    pub session: DemoSessionConfig,
    /// Multi-layer rate-limiting settings for live demo turns.
    pub budget: DemoTurnBudgetConfig,
    /// Dual-mode LLM backend (`OPENAI_API_KEY` / `GEMINI_API_KEY` / `ANTHROPIC_API_KEY` / `CandleQwen`).
    pub provider_backend: ProviderBackend,
    /// Shared invalidation bus (`LocalBus` or `RedisBus`).
    pub bus: Arc<dyn Bus>,
    /// Optional agent skills loaded into `crates/harness` prompts and `skill_view`.
    pub skills: Arc<Package>,
    /// Whether to attach permissive CORS headers for cross-origin browser sessions.
    pub allow_cors: bool,
}

/// Shared application state held by the embeddable [`axum::Router`].
#[derive(Clone)]
pub struct DemoApiState {
    pub(crate) sessions: SessionRegistry,
    pub(crate) budget: Arc<DemoTurnBudget>,
    pub(crate) bus: Arc<dyn Bus>,
    pub(crate) provider_backend: ProviderBackend,
    pub(crate) harness: HarnessRunner,
    pub(crate) allow_cors: bool,
}

impl DemoApiState {
    /// Builds [`DemoApiState`] from explicit configuration.
    pub fn new(config: DemoApiConfig) -> Self {
        let sessions = SessionRegistry::new(
            config.seed_sqlite_bytes,
            config.session.session_seconds,
            Some(config.session.max_sessions),
        );
        let budget = Arc::new(DemoTurnBudget::new(config.budget));
        let harness = HarnessRunner {
            bus: Arc::clone(&config.bus),
            provider_backend: config.provider_backend.clone(),
            skills: Arc::clone(&config.skills),
        };
        Self {
            sessions,
            budget,
            bus: config.bus,
            provider_backend: config.provider_backend,
            harness,
            allow_cors: config.allow_cors,
        }
    }

    /// Builds [`DemoApiState`] by reading environment variables through `get`.
    pub async fn from_env_with(
        seed_sqlite_bytes: Vec<u8>,
        get: impl Fn(&str) -> Option<String>,
    ) -> Result<Self, String> {
        let session = DemoSessionConfig::from_env(&get)?;
        let budget = DemoTurnBudgetConfig::from_env(&get)?;
        let bus = bus_from_env(&get).await?;
        let provider_backend = ProviderBackend::from_env(&get);
        let skills = Arc::new(load_skills(&get));
        let allow_cors = get("CODA_DEMO_ALLOW_CORS")
            .as_deref()
            .is_none_or(|v| !matches!(v.trim(), "0" | "false" | "FALSE"));
        Ok(Self::new(DemoApiConfig {
            seed_sqlite_bytes,
            session,
            budget,
            provider_backend,
            bus,
            skills,
            allow_cors,
        }))
    }

    /// Returns the provider backend configured on this state.
    pub fn provider_backend(&self) -> &ProviderBackend {
        &self.provider_backend
    }

    /// Returns the invalidation bus configured on this state.
    pub fn bus(&self) -> &Arc<dyn Bus> {
        &self.bus
    }

    /// Exposes the configured session registry for diagnostics and testing.
    pub fn sessions(&self) -> &SessionRegistry {
        &self.sessions
    }

    /// Exposes the configured turn budget for diagnostics and testing.
    pub fn budget(&self) -> &Arc<DemoTurnBudget> {
        &self.budget
    }

    /// Builds the embeddable [`axum::Router`] wired to this state.
    pub fn into_router(self) -> Router {
        routes::build_router(self)
    }
}

/// Builds the embeddable [`axum::Router`] from [`DemoApiState`].
pub fn router(state: DemoApiState) -> Router {
    state.into_router()
}

/// Selects the invalidation bus from environment variables:
/// `PHYSICIAN_REDIS_URL`, `VALKEY_URL`, or `REDIS_URL` chooses `physician_bus::RedisBus`,
/// while leaving all unset returns `physician_bus::LocalBus`.
pub async fn bus_from_env(get: impl Fn(&str) -> Option<String>) -> Result<Arc<dyn Bus>, String> {
    let redis_url = ["PHYSICIAN_REDIS_URL", "VALKEY_URL", "REDIS_URL"]
        .into_iter()
        .find_map(|key| {
            get(key)
                .map(|v| v.trim().to_owned())
                .filter(|v| !v.is_empty())
        });
    let ca = get("PHYSICIAN_REDIS_CA_PEM")
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty());
    let hosted = get("PHYSICIAN_REDIS_HOSTED")
        .as_deref()
        .is_some_and(|v| matches!(v.trim(), "1" | "true" | "TRUE"));
    physician_bus::from_env(
        |name| match name {
            "PHYSICIAN_REDIS_URL" => redis_url.clone(),
            "PHYSICIAN_REDIS_CA_PEM" => ca.clone(),
            other => get(other),
        },
        hosted,
    )
    .await
}

/// Builds seed SQLite bytes directly from a `patients/units` fixture directory.
pub fn seed_sqlite_bytes_from_units(units_dir: &Path, limit: usize) -> Result<Vec<u8>, String> {
    let temp = std::env::temp_dir().join(format!(
        "coda-demo-seed-{}-{}.sqlite",
        std::process::id(),
        uuid::Uuid::new_v4().simple()
    ));
    let store = DemoStore::new(SqliteDriver::open_file(&temp).map_err(|e| e.to_string())?);
    physician_demo_db::seed::seed_from_units_dir(&store, units_dir, limit)
        .map_err(|e| e.to_string())?;
    drop(store);
    let bytes = std::fs::read(&temp)
        .map_err(|e| format!("failed to read generated seed db {}: {e}", temp.display()))?;
    let _ = std::fs::remove_file(&temp);
    Ok(bytes)
}

/// Resolves demo seed SQLite bytes from `CODA_DEMO_SEED_DB` or `CODA_DEMO_UNITS_DIR`.
pub fn seed_sqlite_bytes_from_env(get: impl Fn(&str) -> Option<String>) -> Result<Vec<u8>, String> {
    if let Some(path) = get("CODA_DEMO_SEED_DB")
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
    {
        return std::fs::read(&path)
            .map_err(|e| format!("failed to read CODA_DEMO_SEED_DB={path}: {e}"));
    }
    if let Some(units) = get("CODA_DEMO_UNITS_DIR")
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
    {
        return seed_sqlite_bytes_from_units(Path::new(&units), DEFAULT_DEMO_SEED_PATIENT_LIMIT);
    }
    let workspace_units = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../patients/units");
    if workspace_units.is_dir() {
        return seed_sqlite_bytes_from_units(&workspace_units, DEFAULT_DEMO_SEED_PATIENT_LIMIT);
    }
    Err("missing CODA_DEMO_SEED_DB or CODA_DEMO_UNITS_DIR".into())
}

fn load_skills(get: &impl Fn(&str) -> Option<String>) -> Package {
    let dir = get("CODA_DEMO_SKILLS_DIR")
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty());
    if let Some(path) = dir
        && let Ok(pkg) = Package::load("physician", Path::new(&path))
    {
        return pkg;
    }
    let workspace_skills = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills");
    if workspace_skills.is_dir()
        && let Ok(pkg) = Package::load("physician", &workspace_skills)
    {
        return pkg;
    }
    Package::empty("physician")
}
