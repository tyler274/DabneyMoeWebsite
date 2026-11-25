//! An in-memory Linear GraphQL server for tests (feature `mock-linear`).
//!
//! It answers the operations [`crate::linear::LinearClient`] sends, keyed on
//! `operationName`, and keeps attachments idempotent on `(issueId, url)` as
//! Linear does. Failures can be injected ahead of any request.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use serde_json::{json, Value};

/// A failure to return instead of handling the next request.
#[derive(Debug, Clone)]
pub enum Failure {
    /// HTTP 429 with `X-RateLimit-Requests-Reset` this many ms from now.
    RateLimited { reset_in_ms: u64 },
    /// HTTP 500.
    ServerError,
    /// HTTP 400 with a GraphQL validation error (permanent).
    BadRequest,
}

#[derive(Debug, Clone)]
pub struct MockIssue {
    pub id: String,
    pub identifier: String,
    pub team_key: String,
    pub state_id: String,
    pub labels: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct MockAttachment {
    pub id: String,
    pub issue_id: String,
    pub url: String,
    pub title: String,
    pub subtitle: String,
    pub metadata: Value,
}

#[derive(Debug, Clone)]
pub struct MockState {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub position: f64,
}

#[derive(Default)]
struct Inner {
    issues: BTreeMap<String, MockIssue>,
    states: Vec<MockState>,
    attachments: BTreeMap<String, MockAttachment>,
    labels: BTreeMap<String, String>,
    failures: VecDeque<Failure>,
    operations: Vec<String>,
    next_id: u64,
}

impl Inner {
    fn id(&mut self, prefix: &str) -> String {
        self.next_id += 1;
        format!("{prefix}-{}", self.next_id)
    }

    fn issue_by_key_or_id(&self, key: &str) -> Option<&MockIssue> {
        self.issues
            .values()
            .find(|i| i.id == key || i.identifier == key)
    }

    fn state(&self, id: &str) -> Option<&MockState> {
        self.states.iter().find(|s| s.id == id)
    }
}

/// Handle to a running mock. Cloning shares the same state.
#[derive(Clone)]
pub struct MockLinear {
    inner: Arc<Mutex<Inner>>,
    pub endpoint: url::Url,
}

impl MockLinear {
    /// Bind on `127.0.0.1:0` and serve in the background.
    pub async fn start() -> Self {
        let inner = Arc::new(Mutex::new(Inner::default()));
        {
            let mut g = inner.lock().unwrap();
            for (name, kind, position) in [
                ("Backlog", "backlog", 0.0),
                ("Todo", "unstarted", 1.0),
                ("In Progress", "started", 2.0),
                ("In Review", "started", 3.0),
                ("Done", "completed", 4.0),
                ("Canceled", "canceled", 5.0),
            ] {
                let id = g.id("state");
                g.states.push(MockState {
                    id,
                    name: name.to_owned(),
                    kind: kind.to_owned(),
                    position,
                });
            }
            for label in [crate::sync::EXTERNAL_LABEL, crate::sync::SYNC_ERROR_LABEL] {
                let id = g.id("label");
                g.labels.insert(label.to_owned(), id);
            }
        }
        let app = Router::new()
            .route("/graphql", post(handle))
            .with_state(inner.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock Linear");
        let addr = listener.local_addr().expect("mock Linear address");
        tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("mock Linear serves");
        });
        MockLinear {
            inner,
            endpoint: format!("http://{addr}/graphql").parse().expect("mock URL"),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap()
    }

    /// Add an issue like `COD-42` in the team's `Todo` state.
    pub fn add_issue(&self, identifier: &str) {
        let mut g = self.lock();
        let team_key = identifier.split('-').next().unwrap_or_default().to_owned();
        let state_id = g
            .states
            .iter()
            .find(|s| s.name == "Todo")
            .unwrap()
            .id
            .clone();
        let id = g.id("issue");
        g.issues.insert(
            identifier.to_owned(),
            MockIssue {
                id,
                identifier: identifier.to_owned(),
                team_key,
                state_id,
                labels: Vec::new(),
            },
        );
    }

    pub fn issue_state(&self, identifier: &str) -> String {
        let g = self.lock();
        let issue = &g.issues[identifier];
        g.state(&issue.state_id).unwrap().name.clone()
    }

    pub fn issue_labels(&self, identifier: &str) -> Vec<String> {
        let g = self.lock();
        let issue = &g.issues[identifier];
        g.labels
            .iter()
            .filter(|(_, id)| issue.labels.contains(id))
            .map(|(name, _)| name.clone())
            .collect()
    }

    pub fn attachments(&self, identifier: &str) -> Vec<MockAttachment> {
        let g = self.lock();
        let issue_id = g.issues[identifier].id.clone();
        g.attachments
            .values()
            .filter(|a| a.issue_id == issue_id)
            .cloned()
            .collect()
    }

    pub fn fail_next(&self, failure: Failure) {
        self.lock().failures.push_back(failure);
    }

    /// Count of handled requests per operation name.
    pub fn operation_count(&self, operation: &str) -> usize {
        self.lock()
            .operations
            .iter()
            .filter(|o| *o == operation)
            .count()
    }
}

fn gql_error(message: &str) -> Response {
    (
        StatusCode::OK,
        Json(json!({ "errors": [{ "message": message }] })),
    )
        .into_response()
}

fn attachment_json(a: &MockAttachment, with_issue: bool) -> Value {
    let mut v = json!({ "id": a.id, "url": a.url, "metadata": a.metadata });
    if with_issue {
        v["issue"] = json!({ "id": a.issue_id });
    }
    v
}

async fn handle(
    State(inner): State<Arc<Mutex<Inner>>>,
    headers: HeaderMap,
    Json(req): Json<Value>,
) -> Response {
    let mut g = inner.lock().unwrap();
    if headers.get("authorization").is_none() {
        return (StatusCode::UNAUTHORIZED, "missing api key").into_response();
    }
    if let Some(failure) = g.failures.pop_front() {
        return match failure {
            Failure::RateLimited { reset_in_ms } => {
                let reset = u64::try_from(jiff::Timestamp::now().as_millisecond()).unwrap_or(0)
                    + reset_in_ms;
                (
                    StatusCode::TOO_MANY_REQUESTS,
                    [("x-ratelimit-requests-reset", reset.to_string())],
                    "rate limited",
                )
                    .into_response()
            }
            Failure::ServerError => (StatusCode::INTERNAL_SERVER_ERROR, "boom").into_response(),
            Failure::BadRequest => (
                StatusCode::BAD_REQUEST,
                Json(json!({ "errors": [{ "message": "Argument Validation Error" }] })),
            )
                .into_response(),
        };
    }
    let op = req["operationName"].as_str().unwrap_or_default().to_owned();
    let vars = &req["variables"];
    g.operations.push(op.clone());
    let data = match op.as_str() {
        "Issue" => {
            let key = vars["id"].as_str().unwrap_or_default();
            let Some(issue) = g.issue_by_key_or_id(key) else {
                return gql_error("Entity not found: Issue");
            };
            let kind = g.state(&issue.state_id).unwrap().kind.clone();
            json!({ "issue": { "id": issue.id, "identifier": issue.identifier,
                    "team": { "key": issue.team_key }, "state": { "type": kind } } })
        }
        "IssueAttachments" => {
            let id = vars["id"].as_str().unwrap_or_default();
            let nodes: Vec<Value> = g
                .attachments
                .values()
                .filter(|a| a.issue_id == id)
                .map(|a| attachment_json(a, false))
                .collect();
            json!({ "issue": { "attachments": { "nodes": nodes } } })
        }
        "AttachmentsForUrl" => {
            let url = vars["url"].as_str().unwrap_or_default();
            let nodes: Vec<Value> = g
                .attachments
                .values()
                .filter(|a| a.url == url)
                .map(|a| attachment_json(a, true))
                .collect();
            json!({ "attachmentsForURL": { "nodes": nodes } })
        }
        "AttachmentCreate" => {
            let input = &vars["input"];
            let issue_id = input["issueId"].as_str().unwrap_or_default().to_owned();
            let url = input["url"].as_str().unwrap_or_default().to_owned();
            if !g.issues.values().any(|i| i.id == issue_id) {
                return gql_error("Entity not found: Issue");
            }
            let existing = g
                .attachments
                .values()
                .find(|a| a.issue_id == issue_id && a.url == url)
                .map(|a| a.id.clone());
            let id = existing.unwrap_or_else(|| g.id("attachment"));
            g.attachments.insert(
                id.clone(),
                MockAttachment {
                    id: id.clone(),
                    issue_id,
                    url,
                    title: input["title"].as_str().unwrap_or_default().to_owned(),
                    subtitle: input["subtitle"].as_str().unwrap_or_default().to_owned(),
                    metadata: input["metadata"].clone(),
                },
            );
            json!({ "attachmentCreate": { "success": true, "attachment": { "id": id } } })
        }
        "AttachmentDelete" => {
            let id = vars["id"].as_str().unwrap_or_default();
            let removed = g.attachments.remove(id).is_some();
            json!({ "attachmentDelete": { "success": removed } })
        }
        "WorkflowStates" => {
            let nodes: Vec<Value> = g
                .states
                .iter()
                .map(|s| json!({ "id": s.id, "name": s.name, "type": s.kind, "position": s.position }))
                .collect();
            json!({ "workflowStates": { "nodes": nodes } })
        }
        "IssueUpdate" => {
            let id = vars["id"].as_str().unwrap_or_default().to_owned();
            let state = vars["stateId"].as_str().unwrap_or_default().to_owned();
            let Some(issue) = g.issues.values_mut().find(|i| i.id == id) else {
                return gql_error("Entity not found: Issue");
            };
            issue.state_id = state;
            json!({ "issueUpdate": { "success": true } })
        }
        "IssueLabels" => {
            let name = vars["name"].as_str().unwrap_or_default();
            let nodes: Vec<Value> = g
                .labels
                .get(name)
                .map(|id| json!({ "id": id }))
                .into_iter()
                .collect();
            json!({ "issueLabels": { "nodes": nodes } })
        }
        "IssueAddLabel" => {
            let id = vars["id"].as_str().unwrap_or_default().to_owned();
            let label = vars["labelId"].as_str().unwrap_or_default().to_owned();
            let Some(issue) = g.issues.values_mut().find(|i| i.id == id) else {
                return gql_error("Entity not found: Issue");
            };
            if !issue.labels.contains(&label) {
                issue.labels.push(label);
            }
            json!({ "issueAddLabel": { "success": true } })
        }
        other => return gql_error(&format!("unknown operation {other}")),
    };
    Json(json!({ "data": data })).into_response()
}
