//! Pull-request diffs in pure Rust: gix tree-to-tree changes (with rename
//! tracking) and histogram line diffs grouped into unified hunks.

use gix::bstr::ByteSlice;
use gix::diff::blob::{diff_with_slider_heuristics, Algorithm, InternedInput};
use gix::object::tree::diff::ChangeDetached;
use gix::ObjectId;
use serde::Serialize;

use crate::db::Side;
use crate::error::ForgeError;
use crate::repo::git;

/// Unchanged lines shown around each change.
pub const CONTEXT: u32 = 3;
/// Larger blobs are summarized instead of diffed line by line.
const MAX_TEXT_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChangeKind {
    Added,
    Deleted,
    Modified,
    Renamed { from: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LineKind {
    Context,
    Added,
    Removed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiffLine {
    pub kind: LineKind,
    /// 1-based line on the old side, absent for added lines.
    pub old: Option<u32>,
    /// 1-based line on the new side, absent for removed lines.
    pub new: Option<u32>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hunk {
    pub old_start: u32,
    pub old_len: u32,
    pub new_start: u32,
    pub new_len: u32,
    pub lines: Vec<DiffLine>,
}

impl Hunk {
    pub fn header(&self) -> String {
        format!(
            "@@ -{},{} +{},{} @@",
            self.old_start, self.old_len, self.new_start, self.new_len
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FileBody {
    Text { hunks: Vec<Hunk> },
    Binary,
    TooLarge,
    Submodule,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileDiff {
    pub path: String,
    pub change: ChangeKind,
    pub body: FileBody,
}

impl FileDiff {
    /// Whether `line` on `side` is shown in this diff, and so can carry an
    /// inline comment.
    pub fn has_line(&self, side: Side, line: u32) -> bool {
        let FileBody::Text { hunks } = &self.body else {
            return false;
        };
        hunks.iter().flat_map(|h| &h.lines).any(|l| match side {
            Side::Old => l.kind != LineKind::Added && l.old == Some(line),
            Side::New => l.kind != LineKind::Removed && l.new == Some(line),
        })
    }

    pub fn added(&self) -> usize {
        self.count(LineKind::Added)
    }

    pub fn removed(&self) -> usize {
        self.count(LineKind::Removed)
    }

    fn count(&self, kind: LineKind) -> usize {
        match &self.body {
            FileBody::Text { hunks } => hunks
                .iter()
                .flat_map(|h| &h.lines)
                .filter(|l| l.kind == kind)
                .count(),
            _ => 0,
        }
    }
}

fn line_text(raw: &[u8]) -> String {
    let raw = raw.strip_suffix(b"\n").unwrap_or(raw);
    let raw = raw.strip_suffix(b"\r").unwrap_or(raw);
    raw.to_str_lossy().into_owned()
}

/// Unified hunks between two texts, with `context` unchanged lines around
/// each change; nearby changes share a hunk.
pub fn text_hunks(old: &[u8], new: &[u8], context: u32) -> Vec<Hunk> {
    let input = InternedInput::new(old, new);
    let diff = diff_with_slider_heuristics(Algorithm::Histogram, &input);
    let changes: Vec<_> = diff.hunks().collect();
    let before_len = input.before.len() as u32;
    let text = |token| line_text(input.interner[token]);
    let mut groups: Vec<Vec<_>> = Vec::new();
    for change in changes {
        match groups.last_mut() {
            Some(group)
                if change.before.start
                    - group
                        .last()
                        .map_or(0, |c: &gix::diff::blob::Hunk| c.before.end)
                    <= 2 * context =>
            {
                group.push(change)
            }
            _ => groups.push(vec![change]),
        }
    }
    let mut out = Vec::new();
    for group in groups {
        let (first, last) = (&group[0], &group[group.len() - 1]);
        let lead = first.before.start.min(context);
        let (mut b, mut a) = (first.before.start - lead, first.after.start - lead);
        let (old_start, new_start) = (b, a);
        let trail = (before_len - last.before.end).min(context);
        let mut lines = Vec::new();
        let context_line = |b: u32, a: u32| DiffLine {
            kind: LineKind::Context,
            old: Some(b + 1),
            new: Some(a + 1),
            text: text(input.before[b as usize]),
        };
        for change in &group {
            while b < change.before.start {
                lines.push(context_line(b, a));
                b += 1;
                a += 1;
            }
            for i in change.before.clone() {
                lines.push(DiffLine {
                    kind: LineKind::Removed,
                    old: Some(i + 1),
                    new: None,
                    text: text(input.before[i as usize]),
                });
            }
            for i in change.after.clone() {
                lines.push(DiffLine {
                    kind: LineKind::Added,
                    old: None,
                    new: Some(i + 1),
                    text: text(input.after[i as usize]),
                });
            }
            b = change.before.end;
            a = change.after.end;
        }
        for _ in 0..trail {
            lines.push(context_line(b, a));
            b += 1;
            a += 1;
        }
        let (old_len, new_len) = (b - old_start, a - new_start);
        // Git numbers an empty side by the line before it.
        out.push(Hunk {
            old_start: if old_len == 0 {
                old_start
            } else {
                old_start + 1
            },
            old_len,
            new_start: if new_len == 0 {
                new_start
            } else {
                new_start + 1
            },
            new_len,
            lines,
        });
    }
    out
}

fn is_binary(data: &[u8]) -> bool {
    data[..data.len().min(8000)].contains(&0)
}

fn blob(repo: &gix::Repository, id: Option<&gix::oid>) -> Result<Vec<u8>, ForgeError> {
    match id {
        Some(id) => Ok(repo.find_blob(id).map_err(git)?.data.clone()),
        None => Ok(Vec::new()),
    }
}

/// Every file changed between the trees of `base` and `head`.
pub fn diff_commits(
    repo: &gix::Repository,
    base: ObjectId,
    head: ObjectId,
) -> Result<Vec<FileDiff>, ForgeError> {
    let old_tree = repo.find_commit(base).map_err(git)?.tree().map_err(git)?;
    let new_tree = repo.find_commit(head).map_err(git)?.tree().map_err(git)?;
    let changes = repo
        .diff_tree_to_tree(Some(&old_tree), Some(&new_tree), None)
        .map_err(git)?;
    let mut out = Vec::new();
    for change in changes {
        let (mode, _) = change.entry_mode_and_id();
        let (source_mode, _) = change.source_entry_mode_and_id();
        if mode.is_tree() && source_mode.is_tree() {
            continue;
        }
        let path = change.location().to_str_lossy().into_owned();
        let (kind, old_id, new_id) = match &change {
            ChangeDetached::Addition { id, .. } => (ChangeKind::Added, None, Some(id.as_ref())),
            ChangeDetached::Deletion { id, .. } => (ChangeKind::Deleted, Some(id.as_ref()), None),
            ChangeDetached::Modification {
                previous_id, id, ..
            } => (
                ChangeKind::Modified,
                Some(previous_id.as_ref()),
                Some(id.as_ref()),
            ),
            ChangeDetached::Rewrite {
                source_location,
                source_id,
                id,
                ..
            } => (
                ChangeKind::Renamed {
                    from: source_location.to_str_lossy().into_owned(),
                },
                Some(source_id.as_ref()),
                Some(id.as_ref()),
            ),
        };
        if mode.is_tree() || source_mode.is_tree() {
            continue;
        }
        let body = if mode.is_commit() || source_mode.is_commit() {
            FileBody::Submodule
        } else {
            let old = blob(repo, old_id)?;
            let new = blob(repo, new_id)?;
            if old.len() > MAX_TEXT_BYTES || new.len() > MAX_TEXT_BYTES {
                FileBody::TooLarge
            } else if is_binary(&old) || is_binary(&new) {
                FileBody::Binary
            } else {
                FileBody::Text {
                    hunks: text_hunks(&old, &new, CONTEXT),
                }
            }
        };
        out.push(FileDiff {
            path,
            change: kind,
            body,
        });
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

/// The commit a pull request's diff starts from: where its head forked from
/// `target` (the target tip while open, the merge's first parent after).
pub fn fork_point(
    repo: &gix::Repository,
    target: ObjectId,
    head: ObjectId,
) -> Result<ObjectId, ForgeError> {
    Ok(repo
        .merge_base(target, head)
        .map_err(git)?
        .map(|id| id.detach())
        .unwrap_or(target))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hunks_number_lines_and_keep_context() {
        let old = b"a\nb\nc\nd\ne\nf\ng\nh\ni\nj\n";
        let new = b"a\nb\nc\nd\nE\nf\ng\nh\ni\nj\nk\n";
        let hunks = text_hunks(old, new, 3);
        assert_eq!(hunks.len(), 1, "{hunks:#?}");
        let h = &hunks[0];
        assert_eq!(h.header(), "@@ -2,9 +2,10 @@");
        let removed: Vec<_> = h
            .lines
            .iter()
            .filter(|l| l.kind == LineKind::Removed)
            .collect();
        assert_eq!(removed.len(), 1);
        assert_eq!((removed[0].old, removed[0].text.as_str()), (Some(5), "e"));
        let added: Vec<_> = h
            .lines
            .iter()
            .filter(|l| l.kind == LineKind::Added)
            .map(|l| (l.new, l.text.as_str()))
            .collect();
        assert_eq!(added, vec![(Some(5), "E"), (Some(11), "k")]);
        let file = FileDiff {
            path: "x".into(),
            change: ChangeKind::Modified,
            body: FileBody::Text { hunks },
        };
        assert!(file.has_line(Side::New, 11));
        assert!(file.has_line(Side::Old, 5));
        assert!(!file.has_line(Side::Old, 11));
        assert!(!file.has_line(Side::New, 1), "line 1 is outside the hunk");
    }

    #[test]
    fn distant_changes_get_separate_hunks_and_new_files_start_at_one() {
        let old: String = (1..=20).map(|i| format!("{i}\n")).collect();
        let new = old
            .replacen("2\n", "two\n", 1)
            .replacen("19\n", "nineteen\n", 1);
        let hunks = text_hunks(old.as_bytes(), new.as_bytes(), 3);
        assert_eq!(hunks.len(), 2, "{hunks:#?}");
        let added = text_hunks(b"", b"x\ny\n", 3);
        assert_eq!(added[0].header(), "@@ -0,0 +1,2 @@");
    }
}
