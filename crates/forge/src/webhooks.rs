//! Signed, durable, Forgejo-compatible pull-request webhooks.
//!
//! Each event is recorded per target in `webhook_deliveries` before any
//! network I/O, then delivered in emission order by one background task.
//! The payload type and the HMAC signer are the ones `forge-linear-sync`
//! parses and verifies with, so emitter and receiver share one definition.

use std::time::Duration;

use forge_linear_sync::event::{
    ForgejoBranch, ForgejoPullRequest, ForgejoPullRequestEvent, ForgejoRepository,
};
use forge_linear_sync::verify::sign_hex;

use crate::app::{blocking, App, Forge};
use crate::db::{DeliveryStatus, PrState, PullRequest, RepoRecord, WebhookDelivery};
use crate::error::ForgeError;

/// Attempts before a delivery is marked failed.
pub const MAX_ATTEMPTS: u32 = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrAction {
    Opened,
    Synchronized,
    Closed,
}

impl PrAction {
    pub fn as_str(self) -> &'static str {
        match self {
            PrAction::Opened => "opened",
            PrAction::Synchronized => "synchronized",
            PrAction::Closed => "closed",
        }
    }
}

/// The Forgejo `pull_request` event for `pr`.
pub fn payload(
    forge: &Forge,
    record: &RepoRecord,
    pr: &PullRequest,
    action: PrAction,
) -> ForgejoPullRequestEvent {
    let (state, merged, merge_commit_sha) = match &pr.state {
        PrState::Open => ("open", false, None),
        PrState::Closed => ("closed", false, None),
        PrState::Merged { commit } => ("closed", true, Some(commit.to_string())),
    };
    ForgejoPullRequestEvent {
        action: action.as_str().to_owned(),
        number: pr.number,
        pull_request: ForgejoPullRequest {
            number: pr.number,
            html_url: forge
                .config
                .link(&format!("{}/pull/{}", record.name, pr.number)),
            title: pr.title.clone(),
            body: Some(pr.body.clone()),
            state: state.to_owned(),
            draft: false,
            merged,
            merge_commit_sha,
            updated_at: crate::db::rfc3339(pr.updated_at),
            head: ForgejoBranch {
                branch: pr.source_branch.clone(),
                sha: pr.head_oid.to_string(),
            },
            base: ForgejoBranch {
                branch: pr.target_branch.clone(),
                sha: pr.base_oid.to_string(),
            },
        },
        repository: ForgejoRepository {
            full_name: record.name.to_string(),
            default_branch: Some(record.default_branch.clone()),
        },
    }
}

fn delivery_id() -> Result<String, ForgeError> {
    let mut buf = [0u8; 16];
    getrandom::fill(&mut buf).map_err(|e| ForgeError::Io(std::io::Error::other(e)))?;
    Ok(hex::encode(buf))
}

/// Prepare one delivery record per configured target for transactional insert.
pub fn prepare(
    forge: &Forge,
    record: &RepoRecord,
    pr: &PullRequest,
    action: PrAction,
) -> Result<Vec<crate::db::PendingDelivery>, ForgeError> {
    if forge.config.webhooks.is_empty() {
        return Ok(Vec::new());
    }
    let body = serde_json::to_vec(&payload(forge, record, pr, action))
        .map_err(|e| ForgeError::Io(std::io::Error::other(e)))?;
    let mut out = Vec::with_capacity(forge.config.webhooks.len());
    for target in &forge.config.webhooks {
        out.push(crate::db::PendingDelivery {
            delivery_id: delivery_id()?,
            action: action.as_str().to_owned(),
            target: target.url.as_str().to_owned(),
            payload: body.clone(),
        });
    }
    Ok(out)
}

/// Record one delivery per configured target and wake the deliverer.
pub fn emit(
    forge: &Forge,
    record: &RepoRecord,
    pr: &PullRequest,
    action: PrAction,
) -> Result<(), ForgeError> {
    let deliveries = prepare(forge, record, pr, action)?;
    if deliveries.is_empty() {
        return Ok(());
    }
    for d in deliveries {
        forge
            .db
            .insert_delivery(pr.id, &d.delivery_id, &d.action, &d.target, &d.payload)?;
    }
    forge.hooks_ready.notify_one();
    Ok(())
}

fn backoff(attempt: u32) -> Duration {
    Duration::from_millis(500u64.saturating_mul(1 << attempt.min(7))).min(Duration::from_secs(60))
}

enum Sent {
    Delivered(u16),
    /// Worth retrying (network error, 429, 5xx).
    Retry(Option<u16>, String),
    /// Retrying cannot help (other 4xx, target removed).
    Permanent(Option<u16>, String),
}

async fn send(app: &App, delivery: &WebhookDelivery) -> Sent {
    let Some(target) = app
        .config
        .webhooks
        .iter()
        .find(|t| t.url.as_str() == delivery.target)
    else {
        return Sent::Permanent(None, "target is no longer configured".into());
    };
    let signature = sign_hex(&target.secret, &delivery.payload);
    let request = app
        .http
        .post(target.url.clone())
        .header("content-type", "application/json")
        .header("x-forgejo-event", "pull_request")
        .header("x-forgejo-delivery", &delivery.delivery_id)
        .header("x-forgejo-signature", &signature)
        .header("x-gitea-event", "pull_request")
        .header("x-gitea-delivery", &delivery.delivery_id)
        .header("x-gitea-signature", &signature)
        .body(delivery.payload.clone());
    match request.send().await {
        Ok(response) => {
            let status = response.status();
            let code = status.as_u16();
            if status.is_success() {
                Sent::Delivered(code)
            } else if status.is_server_error() || code == 429 {
                Sent::Retry(Some(code), format!("HTTP {status}"))
            } else {
                Sent::Permanent(Some(code), format!("HTTP {status}"))
            }
        }
        Err(e) => Sent::Retry(None, format!("{e}")),
    }
}

/// Deliver pending webhooks forever, oldest first. A failing delivery is
/// retried with backoff before later ones go out, so a receiver sees events
/// in the order they happened.
pub async fn run_deliverer(app: App) {
    loop {
        let next = blocking(&app, |forge| forge.db.next_pending_delivery()).await;
        let delivery = match next {
            Ok(Some(delivery)) => delivery,
            Ok(None) => {
                app.hooks_ready.notified().await;
                continue;
            }
            Err(e) => {
                tracing::error!(error = %e.chain(), "cannot read webhook queue");
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }
        };
        let attempts = delivery.attempts + 1;
        let (status, http_status, error, retry) = match send(&app, &delivery).await {
            Sent::Delivered(code) => (DeliveryStatus::Delivered, Some(code), None, false),
            Sent::Retry(code, e) if attempts < MAX_ATTEMPTS => {
                (DeliveryStatus::Pending, code, Some(e), true)
            }
            Sent::Retry(code, e) | Sent::Permanent(code, e) => {
                (DeliveryStatus::Failed, code, Some(e), false)
            }
        };
        match &error {
            None => {
                tracing::info!(delivery = %delivery.delivery_id, action = %delivery.action, "webhook delivered")
            }
            Some(e) => {
                tracing::warn!(delivery = %delivery.delivery_id, attempts, error = %e, "webhook delivery failed")
            }
        }
        let id = delivery.id;
        let recorded = blocking(&app, move |forge| {
            forge
                .db
                .finish_delivery(id, status, http_status, attempts, error.as_deref())
        })
        .await;
        if let Err(e) = recorded {
            tracing::error!(error = %e.chain(), "cannot record webhook delivery");
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
        if retry {
            tokio::time::sleep(backoff(attempts)).await;
        }
    }
}
