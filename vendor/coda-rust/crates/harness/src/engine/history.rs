//! Navigation of product-owned messages and authorized retained tool results.
//! Products check access; this module bounds excerpts, reads exact message chunks
//! and checks delivered message coverage.
use super::{
    Tool,
    recipe::{ReplayEvent, replay_events},
};
mod retained;
mod saved;
pub use retained::ResultChunk;
pub use saved::SavedPage;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Discover retained messages and tool results without loading their full text.
pub const LIST: &str = "list_conversation_history";
/// Read one retained message by its stable product identity.
pub const READ: &str = "read_conversation_message";
/// Find retained messages and tool results by literal text.
pub const SEARCH: &str = "search_conversation_history";
// Keep the default directory navigable without loading a conversation at once.
const DEFAULT_ENTRIES: usize = 20;
// Twenty previews leave room for identity and navigation within the smallest result bound.
const DEFAULT_PREVIEW_CHARS: usize = 200;
// A useful excerpt or read chunk; callers may request a different range.
const DEFAULT_CHARS: usize = 4_000;

/// All history operations are reads; products still recheck current authority.
pub fn is_tool(name: &str) -> bool {
    matches!(name, LIST | READ | SEARCH)
}

/// List, exact read and search share one message identity and character protocol.
pub fn tools() -> [Tool; 3] {
    let list = json!({"type":"object","properties":{
        "cursor":{"type":["string","null"],"minLength":1,"description":"next_cursor from an earlier page. Null starts at the first entry."},
        "order":{"type":["string","null"],"enum":["newest_first","oldest_first",null],"description":"Null lists newest first."},
        "limit":{"type":["integer","null"],"minimum":1,"description":format!("Entries in this page. Null returns up to {DEFAULT_ENTRIES}.")},
        "max_chars":{"type":["integer","null"],"minimum":1,"description":format!("Characters shown of each entry. Null shows up to {DEFAULT_PREVIEW_CHARS}.")},
        "tool_name":{"type":["string","null"],"minLength":1,"description":"Only tool results from this tool. Null lists messages and results from every tool."},
        "window":{"type":["integer","null"],"minimum":0,"description":"Only tool results from this zero-based context window. Null lists messages and results from every window."}
    },"required":[],"additionalProperties":false});
    let mut search = list.clone();
    search["properties"]["text"] = json!({"type":"string","minLength":1});
    search["required"] = json!(["text"]);
    [
        Tool { name: LIST.into(), description: format!("List retained messages and this work's tool results, newest first by default or oldest_first. Filter tool results by tool_name or zero-based window; when no result has that tool_name, the page lists the tool names that have results. Previews default to {DEFAULT_PREVIEW_CHARS} characters; originals stay intact. Use next_cursor to continue. Read messages with read_conversation_message and an id; read tool results with read_tool_result and call.id. A shortened preview is not a complete read."), parameters:list },
        Tool { name: READ.into(), description: format!("Read exact text of one retained message in character chunks. Supply its id and use next_offset_chars to continue. Offsets count Unicode characters, not bytes; default chunk length is {DEFAULT_CHARS}. The original message is unchanged."), parameters:json!({"type":"object","properties":{"id":{"type":"string","minLength":1},"offset_chars":{"type":["integer","null"],"minimum":0,"description":"First character to read. Null starts at 0."},"limit_chars":{"type":["integer","null"],"minimum":1,"description":format!("Characters to read. Null reads up to {DEFAULT_CHARS}.")}},"required":["id"],"additionalProperties":false}) },
        Tool { name: SEARCH.into(), description: "Search retained messages and this work's tool results for a literal, case-sensitive substring. Return bounded matching excerpts, newest first by default. Use next_cursor to continue, read_conversation_message for a message, or read_tool_result with call.id for a tool result.".into(), parameters:search },
    ]
}

/// Authorship is part of the retained message, not inferred from its text.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// The person using the product.
    User,
    /// Product-published model output, including explicitly marked partial output.
    Assistant,
}

impl Role {
    /// The role's name on the provider wire.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
        }
    }
}

/// An exact authored message. Its metadata belongs to the product.
#[derive(Clone, Debug)]
pub struct Message<M> {
    /// Stable identity scoped to the current conversation or job.
    pub id: String,
    /// Authorship.
    pub role: Role,
    /// Original text, never changed by navigation.
    pub text: String,
    /// Product facts such as accepted steering, work identity or partial output.
    pub metadata: M,
}

/// Exact segment of an original message, carrying its identity and full length.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chunk<M> {
    /// Stable identity for reopening this message.
    pub id: String,
    /// Retained authorship.
    pub role: Role,
    /// Exact source characters beginning at offset_chars.
    pub text: String,
    /// Product-authored message metadata.
    pub metadata: M,
    /// First character in this segment.
    pub offset_chars: usize,
    /// Full original length, before shortening.
    pub total_chars: usize,
    /// Next unread character, or null at the end of the original.
    pub next_offset_chars: Option<usize>,
}

/// Bounded navigation result, with an inclusive cursor for the next unread entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Page<M> {
    /// Message segments in requested order; one segment for an exact read.
    pub items: Vec<HistoryItem<M>>,
    /// First unread message in the requested order, or null at the current end.
    pub next_cursor: Option<String>,
    /// When a tool_name filter matches no result, the tool names that have
    /// results in this work, so the model can correct the filter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools_with_results: Option<Vec<String>>,
}

/// Authored messages and retained tool results remain distinct evidence.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum HistoryItem<M> {
    /// A product-authored message excerpt.
    Message(Chunk<M>),
    /// A retained tool result excerpt, reopened by invocation identity.
    Tool(ResultChunk),
}

impl<M> Page<M> {
    /// Change product metadata without encoding and reparsing the history page.
    pub fn map_metadata<N>(self, mut map: impl FnMut(M) -> N) -> Page<N> {
        Page {
            items: self
                .items
                .into_iter()
                .map(|item| match item {
                    HistoryItem::Message(chunk) => HistoryItem::Message(Chunk {
                        id: chunk.id,
                        role: chunk.role,
                        text: chunk.text,
                        metadata: map(chunk.metadata),
                        offset_chars: chunk.offset_chars,
                        total_chars: chunk.total_chars,
                        next_offset_chars: chunk.next_offset_chars,
                    }),
                    HistoryItem::Tool(result) => HistoryItem::Tool(result),
                })
                .collect(),
            next_cursor: self.next_cursor,
            tools_with_results: self.tools_with_results,
        }
    }
}

enum Record<'a, M> {
    Message(&'a Message<M>),
    Tool(&'a retained::ResultEntry<'a>),
}
impl<M: Clone> Record<'_, M> {
    fn cursor(&self) -> String {
        match self {
            Self::Message(message) => message.id.clone(),
            Self::Tool(result) => format!("tool:{}", result.call.id),
        }
    }
    fn text(&self) -> &str {
        match self {
            Self::Message(message) => &message.text,
            Self::Tool(result) => &result.output,
        }
    }
    fn selected(&self, listing: &Listing) -> bool {
        match self {
            Self::Message(_) => listing.tool_name.is_none() && listing.window.is_none(),
            Self::Tool(result) => {
                listing
                    .tool_name
                    .as_ref()
                    .is_none_or(|name| name == &result.call.name)
                    && listing.window.is_none_or(|window| window == result.window)
            }
        }
    }
    fn chunk(&self, byte_offset: usize, limit: usize) -> Result<HistoryItem<M>, QueryError> {
        match self {
            Self::Message(message) => message
                .chunk(message.text[..byte_offset].chars().count(), limit)
                .map(HistoryItem::Message),
            Self::Tool(result) => Ok(HistoryItem::Tool(result.chunk(byte_offset, limit))),
        }
    }
}

#[derive(Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Order {
    #[default]
    NewestFirst,
    OldestFirst,
}

// Every optional argument is an Option: null and an omitted field both mean
// the default.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Listing {
    cursor: Option<String>,
    tool_name: Option<String>,
    window: Option<u64>,
    order: Option<Order>,
    limit: Option<usize>,
    max_chars: Option<usize>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reading {
    id: String,
    offset_chars: Option<usize>,
    limit_chars: Option<usize>,
}

impl Listing {
    fn limit(&self) -> usize {
        self.limit.unwrap_or(DEFAULT_ENTRIES)
    }
    fn max_chars(&self) -> usize {
        self.max_chars.unwrap_or(DEFAULT_PREVIEW_CHARS)
    }
}

impl Reading {
    fn limit_chars(&self) -> usize {
        self.limit_chars.unwrap_or(DEFAULT_CHARS)
    }
}

/// Validated navigation operation, independent of product storage.
pub struct Query(Operation);
enum Operation {
    List(Listing),
    Read(Reading),
    Search { listing: Listing, text: String },
}

/// Correctable arguments and missing identities are distinct from storage failures.
#[derive(Debug, thiserror::Error)]
pub enum QueryError {
    /// Keep the original decoder error.
    #[error("History arguments do not match the offered fields: {0}")]
    Arguments(#[source] serde_json::Error),
    /// A supplied cursor has no identity.
    #[error("History cursor must not be empty.")]
    EmptyCursor,
    /// A requested count or segment length is zero.
    #[error("History counts and character lengths must be positive.")]
    Limit,
    /// A message read has no identity.
    #[error("Supply a nonempty message id.")]
    EmptyMessage,
    /// Search needs nonempty literal text.
    #[error("Search text must be a nonempty string.")]
    SearchText,
    /// The named operation is not a history capability.
    #[error("Unknown history operation.")]
    UnknownOperation,
    /// The product did not supply the requested message.
    #[error("Message is not available in this conversation.")]
    UnavailableMessage,
    /// The product did not supply the requested cursor.
    #[error("History cursor is not available in this conversation.")]
    UnavailableCursor,
    /// Keep the run-record failure when a retained projection cannot be resolved.
    #[error("A retained result source could not be resolved: {0}")]
    ResultSource(#[source] super::Failure),
    /// The character offset is past the original's end.
    #[error("Character offset exceeds the original message length.")]
    Offset,
}
impl Listing {
    fn parse(arguments: Value) -> Result<Self, QueryError> {
        let value: Self = serde_json::from_value(arguments).map_err(QueryError::Arguments)?;
        if value.cursor.as_ref().is_some_and(String::is_empty) {
            return Err(QueryError::EmptyCursor);
        }
        if value.limit() == 0 || value.max_chars() == 0 {
            return Err(QueryError::Limit);
        }
        Ok(value)
    }
}
/// Dispatch a history call against already-authorized product messages and events.
pub fn execute<'a, M: Clone>(
    name: &str,
    arguments: &Value,
    messages: &[Message<M>],
    events: impl IntoIterator<Item = impl Into<ReplayEvent<'a>>>,
) -> Result<Page<M>, QueryError> {
    Query::parse(name, arguments)?.answer(messages, events)
}

/// Marker immediately after a partial product answer in model context.
pub fn partial_answer() -> super::Item {
    super::Item::Message {
        role: "developer".into(),
        text: "The preceding assistant response is a retained partial answer, not completed work."
            .into(),
    }
}

/// Pointer supplied only when the product knows earlier messages exist.
pub fn earlier_messages() -> super::Item {
    super::Item::Message { role: "system".into(), text: "Earlier messages are available through list_conversation_history, search_conversation_history and read_conversation_message. Shortened previews are not complete reads.".into() }
}

impl Query {
    /// Decode the operation once, then resolve only authorized product messages.
    pub fn parse(name: &str, arguments: &Value) -> Result<Self, QueryError> {
        let operation = match name {
            LIST => Operation::List(Listing::parse(arguments.clone())?),
            READ => {
                let read: Reading =
                    serde_json::from_value(arguments.clone()).map_err(QueryError::Arguments)?;
                if read.id.is_empty() {
                    return Err(QueryError::EmptyMessage);
                }
                if read.limit_chars() == 0 {
                    return Err(QueryError::Limit);
                }
                Operation::Read(read)
            }
            SEARCH => {
                let mut listing = arguments.clone();
                let text = listing
                    .as_object_mut()
                    .and_then(|object| object.remove("text"))
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .filter(|text| !text.is_empty())
                    .ok_or(QueryError::SearchText)?;
                Operation::Search {
                    listing: Listing::parse(listing)?,
                    text,
                }
            }
            _ => return Err(QueryError::UnknownOperation),
        };
        Ok(Self(operation))
    }

    /// Read bounded segments without altering the supplied originals. Messages use
    /// product chronology; this work's tool results follow in durable event order.
    /// A list or search sees the tool results saved before its turn started.
    pub fn answer<'a, M: Clone>(
        &self,
        messages: &[Message<M>],
        events: impl IntoIterator<Item = impl Into<ReplayEvent<'a>>>,
    ) -> Result<Page<M>, QueryError> {
        let (listing, search) = match &self.0 {
            Operation::Read(read) => {
                let message = messages
                    .iter()
                    .find(|message| message.id == read.id)
                    .ok_or(QueryError::UnavailableMessage)?;
                let chunk = message.chunk(read.offset_chars.unwrap_or(0), read.limit_chars())?;
                return Ok(Page {
                    items: vec![HistoryItem::Message(chunk)],
                    next_cursor: None,
                    tools_with_results: None,
                });
            }
            Operation::List(listing) => (listing, None),
            Operation::Search { listing, text } => (listing, Some(text.as_str())),
        };
        let sources: Vec<_> = messages.iter().map(Message::source).collect();
        let results = retained::entries(
            replay_events(events).filter_map(ReplayEvent::admitted),
            &sources,
        )?;
        if let Some(tools) = unmatched_tool_name(listing, &results) {
            return Ok(Page {
                items: Vec::new(),
                next_cursor: None,
                tools_with_results: Some(tools),
            });
        }
        let mut ordered: Vec<_> = messages
            .iter()
            .map(Record::Message)
            .chain(results.iter().map(Record::Tool))
            .filter(|record| record.selected(listing))
            .collect();
        match listing.order.unwrap_or_default() {
            Order::NewestFirst => ordered.reverse(),
            Order::OldestFirst => (),
        }
        let start = match &listing.cursor {
            Some(cursor) => ordered
                .iter()
                .position(|record| record.cursor() == *cursor)
                .ok_or(QueryError::UnavailableCursor)?,
            None => 0,
        };
        let mut matches = ordered.into_iter().skip(start).filter_map(|record| {
            let offset = match search {
                Some(text) => record.text().find(text)?,
                None => 0,
            };
            Some((record, offset))
        });
        let items = matches
            .by_ref()
            .take(listing.limit())
            .map(|(record, offset)| record.chunk(offset, listing.max_chars()))
            .collect::<Result<_, _>>()?;
        let next_cursor = matches.next().map(|(record, _)| record.cursor());
        Ok(Page {
            items,
            next_cursor,
            tools_with_results: None,
        })
    }
}

/// A tool_name filter that matches no result gets the names that do have
/// results, in first-result order. Names are compared exactly.
fn unmatched_tool_name(
    listing: &Listing,
    results: &[retained::ResultEntry<'_>],
) -> Option<Vec<String>> {
    let wanted = listing.tool_name.as_ref()?;
    if results.iter().any(|result| &result.call.name == wanted) {
        return None;
    }
    let mut names: Vec<String> = Vec::new();
    for result in results {
        if !names.contains(&result.call.name) {
            names.push(result.call.name.clone());
        }
    }
    Some(names)
}
impl<M> Message<M> {
    /// The current product-owned text used to reopen retained message references.
    pub fn source(&self) -> super::ProductMessage {
        super::ProductMessage {
            id: self.id.clone(),
            role: self.role,
            text: self.text.clone(),
        }
    }
}

impl<M: Clone> Message<M> {
    fn chunk(&self, offset: usize, limit: usize) -> Result<Chunk<M>, QueryError> {
        let total = self.text.chars().count();
        if offset > total {
            return Err(QueryError::Offset);
        }
        let text: String = self.text.chars().skip(offset).take(limit).collect();
        let end = offset + text.chars().count();
        Ok(Chunk {
            id: self.id.clone(),
            role: self.role,
            text,
            metadata: self.metadata.clone(),
            offset_chars: offset,
            total_chars: total,
            next_offset_chars: (end < total).then_some(end),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    impl<M> HistoryItem<M> {
        fn message(&self) -> &Chunk<M> {
            let Self::Message(message) = self else {
                panic!("expected authored message")
            };
            message
        }
    }

    #[test]
    fn previews_search_and_chunks_keep_exact_unicode_originals() {
        let original = format!("{}needle at the end", "🪕".repeat(4_001));
        let messages = vec![
            Message {
                id: "old".into(),
                role: Role::User,
                text: "First message".into(),
                metadata: json!({}),
            },
            Message {
                id: "long".into(),
                role: Role::Assistant,
                text: original.clone(),
                metadata: json!({"partial":true}),
            },
        ];
        let listed = Query::parse(LIST, &json!({"limit":1,"max_chars":4_000}))
            .unwrap()
            .answer(&messages, std::iter::empty::<ReplayEvent>())
            .unwrap();
        assert_eq!(listed.items[0].message().id, "long");
        assert_eq!(listed.items[0].message().text.chars().count(), 4_000);
        assert_eq!(listed.items[0].message().next_offset_chars, Some(4_000));
        assert_eq!(listed.next_cursor.as_deref(), Some("old"));
        assert!(
            listed.items[0].message().metadata["partial"]
                .as_bool()
                .unwrap()
        );
        let oldest = Query::parse(LIST, &json!({"order":"oldest_first","limit":1}))
            .unwrap()
            .answer(&messages, std::iter::empty::<ReplayEvent>())
            .unwrap();
        assert_eq!(oldest.items[0].message().id, "old");
        assert_eq!(oldest.next_cursor.as_deref(), Some("long"));
        let found = Query::parse(SEARCH, &json!({"text":"needle"}))
            .unwrap()
            .answer(&messages, std::iter::empty::<ReplayEvent>())
            .unwrap();
        assert_eq!(found.items.len(), 1);
        assert_eq!(found.items[0].message().offset_chars, 4_001);
        assert_eq!(found.items[0].message().text, "needle at the end");
        let middle = Query::parse(
            READ,
            &json!({"id":"long","offset_chars":4_000,"limit_chars":1}),
        )
        .unwrap()
        .answer(&messages, std::iter::empty::<ReplayEvent>())
        .unwrap();
        assert_eq!(middle.items[0].message().text, "🪕");
        assert_eq!(
            messages[1].text, original,
            "navigation never rewrites the stored original"
        );
        assert!(
            Query::parse(READ, &json!({"id":"long","offset_chars":9_000}))
                .unwrap()
                .answer(&messages, std::iter::empty::<ReplayEvent>())
                .is_err()
        );
        assert!(
            Query::parse(LIST, &json!({"cursor":"outside"}))
                .unwrap()
                .answer(&messages, std::iter::empty::<ReplayEvent>())
                .is_err()
        );
    }
}
