//! Derive the tool directory from authorized events, without another stored index.
use super::{
    super::{Event, EventKind, Invocation, results},
    QueryError,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub(super) struct ResultEntry<'a> {
    pub(super) call: &'a Invocation,
    pub(super) output: String,
    pub(super) window: u64,
}

/// A bounded excerpt of one exact retained result. Reopen it through read_tool_result.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResultChunk {
    /// Identity for reopening the result, without repeating invocation arguments.
    pub call: CallReference,
    /// Zero-based working window where the invocation began.
    pub window: u64,
    /// Exact result excerpt, never an authored message.
    pub text: String,
    /// UTF-8 byte offset compatible with read_tool_result.
    pub offset: usize,
    /// Complete retained result length in UTF-8 bytes.
    pub total_bytes: usize,
    /// Capability that reads the exact result by call.id.
    pub read_with: String,
}

/// The call identity needed to discover and reopen an exact retained result.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallReference {
    /// Stable invocation identity.
    pub id: String,
    /// Tool that produced the result.
    pub name: String,
}

impl ResultEntry<'_> {
    pub(super) fn chunk(&self, offset: usize, limit: usize) -> ResultChunk {
        ResultChunk {
            call: CallReference {
                id: self.call.id.clone(),
                name: self.call.name.clone(),
            },
            window: self.window,
            text: self.output[offset..].chars().take(limit).collect(),
            offset,
            total_bytes: self.output.len(),
            read_with: results::READ.into(),
        }
    }
}

/// This work's tool results saved before the turn that made the current call.
/// The call's own turn is the newest returned turn, because no request starts
/// while its calls run. Its sibling calls' results are left out, so the answer
/// never depends on which call finished first; the model gets those results
/// directly in that turn.
pub(super) fn entries<'a>(
    events: impl IntoIterator<Item = &'a Event>,
    messages: &[crate::engine::ProductMessage],
) -> Result<Vec<ResultEntry<'a>>, QueryError> {
    let mut events: Vec<_> = events.into_iter().collect();
    if let Some(turn) = events
        .iter()
        .rposition(|event| matches!(event.kind, EventKind::ProviderReturned(_)))
    {
        events.truncate(turn);
    }
    let mut window = 0;
    let mut calls = BTreeMap::new();
    let mut parts = super::super::request_parts::Parts::new(messages);
    let mut entries: Vec<ResultEntry<'_>> = Vec::new();
    for event in events {
        parts.observe(event).map_err(QueryError::ResultSource)?;
        match &event.kind {
            EventKind::WindowOpened { .. } => window += 1,
            EventKind::ToolStarted(call) => {
                calls.entry(call.id.as_str()).or_insert((call, window));
            }
            EventKind::ToolReturned { call_id, result } => {
                if !result.executed() {
                    continue;
                }
                let Some(output) = parts.resolve_result(call_id) else {
                    continue;
                };
                if let Some((call, window)) = calls.get(call_id.as_str()) {
                    // Recovery retains one identity for a call. Its latest acknowledged
                    // result is also the result read_tool_result resolves.
                    if let Some(entry) = entries.iter_mut().find(|entry| entry.call.id == *call_id)
                    {
                        entry.output = output;
                    } else {
                        entries.push(ResultEntry {
                            call,
                            output,
                            window: *window,
                        });
                    }
                }
            }
            EventKind::ProviderStarted { .. }
            | EventKind::ProviderObservation { .. }
            | EventKind::CallBegan { .. }
            | EventKind::ProviderFailed { .. }
            | EventKind::TransportRetry { .. }
            | EventKind::ProviderReturned(_)
            | EventKind::ProviderBlocked { .. }
            | EventKind::ContextRejected
            | EventKind::Candidate { .. } => (),
        }
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_directory_does_not_repeat_large_call_arguments() {
        let mut events = Vec::new();
        for index in 0..21 {
            let call = Invocation {
                id: format!("note-{index}"),
                name: "save_working_note".into(),
                arguments: serde_json::json!({"text":"private handoff".repeat(10_000), "fresh_window":false}).to_string(),
            };
            for kind in [
                EventKind::ToolStarted(call.clone()),
                EventKind::ToolReturned {
                    call_id: call.id,
                    result: crate::engine::results::ToolResult::Original(
                        "retained result ".repeat(100),
                    ),
                },
            ] {
                events.push(Event {
                    attempt_id: "attempt".into(),
                    sequence: events.len() as u64 + 1,
                    ordinal: 1,
                    kind,
                });
            }
        }
        let page = super::super::Query::parse(super::super::LIST, &serde_json::json!({}))
            .unwrap()
            .answer::<serde_json::Value>(
                &[],
                events
                    .iter()
                    .map(crate::engine::recipe::ReplayEvent::Admitted),
            )
            .unwrap();
        let output = serde_json::to_string(&page).unwrap();
        let bound = results::Presentation::for_window(1).inline;
        assert!(
            output.chars().count() <= bound,
            "history defaults must fit one result: {} > {bound}",
            output.chars().count()
        );
        assert_eq!(page.items.len(), 20);
        assert!(page.next_cursor.is_some());
        assert!(!output.contains("private handoff"));
    }

    #[test]
    fn replayed_calls_keep_their_original_window_and_result() {
        let call = Invocation {
            id: "read-1".into(),
            name: "read_chart".into(),
            arguments: "{}".into(),
        };
        let kinds = [
            EventKind::ToolStarted(call.clone()),
            EventKind::ToolReturned {
                call_id: call.id.clone(),
                result: crate::engine::results::ToolResult::Original("first".into()),
            },
            EventKind::WindowOpened {
                opened_at_unix_ms: 0,
                handoff: None,
                reason: crate::engine::WindowReason::ModelAsked,
            },
            EventKind::ToolStarted(call.clone()),
            EventKind::ToolReturned {
                call_id: call.id.clone(),
                result: crate::engine::results::ToolResult::Original("first".into()),
            },
        ];
        let events: Vec<_> = kinds
            .into_iter()
            .enumerate()
            .map(|(i, kind)| Event {
                attempt_id: "attempt".into(),
                sequence: i as u64 + 1,
                ordinal: i as u64 + 1,
                kind,
            })
            .collect();
        let restored: Vec<Event> =
            serde_json::from_str(&serde_json::to_string(&events).unwrap()).unwrap();
        let indexed = entries(&restored, &[]).unwrap();
        assert_eq!(indexed.len(), 1);
        assert_eq!(indexed[0].window, 0);
        assert_eq!(
            Some(indexed[0].output.as_str()),
            results::resolve(
                restored
                    .iter()
                    .map(crate::engine::recipe::ReplayEvent::Admitted),
                &call.id,
                &[],
            )
            .unwrap()
            .as_deref()
        );
    }
}
