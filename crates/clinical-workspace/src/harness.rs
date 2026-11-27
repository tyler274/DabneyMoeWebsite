//! Pure-Rust bounded agentic chat harness for the Dabney.moe clinical workspace.
//!
//! Coordinates multi-step, tool-calling clinical reasoning strictly scoped to a
//! single synthetic patient chart:
//! - Typed tool vocabulary (`AgentTool`) with explicit JSON Schemas
//! - Deterministic bounded execution loop (`RunBudget`, explicit `Stopped` & `CutOff` outcomes)
//! - Ordered event stream (`AgentEvent`) with W3C Server-Sent Events serialization
//! - Multi-turn conversation threads (`AgentThread`) with working-note checkpoints
//! - Honest zero-evidence refusal (`HarnessOutcome::NoMatchingEvidence`) whenever no tool finds matching patient data
//! - Grounded prompt builder (`build_harness_llm_prompt`) for optional on-device LLM synthesis (Android AICore / Gemini Nano)
//!
//! Compiles cleanly on `x86_64` Linux/FreeBSD (SSR) and `wasm32-unknown-unknown` (hydrate/CSR)
//! with zero unsafe code and zero external runtime dependencies.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{
    mentions, query_terms, relevant_excerpt, same_word, term_weight, ChartRecord, ChatCitation,
    FollowUpStanding, PatientChart, RecordKind,
};

/// Minimum allowed step budget for a single harness run.
pub const MIN_RUN_STEPS: usize = 1;
/// Default step budget for a single harness run.
pub const DEFAULT_RUN_STEPS: usize = 6;
/// Maximum allowed step budget for a single harness run.
pub const MAX_RUN_STEPS: usize = 10;

/// Intended audience for a clinical agentic chat run.
///
/// Controls tool availability (clinician-only reply drafts stay hidden in
/// `Patient` mode) and response framing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentAudience {
    #[default]
    Physician,
    Patient,
}

impl AgentAudience {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Physician => "physician",
            Self::Patient => "patient",
        }
    }
}

/// Explicit per-turn execution limits enforced by the harness loop.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunBudget {
    pub max_steps: usize,
    pub max_tool_calls: usize,
    pub max_citations: usize,
}

impl Default for RunBudget {
    fn default() -> Self {
        Self {
            max_steps: DEFAULT_RUN_STEPS,
            max_tool_calls: DEFAULT_RUN_STEPS,
            max_citations: 4,
        }
    }
}

impl RunBudget {
    /// Clamps caller-supplied limits into safe bounded ranges.
    #[must_use]
    pub fn clamped(self) -> Self {
        let max_steps = self.max_steps.clamp(MIN_RUN_STEPS, MAX_RUN_STEPS);
        let max_tool_calls = self.max_tool_calls.clamp(MIN_RUN_STEPS, MAX_RUN_STEPS);
        let max_citations = self.max_citations.clamp(1, 6);
        Self {
            max_steps,
            max_tool_calls,
            max_citations,
        }
    }
}

/// Typed tool invocation planned and executed by the clinical harness.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "tool", rename_all = "snake_case")]
pub enum AgentTool {
    ChartSearch {
        query: String,
        kind_filter: Option<RecordKind>,
    },
    ChartRead {
        record_ids: Vec<String>,
    },
    MedicationReview {
        query_terms: Vec<String>,
    },
    ProblemListReview {
        query_terms: Vec<String>,
    },
    FollowUpAndVisitReview {
        include_reply_draft: bool,
    },
    SaveWorkingNote {
        summary: String,
        cited_record_ids: Vec<String>,
    },
}

impl AgentTool {
    pub const fn name(&self) -> &'static str {
        match self {
            Self::ChartSearch { .. } => "chart_search",
            Self::ChartRead { .. } => "chart_read",
            Self::MedicationReview { .. } => "medication_review",
            Self::ProblemListReview { .. } => "problem_list_review",
            Self::FollowUpAndVisitReview { .. } => "follow_up_review",
            Self::SaveWorkingNote { .. } => "working_note_save",
        }
    }

    pub const fn label(&self) -> &'static str {
        match self {
            Self::ChartSearch { .. } => "Search patient records",
            Self::ChartRead { .. } => "Read cited chart records",
            Self::MedicationReview { .. } => "Review active medications",
            Self::ProblemListReview { .. } => "Review problem list",
            Self::FollowUpAndVisitReview { .. } => "Check follow-ups & visit schedule",
            Self::SaveWorkingNote { .. } => "Save thread working note",
        }
    }

    pub const fn is_read_only(&self) -> bool {
        !matches!(self, Self::SaveWorkingNote { .. })
    }

    pub fn arguments_summary(&self) -> String {
        match self {
            Self::ChartSearch { query, kind_filter } => match kind_filter {
                Some(kind) => format!("query=\"{query}\", kind={}", kind.label()),
                None => format!("query=\"{query}\""),
            },
            Self::ChartRead { record_ids } => format!("record_ids=[{}]", record_ids.join(", ")),
            Self::MedicationReview { query_terms } => {
                if query_terms.is_empty() {
                    "scope=all_active".to_string()
                } else {
                    format!("terms=[{}]", query_terms.join(", "))
                }
            }
            Self::ProblemListReview { query_terms } => {
                if query_terms.is_empty() {
                    "scope=all_active".to_string()
                } else {
                    format!("terms=[{}]", query_terms.join(", "))
                }
            }
            Self::FollowUpAndVisitReview {
                include_reply_draft,
            } => {
                if *include_reply_draft {
                    "scope=visit+follow_ups+reply_draft".to_string()
                } else {
                    "scope=visit+follow_ups".to_string()
                }
            }
            Self::SaveWorkingNote {
                cited_record_ids, ..
            } => format!("cited=[{}]", cited_record_ids.join(", ")),
        }
    }
}

/// Declarative tool catalog entry advertised by the harness.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub label: String,
    pub description: String,
    pub read_only: bool,
    pub parameters_schema: Value,
}

/// Returns the bounded tool catalog offered to the given audience.
pub fn tool_catalog(audience: AgentAudience) -> Vec<ToolSpec> {
    let follow_up_desc = match audience {
        AgentAudience::Physician => {
            "Inspect scheduled visit slot, open clinical follow-ups, and pending patient reply drafts."
        }
        AgentAudience::Patient => {
            "Inspect upcoming demo visit slot and open care follow-up checkpoints."
        }
    };
    vec![
        ToolSpec {
            name: "chart_search".into(),
            label: "Search patient records".into(),
            description:
                "Whole-word clinical search scoped strictly to the selected patient chart.".into(),
            read_only: true,
            parameters_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "minLength": 1 },
                    "kind_filter": {
                        "type": ["string", "null"],
                        "enum": ["lab_result", "progress_note", "cgm_summary", "patient_message", "imaging", null]
                    }
                },
                "required": ["query"],
                "additionalProperties": false
            }),
        },
        ToolSpec {
            name: "chart_read".into(),
            label: "Read cited chart records".into(),
            description:
                "Read verbatim body text and structured key values for specific record IDs.".into(),
            read_only: true,
            parameters_schema: json!({
                "type": "object",
                "properties": {
                    "record_ids": {
                        "type": "array",
                        "items": { "type": "string" },
                        "minItems": 1,
                        "maxItems": 6
                    }
                },
                "required": ["record_ids"],
                "additionalProperties": false
            }),
        },
        ToolSpec {
            name: "medication_review".into(),
            label: "Review active medications".into(),
            description: "Inspect active medications, doses, schedules, and clinical indications."
                .into(),
            read_only: true,
            parameters_schema: json!({
                "type": "object",
                "properties": {
                    "query_terms": {
                        "type": "array",
                        "items": { "type": "string" }
                    }
                },
                "additionalProperties": false
            }),
        },
        ToolSpec {
            name: "problem_list_review".into(),
            label: "Review problem list".into(),
            description: "Inspect active diagnoses, ICD-10 codes, and clinical status.".into(),
            read_only: true,
            parameters_schema: json!({
                "type": "object",
                "properties": {
                    "query_terms": {
                        "type": "array",
                        "items": { "type": "string" }
                    }
                },
                "additionalProperties": false
            }),
        },
        ToolSpec {
            name: "follow_up_review".into(),
            label: "Check follow-ups & visit schedule".into(),
            description: follow_up_desc.into(),
            read_only: true,
            parameters_schema: json!({
                "type": "object",
                "properties": {
                    "include_reply_draft": { "type": "boolean" }
                },
                "additionalProperties": false
            }),
        },
        ToolSpec {
            name: "working_note_save".into(),
            label: "Save thread working note".into(),
            description:
                "Record a compact verified checkpoint carrying findings into subsequent turns."
                    .into(),
            read_only: false,
            parameters_schema: json!({
                "type": "object",
                "properties": {
                    "summary": { "type": "string", "minLength": 1 },
                    "cited_record_ids": {
                        "type": "array",
                        "items": { "type": "string" }
                    }
                },
                "required": ["summary", "cited_record_ids"],
                "additionalProperties": false
            }),
        },
    ]
}

/// Outcome of a single tool invocation inside the harness.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolExecutionStatus {
    Succeeded,
    Empty,
    Refused { reason: String },
}

impl ToolExecutionStatus {
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Succeeded => "ok",
            Self::Empty => "no match",
            Self::Refused { .. } => "refused",
        }
    }
}

/// Structured clinical value extracted from a tool-read record (`key_values`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CitedValue {
    pub record_id: String,
    pub record_title: String,
    pub recorded_at: String,
    pub label: String,
    pub value: String,
}

/// Recorded execution trace entry for one tool invocation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentStep {
    pub index: usize,
    pub tool_name: String,
    pub tool_label: String,
    pub arguments_summary: String,
    pub status: ToolExecutionStatus,
    pub observation_summary: String,
    pub record_ids: Vec<String>,
}

/// Compact working-note checkpoint persisted across turns in a thread.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkingNote {
    pub turn_index: usize,
    pub summary: String,
    pub cited_record_ids: Vec<String>,
}

/// Explicit terminal outcome of a bounded agentic chat run.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum HarnessOutcome {
    Finished,
    NoMatchingEvidence,
    Stopped {
        completed_steps: usize,
        resumable: bool,
    },
    CutOff {
        budget_limit: usize,
    },
}

impl HarnessOutcome {
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Finished => "Grounded",
            Self::NoMatchingEvidence => "No matching evidence",
            Self::Stopped { .. } => "Stopped (partial)",
            Self::CutOff { .. } => "Step budget reached",
        }
    }
}

/// Ordered event emitted during an agentic harness run and serialized over SSE.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    RunStarted {
        run_id: String,
        patient_id: String,
        audience: AgentAudience,
        max_steps: usize,
    },
    ToolCalled {
        step_index: usize,
        tool_name: String,
        tool_label: String,
        arguments_summary: String,
    },
    ToolCompleted {
        step: AgentStep,
    },
    WorkingNoteSaved {
        note: WorkingNote,
    },
    AnswerReady {
        grounded: bool,
        answer: String,
        citations: Vec<ChatCitation>,
        cited_values: Vec<CitedValue>,
    },
    RunFinished {
        outcome: HarnessOutcome,
        steps_used: usize,
        tool_calls_used: usize,
    },
}

impl AgentEvent {
    pub const fn sse_event_name(&self) -> &'static str {
        match self {
            Self::RunStarted { .. } => "run_started",
            Self::ToolCalled { .. } => "tool_called",
            Self::ToolCompleted { .. } => "tool_completed",
            Self::WorkingNoteSaved { .. } => "working_note_saved",
            Self::AnswerReady { .. } => "answer_ready",
            Self::RunFinished { .. } => "run_finished",
        }
    }
}

/// Serializes an ordered slice of [`AgentEvent`] frames into standard W3C
/// Server-Sent Events (`text/event-stream`) wire format.
#[must_use]
pub fn format_sse_events(events: &[AgentEvent]) -> String {
    let mut out = String::new();
    for (idx, event) in events.iter().enumerate() {
        let payload = serde_json::to_string(event).unwrap_or_else(|_| "{}".to_string());
        out.push_str(&format!(
            "id: {}\nevent: {}\ndata: {}\n\n",
            idx + 1,
            event.sse_event_name(),
            payload
        ));
    }
    out
}

/// Complete, inspectable result of one agentic chat turn.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentTurn {
    pub run_id: String,
    pub turn_index: usize,
    pub patient_id: String,
    pub audience: AgentAudience,
    pub question: String,
    pub outcome: HarnessOutcome,
    pub grounded: bool,
    pub answer: String,
    pub basis: String,
    pub engine_label: String,
    pub steps: Vec<AgentStep>,
    pub citations: Vec<ChatCitation>,
    pub cited_values: Vec<CitedValue>,
    pub working_note: Option<WorkingNote>,
    pub budget: RunBudget,
    pub events: Vec<AgentEvent>,
}

/// Multi-turn conversation state scoped to a single synthetic patient.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentThread {
    pub patient_id: String,
    pub audience: AgentAudience,
    pub turns: Vec<AgentTurn>,
    pub working_notes: Vec<WorkingNote>,
}

impl AgentThread {
    pub fn new(patient_id: impl Into<String>, audience: AgentAudience) -> Self {
        Self {
            patient_id: patient_id.into(),
            audience,
            turns: Vec::new(),
            working_notes: Vec::new(),
        }
    }

    pub fn next_turn_index(&self) -> usize {
        self.turns.len() + 1
    }

    /// Records a completed turn and appends its working note if one was saved.
    pub fn record_turn(&mut self, turn: AgentTurn) {
        if let Some(note) = turn.working_note.clone() {
            self.working_notes.push(note);
            if self.working_notes.len() > 8 {
                let drop_n = self.working_notes.len() - 8;
                self.working_notes.drain(0..drop_n);
            }
        }
        self.turns.push(turn);
        if self.turns.len() > 20 {
            let drop_n = self.turns.len() - 20;
            self.turns.drain(0..drop_n);
        }
    }

    /// Updates an existing turn by `run_id` (for example after optional on-device LLM refinement).
    pub fn replace_turn(&mut self, updated: AgentTurn) {
        if let Some(slot) = self.turns.iter_mut().find(|t| t.run_id == updated.run_id) {
            *slot = updated;
        }
    }

    pub fn clear(&mut self) {
        self.turns.clear();
        self.working_notes.clear();
    }

    pub fn latest_turn(&self) -> Option<&AgentTurn> {
        self.turns.last()
    }

    pub fn latest_cited_ids(&self) -> HashSet<String> {
        self.turns
            .last()
            .map(|t| t.citations.iter().map(|c| c.record_id.clone()).collect())
            .unwrap_or_default()
    }
}

/// Optional execution knobs for one agentic harness turn.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AgentRunOptions {
    pub budget: RunBudget,
    /// When set, stops cleanly after completing `stop_after_step` tool calls
    /// with `HarnessOutcome::Stopped { resumable: true }`.
    pub stop_after_step: Option<usize>,
}

/// HTTP request payload for `POST /clinical/api/patients/{id}/agent-chat`
/// and `/agent-chat/stream`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentChatRequest {
    pub question: String,
    #[serde(default)]
    pub audience: AgentAudience,
    #[serde(default)]
    pub prior_working_notes: Vec<WorkingNote>,
    #[serde(default)]
    pub max_steps: Option<usize>,
    #[serde(default)]
    pub stop_after_step: Option<usize>,
}

/// Accumulated tool observations within a single bounded run.
#[derive(Default)]
struct RunAccumulator {
    matched_records: Vec<(u32, ChartRecord)>,
    read_records: Vec<ChartRecord>,
    citations: Vec<ChatCitation>,
    cited_values: Vec<CitedValue>,
    matched_medications: Vec<String>,
    matched_problems: Vec<String>,
    matched_follow_ups: Vec<String>,
    visit_summary: Option<String>,
    reply_draft_summary: Option<String>,
    working_note: Option<WorkingNote>,
}

/// Intent signals parsed from the user question to plan multi-step tool calls.
#[derive(Clone, Copy, Debug, Default)]
struct QuestionIntent {
    kind_filter: Option<RecordKind>,
    wants_medications: bool,
    wants_all_medications: bool,
    wants_problems: bool,
    wants_all_problems: bool,
    wants_follow_ups: bool,
    wants_recap: bool,
}

fn infer_intent(question: &str, terms: &[String]) -> QuestionIntent {
    const MED_GENERAL: &[&str] = &[
        "med",
        "meds",
        "medication",
        "medications",
        "prescription",
        "prescriptions",
        "rx",
        "drug",
        "drugs",
        "regimen",
        "taking",
        "dosing",
        "dose",
        "refill",
    ];
    const MED_SPECIFIC: &[&str] = &[
        "statin",
        "statins",
        "insulin",
        "metformin",
        "apixaban",
        "lisinopril",
        "semaglutide",
        "ozempic",
        "albuterol",
        "budesonide",
        "sertraline",
        "atorvastatin",
        "losartan",
        "amlodipine",
        "warfarin",
        "aspirin",
        "inhaler",
        "glp-1",
    ];
    const PROBLEM_GENERAL: &[&str] = &[
        "problem",
        "problems",
        "diagnosis",
        "diagnoses",
        "condition",
        "conditions",
        "icd",
        "comorbidity",
        "comorbidities",
    ];
    const PROBLEM_SPECIFIC: &[&str] = &[
        "stroke",
        "diabetes",
        "hypertension",
        "afib",
        "fibrillation",
        "asthma",
        "depression",
        "anxiety",
        "migraine",
        "ckd",
        "kidney",
        "neuropathy",
    ];
    const FOLLOW_UP_WORDS: &[&str] = &[
        "follow",
        "followup",
        "follow-up",
        "followups",
        "follow-ups",
        "visit",
        "appointment",
        "schedule",
        "scheduled",
        "due",
        "overdue",
        "next",
        "plan",
        "today",
        "draft",
        "reply",
        "pending",
        "action",
        "prepare",
        "prep",
    ];
    const RECAP_WORDS: &[&str] = &[
        "recap", "earlier", "previous", "checked", "summary", "overview",
    ];

    let has_any = |words: &[&str]| {
        terms
            .iter()
            .any(|t| words.iter().any(|w| same_word(t, w) || t == w))
            || words.iter().any(|w| mentions(question, w))
    };

    let wants_all_medications = has_any(MED_GENERAL);
    let wants_medications = wants_all_medications || has_any(MED_SPECIFIC);
    let wants_all_problems = has_any(PROBLEM_GENERAL);
    let wants_problems = wants_all_problems || has_any(PROBLEM_SPECIFIC);
    let wants_follow_ups = has_any(FOLLOW_UP_WORDS);
    let wants_recap = has_any(RECAP_WORDS);

    // Narrow to a single record kind only when the caller explicitly asks to
    // filter by one category (or queries solely by a single category word),
    // so multi-domain questions like "HbA1c and CGM TIR" search all records.
    let explicit_filter = mentions(question, "only") || mentions(question, "just");
    let mut matched_kinds = Vec::new();
    for kind in [
        RecordKind::LabResult,
        RecordKind::CgmSummary,
        RecordKind::Imaging,
        RecordKind::PatientMessage,
    ] {
        let hit = terms
            .iter()
            .any(|t| kind.query_words().iter().any(|w| same_word(t, w)));
        if hit {
            matched_kinds.push(kind);
        }
    }
    let kind_filter = if matched_kinds.len() == 1 && (explicit_filter || terms.len() == 1) {
        Some(matched_kinds[0])
    } else {
        None
    };

    QuestionIntent {
        kind_filter,
        wants_medications,
        wants_all_medications,
        wants_problems,
        wants_all_problems,
        wants_follow_ups,
        wants_recap,
    }
}

/// Executes one bounded, multi-step tool-calling agentic chat turn scoped
/// strictly to `patient`.
pub fn run_agent_turn(
    patient: &PatientChart,
    raw_question: &str,
    audience: AgentAudience,
    turn_index: usize,
    prior_notes: &[WorkingNote],
    options: AgentRunOptions,
) -> AgentTurn {
    let question = raw_question.trim();
    let budget = options.budget.clamped();
    let run_id = format!("run-{}-t{turn_index}", patient.id);
    let terms = query_terms(question);
    let intent = infer_intent(question, &terms);

    let mut events = vec![AgentEvent::RunStarted {
        run_id: run_id.clone(),
        patient_id: patient.id.clone(),
        audience,
        max_steps: budget.max_steps,
    }];
    let mut steps: Vec<AgentStep> = Vec::new();
    let mut acc = RunAccumulator::default();
    let mut tool_calls_used = 0usize;

    // Helper closure to enforce budgets, cancellation, and structured event emission.
    let exec_step = |tool: AgentTool,
                     acc: &mut RunAccumulator,
                     steps: &mut Vec<AgentStep>,
                     events: &mut Vec<AgentEvent>,
                     tool_calls_used: &mut usize|
     -> Result<ToolExecutionStatus, HarnessOutcome> {
        if let Some(stop_at) = options.stop_after_step {
            if *tool_calls_used >= stop_at {
                return Err(HarnessOutcome::Stopped {
                    completed_steps: steps.len(),
                    resumable: true,
                });
            }
        }
        if steps.len() >= budget.max_steps || *tool_calls_used >= budget.max_tool_calls {
            return Err(HarnessOutcome::CutOff {
                budget_limit: budget.max_steps.min(budget.max_tool_calls),
            });
        }

        let step_index = steps.len() + 1;
        *tool_calls_used += 1;
        let tool_name = tool.name().to_string();
        let tool_label = tool.label().to_string();
        let arguments_summary = tool.arguments_summary();

        events.push(AgentEvent::ToolCalled {
            step_index,
            tool_name: tool_name.clone(),
            tool_label: tool_label.clone(),
            arguments_summary: arguments_summary.clone(),
        });

        let (status, observation_summary, record_ids) = execute_tool(
            patient,
            &tool,
            &terms,
            intent,
            budget.max_citations,
            turn_index,
            acc,
        );

        let step = AgentStep {
            index: step_index,
            tool_name,
            tool_label,
            arguments_summary,
            status: status.clone(),
            observation_summary,
            record_ids,
        };
        events.push(AgentEvent::ToolCompleted { step: step.clone() });
        if let Some(note) = acc.working_note.clone() {
            if matches!(tool, AgentTool::SaveWorkingNote { .. }) {
                events.push(AgentEvent::WorkingNoteSaved { note });
            }
        }
        steps.push(step);
        Ok(status)
    };

    let mut early_outcome: Option<HarnessOutcome> = None;

    // Step 1: Search patient records with optional inferred kind filter.
    let search_query = if terms.is_empty() {
        question.to_string()
    } else {
        terms.join(", ")
    };
    match exec_step(
        AgentTool::ChartSearch {
            query: search_query.clone(),
            kind_filter: intent.kind_filter,
        },
        &mut acc,
        &mut steps,
        &mut events,
        &mut tool_calls_used,
    ) {
        Ok(ToolExecutionStatus::Empty) if intent.kind_filter.is_some() => {
            // Broaden search cleanly when narrow kind filter found no hits.
            if let Err(outcome) = exec_step(
                AgentTool::ChartSearch {
                    query: search_query,
                    kind_filter: None,
                },
                &mut acc,
                &mut steps,
                &mut events,
                &mut tool_calls_used,
            ) {
                early_outcome = Some(outcome);
            }
        }
        Ok(_) => {}
        Err(outcome) => early_outcome = Some(outcome),
    }

    // Step 2: Read top matched records to extract verbatim citations and clinical values.
    if early_outcome.is_none() && !acc.matched_records.is_empty() {
        let ids: Vec<String> = acc
            .matched_records
            .iter()
            .take(budget.max_citations)
            .map(|(_, r)| r.id.clone())
            .collect();
        if let Err(outcome) = exec_step(
            AgentTool::ChartRead { record_ids: ids },
            &mut acc,
            &mut steps,
            &mut events,
            &mut tool_calls_used,
        ) {
            early_outcome = Some(outcome);
        }
    }

    // Step 3+: Intent-driven structured domain tools (medications, problem list, follow-ups).
    if early_outcome.is_none() && intent.wants_medications {
        let med_terms = if intent.wants_all_medications {
            Vec::new()
        } else {
            terms.clone()
        };
        if let Err(outcome) = exec_step(
            AgentTool::MedicationReview {
                query_terms: med_terms,
            },
            &mut acc,
            &mut steps,
            &mut events,
            &mut tool_calls_used,
        ) {
            early_outcome = Some(outcome);
        }
    }

    if early_outcome.is_none() && intent.wants_problems {
        let prob_terms = if intent.wants_all_problems {
            Vec::new()
        } else {
            terms.clone()
        };
        if let Err(outcome) = exec_step(
            AgentTool::ProblemListReview {
                query_terms: prob_terms,
            },
            &mut acc,
            &mut steps,
            &mut events,
            &mut tool_calls_used,
        ) {
            early_outcome = Some(outcome);
        }
    }

    if early_outcome.is_none() && intent.wants_follow_ups {
        if let Err(outcome) = exec_step(
            AgentTool::FollowUpAndVisitReview {
                include_reply_draft: audience == AgentAudience::Physician,
            },
            &mut acc,
            &mut steps,
            &mut events,
            &mut tool_calls_used,
        ) {
            early_outcome = Some(outcome);
        }
    }

    let has_direct_evidence = !acc.citations.is_empty()
        || !acc.matched_medications.is_empty()
        || !acc.matched_problems.is_empty()
        || !acc.matched_follow_ups.is_empty()
        || acc.visit_summary.is_some()
        || acc.reply_draft_summary.is_some();
    let can_recap = intent.wants_recap && !prior_notes.is_empty();

    // Final tool step: persist a verified working note when grounded evidence was found
    // and we still have step budget remaining.
    if early_outcome.is_none() && has_direct_evidence && steps.len() < budget.max_steps {
        let note_summary = build_working_note_summary(question, &acc);
        let note_ids: Vec<String> = acc.citations.iter().map(|c| c.record_id.clone()).collect();
        let _ = exec_step(
            AgentTool::SaveWorkingNote {
                summary: note_summary,
                cited_record_ids: note_ids,
            },
            &mut acc,
            &mut steps,
            &mut events,
            &mut tool_calls_used,
        );
    }

    let (outcome, grounded, answer, basis) = synthesize_answer(
        patient,
        question,
        audience,
        &terms,
        &steps,
        &acc,
        prior_notes,
        can_recap,
        early_outcome,
    );

    events.push(AgentEvent::AnswerReady {
        grounded,
        answer: answer.clone(),
        citations: acc.citations.clone(),
        cited_values: acc.cited_values.clone(),
    });
    events.push(AgentEvent::RunFinished {
        outcome: outcome.clone(),
        steps_used: steps.len(),
        tool_calls_used,
    });

    AgentTurn {
        run_id,
        turn_index,
        patient_id: patient.id.clone(),
        audience,
        question: question.to_string(),
        outcome,
        grounded,
        answer,
        basis,
        engine_label: "Agentic Harness · Local Rust".to_string(),
        steps,
        citations: acc.citations,
        cited_values: acc.cited_values,
        working_note: acc.working_note,
        budget,
        events,
    }
}

fn execute_tool(
    patient: &PatientChart,
    tool: &AgentTool,
    terms: &[String],
    intent: QuestionIntent,
    max_citations: usize,
    turn_index: usize,
    acc: &mut RunAccumulator,
) -> (ToolExecutionStatus, String, Vec<String>) {
    match tool {
        AgentTool::ChartSearch { kind_filter, .. } => {
            if terms.is_empty() {
                return (
                    ToolExecutionStatus::Empty,
                    "No searchable clinical terms were extracted from the question.".to_string(),
                    Vec::new(),
                );
            }
            let mut scored: Vec<(u32, &ChartRecord)> = patient
                .records
                .iter()
                .filter(|rec| kind_filter.is_none_or(|k| rec.kind == k))
                .map(|rec| (terms.iter().map(|t| term_weight(rec, t)).sum(), rec))
                .filter(|(score, _)| *score > 0)
                .collect();
            scored.sort_by(|(sa, a), (sb, b)| {
                sb.cmp(sa)
                    .then_with(|| a.kind.body_weight().cmp(&b.kind.body_weight()).reverse())
                    .then_with(|| b.recorded_at.cmp(&a.recorded_at))
                    .then_with(|| a.id.cmp(&b.id))
            });
            if scored.is_empty() {
                let scope = kind_filter
                    .map(|k| format!("{} records", k.label()))
                    .unwrap_or_else(|| format!("{} chart records", patient.records.len()));
                return (
                    ToolExecutionStatus::Empty,
                    format!("Searched {scope} for [{}]; 0 hits.", terms.join(", ")),
                    Vec::new(),
                );
            }
            let ids: Vec<String> = scored
                .iter()
                .take(max_citations)
                .map(|(_, r)| r.id.clone())
                .collect();
            let hits: Vec<String> = scored
                .iter()
                .take(max_citations)
                .map(|(s, r)| format!("{} ({}, score {s})", r.id, r.kind.label()))
                .collect();
            acc.matched_records = scored
                .into_iter()
                .take(max_citations)
                .map(|(s, r)| (s, r.clone()))
                .collect();
            (
                ToolExecutionStatus::Succeeded,
                format!("Matched {} record(s): {}", ids.len(), hits.join("; ")),
                ids,
            )
        }
        AgentTool::ChartRead { record_ids } => {
            let mut read_ids = Vec::new();
            let mut refused_ids = Vec::new();
            for id in record_ids {
                if let Some(rec) = patient.records.iter().find(|r| &r.id == id) {
                    if !acc.citations.iter().any(|c| c.record_id == rec.id) {
                        acc.citations.push(ChatCitation {
                            record_id: rec.id.clone(),
                            title: rec.title.clone(),
                            recorded_at: rec.recorded_at.clone(),
                            kind: rec.kind,
                            excerpt: rec.body.clone(),
                        });
                    }
                    if !acc.read_records.iter().any(|r| r.id == rec.id) {
                        acc.read_records.push(rec.clone());
                    }
                    collect_cited_values(rec, terms, &mut acc.cited_values);
                    read_ids.push(rec.id.clone());
                } else {
                    refused_ids.push(id.clone());
                }
            }
            if read_ids.is_empty() {
                return (
                    ToolExecutionStatus::Refused {
                        reason: format!(
                            "Record ID(s) not present in {}'s chart: {}",
                            patient.mrn,
                            refused_ids.join(", ")
                        ),
                    },
                    format!(
                        "Refused out-of-chart read for [{}].",
                        refused_ids.join(", ")
                    ),
                    Vec::new(),
                );
            }
            let obs = if refused_ids.is_empty() {
                format!(
                    "Read {} verbatim record(s) and extracted {} structured clinical value(s).",
                    read_ids.len(),
                    acc.cited_values.len()
                )
            } else {
                format!(
                    "Read {} record(s); refused {} foreign/unknown ID(s).",
                    read_ids.len(),
                    refused_ids.len()
                )
            };
            (ToolExecutionStatus::Succeeded, obs, read_ids)
        }
        AgentTool::MedicationReview { query_terms } => {
            let matches: Vec<String> = patient
                .medications
                .iter()
                .filter(|m| {
                    intent.wants_all_medications
                        || query_terms.is_empty()
                        || query_terms.iter().any(|t| {
                            mentions(&m.name, t)
                                || mentions(&m.dose, t)
                                || mentions(&m.indication, t)
                        })
                })
                .map(|m| format!("{} {} ({}; {})", m.name, m.dose, m.schedule, m.indication))
                .collect();
            if matches.is_empty() {
                (
                    ToolExecutionStatus::Empty,
                    format!(
                        "Reviewed {} active medication(s); none matched [{}].",
                        patient.medications.len(),
                        query_terms.join(", ")
                    ),
                    Vec::new(),
                )
            } else {
                let count = matches.len();
                acc.matched_medications = matches;
                (
                    ToolExecutionStatus::Succeeded,
                    format!(
                        "Found {count} active medication(s) on {}'s medication list.",
                        patient.mrn
                    ),
                    Vec::new(),
                )
            }
        }
        AgentTool::ProblemListReview { query_terms } => {
            let matches: Vec<String> = patient
                .problems
                .iter()
                .filter(|p| {
                    intent.wants_all_problems
                        || query_terms.is_empty()
                        || query_terms.iter().any(|t| {
                            mentions(&p.label, t) || mentions(&p.code, t) || mentions(&p.status, t)
                        })
                })
                .map(|p| format!("{} [{}] — {}", p.label, p.code, p.status))
                .collect();
            if matches.is_empty() {
                (
                    ToolExecutionStatus::Empty,
                    format!(
                        "Reviewed {} active problem(s); none matched [{}].",
                        patient.problems.len(),
                        query_terms.join(", ")
                    ),
                    Vec::new(),
                )
            } else {
                let count = matches.len();
                acc.matched_problems = matches;
                (
                    ToolExecutionStatus::Succeeded,
                    format!(
                        "Found {count} active problem(s) on {}'s chart.",
                        patient.mrn
                    ),
                    Vec::new(),
                )
            }
        }
        AgentTool::FollowUpAndVisitReview {
            include_reply_draft,
        } => {
            if let Some(visit) = &patient.visit {
                acc.visit_summary = Some(format!(
                    "{} ({}) — {}",
                    visit.starts_at, visit.modality, visit.reason
                ));
            }
            acc.matched_follow_ups = patient
                .follow_ups
                .iter()
                .map(|fu| {
                    let badge = match fu.standing {
                        FollowUpStanding::DueToday => "Due today",
                        FollowUpStanding::Monitoring => "Monitoring",
                        FollowUpStanding::Completed => "Completed",
                    };
                    format!("[{badge} · {}] {}", fu.next_check, fu.summary)
                })
                .collect();
            if *include_reply_draft {
                if let Some(draft) = &patient.reply_draft {
                    acc.reply_draft_summary = Some(format!(
                        "Pending clinician reply draft for [{}]: \"{}\"",
                        draft.message_record_id, draft.patient_question
                    ));
                }
            }
            if acc.matched_follow_ups.is_empty()
                && acc.visit_summary.is_none()
                && acc.reply_draft_summary.is_none()
            {
                (
                    ToolExecutionStatus::Empty,
                    "No scheduled visit or open follow-ups on this chart.".to_string(),
                    Vec::new(),
                )
            } else {
                (
                    ToolExecutionStatus::Succeeded,
                    format!(
                        "Loaded {} follow-up item(s){}{}.",
                        acc.matched_follow_ups.len(),
                        if acc.visit_summary.is_some() {
                            " + visit slot"
                        } else {
                            ""
                        },
                        if acc.reply_draft_summary.is_some() {
                            " + pending reply draft"
                        } else {
                            ""
                        }
                    ),
                    Vec::new(),
                )
            }
        }
        AgentTool::SaveWorkingNote {
            summary,
            cited_record_ids,
        } => {
            let note = WorkingNote {
                turn_index,
                summary: summary.clone(),
                cited_record_ids: cited_record_ids.clone(),
            };
            acc.working_note = Some(note);
            (
                ToolExecutionStatus::Succeeded,
                format!("Saved turn-{turn_index} working note checkpoint for follow-up turns."),
                cited_record_ids.clone(),
            )
        }
    }
}

fn collect_cited_values(rec: &ChartRecord, terms: &[String], out: &mut Vec<CitedValue>) {
    let matching: Vec<&(String, String)> = rec
        .key_values
        .iter()
        .filter(|(k, v)| terms.iter().any(|t| mentions(k, t) || mentions(v, t)))
        .collect();
    let chosen: Vec<&(String, String)> = if matching.is_empty() {
        rec.key_values.iter().take(2).collect()
    } else {
        matching
    };
    for (label, value) in chosen {
        if out.len() >= 6 {
            break;
        }
        let dup = out
            .iter()
            .any(|v| v.record_id == rec.id && v.label == *label && v.value == *value);
        if !dup {
            out.push(CitedValue {
                record_id: rec.id.clone(),
                record_title: rec.title.clone(),
                recorded_at: rec.recorded_at.clone(),
                label: label.clone(),
                value: value.clone(),
            });
        }
    }
}

fn build_working_note_summary(question: &str, acc: &RunAccumulator) -> String {
    let mut pieces = Vec::new();
    if !acc.citations.is_empty() {
        let ids: Vec<&str> = acc.citations.iter().map(|c| c.record_id.as_str()).collect();
        pieces.push(format!("cited [{}]", ids.join(", ")));
    }
    if !acc.cited_values.is_empty() {
        let vals: Vec<String> = acc
            .cited_values
            .iter()
            .take(3)
            .map(|v| format!("{}={}", v.label, v.value))
            .collect();
        pieces.push(vals.join(", "));
    }
    if !acc.matched_medications.is_empty() {
        pieces.push(format!("{} active med(s)", acc.matched_medications.len()));
    }
    if !acc.matched_problems.is_empty() {
        pieces.push(format!("{} problem(s)", acc.matched_problems.len()));
    }
    if !acc.matched_follow_ups.is_empty() {
        pieces.push(format!("{} follow-up(s)", acc.matched_follow_ups.len()));
    }
    let short_q: String = question.chars().take(64).collect();
    format!("Checked \"{short_q}\": {}", pieces.join(" · "))
}

#[allow(clippy::too_many_arguments)]
fn synthesize_answer(
    patient: &PatientChart,
    question: &str,
    audience: AgentAudience,
    terms: &[String],
    steps: &[AgentStep],
    acc: &RunAccumulator,
    prior_notes: &[WorkingNote],
    can_recap: bool,
    early_outcome: Option<HarnessOutcome>,
) -> (HarnessOutcome, bool, String, String) {
    let mut sections: Vec<String> = Vec::new();

    if !acc.read_records.is_empty() {
        let findings: Vec<String> = acc
            .read_records
            .iter()
            .map(|r| {
                format!(
                    "- {} [{}], {}, {}: {}",
                    r.title,
                    r.id,
                    r.kind.label(),
                    r.recorded_at,
                    relevant_excerpt(r, terms)
                )
            })
            .collect();
        let header = match audience {
            AgentAudience::Physician => format!(
                "{} matching record(s) in {}'s chart ({}):",
                acc.read_records.len(),
                patient.display_name,
                patient.mrn
            ),
            AgentAudience::Patient => format!(
                "From {}'s synthetic sample records (demo only, not medical advice):",
                patient.display_name
            ),
        };
        sections.push(format!("{header}\n{}", findings.join("\n")));
    }

    if !acc.matched_medications.is_empty() {
        sections.push(format!(
            "Active medications:\n{}",
            acc.matched_medications
                .iter()
                .map(|m| format!("- {m}"))
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }

    if !acc.matched_problems.is_empty() {
        sections.push(format!(
            "Problem list:\n{}",
            acc.matched_problems
                .iter()
                .map(|p| format!("- {p}"))
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }

    if acc.visit_summary.is_some()
        || !acc.matched_follow_ups.is_empty()
        || acc.reply_draft_summary.is_some()
    {
        let mut lines = Vec::new();
        if let Some(visit) = &acc.visit_summary {
            lines.push(format!("- Scheduled visit: {visit}"));
        }
        for fu in &acc.matched_follow_ups {
            lines.push(format!("- {fu}"));
        }
        if let Some(draft) = &acc.reply_draft_summary {
            lines.push(format!("- {draft}"));
        }
        sections.push(format!("Visit & follow-up status:\n{}", lines.join("\n")));
    }

    if sections.is_empty() && can_recap {
        let recap_lines: Vec<String> = prior_notes
            .iter()
            .map(|n| format!("- Turn {}: {}", n.turn_index, n.summary))
            .collect();
        sections.push(format!(
            "Verified working notes from earlier turns in this thread ({}):\n{}",
            patient.mrn,
            recap_lines.join("\n")
        ));
    }

    let checked_tools: Vec<&str> = steps.iter().map(|s| s.tool_name.as_str()).collect();
    let tools_list = if checked_tools.is_empty() {
        "none".to_string()
    } else {
        checked_tools.join(" -> ")
    };

    if let Some(early) = early_outcome {
        let grounded = !sections.is_empty();
        let prefix = match &early {
            HarnessOutcome::Stopped {
                completed_steps, ..
            } => format!(
                "Stopped after {completed_steps} step(s). Partial verified output kept below (resumable)."
            ),
            HarnessOutcome::CutOff { budget_limit } => format!(
                "Reached step budget ({budget_limit} steps). Returning verified findings gathered so far."
            ),
            HarnessOutcome::Finished | HarnessOutcome::NoMatchingEvidence => String::new(),
        };
        let body = if sections.is_empty() {
            format!("{prefix} No matching chart records were read before stopping.")
        } else {
            format!("{prefix}\n\n{}", sections.join("\n\n"))
        };
        let basis = format!(
            "Bounded agentic harness ({}) on {} [{}]; {} step(s).",
            early.label(),
            patient.mrn,
            tools_list,
            steps.len()
        );
        return (early, grounded, body, basis);
    }

    if sections.is_empty() {
        let reason = if terms.is_empty() {
            "The question has no searchable clinical terms.".to_string()
        } else {
            format!(
                "None of {}'s {} records or checked clinical lists mention: {}.",
                patient.display_name,
                patient.records.len(),
                terms.join(", ")
            )
        };
        let answer = format!(
            "No matching records in this chart for {} ({}). {reason} \
             Nothing is cited, and absence from these records is not evidence either way. \
             Try asking about a lab value, CGM metric, active medication, problem, follow-up, or message on this chart.",
            patient.display_name, patient.mrn
        );
        let basis = format!(
            "Agentic harness checked [{tools_list}] on {}; no record or clinical entry matched \"{question}\".",
            patient.mrn
        );
        return (HarnessOutcome::NoMatchingEvidence, false, answer, basis);
    }

    let cited_ids: Vec<&str> = acc.citations.iter().map(|c| c.record_id.as_str()).collect();
    let cite_note = if cited_ids.is_empty() {
        "structured chart lists".to_string()
    } else {
        format!("cited: {}", cited_ids.join(", "))
    };
    let basis = format!(
        "Agentic harness ({} step(s): {tools_list}) scoped strictly to {} ({cite_note}).",
        steps.len(),
        patient.mrn
    );
    (HarnessOutcome::Finished, true, sections.join("\n\n"), basis)
}

/// Builds a strictly tool-grounded prompt for optional on-device LLM synthesis
/// (e.g. Android AICore / Gemini Nano) from an executed [`AgentTurn`].
#[must_use]
pub fn build_harness_llm_prompt(patient: &PatientChart, turn: &AgentTurn) -> String {
    let mut prompt = String::with_capacity(2400);
    prompt.push_str(&format!(
        "You are a careful, concise on-device clinical demo assistant speaking to a {}.\n\
         Rules:\n\
         - Synthesize ONLY from the verified tool observations and verbatim excerpts below for {} ({}).\n\
         - Keep it to 3-5 clear sentences, cite record IDs/dates accurately, never invent values, and state gaps honestly.\n\n",
        turn.audience.label(),
        patient.display_name,
        patient.mrn
    ));
    prompt.push_str("Executed agent tool steps:\n");
    for step in &turn.steps {
        prompt.push_str(&format!(
            "- Step {} [{} | {}]: {}\n",
            step.index,
            step.tool_name,
            step.status.label(),
            step.observation_summary
        ));
    }
    if !turn.cited_values.is_empty() {
        prompt.push_str("\nExtracted clinical values:\n");
        for val in &turn.cited_values {
            prompt.push_str(&format!(
                "- {} ({}, {}): {} = {}\n",
                val.record_title, val.record_id, val.recorded_at, val.label, val.value
            ));
        }
    }
    if !turn.citations.is_empty() {
        prompt.push_str("\nVerbatim cited excerpts:\n");
        for cite in &turn.citations {
            prompt.push_str(&format!(
                "- [{} | {} | {}] {}\n",
                cite.record_id, cite.title, cite.recorded_at, cite.excerpt
            ));
        }
    }
    prompt.push_str(&format!(
        "\nBaseline verified answer:\n{}\n\nQuestion: {}\nGrounded answer:",
        turn.answer, turn.question
    ));
    prompt
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::find_patient;

    #[test]
    fn physician_run_executes_multi_step_tools_and_extracts_cited_values() {
        let nadia = find_patient("pt-nadia-kulkarni").expect("nadia");
        let turn = run_agent_turn(
            &nadia,
            "What is her latest HbA1c, CGM TIR, and active medications?",
            AgentAudience::Physician,
            1,
            &[],
            AgentRunOptions::default(),
        );
        assert_eq!(turn.outcome, HarnessOutcome::Finished);
        assert!(turn.grounded);
        assert!(turn.steps.len() >= 3, "expected search + read + med review");
        assert!(turn.steps.iter().any(|s| s.tool_name == "chart_search"));
        assert!(turn.steps.iter().any(|s| s.tool_name == "chart_read"));
        assert!(turn
            .steps
            .iter()
            .any(|s| s.tool_name == "medication_review"));
        assert!(turn
            .steps
            .iter()
            .any(|s| s.tool_name == "working_note_save"));
        assert!(!turn.citations.is_empty());
        assert!(!turn.cited_values.is_empty());
        assert!(turn.working_note.is_some());
        assert!(turn.answer.contains("6.7%"));
        assert!(turn.answer.contains("Active medications:"));
        // Verify all citations remain strictly within Nadia's chart.
        for cite in &turn.citations {
            assert!(nadia.records.iter().any(|r| r.id == cite.record_id));
        }
    }

    #[test]
    fn absent_medication_and_gibberish_return_honest_no_matching_evidence() {
        let carlos = find_patient("pt-carlos-mendez").expect("carlos");
        let statin = run_agent_turn(
            &carlos,
            "Is he on a statin?",
            AgentAudience::Physician,
            1,
            &[],
            AgentRunOptions::default(),
        );
        assert_eq!(statin.outcome, HarnessOutcome::NoMatchingEvidence);
        assert!(!statin.grounded);
        assert!(statin.citations.is_empty());
        assert!(statin
            .steps
            .iter()
            .any(|s| s.tool_name == "medication_review" && s.status == ToolExecutionStatus::Empty));

        let david = find_patient("pt-david-okafor").expect("david");
        let stroke = run_agent_turn(
            &david,
            "Any history of stroke?",
            AgentAudience::Physician,
            1,
            &[],
            AgentRunOptions::default(),
        );
        assert_eq!(stroke.outcome, HarnessOutcome::NoMatchingEvidence);
        assert!(!stroke.grounded);
        assert!(stroke.citations.is_empty());
        assert!(stroke.steps.iter().any(
            |s| s.tool_name == "problem_list_review" && s.status == ToolExecutionStatus::Empty
        ));
    }

    #[test]
    fn step_budget_cutoff_and_stop_after_step_preserve_partial_work() {
        let nadia = find_patient("pt-nadia-kulkarni").expect("nadia");

        let stopped = run_agent_turn(
            &nadia,
            "Check HbA1c, medications, and follow-ups",
            AgentAudience::Physician,
            1,
            &[],
            AgentRunOptions {
                budget: RunBudget::default(),
                stop_after_step: Some(2),
            },
        );
        assert_eq!(
            stopped.outcome,
            HarnessOutcome::Stopped {
                completed_steps: 2,
                resumable: true
            }
        );
        assert_eq!(stopped.steps.len(), 2);
        assert!(stopped.grounded);
        assert!(!stopped.citations.is_empty());

        let cutoff = run_agent_turn(
            &nadia,
            "Check HbA1c, medications, and follow-ups",
            AgentAudience::Physician,
            1,
            &[],
            AgentRunOptions {
                budget: RunBudget {
                    max_steps: 2,
                    max_tool_calls: 2,
                    max_citations: 2,
                },
                stop_after_step: None,
            },
        );
        assert_eq!(cutoff.outcome, HarnessOutcome::CutOff { budget_limit: 2 });
        assert_eq!(cutoff.steps.len(), 2);
        assert!(cutoff.answer.contains("Reached step budget"));
    }

    #[test]
    fn patient_audience_hides_clinician_reply_drafts_and_supports_thread_recap() {
        let nadia = find_patient("pt-nadia-kulkarni").expect("nadia");
        let mut thread = AgentThread::new(&nadia.id, AgentAudience::Patient);

        let t1 = run_agent_turn(
            &nadia,
            "What is my upcoming visit and follow-up plan?",
            AgentAudience::Patient,
            thread.next_turn_index(),
            &thread.working_notes,
            AgentRunOptions::default(),
        );
        assert_eq!(t1.outcome, HarnessOutcome::Finished);
        assert!(!t1.answer.contains("Pending clinician reply draft"));
        assert!(t1.working_note.is_some());
        thread.record_turn(t1);

        let t2 = run_agent_turn(
            &nadia,
            "Recap what we checked earlier",
            AgentAudience::Patient,
            thread.next_turn_index(),
            &thread.working_notes,
            AgentRunOptions::default(),
        );
        assert_eq!(t2.outcome, HarnessOutcome::Finished);
        assert!(t2.grounded);
        assert!(t2.answer.contains("Verified working notes"));
    }

    #[test]
    fn sse_event_formatting_preserves_order_and_json_frames() {
        let nadia = find_patient("pt-nadia-kulkarni").expect("nadia");
        let turn = run_agent_turn(
            &nadia,
            "What is her potassium?",
            AgentAudience::Physician,
            1,
            &[],
            AgentRunOptions::default(),
        );
        let sse = format_sse_events(&turn.events);
        assert!(sse.contains("event: run_started\n"));
        assert!(sse.contains("event: tool_called\n"));
        assert!(sse.contains("event: tool_completed\n"));
        assert!(sse.contains("event: answer_ready\n"));
        assert!(sse.contains("event: run_finished\n"));

        let prompt = build_harness_llm_prompt(&nadia, &turn);
        assert!(prompt.contains("chart_search"));
        assert!(prompt.contains("nk-lab-02"));
    }

    #[test]
    fn tool_catalog_advertises_valid_json_schemas() {
        let catalog = tool_catalog(AgentAudience::Physician);
        assert_eq!(catalog.len(), 6);
        for spec in catalog {
            assert_eq!(spec.parameters_schema["type"], "object");
        }
    }
}
