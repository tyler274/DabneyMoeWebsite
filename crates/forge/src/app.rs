//! The forge service: its stores, configuration, background workers and
//! the Axum router every surface is mounted on.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use axum::extract::DefaultBodyLimit;
use axum::routing::{get, post};
use axum::Router;
use forge_linear_sync::verify::Secret;
use tokio::sync::Notify;
use url::Url;

use crate::auth::{self, Principal};
use crate::db::{Db, RepoRecord};
use crate::error::ForgeError;
use crate::repo::{RepoName, RepoStore};

/// Where a signed pull-request webhook is delivered.
#[derive(Debug, Clone)]
pub struct WebhookTarget {
    pub url: Url,
    pub secret: Secret,
}

/// Resolves a secret named by environment variable (mirror push tokens).
pub type SecretSource = Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;

pub fn env_secrets() -> SecretSource {
    Arc::new(|name| std::env::var(name).ok())
}

#[derive(Clone)]
pub struct ForgeConfig {
    /// Public base URL, e.g. `https://git.dabney.moe`; links in webhooks and
    /// the UI are built from it.
    pub base_url: Url,
    pub webhooks: Vec<WebhookTarget>,
    /// Largest Git request body accepted (pushes carry whole packs).
    pub max_git_body: usize,
    pub secrets: SecretSource,
}

impl ForgeConfig {
    pub fn new(base_url: Url) -> Self {
        ForgeConfig {
            base_url,
            webhooks: Vec::new(),
            max_git_body: 1024 * 1024 * 1024,
            secrets: env_secrets(),
        }
    }

    /// `{base}/{path}` without doubling slashes.
    pub fn link(&self, path: &str) -> String {
        format!(
            "{}/{}",
            self.base_url.as_str().trim_end_matches('/'),
            path.trim_start_matches('/')
        )
    }
}

/// Everything a request handler or worker needs.
pub struct Forge {
    pub db: Db,
    pub repos: RepoStore,
    pub config: ForgeConfig,
    /// Wakes the webhook deliverer.
    pub hooks_ready: Notify,
    /// Wakes the mirror worker.
    pub mirrors_ready: Notify,
    pub http: reqwest::Client,
    /// Serializes ref-moving work (pushes, merges) per repository.
    ref_locks: Mutex<HashMap<i64, Arc<Mutex<()>>>>,
}

pub type App = Arc<Forge>;

impl Forge {
    pub fn new(db: Db, repos: RepoStore, config: ForgeConfig) -> App {
        Arc::new(Forge {
            db,
            repos,
            config,
            hooks_ready: Notify::new(),
            mirrors_ready: Notify::new(),
            ref_locks: Mutex::new(HashMap::new()),
            http: reqwest::Client::builder()
                .user_agent(concat!("dabney-forge/", env!("CARGO_PKG_VERSION")))
                .timeout(std::time::Duration::from_secs(60))
                .build()
                .expect("static reqwest configuration is valid"),
        })
    }

    /// Open the on-disk layout: `forge.db` and `repos/` under `data_dir`.
    pub fn open(data_dir: &Path, config: ForgeConfig) -> Result<App, ForgeError> {
        std::fs::create_dir_all(data_dir.join("repos")).map_err(ForgeError::Io)?;
        let db = Db::open(&data_dir.join("forge.db"))?;
        Ok(Self::new(
            db,
            RepoStore::new(data_dir.join("repos")),
            config,
        ))
    }

    /// Register a repository and create its bare Git directory.
    pub fn create_repo(
        &self,
        name: &RepoName,
        private: bool,
        default_branch: &str,
    ) -> Result<RepoRecord, ForgeError> {
        gix::refs::FullName::try_from(format!("refs/heads/{default_branch}"))
            .map_err(|_| ForgeError::InvalidName(default_branch.to_owned()))?;
        if self.db.repo(name)?.is_some() {
            return Err(ForgeError::Conflict(format!(
                "repository {name} already exists"
            )));
        }
        self.repos.create(name, default_branch)?;
        let id = self.db.insert_repo(name, private, default_branch)?;
        self.db.repo_by_id(id)
    }

    /// The lock every push and merge on `repo_id` holds while it moves refs.
    pub fn repo_lock(&self, repo_id: i64) -> Arc<Mutex<()>> {
        let mut locks = self.ref_locks.lock().unwrap_or_else(|p| p.into_inner());
        locks.entry(repo_id).or_default().clone()
    }

    /// The repository record, or 404.
    pub fn repo_record(&self, name: &RepoName) -> Result<RepoRecord, ForgeError> {
        self.db
            .repo(name)?
            .ok_or_else(|| ForgeError::NotFound(format!("repository {name}")))
    }

    /// Look up a repository and require `action` on it. Private
    /// repositories an anonymous caller may not read are 401, so Git
    /// clients prompt for credentials.
    pub fn authorize(
        &self,
        owner: &str,
        repo: &str,
        principal: Option<&Principal>,
        action: auth::Action,
    ) -> Result<RepoRecord, ForgeError> {
        let name = RepoName::new(owner, repo)?;
        let record = self.repo_record(&name)?;
        if action != auth::Action::Read
            && !auth::can(&self.db, principal, &record, auth::Action::Read)?
        {
            // Do not reveal private repositories to callers who cannot read them.
            return match principal {
                None => Err(ForgeError::Unauthenticated),
                Some(_) => Err(ForgeError::NotFound(format!("repository {name}"))),
            };
        }
        auth::require(&self.db, principal, &record, action)?;
        Ok(record)
    }
}

/// Run blocking forge work (SQLite, gix) off the async executor.
pub async fn blocking<T, F>(app: &App, work: F) -> Result<T, ForgeError>
where
    T: Send + 'static,
    F: FnOnce(&Forge) -> Result<T, ForgeError> + Send + 'static,
{
    let app = app.clone();
    tokio::task::spawn_blocking(move || work(&app)).await?
}

/// Every route: Smart HTTP, the JSON API, the UI and health.
pub fn router(app: App) -> Router {
    let git = Router::new()
        .route(
            "/{owner}/{repo}/info/refs",
            get(crate::git::http::info_refs),
        )
        .route(
            "/{owner}/{repo}/git-upload-pack",
            post(crate::git::http::upload_pack),
        )
        .route(
            "/{owner}/{repo}/git-receive-pack",
            post(crate::git::http::receive_pack),
        )
        .layer(DefaultBodyLimit::max(app.config.max_git_body));
    Router::new()
        .merge(git)
        .merge(crate::api::routes())
        .merge(crate::ui::routes())
        .route("/healthz", get(|| async { "ok" }))
        .with_state(app)
}

/// Start the webhook deliverer and mirror worker.
pub fn spawn_workers(app: &App) -> Result<(), ForgeError> {
    app.db.requeue_running_mirror_jobs()?;
    tokio::spawn(crate::webhooks::run_deliverer(app.clone()));
    tokio::spawn(crate::mirror::run_worker(app.clone()));
    Ok(())
}
