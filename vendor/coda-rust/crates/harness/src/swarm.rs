//! Typed multi-agent workflow orchestration.
//!
//! Provides composable, product-neutral orchestration primitives built on the
//! harness [`Cancellation`] signal:
//! - [`SequentialWorkflow`] pipelines outputs from one [`AgentMember`] to the next.
//! - [`ConcurrentWorkflow`] fans out via [`tokio::task::JoinSet`], propagates
//!   cancellation, and preserves every per-member result or failure in
//!   deterministic registration order.
//! - [`HierarchicalWorkflow`] asks a typed [`Router`] for a [`RoutingPlan`],
//!   runs the selected specialists concurrently, and passes their outputs to a
//!   synthesizer member.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::Arc,
};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::task::JoinSet;

use crate::engine::Cancellation;

/// Typed errors produced by swarm construction and execution.
///
/// Causes are always retained so callers and hosts never lose the underlying
/// member, router, or task failure.
#[derive(Debug, thiserror::Error)]
pub enum SwarmError {
    /// Execution stopped because the shared [`Cancellation`] signal fired.
    #[error("swarm execution cancelled")]
    Cancelled,
    /// A workflow was constructed with no executable members.
    #[error("{workflow} workflow requires at least one member")]
    EmptyWorkflow {
        /// Workflow kind that was empty.
        workflow: &'static str,
    },
    /// A member name was empty or whitespace-only.
    #[error("invalid member name: {reason}")]
    InvalidMemberName {
        /// Exact validation rule violated.
        reason: &'static str,
    },
    /// Two members or routing entries shared the same name where unique names are required.
    #[error("duplicate member name: {name}")]
    DuplicateMember {
        /// Conflicting member name.
        name: String,
    },
    /// A [`RoutingPlan`] named a specialist that is not registered on the workflow.
    #[error("unknown specialist in routing plan: {name}")]
    UnknownMember {
        /// Unregistered specialist name.
        name: String,
    },
    /// A [`RoutingPlan`] selected zero specialists.
    #[error("routing plan must select at least one specialist")]
    EmptyRoutingPlan,
    /// An [`AgentMember`] returned an error.
    #[error("member '{member}' failed: {source}")]
    MemberFailed {
        /// Name of the failing member.
        member: String,
        /// Original member failure cause.
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    /// A [`Router`] failed before specialists could run.
    #[error("router failed: {source}")]
    RouterFailed {
        /// Original router failure cause.
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    /// A spawned concurrent member task panicked or failed to join.
    #[error("member '{member}' task failed to join: {source}")]
    TaskJoinFailed {
        /// Name of the member whose task failed.
        member: String,
        /// Underlying Tokio join error.
        #[source]
        source: tokio::task::JoinError,
    },
}

impl SwarmError {
    /// Wrap a member failure while preserving its original error source.
    pub fn member_failed(
        member: impl Into<String>,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self::MemberFailed {
            member: member.into(),
            source: Box::new(source),
        }
    }

    /// Wrap a router failure while preserving its original error source.
    pub fn router_failed(source: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self::RouterFailed {
            source: Box::new(source),
        }
    }
}

/// Output captured from an earlier member and forwarded as structured context.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MemberArtifact {
    /// Name of the member that produced this artifact.
    pub member: String,
    /// Textual output produced by that member.
    pub content: String,
    /// Structured metadata attached by that member.
    #[serde(default)]
    pub metadata: Value,
}

impl From<&MemberOutput> for MemberArtifact {
    fn from(output: &MemberOutput) -> Self {
        Self {
            member: output.member.clone(),
            content: output.content.clone(),
            metadata: output.metadata.clone(),
        }
    }
}

/// Input handed to an [`AgentMember`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MemberInput {
    /// Primary assignment or prompt for this step.
    pub prompt: String,
    /// Ordered artifacts from upstream members.
    #[serde(default)]
    pub context: Vec<MemberArtifact>,
    /// Caller-supplied structured metadata preserved across steps.
    #[serde(default)]
    pub metadata: Value,
}

impl MemberInput {
    /// Create a fresh input with a prompt and empty context/metadata.
    pub fn new(prompt: impl Into<String>) -> Self {
        Self {
            prompt: prompt.into(),
            context: Vec::new(),
            metadata: Value::Null,
        }
    }

    /// Attach upstream artifacts to this input.
    pub fn with_context(mut self, context: Vec<MemberArtifact>) -> Self {
        self.context = context;
        self
    }

    /// Attach structured metadata to this input.
    pub fn with_metadata(mut self, metadata: Value) -> Self {
        self.metadata = metadata;
        self
    }
}

/// Output returned by an [`AgentMember`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MemberOutput {
    /// Name of the member that produced this output.
    pub member: String,
    /// Produced output text.
    pub content: String,
    /// Optional structured metadata for downstream members or callers.
    #[serde(default)]
    pub metadata: Value,
}

impl MemberOutput {
    /// Create a member output with null metadata.
    pub fn new(member: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            member: member.into(),
            content: content.into(),
            metadata: Value::Null,
        }
    }

    /// Attach structured metadata to this output.
    pub fn with_metadata(mut self, metadata: Value) -> Self {
        self.metadata = metadata;
        self
    }
}

/// One executable member in a typed swarm workflow.
#[async_trait]
pub trait AgentMember: Send + Sync {
    /// Stable non-empty identifier for this member.
    fn name(&self) -> &str;

    /// Run this member on `input`, honoring cooperative cancellation through `cancel`.
    async fn run(
        &self,
        input: MemberInput,
        cancel: &Cancellation,
    ) -> Result<MemberOutput, SwarmError>;
}

fn validate_member_name(name: &str) -> Result<(), SwarmError> {
    if name.trim().is_empty() {
        return Err(SwarmError::InvalidMemberName {
            reason: "member name must not be empty or whitespace",
        });
    }
    Ok(())
}

async fn run_member_cancellable(
    member: &dyn AgentMember,
    input: MemberInput,
    cancel: &Cancellation,
) -> Result<MemberOutput, SwarmError> {
    if cancel.is_cancelled() {
        return Err(SwarmError::Cancelled);
    }
    tokio::select! {
        biased;
        () = cancel.cancelled() => Err(SwarmError::Cancelled),
        result = member.run(input, cancel) => {
            if cancel.is_cancelled() {
                Err(SwarmError::Cancelled)
            } else {
                result
            }
        }
    }
}

/// Result of a completed [`SequentialWorkflow`].
#[derive(Clone, Debug, PartialEq)]
pub struct SequentialOutput {
    /// Output of every member in execution order.
    pub steps: Vec<MemberOutput>,
    /// Output of the last member in the pipeline.
    pub final_output: MemberOutput,
}

/// Executes members strictly in order, feeding each output into the next member.
pub struct SequentialWorkflow {
    members: Vec<Arc<dyn AgentMember>>,
}

impl SequentialWorkflow {
    /// Construct a non-empty sequential pipeline.
    pub fn new(members: Vec<Arc<dyn AgentMember>>) -> Result<Self, SwarmError> {
        if members.is_empty() {
            return Err(SwarmError::EmptyWorkflow {
                workflow: "sequential",
            });
        }
        for member in &members {
            validate_member_name(member.name())?;
        }
        Ok(Self { members })
    }

    /// Execute every member in sequence, stopping immediately on error or cancellation.
    pub async fn run(
        &self,
        initial: MemberInput,
        cancel: &Cancellation,
    ) -> Result<SequentialOutput, SwarmError> {
        let mut steps = Vec::with_capacity(self.members.len());
        let mut current = initial;

        for member in &self.members {
            let output = run_member_cancellable(member.as_ref(), current.clone(), cancel).await?;
            let artifact = MemberArtifact::from(&output);
            current.prompt = output.content.clone();
            current.context.push(artifact);
            steps.push(output);
        }

        let final_output = steps.last().cloned().ok_or(SwarmError::EmptyWorkflow {
            workflow: "sequential",
        })?;
        Ok(SequentialOutput {
            steps,
            final_output,
        })
    }
}

/// Outcome of one member inside a [`ConcurrentWorkflow`].
#[derive(Debug)]
pub struct ConcurrentOutcome {
    /// Registered name of the member.
    pub member: String,
    /// Per-member output or preserved error.
    pub result: Result<MemberOutput, SwarmError>,
}

/// Aggregated outcomes of a [`ConcurrentWorkflow`], in registration order.
#[derive(Debug)]
pub struct ConcurrentOutput {
    /// One outcome per registered member, matching construction order.
    pub outcomes: Vec<ConcurrentOutcome>,
}

impl ConcurrentOutput {
    /// Borrow all successful member outputs in registration order.
    pub fn successes(&self) -> Vec<&MemberOutput> {
        self.outcomes
            .iter()
            .filter_map(|outcome| outcome.result.as_ref().ok())
            .collect()
    }

    /// Borrow all failed members and their errors in registration order.
    pub fn failures(&self) -> Vec<(&str, &SwarmError)> {
        self.outcomes
            .iter()
            .filter_map(|outcome| {
                outcome
                    .result
                    .as_ref()
                    .err()
                    .map(|error| (outcome.member.as_str(), error))
            })
            .collect()
    }

    /// Returns true when every concurrent member succeeded.
    pub fn all_succeeded(&self) -> bool {
        self.outcomes.iter().all(|outcome| outcome.result.is_ok())
    }
}

/// Executes members concurrently on a [`tokio::task::JoinSet`] with cancellation
/// propagation while preserving every member's individual result or failure.
pub struct ConcurrentWorkflow {
    members: Vec<Arc<dyn AgentMember>>,
}

impl ConcurrentWorkflow {
    /// Construct a non-empty concurrent fan-out workflow with unique member names.
    pub fn new(members: Vec<Arc<dyn AgentMember>>) -> Result<Self, SwarmError> {
        if members.is_empty() {
            return Err(SwarmError::EmptyWorkflow {
                workflow: "concurrent",
            });
        }
        let mut seen = HashSet::with_capacity(members.len());
        for member in &members {
            let name = member.name();
            validate_member_name(name)?;
            if !seen.insert(name.to_owned()) {
                return Err(SwarmError::DuplicateMember {
                    name: name.to_owned(),
                });
            }
        }
        Ok(Self { members })
    }

    /// Fan out `input` to all members concurrently and collect every outcome in order.
    ///
    /// If `cancel` fires, all in-flight tasks are aborted and [`SwarmError::Cancelled`]
    /// is returned. Otherwise, individual member failures are preserved inside
    /// [`ConcurrentOutput::outcomes`] rather than discarding sibling successes.
    pub async fn run(
        &self,
        input: MemberInput,
        cancel: &Cancellation,
    ) -> Result<ConcurrentOutput, SwarmError> {
        if cancel.is_cancelled() {
            return Err(SwarmError::Cancelled);
        }

        let mut join_set: JoinSet<(usize, String, Result<MemberOutput, SwarmError>)> =
            JoinSet::new();
        let mut task_owners: HashMap<tokio::task::Id, (usize, String)> =
            HashMap::with_capacity(self.members.len());

        for (index, member) in self.members.iter().enumerate() {
            let member_handle = Arc::clone(member);
            let owner_name = member.name().to_owned();
            let member_name = owner_name.clone();
            let member_input = input.clone();
            let member_cancel = cancel.clone();
            let abort_handle = join_set.spawn(async move {
                let result =
                    run_member_cancellable(member_handle.as_ref(), member_input, &member_cancel)
                        .await;
                (index, member_name, result)
            });
            task_owners.insert(abort_handle.id(), (index, owner_name));
        }

        let mut slots: Vec<Option<ConcurrentOutcome>> =
            (0..self.members.len()).map(|_| None).collect();

        loop {
            let next = tokio::select! {
                biased;
                () = cancel.cancelled() => {
                    join_set.abort_all();
                    while join_set.join_next().await.is_some() {}
                    return Err(SwarmError::Cancelled);
                }
                joined = join_set.join_next() => joined,
            };

            let Some(joined) = next else {
                break;
            };

            match joined {
                Ok((index, member, result)) => {
                    slots[index] = Some(ConcurrentOutcome { member, result });
                }
                Err(join_error) => {
                    let task_id = join_error.id();
                    if let Some((index, member)) = task_owners.remove(&task_id) {
                        slots[index] = Some(ConcurrentOutcome {
                            member: member.clone(),
                            result: Err(SwarmError::TaskJoinFailed {
                                member,
                                source: join_error,
                            }),
                        });
                    } else {
                        return Err(SwarmError::TaskJoinFailed {
                            member: "unknown".into(),
                            source: join_error,
                        });
                    }
                }
            }
        }

        if cancel.is_cancelled() {
            return Err(SwarmError::Cancelled);
        }

        let outcomes = slots
            .into_iter()
            .enumerate()
            .map(|(index, slot)| {
                slot.unwrap_or_else(|| ConcurrentOutcome {
                    member: self.members[index].name().to_owned(),
                    result: Err(SwarmError::Cancelled),
                })
            })
            .collect();

        Ok(ConcurrentOutput { outcomes })
    }
}

/// Typed plan emitted by a [`Router`] naming the specialists to invoke.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutingPlan {
    /// Ordered names of selected specialists.
    pub selected_specialists: Vec<String>,
    /// Optional human-readable rationale for the selection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rationale: Option<String>,
}

impl RoutingPlan {
    /// Construct a validated non-empty routing plan with unique specialist names.
    pub fn new(selected_specialists: Vec<String>) -> Result<Self, SwarmError> {
        if selected_specialists.is_empty() {
            return Err(SwarmError::EmptyRoutingPlan);
        }
        let mut seen = HashSet::with_capacity(selected_specialists.len());
        for name in &selected_specialists {
            validate_member_name(name)?;
            if !seen.insert(name.clone()) {
                return Err(SwarmError::DuplicateMember { name: name.clone() });
            }
        }
        Ok(Self {
            selected_specialists,
            rationale: None,
        })
    }

    /// Attach a routing rationale to this plan.
    pub fn with_rationale(mut self, rationale: impl Into<String>) -> Self {
        self.rationale = Some(rationale.into());
        self
    }
}

/// Decides which registered specialists should handle a given [`MemberInput`].
#[async_trait]
pub trait Router: Send + Sync {
    /// Produce a [`RoutingPlan`] selecting from `available_specialists`.
    async fn route(
        &self,
        input: &MemberInput,
        available_specialists: &[String],
        cancel: &Cancellation,
    ) -> Result<RoutingPlan, SwarmError>;
}

/// Output of a completed [`HierarchicalWorkflow`].
#[derive(Clone, Debug, PartialEq)]
pub struct HierarchicalOutput {
    /// Plan chosen by the router.
    pub plan: RoutingPlan,
    /// Outputs produced by the selected specialists, in plan order.
    pub specialist_outputs: Vec<MemberOutput>,
    /// Final synthesized output produced from the specialist artifacts.
    pub synthesis: MemberOutput,
}

/// Coordinates a [`Router`], a registry of specialist [`AgentMember`]s, and a
/// synthesizer [`AgentMember`].
pub struct HierarchicalWorkflow {
    router: Arc<dyn Router>,
    specialists: BTreeMap<String, Arc<dyn AgentMember>>,
    synthesizer: Arc<dyn AgentMember>,
}

impl HierarchicalWorkflow {
    /// Construct a hierarchical workflow with a router, unique specialists, and a synthesizer.
    pub fn new(
        router: Arc<dyn Router>,
        specialists: Vec<Arc<dyn AgentMember>>,
        synthesizer: Arc<dyn AgentMember>,
    ) -> Result<Self, SwarmError> {
        if specialists.is_empty() {
            return Err(SwarmError::EmptyWorkflow {
                workflow: "hierarchical",
            });
        }
        validate_member_name(synthesizer.name())?;
        let mut registry = BTreeMap::new();
        for specialist in specialists {
            let name = specialist.name().to_owned();
            validate_member_name(&name)?;
            if registry.insert(name.clone(), specialist).is_some() {
                return Err(SwarmError::DuplicateMember { name });
            }
        }
        Ok(Self {
            router,
            specialists: registry,
            synthesizer,
        })
    }

    /// Route `input`, run the chosen specialists concurrently, and synthesize their outputs.
    ///
    /// Failures from the router, unknown specialist names, specialist execution
    /// errors, or synthesizer failures are propagated with their original cause
    /// intact; no failure is swallowed.
    pub async fn run(
        &self,
        input: MemberInput,
        cancel: &Cancellation,
    ) -> Result<HierarchicalOutput, SwarmError> {
        if cancel.is_cancelled() {
            return Err(SwarmError::Cancelled);
        }

        let available: Vec<String> = self.specialists.keys().cloned().collect();
        let plan = tokio::select! {
            biased;
            () = cancel.cancelled() => return Err(SwarmError::Cancelled),
            routed = self.router.route(&input, &available, cancel) => routed?,
        };

        if plan.selected_specialists.is_empty() {
            return Err(SwarmError::EmptyRoutingPlan);
        }

        let mut selected = Vec::with_capacity(plan.selected_specialists.len());
        let mut seen = HashSet::with_capacity(plan.selected_specialists.len());
        for name in &plan.selected_specialists {
            validate_member_name(name)?;
            if !seen.insert(name.as_str()) {
                return Err(SwarmError::DuplicateMember { name: name.clone() });
            }
            let specialist = self
                .specialists
                .get(name)
                .ok_or_else(|| SwarmError::UnknownMember { name: name.clone() })?;
            selected.push(Arc::clone(specialist));
        }

        let fanout = ConcurrentWorkflow::new(selected)?;
        let concurrent_output = fanout.run(input.clone(), cancel).await?;

        let mut specialist_outputs = Vec::with_capacity(concurrent_output.outcomes.len());
        for outcome in concurrent_output.outcomes {
            specialist_outputs.push(outcome.result?);
        }

        let mut synth_context = input.context;
        synth_context.extend(specialist_outputs.iter().map(MemberArtifact::from));
        let synth_input = MemberInput {
            prompt: input.prompt,
            context: synth_context,
            metadata: input.metadata,
        };

        let synthesis =
            run_member_cancellable(self.synthesizer.as_ref(), synth_input, cancel).await?;

        Ok(HierarchicalOutput {
            plan,
            specialist_outputs,
            synthesis,
        })
    }
}
