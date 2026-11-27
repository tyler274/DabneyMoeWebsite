//! Clean-room Dabney.moe Clinical Workspace pages (`/clinical` and `/clinical/physician`).
//!
//! Backed directly by [`clinical_workspace`] with zero external runtime dependencies,
//! rendering identically in native SSR and browser WebAssembly. All displayed patient
//! charts are fictional synthetic examples containing zero real patient data.

use std::collections::HashMap;

#[cfg(any(feature = "csr", feature = "hydrate"))]
use clinical_workspace::build_harness_llm_prompt;
use clinical_workspace::{
    run_agent_turn, synthetic_panel, today_overview, AgentAudience, AgentRunOptions, AgentThread,
    ChartRecord, FollowUpStanding, PatientChart, RecordKind, ReplyDraft, RunBudget, Urgency,
};
use leptos::prelude::*;
use leptos_meta::{Link, Meta, Title};
use leptos_router::components::A;
use serde::{Deserialize, Serialize};

use crate::agent_chat::{AgentToolCatalogBar, AgenticThreadView};
use crate::chart_citations::{
    record_anchor_id, scroll_to_record_anchor, CitationAccent, CitationBadgeLink,
    CitationLinkedText,
};
#[cfg(any(feature = "csr", feature = "hydrate"))]
use crate::dashboard_layout::{load_layout_prefs, save_layout_prefs, STORAGE_KEY_PHYSICIAN_LAYOUT};
use crate::dashboard_layout::{
    LayoutDensity, PhysicianFocusOrder, PhysicianLayoutCustomizer, PhysicianLayoutPrefs,
    PhysicianLayoutPreset, SidebarPlacement,
};
use crate::health_bridge::LocalLlmStatusUi;
#[cfg(any(feature = "csr", feature = "hydrate"))]
use crate::health_bridge::{generate_local_llm, prepare_local_llm, probe_local_llm};
use crate::i18n::{t, use_locale, Locale, Msg};
use crate::local_ai_chat::LocalLlmStatusBanner;
use crate::patient_demo::ClinicalSuiteSwitcher;
use crate::sections::{CrestSize, DabneyCrestBadge, Footer, NavBar};
use crate::AppShellMode;

/// `localStorage` key holding each patient's reply-draft decision.
#[cfg(any(feature = "csr", feature = "hydrate"))]
const DRAFT_STORAGE_KEY: &str = "dabney_clinical_draft_decisions";

/// DOM id of the selected patient's chart header, the scroll and focus
/// target after choosing a patient on a small screen.
const PATIENT_CHART_HEADER_ID: &str = "chart-summary";

const EMPTY_QUESTION_MESSAGE: &str =
    "Type a question about this patient's records, or pick a suggested question below.";

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
    let app_mode = use_context::<AppShellMode>().unwrap_or(AppShellMode::Website);
    let locale = use_locale();
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
                            {move || format!("Curie {}", t(locale.get(), Msg::ToolClinicalSuite))}
                        </h1>
                        <p class="text-base leading-relaxed text-slate-300">
                            "A local-first Rust and WebAssembly clinical systems showcase engineered from scratch: "
                            "typed longitudinal patient charts, strict per-patient citation boundaries, patient-controlled visit packets, and browser-native CGM & mood analytics. "
                            "The physician demo runs its chart search entirely in your browser via WebAssembly and sends no chart data over the network; "
                            "the same crate also serves a read-only JSON API at /clinical/api for scripted access."
                        </p>
                        <div class="flex flex-wrap gap-3 pt-2">
                            <A
                                href="/clinical/patient"
                                attr:class="inline-flex items-center gap-2 rounded-full bg-emerald-500 px-6 py-2.5 text-sm font-semibold text-slate-950 transition hover:bg-emerald-400"
                            >
                                {move || t(locale.get(), Msg::MhOpenMobile)}
                            </A>
                            <A
                                href="/clinical/physician"
                                attr:class="inline-flex items-center gap-2 rounded-full bg-cyan-500 px-6 py-2.5 text-sm font-semibold text-slate-950 transition hover:bg-cyan-400"
                            >
                                {move || t(locale.get(), Msg::MhOpenClinic)}
                            </A>
                            <A
                                href="/mental-health"
                                attr:class="inline-flex items-center gap-2 rounded-full border border-emerald-400/30 bg-emerald-500/10 px-5 py-2.5 text-sm font-semibold text-emerald-200 transition hover:bg-emerald-500/20"
                            >
                                {move || t(locale.get(), Msg::SuiteMentalHealth)}
                            </A>
                            {(app_mode == AppShellMode::Website).then(|| view! {
                                <A
                                    href="/android"
                                    attr:class="inline-flex items-center gap-2 rounded-full border border-cyan-400/35 bg-slate-950/70 px-5 py-2.5 text-sm font-semibold text-cyan-200 transition hover:bg-cyan-500/15"
                                >
                                    {move || t(locale.get(), Msg::ToolAndroidApp)}
                                </A>
                            })}
                        </div>
                    </div>
                    <div class="hidden shrink-0 sm:flex sm:flex-col sm:items-center sm:gap-2.5 sm:rounded-2xl sm:border sm:border-emerald-400/25 sm:bg-[#01081b]/90 sm:p-4 sm:shadow-xl sm:shadow-emerald-500/10">
                        <DabneyCrestBadge size=CrestSize::Lg decorative=false />
                        <div class="text-center">
                            <p class="font-mono text-[10px] font-semibold uppercase tracking-widest text-emerald-300">
                                "dabney.moe"
                            </p>
                            <p class="text-[11px] text-slate-300">
                                "Pure Rust · Zero PHI"
                            </p>
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
                            {move || t(locale.get(), Msg::ZeroPhiBannerTitle)}
                        </p>
                        <p class="mt-1 text-sm leading-relaxed text-slate-200">
                            {overview.synthetic_notice.clone()}
                            " No real patient identifiers, EHR credentials, or protected health information are stored, transmitted, or logged anywhere in this workspace."
                        </p>
                    </div>
                    <span class="shrink-0 self-start rounded-full border border-emerald-400/40 bg-slate-950/60 px-3 py-1 text-xs font-semibold text-emerald-300 sm:self-center">
                        {move || t(locale.get(), Msg::ZeroPhiBannerBadge)}
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
                            "A single dependency-light Rust crate models longitudinal charts, follow-ups, reviewable draft replies, "
                            "and deterministic whole-word chart search. This site runs it in your browser via WebAssembly without external cloud dependencies; the server exposes the same functions as a JSON API."
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
    let app_mode = use_context::<AppShellMode>().unwrap_or(AppShellMode::Website);
    let locale = use_locale();
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

    let (initial_id, initial_question, initial_threads, initial_draft_texts) =
        panel.with_value(|charts| {
            let mut texts = HashMap::new();
            for chart in charts {
                if let Some(draft) = chart.reply_draft.as_ref() {
                    texts.insert(chart.id.clone(), draft.proposed_reply.clone());
                }
            }
            let mut map = HashMap::new();
            if let Some(first) = charts.first() {
                let q = preset_questions_for_patient(&first.id)
                    .first()
                    .copied()
                    .unwrap_or("Summarize recent records")
                    .to_string();
                let mut thread = AgentThread::new(&first.id, AgentAudience::Physician);
                let turn = run_agent_turn(
                    first,
                    &q,
                    AgentAudience::Physician,
                    thread.next_turn_index(),
                    &thread.working_notes,
                    AgentRunOptions::default(),
                );
                thread.record_turn(turn);
                map.insert(first.id.clone(), thread);
                (first.id.clone(), q, map, texts)
            } else {
                (String::new(), String::new(), map, texts)
            }
        });

    let selected_id = RwSignal::new(initial_id);
    let focused_record_id = RwSignal::new(Option::<String>::None);
    let panel_filter = RwSignal::new(PanelFilter::All);
    let panel_search = RwSignal::new(String::new());
    let question_input = RwSignal::new(initial_question);
    let question_error = RwSignal::new(false);
    let step_budget = RwSignal::new(6usize);
    let threads = RwSignal::new(initial_threads);
    // Server render and first hydration pass start with every draft pending,
    // so the markup matches; saved decisions are applied once mounted.
    let draft_decisions = RwSignal::new(DraftDecisions::default());
    let draft_texts = RwSignal::new(initial_draft_texts);

    #[cfg(any(feature = "csr", feature = "hydrate"))]
    Effect::new(move |_| {
        if let Some(saved) = load_draft_decisions() {
            draft_decisions.set(saved);
        }
    });

    let cited_ids = Memo::new(move |_| {
        let active_id = selected_id.get();
        threads.with(|map| {
            map.get(&active_id)
                .map(AgentThread::latest_cited_ids)
                .unwrap_or_default()
        })
    });

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
        let default_q = preset_questions_for_patient(&patient_id)
            .first()
            .copied()
            .unwrap_or("Summarize recent records")
            .to_string();
        let has_existing = threads.with_untracked(|map| {
            map.get(&patient_id)
                .is_some_and(|thread| !thread.turns.is_empty())
        });
        if !has_existing {
            let max_steps = step_budget.get_untracked();
            let seeded = panel.with_value(|charts| {
                charts.iter().find(|p| p.id == patient_id).map(|chart| {
                    let mut thread = AgentThread::new(&chart.id, AgentAudience::Physician);
                    let turn = run_agent_turn(
                        chart,
                        &default_q,
                        AgentAudience::Physician,
                        thread.next_turn_index(),
                        &thread.working_notes,
                        AgentRunOptions {
                            budget: RunBudget {
                                max_steps,
                                max_tool_calls: max_steps,
                                max_citations: 4,
                            },
                            stop_after_step: None,
                        },
                    );
                    thread.record_turn(turn);
                    thread
                })
            });
            if let Some(thread) = seeded {
                threads.update(|map| {
                    map.insert(patient_id.clone(), thread);
                });
            }
        }
        question_input.set(default_q);
        question_error.set(false);
        selected_id.set(patient_id);
        #[cfg(any(feature = "csr", feature = "hydrate"))]
        reveal_chart_on_small_screens();
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
        let trimmed = raw_q.trim();
        if trimmed.is_empty() {
            question_error.set(true);
            return;
        }
        question_error.set(false);
        focused_record_id.set(None);
        let active_id = selected_id.get_untracked();
        let max_steps = step_budget.get_untracked();
        let (turn_index, prior_notes) = threads.with_untracked(|map| {
            map.get(&active_id)
                .map(|t| (t.next_turn_index(), t.working_notes.clone()))
                .unwrap_or_else(|| (1, Vec::new()))
        });
        let next_pair = panel.with_value(|charts| {
            charts.iter().find(|p| p.id == active_id).map(|p| {
                let turn = run_agent_turn(
                    p,
                    trimmed,
                    AgentAudience::Physician,
                    turn_index,
                    &prior_notes,
                    AgentRunOptions {
                        budget: RunBudget {
                            max_steps,
                            max_tool_calls: max_steps,
                            max_citations: 4,
                        },
                        stop_after_step: None,
                    },
                );
                (p.clone(), turn)
            })
        });
        let Some((patient_chart, turn)) = next_pair else {
            return;
        };
        threads.update(|map| {
            map.entry(active_id.clone())
                .or_insert_with(|| AgentThread::new(&active_id, AgentAudience::Physician))
                .record_turn(turn.clone());
        });
        llm_error.set(None);

        let req_id = llm_req_seq.get_untracked() + 1;
        llm_req_seq.set(req_id);

        let should_run_llm = use_local_llm.get_untracked()
            && turn.grounded
            && llm_status
                .get_untracked()
                .is_some_and(|s| s.enabled && s.availability == "available");

        #[cfg(any(feature = "csr", feature = "hydrate"))]
        if should_run_llm {
            let prompt = build_harness_llm_prompt(&patient_chart, &turn);
            let pid_for_cb = active_id.clone();
            llm_thinking.set(true);
            generate_local_llm(&prompt, Some(360), move |res| {
                if llm_req_seq.get_untracked() != req_id {
                    return;
                }
                llm_thinking.set(false);
                match res {
                    Ok(gen) => {
                        let mut enhanced = turn.clone();
                        enhanced.answer = gen.text;
                        enhanced.engine_label = format!(
                            "Agentic Harness + {} ({} ms)",
                            gen.model_name, gen.latency_ms
                        );
                        enhanced.basis = format!(
                            "{} (on-device · {} ms) synthesized strictly from {} verified harness step(s) and {} citation(s).",
                            gen.model_name,
                            gen.latency_ms,
                            enhanced.steps.len(),
                            enhanced.citations.len()
                        );
                        threads.update(|map| {
                            if let Some(thread) = map.get_mut(&pid_for_cb) {
                                thread.replace_turn(enhanced);
                            }
                        });
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
        let _ = (patient_chart, should_run_llm);
    };

    let clear_active_thread = move |_| {
        let active_id = selected_id.get_untracked();
        llm_req_seq.update(|n| *n += 1);
        llm_thinking.set(false);
        focused_record_id.set(None);
        threads.update(|map| {
            if let Some(thread) = map.get_mut(&active_id) {
                thread.clear();
            }
        });
    };

    let on_submit_question = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        ask_question(question_input.get_untracked());
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
                            {move || t(locale.get(), Msg::PhysicianEyebrow)}
                            {format!(" · {} · {}", overview.practice_name, overview.clinic_date)}
                        </p>
                        <h1 class="text-2xl font-bold tracking-tight text-white sm:text-3xl">
                            {move || t(locale.get(), Msg::PhysicianTitle)}
                        </h1>
                    </div>
                </div>
                <div class="flex flex-wrap items-center gap-2.5 text-xs">
                    <A
                        href="/clinical/patient"
                        attr:class="rounded-full border border-emerald-400/35 bg-emerald-500/15 px-4 py-2 font-medium text-emerald-200 transition hover:bg-emerald-500/25"
                    >
                        {move || t(locale.get(), Msg::SuitePatient)}
                    </A>
                    <A
                        href="/clinical"
                        attr:class="inline-flex items-center gap-1 rounded-full border border-white/15 px-4 py-2 font-medium text-slate-200 transition hover:bg-white/5"
                    >
                        <span class="inline-block rtl:rotate-180" aria-hidden="true">"←"</span>
                        <span>{move || t(locale.get(), Msg::SuiteOverview)}</span>
                    </A>
                    <A
                        href="/mental-health"
                        attr:class="rounded-full border border-emerald-400/30 bg-emerald-500/10 px-4 py-2 font-medium text-emerald-200 transition hover:bg-emerald-500/20"
                    >
                        {move || t(locale.get(), Msg::SuiteMentalHealth)}
                    </A>
                </div>
            </header>

            <ClinicalSuiteSwitcher />

            // Synthetic-data banner
            <div
                class="flex flex-wrap items-center justify-between gap-3 rounded-2xl border border-emerald-400/30 bg-emerald-500/10 px-5 py-3.5"
                role="note"
            >
                <p class="text-xs leading-relaxed text-emerald-100 sm:text-sm">
                    <span class="font-semibold text-emerald-300">
                        {move || format!("{}: ", t(locale.get(), Msg::ZeroPhiBannerBadge))}
                    </span>
                    {overview.synthetic_notice.clone()}
                    " Chart search runs deterministically in Rust/WASM on your device, with zero external services."
                </p>
                <span class="rounded-full border border-emerald-400/40 bg-slate-950/60 px-3 py-1 text-[11px] font-semibold uppercase tracking-wider text-emerald-300">
                    {move || t(locale.get(), Msg::OfflineReadyBadge)}
                </span>
            </div>

            // Compact clinic summary & urgency filter strip (deduplicated from Patient Panel)
            <section
                aria-label="Today's clinic queue summary and filters"
                class="flex flex-wrap items-center justify-between gap-3 rounded-2xl border border-white/10 bg-white/[0.02] px-5 py-3.5"
            >
                <div class="flex flex-wrap items-center gap-2 text-xs">
                    <h2 class="font-semibold uppercase tracking-wider text-cyan-300">
                        {move || t(locale.get(), Msg::PhysicianTodayQueue)}
                    </h2>
                    <span class="text-slate-400">
                        {format!("· {total_patients} charts · {draft_count} draft replies")}
                    </span>
                </div>
                <div class="flex flex-wrap items-center gap-1.5 text-xs" role="group" aria-label="Filter patient panel">
                    {[
                        (PanelFilter::All, Msg::PhysicianFilterAll, total_patients),
                        (PanelFilter::High, Msg::PhysicianFilterHigh, high_count),
                        (PanelFilter::Moderate, Msg::PhysicianFilterModerate, moderate_count),
                        (PanelFilter::Routine, Msg::PhysicianFilterRoutine, routine_count),
                        (PanelFilter::HasDraft, Msg::PhysicianFilterDrafts, draft_count),
                    ]
                    .into_iter()
                    .map(|(f, msg, count)| {
                        view! {
                            <button
                                type="button"
                                aria-pressed=move || (panel_filter.get() == f).to_string()
                                on:click=move |_| panel_filter.set(f)
                                class=move || {
                                    if panel_filter.get() == f {
                                        "min-h-[34px] rounded-full bg-cyan-500 px-3 py-1 font-semibold text-slate-950 transition"
                                    } else {
                                        "min-h-[34px] rounded-full border border-white/15 bg-slate-950/60 px-3 py-1 font-medium text-slate-300 transition hover:border-white/30 hover:text-white"
                                    }
                                }
                            >
                                {move || format!("{} ({count})", t(locale.get(), msg))}
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
                                    {move || t(locale.get(), Msg::PhysicianShowPanel)}
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
                        {move || {
                            let loc = locale.get();
                            let preset_label = match layout_prefs.get().preset {
                                Some(PhysicianLayoutPreset::FocusedTriage) => {
                                    t(loc, Msg::LayoutPresetFocusedTriage)
                                }
                                Some(PhysicianLayoutPreset::ChartFirstSplit) => {
                                    t(loc, Msg::LayoutPresetChartFirstSplit)
                                }
                                Some(PhysicianLayoutPreset::CalmReading) => {
                                    t(loc, Msg::LayoutPresetCalmReading)
                                }
                                None => t(loc, Msg::LayoutCustomLabel),
                            };
                            format!("{}: {} ▾", t(loc, Msg::LayoutButtonPrefix), preset_label)
                        }}
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
                                                aria-pressed=move || (selected_id.get() == pid_active).to_string()
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
                                {move || t(locale.get(), Msg::PhysicianPatientPanel)}
                            </h2>
                            <span class="text-[11px] text-slate-400">{format!("{total_patients} total")}</span>
                        </div>
                        <div class="mb-3">
                            <label for="physician-panel-search" class="sr-only">
                                {move || t(locale.get(), Msg::PhysicianSearchPlaceholder)}
                            </label>
                            <input
                                id="physician-panel-search"
                                type="search"
                                placeholder=move || t(locale.get(), Msg::PhysicianSearchPlaceholder)
                                prop:value=move || panel_search.get()
                                on:input=move |ev| panel_search.set(event_target_value(&ev))
                                class="min-h-[38px] w-full rounded-xl border border-white/15 bg-slate-950 px-3 py-1.5 text-xs text-white placeholder-slate-400 focus:border-cyan-400 focus:outline-none"
                            />
                        </div>
                        <div class="space-y-2.5">
                            {move || {
                                let active_filter = panel_filter.get();
                                let search_q = panel_search.get().trim().to_lowercase();
                                let decisions_map = draft_decisions.get();
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
                                                {move || t(locale.get(), Msg::PhysicianNoMatch)}
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
                                                let st = decisions_map.get(&patient.id);
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
                                                            "w-full rounded-xl border border-cyan-400/60 bg-cyan-500/10 p-3.5 text-start transition"
                                                        } else {
                                                            "w-full rounded-xl border border-white/10 bg-slate-950/50 p-3.5 text-start transition hover:border-white/25 hover:bg-white/[0.03]"
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

                        let presets = preset_questions_for_patient(&patient.id);
                        let urgency_cls = urgency_badge_class(patient.urgency);
                        let answer_patient_id = patient.id.clone();

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
                                    <span class="pe-1 text-[11px] font-semibold uppercase tracking-wider text-slate-400">
                                        {move || t(locale.get(), Msg::PhysicianJumpTo)}
                                    </span>
                                    <a href="#chart-summary" class="rounded-full border border-white/10 px-2.5 py-1 text-slate-200 transition hover:border-cyan-400/40 hover:text-white">
                                        {move || t(locale.get(), Msg::PhysicianJumpSummary)}
                                    </a>
                                    {move || layout_prefs.get().show_qa.then(|| view! {
                                        <a href="#chart-qa" class="rounded-full border border-white/10 px-2.5 py-1 text-slate-200 transition hover:border-cyan-400/40 hover:text-white">
                                            {move || t(locale.get(), Msg::PhysicianJumpQa)}
                                        </a>
                                    })}
                                    {move || layout_prefs.get().show_clinical_grid.then(|| view! {
                                        <a href="#chart-clinical" class="rounded-full border border-white/10 px-2.5 py-1 text-slate-200 transition hover:border-emerald-400/40 hover:text-white">
                                            {move || t(locale.get(), Msg::PhysicianJumpClinical)}
                                        </a>
                                    })}
                                    {move || (has_reply_draft && layout_prefs.get().show_reply_draft).then(|| view! {
                                        <a href="#chart-reply" class="rounded-full border border-amber-400/30 bg-amber-500/10 px-2.5 py-1 font-medium text-amber-200 transition hover:bg-amber-500/20">
                                            {move || t(locale.get(), Msg::PhysicianJumpReply)}
                                        </a>
                                    })}
                                    {move || layout_prefs.get().show_source_records.then(|| view! {
                                        <a href="#chart-records" class="rounded-full border border-white/10 px-2.5 py-1 text-slate-200 transition hover:border-white/30 hover:text-white">
                                            {move || format!("{} ({rec_total})", t(locale.get(), Msg::PhysicianJumpRecords))}
                                        </a>
                                    })}
                                </nav>

                                // Selected patient header
                                <div
                                    id=PATIENT_CHART_HEADER_ID
                                    role="region"
                                    tabindex="-1"
                                    aria-label=format!("Chart for {}", patient.display_name)
                                    class="order-2 scroll-mt-32 rounded-2xl border border-white/10 bg-white/[0.02] p-6 focus:outline-none focus-visible:ring-2 focus-visible:ring-cyan-400/60"
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
                                            <div class="rounded-xl border border-cyan-400/25 bg-cyan-500/10 px-3.5 py-2 text-end">
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

                                // Interactive Bounded Agentic Chat Harness
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
                                    <div class="flex flex-wrap items-start justify-between gap-3">
                                        <div>
                                            <p class="text-xs font-semibold uppercase tracking-widest text-cyan-400">
                                                "Bounded Agentic Chat Harness"
                                            </p>
                                            <h3 class="mt-1 text-lg font-semibold text-white">
                                                {move || t(locale.get(), Msg::PhysicianQaHeading)}
                                            </h3>
                                            <p class="mt-1 text-xs text-slate-400">
                                                "Multi-step tool-calling retrieval with working notes scoped strictly to "
                                                <span class="font-mono text-slate-200">{patient.mrn.clone()}</span>
                                                " — cited source record IDs are highlighted below."
                                            </p>
                                        </div>
                                        <div class="flex flex-wrap items-center gap-2 text-xs">
                                            <label class="inline-flex items-center gap-1.5 text-slate-300">
                                                <span class="text-[11px] text-slate-400">"Step budget"</span>
                                                <select
                                                    aria-label="Agent step budget"
                                                    class="rounded-lg border border-white/15 bg-slate-950 px-2 py-1 font-mono text-[11px] text-slate-200 focus:border-cyan-400/60 focus:outline-none"
                                                    prop:value=move || step_budget.get().to_string()
                                                    on:change=move |ev| {
                                                        if let Ok(parsed) = event_target_value(&ev).parse::<usize>() {
                                                            step_budget.set(parsed.clamp(2, 6));
                                                        }
                                                    }
                                                >
                                                    <option value="2">"2 steps"</option>
                                                    <option value="4">"4 steps"</option>
                                                    <option value="6">"6 steps"</option>
                                                </select>
                                            </label>
                                            <button
                                                type="button"
                                                on:click=clear_active_thread
                                                class="rounded-full border border-white/15 px-3 py-1 text-[11px] text-slate-300 transition hover:border-cyan-400/40 hover:text-white"
                                            >
                                                "Clear thread"
                                            </button>
                                        </div>
                                    </div>

                                    <AgentToolCatalogBar audience=AgentAudience::Physician />

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

                                    <form on:submit=on_submit_question class="space-y-2" novalidate>
                                        <div class="flex flex-col gap-2.5 sm:flex-row">
                                            <input
                                                type="text"
                                                aria-label="Ask a clinical question about the selected chart"
                                                aria-invalid=move || question_error.get().to_string()
                                                aria-describedby="chart-question-error"
                                                placeholder=move || t(locale.get(), Msg::PhysicianAskPlaceholder)
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
                                                disabled=move || llm_thinking.get()
                                                class="rounded-full bg-cyan-500 px-5 py-2.5 text-sm font-semibold text-slate-950 transition hover:bg-cyan-400 disabled:opacity-50"
                                            >
                                                {move || {
                                                    let loc = locale.get();
                                                    if llm_thinking.get() {
                                                        t(loc, Msg::PhysicianSynthesizing)
                                                    } else {
                                                        t(loc, Msg::PhysicianAskButton)
                                                    }
                                                }}
                                            </button>
                                        </div>
                                        <p id="chart-question-error" class="min-h-[1rem] text-xs text-rose-300" role="alert">
                                            {move || question_error.get().then_some(EMPTY_QUESTION_MESSAGE)}
                                        </p>
                                    </form>

                                    <div class="flex flex-wrap gap-2">
                                        {presets
                                            .iter()
                                            .copied()
                                            .chain([
                                                "Review active medications and follow-up plan",
                                                "Recap earlier working notes",
                                            ])
                                            .map(|preset| {
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

                                    <div aria-live="polite" aria-label="Chart answer">
                                        {
                                            let thread_records = patient.records.clone();
                                            move || {
                                                let recs = thread_records.clone();
                                                threads
                                                    .get()
                                                    .get(&answer_patient_id)
                                                    .filter(|t| !t.turns.is_empty())
                                                    .cloned()
                                                    .map(|thread| view! {
                                                        <AgenticThreadView
                                                            thread=thread
                                                            active_record=focused_record_id
                                                            records=Some(recs)
                                                            anchor_prefix="chart-record"
                                                            on_select_record=Some(jump_to_record)
                                                        />
                                                    })
                                            }
                                        }
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
                                            {move || t(locale.get(), Msg::PhysicianProblems)}
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
                                            {move || t(locale.get(), Msg::PhysicianMedications)}
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
                                            {move || t(locale.get(), Msg::PhysicianFollowUps)}
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
                                                decisions=draft_decisions
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
                                                {move || t(locale.get(), Msg::PhysicianSourceRecords)}
                                            </h3>
                                            <p class="text-xs text-slate-400">
                                                "Labs, CGM summaries, progress notes, imaging, and inbound messages. Records cited by the active Q&A response are highlighted."
                                            </p>
                                        </div>
                                        <span class="rounded-full border border-white/10 bg-slate-950 px-3 py-1 font-mono text-xs text-slate-300">
                                            {count_label(patient.records.len(), "record", "records")}
                                        </span>
                                    </div>
                                    {move || {
                                        let loc = locale.get();
                                        (loc != Locale::En).then(|| view! {
                                            <p
                                                role="note"
                                                class="rounded-xl border border-cyan-400/25 bg-cyan-500/10 px-3.5 py-2 text-xs text-cyan-100"
                                            >
                                                {t(loc, Msg::CitationSourceLanguageNote)}
                                            </p>
                                        })
                                    }}
                                    <div class="space-y-3">
                                        {patient
                                            .records
                                            .into_iter()
                                            .map(|rec| {
                                                let rec_id = rec.id.clone();
                                                let rec_id_for_cite = rec.id.clone();
                                                let is_focused = Memo::new(move |_| {
                                                    focused_record_id.with(|f| {
                                                        f.as_deref() == Some(rec_id.as_str())
                                                    })
                                                });
                                                let is_cited = Memo::new(move |_| {
                                                    cited_ids.with(|ids| ids.contains(&rec_id_for_cite))
                                                });
                                                view! {
                                                    {move || {
                                                        view! {
                                                            <SourceRecordCard
                                                                record=rec.clone()
                                                                is_cited=is_cited.get()
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
fn ReplyDraftCard(
    patient_id: String,
    draft: ReplyDraft,
    records: Vec<ChartRecord>,
    on_select_record: Callback<String>,
    decisions: RwSignal<DraftDecisions>,
    texts: RwSignal<HashMap<String, String>>,
) -> impl IntoView {
    let original_reply = draft.proposed_reply.clone();
    let message_record = records
        .iter()
        .find(|rec| rec.id == draft.message_record_id)
        .cloned();
    let patient_id_stored = StoredValue::new(patient_id.clone());
    let pid_text = patient_id.clone();
    let pid_edited = patient_id.clone();
    let pid_input = patient_id.clone();
    let pid_reset = patient_id;
    let orig_for_edited = original_reply.clone();
    let orig_for_reset = original_reply.clone();

    let state =
        Memo::new(move |_| decisions.with(|d| patient_id_stored.with_value(|id| d.get(id))));
    let decide = move |next: DraftReviewState| {
        decisions.update(|d| patient_id_stored.with_value(|id| d.set(id, next)));
        #[cfg(any(feature = "csr", feature = "hydrate"))]
        decisions.with_untracked(store_draft_decisions);
    };

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
                                state.get().badge_class()
                            )
                        }
                    >
                        {move || state.get().label()}
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
                            map.insert(pid, val);
                        });
                        decide(DraftReviewState::PendingReview);
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
                    aria-pressed=move || (state.get() == DraftReviewState::ApprovedLocal).to_string()
                    on:click=move |_| decide(DraftReviewState::ApprovedLocal)
                    class="min-h-[38px] rounded-full bg-emerald-500 px-4 py-1.5 text-xs font-semibold text-slate-950 transition hover:bg-emerald-400"
                >
                    "Approve draft (demo)"
                </button>
                <button
                    type="button"
                    aria-pressed=move || (state.get() == DraftReviewState::HeldForVisit).to_string()
                    on:click=move |_| decide(DraftReviewState::HeldForVisit)
                    class="min-h-[38px] rounded-full border border-cyan-400/30 bg-cyan-500/10 px-4 py-1.5 text-xs font-semibold text-cyan-200 transition hover:bg-cyan-500/20"
                >
                    "Hold for visit"
                </button>
                <button
                    type="button"
                    on:click=move |_| {
                        let pid = pid_reset.clone();
                        let orig = orig_for_reset.clone();
                        decide(DraftReviewState::PendingReview);
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
    let locale = use_locale();
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
                            {move || t(locale.get(), Msg::CitationFocusedBadge)}
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
                                {move || t(locale.get(), Msg::PhysicianBackToQa)}
                            </a>
                            <button
                                type="button"
                                on:click=move |_| on_clear_focus.run(())
                                class="rounded-full border border-white/15 bg-slate-950/70 px-2.5 py-0.5 text-[11px] font-medium text-slate-200 transition hover:border-white/30 hover:text-white focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-cyan-400"
                            >
                                {move || t(locale.get(), Msg::PhysicianClearFocus)}
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
        assert!(html.contains("/favicon.png"));
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
        assert!(html.contains("Bounded Agentic Chat Harness"));
        assert!(html.contains("chart_search"));
        assert!(html.contains("chart_read"));
        assert!(html.contains("working_note_save"));
        assert!(html.contains("Cited values:"));
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

    #[test]
    fn physician_workspace_is_accessible_and_linkable() {
        let html = render_in_app_context("/clinical/physician", || {
            view! { <PhysicianWorkspacePage /> }.into_any()
        });
        // Live regions for the answer and the draft decision.
        assert!(html.contains(r#"aria-live="polite" aria-label="Chart answer""#));
        assert!(html.contains(r#"role="status" aria-live="polite""#));
        // Citation chips link to their source records, which carry the anchor.
        assert!(html.contains(r##"href="#chart-record-nk-cgm-01""##));
        assert!(html.contains(r#"id="chart-record-nk-cgm-01""#));
        assert!(html.contains("scroll-mt-32 rounded-xl"));
        // Empty-question guidance target and mobile chart focus target.
        assert!(html.contains(r#"aria-describedby="chart-question-error""#));
        assert!(html.contains(r#"id="chart-summary" role="region" tabindex="-1""#));
        // Sticky sidebar panel selector and AA-contrast metadata.
        assert!(html.contains("lg:col-span-4 space-y-3 lg:sticky"));
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
