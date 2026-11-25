//! Linear GraphQL client: just the operations the bridge needs.

use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::config::LinearConfig;

/// Marker in attachment metadata identifying cards this bridge owns.
pub const BRIDGE_ID: &str = "forge-linear-sync";

#[derive(Debug, thiserror::Error)]
pub enum LinearError {
    #[error("request to Linear failed")]
    Transport(#[source] reqwest::Error),
    #[error("Linear rate limit reached")]
    RateLimited { retry_after: Option<Duration> },
    #[error("Linear answered HTTP {status}")]
    Server { status: u16 },
    #[error("Linear rejected the request with HTTP {status}: {body}")]
    Rejected { status: u16, body: String },
    #[error("Linear returned GraphQL errors: {}", messages.join("; "))]
    GraphQl { messages: Vec<String> },
    #[error("Linear response did not have the expected shape")]
    Decode(#[source] serde_json::Error),
}

impl LinearError {
    /// Whether retrying the same request later may succeed.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            LinearError::Transport(_)
                | LinearError::RateLimited { .. }
                | LinearError::Server { .. }
        )
    }

    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            LinearError::RateLimited { retry_after } => *retry_after,
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct LinearIssue {
    pub id: String,
    pub identifier: String,
    pub team_key: String,
    /// Linear's workflow state type: `backlog`, `unstarted`, `started`,
    /// `completed`, `canceled` or `triage`.
    pub state_type: String,
}

impl LinearIssue {
    pub fn is_closed(&self) -> bool {
        matches!(self.state_type.as_str(), "completed" | "canceled")
    }
}

/// Metadata the bridge stores on each card it owns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardMetadata {
    pub bridge: String,
    pub forge: String,
    pub repo: String,
    pub number: u64,
    pub state: String,
    pub head: String,
    pub forge_updated_at: String,
    pub authority: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    pub id: String,
    pub url: String,
    pub issue_id: String,
    /// `None` when the card was not written by this bridge.
    pub metadata: Option<CardMetadata>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct WorkflowState {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub position: f64,
}

#[derive(Debug, Clone)]
pub struct NewAttachment<'a> {
    pub issue_id: &'a str,
    pub url: &'a str,
    pub title: String,
    pub subtitle: String,
    pub metadata: CardMetadata,
}

#[derive(Clone)]
pub struct LinearClient {
    http: reqwest::Client,
    config: LinearConfig,
}

#[derive(Deserialize)]
struct GraphQlError {
    message: String,
    #[serde(default)]
    extensions: Option<Value>,
}

#[derive(Deserialize)]
struct GraphQlResponse {
    #[serde(default)]
    data: Option<Value>,
    #[serde(default)]
    errors: Option<Vec<GraphQlError>>,
}

/// `X-RateLimit-Requests-Reset` is a UTC epoch in milliseconds.
fn reset_delay(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    let reset_ms: u64 = headers
        .get("x-ratelimit-requests-reset")?
        .to_str()
        .ok()?
        .parse()
        .ok()?;
    let now_ms = u64::try_from(jiff::Timestamp::now().as_millisecond()).ok()?;
    Some(Duration::from_millis(reset_ms.saturating_sub(now_ms)))
}

fn decode<T: DeserializeOwned>(value: Value) -> Result<T, LinearError> {
    serde_json::from_value(value).map_err(LinearError::Decode)
}

#[derive(Deserialize)]
struct RawAttachment {
    id: String,
    url: String,
    #[serde(default)]
    metadata: Option<Value>,
    #[serde(default)]
    issue: Option<RawIssueRef>,
}

#[derive(Deserialize)]
struct RawIssueRef {
    id: String,
}

fn attachment(raw: RawAttachment, issue_id: Option<&str>) -> Option<Attachment> {
    let issue_id = raw
        .issue
        .map(|i| i.id)
        .or_else(|| issue_id.map(str::to_owned))?;
    let metadata = raw
        .metadata
        .and_then(|m| serde_json::from_value::<CardMetadata>(m).ok())
        .filter(|m| m.bridge == BRIDGE_ID);
    Some(Attachment {
        id: raw.id,
        url: raw.url,
        issue_id,
        metadata,
    })
}

impl LinearClient {
    pub fn new(config: LinearConfig) -> Self {
        LinearClient {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("reqwest client with rustls builds"),
            config,
        }
    }

    async fn run(
        &self,
        operation: &str,
        query: &str,
        variables: Value,
    ) -> Result<Value, LinearError> {
        let api_key = String::from_utf8_lossy(self.config.api_key.expose()).into_owned();
        let response = self
            .http
            .post(self.config.endpoint.clone())
            .header("authorization", api_key)
            .json(&json!({ "operationName": operation, "query": query, "variables": variables }))
            .send()
            .await
            .map_err(LinearError::Transport)?;
        let status = response.status();
        let retry_after = reset_delay(response.headers());
        if status.as_u16() == 429 {
            return Err(LinearError::RateLimited { retry_after });
        }
        if status.is_server_error() {
            return Err(LinearError::Server {
                status: status.as_u16(),
            });
        }
        let text = response.text().await.map_err(LinearError::Transport)?;
        let parsed: Result<GraphQlResponse, _> = serde_json::from_str(&text);
        let body = match parsed {
            Ok(body) => body,
            Err(_) if !status.is_success() => {
                return Err(LinearError::Rejected {
                    status: status.as_u16(),
                    body: text.chars().take(500).collect(),
                })
            }
            Err(e) => return Err(LinearError::Decode(e)),
        };
        if let Some(errors) = body.errors.filter(|e| !e.is_empty()) {
            let rate_limited = errors.iter().any(|e| {
                e.extensions
                    .as_ref()
                    .and_then(|x| x.get("code"))
                    .and_then(Value::as_str)
                    == Some("RATELIMITED")
            });
            if rate_limited {
                return Err(LinearError::RateLimited { retry_after });
            }
            let messages: Vec<String> = errors.into_iter().map(|e| e.message).collect();
            if !status.is_success() {
                return Err(LinearError::Rejected {
                    status: status.as_u16(),
                    body: messages.join("; "),
                });
            }
            return Err(LinearError::GraphQl { messages });
        }
        if !status.is_success() {
            return Err(LinearError::Rejected {
                status: status.as_u16(),
                body: text.chars().take(500).collect(),
            });
        }
        Ok(body.data.unwrap_or(Value::Null))
    }

    /// The issue for a key such as `COD-42`, or `None` if Linear has none.
    pub async fn issue(&self, key: &str) -> Result<Option<LinearIssue>, LinearError> {
        let data = self
            .run(
                "Issue",
                "query Issue($id: String!) { issue(id: $id) { id identifier team { key } state { type } } }",
                json!({ "id": key }),
            )
            .await;
        let data = match data {
            Ok(data) => data,
            // Linear reports an unknown identifier as an "Entity not found" error.
            Err(LinearError::GraphQl { messages })
                if messages.iter().any(|m| m.contains("not found")) =>
            {
                return Ok(None)
            }
            Err(e) => return Err(e),
        };
        let issue = &data["issue"];
        if issue.is_null() {
            return Ok(None);
        }
        Ok(Some(LinearIssue {
            id: decode(issue["id"].clone())?,
            identifier: decode(issue["identifier"].clone())?,
            team_key: decode(issue["team"]["key"].clone())?,
            state_type: decode(issue["state"]["type"].clone())?,
        }))
    }

    pub async fn attachments_for_url(&self, url: &str) -> Result<Vec<Attachment>, LinearError> {
        let data = self
            .run(
                "AttachmentsForUrl",
                "query AttachmentsForUrl($url: String!) { attachmentsForURL(url: $url) { nodes { id url metadata issue { id } } } }",
                json!({ "url": url }),
            )
            .await?;
        let nodes: Vec<RawAttachment> = decode(data["attachmentsForURL"]["nodes"].clone())?;
        Ok(nodes
            .into_iter()
            .filter_map(|n| attachment(n, None))
            .collect())
    }

    pub async fn issue_attachments(&self, issue_id: &str) -> Result<Vec<Attachment>, LinearError> {
        let data = self
            .run(
                "IssueAttachments",
                "query IssueAttachments($id: String!) { issue(id: $id) { attachments { nodes { id url metadata } } } }",
                json!({ "id": issue_id }),
            )
            .await?;
        let nodes: Vec<RawAttachment> = decode(data["issue"]["attachments"]["nodes"].clone())?;
        Ok(nodes
            .into_iter()
            .filter_map(|n| attachment(n, Some(issue_id)))
            .collect())
    }

    /// Create or update (Linear dedups on `(issueId, url)`) a card.
    pub async fn attachment_create(&self, input: NewAttachment<'_>) -> Result<String, LinearError> {
        let data = self
            .run(
                "AttachmentCreate",
                "mutation AttachmentCreate($input: AttachmentCreateInput!) { attachmentCreate(input: $input) { success attachment { id } } }",
                json!({ "input": {
                    "issueId": input.issue_id,
                    "url": input.url,
                    "title": input.title,
                    "subtitle": input.subtitle,
                    "metadata": input.metadata,
                }}),
            )
            .await?;
        decode(data["attachmentCreate"]["attachment"]["id"].clone())
    }

    pub async fn attachment_delete(&self, id: &str) -> Result<(), LinearError> {
        self.run(
            "AttachmentDelete",
            "mutation AttachmentDelete($id: String!) { attachmentDelete(id: $id) { success } }",
            json!({ "id": id }),
        )
        .await
        .map(drop)
    }

    pub async fn workflow_states(&self, team_key: &str) -> Result<Vec<WorkflowState>, LinearError> {
        let data = self
            .run(
                "WorkflowStates",
                "query WorkflowStates($team: String!) { workflowStates(filter: { team: { key: { eq: $team } } }) { nodes { id name type position } } }",
                json!({ "team": team_key }),
            )
            .await?;
        decode(data["workflowStates"]["nodes"].clone())
    }

    pub async fn issue_update_state(
        &self,
        issue_id: &str,
        state_id: &str,
    ) -> Result<(), LinearError> {
        self.run(
            "IssueUpdate",
            "mutation IssueUpdate($id: String!, $stateId: String!) { issueUpdate(id: $id, input: { stateId: $stateId }) { success } }",
            json!({ "id": issue_id, "stateId": state_id }),
        )
        .await
        .map(drop)
    }

    pub async fn label_id(&self, name: &str) -> Result<Option<String>, LinearError> {
        let data = self
            .run(
                "IssueLabels",
                "query IssueLabels($name: String!) { issueLabels(filter: { name: { eq: $name } }) { nodes { id } } }",
                json!({ "name": name }),
            )
            .await?;
        let nodes: Vec<RawIssueRef> = decode(data["issueLabels"]["nodes"].clone())?;
        Ok(nodes.into_iter().next().map(|n| n.id))
    }

    pub async fn issue_add_label(&self, issue_id: &str, label_id: &str) -> Result<(), LinearError> {
        self.run(
            "IssueAddLabel",
            "mutation IssueAddLabel($id: String!, $labelId: String!) { issueAddLabel(id: $id, labelId: $labelId) { success } }",
            json!({ "id": issue_id, "labelId": label_id }),
        )
        .await
        .map(drop)
    }
}
