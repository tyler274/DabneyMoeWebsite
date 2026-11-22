//! Responses transport for accepted work. The product
//! supplies the model per request and retains observations through its owner.

use crate::{
    engine::{Cause, RunLimits, recipe::*},
    responses,
};
use responses::Authorization;
use serde_json::Value;

use std::{sync::Arc, time::Duration};

/// Same five-minute silence allowance for API and plan transport, per the harness spec.
const STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(300);
/// The longest provider retry hint the harness waits for. OpenClaw and Pi
/// stop retrying the same request at the same delay.
const MAX_RETRY_HINT: Duration = Duration::from_secs(60);
/// Every request asks the provider for a summary of the model's reasoning,
/// returned in each reasoning item. With `auto` the provider picks how detailed
/// the summary is for the model.
const REASONING_SUMMARY: &str = "auto";

/// The shared provider deadline is further away than the harness waits.
#[derive(Debug, thiserror::Error)]
#[error(
    "the provider asks to wait {} s, longer than the {} s the harness waits",
    .0.as_secs(),
    MAX_RETRY_HINT.as_secs()
)]
struct WaitTooLong(Duration);

/// One quota deadline shared by all replicas that use the same product quota.
#[async_trait::async_trait]
pub trait QuotaGate: Send + Sync {
    /// Time until a provider request may start.
    async fn remaining(&self) -> Result<Duration, Cause>;
    /// Extend the shared deadline, without shortening any existing wait.
    async fn defer(&self, delay: Duration) -> Result<(), Cause>;
}

/// Explicitly configured Responses transport for structured execution.
pub struct ResponsesProvider {
    client: responses::Client,
    quota: Arc<dyn QuotaGate>,
    idle: Duration,
}

impl ResponsesProvider {
    /// Constructs the client without contacting the endpoint.
    pub fn new(endpoint: String, key: String, quota: Arc<dyn QuotaGate>) -> Result<Self, String> {
        Ok(Self {
            client: responses::Client::new(key, &endpoint, Authorization::Api)?,
            quota,
            idle: STREAM_IDLE_TIMEOUT,
        })
    }

    /// A local developer's scoped plan token, only sent to the public Responses endpoint.
    pub fn chatgpt_plan(access_token: String, quota: Arc<dyn QuotaGate>) -> Result<Self, String> {
        Ok(Self {
            client: responses::Client::new(
                access_token,
                responses::ENDPOINT,
                Authorization::ChatGptPlan,
            )?,
            quota,
            idle: STREAM_IDLE_TIMEOUT,
        })
    }
}

fn encode_for(request: &Request, authorization: Authorization) -> Result<Value, String> {
    let mut input: Vec<Value> = request
        .items
        .iter()
        .map(responses::input_item)
        .collect::<Result<_, _>>()?;
    let tools = match authorization {
        Authorization::Api => request.tools.clone(),
        Authorization::ChatGptPlan => {
            if !request.tools.is_empty() {
                input.insert(0, serde_json::json!({"type":"additional_tools", "role":"developer", "tools":request.tools}));
            }
            Vec::new()
        }
    };
    let mut body = responses::body(
        &request.model,
        &request.instructions,
        input,
        tools,
        request.cache_key.as_ref(),
    );
    let mut reasoning = match request.options.clone() {
        Value::Null => serde_json::Map::new(),
        Value::Object(options) => options,
        _ => return Err("reasoning options must be a JSON object".into()),
    };
    reasoning.insert("summary".into(), REASONING_SUMMARY.into());
    body["reasoning"] = Value::Object(reasoning);
    Ok(body)
}

impl Provider for ResponsesProvider {
    async fn ready(&mut self) -> Result<(), Cause> {
        loop {
            let wait = self.quota.remaining().await?;
            if wait.is_zero() {
                return Ok(());
            }
            // A longer wait ends the attempt; the gate keeps the deadline.
            if wait > MAX_RETRY_HINT {
                return Err(Box::new(WaitTooLong(wait)));
            }
            tokio::time::sleep(wait).await;
        }
    }

    async fn exchange(
        &mut self,
        request: &Request,
        cancel: &Cancellation,
        limits: &RunLimits,
        observer: &mut dyn Observer,
    ) -> Result<Turn, Cause> {
        cancel.check()?;
        let body = encode_for(request, self.client.authorization)?;
        let exchange = self.client.stream(
            &body,
            request.cache_key.as_ref(),
            limits.max_response_bytes,
            self.idle,
            observer,
        );
        let result = tokio::select! {
            value = exchange => value,
            () = cancel.cancelled() => return Err(crate::engine::Failure::Cancelled.into()),
        };
        let result = result.map_err(|cause| match cause.downcast::<crate::engine::Failure>() {
            Ok(failure) => match *failure {
                crate::engine::Failure::Provider(cause) => cause,
                other @ (crate::engine::Failure::Cancelled
                | crate::engine::Failure::Limit(_)
                | crate::engine::Failure::ContextRefused
                | crate::engine::Failure::Final(_)
                | crate::engine::Failure::Protocol(_)
                | crate::engine::Failure::AfterOutput(_)
                | crate::engine::Failure::NotCurrent(_)
                | crate::engine::Failure::Host(_)
                | crate::engine::Failure::Encoding(_)) => Box::new(other),
            },
            Err(other) => other,
        });
        let error = match result {
            Ok(turn) => return Ok(turn),
            Err(error) => error,
        };
        if let Some(delay) = error
            .downcast_ref::<responses::Transient>()
            .and_then(|transient| transient.quota)
        {
            self.quota.defer(delay).await?;
        }
        Err(error)
    }
}

#[cfg(test)]
mod tests {
    struct Ready;
    #[async_trait::async_trait]
    impl super::QuotaGate for Ready {
        async fn remaining(&self) -> Result<std::time::Duration, crate::engine::Cause> {
            Ok(std::time::Duration::ZERO)
        }
        async fn defer(&self, _: std::time::Duration) -> Result<(), crate::engine::Cause> {
            panic!("this protocol fixture never returns a quota refusal")
        }
    }

    use super::*;
    use crate::engine::{Invocation, Item};
    use serde_json::json;

    #[tokio::test]
    async fn plan_stream_admits_only_finished_items_after_response_completion() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        for finished in [true, false] {
            let key = crate::PromptCacheKey::scoped("chords", &["scope-a"]);
            let session = key.wire();
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}/responses", listener.local_addr().unwrap());
            let server = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut incoming = Vec::new();
                let headers = loop {
                    let mut chunk = [0; 4096];
                    let count = socket.read(&mut chunk).await.unwrap();
                    assert!(count > 0);
                    incoming.extend_from_slice(&chunk[..count]);
                    if let Some(end) = incoming.windows(4).position(|part| part == b"\r\n\r\n") {
                        break String::from_utf8(incoming[..end].to_vec()).unwrap();
                    }
                };
                assert!(headers.starts_with("POST /responses "));
                assert_eq!(
                    headers.lines().find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("session-id")
                            .then(|| value.trim())
                    }),
                    Some(session.as_str()),
                    "session-id header carries the saved scope key"
                );
                let call = json!({"id":"f","type":"function_call","call_id":"read-1","name":"skill_view","arguments":"{}","status":"completed"});
                let mut events = vec![
                    json!({"type":"response.created","response":{"id":"r"}}),
                    json!({"type":"response.output_item.added","output_index":0,"item":{"id":"f","type":"function_call","call_id":"read-1","name":"skill_view"}}),
                    json!({"type":"response.function_call_arguments.delta","output_index":0,"item_id":"f","delta":"{}"}),
                    json!({"type":"response.metadata","response_id":"r"}),
                    json!({"type":"response.function_call_arguments.done","output_index":0,"item_id":"f","arguments":"{}"}),
                ];
                if finished {
                    events.push(
                        json!({"type":"response.output_item.done","output_index":0,"item":call}),
                    );
                }
                events.push(json!({"type":"response.completed","response":{"id":"r","status":"completed","output":[]}}));
                let mut wire = String::from("HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n");
                for (index, mut event) in events.into_iter().enumerate() {
                    event["sequence_number"] = json!(index);
                    wire.push_str(&format!("data: {event}\n\n"));
                }
                socket.write_all(wire.as_bytes()).await.unwrap();
            });
            let mut provider = ResponsesProvider {
                client: responses::Client::new(
                    "local-proof".into(),
                    &url,
                    Authorization::ChatGptPlan,
                )
                .unwrap(),
                quota: Arc::new(Ready),
                idle: Duration::from_secs(120),
            };
            let request = Request {
                cache_key: Some(key),
                model: "test".into(),
                context_window_tokens: 100_000,
                max_input_tokens: 100_000,
                instructions: "Chords".into(),
                options: json!({}),
                tools: vec![],
                items: vec![],
            };
            let result = provider
                .exchange(
                    &request,
                    &Cancellation::default(),
                    &RunLimits::default(),
                    &mut |_| Ok(()),
                )
                .await;
            server.await.unwrap();
            if finished {
                let response = result.unwrap();
                assert_eq!(response.receipt["mode"], "subscription");
                let call = response.calls().next().unwrap();
                assert_eq!(
                    (&*call.id, &*call.name, &*call.arguments),
                    ("read-1", "skill_view", "{}")
                );
            } else {
                assert!(
                    result
                        .unwrap_err()
                        .to_string()
                        .contains("unfinished output")
                );
            }
        }
    }

    #[test]
    fn plan_requests_declare_local_tools_and_continue_the_exchange_by_id() {
        let function =
            json!({"type":"function","name":"skill_view","parameters":{"type":"object"}});
        let request = Request {
            cache_key: Some(crate::PromptCacheKey::scoped("chords", &["scope-a"])),
            model: "test".into(),
            context_window_tokens: 100_000,
            max_input_tokens: 100_000,
            instructions: "Chords instructions".into(),
            options: json!({"effort":"high"}),
            tools: vec![function.clone()],
            items: vec![
                Item::Call(Invocation {
                    id: "read-1".into(),
                    name: "skill_view".into(),
                    arguments: "{}".into(),
                }),
                Item::MaterializedResult {
                    call_id: "read-1".into(),
                    output: "packaged guidance".into(),
                },
            ],
        };
        let plan = encode_for(&request, Authorization::ChatGptPlan).unwrap();
        assert_eq!(plan["instructions"], "Chords instructions");
        assert_eq!(
            plan["input"],
            json!([
            {"type":"additional_tools","role":"developer","tools":[function]},
            {"type":"function_call","call_id":"read-1","name":"skill_view","arguments":"{}"},
            {"type":"function_call_output","call_id":"read-1","output":"packaged guidance"}])
        );
        assert_eq!(plan["tools"], json!([]));
        assert_eq!(plan["stream"], true);
        assert_eq!(plan["store"], false);
        assert_eq!(plan["reasoning"], json!({"effort":"high","summary":"auto"}));
        let unset = Request {
            options: Value::Null,
            ..request.clone()
        };
        assert_eq!(
            encode_for(&unset, Authorization::Api).unwrap()["reasoning"],
            json!({"summary":"auto"})
        );
        let api = encode_for(&request, Authorization::Api).unwrap();
        let key = plan["prompt_cache_key"]
            .as_str()
            .expect("plan cache partition");
        assert_eq!(key.len(), 64);
        assert_eq!(api["prompt_cache_key"], key);
        assert_eq!(api["tools"], json!([function]));
    }
}

#[cfg(test)]
#[path = "../tests/support/quota.rs"]
mod test_quota;

#[cfg(test)]
#[path = "../tests/support/recover.rs"]
mod test_recover;

#[cfg(test)]
#[path = "provider/tests/recovery.rs"]
mod recovery_tests;

#[cfg(test)]
#[path = "provider/tests/streaming.rs"]
mod streaming_tests;
