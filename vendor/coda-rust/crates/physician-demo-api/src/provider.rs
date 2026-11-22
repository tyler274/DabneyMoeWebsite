//! Dual-mode LLM provider configuration and cloud/local dispatch adapters.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use harness::engine::recipe::{Observer, Provider, Request, StreamEvent, Turn, Usage};
use harness::engine::{Cancellation, Cause, Failure, Invocation, Item, RunLimits};
use harness::provider::{QuotaGate, ResponsesProvider};
use serde_json::{Value, json};

use crate::candle_qwen::{CandleQwenConfig, CandleQwenProvider, CandleQwenRuntime};

const DEFAULT_OPENAI_ENDPOINT: &str = "https://api.openai.com/v1/responses";
const DEFAULT_OPENAI_MODEL: &str = "gpt-4o-mini";
const DEFAULT_GEMINI_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta";
const DEFAULT_GEMINI_MODEL: &str = "gemini-2.5-flash";
const DEFAULT_ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com/v1";
const DEFAULT_ANTHROPIC_MODEL: &str = "claude-3-5-haiku-latest";

pub(crate) async fn wait_cancelled(cancel: &Cancellation) {
    while !cancel.is_cancelled() {
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

fn cloud_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

struct NoQuotaGate;

#[async_trait]
impl QuotaGate for NoQuotaGate {
    async fn remaining(&self) -> Result<Duration, Cause> {
        Ok(Duration::ZERO)
    }

    async fn defer(&self, _delay: Duration) -> Result<(), Cause> {
        Ok(())
    }
}

/// Errors returned by HTTP cloud provider adapters (`Gemini` / `Anthropic`).
#[derive(Debug, thiserror::Error)]
pub enum CloudProviderError {
    /// Failed HTTP transport to upstream provider endpoint.
    #[error("cloud provider HTTP request failed")]
    Http(#[from] reqwest::Error),
    /// Upstream returned a non-success HTTP status code.
    #[error("{provider} returned HTTP {status}: {body}")]
    Status {
        /// Upstream provider label.
        provider: &'static str,
        /// HTTP status code.
        status: u16,
        /// Truncated response body.
        body: String,
    },
    /// Upstream JSON response lacked expected candidate or message fields.
    #[error("{provider} returned malformed response: {detail}")]
    Malformed {
        /// Upstream provider label.
        provider: &'static str,
        /// Failure detail.
        detail: String,
    },
}

/// Trait allowing test harnesses or callers to inject custom [`harness::engine::recipe::Provider`]s.
#[async_trait]
pub trait CustomProvider: Send {
    /// Executes one provider turn inside `harness::engine`.
    async fn exchange(
        &mut self,
        request: &Request,
        cancel: &Cancellation,
        limits: &RunLimits,
        observer: &mut dyn Observer,
    ) -> Result<Turn, Cause>;
}

/// Factory creating fresh boxed [`CustomProvider`] instances per harness run.
pub trait CustomProviderFactory: Send + Sync {
    /// Name shown in recipe telemetry/diagnostics.
    fn model_name(&self) -> &str;
    /// Instantiates a fresh provider handle for one run.
    fn create(&self) -> Box<dyn CustomProvider>;
}

/// Selected provider backend resolved from environment or caller configuration.
#[derive(Clone)]
pub enum ProviderBackend {
    /// OpenAI Responses API via `harness::provider::ResponsesProvider`.
    OpenAi {
        /// Full Responses endpoint URL.
        endpoint: String,
        /// Bearer API key.
        api_key: String,
        /// Model name bound in request recipes.
        model: String,
    },
    /// Google Gemini `generateContent` REST adapter over `reqwest`.
    Gemini {
        /// Base Gemini API URL.
        base_url: String,
        /// Google AI API key.
        api_key: String,
        /// Model name bound in request recipes.
        model: String,
    },
    /// Anthropic Messages API REST adapter over `reqwest`.
    Anthropic {
        /// Base Anthropic API URL.
        base_url: String,
        /// Anthropic `x-api-key`.
        api_key: String,
        /// Model name bound in request recipes.
        model: String,
    },
    /// Pure-Rust quantized Qwen3 / Qwen2 GGUF inference via `candle-transformers`.
    CandleQwen(Arc<CandleQwenRuntime>),
    /// Caller-supplied provider factory (e.g. deterministic test provider).
    Custom(Arc<dyn CustomProviderFactory>),
}

impl std::fmt::Debug for ProviderBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OpenAi {
                endpoint, model, ..
            } => f
                .debug_struct("OpenAi")
                .field("endpoint", endpoint)
                .field("model", model)
                .finish_non_exhaustive(),
            Self::Gemini {
                base_url, model, ..
            } => f
                .debug_struct("Gemini")
                .field("base_url", base_url)
                .field("model", model)
                .finish_non_exhaustive(),
            Self::Anthropic {
                base_url, model, ..
            } => f
                .debug_struct("Anthropic")
                .field("base_url", base_url)
                .field("model", model)
                .finish_non_exhaustive(),
            Self::CandleQwen(rt) => f.debug_tuple("CandleQwen").field(rt).finish(),
            Self::Custom(factory) => f
                .debug_tuple("Custom")
                .field(&factory.model_name())
                .finish(),
        }
    }
}

impl ProviderBackend {
    /// Resolves the active provider mode from `get`:
    /// `OPENAI_API_KEY` -> `GEMINI_API_KEY` -> `ANTHROPIC_API_KEY` -> pure-Rust [`CandleQwenRuntime`].
    pub fn from_env(get: impl Fn(&str) -> Option<String>) -> Self {
        let non_empty = |key: &str| {
            get(key)
                .map(|v| v.trim().to_owned())
                .filter(|v| !v.is_empty())
        };
        if let Some(api_key) = non_empty("OPENAI_API_KEY") {
            let endpoint =
                non_empty("OPENAI_BASE_URL").unwrap_or_else(|| DEFAULT_OPENAI_ENDPOINT.to_owned());
            let model = non_empty("CODA_DEMO_OPENAI_MODEL")
                .unwrap_or_else(|| DEFAULT_OPENAI_MODEL.to_owned());
            return Self::OpenAi {
                endpoint,
                api_key,
                model,
            };
        }
        if let Some(api_key) = non_empty("GEMINI_API_KEY") {
            let base_url =
                non_empty("GEMINI_BASE_URL").unwrap_or_else(|| DEFAULT_GEMINI_BASE_URL.to_owned());
            let model = non_empty("CODA_DEMO_GEMINI_MODEL")
                .unwrap_or_else(|| DEFAULT_GEMINI_MODEL.to_owned());
            return Self::Gemini {
                base_url,
                api_key,
                model,
            };
        }
        if let Some(api_key) = non_empty("ANTHROPIC_API_KEY") {
            let base_url = non_empty("ANTHROPIC_BASE_URL")
                .unwrap_or_else(|| DEFAULT_ANTHROPIC_BASE_URL.to_owned());
            let model = non_empty("CODA_DEMO_ANTHROPIC_MODEL")
                .unwrap_or_else(|| DEFAULT_ANTHROPIC_MODEL.to_owned());
            return Self::Anthropic {
                base_url,
                api_key,
                model,
            };
        }
        Self::CandleQwen(Arc::new(CandleQwenRuntime::new(
            CandleQwenConfig::from_env(get),
        )))
    }

    /// Short provider mode identifier exposed in `/health`.
    pub fn mode_label(&self) -> &'static str {
        match self {
            Self::OpenAi { .. } => "openai",
            Self::Gemini { .. } => "gemini",
            Self::Anthropic { .. } => "anthropic",
            Self::CandleQwen(_) => "candle_qwen3",
            Self::Custom(_) => "custom",
        }
    }

    /// Model identifier bound into [`harness::engine::recipe::RequestRecipe`].
    pub fn model_name(&self) -> &str {
        match self {
            Self::OpenAi { model, .. }
            | Self::Gemini { model, .. }
            | Self::Anthropic { model, .. } => model,
            Self::CandleQwen(rt) => rt.model_label(),
            Self::Custom(factory) => factory.model_name(),
        }
    }

    pub(crate) fn instantiate(&self) -> Result<DemoProvider, String> {
        Ok(match self {
            Self::OpenAi {
                endpoint, api_key, ..
            } => DemoProvider::OpenAi(ResponsesProvider::new(
                endpoint.clone(),
                api_key.clone(),
                Arc::new(NoQuotaGate),
            )?),
            Self::Gemini {
                base_url,
                api_key,
                model,
            } => DemoProvider::Gemini(GeminiChatProvider {
                client: cloud_http_client(),
                base_url: base_url.clone(),
                api_key: api_key.clone(),
                model: model.clone(),
            }),
            Self::Anthropic {
                base_url,
                api_key,
                model: _,
            } => DemoProvider::Anthropic(AnthropicMessagesProvider {
                client: cloud_http_client(),
                base_url: base_url.clone(),
                api_key: api_key.clone(),
            }),
            Self::CandleQwen(rt) => DemoProvider::CandleQwen(rt.provider()),
            Self::Custom(factory) => DemoProvider::Custom(factory.create()),
        })
    }
}

pub(crate) enum DemoProvider {
    OpenAi(ResponsesProvider),
    Gemini(GeminiChatProvider),
    Anthropic(AnthropicMessagesProvider),
    CandleQwen(CandleQwenProvider),
    Custom(Box<dyn CustomProvider>),
}

impl Provider for DemoProvider {
    async fn ready(&mut self) -> Result<(), Cause> {
        match self {
            Self::OpenAi(p) => p.ready().await,
            Self::Gemini(_) | Self::Anthropic(_) | Self::CandleQwen(_) | Self::Custom(_) => Ok(()),
        }
    }

    async fn exchange(
        &mut self,
        request: &Request,
        cancel: &Cancellation,
        limits: &RunLimits,
        observer: &mut dyn Observer,
    ) -> Result<Turn, Cause> {
        match self {
            Self::OpenAi(p) => p.exchange(request, cancel, limits, observer).await,
            Self::Gemini(p) => p.exchange(request, cancel, limits, observer).await,
            Self::Anthropic(p) => p.exchange(request, cancel, limits, observer).await,
            Self::CandleQwen(p) => p.exchange(request, cancel, limits, observer).await,
            Self::Custom(p) => p.exchange(request, cancel, limits, observer).await,
        }
    }
}

pub(crate) struct GeminiChatProvider {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    model: String,
}

impl GeminiChatProvider {
    pub(crate) fn encode_request(request: &Request) -> Value {
        let mut call_names = std::collections::HashMap::<&str, &str>::new();
        let mut contents = Vec::new();
        for item in &request.items {
            match item {
                Item::Message { role, text } => {
                    let g_role = if role == "assistant" { "model" } else { "user" };
                    contents.push(json!({
                        "role": g_role,
                        "parts": [{ "text": text }]
                    }));
                }
                Item::Feedback { text } => {
                    contents.push(json!({
                        "role": "user",
                        "parts": [{ "text": text }]
                    }));
                }
                Item::FinishRefused(refusal) => {
                    contents.push(json!({
                        "role": "user",
                        "parts": [{ "text": refusal.reason() }]
                    }));
                }
                Item::Call(call) => {
                    call_names.insert(call.id.as_str(), call.name.as_str());
                    let args: Value =
                        serde_json::from_str(&call.arguments).unwrap_or_else(|_| json!({}));
                    contents.push(json!({
                        "role": "model",
                        "parts": [{ "functionCall": { "name": call.name, "args": args } }]
                    }));
                }
                Item::MaterializedResult { call_id, output } => {
                    let name = call_names
                        .get(call_id.as_str())
                        .copied()
                        .unwrap_or("tool_result");
                    let parsed = serde_json::from_str::<Value>(output)
                        .unwrap_or_else(|_| json!({ "output": output }));
                    contents.push(json!({
                        "role": "user",
                        "parts": [{
                            "functionResponse": {
                                "name": name,
                                "response": { "result": parsed }
                            }
                        }]
                    }));
                }
                Item::ProductMessage(_)
                | Item::OutputCutoff
                | Item::Result { .. }
                | Item::Opaque(_) => {}
            }
        }
        let declarations: Vec<Value> = request
            .tools
            .iter()
            .filter_map(|t| {
                let name = t.get("name").and_then(Value::as_str)?;
                let desc = t.get("description").and_then(Value::as_str).unwrap_or("");
                let params = t
                    .get("parameters")
                    .cloned()
                    .unwrap_or_else(|| json!({"type":"object"}));
                Some(json!({
                    "name": name,
                    "description": desc,
                    "parameters": params,
                }))
            })
            .collect();
        let mut body = json!({
            "systemInstruction": { "parts": [{ "text": request.instructions }] },
            "contents": contents,
        });
        if !declarations.is_empty() {
            body["tools"] = json!([{ "functionDeclarations": declarations }]);
        }
        body
    }

    pub(crate) fn decode_response(raw: &Value) -> Result<(Vec<Item>, Usage), CloudProviderError> {
        let parts = raw
            .pointer("/candidates/0/content/parts")
            .and_then(Value::as_array)
            .ok_or_else(|| CloudProviderError::Malformed {
                provider: "Gemini",
                detail: "missing candidates[0].content.parts".into(),
            })?;
        let mut items = Vec::new();
        for (idx, part) in parts.iter().enumerate() {
            if let Some(fc) = part.get("functionCall") {
                let name = fc.get("name").and_then(Value::as_str).ok_or_else(|| {
                    CloudProviderError::Malformed {
                        provider: "Gemini",
                        detail: "functionCall missing name".into(),
                    }
                })?;
                let args = fc.get("args").cloned().unwrap_or_else(|| json!({}));
                items.push(Item::Call(Invocation {
                    id: format!("gem-{idx}-{}", uuid::Uuid::new_v4().simple()),
                    name: name.to_owned(),
                    arguments: args.to_string(),
                }));
            } else if let Some(text) = part.get("text").and_then(Value::as_str)
                && !text.is_empty()
            {
                items.push(Item::assistant(text));
            }
        }
        let usage = Usage {
            input_tokens: raw
                .pointer("/usageMetadata/promptTokenCount")
                .and_then(Value::as_u64),
            cached_input_tokens: None,
            output_tokens: raw
                .pointer("/usageMetadata/candidatesTokenCount")
                .and_then(Value::as_u64),
        };
        Ok((items, usage))
    }

    async fn exchange(
        &mut self,
        request: &Request,
        cancel: &Cancellation,
        _limits: &RunLimits,
        observer: &mut dyn Observer,
    ) -> Result<Turn, Cause> {
        cancel.check()?;
        let url = format!(
            "{}/models/{}:generateContent",
            self.base_url.trim_end_matches('/'),
            self.model
        );
        let body = Self::encode_request(request);
        let send = self
            .client
            .post(url)
            .header("x-goog-api-key", &self.api_key)
            .json(&body)
            .send();
        let resp = tokio::select! {
            r = send => r.map_err(|e| Box::new(CloudProviderError::from(e)) as Cause)?,
            () = wait_cancelled(cancel) => return Err(Box::new(Failure::Cancelled)),
        };
        let status = resp.status();
        let text = tokio::select! {
            t = resp.text() => t.map_err(|e| Box::new(CloudProviderError::from(e)) as Cause)?,
            () = wait_cancelled(cancel) => return Err(Box::new(Failure::Cancelled)),
        };
        if !status.is_success() {
            return Err(Box::new(CloudProviderError::Status {
                provider: "Gemini",
                status: status.as_u16(),
                body: text.chars().take(400).collect(),
            }));
        }
        let parsed: Value = serde_json::from_str(&text).map_err(|err| {
            Box::new(CloudProviderError::Malformed {
                provider: "Gemini",
                detail: err.to_string(),
            }) as Cause
        })?;
        let (items, usage) = Self::decode_response(&parsed).map_err(|e| Box::new(e) as Cause)?;
        for item in &items {
            match item {
                Item::Call(call) => {
                    observer
                        .emit(StreamEvent::CallBegan {
                            name: call.name.clone(),
                        })
                        .await?;
                }
                Item::Message { role, text } if role == "assistant" => {
                    observer
                        .emit(StreamEvent::Text(text.clone().into()))
                        .await?;
                }
                _ => {}
            }
        }
        Ok(Turn {
            items,
            receipt: parsed,
            usage,
        })
    }
}

pub(crate) struct AnthropicMessagesProvider {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
}

impl AnthropicMessagesProvider {
    pub(crate) fn encode_request(request: &Request) -> Value {
        let mut messages = Vec::new();
        for item in &request.items {
            match item {
                Item::Message { role, text } => {
                    let a_role = if role == "assistant" {
                        "assistant"
                    } else {
                        "user"
                    };
                    messages.push(json!({
                        "role": a_role,
                        "content": [{ "type": "text", "text": text }]
                    }));
                }
                Item::Feedback { text } => {
                    messages.push(json!({
                        "role": "user",
                        "content": [{ "type": "text", "text": text }]
                    }));
                }
                Item::FinishRefused(refusal) => {
                    messages.push(json!({
                        "role": "user",
                        "content": [{ "type": "text", "text": refusal.reason() }]
                    }));
                }
                Item::Call(call) => {
                    let input: Value =
                        serde_json::from_str(&call.arguments).unwrap_or_else(|_| json!({}));
                    messages.push(json!({
                        "role": "assistant",
                        "content": [{
                            "type": "tool_use",
                            "id": call.id,
                            "name": call.name,
                            "input": input,
                        }]
                    }));
                }
                Item::MaterializedResult { call_id, output } => {
                    messages.push(json!({
                        "role": "user",
                        "content": [{
                            "type": "tool_result",
                            "tool_use_id": call_id,
                            "content": output,
                        }]
                    }));
                }
                Item::ProductMessage(_)
                | Item::OutputCutoff
                | Item::Result { .. }
                | Item::Opaque(_) => {}
            }
        }
        let tools: Vec<Value> = request
            .tools
            .iter()
            .filter_map(|t| {
                let name = t.get("name").and_then(Value::as_str)?;
                let desc = t.get("description").and_then(Value::as_str).unwrap_or("");
                let params = t
                    .get("parameters")
                    .cloned()
                    .unwrap_or_else(|| json!({"type":"object"}));
                Some(json!({
                    "name": name,
                    "description": desc,
                    "input_schema": params,
                }))
            })
            .collect();
        let mut body = json!({
            "model": request.model,
            "max_tokens": 1024,
            "system": request.instructions,
            "messages": messages,
        });
        if !tools.is_empty() {
            body["tools"] = Value::Array(tools);
        }
        body
    }

    pub(crate) fn decode_response(raw: &Value) -> Result<(Vec<Item>, Usage), CloudProviderError> {
        let blocks = raw
            .get("content")
            .and_then(Value::as_array)
            .ok_or_else(|| CloudProviderError::Malformed {
                provider: "Anthropic",
                detail: "missing content blocks".into(),
            })?;
        let mut items = Vec::new();
        for (idx, block) in blocks.iter().enumerate() {
            match block.get("type").and_then(Value::as_str) {
                Some("tool_use") => {
                    let name = block.get("name").and_then(Value::as_str).ok_or_else(|| {
                        CloudProviderError::Malformed {
                            provider: "Anthropic",
                            detail: "tool_use block missing name".into(),
                        }
                    })?;
                    let id = block
                        .get("id")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                        .unwrap_or_else(|| format!("ant-{idx}-{}", uuid::Uuid::new_v4().simple()));
                    let input = block.get("input").cloned().unwrap_or_else(|| json!({}));
                    items.push(Item::Call(Invocation {
                        id,
                        name: name.to_owned(),
                        arguments: input.to_string(),
                    }));
                }
                Some("text") => {
                    if let Some(text) = block.get("text").and_then(Value::as_str)
                        && !text.is_empty()
                    {
                        items.push(Item::assistant(text));
                    }
                }
                _ => {}
            }
        }
        let usage = Usage {
            input_tokens: raw.pointer("/usage/input_tokens").and_then(Value::as_u64),
            cached_input_tokens: raw
                .pointer("/usage/cache_read_input_tokens")
                .and_then(Value::as_u64),
            output_tokens: raw.pointer("/usage/output_tokens").and_then(Value::as_u64),
        };
        Ok((items, usage))
    }

    async fn exchange(
        &mut self,
        request: &Request,
        cancel: &Cancellation,
        _limits: &RunLimits,
        observer: &mut dyn Observer,
    ) -> Result<Turn, Cause> {
        cancel.check()?;
        let url = format!("{}/messages", self.base_url.trim_end_matches('/'));
        let body = Self::encode_request(request);
        let send = self
            .client
            .post(url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&body)
            .send();
        let resp = tokio::select! {
            r = send => r.map_err(|e| Box::new(CloudProviderError::from(e)) as Cause)?,
            () = wait_cancelled(cancel) => return Err(Box::new(Failure::Cancelled)),
        };
        let status = resp.status();
        let text = tokio::select! {
            t = resp.text() => t.map_err(|e| Box::new(CloudProviderError::from(e)) as Cause)?,
            () = wait_cancelled(cancel) => return Err(Box::new(Failure::Cancelled)),
        };
        if !status.is_success() {
            return Err(Box::new(CloudProviderError::Status {
                provider: "Anthropic",
                status: status.as_u16(),
                body: text.chars().take(400).collect(),
            }));
        }
        let parsed: Value = serde_json::from_str(&text).map_err(|err| {
            Box::new(CloudProviderError::Malformed {
                provider: "Anthropic",
                detail: err.to_string(),
            }) as Cause
        })?;
        let (items, usage) = Self::decode_response(&parsed).map_err(|e| Box::new(e) as Cause)?;
        for item in &items {
            match item {
                Item::Call(call) => {
                    observer
                        .emit(StreamEvent::CallBegan {
                            name: call.name.clone(),
                        })
                        .await?;
                }
                Item::Message { role, text } if role == "assistant" => {
                    observer
                        .emit(StreamEvent::Text(text.clone().into()))
                        .await?;
                }
                _ => {}
            }
        }
        Ok(Turn {
            items,
            receipt: parsed,
            usage,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_from_env_prefers_cloud_keys_before_candle_fallback() {
        let none = ProviderBackend::from_env(|_| None);
        assert_eq!(none.mode_label(), "candle_qwen3");

        let anthropic =
            ProviderBackend::from_env(|k| (k == "ANTHROPIC_API_KEY").then(|| "sk-ant".to_owned()));
        assert_eq!(anthropic.mode_label(), "anthropic");

        let gemini = ProviderBackend::from_env(|k| match k {
            "GEMINI_API_KEY" => Some("gem-key".to_owned()),
            "ANTHROPIC_API_KEY" => Some("sk-ant".to_owned()),
            _ => None,
        });
        assert_eq!(gemini.mode_label(), "gemini");

        let openai = ProviderBackend::from_env(|k| match k {
            "OPENAI_API_KEY" => Some("sk-oa".to_owned()),
            "GEMINI_API_KEY" => Some("gem-key".to_owned()),
            _ => None,
        });
        assert_eq!(openai.mode_label(), "openai");
    }

    #[test]
    fn gemini_request_and_response_roundtrip() {
        let req = Request {
            cache_key: None,
            model: "gemini-2.5-flash".into(),
            context_window_tokens: 16_000,
            max_input_tokens: 12_000,
            options: Value::Null,
            instructions: "Be concise".into(),
            tools: vec![json!({
                "type": "function",
                "name": "chart_search",
                "description": "Search chart",
                "parameters": { "type": "object", "properties": {} }
            })],
            items: vec![Item::Message {
                role: "user".into(),
                text: "Check labs".into(),
            }],
        };
        let encoded = GeminiChatProvider::encode_request(&req);
        assert_eq!(
            encoded["systemInstruction"]["parts"][0]["text"],
            "Be concise"
        );
        assert_eq!(
            encoded["tools"][0]["functionDeclarations"][0]["name"],
            "chart_search"
        );

        let wire_resp = json!({
            "candidates": [{
                "content": {
                    "parts": [
                        { "functionCall": { "name": "chart_search", "args": { "query": "K+" } } }
                    ]
                }
            }],
            "usageMetadata": { "promptTokenCount": 12, "candidatesTokenCount": 5 }
        });
        let (items, usage) = GeminiChatProvider::decode_response(&wire_resp).unwrap();
        assert_eq!(usage.input_tokens, Some(12));
        assert_eq!(usage.output_tokens, Some(5));
        assert!(matches!(&items[0], Item::Call(c) if c.name == "chart_search"));
    }

    #[test]
    fn anthropic_request_and_response_roundtrip() {
        let req = Request {
            cache_key: None,
            model: "claude-sonnet".into(),
            context_window_tokens: 16_000,
            max_input_tokens: 12_000,
            options: Value::Null,
            instructions: "Cite records".into(),
            tools: vec![json!({
                "name": "chart_read",
                "description": "Read chart",
                "parameters": { "type": "object", "properties": {} }
            })],
            items: vec![
                Item::Message {
                    role: "user".into(),
                    text: "Read BMP".into(),
                },
                Item::Call(Invocation {
                    id: "toolu_1".into(),
                    name: "chart_read".into(),
                    arguments: r#"{"candidate_ids":["obs-1"]}"#.into(),
                }),
                Item::MaterializedResult {
                    call_id: "toolu_1".into(),
                    output: r#"{"records":[]}"#.into(),
                },
            ],
        };
        let encoded = AnthropicMessagesProvider::encode_request(&req);
        assert_eq!(encoded["system"], "Cite records");
        assert_eq!(encoded["tools"][0]["name"], "chart_read");
        assert_eq!(encoded["messages"][2]["content"][0]["type"], "tool_result");

        let wire_resp = json!({
            "content": [{ "type": "text", "text": "Potassium is 5.2 mmol/L [1]." }],
            "usage": { "input_tokens": 20, "output_tokens": 9 }
        });
        let (items, usage) = AnthropicMessagesProvider::decode_response(&wire_resp).unwrap();
        assert_eq!(usage.input_tokens, Some(20));
        assert!(
            matches!(&items[0], Item::Message { role, text } if role == "assistant" && text.contains("[1]"))
        );
    }
}
