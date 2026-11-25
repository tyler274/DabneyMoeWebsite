//! `dabney-forge`: serve git.dabney.moe, or administer its repositories,
//! tokens, collaborators, merge policy and mirrors.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use forge::app::{env_secrets, Forge, ForgeConfig, WebhookTarget};
use forge::auth;
use forge::error::ForgeError;
use forge::repo::RepoName;
use forge_linear_sync::verify::Secret;

#[derive(Parser)]
#[command(about = "git.dabney.moe: a pure-Rust Git forge and pull-request coordinator")]
struct Cli {
    /// Directory holding `forge.db` and `repos/`.
    #[arg(
        long,
        env = "DABNEY_FORGE_DATA",
        default_value = "forge-data",
        global = true
    )]
    data_dir: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Serve Git Smart HTTP, the API and the UI, and run the webhook and
    /// mirror workers.
    Serve {
        #[arg(long, env = "DABNEY_FORGE_LISTEN", default_value = "127.0.0.1:8080")]
        listen: SocketAddr,
        /// Public base URL used in links and webhook payloads.
        #[arg(
            long,
            env = "DABNEY_FORGE_BASE_URL",
            default_value = "http://127.0.0.1:8080"
        )]
        base_url: url::Url,
        /// Pull-request webhook targets (repeatable), signed with the secret
        /// in `--webhook-secret-env`.
        #[arg(long = "webhook-url")]
        webhook_urls: Vec<url::Url>,
        #[arg(long, default_value = "DABNEY_FORGE_WEBHOOK_SECRET")]
        webhook_secret_env: String,
        /// Also serve `POST /hooks/forge/{forge}` from this forge-linear-sync
        /// configuration, with its queue in `--sync-db`.
        #[arg(long)]
        sync_config: Option<PathBuf>,
        #[arg(long, default_value = "forge-linear-sync.sqlite")]
        sync_db: PathBuf,
    },
    /// Create a repository.
    CreateRepo {
        /// `owner/name`.
        name: String,
        #[arg(long)]
        private: bool,
        #[arg(long, default_value = "main")]
        default_branch: String,
    },
    /// Issue a personal access token and print it once.
    IssueToken {
        user: String,
        /// Comma-separated: read, write, status, admin.
        #[arg(long, default_value = "read,write")]
        scopes: String,
        #[arg(long, default_value = "cli")]
        name: String,
    },
    /// Let `user` push, review and merge in a repository.
    AddCollaborator { repo: String, user: String },
    /// Set the merge policy of a repository.
    SetPolicy {
        repo: String,
        #[arg(long, default_value_t = 1)]
        approvals: u32,
        /// Required status contexts (repeatable).
        #[arg(long = "check")]
        checks: Vec<String>,
    },
    /// Mirror a repository's default branch and tags to another Git host.
    AddMirror {
        repo: String,
        name: String,
        url: String,
        #[arg(long)]
        username: Option<String>,
        /// Environment variable holding the push token.
        #[arg(long)]
        token_env: Option<String>,
    },
}

#[derive(Debug, thiserror::Error)]
enum CliError {
    #[error(transparent)]
    Forge(#[from] ForgeError),
    #[error(transparent)]
    SyncConfig(#[from] forge_linear_sync::config::ConfigError),
    #[error(transparent)]
    SyncStore(#[from] forge_linear_sync::queue::StoreError),
    #[error("environment variable {0} is not set")]
    MissingEnv(String),
    #[error("cannot listen on {addr}")]
    Listen {
        addr: SocketAddr,
        #[source]
        source: std::io::Error,
    },
    #[error("server failed")]
    Serve(#[source] std::io::Error),
}

fn chain(e: &dyn std::error::Error) -> String {
    let mut text = e.to_string();
    let mut source = e.source();
    while let Some(s) = source {
        text.push_str(": ");
        text.push_str(&s.to_string());
        source = s.source();
    }
    text
}

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    match run(Cli::parse()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {}", chain(&e));
            ExitCode::FAILURE
        }
    }
}

async fn run(cli: Cli) -> Result<(), CliError> {
    let config = |base_url: url::Url| ForgeConfig::new(base_url);
    let admin = || {
        Forge::open(
            &cli.data_dir,
            config("http://127.0.0.1:8080".parse().expect("static URL")),
        )
    };
    match cli.command {
        Command::Serve {
            listen,
            base_url,
            webhook_urls,
            webhook_secret_env,
            sync_config,
            sync_db,
        } => {
            let mut forge_config = config(base_url);
            forge_config.secrets = env_secrets();
            if !webhook_urls.is_empty() {
                let secret = std::env::var(&webhook_secret_env)
                    .map_err(|_| CliError::MissingEnv(webhook_secret_env.clone()))?;
                forge_config.webhooks = webhook_urls
                    .into_iter()
                    .map(|url| WebhookTarget {
                        url,
                        secret: Secret::new(secret.clone()),
                    })
                    .collect();
            }
            let app = Forge::open(&cli.data_dir, forge_config)?;
            forge::spawn_workers(&app)?;
            let mut router = forge::router(app);
            if let Some(path) = sync_config {
                let bridge = forge_linear_sync::Bridge::new(
                    forge_linear_sync::config::SyncConfig::load(&path)?,
                    forge_linear_sync::queue::EventStore::open(&sync_db)?,
                );
                tokio::spawn(bridge.worker.run());
                router = router.merge(bridge.router);
            }
            let listener = tokio::net::TcpListener::bind(listen)
                .await
                .map_err(|source| CliError::Listen {
                    addr: listen,
                    source,
                })?;
            tracing::info!(%listen, "dabney-forge listening");
            axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = tokio::signal::ctrl_c().await;
                })
                .await
                .map_err(CliError::Serve)
        }
        Command::CreateRepo {
            name,
            private,
            default_branch,
        } => {
            let record =
                admin()?.create_repo(&RepoName::parse(&name)?, private, &default_branch)?;
            println!("created {}", record.name);
            Ok(())
        }
        Command::IssueToken { user, scopes, name } => {
            let app = admin()?;
            let token = auth::issue_token(&app.db, &user, &name, &auth::parse_scopes(&scopes)?)?;
            println!("{token}");
            Ok(())
        }
        Command::AddCollaborator { repo, user } => {
            let app = admin()?;
            let record = app.repo_record(&RepoName::parse(&repo)?)?;
            app.db
                .add_collaborator(record.id, forge::repo::Slug::parse(&user)?.as_str())?;
            Ok(())
        }
        Command::SetPolicy {
            repo,
            approvals,
            checks,
        } => {
            let app = admin()?;
            let record = app.repo_record(&RepoName::parse(&repo)?)?;
            app.db.set_repo_policy(record.id, approvals, &checks)?;
            Ok(())
        }
        Command::AddMirror {
            repo,
            name,
            url,
            username,
            token_env,
        } => {
            let app = admin()?;
            let record = app.repo_record(&RepoName::parse(&repo)?)?;
            app.db.add_mirror(
                record.id,
                &name,
                &url,
                username.as_deref(),
                token_env.as_deref(),
            )?;
            Ok(())
        }
    }
}
