//! Durable working-window transitions. The product stores note contents and
//! returns the seed for a new window. A full window can reopen without a note;
//! accepted inputs and pending results remain exact.

use super::*;

pub(crate) const SAVE: &str = "save_working_note";
pub(crate) const READ: &str = "read_working_note";
pub(super) const WARNING_FIELD: &str = "run_budget_warning";

/// Notice added to the newest tool result when a run backstop is near: stop new
/// work and answer from what is established.
pub const LIMIT_WARNING: &str = "The run budget is nearly used. Stop new reading and investigation now. Give your answer from what is already established, and say plainly what you did not finish or check.";

/// The second request near the end, after the model kept working through the first.
pub fn handoff_ending() -> String {
    format!(
        "This context window is ending. Stop the task here. If the work is complete, answer now. Otherwise make one {SAVE} call with fresh_window:true now, and no other calls. Work that is not in the note will not be in the next window."
    )
}

/// The first request near the end of usable input, with the input tokens left.
pub fn handoff_ask(tokens_left: u64) -> String {
    format!(
        "This context window is nearly full: about {tokens_left} tokens of input remain. If the work is complete, answer now. Otherwise call {SAVE} with fresh_window:true, with the assignment, what is established with its source references, the decisions made and what remains. The next window will not hold this conversation."
    )
}

/// The ask for this request: the first one, or the ending one when the
/// previous request in this window already asked.
pub(crate) fn ask(repeated: bool, tokens_left: u64) -> String {
    if repeated {
        handoff_ending()
    } else {
        handoff_ask(tokens_left)
    }
}

/// Final text is retained as a handoff. The harness ignores calls on this exchange.
pub const FINAL_REQUEST: &str = "This is the final model request available in this run. Do not call tools. Give the best honest answer from completed work, including material limits and unfinished checks; do not claim missing work succeeded.";

/// A note save confirmed by the product before a fresh provider request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteReference {
    /// Invocation that saved the note.
    pub call_id: String,
    /// Product-scoped reference for reading the exact note.
    pub reference: String,
}

/// A confirmed note save with its exact text, as the run record holds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Handoff {
    pub(crate) note: NoteReference,
    text: String,
}

/// The newest note a fresh window carries, and why the window opened.
pub(crate) enum Carried<'a> {
    /// The model saved this note to ask for the fresh window.
    Asked(&'a Handoff),
    /// The window closed without a note for this change; this is the newest
    /// note saved earlier in this work.
    Earlier(&'a Handoff),
    /// No note has been saved in this work.
    Nothing,
}

impl Carried<'_> {
    fn note(&self) -> Option<&Handoff> {
        match self {
            Self::Asked(note) | Self::Earlier(note) => Some(note),
            Self::Nothing => None,
        }
    }

    /// The one developer message that opens a fresh window, then the exact note.
    fn items(&self, recipe: &recipe::Recipe, earlier: &[String]) -> Result<Vec<Item>, Failure> {
        let mut text = String::from(
            "This context window starts fresh for the same accepted work. It does not hold the earlier conversation or earlier tool results.",
        );
        if recipe.offers(history::LIST) && recipe.offers(results::READ) {
            text.push_str(&format!(
                " {} lists them and {} reads a result.",
                history::LIST,
                results::READ
            ));
        }
        if !earlier.is_empty() {
            text.push_str(&format!(
                " Earlier working notes, oldest first, are available through {READ} at these references: {}. Read them if useful; current data comes from its source.",
                serde_json::to_string(earlier).map_err(Failure::Encoding)?
            ));
        }
        if let Some(note) = self.note() {
            text.push_str(&format!(
                " The newest working note follows, saved under reference {}.",
                note.note.reference
            ));
        }
        if let Self::Earlier(_) = self {
            text.push_str(" Work done after this note was saved is not in it.");
        }
        let mut items = vec![Item::Message {
            role: "developer".into(),
            text,
        }];
        items.extend(self.note().map(|note| Item::assistant(&note.text)));
        Ok(items)
    }
}

/// The message that starts a run in a conversation whose earlier work saved
/// working notes, as Codex puts its note hint into the first context of a
/// thread. It names the notes and never carries their text.
pub(crate) fn earlier_notes(references: &[String]) -> Result<Option<Item>, Failure> {
    if references.is_empty() {
        return Ok(None);
    }
    Ok(Some(Item::Message {
        role: "developer".into(),
        text: format!(
            "Earlier work in this conversation saved working notes. Their references, oldest first: {}. If this message continues that work, read the note you need with {READ}.",
            serde_json::to_string(references).map_err(Failure::Encoding)?
        ),
    }))
}

/// Exact arguments shared by the harness and each note persistence owner.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveWorkingNote {
    /// Nonempty authored note, retained exactly.
    #[serde(deserialize_with = "note_text")]
    pub text: String,
    /// Close this window after the product confirms the save.
    pub fresh_window: bool,
}
fn note_text<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let text = String::deserialize(deserializer)?;
    if text.trim().is_empty() {
        return Err(serde::de::Error::custom(
            "working note text must not be empty",
        ));
    }
    Ok(text)
}

/// Exact read arguments; each product owns its reference identity type.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadWorkingNote<R = String> {
    /// Reference issued by the authorized product owner.
    pub reference: R,
}

/// Authorized material for a fresh window. Products supply their exact inputs
/// and conversation-wide note references; the harness renders one note pointer.
#[derive(Clone, Debug)]
pub struct WindowSeed {
    /// Product-owned context, including current accepted messages.
    pub items: Vec<Item>,
    /// Readable note identities across the authorized conversation, including earlier runs.
    pub note_references: Vec<String>,
}

/// Product-confirmed save of a working note. Both products return this receipt;
/// the harness uses its reference to carry the exact note into a fresh window.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkingNoteReceipt {
    /// Stable reference accepted by the product's note reader.
    pub reference: String,
}

pub(crate) fn saved_note(call: &Invocation, output: &str) -> Option<Handoff> {
    if call.name != SAVE {
        return None;
    }
    let arguments: SaveWorkingNote = serde_json::from_str(&call.arguments).ok()?;
    let receipt: WorkingNoteReceipt = serde_json::from_str(output).ok()?;
    if receipt.reference.trim().is_empty() {
        return None;
    }
    Some(Handoff {
        note: NoteReference {
            call_id: call.id.clone(),
            reference: receipt.reference,
        },
        text: arguments.text,
    })
}

pub(crate) fn saved_notes(events: &[&Event]) -> Vec<Handoff> {
    events
        .iter()
        .enumerate()
        .filter_map(|(index, event)| {
            let EventKind::ToolReturned { call_id, result } = &event.kind else {
                return None;
            };
            let call = events[..index]
                .iter()
                .rev()
                .find_map(|event| match &event.kind {
                    EventKind::ToolStarted(call) if &call.id == call_id => Some(call),
                    EventKind::ToolStarted(_)
                    | EventKind::ProviderStarted { .. }
                    | EventKind::ProviderObservation { .. }
                    | EventKind::CallBegan { .. }
                    | EventKind::ProviderFailed { .. }
                    | EventKind::TransportRetry { .. }
                    | EventKind::ProviderReturned(_)
                    | EventKind::ToolReturned { .. }
                    | EventKind::ProviderBlocked { .. }
                    | EventKind::ContextRejected
                    | EventKind::Candidate { .. }
                    | EventKind::WindowOpened { .. } => None,
                })?;
            saved_note(call, result.original()?)
        })
        .collect()
}

/// Offer working notes: add the note tools and the guidance that says how to
/// use them. Products call this once, after adding their other tools, so the
/// guidance names the history tools only when they are offered.
pub fn offer_working_notes(instructions: &mut String, tools: &mut Vec<Value>) {
    let offers = |name: &str| tools.iter().any(|tool| tool["name"] == name);
    let recovers = offers(history::LIST) && offers(history::SEARCH) && offers(results::READ);
    tools.extend(note_tools().iter().map(Tool::function));
    instructions.push_str("\n\n");
    instructions.push_str(&guidance(recovers));
}

/// Up-front guidance for work that can outlast one window, as Codex gives it.
fn guidance(recovers: bool) -> String {
    let recover = if recovers {
        format!(
            ", then use {}, {} and {} to recover exact earlier results instead of repeating work",
            history::LIST,
            history::SEARCH,
            results::READ
        )
    } else {
        String::new()
    };
    format!(
        "This work can outlast one context window. Keep a working note with {SAVE} as you go: a complete, concise checkpoint of the assignment, what is established with its source references, the decisions made and what remains. Save again whenever progress is worth keeping; the newest note is the checkpoint. When the context window runs out, it is lost. The next window holds the request, any results still pending and the newest note, not this conversation, so do not work long without a saved note. To continue in a fresh window yourself, save with fresh_window:true. In a fresh window, read the note first{recover}. Notes and history are private working tools, not part of the answer."
    )
}

/// Shared working-note capabilities. Products persist and authorize both calls.
fn note_tools() -> [Tool; 2] {
    [
        Tool {
            name: SAVE.into(),
            description: "Save a complete private working note for this accepted work: a checkpoint of the assignment, what is established with its source references, the decisions made and what remains. The newest note is carried into the next context window. Set fresh_window:true to continue in a fresh window after this save. The note is not a user-facing answer or clinical evidence.".into(),
            parameters: serde_json::json!({"type":"object","properties":{"text":{"type":"string","minLength":1},"fresh_window":{"type":"boolean"}},"required":["text","fresh_window"],"additionalProperties":false}),
        },
        Tool {
            name: READ.into(),
            description: "Open an exact saved working note by its product-scoped reference under current authorization.".into(),
            parameters: serde_json::json!({"type":"object","properties":{"reference":{"type":"string","minLength":1}},"required":["reference"],"additionalProperties":false}),
        },
    ]
}

impl DispatchContext {
    /// Remove only this dispatch's own projections before appending new
    /// evidence. Product-authored content is never inspected for wording.
    pub(super) fn restore(&self, items: &mut Vec<Item>) -> Result<(), Failure> {
        match self.purpose {
            RequestPurpose::Work => (),
            RequestPurpose::Handoff | RequestPurpose::Final { .. } => match items.pop() {
                Some(Item::Message { role, .. }) if role == "developer" => (),
                None
                | Some(
                    Item::Message { .. }
                    | Item::ProductMessage(_)
                    | Item::Feedback { .. }
                    | Item::FinishRefused(_)
                    | Item::OutputCutoff
                    | Item::Call(_)
                    | Item::Result { .. }
                    | Item::MaterializedResult { .. }
                    | Item::Opaque(_),
                ) => {
                    return Err(Failure::Protocol(
                        "saved dispatch lacks its instruction".into(),
                    ));
                }
            },
        }
        Ok(())
    }
}

pub(crate) fn append_warning(items: &mut [Item]) -> bool {
    let Some(output) = items.iter_mut().rev().find_map(|item| match item {
        Item::Result { output, .. } => Some(output),
        Item::ProductMessage(_)
        | Item::Feedback { .. }
        | Item::FinishRefused(_)
        | Item::OutputCutoff
        | Item::Message { .. }
        | Item::Call(_)
        | Item::MaterializedResult { .. }
        | Item::Opaque(_) => None,
    }) else {
        return false;
    };
    output.annotate(WARNING_FIELD, LIMIT_WARNING);
    true
}

/// A confirmed save that asks for a fresh window, in the newest complete tool
/// batch, closes the old window. The new one carries the newest confirmed note,
/// which can be a later save in the same batch. Sibling results remain
/// available in the new window so effects are never replayed.
pub(crate) fn handoff<'a>(
    items: &[Item],
    notes: &'a [Handoff],
    parts: &super::request_parts::Parts,
) -> Result<Option<(&'a Handoff, Vec<Item>)>, Failure> {
    // New accepted direction or feedback after a save means that note was
    // not written for this boundary. Reopening still retains its receipts.
    if !matches!(items.last(), Some(Item::Result { .. })) {
        return Ok(None);
    }
    let batch = pending_results(items)?.results;
    let asked = batch.as_chunks::<2>().0.iter().any(|pair| {
        let (Item::Call(call), Item::Result { output, .. }) = (&pair[0], &pair[1]) else {
            unreachable!()
        };
        call.name == SAVE
            && serde_json::from_str::<SaveWorkingNote>(&call.arguments)
                .is_ok_and(|args| args.fresh_window)
            && parts
                .original_for(output)
                .and_then(|value| saved_note(call, value.text()?))
                .is_some()
    });
    Ok(asked
        .then(|| notes.last().map(|newest| (newest, batch)))
        .flatten())
}

/// Pending receipts and exact harness feedback share a window boundary.
pub(crate) struct WindowTail {
    results: Vec<Item>,
    feedback: Vec<Item>,
}

pub(crate) fn pending_results(items: &[Item]) -> Result<WindowTail, Failure> {
    let end = items
        .iter()
        .rposition(|item| matches!(item, Item::Result { .. }))
        .map_or(0, |index| index + 1);
    let feedback = items[end..]
        .iter()
        .filter(|item| match item {
            Item::Feedback { .. } | Item::FinishRefused(_) => true,
            Item::ProductMessage(_)
            | Item::OutputCutoff
            | Item::Message { .. }
            | Item::Call(_)
            | Item::Result { .. }
            | Item::MaterializedResult { .. }
            | Item::Opaque(_) => false,
        })
        .cloned()
        .collect();
    let start = items[..end]
        .iter()
        .rposition(|item| !matches!(item, Item::Result { .. }))
        .map_or(0, |index| index + 1);
    let mut results = Vec::new();
    for result in &items[start..end] {
        let Item::Result { call_id, .. } = result else {
            unreachable!()
        };
        let call = items[..start]
            .iter()
            .rev()
            .find_map(|item| item.call().filter(|call| &call.id == call_id))
            .ok_or_else(|| Failure::Protocol("tool receipt without its invocation".into()))?;
        results.push(Item::Call(call.clone()));
        results.push(result.clone());
    }
    Ok(WindowTail { results, feedback })
}

pub(crate) async fn rotate(
    product: &mut dyn recipe::Product,
    recipe: &recipe::Recipe,
    items: &[Item],
    notes: &[Handoff],
    parts: &super::request_parts::Parts,
) -> Result<Option<(Handoff, Vec<Item>)>, Failure> {
    let Some((newest, batch)) = handoff(items, notes, parts)? else {
        return Ok(None);
    };
    // The batch's saves are left out by the seed: the newest note is carried
    // and the others are references.
    let items = seed(
        product,
        recipe,
        Carried::Asked(newest),
        notes,
        WindowTail {
            results: batch,
            feedback: Vec::new(),
        },
        parts,
    )
    .await?;
    Ok(Some((newest.clone(), items)))
}

pub(crate) async fn seed(
    product: &mut dyn recipe::Product,
    recipe: &recipe::Recipe,
    carried: Carried<'_>,
    notes: &[Handoff],
    pending: WindowTail,
    parts: &super::request_parts::Parts,
) -> Result<Vec<Item>, Failure> {
    let WindowSeed {
        items: mut seed,
        note_references,
    } = product.window_seed().await.map_err(Failure::from)?;
    // Products list notes in the order saved; this work's own saves follow.
    // The carried note is inline, so only the others are references.
    let newest = carried.note().map(|note| note.note.reference.as_str());
    let mut earlier: Vec<String> = Vec::new();
    for reference in note_references
        .into_iter()
        .chain(notes.iter().map(|note| note.note.reference.clone()))
    {
        if Some(reference.as_str()) != newest && !earlier.contains(&reference) {
            earlier.push(reference);
        }
    }
    seed.extend(carried.items(recipe, &earlier)?);
    let mut calls = Vec::new();
    let mut receipts = Vec::new();
    for pair in pending.results.as_chunks::<2>().0 {
        // A successful save is represented by the carried note or an earlier-note
        // pointer; carrying its invocation would also copy the old note text.
        if let (Item::Call(call), Item::Result { output, .. }) = (&pair[0], &pair[1])
            && parts
                .original_for(output)
                .and_then(|value| saved_note(call, value.text()?))
                .is_some()
        {
            continue;
        }
        calls.push(pair[0].clone());
        receipts.push(pair[1].clone());
    }
    // Preserve the complete batch shape so a second refused window retains
    // every pending receipt, not just the last call/result pair.
    seed.extend(calls);
    seed.extend(receipts);
    seed.extend(pending.feedback);
    window_items(seed)
}
