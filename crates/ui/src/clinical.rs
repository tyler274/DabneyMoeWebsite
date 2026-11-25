//! Clean-room Dabney.moe Clinical Workspace pages (`/clinical` and `/clinical/physician`).
//!
//! Backed directly by [`clinical_workspace`] with zero external runtime dependencies,
//! rendering identically in native SSR and browser WebAssembly. All displayed patient
//! charts are fictional synthetic examples containing zero real patient data.

use std::collections::{HashMap, HashSet};

use clinical_workspace::{
    answer_chart_question, synthetic_panel, today_overview, ChartRecord, ChatAnswer,
    FollowUpStanding, PatientChart, RecordKind, ReplyDraft, Urgency,
};
use leptos::prelude::*;
use leptos_meta::{Link, Meta, Title};
use leptos_router::components::A;
use serde::{Deserialize, Serialize};

use crate::sections::{Footer, NavBar};

/// `localStorage` key holding each patient's reply-draft decision.
#[cfg(any(feature = "csr", feature = "hydrate"))]
const DRAFT_STORAGE_KEY: &str = "dabney_clinical_draft_decisions";

/// DOM id of the selected patient's chart header, the scroll and focus
/// target after choosing a patient on a small screen.
const PATIENT_CHART_HEADER_ID: &str = "patient-chart-header";

const EMPTY_QUESTION_MESSAGE: &str =
    "Type a question about this patient's records, or pick a suggested question below.";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum DraftReviewState {
    #[default]
    PendingReview,
    ApprovedLocal,
    HeldForVisit,
}

impl DraftReviewState {
    const fn label(self) -> &'static str {
        match self {
            Self::PendingReview => "Awaiting clinician review",
            Self::ApprovedLocal => "Approved locally (demo)",
            Self::HeldForVisit => "Held for visit discussion",
        }
    }

    const fn badge_class(self) -> &'static str {
        match self {
            Self::PendingReview => "border-amber-400/30 bg-amber-500/15 text-amber-300",
            Self::ApprovedLocal => "border-emerald-400/30 bg-emerald-500/15 text-emerald-300",
            Self::HeldForVisit => "border-cyan-400/30 bg-cyan-500/15 text-cyan-300",
        }
    }
}

/// Each patient's reply-draft decision, kept per patient so switching charts
/// never carries one patient's decision over to another. Patients without an
/// entry are awaiting review.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
struct DraftDecisions(HashMap<String, DraftReviewState>);

impl DraftDecisions {
    fn get(&self, patient_id: &str) -> DraftReviewState {
        self.0.get(patient_id).copied().unwrap_or_default()
    }

    fn set(&mut self, patient_id: &str, state: DraftReviewState) {
        match state {
            DraftReviewState::PendingReview => {
                self.0.remove(patient_id);
            }
            DraftReviewState::ApprovedLocal | DraftReviewState::HeldForVisit => {
                self.0.insert(patient_id.to_string(), state);
            }
        }
    }

    #[cfg(any(feature = "csr", feature = "hydrate", test))]
    fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string(self)
    }

    #[cfg(any(feature = "csr", feature = "hydrate", test))]
    fn from_json(raw: &str) -> serde_json::Result<Self> {
        serde_json::from_str(raw)
    }
}

/// Reads saved draft decisions. A missing entry reads as `None`; an
/// unreadable one is reported and also ignored, so a corrupt entry from an
/// older build cannot break the page.
#[cfg(any(feature = "csr", feature = "hydrate"))]
fn load_draft_decisions() -> Option<DraftDecisions> {
    let storage = web_sys::window()?.local_storage().ok()??;
    let raw = storage.get_item(DRAFT_STORAGE_KEY).ok()??;
    match DraftDecisions::from_json(&raw) {
        Ok(decisions) => Some(decisions),
        Err(err) => {
            leptos::logging::warn!("clinical: ignoring unreadable saved draft decisions: {err}");
            None
        }
    }
}

#[cfg(any(feature = "csr", feature = "hydrate"))]
fn store_draft_decisions(decisions: &DraftDecisions) {
    let json = match decisions.to_json() {
        Ok(json) => json,
        Err(err) => {
            leptos::logging::warn!("clinical: could not encode draft decisions: {err}");
            return;
        }
    };
    let stored = web_sys::window()
        .and_then(|w| w.local_storage().ok().flatten())
        .map(|storage| storage.set_item(DRAFT_STORAGE_KEY, &json));
    if !matches!(stored, Some(Ok(()))) {
        leptos::logging::warn!("clinical: draft decision was not saved to localStorage");
    }
}

/// Scrolls to and focuses the chart header after a patient is chosen on a
/// screen too narrow to show the chart beside the selector (below Tailwind's
/// `lg` breakpoint).
#[cfg(any(feature = "csr", feature = "hydrate"))]
fn reveal_chart_on_small_screens() {
    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::JsCast;

    const LG_BREAKPOINT_PX: f64 = 1024.0;

    let Some(window) = web_sys::window() else {
        return;
    };
    let narrow = window
        .inner_width()
        .ok()
        .and_then(|w| w.as_f64())
        .is_some_and(|w| w < LG_BREAKPOINT_PX);
    if !narrow {
        return;
    }
    // Wait a frame so the newly selected chart has rendered.
    let reveal = Closure::once(Box::new(|| {
        let Some(header) = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.get_element_by_id(PATIENT_CHART_HEADER_ID))
        else {
            return;
        };
        // `scroll-smooth` on <html> makes this a smooth scroll.
        header.scroll_into_view();
        if let Ok(header) = header.dyn_into::<web_sys::HtmlElement>() {
            let options = web_sys::FocusOptions::new();
            options.set_prevent_scroll(true);
            let _ = header.focus_with_options(&options);
        }
    }) as Box<dyn FnMut()>);
    let _ = window.request_animation_frame(reveal.as_ref().unchecked_ref());
    reveal.forget();
}

/// "1 open follow-up", "2 open follow-ups".
fn count_label(count: usize, singular: &str, plural: &str) -> String {
    if count == 1 {
        format!("1 {singular}")
    } else {
        format!("{count} {plural}")
    }
}

fn open_follow_ups_label(count: usize) -> String {
    count_label(count, "open follow-up", "open follow-ups")
}

const fn urgency_badge_class(urgency: Urgency) -> &'static str {
    match urgency {
        Urgency::High => "border-rose-400/30 bg-rose-500/15 text-rose-300",
        Urgency::Moderate => "border-amber-400/30 bg-amber-500/15 text-amber-300",
        Urgency::Routine => "border-emerald-400/30 bg-emerald-500/15 text-emerald-300",
    }
}

const fn standing_badge_class(standing: FollowUpStanding) -> &'static str {
    match standing {
        FollowUpStanding::DueToday => "border-rose-400/30 bg-rose-500/15 text-rose-300",
        FollowUpStanding::Monitoring => "border-cyan-400/30 bg-cyan-500/15 text-cyan-300",
        FollowUpStanding::Completed => "border-emerald-400/30 bg-emerald-500/15 text-emerald-300",
    }
}

const fn record_kind_badge_class(kind: RecordKind) -> &'static str {
    match kind {
        RecordKind::LabResult => "border-violet-400/30 bg-violet-500/15 text-violet-300",
        RecordKind::ProgressNote => "border-cyan-400/30 bg-cyan-500/15 text-cyan-300",
        RecordKind::CgmSummary => "border-emerald-400/30 bg-emerald-500/15 text-emerald-300",
        RecordKind::PatientMessage => "border-amber-400/30 bg-amber-500/15 text-amber-300",
        RecordKind::Imaging => "border-sky-400/30 bg-sky-500/15 text-sky-300",
    }
}

fn preset_questions_for_patient(patient_id: &str) -> &'static [&'static str] {
    match patient_id {
        "pt-nadia-kulkarni" => &[
            "What does the 14-day CGM show about morning fasting glucose?",
            "Summarize HbA1c, eGFR, and metformin regimen",
            "What did the patient ask about metformin timing?",
        ],
        "pt-carlos-mendez" => &[
            "What is the 14-day home blood pressure average?",
            "Review eGFR, potassium, and urine ACR labs",
            "Any ankle edema or symptoms on amlodipine?",
        ],
        "pt-priya-nair" => &[
            "How did PHQ-9, GAD-7, and ASRM scores change?",
            "Summarize CGM time in range and post-lunch spikes",
        ],
        "pt-david-okafor" => &[
            "What did the transthoracic echocardiogram show?",
            "Review hemoglobin, renal panel, and apixaban dosing",
        ],
        _ => &["Summarize recent records and active medications"],
    }
}

/// Overview page for the Dabney.moe Clinical Workspace (`/clinical`).
#[component]
pub fn ClinicalOverviewPage() -> impl IntoView {
    let overview = today_overview();

    view! {
        <Title text="Dabney.moe Clinical Workspace — Local-First Rust & WASM | dabney.moe" />
        <Meta
            name="description"
            content="Clean-room Dabney.moe Clinical Workspace built in pure Rust and WebAssembly: synthetic zero-PHI physician chart review, verbatim record citations, and local-first metabolic & mental health tracking."
        />
        <Link rel="canonical" href="https://dabney.moe/clinical" />

        <NavBar />

        <div class="mx-auto max-w-5xl px-6 py-12 space-y-12">
            // Hero
            <section class="relative overflow-hidden rounded-3xl border border-white/10 bg-gradient-to-br from-slate-900/90 via-slate-950 to-slate-950 p-8 sm:p-10">
                <div class="pointer-events-none absolute inset-0 -z-10 bg-[radial-gradient(60%_50%_at_80%_0%,rgba(34,211,238,0.14),transparent)]"></div>
                <div class="flex flex-col gap-6 sm:flex-row sm:items-start sm:justify-between">
                    <div class="max-w-2xl space-y-4">
                        <div class="inline-flex items-center gap-3">
                            <div class="flex h-12 w-12 shrink-0 items-center justify-center rounded-2xl border border-emerald-400/30 bg-emerald-500/10 p-2">
                                <img
                                    src="/dabney-elephant.svg"
                                    alt="Dabney.moe elephant emblem"
                                    class="h-full w-full object-contain"
                                />
                            </div>
                            <p class="text-xs font-semibold uppercase tracking-widest text-cyan-400">
                                "Dabney.moe · Clean-Room Clinical Systems"
                            </p>
                        </div>
                        <h1 class="text-3xl font-bold tracking-tight text-white sm:text-4xl">
                            "Dabney.moe Clinical Workspace"
                        </h1>
                        <p class="text-base leading-relaxed text-slate-300">
                            "A local-first Rust and WebAssembly clinical systems showcase engineered from scratch: "
                            "typed longitudinal patient charts, strict per-patient citation boundaries, and browser-native CGM & mood analytics. "
                            "The physician demo runs its chart search entirely in your browser via WebAssembly and sends no chart data over the network; "
                            "the same crate also serves a read-only JSON API at /clinical/api for scripted access."
                        </p>
                        <div class="flex flex-wrap gap-3 pt-2">
                            <A
                                href="/clinical/physician"
                                attr:class="inline-flex items-center gap-2 rounded-full bg-cyan-500 px-6 py-2.5 text-sm font-semibold text-slate-950 transition hover:bg-cyan-400"
                            >
                                "Open Physician Workspace →"
                            </A>
                            <A
                                href="/mental-health"
                                attr:class="inline-flex items-center gap-2 rounded-full border border-emerald-400/30 bg-emerald-500/10 px-5 py-2.5 text-sm font-semibold text-emerald-200 transition hover:bg-emerald-500/20"
                            >
                                "Mental Health & CGM Tracker"
                            </A>
                        </div>
                    </div>
                </div>
            </section>

            // Synthetic Zero-PHI Guarantee Banner
            <section
                class="rounded-2xl border border-emerald-400/30 bg-emerald-500/10 px-6 py-5"
                role="note"
            >
                <div class="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
                    <div>
                        <p class="text-xs font-semibold uppercase tracking-widest text-emerald-300">
                            "Synthetic Zero-PHI Guarantee"
                        </p>
                        <p class="mt-1 text-sm leading-relaxed text-slate-200">
                            {overview.synthetic_notice.clone()}
                            " No real patient identifiers, EHR credentials, or protected health information are stored, transmitted, or logged anywhere in this workspace."
                        </p>
                    </div>
                    <span class="shrink-0 self-start rounded-full border border-emerald-400/40 bg-slate-950/60 px-3 py-1 text-xs font-semibold text-emerald-300 sm:self-center">
                        "100% Synthetic"
                    </span>
                </div>
            </section>

            // Architecture Pillars
            <section class="space-y-4">
                <div>
                    <p class="text-xs font-semibold uppercase tracking-widest text-cyan-400">
                        "Architecture"
                    </p>
                    <h2 class="mt-1 text-2xl font-bold tracking-tight text-white">
                        "Local-First Rust & WebAssembly Design"
                    </h2>
                </div>
                <div class="grid gap-5 md:grid-cols-3">
                    <article class="rounded-2xl border border-white/10 bg-white/[0.02] p-6">
                        <p class="text-xs font-semibold uppercase tracking-wider text-cyan-300">
                            "01 · Shared Domain Crate"
                        </p>
                        <h3 class="mt-2 text-lg font-semibold text-white">
                            "Identical SSR & WASM Execution"
                        </h3>
                        <p class="mt-2 text-sm leading-relaxed text-slate-400">
                            "A single dependency-light Rust crate models charts, follow-ups, reviewable draft replies, "
                            "and deterministic whole-word chart search. This site runs it in your browser via WebAssembly; the server exposes the same functions as a JSON API."
                        </p>
                    </article>
                    <article class="rounded-2xl border border-white/10 bg-white/[0.02] p-6">
                        <p class="text-xs font-semibold uppercase tracking-wider text-emerald-300">
                            "02 · Verbatim Provenance"
                        </p>
                        <h3 class="mt-2 text-lg font-semibold text-white">
                            "Patient-Scoped Record Citations"
                        </h3>
                        <p class="mt-2 text-sm leading-relaxed text-slate-400">
                            "Every chart question is answered exclusively from the selected patient's longitudinal source records, "
                            "returning exact record IDs and verbatim excerpts with zero cross-patient leakage."
                        </p>
                    </article>
                    <article class="rounded-2xl border border-white/10 bg-white/[0.02] p-6">
                        <p class="text-xs font-semibold uppercase tracking-wider text-violet-300">
                            "03 · On-Device Self-Monitoring"
                        </p>
                        <h3 class="mt-2 text-lg font-semibold text-white">
                            "Browser-Only CGM & Cadence Tools"
                        </h3>
                        <p class="mt-2 text-sm leading-relaxed text-slate-400">
                            "Pair commercial CGM/BGM exports with mood, PHQ-9, GAD-7, and ASRM tracking on "
                            <A href="/mental-health" attr:class="text-cyan-300 underline decoration-cyan-400/40 underline-offset-2 hover:text-cyan-200">
                                "/mental-health"
                            </A>
                            " where files stay entirely inside local browser WASM."
                        </p>
                    </article>
                </div>
            </section>

            // Built-in Synthetic Clinic Preview
            <section class="space-y-4">
                <div class="flex flex-wrap items-end justify-between gap-3">
                    <div>
                        <p class="text-xs font-semibold uppercase tracking-widest text-cyan-400">
                            {format!("{} · {}", overview.practice_name, overview.clinic_date)}
                        </p>
                        <h2 class="mt-1 text-2xl font-bold tracking-tight text-white">
                            "Synthetic Clinic Panel"
                        </h2>
                    </div>
                    <A
                        href="/clinical/physician"
                        attr:class="text-sm font-semibold text-cyan-300 transition hover:text-cyan-200"
                    >
                        "Launch interactive physician demo →"
                    </A>
                </div>
                <div class="grid gap-4 sm:grid-cols-2">
                    {overview
                        .patients
                        .into_iter()
                        .map(|patient| {
                            let visit_label = patient
                                .visit_time
                                .map(|t| format!("Visit {t}"))
                                .unwrap_or_else(|| "Panel review".to_string());
                            let urgency_cls = urgency_badge_class(patient.urgency);
                            view! {
                                <div class="rounded-2xl border border-white/10 bg-white/[0.02] p-5 transition hover:border-cyan-400/35">
                                    <div class="flex items-start justify-between gap-3">
                                        <div>
                                            <h3 class="text-base font-semibold text-white">{patient.display_name}</h3>
                                            <p class="font-mono text-xs text-slate-400">
                                                {format!("{} · DOB {}", patient.mrn, patient.birth_date)}
                                            </p>
                                        </div>
                                        <span class=format!("rounded-full border px-2.5 py-0.5 text-[11px] font-semibold {urgency_cls}")>
                                            {patient.urgency.label()}
                                        </span>
                                    </div>
                                    <p class="mt-3 text-sm text-slate-300">{patient.primary_focus}</p>
                                    <div class="mt-4 flex flex-wrap items-center gap-3 text-xs text-slate-400">
                                        <span class="rounded-md bg-white/5 px-2 py-0.5 text-cyan-300">{visit_label}</span>
                                        <span>{count_label(patient.record_count, "source record", "source records")}</span>
                                        <span>{open_follow_ups_label(patient.open_follow_ups)}</span>
                                    </div>
                                </div>
                            }
                        })
                        .collect_view()}
                </div>
            </section>
        </div>

        <Footer />
    }
}

/// Interactive Physician Workspace demo (`/clinical/physician`).
#[component]
pub fn PhysicianWorkspacePage() -> impl IntoView {
    let panel = StoredValue::new(synthetic_panel());
    let overview = today_overview();

    let (initial_id, initial_question, initial_answer) = panel.with_value(|charts| {
        if let Some(first) = charts.first() {
            let q = preset_questions_for_patient(&first.id)
                .first()
                .copied()
                .unwrap_or("Summarize recent records")
                .to_string();
            let ans = answer_chart_question(first, &q);
            (first.id.clone(), q, Some(ans))
        } else {
            (String::new(), String::new(), None)
        }
    });

    let selected_id = RwSignal::new(initial_id);
    let question_input = RwSignal::new(initial_question);
    let question_error = RwSignal::new(false);
    let latest_answer = RwSignal::new(initial_answer);
    let active_record = RwSignal::new(None::<String>);
    // Server render and first hydration pass start with every draft pending,
    // so the markup matches; saved decisions are applied once mounted.
    let draft_decisions = RwSignal::new(DraftDecisions::default());

    #[cfg(any(feature = "csr", feature = "hydrate"))]
    Effect::new(move |_| {
        if let Some(saved) = load_draft_decisions() {
            draft_decisions.set(saved);
        }
    });

    let cited_ids = Memo::new(move |_| {
        let active_id = selected_id.get();
        latest_answer.with(|answer| {
            answer
                .as_ref()
                .filter(|ans| ans.patient_id == active_id)
                .map(|ans| ans.citations.iter().map(|c| c.record_id.clone()).collect())
                .unwrap_or_default()
        })
    });

    let answer_for = move |patient_id: &str, question: &str| {
        panel.with_value(|charts| {
            charts
                .iter()
                .find(|p| p.id == patient_id)
                .map(|p| answer_chart_question(p, question))
        })
    };

    let select_patient = move |patient_id: String| {
        let default_q = preset_questions_for_patient(&patient_id)
            .first()
            .copied()
            .unwrap_or("Summarize recent records")
            .to_string();
        latest_answer.set(answer_for(&patient_id, &default_q));
        question_input.set(default_q);
        question_error.set(false);
        active_record.set(None);
        selected_id.set(patient_id);
        #[cfg(any(feature = "csr", feature = "hydrate"))]
        reveal_chart_on_small_screens();
    };

    let ask_question = move |raw_q: String| {
        let trimmed = raw_q.trim();
        if trimmed.is_empty() {
            question_error.set(true);
            return;
        }
        question_error.set(false);
        active_record.set(None);
        latest_answer.set(answer_for(&selected_id.get_untracked(), trimmed));
    };

    let on_submit_question = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        ask_question(question_input.get_untracked());
    };

    view! {
        <Title text="Physician Workspace Demo — Dabney.moe Clinical Workspace | dabney.moe" />
        <Meta
            name="description"
            content="Interactive zero-PHI synthetic physician workspace in Rust and WebAssembly: today's clinic queue, 4 synthetic longitudinal charts, reviewable draft replies, and record-cited chart Q&A."
        />
        <Link rel="canonical" href="https://dabney.moe/clinical/physician" />

        <NavBar />

        <div class="mx-auto max-w-6xl px-6 py-10 space-y-6">
            // Top bar
            <header class="flex flex-wrap items-center justify-between gap-4 border-b border-white/10 pb-6">
                <div class="flex items-center gap-4">
                    <div class="flex h-12 w-12 shrink-0 items-center justify-center rounded-2xl border border-emerald-400/30 bg-emerald-500/10 p-2">
                        <img
                            src="/dabney-elephant.svg"
                            alt="Dabney.moe elephant emblem"
                            class="h-full w-full object-contain"
                        />
                    </div>
                    <div>
                        <p class="text-xs font-semibold uppercase tracking-widest text-cyan-400">
                            {format!("{} · {}", overview.practice_name, overview.clinic_date)}
                        </p>
                        <h1 class="text-2xl font-bold tracking-tight text-white sm:text-3xl">
                            "Physician Workspace Demo"
                        </h1>
                    </div>
                </div>
                <div class="flex flex-wrap items-center gap-2.5 text-xs">
                    <A
                        href="/clinical"
                        attr:class="rounded-full border border-white/15 px-4 py-2 font-medium text-slate-200 transition hover:bg-white/5"
                    >
                        "← Workspace Overview"
                    </A>
                    <A
                        href="/mental-health"
                        attr:class="rounded-full border border-emerald-400/30 bg-emerald-500/10 px-4 py-2 font-medium text-emerald-200 transition hover:bg-emerald-500/20"
                    >
                        "Mental Health & CGM"
                    </A>
                </div>
            </header>

            // Synthetic-data banner
            <div
                class="flex flex-wrap items-center justify-between gap-3 rounded-2xl border border-emerald-400/30 bg-emerald-500/10 px-5 py-3.5"
                role="note"
            >
                <p class="text-xs leading-relaxed text-emerald-100 sm:text-sm">
                    <span class="font-semibold text-emerald-300">"Synthetic Zero-PHI Demo: "</span>
                    {overview.synthetic_notice.clone()}
                    " Chart search runs deterministically in Rust/WASM in your browser, with zero external services."
                </p>
                <span class="rounded-full border border-emerald-400/40 bg-slate-950/60 px-3 py-1 text-[11px] font-semibold uppercase tracking-wider text-emerald-300">
                    "Offline-Ready"
                </span>
            </div>

            // Today's clinic schedule / queue: the one patient selector on small screens.
            <section class="rounded-2xl border border-white/10 bg-white/[0.02] p-5" aria-labelledby="clinic-queue-heading">
                <div class="mb-3 flex flex-wrap items-center justify-between gap-2">
                    <h2 id="clinic-queue-heading" class="text-sm font-semibold uppercase tracking-wider text-cyan-300">
                        "Today's Clinic Schedule & Queue"
                    </h2>
                    <span class="text-xs text-slate-400">
                        {format!("{} synthetic charts in panel", overview.patients.len())}
                    </span>
                </div>
                <div class="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
                    {overview
                        .patients
                        .into_iter()
                        .map(|summary| {
                            let pid_click = summary.id.clone();
                            let pid_active = summary.id.clone();
                            let is_selected = Memo::new(move |_| selected_id.with(|id| *id == pid_active));
                            let urgency_cls = urgency_badge_class(summary.urgency);
                            let time_text = summary
                                .visit_time
                                .unwrap_or_else(|| "Panel review".to_string());
                            view! {
                                <button
                                    type="button"
                                    aria-pressed=move || is_selected.get().to_string()
                                    on:click=move |_| select_patient(pid_click.clone())
                                    class=move || {
                                        if is_selected.get() {
                                            "w-full rounded-xl border border-cyan-400/60 bg-cyan-500/10 p-3.5 text-left transition"
                                        } else {
                                            "w-full rounded-xl border border-white/10 bg-slate-950/60 p-3.5 text-left transition hover:border-white/25 hover:bg-white/[0.04]"
                                        }
                                    }
                                >
                                    <div class="flex items-center justify-between gap-2">
                                        <span class="font-mono text-xs font-semibold text-cyan-300">
                                            {time_text}
                                        </span>
                                        <span class=format!("rounded-full border px-2 py-0.5 text-[10px] font-semibold {urgency_cls}")>
                                            {summary.urgency.label()}
                                        </span>
                                    </div>
                                    <p class="mt-1.5 text-sm font-semibold text-white">{summary.display_name}</p>
                                    <p class="mt-0.5 line-clamp-1 text-xs text-slate-400">{summary.primary_focus}</p>
                                    <p class="mt-2 text-[11px] text-slate-400">
                                        {format!(
                                            "{} · {}",
                                            count_label(summary.record_count, "record", "records"),
                                            open_follow_ups_label(summary.open_follow_ups),
                                        )}
                                    </p>
                                </button>
                            }
                        })
                        .collect_view()}
                </div>
            </section>

            // Main workspace: Patient selector + Selected patient chart
            <div class="grid gap-6 lg:grid-cols-12">
                // Desktop sidebar selector. Hidden below `lg`, where the queue
                // above is the single selector; sticky below the navbar.
                <aside class="hidden lg:col-span-4 lg:block lg:sticky lg:top-24 lg:self-start lg:max-h-[calc(100vh-7rem)] lg:overflow-y-auto">
                    <div class="rounded-2xl border border-white/10 bg-white/[0.02] p-4">
                        <h2 class="mb-3 text-xs font-semibold uppercase tracking-widest text-slate-400">
                            "Patient Panel"
                        </h2>
                        <div class="space-y-2.5">
                            {panel.with_value(|charts| {
                                charts
                                    .iter()
                                    .map(|patient| {
                                        let pid_click = patient.id.clone();
                                        let pid_active = patient.id.clone();
                                        let is_selected = Memo::new(move |_| selected_id.with(|id| *id == pid_active));
                                        let urgency_cls = urgency_badge_class(patient.urgency);
                                        let visit_note = patient
                                            .visit
                                            .as_ref()
                                            .map(|v| format!("{} · {}", v.starts_at, v.modality))
                                            .unwrap_or_else(|| "Asynchronous review".to_string());
                                        let has_draft = patient.reply_draft.is_some();
                                        view! {
                                            <button
                                                type="button"
                                                aria-pressed=move || is_selected.get().to_string()
                                                on:click=move |_| select_patient(pid_click.clone())
                                                class=move || {
                                                    if is_selected.get() {
                                                        "w-full rounded-xl border border-cyan-400/60 bg-cyan-500/10 p-3.5 text-left transition"
                                                    } else {
                                                        "w-full rounded-xl border border-white/10 bg-slate-950/50 p-3.5 text-left transition hover:border-white/25 hover:bg-white/[0.03]"
                                                    }
                                                }
                                            >
                                                <div class="flex items-start justify-between gap-2">
                                                    <div>
                                                        <p class="text-sm font-semibold text-white">{patient.display_name.clone()}</p>
                                                        <p class="font-mono text-[11px] text-slate-400">
                                                            {format!("{} · DOB {}", patient.mrn, patient.birth_date)}
                                                        </p>
                                                    </div>
                                                    <span class=format!("shrink-0 rounded-full border px-2 py-0.5 text-[10px] font-semibold {urgency_cls}")>
                                                        {patient.urgency.label()}
                                                    </span>
                                                </div>
                                                <p class="mt-2 text-xs text-slate-300">{patient.primary_focus.clone()}</p>
                                                <div class="mt-2.5 flex flex-wrap items-center gap-2 text-[11px] text-slate-400">
                                                    <span class="rounded bg-white/5 px-1.5 py-0.5 text-cyan-300">{visit_note}</span>
                                                    {has_draft.then(|| view! {
                                                        <span class="rounded border border-amber-400/30 bg-amber-500/10 px-1.5 py-0.5 text-amber-300">
                                                            "Draft reply"
                                                        </span>
                                                    })}
                                                </div>
                                            </button>
                                        }
                                    })
                                    .collect_view()
                            })}
                        </div>
                    </div>
                </aside>

                // Right column: Selected patient chart & grounded Q&A. Rebuilt
                // only when the selected patient changes; answers, citations and
                // draft decisions update in place so live regions stay mounted.
                <section class="lg:col-span-8 space-y-6">
                    {move || {
                        let active_id = selected_id.get();
                        let active_patient: Option<PatientChart> = panel.with_value(|charts| {
                            charts.iter().find(|p| p.id == active_id).cloned()
                        });
                        let Some(patient) = active_patient else {
                            return view! {
                                <div class="rounded-2xl border border-white/10 bg-white/[0.02] p-6 text-sm text-slate-400">
                                    "Select a synthetic patient from the panel."
                                </div>
                            }
                            .into_any();
                        };

                        let presets = preset_questions_for_patient(&patient.id);
                        let urgency_cls = urgency_badge_class(patient.urgency);
                        let answer_patient_id = patient.id.clone();

                        view! {
                            <div class="space-y-6">
                                // Selected patient header
                                <div
                                    id=PATIENT_CHART_HEADER_ID
                                    role="region"
                                    tabindex="-1"
                                    class="scroll-mt-24 rounded-2xl border border-white/10 bg-white/[0.02] p-6 focus:outline-none focus-visible:ring-2 focus-visible:ring-cyan-400/60"
                                    aria-label=format!("Chart for {}", patient.display_name)
                                >
                                    <div class="flex flex-wrap items-start justify-between gap-4">
                                        <div>
                                            <div class="flex flex-wrap items-center gap-2.5">
                                                <h2 class="text-2xl font-bold text-white">{patient.display_name.clone()}</h2>
                                                <span class="rounded-md border border-white/10 bg-slate-900 px-2 py-0.5 font-mono text-xs text-cyan-300">
                                                    {patient.mrn.clone()}
                                                </span>
                                                <span class=format!("rounded-full border px-2.5 py-0.5 text-xs font-semibold {urgency_cls}")>
                                                    {patient.urgency.label()}
                                                </span>
                                            </div>
                                            <p class="mt-1 text-xs text-slate-400">
                                                {format!("DOB {} · {}", patient.birth_date, patient.primary_focus)}
                                            </p>
                                        </div>
                                        {patient.visit.clone().map(|slot| view! {
                                            <div class="rounded-xl border border-cyan-400/25 bg-cyan-500/10 px-3.5 py-2 text-right">
                                                <p class="font-mono text-xs font-semibold text-cyan-300">
                                                    {format!("{} · {}", slot.starts_at, slot.modality)}
                                                </p>
                                                <p class="text-xs text-slate-200">{slot.reason}</p>
                                            </div>
                                        })}
                                    </div>
                                    <p class="mt-4 text-sm leading-relaxed text-slate-200">
                                        {patient.summary.clone()}
                                    </p>
                                </div>

                                // Interactive Chart-Grounded Q&A
                                <div class="rounded-2xl border border-cyan-400/25 bg-slate-900/50 p-6 space-y-4">
                                    <div>
                                        <p class="text-xs font-semibold uppercase tracking-widest text-cyan-400">
                                            "Chart-Grounded Q&A"
                                        </p>
                                        <h3 class="mt-1 text-lg font-semibold text-white">
                                            "Ask About This Patient's Records"
                                        </h3>
                                        <p class="mt-1 text-xs text-slate-400">
                                            "Deterministic whole-word retrieval scoped strictly to "
                                            <span class="font-mono text-slate-200">{patient.mrn.clone()}</span>
                                            ". Questions with no matching record say so and cite nothing."
                                        </p>
                                    </div>

                                    <form on:submit=on_submit_question class="space-y-2" novalidate>
                                        <div class="flex flex-col gap-2.5 sm:flex-row">
                                            <input
                                                type="text"
                                                aria-label="Ask a clinical question about the selected chart"
                                                aria-invalid=move || question_error.get().to_string()
                                                aria-describedby="chart-question-error"
                                                placeholder="Ask about labs, CGM trends, medications, or messages..."
                                                prop:value=move || question_input.get()
                                                on:input=move |ev| {
                                                    question_input.set(event_target_value(&ev));
                                                    question_error.set(false);
                                                }
                                                class=move || {
                                                    let border = if question_error.get() {
                                                        "border-rose-400/70"
                                                    } else {
                                                        "border-white/15"
                                                    };
                                                    format!("flex-1 rounded-xl border {border} bg-slate-950 px-3.5 py-2.5 text-sm text-white placeholder:text-slate-400 focus:border-cyan-400/60 focus:outline-none focus:ring-1 focus:ring-cyan-400/50")
                                                }
                                            />
                                            <button
                                                type="submit"
                                                class="rounded-full bg-cyan-500 px-5 py-2.5 text-sm font-semibold text-slate-950 transition hover:bg-cyan-400"
                                            >
                                                "Ask chart"
                                            </button>
                                        </div>
                                        <p id="chart-question-error" class="min-h-[1rem] text-xs text-rose-300" role="alert">
                                            {move || question_error.get().then_some(EMPTY_QUESTION_MESSAGE)}
                                        </p>
                                    </form>

                                    <div class="flex flex-wrap gap-2">
                                        {presets
                                            .iter()
                                            .map(|&preset| {
                                                view! {
                                                    <button
                                                        type="button"
                                                        on:click=move |_| {
                                                            let q = preset.to_string();
                                                            question_input.set(q.clone());
                                                            ask_question(q);
                                                        }
                                                        class="rounded-full border border-white/15 bg-slate-950/70 px-3 py-1 text-xs text-slate-300 transition hover:border-cyan-400/40 hover:text-white"
                                                    >
                                                        {preset}
                                                    </button>
                                                }
                                            })
                                            .collect_view()}
                                    </div>

                                    <div aria-live="polite" aria-label="Chart answer">
                                        {move || {
                                            latest_answer
                                                .get()
                                                .filter(|ans| ans.patient_id == answer_patient_id)
                                                .map(|ans| view! { <GroundedAnswerView answer=ans active_record=active_record /> })
                                        }}
                                    </div>
                                </div>

                                // Problems, Medications, Follow-ups
                                <div class="grid gap-4 md:grid-cols-3">
                                    <div class="rounded-2xl border border-white/10 bg-white/[0.02] p-5">
                                        <h3 class="text-xs font-semibold uppercase tracking-widest text-cyan-300">
                                            "Problems"
                                        </h3>
                                        <ul class="mt-3 space-y-2.5">
                                            {patient
                                                .problems
                                                .iter()
                                                .map(|prob| {
                                                    view! {
                                                        <li class="rounded-xl border border-white/5 bg-slate-950/50 p-3">
                                                            <div class="flex items-center justify-between gap-2">
                                                                <span class="font-mono text-[11px] font-semibold text-cyan-300">
                                                                    {prob.code.clone()}
                                                                </span>
                                                                <span class="text-[11px] text-emerald-300">
                                                                    {prob.status.clone()}
                                                                </span>
                                                            </div>
                                                            <p class="mt-1 text-xs font-medium text-slate-200">
                                                                {prob.label.clone()}
                                                            </p>
                                                        </li>
                                                    }
                                                })
                                                .collect_view()}
                                        </ul>
                                    </div>

                                    <div class="rounded-2xl border border-white/10 bg-white/[0.02] p-5">
                                        <h3 class="text-xs font-semibold uppercase tracking-widest text-emerald-300">
                                            "Active Medications"
                                        </h3>
                                        <ul class="mt-3 space-y-2.5">
                                            {patient
                                                .medications
                                                .iter()
                                                .map(|med| {
                                                    view! {
                                                        <li class="rounded-xl border border-white/5 bg-slate-950/50 p-3">
                                                            <p class="text-xs font-semibold text-white">
                                                                {format!("{} {}", med.name, med.dose)}
                                                            </p>
                                                            <p class="mt-0.5 text-[11px] text-slate-300">
                                                                {med.schedule.clone()}
                                                            </p>
                                                            <p class="mt-1 text-[11px] text-slate-400">
                                                                {med.indication.clone()}
                                                            </p>
                                                        </li>
                                                    }
                                                })
                                                .collect_view()}
                                        </ul>
                                    </div>

                                    <div class="rounded-2xl border border-white/10 bg-white/[0.02] p-5">
                                        <h3 class="text-xs font-semibold uppercase tracking-widest text-amber-300">
                                            "Follow-Ups"
                                        </h3>
                                        <ul class="mt-3 space-y-2.5">
                                            {patient
                                                .follow_ups
                                                .iter()
                                                .map(|fu| {
                                                    let standing_cls = standing_badge_class(fu.standing);
                                                    view! {
                                                        <li class="rounded-xl border border-white/5 bg-slate-950/50 p-3">
                                                            <div class="flex items-center justify-between gap-2">
                                                                <span class=format!("rounded-full border px-2 py-0.5 text-[10px] font-semibold {standing_cls}")>
                                                                    {fu.standing.label()}
                                                                </span>
                                                                <span class="font-mono text-[11px] text-slate-400">
                                                                    {fu.next_check.clone()}
                                                                </span>
                                                            </div>
                                                            <p class="mt-1.5 text-xs text-slate-200">
                                                                {fu.summary.clone()}
                                                            </p>
                                                        </li>
                                                    }
                                                })
                                                .collect_view()}
                                        </ul>
                                    </div>
                                </div>

                                // Optional reviewable draft reply card
                                {patient.reply_draft.clone().map(|draft| {
                                    view! {
                                        <ReplyDraftCard
                                            draft=draft
                                            patient_id=patient.id.clone()
                                            decisions=draft_decisions
                                        />
                                    }
                                })}

                                // Longitudinal source records
                                <div class="rounded-2xl border border-white/10 bg-white/[0.02] p-6 space-y-4">
                                    <div class="flex flex-wrap items-center justify-between gap-2">
                                        <div>
                                            <h3 class="text-lg font-semibold text-white">
                                                "Longitudinal Source Records"
                                            </h3>
                                            <p class="text-xs text-slate-400">
                                                "Labs, CGM summaries, progress notes, imaging, and inbound messages. Records cited by the active Q&A response are highlighted."
                                            </p>
                                        </div>
                                        <span class="rounded-full border border-white/10 bg-slate-950 px-3 py-1 font-mono text-xs text-slate-300">
                                            {count_label(patient.records.len(), "record", "records")}
                                        </span>
                                    </div>
                                    <div class="space-y-3">
                                        {patient
                                            .records
                                            .into_iter()
                                            .map(|rec| {
                                                view! {
                                                    <SourceRecordCard
                                                        record=rec
                                                        cited_ids=cited_ids
                                                        active_record=active_record
                                                    />
                                                }
                                            })
                                            .collect_view()}
                                    </div>
                                </div>
                            </div>
                        }
                        .into_any()
                    }}
                </section>
            </div>
        </div>

        <Footer />
    }
}

/// DOM id of a source record card, the target of its citation chips.
fn record_anchor_id(record_id: &str) -> String {
    format!("record-{record_id}")
}

#[component]
fn GroundedAnswerView(
    answer: ChatAnswer,
    active_record: RwSignal<Option<String>>,
) -> impl IntoView {
    let (badge_cls, badge_text) = if answer.grounded {
        (
            "border-emerald-400/30 bg-emerald-500/10 text-emerald-300",
            count_label(answer.citations.len(), "cited record", "cited records"),
        )
    } else {
        (
            "border-amber-400/30 bg-amber-500/10 text-amber-300",
            "No matching records".to_string(),
        )
    };

    view! {
        <div class="rounded-xl border border-white/10 bg-slate-950/80 p-4 space-y-4">
            <div class="flex flex-wrap items-center justify-between gap-2 border-b border-white/5 pb-3">
                <p class="text-xs font-medium text-cyan-300">
                    {format!("Q: {}", answer.question)}
                </p>
                <span class=format!("rounded-full border px-2.5 py-0.5 text-[11px] font-medium {badge_cls}")>
                    {badge_text}
                </span>
            </div>

            <p class="whitespace-pre-line text-sm leading-relaxed text-slate-200">
                {answer.answer}
            </p>

            {(!answer.citations.is_empty()).then(|| view! {
                <div class="space-y-2.5">
                    <p class="text-[11px] font-semibold uppercase tracking-wider text-slate-400">
                        "Cited Source Records"
                    </p>
                    <ul class="grid gap-2.5 sm:grid-cols-2">
                        {answer
                            .citations
                            .into_iter()
                            .map(|cite| {
                                let kind_cls = record_kind_badge_class(cite.kind);
                                let href = format!("#{}", record_anchor_id(&cite.record_id));
                                let record_id = cite.record_id.clone();
                                let clicked_id = cite.record_id.clone();
                                let is_active = Memo::new(move |_| {
                                    active_record.with(|active| active.as_deref() == Some(record_id.as_str()))
                                });
                                view! {
                                    <li>
                                        <a
                                            href=href
                                            aria-current=move || is_active.get().then_some("true")
                                            on:click=move |_| active_record.set(Some(clicked_id.clone()))
                                            class=move || {
                                                if is_active.get() {
                                                    "block rounded-xl border border-cyan-300 bg-cyan-500/20 p-3 ring-2 ring-cyan-300/50 transition"
                                                } else {
                                                    "block rounded-xl border border-cyan-400/40 bg-cyan-500/[0.08] p-3 transition hover:border-cyan-300/70 hover:bg-cyan-500/15 focus:outline-none focus-visible:ring-2 focus-visible:ring-cyan-300/60"
                                                }
                                            }
                                        >
                                            <div class="flex flex-wrap items-center justify-between gap-1.5">
                                                <span class="rounded bg-cyan-500/20 px-2 py-0.5 font-mono text-[11px] font-semibold text-cyan-200">
                                                    {cite.record_id}
                                                </span>
                                                <span class=format!("rounded-full border px-2 py-0.5 text-[10px] font-semibold {kind_cls}")>
                                                    {cite.kind.label()}
                                                </span>
                                            </div>
                                            <p class="mt-1.5 text-xs font-semibold text-white">
                                                {cite.title}
                                                <span class="ml-1.5 font-mono font-normal text-slate-400">
                                                    {cite.recorded_at}
                                                </span>
                                            </p>
                                            <p class="mt-1 text-[11px] text-cyan-300">"Jump to source record ↓"</p>
                                        </a>
                                    </li>
                                }
                            })
                            .collect_view()}
                    </ul>
                </div>
            })}

            <p class="border-t border-white/5 pt-2.5 font-mono text-[11px] text-slate-400">
                {answer.basis}
            </p>
        </div>
    }
}

#[component]
fn ReplyDraftCard(
    draft: ReplyDraft,
    patient_id: String,
    decisions: RwSignal<DraftDecisions>,
) -> impl IntoView {
    let patient_id = StoredValue::new(patient_id);
    let state = Memo::new(move |_| decisions.with(|d| patient_id.with_value(|id| d.get(id))));
    let decide = move |next: DraftReviewState| {
        decisions.update(|d| patient_id.with_value(|id| d.set(id, next)));
        #[cfg(any(feature = "csr", feature = "hydrate"))]
        decisions.with_untracked(store_draft_decisions);
    };

    view! {
        <div class="rounded-2xl border border-amber-400/30 bg-amber-500/[0.06] p-6 space-y-3">
            <div class="flex flex-wrap items-center justify-between gap-2">
                <div>
                    <p class="text-xs font-semibold uppercase tracking-widest text-amber-300">
                        "Reviewable Patient Reply Draft"
                    </p>
                    <p class="mt-0.5 text-xs text-slate-400">
                        "In response to message "
                        <span class="font-mono text-amber-200">{draft.message_record_id}</span>
                        " — clinician approval required; never auto-sent."
                    </p>
                </div>
                <span
                    role="status"
                    aria-live="polite"
                    class=move || {
                        format!(
                            "rounded-full border px-3 py-1 text-xs font-semibold {}",
                            state.get().badge_class()
                        )
                    }
                >
                    {move || state.get().label()}
                </span>
            </div>

            <div class="rounded-xl border border-white/10 bg-slate-950/60 p-3.5 text-xs">
                <p class="font-semibold text-slate-400">"Patient question:"</p>
                <p class="mt-1 text-slate-200">{draft.patient_question}</p>
            </div>

            <div class="rounded-xl border border-white/10 bg-slate-950/80 p-4 text-sm leading-relaxed text-slate-100">
                {draft.proposed_reply}
            </div>

            <p class="font-mono text-[11px] text-slate-400">{draft.basis}</p>

            <div class="flex flex-wrap gap-2 pt-1">
                <button
                    type="button"
                    aria-pressed=move || (state.get() == DraftReviewState::ApprovedLocal).to_string()
                    on:click=move |_| decide(DraftReviewState::ApprovedLocal)
                    class="rounded-full bg-emerald-500 px-4 py-1.5 text-xs font-semibold text-slate-950 transition hover:bg-emerald-400"
                >
                    "Approve draft (demo)"
                </button>
                <button
                    type="button"
                    aria-pressed=move || (state.get() == DraftReviewState::HeldForVisit).to_string()
                    on:click=move |_| decide(DraftReviewState::HeldForVisit)
                    class="rounded-full border border-cyan-400/30 bg-cyan-500/10 px-4 py-1.5 text-xs font-semibold text-cyan-200 transition hover:bg-cyan-500/20"
                >
                    "Hold for visit"
                </button>
                <button
                    type="button"
                    on:click=move |_| decide(DraftReviewState::PendingReview)
                    class="rounded-full border border-white/15 px-3.5 py-1.5 text-xs font-medium text-slate-300 transition hover:bg-white/5"
                >
                    "Reset"
                </button>
            </div>
        </div>
    }
}

#[component]
fn SourceRecordCard(
    record: ChartRecord,
    cited_ids: Memo<HashSet<String>>,
    active_record: RwSignal<Option<String>>,
) -> impl IntoView {
    let kind_cls = record_kind_badge_class(record.kind);
    let anchor_id = record_anchor_id(&record.id);
    let cited_key = record.id.clone();
    let active_key = record.id.clone();
    let is_cited = Memo::new(move |_| cited_ids.with(|ids| ids.contains(&cited_key)));
    let is_active = Memo::new(move |_| {
        active_record.with(|active| active.as_deref() == Some(active_key.as_str()))
    });
    let container_cls = move || {
        match (is_active.get(), is_cited.get()) {
        (true, _) => "scroll-mt-24 rounded-xl border border-cyan-300 bg-cyan-500/[0.14] p-4 ring-2 ring-cyan-300/60 transition",
        (false, true) => "scroll-mt-24 rounded-xl border border-cyan-400/60 bg-cyan-500/[0.08] p-4 ring-1 ring-cyan-400/25 transition",
        (false, false) => "scroll-mt-24 rounded-xl border border-white/10 bg-slate-950/60 p-4 transition",
    }
    };
    let id_badge_cls = move || {
        if is_cited.get() {
            "rounded bg-cyan-500/25 px-2 py-0.5 font-mono text-xs font-semibold text-cyan-200"
        } else {
            "rounded bg-white/5 px-2 py-0.5 font-mono text-xs font-medium text-slate-300"
        }
    };

    view! {
        <article id=anchor_id class=container_cls>
            <div class="flex flex-wrap items-center justify-between gap-2">
                <div class="flex flex-wrap items-center gap-2">
                    <span class=id_badge_cls>{record.id}</span>
                    <span class=format!("rounded-full border px-2.5 py-0.5 text-[11px] font-semibold {kind_cls}")>
                        {record.kind.label()}
                    </span>
                    <Show when=move || is_cited.get()>
                        <span class="rounded-full border border-emerald-400/40 bg-emerald-500/15 px-2 py-0.5 text-[10px] font-semibold uppercase tracking-wider text-emerald-300">
                            "Cited in answer"
                        </span>
                    </Show>
                </div>
                <span class="font-mono text-xs text-slate-400">
                    {format!("{} · {}", record.recorded_at, record.source_label)}
                </span>
            </div>

            <h4 class="mt-2.5 text-sm font-semibold text-white">{record.title}</h4>
            <p class="mt-1.5 text-xs leading-relaxed text-slate-300">{record.body}</p>

            {(!record.key_values.is_empty()).then(|| view! {
                <dl class="mt-3 flex flex-wrap gap-2">
                    {record
                        .key_values
                        .into_iter()
                        .map(|(k, v)| {
                            view! {
                                <div class="rounded-lg border border-white/10 bg-slate-900/80 px-2.5 py-1 text-xs">
                                    <dt class="inline text-slate-400">{format!("{k}: ")}</dt>
                                    <dd class="inline font-semibold text-slate-100">{v}</dd>
                                </div>
                            }
                        })
                        .collect_view()}
                </dl>
            })}
        </article>
    }
}

#[cfg(all(test, feature = "ssr"))]
mod tests {
    use super::*;
    use leptos_meta::provide_meta_context;
    use leptos_router::components::Router;
    use leptos_router::location::RequestUrl;

    use crate::consent::provide_consent_context;

    fn render_in_app_context(
        path: &str,
        view_fn: impl FnOnce() -> AnyView + Send + 'static,
    ) -> String {
        let owner = Owner::new();
        let html = owner.with(|| {
            provide_meta_context();
            provide_consent_context();
            provide_context(RequestUrl::new(path));
            view! {
                <Router>
                    {view_fn()}
                </Router>
            }
            .to_html()
        });
        drop(owner);
        html.replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&#x27;", "'")
            .replace("&#39;", "'")
            .replace("&amp;", "&")
    }

    #[test]
    fn clinical_overview_page_renders_cleanly_without_legacy_brand() {
        let html = render_in_app_context("/clinical", || {
            view! { <ClinicalOverviewPage /> }.into_any()
        });
        assert!(html.contains("Dabney.moe Clinical Workspace"));
        assert!(html.contains("/dabney-elephant.svg"));
        assert!(html.contains("/clinical/physician"));
        assert!(html.contains("/mental-health"));
        assert!(html.contains("Synthetic Zero-PHI Guarantee"));
        assert!(html.contains("Nadia Kulkarni"));
        assert!(html.contains("David Okafor"));
        let legacy_brand = ["co", "da"].concat();
        assert!(
            !html.to_ascii_lowercase().contains(&legacy_brand),
            "clinical overview HTML must not contain legacy brand strings"
        );
    }

    #[test]
    fn physician_workspace_page_renders_all_synthetic_patients_and_citations() {
        let html = render_in_app_context("/clinical/physician", || {
            view! { <PhysicianWorkspacePage /> }.into_any()
        });
        assert!(html.contains("Physician Workspace Demo"));
        assert!(html.contains("/dabney-elephant.svg"));
        assert!(html.contains("Dabney.moe Synthetic Clinic"));
        for name in [
            "Nadia Kulkarni",
            "Carlos Mendez",
            "Priya Nair",
            "David Okafor",
        ] {
            assert!(html.contains(name), "missing synthetic patient: {name}");
        }
        assert!(html.contains("nk-cgm-01"));
        assert!(html.contains("nk-lab-02"));
        assert!(html.contains("Metformin ER"));
        assert!(html.contains("Cited in answer"));
        assert!(html.contains("Reviewable Patient Reply Draft"));
        let legacy_brand = ["co", "da"].concat();
        assert!(
            !html.to_ascii_lowercase().contains(&legacy_brand),
            "physician workspace HTML must not contain legacy brand strings"
        );
    }

    #[test]
    fn physician_workspace_is_accessible_and_linkable() {
        let html = render_in_app_context("/clinical/physician", || {
            view! { <PhysicianWorkspacePage /> }.into_any()
        });
        // Live regions for the answer and the draft decision.
        assert!(html.contains(r#"aria-live="polite" aria-label="Chart answer""#));
        assert!(html.contains(r#"role="status" aria-live="polite""#));
        // Selected patient is announced, once per selector.
        assert_eq!(html.matches(r#"aria-pressed="true""#).count(), 2);
        assert_eq!(html.matches(r#"aria-pressed="false""#).count(), 6 + 2);
        // Citation chips link to their source records, which carry the anchor.
        assert!(html.contains(r##"href="#record-nk-cgm-01""##));
        assert!(html.contains(r#"id="record-nk-cgm-01""#));
        assert!(html.contains("scroll-mt-24 rounded-xl"));
        // Empty-question guidance target and mobile chart focus target.
        assert!(html.contains(r#"aria-describedby="chart-question-error""#));
        assert!(html.contains(r#"id="patient-chart-header" role="region" tabindex="-1""#));
        // Sidebar selector is desktop-only and sticks below the navbar.
        assert!(html.contains("hidden lg:col-span-4 lg:block lg:sticky lg:top-24"));
        // Singular follow-up counts and AA-contrast metadata.
        assert!(html.contains("1 open follow-up<") || html.contains("1 open follow-up\""));
        assert!(!html.contains("1 open follow-ups"));
        assert!(!html.contains("text-slate-500"));
    }

    #[test]
    fn clinical_overview_copy_matches_the_in_browser_workspace() {
        let html = render_in_app_context("/clinical", || {
            view! { <ClinicalOverviewPage /> }.into_any()
        });
        assert!(!html.contains("reverse proxy"));
        assert!(html.contains("entirely in your browser via WebAssembly"));
        assert!(html.contains("1 open follow-up<"));
        assert!(!html.contains("1 open follow-ups"));
        assert!(!html.contains("text-slate-500"));
    }
}

#[cfg(test)]
mod logic_tests {
    use super::*;

    #[test]
    fn draft_decisions_are_kept_per_patient() {
        let mut decisions = DraftDecisions::default();
        decisions.set("pt-nadia-kulkarni", DraftReviewState::ApprovedLocal);
        decisions.set("pt-carlos-mendez", DraftReviewState::HeldForVisit);
        assert_eq!(
            decisions.get("pt-nadia-kulkarni"),
            DraftReviewState::ApprovedLocal
        );
        assert_eq!(
            decisions.get("pt-carlos-mendez"),
            DraftReviewState::HeldForVisit
        );
        assert_eq!(
            decisions.get("pt-priya-nair"),
            DraftReviewState::PendingReview
        );
        decisions.set("pt-carlos-mendez", DraftReviewState::PendingReview);
        assert_eq!(
            decisions.get("pt-carlos-mendez"),
            DraftReviewState::PendingReview
        );
        assert_eq!(
            decisions.get("pt-nadia-kulkarni"),
            DraftReviewState::ApprovedLocal
        );
    }

    #[test]
    fn draft_decisions_survive_a_storage_round_trip() {
        let mut decisions = DraftDecisions::default();
        decisions.set("pt-nadia-kulkarni", DraftReviewState::HeldForVisit);
        let json = decisions.to_json().expect("encode");
        assert_eq!(json, r#"{"pt-nadia-kulkarni":"held_for_visit"}"#);
        assert_eq!(DraftDecisions::from_json(&json).expect("decode"), decisions);
        assert!(DraftDecisions::from_json(r#"{"pt-nadia-kulkarni":"sent"}"#).is_err());
        assert!(DraftDecisions::from_json("not json").is_err());
    }

    #[test]
    fn counts_are_singularized() {
        assert_eq!(open_follow_ups_label(0), "0 open follow-ups");
        assert_eq!(open_follow_ups_label(1), "1 open follow-up");
        assert_eq!(open_follow_ups_label(2), "2 open follow-ups");
        assert_eq!(count_label(1, "record", "records"), "1 record");
    }
}
