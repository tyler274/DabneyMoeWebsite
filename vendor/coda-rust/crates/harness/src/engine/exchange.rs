//! Await the product's acknowledgment of each transport observation. Work can
//! lose authority or have its inputs change while the provider is silent, so
//! currency is checked in flight. Late usage survives a failed presentation fence.
use super::*;
use recipe::{CallUsage, Observer, Provider, Request, StreamEvent};
use tokio::sync::{mpsc, oneshot};

// Give cancellation a brief chance to retain final usage without waiting on a stalled callback.
const GRACE: Duration = Duration::from_secs(1);
type Receipt = oneshot::Sender<()>;
struct Sink(mpsc::Sender<(StreamEvent, Receipt)>);

#[async_trait]
impl Observer for Sink {
    async fn emit(&mut self, event: StreamEvent) -> Result<(), Cause> {
        let (receipt, ack) = oneshot::channel();
        self.0
            .send((event, receipt))
            .await
            .map_err(|_| "observation channel closed")?;
        Ok(ack.await?)
    }
}

// Once cancellation starts, draining transport and its callbacks share one
// deadline. Final retention gets a separate grace period. A drain that runs
// out ends as a cancellation; when a presentation failure started the drain,
// `dispatch` returns that failure instead.
async fn settle<T>(
    cancel: &Cancellation,
    deadline: &mut Option<Instant>,
    future: impl Future<Output = Result<T, Failure>>,
) -> Result<T, Failure> {
    tokio::pin!(future);
    let at = match *deadline {
        Some(at) => at,
        None => tokio::select! {
            biased;
            value=&mut future=>return value,
            ()=cancel.cancelled()=>*deadline.insert(Instant::now()+GRACE)
        },
    };
    tokio::time::timeout_at(at, future)
        .await
        .map_err(|_| Failure::Cancelled)?
}

fn provider_failure(cause: Cause) -> Failure {
    match cause.downcast::<Failure>() {
        Ok(failure) => *failure,
        Err(cause) => Failure::Provider(cause),
    }
}

pub(super) async fn dispatch<P: Provider + ?Sized>(
    run: &mut work::Run<'_>,
    provider: &mut P,
    request: &Request,
    context: &DispatchContext,
) -> Result<Turn, Failure> {
    let (send, mut receive) = mpsc::channel(1);
    let mut sink = Sink(send);
    let cancel = run.cancel;
    let limits = run.limits;
    let (transport, interrupt) = cancel.for_request();
    let wire_request = run.for_provider(request)?;
    let pending = provider.exchange(&wire_request, &transport, &limits, &mut sink);
    tokio::pin!(pending);
    let mut deadline = None;
    let mut call = CallUsage::new(context.usage.input_tokens);
    let mut receipt = Value::Null;
    let mut visible = false;
    let mut presentation: Option<Failure> = None;
    let prior = run.progress.usage.clone();
    // The transport may be dropped at cancellation during any callback.
    // Count available observations as they arrive. Recovery independently
    // reconstructs usage from the acknowledgments that reached the store.
    let account = |progress: &mut recipe::Progress, call: &CallUsage| {
        progress.clone_from(&prior);
        progress.account(call);
    };
    account(&mut run.progress.usage, &call);
    let interval = work::CURRENCY_INTERVAL;
    let mut tick = tokio::time::interval_at(Instant::now() + interval, interval);
    // Keep the first presentation/currentness failure authoritative even when
    // a later acknowledgment fails or exhausts the drain grace.
    let result = async {
        let result = loop {
            if cancel.is_cancelled() {
                deadline.get_or_insert_with(|| Instant::now() + GRACE);
            }
            tokio::select! {
                biased;
                () = tokio::time::sleep_until(deadline.unwrap_or_else(Instant::now)), if deadline.is_some() => {
                    break Err(Failure::Cancelled);
                }
                () = cancel.cancelled(), if deadline.is_none() => {
                    deadline = Some(Instant::now() + GRACE);
                }
                _ = tick.tick(), if presentation.is_none() => {
                    cancel.mark();
                    if let Err(reason) = settle(cancel, &mut deadline, async {run.product.assert_current().await.map_err(Failure::from)}).await {
                        interrupt.send_replace(true);
                        presentation = Some(reason);
                        deadline.get_or_insert_with(|| Instant::now() + GRACE);
                    }
                }
                Some((event,ack))=receive.recv()=>{
                    cancel.mark();
                    let kind = match event {
                        StreamEvent::Usage(reported) => {
                            call.usage.merge(&reported);
                            EventKind::ProviderObservation { usage: call.usage.clone(), receipt: receipt.clone() }
                        }
                        StreamEvent::Receipt { identity, usage: reported } => {
                            receipt = identity;
                            call.usage.merge(&reported);
                            EventKind::ProviderObservation { usage: call.usage.clone(), receipt: receipt.clone() }
                        }
                        StreamEvent::CallBegan { name } => EventKind::CallBegan { name },
                        StreamEvent::Text(delta) => {
                            if presentation.is_some() {
                                drop(ack);
                                continue;
                            }
                            let has_text = !delta.text.is_empty();
                            let delivered = settle(cancel, &mut deadline, async {
                                cancel.check()?;
                                run.product.live_text(run.progress.provider_ordinal, delta)
                                    .await.map_err(Failure::from)
                            }).await;
                            match delivered {
                                Ok(delivery) => {
                                    visible |= has_text && delivery == recipe::TextDelivery::Visible;
                                    let _ = ack.send(());
                                }
                                Err(error) => {
                                    drop(ack);
                                    interrupt.send_replace(true);
                                    presentation.get_or_insert(error);
                                    deadline.get_or_insert_with(|| Instant::now() + GRACE);
                                }
                            }
                            continue;
                        }
                    };
                    call.observe(&kind);
                    account(&mut run.progress.usage, &call);
                    settle(cancel, &mut deadline, run.record(kind)).await?;
                    let _ = ack.send(());
                }
                result=&mut pending=>break result.map_err(|cause| {
                    if visible && cause.is::<crate::responses::Transient>() {
                        return Failure::AfterOutput(cause);
                    }
                    provider_failure(cause)
                }),

            }
        };
        let mut final_deadline =
            (deadline.is_some() || cancel.is_cancelled()).then(|| Instant::now() + GRACE);
        match result {
            Ok(mut turn) => {
                run.assign_call_ids(&mut turn);
                call.usage.merge(&turn.usage);
                turn.usage = call.usage.clone();
                call.observe(&EventKind::ProviderReturned(turn.clone()));
                account(&mut run.progress.usage, &call);
                settle(
                    cancel,
                    &mut final_deadline,
                    run.record(EventKind::ProviderReturned(turn.clone())),
                )
                .await?;
                run.progress.warned |= context.warning_added;
                Ok(turn)
            }
            Err(error) => {
                settle(
                    cancel,
                    &mut final_deadline,
                    run.record(EventKind::ProviderObservation {
                        usage: call.usage.clone(),
                        receipt,
                    }),
                )
                .await?;
                let kind = if crate::responses::context_rejected(&error) {
                    EventKind::ContextRejected
                } else if crate::responses::provider_blocked(&error) {
                    EventKind::ProviderBlocked {
                        reason: error.to_string(),
                    }
                } else if error.transport_retry() {
                    EventKind::TransportRetry {
                        reason: error.to_string(),
                    }
                } else {
                    EventKind::ProviderFailed {
                        reason: error.to_string(),
                    }
                };
                settle(cancel, &mut final_deadline, run.record(kind)).await?;
                if error.transport_retry() {
                    run.progress.usage.transport_retries += 1;
                }
                Err(error)
            }
        }
    }
    .await;
    if let Some(error) = presentation {
        return Err(error);
    }
    result
}
