//! Forge payload formats and their translation into [`PrSnapshot`].
//!
//! The Forgejo types are also what git.dabney.moe serializes when it emits
//! webhooks, so the emitter and this parser share one definition.

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::config::{RepoConfig, SyncConfig};
use crate::domain::Forge;
use crate::domain::{
    CanonicalUrl, CommitSha, IssueKeyMatcher, PrKey, PrSnapshot, PrState, RepoPath, RepoRef,
};
use crate::verify::VerifiedPayload;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgejoBranch {
    #[serde(rename = "ref")]
    pub branch: String,
    pub sha: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgejoPullRequest {
    pub number: u64,
    pub html_url: String,
    pub title: String,
    #[serde(default)]
    pub body: Option<String>,
    /// `open` or `closed`.
    pub state: String,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub merged: bool,
    #[serde(default)]
    pub merge_commit_sha: Option<String>,
    pub updated_at: String,
    pub head: ForgejoBranch,
    pub base: ForgejoBranch,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgejoRepository {
    pub full_name: String,
    #[serde(default)]
    pub default_branch: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgejoPullRequestEvent {
    pub action: String,
    pub number: u64,
    pub pull_request: ForgejoPullRequest,
    pub repository: ForgejoRepository,
}

#[derive(Debug, Deserialize)]
struct GitLabCommit {
    id: String,
}

#[derive(Debug, Deserialize)]
struct GitLabAttributes {
    iid: u64,
    url: String,
    title: String,
    #[serde(default)]
    description: Option<String>,
    state: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    merge_commit_sha: Option<String>,
    updated_at: String,
    source_branch: String,
    target_branch: String,
    last_commit: GitLabCommit,
}

#[derive(Debug, Deserialize)]
struct GitLabProject {
    path_with_namespace: String,
}

#[derive(Debug, Deserialize)]
struct GitLabMergeRequestEvent {
    object_attributes: GitLabAttributes,
    project: GitLabProject,
}

#[derive(Debug, thiserror::Error)]
pub enum PayloadError {
    #[error("payload is not the expected JSON")]
    Json(#[from] serde_json::Error),
    #[error("payload repository {payload} does not match the verifying repository {verified}")]
    RepoMismatch { payload: String, verified: RepoPath },
    #[error("repository {0} is not configured")]
    UnknownRepo(RepoPath),
    #[error("invalid repository path")]
    RepoPath(#[from] crate::domain::InvalidRepoPath),
    #[error("invalid pull request URL")]
    Url(#[from] crate::domain::InvalidUrl),
    #[error("invalid commit sha")]
    Sha(#[from] crate::domain::InvalidSha),
    #[error("merged pull request has no merge commit sha")]
    MissingMergeSha,
    #[error("unknown pull request state {0:?}")]
    State(String),
    #[error("unparseable timestamp {0:?}")]
    Timestamp(String),
}

/// RFC 3339 (Forgejo) or `2013-12-03 17:23:34 UTC` (GitLab).
fn parse_timestamp(raw: &str) -> Result<Timestamp, PayloadError> {
    if let Ok(ts) = raw.parse::<Timestamp>() {
        return Ok(ts);
    }
    let trimmed = raw.strip_suffix(" UTC").unwrap_or(raw);
    jiff::civil::DateTime::strptime("%Y-%m-%d %H:%M:%S", trimmed)
        .ok()
        .and_then(|dt| dt.to_zoned(jiff::tz::TimeZone::UTC).ok())
        .map(|z| z.timestamp())
        .ok_or_else(|| PayloadError::Timestamp(raw.to_owned()))
}

fn snapshot_from_forgejo(
    forge: Forge,
    pr: &ForgejoPullRequest,
    repo: &RepoConfig,
    matcher: &IssueKeyMatcher,
) -> Result<PrSnapshot, PayloadError> {
    let state = match (pr.state.as_str(), pr.merged, pr.draft) {
        (_, true, _) => PrState::Merged {
            sha: CommitSha::parse(
                pr.merge_commit_sha
                    .as_deref()
                    .ok_or(PayloadError::MissingMergeSha)?,
            )?,
        },
        ("closed", false, _) => PrState::Closed,
        ("open", false, true) => PrState::Draft,
        ("open", false, false) => PrState::Open,
        (other, _, _) => return Err(PayloadError::State(other.to_owned())),
    };
    let text = format!(
        "{}\n{}\n{}",
        pr.title,
        pr.head.branch,
        pr.body.as_deref().unwrap_or("")
    );
    Ok(PrSnapshot {
        key: PrKey {
            repo: RepoRef {
                forge,
                path: repo.path.clone(),
            },
            number: pr.number,
        },
        url: CanonicalUrl::parse(&pr.html_url)?,
        title: pr.title.clone(),
        head: CommitSha::parse(&pr.head.sha)?,
        state,
        target_is_default_branch: pr.base.branch == repo.default_branch,
        authority: repo.authority,
        forge_updated_at: parse_timestamp(&pr.updated_at)?,
        issues: matcher.find_in(&text),
    })
}

/// A pull request as returned by a Forgejo-compatible `GET .../pulls` API,
/// for backfill.
pub fn snapshot_from_forgejo_api(
    forge: Forge,
    pr_json: &serde_json::Value,
    repo: &RepoConfig,
    matcher: &IssueKeyMatcher,
) -> Result<PrSnapshot, PayloadError> {
    let pr: ForgejoPullRequest = serde_json::from_value(pr_json.clone())?;
    snapshot_from_forgejo(forge, &pr, repo, matcher)
}

fn check_repo(payload: &str, verified: &RepoPath) -> Result<(), PayloadError> {
    if &RepoPath::parse(payload)? == verified {
        Ok(())
    } else {
        Err(PayloadError::RepoMismatch {
            payload: payload.to_owned(),
            verified: verified.clone(),
        })
    }
}

/// Parse a verified pull-request event into a snapshot. The payload's
/// repository must be the one whose secret verified it.
pub fn parse_pull_request(
    payload: &VerifiedPayload<bytes::Bytes>,
    config: &SyncConfig,
) -> Result<PrSnapshot, PayloadError> {
    let forge = payload.forge();
    let verified = payload.repo();
    let repo = config
        .repo(forge, verified)
        .ok_or_else(|| PayloadError::UnknownRepo(verified.clone()))?;
    let matcher = config.matcher();
    match forge {
        Forge::Forgejo | Forge::Gitea | Forge::Dabney => {
            let event: ForgejoPullRequestEvent = serde_json::from_slice(payload.body())?;
            check_repo(&event.repository.full_name, verified)?;
            snapshot_from_forgejo(forge, &event.pull_request, repo, &matcher)
        }
        Forge::GitLab => {
            let event: GitLabMergeRequestEvent = serde_json::from_slice(payload.body())?;
            check_repo(&event.project.path_with_namespace, verified)?;
            let a = &event.object_attributes;
            let state = match a.state.as_str() {
                "merged" => PrState::Merged {
                    sha: CommitSha::parse(
                        a.merge_commit_sha
                            .as_deref()
                            .ok_or(PayloadError::MissingMergeSha)?,
                    )?,
                },
                "closed" | "locked" => PrState::Closed,
                "opened" if a.draft => PrState::Draft,
                "opened" => PrState::Open,
                other => return Err(PayloadError::State(other.to_owned())),
            };
            let text = format!(
                "{}\n{}\n{}",
                a.title,
                a.source_branch,
                a.description.as_deref().unwrap_or("")
            );
            Ok(PrSnapshot {
                key: PrKey {
                    repo: RepoRef {
                        forge,
                        path: repo.path.clone(),
                    },
                    number: a.iid,
                },
                url: CanonicalUrl::parse(&a.url)?,
                title: a.title.clone(),
                head: CommitSha::parse(&a.last_commit.id)?,
                state,
                target_is_default_branch: a.target_branch == repo.default_branch,
                authority: repo.authority,
                forge_updated_at: parse_timestamp(&a.updated_at)?,
                issues: matcher.find_in(&text),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_both_timestamp_forms() {
        assert_eq!(
            parse_timestamp("2026-11-25T10:00:00Z").unwrap(),
            parse_timestamp("2026-11-25 10:00:00 UTC").unwrap()
        );
        assert!(parse_timestamp("yesterday").is_err());
    }
}
