//! Durable mirror fan-out: after the default branch or a tag moves, one job
//! per configured mirror is queued, and a background worker pushes the
//! default branch and all tags with a pure-Rust Smart HTTP push client.

use std::collections::HashMap;
use std::time::Duration;

use gix::bstr::ByteSlice;
use gix::ObjectId;

use crate::app::{blocking, App, Forge};
use crate::db::{JobStatus, MirrorJob, RepoRecord};
use crate::error::ForgeError;
use crate::git::pack;
use crate::git::pkt::{Pkt, PktReader, PktWriter};
use crate::repo;

const ATTEMPTS: u32 = 3;

#[derive(Debug, thiserror::Error)]
pub enum MirrorError {
    #[error("environment variable {0} holding the mirror token is not set")]
    MissingSecret(String),
    #[error("invalid mirror URL {0:?}")]
    Url(String),
    #[error("mirror request failed")]
    Http(#[from] reqwest::Error),
    #[error("mirror answered HTTP {0}")]
    Status(reqwest::StatusCode),
    #[error("mirror protocol error: {0}")]
    Protocol(String),
    #[error("mirror rejected the push: {0}")]
    Rejected(String),
    #[error(transparent)]
    Forge(#[from] ForgeError),
}

/// Queue a push to every mirror of `record`.
pub fn queue(forge: &Forge, record: &RepoRecord, trigger: &str) -> Result<(), ForgeError> {
    if forge.db.queue_mirror_jobs(record.id, trigger)? > 0 {
        forge.mirrors_ready.notify_one();
    }
    Ok(())
}

pub async fn run_worker(app: App) {
    loop {
        match blocking(&app, |forge| forge.db.claim_mirror_job()).await {
            Ok(Some(job)) => process(&app, job).await,
            Ok(None) => app.mirrors_ready.notified().await,
            Err(e) => {
                tracing::error!(error = %e.chain(), "cannot read mirror queue");
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }
}

fn chain(e: &dyn std::error::Error) -> String {
    let mut text = e.to_string();
    let mut source = e.source();
    while let Some(s) = source {
        text.push_str(": ");
        text.push_str(&s.to_string());
        source = s.source();
    }
    text
}

async fn process(app: &App, job: MirrorJob) {
    let mut outcome = Err(String::new());
    let mut tried = 0;
    for attempt in 1..=ATTEMPTS {
        tried = attempt;
        match push_mirror(app, &job).await {
            Ok(summary) => {
                tracing::info!(mirror = %job.mirror.name, %summary, "mirror push succeeded");
                outcome = Ok(());
                break;
            }
            Err(e) => {
                let text = chain(&e);
                tracing::warn!(mirror = %job.mirror.name, attempt, error = %text, "mirror push failed");
                outcome = Err(text);
                if attempt < ATTEMPTS {
                    tokio::time::sleep(Duration::from_secs(u64::from(attempt) * 2)).await;
                }
            }
        }
    }
    let id = job.id;
    let recorded = blocking(app, move |forge| match outcome {
        Ok(()) => forge
            .db
            .finish_mirror_job(id, JobStatus::Succeeded, tried, None),
        Err(e) => forge
            .db
            .finish_mirror_job(id, JobStatus::Failed, tried, Some(&e)),
    })
    .await;
    if let Err(e) = recorded {
        tracing::error!(error = %e.chain(), "cannot record mirror job");
    }
}

/// The refs a mirror receives: the default branch and every tag.
fn mirrored_refs(
    forge: &Forge,
    record: &RepoRecord,
) -> Result<Vec<(String, ObjectId)>, ForgeError> {
    let repo = forge.repos.open(&record.name)?;
    let default_ref = format!("refs/heads/{}", record.default_branch);
    Ok(repo::list_refs(&repo)?
        .into_iter()
        .filter(|r| r.name == default_ref || r.name.starts_with("refs/tags/"))
        .map(|r| (r.name, r.target))
        .collect())
}

/// Parse a v0 `git-receive-pack` advertisement into `ref -> oid`.
pub fn parse_advertisement(body: &[u8]) -> Result<HashMap<String, ObjectId>, MirrorError> {
    let protocol = |e: &dyn std::fmt::Display| MirrorError::Protocol(e.to_string());
    let mut reader = PktReader::new(body);
    let mut refs = HashMap::new();
    let mut first = true;
    while let Some(pkt) = reader.next_pkt().map_err(|e| protocol(&e))? {
        let Some(line) = pkt.line() else {
            continue;
        };
        if first && line.starts_with(b"# service=") {
            first = false;
            continue;
        }
        first = false;
        if let Some(err) = line.strip_prefix(b"ERR ") {
            return Err(MirrorError::Rejected(err.to_str_lossy().into_owned()));
        }
        let line = line.split_str("\0").next().unwrap_or_default();
        let Some((oid, name)) = line.split_once_str(" ") else {
            return Err(protocol(&format!("bad ref line {:?}", line.as_bstr())));
        };
        if name == b"capabilities^{}" {
            continue;
        }
        let oid = ObjectId::from_hex(oid).map_err(|e| protocol(&e))?;
        refs.insert(name.to_str_lossy().into_owned(), oid);
    }
    Ok(refs)
}

/// Parse `report-status`: `Ok` if the pack unpacked and every ref is `ok`.
pub fn parse_report(body: &[u8]) -> Result<(), MirrorError> {
    let mut reader = PktReader::new(body);
    let mut problems = Vec::new();
    let mut unpacked = false;
    while let Some(pkt) = reader
        .next_pkt()
        .map_err(|e| MirrorError::Protocol(e.to_string()))?
    {
        let Pkt::Data(_) = pkt else { continue };
        let line = pkt.line().unwrap_or_default().to_str_lossy().into_owned();
        if let Some(rest) = line.strip_prefix("unpack ") {
            unpacked = rest == "ok";
            if !unpacked {
                problems.push(format!("unpack {rest}"));
            }
        } else if let Some(rest) = line.strip_prefix("ng ") {
            problems.push(rest.to_owned());
        }
    }
    if !unpacked && problems.is_empty() {
        problems.push("no report-status in the response".into());
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(MirrorError::Rejected(problems.join("; ")))
    }
}

async fn push_mirror(app: &App, job: &MirrorJob) -> Result<String, MirrorError> {
    let repo_id = job.repo_id;
    let (record, local) = blocking(app, move |forge| {
        let record = forge.db.repo_by_id(repo_id)?;
        let local = mirrored_refs(forge, &record)?;
        Ok((record, local))
    })
    .await?;
    let token = match &job.mirror.token_env {
        Some(name) => Some(
            (app.config.secrets)(name).ok_or_else(|| MirrorError::MissingSecret(name.clone()))?,
        ),
        None => None,
    };
    let base = job.mirror.url.trim_end_matches('/').to_owned();
    if !(base.starts_with("https://") || base.starts_with("http://")) {
        return Err(MirrorError::Url(base));
    }
    let authed = |request: reqwest::RequestBuilder| match &token {
        Some(token) => {
            request.basic_auth(job.mirror.username.as_deref().unwrap_or("git"), Some(token))
        }
        None => request,
    };
    let response = authed(
        app.http
            .get(format!("{base}/info/refs?service=git-receive-pack")),
    )
    .send()
    .await?;
    if !response.status().is_success() {
        return Err(MirrorError::Status(response.status()));
    }
    let remote = parse_advertisement(&response.bytes().await?)?;
    let mut commands = Vec::new();
    for (name, oid) in &local {
        let old = remote.get(name).copied();
        if old != Some(*oid) {
            commands.push((old, *oid, name.clone()));
        }
    }
    if commands.is_empty() {
        return Ok(format!("{} refs already up to date", local.len()));
    }
    let wants: Vec<ObjectId> = commands.iter().map(|(_, new, _)| *new).collect();
    let remote_tips: Vec<ObjectId> = remote.values().copied().collect();
    let pack_bytes = blocking(app, move |forge| {
        let repo = forge.repos.open(&record.name)?;
        let haves: Vec<ObjectId> = remote_tips
            .into_iter()
            .filter(|id| repo.has_object(id))
            .collect();
        let ids = pack::objects_for_fetch(&repo, &wants, &haves)?;
        Ok(pack::write_pack(&repo, ids)?)
    })
    .await?;
    let mut body = PktWriter::new();
    let null = ObjectId::null(gix::hash::Kind::Sha1);
    for (i, (old, new, name)) in commands.iter().enumerate() {
        let old = old.unwrap_or(null);
        if i == 0 {
            let caps = concat!(
                "report-status agent=dabney-forge/",
                env!("CARGO_PKG_VERSION")
            );
            body.data(format!("{old} {new} {name}\0{caps}\n").as_bytes());
        } else {
            body.line(format!("{old} {new} {name}"));
        }
    }
    body.flush().raw(&pack_bytes);
    let response = authed(app.http.post(format!("{base}/git-receive-pack")))
        .header("content-type", "application/x-git-receive-pack-request")
        .header("accept", "application/x-git-receive-pack-result")
        .body(body.into_bytes())
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(MirrorError::Status(response.status()));
    }
    parse_report(&response.bytes().await?)?;
    Ok(format!("pushed {} refs", commands.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advertisement_and_report_parse() {
        let a = "a".repeat(40);
        let mut w = PktWriter::new();
        w.line("# service=git-receive-pack")
            .flush()
            .data(format!("{a} refs/heads/main\0report-status\n").as_bytes())
            .line(format!("{a} refs/tags/v1"))
            .flush();
        let refs = parse_advertisement(&w.into_bytes()).unwrap();
        assert_eq!(refs.len(), 2);
        let mut ok = PktWriter::new();
        ok.line("unpack ok").line("ok refs/heads/main").flush();
        assert!(parse_report(&ok.into_bytes()).is_ok());
        let mut ng = PktWriter::new();
        ng.line("unpack ok")
            .line("ng refs/heads/main non-fast-forward")
            .flush();
        assert!(
            matches!(parse_report(&ng.into_bytes()), Err(MirrorError::Rejected(r)) if r.contains("non-fast-forward"))
        );
    }
}
