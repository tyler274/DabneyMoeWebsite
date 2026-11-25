//! Git Smart HTTP in pure Rust: protocol v2 `ls-refs`/`fetch` for clones
//! and fetches, and v0 `receive-pack` (the only push protocol) with
//! `report-status`. No `git` executable is involved.

use std::collections::HashSet;
use std::io::Read;

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::Response;
use bytes::Bytes;
use gix::bstr::ByteSlice;
use gix::refs::transaction::{Change, LogChange, PreviousValue, RefEdit, RefLog};
use gix::refs::Target;
use gix::ObjectId;
use gix_packetline::Channel;
use serde::Deserialize;

use crate::app::{blocking, App, Forge};
use crate::auth::{self, Action};
use crate::db::RepoRecord;
use crate::error::ForgeError;
use crate::git::pack;
use crate::git::pkt::{Pkt, PktError, PktReader, PktWriter};
use crate::repo::{self, git};

const AGENT: &str = concat!("agent=dabney-forge/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Service {
    UploadPack,
    ReceivePack,
}

impl Service {
    fn parse(raw: &str) -> Result<Self, ForgeError> {
        match raw {
            "git-upload-pack" => Ok(Service::UploadPack),
            "git-receive-pack" => Ok(Service::ReceivePack),
            other => Err(ForgeError::BadRequest(format!(
                "unsupported service {other:?}"
            ))),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Service::UploadPack => "git-upload-pack",
            Service::ReceivePack => "git-receive-pack",
        }
    }

    fn action(self) -> Action {
        match self {
            Service::UploadPack => Action::Read,
            Service::ReceivePack => Action::Push,
        }
    }
}

#[derive(Deserialize)]
pub struct ServiceQuery {
    service: String,
}

fn git_response(content_type: &'static str, body: Vec<u8>) -> Response {
    let mut response = Response::new(Body::from(body));
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-cache, max-age=0, must-revalidate"),
    );
    response
}

fn wants_v2(headers: &HeaderMap) -> bool {
    headers
        .get_all("git-protocol")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(':'))
        .any(|p| p.trim() == "version=2")
}

/// The request body, gunzipped when the client compressed it, bounded by
/// the configured limit.
fn request_body(forge: &Forge, headers: &HeaderMap, body: Bytes) -> Result<Vec<u8>, ForgeError> {
    let gzip = headers
        .get(header::CONTENT_ENCODING)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.eq_ignore_ascii_case("gzip") || v.eq_ignore_ascii_case("x-gzip"));
    if !gzip {
        return Ok(body.to_vec());
    }
    let limit = forge.config.max_git_body as u64;
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(&body[..])
        .take(limit + 1)
        .read_to_end(&mut out)
        .map_err(|e| ForgeError::BadRequest(format!("invalid gzip body: {e}")))?;
    if out.len() as u64 > limit {
        return Err(ForgeError::BadRequest("request body too large".into()));
    }
    Ok(out)
}

/// `GET /{owner}/{repo}.git/info/refs?service=...`
pub async fn info_refs(
    State(app): State<App>,
    Path((owner, repo)): Path<(String, String)>,
    Query(query): Query<ServiceQuery>,
    headers: HeaderMap,
) -> Result<Response, ForgeError> {
    let service = Service::parse(&query.service)?;
    blocking(&app, move |forge| {
        let principal = auth::authenticate(&forge.db, &headers)?;
        let record = forge.authorize(&owner, &repo, principal.as_ref(), service.action())?;
        match service {
            Service::UploadPack => Ok(upload_pack_advertisement(wants_v2(&headers))),
            Service::ReceivePack => {
                let repo = forge.repos.open(&record.name)?;
                receive_pack_advertisement(&repo)
            }
        }
    })
    .await
}

fn upload_pack_advertisement(v2: bool) -> Response {
    let mut w = PktWriter::new();
    if !v2 {
        // Protocol v0/v1 upload-pack needs stateful negotiation this server
        // does not implement; every supported Git (>= 2.26) speaks v2.
        w.line(format!("# service={}", Service::UploadPack.name()))
            .flush();
        w.line("ERR dabney-forge requires Git protocol v2 (git -c protocol.version=2)");
        return git_response(
            "application/x-git-upload-pack-advertisement",
            w.into_bytes(),
        );
    }
    w.line("version 2")
        .line(AGENT)
        .line("ls-refs=unborn")
        .line("fetch")
        .line("object-format=sha1")
        .flush();
    git_response(
        "application/x-git-upload-pack-advertisement",
        w.into_bytes(),
    )
}

const RECEIVE_CAPS: &str =
    "report-status delete-refs side-band-64k atomic ofs-delta object-format=sha1";

fn receive_pack_advertisement(repo: &gix::Repository) -> Result<Response, ForgeError> {
    let mut w = PktWriter::new();
    w.line(format!("# service={}", Service::ReceivePack.name()))
        .flush();
    let caps = format!("{RECEIVE_CAPS} {AGENT}");
    let refs: Vec<_> = repo::list_refs(repo)?
        .into_iter()
        .filter(|r| is_pushable(&r.name))
        .collect();
    if refs.is_empty() {
        let null = ObjectId::null(repo.object_hash());
        w.data(format!("{null} capabilities^{{}}\0{caps}\n").as_bytes());
    }
    for (i, tip) in refs.iter().enumerate() {
        if i == 0 {
            w.data(format!("{} {}\0{caps}\n", tip.target, tip.name).as_bytes());
        } else {
            w.line(format!("{} {}", tip.target, tip.name));
        }
    }
    w.flush();
    Ok(git_response(
        "application/x-git-receive-pack-advertisement",
        w.into_bytes(),
    ))
}

/// Refs clients may create, move or delete; `refs/pull/*` belongs to the forge.
fn is_pushable(name: &str) -> bool {
    name.starts_with("refs/heads/") || name.starts_with("refs/tags/")
}

/// The command and capability section, then the arguments, of a v2 request.
struct V2Request<'a> {
    command: String,
    args: Vec<&'a [u8]>,
}

fn parse_v2(body: &[u8]) -> Result<V2Request<'_>, ForgeError> {
    let mut reader = PktReader::new(body);
    let mut command = None;
    loop {
        match reader.next_pkt()? {
            Some(pkt @ Pkt::Data(_)) => {
                let line = pkt.line().unwrap_or_default();
                if let Some(c) = line.strip_prefix(b"command=") {
                    command = Some(c.to_str_lossy().into_owned());
                }
                // agent=, object-format= and other capabilities need no answer.
            }
            Some(Pkt::Delim) | Some(Pkt::Flush) => break,
            Some(Pkt::ResponseEnd) | None => return Err(PktError::Truncated.into()),
        }
    }
    let mut args = Vec::new();
    // If the capability section ended with a flush there are no arguments.
    while let Some(pkt) = reader.next_pkt()? {
        match pkt {
            Pkt::Data(_) => args.push(pkt.line().unwrap_or_default()),
            Pkt::Flush => break,
            other => return Err(PktError::Unexpected(format!("{other:?} in arguments")).into()),
        }
    }
    let command = command.ok_or_else(|| PktError::Unexpected("request has no command".into()))?;
    Ok(V2Request { command, args })
}

fn parse_oid(raw: &[u8]) -> Result<ObjectId, ForgeError> {
    ObjectId::from_hex(raw)
        .map_err(|_| ForgeError::BadRequest(format!("invalid object id {:?}", raw.as_bstr())))
}

/// `POST /{owner}/{repo}.git/git-upload-pack` (protocol v2).
pub async fn upload_pack(
    State(app): State<App>,
    Path((owner, repo)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ForgeError> {
    blocking(&app, move |forge| {
        let principal = auth::authenticate(&forge.db, &headers)?;
        let record = forge.authorize(&owner, &repo, principal.as_ref(), Action::Read)?;
        if !wants_v2(&headers) {
            return Err(ForgeError::BadRequest("Git protocol v2 is required".into()));
        }
        let body = request_body(forge, &headers, body)?;
        let request = parse_v2(&body)?;
        let repo = forge.repos.open(&record.name)?;
        let out = match request.command.as_str() {
            "ls-refs" => ls_refs(&repo, &request.args)?,
            "fetch" => fetch(&repo, &request.args)?,
            other => {
                return Err(ForgeError::BadRequest(format!(
                    "unsupported command {other:?}"
                )))
            }
        };
        Ok(git_response("application/x-git-upload-pack-result", out))
    })
    .await
}

fn ls_refs(repo: &gix::Repository, args: &[&[u8]]) -> Result<Vec<u8>, ForgeError> {
    let mut peel = false;
    let mut symrefs = false;
    let mut unborn = false;
    let mut prefixes = Vec::new();
    for arg in args {
        match *arg {
            b"peel" => peel = true,
            b"symrefs" => symrefs = true,
            b"unborn" => unborn = true,
            other => {
                if let Some(p) = other.strip_prefix(b"ref-prefix ") {
                    prefixes.push(p.to_str_lossy().into_owned());
                }
            }
        }
    }
    let wanted =
        |name: &str| prefixes.is_empty() || prefixes.iter().any(|p| name.starts_with(p.as_str()));
    let mut w = PktWriter::new();
    if wanted("HEAD") {
        if let Some(target) = repo::head_symref(repo) {
            let symref = if symrefs {
                format!(" symref-target:{target}")
            } else {
                String::new()
            };
            match repo::resolve_ref(repo, &target)? {
                Some(id) => {
                    w.line(format!("{id} HEAD{symref}"));
                }
                None if unborn => {
                    w.line(format!("unborn HEAD{symref}"));
                }
                None => {}
            }
        }
    }
    for tip in repo::list_refs(repo)? {
        if !wanted(&tip.name) {
            continue;
        }
        let mut line = format!("{} {}", tip.target, tip.name);
        if let (true, Some(peeled)) = (peel, tip.peeled) {
            line.push_str(&format!(" peeled:{peeled}"));
        }
        w.line(line);
    }
    w.flush();
    Ok(w.into_bytes())
}

fn fetch(repo: &gix::Repository, args: &[&[u8]]) -> Result<Vec<u8>, ForgeError> {
    let mut wants = Vec::new();
    let mut haves = Vec::new();
    let mut done = false;
    for arg in args {
        if let Some(id) = arg.strip_prefix(b"want ") {
            wants.push(parse_oid(id)?);
        } else if let Some(id) = arg.strip_prefix(b"have ") {
            haves.push(parse_oid(id)?);
        } else if *arg == b"done" {
            done = true;
        } else if arg.starts_with(b"deepen")
            || arg.starts_with(b"shallow ")
            || arg.starts_with(b"filter ")
        {
            return Err(ForgeError::BadRequest(
                "shallow and partial clones are not supported".into(),
            ));
        } else if arg.starts_with(b"want-ref ") {
            return Err(ForgeError::BadRequest("want-ref is not supported".into()));
        }
        // thin-pack, ofs-delta, no-progress and include-tag are hints.
    }
    for want in &wants {
        if !repo.has_object(want) {
            return Err(ForgeError::BadRequest(format!("not our ref {want}")));
        }
    }
    let common: Vec<ObjectId> = haves
        .iter()
        .copied()
        .filter(|h| repo.has_object(h))
        .collect();
    let mut w = PktWriter::new();
    if !done {
        w.line("acknowledgments");
        if common.is_empty() {
            // Nothing in common yet: ask for more haves.
            w.line("NAK").flush();
            return Ok(w.into_bytes());
        }
        for id in &common {
            w.line(format!("ACK {id}"));
        }
        w.line("ready").delim();
    }
    let ids = pack::objects_for_fetch(repo, &wants, &common)?;
    let pack = pack::write_pack(repo, ids)?;
    w.line("packfile").band(Channel::Data, &pack).flush();
    Ok(w.into_bytes())
}

/// One `old new ref` line of a push.
#[derive(Debug, Clone)]
pub struct PushCommand {
    pub old: ObjectId,
    pub new: ObjectId,
    pub name: String,
}

impl PushCommand {
    pub fn is_delete(&self) -> bool {
        self.new.is_null()
    }
}

struct PushRequest<'a> {
    commands: Vec<PushCommand>,
    caps: HashSet<String>,
    pack: &'a [u8],
}

fn parse_push(body: &[u8]) -> Result<PushRequest<'_>, ForgeError> {
    let mut reader = PktReader::new(body);
    let mut commands = Vec::new();
    let mut caps = HashSet::new();
    loop {
        match reader.next_pkt()? {
            Some(pkt @ Pkt::Data(_)) => {
                let line = pkt.line().unwrap_or_default();
                let (line, cap_text) = match line.find_byte(0) {
                    Some(nul) => (&line[..nul], Some(&line[nul + 1..])),
                    None => (line, None),
                };
                if let Some(cap_text) = cap_text {
                    caps.extend(
                        cap_text
                            .to_str_lossy()
                            .split_ascii_whitespace()
                            .map(str::to_owned),
                    );
                }
                if line.starts_with(b"shallow ") {
                    return Err(ForgeError::BadRequest(
                        "pushing from a shallow clone is not supported".into(),
                    ));
                }
                let mut parts = line.splitn_str(3, " ");
                let (Some(old), Some(new), Some(name)) = (parts.next(), parts.next(), parts.next())
                else {
                    return Err(PktError::Unexpected(format!(
                        "bad push command {:?}",
                        line.as_bstr()
                    ))
                    .into());
                };
                commands.push(PushCommand {
                    old: parse_oid(old)?,
                    new: parse_oid(new)?,
                    name: name.to_str_lossy().into_owned(),
                });
            }
            Some(Pkt::Flush) => break,
            Some(other) => {
                return Err(PktError::Unexpected(format!("{other:?} in push commands")).into())
            }
            None => return Err(PktError::Truncated.into()),
        }
    }
    Ok(PushRequest {
        commands,
        caps,
        pack: reader.rest(),
    })
}

/// Why one ref of a push was refused.
pub type Rejection = String;

/// The outcome of a push: the unpack result and, per command, `None` for
/// success or the rejection reason.
pub struct PushReport {
    pub unpack: Result<(), String>,
    pub results: Vec<(PushCommand, Option<Rejection>)>,
}

impl PushReport {
    pub fn updated(&self) -> impl Iterator<Item = &PushCommand> {
        self.results
            .iter()
            .filter(|(_, r)| r.is_none())
            .map(|(c, _)| c)
    }

    fn encode(&self, side_band: bool) -> Vec<u8> {
        let mut status = PktWriter::new();
        match &self.unpack {
            Ok(()) => status.line("unpack ok"),
            Err(e) => status.line(format!("unpack {e}")),
        };
        for (command, rejection) in &self.results {
            match rejection {
                None => status.line(format!("ok {}", command.name)),
                Some(reason) => status.line(format!("ng {} {reason}", command.name)),
            };
        }
        status.flush();
        let status = status.into_bytes();
        if !side_band {
            return status;
        }
        let mut out = PktWriter::new();
        out.band(Channel::Data, &status).flush();
        out.into_bytes()
    }
}

/// `POST /{owner}/{repo}.git/git-receive-pack`
pub async fn receive_pack(
    State(app): State<App>,
    Path((owner, repo)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ForgeError> {
    blocking(&app, move |forge| {
        let principal = auth::authenticate(&forge.db, &headers)?;
        let record = forge.authorize(&owner, &repo, principal.as_ref(), Action::Push)?;
        let pusher = principal.map(|p| p.user).unwrap_or_default();
        let body = request_body(forge, &headers, body)?;
        let request = parse_push(&body)?;
        let side_band = request.caps.contains("side-band-64k");
        let atomic = request.caps.contains("atomic");
        let lock = forge.repo_lock(record.id);
        let _guard = lock.lock().unwrap_or_else(|p| p.into_inner());
        let report = apply_push(forge, &record, request.commands, request.pack, atomic)?;
        let updated: Vec<PushCommand> = report.updated().cloned().collect();
        if !updated.is_empty() {
            tracing::info!(repo = %record.name, %pusher, refs = updated.len(), "push accepted");
            // Refs have already moved; do not replace the client's report-status
            // packet with HTTP 500 if follow-up metadata work fails.
            if let Err(e) = crate::pr::after_push(forge, &record, &updated) {
                tracing::error!(
                    repo = %record.name,
                    error = %e.chain(),
                    "post-push hook failed after refs were updated"
                );
            }
        }
        let mut response = git_response(
            "application/x-git-receive-pack-result",
            report.encode(side_band),
        );
        *response.status_mut() = StatusCode::OK;
        Ok(response)
    })
    .await
}

/// Unpack the pack, check every command and apply the accepted ones in
/// one reference transaction guarded by each command's old value.
pub fn apply_push(
    forge: &Forge,
    record: &RepoRecord,
    commands: Vec<PushCommand>,
    pack_bytes: &[u8],
    atomic: bool,
) -> Result<PushReport, ForgeError> {
    let repo = forge.repos.open(&record.name)?;
    let received = match pack::receive_pack(&repo, pack_bytes, forge.config.max_pack_alloc) {
        Ok(received) => received,
        Err(e) => {
            let reason = format!("unpacker error: {e}");
            tracing::warn!(repo = %record.name, error = %reason, "push unpack failed");
            return Ok(PushReport {
                unpack: Err(reason),
                results: commands
                    .into_iter()
                    .map(|c| (c, Some("unpacker error".to_owned())))
                    .collect(),
            });
        }
    };
    let mut known_commits = Vec::new();
    let mut checked_trees = HashSet::new();
    let mut checked_blobs = HashSet::new();
    for tip in repo::list_refs(&repo)? {
        let id = tip.peeled.unwrap_or(tip.target);
        if let Ok(commit) = repo.find_commit(id) {
            known_commits.push(id);
            if let Ok(tree_id) = commit.tree_id() {
                checked_trees.insert(tree_id.detach());
            }
        }
    }
    let default_ref = format!("refs/heads/{}", record.default_branch);
    let mut results: Vec<(PushCommand, Option<Rejection>)> = commands
        .into_iter()
        .map(|c| {
            let verdict = check_command(
                &repo,
                &c,
                &known_commits,
                &mut checked_trees,
                &mut checked_blobs,
                &default_ref,
            );
            (c, verdict.err())
        })
        .collect();
    if atomic && results.iter().any(|(_, r)| r.is_some()) {
        for (_, r) in results.iter_mut() {
            r.get_or_insert_with(|| "atomic push failed".to_owned());
        }
    }
    let edits: Vec<RefEdit> = results
        .iter()
        .filter(|(_, r)| r.is_none())
        .map(|(c, _)| push_edit(c))
        .collect::<Result<_, _>>()?;
    let mut refs_moved = false;
    if !edits.is_empty() {
        match edit_refs(&repo, edits) {
            Ok(()) => refs_moved = true,
            Err(e) => {
                let reason = format!("failed to update ref: {}", e.chain());
                tracing::warn!(repo = %record.name, error = %reason, "push ref transaction failed");
                for (_, r) in results.iter_mut() {
                    r.get_or_insert_with(|| "failed to update ref".to_owned());
                }
            }
        }
    }
    // Release the repository handle before unlinking any rejected packfiles.
    drop(repo);
    let has_new_objects = refs_moved && results.iter().any(|(c, r)| r.is_none() && !c.is_delete());
    if has_new_objects {
        if let Some(pack) = received {
            pack.commit();
        }
    }
    // Otherwise `received` drops here and removes any newly written pack/idx/.keep.
    Ok(PushReport {
        unpack: Ok(()),
        results,
    })
}

fn check_command(
    repo: &gix::Repository,
    command: &PushCommand,
    known_commits: &[ObjectId],
    checked_trees: &mut HashSet<ObjectId>,
    checked_blobs: &mut HashSet<ObjectId>,
    default_ref: &str,
) -> Result<(), Rejection> {
    if !is_pushable(&command.name) || gix::refs::FullName::try_from(command.name.as_str()).is_err()
    {
        return Err("refusing to update this ref".into());
    }
    let current = repo::resolve_ref_target(repo, &command.name).map_err(|e| e.to_string())?;
    let expected = (!command.old.is_null()).then_some(command.old);
    if current != expected {
        return Err("stale info: the ref moved since it was advertised".into());
    }
    if command.is_delete() {
        if command.name == default_ref {
            return Err("cannot delete the default branch".into());
        }
        return Ok(());
    }
    if command.name.starts_with("refs/heads/") {
        pack::require_commit(repo, command.new).map_err(|e| e.to_string())?;
    }
    pack::check_connected(
        repo,
        command.new,
        known_commits,
        checked_trees,
        checked_blobs,
    )
    .map_err(|e| format!("missing objects: {e}"))?;
    if command.name == default_ref && !command.old.is_null() {
        let base = repo
            .merge_base(command.old, command.new)
            .map_err(|e| e.to_string())?;
        if base.map(|b| b.detach()) != Some(command.old) {
            return Err("non-fast-forward updates to the default branch are not allowed".into());
        }
    }
    Ok(())
}

fn push_edit(command: &PushCommand) -> Result<RefEdit, ForgeError> {
    let name = gix::refs::FullName::try_from(command.name.as_str())
        .map_err(|_| ForgeError::InvalidName(command.name.clone()))?;
    let expected = if command.old.is_null() {
        PreviousValue::MustNotExist
    } else {
        PreviousValue::MustExistAndMatch(Target::Object(command.old))
    };
    let change = if command.is_delete() {
        Change::Delete {
            expected,
            log: RefLog::AndReference,
        }
    } else {
        Change::Update {
            log: LogChange {
                message: "push".into(),
                ..Default::default()
            },
            expected,
            new: Target::Object(command.new),
        }
    };
    Ok(RefEdit::new(name, change))
}

/// Apply `edits` in one transaction, attributing reflogs to the forge.
pub fn edit_refs(repo: &gix::Repository, edits: Vec<RefEdit>) -> Result<(), ForgeError> {
    let committer = forge_signature();
    let mut buf = gix::date::parse::TimeBuf::default();
    repo.edit_references_as(edits, Some(committer.to_ref(&mut buf)))
        .map_err(git)?;
    Ok(())
}

/// The identity the forge commits and logs as.
pub fn forge_signature() -> gix::actor::Signature {
    gix::actor::Signature {
        name: "dabney-forge".into(),
        email: "forge@dabney.moe".into(),
        time: gix::date::Time::now_utc(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_commands_and_caps_parse() {
        let a = "a".repeat(40);
        let z = "0".repeat(40);
        let mut w = PktWriter::new();
        w.data(
            format!("{z} {a} refs/heads/main\0report-status side-band-64k agent=git/2\n")
                .as_bytes(),
        )
        .line(format!("{a} {z} refs/heads/old"))
        .flush()
        .raw(b"PACK");
        let bytes = w.into_bytes();
        let push = parse_push(&bytes).unwrap();
        assert_eq!(push.commands.len(), 2);
        assert!(push.commands[0].old.is_null());
        assert_eq!(push.commands[0].name, "refs/heads/main");
        assert!(push.commands[1].is_delete());
        assert!(push.caps.contains("side-band-64k"));
        assert_eq!(push.pack, b"PACK");
    }

    #[test]
    fn v2_request_parses_command_and_args() {
        let mut w = PktWriter::new();
        w.line("command=ls-refs")
            .line("agent=git/2")
            .delim()
            .line("peel")
            .line("ref-prefix refs/heads/")
            .flush();
        let bytes = w.into_bytes();
        let req = parse_v2(&bytes).unwrap();
        assert_eq!(req.command, "ls-refs");
        assert_eq!(req.args, vec![&b"peel"[..], &b"ref-prefix refs/heads/"[..]]);
    }

    #[test]
    fn report_status_is_wrapped_in_side_band() {
        let report = PushReport {
            unpack: Ok(()),
            results: vec![(
                PushCommand {
                    old: ObjectId::null(gix::hash::Kind::Sha1),
                    new: ObjectId::null(gix::hash::Kind::Sha1),
                    name: "refs/heads/x".into(),
                },
                Some("nope".into()),
            )],
        };
        let plain = report.encode(false);
        assert!(plain.windows(10).any(|w| w == b"unpack ok\n"));
        let banded = report.encode(true);
        let mut r = PktReader::new(&banded);
        let Some(Pkt::Data(first)) = r.next_pkt().unwrap() else {
            panic!()
        };
        assert_eq!(first[0], 1);
        assert_eq!(&first[1..], &plain[..]);
        assert_eq!(r.next_pkt().unwrap(), Some(Pkt::Flush));
    }
}
