//! Exact request ingredients retained by the existing product run record.
//! Responses and tool receipts already own their items. A dispatch records only
//! previously unseen input or send-time projections, then refers to those facts.

use super::{Event, EventKind, Failure, Item, ProductMessage};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

/// An immutable event item or a reference to the product's authored message.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum RequestItem {
    /// An authored message is resolved under current product authority.
    Message {
        /// Stable product message identity, never its copied text.
        id: String,
    },
    /// An immutable ingredient retained in this work's event record.
    Event {
        /// Attempt that retained the item.
        attempt: String,
        /// Event sequence within that attempt.
        sequence: u64,
        /// Position in that event's retained items.
        index: usize,
    },
}

/// A dispatch's exact ordered context, referring to the product's frozen recipe
/// and earlier saved items instead of copying them into each request.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedRequest {
    /// Exact items first introduced by this dispatch, including warning,
    /// handoff, final-request and preview projections.
    pub added: Vec<Item>,
    /// Ordered references to product messages and acknowledged event items.
    pub items: Vec<RequestItem>,
}

impl SavedRequest {
    /// Resolve the exact model-facing context from the accepted work's events.
    /// Callers supply the current authorized messages and every referenced attempt.
    /// `None` means a source is no longer available and exact reconstruction is
    /// impossible. Live recovery separately builds a new request with feedback.
    pub fn restore<'a>(
        &self,
        events: impl IntoIterator<Item = &'a Event>,
        messages: &[ProductMessage],
    ) -> Result<Option<Vec<Item>>, Failure> {
        let (items, changed) = Parts::from_events(events, messages)?.restore_current(self)?;
        Ok((!changed).then_some(items))
    }
}

/// Rebuilt from the run record. This index is never another durable store.
#[derive(Default)]
pub(super) struct Parts {
    items: BTreeMap<RequestItem, Item>,
    identities: HashMap<String, RequestItem>,
    originals: HashMap<super::results::ResultSource, super::results::ToolResult>,
    messages: Vec<ProductMessage>,
}

impl Parts {
    pub(super) fn message(&self, id: &str) -> Option<&ProductMessage> {
        self.messages.iter().find(|message| message.id == id)
    }

    pub(super) fn new(messages: &[ProductMessage]) -> Self {
        Self {
            messages: messages.to_vec(),
            ..Self::default()
        }
    }

    pub(super) fn from_events<'a>(
        events: impl IntoIterator<Item = &'a Event>,
        messages: &[ProductMessage],
    ) -> Result<Self, Failure> {
        let mut parts = Self::new(messages);
        for event in events {
            parts.observe(event)?;
        }
        Ok(parts)
    }

    pub(super) fn observe(&mut self, event: &Event) -> Result<(), Failure> {
        let items = match &event.kind {
            EventKind::ProviderStarted { request, .. } => request.added.clone(),
            EventKind::ProviderReturned(turn) => turn.items.clone(),
            EventKind::ToolReturned { call_id, result } => {
                use super::results::ResultSource;
                let identity = if result.executed() {
                    ResultSource::Retained {
                        call_id: call_id.clone(),
                    }
                } else {
                    ResultSource::Refused {
                        attempt_id: event.attempt_id.clone(),
                        sequence: event.sequence,
                    }
                };
                let sources: Vec<_> = match result {
                    super::results::ToolResult::Original(_)
                    | super::results::ToolResult::Refused(_) => vec![],
                    super::results::ToolResult::History(page) => page.result_ids().collect(),
                    super::results::ToolResult::View(view) => {
                        view.retained_call_id().into_iter().collect()
                    }
                };
                if sources.iter().any(|source| {
                    !self.originals.contains_key(&ResultSource::Retained {
                        call_id: (*source).into(),
                    })
                }) {
                    return Err(Failure::Protocol(
                        "result projection refers to an unavailable original".into(),
                    ));
                }
                match self.originals.get(&identity) {
                    Some(saved) if saved != result => {
                        return Err(Failure::Protocol(
                            "tool result identity changed its original value".into(),
                        ));
                    }
                    Some(_) => (),
                    None => {
                        self.originals.insert(identity, result.clone());
                    }
                }
                return Ok(());
            }
            EventKind::ProviderObservation { .. }
            | EventKind::CallBegan { .. }
            | EventKind::ProviderFailed { .. }
            | EventKind::TransportRetry { .. }
            | EventKind::ToolStarted(_)
            | EventKind::ProviderBlocked { .. }
            | EventKind::ContextRejected
            | EventKind::Candidate { .. }
            | EventKind::WindowOpened { .. } => return Ok(()),
        };
        for (index, item) in items.into_iter().enumerate() {
            let identity = RequestItem::Event {
                attempt: event.attempt_id.clone(),
                sequence: event.sequence,
                index,
            };
            let content = serde_json::to_string(&item).map_err(Failure::Encoding)?;
            self.identities
                .entry(content)
                .or_insert_with(|| identity.clone());
            self.items.insert(identity, item);
        }
        Ok(())
    }

    /// IDs of every call this work's record holds: calls in saved responses
    /// and in the requests that carried them.
    pub(super) fn call_ids(&self) -> impl Iterator<Item = &str> {
        self.items
            .values()
            .filter_map(|item| item.call().map(|call| call.id.as_str()))
    }

    pub(super) fn save(
        &self,
        attempt: &str,
        sequence: u64,
        items: &[Item],
    ) -> Result<SavedRequest, Failure> {
        let mut added = Vec::new();
        let mut references = Vec::with_capacity(items.len());
        let mut new = HashMap::new();
        for item in items {
            if let Item::ProductMessage(message) = item {
                references.push(RequestItem::Message {
                    id: message.id.clone(),
                });
                continue;
            }
            let content = serde_json::to_string(item).map_err(Failure::Encoding)?;
            let identity = match self.identities.get(&content).or_else(|| new.get(&content)) {
                Some(identity) => identity.clone(),
                None => {
                    let identity = RequestItem::Event {
                        attempt: attempt.to_owned(),
                        sequence,
                        index: added.len(),
                    };
                    added.push(item.clone());
                    new.insert(content, identity.clone());
                    identity
                }
            };
            references.push(identity);
        }
        Ok(SavedRequest {
            added,
            items: references,
        })
    }

    pub(super) fn restore_current(
        &self,
        request: &SavedRequest,
    ) -> Result<(Vec<Item>, bool), Failure> {
        let mut items = Vec::with_capacity(request.items.len());
        let mut missing = false;
        for reference in &request.items {
            match reference {
                RequestItem::Message { id } => {
                    if let Some(message) = self.message(id) {
                        items.push(Item::ProductMessage(message.clone()));
                    } else {
                        items.push(unavailable_message());
                        missing = true;
                    }
                }
                RequestItem::Event { .. } => {
                    items.push(self.items.get(reference).cloned().ok_or_else(|| {
                        Failure::Protocol("request refers to an unavailable saved item".into())
                    })?);
                }
            }
        }
        let changed = self.resolve_current(&mut items)?;
        Ok((items, missing || changed))
    }

    pub(super) fn refresh_messages(
        &mut self,
        messages: Vec<ProductMessage>,
        items: &mut Vec<Item>,
    ) -> Result<bool, Failure> {
        self.messages = messages;
        self.resolve_current(items)
    }

    /// Live dispatch and recovery resolve authored references through one owner.
    fn resolve_current(&self, items: &mut Vec<Item>) -> Result<bool, Failure> {
        let mut changed = false;
        for item in items.iter_mut() {
            let Item::ProductMessage(message) = item else {
                continue;
            };
            match self.message(&message.id) {
                Some(current) => {
                    changed |= message != current;
                    message.clone_from(current);
                }
                None => {
                    changed = true;
                    *item = unavailable_message();
                }
            }
        }
        for item in items {
            if let Item::Result { output, .. } = item
                && output.retained_call_id().is_some()
            {
                if !self.originals.contains_key(&output.source) {
                    return Err(Failure::Protocol(
                        "result projection refers to an unavailable original".into(),
                    ));
                }
                if self.original_for(output).is_none() {
                    *output = super::results::ResultView::unavailable();
                    changed = true;
                }
            }
        }
        Ok(changed)
    }

    fn resolve_original(
        &self,
        source: &super::results::ResultSource,
    ) -> Option<super::results::OriginalResult> {
        use super::results::{OriginalResult, ToolResult};
        match self.originals.get(source)? {
            ToolResult::Original(output) => Some(OriginalResult::Text(output.clone())),
            ToolResult::History(page) => page
                .resolve(&self.messages, |id| self.resolve_result(id))
                .map(OriginalResult::History),
            ToolResult::View(view) => {
                super::results::materialize_original(view, &self.original_for(view)?)
                    .map(OriginalResult::Text)
            }
            ToolResult::Refused(refusal) => {
                Some(OriginalResult::Text(refusal.feedback().to_string()))
            }
        }
    }

    /// Resolve the retained original through the same owner used by requests.
    pub(super) fn resolve_result(&self, id: &str) -> Option<String> {
        self.resolve_original(&super::results::ResultSource::Retained { call_id: id.into() })
            .and_then(|original| original.encoded())
    }

    /// Materialize typed result projections only for the provider boundary.
    pub(super) fn for_provider(
        &self,
        request: &super::recipe::Request,
    ) -> Result<super::recipe::Request, Failure> {
        let mut request = request.clone();
        request.items = self.materialize_items(request.items)?;
        Ok(request.for_provider())
    }

    fn materialize_items(&self, mut items: Vec<Item>) -> Result<Vec<Item>, Failure> {
        for item in &mut items {
            let Item::Result { call_id, output } = item else {
                continue;
            };
            let original = self.original_for(output).ok_or_else(|| {
                Failure::Protocol("result projection refers to an unavailable original".into())
            })?;
            let wire = super::results::materialize_original(output, &original)
                .ok_or_else(|| Failure::Protocol("invalid retained result projection".into()))?;
            *item = Item::MaterializedResult {
                call_id: call_id.clone(),
                output: wire,
            };
        }
        Ok(items)
    }

    pub(super) fn rendered_len(&self, view: &super::results::ResultView) -> Result<usize, Failure> {
        self.original_for(view)
            .and_then(|original| super::results::materialize_original(view, &original))
            .map(|value| value.chars().count())
            .ok_or_else(|| Failure::Protocol("invalid retained result projection".into()))
    }

    pub(super) fn original_for(
        &self,
        view: &super::results::ResultView,
    ) -> Option<super::results::OriginalResult> {
        match &view.source {
            super::results::ResultSource::Retained { .. }
            | super::results::ResultSource::Refused { .. } => self.resolve_original(&view.source),
            super::results::ResultSource::FinishRefused { refusal } => Some(
                super::results::OriginalResult::Text(refusal.feedback().to_string()),
            ),
            super::results::ResultSource::Inline { output } => {
                Some(super::results::OriginalResult::Text(output.clone()))
            }
        }
    }

    /// Typed refusals this request carried, found through its item references.
    pub(super) fn refusals<'a>(
        &'a self,
        request: &'a SavedRequest,
    ) -> impl Iterator<Item = &'a super::recipe::Refusal> + 'a {
        use super::results::{ResultSource, ToolResult};
        request
            .items
            .iter()
            .filter_map(|reference| match self.items.get(reference)? {
                Item::FinishRefused(refusal) => Some(refusal),
                Item::Result { output, .. } => match &output.source {
                    ResultSource::FinishRefused { refusal } => Some(refusal),
                    source @ ResultSource::Refused { .. } => match self.originals.get(source)? {
                        ToolResult::Refused(refusal) => Some(refusal),
                        ToolResult::Original(_) | ToolResult::History(_) | ToolResult::View(_) => {
                            None
                        }
                    },
                    ResultSource::Retained { .. } | ResultSource::Inline { .. } => None,
                },
                Item::ProductMessage(_)
                | Item::Feedback { .. }
                | Item::OutputCutoff
                | Item::Message { .. }
                | Item::Call(_)
                | Item::MaterializedResult { .. }
                | Item::Opaque(_) => None,
            })
    }

    pub(super) fn result_views<'a>(
        &'a self,
        request: &'a SavedRequest,
    ) -> Result<
        Vec<(
            &'a super::results::ResultView,
            super::results::OriginalResult,
        )>,
        Failure,
    > {
        let mut views = Vec::new();
        for reference in &request.items {
            if matches!(reference, RequestItem::Message { .. }) {
                continue;
            }
            let item = self.items.get(reference).ok_or_else(|| {
                Failure::Protocol("request refers to an unavailable saved item".into())
            })?;
            if let Item::Result { output, .. } = item {
                if output.retained_call_id().is_some()
                    && !self.originals.contains_key(&output.source)
                {
                    return Err(Failure::Protocol(
                        "result projection refers to an unavailable original".into(),
                    ));
                }
                if let Some(original) = self.original_for(output) {
                    views.push((output, original));
                }
            }
        }
        Ok(views)
    }
}

/// The result a request gives a call that has none. The text never changes, so
/// a request keeps the same prefix as the one before it.
const INTERRUPTED: &str = r#"{"outcome":"unknown","instruction":"This call was interrupted before its result was recorded. Its outcome is unknown. Check the current state before you repeat it."}"#;

/// Match every call with one result, as Codex, OpenClaw and Hermes repair their
/// history for each request. The repaired items have the shape of a normal
/// turn: a turn's calls, then one result for each. Existing results stay where
/// they are. A call with no result gets the interrupted result after the
/// results that follow its turn, in call order. A result with no call before
/// it, or a second result for the same call, is left out. The repair is
/// deterministic, so a request built again from the same items is the same.
/// Saved records are never rewritten.
pub(super) fn paired(items: Vec<Item>) -> Vec<Item> {
    use std::collections::HashSet;
    let answered: HashSet<String> = {
        let mut called = HashSet::new();
        let mut answered = HashSet::new();
        for item in &items {
            match item {
                Item::Call(call) => {
                    called.insert(call.id.as_str());
                }
                Item::Result { call_id, .. } if called.contains(call_id.as_str()) => {
                    answered.insert(call_id.clone());
                }
                Item::Result { .. }
                | Item::ProductMessage(_)
                | Item::Feedback { .. }
                | Item::FinishRefused(_)
                | Item::OutputCutoff
                | Item::Message { .. }
                | Item::MaterializedResult { .. }
                | Item::Opaque(_) => (),
            }
        }
        answered
    };
    let interrupted = |call_id: String| Item::Result {
        call_id,
        output: super::results::ResultView::inline(INTERRUPTED.into()),
    };
    let mut called = HashSet::new();
    let mut delivered = HashSet::new();
    // The current turn's calls that have no result, and whether its results began.
    let mut unanswered = Vec::new();
    let mut in_results = false;
    let mut paired = Vec::with_capacity(items.len());
    for item in items {
        let in_turn = match &item {
            Item::Result { .. } => true,
            Item::Call(_) | Item::Opaque(_) => !in_results,
            Item::Message { role, .. } => role == "assistant" && !in_results,
            Item::ProductMessage(_)
            | Item::Feedback { .. }
            | Item::FinishRefused(_)
            | Item::OutputCutoff
            | Item::MaterializedResult { .. } => false,
        };
        if !in_turn {
            paired.extend(unanswered.drain(..).map(interrupted));
            in_results = false;
        }
        match item {
            Item::Result { ref call_id, .. }
                if !called.contains(call_id) || !delivered.insert(call_id.clone()) => {}
            Item::Result { .. } => {
                in_results = true;
                paired.push(item);
            }
            Item::Call(call) => {
                called.insert(call.id.clone());
                if !answered.contains(&call.id) {
                    unanswered.push(call.id.clone());
                }
                paired.push(Item::Call(call));
            }
            item @ (Item::ProductMessage(_)
            | Item::Feedback { .. }
            | Item::FinishRefused(_)
            | Item::OutputCutoff
            | Item::Message { .. }
            | Item::MaterializedResult { .. }
            | Item::Opaque(_)) => paired.push(item),
        }
    }
    paired.extend(unanswered.into_iter().map(interrupted));
    paired
}

// Keep source position so lead-in context still points at this unavailable message.
fn unavailable_message() -> Item {
    Item::Message {
        role: "developer".into(),
        text: "Authored message source unavailable.".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_references_preserve_identity_across_runs_and_honor_deletion() {
        let messages: Vec<_> = ["first", "second"]
            .into_iter()
            .map(|id| ProductMessage {
                id: id.into(),
                role: crate::engine::history::Role::User,
                text: "The exact same private text 🪕".into(),
            })
            .collect();
        let items: Vec<_> = messages.iter().cloned().map(Item::ProductMessage).collect();
        let first = Parts::default().save("run-one", 1, &items).unwrap();
        let second = Parts::default().save("run-two", 1, &items).unwrap();
        for request in [first, second] {
            let encoded = serde_json::to_string(&request).unwrap();
            assert!(!encoded.contains("private text"));
            let restored: SavedRequest = serde_json::from_str(&encoded).unwrap();
            assert_eq!(
                restored.items,
                vec![
                    RequestItem::Message { id: "first".into() },
                    RequestItem::Message {
                        id: "second".into()
                    }
                ]
            );
            assert_eq!(
                restored.restore([], &messages).unwrap(),
                Some(items.clone())
            );
            assert_eq!(
                restored.restore([], &messages[1..]).unwrap(),
                None,
                "deletion makes exact restoration unavailable even with identical surviving text"
            );
        }
    }
}
