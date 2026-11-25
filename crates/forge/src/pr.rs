//! Pull requests: opening, following pushes, OID-anchored reviews and
//! inline comments, commit statuses and closing.

use gix::refs::transaction::{Change, LogChange, PreviousValue, RefEdit};
use gix::refs::Target;
use gix::ObjectId;

use crate::app::Forge;
use crate::auth::Principal;
use crate::db::{
    Anchor, CheckState, CheckStatus, InlineComment, PrState, PullRequest, RepoRecord, Review,
    Verdict,
};
use crate::diff::{self, FileDiff};
use crate::error::ForgeError;
use crate::git::http::{edit_refs, PushCommand};
use crate::merge::{self, MergeReadiness};
use crate::repo::{self, git};
use crate::webhooks::{self, PrAction};

pub fn find(forge: &Forge, record: &RepoRecord, number: u64) -> Result<PullRequest, ForgeError> {
    forge
        .db
        .pr(record.id, number)?
        .ok_or_else(|| ForgeError::NotFound(format!("pull request #{number}")))
}

fn require_open(pr: &PullRequest) -> Result<(), ForgeError> {
    match pr.state {
        PrState::Open => Ok(()),
        _ => Err(ForgeError::Conflict(format!(
            "pull request #{} is {}",
            pr.number,
            pr.state.label()
        ))),
    }
}

/// Point `refs/pull/<n>/head` at the pull request's head.
fn set_pull_ref(repo: &gix::Repository, number: u64, head: ObjectId) -> Result<(), ForgeError> {
    let name = format!("refs/pull/{number}/head");
    let edit = RefEdit::new(
        gix::refs::FullName::try_from(name.as_str())
            .map_err(|_| ForgeError::InvalidName(name.clone()))?,
        Change::Update {
            log: LogChange {
                message: "pull request head".into(),
                ..Default::default()
            },
            expected: PreviousValue::Any,
            new: Target::Object(head),
        },
    );
    edit_refs(repo, vec![edit])
}

pub struct NewPullRequest {
    pub title: String,
    pub body: String,
    /// Source branch in the same repository.
    pub head: String,
    /// Target branch; the default branch when absent.
    pub base: Option<String>,
}

pub fn open(
    forge: &Forge,
    record: &RepoRecord,
    author: &Principal,
    new: NewPullRequest,
) -> Result<PullRequest, ForgeError> {
    let title = new.title.trim();
    if title.is_empty() {
        return Err(ForgeError::BadRequest(
            "a pull request needs a title".into(),
        ));
    }
    let base = new.base.unwrap_or_else(|| record.default_branch.clone());
    if base == new.head {
        return Err(ForgeError::BadRequest(
            "source and target branch are the same".into(),
        ));
    }
    let lock = forge.repo_lock(record.id);
    let _guard = lock.lock().unwrap_or_else(|p| p.into_inner());
    let repo = forge.repos.open(&record.name)?;
    let head = repo::branch_tip(&repo, &new.head)?
        .ok_or_else(|| ForgeError::NotFound(format!("branch {}", new.head)))?;
    let target = repo::branch_tip(&repo, &base)?
        .ok_or_else(|| ForgeError::NotFound(format!("branch {base}")))?;
    match diff::fork_point(&repo, target, head)? {
        None => {
            return Err(ForgeError::Unprocessable(format!(
                "{} and {base} have no common ancestor",
                new.head
            )));
        }
        Some(base_id) if base_id == head => {
            return Err(ForgeError::BadRequest(format!(
                "{} has no commits that are not on {base}",
                new.head
            )));
        }
        Some(_) => {}
    }
    let pr = forge.db.insert_pr(
        record.id,
        title,
        &new.body,
        &author.user,
        &new.head,
        &base,
        target,
        head,
    )?;
    set_pull_ref(&repo, pr.number, head)?;
    tracing::info!(repo = %record.name, pr = pr.number, by = %author.user, "opened pull request");
    webhooks::emit(forge, record, &pr, PrAction::Opened)?;
    Ok(pr)
}

/// Follow a push: open pull requests from an updated branch move to its new
/// head (which makes earlier approvals stale), and a moved default branch or
/// tag is queued for the mirrors. The caller holds the repository lock.
pub fn after_push(
    forge: &Forge,
    record: &RepoRecord,
    updated: &[PushCommand],
) -> Result<(), ForgeError> {
    let repo = forge.repos.open(&record.name)?;
    let default_ref = format!("refs/heads/{}", record.default_branch);
    let mut mirror = false;
    for command in updated {
        mirror |= command.name == default_ref || command.name.starts_with("refs/tags/");
        let Some(branch) = command.name.strip_prefix("refs/heads/") else {
            continue;
        };
        if command.is_delete() {
            continue;
        }
        for pr in forge.db.open_prs_from_branch(record.id, branch)? {
            if pr.head_oid == command.new {
                continue;
            }
            if !forge.db.set_pr_head(pr.id, command.new)? {
                continue;
            }
            set_pull_ref(&repo, pr.number, command.new)?;
            let pr = forge.db.pr_by_id(pr.id)?;
            tracing::info!(repo = %record.name, pr = pr.number, head = %command.new, "pull request head moved");
            webhooks::emit(forge, record, &pr, PrAction::Synchronized)?;
        }
    }
    if mirror {
        crate::mirror::queue(forge, record, "push")?;
    }
    Ok(())
}

/// A review must name the head it saw when it names one.
fn check_seen_head(pr: &PullRequest, seen: Option<ObjectId>) -> Result<ObjectId, ForgeError> {
    match seen {
        Some(seen) if seen != pr.head_oid => Err(ForgeError::Conflict(format!(
            "pull request head is now {}, not {seen}",
            pr.head_oid
        ))),
        _ => Ok(pr.head_oid),
    }
}

pub fn review(
    forge: &Forge,
    record: &RepoRecord,
    number: u64,
    reviewer: &Principal,
    verdict: Verdict,
    body: &str,
    seen: Option<ObjectId>,
) -> Result<Review, ForgeError> {
    let lock = forge.repo_lock(record.id);
    let _guard = lock.lock().unwrap_or_else(|p| p.into_inner());
    let pr = find(forge, record, number)?;
    require_open(&pr)?;
    if verdict != Verdict::Comment && reviewer.user == pr.author {
        return Err(ForgeError::Forbidden(
            "authors cannot approve or reject their own pull request".into(),
        ));
    }
    let head = check_seen_head(&pr, seen)?;
    let id = forge
        .db
        .insert_review(pr.id, &reviewer.user, verdict, head, body)?;
    let review = forge
        .db
        .reviews(pr.id)?
        .into_iter()
        .find(|r| r.id == id)
        .ok_or_else(|| ForgeError::NotFound(format!("review {id}")))?;
    Ok(review)
}

pub struct NewComment {
    pub path: String,
    pub side: crate::db::Side,
    pub line: u32,
    pub body: String,
    pub seen: Option<ObjectId>,
}

/// Comment on one line of the current diff. The comment is anchored to the
/// head it was written against and is shown as outdated after a new push.
pub fn comment(
    forge: &Forge,
    record: &RepoRecord,
    number: u64,
    author: &Principal,
    new: NewComment,
) -> Result<InlineComment, ForgeError> {
    let lock = forge.repo_lock(record.id);
    let _guard = lock.lock().unwrap_or_else(|p| p.into_inner());
    let pr = find(forge, record, number)?;
    require_open(&pr)?;
    if new.body.trim().is_empty() {
        return Err(ForgeError::BadRequest("a comment needs a body".into()));
    }
    let head = check_seen_head(&pr, new.seen)?;
    let repo = forge.repos.open(&record.name)?;
    let (_, files) = pr_diff(&repo, &pr)?;
    let on_diff = files
        .iter()
        .find(|f| f.path == new.path)
        .is_some_and(|f| f.has_line(new.side, new.line));
    if !on_diff {
        return Err(ForgeError::BadRequest(format!(
            "{}:{} ({}) is not a line of this diff",
            new.path,
            new.line,
            new.side.as_str()
        )));
    }
    let anchor = Anchor {
        head_oid: head,
        path: new.path,
        side: new.side,
        line: new.line,
    };
    let id = forge
        .db
        .insert_comment(pr.id, &author.user, &anchor, &new.body)?;
    forge
        .db
        .comments(pr.id)?
        .into_iter()
        .find(|c| c.id == id)
        .ok_or_else(|| ForgeError::NotFound(format!("comment {id}")))
}

pub fn close(
    forge: &Forge,
    record: &RepoRecord,
    number: u64,
    by: &Principal,
) -> Result<PullRequest, ForgeError> {
    let lock = forge.repo_lock(record.id);
    let _guard = lock.lock().unwrap_or_else(|p| p.into_inner());
    let pr = find(forge, record, number)?;
    require_open(&pr)?;
    let Some((closed, deliveries)) = forge.db.commit_close(pr.id, |closed| {
        webhooks::prepare(forge, record, closed, PrAction::Closed)
    })?
    else {
        return Err(ForgeError::Conflict(format!(
            "pull request #{number} is no longer open"
        )));
    };
    if deliveries > 0 {
        forge.hooks_ready.notify_one();
    }
    tracing::info!(repo = %record.name, pr = number, by = %by.user, "closed pull request");
    Ok(closed)
}

/// The single rule for safe external URLs stored and linked from the UI:
/// only `http://` or `https://` with a host.
pub fn is_safe_http_url(raw: &str) -> bool {
    url::Url::parse(raw)
        .is_ok_and(|u| matches!(u.scheme(), "http" | "https") && u.host_str().is_some())
}

pub struct NewStatus {
    pub state: CheckState,
    pub context: String,
    pub target_url: Option<String>,
    pub description: Option<String>,
}

/// Record a CI status on a commit of this repository.
pub fn post_status(
    forge: &Forge,
    record: &RepoRecord,
    commit: ObjectId,
    creator: &Principal,
    new: NewStatus,
) -> Result<CheckStatus, ForgeError> {
    let context = new.context.trim();
    if context.is_empty() || context.len() > 255 {
        return Err(ForgeError::BadRequest(
            "status context must be 1-255 characters".into(),
        ));
    }
    let target_url = match new.target_url {
        Some(raw) => {
            let trimmed = raw.trim();
            if !is_safe_http_url(trimmed) {
                return Err(ForgeError::BadRequest(
                    "target_url must be a valid http(s) URL with a host".into(),
                ));
            }
            Some(trimmed.to_owned())
        }
        None => None,
    };
    let repo = forge.repos.open(&record.name)?;
    crate::git::pack::require_commit(&repo, commit)
        .map_err(|_| ForgeError::NotFound(format!("commit {commit}")))?;
    let status = CheckStatus {
        context: context.to_owned(),
        state: new.state,
        target_url,
        description: new.description,
        creator: creator.user.clone(),
        created_at: crate::db::now(),
    };
    forge.db.insert_status(record.id, commit, &status)?;
    Ok(status)
}

/// The commit the pull request's diff starts from (`None` if unrelated), and
/// the diff.
pub fn pr_diff(
    repo: &gix::Repository,
    pr: &PullRequest,
) -> Result<(Option<ObjectId>, Vec<FileDiff>), ForgeError> {
    let target = match &pr.state {
        PrState::Merged { commit } => repo
            .find_commit(*commit)
            .map_err(git)?
            .parent_ids()
            .next()
            .map(|id| id.detach())
            .unwrap_or(pr.base_oid),
        _ => repo::branch_tip(repo, &pr.target_branch)?.unwrap_or(pr.base_oid),
    };
    let base = diff::fork_point(repo, target, pr.head_oid)?;
    Ok((base, diff::diff_commits(repo, base, pr.head_oid)?))
}

/// Everything a pull request page shows.
pub struct PrView {
    pub pr: PullRequest,
    pub base: Option<ObjectId>,
    pub files: Vec<FileDiff>,
    pub reviews: Vec<Review>,
    pub comments: Vec<InlineComment>,
    pub statuses: Vec<CheckStatus>,
    pub readiness: MergeReadiness,
}

pub fn view(forge: &Forge, record: &RepoRecord, number: u64) -> Result<PrView, ForgeError> {
    let pr = find(forge, record, number)?;
    let repo = forge.repos.open(&record.name)?;
    let (base, files) = pr_diff(&repo, &pr)?;
    Ok(PrView {
        base,
        files,
        reviews: forge.db.reviews(pr.id)?,
        comments: forge.db.comments(pr.id)?,
        statuses: forge.db.latest_statuses(record.id, pr.head_oid)?,
        readiness: merge::readiness(forge, record, &pr)?,
        pr,
    })
}
