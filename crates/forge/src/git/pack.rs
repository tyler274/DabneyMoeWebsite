//! Packfiles in and out, in pure Rust: object selection over gix's revwalk,
//! encoding with gix-pack's parallel entry pipeline, and decoding/indexing
//! received (possibly thin) packs with `gix_pack::Bundle`.

use std::collections::HashSet;
use std::io::BufReader;
use std::sync::atomic::AtomicBool;

use gix::odb::pack;
use gix::ObjectId;

#[derive(Debug, thiserror::Error)]
pub enum PackError {
    #[error("object database error")]
    Odb(#[source] gix::Error),
    #[error("object {0} is missing")]
    Missing(ObjectId),
    #[error("object {id} is a {kind}, expected {expected}")]
    Kind {
        id: ObjectId,
        kind: gix::objs::Kind,
        expected: &'static str,
    },
    #[error("pack has more than u32::MAX objects")]
    TooLarge,
    #[error("received pack is malformed")]
    Malformed,
}

fn odb(e: impl Into<gix::Error>) -> PackError {
    PackError::Odb(e.into())
}

/// Add `tree` and everything below it to `out`, skipping objects already in
/// `seen` (submodule commits are not ours to send).
fn collect_tree(
    repo: &gix::Repository,
    tree: ObjectId,
    seen: &mut HashSet<ObjectId>,
    out: &mut Vec<ObjectId>,
) -> Result<(), PackError> {
    let mut stack = vec![tree];
    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        out.push(id);
        let tree = repo.find_tree(id).map_err(odb)?;
        for entry in tree.iter() {
            let entry = entry.map_err(odb)?;
            let mode = entry.mode();
            let oid = entry.oid().to_owned();
            if mode.is_tree() {
                stack.push(oid);
            } else if (mode.is_blob() || mode.is_link()) && seen.insert(oid) {
                out.push(oid);
            }
        }
    }
    Ok(())
}

/// Every object reachable from `wants` but not from `haves` the server has,
/// treating the trees of the `haves` as already present on the client.
pub fn objects_for_fetch(
    repo: &gix::Repository,
    wants: &[ObjectId],
    haves: &[ObjectId],
) -> Result<Vec<ObjectId>, PackError> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut commit_tips = Vec::new();
    for want in wants {
        let mut id = *want;
        loop {
            let object = repo
                .try_find_object(id)
                .map_err(odb)?
                .ok_or(PackError::Missing(id))?;
            match object.kind {
                gix::objs::Kind::Tag => {
                    if seen.insert(id) {
                        out.push(id);
                    }
                    id = object.into_tag().target_id().map_err(odb)?.detach();
                }
                gix::objs::Kind::Commit => {
                    commit_tips.push(id);
                    break;
                }
                gix::objs::Kind::Tree => {
                    collect_tree(repo, id, &mut seen, &mut out)?;
                    break;
                }
                gix::objs::Kind::Blob => {
                    if seen.insert(id) {
                        out.push(id);
                    }
                    break;
                }
            }
        }
    }
    let hidden: Vec<ObjectId> = haves
        .iter()
        .copied()
        .filter(|h| repo.find_commit(*h).is_ok())
        .collect();
    let mut boundary = HashSet::new();
    for have in &hidden {
        let tree = repo
            .find_commit(*have)
            .map_err(odb)?
            .tree_id()
            .map_err(odb)?
            .detach();
        let mut ignored = Vec::new();
        collect_tree(repo, tree, &mut boundary, &mut ignored)?;
    }
    seen.extend(boundary);
    if commit_tips.is_empty() {
        return Ok(out);
    }
    let walk = repo
        .rev_walk(commit_tips)
        .with_hidden(hidden)
        .all()
        .map_err(odb)?;
    for info in walk {
        let info = info.map_err(odb)?;
        if !seen.insert(info.id) {
            continue;
        }
        out.push(info.id);
        let tree = repo
            .find_commit(info.id)
            .map_err(odb)?
            .tree_id()
            .map_err(odb)?
            .detach();
        collect_tree(repo, tree, &mut seen, &mut out)?;
    }
    Ok(out)
}

/// Encode `ids` into a version-2 packfile (with trailer).
pub fn write_pack(repo: &gix::Repository, ids: Vec<ObjectId>) -> Result<Vec<u8>, PackError> {
    let mut db = repo.objects.clone().into_inner();
    // Pack copying hands out pack locations, which must stay mapped.
    db.prevent_pack_unload();
    let interrupt = AtomicBool::new(false);
    let (counts, _) = pack::data::output::count::objects(
        db.clone(),
        Box::new(ids.into_iter().map(Ok)),
        &gix::progress::Discard,
        &interrupt,
        pack::data::output::count::objects::Options {
            thread_limit: Some(4),
            chunk_size: 64,
            input_object_expansion: pack::data::output::count::objects::ObjectExpansion::AsIs,
        },
    )
    .map_err(odb)?;
    let num_entries = u32::try_from(counts.len()).map_err(|_| PackError::TooLarge)?;
    let entries = pack::data::output::entry::iter_from_counts(
        counts,
        db,
        Box::new(gix::progress::Discard),
        pack::data::output::entry::iter_from_counts::Options {
            thread_limit: Some(4),
            ..Default::default()
        },
    )
    .map_err(odb)?;
    let ordered = gix::parallel::InOrderIter::from(entries);
    let mut out = Vec::new();
    let mut writer = pack::data::output::bytes::FromEntriesIter::new(
        ordered,
        &mut out,
        num_entries,
        pack::data::Version::V2,
        repo.object_hash(),
    );
    for written in writer.by_ref() {
        written.map_err(odb)?;
    }
    drop(writer);
    Ok(out)
}

/// Number of objects a received pack declares, from its 12-byte header.
fn declared_objects(pack: &[u8]) -> Result<u32, PackError> {
    if pack.len() < 12 || &pack[..4] != b"PACK" {
        return Err(PackError::Malformed);
    }
    Ok(u32::from_be_bytes([pack[8], pack[9], pack[10], pack[11]]))
}

/// Decode `pack` (resolving thin-pack bases from the repository), write it
/// and its index into `objects/pack`. Returns the `.keep` marker to remove
/// once refs point into the pack.
pub fn receive_pack(
    repo: &gix::Repository,
    pack_bytes: &[u8],
) -> Result<Option<std::path::PathBuf>, PackError> {
    if pack_bytes.is_empty() || declared_objects(pack_bytes)? == 0 {
        return Ok(None);
    }
    let directory = repo.git_dir().join("objects").join("pack");
    let mut reader = BufReader::new(pack_bytes);
    let outcome = pack::Bundle::write_to_directory(
        &mut reader,
        Some(&directory),
        &mut gix::progress::Discard,
        &AtomicBool::new(false),
        Some(repo.objects.clone().into_inner()),
        repo.object_hash(),
        pack::bundle::write::Options::default(),
    )
    .map_err(odb)?;
    Ok(outcome.keep_path)
}

/// Check that `tip` is a commit whose new history has its trees present.
pub fn check_connected(
    repo: &gix::Repository,
    tip: ObjectId,
    known: &[ObjectId],
) -> Result<(), PackError> {
    let object = repo
        .try_find_object(tip)
        .map_err(odb)?
        .ok_or(PackError::Missing(tip))?;
    if object.kind != gix::objs::Kind::Commit {
        // Tags may point at any object; branch tips are checked by the caller.
        return Ok(());
    }
    let walk = repo
        .rev_walk([tip])
        .with_hidden(known.iter().copied())
        .all()
        .map_err(odb)?;
    for info in walk {
        let info = info.map_err(odb)?;
        let commit = repo.find_commit(info.id).map_err(odb)?;
        let tree = commit.tree_id().map_err(odb)?.detach();
        if !repo.has_object(tree) {
            return Err(PackError::Missing(tree));
        }
    }
    Ok(())
}

/// Require `id` to be a commit.
pub fn require_commit(repo: &gix::Repository, id: ObjectId) -> Result<(), PackError> {
    let object = repo
        .try_find_object(id)
        .map_err(odb)?
        .ok_or(PackError::Missing(id))?;
    if object.kind == gix::objs::Kind::Commit {
        Ok(())
    } else {
        Err(PackError::Kind {
            id,
            kind: object.kind,
            expected: "commit",
        })
    }
}
