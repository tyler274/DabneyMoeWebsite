//! Bridge configuration: which teams link, which repositories are trusted,
//! and with what authority. Secrets are named by environment variable so the
//! file itself carries none (Doppler injects the values).

use std::path::Path;
use std::time::Duration;

use serde::Deserialize;
use url::Url;

use crate::domain::{Authority, Forge, IssueKeyMatcher, RepoPath, TeamKey};
use crate::verify::Secret;

#[derive(Debug, Clone)]
pub struct RepoConfig {
    pub forge: Forge,
    pub path: RepoPath,
    pub authority: Authority,
    pub default_branch: String,
    /// Current and (during rotation) previous webhook secrets.
    pub secrets: Vec<Secret>,
}

#[derive(Debug, Clone)]
pub struct LinearConfig {
    pub endpoint: Url,
    pub api_key: Secret,
}

#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub base_delay: Duration,
    pub max_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        RetryPolicy {
            max_attempts: 8,
            base_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(600),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SyncConfig {
    pub teams: Vec<TeamKey>,
    pub repos: Vec<RepoConfig>,
    pub linear: LinearConfig,
    pub retry: RetryPolicy,
}

impl SyncConfig {
    pub fn matcher(&self) -> IssueKeyMatcher {
        IssueKeyMatcher::new(self.teams.iter().cloned())
    }

    pub fn repo(&self, forge: Forge, path: &RepoPath) -> Option<&RepoConfig> {
        self.repos
            .iter()
            .find(|r| r.forge == forge && &r.path == path)
    }

    /// `(repo, secrets)` pairs a webhook on `forge` may verify against.
    pub fn candidates(&self, forge: Forge) -> impl Iterator<Item = (&RepoPath, &[Secret])> {
        self.repos
            .iter()
            .filter(move |r| r.forge == forge)
            .map(|r| (&r.path, r.secrets.as_slice()))
    }

    /// Load the JSON file form, resolving secret environment variables.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
            path: path.display().to_string(),
            source,
        })?;
        let file: FileConfig = serde_json::from_str(&text).map_err(ConfigError::Parse)?;
        file.resolve(|name| std::env::var(name).ok())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("cannot read config {path}")]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid config JSON")]
    Parse(#[source] serde_json::Error),
    #[error("invalid team key")]
    Team(#[from] crate::domain::InvalidTeamKey),
    #[error("invalid repository path")]
    Repo(#[from] crate::domain::InvalidRepoPath),
    #[error("environment variable {0} is not set")]
    MissingEnv(String),
}

#[derive(Debug, Deserialize)]
struct FileRepo {
    forge: Forge,
    path: String,
    authority: Authority,
    default_branch: String,
    secret_envs: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct FileConfig {
    teams: Vec<String>,
    repos: Vec<FileRepo>,
    linear_endpoint: Url,
    linear_api_key_env: String,
    #[serde(default)]
    max_attempts: Option<u32>,
}

impl FileConfig {
    fn resolve(self, env: impl Fn(&str) -> Option<String>) -> Result<SyncConfig, ConfigError> {
        let secret = |name: &str| {
            env(name)
                .map(Secret::new)
                .ok_or_else(|| ConfigError::MissingEnv(name.to_owned()))
        };
        let teams = self
            .teams
            .iter()
            .map(|t| TeamKey::parse(t))
            .collect::<Result<_, _>>()?;
        let mut repos = Vec::new();
        for r in self.repos {
            repos.push(RepoConfig {
                forge: r.forge,
                path: RepoPath::parse(&r.path)?,
                authority: r.authority,
                default_branch: r.default_branch,
                secrets: r
                    .secret_envs
                    .iter()
                    .map(|n| secret(n))
                    .collect::<Result<_, _>>()?,
            });
        }
        let mut retry = RetryPolicy::default();
        if let Some(max) = self.max_attempts {
            retry.max_attempts = max;
        }
        Ok(SyncConfig {
            teams,
            repos,
            linear: LinearConfig {
                endpoint: self.linear_endpoint,
                api_key: secret(&self.linear_api_key_env)?,
            },
            retry,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_config_resolves_env_secrets() {
        let file: FileConfig = serde_json::from_str(
            r#"{"teams":["COD"],"repos":[{"forge":"forgejo","path":"tyler/site","authority":"canonical",
                "default_branch":"main","secret_envs":["HOOK"]}],
                "linear_endpoint":"https://api.linear.app/graphql","linear_api_key_env":"LINEAR"}"#,
        )
        .unwrap();
        let cfg = file
            .resolve(|n| matches!(n, "HOOK" | "LINEAR").then(|| "v".to_owned()))
            .unwrap();
        assert_eq!(cfg.repos[0].path.as_str(), "tyler/site");
        assert_eq!(cfg.retry.max_attempts, 8);
    }

    #[test]
    fn missing_env_is_an_error() {
        let file: FileConfig = serde_json::from_str(
            r#"{"teams":[],"repos":[],"linear_endpoint":"https://x/graphql","linear_api_key_env":"NOPE"}"#,
        )
        .unwrap();
        assert!(matches!(
            file.resolve(|_| None),
            Err(ConfigError::MissingEnv(_))
        ));
    }

    #[test]
    fn serv00_example_config_parses_and_resolves() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../selfhost/serv00/sync.json.example");
        let raw = std::fs::read_to_string(&path).expect("read Serv00 sync.json.example");
        let file: FileConfig = serde_json::from_str(&raw).expect("valid Serv00 sync.json.example");
        let cfg = file
            .resolve(|name| Some(format!("stub-{name}")))
            .expect("all example env vars resolve");
        assert_eq!(cfg.teams.len(), 2);
        assert!(cfg.repos.iter().any(|r| r.forge == Forge::Dabney));
        assert!(cfg.repos.iter().any(|r| r.forge == Forge::Forgejo));
        assert!(cfg
            .repos
            .iter()
            .filter(|r| matches!(r.forge, Forge::Gitea | Forge::GitLab))
            .all(|r| r.authority == Authority::External));
    }
}
