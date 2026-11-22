//! Synthetic HTTP failures through the real transport and ordered execution loop.
//! No requests leave loopback and no provider credentials are used.
use super::{test_quota as quota, test_recover as recover};
use async_trait::async_trait;
use harness::{
    engine::{recipe::*, *},
    provider::{QuotaGate, ResponsesProvider},
};
use serde_json::{Value, json};
#[path = "../../../tests/support/requests.rs"]
#[allow(dead_code)]
mod request_records;
#[path = "../../../tests/support/responses.rs"]
mod wire;
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    time::Instant,
};

use quota::Gate;

// Leave real loopback I/O scheduling headroom under parallel workspace tests.
// Stalled fixtures outlast this budget; ordinary responses do not sleep.
pub(super) const TEST_IDLE: Duration = Duration::from_millis(500);

enum Reply {
    Http(&'static str, String, String),
    IncompleteHttp(&'static str, String, String),
    Disconnect,
    Idle,
    SlowBody,
}
impl Reply {
    fn ok(value: Value) -> Self {
        Self::Http("200 OK", String::new(), value.to_string())
    }
}

async fn server(
    replies: Vec<Reply>,
) -> (
    String,
    Arc<AtomicUsize>,
    tokio::task::JoinHandle<Vec<Value>>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/responses", listener.local_addr().unwrap());
    let count = Arc::new(AtomicUsize::new(0));
    let observed = count.clone();
    let task = tokio::spawn(async move {
        let mut requests = Vec::new();
        let mut responses = tokio::task::JoinSet::new();
        for reply in replies {
            let (mut socket, _) = listener.accept().await.unwrap();
            observed.fetch_add(1, Ordering::SeqCst);
            let mut bytes = Vec::new();
            let (offset, length) = loop {
                let mut chunk = [0; 4096];
                let read = socket.read(&mut chunk).await.unwrap();
                assert!(read > 0);
                bytes.extend_from_slice(&chunk[..read]);
                if let Some(offset) = bytes.windows(4).position(|value| value == b"\r\n\r\n") {
                    let length = String::from_utf8_lossy(&bytes[..offset])
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().unwrap())
                        })
                        .unwrap();
                    break (offset + 4, length);
                }
            };
            while bytes.len() < offset + length {
                let mut chunk = [0; 4096];
                let read = socket.read(&mut chunk).await.unwrap();
                assert!(read > 0);
                bytes.extend_from_slice(&chunk[..read]);
            }
            requests.push(serde_json::from_slice(&bytes[offset..offset + length]).unwrap());
            // The retry connects while the prior response is stalled.
            responses.spawn(async move {
            match reply {
                Reply::Http(status, headers, body) => {
                    let content_type = if status == "200 OK" {
                        "text/event-stream"
                    } else {
                        "application/json"
                    };
                    let body = if status == "200 OK" {
                        serde_json::from_str(&body)
                            .map(wire::stream)
                            .unwrap_or(body)
                    } else {
                        body
                    };
                    socket.write_all(format!("HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
                }
                Reply::IncompleteHttp(status, headers, body) => {
                    socket.write_all(format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()+1).as_bytes()).await.unwrap();
                }
                Reply::Disconnect => (),
                Reply::SlowBody => {
                    let body = wire::stream(answer());
                    let headers = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    socket.write_all(headers.as_bytes()).await.unwrap();
                    // Bytes keep arriving, but no complete event arrives.
                    // Only event progress can reset the silence timer.
                    for byte in &body.as_bytes()[..20] {
                        tokio::time::sleep(TEST_IDLE / 10).await;
                        if socket.write_all(&[*byte]).await.is_err() {
                            break;
                        }
                    }
                    let suffix = body[20..].to_owned();
                    tokio::time::sleep(TEST_IDLE * 2).await;
                    // A timed-out client may already have closed this fixture socket.
                    let _ = socket.write_all(suffix.as_bytes()).await;
                }
                Reply::Idle => {
                    tokio::time::sleep(TEST_IDLE * 2).await;
                }
            }
            });
        }
        while let Some(response) = responses.join_next().await {
            response.unwrap();
        }
        requests
    });
    (endpoint, count, task)
}

fn answer() -> Value {
    json!({"id":"paid-result","status":"completed","usage":{"input_tokens":5,"output_tokens":3},"output":[{"type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":"Saved answer"}]}]})
}
fn request() -> Request {
    Request {
        cache_key: None,
        model: "synthetic".into(),
        context_window_tokens: 100_000,
        max_input_tokens: 100_000,
        options: json!({}),
        instructions: "Owner".into(),
        items: vec![],
        tools: vec![
            Tool {
                name: "read".into(),
                description: "Read".into(),
                parameters: json!({"type":"object"}),
            }
            .function(),
        ],
    }
}
/// The result text before the appended limit warning, when the warning is there.
fn before_warning(output: &str) -> Option<&str> {
    output.strip_suffix(LIMIT_WARNING)?.strip_suffix("\n\n")
}
fn limits() -> RunLimits {
    RunLimits {
        max_calls: 6,
        ..RunLimits::default()
    }
}
#[derive(Default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each bool injects one independent fault into the fake"
)]
struct Owner {
    events: Vec<Event>,
    seed: Vec<Item>,
    stop_after_context_rejection: bool,
    stop_after_transport_retry: bool,
    calls: usize,
    output: Option<String>,
    partials: usize,
    write_failures: usize,
    failed_write: Option<Value>,
    permanent: bool,
    /// Authored sources the product reports at each currency check, in order.
    sources: std::collections::VecDeque<Vec<ProductMessage>>,
}
#[async_trait]
impl Product for Owner {
    async fn message_sources(
        &mut self,
    ) -> Result<Option<Vec<harness::engine::ProductMessage>>, harness::engine::ProductError> {
        Ok(self.sources.pop_front())
    }

    async fn admit_binding(
        &mut self,
        _: &harness::engine::recipe::RequestBinding,
    ) -> Result<(), harness::engine::ProductError> {
        Ok(())
    }

    async fn assert_current(&mut self) -> Result<(), ProductError> {
        Ok(())
    }
    async fn persist(&mut self, event: Event) -> Result<(), ProductError> {
        let value = json!(event);
        if matches!(event.kind, EventKind::ProviderReturned(_)) && self.write_failures > 0 {
            self.write_failures -= 1;
            if let Some(first) = &self.failed_write {
                assert_eq!(first, &value);
            }
            self.failed_write = Some(value);
            if self.permanent {
                return Err("storage semantic refusal".into());
            }
            return Err(RetryablePersistence::new(std::io::Error::from(
                std::io::ErrorKind::ConnectionReset,
            ))
            .into());
        }
        if self.stop_after_transport_retry && matches!(event.kind, EventKind::TransportRetry { .. })
        {
            self.events.push(event);
            self.stop_after_transport_retry = false;
            return Err("process stopped after TransportRetry was retained".into());
        }
        let stop =
            self.stop_after_context_rejection && matches!(event.kind, EventKind::ContextRejected);
        self.events.push(event);
        if stop {
            self.stop_after_context_rejection = false;
            return Err("process stopped after the context rejection was retained".into());
        }
        Ok(())
    }
    fn execute(&mut self, call: &Invocation, _: Value) -> harness::engine::recipe::ToolFuture {
        let outcome = {
            self.calls += 1;
            Ok(ToolOutcome::Returned(if call.name == "save_working_note" {
                json!({"reference":call.id}).to_string()
            } else {
                self.output.clone().unwrap_or_else(|| "source".into())
            }))
        };
        Box::pin(std::future::ready(outcome))
    }
    fn read_result(&mut self, id: &str) -> ToolFuture<Option<String>> {
        let original = harness::engine::results::resolve(
            self.events
                .iter()
                .map(harness::engine::recipe::ReplayEvent::Admitted),
            id,
            &[],
        )
        .map_err(Into::into);
        Box::pin(std::future::ready(original))
    }
    async fn finish(&mut self, _: &Turn) -> Result<Finish, ProductError> {
        Ok(Finish::Committed)
    }

    async fn finish_partial(
        &mut self,
        _: &Turn,
        _: &[harness::engine::LimitBackstop],
    ) -> Result<(), ProductError> {
        self.partials += 1;
        Ok(())
    }

    async fn window_seed(&mut self) -> Result<harness::engine::WindowSeed, ProductError> {
        Ok(harness::engine::WindowSeed {
            items: self.seed.clone(),
            note_references: Vec::new(),
        })
    }
}

#[tokio::test]
async fn context_refusal_opens_a_fresh_window_without_repeating_tools() {
    for (index, refusal) in [
        Reply::Http(
            "400 Bad Request",
            String::new(),
            json!({"error":{"code":"context_length_exceeded"}}).to_string(),
        ),
        Reply::ok(
            json!({"id":"too-long","status":"failed","error":{"code":"context_length_exceeded"}}),
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let restart = index == 1;
        let mut tool = json!({"id":"read-response","status":"completed","output":[{"type":"function_call","call_id":"source-read","name":"read","arguments":"{}","status":"completed"}]});
        if restart {
            tool["output"].as_array_mut().unwrap().insert(0, json!({
                "type":"function_call", "call_id":"saved-note", "name":"save_working_note",
                "arguments":json!({"text":"Exact saved working note.","fresh_window":false}).to_string(),
                "status":"completed"
            }));
        }
        let (endpoint, _, task) = server(vec![Reply::ok(tool), refusal, Reply::ok(answer())]).await;
        let mut provider = ResponsesProvider::new(
            endpoint,
            "synthetic-key".into(),
            std::sync::Arc::new(quota::Gate::default()),
        )
        .unwrap();
        let mut req = request();
        offer_working_notes(&mut req.instructions, &mut req.tools);
        req.items.push(Item::assistant(
            "Earlier investigation stays in the run record.",
        ));
        let mut owner = Owner {
            stop_after_context_rejection: restart,
            seed: vec![Item::Message {
                role: "user".into(),
                text: "Exact assignment and accepted steering.".into(),
            }],
            ..Default::default()
        };
        let bounds = limits();
        let mut result = recover::work(req.clone().freeze().unwrap(), "attempt", &[], 0)
            .unwrap()
            .run(&mut provider, &mut owner, bounds, &Cancellation::default())
            .await;
        if restart {
            assert!(matches!(result, Err(Failure::Host(_))));
            let saved = owner.events.clone();
            result = recover::work(req.freeze().unwrap(), "successor", &saved, 0)
                .unwrap()
                .run(
                    &mut provider,
                    &mut owner,
                    limits(),
                    &Cancellation::default(),
                )
                .await;
        }
        assert!(
            result.is_ok(),
            "context refusal must continue in a fresh window: {result:?}"
        );
        let sent = task.await.unwrap();
        assert_eq!(sent.len(), 3);
        let fresh = sent[2]["input"].to_string();
        assert!(fresh.contains("Exact assignment and accepted steering."));
        assert!(!fresh.contains("Earlier investigation stays in the run record."));
        assert!(fresh.contains("source-read") && fresh.contains("source"));
        assert_eq!(owner.calls, if restart { 2 } else { 1 });
        assert_eq!(owner.partials, 0);
        assert_eq!(
            fresh.contains("Exact saved working note."),
            restart,
            "the newest note saved in this work is carried exactly"
        );
        if restart {
            assert!(
                fresh.contains("Work done after this note was saved is not in it."),
                "a note saved before the window closed says it may be behind"
            );
        }
        assert!(owner.events.iter().any(|event| matches!(
            event.kind,
            EventKind::WindowOpened {
                reason: WindowReason::Refused,
                ..
            }
        )));
    }
}

#[tokio::test]
async fn a_live_or_recovered_retry_checks_backstops_before_another_dispatch() {
    for restart in [false, true] {
        let (endpoint, _, task) = server(vec![
            Reply::Http(
                "200 OK",
                String::new(),
                "data: {\"type\":\"response.created\",\"sequence_number\":0,\"response\":{\"id\":\"accepted\"}}\n\n".into(),
            ),
            Reply::ok(answer()),
        ])
        .await;
        let mut provider = ResponsesProvider::new(
            endpoint,
            "synthetic-key".into(),
            std::sync::Arc::new(quota::Gate::default()),
        )
        .unwrap();
        let mut owner = Owner {
            stop_after_transport_retry: restart,
            ..Default::default()
        };
        let req = request();
        let bounds = RunLimits {
            max_tokens: 1,
            ..limits()
        };
        let first = recover::work(req.clone().freeze().unwrap(), "first", &[], 0)
            .unwrap()
            .run(&mut provider, &mut owner, bounds, &Cancellation::default())
            .await;
        if restart {
            assert!(matches!(first, Err(Failure::Host(_))), "{first:?}");
            let saved: Vec<Event> =
                serde_json::from_slice(&serde_json::to_vec(&owner.events).unwrap()).unwrap();
            assert!(matches!(
                saved.last().unwrap().kind,
                EventKind::TransportRetry { .. }
            ));
            assert!(Progress::from_events(&saved).budget_tokens >= bounds.max_tokens);
            recover::work(req.freeze().unwrap(), "recovered", &saved, 0)
                .unwrap()
                .run(&mut provider, &mut owner, bounds, &Cancellation::default())
                .await
                .unwrap();
        } else {
            first.unwrap();
        }
        let requests = task.await.unwrap();
        assert_eq!(requests.len(), 2);
        let has_final = |request: &Value| {
            request["input"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["role"] == "developer" && item["content"] == FINAL_REQUEST)
        };
        assert!(!has_final(&requests[0]));
        assert!(
            has_final(&requests[1]),
            "a reached backstop must replace retry with the final request"
        );
        assert_eq!(owner.partials, 1, "the final answer is retained as partial");
        assert_eq!(owner.calls, 0);
    }
}

#[tokio::test]
async fn quiet_transients_retry_the_exact_request_in_the_same_run() {
    for first in [
        Reply::Http(
            "503 Service Unavailable",
            String::new(),
            json!({"error":{"code":"server_error"}}).to_string(),
        ),
        Reply::Http(
            "429 Too Many Requests",
            "Retry-After: 1\r\n".into(),
            json!({"error":{"code":"rate_limit_exceeded"}}).to_string(),
        ),
        Reply::Http(
            "408 Request Timeout",
            String::new(),
            json!({"error":{"type":"invalid_request_error"}}).to_string(),
        ),
        Reply::Http(
            "409 Conflict",
            String::new(),
            json!({"error":{"type":"invalid_request_error"}}).to_string(),
        ),
        Reply::IncompleteHttp(
            "429 Too Many Requests",
            "Retry-After: 1\r\n".into(),
            "{".into(),
        ),
        Reply::Http(
            "200 OK", String::new(),
            "data: {\"type\":\"error\",\"sequence_number\":0,\"error\":{\"code\":\"slow_down\",\"message\":\"Try again in 10s\",\"headers\":{\"Retry-After\":\"1\"}}}\n\n".into(),
        ),
        Reply::Http(
            "200 OK", String::new(),
            "data: {\"type\":\"error\",\"sequence_number\":0,\"error\":{\"code\":\"rate_limit_exceeded\",\"message\":\"Try again in 1000ms\"}}\n\n".into(),
        ),
        Reply::Disconnect,
        Reply::Idle,
        Reply::SlowBody,
        Reply::Http("200 OK", String::new(), String::new()),
        Reply::Http(
            "200 OK",
            String::new(),
            r#"data: {"type":"response.created"#.into(),
        ),
    ] {
        let is_quota = matches!(
            &first,
            Reply::Http("429 Too Many Requests", _, _)
                | Reply::IncompleteHttp("429 Too Many Requests", _, _)
        );
        let is_quota = is_quota || matches!(&first, Reply::Http("200 OK", _, body) if body.contains("Try again in"));
        let mut paid = answer();
        paid["usage"]["input_tokens_details"] = json!({"cached_tokens":0});
        let (endpoint, count, task) = server(vec![first, Reply::ok(paid)]).await;
        let mut provider = ResponsesProvider::new(endpoint, "synthetic-key".into(), Arc::new(Gate::default()))
            .unwrap();
        provider.idle = TEST_IDLE;
        let mut owner = Owner::default();
        let mut req = request();
        req.model = "gpt-6-luna".into();
        let start = Instant::now();
        let turn = recover::work(req.freeze().unwrap(), "attempt", &[], 0)
            .unwrap()
            .run(
                &mut provider,
                &mut owner,
                RunLimits {
                    max_calls: 2,
                    ..limits()
                },
                &Cancellation::default(),
            )
        .await
        .unwrap();
        let wire = task.await.unwrap();
        assert_eq!(wire.len(), 2);
        assert_eq!(wire[0], wire[1]);
        assert_eq!(count.load(Ordering::SeqCst), 2);
        assert_eq!(turn.text(), "Saved answer");
        let progress = Progress::from_events(&owner.events);
        assert_eq!(progress.calls, 2);
        assert_eq!(progress.model_steps(), 1);
        assert_eq!(progress.budget_tokens, 8, "a refusal has no provider receipt");
        assert_eq!(progress.unknown_usage, 1);
        assert_eq!(
            owner
                .events
                .iter()
                .filter(|event| matches!(event.kind, EventKind::ProviderReturned(_)))
                .count(),
            1
        );
        if is_quota {
            assert!(start.elapsed() >= Duration::from_secs(1));
        }
    }
}

#[tokio::test]
async fn a_retry_hint_above_sixty_seconds_ends_the_attempt_and_one_at_sixty_is_waited_for() {
    let refusal = |hint: u64| {
        Reply::Http(
            "429 Too Many Requests",
            format!("Retry-After: {hint}\r\n"),
            json!({"error":{"code":"rate_limit_exceeded"}}).to_string(),
        )
    };
    // A longer hint ends the attempt as failed, without waiting.
    let (endpoint, count, task) = server(vec![refusal(61)]).await;
    let gate = Arc::new(Gate::default());
    let mut provider =
        ResponsesProvider::new(endpoint, "synthetic-key".into(), gate.clone()).unwrap();
    let mut owner = Owner::default();
    let cancel = Cancellation::default();
    let start = Instant::now();
    let error = recover::work(request().freeze().unwrap(), "attempt", &[], 0)
        .unwrap()
        .run(&mut provider, &mut owner, limits(), &cancel)
        .await
        .unwrap_err();
    task.await.unwrap();
    assert_eq!(
        AttemptFailure::of(&error, &cancel),
        AttemptFailure::Failed,
        "{error}"
    );
    // The refusal is recorded as retryable; the wait before the retry then
    // ends the attempt without another request.
    assert!(matches!(
        owner.events.last().map(|event| &event.kind),
        Some(EventKind::TransportRetry { .. })
    ));
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert!(start.elapsed() < Duration::from_secs(60));
    // The gate keeps the provider's deadline, so the next attempt, from this
    // work or another, ends before any request reaches the provider.
    assert!(gate.remaining().await.unwrap() > Duration::from_secs(59));
    let (endpoint, count, task) = server(vec![Reply::ok(json!({}))]).await;
    let mut provider =
        ResponsesProvider::new(endpoint, "synthetic-key".into(), gate.clone()).unwrap();
    let error = recover::work(request().freeze().unwrap(), "next-attempt", &[], 0)
        .unwrap()
        .run(&mut provider, &mut Owner::default(), limits(), &cancel)
        .await
        .unwrap_err();
    task.abort();
    assert_eq!(
        AttemptFailure::of(&error, &cancel),
        AttemptFailure::Failed,
        "{error}"
    );
    assert_eq!(
        count.load(Ordering::SeqCst),
        0,
        "no request in the cooldown"
    );
    assert!(start.elapsed() < Duration::from_secs(60));
    // A hint at the cap is kept as the wait before the next request.
    let (endpoint, _, task) = server(vec![refusal(60)]).await;
    let gate = Arc::new(Gate::default());
    let mut provider =
        ResponsesProvider::new(endpoint, "synthetic-key".into(), gate.clone()).unwrap();
    let error = provider
        .exchange(&request(), &Cancellation::default(), &limits(), &mut |_| {
            Ok(())
        })
        .await
        .unwrap_err();
    task.await.unwrap();
    assert!(
        error.is::<crate::responses::Transient>(),
        "the request is tried again: {error}"
    );
    assert!(gate.remaining().await.unwrap() > Duration::from_secs(59));
}

#[tokio::test]
async fn a_message_deleted_before_a_transport_retry_is_not_sent_again() {
    let kept = ProductMessage {
        id: "kept".into(),
        role: crate::engine::history::Role::User,
        text: "Kept question.".into(),
    };
    let deleted = ProductMessage {
        id: "deleted".into(),
        role: crate::engine::history::Role::User,
        text: "Private text deleted before the retry.".into(),
    };
    let (endpoint, count, task) = server(vec![Reply::Disconnect, Reply::ok(answer())]).await;
    let mut provider =
        ResponsesProvider::new(endpoint, "synthetic-key".into(), Arc::new(Gate::default()))
            .unwrap();
    provider.idle = TEST_IDLE;
    let mut owner = Owner {
        sources: [vec![kept.clone(), deleted.clone()], vec![kept.clone()]].into(),
        ..Default::default()
    };
    let mut req = request();
    req.items = vec![Item::ProductMessage(kept), Item::ProductMessage(deleted)];
    let turn = recover::work(req.freeze().unwrap(), "attempt", &[], 0)
        .unwrap()
        .run(
            &mut provider,
            &mut owner,
            limits(),
            &Cancellation::default(),
        )
        .await
        .unwrap();
    assert_eq!(turn.text(), "Saved answer");
    let wire = task.await.unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 2);
    assert!(wire[0].to_string().contains("Private text deleted"));
    let retried = wire[1].to_string();
    assert!(
        !retried.contains("Private text deleted"),
        "the saved retry is dropped, so deleted text is not sent again"
    );
    assert!(
        retried.contains("changed_sources"),
        "the next request says the sources changed"
    );
}

#[tokio::test]
async fn incomplete_error_body_cannot_retry_a_permanent_http_rejection() {
    let (endpoint, count, task) = server(vec![Reply::IncompleteHttp(
        "401 Unauthorized",
        String::new(),
        "{".into(),
    )])
    .await;
    let mut provider = ResponsesProvider::new(
        endpoint,
        "synthetic-key".into(),
        std::sync::Arc::new(quota::Gate::default()),
    )
    .unwrap();
    let mut owner = Owner::default();
    let result = recover::work(request().freeze().unwrap(), "attempt", &[], 0)
        .unwrap()
        .run(
            &mut provider,
            &mut owner,
            limits(),
            &Cancellation::default(),
        )
        .await;
    task.await.unwrap();
    let error = result.unwrap_err();
    let Failure::Provider(cause) = &error else {
        panic!("expected a provider block: {error}")
    };
    assert!(cause.is::<harness::responses::ProviderBlock>());
    assert!(cause.source().unwrap().to_string().contains("401"));
    assert_eq!(
        AttemptFailure::of(&error, &Cancellation::default()),
        AttemptFailure::ProviderBlocked
    );
    assert!(matches!(owner.events.last().map(|event| &event.kind),
        Some(EventKind::ProviderBlocked { reason })
            if reason == "The model provider did not accept this request. Try again later."));
    assert_eq!(Progress::from_events(&owner.events).calls, 1);
    assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_rejected_request_holds_the_run_and_is_not_sent_again() {
    // A rejected request, and a retryable status whose body names a code
    // another try cannot fix.
    for (status, code) in [
        ("400 Bad Request", Value::Null),
        ("408 Request Timeout", json!("model_not_found")),
    ] {
        let (endpoint, count, task) = server(vec![Reply::Http(
            status,
            String::new(),
            json!({"error":{"type":"invalid_request_error","code":code}}).to_string(),
        )])
        .await;
        let mut provider =
            ResponsesProvider::new(endpoint, "synthetic-key".into(), Arc::new(Gate::default()))
                .unwrap();
        let mut owner = Owner::default();
        let cancel = Cancellation::default();
        let result = recover::work(request().freeze().unwrap(), "attempt", &[], 0)
            .unwrap()
            .run(&mut provider, &mut owner, limits(), &cancel)
            .await;
        task.await.unwrap();
        let error = result.unwrap_err();
        assert_eq!(
            AttemptFailure::of(&error, &cancel),
            AttemptFailure::ProviderBlocked,
            "{status}: {error}"
        );
        assert!(
            matches!(
                owner.events.last().map(|event| &event.kind),
                Some(EventKind::ProviderBlocked { .. })
            ),
            "{status}"
        );
        assert_eq!(count.load(Ordering::SeqCst), 1, "{status}: not sent again");
    }
}

#[tokio::test]
async fn insufficient_quota_is_a_named_permanent_billing_failure() {
    for (field, expected_cause) in [
        ("type", "usage_limit_reached"),
        ("type", "insufficient_quota"),
        ("type", "usage_not_included"),
        ("code", "insufficient_quota"),
    ] {
        let mut error = json!({"message":"private provider prose"});
        error[field] = json!(expected_cause);
        let (endpoint, count, task) = server(vec![Reply::Http(
            "429 Too Many Requests",
            "Retry-After: 60\r\n".into(),
            json!({"error":error}).to_string(),
        )])
        .await;
        let gate = Arc::new(Gate::default());
        let mut provider =
            ResponsesProvider::new(endpoint, "synthetic-key".into(), gate.clone()).unwrap();
        let mut owner = Owner::default();
        let result = recover::work(request().freeze().unwrap(), "attempt", &[], 0)
            .unwrap()
            .run(
                &mut provider,
                &mut owner,
                limits(),
                &Cancellation::default(),
            )
            .await;
        task.await.unwrap();
        let Failure::Provider(cause) = result.unwrap_err() else {
            panic!("expected provider failure")
        };
        assert!(cause.is::<harness::responses::ProviderBlock>());
        assert_eq!(
            cause.to_string(),
            format!(
                "Provider credits are exhausted. Restore billing, then retry this work ({expected_cause})."
            )
        );
        assert!(cause.source().is_some());
        assert!(
            matches!(owner.events.last().map(|event| &event.kind), Some(EventKind::ProviderBlocked { reason }) if reason.contains("Restore billing"))
        );
        assert_eq!(gate.remaining().await.unwrap(), Duration::ZERO);
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn cutoff_and_completed_empty_turns_get_correction_without_running_partial_tools() {
    for first in [
        json!({"id":"cutoff","status":"incomplete","incomplete_details":{"reason":"max_output_tokens"},"output":[{"type":"function_call","call_id":"partial","name":"read","arguments":"{"}],"usage":{"output_tokens":100}}),
        json!({"id":"empty","status":"completed","output":[]}),
    ] {
        let (endpoint, _, task) = server(vec![Reply::ok(first.clone()), Reply::ok(answer())]).await;
        let mut provider =
            ResponsesProvider::new(endpoint, "synthetic-key".into(), Arc::new(Gate::default()))
                .unwrap();
        let mut owner = Owner::default();
        recover::work(request().freeze().unwrap(), "attempt", &[], 0)
            .unwrap()
            .run(
                &mut provider,
                &mut owner,
                limits(),
                &Cancellation::default(),
            )
            .await
            .unwrap();
        let wire = task.await.unwrap();
        assert_eq!(wire.len(), 2);
        assert_eq!(wire[1]["input"][0]["role"], "developer");
        assert!(wire[1]["input"][0]["content"].as_str().unwrap().contains(
            if first["status"] == "incomplete" {
                "cut off"
            } else {
                "no answer"
            }
        ));
        assert_eq!(owner.calls, 0);
        if first["status"] == "incomplete" {
            let saved = owner
                .events
                .iter()
                .find_map(|event| match &event.kind {
                    EventKind::ProviderReturned(turn) => Some(turn),
                    _ => None,
                })
                .unwrap();
            assert_eq!(saved.items, [Item::OutputCutoff]);
            assert_eq!(saved.receipt["incomplete_output"], first["output"]);
            assert_eq!(
                saved.receipt["incomplete_details"],
                first["incomplete_details"]
            );
        }
    }
}

#[tokio::test]
async fn transient_paid_result_write_retries_same_event_without_another_model_call() {
    for permanent in [false, true] {
        let mut paid = answer();
        paid["usage"]["input_tokens_details"] = json!({"cached_tokens":0});
        let (endpoint, count, task) = server(vec![Reply::ok(paid)]).await;
        let mut provider =
            ResponsesProvider::new(endpoint, "synthetic-key".into(), Arc::new(Gate::default()))
                .unwrap();
        let mut owner = Owner {
            write_failures: 2,
            permanent,
            ..Owner::default()
        };
        let mut req = request();
        req.model = "gpt-6-luna".into();
        let result = recover::work(req.freeze().unwrap(), "attempt", &[], 0)
            .unwrap()
            .run(
                &mut provider,
                &mut owner,
                limits(),
                &Cancellation::default(),
            )
            .await;
        task.await.unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert_eq!(Progress::from_events(&owner.events).calls, 1);
        if permanent {
            assert!(matches!(result, Err(Failure::Host(_))));
        } else {
            assert_eq!(result.unwrap().text(), "Saved answer");
            let saved = owner
                .events
                .iter()
                .find(|event| matches!(event.kind, EventKind::ProviderReturned(_)))
                .unwrap();
            assert_eq!(json!(saved), owner.failed_write.unwrap());
            let progress = Progress::from_events(&owner.events);
            assert_eq!(progress.budget_tokens, 8);
            assert_eq!(progress.unknown_usage, 0);
            assert_eq!(
                owner
                    .events
                    .iter()
                    .map(|event| event.sequence)
                    .collect::<Vec<_>>(),
                (1..=owner.events.len() as u64).collect::<Vec<_>>()
            );
        }
    }
}

fn context_refusal() -> Reply {
    Reply::Http(
        "400 Bad Request",
        String::new(),
        json!({"error":{"code":"context_length_exceeded"}}).to_string(),
    )
}

#[tokio::test]
async fn a_refused_fresh_window_gets_one_smaller_retry_across_restarts() {
    for restart in [false, true] {
        let tool = json!({"id":"read-response","status":"completed","output":[
            {"type":"function_call","call_id":"source-read","name":"read","arguments":"{}","status":"completed"},
            {"type":"function_call","call_id":"page","name":"read_tool_result","arguments":json!({"call_id":"source-read","offset":0}).to_string(),"status":"completed"}
        ]});
        let (endpoint, _, task) = server(vec![
            Reply::ok(tool),
            context_refusal(),
            context_refusal(),
            context_refusal(),
        ])
        .await;
        let mut provider = ResponsesProvider::new(
            endpoint,
            "synthetic-key".into(),
            std::sync::Arc::new(quota::Gate::default()),
        )
        .unwrap();
        let mut req = request();
        offer_working_notes(&mut req.instructions, &mut req.tools);
        req.tools.push(harness::engine::results::tool().function());
        let mut owner = Owner {
            output: Some("evidence 🪕".repeat(4000)),
            seed: vec![Item::Message {
                role: "user".into(),
                text: "Finish the accepted task".into(),
            }],
            ..Default::default()
        };
        let mut result;
        let mut attempt = 0;
        loop {
            owner.stop_after_context_rejection = restart;
            let id = format!("attempt-{attempt}");
            let saved = owner.events.clone();
            result = recover::work(req.clone().freeze().unwrap(), &id, &saved, 0)
                .unwrap()
                .run(
                    &mut provider,
                    &mut owner,
                    limits(),
                    &Cancellation::default(),
                )
                .await;
            if !restart || !matches!(result, Err(Failure::Host(_))) {
                break;
            }
            owner.events =
                serde_json::from_slice(&serde_json::to_vec(&owner.events).unwrap()).unwrap();
            attempt += 1;
            assert!(
                attempt <= 3,
                "only three rejected requests may be dispatched"
            );
        }
        let sent = task.await.unwrap();
        assert_eq!(
            result.unwrap_err().to_string(),
            "provider refused the last permitted context request",
            "refusal must hold immediately, without waiting for the run deadline"
        );
        assert_eq!(sent.len(), 4);
        let outputs = |index: usize| {
            sent[index]["input"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|item| item["type"] == "function_call_output")
                .map(|item| {
                    (
                        item["call_id"].as_str().unwrap().to_owned(),
                        item["output"].as_str().unwrap().to_owned(),
                    )
                })
                .collect::<std::collections::BTreeMap<_, _>>()
        };
        let fresh = outputs(2);
        let reduced = outputs(3);
        assert!(fresh["source-read"].chars().count() > 1500);
        assert!(reduced["source-read"].chars().count() <= 1500);
        assert!(fresh["page"].chars().count() > 8000);
        assert!(reduced["page"].chars().count() <= 8000);
        let page: Value = serde_json::from_str(&reduced["page"]).unwrap();
        assert_eq!(page["source_call_id"], "source-read");
        assert!(
            page.get("tail").is_none(),
            "a requested page is contiguous, never a preview"
        );
        assert!(page["next_offset"].as_u64().unwrap() > 0);
        assert_eq!(
            owner.calls, 1,
            "window retries cannot repeat source effects"
        );
    }
}

#[tokio::test]
async fn a_crash_during_a_fresh_windows_tool_calls_keeps_the_live_refusal_order() {
    // What each dispatch's window stage was and why each window opened.
    fn choices(events: &[Event]) -> (Vec<WindowStage>, Vec<WindowReason>) {
        let stages = events
            .iter()
            .filter_map(|event| match &event.kind {
                EventKind::ProviderStarted { context, .. } => Some(context.window_stage),
                _ => None,
            })
            .collect();
        let reasons = events
            .iter()
            .filter_map(|event| match &event.kind {
                EventKind::WindowOpened { reason, .. } => Some(*reason),
                _ => None,
            })
            .collect();
        (stages, reasons)
    }
    let tool = json!({"id":"read-response","status":"completed","output":[
        {"type":"function_call","call_id":"source-read","name":"read","arguments":"{}","status":"completed"}
    ]});
    // Live: refused, then a fresh window returns a call, then refused again.
    let (endpoint, _, task) = server(vec![
        context_refusal(),
        Reply::ok(tool),
        context_refusal(),
        Reply::ok(answer()),
    ])
    .await;
    let mut provider =
        ResponsesProvider::new(endpoint, "synthetic-key".into(), Arc::new(Gate::default()))
            .unwrap();
    let mut req = request();
    offer_working_notes(&mut req.instructions, &mut req.tools);
    let mut owner = Owner {
        seed: vec![Item::Message {
            role: "user".into(),
            text: "Finish the accepted task".into(),
        }],
        ..Default::default()
    };
    recover::work(req.clone().freeze().unwrap(), "attempt", &[], 0)
        .unwrap()
        .run(
            &mut provider,
            &mut owner,
            limits(),
            &Cancellation::default(),
        )
        .await
        .unwrap();
    assert_eq!(task.await.unwrap().len(), 4);
    let live = choices(&owner.events);
    assert_eq!(
        live,
        (
            vec![
                WindowStage::Accumulated,
                WindowStage::Fresh,
                WindowStage::Accumulated,
                WindowStage::Fresh,
            ],
            vec![WindowReason::Refused, WindowReason::Refused],
        )
    );
    // Recovered: the process stopped once the fresh window's call turn was saved.
    let returned = owner
        .events
        .iter()
        .position(|event| matches!(&event.kind, EventKind::ProviderReturned(turn) if turn.calls().next().is_some()))
        .unwrap();
    owner.events.truncate(returned + 1);
    owner.calls = 0;
    let saved = owner.events.clone();
    let (endpoint, _, task) = server(vec![context_refusal(), Reply::ok(answer())]).await;
    let mut provider =
        ResponsesProvider::new(endpoint, "synthetic-key".into(), Arc::new(Gate::default()))
            .unwrap();
    let result = recover::work(req.freeze().unwrap(), "successor", &saved, 0)
        .unwrap()
        .run(
            &mut provider,
            &mut owner,
            limits(),
            &Cancellation::default(),
        )
        .await;
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(task.await.unwrap().len(), 2);
    assert_eq!(owner.calls, 1);
    assert_eq!(
        choices(&owner.events),
        live,
        "after one refusal, recovery must reopen the window as the live loop did"
    );
}

#[tokio::test]
async fn refusal_retry_previews_the_original_instead_of_its_added_warning() {
    let (endpoint, _, task) = server(vec![
        Reply::ok(json!({"id":"read","status":"completed","output":[
            {"type":"function_call","call_id":"source","name":"read","arguments":"{}","status":"completed"}
        ]})), context_refusal(), context_refusal(), context_refusal(),
    ]).await;
    let mut provider = ResponsesProvider::new(
        endpoint,
        "synthetic-key".into(),
        std::sync::Arc::new(quota::Gate::default()),
    )
    .unwrap();
    let mut req = request();
    offer_working_notes(&mut req.instructions, &mut req.tools);
    req.tools.push(harness::engine::results::tool().function());
    let original = "Source 🪕 with exact ending.".repeat(1000);
    let mut owner = Owner {
        output: Some(original.clone()),
        seed: vec![Item::Message {
            role: "user".into(),
            text: "Continue".into(),
        }],
        ..Default::default()
    };
    let saved: Vec<_> = (1..=88)
        .map(|ordinal| Event {
            attempt_id: "prior".into(),
            sequence: ordinal,
            ordinal,
            kind: EventKind::ProviderStarted {
                dispatched_at_unix_ms: 1,
                request: request_records::saved("prior", ordinal, &request()),
                context: Default::default(),
            },
        })
        .collect();
    let rows: Vec<_> = saved.into_iter().map(|event| (event, false)).collect();
    let result = recover::from_rows(req.freeze().unwrap(), "attempt", &rows, 0)
        .unwrap()
        .run(
            &mut provider,
            &mut owner,
            RunLimits {
                max_calls: 100,
                ..limits()
            },
            &Cancellation::default(),
        )
        .await;
    assert!(matches!(result, Err(Failure::ContextRefused)), "{result:?}");
    let requests = task.await.unwrap();
    let output = |index: usize| {
        requests[index]["input"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["type"] == "function_call_output")
            .unwrap()["output"]
            .as_str()
            .unwrap()
    };
    assert!(
        output(1).contains(LIMIT_WARNING),
        "the refused original had harness guidance added"
    );
    let reduced = output(3);
    assert!(
        reduced.contains(LIMIT_WARNING),
        "a refused warning must survive the bounded retry"
    );
    let preview: Value =
        serde_json::from_str(before_warning(reduced).expect("the warning follows the preview"))
            .unwrap();
    assert!(reduced.chars().count() <= 1500);
    assert_eq!(preview["total_bytes"], original.len());
    let offset = preview["tail"]["offset"].as_u64().unwrap() as usize;
    let tail = preview["tail"]["text"].as_str().unwrap();
    assert_eq!(original.get(offset..), Some(tail));
    assert_eq!(offset + tail.len(), original.len());
    assert_eq!(owner.calls, 1);
}

#[tokio::test]
async fn a_refused_final_is_never_dispatched_again() {
    for restart in [false, true] {
        let (endpoint, _, task) = server(vec![context_refusal()]).await;
        let mut provider = ResponsesProvider::new(
            endpoint,
            "synthetic-key".into(),
            std::sync::Arc::new(quota::Gate::default()),
        )
        .unwrap();
        let mut req = request();
        offer_working_notes(&mut req.instructions, &mut req.tools);
        let mut owner = Owner {
            stop_after_context_rejection: restart,
            ..Default::default()
        };
        let bounds = RunLimits {
            max_calls: 1,
            ..limits()
        };
        let mut result = recover::work(req.clone().freeze().unwrap(), "first", &[], 0)
            .unwrap()
            .run(&mut provider, &mut owner, bounds, &Cancellation::default())
            .await;
        if restart {
            assert!(matches!(result, Err(Failure::Host(_))));
            let saved = owner.events.clone();
            result = recover::work(req.freeze().unwrap(), "recovered", &saved, 0)
                .unwrap()
                .run(&mut provider, &mut owner, bounds, &Cancellation::default())
                .await;
        }
        task.await.unwrap();
        let error = result.unwrap_err();
        assert_eq!(
            AttemptFailure::of(&error, &harness::engine::Cancellation::default()),
            AttemptFailure::Limit,
            "a refused final stays held at the limit, live and after recovery: {error}"
        );
        assert_eq!(
            owner
                .events
                .iter()
                .filter(|event| matches!(event.kind, EventKind::ProviderStarted { .. }))
                .count(),
            1,
            "a refused final must end without another dispatch"
        );
        assert_eq!(owner.calls, 0);
    }
}

#[tokio::test]
async fn a_refused_handoff_keeps_the_note_saved_for_that_window() {
    for restart in [false, true] {
        let note = "Exact note for this fresh window.";
        let saved = json!({"id":"save-response","status":"completed","output":[{"type":"function_call","call_id":"new-note","name":"save_working_note","arguments":json!({"text":note,"fresh_window":true}).to_string(),"status":"completed"}]});
        let (endpoint, _, task) = server(vec![
            Reply::ok(saved),
            context_refusal(),
            Reply::ok(answer()),
        ])
        .await;
        let mut provider = ResponsesProvider::new(
            endpoint,
            "synthetic-key".into(),
            std::sync::Arc::new(quota::Gate::default()),
        )
        .unwrap();
        let mut req = request();
        offer_working_notes(&mut req.instructions, &mut req.tools);
        let mut owner = Owner {
            stop_after_context_rejection: restart,
            ..Default::default()
        };
        let mut result = recover::work(req.clone().freeze().unwrap(), "first", &[], 0)
            .unwrap()
            .run(
                &mut provider,
                &mut owner,
                limits(),
                &Cancellation::default(),
            )
            .await;
        if restart {
            assert!(matches!(result, Err(Failure::Host(_))));
            let saved = owner.events.clone();
            result = recover::work(req.freeze().unwrap(), "recovered", &saved, 0)
                .unwrap()
                .run(
                    &mut provider,
                    &mut owner,
                    limits(),
                    &Cancellation::default(),
                )
                .await;
        }
        result.unwrap();
        let sent = task.await.unwrap();
        assert!(sent[1].to_string().contains(note));
        assert!(
            sent[2].to_string().contains(note),
            "a refused fresh window must retain its confirmed handoff"
        );
        assert_eq!(owner.calls, 1, "retry never saves the note again");
    }
}
