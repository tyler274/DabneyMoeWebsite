//! End to end against the real `git` CLI: clone, authenticated push, a pull
//! request through diff, inline comment, review, force-push staleness, CI
//! status and merge, then a fresh clone, the mirror and the signed webhook
//! that moves the Linear issue to Done.
//!
//! Only this test spawns `git`; the server never does.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Arc;
use std::time::Duration;

use forge::app::{Forge, ForgeConfig, WebhookTarget};
use forge::auth::{issue_token, parse_scopes};
use forge::db::JobStatus;
use forge::repo::RepoName;
use forge_linear_sync::config::{LinearConfig, RepoConfig, RetryPolicy, SyncConfig};
use forge_linear_sync::domain::{Authority, Forge as ForgeKind, RepoPath, TeamKey};
use forge_linear_sync::mock::MockLinear;
use forge_linear_sync::queue::EventStore;
use forge_linear_sync::verify::Secret;
use forge_linear_sync::Bridge;
use serde_json::{json, Value};

const HOOK_SECRET: &str = "dabney-hook-secret";

struct GitEnv {
    home: PathBuf,
}

impl GitEnv {
    fn new(root: &Path) -> Self {
        let home = root.join("home");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(
            home.join(".gitconfig"),
            "[user]\n\tname = Tyler\n\temail = tyler@example.com\n[init]\n\tdefaultBranch = main\n\
             [protocol]\n\tversion = 2\n[credential]\n\thelper =\n[advice]\n\tdetachedHead = false\n",
        )
        .unwrap();
        GitEnv { home }
    }

    fn run(&self, dir: &Path, args: &[&str]) -> Output {
        Command::new("git")
            .args(args)
            .current_dir(dir)
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", &self.home)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_TRACE_CURL_NO_DATA", "1")
            .output()
            .expect("git runs")
    }

    #[track_caller]
    fn ok(&self, dir: &Path, args: &[&str]) -> String {
        let out = self.run(dir, args);
        assert!(
            out.status.success(),
            "git {args:?} failed\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    }
}

struct Api {
    http: reqwest::Client,
    base: String,
}

impl Api {
    async fn call(
        &self,
        method: reqwest::Method,
        path: &str,
        token: Option<&str>,
        body: Option<Value>,
    ) -> (u16, Value) {
        let mut request = self.http.request(method, format!("{}{path}", self.base));
        if let Some(token) = token {
            request = request.header("authorization", format!("token {token}"));
        }
        if let Some(body) = body {
            request = request
                .header("content-type", "application/json")
                .body(serde_json::to_vec(&body).unwrap());
        }
        let response = request.send().await.unwrap();
        let status = response.status().as_u16();
        let text = response.text().await.unwrap();
        (
            status,
            serde_json::from_str(&text).unwrap_or(Value::String(text)),
        )
    }

    async fn get(&self, path: &str) -> (u16, Value) {
        self.call(reqwest::Method::GET, path, None, None).await
    }

    async fn post(&self, path: &str, token: Option<&str>, body: Value) -> (u16, Value) {
        self.call(reqwest::Method::POST, path, token, Some(body))
            .await
    }
}

async fn wait_for(what: &str, mut check: impl FnMut() -> bool) {
    for _ in 0..200 {
        if check() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("timed out waiting for {what}");
}

fn write(path: &Path, text: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, text).unwrap();
}

fn blockers(readiness: &Value) -> Vec<String> {
    readiness["blockers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["kind"].as_str().unwrap().to_owned())
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn git_cli_clone_push_review_merge_mirror_and_linear() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let git = GitEnv::new(root);

    // Mock Linear and the bridge, mounted on the forge itself.
    let linear = MockLinear::start().await;
    linear.add_issue("COD-103");
    let bridge = Bridge::new(
        SyncConfig {
            teams: vec![TeamKey::parse("COD").unwrap()],
            repos: vec![RepoConfig {
                forge: ForgeKind::Dabney,
                path: RepoPath::parse("tyler/site").unwrap(),
                authority: Authority::Canonical,
                default_branch: "main".into(),
                secrets: vec![Secret::new(HOOK_SECRET)],
            }],
            linear: LinearConfig {
                endpoint: linear.endpoint.clone(),
                api_key: Secret::new("lin_api_test"),
            },
            retry: RetryPolicy {
                max_attempts: 5,
                base_delay: Duration::from_millis(10),
                max_delay: Duration::from_millis(50),
            },
        },
        EventStore::in_memory().unwrap(),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base = format!("http://{addr}");
    let mirror_token = Arc::new(std::sync::Mutex::new(String::new()));
    let mut config = ForgeConfig::new(base.parse().unwrap());
    config.webhooks = vec![WebhookTarget {
        url: format!("{base}/hooks/forge/dabney").parse().unwrap(),
        secret: Secret::new(HOOK_SECRET),
    }];
    let secrets = mirror_token.clone();
    config.secrets =
        Arc::new(move |name| (name == "MIRROR_TOKEN").then(|| secrets.lock().unwrap().clone()));
    let app = Forge::open(&root.join("data"), config).unwrap();

    let site = RepoName::parse("tyler/site").unwrap();
    let record = app.create_repo(&site, false, "main").unwrap();
    app.create_repo(&RepoName::parse("tyler/site-mirror").unwrap(), true, "main")
        .unwrap();
    app.db.add_collaborator(record.id, "bob").unwrap();
    app.db.add_collaborator(record.id, "ci").unwrap();
    app.db
        .set_repo_policy(record.id, 1, &["ci/test".to_owned()])
        .unwrap();
    let tyler = issue_token(
        &app.db,
        "tyler",
        "laptop",
        &parse_scopes("read,write").unwrap(),
    )
    .unwrap();
    let bob = issue_token(
        &app.db,
        "bob",
        "laptop",
        &parse_scopes("read,write").unwrap(),
    )
    .unwrap();
    let ci = issue_token(&app.db, "ci", "runner", &parse_scopes("status").unwrap()).unwrap();
    let outsider_ci = issue_token(
        &app.db,
        "outsider",
        "runner",
        &parse_scopes("status").unwrap(),
    )
    .unwrap();
    *mirror_token.lock().unwrap() = tyler.clone();
    app.db
        .add_mirror(
            record.id,
            "backup",
            &format!("{base}/tyler/site-mirror.git"),
            Some("tyler"),
            Some("MIRROR_TOKEN"),
        )
        .unwrap();

    forge::spawn_workers(&app).unwrap();
    tokio::spawn(bridge.worker.run());
    let router = forge::router(app.clone()).merge(bridge.router);
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });

    let api = Api {
        http: reqwest::Client::new(),
        base: base.clone(),
    };
    assert_eq!(api.get("/healthz").await, (200, Value::String("ok".into())));

    // 1. Clone the empty repository over protocol v2.
    let clone_url = format!("{base}/tyler/site.git");
    let out = git.run(root, &["clone", &clone_url, "work"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stderr).contains("empty repository"));
    let work = root.join("work");
    assert_eq!(git.ok(&work, &["symbolic-ref", "HEAD"]), "refs/heads/main");

    // 2. Commit on main; pushing without credentials gets 401 and fails.
    write(
        &work.join("README.md"),
        "# Site\n\nline one\nline two\nline three\n",
    );
    write(&work.join("src/app.txt"), "fn main() {}\n");
    git.ok(&work, &["add", "-A"]);
    git.ok(&work, &["commit", "-m", "Initial commit"]);
    let advert = api
        .http
        .get(format!("{clone_url}/info/refs?service=git-receive-pack"))
        .send()
        .await
        .unwrap();
    assert_eq!(advert.status().as_u16(), 401);
    let challenge = advert.headers()["www-authenticate"]
        .to_str()
        .unwrap()
        .to_owned();
    assert!(challenge.starts_with("Basic"), "{challenge}");
    let anonymous = git.run(&work, &["push", "origin", "main"]);
    assert!(!anonymous.status.success(), "anonymous push must fail");
    let bad_url = format!("http://tyler:dfp_0000000000000000_bad@{addr}/tyler/site.git");
    let bad = git.run(&work, &["push", &bad_url, "main"]);
    assert!(!bad.status.success(), "push with a wrong token must fail");
    assert!(
        git.ok(&work, &["ls-remote", &clone_url]).is_empty(),
        "nothing was pushed"
    );

    // 3. Push main and a feature branch with the PAT.
    let push_url = format!("http://tyler:{tyler}@{addr}/tyler/site.git");
    git.ok(&work, &["remote", "set-url", "origin", &push_url]);
    git.ok(&work, &["push", "origin", "main"]);
    let main_tip = git.ok(&work, &["rev-parse", "HEAD"]);
    let remote = git.ok(&work, &["ls-remote", &clone_url]);
    assert!(
        remote.contains(&format!("{main_tip}\trefs/heads/main")),
        "{remote}"
    );
    assert!(remote.contains(&format!("{main_tip}\tHEAD")), "{remote}");

    git.ok(&work, &["checkout", "-b", "cod-103-forge"]);
    write(
        &work.join("README.md"),
        "# Site\n\nline one\nline 2, revised\nline three\nline four\n",
    );
    write(&work.join("docs/forge.md"), "Pure-Rust forge.\n");
    git.ok(&work, &["add", "-A"]);
    git.ok(&work, &["commit", "-m", "COD-103: describe the forge"]);
    git.ok(&work, &["push", "-u", "origin", "cod-103-forge"]);
    let first_head = git.ok(&work, &["rev-parse", "HEAD"]);

    // 4. Open the pull request; its diff renders.
    let pulls = "/api/v1/repos/tyler/site/pulls";
    let (status, pr) = api
        .post(
            pulls,
            Some(&tyler),
            json!({"title": "COD-103: pure-Rust forge", "body": "Implements COD-103.", "head": "cod-103-forge", "base": "main"}),
        )
        .await;
    assert_eq!(status, 201, "{pr}");
    assert_eq!(pr["number"], 1);
    assert_eq!(pr["head"]["sha"], first_head.as_str());
    let (status, files) = api.get(&format!("{pulls}/1/files")).await;
    assert_eq!(status, 200);
    let files = files.as_array().unwrap();
    let paths: Vec<_> = files.iter().map(|f| f["path"].as_str().unwrap()).collect();
    assert_eq!(paths, vec!["README.md", "docs/forge.md"]);
    let page = api
        .http
        .get(format!("{base}/tyler/site/pull/1"))
        .send()
        .await
        .unwrap();
    assert_eq!(page.status().as_u16(), 200);
    let html = page.text().await.unwrap();
    assert!(
        html.contains("line 2, revised") && html.contains("docs/forge.md"),
        "diff renders"
    );
    assert!(html.contains("Merge blocked"));
    for path in ["/", "/tyler/site", "/tyler/site/pulls"] {
        let response = api.http.get(format!("{base}{path}")).send().await.unwrap();
        assert_eq!(response.status().as_u16(), 200, "{path}");
    }

    // 5. Inline comment on an added line, then approval.
    let comments = format!("{pulls}/1/comments");
    let (status, comment) = api
        .post(
            &comments,
            Some(&bob),
            json!({"path": "README.md", "side": "new", "line": 4, "body": "Nice wording."}),
        )
        .await;
    assert_eq!(status, 201, "{comment}");
    assert_eq!(comment["commit_id"], first_head.as_str());
    let (status, _) = api
        .post(
            &comments,
            Some(&bob),
            json!({"path": "README.md", "side": "new", "line": 99, "body": "nowhere"}),
        )
        .await;
    assert_eq!(status, 400, "a line outside the diff is refused");
    let reviews = format!("{pulls}/1/reviews");
    let (status, _) = api
        .post(&reviews, Some(&tyler), json!({"event": "APPROVE"}))
        .await;
    assert_eq!(status, 403, "authors cannot approve their own pull request");
    let (status, _) = api
        .post(
            &reviews,
            Some(&bob),
            json!({"event": "APPROVE", "commit_id": first_head}),
        )
        .await;
    assert_eq!(status, 201);
    let readiness = format!("{pulls}/1/readiness");
    let (_, ready) = api.get(&readiness).await;
    assert_eq!(blockers(&ready), vec!["check_missing"], "{ready}");

    // 6. A force-push makes the approval stale and blocks the merge.
    write(
        &work.join("docs/forge.md"),
        "Pure-Rust forge, gitoxide inside.\n",
    );
    git.ok(&work, &["add", "-A"]);
    git.ok(
        &work,
        &[
            "commit",
            "--amend",
            "-m",
            "COD-103: describe the forge (gitoxide)",
        ],
    );
    git.ok(&work, &["push", "--force", "origin", "cod-103-forge"]);
    let new_head = git.ok(&work, &["rev-parse", "HEAD"]);
    assert_ne!(new_head, first_head);
    let (_, ready) = api.get(&readiness).await;
    assert_eq!(ready["head"], new_head.as_str());
    assert_eq!(ready["stale_approvals"], json!(["bob"]));
    assert_eq!(
        blockers(&ready),
        vec!["needs_approvals", "check_missing"],
        "{ready}"
    );
    let (status, refused) = api
        .post(&format!("{pulls}/1/merge"), Some(&tyler), json!({}))
        .await;
    assert_eq!(status, 409, "{refused}");
    let (status, _) = api
        .post(
            &reviews,
            Some(&bob),
            json!({"event": "APPROVE", "commit_id": first_head}),
        )
        .await;
    assert_eq!(status, 409, "an approval naming the old head is refused");
    let html = api
        .http
        .get(format!("{base}/tyler/site/pull/1"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(html.contains("outdated") && html.contains("Stale approvals"));

    // 7. Re-approve, report CI, merge.
    let (status, _) = api
        .post(
            &reviews,
            Some(&bob),
            json!({"event": "APPROVE", "commit_id": new_head}),
        )
        .await;
    assert_eq!(status, 201);
    let statuses = format!("/api/v1/repos/tyler/site/statuses/{new_head}");
    let passing = json!({
        "state": "success",
        "context": "ci/test",
        "description": "all green",
        "target_url": "https://ci.dabney.moe/runs/42"
    });
    assert_eq!(api.post(&statuses, None, passing.clone()).await.0, 401);
    assert_eq!(
        api.post(&statuses, Some(&bob), passing.clone()).await.0,
        403,
        "bob's token has no status scope"
    );
    assert_eq!(
        api.post(&statuses, Some(&outsider_ci), passing.clone())
            .await
            .0,
        403,
        "non-member with status scope cannot spoof statuses on a public repo"
    );
    assert_eq!(
        api.post(
            &statuses,
            Some(&ci),
            json!({"state": "success", "context": "ci/test", "target_url": "javascript:alert(1)"})
        )
        .await
        .0,
        400,
        "non-http(s) target_url must be rejected"
    );
    assert_eq!(api.post(&statuses, Some(&ci), passing).await.0, 201);
    let (_, ready) = api.get(&readiness).await;
    assert!(ready["blockers"].as_array().unwrap().is_empty(), "{ready}");
    let (status, merged) = api
        .post(
            &format!("{pulls}/1/merge"),
            Some(&tyler),
            json!({"sha": new_head}),
        )
        .await;
    assert_eq!(status, 200, "{merged}");
    assert_eq!(merged["state"], "merged");
    let merge_sha = merged["merge_commit_sha"].as_str().unwrap().to_owned();

    // 8. A fresh clone has the merge commit and the merged files.
    let out = git.run(root, &["clone", &clone_url, "fresh"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let fresh = root.join("fresh");
    assert_eq!(git.ok(&fresh, &["rev-parse", "HEAD"]), merge_sha);
    let parents = git.ok(&fresh, &["log", "-1", "--format=%P"]);
    assert_eq!(parents, format!("{main_tip} {new_head}"));
    assert_eq!(
        std::fs::read_to_string(fresh.join("README.md")).unwrap(),
        "# Site\n\nline one\nline 2, revised\nline three\nline four\n"
    );
    assert_eq!(
        std::fs::read_to_string(fresh.join("docs/forge.md")).unwrap(),
        "Pure-Rust forge, gitoxide inside.\n"
    );
    git.ok(&fresh, &["fsck", "--strict", "--no-dangling"]);
    // An incremental fetch negotiates with haves.
    git.ok(&work, &["fetch", "origin"]);
    assert_eq!(git.ok(&work, &["rev-parse", "origin/main"]), merge_sha);

    // 9. The signed webhook moved COD-103 to Done through forge-linear-sync.
    wait_for("COD-103 to reach Done", || {
        linear.issue_state("COD-103") == "Done"
    })
    .await;
    let attachments = linear.attachments("COD-103");
    assert_eq!(attachments.len(), 1, "one attachment for the pull request");
    assert_eq!(attachments[0].url, format!("{base}/tyler/site/pull/1"));
    let pr_id = app.db.pr(record.id, 1).unwrap().unwrap().id;
    let deliveries = app.db.deliveries(pr_id).unwrap();
    let actions: Vec<_> = deliveries.iter().map(|d| d.action.as_str()).collect();
    assert_eq!(actions, vec!["opened", "synchronized", "closed"]);
    assert!(
        deliveries.iter().all(|d| d.http_status == Some(202)),
        "{deliveries:?}"
    );

    // 10. The mirror received main.
    let mirror_record = app
        .repo_record(&RepoName::parse("tyler/site-mirror").unwrap())
        .unwrap();
    wait_for("the mirror push", || {
        let jobs = app.db.mirror_status(record.id).unwrap();
        jobs.iter().any(|j| j.status == JobStatus::Succeeded) && {
            let repo = app.repos.open(&mirror_record.name).unwrap();
            forge::repo::branch_tip(&repo, "main")
                .unwrap()
                .map(|id| id.to_string())
                == Some(merge_sha.clone())
        }
    })
    .await;
    let mirror_url = format!("http://tyler:{tyler}@{addr}/tyler/site-mirror.git");
    let mirrored: HashMap<_, _> = git
        .ok(root, &["ls-remote", &mirror_url])
        .lines()
        .map(|l| {
            let (oid, name) = l.split_once('\t').unwrap();
            (name.to_owned(), oid.to_owned())
        })
        .collect();
    assert_eq!(mirrored.get("refs/heads/main"), Some(&merge_sha));
    assert!(
        !mirrored.contains_key("refs/heads/cod-103-forge"),
        "only main and tags are mirrored"
    );
}

/// The server's source never runs a subprocess; Git work is gitoxide's.
#[test]
fn forge_source_spawns_no_processes() {
    fn visit(dir: &Path, hits: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(&path, hits);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let text = std::fs::read_to_string(&path).unwrap();
                for needle in [
                    "std::process::Command",
                    "process::Command",
                    "Command::new(",
                    "tokio::process",
                ] {
                    if text.contains(needle) {
                        hits.push(format!("{}: {needle}", path.display()));
                    }
                }
            }
        }
    }
    let mut hits = Vec::new();
    visit(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut hits,
    );
    assert!(hits.is_empty(), "{hits:?}");
}

/// B2 + B3: Two repos hosted on one `dabney-forge` share a webhook secret and
/// both sync to Linear without `RepoMismatch`; `backfill_forgejo` paginates
/// `dabney-forge`'s `/api/v1/repos/.../pulls` cleanly with RFC 3339 timestamps.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn multi_repo_webhooks_and_backfill_against_dabney_forge() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let git = GitEnv::new(root);

    let linear = MockLinear::start().await;
    for key in ["COD-201", "COD-202", "COD-203", "COD-204"] {
        linear.add_issue(key);
    }
    let repos_cfg = vec![
        RepoConfig {
            forge: ForgeKind::Dabney,
            path: RepoPath::parse("tyler/site").unwrap(),
            authority: Authority::Canonical,
            default_branch: "main".into(),
            secrets: vec![Secret::new(HOOK_SECRET)],
        },
        RepoConfig {
            forge: ForgeKind::Dabney,
            path: RepoPath::parse("tyler/dabney-moe").unwrap(),
            authority: Authority::Canonical,
            default_branch: "main".into(),
            secrets: vec![Secret::new(HOOK_SECRET)],
        },
    ];
    let sync_cfg = SyncConfig {
        teams: vec![TeamKey::parse("COD").unwrap()],
        repos: repos_cfg,
        linear: LinearConfig {
            endpoint: linear.endpoint.clone(),
            api_key: Secret::new("lin_api_test"),
        },
        retry: RetryPolicy {
            max_attempts: 3,
            base_delay: Duration::from_millis(10),
            max_delay: Duration::from_millis(50),
        },
    };
    let bridge = Bridge::new(sync_cfg.clone(), EventStore::in_memory().unwrap());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base = format!("http://{addr}");
    let mut config = ForgeConfig::new(base.parse().unwrap());
    config.webhooks = vec![WebhookTarget {
        url: format!("{base}/hooks/forge/dabney").parse().unwrap(),
        secret: Secret::new(HOOK_SECRET),
    }];
    let app = Forge::open(&root.join("data"), config).unwrap();
    let site = app
        .create_repo(&RepoName::parse("tyler/site").unwrap(), false, "main")
        .unwrap();
    let curie = app
        .create_repo(&RepoName::parse("tyler/dabney-moe").unwrap(), false, "main")
        .unwrap();
    app.db.add_collaborator(site.id, "bob").unwrap();
    app.db.add_collaborator(curie.id, "bob").unwrap();
    app.db.set_repo_policy(site.id, 1, &[]).unwrap();
    app.db.set_repo_policy(curie.id, 1, &[]).unwrap();
    let tyler = issue_token(
        &app.db,
        "tyler",
        "laptop",
        &parse_scopes("read,write").unwrap(),
    )
    .unwrap();
    let bob = issue_token(
        &app.db,
        "bob",
        "laptop",
        &parse_scopes("read,write").unwrap(),
    )
    .unwrap();

    forge::spawn_workers(&app).unwrap();
    tokio::spawn(bridge.worker.run());
    let router = forge::router(app.clone()).merge(bridge.router);
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let api = Api {
        http: reqwest::Client::new(),
        base: base.clone(),
    };

    // Seed both repos and push branches.
    for (name, dir, branches) in [
        (
            "tyler/site",
            "site-work",
            vec![("b1", "f1.txt"), ("b2", "f2.txt"), ("b3", "f3.txt")],
        ),
        ("tyler/dabney-moe", "curie-work", vec![("c1", "c1.txt")]),
    ] {
        let url = format!("http://tyler:{tyler}@{addr}/{name}.git");
        git.ok(root, &["clone", &url, dir]);
        let w = root.join(dir);
        write(&w.join("README.md"), "base\n");
        git.ok(&w, &["add", "-A"]);
        git.ok(&w, &["commit", "-m", "init"]);
        git.ok(&w, &["push", "origin", "main"]);
        for (br, file) in branches {
            git.ok(&w, &["checkout", "-b", br, "main"]);
            write(&w.join(file), &format!("content {br}\n"));
            git.ok(&w, &["add", "-A"]);
            git.ok(&w, &["commit", "-m", &format!("add {file}")]);
            git.ok(&w, &["push", "origin", br]);
        }
    }

    // Open 3 PRs in repo 1 (`tyler/site`) and 1 PR in repo 2 (`tyler/dabney-moe`).
    let site_pulls = "/api/v1/repos/tyler/site/pulls";
    assert_eq!(
        api.post(
            site_pulls,
            Some(&tyler),
            json!({"title": "COD-201: merged pr", "head": "b1"})
        )
        .await
        .0,
        201
    );
    assert_eq!(
        api.post(
            site_pulls,
            Some(&tyler),
            json!({"title": "COD-202: closed pr", "head": "b2"})
        )
        .await
        .0,
        201
    );
    assert_eq!(
        api.post(
            site_pulls,
            Some(&tyler),
            json!({"title": "COD-203: open pr", "head": "b3"})
        )
        .await
        .0,
        201
    );
    assert_eq!(
        api.post(
            "/api/v1/repos/tyler/dabney-moe/pulls",
            Some(&tyler),
            json!({"title": "COD-204: second repo pr", "head": "c1"})
        )
        .await
        .0,
        201
    );

    // Approve + merge PR #1 in site, close PR #2, approve + merge PR #1 in dabney-moe.
    assert_eq!(
        api.post(
            &format!("{site_pulls}/1/reviews"),
            Some(&bob),
            json!({"event": "APPROVE"})
        )
        .await
        .0,
        201
    );
    assert_eq!(
        api.post(&format!("{site_pulls}/1/merge"), Some(&tyler), json!({}))
            .await
            .0,
        200
    );
    assert_eq!(
        api.post(&format!("{site_pulls}/2/close"), Some(&tyler), json!({}))
            .await
            .0,
        200
    );
    assert_eq!(
        api.post(
            "/api/v1/repos/tyler/dabney-moe/pulls/1/reviews",
            Some(&bob),
            json!({"event": "APPROVE"})
        )
        .await
        .0,
        201
    );
    assert_eq!(
        api.post(
            "/api/v1/repos/tyler/dabney-moe/pulls/1/merge",
            Some(&tyler),
            json!({})
        )
        .await
        .0,
        200
    );

    // Second repo (`tyler/dabney-moe`) MUST reach Done without `RepoMismatch`!
    wait_for("COD-204 from second repo to reach Done", || {
        linear.issue_state("COD-204") == "Done"
    })
    .await;
    wait_for("COD-201 from first repo to reach Done", || {
        linear.issue_state("COD-201") == "Done"
    })
    .await;

    // Verify pagination & state filtering on `GET /api/v1/repos/tyler/site/pulls`.
    let (s, p1) = api
        .get(&format!("{site_pulls}?state=all&limit=2&page=1"))
        .await;
    assert_eq!(s, 200);
    assert_eq!(p1.as_array().unwrap().len(), 2);
    // Verify RFC 3339 string timestamps.
    assert!(p1[0]["updated_at"].as_str().unwrap().ends_with('Z'));
    let (_, p2) = api
        .get(&format!("{site_pulls}?state=all&limit=2&page=2"))
        .await;
    assert_eq!(p2.as_array().unwrap().len(), 1);
    let (_, p3) = api
        .get(&format!("{site_pulls}?state=all&limit=2&page=3"))
        .await;
    assert!(p3.as_array().unwrap().is_empty());
    let (_, open_only) = api.get(&format!("{site_pulls}?state=open")).await;
    assert_eq!(open_only.as_array().unwrap().len(), 1);
    let (_, closed_only) = api.get(&format!("{site_pulls}?state=closed")).await;
    assert_eq!(closed_only.as_array().unwrap().len(), 2);
    assert_eq!(api.get(&format!("{site_pulls}?page=0")).await.0, 400);
    assert_eq!(api.get(&format!("{site_pulls}?state=bogus")).await.0, 400);

    // Run `backfill_forgejo` with `limit=2` (forces multi-page fetch) against a fresh MockLinear!
    let fresh_linear = MockLinear::start().await;
    for key in ["COD-201", "COD-202", "COD-203"] {
        fresh_linear.add_issue(key);
    }
    let backfill_client = forge_linear_sync::linear::LinearClient::new(LinearConfig {
        endpoint: fresh_linear.endpoint.clone(),
        api_key: Secret::new("lin_api_test"),
    });
    let api_base: url::Url = format!("{base}/api/v1").parse().unwrap();
    let report = forge_linear_sync::sync::backfill_forgejo(
        &backfill_client,
        ForgeKind::Dabney,
        &sync_cfg.repos[0],
        &sync_cfg.matcher(),
        &api_base,
        Some(&tyler),
        2,
    )
    .await
    .unwrap();
    assert_eq!(report.pages, 2);
    assert_eq!(report.total, 3);
    assert_eq!(fresh_linear.issue_state("COD-201"), "Done");
    assert_eq!(fresh_linear.issue_state("COD-202"), "Todo");
    assert_eq!(fresh_linear.issue_state("COD-203"), "Todo");
    assert_eq!(fresh_linear.attachments("COD-201").len(), 1);
    assert_eq!(fresh_linear.attachments("COD-202").len(), 1);
    assert_eq!(fresh_linear.attachments("COD-203").len(), 1);
}

/// B4 + M3 + mirror hygiene: private repo existence is never leaked as 403,
/// XSS `target_url` schemes are blocked at ingest AND render, and mirror URLs
/// cannot embed credentials.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn security_privacy_xss_status_auth_and_mirror_urls() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let git = GitEnv::new(root);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base = format!("http://{addr}");
    let app = Forge::open(&root.join("data"), ForgeConfig::new(base.parse().unwrap())).unwrap();
    let priv_repo = app
        .create_repo(&RepoName::parse("tyler/secret").unwrap(), true, "main")
        .unwrap();
    let pub_repo = app
        .create_repo(&RepoName::parse("tyler/public").unwrap(), false, "main")
        .unwrap();

    // Mirror URLs with embedded credentials must be rejected.
    assert!(app
        .db
        .add_mirror(
            pub_repo.id,
            "bad",
            "https://user:secret@example.com/repo.git",
            None,
            None,
        )
        .is_err());

    let tyler = issue_token(
        &app.db,
        "tyler",
        "owner",
        &parse_scopes("read,write,status").unwrap(),
    )
    .unwrap();
    let mallory = issue_token(
        &app.db,
        "mallory",
        "outsider",
        &parse_scopes("read,write,status").unwrap(),
    )
    .unwrap();

    let router = forge::router(app.clone());
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let api = Api {
        http: reqwest::Client::new(),
        base: base.clone(),
    };

    // Private repo: anonymous gets 401; authenticated non-member gets 404 (NEVER 403).
    for path in [
        "/api/v1/repos/tyler/secret",
        "/tyler/secret",
        "/tyler/secret/pulls",
        "/tyler/secret.git/info/refs?service=git-upload-pack",
    ] {
        let anon = api.http.get(format!("{base}{path}")).send().await.unwrap();
        assert_eq!(anon.status().as_u16(), 401, "anon {path}");
        let non_member = api
            .http
            .get(format!("{base}{path}"))
            .header("authorization", format!("token {mallory}"))
            .send()
            .await
            .unwrap();
        assert_eq!(
            non_member.status().as_u16(),
            404,
            "non-member must see 404 (not 403) on private {path}"
        );
    }
    assert_eq!(
        api.post(
            "/api/v1/repos/tyler/secret/pulls",
            Some(&mallory),
            json!({"title": "x", "head": "feat"})
        )
        .await
        .0,
        404
    );

    // Seed private repo with one commit so we can test collaborator status-only CI token vs non-member 404.
    app.db.add_collaborator(priv_repo.id, "priv-ci").unwrap();
    let priv_ci = issue_token(
        &app.db,
        "priv-ci",
        "runner",
        &parse_scopes("status").unwrap(),
    )
    .unwrap();
    let priv_url = format!("http://tyler:{tyler}@{addr}/tyler/secret.git");
    git.ok(root, &["clone", &priv_url, "priv-w"]);
    let pw = root.join("priv-w");
    write(&pw.join("s.txt"), "secret\n");
    git.ok(&pw, &["add", "-A"]);
    git.ok(&pw, &["commit", "-m", "secret init"]);
    git.ok(&pw, &["push", "origin", "main"]);
    let priv_head = git.ok(&pw, &["rev-parse", "HEAD"]);
    assert_eq!(
        api.post(
            &format!("/api/v1/repos/tyler/secret/statuses/{priv_head}"),
            Some(&mallory),
            json!({"state": "success", "context": "ci"})
        )
        .await
        .0,
        404,
        "non-member posting status on private repo must see 404"
    );
    assert_eq!(
        api.post(
            &format!("/api/v1/repos/tyler/secret/statuses/{priv_head}"),
            Some(&priv_ci),
            json!({"state": "success", "context": "ci", "target_url": "https://ci.dabney.moe/1"})
        )
        .await
        .0,
        201,
        "collaborator with status-only scope must be allowed to post status on private repo"
    );
    let priv_ci_read = api
        .http
        .get(format!("{base}/api/v1/repos/tyler/secret"))
        .header("authorization", format!("token {priv_ci}"))
        .send()
        .await
        .unwrap();
    assert_eq!(
        priv_ci_read.status().as_u16(),
        403,
        "status-only collaborator may not read private repo"
    );

    // Seed public repo + PR to verify XSS defense-in-depth at both ingest and UI render.
    let url = format!("http://tyler:{tyler}@{addr}/tyler/public.git");
    git.ok(root, &["clone", &url, "w"]);
    let w = root.join("w");
    write(&w.join("a.txt"), "hello\n");
    git.ok(&w, &["add", "-A"]);
    git.ok(&w, &["commit", "-m", "init"]);
    git.ok(&w, &["push", "origin", "main"]);
    git.ok(&w, &["checkout", "-b", "feat"]);
    write(&w.join("a.txt"), "hello world\n");
    git.ok(&w, &["add", "-A"]);
    git.ok(&w, &["commit", "-m", "feat"]);
    git.ok(&w, &["push", "origin", "feat"]);
    let head = git.ok(&w, &["rev-parse", "HEAD"]);
    assert_eq!(
        api.post(
            "/api/v1/repos/tyler/public/pulls",
            Some(&tyler),
            json!({"title": "Test", "head": "feat"})
        )
        .await
        .0,
        201
    );
    // Ingest rejects javascript: and data: URLs.
    for bad_url in [
        "javascript:alert(1)",
        "data:text/html,<script>x</script>",
        "ftp://example.com",
    ] {
        assert_eq!(
            api.post(
                &format!("/api/v1/repos/tyler/public/statuses/{head}"),
                Some(&tyler),
                json!({"state": "success", "context": "ci", "target_url": bad_url})
            )
            .await
            .0,
            400,
            "{bad_url}"
        );
    }
    // Defense in depth: even if a bad URL is inserted directly into SQLite, UI must NOT emit href.
    let head_oid = gix::ObjectId::from_hex(head.as_bytes()).unwrap();
    app.db
        .insert_status(
            pub_repo.id,
            head_oid,
            &forge::db::CheckStatus {
                context: "poisoned".into(),
                state: forge::db::CheckState::Success,
                target_url: Some("javascript:alert(1)".into()),
                description: None,
                creator: "db".into(),
                created_at: forge::db::now(),
            },
        )
        .unwrap();
    let html = api
        .http
        .get(format!("{base}/tyler/public/pull/1"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        !html.contains("javascript:"),
        "UI must never render javascript: href"
    );
    // Suppress unused warning on priv_repo.
    assert!(priv_repo.private);
}

/// B5 + M1 + M2 + M4: conflicts, orphan/equal-tip PRs, invalid branch names,
/// push refusals (non-ff main, delete main, refs/pull/*, atomic rollback,
/// broken annotated tags & missing blobs with pack cleanup), Side::Old context
/// + outdated comments, and post-close push isolation.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn git_edge_cases_conflicts_orphans_equal_tip_tags_and_comments() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let git = GitEnv::new(root);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base = format!("http://{addr}");
    let app = Forge::open(&root.join("data"), ForgeConfig::new(base.parse().unwrap())).unwrap();
    let record = app
        .create_repo(&RepoName::parse("tyler/edge").unwrap(), false, "main")
        .unwrap();
    app.db.add_collaborator(record.id, "bob").unwrap();
    app.db.set_repo_policy(record.id, 1, &[]).unwrap();
    let tyler = issue_token(
        &app.db,
        "tyler",
        "owner",
        &parse_scopes("read,write").unwrap(),
    )
    .unwrap();
    let bob = issue_token(&app.db, "bob", "rev", &parse_scopes("read,write").unwrap()).unwrap();

    let router = forge::router(app.clone());
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let api = Api {
        http: reqwest::Client::new(),
        base: base.clone(),
    };

    let push_url = format!("http://tyler:{tyler}@{addr}/tyler/edge.git");
    git.ok(root, &["clone", &push_url, "w"]);
    let w = root.join("w");
    write(&w.join("shared.txt"), "line 1\nline 2\nline 3\nline 4\n");
    write(&w.join("extra.txt"), "keep\n");
    git.ok(&w, &["add", "-A"]);
    git.ok(&w, &["commit", "-m", "base"]);
    git.ok(&w, &["push", "origin", "main"]);
    let main_base = git.ok(&w, &["rev-parse", "HEAD"]);

    // 1. Real conflicting 3-way merge via gix-merge.
    git.ok(&w, &["checkout", "-b", "conflict-br"]);
    write(
        &w.join("shared.txt"),
        "line 1\nbranch edit\nline 3\nline 4\n",
    );
    git.ok(&w, &["add", "-A"]);
    git.ok(&w, &["commit", "-m", "branch edit"]);
    git.ok(&w, &["push", "origin", "conflict-br"]);

    git.ok(&w, &["checkout", "main"]);
    write(&w.join("shared.txt"), "line 1\nmain edit\nline 3\nline 4\n");
    git.ok(&w, &["add", "-A"]);
    git.ok(&w, &["commit", "-m", "main edit"]);
    git.ok(&w, &["push", "origin", "main"]);
    let main_tip = git.ok(&w, &["rev-parse", "HEAD"]);

    let pulls = "/api/v1/repos/tyler/edge/pulls";
    // Invalid branch names must return 400, never 500.
    for bad_branch in ["../tags/v1", "a..b", "", "/leading"] {
        let (st, _) = api
            .post(
                pulls,
                Some(&tyler),
                json!({"title": "bad", "head": bad_branch}),
            )
            .await;
        assert_eq!(st, 400, "invalid branch {bad_branch:?}");
    }

    assert_eq!(
        api.post(
            pulls,
            Some(&tyler),
            json!({"title": "conflicting pr", "head": "conflict-br"})
        )
        .await
        .0,
        201
    );
    assert_eq!(
        api.post(
            &format!("{pulls}/1/reviews"),
            Some(&bob),
            json!({"event": "APPROVE"})
        )
        .await
        .0,
        201
    );
    let (_, ready) = api.get(&format!("{pulls}/1/readiness")).await;
    assert_eq!(blockers(&ready), vec!["conflicts"], "{ready}");
    assert_eq!(
        api.post(&format!("{pulls}/1/merge"), Some(&tyler), json!({}))
            .await
            .0,
        409
    );

    // 2. Side::Old context-line comment AND shifted/removed file outdated comment.
    // On PR #1 (`conflict-br`), line 1 of `shared.txt` is a context line.
    assert_eq!(
        api.post(
            &format!("{pulls}/1/comments"),
            Some(&bob),
            json!({"path": "shared.txt", "side": "old", "line": 1, "body": "Context old-side note"})
        )
        .await
        .0,
        201
    );
    let html = api
        .http
        .get(format!("{base}/tyler/edge/pull/1"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        html.contains("Context old-side note"),
        "Side::Old context comment must render inline"
    );

    // Force-push `conflict-br` to edit only `extra.txt`, removing `shared.txt` from the diff.
    git.ok(&w, &["checkout", "conflict-br"]);
    git.ok(&w, &["reset", "--hard", "main"]);
    write(&w.join("extra.txt"), "changed\n");
    git.ok(&w, &["add", "-A"]);
    git.ok(&w, &["commit", "-m", "edit extra only"]);
    git.ok(&w, &["push", "--force", "origin", "conflict-br"]);
    let html_after = api
        .http
        .get(format!("{base}/tyler/edge/pull/1"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        html_after.contains("Outdated comments") && html_after.contains("Context old-side note"),
        "shifted/removed-file comment must appear in Outdated comments section"
    );
    let (st, api_comments) = api.get(&format!("{pulls}/1/comments")).await;
    assert_eq!(st, 200);
    assert_eq!(api_comments.as_array().unwrap().len(), 1);
    let (st, api_reviews) = api.get(&format!("{pulls}/1/reviews")).await;
    assert_eq!(st, 200);
    assert_eq!(api_reviews.as_array().unwrap().len(), 1);

    // 3. Orphan branch rejected on open; force-pushing an open PR to orphan or equal-tip
    // blocks cleanly (no 500, no duplicate-parent commit).
    git.ok(&w, &["checkout", "--orphan", "orphan-br"]);
    git.ok(&w, &["rm", "-rf", "."]);
    write(&w.join("alone.txt"), "orphan\n");
    git.ok(&w, &["add", "-A"]);
    git.ok(&w, &["commit", "-m", "orphan root"]);
    git.ok(&w, &["push", "origin", "orphan-br"]);
    let orphan_sha = git.ok(&w, &["rev-parse", "HEAD"]);
    assert_eq!(
        api.post(
            pulls,
            Some(&tyler),
            json!({"title": "orphan pr", "head": "orphan-br"})
        )
        .await
        .0,
        422
    );
    // Force-push PR #1's branch to the orphan commit: UI/API must return 200 (not 500), merge 409.
    git.ok(
        &w,
        &[
            "push",
            "--force",
            "origin",
            &format!("{orphan_sha}:refs/heads/conflict-br"),
        ],
    );
    let (st, ready) = api.get(&format!("{pulls}/1/readiness")).await;
    assert_eq!(st, 200);
    assert!(
        blockers(&ready).contains(&"no_common_ancestor".to_owned()),
        "{ready}"
    );
    assert_eq!(
        api.http
            .get(format!("{base}/tyler/edge/pull/1"))
            .send()
            .await
            .unwrap()
            .status()
            .as_u16(),
        200
    );
    assert_eq!(
        api.post(&format!("{pulls}/1/merge"), Some(&tyler), json!({}))
            .await
            .0,
        409
    );

    // Force-push PR #1's branch to `main_tip` (equal tip): blocks with `nothing_to_merge`.
    git.ok(
        &w,
        &[
            "push",
            "--force",
            "origin",
            &format!("{main_tip}:refs/heads/conflict-br"),
        ],
    );
    let (st, ready) = api.get(&format!("{pulls}/1/readiness")).await;
    assert_eq!(st, 200);
    assert!(
        blockers(&ready).contains(&"nothing_to_merge".to_owned()),
        "{ready}"
    );
    assert_eq!(
        api.post(&format!("{pulls}/1/merge"), Some(&tyler), json!({}))
            .await
            .0,
        409
    );

    // 4. Close PR #1, then push a new commit to `conflict-br`: closed PR must NOT change head.
    assert_eq!(
        api.post(&format!("{pulls}/1/close"), Some(&tyler), json!({}))
            .await
            .0,
        200
    );
    let (_, closed_before) = api.get(&format!("{pulls}/1")).await;
    git.ok(&w, &["checkout", "-B", "conflict-br", "main"]);
    write(&w.join("after-close.txt"), "ignored\n");
    git.ok(&w, &["add", "-A"]);
    git.ok(&w, &["commit", "-m", "after close"]);
    git.ok(&w, &["push", "--force", "origin", "conflict-br"]);
    let (_, closed_after) = api.get(&format!("{pulls}/1")).await;
    assert_eq!(closed_after["head"]["sha"], closed_before["head"]["sha"]);
    assert_eq!(closed_after["state"], "closed");

    // 5. Push refusals + annotated tag + disconnected tag/commit pack cleanup.
    // Non-fast-forward to main is refused.
    assert!(!git
        .run(
            &w,
            &[
                "push",
                "--force",
                "origin",
                &format!("{main_base}:refs/heads/main")
            ]
        )
        .status
        .success());
    // Deleting main is refused.
    assert!(!git.run(&w, &["push", "origin", ":main"]).status.success());
    // Pushing directly to refs/pull/* is refused.
    assert!(!git
        .run(
            &w,
            &["push", "origin", &format!("{main_tip}:refs/pull/99/head")]
        )
        .status
        .success());
    // Atomic push rollback: one valid branch + one forbidden push leaves valid branch uncreated.
    assert!(!git
        .run(
            &w,
            &[
                "push",
                "--atomic",
                "origin",
                &format!("{main_tip}:refs/heads/atomic-rolled-back"),
                &format!("{main_base}:refs/heads/main"),
            ]
        )
        .status
        .success());
    let refs_now = git.ok(&w, &["ls-remote", &push_url]);
    assert!(!refs_now.contains("atomic-rolled-back"));

    // Valid annotated tag push works and peels cleanly.
    git.ok(&w, &["checkout", "main"]);
    git.ok(&w, &["tag", "-a", "v1.0", "-m", "release 1.0"]);
    git.ok(&w, &["push", "origin", "v1.0"]);
    let refs_with_tag = git.ok(&w, &["ls-remote", &push_url]);
    assert!(refs_with_tag.contains("refs/tags/v1.0^{}"));

    // Crafted push with a dangling annotated tag (pointing to a missing OID):
    // must be rejected cleanly, leave zero leaked pack files, and NOT break `list_refs`.
    let pack_dir = app.repos.path(&record.name).join("objects").join("pack");
    let packs_before: Vec<_> = std::fs::read_dir(&pack_dir)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    let scratch_repo_dir = root.join("dangling-tag-src");
    std::fs::create_dir_all(&scratch_repo_dir).unwrap();
    let scratch_repo = gix::init_bare(&scratch_repo_dir).unwrap();
    let missing_oid = gix::ObjectId::from_bytes_or_panic(&[0xab; 20]);
    let bad_tag = gix::objs::Tag {
        target: missing_oid,
        target_kind: gix::objs::Kind::Commit,
        name: "bad-tag".into(),
        tagger: None,
        message: "dangling\n".into(),
        signature: None,
    };
    let bad_tag_oid = scratch_repo.write_object(&bad_tag).unwrap().detach();
    let bad_pack = forge::git::pack::write_pack(&scratch_repo, vec![bad_tag_oid]).unwrap();
    let report = forge::git::http::apply_push(
        &app,
        &record,
        vec![forge::git::http::PushCommand {
            old: gix::ObjectId::null(gix::hash::Kind::Sha1),
            new: bad_tag_oid,
            name: "refs/tags/bad-tag".into(),
        }],
        &bad_pack,
        false,
    )
    .unwrap();
    assert!(
        report.results[0].1.is_some(),
        "dangling annotated tag must be rejected"
    );
    let packs_after: Vec<_> = std::fs::read_dir(&pack_dir)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(
        packs_after, packs_before,
        "rejected push must clean up its newly written pack/idx/.keep"
    );
    // Subsequent ls-remote still succeeds (not permanently 500'd!).
    assert!(git
        .ok(&w, &["ls-remote", &push_url])
        .contains("refs/heads/main"));

    // Crafted push with a commit whose tree references a missing blob: must also be rejected!
    let missing_blob = gix::ObjectId::from_bytes_or_panic(&[0xcd; 20]);
    let bad_tree = gix::objs::Tree {
        entries: vec![gix::objs::tree::Entry {
            mode: gix::objs::tree::EntryKind::Blob.into(),
            filename: "ghost.txt".into(),
            oid: missing_blob,
        }],
    };
    let bad_tree_oid = scratch_repo.write_object(&bad_tree).unwrap().detach();
    let bad_commit = gix::objs::Commit {
        tree: bad_tree_oid,
        parents: Default::default(),
        author: forge::git::http::forge_signature(),
        committer: forge::git::http::forge_signature(),
        encoding: None,
        message: "missing blob\n".into(),
        extra_headers: Vec::new(),
    };
    let bad_commit_oid = scratch_repo.write_object(&bad_commit).unwrap().detach();
    let bad_commit_pack =
        forge::git::pack::write_pack(&scratch_repo, vec![bad_commit_oid, bad_tree_oid]).unwrap();
    let report = forge::git::http::apply_push(
        &app,
        &record,
        vec![forge::git::http::PushCommand {
            old: gix::ObjectId::null(gix::hash::Kind::Sha1),
            new: bad_commit_oid,
            name: "refs/heads/missing-blob".into(),
        }],
        &bad_commit_pack,
        false,
    )
    .unwrap();
    assert!(
        report.results[0]
            .1
            .as_deref()
            .is_some_and(|r| r.contains("missing objects")),
        "commit with missing blob must be rejected: {:?}",
        report.results
    );
    let packs_final: Vec<_> = std::fs::read_dir(&pack_dir)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(packs_final, packs_before);
}

/// B5: Concurrent `pr::close` and `merge::merge` on the same open PR are
/// serialized by `repo_lock`; exactly one wins, and Git `refs/heads/main`,
/// SQLite PR state, and queued webhook deliveries always stay consistent.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_close_and_merge_never_corrupts_state() {
    use std::sync::Barrier;

    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let git = GitEnv::new(root);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base = format!("http://{addr}");
    let mut cfg = ForgeConfig::new(base.parse().unwrap());
    cfg.webhooks = vec![WebhookTarget {
        url: format!("{base}/hooks/forge/dabney").parse().unwrap(),
        secret: Secret::new(HOOK_SECRET),
    }];
    let app = Forge::open(&root.join("data"), cfg).unwrap();
    let record = app
        .create_repo(&RepoName::parse("tyler/race").unwrap(), false, "main")
        .unwrap();
    app.db.add_collaborator(record.id, "bob").unwrap();
    app.db.set_repo_policy(record.id, 1, &[]).unwrap();
    let tyler = issue_token(
        &app.db,
        "tyler",
        "owner",
        &parse_scopes("read,write").unwrap(),
    )
    .unwrap();
    let router = forge::router(app.clone());
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });

    let push_url = format!("http://tyler:{tyler}@{addr}/tyler/race.git");
    git.ok(root, &["clone", &push_url, "w"]);
    let w = root.join("w");
    write(&w.join("base.txt"), "base\n");
    git.ok(&w, &["add", "-A"]);
    git.ok(&w, &["commit", "-m", "base"]);
    git.ok(&w, &["push", "origin", "main"]);

    let tyler_p = forge::auth::Principal {
        user: "tyler".into(),
        scopes: parse_scopes("read,write").unwrap(),
    };
    let bob_p = forge::auth::Principal {
        user: "bob".into(),
        scopes: parse_scopes("read,write").unwrap(),
    };

    for round in 1..=6u64 {
        let branch = format!("race-{round}");
        git.ok(&w, &["checkout", "-b", &branch, "main"]);
        write(
            &w.join(format!("r{round}.txt")),
            &format!("round {round}\n"),
        );
        git.ok(&w, &["add", "-A"]);
        git.ok(&w, &["commit", "-m", &format!("round {round}")]);
        git.ok(&w, &["push", "origin", &branch]);

        let pr = forge::pr::open(
            &app,
            &record,
            &tyler_p,
            forge::pr::NewPullRequest {
                title: format!("race #{round}"),
                body: String::new(),
                head: branch,
                base: None,
            },
        )
        .unwrap();
        forge::pr::review(
            &app,
            &record,
            pr.number,
            &bob_p,
            forge::db::Verdict::Approve,
            "lgtm",
            Some(pr.head_oid),
        )
        .unwrap();

        let main_before = {
            let repo = app.repos.open(&record.name).unwrap();
            forge::repo::branch_tip(&repo, "main").unwrap().unwrap()
        };
        let barrier = Arc::new(Barrier::new(2));

        let app_m = app.clone();
        let rec_m = record.clone();
        let p_m = tyler_p.clone();
        let b_m = barrier.clone();
        let merge_handle = std::thread::spawn(move || {
            b_m.wait();
            forge::merge::merge(&app_m, &rec_m, pr.number, &p_m, Some(pr.head_oid))
        });

        let app_c = app.clone();
        let rec_c = record.clone();
        let p_c = tyler_p.clone();
        let b_c = barrier.clone();
        let close_handle = std::thread::spawn(move || {
            b_c.wait();
            forge::pr::close(&app_c, &rec_c, pr.number, &p_c)
        });

        let merge_res = merge_handle.join().unwrap();
        let close_res = close_handle.join().unwrap();
        assert_ne!(
            merge_res.is_ok(),
            close_res.is_ok(),
            "round {round}: exactly one of merge/close must win: merge={merge_res:?} close={close_res:?}"
        );

        let final_pr = app.db.pr_by_id(pr.id).unwrap();
        let main_after = {
            let repo = app.repos.open(&record.name).unwrap();
            forge::repo::branch_tip(&repo, "main").unwrap().unwrap()
        };
        let closed_deliveries: Vec<_> = app
            .db
            .deliveries(pr.id)
            .unwrap()
            .into_iter()
            .filter(|d| d.action == "closed")
            .collect();
        assert_eq!(
            closed_deliveries.len(),
            1,
            "round {round}: exactly one closed webhook delivery"
        );
        let hook_json: Value = serde_json::from_slice(&closed_deliveries[0].payload).unwrap();

        match final_pr.state {
            forge::db::PrState::Merged { commit } => {
                assert!(merge_res.is_ok());
                assert_eq!(main_after, commit, "main must point at the merge commit");
                assert_ne!(main_after, main_before);
                assert_eq!(hook_json["pull_request"]["merged"], Value::Bool(true));
                // Keep local git repo in sync for next round.
                git.ok(&w, &["fetch", "origin"]);
                git.ok(&w, &["checkout", "main"]);
                git.ok(&w, &["reset", "--hard", "origin/main"]);
            }
            forge::db::PrState::Closed => {
                assert!(close_res.is_ok());
                assert_eq!(
                    main_after, main_before,
                    "main must NOT advance when close won"
                );
                assert_eq!(hook_json["pull_request"]["merged"], Value::Bool(false));
            }
            forge::db::PrState::Open => panic!("PR must not remain open after race"),
        }
    }
}
