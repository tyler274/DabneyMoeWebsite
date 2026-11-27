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

use crate::chart_citations::{
    record_anchor_id, scroll_to_record_anchor, CitationAccent, CitationBadgeLink, CitationCardLink,
    CitationLinkedText,
};
#[cfg(any(feature = "csr", feature = "hydrate"))]
use crate::dashboard_layout::{load_layout_prefs, save_layout_prefs, STORAGE_KEY_PHYSICIAN_LAYOUT};
use crate::dashboard_layout::{
    LayoutDensity, PhysicianFocusOrder, PhysicianLayoutCustomizer, PhysicianLayoutPrefs,
    SidebarPlacement,
};
use crate::health_bridge::LocalLlmStatusUi;
#[cfg(any(feature = "csr", feature = "hydrate"))]
use crate::health_bridge::{
    build_chart_chat_prompt, generate_local_llm, prepare_local_llm, probe_local_llm,
};
use crate::local_ai_chat::LocalLlmStatusBanner;
use crate::patient_demo::ClinicalSuiteSwitcher;
use crate::sections::{Footer, NavBar};
use crate::AppShellMode;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PanelFilter {
    All,
    High,
    Moderate,
    Routine,
    HasDraft,
}

impl PanelFilter {
    const fn matches(self, patient: &PatientChart) -> bool {
        match self {
            Self::All => true,
            Self::High => matches!(patient.urgency, Urgency::High),
            Self::Moderate => matches!(patient.urgency, Urgency::Moderate),
            Self::Routine => matches!(patient.urgency, Urgency::Routine),
            Self::HasDraft => patient.reply_draft.is_some(),
        }
    }
}

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
        <Title text="Curie Clinic & Curie Mobile — Local-First Clinical Showcase | dabney.moe" />
        <Meta
            name="description"
            content="Clean-room Curie Clinic and Curie Mobile clinical showcase: synthetic zero-PHI patient & physician demos, verbatim record citations, and private metabolic & mental health tracking."
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
                            <div class="flex h-12 w-12 shrink-0 items-center justify-center rounded-2xl border border-emerald-400/40 bg-[#01081b] p-1 shadow-sm">
                                <img
                                    src="/dabney-elephant.svg"
                                    alt="Curie Clinical Workspace emblem"
                                    class="h-full w-full object-contain"
                                />
                            </div>
                            <p class="text-xs font-semibold uppercase tracking-widest text-cyan-400">
                                "Curie Clinic & Curie Mobile · Clean-Room Clinical Showcase"
                            </p>
                        </div>
                        <h1 class="text-3xl font-bold tracking-tight text-white sm:text-4xl">
                            "Curie Clinical Workspace"
                        </h1>
                        <p class="text-base leading-relaxed text-slate-300">
                            "A local-first clinical and patient companion showcase built from scratch: "
                            "structured longitudinal patient charts, strict per-patient record citations, "
                            "patient-controlled visit packets, and private on-device glucose & mood analytics."
                        </p>
                        <div class="flex flex-wrap gap-3 pt-2">
                            <A
                                href="/clinical/patient"
                                attr:class="inline-flex items-center gap-2 rounded-full bg-emerald-500 px-6 py-2.5 text-sm font-semibold text-slate-950 transition hover:bg-emerald-400"
                            >
                                "Open Curie Mobile (Patient) →"
                            </A>
                            <A
                                href="/clinical/physician"
                                attr:class="inline-flex items-center gap-2 rounded-full bg-cyan-500 px-6 py-2.5 text-sm font-semibold text-slate-950 transition hover:bg-cyan-400"
                            >
                                "Open Curie Clinic (Physician) →"
                            </A>
                            <A
                                href="/mental-health"
                                attr:class="inline-flex items-center gap-2 rounded-full border border-emerald-400/30 bg-emerald-500/10 px-5 py-2.5 text-sm font-semibold text-emerald-200 transition hover:bg-emerald-500/20"
                            >
                                "Mental Health & CGM Tracker"
                            </A>
                            {(app_mode == AppShellMode::Website).then(|| view! {
                                <A
                                    href="/android"
                                    attr:class="inline-flex items-center gap-2 rounded-full border border-cyan-400/35 bg-slate-950/70 px-5 py-2.5 text-sm font-semibold text-cyan-200 transition hover:bg-cyan-500/15"
                                >
                                    "Install Android App (F-Droid / APK)"
                                </A>
                            })}
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
                        "Design Pillars"
                    </p>
                    <h2 class="mt-1 text-2xl font-bold tracking-tight text-white">
                        "Local-First Clinical & Patient Design"
                    </h2>
                </div>
                <div class="grid gap-5 md:grid-cols-3">
                    <article class="rounded-2xl border border-white/10 bg-white/[0.02] p-6">
                        <p class="text-xs font-semibold uppercase tracking-wider text-cyan-300">
                            "01 · Unified Clinical Model"
                        </p>
                        <h3 class="mt-2 text-lg font-semibold text-white">
                            "Identical Web & On-Device Experience"
                        </h3>
                        <p class="mt-2 text-sm leading-relaxed text-slate-400">
                            "A single shared clinical core models longitudinal charts, follow-ups, reviewable draft replies, "
                            "and grounded chart search—running directly on your device without external cloud dependencies."
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
                            "Private CGM & Mood Analytics"
                        </h3>
                        <p class="mt-2 text-sm leading-relaxed text-slate-400">
                            "Pair commercial glucose exports with mood, PHQ-9, GAD-7, and ASRM tracking on "
                            <A href="/mental-health" attr:class="text-cyan-300 underline decoration-cyan-400/40 underline-offset-2 hover:text-cyan-200">
                                "/mental-health"
                            </A>
                            " where personal files stay strictly on your own device."
                        </p>
                    </article>
                </div>
            </section>

            // Standalone Android Demo App card
            <section class="rounded-2xl border border-cyan-400/25 bg-slate-900/50 p-6">
                <div class="flex flex-col gap-4 sm:flex-row sm:items-center sm:justify-between">
                    <div class="space-y-1.5">
                        <p class="text-xs font-semibold uppercase tracking-widest text-emerald-300">
                            "Curie Mobile · Android App"
                        </p>
                        <h2 class="text-xl font-bold text-white">
                            "Same Zero-PHI Clinical Suite on Android"
                        </h2>
                        <p class="max-w-2xl text-sm leading-relaxed text-slate-300">
                            "Packaged from the exact same local-first clinical core for offline Android demonstration. "
                            "Runs with 100% synthetic sample profiles, zero analytics, zero health permissions, "
                            "and zero PHI collection."
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
    let layout_prefs = RwSignal::new(PhysicianLayoutPrefs::default());
    let layout_open = RwSignal::new(false);
    let layout_notice = RwSignal::new(Option::<String>::None);

    #[cfg(any(feature = "csr", feature = "hydrate"))]
    Effect::new(move |_| {
        if let Some(saved) = load_layout_prefs::<PhysicianLayoutPrefs>(STORAGE_KEY_PHYSICIAN_LAYOUT)
        {
            layout_prefs.set(saved);
        }
    });

    let container_cls = move || {
        let prefs = layout_prefs.get();
        let width = prefs.density.container_width_class();
        let gap = prefs.density.stack_gap_class();
        match app_mode {
            AppShellMode::Website => format!("mx-auto {width} px-6 py-10 {gap}"),
            AppShellMode::ClinicalDemoApp => {
                format!("mx-auto {width} px-4 pt-5 pb-28 sm:px-6 {gap}")
            }
        }
    };

    let (initial_id, initial_question, initial_answer, initial_draft_states, initial_draft_texts) =
        panel.with_value(|charts| {
            let mut states = HashMap::new();
            let mut texts = HashMap::new();
            for chart in charts {
                if let Some(draft) = chart.reply_draft.as_ref() {
                    states.insert(chart.id.clone(), DraftReviewState::PendingReview);
                    texts.insert(chart.id.clone(), draft.proposed_reply.clone());
                }
            }
            if let Some(first) = charts.first() {
                let q = preset_questions_for_patient(&first.id)
                    .first()
                    .copied()
                    .unwrap_or("Summarize recent records")
                    .to_string();
                let ans = answer_chart_question(first, &q);
                (first.id.clone(), q, Some(ans), states, texts)
            } else {
                (String::new(), String::new(), None, states, texts)
            }
        });

    let selected_id = RwSignal::new(initial_id);
    let focused_record_id = RwSignal::new(Option::<String>::None);
    let panel_filter = RwSignal::new(PanelFilter::All);
    let panel_search = RwSignal::new(String::new());
    let question_input = RwSignal::new(initial_question);
    let latest_answer = RwSignal::new(initial_answer);
    let draft_states = RwSignal::new(initial_draft_states);
    let draft_texts = RwSignal::new(initial_draft_texts);

    // Optional on-device Gemini Nano (AICore) synthesis inside `moe.dabney.personal`.
    let llm_status = RwSignal::new(Option::<LocalLlmStatusUi>::None);
    let use_local_llm = RwSignal::new(true);
    let llm_preparing = RwSignal::new(false);
    let llm_prepare_notice = RwSignal::new(Option::<String>::None);
    let llm_thinking = RwSignal::new(false);
    let llm_error = RwSignal::new(Option::<String>::None);
    let llm_req_seq = RwSignal::new(0usize);

    Effect::new(move |_| {
        #[cfg(any(feature = "csr", feature = "hydrate"))]
        {
            probe_local_llm(move |status| {
                llm_status.set(Some(status));
            });
        }
    });

    let prepare_llm_now = Callback::new(move |()| {
        #[cfg(any(feature = "csr", feature = "hydrate"))]
        {
            llm_preparing.set(true);
            llm_prepare_notice.set(Some(
                "Requesting Gemini Nano download via Android AICore…".to_string(),
            ));
            prepare_local_llm(move |res| {
                llm_preparing.set(false);
                match res {
                    Ok(outcome) => {
                        llm_prepare_notice.set(Some(outcome.detail));
                        probe_local_llm(move |status| llm_status.set(Some(status)));
                    }
                    Err(err) => llm_prepare_notice.set(Some(err)),
                }
            });
        }
    });

    let select_patient = move |patient_id: String| {
        llm_req_seq.update(|n| *n += 1);
        llm_thinking.set(false);
        llm_error.set(None);
        focused_record_id.set(None);
        selected_id.set(patient_id.clone());
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

    let jump_to_record = Callback::new(move |record_id: String| {
        let mut prefs = layout_prefs.get_untracked();
        if !prefs.show_source_records {
            prefs.preset = None;
            prefs.show_source_records = true;
            #[cfg(any(feature = "csr", feature = "hydrate"))]
            save_layout_prefs(STORAGE_KEY_PHYSICIAN_LAYOUT, &prefs);
            layout_prefs.set(prefs);
        }
        focused_record_id.set(Some(record_id.clone()));
        scroll_to_record_anchor(&record_anchor_id("chart-record", &record_id));
    });

    let clear_focused_record = Callback::new(move |()| {
        focused_record_id.set(None);
    });

    let ask_question = move |raw_q: String| {
        let trimmed = raw_q.trim().to_string();
        if trimmed.is_empty() {
            return;
        }
        let active_id = selected_id.get();
        let next_pair = panel.with_value(|charts| {
            charts
                .iter()
                .find(|p| p.id == active_id)
                .map(|p| (p.display_name.clone(), answer_chart_question(p, &trimmed)))
        });
        let Some((patient_name, grounded)) = next_pair else {
            return;
        };
        latest_answer.set(Some(grounded.clone()));
        llm_error.set(None);

        let req_id = llm_req_seq.get_untracked() + 1;
        llm_req_seq.set(req_id);

        let should_run_llm = use_local_llm.get_untracked()
            && !grounded.citations.is_empty()
            && llm_status
                .get_untracked()
                .is_some_and(|s| s.enabled && s.availability == "available");

        #[cfg(any(feature = "csr", feature = "hydrate"))]
        if should_run_llm {
            let prompt = build_chart_chat_prompt("physician", &patient_name, &trimmed, &grounded);
            llm_thinking.set(true);
            generate_local_llm(&prompt, Some(340), move |res| {
                if llm_req_seq.get_untracked() != req_id {
                    return;
                }
                llm_thinking.set(false);
                match res {
                    Ok(gen) => {
                        let mut enhanced = grounded.clone();
                        enhanced.answer = gen.text;
                        enhanced.basis = format!(
                            "{} (on-device · {} ms) grounded strictly in {} verbatim single-patient citation(s).",
                            gen.model_name,
                            gen.latency_ms,
                            enhanced.citations.len()
                        );
                        latest_answer.set(Some(enhanced));
                    }
                    Err(_) => {
                        llm_error.set(Some(
                            "On-device AI synthesis was unavailable for this turn; showing record-cited summary."
                                .to_string(),
                        ));
                    }
                }
            });
        }
        #[cfg(not(any(feature = "csr", feature = "hydrate")))]
        let _ = (patient_name, should_run_llm);
    };

    let on_submit_question = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        ask_question(question_input.get());
    };

    let (total_patients, high_count, moderate_count, routine_count, draft_count) = panel
        .with_value(|charts| {
            let total = charts.len();
            let high = charts
                .iter()
                .filter(|p| matches!(p.urgency, Urgency::High))
                .count();
            let moderate = charts
                .iter()
                .filter(|p| matches!(p.urgency, Urgency::Moderate))
                .count();
            let routine = charts
                .iter()
                .filter(|p| matches!(p.urgency, Urgency::Routine))
                .count();
            let drafts = charts.iter().filter(|p| p.reply_draft.is_some()).count();
            (total, high, moderate, routine, drafts)
        });

    view! {
        <Title text="Curie Clinic — Physician Workspace Demo | dabney.moe" />
        <Meta
            name="description"
            content="Interactive zero-PHI Curie Clinic synthetic physician workspace: compact clinic queue summary, 4 synthetic longitudinal charts, editable reviewable draft replies, and record-cited chart Q&A."
        />
        <Link rel="canonical" href="https://dabney.moe/clinical/physician" />

        <NavBar />

        <div class=container_cls>
            // Top bar
            <header class="flex flex-wrap items-center justify-between gap-4 border-b border-white/10 pb-6">
                <div class="flex items-center gap-4">
                    <div class="flex h-12 w-12 shrink-0 items-center justify-center rounded-2xl border border-emerald-400/40 bg-[#01081b] p-1 shadow-sm">
                        <img
                            src="/dabney-elephant.svg"
                            alt="Curie Clinic emblem"
                            class="h-full w-full object-contain"
                        />
                    </div>
                    <div>
                        <p class="text-xs font-semibold uppercase tracking-widest text-cyan-400">
                            {format!("Curie Clinic · {} · {}", overview.practice_name, overview.clinic_date)}
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
                        "Curie Mobile (Patient)"
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
                    " Runs locally on your device with zero external services."
                </p>
                <span class="rounded-full border border-emerald-400/40 bg-slate-950/60 px-3 py-1 text-[11px] font-semibold uppercase tracking-wider text-emerald-300">
                    "Offline-Ready"
                </span>
            </div>

            // Compact clinic summary & urgency filter strip (deduplicated from Patient Panel)
            <section
                aria-label="Today's clinic queue summary and filters"
                class="flex flex-wrap items-center justify-between gap-3 rounded-2xl border border-white/10 bg-white/[0.02] px-5 py-3.5"
            >
                <div class="flex flex-wrap items-center gap-2 text-xs">
                    <h2 class="font-semibold uppercase tracking-wider text-cyan-300">
                        "Today's Clinic Queue"
                    </h2>
                    <span class="text-slate-400">
                        {format!("· {total_patients} charts · {draft_count} draft replies")}
                    </span>
                </div>
                <div class="flex flex-wrap items-center gap-1.5 text-xs" role="group" aria-label="Filter patient panel">
                    {[
                        (PanelFilter::All, format!("All ({total_patients})")),
                        (PanelFilter::High, format!("High ({high_count})")),
                        (PanelFilter::Moderate, format!("Moderate ({moderate_count})")),
                        (PanelFilter::Routine, format!("Routine ({routine_count})")),
                        (PanelFilter::HasDraft, format!("Drafts ({draft_count})")),
                    ]
                    .into_iter()
                    .map(|(f, label)| {
                        view! {
                            <button
                                type="button"
                                aria-pressed=move || panel_filter.get() == f
                                on:click=move |_| panel_filter.set(f)
                                class=move || {
                                    if panel_filter.get() == f {
                                        "min-h-[34px] rounded-full bg-cyan-500 px-3 py-1 font-semibold text-slate-950 transition"
                                    } else {
                                        "min-h-[34px] rounded-full border border-white/15 bg-slate-950/60 px-3 py-1 font-medium text-slate-300 transition hover:border-white/30 hover:text-white"
                                    }
                                }
                            >
                                {label}
                            </button>
                        }
                    })
                    .collect_view()}
                    {move || {
                        (layout_prefs.get().sidebar == SidebarPlacement::Hidden).then(|| {
                            view! {
                                <button
                                    type="button"
                                    on:click=move |_| {
                                        let mut next = layout_prefs.get();
                                        next.preset = None;
                                        next.sidebar = SidebarPlacement::Left;
                                        #[cfg(any(feature = "csr", feature = "hydrate"))]
                                        save_layout_prefs(STORAGE_KEY_PHYSICIAN_LAYOUT, &next);
                                        layout_prefs.set(next);
                                    }
                                    class="min-h-[34px] rounded-full border border-emerald-400/40 bg-emerald-500/15 px-3 py-1 font-semibold text-emerald-200 transition hover:bg-emerald-500/25"
                                >
                                    "Show Patient Panel"
                                </button>
                            }
                        })
                    }}
                    <button
                        type="button"
                        aria-expanded=move || layout_open.get().to_string()
                        on:click=move |_| layout_open.update(|v| *v = !*v)
                        class=move || {
                            if layout_open.get() {
                                "min-h-[34px] rounded-full border border-cyan-400/60 bg-cyan-500/20 px-3 py-1 font-semibold text-cyan-100 transition"
                            } else {
                                "min-h-[34px] rounded-full border border-white/15 bg-slate-950/60 px-3 py-1 font-medium text-slate-200 transition hover:border-cyan-400/40 hover:text-white"
                            }
                        }
                    >
                        {move || format!("Layout: {} ▾", layout_prefs.get().active_label())}
                    </button>
                </div>
            </section>

            <PhysicianLayoutCustomizer
                prefs=layout_prefs
                open=layout_open
                saved_notice=layout_notice
            />

            // Compact horizontal patient switcher when sidebar is tucked away in Full-Width / Calm Reading layout
            {move || {
                (layout_prefs.get().sidebar == SidebarPlacement::Hidden).then(|| {
                    panel.with_value(|charts| {
                        view! {
                            <nav
                                aria-label="Quick patient switcher"
                                class="flex flex-wrap items-center gap-2 rounded-2xl border border-white/10 bg-slate-900/60 px-4 py-2.5 text-xs"
                            >
                                <span class="text-[11px] font-semibold uppercase tracking-wider text-slate-400">
                                    "Patient:"
                                </span>
                                {charts
                                    .iter()
                                    .map(|p| {
                                        let pid_click = p.id.clone();
                                        let pid_active = p.id.clone();
                                        let pid_cls = p.id.clone();
                                        let label = p.display_name.clone();
                                        view! {
                                            <button
                                                type="button"
                                                aria-pressed=move || selected_id.get() == pid_active
                                                on:click=move |_| select_patient(pid_click.clone())
                                                class=move || {
                                                    if selected_id.get() == pid_cls {
                                                        "min-h-[34px] rounded-full bg-cyan-400 px-3 py-1 font-semibold text-slate-950 transition"
                                                    } else {
                                                        "min-h-[34px] rounded-full border border-white/15 bg-slate-950/60 px-3 py-1 text-slate-200 transition hover:border-white/30 hover:text-white"
                                                    }
                                                }
                                            >
                                                {label}
                                            </button>
                                        }
                                    })
                                    .collect_view()}
                            </nav>
                        }
                    })
                })
            }}

            // Main workspace: Single authoritative Patient Panel + Selected patient chart
            <div class="grid gap-6 lg:grid-cols-12">
                // Patient panel sidebar (left, right, or tucked away per user layout preference)
                <aside class=move || match layout_prefs.get().sidebar {
                    SidebarPlacement::Left => {
                        "lg:col-span-4 space-y-3 lg:sticky lg:top-[132px] lg:self-start"
                    }
                    SidebarPlacement::Right => {
                        "lg:col-span-4 lg:order-last space-y-3 lg:sticky lg:top-[132px] lg:self-start"
                    }
                    SidebarPlacement::Hidden => "hidden",
                }>
                    <div class="rounded-2xl border border-white/10 bg-white/[0.02] p-4">
                        <div class="mb-3 flex items-center justify-between gap-2">
                            <h2 class="text-xs font-semibold uppercase tracking-widest text-slate-400">
                                "Patient Panel"
                            </h2>
                            <span class="text-[11px] text-slate-500">{format!("{total_patients} total")}</span>
                        </div>
                        <div class="mb-3">
                            <label for="physician-panel-search" class="sr-only">
                                "Filter patients by name, MRN, or focus"
                            </label>
                            <input
                                id="physician-panel-search"
                                type="search"
                                placeholder="Find patient by name, MRN, or focus…"
                                prop:value=move || panel_search.get()
                                on:input=move |ev| panel_search.set(event_target_value(&ev))
                                class="min-h-[38px] w-full rounded-xl border border-white/15 bg-slate-950 px-3 py-1.5 text-xs text-white placeholder-slate-500 focus:border-cyan-400 focus:outline-none"
                            />
                        </div>
                        <div class="space-y-2.5">
                            {move || {
                                let active_filter = panel_filter.get();
                                let search_q = panel_search.get().trim().to_lowercase();
                                let states_map = draft_states.get();
                                panel.with_value(|charts| {
                                    let visible: Vec<&PatientChart> = charts
                                        .iter()
                                        .filter(|p| active_filter.matches(p))
                                        .filter(|p| {
                                            if search_q.is_empty() {
                                                return true;
                                            }
                                            p.display_name.to_lowercase().contains(&search_q)
                                                || p.mrn.to_lowercase().contains(&search_q)
                                                || p.primary_focus.to_lowercase().contains(&search_q)
                                        })
                                        .collect();
                                    if visible.is_empty() {
                                        return view! {
                                            <p class="rounded-xl border border-white/10 bg-slate-950/50 p-3 text-xs text-slate-400">
                                                "No charts match this filter."
                                            </p>
                                        }
                                        .into_any();
                                    }
                                    visible
                                        .into_iter()
                                        .map(|patient| {
                                            let pid_click = patient.id.clone();
                                            let pid_active = patient.id.clone();
                                            let pid_aria = patient.id.clone();
                                            let urgency_cls = urgency_badge_class(patient.urgency);
                                            let visit_note = patient
                                                .visit
                                                .as_ref()
                                                .map(|v| format!("{} · {}", v.starts_at, v.modality))
                                                .unwrap_or_else(|| "Asynchronous review".to_string());
                                            let draft_badge = patient.reply_draft.as_ref().map(|_| {
                                                let st = states_map
                                                    .get(&patient.id)
                                                    .copied()
                                                    .unwrap_or(DraftReviewState::PendingReview);
                                                match st {
                                                    DraftReviewState::PendingReview => (
                                                        "Draft reply",
                                                        "border-amber-400/30 bg-amber-500/10 text-amber-300",
                                                    ),
                                                    DraftReviewState::ApprovedLocal => (
                                                        "Draft approved",
                                                        "border-emerald-400/35 bg-emerald-500/15 text-emerald-300",
                                                    ),
                                                    DraftReviewState::HeldForVisit => (
                                                        "Held for visit",
                                                        "border-cyan-400/35 bg-cyan-500/15 text-cyan-300",
                                                    ),
                                                }
                                            });
                                            let open_fu = patient
                                                .follow_ups
                                                .iter()
                                                .filter(|f| !matches!(f.standing, FollowUpStanding::Completed))
                                                .count();
                                            let rec_count = patient.records.len();
                                            view! {
                                                <button
                                                    type="button"
                                                    aria-pressed=move || selected_id.get() == pid_aria
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
                                                        <span>{format!("{rec_count} recs · {open_fu} open")}</span>
                                                        {draft_badge.map(|(label, cls)| view! {
                                                            <span class=format!("rounded border px-1.5 py-0.5 {cls}")>
                                                                {label}
                                                            </span>
                                                        })}
                                                    </div>
                                                </button>
                                            }
                                        })
                                        .collect_view()
                                        .into_any()
                                })
                            }}
                        </div>
                    </div>
                </aside>

                // Selected patient chart & grounded Q&A
                <section class=move || {
                    if layout_prefs.get().sidebar == SidebarPlacement::Hidden {
                        "lg:col-span-12"
                    } else {
                        "lg:col-span-8"
                    }
                }>
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

                        let has_reply_draft = patient.reply_draft.is_some();
                        let rec_total = patient.records.len();

                        view! {
                            <div class=move || {
                                let gap = match layout_prefs.get().density {
                                    LayoutDensity::Compact => "gap-4",
                                    LayoutDensity::Balanced => "gap-6",
                                    LayoutDensity::Spacious => "gap-8",
                                };
                                format!("flex flex-col {gap}")
                            }>
                                // Quick in-chart jump bar
                                <nav
                                    aria-label="Chart sections"
                                    class="order-1 flex flex-wrap items-center gap-1.5 rounded-xl border border-white/10 bg-slate-950/60 px-3 py-2 text-xs"
                                >
                                    <span class="pr-1 text-[11px] font-semibold uppercase tracking-wider text-slate-400">
                                        "Jump to:"
                                    </span>
                                    <a href="#chart-summary" class="rounded-full border border-white/10 px-2.5 py-1 text-slate-200 transition hover:border-cyan-400/40 hover:text-white">"Summary"</a>
                                    {move || layout_prefs.get().show_qa.then(|| view! {
                                        <a href="#chart-qa" class="rounded-full border border-white/10 px-2.5 py-1 text-slate-200 transition hover:border-cyan-400/40 hover:text-white">"Chart Q&A"</a>
                                    })}
                                    {move || layout_prefs.get().show_clinical_grid.then(|| view! {
                                        <a href="#chart-clinical" class="rounded-full border border-white/10 px-2.5 py-1 text-slate-200 transition hover:border-emerald-400/40 hover:text-white">"Problems & Meds"</a>
                                    })}
                                    {move || (has_reply_draft && layout_prefs.get().show_reply_draft).then(|| view! {
                                        <a href="#chart-reply" class="rounded-full border border-amber-400/30 bg-amber-500/10 px-2.5 py-1 font-medium text-amber-200 transition hover:bg-amber-500/20">"Proposed Reply"</a>
                                    })}
                                    {move || layout_prefs.get().show_source_records.then(|| view! {
                                        <a href="#chart-records" class="rounded-full border border-white/10 px-2.5 py-1 text-slate-200 transition hover:border-white/30 hover:text-white">
                                            {format!("Source Records ({rec_total})")}
                                        </a>
                                    })}
                                </nav>

                                // Selected patient header
                                <div id="chart-summary" class="order-2 scroll-mt-32 rounded-2xl border border-white/10 bg-white/[0.02] p-6">
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
                                <div
                                    id="chart-qa"
                                    class=move || {
                                        let prefs = layout_prefs.get();
                                        let vis = if prefs.show_qa { "" } else { "hidden " };
                                        let ord = match prefs.order {
                                            PhysicianFocusOrder::QaFirst => "order-3",
                                            PhysicianFocusOrder::ClinicalFirst => "order-4",
                                        };
                                        format!("{vis}{ord} scroll-mt-32 rounded-2xl border border-cyan-400/25 bg-slate-900/50 p-6 space-y-4")
                                    }
                                >
                                    <div>
                                        <p class="text-xs font-semibold uppercase tracking-widest text-cyan-400">
                                            "Chart-Grounded Q&A"
                                        </p>
                                        <h3 class="mt-1 text-lg font-semibold text-white">
                                            "Ask About This Patient's Records"
                                        </h3>
                                        <p class="mt-1 text-xs text-slate-400">
                                            "Scoped strictly to "
                                            <span class="font-mono text-slate-200">{patient.mrn.clone()}</span>
                                            " — cited source record IDs are highlighted below."
                                        </p>
                                    </div>

                                    {move || {
                                        llm_status.get().filter(|s| s.enabled).map(|status| {
                                            view! {
                                                <LocalLlmStatusBanner
                                                    status=status
                                                    use_local_llm=use_local_llm
                                                    preparing=llm_preparing
                                                    prepare_notice=llm_prepare_notice
                                                    on_prepare=prepare_llm_now
                                                />
                                            }
                                        })
                                    }}

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
                                            disabled=move || llm_thinking.get()
                                            class="rounded-full bg-cyan-500 px-5 py-2.5 text-sm font-semibold text-slate-950 transition hover:bg-cyan-400 disabled:opacity-50"
                                        >
                                            {move || if llm_thinking.get() { "Synthesizing…" } else { "Ask chart" }}
                                        </button>
                                    </form>

                                    <div class="flex flex-wrap gap-2">
                                        {presets
                                            .iter()
                                            .map(|&preset| {
                                                view! {
                                                    <button
                                                        type="button"
                                                        disabled=move || llm_thinking.get()
                                                        on:click=move |_| {
                                                            let q = preset.to_string();
                                                            question_input.set(q.clone());
                                                            ask_question(q);
                                                        }
                                                        class="min-h-[34px] rounded-full border border-white/15 bg-slate-950/70 px-3 py-1 text-xs text-slate-300 transition hover:border-cyan-400/40 hover:text-white disabled:cursor-not-allowed disabled:opacity-50"
                                                    >
                                                        {preset}
                                                    </button>
                                                }
                                            })
                                            .collect_view()}
                                    </div>

                                    {move || llm_thinking.get().then(|| view! {
                                        <div
                                            role="status"
                                            aria-live="polite"
                                            class="flex items-center gap-2 rounded-xl border border-cyan-400/25 bg-cyan-500/10 px-3.5 py-2.5 text-xs text-cyan-100"
                                        >
                                            <span
                                                aria-hidden="true"
                                                class="inline-block h-3.5 w-3.5 animate-spin rounded-full border-2 border-cyan-300/30 border-t-cyan-300"
                                            />
                                            <span>"Synthesizing record-cited answer on-device…"</span>
                                        </div>
                                    })}

                                    {move || llm_error.get().map(|msg| view! {
                                        <p role="status" aria-live="polite" class="text-xs text-amber-200">
                                            {msg}
                                        </p>
                                    })}

                                    <div aria-live="polite">
                                        {active_answer.map(|ans| {
                                            let records = patient.records.clone();
                                            view! {
                                                <GroundedAnswerView
                                                    answer=ans
                                                    records=records
                                                    on_select_record=jump_to_record
                                                />
                                            }
                                        })}
                                    </div>
                                </div>

                                // Problems, Medications, Follow-ups
                                <div
                                    id="chart-clinical"
                                    class=move || {
                                        let prefs = layout_prefs.get();
                                        let vis = if prefs.show_clinical_grid { "" } else { "hidden " };
                                        let ord = match prefs.order {
                                            PhysicianFocusOrder::QaFirst => "order-4",
                                            PhysicianFocusOrder::ClinicalFirst => "order-3",
                                        };
                                        format!("{vis}{ord} scroll-mt-32 grid gap-4 md:grid-cols-3")
                                    }
                                >
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

                                // Optional reviewable draft reply card (per-patient state & editable text persisted across patient switches)
                                {patient.reply_draft.clone().map(|draft| {
                                    let pid = patient.id.clone();
                                    let records = patient.records.clone();
                                    view! {
                                        <div
                                            id="chart-reply"
                                            class=move || {
                                                if layout_prefs.get().show_reply_draft {
                                                    "order-5 scroll-mt-32"
                                                } else {
                                                    "hidden"
                                                }
                                            }
                                        >
                                            <ReplyDraftCard
                                                patient_id=pid
                                                draft=draft
                                                records=records
                                                on_select_record=jump_to_record
                                                states=draft_states
                                                texts=draft_texts
                                            />
                                        </div>
                                    }
                                })}

                                // Longitudinal source records
                                <div
                                    id="chart-records"
                                    class=move || {
                                        if layout_prefs.get().show_source_records {
                                            "order-6 scroll-mt-32 rounded-2xl border border-white/10 bg-white/[0.02] p-6 space-y-4"
                                        } else {
                                            "hidden"
                                        }
                                    }
                                >
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
                                                let rec_id = rec.id.clone();
                                                let is_focused = Memo::new(move |_| {
                                                    focused_record_id.with(|f| {
                                                        f.as_deref() == Some(rec_id.as_str())
                                                    })
                                                });
                                                view! {
                                                    {move || {
                                                        view! {
                                                            <SourceRecordCard
                                                                record=rec.clone()
                                                                is_cited=is_cited
                                                                is_focused=is_focused.get()
                                                                on_clear_focus=clear_focused_record
                                                            />
                                                        }
                                                    }}
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

#[component]
fn GroundedAnswerView(
    answer: ChatAnswer,
    records: Vec<ChartRecord>,
    on_select_record: Callback<String>,
) -> impl IntoView {
    let record_map: HashMap<String, ChartRecord> = records
        .iter()
        .cloned()
        .map(|rec| (rec.id.clone(), rec))
        .collect();
    let records_for_basis = records.clone();

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

            <CitationLinkedText
                text=answer.answer
                records=records
                anchor_prefix="chart-record"
                accent=CitationAccent::Cyan
                on_select=on_select_record
                class="text-sm leading-relaxed text-slate-200"
            />

            <div class="space-y-2.5">
                <p class="text-[11px] font-semibold uppercase tracking-wider text-slate-400">
                    "Cited Source Records"
                </p>
                <div class="grid gap-2.5 sm:grid-cols-2">
                    {answer
                        .citations
                        .into_iter()
                        .map(|cite| {
                            let matching_record = record_map.get(&cite.record_id).cloned();
                            view! {
                                <CitationCardLink
                                    citation=cite
                                    record=matching_record
                                    anchor_prefix="chart-record"
                                    accent=CitationAccent::Cyan
                                    on_select=on_select_record
                                />
                            }
                        })
                        .collect_view()}
                </div>
            </div>

            <div class="border-t border-white/5 pt-2.5 font-mono text-[11px] text-slate-400">
                <CitationLinkedText
                    text=answer.basis
                    records=records_for_basis
                    anchor_prefix="chart-record"
                    accent=CitationAccent::Cyan
                    on_select=on_select_record
                />
            </div>
        </div>
    }
}

#[component]
fn ReplyDraftCard(
    patient_id: String,
    draft: ReplyDraft,
    records: Vec<ChartRecord>,
    on_select_record: Callback<String>,
    states: RwSignal<HashMap<String, DraftReviewState>>,
    texts: RwSignal<HashMap<String, String>>,
) -> impl IntoView {
    let original_reply = draft.proposed_reply.clone();
    let message_record = records
        .iter()
        .find(|rec| rec.id == draft.message_record_id)
        .cloned();
    let pid_state = patient_id.clone();
    let pid_text = patient_id.clone();
    let pid_edited = patient_id.clone();
    let pid_input = patient_id.clone();
    let pid_approve = patient_id.clone();
    let pid_hold = patient_id.clone();
    let pid_reset = patient_id.clone();
    let orig_for_edited = original_reply.clone();
    let orig_for_reset = original_reply.clone();

    let current_state = move || {
        states.with(|map| {
            map.get(&pid_state)
                .copied()
                .unwrap_or(DraftReviewState::PendingReview)
        })
    };
    let current_state_cls = current_state.clone();

    let current_text = move || {
        texts.with(|map| {
            map.get(&pid_text)
                .cloned()
                .unwrap_or_else(|| original_reply.clone())
        })
    };

    let is_edited = move || {
        texts.with(|map| {
            map.get(&pid_edited)
                .is_some_and(|val| val != &orig_for_edited)
        })
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
                        {match message_record {
                            Some(rec) => view! {
                                <CitationBadgeLink
                                    record=rec
                                    anchor_prefix="chart-record"
                                    accent=CitationAccent::Cyan
                                    on_select=on_select_record
                                />
                            }
                            .into_any(),
                            None => view! {
                                <span class="font-mono text-amber-200">{draft.message_record_id}</span>
                            }
                            .into_any(),
                        }}
                        " — clinician review and editing required; never auto-sent."
                    </p>
                </div>
                <div class="flex flex-wrap items-center gap-2">
                    {move || is_edited().then(|| view! {
                        <span class="rounded-full border border-cyan-400/30 bg-cyan-500/10 px-2.5 py-0.5 text-[11px] font-medium text-cyan-200">
                            "Edited"
                        </span>
                    })}
                    <span
                        role="status"
                        aria-live="polite"
                        class=move || {
                            format!(
                                "rounded-full border px-3 py-1 text-xs font-semibold {}",
                                current_state_cls().badge_class()
                            )
                        }
                    >
                        {move || current_state().label()}
                    </span>
                </div>
            </div>

            <div class="rounded-xl border border-white/10 bg-slate-950/60 p-3.5 text-xs">
                <p class="font-semibold text-slate-400">"Patient question:"</p>
                <p class="mt-1 text-slate-200">{draft.patient_question}</p>
            </div>

            <label class="block space-y-1.5">
                <span class="text-xs font-medium text-slate-300">
                    "Proposed clinician reply (editable before approval)"
                </span>
                <textarea
                    rows="4"
                    aria-label="Editable patient reply draft"
                    prop:value=current_text
                    on:input=move |ev| {
                        let val = event_target_value(&ev);
                        let pid = pid_input.clone();
                        texts.update(|map| {
                            map.insert(pid.clone(), val);
                        });
                        states.update(|map| {
                            map.insert(pid, DraftReviewState::PendingReview);
                        });
                    }
                    class="w-full rounded-xl border border-white/15 bg-slate-950/85 p-3.5 text-sm leading-relaxed text-slate-100 focus:border-amber-400/60 focus:outline-none focus:ring-1 focus:ring-amber-400/50"
                />
            </label>

            <CitationLinkedText
                text=draft.basis
                records=records
                anchor_prefix="chart-record"
                accent=CitationAccent::Cyan
                on_select=on_select_record
                class="font-mono text-[11px] text-slate-400"
            />

            <div class="flex flex-wrap gap-2 pt-1">
                <button
                    type="button"
                    on:click=move |_| {
                        let pid = pid_approve.clone();
                        states.update(|map| {
                            map.insert(pid, DraftReviewState::ApprovedLocal);
                        });
                    }
                    class="min-h-[38px] rounded-full bg-emerald-500 px-4 py-1.5 text-xs font-semibold text-slate-950 transition hover:bg-emerald-400"
                >
                    "Approve draft (demo)"
                </button>
                <button
                    type="button"
                    on:click=move |_| {
                        let pid = pid_hold.clone();
                        states.update(|map| {
                            map.insert(pid, DraftReviewState::HeldForVisit);
                        });
                    }
                    class="min-h-[38px] rounded-full border border-cyan-400/30 bg-cyan-500/10 px-4 py-1.5 text-xs font-semibold text-cyan-200 transition hover:bg-cyan-500/20"
                >
                    "Hold for visit"
                </button>
                <button
                    type="button"
                    on:click=move |_| {
                        let pid = pid_reset.clone();
                        let orig = orig_for_reset.clone();
                        states.update(|map| {
                            map.insert(pid.clone(), DraftReviewState::PendingReview);
                        });
                        texts.update(|map| {
                            map.insert(pid, orig);
                        });
                    }
                    class="min-h-[38px] rounded-full border border-white/15 px-3.5 py-1.5 text-xs font-medium text-slate-300 transition hover:bg-white/5"
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
    is_cited: bool,
    is_focused: bool,
    on_clear_focus: Callback<()>,
) -> impl IntoView {
    let anchor_id = record_anchor_id("chart-record", &record.id);
    let kind_cls = record_kind_badge_class(record.kind);
    let container_cls = if is_focused {
        "scroll-mt-32 rounded-xl border border-cyan-300 bg-cyan-500/[0.12] p-4 ring-2 ring-cyan-400/80 shadow-lg shadow-cyan-500/10 transition"
    } else if is_cited {
        "scroll-mt-32 rounded-xl border border-cyan-400/60 bg-cyan-500/[0.08] p-4 ring-1 ring-cyan-400/25 transition"
    } else {
        "scroll-mt-32 rounded-xl border border-white/10 bg-slate-950/60 p-4 transition"
    };
    let id_badge_cls = if is_focused || is_cited {
        "rounded bg-cyan-500/25 px-2 py-0.5 font-mono text-xs font-semibold text-cyan-200"
    } else {
        "rounded bg-white/5 px-2 py-0.5 font-mono text-xs font-medium text-slate-300"
    };

    view! {
        <article id=anchor_id class=container_cls>
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
                    {is_focused.then(|| view! {
                        <span class="rounded-full border border-cyan-300/60 bg-cyan-400/20 px-2 py-0.5 text-[10px] font-semibold uppercase tracking-wider text-cyan-100">
                            "Focused from citation"
                        </span>
                    })}
                </div>
                <div class="flex flex-wrap items-center gap-2">
                    {is_focused.then(|| view! {
                        <div class="flex items-center gap-1.5">
                            <a
                                href="#chart-qa"
                                class="rounded-full border border-cyan-400/40 bg-cyan-500/15 px-2.5 py-0.5 text-[11px] font-semibold text-cyan-200 transition hover:border-cyan-300 hover:bg-cyan-500/25 hover:text-white focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-cyan-400"
                            >
                                "Back to Q&A ↑"
                            </a>
                            <button
                                type="button"
                                on:click=move |_| on_clear_focus.run(())
                                class="rounded-full border border-white/15 bg-slate-950/70 px-2.5 py-0.5 text-[11px] font-medium text-slate-200 transition hover:border-white/30 hover:text-white focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-cyan-400"
                            >
                                "Clear"
                            </button>
                        </div>
                    })}
                    <span class="font-mono text-xs text-slate-400">
                        {format!("{} · {}", record.recorded_at, record.source_label)}
                    </span>
                </div>
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
        assert!(html.contains("Curie Clinical Workspace"));
        assert!(html.contains("Curie Clinic"));
        assert!(html.contains("Curie Mobile"));
        assert!(html.contains("/dabney-elephant.svg"));
        assert!(html.contains("/clinical/patient"));
        assert!(html.contains("/clinical/physician"));
        assert!(html.contains("/mental-health"));
        assert!(html.contains("Synthetic Zero-PHI Guarantee"));
        assert!(html.contains("Nadia Kulkarni"));
        assert!(html.contains("David Okafor"));
        for legacy_brand in [
            ["co", "da"].concat(),
            ["du", "et"].concat(),
            ["cho", "rds"].concat(),
        ] {
            assert!(
                !html.to_ascii_lowercase().contains(&legacy_brand),
                "clinical overview HTML must not contain legacy brand string: {legacy_brand}"
            );
        }
    }

    #[test]
    fn physician_workspace_page_renders_all_synthetic_patients_and_citations() {
        let html = render_in_app_context("/clinical/physician", || {
            view! { <PhysicianWorkspacePage /> }.into_any()
        });
        assert!(html.contains("Curie Clinic"));
        assert!(html.contains("Physician Workspace Demo"));
        assert!(html.contains("/dabney-elephant.svg"));
        assert!(html.contains("Dabney.moe Synthetic Clinic"));
        assert!(html.contains("Today's Clinic Queue"));
        assert!(html.contains("aria-label=\"Editable patient reply draft\""));
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
        assert!(html.contains("href=\"#chart-record-nk-cgm-01\""));
        assert!(html.contains("href=\"#chart-record-nk-lab-02\""));
        assert!(html.contains("href=\"#chart-record-nk-msg-03\""));
        assert!(html.contains("id=\"chart-record-nk-cgm-01\""));
        assert!(html.contains("id=\"chart-record-nk-lab-02\""));
        assert!(html.contains("id=\"chart-record-nk-msg-03\""));
        assert!(html.contains("role=\"tooltip\""));
        assert!(html.contains("aria-describedby=\"cite-tip-"));
        assert!(html.contains("Jump to source record ↓"));
        assert!(!html.contains("href=\"#chart-record-cm-lab-02\""));
        assert!(!html.contains("href=\"#chart-record-pn-note-01\""));
        assert!(!html.contains("href=\"#chart-record-do-img-01\""));
        for legacy_brand in [
            ["co", "da"].concat(),
            ["du", "et"].concat(),
            ["cho", "rds"].concat(),
        ] {
            assert!(
                !html.to_ascii_lowercase().contains(&legacy_brand),
                "physician workspace HTML must not contain legacy brand string: {legacy_brand}"
            );
        }
    }

    #[test]
    fn source_record_card_renders_anchor_and_focused_controls() {
        let record = synthetic_panel()
            .into_iter()
            .next()
            .and_then(|chart| chart.records.into_iter().next())
            .expect("nadia primary record");

        let rec_focused = record.clone();
        let focused_html = render_in_app_context("/clinical/physician", move || {
            let on_clear = Callback::new(|()| {});
            view! {
                <SourceRecordCard
                    record=rec_focused
                    is_cited=true
                    is_focused=true
                    on_clear_focus=on_clear
                />
            }
            .into_any()
        });
        assert!(focused_html.contains("id=\"chart-record-nk-cgm-01\""));
        assert!(focused_html.contains("scroll-mt-32"));
        assert!(focused_html.contains("ring-cyan-400/80"));
        assert!(focused_html.contains("Focused from citation"));
        assert!(focused_html.contains("href=\"#chart-qa\""));
        assert!(focused_html.contains("Back to Q&A ↑"));
        assert!(focused_html.contains("Clear"));

        let unfocused_html = render_in_app_context("/clinical/physician", move || {
            let on_clear = Callback::new(|()| {});
            view! {
                <SourceRecordCard
                    record=record
                    is_cited=false
                    is_focused=false
                    on_clear_focus=on_clear
                />
            }
            .into_any()
        });
        assert!(unfocused_html.contains("id=\"chart-record-nk-cgm-01\""));
        assert!(!unfocused_html.contains("Focused from citation"));
        assert!(!unfocused_html.contains("Back to Q&A ↑"));
    }
}
