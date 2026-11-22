//! Present retained tool results without repeating the original operation.
use super::{Event, EventKind, Failure, Item, Tool, ToolOutcome, recipe::Request};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{borrow::Cow, collections::BTreeMap, num::NonZeroUsize, ops::Range};

/// Product-offered capability for reading exact bytes of a saved result.
pub const READ: &str = "read_tool_result";

// Character budgets follow the harness spec; transport offsets remain UTF-8 bytes.
const RESULT_PERCENT: u64 = 15;
const TURN_PERCENT: u64 = 30;
const MIN_RESULT_CHARACTERS: usize = 8_000;
const MAX_RESULT_CHARACTERS: usize = 100_000;
const MIN_TURN_CHARACTERS: usize = 16_000;
const MAX_TURN_CHARACTERS: usize = 200_000;
const PREVIEW_CHARACTERS: usize = 1_500;
// One refusal recovery retains a useful page independently of ordinary window sizing.
const REFUSAL_READ_CHARACTERS: usize = 8_000;

/// Exact result retained once in the run record, or a projection of an earlier
/// retained result. A projection never copies the original bytes into a request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolResult {
    /// Product-owned result bytes, retained by the invocation that produced them.
    Original(String),
    /// Structured history output. Its message ranges remain typed until the
    /// provider boundary instead of being rediscovered from JSON text.
    History(super::history::SavedPage),
    /// A page or other bounded representation of an earlier result.
    View(ResultView),
    /// The operation was refused and did not run, or it failed. Either way
    /// the product returned no output for it.
    Refused(super::recipe::Refusal),
}

impl ToolResult {
    /// Original bytes when this invocation owns them.
    pub fn original(&self) -> Option<&str> {
        match self {
            Self::Original(output) => Some(output),
            Self::History(_) | Self::View(_) | Self::Refused(_) => None,
        }
    }

    /// Whether the product returned output for the operation. A refusal or a
    /// failure has none.
    pub(crate) fn executed(&self) -> bool {
        match self {
            Self::Original(_) | Self::History(_) | Self::View(_) => true,
            Self::Refused(_) => false,
        }
    }
}

/// A result owner reconstructed from the run record. History stays structured;
/// text is encoded only while building the provider request.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum OriginalResult {
    Text(String),
    History(super::history::Page<Value>),
}

impl OriginalResult {
    pub(super) fn text(&self) -> Option<&str> {
        match self {
            Self::Text(output) => Some(output),
            Self::History(_) => None,
        }
    }

    pub(super) fn encoded(&self) -> Option<String> {
        match self {
            Self::Text(output) => Some(output.clone()),
            Self::History(page) => serde_json::to_string(page).ok(),
        }
    }
}

/// Byte range in one retained result. UTF-8 boundaries are checked when the
/// provider representation is materialized.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ByteRange {
    /// Inclusive UTF-8 byte offset.
    pub start: usize,
    /// Exclusive UTF-8 byte offset.
    pub end: usize,
}

impl ByteRange {
    fn as_range(self) -> Range<usize> {
        self.start..self.end
    }
}

/// Model-visible shape of a retained result.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Representation {
    /// The complete original result.
    Whole,
    /// A short view of both ends of the original.
    Preview {
        /// Prefix range.
        head: ByteRange,
        /// Suffix range.
        tail: ByteRange,
    },
    /// One complete contiguous page.
    Page {
        /// Complete contiguous range.
        range: ByteRange,
    },
    /// One contiguous page of one message in a history read. The range
    /// selects bytes of that message's text, not of the encoded result.
    MessagePage {
        /// Position of the message in the history page.
        item: usize,
        /// Contiguous range of the message's text.
        range: ByteRange,
    },
}

/// Harness guidance added to a model-visible result without changing the
/// product-owned original or its delivery ranges.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Annotation {
    /// Kind of guidance. A result keeps one note of each kind.
    pub field: String,
    /// Plain guidance value.
    pub text: String,
}

/// Typed request projection. `source_call_id` names the one ToolReturned event
/// that owns the exact original bytes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResultView {
    /// Owner of the original bytes.
    pub source: ResultSource,
    /// Exact original size in UTF-8 bytes.
    pub total_bytes: usize,
    /// Ranges delivered by this representation.
    pub representation: Representation,
    /// Harness guidance, separate from the source ranges.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub annotations: Vec<Annotation>,
}

/// Canonical owner of bytes used by a result representation.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ResultSource {
    /// Product tool result retained once by ToolReturned.
    Retained {
        /// Invocation that retained the original.
        call_id: String,
    },
    /// One refusal receipt. A later retry of the same call can have a new outcome.
    Refused {
        /// Attempt that acknowledged the feedback.
        attempt_id: String,
        /// Event sequence within that attempt.
        sequence: u64,
    },
    /// A product refusal of a finish proposed with calls. It has no product
    /// ToolReturned.
    FinishRefused {
        /// The typed refusal, rendered only at the provider boundary.
        refusal: super::recipe::Refusal,
    },
    /// Harness-authored corrective feedback. It has no product ToolReturned.
    Inline {
        /// Harness-authored corrective bytes.
        output: String,
    },
}

impl ResultView {
    /// A complete inline representation of one retained result.
    pub fn whole(source_call_id: impl Into<String>, original: &str) -> Self {
        Self {
            source: ResultSource::Retained {
                call_id: source_call_id.into(),
            },
            total_bytes: original.len(),
            representation: Representation::Whole,
            annotations: Vec::new(),
        }
    }

    pub(super) fn refused(
        attempt_id: &str,
        sequence: u64,
        refusal: &super::recipe::Refusal,
    ) -> Self {
        Self {
            source: ResultSource::Refused {
                attempt_id: attempt_id.into(),
                sequence,
            },
            ..Self::whole("", &refusal.feedback().to_string())
        }
    }

    pub(super) fn finish_refused(refusal: super::recipe::Refusal) -> Self {
        Self {
            total_bytes: refusal.feedback().to_string().len(),
            source: ResultSource::FinishRefused { refusal },
            representation: Representation::Whole,
            annotations: Vec::new(),
        }
    }

    /// Harness-owned corrective output with an explicit inline owner.
    pub fn inline(output: String) -> Self {
        Self {
            total_bytes: output.len(),
            source: ResultSource::Inline { output },
            representation: Representation::Whole,
            annotations: Vec::new(),
        }
    }

    /// Retained invocation identity, when this view refers to a product result.
    pub(crate) fn retained_call_id(&self) -> Option<&str> {
        match &self.source {
            ResultSource::Retained { call_id } => Some(call_id),
            ResultSource::Refused { .. }
            | ResultSource::FinishRefused { .. }
            | ResultSource::Inline { .. } => None,
        }
    }

    pub(super) fn unavailable() -> Self {
        Self::inline(json!({
            "source_unavailable":true,
            "instruction":"A message referenced by this result was deleted or changed. Reopen current sources; do not rely on the earlier result."
        }).to_string())
    }

    pub(super) fn whole_history(
        source_call_id: impl Into<String>,
        page: &super::history::Page<Value>,
    ) -> Self {
        let encoded = serde_json::to_string(page).expect("history values serialize");
        Self::retained_whole(source_call_id, encoded.len())
    }

    pub(super) fn retained_whole(source_call_id: impl Into<String>, total_bytes: usize) -> Self {
        Self {
            source: ResultSource::Retained {
                call_id: source_call_id.into(),
            },
            total_bytes,
            representation: Representation::Whole,
            annotations: Vec::new(),
        }
    }

    /// Ranges of the encoded result this view delivers. A message page
    /// selects bytes of one message instead, so it has none.
    fn ranges(&self) -> Option<impl Iterator<Item = ByteRange>> {
        let ranges = match self.representation {
            Representation::Whole => [
                Some(ByteRange {
                    start: 0,
                    end: self.total_bytes,
                }),
                None,
            ],
            Representation::Preview { head, tail } => [Some(head), Some(tail)],
            Representation::Page { range } => [Some(range), None],
            Representation::MessagePage { .. } => return None,
        };
        Some(ranges.into_iter().flatten())
    }

    fn validate_text(&self, original: &str) -> Option<()> {
        if original.len() != self.total_bytes {
            return None;
        }
        self.ranges()?
            .all(|range| {
                range.start <= range.end
                    && range.end <= original.len()
                    && original.is_char_boundary(range.start)
                    && original.is_char_boundary(range.end)
            })
            .then_some(())
    }

    /// Add guidance without changing which source bytes this view covers.
    pub(crate) fn annotate(&mut self, field: impl Into<String>, text: impl Into<String>) {
        let field = field.into();
        let text = text.into();
        if let Some(existing) = self.annotations.iter_mut().find(|note| note.field == field) {
            existing.text = text;
        } else {
            self.annotations.push(Annotation { field, text });
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct Presentation {
    pub(super) inline: usize,
    turn: usize,
}

impl Presentation {
    pub(super) fn for_window(tokens: u64) -> Self {
        let characters = tokens.saturating_mul(super::context::CHARACTERS_PER_TOKEN);
        let percent =
            |value| usize::try_from(characters.saturating_mul(value) / 100).unwrap_or(usize::MAX);
        Self {
            inline: percent(RESULT_PERCENT).clamp(MIN_RESULT_CHARACTERS, MAX_RESULT_CHARACTERS),
            turn: percent(TURN_PERCENT).clamp(MIN_TURN_CHARACTERS, MAX_TURN_CHARACTERS),
        }
    }

    pub(super) fn show(self, id: &str, original: &str) -> ResultView {
        if original.chars().count() <= self.inline {
            ResultView::whole(id, original)
        } else {
            preview(id, original, PREVIEW_CHARACTERS, Vec::new())
        }
    }
}

fn prefix_bytes(text: &str, characters: usize) -> usize {
    text.char_indices()
        .nth(characters)
        .map_or(text.len(), |(offset, _)| offset)
}

/// Read any range or find the next literal match under the product's authority.
pub fn tool() -> Tool {
    Tool {
        name: READ.into(),
        description: "Read a retained result without repeating its tool. Use its source_call_id, any UTF-8 byte offset (default 0), and an optional limit in characters. Use total_bytes to jump near the end. With search, find the next literal, case-sensitive match at or after offset. Continue a page with next_offset. The original stays unchanged.".into(),
        parameters: json!({"type":"object","properties":{
            "call_id":{"type":"string","minLength":1},
            "offset":{"type":["integer","null"],"minimum":0,"description":"UTF-8 byte offset to start at. Null starts at 0."},
            "limit":{"type":["integer","null"],"minimum":1,"description":"Characters to read. Null reads one full page."},
            "search":{"type":["string","null"],"minLength":1,"description":"Literal text to find at or after offset. Null reads from offset without searching."}
        },"required":["call_id"],"additionalProperties":false}),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PageRequest {
    pub(super) call_id: String,
    // Null and an omitted field both start at 0.
    #[serde(default, deserialize_with = "null_as_zero")]
    offset: usize,
    limit: Option<NonZeroUsize>,
    search: Option<String>,
}

fn null_as_zero<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<usize, D::Error> {
    Ok(Option::<usize>::deserialize(deserializer)?.unwrap_or(0))
}

#[derive(Serialize, Deserialize)]
struct Segment<'a> {
    offset: usize,
    text: Cow<'a, str>,
}

#[derive(Serialize, Deserialize)]
struct Page<'a> {
    source_call_id: Cow<'a, str>,
    total_bytes: usize,
    offset: usize,
    text: Cow<'a, str>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tail: Option<Segment<'a>>,
    read_with: Cow<'a, str>,
    next_offset: Option<usize>,
}

fn encode(page: &Page<'_>) -> String {
    serde_json::to_string(page).expect("result page has only scalar fields")
}

/// Count characters in the complete representation, preserving exact UTF-8 ranges.
fn page(
    id: &str,
    original: &str,
    offset: usize,
    limit: usize,
    budget: usize,
) -> Option<ResultView> {
    let remaining = original.get(offset..)?;
    let mut characters = remaining.chars().count().min(limit);
    loop {
        let length = prefix_bytes(remaining, characters);
        let end = offset + length;
        let view = ResultView {
            source: ResultSource::Retained { call_id: id.into() },
            total_bytes: original.len(),
            representation: Representation::Page {
                range: ByteRange { start: offset, end },
            },
            annotations: Vec::new(),
        };
        let encoded = materialize(&view, original)?;
        let overflow = encoded.chars().count().saturating_sub(budget);
        if overflow == 0 {
            return Some(view);
        }
        if characters == 0 {
            return None;
        }
        characters = characters.saturating_sub(overflow);
    }
}

fn preview(id: &str, original: &str, budget: usize, annotations: Vec<Annotation>) -> ResultView {
    let total = original.chars().count();
    let mut characters = total.min(budget);
    loop {
        let head_end = prefix_bytes(original, characters.div_ceil(2));
        let tail_start = prefix_bytes(original, total - characters / 2);
        let view = ResultView {
            source: ResultSource::Retained { call_id: id.into() },
            total_bytes: original.len(),
            representation: Representation::Preview {
                head: ByteRange {
                    start: 0,
                    end: head_end,
                },
                tail: ByteRange {
                    start: tail_start,
                    end: original.len(),
                },
            },
            annotations: annotations.clone(),
        };
        let encoded = materialize(&view, original).expect("preview uses original boundaries");
        let overflow = encoded.chars().count().saturating_sub(budget);
        if overflow == 0 || characters == 0 {
            return view;
        }
        characters = characters.saturating_sub(overflow);
    }
}

/// Encode a typed projection for the provider. This is the only place source
/// ranges become result text.
pub(super) fn materialize(view: &ResultView, original: &str) -> Option<String> {
    materialize_original(view, &OriginalResult::Text(original.to_owned()))
}

pub(super) fn materialize_original(view: &ResultView, original: &OriginalResult) -> Option<String> {
    let encoded = || {
        let encoded = original.encoded()?;
        view.validate_text(&encoded)?;
        Some(encoded)
    };
    let mut output = match view.representation {
        Representation::Whole => encoded()?,
        Representation::Preview { head, tail } => {
            let original = encoded()?;
            encode(&Page {
                source_call_id: view.retained_call_id()?.into(),
                total_bytes: view.total_bytes,
                offset: head.start,
                text: original.get(head.as_range())?.into(),
                tail: Some(Segment {
                    offset: tail.start,
                    text: original.get(tail.as_range())?.into(),
                }),
                read_with: READ.into(),
                next_offset: Some(head.end),
            })
        }
        Representation::Page { range } => {
            let original = encoded()?;
            encode(&Page {
                source_call_id: view.retained_call_id()?.into(),
                total_bytes: view.total_bytes,
                offset: range.start,
                text: original.get(range.as_range())?.into(),
                tail: None,
                read_with: READ.into(),
                next_offset: (range.end < view.total_bytes).then_some(range.end),
            })
        }
        Representation::MessagePage { item, range } => {
            let OriginalResult::History(page) = original else {
                return None;
            };
            message_page(page, item, range)?
        }
    };
    for annotation in &view.annotations {
        append_note(&mut output, &annotation.text);
    }
    Some(output)
}

/// The history page with one message cut to the selected range of its text.
fn message_page(
    page: &super::history::Page<Value>,
    item: usize,
    range: ByteRange,
) -> Option<String> {
    let mut page = page.clone();
    let super::history::HistoryItem::Message(chunk) = page.items.get_mut(item)? else {
        return None;
    };
    let selected = chunk.text.get(range.as_range())?.to_owned();
    let preceding = chunk.text[..range.start].chars().count();
    let selected_chars = selected.chars().count();
    chunk.offset_chars = chunk.offset_chars.checked_add(preceding)?;
    chunk.text = selected;
    let end = chunk.offset_chars.checked_add(selected_chars)?;
    chunk.next_offset_chars = (end < chunk.total_chars).then_some(end);
    serde_json::to_string(&page).ok()
}

/// A requested read keeps one contiguous page and its continuation offset.
pub(super) fn shorten_read(
    name: &str,
    view: &mut ResultView,
    original: &OriginalResult,
    budget: usize,
) -> bool {
    if name == READ {
        let Some(encoded_original) = original.encoded() else {
            return false;
        };
        let Representation::Page { mut range } = view.representation else {
            return false;
        };
        loop {
            view.representation = Representation::Page { range };
            let Some(encoded) = materialize_original(view, original) else {
                return false;
            };
            let overflow = encoded.chars().count().saturating_sub(budget);
            if overflow == 0 {
                return true;
            }
            let Some(text) = encoded_original.get(range.as_range()) else {
                return false;
            };
            let length = text.chars().count();
            if length <= 1 {
                return true;
            }
            let keep = prefix_bytes(text, length.saturating_sub(overflow).max(1));
            range.end = range.start + keep;
        }
    } else if name == super::history::READ {
        let OriginalResult::History(page) = original else {
            return false;
        };
        if page.items.len() != 1 {
            return false;
        }
        let super::history::HistoryItem::Message(item) = &page.items[0] else {
            return false;
        };
        let mut kept_bytes = item.text.len();
        loop {
            view.representation = Representation::MessagePage {
                item: 0,
                range: ByteRange {
                    start: 0,
                    end: kept_bytes,
                },
            };
            let Some(encoded) = materialize_original(view, original) else {
                return false;
            };
            let overflow = encoded.chars().count().saturating_sub(budget);
            if overflow == 0 {
                return true;
            }
            let text = &item.text[..kept_bytes];
            let length = text.chars().count();
            if length <= 1 {
                return false;
            }
            let kept_characters = length.saturating_sub(overflow).max(1);
            let next = prefix_bytes(text, kept_characters);
            if next >= text.len() {
                return false;
            }
            kept_bytes = next;
        }
    }
    false
}

pub(super) fn is_page_read(name: &str) -> bool {
    matches!(name, READ | super::history::READ)
}

/// Only new results can change. Preview the largest nonreads first, then shorten
/// large read pages to one shared final length. Smaller pages and sent items stay whole.
/// The turn bound never exceeds `room`, the characters left in the window.
pub(super) fn page_batch(
    request: &mut Request,
    parts: &super::request_parts::Parts,
    sent_items: usize,
    room: usize,
) -> Result<(), Failure> {
    let presentation = Presentation::for_window(request.context_window_tokens);
    let turn = presentation.turn.min(room);
    // Guidance is part of the display. Rebound previews after annotations, even
    // when the batch itself fits comfortably in its larger turn allowance.
    for item in request.items.iter_mut().skip(sent_items) {
        if let Item::Result { output, .. } = item
            && matches!(output.representation, Representation::Preview { .. })
            && parts.rendered_len(output)? > PREVIEW_CHARACTERS
            && let Some(source) = output.retained_call_id().map(str::to_owned)
            && let Some(original) = parts
                .original_for(output)
                .and_then(|original| original.encoded())
        {
            *output = preview(
                &source,
                &original,
                PREVIEW_CHARACTERS,
                std::mem::take(&mut output.annotations),
            );
        }
    }
    let mut others = Vec::new();
    let mut reads = Vec::new();
    for (index, item) in request.items.iter().enumerate().skip(sent_items) {
        let Item::Result { call_id, output } = item else {
            continue;
        };
        let call = request.items[..index]
            .iter()
            .rev()
            .find_map(|item| item.call().filter(|call| call.id == *call_id))
            .ok_or_else(|| Failure::Protocol("tool receipt without its invocation".into()))?;
        let entry = (index, call.name.clone(), parts.rendered_len(output)?);
        if is_page_read(&call.name) {
            reads.push(entry);
        } else {
            others.push(entry);
        }
    }
    let excess = |request: &Request| -> Result<usize, Failure> {
        let sizes = request.items[sent_items..]
            .iter()
            .filter_map(|item| match item {
                Item::Result { output, .. } => Some(parts.rendered_len(output)),
                Item::ProductMessage(_)
                | Item::Feedback { .. }
                | Item::FinishRefused(_)
                | Item::OutputCutoff
                | Item::Message { .. }
                | Item::Call(_)
                | Item::MaterializedResult { .. }
                | Item::Opaque(_) => None,
            })
            .collect::<Result<Vec<_>, _>>()?;
        let total = sizes.iter().sum::<usize>();
        let single = sizes
            .into_iter()
            .max()
            .unwrap_or(0)
            .saturating_sub(presentation.inline);
        Ok(total.saturating_sub(turn).max(single))
    };
    others.sort_by_key(|(_, _, size)| std::cmp::Reverse(*size));
    for (index, _, size) in others {
        if excess(request)? == 0 {
            break;
        }
        if size <= PREVIEW_CHARACTERS {
            continue;
        }
        let Item::Result { output, .. } = &mut request.items[index] else {
            unreachable!()
        };
        let Some(original) = parts.original_for(output) else {
            continue;
        };
        let Some(original) = original.encoded() else {
            continue;
        };
        let Some(source) = output.retained_call_id().map(str::to_owned) else {
            continue;
        };
        *output = preview(
            &source,
            &original,
            PREVIEW_CHARACTERS,
            std::mem::take(&mut output.annotations),
        );
    }
    // Small pages consume only their actual encoded length, guidance included.
    let total = request.items[sent_items..]
        .iter()
        .filter_map(|item| match item {
            Item::Result { output, .. } => Some(parts.rendered_len(output)),
            Item::ProductMessage(_)
            | Item::Feedback { .. }
            | Item::FinishRefused(_)
            | Item::OutputCutoff
            | Item::Message { .. }
            | Item::Call(_)
            | Item::MaterializedResult { .. }
            | Item::Opaque(_) => None,
        })
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .sum::<usize>();
    for (index, _, size) in &mut reads {
        let Item::Result { output, .. } = &request.items[*index] else {
            unreachable!()
        };
        *size = parts.rendered_len(output)?;
    }
    let read_total = reads.iter().map(|(_, _, size)| *size).sum::<usize>();
    let mut available = turn.saturating_sub(total.saturating_sub(read_total));
    reads.sort_by_key(|(_, _, size)| *size);
    let mut remaining = reads.len();
    let mut ceiling = presentation.inline;
    for (index, name, size) in reads {
        let shared = (available / remaining).min(ceiling);
        remaining -= 1;
        if size <= shared {
            available = available.saturating_sub(size);
            continue;
        }
        // Every remaining page is at least this large, so all share this ceiling.
        ceiling = shared;
        available = available.saturating_sub(shared);
        let Item::Result { output, .. } = &mut request.items[index] else {
            unreachable!()
        };
        let Some(original) = parts.original_for(output) else {
            continue;
        };
        let _ = shorten_read(&name, output, &original, shared);
    }
    Ok(())
}

/// The one refused-fresh-window retry keeps complete pages and reduces only
/// pending representations. The next ordinary batch pass enforces its turn bound.
pub(super) fn reduce_pending(
    request: &mut Request,
    parts: &super::request_parts::Parts,
) -> Result<(), Failure> {
    for index in 0..request.items.len() {
        let Item::Result { call_id, .. } = &request.items[index] else {
            continue;
        };
        let name = request.items[..index].iter().rev().find_map(|item| {
            item.call()
                .filter(|call| call.id == *call_id)
                .map(|call| call.name.clone())
        });
        let Item::Result { output, .. } = &mut request.items[index] else {
            unreachable!()
        };
        if let Some(name) = name.filter(|name| is_page_read(name)) {
            if parts.rendered_len(output)? > REFUSAL_READ_CHARACTERS
                && let Some(original) = parts.original_for(output)
            {
                let _ = shorten_read(&name, output, &original, REFUSAL_READ_CHARACTERS);
            }
        } else if parts.rendered_len(output)? > PREVIEW_CHARACTERS
            && let (Some(original), Some(source)) = (
                parts.original_for(output),
                output.retained_call_id().map(str::to_owned),
            )
            && let Some(original) = original.encoded()
        {
            *output = preview(
                &source,
                &original,
                PREVIEW_CHARACTERS,
                std::mem::take(&mut output.annotations),
            );
        }
    }
    Ok(())
}

impl PageRequest {
    fn start(&self, original: &str) -> Option<usize> {
        let remaining = original.get(self.offset..)?;
        match &self.search {
            Some(query) if !query.is_empty() => {
                remaining.find(query).map(|found| self.offset + found)
            }
            Some(_) => None,
            None => Some(self.offset),
        }
    }

    pub(super) fn read(&self, original: Option<&str>, presentation: Presentation) -> ToolOutcome {
        let refused = || {
            ToolOutcome::refused(
                "The requested result range is unavailable. Use a retained call_id, a UTF-8 byte offset, a positive limit and nonempty search text.",
            )
        };
        let Some(original) = original else {
            return refused();
        };
        if original.get(self.offset..).is_none()
            || self.search.as_ref().is_some_and(String::is_empty)
        {
            return refused();
        }
        let Some(offset) = self.start(original) else {
            return ToolOutcome::Returned(json!({"source_call_id":self.call_id,"total_bytes":original.len(),"search":self.search,"found":false,"offset":self.offset}).to_string());
        };
        let limit = self
            .limit
            .map_or(presentation.inline, NonZeroUsize::get)
            .min(presentation.inline);
        match page(&self.call_id, original, offset, limit, presentation.inline) {
            Some(page) => ToolOutcome::Retained(ToolResult::View(page)),
            None => refused(),
        }
    }
}

/// Resolve any retained tool result, including a page projected from an earlier
/// original, without parsing its provider representation.
pub fn resolve<'a>(
    events: impl IntoIterator<Item = impl Into<super::recipe::ReplayEvent<'a>>>,
    id: &str,
    messages: &[super::ProductMessage],
) -> Result<Option<String>, Failure> {
    let events =
        super::recipe::replay_events(events).filter_map(super::recipe::ReplayEvent::admitted);
    let parts = super::request_parts::Parts::from_events(events, messages)?;
    Ok(parts.resolve_result(id))
}

/// Executed original results fully delivered in the latest working window.
/// Products interpret their receipt type; the harness owns record authority,
/// replay promotion, and accepted delivery.
pub fn delivered_originals<'a>(
    records: impl IntoIterator<Item = impl Into<super::recipe::ReplayEvent<'a>>>,
    messages: &[super::ProductMessage],
) -> Result<Vec<&'a str>, Failure> {
    let records: Vec<_> = records.into_iter().map(Into::into).collect();
    let delivery = Delivery::from_events(records.iter().copied(), messages)?;
    Ok(super::recipe::replay_events(records)
        .filter_map(super::recipe::ReplayEvent::admitted)
        .filter_map(|event| match &event.kind {
            EventKind::ToolReturned {
                call_id,
                result: ToolResult::Original(output),
            } if delivery.covers(call_id, Some(delivery.latest_window())) => Some(output.as_str()),
            EventKind::ToolReturned { .. }
            | EventKind::ProviderStarted { .. }
            | EventKind::ProviderObservation { .. }
            | EventKind::CallBegan { .. }
            | EventKind::ProviderFailed { .. }
            | EventKind::TransportRetry { .. }
            | EventKind::ProviderReturned(_)
            | EventKind::ToolStarted(_)
            | EventKind::ProviderBlocked { .. }
            | EventKind::ContextRejected
            | EventKind::Candidate { .. }
            | EventKind::WindowOpened { .. } => None,
        })
        .collect())
}

/// Accepted provider delivery derived from the run's ordered facts. It is a
/// read model, never another durable ledger.
/// A large result can reach the model only as a preview or a page, so a
/// returned call does not show the model saw it. Products check this before a
/// finish or write that depends on that evidence.
#[derive(Default)]
pub struct Delivery {
    ranges: BTreeMap<(u64, String), (usize, Vec<ByteRange>)>,
    messages: BTreeMap<(u64, String), Vec<MessageDelivery>>,
    direct_messages: BTreeMap<(u64, String), ()>,
    latest_window: u64,
    refusals: Vec<(u64, super::recipe::Refusal)>,
}

#[derive(Clone)]
struct MessageDelivery {
    offset_chars: usize,
    total_chars: usize,
    text: String,
}

/// Match an accepted response to its exact dispatch. All delivery consumers use
/// this ordering, including warning recovery, without another durable record.
#[derive(Default)]
struct AcceptedRequests<'a> {
    window: u64,
    pending: BTreeMap<(&'a str, u64), AcceptedRequest<'a>>,
}

struct AcceptedRequest<'a> {
    window: u64,
    ordinal: u64,
    request: &'a super::SavedRequest,
    context: &'a super::DispatchContext,
}

impl<'a> AcceptedRequests<'a> {
    fn observe(&mut self, event: &'a Event) -> Option<AcceptedRequest<'a>> {
        let key = (event.attempt_id.as_str(), event.ordinal);
        match &event.kind {
            EventKind::WindowOpened { .. } => self.window = self.window.saturating_add(1),
            EventKind::ProviderStarted {
                request, context, ..
            } => {
                self.pending.insert(
                    key,
                    AcceptedRequest {
                        window: self.window,
                        ordinal: event.ordinal,
                        request,
                        context,
                    },
                );
            }
            EventKind::ProviderReturned(_) => return self.pending.remove(&key),
            EventKind::ContextRejected
            | EventKind::ProviderFailed { .. }
            | EventKind::ProviderBlocked { .. }
            | EventKind::TransportRetry { .. } => {
                self.pending.remove(&key);
            }
            EventKind::ProviderObservation { .. }
            | EventKind::CallBegan { .. }
            | EventKind::ToolStarted(_)
            | EventKind::ToolReturned { .. }
            | EventKind::Candidate { .. } => (),
        }
        None
    }
}

pub(super) fn warning_received<'a>(
    events: impl IntoIterator<Item = &'a Event>,
    after: u64,
) -> bool {
    let mut accepted = AcceptedRequests::default();
    events.into_iter().any(|event| {
        accepted
            .observe(event)
            .is_some_and(|request| request.ordinal > after && request.context.warning_added)
    })
}

impl Delivery {
    /// Build coverage from requests that have a matching ProviderReturned.
    /// Context-rejected, failed and merely-started requests never count.
    pub fn from_events<'a>(
        events: impl IntoIterator<Item = impl Into<super::recipe::ReplayEvent<'a>>>,
        messages: &[super::ProductMessage],
    ) -> Result<Self, Failure> {
        let mut delivery = Self::default();
        let mut parts = super::request_parts::Parts::new(messages);
        let mut accepted = AcceptedRequests::default();
        for record in super::recipe::replay_events(events) {
            let Some(event) = record.admitted() else {
                continue;
            };
            parts.observe(event)?;
            let Some(received) = accepted.observe(event) else {
                continue;
            };
            let accepted_window = received.window;
            let request = received.request;
            delivery.refusals.extend(
                parts
                    .refusals(request)
                    .map(|refusal| (accepted_window, refusal.clone())),
            );
            for reference in &request.items {
                if let super::request_parts::RequestItem::Message { id } = reference {
                    delivery
                        .direct_messages
                        .insert((accepted_window, id.clone()), ());
                }
            }
            for (view, original) in parts.result_views(request)? {
                let Some(call_id) = view.retained_call_id() else {
                    continue;
                };
                if let Some(ranges) = view.ranges() {
                    let entry = delivery
                        .ranges
                        .entry((accepted_window, call_id.to_owned()))
                        .or_insert_with(|| (view.total_bytes, Vec::new()));
                    if entry.0 != view.total_bytes {
                        return Err(Failure::Protocol(
                            "one result identity has conflicting original lengths".into(),
                        ));
                    }
                    entry.1.extend(ranges);
                }
                if let OriginalResult::History(page) = &original {
                    let complete;
                    let message_view = if delivery.covers(call_id, Some(accepted_window)) {
                        complete = ResultView::whole_history(call_id, page);
                        &complete
                    } else {
                        view
                    };
                    for (id, message) in delivered_messages(message_view, page) {
                        delivery
                            .messages
                            .entry((accepted_window, id))
                            .or_default()
                            .push(message);
                    }
                }
            }
        }
        delivery.latest_window = accepted.window;
        Ok(delivery)
    }

    /// Whether a completed exchange in the latest window received this refusal.
    /// A failed or merely started dispatch cannot acknowledge a correction.
    pub fn received_refusal(&self, refusal: &super::recipe::Refusal) -> bool {
        self.refusals
            .iter()
            .any(|(window, received)| *window == self.latest_window && received == refusal)
    }

    /// Whether accepted requests cover every byte of the retained result.
    /// `window=None` means the caller's entire authorized event scope.
    pub fn covers(&self, call_id: &str, window: Option<u64>) -> bool {
        let mut total = None;
        let mut ranges = Vec::new();
        for ((seen_window, seen_call), (seen_total, seen_ranges)) in &self.ranges {
            if seen_call == call_id && window.is_none_or(|wanted| wanted == *seen_window) {
                if total.is_some_and(|saved| saved != *seen_total) {
                    return false;
                }
                total = Some(*seen_total);
                ranges.extend(seen_ranges.iter().copied());
            }
        }
        let Some(total) = total.filter(|total| *total > 0) else {
            return false;
        };
        ranges.sort_by_key(|range| range.start);
        let mut covered = 0;
        for range in ranges {
            if range.start > covered {
                break;
            }
            covered = covered.max(range.end);
        }
        covered == total
    }

    /// Whether accepted requests cover every character of the current message.
    /// Exact retained chunks are checked against the supplied product original;
    /// changed or deleted text therefore cannot inherit earlier delivery.
    pub fn covers_message(&self, id: &str, original: &str, window: Option<u64>) -> bool {
        let total = original.chars().count();
        if total == 0 {
            return false;
        }
        if self.direct_messages.keys().any(|(seen_window, seen_id)| {
            seen_id == id && window.is_none_or(|wanted| wanted == *seen_window)
        }) {
            return true;
        }
        let mut ranges = Vec::new();
        for ((seen_window, seen_id), chunks) in &self.messages {
            if seen_id != id || window.is_some_and(|wanted| wanted != *seen_window) {
                continue;
            }
            for chunk in chunks {
                if chunk.total_chars != total {
                    continue;
                }
                let length = chunk.text.chars().count();
                let Some(end) = chunk.offset_chars.checked_add(length) else {
                    continue;
                };
                if end <= total
                    && original
                        .chars()
                        .skip(chunk.offset_chars)
                        .take(length)
                        .eq(chunk.text.chars())
                {
                    ranges.push((chunk.offset_chars, end));
                }
            }
        }
        ranges.sort_unstable();
        let mut covered = 0;
        for (start, end) in ranges {
            if start > covered {
                break;
            }
            covered = covered.max(end);
        }
        covered == total
    }

    /// Latest zero-based working window observed in this delivery record.
    pub(crate) fn latest_window(&self) -> u64 {
        self.latest_window
    }
}

fn delivered_messages(
    view: &ResultView,
    page: &super::history::Page<Value>,
) -> Vec<(String, MessageDelivery)> {
    match view.representation {
        Representation::Whole => page
            .items
            .iter()
            .filter_map(|item| {
                let super::history::HistoryItem::Message(chunk) = item else {
                    return None;
                };
                Some((
                    chunk.id.clone(),
                    MessageDelivery {
                        offset_chars: chunk.offset_chars,
                        total_chars: chunk.total_chars,
                        text: chunk.text.clone(),
                    },
                ))
            })
            .collect(),
        Representation::MessagePage { item, range } => {
            let Some(super::history::HistoryItem::Message(chunk)) = page.items.get(item) else {
                return Vec::new();
            };
            let Some(text) = chunk.text.get(range.as_range()) else {
                return Vec::new();
            };
            let preceding = chunk.text[..range.start].chars().count();
            vec![(
                chunk.id.clone(),
                MessageDelivery {
                    offset_chars: chunk.offset_chars.saturating_add(preceding),
                    total_chars: chunk.total_chars,
                    text: text.to_owned(),
                },
            )]
        }
        Representation::Preview { .. } | Representation::Page { .. } => Vec::new(),
    }
}

/// Add harness guidance after the result text without changing the retained
/// product receipt.
pub(super) fn append_note(output: &mut String, note: &str) {
    output.push_str("\n\n");
    output.push_str(note);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{DispatchContext, RequestItem, SavedRequest, Turn, recipe::Usage};

    fn event(sequence: u64, ordinal: u64, kind: EventKind) -> Event {
        Event {
            attempt_id: "attempt".into(),
            sequence,
            ordinal,
            kind,
        }
    }

    fn started(sequence: u64, ordinal: u64, item: Item) -> Event {
        event(
            sequence,
            ordinal,
            EventKind::ProviderStarted {
                dispatched_at_unix_ms: sequence,
                request: SavedRequest {
                    added: vec![item],
                    items: vec![RequestItem::Event {
                        attempt: "attempt".into(),
                        sequence,
                        index: 0,
                    }],
                },
                context: DispatchContext::default(),
            },
        )
    }

    fn returned(sequence: u64, ordinal: u64) -> Event {
        event(
            sequence,
            ordinal,
            EventKind::ProviderReturned(Turn {
                items: vec![],
                receipt: Value::Null,
                usage: Usage::default(),
            }),
        )
    }

    #[test]
    fn ranges_and_search_read_exact_bytes_without_claiming_a_complete_read() {
        let original = "head 🪕 middle needle match end";
        let presentation = Presentation::for_window(272_000);
        for (arguments, expected) in [
            (json!({"call_id":"source","offset":10,"limit":6}), "middle"),
            (
                json!({"call_id":"source","search":"needle","limit":6}),
                "needle",
            ),
            (json!({"call_id":"source","offset":30,"limit":3}), "end"),
        ] {
            let read: PageRequest = serde_json::from_value(arguments).unwrap();
            let ToolOutcome::Retained(ToolResult::View(view)) =
                read.read(Some(original), presentation)
            else {
                panic!("valid page must be projected")
            };
            let output = materialize(&view, original).unwrap();
            let returned: Page = serde_json::from_str(&output).unwrap();
            assert_eq!(returned.text, expected);
            assert_eq!(
                original.get(returned.offset..returned.offset + returned.text.len()),
                Some(expected)
            );
            assert_eq!(returned.total_bytes, original.len());
            assert!(returned.text.len() < original.len());
        }
        let missing: PageRequest =
            serde_json::from_value(json!({"call_id":"source","search":"absent"})).unwrap();
        let outcome = missing.read(Some(original), presentation);
        let missing: Value = serde_json::from_str(outcome.text().unwrap()).unwrap();
        assert_eq!(missing["found"], false);
        let invalid: PageRequest =
            serde_json::from_value(json!({"call_id":"source","offset":6})).unwrap();
        assert!(matches!(
            invalid.read(Some(original), presentation),
            ToolOutcome::Refused(_)
        ));
    }

    #[test]
    fn null_read_arguments_mean_the_same_as_leaving_them_out() {
        let original = "head 🪕 middle needle match end";
        let presentation = Presentation::for_window(272_000);
        let read = |arguments: Value| {
            serde_json::from_value::<PageRequest>(arguments)
                .unwrap()
                .read(Some(original), presentation)
        };
        assert_eq!(
            read(json!({"call_id":"source","offset":null,"limit":null,"search":null})),
            read(json!({"call_id":"source"}))
        );
        assert!(matches!(
            read(json!({"call_id":"source","offset":null})),
            ToolOutcome::Retained(ToolResult::View(view))
                if view.representation == Representation::Page { range: ByteRange { start: 0, end: original.len() } }
        ));
    }

    #[test]
    fn escaped_multibyte_pages_reassemble_and_keep_the_encoded_bound() {
        let original = "ab🪕\"\n".repeat(600);
        let mut text = String::new();
        let mut offset = 0;
        loop {
            let view = page("source", &original, offset, 256, 256).unwrap();
            let encoded = materialize(&view, &original).unwrap();
            assert!(encoded.chars().count() <= 256);
            let part: Page = serde_json::from_str(&encoded).unwrap();
            assert_eq!(part.offset, text.len());
            text.push_str(&part.text);
            let Some(next) = part.next_offset else {
                break;
            };
            assert!(next > offset);
            offset = next;
        }
        assert_eq!(text, original);
        assert!(page("source", &original, 3, 256, 256).is_none());
    }

    #[test]
    fn delivery_counts_only_accepted_ranges_and_tracks_an_empty_fresh_window() {
        let original = format!("start {} end", "x".repeat(10_000));
        let whole = ResultView::whole("source", &original);
        let preview = preview("source", &original, 1_500, Vec::new());
        let events = [
            event(
                1,
                0,
                EventKind::ToolReturned {
                    call_id: "source".into(),
                    result: ToolResult::Original(original.clone()),
                },
            ),
            started(
                2,
                1,
                Item::Result {
                    call_id: "source".into(),
                    output: whole,
                },
            ),
            event(3, 1, EventKind::ContextRejected),
            started(
                4,
                2,
                Item::Result {
                    call_id: "source".into(),
                    output: preview,
                },
            ),
            returned(5, 2),
            event(
                6,
                2,
                EventKind::WindowOpened {
                    opened_at_unix_ms: 0,
                    handoff: None,
                    reason: super::super::WindowReason::Refused,
                },
            ),
        ];
        let delivery = Delivery::from_events(
            events
                .iter()
                .map(crate::engine::recipe::ReplayEvent::Admitted),
            &[],
        )
        .unwrap();
        assert!(!delivery.covers("source", None));
        assert!(!delivery.covers("source", Some(0)));
        assert!(!delivery.covers("source", Some(1)));
        assert_eq!(delivery.latest_window(), 1);
    }

    #[test]
    fn typed_history_delivery_covers_only_the_exact_current_message() {
        let text = "A🪕B".repeat(100);
        let page = super::super::history::Page {
            items: vec![super::super::history::HistoryItem::Message(
                super::super::history::Chunk {
                    id: "message".into(),
                    role: super::super::history::Role::User,
                    text: text.clone(),
                    metadata: json!({"revision":1}),
                    offset_chars: 0,
                    total_chars: text.chars().count(),
                    next_offset_chars: None,
                },
            )],
            next_cursor: None,
            tools_with_results: None,
        };
        let view = ResultView::whole_history("history-read", &page);
        let events = [
            event(
                1,
                0,
                EventKind::ToolReturned {
                    call_id: "history-read".into(),
                    result: ToolResult::History(page.into()),
                },
            ),
            started(
                2,
                1,
                Item::Result {
                    call_id: "history-read".into(),
                    output: view,
                },
            ),
            returned(3, 1),
        ];
        let delivery = Delivery::from_events(
            events
                .iter()
                .map(crate::engine::recipe::ReplayEvent::Admitted),
            &[super::super::ProductMessage {
                id: "message".into(),
                role: crate::engine::history::Role::User,
                text: text.clone(),
            }],
        )
        .unwrap();
        assert!(delivery.covers_message("message", &text, Some(0)));
        assert!(!delivery.covers_message("message", &(text + "changed"), Some(0)));
    }

    #[test]
    fn complete_result_pages_establish_the_history_messages_they_contain() {
        let text = "Original patient message 🪕".repeat(100);
        let history = super::super::history::Page {
            items: vec![super::super::history::HistoryItem::Message(
                super::super::history::Chunk {
                    id: "message".into(),
                    role: super::super::history::Role::User,
                    text: text.clone(),
                    metadata: Value::Null,
                    offset_chars: 0,
                    total_chars: text.chars().count(),
                    next_offset_chars: None,
                },
            )],
            next_cursor: None,
            tools_with_results: None,
        };
        let encoded = serde_json::to_string(&history).unwrap();
        let mut bounded = ResultView::whole_history("history", &history);
        bounded.representation = Representation::Page {
            range: ByteRange {
                start: 0,
                end: encoded.len(),
            },
        };
        let source = OriginalResult::History(history.clone());
        assert!(
            shorten_read(READ, &mut bounded, &source, 500),
            "result pages over structured history must shorten under the same bound"
        );
        let shown = materialize_original(&bounded, &source).unwrap();
        assert!(shown.chars().count() <= 500);
        let page: Page<'_> = serde_json::from_str(&shown).unwrap();
        assert_eq!(page.offset, 0);
        assert_eq!(encoded.get(..page.text.len()), Some(page.text.as_ref()));
        assert_eq!(page.next_offset, Some(page.text.len()));
        let middle = prefix_bytes(&encoded, encoded.chars().count() / 2);
        let mut events = vec![event(
            1,
            0,
            EventKind::ToolReturned {
                call_id: "history".into(),
                result: ToolResult::History(history.into()),
            },
        )];
        for (index, (start, end)) in [(0, middle), (middle, encoded.len())]
            .into_iter()
            .enumerate()
        {
            let mut view = ResultView::whole("history", &encoded);
            view.representation = Representation::Page {
                range: ByteRange { start, end },
            };
            let ordinal = index as u64 + 1;
            events.push(started(
                ordinal * 2,
                ordinal,
                Item::Result {
                    call_id: format!("page-{index}"),
                    output: view,
                },
            ));
            events.push(returned(ordinal * 2 + 1, ordinal));
            let coverage = Delivery::from_events(
                events
                    .iter()
                    .map(crate::engine::recipe::ReplayEvent::Admitted),
                &[super::super::ProductMessage {
                    id: "message".into(),
                    role: crate::engine::history::Role::User,
                    text: text.clone(),
                }],
            )
            .unwrap();
            assert_eq!(
                coverage.covers_message("message", &text, Some(0)),
                index == 1,
                "message delivery follows complete accepted coverage of the typed history result"
            );
        }
    }

    #[test]
    fn accepted_direct_message_reference_covers_the_current_authorized_text() {
        let events = [
            event(
                1,
                1,
                EventKind::ProviderStarted {
                    dispatched_at_unix_ms: 1,
                    request: SavedRequest {
                        added: vec![],
                        items: vec![RequestItem::Message {
                            id: "message".into(),
                        }],
                    },
                    context: DispatchContext::default(),
                },
            ),
            returned(2, 1),
            event(
                3,
                1,
                EventKind::WindowOpened {
                    opened_at_unix_ms: 0,
                    handoff: None,
                    reason: super::super::WindowReason::Full,
                },
            ),
        ];
        let delivery = Delivery::from_events(
            events
                .iter()
                .map(crate::engine::recipe::ReplayEvent::Admitted),
            &[],
        )
        .unwrap();
        assert!(delivery.covers_message("message", "current text", Some(0)));
        assert!(!delivery.covers_message("message", "current text", Some(1)));
    }

    #[test]
    fn projected_results_resolve_for_later_readback_without_repeating_bytes() {
        let original = "0123456789";
        let view = page("source", original, 2, 4, 1_000).unwrap();
        let events = [
            event(
                1,
                0,
                EventKind::ToolReturned {
                    call_id: "source".into(),
                    result: ToolResult::Original(original.into()),
                },
            ),
            event(
                2,
                0,
                EventKind::ToolReturned {
                    call_id: "page".into(),
                    result: ToolResult::View(view.clone()),
                },
            ),
        ];
        assert_eq!(
            resolve(
                events
                    .iter()
                    .map(crate::engine::recipe::ReplayEvent::Admitted),
                "page",
                &[]
            )
            .unwrap()
            .unwrap(),
            materialize(&view, original).unwrap()
        );
    }

    #[test]
    fn accepted_delivery_fails_when_a_typed_source_is_missing() {
        let events = [
            started(
                1,
                1,
                Item::Result {
                    call_id: "missing".into(),
                    output: ResultView::whole("missing", "not retained"),
                },
            ),
            returned(2, 1),
        ];
        assert!(matches!(
            Delivery::from_events(
                events
                    .iter()
                    .map(crate::engine::recipe::ReplayEvent::Admitted),
                &[]
            ),
            Err(Failure::Protocol(_))
        ));
    }
}
