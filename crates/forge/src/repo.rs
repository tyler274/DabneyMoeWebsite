//! Bare repositories on disk and read-only browsing, all through gix.

use std::path::{Path, PathBuf};

use gix::bstr::ByteSlice;
use gix::ObjectId;
use serde::Serialize;

use crate::error::ForgeError;

/// A lowercase path segment: `[a-z0-9][a-z0-9._-]{0,62}`, not `.git`-suffixed.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub struct Slug(String);

impl Slug {
    pub fn parse(raw: &str) -> Result<Self, ForgeError> {
        let ok = (1..=63).contains(&raw.len())
            && raw.as_bytes()[0].is_ascii_alphanumeric()
            && raw.bytes().all(|b| {
                b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_' | b'.')
            })
            && !raw.contains("..")
            && !raw.ends_with(".git");
        if ok {
            Ok(Slug(raw.to_owned()))
        } else {
            Err(ForgeError::InvalidName(raw.to_owned()))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Slug {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// `owner/name`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub struct RepoName {
    pub owner: Slug,
    pub name: Slug,
}

impl RepoName {
    pub fn new(owner: &str, name: &str) -> Result<Self, ForgeError> {
        Ok(RepoName {
            owner: Slug::parse(owner)?,
            name: Slug::parse(name.strip_suffix(".git").unwrap_or(name))?,
        })
    }

    pub fn parse(full: &str) -> Result<Self, ForgeError> {
        let (owner, name) = full
            .split_once('/')
            .ok_or_else(|| ForgeError::InvalidName(full.to_owned()))?;
        Self::new(owner, name)
    }
}

impl std::fmt::Display for RepoName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.owner, self.name)
    }
}

/// A full ref name a client may advertise or update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefTip {
    pub name: String,
    pub target: ObjectId,
    /// The object an annotated tag ultimately points at.
    pub peeled: Option<ObjectId>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CommitSummary {
    pub id: String,
    pub short: String,
    pub summary: String,
    pub author: String,
    pub time: String,
}

#[derive(Debug, Clone, Serialize)]
pub enum TreeEntryKind {
    Tree,
    Blob,
    Link,
    Submodule,
}

#[derive(Debug, Clone, Serialize)]
pub struct TreeEntry {
    pub name: String,
    pub kind: TreeEntryKind,
    pub id: String,
}

#[derive(Clone)]
pub struct RepoStore {
    root: PathBuf,
}

pub(crate) fn git(e: impl Into<gix::Error>) -> ForgeError {
    ForgeError::Git(e.into())
}

impl RepoStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        RepoStore { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn path(&self, name: &RepoName) -> PathBuf {
        self.root
            .join(name.owner.as_str())
            .join(format!("{}.git", name.name))
    }

    /// Create an empty bare repository whose `HEAD` names `default_branch`.
    pub fn create(&self, name: &RepoName, default_branch: &str) -> Result<(), ForgeError> {
        let path = self.path(name);
        if path.exists() {
            return Err(ForgeError::Conflict(format!(
                "repository {name} already exists"
            )));
        }
        std::fs::create_dir_all(&path).map_err(ForgeError::Io)?;
        gix::init_bare(&path).map_err(git)?;
        std::fs::write(
            path.join("HEAD"),
            format!("ref: refs/heads/{default_branch}\n"),
        )
        .map_err(ForgeError::Io)?;
        Ok(())
    }

    /// Open without consulting user or system Git configuration.
    pub fn open(&self, name: &RepoName) -> Result<gix::Repository, ForgeError> {
        let path = self.path(name);
        if !path.is_dir() {
            return Err(ForgeError::NotFound(format!("repository {name}")));
        }
        let mut repo = gix::open_opts(&path, gix::open::Options::isolated()).map_err(git)?;
        repo.object_cache_size_if_unset(8 * 1024 * 1024);
        Ok(repo)
    }
}

/// All refs, with annotated tags peeled.
pub fn list_refs(repo: &gix::Repository) -> Result<Vec<RefTip>, ForgeError> {
    let mut out = Vec::new();
    let platform = repo.references().map_err(git)?;
    for reference in platform.all().map_err(git)? {
        let mut reference = reference.map_err(git)?;
        let name = reference.name().as_bstr().to_str_lossy().into_owned();
        let Some(target) = reference.target().try_id().map(ToOwned::to_owned) else {
            continue; // symbolic refs other than HEAD are not advertised
        };
        let peeled = reference.peel_to_id().map_err(git)?.detach();
        out.push(RefTip {
            name,
            target,
            peeled: (peeled != target).then_some(peeled),
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// `HEAD`'s symbolic target, e.g. `refs/heads/main`.
pub fn head_symref(repo: &gix::Repository) -> Option<String> {
    let head = repo.head_name().ok().flatten()?;
    Some(head.as_bstr().to_str_lossy().into_owned())
}

/// The object a direct ref names, unpeeled; `None` if the ref is absent.
pub fn resolve_ref_target(
    repo: &gix::Repository,
    name: &str,
) -> Result<Option<ObjectId>, ForgeError> {
    let Some(reference) = repo.try_find_reference(name).map_err(git)? else {
        return Ok(None);
    };
    Ok(reference.target().try_id().map(ToOwned::to_owned))
}

pub fn resolve_ref(repo: &gix::Repository, name: &str) -> Result<Option<ObjectId>, ForgeError> {
    let Some(mut reference) = repo.try_find_reference(name).map_err(git)? else {
        return Ok(None);
    };
    Ok(Some(reference.peel_to_id().map_err(git)?.detach()))
}

/// Validate a Git branch name (e.g. `main` or `feature/cod-103`) before
/// constructing `refs/heads/<branch>`.
pub fn validate_branch_name(branch: &str) -> Result<String, ForgeError> {
    if branch.is_empty() || branch.starts_with('/') || branch.ends_with('/') {
        return Err(ForgeError::InvalidName(branch.to_owned()));
    }
    let full = format!("refs/heads/{branch}");
    gix::refs::FullName::try_from(full.as_str())
        .map_err(|_| ForgeError::InvalidName(branch.to_owned()))?;
    Ok(full)
}

pub fn branch_tip(repo: &gix::Repository, branch: &str) -> Result<Option<ObjectId>, ForgeError> {
    let full = validate_branch_name(branch)?;
    resolve_ref(repo, &full)
}

fn summarize(commit: &gix::Commit<'_>) -> Result<CommitSummary, ForgeError> {
    let id = commit.id;
    let message = commit.message_raw_sloppy().to_str_lossy().into_owned();
    let author = commit.author().map_err(git)?;
    let time = author.time().map_err(git)?;
    Ok(CommitSummary {
        id: id.to_string(),
        short: id.to_hex_with_len(10).to_string(),
        summary: message.lines().next().unwrap_or_default().to_owned(),
        author: author.name.to_str_lossy().into_owned(),
        time: jiff::Timestamp::from_second(time.seconds)
            .map(|t| t.strftime("%Y-%m-%d %H:%M UTC").to_string())
            .unwrap_or_default(),
    })
}

/// Up to `limit` commits reachable from `tip`, newest first.
pub fn log(
    repo: &gix::Repository,
    tip: ObjectId,
    limit: usize,
) -> Result<Vec<CommitSummary>, ForgeError> {
    let walk = repo
        .rev_walk([tip])
        .sorting(gix::revision::walk::Sorting::ByCommitTime(
            Default::default(),
        ))
        .all()
        .map_err(git)?;
    let mut out = Vec::new();
    for info in walk.take(limit) {
        let info = info.map_err(git)?;
        out.push(summarize(&repo.find_commit(info.id).map_err(git)?)?);
    }
    Ok(out)
}

pub fn commit_summary(repo: &gix::Repository, id: ObjectId) -> Result<CommitSummary, ForgeError> {
    summarize(&repo.find_commit(id).map_err(git)?)
}

/// What a path inside a commit names.
pub enum PathContent {
    Tree(Vec<TreeEntry>),
    Blob { data: Vec<u8>, id: String },
}

/// The tree or blob at `path` (slash-separated, empty for the root) in `commit`.
pub fn read_path(
    repo: &gix::Repository,
    commit: ObjectId,
    path: &str,
) -> Result<PathContent, ForgeError> {
    let mut tree = repo.find_commit(commit).map_err(git)?.tree().map_err(git)?;
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    for (i, segment) in segments.iter().enumerate() {
        let entry = tree
            .find_entry(*segment)
            .ok_or_else(|| ForgeError::NotFound(format!("path {path}")))?;
        let mode = entry.mode();
        let id = entry.oid().to_owned();
        let last = i + 1 == segments.len();
        if mode.is_tree() {
            tree = repo.find_tree(id).map_err(git)?;
        } else if last && (mode.is_blob() || mode.is_link()) {
            let blob = repo.find_blob(id).map_err(git)?;
            return Ok(PathContent::Blob {
                data: blob.data.clone(),
                id: id.to_string(),
            });
        } else {
            return Err(ForgeError::NotFound(format!("path {path}")));
        }
    }
    let mut entries = Vec::new();
    for entry in tree.iter() {
        let entry = entry.map_err(git)?;
        let mode = entry.mode();
        let kind = if mode.is_tree() {
            TreeEntryKind::Tree
        } else if mode.is_commit() {
            TreeEntryKind::Submodule
        } else if mode.is_link() {
            TreeEntryKind::Link
        } else {
            TreeEntryKind::Blob
        };
        entries.push(TreeEntry {
            name: entry.filename().to_str_lossy().into_owned(),
            kind,
            id: entry.oid().to_string(),
        });
    }
    entries.sort_by(|a, b| {
        let rank = |e: &TreeEntry| u8::from(!matches!(e.kind, TreeEntryKind::Tree));
        rank(a).cmp(&rank(b)).then(a.name.cmp(&b.name))
    });
    Ok(PathContent::Tree(entries))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs() {
        assert!(Slug::parse("tyler").is_ok());
        assert!(Slug::parse("site.v2").is_ok());
        for bad in ["", "-x", "Upper", "a/b", "..", "x..y", "r.git", "has space"] {
            assert!(Slug::parse(bad).is_err(), "{bad}");
        }
        assert_eq!(
            RepoName::new("tyler", "site.git").unwrap().to_string(),
            "tyler/site"
        );
    }
}
