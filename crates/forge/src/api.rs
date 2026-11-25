//! The JSON API under `/api/v1`, authenticated with personal access tokens.
//! Its pull-request and status shapes follow Forgejo/Gitea where they overlap.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use gix::ObjectId;
use serde::{Deserialize, Serialize};

use crate::app::{blocking, App};
use crate::auth::{self, Action, Principal, Scope};
use crate::db::{
    CheckState, CheckStatus, InlineComment, PrState, PullRequest, RepoRecord, Review, Side, Verdict,
};
use crate::diff::FileDiff;
use crate::error::ForgeError;
use crate::merge::MergeReadiness;
use crate::pr::{self, NewComment, NewPullRequest, NewStatus};
use crate::repo::RepoName;

pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/v1/repos", post(create_repo))
        .route("/api/v1/repos/{owner}/{repo}", get(get_repo))
        .route(
            "/api/v1/repos/{owner}/{repo}/pulls",
            get(list_pulls).post(open_pull),
        )
        .route("/api/v1/repos/{owner}/{repo}/pulls/{number}", get(get_pull))
        .route(
            "/api/v1/repos/{owner}/{repo}/pulls/{number}/files",
            get(pull_files),
        )
        .route(
            "/api/v1/repos/{owner}/{repo}/pulls/{number}/readiness",
            get(pull_readiness),
        )
        .route(
            "/api/v1/repos/{owner}/{repo}/pulls/{number}/reviews",
            get(list_reviews).post(post_review),
        )
        .route(
            "/api/v1/repos/{owner}/{repo}/pulls/{number}/comments",
            get(list_comments).post(post_comment),
        )
        .route(
            "/api/v1/repos/{owner}/{repo}/pulls/{number}/merge",
            post(merge_pull),
        )
        .route(
            "/api/v1/repos/{owner}/{repo}/pulls/{number}/close",
            post(close_pull),
        )
        .route(
            "/api/v1/repos/{owner}/{repo}/statuses/{oid}",
            get(list_statuses).post(post_status),
        )
}

fn signed_in(principal: Option<Principal>) -> Result<Principal, ForgeError> {
    principal.ok_or(ForgeError::Unauthenticated)
}

fn parse_oid(raw: &str) -> Result<ObjectId, ForgeError> {
    ObjectId::from_hex(raw.as_bytes())
        .map_err(|_| ForgeError::BadRequest(format!("invalid commit id {raw:?}")))
}

#[derive(Serialize)]
pub struct RepoJson {
    pub full_name: String,
    pub private: bool,
    pub default_branch: String,
    pub required_approvals: u32,
    pub required_checks: Vec<String>,
    pub clone_url: String,
    pub html_url: String,
}

fn repo_json(app: &crate::app::Forge, r: &RepoRecord) -> RepoJson {
    RepoJson {
        full_name: r.name.to_string(),
        private: r.private,
        default_branch: r.default_branch.clone(),
        required_approvals: r.required_approvals,
        required_checks: r.required_checks.clone(),
        clone_url: app.config.link(&format!("{}.git", r.name)),
        html_url: app.config.link(&r.name.to_string()),
    }
}

#[derive(Deserialize)]
pub struct CreateRepo {
    pub name: String,
    pub owner: Option<String>,
    #[serde(default)]
    pub private: bool,
    pub default_branch: Option<String>,
}

async fn create_repo(
    State(app): State<App>,
    headers: HeaderMap,
    Json(req): Json<CreateRepo>,
) -> Result<(StatusCode, Json<RepoJson>), ForgeError> {
    blocking(&app, move |forge| {
        let principal = signed_in(auth::authenticate(&forge.db, &headers)?)?;
        let owner = req.owner.unwrap_or_else(|| principal.user.clone());
        let allowed = principal.scopes.contains(&Scope::Admin)
            || (owner == principal.user && principal.scopes.contains(&Scope::Write));
        if !allowed {
            return Err(ForgeError::Forbidden(format!(
                "{} may not create repositories for {owner}",
                principal.user
            )));
        }
        let name = RepoName::new(&owner, &req.name)?;
        let record = forge.create_repo(
            &name,
            req.private,
            req.default_branch.as_deref().unwrap_or("main"),
        )?;
        Ok((StatusCode::CREATED, Json(repo_json(forge, &record))))
    })
    .await
}

async fn get_repo(
    State(app): State<App>,
    Path((owner, repo)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Json<RepoJson>, ForgeError> {
    blocking(&app, move |forge| {
        let principal = auth::authenticate(&forge.db, &headers)?;
        let record = forge.authorize(&owner, &repo, principal.as_ref(), Action::Read)?;
        Ok(Json(repo_json(forge, &record)))
    })
    .await
}

#[derive(Serialize)]
pub struct BranchJson {
    #[serde(rename = "ref")]
    pub branch: String,
    pub sha: String,
}

#[derive(Serialize)]
pub struct PullJson {
    pub number: u64,
    pub title: String,
    pub body: String,
    pub user: String,
    pub state: &'static str,
    pub merged: bool,
    pub merge_commit_sha: Option<String>,
    pub head: BranchJson,
    pub base: BranchJson,
    pub html_url: String,
    pub created_at: String,
    pub updated_at: String,
}

fn pull_json(app: &crate::app::Forge, record: &RepoRecord, pr: &PullRequest) -> PullJson {
    let merge_commit_sha = match &pr.state {
        PrState::Merged { commit } => Some(commit.to_string()),
        _ => None,
    };
    PullJson {
        number: pr.number,
        title: pr.title.clone(),
        body: pr.body.clone(),
        user: pr.author.clone(),
        state: pr.state.label(),
        merged: merge_commit_sha.is_some(),
        merge_commit_sha,
        head: BranchJson {
            branch: pr.source_branch.clone(),
            sha: pr.head_oid.to_string(),
        },
        base: BranchJson {
            branch: pr.target_branch.clone(),
            sha: pr.base_oid.to_string(),
        },
        html_url: app
            .config
            .link(&format!("{}/pull/{}", record.name, pr.number)),
        created_at: crate::db::rfc3339(pr.created_at),
        updated_at: crate::db::rfc3339(pr.updated_at),
    }
}

#[derive(Deserialize, Default)]
pub struct ListPullsQuery {
    pub state: Option<String>,
    pub page: Option<usize>,
    pub limit: Option<usize>,
}

async fn list_pulls(
    State(app): State<App>,
    Path((owner, repo)): Path<(String, String)>,
    Query(query): Query<ListPullsQuery>,
    headers: HeaderMap,
) -> Result<Json<Vec<PullJson>>, ForgeError> {
    blocking(&app, move |forge| {
        let principal = auth::authenticate(&forge.db, &headers)?;
        let record = forge.authorize(&owner, &repo, principal.as_ref(), Action::Read)?;
        let page = query.page.unwrap_or(1);
        if page == 0 {
            return Err(ForgeError::BadRequest("page must be >= 1".into()));
        }
        let limit = query.limit.unwrap_or(50);
        if limit == 0 {
            return Err(ForgeError::BadRequest("limit must be >= 1".into()));
        }
        let limit = limit.min(100);
        let state_filter = query.state.as_deref().unwrap_or("all");
        if !matches!(state_filter, "all" | "open" | "closed" | "merged") {
            return Err(ForgeError::BadRequest(format!(
                "unknown pull request state filter {state_filter:?}"
            )));
        }
        let prs = forge.db.prs(record.id)?;
        let skip = (page - 1).saturating_mul(limit);
        let page_items: Vec<PullJson> = prs
            .iter()
            .filter(|p| match state_filter {
                "all" => true,
                "open" => p.state == PrState::Open,
                "closed" => p.state != PrState::Open,
                "merged" => matches!(p.state, PrState::Merged { .. }),
                _ => false,
            })
            .skip(skip)
            .take(limit)
            .map(|p| pull_json(forge, &record, p))
            .collect();
        Ok(Json(page_items))
    })
    .await
}

#[derive(Deserialize)]
pub struct OpenPull {
    pub title: String,
    #[serde(default)]
    pub body: String,
    pub head: String,
    pub base: Option<String>,
}

async fn open_pull(
    State(app): State<App>,
    Path((owner, repo)): Path<(String, String)>,
    headers: HeaderMap,
    Json(req): Json<OpenPull>,
) -> Result<(StatusCode, Json<PullJson>), ForgeError> {
    blocking(&app, move |forge| {
        let principal = auth::authenticate(&forge.db, &headers)?;
        let record = forge.authorize(&owner, &repo, principal.as_ref(), Action::Contribute)?;
        let principal = signed_in(principal)?;
        let pr = pr::open(
            forge,
            &record,
            &principal,
            NewPullRequest {
                title: req.title,
                body: req.body,
                head: req.head,
                base: req.base,
            },
        )?;
        Ok((StatusCode::CREATED, Json(pull_json(forge, &record, &pr))))
    })
    .await
}

#[derive(Serialize)]
pub struct PullDetail {
    #[serde(flatten)]
    pub pull: PullJson,
    pub readiness: MergeReadiness,
}

async fn get_pull(
    State(app): State<App>,
    Path((owner, repo, number)): Path<(String, String, u64)>,
    headers: HeaderMap,
) -> Result<Json<PullDetail>, ForgeError> {
    blocking(&app, move |forge| {
        let principal = auth::authenticate(&forge.db, &headers)?;
        let record = forge.authorize(&owner, &repo, principal.as_ref(), Action::Read)?;
        let pr = pr::find(forge, &record, number)?;
        let readiness = crate::merge::readiness(forge, &record, &pr)?;
        Ok(Json(PullDetail {
            pull: pull_json(forge, &record, &pr),
            readiness,
        }))
    })
    .await
}

async fn pull_files(
    State(app): State<App>,
    Path((owner, repo, number)): Path<(String, String, u64)>,
    headers: HeaderMap,
) -> Result<Json<Vec<FileDiff>>, ForgeError> {
    blocking(&app, move |forge| {
        let principal = auth::authenticate(&forge.db, &headers)?;
        let record = forge.authorize(&owner, &repo, principal.as_ref(), Action::Read)?;
        let pr = pr::find(forge, &record, number)?;
        let git = forge.repos.open(&record.name)?;
        Ok(Json(pr::pr_diff(&git, &pr)?.1))
    })
    .await
}

async fn pull_readiness(
    State(app): State<App>,
    Path((owner, repo, number)): Path<(String, String, u64)>,
    headers: HeaderMap,
) -> Result<Json<MergeReadiness>, ForgeError> {
    blocking(&app, move |forge| {
        let principal = auth::authenticate(&forge.db, &headers)?;
        let record = forge.authorize(&owner, &repo, principal.as_ref(), Action::Read)?;
        let pr = pr::find(forge, &record, number)?;
        Ok(Json(crate::merge::readiness(forge, &record, &pr)?))
    })
    .await
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReviewEvent {
    Approve,
    RequestChanges,
    Comment,
}

#[derive(Deserialize)]
pub struct PostReview {
    pub event: ReviewEvent,
    #[serde(default)]
    pub body: String,
    pub commit_id: Option<String>,
}

#[derive(Serialize)]
pub struct ReviewJson {
    pub id: i64,
    pub user: String,
    pub state: Verdict,
    pub commit_id: String,
    pub body: String,
}

fn review_json(r: &Review) -> ReviewJson {
    ReviewJson {
        id: r.id,
        user: r.author.clone(),
        state: r.verdict,
        commit_id: r.head_oid.to_string(),
        body: r.body.clone(),
    }
}

async fn list_reviews(
    State(app): State<App>,
    Path((owner, repo, number)): Path<(String, String, u64)>,
    headers: HeaderMap,
) -> Result<Json<Vec<ReviewJson>>, ForgeError> {
    blocking(&app, move |forge| {
        let principal = auth::authenticate(&forge.db, &headers)?;
        let record = forge.authorize(&owner, &repo, principal.as_ref(), Action::Read)?;
        let pr = pr::find(forge, &record, number)?;
        let reviews = forge.db.reviews(pr.id)?;
        Ok(Json(reviews.iter().map(review_json).collect()))
    })
    .await
}

async fn post_review(
    State(app): State<App>,
    Path((owner, repo, number)): Path<(String, String, u64)>,
    headers: HeaderMap,
    Json(req): Json<PostReview>,
) -> Result<(StatusCode, Json<ReviewJson>), ForgeError> {
    blocking(&app, move |forge| {
        let principal = auth::authenticate(&forge.db, &headers)?;
        let record = forge.authorize(&owner, &repo, principal.as_ref(), Action::Contribute)?;
        let principal = signed_in(principal)?;
        let verdict = match req.event {
            ReviewEvent::Approve => Verdict::Approve,
            ReviewEvent::RequestChanges => Verdict::RequestChanges,
            ReviewEvent::Comment => Verdict::Comment,
        };
        let seen = req.commit_id.as_deref().map(parse_oid).transpose()?;
        let review = pr::review(forge, &record, number, &principal, verdict, &req.body, seen)?;
        Ok((StatusCode::CREATED, Json(review_json(&review))))
    })
    .await
}

#[derive(Deserialize)]
pub struct PostComment {
    pub path: String,
    #[serde(default = "default_side")]
    pub side: String,
    pub line: u32,
    pub body: String,
    pub commit_id: Option<String>,
}

fn default_side() -> String {
    "new".into()
}

#[derive(Serialize)]
pub struct CommentJson {
    pub id: i64,
    pub user: String,
    pub path: String,
    pub side: Side,
    pub line: u32,
    pub commit_id: String,
    pub body: String,
}

fn comment_json(c: &InlineComment) -> CommentJson {
    CommentJson {
        id: c.id,
        user: c.author.clone(),
        path: c.anchor.path.clone(),
        side: c.anchor.side,
        line: c.anchor.line,
        commit_id: c.anchor.head_oid.to_string(),
        body: c.body.clone(),
    }
}

async fn list_comments(
    State(app): State<App>,
    Path((owner, repo, number)): Path<(String, String, u64)>,
    headers: HeaderMap,
) -> Result<Json<Vec<CommentJson>>, ForgeError> {
    blocking(&app, move |forge| {
        let principal = auth::authenticate(&forge.db, &headers)?;
        let record = forge.authorize(&owner, &repo, principal.as_ref(), Action::Read)?;
        let pr = pr::find(forge, &record, number)?;
        let comments = forge.db.comments(pr.id)?;
        Ok(Json(comments.iter().map(comment_json).collect()))
    })
    .await
}

async fn post_comment(
    State(app): State<App>,
    Path((owner, repo, number)): Path<(String, String, u64)>,
    headers: HeaderMap,
    Json(req): Json<PostComment>,
) -> Result<(StatusCode, Json<CommentJson>), ForgeError> {
    blocking(&app, move |forge| {
        let principal = auth::authenticate(&forge.db, &headers)?;
        let record = forge.authorize(&owner, &repo, principal.as_ref(), Action::Contribute)?;
        let principal = signed_in(principal)?;
        let side = Side::parse(&req.side)
            .ok_or_else(|| ForgeError::BadRequest(format!("side {:?}", req.side)))?;
        let comment = pr::comment(
            forge,
            &record,
            number,
            &principal,
            NewComment {
                path: req.path,
                side,
                line: req.line,
                body: req.body,
                seen: req.commit_id.as_deref().map(parse_oid).transpose()?,
            },
        )?;
        Ok((StatusCode::CREATED, Json(comment_json(&comment))))
    })
    .await
}

#[derive(Deserialize, Default)]
pub struct MergeRequest {
    /// The head the caller expects to merge.
    pub sha: Option<String>,
}

async fn merge_pull(
    State(app): State<App>,
    Path((owner, repo, number)): Path<(String, String, u64)>,
    headers: HeaderMap,
    body: Option<Json<MergeRequest>>,
) -> Result<Json<PullJson>, ForgeError> {
    let req = body.map(|Json(r)| r).unwrap_or_default();
    blocking(&app, move |forge| {
        let principal = auth::authenticate(&forge.db, &headers)?;
        let record = forge.authorize(&owner, &repo, principal.as_ref(), Action::Merge)?;
        let principal = signed_in(principal)?;
        let expected = req.sha.as_deref().map(parse_oid).transpose()?;
        let pr = crate::merge::merge(forge, &record, number, &principal, expected)?;
        Ok(Json(pull_json(forge, &record, &pr)))
    })
    .await
}

async fn close_pull(
    State(app): State<App>,
    Path((owner, repo, number)): Path<(String, String, u64)>,
    headers: HeaderMap,
) -> Result<Json<PullJson>, ForgeError> {
    blocking(&app, move |forge| {
        let principal = auth::authenticate(&forge.db, &headers)?;
        let record = forge.authorize(&owner, &repo, principal.as_ref(), Action::Contribute)?;
        let principal = signed_in(principal)?;
        let pr = pr::close(forge, &record, number, &principal)?;
        Ok(Json(pull_json(forge, &record, &pr)))
    })
    .await
}

#[derive(Deserialize)]
pub struct PostStatus {
    pub state: String,
    pub context: String,
    pub target_url: Option<String>,
    pub description: Option<String>,
}

async fn post_status(
    State(app): State<App>,
    Path((owner, repo, oid)): Path<(String, String, String)>,
    headers: HeaderMap,
    Json(req): Json<PostStatus>,
) -> Result<(StatusCode, Json<CheckStatus>), ForgeError> {
    blocking(&app, move |forge| {
        let principal = auth::authenticate(&forge.db, &headers)?;
        let record = forge.authorize(&owner, &repo, principal.as_ref(), Action::PostStatus)?;
        let principal = signed_in(principal)?;
        let state = CheckState::parse(&req.state).ok_or_else(|| {
            ForgeError::BadRequest(format!("unknown status state {:?}", req.state))
        })?;
        let status = pr::post_status(
            forge,
            &record,
            parse_oid(&oid)?,
            &principal,
            NewStatus {
                state,
                context: req.context,
                target_url: req.target_url,
                description: req.description,
            },
        )?;
        Ok((StatusCode::CREATED, Json(status)))
    })
    .await
}

async fn list_statuses(
    State(app): State<App>,
    Path((owner, repo, oid)): Path<(String, String, String)>,
    headers: HeaderMap,
) -> Result<Json<Vec<CheckStatus>>, ForgeError> {
    blocking(&app, move |forge| {
        let principal = auth::authenticate(&forge.db, &headers)?;
        let record = forge.authorize(&owner, &repo, principal.as_ref(), Action::Read)?;
        Ok(Json(forge.db.latest_statuses(record.id, parse_oid(&oid)?)?))
    })
    .await
}
