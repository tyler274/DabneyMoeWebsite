//! The upsert planner: one verified snapshot in, idempotent Linear writes out.

use jiff::Timestamp;

use crate::domain::{Authority, IssueKey, PrSnapshot, PrState};
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
        m.forge_updated_at
            .parse::<Timestamp>()
            .is_ok_and(|stored| stored > snapshot.forge_updated_at)
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

    let lands_on_default =
        matches!(snapshot.state, PrState::Merged { .. }) && snapshot.target_is_default_branch;
    if snapshot.authority != Authority::Canonical || !lands_on_default {
        return Ok(outcome);
    }
    for (key, issue) in &issues {
        if issue.is_closed() {
            continue;
        }
        let cards = client.issue_attachments(&issue.id).await?;
        let all_finished = cards
            .iter()
            .filter_map(|a| a.metadata.as_ref())
            .filter(|m| m.authority == Authority::Canonical.as_str())
            .all(|m| matches!(m.state.as_str(), "merged" | "closed"));
        if !all_finished {
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
