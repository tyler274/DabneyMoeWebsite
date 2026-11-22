//! Bounded Responses SSE framing and the stream contract. No tool dispatch here.
//!
//! The completed output items (`response.output_item.done`) of a response that
//! completed are the turn. Streamed text deltas are a live preview only and are
//! never compared with anything. Codex, Pi, OpenClaw and Hermes all take the
//! completed item as the truth and ignore event and item types they do not use.
//! The checks here protect only the turn: one item for each output position, and
//! that every announced item completed. The first terminal event ends the stream.
use std::collections::BTreeMap;

use serde_json::Value;

use crate::engine::{
    DeltaLocation, Failure, TextDelta, Turn,
    recipe::{Observer, StreamEvent},
};

fn protocol(message: &str) -> Failure {
    Failure::Protocol(message.into())
}
fn position(value: &Value, key: &str) -> Option<usize> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|n| usize::try_from(n).ok())
}
fn index(value: &Value, key: &str) -> Result<usize, Failure> {
    position(value, key).ok_or_else(|| protocol("missing stream index"))
}

/// The item types the turn is built from. Other items are ignored.
fn used(item: &Value) -> bool {
    match item.get("type").and_then(Value::as_str) {
        Some("function_call" | "reasoning") => true,
        Some("message") => item.get("role").and_then(Value::as_str) == Some("assistant"),
        _ => false,
    }
}

/// The provider location of a live text delta, when it supplied every coordinate.
fn location(response: Option<&str>, event: &Value) -> Option<DeltaLocation> {
    Some(DeltaLocation {
        response_id: response?.to_owned(),
        item_id: event.get("item_id")?.as_str()?.to_owned(),
        sequence: event.get("sequence_number")?.as_u64()?,
        output_index: position(event, "output_index")?,
        content_index: position(event, "content_index")?,
    })
}

fn stream_failure(event: &Value) -> Failure {
    let provider_error = event
        .pointer("/response/error")
        .or_else(|| event.get("error"));
    let error = provider_error.unwrap_or(event);
    // An incomplete response names its reason in place of an error code,
    // such as a reply the provider filtered.
    let code = error.get("code").and_then(Value::as_str).or_else(|| {
        event
            .pointer("/response/incomplete_details/reason")
            .and_then(Value::as_str)
    });
    // A top-level SSE event uses `type` for the event kind. Provider error
    // identity only occupies `type` inside the nested error envelope.
    let error_type = provider_error
        .and_then(|error| error.get("type"))
        .and_then(Value::as_str);
    let header = error
        .get("headers")
        .and_then(Value::as_object)
        .and_then(|headers| {
            headers
                .iter()
                .find(|(key, _)| key.eq_ignore_ascii_case("retry-after"))
                .and_then(|(_, value)| value.as_str())
        });
    let mut hint = super::retry_after(header, std::time::SystemTime::now());
    if matches!(code, Some("rate_limit_exceeded" | "slow_down")) {
        hint = hint.or_else(|| {
            error
                .get("message")
                .and_then(Value::as_str)
                .and_then(retry_delay)
        });
    }
    // As for an HTTP error, the saved error text is the code, never the
    // provider's message.
    Failure::Provider(super::classify_error(
        code,
        error_type,
        code.unwrap_or("provider stream failed or incomplete")
            .into(),
        hint,
        true,
    ))
}

// Codex uses provider prose for delays only on rate-limit errors. Keep this
// parsing at the wire boundary; provider prose never enters model feedback.
fn retry_delay(message: &str) -> Option<std::time::Duration> {
    let message = message.to_ascii_lowercase();
    let (_, delay) = message.split_once("try again in")?;
    let delay = delay.trim_start();
    let number = delay
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(delay.len());
    let value = delay[..number].parse::<f64>().ok()?;
    let unit = delay[number..].trim_start();
    let seconds = if unit.starts_with("ms") {
        value / 1000.0
    } else if unit.starts_with('s') {
        value
    } else {
        return None;
    };
    std::time::Duration::try_from_secs_f64(seconds).ok()
}

pub(super) struct Frame {
    event: Option<String>,
    data: String,
}

/// Bytes are retained until a complete line exists, so UTF-8 may span chunks.
/// Total stream bytes bound line, frame, event count and terminal payload storage.
pub(super) struct Decoder {
    cap: usize,
    consumed: usize,
    line: Vec<u8>,
    data: Vec<String>,
    event: Option<String>,
    cr: bool,
}
impl Decoder {
    pub fn new(cap: usize) -> Self {
        Self {
            cap,
            consumed: 0,
            line: vec![],
            data: vec![],
            event: None,
            cr: false,
        }
    }
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<Frame>, Failure> {
        if bytes.len() > self.cap.saturating_sub(self.consumed) {
            return Err(protocol("SSE byte limit"));
        }
        self.consumed += bytes.len();
        let mut frames = vec![];
        for byte in bytes {
            if self.cr {
                self.cr = false;
                if *byte == b'\n' {
                    continue;
                }
            }
            if matches!(*byte, b'\n' | b'\r') {
                self.cr = *byte == b'\r';
                let line = String::from_utf8(std::mem::take(&mut self.line))
                    .map_err(|_| protocol("invalid SSE UTF-8"))?;
                if line.is_empty() {
                    if !self.data.is_empty() {
                        frames.push(Frame {
                            event: self.event.take(),
                            data: self.data.join("\n"),
                        });
                        self.data.clear();
                    } else {
                        self.event = None;
                    }
                } else if !line.starts_with(':') {
                    let (field, value) = line.split_once(':').unwrap_or((&line, ""));
                    let value = value.strip_prefix(' ').unwrap_or(value);
                    match field {
                        "data" => self.data.push(value.into()),
                        "event" => self.event = Some(value.into()),
                        "id" | "retry" => (), // Never reconnect.
                        _ => (),
                    }
                }
            } else {
                self.line.push(*byte);
            }
        }
        Ok(frames)
    }
}

/// One output position. The announcement holds the position and registers its
/// tool call; the completed item is the turn.
#[derive(Default)]
struct Output {
    added: Option<Value>,
    done: Option<Value>,
}
#[derive(Default)]
pub(super) struct Stream {
    authorization: super::Authorization,
    /// The first created response's id. It only locates live text.
    response: Option<String>,
    outputs: BTreeMap<usize, Output>,
    terminal: Option<Turn>,
}
impl Stream {
    pub fn new(authorization: super::Authorization) -> Self {
        Self {
            authorization,
            ..Self::default()
        }
    }

    pub async fn accept(&mut self, frame: Frame, sink: &mut dyn Observer) -> Result<(), Failure> {
        // The first terminal event ends the stream; later frames are ignored.
        if self.terminal.is_some() {
            return Ok(());
        }
        // A `[DONE]` marker carries nothing, wherever it appears.
        if frame.event.is_none() && frame.data == "[DONE]" {
            return Ok(());
        }
        // A frame that is not a JSON event is skipped and the stream goes on, as
        // Codex skips a frame it cannot parse. The completion rules still catch
        // a skipped frame the turn needed.
        let Ok(event) = serde_json::from_str::<Value>(&frame.data) else {
            return Ok(());
        };
        // The JSON `type` names the event; the SSE `event:` name is not read.
        let Some(kind) = event.get("type").and_then(Value::as_str) else {
            return Ok(());
        };
        match kind {
            "response.created" => self.created(&event, sink).await,
            "response.output_item.added" => self.added(&event, sink).await,
            "response.output_item.done" => self.done(&event),
            "response.output_text.delta" => self.text(&event, sink).await,
            "response.completed" => self.completed(&event, sink).await,
            "response.failed" | "response.incomplete" => self.ended(kind, &event, sink).await,
            "error" => Err(stream_failure(&event)),
            // The completed items carry what the turn needs. Other events, such
            // as argument and part deltas, reasoning summaries, metadata and
            // heartbeats, are ignored, as the references do.
            _ => Ok(()),
        }
    }

    /// The first created response supplies the id that locates live text. A
    /// later creation event is ignored.
    async fn created(&mut self, event: &Value, sink: &mut dyn Observer) -> Result<(), Failure> {
        if self.response.is_some() {
            return Ok(());
        }
        let response = &event["response"];
        self.response = response
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_owned);
        sink.emit(super::observation(response, self.authorization))
            .await
            .map_err(Failure::Host)
    }

    /// An announced item holds its output position. An announced call also
    /// goes to the observer as begun, with its name: the model has started it
    /// and its arguments are still to come. An announced call with no name or
    /// an empty one, and an item that only completes, announce nothing.
    async fn added(&mut self, event: &Value, sink: &mut dyn Observer) -> Result<(), Failure> {
        let item = &event["item"];
        if !used(item) {
            return Ok(());
        }
        let output = self
            .outputs
            .entry(index(event, "output_index")?)
            .or_default();
        if output.added.is_some() || output.done.is_some() {
            return Err(protocol("output index already holds an item"));
        }
        output.added = Some(item.clone());
        if item.get("type").and_then(Value::as_str) != Some("function_call") {
            return Ok(());
        }
        let Some(name) = item
            .get("name")
            .and_then(Value::as_str)
            .filter(|name| !name.is_empty())
        else {
            return Ok(());
        };
        sink.emit(StreamEvent::CallBegan {
            name: name.to_owned(),
        })
        .await
        .map_err(Failure::Host)
    }

    /// The completed item is the truth for the turn. It may arrive without an
    /// announcement.
    fn done(&mut self, event: &Value) -> Result<(), Failure> {
        let item = &event["item"];
        if !used(item) {
            return Ok(());
        }
        let output = self
            .outputs
            .entry(index(event, "output_index")?)
            .or_default();
        if output.done.is_some() {
            return Err(protocol("output index already done"));
        }
        output.done = Some(item.clone());
        Ok(())
    }

    /// Each text delta goes to the observer as it arrives. It is a preview
    /// only; the completed message is the turn's text.
    async fn text(&self, event: &Value, sink: &mut dyn Observer) -> Result<(), Failure> {
        let Some(delta) = event.get("delta").and_then(Value::as_str) else {
            return Ok(());
        };
        sink.emit(StreamEvent::Text(TextDelta {
            location: location(self.response.as_deref(), event),
            text: delta.to_owned(),
        }))
        .await
        .map_err(Failure::Host)
    }

    /// The turn is the completed items in output order. The terminal `output`
    /// array is not read, because the completed items are already the
    /// provider's final values.
    async fn completed(&mut self, event: &Value, sink: &mut dyn Observer) -> Result<(), Failure> {
        let terminal = &event["response"];
        let items = self
            .outputs
            .values()
            .map(|output| {
                output
                    .done
                    .as_ref()
                    .ok_or_else(|| protocol("unfinished output item"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        sink.emit(super::observation(terminal, self.authorization))
            .await
            .map_err(Failure::Host)?;
        self.terminal = Some(
            super::decode_items(terminal, self.authorization, items).map_err(Failure::Protocol)?,
        );
        Ok(())
    }

    /// A failed or incomplete response yields no turn, except the cut-off turn
    /// of a response that reached its output token limit.
    async fn ended(
        &mut self,
        kind: &str,
        event: &Value,
        sink: &mut dyn Observer,
    ) -> Result<(), Failure> {
        let response = &event["response"];
        sink.emit(super::observation(response, self.authorization))
            .await
            .map_err(Failure::Host)?;
        if kind == "response.incomplete"
            && let Some(turn) = super::cut_off(response, self.authorization)
        {
            self.terminal = Some(turn);
            return Ok(());
        }
        Err(stream_failure(event))
    }

    pub fn has_terminal(&self) -> bool {
        self.terminal.is_some()
    }
    pub fn finish(self) -> Result<Turn, Failure> {
        self.terminal
            .ok_or_else(|| protocol("stream ended without completed response"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fragmented_utf8_crlf_multiline_and_comments() {
        let bytes = ": note\r\nevent: test\r\ndata: {\"value\":\r\ndata: \"é\"}\r\n\r\n".as_bytes();
        let mut decoder = Decoder::new(bytes.len());
        let mut frames = Vec::new();
        for byte in bytes {
            frames.extend(decoder.push(&[*byte]).unwrap());
        }
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].event.as_deref(), Some("test"));
        let value: Value = serde_json::from_str(&frames[0].data).unwrap();
        assert_eq!(value["value"], "é");
    }

    #[test]
    fn malformed_utf8_and_bounds_are_rejected() {
        let mut decoder = Decoder::new(100);
        assert!(decoder.push(b"data: \xff\n\n").is_err());
        let mut decoder = Decoder::new(4);
        assert!(decoder.push(b"12345").is_err());
    }

    use super::super::Authorization;
    use crate::engine::{Cause, Invocation, Item};
    use serde_json::json;

    /// Frames each payload as SSE and feeds it through the decoder and the
    /// validator as `Client::stream` does, stopping at the terminal event.
    async fn run(
        authorization: Authorization,
        payloads: &[String],
    ) -> (Result<Turn, Failure>, Vec<TextDelta>) {
        let bytes: String = payloads
            .iter()
            .map(|payload| format!("data: {payload}\n\n"))
            .collect();
        let mut live = Vec::new();
        let result = {
            let mut sink = |event: StreamEvent| -> Result<(), Cause> {
                if let StreamEvent::Text(delta) = event {
                    live.push(delta);
                }
                Ok(())
            };
            let mut decoder = Decoder::new(bytes.len());
            let mut stream = Stream::new(authorization);
            async {
                for frame in decoder.push(bytes.as_bytes())? {
                    stream.accept(frame, &mut sink).await?;
                    if stream.has_terminal() {
                        break;
                    }
                }
                stream.finish()
            }
            .await
        };
        (result, live)
    }
    async fn plan(events: &[Value]) -> (Result<Turn, Failure>, Vec<TextDelta>) {
        run(Authorization::ChatGptPlan, &payloads(events)).await
    }
    /// Numbers events from zero, as a provider does.
    fn numbered(mut events: Vec<Value>) -> Vec<Value> {
        for (index, event) in events.iter_mut().enumerate() {
            event["sequence_number"] = json!(index);
        }
        events
    }
    fn payloads(events: &[Value]) -> Vec<String> {
        events.iter().map(Value::to_string).collect()
    }
    fn protocol_error(result: Result<Turn, Failure>) -> String {
        match result {
            Err(Failure::Protocol(message)) => message,
            other => panic!("expected a protocol failure, got {other:?}"),
        }
    }
    fn created() -> Value {
        json!({"type":"response.created","response":{"id":"r"}})
    }
    /// A completion without the terminal `output` array.
    fn completed() -> Value {
        json!({"type":"response.completed","response":{"id":"r","status":"completed","usage":{"input_tokens":5,"output_tokens":3}}})
    }
    fn added(index: usize, item: Value) -> Value {
        json!({"type":"response.output_item.added","output_index":index,"item":item})
    }
    fn done(index: usize, item: Value) -> Value {
        json!({"type":"response.output_item.done","output_index":index,"item":item})
    }
    fn message(text: &str) -> Value {
        json!({"type":"message","id":"m","role":"assistant","status":"completed","content":[{"type":"output_text","text":text,"annotations":[]}]})
    }
    fn call(id: &str, call_id: &str, name: &str) -> Value {
        json!({"type":"function_call","id":id,"call_id":call_id,"name":name,"arguments":"{}","status":"completed"})
    }
    fn invocation(id: &str, name: &str) -> Item {
        Item::Call(Invocation {
            id: id.into(),
            name: name.into(),
            arguments: "{}".into(),
        })
    }

    #[tokio::test]
    async fn final_message_text_wins_over_streamed_preview() {
        let piece = |delta: &str| json!({"type":"response.output_text.delta","output_index":0,"item_id":"m","content_index":0,"delta":delta});
        let events = numbered(vec![
            created(),
            added(
                0,
                json!({"type":"message","id":"m","role":"assistant","content":[]}),
            ),
            json!({"type":"response.content_part.added","output_index":0,"item_id":"m","content_index":0,"part":{"type":"output_text","text":""}}),
            piece("Hel"),
            piece("lo"),
            json!({"type":"response.output_text.done","output_index":0,"item_id":"m","content_index":0,"text":"Hello"}),
            json!({"type":"response.content_part.done","output_index":0,"item_id":"m","content_index":0,"part":{"type":"output_text","text":"Hello"}}),
            done(0, message("Hello, corrected")),
            completed(),
        ]);
        let (result, live) = plan(&events).await;
        assert_eq!(result.unwrap().text(), "Hello, corrected");
        let texts: Vec<_> = live.iter().map(|delta| delta.text.as_str()).collect();
        assert_eq!(texts, ["Hel", "lo"]);
        assert_eq!(
            live[1].location,
            Some(crate::engine::DeltaLocation {
                response_id: "r".into(),
                item_id: "m".into(),
                sequence: 4,
                output_index: 0,
                content_index: 0,
            })
        );
    }

    #[tokio::test]
    async fn unknown_item_types_are_left_out_of_the_turn() {
        let search = json!({"type":"web_search_call","id":"ws","status":"completed"});
        let events = numbered(vec![
            created(),
            added(0, search.clone()),
            done(0, search),
            added(1, call("f", "c", "save")),
            done(1, call("f", "c", "save")),
            completed(),
        ]);
        let (result, _) = plan(&events).await;
        assert_eq!(result.unwrap().items, vec![invocation("c", "save")]);
    }

    #[tokio::test]
    async fn sequence_numbers_do_not_decide_acceptance() {
        let base = numbered(vec![
            created(),
            added(0, call("f", "c", "save")),
            done(0, call("f", "c", "save")),
            completed(),
        ]);
        let mut repeated = base.clone();
        repeated[2]["sequence_number"] = json!(1);
        let mut skipped = base.clone();
        skipped[2]["sequence_number"] = json!(7);
        let mut absent = base.clone();
        absent[1].as_object_mut().unwrap().remove("sequence_number");
        absent.insert(2, json!({"type":"keepalive"}));
        let mut refused = Vec::new();
        for (case, events) in [
            ("repeated", repeated),
            ("skipped", skipped),
            ("absent", absent),
        ] {
            match plan(&events).await.0 {
                Ok(turn) => assert_eq!(turn.items, vec![invocation("c", "save")]),
                Err(failure) => refused.push(format!("{case}: {failure:?}")),
            }
        }
        assert!(refused.is_empty(), "{refused:?}");
    }

    #[tokio::test]
    async fn call_item_id_may_change_between_its_events() {
        let events = numbered(vec![
            created(),
            added(0, call("fc_a", "c", "save")),
            json!({"type":"response.function_call_arguments.delta","output_index":0,"item_id":"fc_a","delta":"{}"}),
            json!({"type":"response.function_call_arguments.done","output_index":0,"item_id":"fc_a","arguments":"{}"}),
            done(0, call("fc_b", "c", "save")),
            completed(),
        ]);
        let (result, _) = plan(&events).await;
        assert_eq!(result.unwrap().items, vec![invocation("c", "save")]);
    }

    #[tokio::test]
    async fn completed_items_need_no_announcement() {
        let events = numbered(vec![
            created(),
            done(0, message("Saved.")),
            done(1, call("f", "c", "save")),
            completed(),
        ]);
        let (result, _) = plan(&events).await;
        assert_eq!(
            result.unwrap().items,
            vec![
                Item::Message {
                    role: "assistant".into(),
                    text: "Saved.".into()
                },
                invocation("c", "save")
            ]
        );
    }

    #[tokio::test]
    async fn api_turn_comes_from_completed_items_not_the_terminal_echo() {
        let reasoning =
            json!({"type":"reasoning","id":"rs","summary":[],"encrypted_content":"streamed"});
        let mut echoed = reasoning.clone();
        echoed["encrypted_content"] = json!("different");
        let differing = json!({"id":"r","status":"completed","usage":{},"output":[echoed]});
        let omitted = json!({"id":"r","status":"completed","usage":{}});
        let mut refused = Vec::new();
        for (case, terminal) in [("differing", differing), ("omitted", omitted)] {
            let events = numbered(vec![
                created(),
                added(0, reasoning.clone()),
                done(0, reasoning.clone()),
                json!({"type":"response.completed","response":terminal}),
            ]);
            match run(Authorization::Api, &payloads(&events)).await.0 {
                Ok(turn) => assert_eq!(turn.items, vec![Item::Opaque(reasoning.clone())]),
                Err(failure) => refused.push(format!("{case}: {failure:?}")),
            }
        }
        assert!(refused.is_empty(), "{refused:?}");
    }

    #[tokio::test]
    async fn frames_that_are_not_typed_json_are_skipped() {
        let events = numbered(vec![
            created(),
            added(0, call("f", "c", "save")),
            done(0, call("f", "c", "save")),
            completed(),
        ]);
        let clean = plan(&events).await.0.unwrap();
        assert_eq!(clean.items, vec![invocation("c", "save")]);
        let mut noisy = payloads(&events);
        noisy.insert(1, "{not json".into());
        noisy.insert(3, json!({"output_index":0}).to_string());
        noisy.insert(5, json!({"type":7,"output_index":0}).to_string());
        let (result, _) = run(Authorization::ChatGptPlan, &noisy).await;
        assert_eq!(result.unwrap(), clean);
    }

    #[tokio::test]
    async fn done_marker_is_ignored_wherever_it_appears() {
        let mut events = payloads(&numbered(vec![
            created(),
            added(0, call("f", "c", "save")),
            done(0, call("f", "c", "save")),
            completed(),
        ]));
        events.insert(0, "[DONE]".into());
        events.insert(3, "[DONE]".into());
        events.push("[DONE]".into());
        let (result, _) = run(Authorization::ChatGptPlan, &events).await;
        assert_eq!(result.unwrap().items, vec![invocation("c", "save")]);
    }

    #[tokio::test]
    async fn used_items_must_name_their_output_index() {
        for event in [
            json!({"type":"response.output_item.added","item":call("f", "c", "save")}),
            json!({"type":"response.output_item.done","item":message("Hi")}),
            json!({"type":"response.output_item.done","item":{"type":"reasoning","id":"rs","summary":[]}}),
        ] {
            let (result, _) = plan(&numbered(vec![created(), event, completed()])).await;
            assert_eq!(protocol_error(result), "missing stream index");
        }
    }

    #[tokio::test]
    async fn calls_with_empty_or_repeated_ids_and_empty_names_reach_the_engine() {
        for announced in [true, false] {
            let mut events = vec![created()];
            for (index, item) in [
                call("f", "", "save"),
                call("g", "c", ""),
                call("h", "c", "save"),
            ]
            .into_iter()
            .enumerate()
            {
                if announced {
                    events.push(added(index, item.clone()));
                }
                events.push(done(index, item));
            }
            events.push(completed());
            let (result, _) = plan(&numbered(events)).await;
            assert_eq!(
                result.unwrap().items,
                vec![
                    invocation("", "save"),
                    invocation("c", ""),
                    invocation("c", "save")
                ],
                "announced: {announced}"
            );
        }
    }

    #[tokio::test]
    async fn calls_with_missing_fields_reach_the_engine_as_empty() {
        for announced in [true, false] {
            let item = json!({"type":"function_call","id":"f","status":"completed"});
            let mut events = vec![created()];
            if announced {
                events.push(added(0, item.clone()));
            }
            events.push(done(0, item));
            events.push(completed());
            let (result, _) = plan(&numbered(events)).await;
            assert_eq!(
                result.unwrap().items,
                vec![Item::Call(Invocation {
                    id: String::new(),
                    name: String::new(),
                    arguments: String::new(),
                })],
                "announced: {announced}"
            );
        }
    }

    /// The names of the calls the observer heard begin while `events` streamed, and the turn.
    async fn began(events: &[Value]) -> (Result<Turn, Failure>, Vec<String>) {
        let mut names = Vec::new();
        let result = {
            let mut sink = |event: StreamEvent| -> Result<(), Cause> {
                if let StreamEvent::CallBegan { name } = event {
                    names.push(name);
                }
                Ok(())
            };
            let mut stream = Stream::new(Authorization::ChatGptPlan);
            async {
                for payload in payloads(events) {
                    let frame = Frame {
                        event: None,
                        data: payload,
                    };
                    stream.accept(frame, &mut sink).await?;
                }
                stream.finish()
            }
            .await
        };
        (result, names)
    }

    /// Each function call the provider announces begins once, by its name, when it is announced,
    /// before its arguments arrive. A call that arrives only completed begins nothing, nor does an
    /// announced call with no name, nor an announced message or reasoning item. The completed
    /// items stay the turn.
    #[tokio::test]
    async fn each_announced_call_begins_once_and_a_completed_item_announces_nothing() {
        let pending = |id: &str, call_id: &str, name: &str| json!({"type":"function_call","id":id,"call_id":call_id,"name":name,"arguments":"","status":"in_progress"});
        let reasoning = json!({"type":"reasoning","id":"rs","summary":[]});
        let events = numbered(vec![
            created(),
            added(0, reasoning.clone()),
            done(0, reasoning.clone()),
            added(
                1,
                json!({"type":"message","id":"m","role":"assistant","content":[]}),
            ),
            done(1, message("Reading.")),
            added(2, pending("f", "c", "search")),
            json!({"type":"response.function_call_arguments.delta","output_index":2,"item_id":"f","delta":"{}"}),
            done(2, call("f", "c", "search")),
            done(3, call("g", "d", "read")),
            added(4, pending("h", "e", "finish")),
            done(4, call("h", "e", "finish")),
            added(
                5,
                json!({"type":"function_call","id":"k","call_id":"n","arguments":"","status":"in_progress"}),
            ),
            done(5, call("k", "n", "save")),
            added(6, pending("p", "q", "")),
            done(6, call("p", "q", "write")),
            completed(),
        ]);
        let (result, names) = began(&events).await;
        assert_eq!(names, ["search", "finish"]);
        assert_eq!(
            result.unwrap().items,
            vec![
                Item::Opaque(reasoning),
                Item::Message {
                    role: "assistant".into(),
                    text: "Reading.".into()
                },
                invocation("c", "search"),
                invocation("d", "read"),
                invocation("e", "finish"),
                invocation("n", "save"),
                invocation("q", "write"),
            ]
        );
    }

    /// A call announced in a stream that then fails began all the same: the observer heard it
    /// while the stream ran.
    #[tokio::test]
    async fn a_call_announced_before_the_stream_fails_has_begun() {
        let events = numbered(vec![
            created(),
            added(
                0,
                json!({"type":"function_call","id":"f","call_id":"c","name":"finish","arguments":"","status":"in_progress"}),
            ),
            json!({"type":"response.failed","response":{"id":"r","status":"failed"}}),
        ]);
        let (result, names) = began(&events).await;
        assert!(result.is_err());
        assert_eq!(names, ["finish"]);
    }

    #[tokio::test]
    async fn an_output_index_holds_one_item() {
        let twice_added = numbered(vec![
            created(),
            added(0, call("f", "c", "save")),
            added(0, call("g", "d", "save")),
            completed(),
        ]);
        let twice_done = numbered(vec![
            created(),
            done(0, call("f", "c", "save")),
            done(0, call("g", "d", "save")),
            completed(),
        ]);
        let (result, _) = plan(&twice_added).await;
        assert_eq!(protocol_error(result), "output index already holds an item");
        let (result, _) = plan(&twice_done).await;
        assert_eq!(protocol_error(result), "output index already done");
    }
}
