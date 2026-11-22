//! Retained history ranges. Product messages and prior tool receipts own text.
use super::{Chunk, HistoryItem, Page, ResultChunk, Role, retained::CallReference};
use crate::engine::ProductMessage;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A history receipt refers to its sources without retaining their excerpts.
/// Products own message identity; a new authored message has a new ID.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedPage {
    items: Vec<SavedItem>,
    next_cursor: Option<String>,
    /// Tool names listed when a tool_name filter matched no result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tools_with_results: Option<Vec<String>>,
    /// Size of the original provider representation, before presentation bounds.
    pub(crate) total_bytes: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum SavedItem {
    Message {
        id: String,
        role: Role,
        metadata: Value,
        offset_chars: usize,
        length_chars: usize,
        total_chars: usize,
    },
    Tool {
        call: CallReference,
        window: u64,
        offset: usize,
        length_bytes: usize,
        total_bytes: usize,
    },
}

impl From<Page<Value>> for SavedPage {
    fn from(page: Page<Value>) -> Self {
        let total_bytes = serde_json::to_vec(&page)
            .expect("history values serialize")
            .len();
        Self {
            items: page
                .items
                .into_iter()
                .map(|item| match item {
                    HistoryItem::Message(chunk) => SavedItem::Message {
                        id: chunk.id,
                        role: chunk.role,
                        metadata: chunk.metadata,
                        offset_chars: chunk.offset_chars,
                        length_chars: chunk.text.chars().count(),
                        total_chars: chunk.total_chars,
                    },
                    HistoryItem::Tool(chunk) => SavedItem::Tool {
                        call: chunk.call,
                        window: chunk.window,
                        offset: chunk.offset,
                        length_bytes: chunk.text.len(),
                        total_bytes: chunk.total_bytes,
                    },
                })
                .collect(),
            next_cursor: page.next_cursor,
            tools_with_results: page.tools_with_results,
            total_bytes,
        }
    }
}

impl SavedPage {
    pub(crate) fn result_ids(&self) -> impl Iterator<Item = &str> {
        self.items.iter().filter_map(|item| match item {
            SavedItem::Tool { call, .. } => Some(call.id.as_str()),
            SavedItem::Message { .. } => None,
        })
    }

    /// Missing or changed sources cannot reconstruct the old result. The caller
    /// rebuilds the request with explicit source-unavailable feedback instead.
    pub(crate) fn resolve(
        &self,
        messages: &[ProductMessage],
        result: impl Fn(&str) -> Option<String>,
    ) -> Option<Page<Value>> {
        let items = self
            .items
            .iter()
            .map(|item| match item {
                SavedItem::Message {
                    id,
                    role,
                    metadata,
                    offset_chars,
                    length_chars,
                    total_chars,
                } => {
                    let original = &messages.iter().find(|message| &message.id == id)?.text;
                    if original.chars().count() != *total_chars {
                        return None;
                    }
                    let text: String = original
                        .chars()
                        .skip(*offset_chars)
                        .take(*length_chars)
                        .collect();
                    if text.chars().count() != *length_chars {
                        return None;
                    }
                    let end = offset_chars.checked_add(*length_chars)?;
                    Some(HistoryItem::Message(Chunk {
                        id: id.clone(),
                        role: *role,
                        text,
                        metadata: metadata.clone(),
                        offset_chars: *offset_chars,
                        total_chars: *total_chars,
                        next_offset_chars: (end < *total_chars).then_some(end),
                    }))
                }
                SavedItem::Tool {
                    call,
                    window,
                    offset,
                    length_bytes,
                    total_bytes,
                } => {
                    let original = result(&call.id)?;
                    if original.len() != *total_bytes {
                        return None;
                    }
                    let text = original.get(*offset..offset.checked_add(*length_bytes)?)?;
                    Some(HistoryItem::Tool(ResultChunk {
                        call: call.clone(),
                        window: *window,
                        text: text.into(),
                        offset: *offset,
                        total_bytes: *total_bytes,
                        read_with: super::super::results::READ.into(),
                    }))
                }
            })
            .collect::<Option<_>>()?;
        Some(Page {
            items,
            next_cursor: self.next_cursor.clone(),
            tools_with_results: self.tools_with_results.clone(),
        })
    }
}
