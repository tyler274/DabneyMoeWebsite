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
    let passing = json!({"state": "success", "context": "ci/test", "description": "all green"});
    assert_eq!(api.post(&statuses, None, passing.clone()).await.0, 401);
    assert_eq!(
        api.post(&statuses, Some(&bob), passing.clone()).await.0,
        403,
        "bob's token has no status scope"
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
