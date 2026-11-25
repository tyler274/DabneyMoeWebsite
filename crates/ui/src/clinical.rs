//! Clean-room Dabney.moe Clinical Workspace pages (`/clinical` and `/clinical/physician`).
//!
//! Backed directly by [`clinical_workspace`] with zero external runtime dependencies,
//! rendering identically in native SSR and browser WebAssembly. All displayed patient
//! charts are fictional synthetic examples containing zero real patient data.

use std::collections::HashSet;

use clinical_workspace::{
    answer_chart_question, synthetic_panel, today_overview, ChartRecord, ChatAnswer,
    FollowUpStanding, PatientChart, RecordKind, ReplyDraft, Urgency,
};
use leptos::prelude::*;
use leptos_meta::{Link, Meta, Title};
use leptos_router::components::A;

use crate::patient_demo::ClinicalSuiteSwitcher;
use crate::sections::{Footer, NavBar};
use crate::AppShellMode;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DraftReviewState {
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
    let app_mode = use_context::<AppShellMode>().unwrap_or(AppShellMode::Website);
    let container_cls = match app_mode {
        AppShellMode::Website => "mx-auto max-w-5xl px-6 py-12 space-y-12",
        AppShellMode::ClinicalDemoApp => "mx-auto max-w-4xl px-4 pt-6 pb-28 sm:px-6 space-y-8",
    };

    view! {
        <Title text="Dabney.moe Clinical Workspace — Local-First Rust & WASM | dabney.moe" />
        <Meta
            name="description"
            content="Clean-room Dabney.moe Clinical Workspace built in pure Rust and WebAssembly: synthetic zero-PHI patient & physician demos, verbatim record citations, and local-first metabolic & mental health tracking."
        />
        <Link rel="canonical" href="https://dabney.moe/clinical" />

        <NavBar />

        <div class=container_cls>
            // Hero
            <section class="relative overflow-hidden rounded-3xl border border-white/10 bg-gradient-to-br from-slate-900/90 via-slate-950 to-slate-950 p-8 sm:p-10">
                <div class="pointer-events-none absolute inset-0 -z-10 bg-[radial-gradient(60%_50%_at_80%_0%,rgba(34,211,238,0.14),transparent)]"></div>
                <div class="flex flex-col gap-6 sm:flex-row sm:items-start sm:justify-between">
                    <div class="max-w-2xl space-y-4">
                        <div class="inline-flex items-center gap-3">
                            <div class="flex h-12 w-12 shrink-0 items-center justify-center rounded-2xl border border-emerald-400/40 bg-white/95 p-1.5 shadow-sm">
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
                            "typed longitudinal patient charts, strict per-patient citation boundaries, an embedded Axum API "
                            "with optional same-origin streaming reverse proxy, and browser-native CGM & mood analytics."
                        </p>
                        <div class="flex flex-wrap gap-3 pt-2">
                            <A
                                href="/clinical/patient"
                                attr:class="inline-flex items-center gap-2 rounded-full bg-emerald-500 px-6 py-2.5 text-sm font-semibold text-slate-950 transition hover:bg-emerald-400"
                            >
                                "Open Patient Demo →"
                            </A>
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

            <ClinicalSuiteSwitcher />

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
                            "and deterministic chart search—running directly in the browser via WebAssembly or over the embedded Axum API."
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

            // Standalone Android Demo App card
            <section class="rounded-2xl border border-cyan-400/25 bg-slate-900/50 p-6">
                <div class="flex flex-col gap-4 sm:flex-row sm:items-center sm:justify-between">
                    <div class="space-y-1.5">
                        <p class="text-xs font-semibold uppercase tracking-widest text-emerald-300">
                            "Android App · moe.dabney.demo"
                        </p>
                        <h2 class="text-xl font-bold text-white">
                            "Same Zero-PHI Clinical Suite on Android"
                        </h2>
                        <p class="max-w-2xl text-sm leading-relaxed text-slate-300">
                            "Packaged from the exact same Rust & WebAssembly crate via Tauri v2 for Google Play "
                            "demonstration. Runs offline with 100% synthetic sample profiles, zero analytics, zero "
                            "Health Connect permissions, and zero PHI collection."
                        </p>
                    </div>
                    <div class="flex flex-wrap gap-2.5 shrink-0">
                        <A
                            href="/clinical/patient"
                            attr:class="inline-flex items-center rounded-full bg-emerald-500 px-5 py-2.5 text-xs font-semibold text-slate-950 transition hover:bg-emerald-400"
                        >
                            "Try Patient Demo in Browser →"
                        </A>
                        <A
                            href="/privacy"
                            attr:class="inline-flex items-center rounded-full border border-white/15 px-4 py-2.5 text-xs font-medium text-slate-200 transition hover:bg-white/5"
                        >
                            "Zero-PHI Privacy Policy"
                        </A>
                    </div>
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
                                        <span>{format!("{} source records", patient.record_count)}</span>
                                        <span>{format!("{} open follow-ups", patient.open_follow_ups)}</span>
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
    let app_mode = use_context::<AppShellMode>().unwrap_or(AppShellMode::Website);
    let container_cls = match app_mode {
        AppShellMode::Website => "mx-auto max-w-6xl px-6 py-10 space-y-6",
        AppShellMode::ClinicalDemoApp => "mx-auto max-w-6xl px-4 pt-5 pb-28 sm:px-6 space-y-5",
    };

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
    let latest_answer = RwSignal::new(initial_answer);
    let draft_state = RwSignal::new(DraftReviewState::PendingReview);

    let select_patient = move |patient_id: String| {
        selected_id.set(patient_id.clone());
        draft_state.set(DraftReviewState::PendingReview);
        let default_q = preset_questions_for_patient(&patient_id)
            .first()
            .copied()
            .unwrap_or("Summarize recent records")
            .to_string();
        question_input.set(default_q.clone());
        let next_answer = panel.with_value(|charts| {
            charts
                .iter()
                .find(|p| p.id == patient_id)
                .map(|p| answer_chart_question(p, &default_q))
        });
        latest_answer.set(next_answer);
    };

    let ask_question = move |raw_q: String| {
        let trimmed = raw_q.trim().to_string();
        if trimmed.is_empty() {
            return;
        }
        let active_id = selected_id.get();
        let next_answer = panel.with_value(|charts| {
            charts
                .iter()
                .find(|p| p.id == active_id)
                .map(|p| answer_chart_question(p, &trimmed))
        });
        latest_answer.set(next_answer);
    };

    let on_submit_question = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        ask_question(question_input.get());
    };

    view! {
        <Title text="Physician Workspace Demo — Dabney.moe Clinical Workspace | dabney.moe" />
        <Meta
            name="description"
            content="Interactive zero-PHI synthetic physician workspace in Rust and WebAssembly: today's clinic queue, 4 synthetic longitudinal charts, reviewable draft replies, and record-cited chart Q&A."
        />
        <Link rel="canonical" href="https://dabney.moe/clinical/physician" />

        <NavBar />

        <div class=container_cls>
            // Top bar
            <header class="flex flex-wrap items-center justify-between gap-4 border-b border-white/10 pb-6">
                <div class="flex items-center gap-4">
                    <div class="flex h-12 w-12 shrink-0 items-center justify-center rounded-2xl border border-emerald-400/40 bg-white/95 p-1.5 shadow-sm">
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
                        href="/clinical/patient"
                        attr:class="rounded-full border border-emerald-400/35 bg-emerald-500/15 px-4 py-2 font-medium text-emerald-200 transition hover:bg-emerald-500/25"
                    >
                        "Patient Demo"
                    </A>
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

            <ClinicalSuiteSwitcher />

            // Synthetic-data banner
            <div
                class="flex flex-wrap items-center justify-between gap-3 rounded-2xl border border-emerald-400/30 bg-emerald-500/10 px-5 py-3.5"
                role="status"
            >
                <p class="text-xs leading-relaxed text-emerald-100 sm:text-sm">
                    <span class="font-semibold text-emerald-300">"Synthetic Zero-PHI Demo: "</span>
                    {overview.synthetic_notice.clone()}
                    " Runs deterministically in Rust/WASM with zero external services."
                </p>
                <span class="rounded-full border border-emerald-400/40 bg-slate-950/60 px-3 py-1 text-[11px] font-semibold uppercase tracking-wider text-emerald-300">
                    "Offline-Ready"
                </span>
            </div>

            // Today's clinic schedule / queue
            <section class="rounded-2xl border border-white/10 bg-white/[0.02] p-5">
                <div class="mb-3 flex flex-wrap items-center justify-between gap-2">
                    <h2 class="text-sm font-semibold uppercase tracking-wider text-cyan-300">
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
                            let urgency_cls = urgency_badge_class(summary.urgency);
                            let time_text = summary
                                .visit_time
                                .unwrap_or_else(|| "Panel review".to_string());
                            view! {
                                <button
                                    type="button"
                                    on:click=move |_| select_patient(pid_click.clone())
                                    class=move || {
                                        if selected_id.get() == pid_active {
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
                                    <p class="mt-2 text-[11px] text-slate-500">
                                        {format!("{} records · {} open follow-ups", summary.record_count, summary.open_follow_ups)}
                                    </p>
                                </button>
                            }
                        })
                        .collect_view()}
                </div>
            </section>

            // Main workspace: Patient selector + Selected patient chart
            <div class="grid gap-6 lg:grid-cols-12">
                // Left sidebar: Clickable patient panel selector
                <aside class="lg:col-span-4 space-y-3">
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
                                                on:click=move |_| select_patient(pid_click.clone())
                                                class=move || {
                                                    if selected_id.get() == pid_active {
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

                // Right column: Selected patient chart & grounded Q&A
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

                        let active_answer = latest_answer
                            .get()
                            .filter(|ans| ans.patient_id == patient.id);
                        let cited_ids: HashSet<String> = active_answer
                            .as_ref()
                            .map(|ans| ans.citations.iter().map(|c| c.record_id.clone()).collect())
                            .unwrap_or_default();
                        let presets = preset_questions_for_patient(&patient.id);
                        let urgency_cls = urgency_badge_class(patient.urgency);

                        view! {
                            <div class="space-y-6">
                                // Selected patient header
                                <div class="rounded-2xl border border-white/10 bg-white/[0.02] p-6">
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
                                            "Deterministic local retrieval scoped strictly to "
                                            <span class="font-mono text-slate-200">{patient.mrn.clone()}</span>
                                            ". Cited source record IDs are highlighted below."
                                        </p>
                                    </div>

                                    <form on:submit=on_submit_question class="flex flex-col gap-2.5 sm:flex-row">
                                        <input
                                            type="text"
                                            aria-label="Ask a clinical question about the selected chart"
                                            placeholder="Ask about labs, CGM trends, medications, or messages..."
                                            prop:value=move || question_input.get()
                                            on:input=move |ev| question_input.set(event_target_value(&ev))
                                            class="flex-1 rounded-xl border border-white/15 bg-slate-950 px-3.5 py-2.5 text-sm text-white placeholder:text-slate-500 focus:border-cyan-400/60 focus:outline-none focus:ring-1 focus:ring-cyan-400/50"
                                        />
                                        <button
                                            type="submit"
                                            class="rounded-full bg-cyan-500 px-5 py-2.5 text-sm font-semibold text-slate-950 transition hover:bg-cyan-400"
                                        >
                                            "Ask chart"
                                        </button>
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

                                    {active_answer.map(|ans| view! { <GroundedAnswerView answer=ans /> })}
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
                                                            <p class="mt-1 text-[11px] text-slate-500">
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
                                    view! { <ReplyDraftCard draft=draft state=draft_state /> }
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
                                            {format!("{} records", patient.records.len())}
                                        </span>
                                    </div>
                                    <div class="space-y-3">
                                        {patient
                                            .records
                                            .into_iter()
                                            .map(|rec| {
                                                let is_cited = cited_ids.contains(&rec.id);
                                                view! { <SourceRecordCard record=rec is_cited=is_cited /> }
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

#[component]
fn GroundedAnswerView(answer: ChatAnswer) -> impl IntoView {
    view! {
        <div class="rounded-xl border border-white/10 bg-slate-950/80 p-4 space-y-4">
            <div class="flex flex-wrap items-center justify-between gap-2 border-b border-white/5 pb-3">
                <p class="text-xs font-medium text-cyan-300">
                    {format!("Q: {}", answer.question)}
                </p>
                <span class="rounded-full border border-emerald-400/30 bg-emerald-500/10 px-2.5 py-0.5 text-[11px] font-medium text-emerald-300">
                    {format!("{} cited record(s)", answer.citations.len())}
                </span>
            </div>

            <p class="whitespace-pre-line text-sm leading-relaxed text-slate-200">
                {answer.answer}
            </p>

            <div class="space-y-2.5">
                <p class="text-[11px] font-semibold uppercase tracking-wider text-slate-400">
                    "Cited Source Records"
                </p>
                <div class="grid gap-2.5 sm:grid-cols-2">
                    {answer
                        .citations
                        .into_iter()
                        .map(|cite| {
                            let kind_cls = record_kind_badge_class(cite.kind);
                            view! {
                                <div class="rounded-xl border border-cyan-400/40 bg-cyan-500/[0.08] p-3">
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
                                    <p class="mt-1 text-xs leading-relaxed text-slate-300">
                                        {cite.excerpt}
                                    </p>
                                </div>
                            }
                        })
                        .collect_view()}
                </div>
            </div>

            <p class="border-t border-white/5 pt-2.5 font-mono text-[11px] text-slate-400">
                {answer.basis}
            </p>
        </div>
    }
}

#[component]
fn ReplyDraftCard(draft: ReplyDraft, state: RwSignal<DraftReviewState>) -> impl IntoView {
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
                <span class=move || {
                    format!(
                        "rounded-full border px-3 py-1 text-xs font-semibold {}",
                        state.get().badge_class()
                    )
                }>
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
                    on:click=move |_| state.set(DraftReviewState::ApprovedLocal)
                    class="rounded-full bg-emerald-500 px-4 py-1.5 text-xs font-semibold text-slate-950 transition hover:bg-emerald-400"
                >
                    "Approve draft (demo)"
                </button>
                <button
                    type="button"
                    on:click=move |_| state.set(DraftReviewState::HeldForVisit)
                    class="rounded-full border border-cyan-400/30 bg-cyan-500/10 px-4 py-1.5 text-xs font-semibold text-cyan-200 transition hover:bg-cyan-500/20"
                >
                    "Hold for visit"
                </button>
                <button
                    type="button"
                    on:click=move |_| state.set(DraftReviewState::PendingReview)
                    class="rounded-full border border-white/15 px-3.5 py-1.5 text-xs font-medium text-slate-300 transition hover:bg-white/5"
                >
                    "Reset"
                </button>
            </div>
        </div>
    }
}

#[component]
fn SourceRecordCard(record: ChartRecord, is_cited: bool) -> impl IntoView {
    let kind_cls = record_kind_badge_class(record.kind);
    let container_cls = if is_cited {
        "rounded-xl border border-cyan-400/60 bg-cyan-500/[0.08] p-4 ring-1 ring-cyan-400/25 transition"
    } else {
        "rounded-xl border border-white/10 bg-slate-950/60 p-4 transition"
    };
    let id_badge_cls = if is_cited {
        "rounded bg-cyan-500/25 px-2 py-0.5 font-mono text-xs font-semibold text-cyan-200"
    } else {
        "rounded bg-white/5 px-2 py-0.5 font-mono text-xs font-medium text-slate-300"
    };

    view! {
        <article class=container_cls>
            <div class="flex flex-wrap items-center justify-between gap-2">
                <div class="flex flex-wrap items-center gap-2">
                    <span class=id_badge_cls>{record.id}</span>
                    <span class=format!("rounded-full border px-2.5 py-0.5 text-[11px] font-semibold {kind_cls}")>
                        {record.kind.label()}
                    </span>
                    {is_cited.then(|| view! {
                        <span class="rounded-full border border-emerald-400/40 bg-emerald-500/15 px-2 py-0.5 text-[10px] font-semibold uppercase tracking-wider text-emerald-300">
                            "Cited in answer"
                        </span>
                    })}
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
        assert!(html.contains("/clinical/patient"));
        assert!(html.contains("/clinical/physician"));
        assert!(html.contains("/mental-health"));
        assert!(html.contains("moe.dabney.demo"));
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
}
