//! `forge-linear-sync`: serve the webhook route, backfill from a forge API,
//! replay the dead-letter queue, or check a webhook signature offline.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;

use bytes::Bytes;
use clap::{Parser, Subcommand};
use forge_linear_sync::config::SyncConfig;
use forge_linear_sync::domain::{Forge, RepoPath};
use forge_linear_sync::linear::LinearClient;
use forge_linear_sync::queue::EventStore;
use forge_linear_sync::verify::{auth_header, verify, Secret};
use forge_linear_sync::{sync, Bridge};

#[derive(Parser)]
#[command(about = "Forge pull-request webhooks to Linear")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Serve `POST /hooks/forge/{forge}` and run the queue worker.
    Serve {
        #[arg(long)]
        config: PathBuf,
        #[arg(long, default_value = "forge-linear-sync.sqlite")]
        db: PathBuf,
        #[arg(long, default_value = "127.0.0.1:8090")]
        listen: SocketAddr,
    },
    /// Reconcile every PR of a configured Forgejo-compatible repository
    /// (Codeberg, Gitea, git.dabney.moe) against Linear. Idempotent.
    Backfill {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        forge: Forge,
        /// `owner/name`, as configured.
        #[arg(long)]
        repo: String,
        /// API base, e.g. `https://codeberg.org/api/v1`.
        #[arg(long)]
        api_base: url::Url,
        /// Environment variable holding a read token for the forge API.
        #[arg(long)]
        token_env: Option<String>,
    },
    /// Move dead-lettered events back to the queue, then process what is due.
    ReplayDlq {
        #[arg(long)]
        config: PathBuf,
        #[arg(long, default_value = "forge-linear-sync.sqlite")]
        db: PathBuf,
    },
    /// Check a stored webhook body against a signature or token header.
    VerifyWebhook {
        #[arg(long)]
        forge: Forge,
        #[arg(long)]
        body_file: PathBuf,
        /// The signature (hex) or GitLab token as received.
        #[arg(long)]
        signature: String,
        /// Environment variable holding the webhook secret.
        #[arg(long, default_value = "FORGE_WEBHOOK_SECRET")]
        secret_env: String,
    },
}

#[derive(Debug, thiserror::Error)]
enum CliError {
    #[error(transparent)]
    Config(#[from] forge_linear_sync::config::ConfigError),
    #[error(transparent)]
    Store(#[from] forge_linear_sync::queue::StoreError),
    #[error("I/O error")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Backfill(#[from] forge_linear_sync::sync::BackfillError),
    #[error("repository {0} is not configured for that forge")]
    UnknownRepo(String),
    #[error("invalid repository path")]
    RepoPath(#[from] forge_linear_sync::domain::InvalidRepoPath),
    #[error("environment variable {0} is not set")]
    MissingEnv(String),
    #[error("invalid header value")]
    Header(#[from] http::header::InvalidHeaderValue),
    #[error("signature check failed")]
    Verify(#[from] forge_linear_sync::verify::VerifyError),
}

fn report(error: &dyn std::error::Error) {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(s) = source {
        text.push_str(": ");
        text.push_str(&s.to_string());
        source = s.source();
    }
    eprintln!("error: {text}");
}

async fn run(cli: Cli) -> Result<(), CliError> {
    match cli.command {
        Command::Serve { config, db, listen } => {
            let bridge = Bridge::new(SyncConfig::load(&config)?, EventStore::open(&db)?);
            tokio::spawn(bridge.worker.run());
            let listener = tokio::net::TcpListener::bind(listen).await?;
            tracing::info!(%listen, "forge-linear-sync listening");
            axum::serve(listener, bridge.router).await?;
            Ok(())
        }
        Command::Backfill {
            config,
            forge,
            repo,
            api_base,
            token_env,
        } => {
            let config = SyncConfig::load(&config)?;
            let path = RepoPath::parse(&repo)?;
            let repo_config = config
                .repo(forge, &path)
                .ok_or_else(|| CliError::UnknownRepo(repo.clone()))?;
            let client = LinearClient::new(config.linear.clone());
            let matcher = config.matcher();
            let token = match token_env {
                Some(name) => Some(std::env::var(&name).map_err(|_| CliError::MissingEnv(name))?),
                None => None,
            };
            let report = sync::backfill_forgejo(
                &client,
                forge,
                repo_config,
                &matcher,
                &api_base,
                token.as_deref(),
                50,
            )
            .await?;
            for (number, outcome) in report.outcomes {
                println!("#{number} {outcome:?}");
            }
            Ok(())
        }
        Command::ReplayDlq { config, db } => {
            let bridge = Bridge::new(SyncConfig::load(&config)?, EventStore::open(&db)?);
            let replayed = bridge.worker.store.replay_dead()?;
            println!("requeued {replayed} dead-lettered event(s)");
            for processed in bridge.worker.drain_due().await? {
                println!("{processed:?}");
            }
            Ok(())
        }
        Command::VerifyWebhook {
            forge,
            body_file,
            signature,
            secret_env,
        } => {
            let secret =
                std::env::var(&secret_env).map_err(|_| CliError::MissingEnv(secret_env))?;
            let body = Bytes::from(std::fs::read(&body_file)?);
            let mut headers = http::HeaderMap::new();
            headers.insert(auth_header(forge), signature.parse()?);
            let repo = RepoPath::parse("offline/check")?;
            let secrets = [Secret::new(secret)];
            verify(forge, &headers, body, [(&repo, &secrets[..])])?;
            println!("signature valid");
            Ok(())
        }
    }
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
            report(&e);
            ExitCode::FAILURE
        }
    }
}
