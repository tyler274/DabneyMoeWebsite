//! The upsert planner: one verified snapshot in, idempotent Linear writes out.

use std::collections::BTreeSet;

use jiff::Timestamp;
use url::Url;

use crate::config::RepoConfig;
use crate::domain::{Authority, Forge, IssueKey, IssueKeyMatcher, PrSnapshot, PrState};
use crate::event::snapshot_from_forgejo_api;
use crate::linear::{
    CardMetadata, LinearClient, LinearError, LinearIssue, NewAttachment, BRIDGE_ID,
};

/// Label added to an issue whose card comes from a non-canonical mirror.
pub const EXTERNAL_LABEL: &str = "pr-external";
/// Label added to an issue whose sync was dead-lettered.
pub const SYNC_ERROR_LABEL: &str = "pr-sync-error";

#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error(transparent)]
    Linear(#[from] LinearError),
    #[error("Linear has no issue {0}")]
    UnknownIssue(IssueKey),
    #[error("team {0} has no completed workflow state")]
    NoDoneState(String),
}

impl SyncError {
    pub fn is_retryable(&self) -> bool {
        match self {
            SyncError::Linear(e) => e.is_retryable(),
            SyncError::UnknownIssue(_) | SyncError::NoDoneState(_) => false,
        }
    }

    pub fn retry_after(&self) -> Option<std::time::Duration> {
        match self {
            SyncError::Linear(e) => e.retry_after(),
            _ => None,
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SyncOutcome {
    /// The event was older than what Linear already shows; nothing written.
    pub stale: bool,
    pub linked: Vec<String>,
    pub unlinked: Vec<String>,
    pub completed: Vec<String>,
}

fn metadata(snapshot: &PrSnapshot) -> CardMetadata {
    CardMetadata {
        bridge: BRIDGE_ID.to_owned(),
        forge: snapshot.key.repo.forge.as_str().to_owned(),
        repo: snapshot.key.repo.path.as_str().to_owned(),
        number: snapshot.key.number,
        state: snapshot.state.label().to_owned(),
        head: snapshot.head.as_str().to_owned(),
        forge_updated_at: snapshot.forge_updated_at.to_string(),
        authority: snapshot.authority.as_str().to_owned(),
        target_is_default_branch: snapshot.target_is_default_branch,
    }
}

fn subtitle(snapshot: &PrSnapshot) -> String {
    let state = match &snapshot.state {
        PrState::Draft => "Draft".to_owned(),
        PrState::Open => "Open".to_owned(),
        PrState::Merged { sha } => format!("Merged {}", &sha.as_str()[..12]),
        PrState::Closed => "Closed".to_owned(),
    };
    match snapshot.authority {
        Authority::Canonical => format!("{state} · {}", snapshot.key.repo.forge),
        Authority::External => format!(
            "{state} · {} · External — port to canonical",
            snapshot.key.repo.forge
        ),
    }
}

/// Apply one snapshot. Safe to repeat: cards are keyed on `(issue, url)`,
/// and older events than the stored `forgeUpdatedAt` are dropped.
pub async fn apply(client: &LinearClient, snapshot: &PrSnapshot) -> Result<SyncOutcome, SyncError> {
    let mut outcome = SyncOutcome::default();
    let existing = client.attachments_for_url(snapshot.url.as_str()).await?;
    let owned: Vec<_> = existing
        .iter()
        .filter_map(|a| a.metadata.as_ref().map(|m| (a, m)))
        .collect();
    let newer_exists = owned.iter().any(|(_, m)| {
        m.forge_updated_at.parse::<Timestamp>().is_ok_and(|stored| {
            stored > snapshot.forge_updated_at
                || (stored == snapshot.forge_updated_at
                    && ((m.state == "merged" && !matches!(snapshot.state, PrState::Merged { .. }))
                        || (m.state == "closed" && !snapshot.state.is_finished())))
        })
    });
    if newer_exists {
        outcome.stale = true;
        return Ok(outcome);
    }

    let mut issues: Vec<(IssueKey, LinearIssue)> = Vec::new();
    for key in &snapshot.issues {
        let issue = client
            .issue(&key.to_string())
            .await?
            .ok_or_else(|| SyncError::UnknownIssue(key.clone()))?;
        issues.push((key.clone(), issue));
    }

    let external_label = match snapshot.authority {
        Authority::External => client.label_id(EXTERNAL_LABEL).await?,
        Authority::Canonical => None,
    };
    for (key, issue) in &issues {
        client
            .attachment_create(NewAttachment {
                issue_id: &issue.id,
                url: snapshot.url.as_str(),
                title: format!("{} (#{})", snapshot.title, snapshot.key.number),
                subtitle: subtitle(snapshot),
                metadata: metadata(snapshot),
            })
            .await?;
        if let Some(label) = &external_label {
            client.issue_add_label(&issue.id, label).await?;
        }
        outcome.linked.push(key.to_string());
    }

    for (attachment, _) in &owned {
        if !issues
            .iter()
            .any(|(_, issue)| issue.id == attachment.issue_id)
        {
            client.attachment_delete(&attachment.id).await?;
            outcome.unlinked.push(attachment.issue_id.clone());
        }
    }

    if snapshot.authority != Authority::Canonical || !snapshot.state.is_finished() {
        return Ok(outcome);
    }
    for (key, issue) in &issues {
        if issue.is_closed() {
            continue;
        }
        let cards = client.issue_attachments(&issue.id).await?;
        let canonical: Vec<&CardMetadata> = cards
            .iter()
            .filter_map(|a| a.metadata.as_ref())
            .filter(|m| m.authority == Authority::Canonical.as_str())
            .collect();
        let all_finished = !canonical.is_empty()
            && canonical
                .iter()
                .all(|m| matches!(m.state.as_str(), "merged" | "closed"));
        let any_merged_default = canonical
            .iter()
            .any(|m| m.state == "merged" && m.target_is_default_branch);
        if !all_finished || !any_merged_default {
            continue;
        }
        let states = client.workflow_states(&issue.team_key).await?;
        let done = states
            .iter()
            .filter(|s| s.kind == "completed")
            .min_by(|a, b| a.position.total_cmp(&b.position))
            .ok_or_else(|| SyncError::NoDoneState(issue.team_key.clone()))?;
        client.issue_update_state(&issue.id, &done.id).await?;
        outcome.completed.push(key.to_string());
    }
    Ok(outcome)
}

#[derive(Debug, thiserror::Error)]
pub enum BackfillError {
    #[error("forge API request failed")]
    Http(#[from] reqwest::Error),
    #[error("forge API repeated PR #{number} on page {page} (pagination did not advance)")]
    RepeatedPage { number: u64, page: u32 },
    #[error("backfill failed for {failed} of {total} pull request(s): {}", errors.join("; "))]
    PartialFailure {
        failed: usize,
        total: usize,
        errors: Vec<String>,
    },
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct BackfillReport {
    pub pages: u32,
    pub total: usize,
    pub outcomes: Vec<(u64, SyncOutcome)>,
}

/// Paginate a Forgejo-compatible `/repos/{path}/pulls` endpoint and apply
/// every pull request to Linear idempotently. Returns an error if pagination
/// stalls or any pull request fails to parse or sync.
pub async fn backfill_forgejo(
    client: &LinearClient,
    forge: Forge,
    repo_config: &RepoConfig,
    matcher: &IssueKeyMatcher,
    api_base: &Url,
    token: Option<&str>,
    limit: usize,
) -> Result<BackfillReport, BackfillError> {
    let limit = limit.max(1);
    let http = reqwest::Client::new();
    let mut report = BackfillReport::default();
    let mut seen: BTreeSet<u64> = BTreeSet::new();
    let mut prev_page: Option<Vec<serde_json::Value>> = None;
    let mut errors: Vec<String> = Vec::new();
    let mut page: u32 = 1;
    loop {
        let url = format!(
            "{}/repos/{}/pulls?state=all&limit={limit}&page={page}",
            api_base.as_str().trim_end_matches('/'),
            repo_config.path
        );
        let mut request = http.get(&url);
        if let Some(token) = token {
            request = request.header("authorization", format!("token {token}"));
        }
        let prs: Vec<serde_json::Value> = request.send().await?.error_for_status()?.json().await?;
        if prs.is_empty() {
            break;
        }
        if prev_page.as_ref().is_some_and(|prev| prev == &prs) {
            let number = prs[0]
                .get("number")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            return Err(BackfillError::RepeatedPage { number, page });
        }
        report.pages += 1;
        let page_len = prs.len();
        let mut new_numbered = 0usize;
        let mut first_repeat = None;
        for pr in &prs {
            if let Some(number) = pr.get("number").and_then(serde_json::Value::as_u64) {
                if !seen.insert(number) {
                    first_repeat.get_or_insert(number);
                    continue;
                }
                new_numbered += 1;
            }
            report.total += 1;
            match snapshot_from_forgejo_api(forge, pr, repo_config, matcher) {
                Ok(snapshot) => match apply(client, &snapshot).await {
                    Ok(outcome) => report.outcomes.push((snapshot.key.number, outcome)),
                    Err(e) => errors.push(format!("#{}: {e}", snapshot.key.number)),
                },
                Err(e) => errors.push(e.to_string()),
            }
        }
        if let Some(number) = first_repeat {
            if page > 1 && new_numbered == 0 {
                return Err(BackfillError::RepeatedPage { number, page });
            }
        }
        prev_page = Some(prs);
        if page_len < limit {
            break;
        }
        page += 1;
    }
    if !errors.is_empty() {
        return Err(BackfillError::PartialFailure {
            failed: errors.len(),
            total: report.total,
            errors,
        });
    }
    Ok(report)
}

/// Best-effort: tag each resolvable issue of a dead-lettered event.
pub async fn mark_sync_error(
    client: &LinearClient,
    issues: &[IssueKey],
) -> Result<(), LinearError> {
    let Some(label) = client.label_id(SYNC_ERROR_LABEL).await? else {
        return Ok(());
    };
    for key in issues {
        if let Some(issue) = client.issue(&key.to_string()).await? {
            client.issue_add_label(&issue.id, &label).await?;
        }
    }
    Ok(())
}
