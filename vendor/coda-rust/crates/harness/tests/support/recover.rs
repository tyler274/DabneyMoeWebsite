#![allow(
    dead_code,
    reason = "each integration suite uses its needed recovery setup"
)]
//! Integration-test entry point through the public recovery owner.
use harness::engine::{
    Event, Failure, ProductMessage, Work,
    recipe::{self, Request},
    recovery,
    recovery::{Inputs, Messages, Records, Recovered},
};

pub fn work(
    work: Work,
    attempt: &str,
    events: &[Event],
    allowance_after: u64,
) -> Result<Recovered, Failure> {
    let rows: Vec<_> = events.iter().cloned().map(|event| (event, true)).collect();
    from_rows(work, attempt, &rows, allowance_after)
}

pub fn from_rows(
    work: Work,
    attempt: &str,
    rows: &[(Event, bool)],
    allowance_after: u64,
) -> Result<Recovered, Failure> {
    work.recover(
        attempt,
        Records {
            events: rows.iter().map(Into::into).collect(),
            allowance_after,
        },
        Messages {
            sources: &[],
            partial: None,
        },
        Inputs::default(),
    )
}

/// The test owners save every event while current, so each one is admitted.
pub fn admitted(events: &[Event]) -> impl Iterator<Item = recipe::ReplayEvent<'_>> {
    events.iter().map(recipe::ReplayEvent::Admitted)
}

pub fn recovered<'a>(
    request: Request,
    attempt: &str,
    records: impl IntoIterator<Item = impl Into<recipe::ReplayEvent<'a>>>,
    allowance_after: u64,
    sources: &[ProductMessage],
    partial: Option<String>,
    inputs: recovery::Inputs,
) -> recovery::Recovered {
    request
        .freeze()
        .unwrap()
        .recover(
            attempt,
            recovery::Records {
                events: records.into_iter().map(Into::into).collect(),
                allowance_after,
            },
            recovery::Messages { sources, partial },
            inputs,
        )
        .unwrap()
}

pub fn fresh(request: Request, attempt: &str) -> recovery::Recovered {
    recovered(
        request,
        attempt,
        std::iter::empty::<recipe::ReplayEvent>(),
        0,
        &[],
        None,
        recovery::Inputs::default(),
    )
}
