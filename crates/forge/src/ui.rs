//! Server-rendered pages (maud): repositories, a repository, its pull
//! requests and one pull request with diff, reviews, checks and the merge
//! gate. Pages are read-only; changes go through Git and the API.

use std::collections::HashSet;

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::routing::get;
use axum::Router;
use maud::{html, Markup, PreEscaped, DOCTYPE};

use crate::app::{blocking, App, Forge};
use crate::auth::{self, Action};
use crate::db::{InlineComment, PrState, RepoRecord, Verdict};
use crate::diff::{ChangeKind, FileBody, FileDiff, LineKind};
use crate::error::ForgeError;
use crate::pr::{self, PrView};
use crate::repo::{self, PathContent, TreeEntryKind};

pub fn routes() -> Router<App> {
    Router::new()
        .route("/", get(index))
        .route("/{owner}/{repo}", get(repository))
        .route("/{owner}/{repo}/pulls", get(pulls))
        .route("/{owner}/{repo}/pull/{number}", get(pull))
}

const STYLE: &str = r#"
:root{--fg:#1f2328;--muted:#59636e;--line:#d1d9e0;--bg:#f6f8fa;--add:#dafbe1;--del:#ffebe9;--accent:#0969da;--ok:#1a7f37;--bad:#cf222e}
*{box-sizing:border-box}body{margin:0;font:14px/1.5 system-ui,-apple-system,"Segoe UI",sans-serif;color:var(--fg)}
header{border-bottom:1px solid var(--line);background:var(--bg);padding:12px 24px}header a{font-weight:600;color:var(--fg);text-decoration:none}
main{max-width:1100px;margin:0 auto;padding:24px}a{color:var(--accent)}h1{font-size:22px;margin:0 0 8px}h2{font-size:16px;margin:24px 0 8px}
.muted{color:var(--muted)}.box{border:1px solid var(--line);border-radius:6px;margin:12px 0;overflow:hidden}
.box>.head{background:var(--bg);border-bottom:1px solid var(--line);padding:8px 12px;font-weight:600}
ul.rows{list-style:none;margin:0;padding:0}ul.rows li{padding:8px 12px;border-top:1px solid var(--line)}ul.rows li:first-child{border-top:0}
code,pre,.diff{font:12px/1.45 ui-monospace,SFMono-Regular,Menlo,monospace}
.pill{display:inline-block;padding:2px 10px;border-radius:12px;font-size:12px;font-weight:600;color:#fff}
.open{background:var(--ok)}.merged{background:#8250df}.closed{background:var(--bad)}
.ready{color:var(--ok)}.blocked{color:var(--bad)}
table.diff{border-collapse:collapse;width:100%}table.diff td{padding:0 8px;white-space:pre-wrap;word-break:break-all;vertical-align:top}
td.n{width:1%;color:var(--muted);text-align:right;user-select:none}tr.add td{background:var(--add)}tr.del td{background:var(--del)}
tr.hunk td{background:#ddf4ff;color:var(--muted)}tr.note td{background:#fff8c5;font-family:system-ui,sans-serif;padding:6px 12px}
.clone{background:var(--bg);border:1px solid var(--line);border-radius:6px;padding:6px 10px}
"#;

fn page(title: &str, body: Markup) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { (title) " · git.dabney.moe" }
                style { (PreEscaped(STYLE)) }
            }
            body {
                header { a href="/" { "git.dabney.moe" } }
                main { (body) }
            }
        }
    }
}

fn state_pill(state: &PrState) -> Markup {
    html! { span class={ "pill " (state.label()) } { (state.label()) } }
}

fn visible(
    forge: &Forge,
    headers: &HeaderMap,
    owner: &str,
    repo: &str,
) -> Result<RepoRecord, ForgeError> {
    let principal = auth::authenticate(&forge.db, headers)?;
    forge.authorize(owner, repo, principal.as_ref(), Action::Read)
}

async fn index(State(app): State<App>, headers: HeaderMap) -> Result<Markup, ForgeError> {
    blocking(&app, move |forge| {
        let principal = auth::authenticate(&forge.db, &headers)?;
        let mut repos = Vec::new();
        for r in forge.db.repos()? {
            if auth::can(&forge.db, principal.as_ref(), &r, Action::Read)? {
                repos.push(r);
            }
        }
        Ok(page(
            "Repositories",
            html! {
                h1 { "Repositories" }
                div class="box" {
                    ul class="rows" {
                        @for r in &repos {
                            li {
                                a href={ "/" (r.name) } { (r.name) }
                                @if r.private { " " span class="muted" { "private" } }
                            }
                        }
                        @if repos.is_empty() { li class="muted" { "No repositories yet." } }
                    }
                }
            },
        ))
    })
    .await
}

fn repo_nav(record: &RepoRecord) -> Markup {
    html! {
        h1 { a href={ "/" (record.name) } { (record.name) } }
        p class="muted" {
            a href={ "/" (record.name) } { "Code" } " · "
            a href={ "/" (record.name) "/pulls" } { "Pull requests" }
        }
    }
}

async fn repository(
    State(app): State<App>,
    Path((owner, name)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Markup, ForgeError> {
    blocking(&app, move |forge| {
        let record = visible(forge, &headers, &owner, &name)?;
        let git = forge.repos.open(&record.name)?;
        let tip = repo::branch_tip(&git, &record.default_branch)?;
        let (entries, commits) = match tip {
            Some(tip) => {
                let entries = match repo::read_path(&git, tip, "")? {
                    PathContent::Tree(entries) => entries,
                    PathContent::Blob { .. } => Vec::new(),
                };
                (entries, repo::log(&git, tip, 10)?)
            }
            None => (Vec::new(), Vec::new()),
        };
        let open = forge
            .db
            .prs(record.id)?
            .into_iter()
            .filter(|p| p.state == PrState::Open)
            .count();
        let mirrors = forge.db.mirror_status(record.id)?;
        let clone_url = forge.config.link(&format!("{}.git", record.name));
        Ok(page(
            &record.name.to_string(),
            html! {
                (repo_nav(&record))
                p { "Clone: " code class="clone" { (clone_url) } }
                p class="muted" { "Default branch " code { (record.default_branch) } " · " (open) " open pull requests" }
                @if tip.is_none() {
                    div class="box" { div class="head" { "Empty repository" }
                        ul class="rows" { li { "Push a branch to get started: " code { "git push " (clone_url) " " (record.default_branch) } } } }
                } @else {
                    div class="box" {
                        div class="head" { "Files" }
                        ul class="rows" {
                            @for e in &entries {
                                li {
                                    @match e.kind { TreeEntryKind::Tree => { "📁 " }, TreeEntryKind::Submodule => { "📦 " }, _ => { "📄 " } }
                                    (e.name)
                                }
                            }
                        }
                    }
                    div class="box" {
                        div class="head" { "Recent commits" }
                        ul class="rows" {
                            @for c in &commits {
                                li { code { (c.short) } " " (c.summary) " " span class="muted" { "— " (c.author) ", " (c.time) } }
                            }
                        }
                    }
                }
                @if !mirrors.is_empty() {
                    div class="box" {
                        div class="head" { "Mirrors" }
                        ul class="rows" {
                            @for m in &mirrors {
                                li { (m.mirror.name) " " span class="muted" { (m.mirror.url) } " — " (m.status.as_str())
                                    @if let Some(e) = &m.last_error { " " span class="blocked" { (e) } } }
                            }
                        }
                    }
                }
            },
        ))
    })
    .await
}

async fn pulls(
    State(app): State<App>,
    Path((owner, name)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Markup, ForgeError> {
    blocking(&app, move |forge| {
        let record = visible(forge, &headers, &owner, &name)?;
        let prs = forge.db.prs(record.id)?;
        Ok(page(
            &format!("Pull requests · {}", record.name),
            html! {
                (repo_nav(&record))
                div class="box" {
                    div class="head" { "Pull requests" }
                    ul class="rows" {
                        @for p in &prs {
                            li {
                                (state_pill(&p.state)) " "
                                a href={ "/" (record.name) "/pull/" (p.number) } { (p.title) }
                                " " span class="muted" { "#" (p.number) " by " (p.author) ": " (p.source_branch) " → " (p.target_branch) }
                            }
                        }
                        @if prs.is_empty() { li class="muted" { "No pull requests." } }
                    }
                }
            },
        ))
    })
    .await
}

fn file_diff(view: &PrView, file: &FileDiff, rendered: &mut HashSet<i64>) -> Markup {
    let head = view.pr.head_oid;
    let body = match &file.body {
        FileBody::Binary => html! { p class="muted" { "Binary file" } },
        FileBody::TooLarge => html! { p class="muted" { "File too large to display" } },
        FileBody::Submodule => html! { p class="muted" { "Submodule change" } },
        FileBody::Text { hunks } => {
            let mut rows = Vec::new();
            for h in hunks {
                rows.push(html! {
                    tr class="hunk" { td class="n" {} td class="n" {} td { (h.header()) } }
                });
                for l in &h.lines {
                    let (class, mark) = match l.kind {
                        LineKind::Added => ("add", "+"),
                        LineKind::Removed => ("del", "-"),
                        LineKind::Context => ("ctx", " "),
                    };
                    rows.push(html! {
                        tr class=(class) {
                            td class="n" { @if let Some(n) = l.old { (n) } }
                            td class="n" { @if let Some(n) = l.new { (n) } }
                            td { (mark) (l.text) }
                        }
                    });
                    let notes: Vec<&InlineComment> = view
                        .comments
                        .iter()
                        .filter(|c| {
                            c.anchor.path == file.path
                                && l.matches_anchor(c.anchor.side, c.anchor.line)
                        })
                        .collect();
                    for c in notes {
                        rendered.insert(c.id);
                        rows.push(html! {
                            tr class="note" { td colspan="3" {
                                strong { (c.author) } ": " (c.body)
                                @if c.anchor.head_oid != head { " " span class="muted" { "(outdated: on " (c.anchor.head_oid.to_hex_with_len(10)) ")" } }
                            } }
                        });
                    }
                }
            }
            html! { table class="diff" { @for row in rows { (row) } } }
        }
    };
    html! {
        div class="box" id={ "file-" (file.path) } {
            div class="head" {
                code { (file.path) }
                @match &file.change {
                    ChangeKind::Added => { " " span class="muted" { "added" } },
                    ChangeKind::Deleted => { " " span class="muted" { "deleted" } },
                    ChangeKind::Renamed { from } => { " " span class="muted" { "renamed from " (from) } },
                    ChangeKind::Modified => {},
                }
                " " span class="ready" { "+" (file.added()) } " " span class="blocked" { "−" (file.removed()) }
            }
            (body)
        }
    }
}

async fn pull(
    State(app): State<App>,
    Path((owner, name, number)): Path<(String, String, u64)>,
    headers: HeaderMap,
) -> Result<Markup, ForgeError> {
    blocking(&app, move |forge| {
        let record = visible(forge, &headers, &owner, &name)?;
        let view = pr::view(forge, &record, number)?;
        let p = &view.pr;
        let r = &view.readiness;
        let mut rendered_comments = HashSet::new();
        let file_sections: Vec<Markup> = view
            .files
            .iter()
            .map(|f| file_diff(&view, f, &mut rendered_comments))
            .collect();
        let outdated_comments: Vec<&InlineComment> = view
            .comments
            .iter()
            .filter(|c| !rendered_comments.contains(&c.id))
            .collect();
        Ok(page(
            &format!("{} · #{}", p.title, p.number),
            html! {
                (repo_nav(&record))
                h1 { (p.title) " " span class="muted" { "#" (p.number) } }
                p { (state_pill(&p.state)) " " (p.author) " wants to merge " code { (p.source_branch) } " into " code { (p.target_branch) }
                    " · head " code { (p.head_oid.to_hex_with_len(10)) } }
                @if let PrState::Merged { commit } = &p.state { p { "Merged as " code { (commit) } } }
                @if !p.body.is_empty() { div class="box" { ul class="rows" { li { (p.body) } } } }
                div class="box" {
                    div class="head" {
                        @if r.is_ready() { span class="ready" { "✓ Ready to merge" } }
                        @else if p.state == PrState::Open { span class="blocked" { "✗ Merge blocked" } }
                        @else { "Merge gate" }
                    }
                    ul class="rows" {
                        @for b in &r.blockers { li class="blocked" { (b) } }
                        li { "Approvals on current head: " @if r.approvals.is_empty() { span class="muted" { "none" } } @else { (r.approvals.join(", ")) } }
                        @if !r.stale_approvals.is_empty() { li class="muted" { "Stale approvals (earlier head): " (r.stale_approvals.join(", ")) } }
                    }
                }
                div class="box" {
                    div class="head" { "Checks on " code { (p.head_oid.to_hex_with_len(10)) } }
                    ul class="rows" {
                        @for s in &view.statuses {
                            li { strong { (s.context) } " " (s.state.as_str())
                                @if let Some(d) = &s.description { " — " (d) }
                                @if let Some(u) = s.target_url.as_deref().filter(|u| pr::is_safe_http_url(u)) {
                                    " " a href=(u) rel="noopener noreferrer" { "details" }
                                }
                            }
                        }
                        @if view.statuses.is_empty() { li class="muted" { "No checks reported." } }
                    }
                }
                div class="box" {
                    div class="head" { "Reviews" }
                    ul class="rows" {
                        @for rv in &view.reviews {
                            li { strong { (rv.author) } " "
                                @match rv.verdict { Verdict::Approve => { span class="ready" { "approved" } }, Verdict::RequestChanges => { span class="blocked" { "requested changes" } }, Verdict::Comment => { "commented" } }
                                " on " code { (rv.head_oid.to_hex_with_len(10)) }
                                @if rv.head_oid != p.head_oid { " " span class="muted" { "(stale)" } }
                                @if !rv.body.is_empty() { ": " (rv.body) } }
                        }
                        @if view.reviews.is_empty() { li class="muted" { "No reviews yet." } }
                    }
                }
                @if !outdated_comments.is_empty() {
                    div class="box" {
                        div class="head" { "Outdated comments" }
                        ul class="rows" {
                            @for c in &outdated_comments {
                                li {
                                    strong { (c.author) } " on " code { (c.anchor.path) ":" (c.anchor.line) }
                                    " (" (c.anchor.side.as_str()) ", " code { (c.anchor.head_oid.to_hex_with_len(10)) } "): "
                                    (c.body)
                                }
                            }
                        }
                    }
                }
                h2 {
                    "Files changed (" (view.files.len()) ") "
                    span class="muted" {
                        @if let Some(base) = view.base { "from " code { (base.to_hex_with_len(10)) } }
                        @else { "unrelated history" }
                    }
                }
                @for section in file_sections { (section) }
            },
        ))
    })
    .await
}
