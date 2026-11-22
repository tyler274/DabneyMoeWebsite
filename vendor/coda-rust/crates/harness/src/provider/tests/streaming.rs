//! Loopback SSE protocol and owner acknowledgment proof. No live provider calls.
use super::{recovery_tests::TEST_IDLE, test_quota as quota, test_recover as recover};
use async_trait::async_trait;
use harness::{
    engine::{
        recipe::{Finish, Request, TextDelivery},
        *,
    },
    provider::ResponsesProvider,
};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

fn limits() -> RunLimits {
    RunLimits {
        max_calls: 3,
        max_response_bytes: 100_000,
        ..RunLimits::default()
    }
}
fn request() -> Request {
    Request {
        cache_key: None,
        model: "test-model".into(),
        context_window_tokens: 100_000,
        max_input_tokens: 100_000,
        options: json!({}),
        instructions: "owner".into(),
        items: vec![],
        tools: vec![
            Tool {
                name: "save".into(),
                description: "save".into(),
                parameters: json!({"type":"object"}),
            }
            .function(),
        ],
    }
}
fn message() -> Value {
    json!({"type":"message","id":"m","role":"assistant","status":"completed","content":[{"type":"output_text","text":"Hi é","annotations":[]}]})
}
fn events(tool: bool) -> Vec<Value> {
    let mut events = vec![
        json!({"type":"response.created","response":{"id":"r"}}),
        json!({"type":"response.output_item.added","output_index":0,"item":{"type":"message","id":"m","role":"assistant","content":[]}}),
        json!({"type":"response.content_part.added","output_index":0,"item_id":"m","content_index":0,"part":{"type":"output_text","text":""}}),
        json!({"type":"response.output_text.delta","output_index":0,"item_id":"m","content_index":0,"delta":"Hi "}),
        json!({"type":"response.output_text.delta","output_index":0,"item_id":"m","content_index":0,"delta":"é"}),
        json!({"type":"response.output_text.done","output_index":0,"item_id":"m","content_index":0,"text":"Hi é"}),
        json!({"type":"response.content_part.done","output_index":0,"item_id":"m","content_index":0,"part":message()["content"][0]}),
        json!({"type":"response.output_item.done","output_index":0,"item":message()}),
    ];
    let mut output = vec![message()];
    if tool {
        let call = json!({"type":"function_call","id":"f","call_id":"c","name":"save","arguments":"{}","status":"completed"});
        // Arguments arrive between text fragments, without running the tool early.
        events.insert(4, json!({"type":"response.output_item.added","output_index":1,"item":{"type":"function_call","id":"f","call_id":"c","name":"save","arguments":""}}));
        events.insert(5, json!({"type":"response.function_call_arguments.delta","output_index":1,"item_id":"f","delta":"{"}));
        events.push(json!({"type":"response.function_call_arguments.delta","output_index":1,"item_id":"f","delta":"}"}));
        events.push(json!({"type":"response.function_call_arguments.done","output_index":1,"item_id":"f","arguments":"{}"}));
        events.push(json!({"type":"response.output_item.done","output_index":1,"item":call}));
        output.push(call);
    }
    events.push(json!({"type":"response.completed","response":{"id":"r","status":"completed","output":output,"usage":{}}}));
    for (i, event) in events.iter_mut().enumerate() {
        event["sequence_number"] = json!(i);
    }
    events
}
fn bytes(events: &[Value]) -> Vec<u8> {
    let mut result = b": heartbeat\r\n\r\n".to_vec();
    for event in events {
        result.extend_from_slice(
            format!(
                "event: {}\r\ndata: {}\r\n\r\n",
                event["type"].as_str().unwrap(),
                event
            )
            .as_bytes(),
        );
    }
    result
}

async fn server(
    replies: Vec<Vec<u8>>,
    hold: bool,
) -> (String, Arc<AtomicUsize>, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1/responses", listener.local_addr().unwrap());
    let count = Arc::new(AtomicUsize::new(0));
    let observed = count.clone();
    let task = tokio::spawn(async move {
        for reply in replies {
            let (mut stream, _) = listener.accept().await.unwrap();
            observed.fetch_add(1, Ordering::SeqCst);
            let mut request = Vec::new();
            loop {
                let mut chunk = [0; 4096];
                let n = stream.read(&mut chunk).await.unwrap();
                if n == 0 {
                    return;
                }
                request.extend_from_slice(&chunk[..n]);
                if let Some(offset) = request.windows(4).position(|v| v == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&request[..offset]);
                    let len = headers
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().unwrap())
                        })
                        .unwrap();
                    if request.len() >= offset + 4 + len {
                        let body: Value =
                            serde_json::from_slice(&request[offset + 4..offset + 4 + len]).unwrap();
                        assert_eq!(body["stream"], true);
                        break;
                    }
                }
            }
            stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n").await.unwrap();
            // Every byte is a transport write; the decoder is also unit-tested
            // bytewise because TCP may coalesce these writes.
            for byte in reply {
                if stream.write_all(&[byte]).await.is_err() {
                    return;
                }
            }
            if hold {
                std::future::pending::<()>().await;
            }
        }
    });
    (endpoint, count, task)
}
#[derive(Default)]
struct Owner {
    events: Vec<Event>,
    visible: String,
    tools: usize,
    reject: bool,
    cancel: Option<Cancellation>,
    private: bool,
    checks: usize,
}
#[async_trait]
impl Product for Owner {
    fn read_result(
        &mut self,
        _call_id: &str,
    ) -> harness::engine::recipe::ToolFuture<Option<String>> {
        Box::pin(async { Err("result paging is unavailable for this fixture".into()) })
    }
    async fn finish_partial(
        &mut self,
        _turn: &harness::engine::Turn,
        _backstops: &[harness::engine::LimitBackstop],
    ) -> Result<(), harness::engine::ProductError> {
        Err("this fixture has no partial-result disposition".into())
    }
    async fn window_seed(
        &mut self,
    ) -> Result<harness::engine::WindowSeed, harness::engine::ProductError> {
        Err("working windows are unavailable for this fixture".into())
    }

    async fn message_sources(
        &mut self,
    ) -> Result<Option<Vec<harness::engine::ProductMessage>>, harness::engine::ProductError> {
        Ok(None)
    }

    async fn admit_binding(
        &mut self,
        _: &harness::engine::recipe::RequestBinding,
    ) -> Result<(), harness::engine::ProductError> {
        Ok(())
    }

    async fn assert_current(&mut self) -> Result<(), ProductError> {
        self.checks += 1;
        Ok(())
    }
    async fn live_text(&mut self, _: u64, delta: TextDelta) -> Result<TextDelivery, ProductError> {
        if self.reject {
            return Err("publication refused".into());
        }
        if !self.private {
            self.visible.push_str(&delta.text);
        }
        if let Some(cancel) = &self.cancel {
            cancel.cancel();
        }
        Ok(if self.private {
            TextDelivery::Hidden
        } else {
            TextDelivery::Visible
        })
    }
    async fn persist(&mut self, event: Event) -> Result<(), ProductError> {
        self.events.push(event);
        Ok(())
    }
    fn execute(&mut self, _: &Invocation, _: Value) -> harness::engine::recipe::ToolFuture {
        let outcome = {
            self.tools += 1;
            Ok(ToolOutcome::Returned("saved".into()))
        };
        Box::pin(std::future::ready(outcome))
    }
    async fn finish(&mut self, _: &Turn) -> Result<Finish, ProductError> {
        Ok(Finish::Committed)
    }
}

async fn attempt(
    replies: Vec<Vec<u8>>,
    owner: &mut Owner,
    cap: usize,
    hold: bool,
    cancel: &Cancellation,
) -> (Result<Turn, Failure>, usize) {
    let (endpoint, count, task) = server(replies, hold).await;
    let mut provider = ResponsesProvider::new(
        endpoint,
        "local-test".into(),
        std::sync::Arc::new(quota::Gate::default()),
    )
    .unwrap();
    let mut bounds = limits();
    bounds.max_response_bytes = cap;
    let result = recover::work(request().freeze().unwrap(), "a", &[], 0)
        .unwrap()
        .run(&mut provider, owner, bounds, cancel)
        .await;
    let count = count.load(Ordering::SeqCst);
    task.abort();
    (result, count)
}

#[tokio::test]
async fn streamed_prose_survives_tool_interleaving_and_finalization() {
    let mut owner = Owner::default();
    let (result, count) = attempt(
        vec![bytes(&events(true)), bytes(&events(false))],
        &mut owner,
        100_000,
        false,
        &Cancellation::default(),
    )
    .await;
    let result = result.unwrap();
    assert_eq!(count, 2);
    assert_eq!(owner.tools, 1);
    assert_eq!(result.text(), "Hi é");
    assert_eq!(owner.visible, "Hi éHi é");
    let retained_text: String = owner
        .events
        .iter()
        .filter_map(|event| match &event.kind {
            EventKind::ProviderReturned(turn) => Some(turn.text()),
            _ => None,
        })
        .collect();
    assert_eq!(retained_text, owner.visible);
    let start = owner
        .events
        .iter()
        .position(|e| matches!(e.kind, EventKind::ToolStarted(_)))
        .unwrap();
    let returned = owner
        .events
        .iter()
        .position(|e| matches!(e.kind, EventKind::ProviderReturned(_)))
        .unwrap();
    assert!(returned < start);
}
#[tokio::test]
async fn unknown_items_and_content_parts_leave_the_valid_call_to_run() {
    let search = json!({"type":"web_search_call","id":"ws","status":"completed"});
    let note = json!({"type":"message","id":"m","role":"assistant","status":"completed","content":[
        {"type":"output_text","text":"Saving now.","annotations":[]},
        {"type":"output_audio","data":"AAAA"}
    ]});
    let call = json!({"type":"function_call","id":"f","call_id":"c","name":"save","arguments":"{}","status":"completed"});
    let mut first = vec![json!({"type":"response.created","response":{"id":"r"}})];
    for (index, item) in [search, note, call].into_iter().enumerate() {
        first.push(json!({"type":"response.output_item.added","output_index":index,"item":item}));
        first.push(json!({"type":"response.output_item.done","output_index":index,"item":item}));
    }
    first.push(
        json!({"type":"response.completed","response":{"id":"r","status":"completed","usage":{}}}),
    );
    for (index, event) in first.iter_mut().enumerate() {
        event["sequence_number"] = json!(index);
    }
    let mut owner = Owner::default();
    let (result, count) = attempt(
        vec![bytes(&first), bytes(&events(false))],
        &mut owner,
        100_000,
        false,
        &Cancellation::default(),
    )
    .await;
    assert_eq!(result.unwrap().text(), "Hi é");
    assert_eq!(count, 2);
    assert_eq!(owner.tools, 1, "the valid call runs");
    let turn = owner
        .events
        .iter()
        .find_map(|event| match &event.kind {
            EventKind::ProviderReturned(turn) => Some(turn),
            _ => None,
        })
        .unwrap();
    assert_eq!(turn.text(), "Saving now.");
    assert_eq!(turn.calls().count(), 1);
}

#[tokio::test]
async fn disconnect_after_visible_text_is_not_retried_or_completed() {
    let mut owner = Owner::default();
    let prefix = events(true)[..6].to_vec();
    let (result, count) = attempt(
        vec![bytes(&prefix)],
        &mut owner,
        100_000,
        false,
        &Cancellation::default(),
    )
    .await;
    assert!(matches!(result, Err(Failure::AfterOutput(_))));
    assert_eq!(count, 1);
    assert_eq!(owner.visible, "Hi ");
    assert_eq!(owner.tools, 0);
    assert!(
        !owner
            .events
            .iter()
            .any(|e| matches!(e.kind, EventKind::Candidate { .. }))
    );
}

#[tokio::test]
async fn empty_delta_before_disconnect_does_not_count_as_visible_output() {
    for private in [false, true] {
        let mut prefix = events(false)[..4].to_vec();
        if !private {
            prefix[3]["delta"] = json!("");
        }
        let (endpoint, count, task) =
            server(vec![bytes(&prefix), bytes(&events(false))], false).await;
        let mut provider = ResponsesProvider::new(
            endpoint,
            "synthetic".into(),
            std::sync::Arc::new(quota::Gate::default()),
        )
        .unwrap();
        let mut owner = Owner {
            private,
            ..Default::default()
        };
        let result = recover::work(request().freeze().unwrap(), "a", &[], 0)
            .unwrap()
            .run(
                &mut provider,
                &mut owner,
                limits(),
                &Cancellation::default(),
            )
            .await
            .unwrap();
        task.await.unwrap();
        assert_eq!(result.text(), "Hi é");
        assert_eq!(owner.visible, if private { "" } else { "Hi é" });
        assert_eq!(count.load(Ordering::SeqCst), 2);
    }
}

#[tokio::test]
async fn stream_errors_retry_except_outcomes_another_try_cannot_fix() {
    // The hold's fixed text states its kind.
    let billing = |code: &str| {
        Some(format!(
            "Provider credits are exhausted. Restore billing, then retry this work ({code})."
        ))
    };
    let not_accepted =
        || Some("The model provider did not accept this request. Try again later.".to_owned());
    let filtered = || {
        Some("The model provider filtered this reply. Try again, or change the request.".to_owned())
    };
    for (code, blocked) in [
        (Some("server_error"), None),
        (Some("server_is_overloaded"), None),
        (Some("policy_capacity_unavailable"), None),
        (Some("rate_limit_exceeded"), None),
        (Some("slow_down"), None),
        (Some("unknown_code"), None),
        (None, None),
        (Some("insufficient_quota"), billing("insufficient_quota")),
        (
            Some("credit_balance_exhausted"),
            billing("credit_balance_exhausted"),
        ),
        (
            Some("organization_spend_limit_exceeded"),
            billing("organization_spend_limit_exceeded"),
        ),
        (
            Some("project_spend_limit_exceeded"),
            billing("project_spend_limit_exceeded"),
        ),
        (
            Some("organization_usage_limit_exceeded"),
            billing("organization_usage_limit_exceeded"),
        ),
        (Some("model_not_found"), not_accepted()),
        (Some("budget_reached"), not_accepted()),
        (Some("flex_unavailable"), not_accepted()),
        (Some("invalid_prompt"), not_accepted()),
        (Some("usage_not_included"), billing("usage_not_included")),
        (Some("cyber_policy"), not_accepted()),
        (Some("bio_policy"), not_accepted()),
        (Some("misalignment_policy_violation"), not_accepted()),
        (Some("content_filter"), filtered()),
    ] {
        // An error envelope can arrive before response.created. A filtered
        // reply ends as an incomplete response that names its reason.
        let error = match code {
            None => bytes(&[
                json!({"type":"response.created","sequence_number":0,"response":{"id":"r"}}),
                json!({"type":"response.failed","sequence_number":1,"response":{"id":"r"}}),
            ]),
            Some("content_filter") => bytes(&[
                json!({"type":"response.created","sequence_number":0,"response":{"id":"r"}}),
                json!({"type":"response.incomplete","sequence_number":1,"response":{"id":"r","status":"incomplete","incomplete_details":{"reason":"content_filter"}}}),
            ]),
            Some(code) => bytes(&[json!({"type":"error","sequence_number":0,"code":code})]),
        };
        let replies = if blocked.is_some() {
            vec![error]
        } else {
            vec![error, bytes(&events(false))]
        };
        let (endpoint, count, task) = server(replies, false).await;
        let mut provider = ResponsesProvider::new(
            endpoint,
            "synthetic".into(),
            std::sync::Arc::new(quota::Gate::default()),
        )
        .unwrap();
        let mut owner = Owner::default();
        let cancel = Cancellation::default();
        let result = recover::work(request().freeze().unwrap(), "attempt", &[], 0)
            .unwrap()
            .run(&mut provider, &mut owner, limits(), &cancel)
            .await;
        task.abort();
        if let Some(kind) = blocked {
            let error = result.expect_err("an outcome another try cannot fix stops the run");
            let Failure::Provider(cause) = &error else {
                panic!("expected a provider block: {code:?}")
            };
            assert!(
                cause.is::<harness::responses::ProviderBlock>(),
                "expected a provider block: {code:?}"
            );
            assert_eq!(cause.to_string(), kind, "{code:?}");
            assert_eq!(
                AttemptFailure::of(&error, &cancel),
                AttemptFailure::ProviderBlocked,
                "the run is held, not failed and replayed: {code:?}"
            );
            assert!(
                matches!(owner.events.last().map(|event| &event.kind),
                    Some(EventKind::ProviderBlocked { reason }) if *reason == kind),
                "the hold keeps its kind: {code:?}"
            );
            assert_eq!(count.load(Ordering::SeqCst), 1, "not sent again: {code:?}");
            assert!(owner.visible.is_empty());
        } else {
            assert_eq!(result.unwrap().text(), "Hi é");
            assert_eq!(count.load(Ordering::SeqCst), 2);
            assert_eq!(owner.visible, "Hi é");
        }
        assert_eq!(owner.tools, 0);
    }
}

#[tokio::test]
async fn completed_paid_stream_survives_transport_close_or_idle() {
    for (hold, trailing) in [
        (false, b"data: {".as_slice()),
        (true, b"data: {".as_slice()),
        (false, b"data: \xff\n\n".as_slice()),
    ] {
        let mut response = bytes(&events(false));
        // The transport stalls partway through a later frame, after the paid answer.
        response.extend_from_slice(trailing);
        let (endpoint, count, task) = server(vec![response], hold).await;
        let mut provider = ResponsesProvider::new(
            endpoint,
            "synthetic".into(),
            std::sync::Arc::new(quota::Gate::default()),
        )
        .unwrap();
        provider.idle = TEST_IDLE;
        let mut owner = Owner::default();
        let turn = recover::work(request().freeze().unwrap(), "attempt", &[], 0)
            .unwrap()
            .run(
                &mut provider,
                &mut owner,
                limits(),
                &Cancellation::default(),
            )
            .await
            .unwrap();
        task.abort();
        assert_eq!(turn.text(), "Hi é");
        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert_eq!(
            owner
                .events
                .iter()
                .filter(|event| matches!(event.kind, EventKind::ProviderReturned(_)))
                .count(),
            1
        );
    }
}
#[tokio::test]
async fn failed_ack_and_cancellation_stop_stream_without_effects() {
    for mode in 0..2 {
        let cancel = Cancellation::default();
        let mut owner = Owner {
            reject: mode == 0,
            cancel: (mode == 1).then(|| cancel.clone()),
            ..Owner::default()
        };
        let (result, count) = attempt(
            vec![bytes(&events(true))],
            &mut owner,
            100_000,
            true,
            &cancel,
        )
        .await;
        let error = result.unwrap_err();
        assert_eq!(count, 1);
        assert_eq!(owner.tools, 0);
        assert!(
            matches!(error, Failure::Host(_) | Failure::Cancelled),
            "{error}"
        );
        if mode != 1 {
            assert!(owner.visible.is_empty());
        } else {
            assert_eq!(
                owner.visible, "Hi ",
                "cancellation fences the next ready delta"
            );
        }
    }
}
#[tokio::test]
async fn unfinished_response_runs_no_tool() {
    let mut cases = Vec::new();
    let mut incomplete = events(false);
    incomplete.last_mut().unwrap()["type"] = json!("response.incomplete");
    cases.push(bytes(&incomplete));
    let mut failed = events(false);
    failed.last_mut().unwrap()["type"] = json!("response.failed");
    cases.push(bytes(&failed));
    let mut error = events(false);
    error.last_mut().unwrap()["type"] = json!("error");
    cases.push(bytes(&error));
    let mut missing = events(false);
    missing.pop();
    cases.push(bytes(&missing));
    for reply in cases {
        let mut owner = Owner::default();
        let (result, count) = attempt(
            vec![reply],
            &mut owner,
            100_000,
            false,
            &Cancellation::default(),
        )
        .await;
        assert!(result.is_err());
        assert_eq!(count, 1);
        assert_eq!(owner.tools, 0);
    }
}
#[tokio::test]
async fn oversized_stream_fails_without_retry() {
    let mut owner = Owner::default();
    let (result, count) = attempt(
        vec![bytes(&events(false))],
        &mut owner,
        64,
        false,
        &Cancellation::default(),
    )
    .await;
    assert!(result.is_err());
    assert_eq!(count, 1);
}
