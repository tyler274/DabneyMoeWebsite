//! The merge gate: one rule ([`MergeReadiness::evaluate`]) decides whether a
//! pull request may merge, and [`merge`] performs the 3-way merge in pure
//! Rust and moves the target branch only if it still points where the
//! decision saw it.

use std::collections::BTreeMap;

use gix::merge::tree::TreatAsUnresolved;
use gix::refs::transaction::{Change, LogChange, PreviousValue, RefEdit};
use gix::refs::Target;
use gix::ObjectId;
use serde::Serialize;

use crate::app::Forge;
use crate::auth::Principal;
use crate::db::{CheckState, CheckStatus, PrState, PullRequest, RepoRecord, Review, Verdict};
use crate::error::ForgeError;
use crate::git::http::{edit_refs, forge_signature};
use crate::repo::{self, git};

/// Something standing between a pull request and its merge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Blocker {
    NotOpen { state: &'static str },
    TargetMissing { branch: String },
    NeedsApprovals { have: u32, need: u32 },
    ChangesRequested { by: Vec<String> },
    CheckMissing { context: String },
    CheckNotPassing { context: String, state: CheckState },
    Conflicts { paths: Vec<String> },
    NoCommonAncestor,
    NothingToMerge,
}

impl std::fmt::Display for Blocker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Blocker::NotOpen { state } => write!(f, "pull request is {state}"),
            Blocker::TargetMissing { branch } => write!(f, "target branch {branch} does not exist"),
            Blocker::NeedsApprovals { have, need } => {
                write!(f, "{have} of {need} required approvals on the current head")
            }
            Blocker::ChangesRequested { by } => write!(f, "changes requested by {}", by.join(", ")),
            Blocker::CheckMissing { context } => {
                write!(f, "required check {context} has not reported")
            }
            Blocker::CheckNotPassing { context, state } => {
                write!(f, "check {context} is {}", state.as_str())
            }
            Blocker::Conflicts { paths } => write!(f, "merge conflicts in {}", paths.join(", ")),
            Blocker::NoCommonAncestor => {
                write!(f, "source and target branches have no common ancestor")
            }
            Blocker::NothingToMerge => {
                write!(f, "head commit is already included in the target branch")
            }
        }
    }
}

/// What a trial merge of head into the target found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeProbe {
    Clean,
    Conflicted(Vec<String>),
    /// Source and target branches share no common ancestor.
    UnrelatedHistories,
    /// The head commit is equal to or already an ancestor of the target tip.
    AlreadyInTarget,
    /// No trial was possible (not open, or no target branch).
    NotAttempted,
}

fn ser_oid<S: serde::Serializer>(id: &ObjectId, s: S) -> Result<S::Ok, S::Error> {
    s.collect_str(id)
}

fn ser_opt_oid<S: serde::Serializer>(id: &Option<ObjectId>, s: S) -> Result<S::Ok, S::Error> {
    match id {
        Some(id) => s.collect_str(id),
        None => s.serialize_none(),
    }
}

/// Whether a pull request may merge, and why not.
#[derive(Debug, Clone, Serialize)]
pub struct MergeReadiness {
    #[serde(serialize_with = "ser_oid")]
    pub head: ObjectId,
    #[serde(serialize_with = "ser_opt_oid")]
    pub target: Option<ObjectId>,
    /// Reviewers whose approval is on the current head.
    pub approvals: Vec<String>,
    /// Reviewers whose latest approval is on an earlier head.
    pub stale_approvals: Vec<String>,
    pub blockers: Vec<Blocker>,
}

/// The facts the merge rule reads.
pub struct Facts<'a> {
    pub repo: &'a RepoRecord,
    pub pr: &'a PullRequest,
    pub target_tip: Option<ObjectId>,
    /// All reviews, oldest first.
    pub reviews: &'a [Review],
    /// The latest status per context on the pull request's head.
    pub statuses: &'a [CheckStatus],
    pub probe: &'a MergeProbe,
}

impl MergeReadiness {
    /// The merge rule. A review speaks only for the head it saw: each
    /// reviewer's latest approve or request-changes counts if it is on the
    /// current head, so a new head makes earlier approvals stale. Authors
    /// cannot review their own pull requests. Required checks must have
    /// succeeded on the head, and no reported check may be failing.
    pub fn evaluate(facts: Facts<'_>) -> Self {
        let Facts {
            repo,
            pr,
            target_tip,
            reviews,
            statuses,
            probe,
        } = facts;
        let mut blockers = Vec::new();
        if pr.state != PrState::Open {
            blockers.push(Blocker::NotOpen {
                state: pr.state.label(),
            });
        }
        if target_tip.is_none() {
            blockers.push(Blocker::TargetMissing {
                branch: pr.target_branch.clone(),
            });
        }
        let mut latest: BTreeMap<&str, &Review> = BTreeMap::new();
        for review in reviews {
            if review.author != pr.author && review.verdict != Verdict::Comment {
                latest.insert(&review.author, review);
            }
        }
        let mut approvals = Vec::new();
        let mut stale_approvals = Vec::new();
        let mut requesters = Vec::new();
        for (author, review) in latest {
            let current = review.head_oid == pr.head_oid;
            match (review.verdict, current) {
                (Verdict::Approve, true) => approvals.push(author.to_owned()),
                (Verdict::Approve, false) => stale_approvals.push(author.to_owned()),
                (Verdict::RequestChanges, true) => requesters.push(author.to_owned()),
                _ => {}
            }
        }
        let have = approvals.len() as u32;
        if have < repo.required_approvals {
            blockers.push(Blocker::NeedsApprovals {
                have,
                need: repo.required_approvals,
            });
        }
        if !requesters.is_empty() {
            blockers.push(Blocker::ChangesRequested { by: requesters });
        }
        for context in &repo.required_checks {
            match statuses.iter().find(|s| &s.context == context) {
                None => blockers.push(Blocker::CheckMissing {
                    context: context.clone(),
                }),
                Some(s) if s.state != CheckState::Success => {
                    blockers.push(Blocker::CheckNotPassing {
                        context: context.clone(),
                        state: s.state,
                    })
                }
                Some(_) => {}
            }
        }
        for s in statuses {
            let required = repo.required_checks.contains(&s.context);
            if !required && matches!(s.state, CheckState::Failure | CheckState::Error) {
                blockers.push(Blocker::CheckNotPassing {
                    context: s.context.clone(),
                    state: s.state,
                });
            }
        }
        match probe {
            MergeProbe::Conflicted(paths) => blockers.push(Blocker::Conflicts {
                paths: paths.clone(),
            }),
            MergeProbe::UnrelatedHistories => blockers.push(Blocker::NoCommonAncestor),
            MergeProbe::AlreadyInTarget => blockers.push(Blocker::NothingToMerge),
            MergeProbe::Clean | MergeProbe::NotAttempted => {}
        }
        MergeReadiness {
            head: pr.head_oid,
            target: target_tip,
            approvals,
            stale_approvals,
            blockers,
        }
    }

    pub fn is_ready(&self) -> bool {
        self.blockers.is_empty()
    }

    pub fn summary(&self) -> String {
        self.blockers
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ")
    }
}

fn unresolved_paths(outcome: &gix::merge::tree::Outcome<'_>) -> Vec<String> {
    let how = TreatAsUnresolved::git();
    let mut paths: Vec<String> = outcome
        .conflicts
        .iter()
        .filter(|c| c.is_unresolved(how))
        .map(|c| c.ours.location().to_string())
        .collect();
    paths.sort();
    paths.dedup();
    paths
}

fn merge_labels() -> gix::merge::blob::builtin_driver::text::Labels<'static> {
    gix::merge::blob::builtin_driver::text::Labels {
        ancestor: Some("base".into()),
        current: Some("target".into()),
        other: Some("head".into()),
    }
}

/// Trial-merge `head` into `target` without writing objects to disk.
pub fn probe_merge(
    repo: &gix::Repository,
    target: ObjectId,
    head: ObjectId,
) -> Result<MergeProbe, ForgeError> {
    match repo.merge_base(target, head).map_err(git)? {
        None => return Ok(MergeProbe::UnrelatedHistories),
        Some(base) if base.detach() == head => return Ok(MergeProbe::AlreadyInTarget),
        Some(_) => {}
    }
    let repo = repo.clone().with_object_memory();
    let options = repo.tree_merge_options().map_err(git)?;
    let outcome = repo
        .merge_commits(target, head, merge_labels(), options.into())
        .map_err(git)?;
    let paths = unresolved_paths(&outcome.tree_merge);
    Ok(
        if outcome
            .tree_merge
            .has_unresolved_conflicts(TreatAsUnresolved::git())
        {
            MergeProbe::Conflicted(paths)
        } else {
            MergeProbe::Clean
        },
    )
}

/// Gather the facts for `pr` and apply the merge rule.
pub fn readiness(
    forge: &Forge,
    record: &RepoRecord,
    pr: &PullRequest,
) -> Result<MergeReadiness, ForgeError> {
    let repo = forge.repos.open(&record.name)?;
    let target_tip = repo::branch_tip(&repo, &pr.target_branch)?;
    let probe = match (&pr.state, target_tip) {
        (PrState::Open, Some(target)) => probe_merge(&repo, target, pr.head_oid)?,
        _ => MergeProbe::NotAttempted,
    };
    let reviews = forge.db.reviews(pr.id)?;
    let statuses = forge.db.latest_statuses(record.id, pr.head_oid)?;
    Ok(MergeReadiness::evaluate(Facts {
        repo: record,
        pr,
        target_tip,
        reviews: &reviews,
        statuses: &statuses,
        probe: &probe,
    }))
}

/// Merge pull request `number` with a merge commit, if the merge rule allows
/// it. `expected_head`, when given, must still be the pull request's head.
pub fn merge(
    forge: &Forge,
    record: &RepoRecord,
    number: u64,
    merger: &Principal,
    expected_head: Option<ObjectId>,
) -> Result<PullRequest, ForgeError> {
    let lock = forge.repo_lock(record.id);
    let _guard = lock.lock().unwrap_or_else(|p| p.into_inner());
    let pr = crate::pr::find(forge, record, number)?;
    if expected_head.is_some_and(|e| e != pr.head_oid) {
        return Err(ForgeError::Conflict(format!(
            "head is now {}, not the expected commit",
            pr.head_oid
        )));
    }
    let readiness = readiness(forge, record, &pr)?;
    if !readiness.is_ready() {
        return Err(ForgeError::Conflict(format!(
            "not mergeable: {}",
            readiness.summary()
        )));
    }
    let target_tip = readiness.target.expect("ready implies a target tip");
    let repo = forge.repos.open(&record.name)?;
    let options = repo.tree_merge_options().map_err(git)?;
    let mut outcome = repo
        .merge_commits(target_tip, pr.head_oid, merge_labels(), options.into())
        .map_err(git)?;
    if outcome
        .tree_merge
        .has_unresolved_conflicts(TreatAsUnresolved::git())
    {
        let paths = unresolved_paths(&outcome.tree_merge);
        return Err(ForgeError::Conflict(format!(
            "merge conflicts in {}",
            paths.join(", ")
        )));
    }
    let tree = outcome.tree_merge.tree.write().map_err(git)?.detach();
    let mut author = forge_signature();
    author.name = merger.user.as_str().into();
    author.email = format!("{}@users.dabney.moe", merger.user).into();
    let commit = gix::objs::Commit {
        tree,
        parents: vec![target_tip, pr.head_oid].into(),
        author,
        committer: forge_signature(),
        encoding: None,
        message: format!(
            "Merge pull request #{} from {}\n\n{}\n",
            pr.number, pr.source_branch, pr.title
        )
        .into(),
        extra_headers: Vec::new(),
    };
    let merged = repo.write_object(&commit).map_err(git)?.detach();
    let target_ref = format!("refs/heads/{}", pr.target_branch);
    let edit = RefEdit::new(
        gix::refs::FullName::try_from(target_ref.as_str())
            .map_err(|_| ForgeError::InvalidName(target_ref.clone()))?,
        Change::Update {
            log: LogChange {
                message: format!("merge pull request #{}", pr.number).into(),
                ..Default::default()
            },
            expected: PreviousValue::MustExistAndMatch(Target::Object(target_tip)),
            new: Target::Object(merged),
        },
    );
    edit_refs(&repo, vec![edit]).map_err(|e| {
        ForgeError::Conflict(format!(
            "{target_ref} moved during the merge; retry ({})",
            e.chain()
        ))
    })?;
    let mirror_trigger =
        (pr.target_branch == record.default_branch).then(|| format!("merge #{}", pr.number));
    let Some((merged_pr, deliveries, mirrors)) = forge.db.commit_merge(
        pr.id,
        record.id,
        pr.head_oid,
        merged,
        mirror_trigger.as_deref(),
        |merged_pr| {
            crate::webhooks::prepare(forge, record, merged_pr, crate::webhooks::PrAction::Closed)
        },
    )?
    else {
        tracing::error!(
            pr = pr.number,
            %merged,
            "target ref updated but pull request was no longer open at its head"
        );
        return Err(ForgeError::Conflict(format!(
            "pull request #{} changed state before merge could be recorded",
            pr.number
        )));
    };
    if deliveries > 0 {
        forge.hooks_ready.notify_one();
    }
    if mirrors > 0 {
        forge.mirrors_ready.notify_one();
    }
    tracing::info!(repo = %record.name, pr = pr.number, %merged, by = %merger.user, "merged pull request");
    Ok(merged_pr)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::RepoName;

    fn oid(byte: u8) -> ObjectId {
        ObjectId::from_bytes_or_panic(&[byte; 20])
    }

    fn repo() -> RepoRecord {
        RepoRecord {
            id: 1,
            name: RepoName::new("tyler", "site").unwrap(),
            private: false,
            default_branch: "main".into(),
            required_approvals: 1,
            required_checks: vec!["ci/test".into()],
        }
    }

    fn pr(head: ObjectId) -> PullRequest {
        PullRequest {
            id: 1,
            repo_id: 1,
            number: 1,
            title: "COD-103: forge".into(),
            body: String::new(),
            author: "tyler".into(),
            source_branch: "feature".into(),
            target_branch: "main".into(),
            base_oid: oid(9),
            head_oid: head,
            state: PrState::Open,
            created_at: 0,
            updated_at: 0,
        }
    }

    fn review(id: i64, author: &str, verdict: Verdict, head: ObjectId) -> Review {
        Review {
            id,
            author: author.into(),
            verdict,
            head_oid: head,
            body: String::new(),
            created_at: 0,
        }
    }

    fn passing() -> Vec<CheckStatus> {
        vec![CheckStatus {
            context: "ci/test".into(),
            state: CheckState::Success,
            target_url: None,
            description: None,
            creator: "ci".into(),
            created_at: 0,
        }]
    }

    fn eval(
        pr: &PullRequest,
        reviews: &[Review],
        statuses: &[CheckStatus],
        probe: MergeProbe,
    ) -> MergeReadiness {
        MergeReadiness::evaluate(Facts {
            repo: &repo(),
            pr,
            target_tip: Some(oid(9)),
            reviews,
            statuses,
            probe: &probe,
        })
    }

    #[test]
    fn approval_on_current_head_with_passing_checks_is_ready() {
        let p = pr(oid(1));
        let r = eval(
            &p,
            &[review(1, "bob", Verdict::Approve, oid(1))],
            &passing(),
            MergeProbe::Clean,
        );
        assert!(r.is_ready(), "{:?}", r.blockers);
        assert_eq!(r.approvals, vec!["bob".to_owned()]);
    }

    #[test]
    fn a_new_head_makes_approval_stale() {
        let p = pr(oid(2));
        let r = eval(
            &p,
            &[review(1, "bob", Verdict::Approve, oid(1))],
            &passing(),
            MergeProbe::Clean,
        );
        assert_eq!(
            r.blockers,
            vec![Blocker::NeedsApprovals { have: 0, need: 1 }]
        );
        assert_eq!(r.stale_approvals, vec!["bob".to_owned()]);
    }

    #[test]
    fn self_approval_comments_and_superseded_reviews() {
        let p = pr(oid(1));
        let reviews = [
            review(1, "tyler", Verdict::Approve, oid(1)),
            review(2, "bob", Verdict::Approve, oid(1)),
            review(3, "bob", Verdict::RequestChanges, oid(1)),
            review(4, "bob", Verdict::Comment, oid(1)),
        ];
        let r = eval(&p, &reviews, &passing(), MergeProbe::Clean);
        assert!(r.approvals.is_empty());
        assert!(r.blockers.contains(&Blocker::ChangesRequested {
            by: vec!["bob".into()]
        }));
    }

    #[test]
    fn checks_and_conflicts_block() {
        let p = pr(oid(1));
        let approved = [review(1, "bob", Verdict::Approve, oid(1))];
        let r = eval(&p, &approved, &[], MergeProbe::Clean);
        assert_eq!(
            r.blockers,
            vec![Blocker::CheckMissing {
                context: "ci/test".into()
            }]
        );
        let mut failing = passing();
        failing[0].state = CheckState::Failure;
        failing.push(CheckStatus {
            context: "lint".into(),
            state: CheckState::Error,
            ..passing().remove(0)
        });
        let r = eval(
            &p,
            &approved,
            &failing,
            MergeProbe::Conflicted(vec!["a.txt".into()]),
        );
        assert_eq!(r.blockers.len(), 3, "{:?}", r.blockers);
        let mut closed = p.clone();
        closed.state = PrState::Closed;
        let r = eval(&closed, &approved, &passing(), MergeProbe::NotAttempted);
        assert_eq!(r.blockers, vec![Blocker::NotOpen { state: "closed" }]);
    }
}
