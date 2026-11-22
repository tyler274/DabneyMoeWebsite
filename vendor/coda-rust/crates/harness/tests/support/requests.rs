use harness::engine::{Event, EventKind, RequestItem, SavedRequest, recipe::Request};

/// A self-contained dispatch fixture. Production dispatches reuse prior events.
pub fn saved(attempt: &str, sequence: u64, request: &Request) -> SavedRequest {
    SavedRequest {
        added: request.items.clone(),
        items: (0..request.items.len())
            .map(|index| RequestItem::Event {
                attempt: attempt.into(),
                sequence,
                index,
            })
            .collect(),
    }
}

pub fn dispatched(events: &[Event], frozen: &Request) -> Vec<Request> {
    events
        .iter()
        .filter_map(|event| match &event.kind {
            EventKind::ProviderStarted { request, .. } => Some(
                frozen.binding().request(
                    request
                        .restore(events, &[])
                        .unwrap()
                        .expect("saved request sources remain available"),
                ),
            ),
            _ => None,
        })
        .collect()
}
