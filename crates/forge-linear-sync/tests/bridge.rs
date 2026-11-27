//! End-to-end bridge behaviour against the in-memory Linear GraphQL server.

use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use forge_linear_sync::config::{LinearConfig, RepoConfig, RetryPolicy, SyncConfig};
use forge_linear_sync::domain::{Authority, Forge, RepoPath, TeamKey};
use forge_linear_sync::mock::{Failure, MockLinear};
use forge_linear_sync::queue::{EventStatus, EventStore};
use forge_linear_sync::verify::{sign_hex, Secret};
use forge_linear_sync::worker::Processed;
use forge_linear_sync::Bridge;
use serde_json::json;
use tower::ServiceExt;

const FORGEJO_SECRET: &str = "forgejo-secret";
const GITLAB_TOKEN: &str = "gitlab-token";
const HEAD: &str = "1111111111111111111111111111111111111111";
const MERGE: &str = "2222222222222222222222222222222222222222";

fn config(linear: &MockLinear, max_attempts: u32) -> SyncConfig {
    SyncConfig {
        teams: vec![
            TeamKey::parse("COD").unwrap(),
            TeamKey::parse("TYL").unwrap(),
        ],
        repos: vec![
            RepoConfig {
                forge: Forge::Forgejo,
                path: RepoPath::parse("tyler/site").unwrap(),
                authority: Authority::Canonical,
                default_branch: "main".into(),
                secrets: vec![Secret::new(FORGEJO_SECRET)],
            },
            RepoConfig {
                forge: Forge::GitLab,
                path: RepoPath::parse("mirror/site").unwrap(),
                authority: Authority::External,
                default_branch: "main".into(),
                secrets: vec![Secret::new(GITLAB_TOKEN)],
            },
        ],
        linear: LinearConfig {
            endpoint: linear.endpoint.clone(),
            api_key: Secret::new("lin_api_test"),
        },
        retry: RetryPolicy {
            max_attempts,
            base_delay: Duration::from_millis(1),
            max_delay: Duration::from_millis(5),
        },
    }
}

async fn setup(max_attempts: u32) -> (MockLinear, Bridge) {
    let linear = MockLinear::start().await;
    for key in ["COD-1", "COD-2", "COD-3", "COD-4", "COD-5", "COD-6"] {
        linear.add_issue(key);
    }
    let bridge = Bridge::new(
        config(&linear, max_attempts),
        EventStore::in_memory().unwrap(),
    );
    (linear, bridge)
}

struct Pr<'a> {
    number: u64,
    title: &'a str,
    branch: &'a str,
    base: &'a str,
    state: &'a str,
    merged: bool,
    updated_at: &'a str,
}

impl Default for Pr<'_> {
    fn default() -> Self {
        Pr {
            number: 7,
            title: "COD-1: improve things",
            branch: "feature",
            base: "main",
            state: "open",
            merged: false,
            updated_at: "2026-11-25T10:00:00Z",
        }
    }
}

fn forgejo_body(pr: &Pr<'_>) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "action": if pr.merged || pr.state == "closed" { "closed" } else { "opened" },
        "number": pr.number,
        "pull_request": {
            "number": pr.number,
            "html_url": format!("https://codeberg.org/tyler/site/pulls/{}", pr.number),
            "title": pr.title,
            "body": "",
            "state": pr.state,
            "draft": false,
            "merged": pr.merged,
            "merge_commit_sha": if pr.merged { Some(MERGE) } else { None },
            "updated_at": pr.updated_at,
            "head": { "ref": pr.branch, "sha": HEAD },
            "base": { "ref": pr.base, "sha": HEAD },
        },
        "repository": { "full_name": "tyler/site", "default_branch": "main" },
    }))
    .unwrap()
}

fn gitlab_body(state: &str, title: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "object_kind": "merge_request",
        "project": { "path_with_namespace": "mirror/site" },
        "object_attributes": {
            "iid": 3,
            "url": "https://gitlab.com/mirror/site/-/merge_requests/3",
            "title": title,
            "description": "",
            "state": state,
            "draft": false,
            "merge_commit_sha": if state == "merged" { Some(MERGE) } else { None },
            "updated_at": "2026-11-25 10:00:00 UTC",
            "source_branch": "patch",
            "target_branch": "main",
            "last_commit": { "id": HEAD },
        }
    }))
    .unwrap()
}

async fn post_forgejo(
    bridge: &Bridge,
    body: Vec<u8>,
    delivery: &str,
    signature: Option<String>,
) -> StatusCode {
    let signature = signature.unwrap_or_else(|| sign_hex(&Secret::new(FORGEJO_SECRET), &body));
    let request = Request::post("/hooks/forge/forgejo")
        .header("x-forgejo-event", "pull_request")
        .header("x-forgejo-delivery", delivery)
        .header("x-forgejo-signature", signature)
        .body(Body::from(body))
        .unwrap();
    bridge
        .router
        .clone()
        .oneshot(request)
        .await
        .unwrap()
        .status()
}

async fn deliver(bridge: &Bridge, pr: &Pr<'_>, delivery: &str) -> Vec<Processed> {
    assert_eq!(
        post_forgejo(bridge, forgejo_body(pr), delivery, None).await,
        StatusCode::ACCEPTED
    );
    bridge.worker.drain_due().await.unwrap()
}

#[tokio::test]
async fn bad_signature_is_401_before_any_json_parse() {
    let (linear, bridge) = setup(3).await;
    // Not JSON at all: a parser running first would answer 400/422, not 401.
    let status = post_forgejo(
        &bridge,
        b"{{{ not json".to_vec(),
        "d1",
        Some("00".repeat(32)),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    // A valid body with one byte flipped after signing.
    let body = forgejo_body(&Pr::default());
    let signature = sign_hex(&Secret::new(FORGEJO_SECRET), &body);
    let mut tampered = body.clone();
    tampered[10] ^= 1;
    assert_eq!(
        post_forgejo(&bridge, tampered, "d2", Some(signature)).await,
        StatusCode::UNAUTHORIZED
    );
    // Missing signature header.
    let request = Request::post("/hooks/forge/forgejo")
        .header("x-forgejo-event", "pull_request")
        .body(Body::from(body))
        .unwrap();
    assert_eq!(
        bridge
            .router
            .clone()
            .oneshot(request)
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert!(bridge.worker.store.events(None).unwrap().is_empty());
    assert_eq!(linear.operation_count("AttachmentCreate"), 0);
}

#[tokio::test]
async fn verified_but_unhandled_event_is_204() {
    let (_linear, bridge) = setup(3).await;
    let body = b"{\"ref\":\"refs/heads/main\"}".to_vec();
    let request = Request::post("/hooks/forge/forgejo")
        .header("x-forgejo-event", "push")
        .header(
            "x-forgejo-signature",
            sign_hex(&Secret::new(FORGEJO_SECRET), &body),
        )
        .body(Body::from(body))
        .unwrap();
    assert_eq!(
        bridge
            .router
            .clone()
            .oneshot(request)
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );
    let request = Request::post("/hooks/forge/bitbucket")
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        bridge
            .router
            .clone()
            .oneshot(request)
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn gitlab_token_is_checked() {
    let (_linear, bridge) = setup(3).await;
    for (token, expected) in [
        ("wrong", StatusCode::UNAUTHORIZED),
        (GITLAB_TOKEN, StatusCode::ACCEPTED),
    ] {
        let request = Request::post("/hooks/forge/gitlab")
            .header("x-gitlab-event", "Merge Request Hook")
            .header("x-gitlab-token", token)
            .header("x-gitlab-event-uuid", "gl-1")
            .body(Body::from(gitlab_body("opened", "COD-3: mirror patch")))
            .unwrap();
        assert_eq!(
            bridge
                .router
                .clone()
                .oneshot(request)
                .await
                .unwrap()
                .status(),
            expected
        );
    }
}

#[tokio::test]
async fn duplicate_delivery_creates_one_attachment() {
    let (linear, bridge) = setup(3).await;
    let pr = Pr::default();
    deliver(&bridge, &pr, "same").await;
    assert_eq!(
        post_forgejo(&bridge, forgejo_body(&pr), "same", None).await,
        StatusCode::OK
    );
    assert!(bridge.worker.drain_due().await.unwrap().is_empty());
    // A redelivery under a new id is applied again, still as one card.
    deliver(&bridge, &pr, "other").await;
    let cards = linear.attachments("COD-1");
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].url, "https://codeberg.org/tyler/site/pulls/7");
    assert_eq!(cards[0].metadata["state"], "open");
    assert_eq!(
        linear.issue_state("COD-1"),
        "Todo",
        "never moved into a started state"
    );
}

#[tokio::test]
async fn out_of_order_closed_then_older_opened_stays_closed() {
    let (linear, bridge) = setup(3).await;
    let closed = Pr {
        state: "closed",
        updated_at: "2026-11-25T12:00:00Z",
        ..Pr::default()
    };
    deliver(&bridge, &closed, "late").await;
    let processed = deliver(&bridge, &Pr::default(), "early").await;
    assert!(matches!(&processed[..], [Processed::Done(o)] if o.stale));
    let cards = linear.attachments("COD-1");
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].metadata["state"], "closed");
}

#[tokio::test]
async fn removing_a_key_from_the_title_unlinks_it() {
    let (linear, bridge) = setup(3).await;
    deliver(&bridge, &Pr::default(), "a").await;
    assert_eq!(linear.attachments("COD-1").len(), 1);
    let retitled = Pr {
        title: "COD-2: improve things",
        updated_at: "2026-11-25T11:00:00Z",
        ..Pr::default()
    };
    deliver(&bridge, &retitled, "b").await;
    assert!(linear.attachments("COD-1").is_empty());
    assert_eq!(linear.attachments("COD-2").len(), 1);
}

#[tokio::test]
async fn external_mirror_merge_never_changes_state() {
    let (linear, bridge) = setup(3).await;
    let request = Request::post("/hooks/forge/gitlab")
        .header("x-gitlab-event", "Merge Request Hook")
        .header("x-gitlab-token", GITLAB_TOKEN)
        .header("x-gitlab-event-uuid", "gl-merge")
        .body(Body::from(gitlab_body("merged", "COD-3: mirror patch")))
        .unwrap();
    assert_eq!(
        bridge
            .router
            .clone()
            .oneshot(request)
            .await
            .unwrap()
            .status(),
        StatusCode::ACCEPTED
    );
    bridge.worker.drain_due().await.unwrap();
    let cards = linear.attachments("COD-3");
    assert_eq!(cards.len(), 1);
    assert!(
        cards[0].subtitle.contains("External"),
        "{}",
        cards[0].subtitle
    );
    assert_eq!(cards[0].metadata["authority"], "external");
    assert_eq!(linear.issue_labels("COD-3"), ["pr-external"]);
    assert_eq!(linear.issue_state("COD-3"), "Todo");
    assert_eq!(linear.operation_count("IssueUpdate"), 0);
}

#[tokio::test]
async fn multi_pr_issue_is_done_only_after_both_canonical_prs_merge() {
    let (linear, bridge) = setup(3).await;
    let first = Pr {
        number: 10,
        title: "COD-4: part one",
        ..Pr::default()
    };
    let second = Pr {
        number: 11,
        title: "COD-4: part two",
        ..Pr::default()
    };
    deliver(&bridge, &first, "1o").await;
    deliver(&bridge, &second, "2o").await;
    let merged = |pr: &Pr<'static>| Pr {
        state: "closed",
        merged: true,
        updated_at: "2026-11-25T13:00:00Z",
        ..*pr
    };
    deliver(&bridge, &merged(&first), "1m").await;
    assert_eq!(linear.issue_state("COD-4"), "Todo", "second PR still open");
    deliver(&bridge, &merged(&second), "2m").await;
    assert_eq!(linear.issue_state("COD-4"), "Done");
    assert_eq!(linear.attachments("COD-4").len(), 2);
}

#[tokio::test]
async fn merge_into_a_non_default_branch_does_not_finish_the_issue() {
    let (linear, bridge) = setup(3).await;
    let pr = Pr {
        title: "COD-5: stacked",
        base: "release",
        state: "closed",
        merged: true,
        ..Pr::default()
    };
    deliver(&bridge, &pr, "x").await;
    assert_eq!(linear.attachments("COD-5")[0].metadata["state"], "merged");
    assert_eq!(linear.issue_state("COD-5"), "Todo");
}

#[tokio::test]
async fn rate_limit_and_server_errors_retry_then_succeed() {
    use std::sync::atomic::{AtomicI64, Ordering};
    use std::sync::Arc;

    let linear = MockLinear::start().await;
    linear.add_issue("COD-1");
    let mut cfg = config(&linear, 5);
    cfg.retry.base_delay = Duration::from_secs(5);
    cfg.retry.max_delay = Duration::from_secs(20);
    let clock = Arc::new(AtomicI64::new(jiff::Timestamp::now().as_millisecond()));
    let clock_fn = clock.clone();
    let store = EventStore::in_memory_with_clock(Arc::new(move || clock_fn.load(Ordering::SeqCst)))
        .unwrap();
    let bridge = Bridge::new(cfg, store);
    linear.fail_next(Failure::ServerError);
    // 60 s reset window guarantees `reset - now` stays well above exponential backoff
    // even under heavy parallel load; virtual clock advances instantaneously below.
    linear.fail_next(Failure::RateLimited {
        reset_in_ms: 60_000,
    });
    assert_eq!(
        post_forgejo(&bridge, forgejo_body(&Pr::default()), "r", None).await,
        StatusCode::ACCEPTED
    );

    // Step 1: single attempt hits 500; process_one_due never loops into step 2.
    let first = bridge.worker.process_one_due().await.unwrap().unwrap();
    let first_delay = match first {
        Processed::Retrying { delay, ref error } if error.contains("500") => delay,
        other => panic!("expected retry after 500, got {other:?}"),
    };
    assert!(first_delay >= Duration::from_secs(5), "{first_delay:?}");
    assert!(
        bridge.worker.process_one_due().await.unwrap().is_none(),
        "not due before exponential backoff elapses"
    );

    // Advance virtual clock just past the first backoff.
    clock.fetch_add(
        i64::try_from(first_delay.as_millis()).unwrap() + 1,
        Ordering::SeqCst,
    );

    // Step 2: hits 429 and must reschedule at least until the advertised reset.
    let second = bridge.worker.process_one_due().await.unwrap().unwrap();
    let second_delay = match second {
        Processed::Retrying { delay, .. } => delay,
        other => panic!("expected retry after 429, got {other:?}"),
    };
    assert!(
        second_delay >= Duration::from_secs(30),
        "must honor advertised rate-limit reset: {second_delay:?}"
    );
    assert!(
        bridge.worker.process_one_due().await.unwrap().is_none(),
        "not due before the reset"
    );

    // Advance virtual clock just past the rate-limit reset.
    clock.fetch_add(
        i64::try_from(second_delay.as_millis()).unwrap() + 1,
        Ordering::SeqCst,
    );

    // Step 3: succeeds.
    let third = bridge.worker.process_one_due().await.unwrap().unwrap();
    assert!(matches!(third, Processed::Done(_)), "{third:?}");
    let events = bridge.worker.store.events(None).unwrap();
    assert_eq!(events[0].status, EventStatus::Done);
    assert_eq!(events[0].attempts, 3);
    assert_eq!(linear.attachments("COD-1").len(), 1);
}

#[tokio::test]
async fn permanent_4xx_goes_to_dlq_with_sync_error_label() {
    let (linear, bridge) = setup(5).await;
    linear.fail_next(Failure::BadRequest);
    let processed = deliver(&bridge, &Pr::default(), "p").await;
    assert!(
        matches!(&processed[..], [Processed::DeadLettered { .. }]),
        "{processed:?}"
    );
    let dead = bridge.worker.store.events(Some(EventStatus::Dead)).unwrap();
    assert_eq!(dead.len(), 1);
    assert!(
        dead[0].last_error.as_deref().unwrap().contains("HTTP 400"),
        "{:?}",
        dead[0].last_error
    );
    assert_eq!(linear.issue_labels("COD-1"), ["pr-sync-error"]);
    // Replaying the DLQ succeeds once Linear accepts the request.
    assert_eq!(bridge.worker.store.replay_dead().unwrap(), 1);
    let processed = bridge.worker.drain_due().await.unwrap();
    assert!(
        matches!(&processed[..], [Processed::Done(_)]),
        "{processed:?}"
    );
    assert_eq!(linear.attachments("COD-1").len(), 1);
}

#[tokio::test]
async fn exhausted_retries_go_to_dlq() {
    use std::sync::atomic::{AtomicI64, Ordering};
    use std::sync::Arc;

    let linear = MockLinear::start().await;
    linear.add_issue("COD-1");
    let mut cfg = config(&linear, 3);
    cfg.retry.base_delay = Duration::from_secs(1);
    cfg.retry.max_delay = Duration::from_secs(10);
    let clock = Arc::new(AtomicI64::new(jiff::Timestamp::now().as_millisecond()));
    let clock_fn = clock.clone();
    let bridge = Bridge::new(
        cfg,
        EventStore::in_memory_with_clock(Arc::new(move || clock_fn.load(Ordering::SeqCst)))
            .unwrap(),
    );
    for _ in 0..3 {
        linear.fail_next(Failure::ServerError);
    }
    assert_eq!(
        post_forgejo(&bridge, forgejo_body(&Pr::default()), "e", None).await,
        StatusCode::ACCEPTED
    );
    for attempt in 0..3 {
        let processed = bridge.worker.process_one_due().await.unwrap().unwrap();
        match (attempt, processed) {
            (0 | 1, Processed::Retrying { delay, .. }) => {
                clock.fetch_add(
                    i64::try_from(delay.as_millis()).unwrap() + 1,
                    Ordering::SeqCst,
                );
            }
            (2, Processed::DeadLettered { .. }) => {}
            other => panic!("unexpected outcome on attempt {attempt}: {other:?}"),
        }
    }
    let dead = bridge.worker.store.events(Some(EventStatus::Dead)).unwrap();
    assert_eq!(dead.len(), 1, "{:?}", bridge.worker.store.events(None));
    assert_eq!(dead[0].attempts, 3);
    assert!(dead[0].last_error.as_deref().unwrap().contains("HTTP 500"));
}

#[tokio::test]
async fn unknown_issue_key_is_permanent() {
    let (linear, bridge) = setup(3).await;
    let pr = Pr {
        title: "COD-999: nobody",
        ..Pr::default()
    };
    let processed = deliver(&bridge, &pr, "u").await;
    assert!(
        matches!(&processed[..], [Processed::DeadLettered { error }] if error.contains("COD-999")),
        "{processed:?}"
    );
    assert_eq!(linear.operation_count("AttachmentCreate"), 0);
}

#[tokio::test]
async fn multi_repo_shared_secret_routes_second_repo_and_distinct_secret_isolates() {
    let linear = MockLinear::start().await;
    linear.add_issue("COD-1");
    linear.add_issue("COD-2");
    linear.add_issue("COD-3");
    let shared = Secret::new("shared-dabney-secret");
    let distinct = Secret::new("isolated-repo-secret");
    let bridge = Bridge::new(
        SyncConfig {
            teams: vec![TeamKey::parse("COD").unwrap()],
            repos: vec![
                RepoConfig {
                    forge: Forge::Dabney,
                    path: RepoPath::parse("tyler/site").unwrap(),
                    authority: Authority::Canonical,
                    default_branch: "main".into(),
                    secrets: vec![shared.clone()],
                },
                RepoConfig {
                    forge: Forge::Dabney,
                    path: RepoPath::parse("tyler/dabney-moe").unwrap(),
                    authority: Authority::Canonical,
                    default_branch: "main".into(),
                    secrets: vec![shared.clone()],
                },
                RepoConfig {
                    forge: Forge::Dabney,
                    path: RepoPath::parse("tyler/private-secret").unwrap(),
                    authority: Authority::Canonical,
                    default_branch: "main".into(),
                    secrets: vec![distinct],
                },
            ],
            linear: LinearConfig {
                endpoint: linear.endpoint.clone(),
                api_key: Secret::new("lin_api_test"),
            },
            retry: RetryPolicy {
                max_attempts: 3,
                base_delay: Duration::from_millis(1),
                max_delay: Duration::from_millis(5),
            },
        },
        EventStore::in_memory().unwrap(),
    );

    let make_body = |repo: &str, number: u64, title: &str| {
        serde_json::to_vec(&json!({
            "action": "opened",
            "number": number,
            "pull_request": {
                "number": number,
                "html_url": format!("https://git.dabney.moe/{repo}/pull/{number}"),
                "title": title,
                "body": "",
                "state": "open",
                "draft": false,
                "merged": false,
                "merge_commit_sha": null,
                "updated_at": "2026-11-25T10:00:00Z",
                "head": { "ref": "feat", "sha": HEAD },
                "base": { "ref": "main", "sha": HEAD },
            },
            "repository": { "full_name": repo, "default_branch": "main" },
        }))
        .unwrap()
    };

    let send = |body: Vec<u8>, delivery: &str, secret: &Secret| {
        let sig = sign_hex(secret, &body);
        Request::post("/hooks/forge/dabney")
            .header("x-forgejo-event", "pull_request")
            .header("x-forgejo-delivery", delivery)
            .header("x-forgejo-signature", sig)
            .body(Body::from(body))
            .unwrap()
    };

    // Repo 1 and Repo 2 share the same secret; BOTH must sync cleanly.
    for (repo, num, title, delivery) in [
        ("tyler/site", 1, "COD-1: first repo", "mr-1"),
        ("tyler/dabney-moe", 2, "COD-2: second repo", "mr-2"),
    ] {
        let req = send(make_body(repo, num, title), delivery, &shared);
        assert_eq!(
            bridge.router.clone().oneshot(req).await.unwrap().status(),
            StatusCode::ACCEPTED
        );
    }
    let outcomes = bridge.worker.drain_due().await.unwrap();
    assert_eq!(outcomes.len(), 2);
    assert!(
        outcomes.iter().all(|p| matches!(p, Processed::Done(_))),
        "{outcomes:?}"
    );
    assert_eq!(linear.attachments("COD-1").len(), 1);
    assert_eq!(linear.attachments("COD-2").len(), 1);

    // Signing a payload claiming `tyler/private-secret` with `shared` must NOT sync it!
    let spoof = send(
        make_body("tyler/private-secret", 3, "COD-3: spoofed"),
        "mr-3",
        &shared,
    );
    assert_eq!(
        bridge.router.clone().oneshot(spoof).await.unwrap().status(),
        StatusCode::ACCEPTED
    );
    let spoof_out = bridge.worker.drain_due().await.unwrap();
    assert!(
        matches!(&spoof_out[..], [Processed::DeadLettered { error }] if error.contains("does not match")),
        "{spoof_out:?}"
    );
    assert!(linear.attachments("COD-3").is_empty());
}

#[tokio::test]
async fn multi_pr_merge_then_close_reaches_done_while_both_closed_stays_todo() {
    let (linear, bridge) = setup(3).await;
    // Case A: PR #10 merges to main, then PR #11 closes unmerged -> Done.
    let p10 = Pr {
        number: 10,
        title: "COD-4: merged part",
        ..Pr::default()
    };
    let p11 = Pr {
        number: 11,
        title: "COD-4: superseded part",
        ..Pr::default()
    };
    deliver(&bridge, &p10, "a1").await;
    deliver(&bridge, &p11, "a2").await;
    deliver(
        &bridge,
        &Pr {
            state: "closed",
            merged: true,
            updated_at: "2026-11-25T13:00:00Z",
            ..p10
        },
        "a3",
    )
    .await;
    assert_eq!(linear.issue_state("COD-4"), "Todo", "PR #11 is still open");
    deliver(
        &bridge,
        &Pr {
            state: "closed",
            merged: false,
            updated_at: "2026-11-25T14:00:00Z",
            ..p11
        },
        "a4",
    )
    .await;
    assert_eq!(
        linear.issue_state("COD-4"),
        "Done",
        "closing remaining PR after a canonical default-branch merge finishes the issue"
    );

    // Case B: both PRs close unmerged -> must stay Todo.
    let p20 = Pr {
        number: 20,
        title: "COD-6: abandoned one",
        ..Pr::default()
    };
    let p21 = Pr {
        number: 21,
        title: "COD-6: abandoned two",
        ..Pr::default()
    };
    deliver(&bridge, &p20, "b1").await;
    deliver(&bridge, &p21, "b2").await;
    deliver(
        &bridge,
        &Pr {
            state: "closed",
            merged: false,
            updated_at: "2026-11-25T13:00:00Z",
            ..p20
        },
        "b3",
    )
    .await;
    deliver(
        &bridge,
        &Pr {
            state: "closed",
            merged: false,
            updated_at: "2026-11-25T14:00:00Z",
            ..p21
        },
        "b4",
    )
    .await;
    assert_eq!(linear.issue_state("COD-6"), "Todo");
}

#[tokio::test]
async fn mutation_success_false_is_failure_and_http_400_not_found_does_not_abort_labeling() {
    use forge_linear_sync::domain::IssueKey;

    let (linear, bridge) = setup(3).await;
    let pr = Pr::default();
    let _ = deliver(&bridge, &pr, "m0").await;
    assert_eq!(linear.attachments("COD-1").len(), 1);

    let client = &bridge.worker.client;
    for op in ["issue_update_state", "attachment_delete", "issue_add_label"] {
        linear.fail_next(Failure::MutationRejected);
        let err = match op {
            "issue_update_state" => client
                .issue_update_state("issue-1", "state-1")
                .await
                .unwrap_err(),
            "attachment_delete" => client.attachment_delete("att-1").await.unwrap_err(),
            "issue_add_label" => client
                .issue_add_label("issue-1", "label-1")
                .await
                .unwrap_err(),
            _ => unreachable!(),
        };
        assert!(err.to_string().contains("success=false"), "{op}: {err}");
    }

    // HTTP 400 "Entity not found" on the first issue must be treated as Ok(None)
    // so `mark_sync_error` continues and labels the second valid issue.
    // IssueLabels succeeds first, then the first issue lookup gets HTTP 400 NotFound,
    // wait: fail_next fires on the very next request, so test client.issue directly AND
    // exercise mark_sync_error with a missing issue + valid issue.
    linear.fail_next(Failure::NotFoundHttp400);
    assert!(client.issue("COD-999").await.unwrap().is_none());

    let keys = vec![
        IssueKey {
            team: TeamKey::parse("COD").unwrap(),
            number: 999,
        },
        IssueKey {
            team: TeamKey::parse("COD").unwrap(),
            number: 2,
        },
    ];
    forge_linear_sync::sync::mark_sync_error(client, &keys)
        .await
        .unwrap();
    assert_eq!(linear.issue_labels("COD-2"), ["pr-sync-error"]);
}

#[tokio::test]
async fn backfill_detects_repeated_page_partial_failure_and_cli_exits_nonzero() {
    use axum::routing::get;
    use axum::{Json, Router};

    let linear = MockLinear::start().await;
    linear.add_issue("COD-1");

    // 1. A broken forge API that ignores `page` and repeats PR #1 forever -> RepeatedPage error.
    let repeating_app = Router::new().route(
        "/api/v1/repos/tyler/site/pulls",
        get(|| async {
            Json(json!([{
                "number": 1,
                "html_url": "https://git.dabney.moe/tyler/site/pull/1",
                "title": "COD-1: stuck page",
                "body": "",
                "state": "open",
                "draft": false,
                "merged": false,
                "merge_commit_sha": null,
                "updated_at": "2026-11-25T10:00:00Z",
                "head": { "ref": "feat", "sha": HEAD },
                "base": { "ref": "main", "sha": HEAD }
            }]))
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, repeating_app).await.unwrap() });

    let cfg = config(&linear, 3);
    let repo_cfg = RepoConfig {
        forge: Forge::Dabney,
        path: RepoPath::parse("tyler/site").unwrap(),
        authority: Authority::Canonical,
        default_branch: "main".into(),
        secrets: vec![Secret::new("s")],
    };
    let client = forge_linear_sync::linear::LinearClient::new(cfg.linear.clone());
    let api_base: url::Url = format!("http://{addr}/api/v1").parse().unwrap();
    let err = forge_linear_sync::sync::backfill_forgejo(
        &client,
        Forge::Dabney,
        &repo_cfg,
        &cfg.matcher(),
        &api_base,
        None,
        1,
    )
    .await
    .unwrap_err();
    assert!(
        matches!(
            err,
            forge_linear_sync::sync::BackfillError::RepeatedPage { number: 1, page: 2 }
        ),
        "{err:?}"
    );

    // 2. A forge API returning an unparseable PR (integer updated_at) -> PartialFailure AND CLI exits non-zero.
    let bad_app = Router::new().route(
        "/api/v1/repos/tyler/site/pulls",
        get(|| async {
            Json(json!([{
                "number": 1,
                "html_url": "https://git.dabney.moe/tyler/site/pull/1",
                "title": "COD-1: bad timestamp",
                "state": "open",
                "updated_at": 1700000000,
                "head": { "ref": "feat", "sha": HEAD },
                "base": { "ref": "main", "sha": HEAD }
            }]))
        }),
    );
    let bad_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let bad_addr = bad_listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(bad_listener, bad_app).await.unwrap() });
    let bad_base: url::Url = format!("http://{bad_addr}/api/v1").parse().unwrap();

    let err = forge_linear_sync::sync::backfill_forgejo(
        &client,
        Forge::Dabney,
        &repo_cfg,
        &cfg.matcher(),
        &bad_base,
        None,
        50,
    )
    .await
    .unwrap_err();
    assert!(
        matches!(
            err,
            forge_linear_sync::sync::BackfillError::PartialFailure {
                failed: 1,
                total: 1,
                ..
            }
        ),
        "{err:?}"
    );

    // Verify the `forge-linear-sync backfill` binary exits non-zero on error.
    let tmp = tempfile::tempdir().unwrap();
    let cfg_path = tmp.path().join("sync.json");
    std::fs::write(
        &cfg_path,
        serde_json::to_vec(&json!({
            "teams": ["COD"],
            "repos": [{
                "forge": "dabney",
                "path": "tyler/site",
                "authority": "canonical",
                "default_branch": "main",
                "secret_envs": ["TEST_HOOK_SECRET"]
            }],
            "linear_endpoint": linear.endpoint.as_str(),
            "linear_api_key_env": "TEST_LINEAR_KEY"
        }))
        .unwrap(),
    )
    .unwrap();
    let bad_base_str = bad_base.to_string();
    let cfg_path_str = cfg_path.to_str().unwrap().to_owned();
    let out = tokio::task::spawn_blocking(move || {
        std::process::Command::new(env!("CARGO_BIN_EXE_forge-linear-sync"))
            .args([
                "backfill",
                "--config",
                &cfg_path_str,
                "--forge",
                "dabney",
                "--repo",
                "tyler/site",
                "--api-base",
                &bad_base_str,
            ])
            .env("TEST_HOOK_SECRET", "s")
            .env("TEST_LINEAR_KEY", "lin_api_test")
            .output()
            .expect("run forge-linear-sync binary")
    })
    .await
    .unwrap();
    assert!(
        !out.status.success(),
        "backfill CLI must exit non-zero when PRs fail: stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[tokio::test]
async fn same_second_open_does_not_downgrade_closed_state() {
    let (linear, bridge) = setup(3).await;
    let closed = Pr {
        number: 30,
        title: "COD-2: closed fast",
        state: "closed",
        merged: false,
        updated_at: "2026-11-25T10:00:00Z",
        ..Pr::default()
    };
    deliver(&bridge, &closed, "ss1").await;
    assert_eq!(linear.attachments("COD-2")[0].metadata["state"], "closed");

    let stale_open = Pr {
        number: 30,
        title: "COD-2: closed fast",
        state: "open",
        merged: false,
        updated_at: "2026-11-25T10:00:00Z",
        ..Pr::default()
    };
    let out = deliver(&bridge, &stale_open, "ss2").await;
    assert!(
        matches!(&out[..], [Processed::Done(o)] if o.stale),
        "{out:?}"
    );
    assert_eq!(linear.attachments("COD-2")[0].metadata["state"], "closed");
}
