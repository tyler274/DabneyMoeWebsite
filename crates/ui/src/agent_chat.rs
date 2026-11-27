//! Shared Leptos UI components for the bounded clinical agentic chat harness.
//!
//! Renders multi-turn patient-scoped threads, expandable tool execution traces,
//! clickable cited clinical values, verbatim record citations, and working-note
//! checkpoints across `/clinical/physician` and `/clinical/patient`.

use clinical_workspace::{
    tool_catalog, AgentAudience, AgentStep, AgentThread, AgentTurn, CitedValue, HarnessOutcome,
    ToolExecutionStatus,
};
use leptos::prelude::*;

fn scroll_to_record(record_id: &str) {
    #[cfg(any(feature = "csr", feature = "hydrate"))]
    {
        if let Some(el) = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.get_element_by_id(&format!("record-{record_id}")))
        {
            el.scroll_into_view();
        }
    }
    #[cfg(not(any(feature = "csr", feature = "hydrate")))]
    let _ = record_id;
}

fn outcome_badge_class(outcome: &HarnessOutcome) -> &'static str {
    match outcome {
        HarnessOutcome::Finished => "border-emerald-400/40 bg-emerald-500/15 text-emerald-200",
        HarnessOutcome::NoMatchingEvidence => "border-amber-400/40 bg-amber-500/15 text-amber-200",
        HarnessOutcome::Stopped { .. } => "border-cyan-400/40 bg-cyan-500/15 text-cyan-200",
        HarnessOutcome::CutOff { .. } => "border-violet-400/40 bg-violet-500/15 text-violet-200",
    }
}

fn step_status_class(status: &ToolExecutionStatus) -> &'static str {
    match status {
        ToolExecutionStatus::Succeeded => {
            "border-emerald-400/35 bg-emerald-500/15 text-emerald-200"
        }
        ToolExecutionStatus::Empty => "border-amber-400/35 bg-amber-500/15 text-amber-200",
        ToolExecutionStatus::Refused { .. } => "border-rose-400/40 bg-rose-500/15 text-rose-200",
    }
}

/// Compact collapsible tool-catalog strip showing the exact bounded tools
/// available for the active clinical audience.
#[component]
pub fn AgentToolCatalogBar(audience: AgentAudience) -> impl IntoView {
    let specs = tool_catalog(audience);
    let count = specs.len();

    view! {
        <details
            data-agent-tool-catalog="true"
            class="rounded-xl border border-white/10 bg-slate-950/60 px-3.5 py-2 text-xs text-slate-300"
        >
            <summary class="cursor-pointer select-none list-none flex flex-wrap items-center justify-between gap-2 font-medium text-slate-200">
                <span class="inline-flex items-center gap-2">
                    <span class="h-1.5 w-1.5 rounded-full bg-emerald-400"></span>
                    {format!("Bounded tool catalog ({count} tools · single-patient scoped)")}
                </span>
                <span class="font-mono text-[11px] text-cyan-300">"Inspect schemas ▾"</span>
            </summary>
            <div class="mt-3 grid gap-2 border-t border-white/10 pt-3 sm:grid-cols-2">
                {specs
                    .into_iter()
                    .map(|spec| {
                        let rw_cls = if spec.read_only {
                            "border-cyan-400/30 bg-cyan-500/10 text-cyan-200"
                        } else {
                            "border-emerald-400/30 bg-emerald-500/10 text-emerald-200"
                        };
                        let rw_label = if spec.read_only { "read-only" } else { "checkpoint" };
                        view! {
                            <div class="rounded-lg border border-white/10 bg-slate-900/70 p-2.5">
                                <div class="flex items-center justify-between gap-2">
                                    <code class="font-mono text-[11px] font-semibold text-white">
                                        {spec.name}
                                    </code>
                                    <span class=format!("rounded-full border px-2 py-0.5 text-[10px] font-medium {rw_cls}")>
                                        {rw_label}
                                    </span>
                                </div>
                                <p class="mt-1 text-[11px] leading-relaxed text-slate-300">
                                    {spec.description}
                                </p>
                            </div>
                        }
                    })
                    .collect_view()}
            </div>
        </details>
    }
}

/// Renders one complete [`AgentTurn`] with expandable tool-execution trace,
/// clickable cited clinical values, verbatim record citations, and working note.
#[component]
pub fn AgenticTurnCard(
    turn: AgentTurn,
    #[prop(default = None)] active_record: Option<RwSignal<Option<String>>>,
    #[prop(default = true)] expand_trace_by_default: bool,
) -> impl IntoView {
    let container_cls = if turn.grounded {
        "rounded-xl border border-white/10 bg-slate-950/75 p-4 space-y-3"
    } else {
        "rounded-xl border border-amber-400/30 bg-amber-500/5 p-4 space-y-3"
    };
    let outcome_cls = outcome_badge_class(&turn.outcome);
    let outcome_label = turn.outcome.label();
    let step_count = turn.steps.len();
    let max_steps = turn.budget.max_steps;
    let citation_count = turn.citations.len();
    let is_physician = turn.audience == AgentAudience::Physician;

    view! {
        <article data-agent-turn=turn.turn_index.to_string() class=container_cls>
            // Turn header: question, outcome badge, and engine provenance
            <div class="flex flex-wrap items-start justify-between gap-2 border-b border-white/10 pb-2.5">
                <div class="space-y-0.5">
                    <p class="text-[11px] font-mono uppercase tracking-wider text-slate-400">
                        {format!("Turn {} · {}", turn.turn_index, turn.engine_label)}
                    </p>
                    <p class="text-sm font-semibold text-white">{turn.question}</p>
                </div>
                <div class="flex flex-wrap items-center gap-1.5">
                    <span class=format!("rounded-full border px-2.5 py-0.5 text-[11px] font-semibold {outcome_cls}")>
                        {outcome_label}
                    </span>
                    <span class="rounded-full border border-white/15 bg-slate-900 px-2 py-0.5 font-mono text-[10px] text-slate-300">
                        {format!("{step_count}/{max_steps} steps")}
                    </span>
                </div>
            </div>

            // Expandable agent tool-execution trace
            {(!turn.steps.is_empty()).then(|| {
                let tool_chain: Vec<String> = turn
                    .steps
                    .iter()
                    .map(|s| s.tool_name.clone())
                    .collect();
                let steps_list = turn.steps.clone();
                view! {
                    <details
                        open=expand_trace_by_default
                        data-agent-trace="true"
                        class="rounded-lg border border-white/10 bg-slate-900/60 px-3 py-2 text-xs"
                    >
                        <summary class="cursor-pointer select-none list-none flex flex-wrap items-center justify-between gap-2 text-slate-200">
                            <span class="font-medium">
                                {format!(
                                    "Agent trace · {} step(s) · {} citation(s)",
                                    step_count, citation_count
                                )}
                            </span>
                            <span class="flex flex-wrap items-center gap-1">
                                {tool_chain
                                    .into_iter()
                                    .map(|name| {
                                        view! {
                                            <code class="rounded bg-white/10 px-1.5 py-0.5 font-mono text-[10px] text-cyan-200">
                                                {name}
                                            </code>
                                        }
                                    })
                                    .collect_view()}
                            </span>
                        </summary>
                        <ol class="mt-2.5 space-y-2 border-t border-white/10 pt-2.5">
                            {steps_list
                                .into_iter()
                                .map(|step| view! { <AgentStepRow step=step /> })
                                .collect_view()}
                        </ol>
                    </details>
                }
            })}

            // Clickable cited clinical values extracted from tool-read records
            {(!turn.cited_values.is_empty()).then(|| {
                let vals = turn.cited_values.clone();
                view! {
                    <div
                        data-cited-values="true"
                        class="flex flex-wrap items-center gap-1.5"
                    >
                        <span class="text-[11px] font-semibold uppercase tracking-wider text-slate-400">
                            "Cited values:"
                        </span>
                        {vals
                            .into_iter()
                            .map(|val| view! {
                                <CitedValueChip
                                    val=val
                                    link_to_record=is_physician || active_record.is_some()
                                    active_record=active_record
                                />
                            })
                            .collect_view()}
                    </div>
                }
            })}

            // Main grounded answer text
            <p class="whitespace-pre-line text-sm leading-relaxed text-slate-200">
                {turn.answer}
            </p>

            // Verbatim source record citations
            {(!turn.citations.is_empty()).then(|| {
                let citations = turn.citations.clone();
                if is_physician {
                    view! {
                        <div class="flex flex-wrap items-center gap-2 pt-1">
                            <span class="text-[11px] font-semibold uppercase tracking-wider text-slate-400">
                                "Citations:"
                            </span>
                            {citations
                                .into_iter()
                                .map(|cit| {
                                    let target_id = cit.record_id.clone();
                                    let href = format!("#record-{target_id}");
                                    let on_pick = move |_| {
                                        if let Some(sig) = active_record {
                                            sig.set(Some(target_id.clone()));
                                        }
                                        scroll_to_record(&target_id);
                                    };
                                    view! {
                                        <a
                                            href=href
                                            on:click=on_pick
                                            class="inline-flex items-center gap-1.5 rounded-full border border-cyan-400/35 bg-cyan-500/10 px-2.5 py-0.5 text-xs text-cyan-200 transition hover:bg-cyan-500/20"
                                        >
                                            <span class="font-mono text-[11px]">{cit.record_id}</span>
                                            <span>{cit.title}</span>
                                        </a>
                                    }
                                })
                                .collect_view()}
                        </div>
                    }
                    .into_any()
                } else {
                    view! {
                        <div class="space-y-2 border-t border-white/10 pt-2.5">
                            <p class="text-[11px] font-semibold uppercase tracking-wider text-cyan-300">
                                "Verbatim Citations (Same Synthetic Profile Only)"
                            </p>
                            <div class="space-y-2">
                                {citations
                                    .into_iter()
                                    .map(|cit| {
                                        view! {
                                            <div class="rounded-lg border border-cyan-400/25 bg-cyan-500/5 p-2.5 text-xs">
                                                <div class="flex flex-wrap items-center justify-between gap-2">
                                                    <span class="font-semibold text-white">
                                                        {cit.title}
                                                    </span>
                                                    <span class="font-mono text-[11px] text-cyan-200">
                                                        {format!("{} · {}", cit.record_id, cit.recorded_at)}
                                                    </span>
                                                </div>
                                                <p class="mt-1 text-slate-300">{cit.excerpt}</p>
                                            </div>
                                        }
                                    })
                                    .collect_view()}
                            </div>
                        </div>
                    }
                    .into_any()
                }
            })}

            // Working-note checkpoint saved for subsequent follow-up turns
            {turn.working_note.map(|note| {
                view! {
                    <div
                        data-working-note="true"
                        class="rounded-lg border border-emerald-400/25 bg-emerald-500/5 px-3 py-1.5 text-[11px] text-emerald-200"
                    >
                        <strong class="font-semibold text-emerald-300">
                            {format!("Working note (Turn {}): ", note.turn_index)}
                        </strong>
                        <span>{note.summary}</span>
                    </div>
                }
            })}

            <p class="text-[11px] text-slate-400">{turn.basis}</p>
        </article>
    }
}

#[component]
fn AgentStepRow(step: AgentStep) -> impl IntoView {
    let status_cls = step_status_class(&step.status);
    let status_label = step.status.label();
    view! {
        <li class="rounded-lg border border-white/5 bg-slate-950/70 p-2.5 space-y-1">
            <div class="flex flex-wrap items-center justify-between gap-2">
                <div class="flex flex-wrap items-center gap-1.5">
                    <span class="font-mono text-[10px] text-slate-400">
                        {format!("#{}", step.index)}
                    </span>
                    <code class="rounded bg-white/10 px-1.5 py-0.5 font-mono text-[11px] font-semibold text-cyan-200">
                        {step.tool_name}
                    </code>
                    <span class="text-xs font-medium text-white">{step.tool_label}</span>
                </div>
                <span class=format!("rounded-full border px-2 py-0.5 text-[10px] font-semibold {status_cls}")>
                    {status_label}
                </span>
            </div>
            <p class="font-mono text-[11px] text-slate-400">{step.arguments_summary}</p>
            <p class="text-xs text-slate-300">{step.observation_summary}</p>
        </li>
    }
}

#[component]
fn CitedValueChip(
    val: CitedValue,
    link_to_record: bool,
    #[prop(default = None)] active_record: Option<RwSignal<Option<String>>>,
) -> impl IntoView {
    let target_id = val.record_id.clone();
    let label_text = format!("{}: {}", val.label, val.value);
    if link_to_record {
        let href = format!("#record-{target_id}");
        let on_pick = move |_| {
            if let Some(sig) = active_record {
                sig.set(Some(target_id.clone()));
            }
            scroll_to_record(&target_id);
        };
        view! {
            <a
                href=href
                on:click=on_pick
                title=format!("{} ({})", val.record_title, val.recorded_at)
                class="inline-flex items-center gap-1 rounded-md border border-emerald-400/35 bg-emerald-500/10 px-2 py-0.5 font-mono text-[11px] text-emerald-200 transition hover:bg-emerald-500/20"
            >
                <span>{label_text}</span>
            </a>
        }
        .into_any()
    } else {
        view! {
            <span
                title=format!("{} ({})", val.record_title, val.recorded_at)
                class="inline-flex items-center gap-1 rounded-md border border-emerald-400/30 bg-emerald-500/10 px-2 py-0.5 font-mono text-[11px] text-emerald-200"
            >
                <span>{label_text}</span>
            </span>
        }
        .into_any()
    }
}

/// Renders an entire multi-turn [`AgentThread`] (newest turn first with its
/// trace expanded; earlier turns collapsed cleanly below).
#[component]
pub fn AgenticThreadView(
    thread: AgentThread,
    #[prop(optional)] active_record: Option<RwSignal<Option<String>>>,
) -> impl IntoView {
    let total_turns = thread.turns.len();
    view! {
        <div class="space-y-3">
            {thread
                .turns
                .into_iter()
                .rev()
                .enumerate()
                .map(|(rev_idx, turn)| {
                    view! {
                        <AgenticTurnCard
                            turn=turn
                            active_record=active_record
                            expand_trace_by_default=rev_idx == 0 && total_turns <= 3
                        />
                    }
                })
                .collect_view()}
        </div>
    }
}

#[cfg(all(test, feature = "ssr"))]
mod tests {
    use super::*;
    use clinical_workspace::{find_patient, run_agent_turn, AgentRunOptions};
    use leptos::prelude::RenderHtml;

    #[test]
    fn agentic_turn_card_renders_trace_cited_values_citations_and_working_note() {
        let nadia = find_patient("pt-nadia-kulkarni").expect("nadia");
        let turn = run_agent_turn(
            &nadia,
            "What is her latest HbA1c, CGM TIR, and active medications?",
            AgentAudience::Physician,
            1,
            &[],
            AgentRunOptions::default(),
        );
        let html = view! { <AgenticTurnCard turn=turn /> }.to_html();
        assert!(html.contains("chart_search"));
        assert!(html.contains("chart_read"));
        assert!(html.contains("medication_review"));
        assert!(html.contains("working_note_save"));
        assert!(html.contains("Cited values:"));
        assert!(html.contains("Working note (Turn 1):"));
        assert!(html.contains(r##"href="#record-nk-cgm-01""##));
        assert!(!html.contains("text-slate-500"));
    }

    #[test]
    fn tool_catalog_bar_lists_all_six_bounded_tools() {
        let html = view! { <AgentToolCatalogBar audience=AgentAudience::Physician /> }.to_html();
        for name in [
            "chart_search",
            "chart_read",
            "medication_review",
            "problem_list_review",
            "follow_up_review",
            "working_note_save",
        ] {
            assert!(html.contains(name), "missing tool {name} in catalog HTML");
        }
    }
}
