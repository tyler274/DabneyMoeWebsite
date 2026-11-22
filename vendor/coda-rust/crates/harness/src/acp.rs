//! Typed JSON-RPC / NDJSON control-plane bridge (T3 Code / ACP style).
//!
//! Enables a harness run to emit structured live events over a bounded
//! [`tokio::sync::mpsc`] channel and pause for remote approve/deny decisions
//! over a [`tokio::sync::oneshot`] waiter wired to [`Cancellation`].
//!
//! Approval requests always fail closed: if the remote control plane
//! disconnects, the event channel closes, or the thread cancels before a
//! decision arrives, [`ControlPlaneBridge::request_approval`] returns an error
//! rather than implicitly approving.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};

use crate::engine::Cancellation;

/// JSON-RPC version mandated on control-plane wire envelopes.
pub const JSONRPC_VERSION: &str = "2.0";
/// JSON-RPC method name for outbound [`ControlPlaneEvent`] notifications.
pub const METHOD_EVENT: &str = "acp/event";
/// JSON-RPC method name for inbound [`ControlPlaneCommand`] requests.
pub const METHOD_COMMAND: &str = "acp/command";

/// Typed failures for control-plane wire framing, event delivery, and approvals.
#[derive(Debug, thiserror::Error)]
pub enum AcpError {
    /// Thread identifier was empty or whitespace-only.
    #[error("thread id must not be empty")]
    InvalidThreadId,
    /// Approval identifier was empty or whitespace-only.
    #[error("approval id must not be empty")]
    InvalidApprovalId,
    /// Event buffer size must be at least 1.
    #[error("event channel buffer size must be at least 1")]
    InvalidBufferSize,
    /// The remote control-plane event receiver closed.
    #[error("control-plane event channel closed")]
    ChannelClosed,
    /// An approval request failed closed because the remote waiter dropped before replying.
    #[error("approval '{approval_id}' waiter dropped before a decision arrived")]
    ApprovalChannelClosed {
        /// Identifier of the unresolved approval request.
        approval_id: String,
    },
    /// A duplicate approval identifier was requested while an identical one was pending.
    #[error("approval id '{approval_id}' is already pending")]
    DuplicateApprovalId {
        /// Conflicting approval identifier.
        approval_id: String,
    },
    /// A response referenced an approval identifier that is not currently pending.
    #[error("unknown pending approval id '{approval_id}'")]
    UnknownApprovalId {
        /// Unknown or already-settled approval identifier.
        approval_id: String,
    },
    /// A command targeted a different thread than the one bound to this bridge.
    #[error("command targeted thread '{actual}', expected '{expected}'")]
    ThreadMismatch {
        /// Thread bound to this control-plane endpoint.
        expected: String,
        /// Thread named in the incoming command.
        actual: String,
    },
    /// The thread was cancelled while emitting or awaiting approval (failing closed).
    #[error("control-plane thread cancelled")]
    Cancelled,
    /// The pending-approval registry lock was poisoned.
    #[error("control-plane approval registry lock poisoned")]
    LockPoisoned,
    /// A JSON-RPC frame carried a version other than `"2.0"`.
    #[error("unsupported jsonrpc version '{found}', expected '{JSONRPC_VERSION}'")]
    UnsupportedJsonRpcVersion {
        /// Version string observed on the wire.
        found: String,
    },
    /// An NDJSON line was empty or contained embedded line breaks.
    #[error("invalid NDJSON frame: {reason}")]
    InvalidNdjson {
        /// Exact framing violation.
        reason: &'static str,
    },
    /// JSON serialization or deserialization failed.
    #[error("control-plane JSON codec error: {source}")]
    Codec {
        /// Underlying JSON error.
        #[source]
        source: serde_json::Error,
    },
}

/// Decision supplied by a remote control plane in response to an approval request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum ApprovalDecision {
    /// Authorize the requested action.
    Approve,
    /// Refuse the requested action with an explicit reason.
    Deny {
        /// Human-readable reason for refusal.
        reason: String,
    },
}

impl ApprovalDecision {
    /// Returns true only for [`Self::Approve`].
    pub fn is_approved(&self) -> bool {
        matches!(self, Self::Approve)
    }
}

/// Outbound live events emitted by a harness run to a remote control plane.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ControlPlaneEvent {
    /// A managed thread started execution.
    ThreadStarted {
        /// Thread identifier.
        thread_id: String,
        /// Optional human-readable title.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
    },
    /// Execution progressed to a named step.
    StepUpdated {
        /// Thread identifier.
        thread_id: String,
        /// Short step label.
        step: String,
        /// Optional detail about the step.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
    /// Live incremental text produced during the thread.
    TextDelta {
        /// Thread identifier.
        thread_id: String,
        /// Incremental text chunk.
        delta: String,
    },
    /// Execution paused awaiting a remote approve/deny decision.
    ApprovalRequested {
        /// Thread identifier.
        thread_id: String,
        /// Unique identifier used to match [`ControlPlaneCommand::RespondApproval`].
        approval_id: String,
        /// Short name of the action awaiting approval.
        action: String,
        /// Structured parameters of the action shown to the approver.
        #[serde(default)]
        payload: Value,
    },
    /// The thread finished cleanly.
    ThreadFinished {
        /// Thread identifier.
        thread_id: String,
        /// Optional terminal summary.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        summary: Option<String>,
    },
    /// The thread terminated with an error.
    ThreadFailed {
        /// Thread identifier.
        thread_id: String,
        /// Error description.
        error: String,
    },
}

impl ControlPlaneEvent {
    /// Borrow the `thread_id` carried by this event.
    pub fn thread_id(&self) -> &str {
        match self {
            Self::ThreadStarted { thread_id, .. }
            | Self::StepUpdated { thread_id, .. }
            | Self::TextDelta { thread_id, .. }
            | Self::ApprovalRequested { thread_id, .. }
            | Self::ThreadFinished { thread_id, .. }
            | Self::ThreadFailed { thread_id, .. } => thread_id,
        }
    }

    /// Encode this event as a single-line NDJSON frame (without trailing newline).
    pub fn to_ndjson_line(&self) -> Result<String, AcpError> {
        encode_ndjson(self)
    }

    /// Decode a single NDJSON line into a [`ControlPlaneEvent`].
    pub fn from_ndjson_line(line: &str) -> Result<Self, AcpError> {
        decode_ndjson(line)
    }

    /// Wrap this event in a JSON-RPC 2.0 notification (`method = "acp/event"`).
    pub fn into_jsonrpc_notification(self) -> JsonRpcNotification<Self> {
        JsonRpcNotification::new(METHOD_EVENT, self)
    }
}

/// Inbound commands delivered from a remote control plane into the harness bridge.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ControlPlaneCommand {
    /// Start a new thread with an initial prompt and optional metadata.
    StartThread {
        /// Thread identifier.
        thread_id: String,
        /// Initial assignment prompt.
        prompt: String,
        /// Optional structured metadata.
        #[serde(default)]
        metadata: Value,
    },
    /// Deliver an approve/deny decision for a pending [`ControlPlaneEvent::ApprovalRequested`].
    RespondApproval {
        /// Approval identifier from [`ControlPlaneEvent::ApprovalRequested`].
        approval_id: String,
        /// Remote decision.
        decision: ApprovalDecision,
    },
    /// Cancel a running thread cooperatively.
    CancelThread {
        /// Thread identifier to cancel.
        thread_id: String,
        /// Optional cancellation reason.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
}

impl ControlPlaneCommand {
    /// Encode this command as a single-line NDJSON frame (without trailing newline).
    pub fn to_ndjson_line(&self) -> Result<String, AcpError> {
        encode_ndjson(self)
    }

    /// Decode a single NDJSON line into a [`ControlPlaneCommand`].
    pub fn from_ndjson_line(line: &str) -> Result<Self, AcpError> {
        decode_ndjson(line)
    }

    /// Wrap this command in a JSON-RPC 2.0 request (`method = "acp/command"`).
    pub fn into_jsonrpc_request(self, id: JsonRpcId) -> JsonRpcRequest<Self> {
        JsonRpcRequest::new(id, METHOD_COMMAND, self)
    }
}

/// JSON-RPC 2.0 request/response correlation identifier.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum JsonRpcId {
    /// Numeric JSON-RPC identifier.
    Number(i64),
    /// String JSON-RPC identifier.
    String(String),
}

/// JSON-RPC 2.0 notification (no `id`, no response expected).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonRpcNotification<P> {
    /// Protocol version; must be `"2.0"`.
    pub jsonrpc: String,
    /// Method name.
    pub method: String,
    /// Typed notification payload.
    pub params: P,
}

impl<P: Serialize + DeserializeOwned> JsonRpcNotification<P> {
    /// Construct a JSON-RPC 2.0 notification envelope.
    pub fn new(method: impl Into<String>, params: P) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.into(),
            method: method.into(),
            params,
        }
    }

    /// Encode as a single-line NDJSON frame.
    pub fn to_ndjson_line(&self) -> Result<String, AcpError> {
        encode_ndjson(self)
    }

    /// Decode and validate a single-line NDJSON JSON-RPC 2.0 notification.
    pub fn from_ndjson_line(line: &str) -> Result<Self, AcpError> {
        let envelope: Self = decode_ndjson(line)?;
        if envelope.jsonrpc != JSONRPC_VERSION {
            return Err(AcpError::UnsupportedJsonRpcVersion {
                found: envelope.jsonrpc,
            });
        }
        Ok(envelope)
    }
}

/// JSON-RPC 2.0 request carrying a correlation [`JsonRpcId`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonRpcRequest<P> {
    /// Protocol version; must be `"2.0"`.
    pub jsonrpc: String,
    /// Correlation identifier.
    pub id: JsonRpcId,
    /// Method name.
    pub method: String,
    /// Typed request payload.
    pub params: P,
}

impl<P: Serialize + DeserializeOwned> JsonRpcRequest<P> {
    /// Construct a JSON-RPC 2.0 request envelope.
    pub fn new(id: JsonRpcId, method: impl Into<String>, params: P) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.into(),
            id,
            method: method.into(),
            params,
        }
    }

    /// Encode as a single-line NDJSON frame.
    pub fn to_ndjson_line(&self) -> Result<String, AcpError> {
        encode_ndjson(self)
    }

    /// Decode and validate a single-line NDJSON JSON-RPC 2.0 request.
    pub fn from_ndjson_line(line: &str) -> Result<Self, AcpError> {
        let envelope: Self = decode_ndjson(line)?;
        if envelope.jsonrpc != JSONRPC_VERSION {
            return Err(AcpError::UnsupportedJsonRpcVersion {
                found: envelope.jsonrpc,
            });
        }
        Ok(envelope)
    }
}

fn encode_ndjson<T: Serialize>(value: &T) -> Result<String, AcpError> {
    let encoded = serde_json::to_string(value).map_err(|source| AcpError::Codec { source })?;
    if encoded.contains('\n') || encoded.contains('\r') {
        return Err(AcpError::InvalidNdjson {
            reason: "serialized frame contains newline characters",
        });
    }
    Ok(encoded)
}

fn decode_ndjson<T: DeserializeOwned>(line: &str) -> Result<T, AcpError> {
    let trimmed = line.strip_suffix('\n').unwrap_or(line);
    let trimmed = trimmed.strip_suffix('\r').unwrap_or(trimmed);
    if trimmed.trim().is_empty() {
        return Err(AcpError::InvalidNdjson {
            reason: "frame is empty",
        });
    }
    if trimmed.contains('\n') || trimmed.contains('\r') {
        return Err(AcpError::InvalidNdjson {
            reason: "frame contains multiple lines",
        });
    }
    serde_json::from_str(trimmed).map_err(|source| AcpError::Codec { source })
}

#[derive(Default)]
struct ApprovalRegistry {
    disconnected: bool,
    pending: HashMap<String, oneshot::Sender<ApprovalDecision>>,
}

/// Outcome of applying an inbound [`ControlPlaneCommand`] at the remote endpoint.
#[derive(Clone, Debug, PartialEq)]
pub enum CommandEffect {
    /// The remote control plane requested starting a thread; the host launches it.
    StartRequested {
        /// Thread identifier.
        thread_id: String,
        /// Initial assignment prompt.
        prompt: String,
        /// Structured metadata.
        metadata: Value,
    },
    /// A pending approval waiter was resolved with the remote decision.
    ApprovalDelivered {
        /// Resolved approval identifier.
        approval_id: String,
    },
    /// The thread's [`Cancellation`] signal was triggered.
    ThreadCancelled {
        /// Cancelled thread identifier.
        thread_id: String,
        /// Optional cancellation reason.
        reason: Option<String>,
    },
}

/// Harness-side handle for emitting live [`ControlPlaneEvent`]s and awaiting
/// remote approval decisions under cooperative [`Cancellation`].
#[derive(Clone)]
pub struct ControlPlaneBridge {
    thread_id: String,
    events_tx: mpsc::Sender<ControlPlaneEvent>,
    approvals: Arc<Mutex<ApprovalRegistry>>,
    cancel: Cancellation,
}

/// Remote control-plane endpoint paired with a [`ControlPlaneBridge`].
///
/// Dropping this endpoint disconnects the event stream and fails closed every
/// in-flight approval waiter immediately.
pub struct ControlPlaneEndpoint {
    thread_id: String,
    events_rx: mpsc::Receiver<ControlPlaneEvent>,
    approvals: Arc<Mutex<ApprovalRegistry>>,
    cancel: Cancellation,
}

/// Cloneable command handle derived from a [`ControlPlaneEndpoint`] so one task
/// can drain events while another dispatches commands.
#[derive(Clone)]
pub struct ControlPlaneCommander {
    thread_id: String,
    approvals: Arc<Mutex<ApprovalRegistry>>,
    cancel: Cancellation,
}

impl ControlPlaneBridge {
    /// Create a connected `(bridge, endpoint)` pair backed by a fresh [`Cancellation`].
    pub fn pair(
        thread_id: impl Into<String>,
        event_buffer: usize,
    ) -> Result<(Self, ControlPlaneEndpoint), AcpError> {
        Self::with_cancellation(thread_id, event_buffer, Cancellation::default())
    }

    /// Create a connected `(bridge, endpoint)` pair wired to an existing [`Cancellation`].
    pub fn with_cancellation(
        thread_id: impl Into<String>,
        event_buffer: usize,
        cancel: Cancellation,
    ) -> Result<(Self, ControlPlaneEndpoint), AcpError> {
        let thread_id = thread_id.into();
        if thread_id.trim().is_empty() {
            return Err(AcpError::InvalidThreadId);
        }
        if event_buffer == 0 {
            return Err(AcpError::InvalidBufferSize);
        }

        let (events_tx, events_rx) = mpsc::channel(event_buffer);
        let approvals = Arc::new(Mutex::new(ApprovalRegistry::default()));

        let bridge = Self {
            thread_id: thread_id.clone(),
            events_tx,
            approvals: Arc::clone(&approvals),
            cancel: cancel.clone(),
        };
        let endpoint = ControlPlaneEndpoint {
            thread_id,
            events_rx,
            approvals,
            cancel,
        };
        Ok((bridge, endpoint))
    }

    /// Borrow the bound thread identifier.
    pub fn thread_id(&self) -> &str {
        &self.thread_id
    }

    /// Borrow the shared [`Cancellation`] signal.
    pub fn cancellation(&self) -> &Cancellation {
        &self.cancel
    }

    /// Emit a live [`ControlPlaneEvent`] to the remote control plane.
    pub async fn emit(&self, event: ControlPlaneEvent) -> Result<(), AcpError> {
        if self.events_tx.send(event).await.is_err() {
            return Err(AcpError::ChannelClosed);
        }
        Ok(())
    }

    /// Emit [`ControlPlaneEvent::ThreadStarted`] for this bridge's thread.
    pub async fn emit_started(&self, title: Option<String>) -> Result<(), AcpError> {
        self.emit(ControlPlaneEvent::ThreadStarted {
            thread_id: self.thread_id.clone(),
            title,
        })
        .await
    }

    /// Emit [`ControlPlaneEvent::StepUpdated`] for this bridge's thread.
    pub async fn emit_step_updated(
        &self,
        step: impl Into<String>,
        detail: Option<String>,
    ) -> Result<(), AcpError> {
        self.emit(ControlPlaneEvent::StepUpdated {
            thread_id: self.thread_id.clone(),
            step: step.into(),
            detail,
        })
        .await
    }

    /// Emit [`ControlPlaneEvent::TextDelta`] for this bridge's thread.
    pub async fn emit_text_delta(&self, delta: impl Into<String>) -> Result<(), AcpError> {
        self.emit(ControlPlaneEvent::TextDelta {
            thread_id: self.thread_id.clone(),
            delta: delta.into(),
        })
        .await
    }

    /// Emit [`ControlPlaneEvent::ThreadFinished`] for this bridge's thread.
    pub async fn emit_finished(&self, summary: Option<String>) -> Result<(), AcpError> {
        self.emit(ControlPlaneEvent::ThreadFinished {
            thread_id: self.thread_id.clone(),
            summary,
        })
        .await
    }

    /// Emit [`ControlPlaneEvent::ThreadFailed`] for this bridge's thread.
    pub async fn emit_failed(&self, error: impl Into<String>) -> Result<(), AcpError> {
        self.emit(ControlPlaneEvent::ThreadFailed {
            thread_id: self.thread_id.clone(),
            error: error.into(),
        })
        .await
    }

    /// Emit [`ControlPlaneEvent::ApprovalRequested`] and await the remote decision.
    ///
    /// Always fails closed if:
    /// - the thread is already cancelled or cancels while waiting ([`AcpError::Cancelled`]),
    /// - the event channel is closed ([`AcpError::ChannelClosed`]), or
    /// - the remote endpoint drops before responding ([`AcpError::ApprovalChannelClosed`]).
    pub async fn request_approval(
        &self,
        approval_id: impl Into<String>,
        action: impl Into<String>,
        payload: Value,
    ) -> Result<ApprovalDecision, AcpError> {
        let approval_id = approval_id.into();
        if approval_id.trim().is_empty() {
            return Err(AcpError::InvalidApprovalId);
        }
        if self.cancel.is_cancelled() {
            return Err(AcpError::Cancelled);
        }

        let (tx, rx) = oneshot::channel();
        {
            let mut registry = lock_approvals(&self.approvals)?;
            if registry.disconnected {
                return Err(AcpError::ApprovalChannelClosed { approval_id });
            }
            if registry.pending.contains_key(&approval_id) {
                return Err(AcpError::DuplicateApprovalId { approval_id });
            }
            registry.pending.insert(approval_id.clone(), tx);
        }

        let _pending_guard = PendingApprovalGuard {
            approvals: &self.approvals,
            approval_id: approval_id.clone(),
        };

        let event = ControlPlaneEvent::ApprovalRequested {
            thread_id: self.thread_id.clone(),
            approval_id: approval_id.clone(),
            action: action.into(),
            payload,
        };
        tokio::select! {
            biased;
            () = self.cancel.cancelled() => return Err(AcpError::Cancelled),
            sent = self.emit(event) => sent?,
        }

        tokio::select! {
            biased;
            () = self.cancel.cancelled() => Err(AcpError::Cancelled),
            decision = rx => {
                match decision {
                    Ok(decision) => Ok(decision),
                    Err(_) => Err(AcpError::ApprovalChannelClosed { approval_id }),
                }
            }
        }
    }
}

struct PendingApprovalGuard<'a> {
    approvals: &'a Mutex<ApprovalRegistry>,
    approval_id: String,
}

impl Drop for PendingApprovalGuard<'_> {
    fn drop(&mut self) {
        remove_pending(self.approvals, &self.approval_id);
    }
}

impl ControlPlaneEndpoint {
    /// Borrow the bound thread identifier.
    pub fn thread_id(&self) -> &str {
        &self.thread_id
    }

    /// Receive the next live [`ControlPlaneEvent`], or `None` once all bridge clones drop.
    pub async fn next_event(&mut self) -> Option<ControlPlaneEvent> {
        self.events_rx.recv().await
    }

    /// Obtain a cloneable [`ControlPlaneCommander`] for delivering commands concurrently.
    pub fn commander(&self) -> ControlPlaneCommander {
        ControlPlaneCommander {
            thread_id: self.thread_id.clone(),
            approvals: Arc::clone(&self.approvals),
            cancel: self.cancel.clone(),
        }
    }

    /// Deliver an approve or deny decision for a pending approval request.
    pub fn respond_approval(
        &self,
        approval_id: &str,
        decision: ApprovalDecision,
    ) -> Result<(), AcpError> {
        deliver_approval(&self.approvals, approval_id, decision)
    }

    /// Cancel the bound thread via its shared [`Cancellation`] signal.
    pub fn cancel_thread(&self) {
        self.cancel.cancel();
    }

    /// Apply a typed inbound [`ControlPlaneCommand`] against this endpoint.
    pub fn apply_command(&self, command: ControlPlaneCommand) -> Result<CommandEffect, AcpError> {
        apply_command_inner(&self.thread_id, &self.approvals, &self.cancel, command)
    }
}

impl Drop for ControlPlaneEndpoint {
    fn drop(&mut self) {
        if let Ok(mut registry) = self.approvals.lock() {
            registry.disconnected = true;
            // Dropping all pending oneshot senders wakes every waiting
            // `request_approval` immediately so it fails closed.
            registry.pending.clear();
        }
    }
}

impl ControlPlaneCommander {
    /// Deliver an approve or deny decision for a pending approval request.
    pub fn respond_approval(
        &self,
        approval_id: &str,
        decision: ApprovalDecision,
    ) -> Result<(), AcpError> {
        deliver_approval(&self.approvals, approval_id, decision)
    }

    /// Cancel the bound thread via its shared [`Cancellation`] signal.
    pub fn cancel_thread(&self) {
        self.cancel.cancel();
    }

    /// Apply a typed inbound [`ControlPlaneCommand`] against this thread.
    pub fn apply_command(&self, command: ControlPlaneCommand) -> Result<CommandEffect, AcpError> {
        apply_command_inner(&self.thread_id, &self.approvals, &self.cancel, command)
    }
}

fn lock_approvals(
    approvals: &Mutex<ApprovalRegistry>,
) -> Result<std::sync::MutexGuard<'_, ApprovalRegistry>, AcpError> {
    match approvals.lock() {
        Ok(guard) => Ok(guard),
        Err(_) => Err(AcpError::LockPoisoned),
    }
}

fn remove_pending(approvals: &Mutex<ApprovalRegistry>, approval_id: &str) {
    if let Ok(mut registry) = approvals.lock() {
        registry.pending.remove(approval_id);
    }
}

fn deliver_approval(
    approvals: &Mutex<ApprovalRegistry>,
    approval_id: &str,
    decision: ApprovalDecision,
) -> Result<(), AcpError> {
    let sender = {
        let mut registry = lock_approvals(approvals)?;
        registry
            .pending
            .remove(approval_id)
            .ok_or_else(|| AcpError::UnknownApprovalId {
                approval_id: approval_id.to_owned(),
            })?
    };
    match sender.send(decision) {
        Ok(()) => Ok(()),
        Err(_) => Err(AcpError::ApprovalChannelClosed {
            approval_id: approval_id.to_owned(),
        }),
    }
}

fn apply_command_inner(
    expected_thread_id: &str,
    approvals: &Mutex<ApprovalRegistry>,
    cancel: &Cancellation,
    command: ControlPlaneCommand,
) -> Result<CommandEffect, AcpError> {
    match command {
        ControlPlaneCommand::StartThread {
            thread_id,
            prompt,
            metadata,
        } => {
            if thread_id.trim().is_empty() {
                return Err(AcpError::InvalidThreadId);
            }
            Ok(CommandEffect::StartRequested {
                thread_id,
                prompt,
                metadata,
            })
        }
        ControlPlaneCommand::RespondApproval {
            approval_id,
            decision,
        } => {
            deliver_approval(approvals, &approval_id, decision)?;
            Ok(CommandEffect::ApprovalDelivered { approval_id })
        }
        ControlPlaneCommand::CancelThread { thread_id, reason } => {
            if thread_id != expected_thread_id {
                return Err(AcpError::ThreadMismatch {
                    expected: expected_thread_id.to_owned(),
                    actual: thread_id,
                });
            }
            cancel.cancel();
            Ok(CommandEffect::ThreadCancelled { thread_id, reason })
        }
    }
}
