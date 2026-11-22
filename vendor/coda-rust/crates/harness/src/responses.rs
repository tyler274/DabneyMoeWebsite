//! API-backed OpenAI Responses adapter: bounded SSE output,
//! `store: false`, bounded transient failures and no secret logging. The provider awaits each streamed observation acknowledgment.

mod sse;

use crate::engine::Cause;
use serde_json::{Value, json};
use std::time::{Duration, SystemTime};

use crate::engine::{
    Failure, Invocation, Item, Turn,
    recipe::{Observer, StreamEvent},
};

/// Production Responses API endpoint.
pub const ENDPOINT: &str = "https://api.openai.com/v1/responses";

#[derive(Clone, Copy, Default, serde::Serialize)]
pub(crate) enum Authorization {
    #[default]
    #[serde(rename = "api")]
    Api,
    #[serde(rename = "subscription")]
    ChatGptPlan,
}

pub(crate) struct Client {
    pub(crate) authorization: Authorization,
    client: reqwest::Client,
    endpoint: reqwest::Url,
    key: String,
}

impl Client {
    pub(crate) fn new(
        key: String,
        endpoint: &str,
        authorization: Authorization,
    ) -> Result<Self, String> {
        let endpoint = reqwest::Url::parse(endpoint).map_err(|_| "invalid provider endpoint")?;
        // Every request carries the key as a bearer token, so it goes only over
        // HTTPS, where the network cannot read it. Plain HTTP is allowed only to
        // this machine, such as a local test server.
        let loopback = matches!(
            endpoint.host_str(),
            Some("127.0.0.1" | "localhost" | "[::1]")
        );
        if (endpoint.scheme() != "https" && !(endpoint.scheme() == "http" && loopback))
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.fragment().is_some()
            || endpoint.query().is_some()
            || key.trim().is_empty()
        {
            return Err("provider requires credentials and trusted HTTPS endpoint".into());
        }
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .build()
            .map_err(|_| "cannot construct provider client")?;
        Ok(Self {
            authorization,
            client,
            endpoint,
            key,
        })
    }

    async fn send(
        &self,
        body: &Value,
        cache_key: Option<&crate::PromptCacheKey>,
        accept: &'static str,
        cap: usize,
        idle: Duration,
    ) -> Result<reqwest::Response, Cause> {
        let mut request = self
            .client
            .post(self.endpoint.clone())
            .header(reqwest::header::ACCEPT, accept)
            .bearer_auth(&self.key)
            .json(body);
        if let (Authorization::ChatGptPlan, Some(key)) = (self.authorization, cache_key) {
            request = request.header("session-id", key.wire());
        }
        let response = tokio::time::timeout(idle, request.send())
            .await
            .map_err(Transient::transport)?
            .map_err(|error| Transient::transport(error.without_url()))?;
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }
        let error = response
            .error_for_status_ref()
            .err()
            .map(|error| Box::new(error.without_url()) as Cause)
            .unwrap_or_else(|| format!("provider returned HTTP {status}").into());
        let hint = retry_after(
            response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok()),
            SystemTime::now(),
        );
        // Read only the bounded error envelope. Provider prose and credentials
        // never become an error message; the code distinguishes billing from quota.
        // The error text is saved in the run record as the event's reason, and the
        // body's own text comes from outside the harness and can hold anything.
        // Another try can fix a request timeout (408), a lock timeout (409), a
        // rate limit (429) and a server error, so the provider's own SDK retries
        // those. A terminal code in the body still holds the run.
        let transient = matches!(
            status,
            reqwest::StatusCode::REQUEST_TIMEOUT
                | reqwest::StatusCode::CONFLICT
                | reqwest::StatusCode::TOO_MANY_REQUESTS
        ) || status.is_server_error();
        let quota = (status == reqwest::StatusCode::TOO_MANY_REQUESTS || hint.is_some())
            .then_some(hint.unwrap_or_default());
        let bytes = match response_bytes(response, cap, idle).await {
            Ok(bytes) => bytes,
            // A body disconnect cannot turn a known permanent HTTP rejection
            // into a transport retry, or discard a known quota retry hint.
            Err(read) if read.is::<Transient>() => {
                return Err(classify_error(None, None, error, quota, transient));
            }
            Err(read) => return Err(read),
        };
        let envelope: Option<Value> = serde_json::from_slice(&bytes).ok();
        let code = envelope
            .as_ref()
            .and_then(|body| body.pointer("/error/code"))
            .and_then(Value::as_str);
        let error_type = envelope
            .as_ref()
            .and_then(|body| body.pointer("/error/type"))
            .and_then(Value::as_str);
        Err(classify_error(code, error_type, error, quota, transient))
    }
    pub(crate) async fn stream(
        &self,
        body: &Value,
        cache_key: Option<&crate::PromptCacheKey>,
        cap: usize,
        idle: Duration,
        sink: &mut dyn Observer,
    ) -> Result<Turn, Cause> {
        let mut response = self
            .send(body, cache_key, "text/event-stream", cap, idle)
            .await?;
        let mut decoder = sse::Decoder::new(cap);
        let mut stream = sse::Stream::new(self.authorization);
        let mut next_event = tokio::time::Instant::now() + idle;
        loop {
            let chunk = match tokio::time::timeout_at(next_event, response.chunk())
                .await
                .map_err(Transient::transport)
                .and_then(|chunk| chunk.map_err(|error| Transient::transport(error.without_url())))
            {
                Ok(Some(chunk)) => chunk,
                Ok(None) => break,
                Err(error) => return Err(error.into()),
            };
            // Stop decoding at the accepted terminal frame, even when later
            // bytes share its transport chunk. Completion owns this boundary.
            for line in chunk.split_inclusive(|byte| matches!(byte, b'\n' | b'\r')) {
                for frame in decoder.push(line)? {
                    stream.accept(frame, sink).await?;
                    if stream.has_terminal() {
                        return Ok(stream.finish()?);
                    }
                    next_event = tokio::time::Instant::now() + idle;
                }
            }
        }
        Err(Transient::transport("stream ended without completed response").into())
    }
}

fn classify_error(
    code: Option<&str>,
    error_type: Option<&str>,
    source: Cause,
    quota: Option<Duration>,
    transient: bool,
) -> Cause {
    if code == Some("context_length_exceeded") {
        return Box::new(ContextLengthExceeded { source });
    }
    if code == Some("content_filter") {
        return Box::new(ProviderBlock {
            source,
            kind: BlockKind::Filtered,
        });
    }
    // Codex stops on the same usage and quota causes, by HTTP error type or
    // by stream code, and reports them as one usage-limit class.
    let billing_cause = error_type
        .filter(|cause| {
            matches!(
                cause,
                &"usage_limit_reached" | &"usage_not_included" | &"insufficient_quota"
            )
        })
        .or_else(|| {
            code.filter(|cause| {
                matches!(
                    cause,
                    &"insufficient_quota"
                        | &"usage_not_included"
                        | &"credit_balance_exhausted"
                        | &"organization_spend_limit_exceeded"
                        | &"project_spend_limit_exceeded"
                        | &"organization_usage_limit_exceeded"
                )
            })
        });
    if let Some(cause) = billing_cause {
        return Box::new(ProviderBlock {
            source,
            kind: BlockKind::Billing(cause.into()),
        });
    }
    let terminal = matches!(
        code,
        Some(
            "model_not_found"
                | "budget_reached"
                | "flex_unavailable"
                | "cyber_policy"
                | "bio_policy"
                | "misalignment_policy_violation"
                | "invalid_prompt"
        )
    );
    if transient && !terminal {
        return Box::new(Transient { source, quota });
    }
    // None of the references sends a request the provider rejected again.
    Box::new(ProviderBlock {
        source,
        kind: BlockKind::NotAccepted,
    })
}

/// A known context rejection asks the loop to open a fresh window.
#[derive(Debug, thiserror::Error)]
#[error("provider context window is full")]
struct ContextLengthExceeded {
    source: Cause,
}

pub(crate) fn context_rejected(error: &(dyn std::error::Error + 'static)) -> bool {
    let mut cause = Some(error);
    while let Some(error) = cause {
        if error.is::<ContextLengthExceeded>() {
            return true;
        }
        cause = error.source();
    }
    false
}

/// What kind of provider outcome another try can't fix.
#[derive(Debug)]
enum BlockKind {
    /// The account's credits or usage are exhausted, with the provider's code.
    Billing(String),
    /// The provider filtered the reply.
    Filtered,
    /// The provider rejected the request for another reason.
    NotAccepted,
}

/// A provider outcome that another try can't fix. The run is held as blocked
/// by the provider and the same request is not sent again. The text states
/// the kind in fixed words; the provider's own message never enters it.
#[derive(Debug, thiserror::Error)]
pub(crate) struct ProviderBlock {
    source: Cause,
    kind: BlockKind,
}

/// Find the provider block through retained product and engine causes.
pub(crate) fn provider_blocked(error: &(dyn std::error::Error + 'static)) -> bool {
    let mut cause = Some(error);
    while let Some(error) = cause {
        if error.is::<ProviderBlock>() {
            return true;
        }
        cause = error.source();
    }
    false
}

impl std::fmt::Display for ProviderBlock {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.kind {
            BlockKind::Billing(cause) => write!(
                formatter,
                "Provider credits are exhausted. Restore billing, then retry this work ({cause})."
            ),
            BlockKind::Filtered => formatter.write_str(
                "The model provider filtered this reply. Try again, or change the request.",
            ),
            BlockKind::NotAccepted => formatter
                .write_str("The model provider did not accept this request. Try again later."),
        }
    }
}

fn retry_after(header: Option<&str>, now: SystemTime) -> Option<Duration> {
    let value = header?.trim();
    value
        .parse::<u64>()
        .ok()
        .map(Duration::from_secs)
        .or_else(|| {
            httpdate::parse_http_date(value)
                .ok()
                .map(|date| date.duration_since(now).unwrap_or_default())
        })
}

async fn response_bytes(
    mut response: reqwest::Response,
    cap: usize,
    idle: Duration,
) -> Result<Vec<u8>, Cause> {
    let mut bytes = Vec::new();
    while let Some(chunk) = next_chunk(&mut response, idle).await? {
        if chunk.len() > cap.saturating_sub(bytes.len()) {
            return Err(Failure::Protocol("provider response byte limit".into()).into());
        }
        bytes.extend_from_slice(chunk.as_ref());
    }
    Ok(bytes)
}

#[derive(Debug, thiserror::Error)]
#[error("{source}")]
pub(crate) struct Transient {
    source: Cause,
    pub(crate) quota: Option<Duration>,
}
impl Transient {
    pub(crate) fn transport(error: impl Into<Cause>) -> Self {
        Self {
            source: error.into(),
            quota: None,
        }
    }
}
async fn next_chunk(
    response: &mut reqwest::Response,
    idle: Duration,
) -> Result<Option<impl AsRef<[u8]> + std::ops::Deref<Target = [u8]>>, Cause> {
    let chunk = tokio::time::timeout(idle, response.chunk())
        .await
        .map_err(Transient::transport)?;
    Ok(chunk.map_err(|error| Transient::transport(error.without_url()))?)
}

pub(crate) fn body(
    model: &str,
    instructions: &str,
    input: Vec<Value>,
    tools: Vec<Value>,
    key: Option<&crate::PromptCacheKey>,
) -> Value {
    let mut body = json!({"model":model,"instructions":instructions,"input":input,"tools":tools,"stream":true,
        "store":false,"include":["reasoning.encrypted_content"]});
    if let Some(key) = key {
        body["prompt_cache_key"] = json!(key.wire());
    }
    body
}

pub(crate) fn input_item(item: &Item) -> Result<Value, String> {
    Ok(match item {
        Item::OutputCutoff => return Err("incomplete turn cannot enter dispatched context".into()),
        Item::Message { role, text } => json!({"role": role, "content": text}),
        Item::Feedback { text } => json!({"role": "developer", "content": text}),
        Item::FinishRefused(refusal) => {
            json!({"role": "developer", "content": refusal.feedback().to_string()})
        }
        Item::ProductMessage(message) => {
            json!({"role": message.role.as_str(), "content": message.text})
        }
        Item::Call(call) => json!({"type": "function_call", "call_id": call.id,
                "name": call.name, "arguments": call.arguments}),
        Item::MaterializedResult { call_id, output } => {
            json!({"type": "function_call_output", "call_id": call_id, "output": output})
        }
        Item::Result { .. } => return Err("unmaterialized result reached provider encoding".into()),
        Item::Opaque(value) => {
            // Opaque items cannot smuggle calls, results or instruction roles.
            if value.get("type").and_then(Value::as_str) != Some("reasoning") {
                return Err("unsupported opaque Responses continuation item".to_owned());
            }
            value.clone()
        }
    })
}

pub(crate) fn observation(response: &Value, authorization: Authorization) -> StreamEvent {
    StreamEvent::Receipt {
        identity: receipt(response, authorization),
        usage: usage(response),
    }
}

pub(crate) fn usage(response: &Value) -> crate::engine::recipe::Usage {
    crate::engine::recipe::Usage {
        cached_input_tokens: response
            .pointer("/usage/input_tokens_details/cached_tokens")
            .and_then(Value::as_u64),
        input_tokens: response
            .pointer("/usage/input_tokens")
            .and_then(Value::as_u64),
        output_tokens: response
            .pointer("/usage/output_tokens")
            .and_then(Value::as_u64),
    }
}

pub(crate) fn receipt(response: &Value, authorization: Authorization) -> Value {
    json!({"adapter":"openai-responses", "mode":authorization, "id":response.get("id"), "model":response.get("model"), "service_tier":response.get("service_tier"), "usage":response.get("usage"), "status":response.get("status")})
}

fn cut_off(response: &Value, authorization: Authorization) -> Option<Turn> {
    if response["status"] != "incomplete"
        || response["incomplete_details"]["reason"] != "max_output_tokens"
    {
        return None;
    }
    let mut evidence = receipt(response, authorization);
    evidence["incomplete_output"] = response["output"].clone();
    evidence["incomplete_details"] = response["incomplete_details"].clone();
    Some(Turn {
        items: vec![Item::OutputCutoff],
        receipt: evidence,
        usage: usage(response),
    })
}

fn decode_items<'a>(
    response: &Value,
    authorization: Authorization,
    output: impl IntoIterator<Item = &'a Value>,
) -> Result<Turn, String> {
    if response.get("status").and_then(Value::as_str) != Some("completed") {
        return Err("provider response did not complete; output is not executable".into());
    }
    let mut items = Vec::new();
    for item in output {
        match item.get("type").and_then(Value::as_str) {
            Some("function_call") => {
                if item
                    .get("status")
                    .and_then(Value::as_str)
                    .is_some_and(|s| s != "completed")
                {
                    return Err("incomplete provider tool call".into());
                }
                // A missing field reads as empty. The engine gives an empty ID a
                // new one and answers an empty name or arguments as a
                // correctable error.
                items.push(Item::Call(Invocation {
                    id: field(item, "call_id"),
                    name: field(item, "name"),
                    arguments: field(item, "arguments"),
                }));
            }
            Some("message") => {
                if item.get("role").and_then(Value::as_str) != Some("assistant")
                    || item.get("status").and_then(Value::as_str) != Some("completed")
                {
                    return Err("invalid provider message".into());
                }
                let parts = item
                    .get("content")
                    .and_then(Value::as_array)
                    .ok_or("missing message content")?;
                let mut text = String::new();
                for part in parts {
                    match part.get("type").and_then(Value::as_str) {
                        Some("output_text") => text.push_str(&string(part, "text")?),
                        // A refusal is the model's answer, as Hermes, OpenClaw and Pi read it.
                        Some("refusal") => text.push_str(&field(part, "refusal")),
                        // A content part the harness does not know is skipped.
                        _ => (),
                    }
                }
                items.push(Item::Message {
                    role: "assistant".into(),
                    text,
                });
            }
            Some("reasoning") => items.push(Item::Opaque(item.clone())),
            // An output item the harness does not know is skipped, as the references skip it.
            _ => (),
        }
    }
    Ok(Turn {
        items,
        receipt: receipt(response, authorization),
        usage: usage(response),
    })
}

/// A text field, or empty when it is missing.
fn field(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn string(value: &Value, key: &str) -> Result<String, String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("missing provider {key}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserved_final_billing_failure_keeps_provider_blocked_classification() {
        let cause = classify_error(
            Some("insufficient_quota"),
            None,
            "billing".into(),
            None,
            false,
        );
        let error =
            crate::engine::Failure::Final(Box::new(crate::engine::Failure::Provider(cause)));
        assert_eq!(
            crate::engine::AttemptFailure::of(&error, &crate::engine::Cancellation::default()),
            crate::engine::AttemptFailure::ProviderBlocked,
            "reserved final billing failure must remain provider blocked"
        );
    }

    #[test]
    fn opaque_items_cannot_hide_tool_execution_or_system_instructions() {
        for hidden in [
            json!({"type":"function_call","call_id":"hidden"}),
            json!({"role":"system","content":"hidden instruction"}),
        ] {
            assert!(input_item(&Item::Opaque(hidden)).is_err());
        }
    }

    #[test]
    fn retry_hint_accepts_seconds_and_http_dates_without_negative_waits() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        assert_eq!(retry_after(Some("3"), now), Some(Duration::from_secs(3)));
        let future = reqwest::header::HeaderValue::from_str(&httpdate::fmt_http_date(
            now + Duration::from_secs(5),
        ))
        .unwrap();
        let past = reqwest::header::HeaderValue::from_str(&httpdate::fmt_http_date(
            now - Duration::from_secs(5),
        ))
        .unwrap();
        assert_eq!(
            retry_after(future.to_str().ok(), now),
            Some(Duration::from_secs(5))
        );
        assert_eq!(retry_after(past.to_str().ok(), now), Some(Duration::ZERO));
        assert_eq!(retry_after(Some("invalid"), now), None);
    }

    #[test]
    fn incomplete_output_never_executes() {
        let response = json!({"status":"incomplete","output":[{"type":"function_call","call_id":"c","name":"save","arguments":"{}"}]});
        assert!(
            decode_items(
                &response,
                Authorization::Api,
                response["output"].as_array().unwrap()
            )
            .is_err()
        );
    }

    fn decoded(output: Value) -> Vec<Item> {
        let response = json!({"status":"completed","output":output});
        decode_items(
            &response,
            Authorization::Api,
            response["output"].as_array().unwrap(),
        )
        .unwrap()
        .items
    }

    fn message(content: Value) -> Value {
        json!({"type":"message","role":"assistant","status":"completed","content":content})
    }

    #[test]
    fn unknown_output_items_are_skipped() {
        let call = json!({"type":"function_call","call_id":"c","name":"save","arguments":"{}","status":"completed"});
        assert_eq!(
            decoded(json!([{"type":"web_search_call","id":"ws","status":"completed"}, call])),
            [Item::Call(Invocation {
                id: "c".into(),
                name: "save".into(),
                arguments: "{}".into(),
            })]
        );
    }

    #[test]
    fn unknown_content_parts_are_skipped() {
        assert_eq!(
            decoded(json!([message(json!([
                {"type":"output_text","text":"Before. "},
                {"type":"output_audio","data":"AAAA"},
                {"type":"output_text","text":"After."}
            ]))])),
            [Item::assistant("Before. After.")]
        );
    }

    #[test]
    fn a_refusal_is_the_models_answer_as_text() {
        assert_eq!(
            decoded(json!([message(
                json!([{"type":"refusal","refusal":"I can't help with that."}])
            )])),
            [Item::assistant("I can't help with that.")]
        );
    }

    #[test]
    fn a_call_with_missing_fields_reads_them_as_empty() {
        assert_eq!(
            decoded(json!([{"type":"function_call","status":"completed"}])),
            [Item::Call(Invocation {
                id: String::new(),
                name: String::new(),
                arguments: String::new(),
            })]
        );
    }

    #[test]
    fn endpoint_requires_tls_except_local_tests() {
        assert!(
            Client::new(
                "secret".into(),
                "http://example.com/v1/responses",
                Authorization::Api
            )
            .is_err()
        );
        assert!(Client::new(String::new(), ENDPOINT, Authorization::Api).is_err());
    }
}
