//! Persistent, user-customizable dashboard layouts & suggested presets for both
//! patients (`/mental-health`, `/clinical/patient`) and physicians (`/clinical/physician`).
//!
//! All layout preferences persist locally in `localStorage` (`dabney.layout.*.v1`) so
//! customizations stay strictly on the user's device, survive reloads, and restore safely
//! after SSR hydration without layout mismatches.

use leptos::prelude::*;
use serde::{Deserialize, Serialize};

use crate::i18n::{t, use_locale, Locale, Msg};

#[cfg(any(feature = "hydrate", feature = "csr"))]
pub const STORAGE_KEY_PHYSICIAN_LAYOUT: &str = "dabney.layout.physician.v1";
#[cfg(any(feature = "hydrate", feature = "csr"))]
pub const STORAGE_KEY_PATIENT_LAYOUT: &str = "dabney.layout.patient.v1";
#[cfg(any(feature = "hydrate", feature = "csr"))]
pub const STORAGE_KEY_MENTAL_HEALTH_LAYOUT: &str = "dabney.layout.mental_health.v1";

/// Shared visual density across all dashboards.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum LayoutDensity {
    Compact,
    #[default]
    Balanced,
    Spacious,
}

impl LayoutDensity {
    pub const ALL: &'static [Self] = &[Self::Compact, Self::Balanced, Self::Spacious];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Compact => "Compact",
            Self::Balanced => "Balanced",
            Self::Spacious => "Spacious",
        }
    }

    pub const fn label_for(self, locale: Locale) -> &'static str {
        match self {
            Self::Compact => t(locale, Msg::LayoutDensityCompact),
            Self::Balanced => t(locale, Msg::LayoutDensityBalanced),
            Self::Spacious => t(locale, Msg::LayoutDensitySpacious),
        }
    }

    pub const fn stack_gap_class(self) -> &'static str {
        match self {
            Self::Compact => "space-y-4",
            Self::Balanced => "space-y-6",
            Self::Spacious => "space-y-8",
        }
    }

    pub const fn container_width_class(self) -> &'static str {
        match self {
            Self::Compact => "max-w-7xl",
            Self::Balanced => "max-w-6xl",
            Self::Spacious => "max-w-5xl",
        }
    }
}

// ============================================================================
// 1. Physician Workspace Layout (`/clinical/physician`)
// ============================================================================

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SidebarPlacement {
    #[default]
    Left,
    Right,
    Hidden,
}

impl SidebarPlacement {
    pub const ALL: &'static [Self] = &[Self::Left, Self::Right, Self::Hidden];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Left => "Panel left",
            Self::Right => "Panel right",
            Self::Hidden => "Full-width chart",
        }
    }

    pub const fn label_for(self, locale: Locale) -> &'static str {
        match self {
            Self::Left => t(locale, Msg::LayoutPanelLeft),
            Self::Right => t(locale, Msg::LayoutPanelRight),
            Self::Hidden => t(locale, Msg::LayoutPanelFullWidth),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PhysicianFocusOrder {
    #[default]
    QaFirst,
    ClinicalFirst,
}

impl PhysicianFocusOrder {
    pub const fn label(self) -> &'static str {
        match self {
            Self::QaFirst => "Chart Q&A first",
            Self::ClinicalFirst => "Problems & Meds first",
        }
    }

    pub const fn label_for(self, locale: Locale) -> &'static str {
        match self {
            Self::QaFirst => t(locale, Msg::LayoutQaFirst),
            Self::ClinicalFirst => t(locale, Msg::LayoutClinicalFirst),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PhysicianLayoutPreset {
    #[default]
    FocusedTriage,
    ChartFirstSplit,
    CalmReading,
}

impl PhysicianLayoutPreset {
    pub const ALL: &'static [Self] = &[
        Self::FocusedTriage,
        Self::ChartFirstSplit,
        Self::CalmReading,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::FocusedTriage => "Focused Triage",
            Self::ChartFirstSplit => "Chart-First Split",
            Self::CalmReading => "Calm Reading",
        }
    }

    pub const fn label_for(self, locale: Locale) -> &'static str {
        match self {
            Self::FocusedTriage => t(locale, Msg::LayoutPresetFocusedTriage),
            Self::ChartFirstSplit => t(locale, Msg::LayoutPresetChartFirstSplit),
            Self::CalmReading => t(locale, Msg::LayoutPresetCalmReading),
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::FocusedTriage => {
                "Left patient panel, Chart Q&A + Draft Reply up top for fast inbox & visit triage."
            }
            Self::ChartFirstSplit => {
                "Compact wide split with right-hand panel and Problems, Meds & Follow-Ups above Q&A."
            }
            Self::CalmReading => {
                "Full-width spacious chart reading with the patient panel tucked out of the way."
            }
        }
    }

    pub fn build(self) -> PhysicianLayoutPrefs {
        match self {
            Self::FocusedTriage => PhysicianLayoutPrefs {
                preset: Some(Self::FocusedTriage),
                sidebar: SidebarPlacement::Left,
                density: LayoutDensity::Balanced,
                order: PhysicianFocusOrder::QaFirst,
                show_qa: true,
                show_clinical_grid: true,
                show_reply_draft: true,
                show_source_records: true,
            },
            Self::ChartFirstSplit => PhysicianLayoutPrefs {
                preset: Some(Self::ChartFirstSplit),
                sidebar: SidebarPlacement::Right,
                density: LayoutDensity::Compact,
                order: PhysicianFocusOrder::ClinicalFirst,
                show_qa: true,
                show_clinical_grid: true,
                show_reply_draft: true,
                show_source_records: true,
            },
            Self::CalmReading => PhysicianLayoutPrefs {
                preset: Some(Self::CalmReading),
                sidebar: SidebarPlacement::Hidden,
                density: LayoutDensity::Spacious,
                order: PhysicianFocusOrder::ClinicalFirst,
                show_qa: true,
                show_clinical_grid: true,
                show_reply_draft: true,
                show_source_records: true,
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PhysicianLayoutPrefs {
    pub preset: Option<PhysicianLayoutPreset>,
    pub sidebar: SidebarPlacement,
    pub density: LayoutDensity,
    pub order: PhysicianFocusOrder,
    pub show_qa: bool,
    pub show_clinical_grid: bool,
    pub show_reply_draft: bool,
    pub show_source_records: bool,
}

impl Default for PhysicianLayoutPrefs {
    fn default() -> Self {
        PhysicianLayoutPreset::FocusedTriage.build()
    }
}

impl PhysicianLayoutPrefs {
    pub fn active_label(&self) -> &'static str {
        self.preset.map_or("Custom", PhysicianLayoutPreset::label)
    }

    pub fn active_label_for(&self, locale: Locale) -> &'static str {
        self.preset.map_or_else(
            || t(locale, Msg::LayoutCustomLabel),
            |p| p.label_for(locale),
        )
    }
}

// ============================================================================
// 2. Patient Companion Layout (`/clinical/patient`)
// ============================================================================

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PatientSectionOrder {
    #[default]
    AskFirst,
    RecordsFirst,
}

impl PatientSectionOrder {
    pub const fn label(self) -> &'static str {
        match self {
            Self::AskFirst => "Ask Curie first",
            Self::RecordsFirst => "Health records first",
        }
    }

    pub const fn label_for(self, locale: Locale) -> &'static str {
        match self {
            Self::AskFirst => t(locale, Msg::LayoutAskFirst),
            Self::RecordsFirst => t(locale, Msg::LayoutRecordsFirst),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PatientStartTab {
    #[default]
    You,
    Shared,
}

impl PatientStartTab {
    pub const fn label(self) -> &'static str {
        match self {
            Self::You => "Your Health (You)",
            Self::Shared => "For My Doctor",
        }
    }

    pub const fn label_for(self, locale: Locale) -> &'static str {
        match self {
            Self::You => t(locale, Msg::PatientTabYou),
            Self::Shared => t(locale, Msg::PatientTabDoctor),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PatientLayoutPreset {
    #[default]
    CalmCompanion,
    RecordsFirstCompact,
    VisitPrepFocus,
}

impl PatientLayoutPreset {
    pub const ALL: &'static [Self] = &[
        Self::CalmCompanion,
        Self::RecordsFirstCompact,
        Self::VisitPrepFocus,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::CalmCompanion => "Calm Companion",
            Self::RecordsFirstCompact => "Records-First Compact",
            Self::VisitPrepFocus => "Visit Prep Focus",
        }
    }

    pub const fn label_for(self, locale: Locale) -> &'static str {
        match self {
            Self::CalmCompanion => t(locale, Msg::LayoutPresetCalmCompanion),
            Self::RecordsFirstCompact => t(locale, Msg::LayoutPresetRecordsFirstCompact),
            Self::VisitPrepFocus => t(locale, Msg::LayoutPresetVisitPrepFocus),
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::CalmCompanion => {
                "Balanced layout starting on Your Health with plain-language Q&A right under your summary."
            }
            Self::RecordsFirstCompact => {
                "Compact density putting your health records directly after your summary above Q&A."
            }
            Self::VisitPrepFocus => {
                "Spacious layout opening straight to For My Doctor so you can review what you'll share."
            }
        }
    }

    pub fn build(self) -> PatientLayoutPrefs {
        match self {
            Self::CalmCompanion => PatientLayoutPrefs {
                preset: Some(Self::CalmCompanion),
                density: LayoutDensity::Balanced,
                order: PatientSectionOrder::AskFirst,
                start_tab: PatientStartTab::You,
                show_ask_section: true,
                show_private_notes: true,
            },
            Self::RecordsFirstCompact => PatientLayoutPrefs {
                preset: Some(Self::RecordsFirstCompact),
                density: LayoutDensity::Compact,
                order: PatientSectionOrder::RecordsFirst,
                start_tab: PatientStartTab::You,
                show_ask_section: true,
                show_private_notes: true,
            },
            Self::VisitPrepFocus => PatientLayoutPrefs {
                preset: Some(Self::VisitPrepFocus),
                density: LayoutDensity::Spacious,
                order: PatientSectionOrder::RecordsFirst,
                start_tab: PatientStartTab::Shared,
                show_ask_section: true,
                show_private_notes: true,
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PatientLayoutPrefs {
    pub preset: Option<PatientLayoutPreset>,
    pub density: LayoutDensity,
    pub order: PatientSectionOrder,
    pub start_tab: PatientStartTab,
    pub show_ask_section: bool,
    pub show_private_notes: bool,
}

impl Default for PatientLayoutPrefs {
    fn default() -> Self {
        PatientLayoutPreset::CalmCompanion.build()
    }
}

impl PatientLayoutPrefs {
    pub fn active_label(&self) -> &'static str {
        self.preset.map_or("Custom", PatientLayoutPreset::label)
    }

    pub fn active_label_for(&self, locale: Locale) -> &'static str {
        self.preset.map_or_else(
            || t(locale, Msg::LayoutCustomLabel),
            |p| p.label_for(locale),
        )
    }
}

// ============================================================================
// 3. Mental Health & Recovery Dashboard Layout (`/mental-health`)
// ============================================================================

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum MentalHealthTab {
    #[default]
    Overview,
    LogImport,
    AlertsAi,
    CareBrief,
    Recovery,
    All,
}

impl MentalHealthTab {
    pub const ALL: &'static [Self] = &[
        Self::Overview,
        Self::LogImport,
        Self::AlertsAi,
        Self::CareBrief,
        Self::Recovery,
        Self::All,
    ];

    pub const fn slug(self) -> &'static str {
        match self {
            Self::Overview => "overview",
            Self::LogImport => "log",
            Self::AlertsAi => "alerts",
            Self::CareBrief => "care",
            Self::Recovery => "recovery",
            Self::All => "all",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Overview => "Overview & Charts",
            Self::LogImport => "Log & Import",
            Self::AlertsAi => "Alerts & AI",
            Self::CareBrief => "Care & Brief",
            Self::Recovery => "Find Recovery Care",
            Self::All => "All Sections",
        }
    }

    pub const fn label_for(self, locale: Locale) -> &'static str {
        match self {
            Self::Overview => t(locale, Msg::MhTabOverview),
            Self::LogImport => t(locale, Msg::MhTabLogImport),
            Self::AlertsAi => t(locale, Msg::MhTabAlertsAi),
            Self::CareBrief => t(locale, Msg::MhTabCareBrief),
            Self::Recovery => t(locale, Msg::MhTabRecovery),
            Self::All => t(locale, Msg::MhTabAll),
        }
    }

    pub fn from_query_search(search: &str) -> Option<Self> {
        for pair in search.trim_start_matches('?').split('&') {
            if let Some(val) = pair.strip_prefix("tab=") {
                return match val.to_ascii_lowercase().as_str() {
                    "overview" | "charts" => Some(Self::Overview),
                    "log" | "import" => Some(Self::LogImport),
                    "alerts" | "ai" => Some(Self::AlertsAi),
                    "care" | "brief" | "support" => Some(Self::CareBrief),
                    "recovery" | "finder" | "care-finder" => Some(Self::Recovery),
                    "all" => Some(Self::All),
                    _ => None,
                };
            }
        }
        None
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum MentalHealthLayoutPreset {
    #[default]
    GuidedTabs,
    DailyCheckInAndRecovery,
    FullAnalyticsWall,
}

impl MentalHealthLayoutPreset {
    pub const ALL: &'static [Self] = &[
        Self::GuidedTabs,
        Self::DailyCheckInAndRecovery,
        Self::FullAnalyticsWall,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::GuidedTabs => "Guided Tabs",
            Self::DailyCheckInAndRecovery => "Daily Check-In & Care",
            Self::FullAnalyticsWall => "Full Analytics Wall",
        }
    }

    pub const fn label_for(self, locale: Locale) -> &'static str {
        match self {
            Self::GuidedTabs => t(locale, Msg::LayoutPresetGuidedTabs),
            Self::DailyCheckInAndRecovery => t(locale, Msg::LayoutPresetDailyCheckIn),
            Self::FullAnalyticsWall => t(locale, Msg::LayoutPresetFullAnalyticsWall),
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::GuidedTabs => {
                "Clean tabbed views so charts, check-ins, alerts, crisis brief, and recovery search each stay focused."
            }
            Self::DailyCheckInAndRecovery => {
                "Compact layout opening directly to Check-In & CGM Import with quick access to Recovery Search."
            }
            Self::FullAnalyticsWall => {
                "Shows every chart, log, alert, brief, and recovery finder together on one scrollable page."
            }
        }
    }

    pub fn build(self) -> MentalHealthLayoutPrefs {
        match self {
            Self::GuidedTabs => MentalHealthLayoutPrefs {
                preset: Some(Self::GuidedTabs),
                default_tab: MentalHealthTab::Overview,
                density: LayoutDensity::Balanced,
                show_kpi_strip: true,
                show_alerts: true,
                show_ai_chat: true,
                show_recovery_search: true,
                show_support_brief: true,
            },
            Self::DailyCheckInAndRecovery => MentalHealthLayoutPrefs {
                preset: Some(Self::DailyCheckInAndRecovery),
                default_tab: MentalHealthTab::LogImport,
                density: LayoutDensity::Compact,
                show_kpi_strip: true,
                show_alerts: true,
                show_ai_chat: true,
                show_recovery_search: true,
                show_support_brief: true,
            },
            Self::FullAnalyticsWall => MentalHealthLayoutPrefs {
                preset: Some(Self::FullAnalyticsWall),
                default_tab: MentalHealthTab::All,
                density: LayoutDensity::Spacious,
                show_kpi_strip: true,
                show_alerts: true,
                show_ai_chat: true,
                show_recovery_search: true,
                show_support_brief: true,
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MentalHealthLayoutPrefs {
    pub preset: Option<MentalHealthLayoutPreset>,
    pub default_tab: MentalHealthTab,
    pub density: LayoutDensity,
    pub show_kpi_strip: bool,
    pub show_alerts: bool,
    pub show_ai_chat: bool,
    pub show_recovery_search: bool,
    pub show_support_brief: bool,
}

impl Default for MentalHealthLayoutPrefs {
    fn default() -> Self {
        MentalHealthLayoutPreset::GuidedTabs.build()
    }
}

impl MentalHealthLayoutPrefs {
    pub fn active_label(&self) -> &'static str {
        self.preset
            .map_or("Custom", MentalHealthLayoutPreset::label)
    }

    pub fn active_label_for(&self, locale: Locale) -> &'static str {
        self.preset.map_or_else(
            || t(locale, Msg::LayoutCustomLabel),
            |p| p.label_for(locale),
        )
    }
}

// ============================================================================
// LocalStorage helpers (client-only; safe no-ops on SSR)
// ============================================================================

#[cfg(any(feature = "hydrate", feature = "csr"))]
pub fn load_layout_prefs<T: serde::de::DeserializeOwned>(key: &str) -> Option<T> {
    let storage = web_sys::window()?.local_storage().ok()??;
    let raw = storage.get_item(key).ok()??;
    serde_json::from_str(&raw).ok()
}

#[cfg(any(feature = "hydrate", feature = "csr"))]
pub fn save_layout_prefs<T: Serialize>(key: &str, value: &T) {
    if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        if let Ok(raw) = serde_json::to_string(value) {
            let _ = storage.set_item(key, &raw);
        }
    }
}

#[cfg(any(feature = "hydrate", feature = "csr"))]
pub fn clear_layout_prefs(key: &str) {
    if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        let _ = storage.remove_item(key);
    }
}

// ============================================================================
// Reusable Collapsible Layout Picker UI Components
// ============================================================================

#[component]
pub fn PhysicianLayoutCustomizer(
    prefs: RwSignal<PhysicianLayoutPrefs>,
    open: RwSignal<bool>,
    saved_notice: RwSignal<Option<String>>,
) -> impl IntoView {
    let locale = use_locale();
    let persist = move |next: PhysicianLayoutPrefs, label: &str| {
        #[cfg(any(feature = "hydrate", feature = "csr"))]
        save_layout_prefs(STORAGE_KEY_PHYSICIAN_LAYOUT, &next);
        prefs.set(next);
        let loc = locale.get_untracked();
        saved_notice.set(Some(format!(
            "{}: {label}",
            t(loc, Msg::LayoutButtonPrefix)
        )));
    };

    view! {
        <Show when=move || open.get()>
            <section
                aria-label="Customize physician workspace layout"
                class="mb-6 rounded-2xl border border-cyan-400/30 bg-slate-900/90 p-5 space-y-4 shadow-xl"
            >
                <div class="flex flex-wrap items-start justify-between gap-3">
                    <div>
                        <p class="text-xs font-semibold uppercase tracking-widest text-cyan-300">
                            {move || {
                                let loc = locale.get();
                                format!(
                                    "{} · {}",
                                    t(loc, Msg::LayoutButtonPrefix),
                                    t(loc, Msg::LayoutSavedOnDevice)
                                )
                            }}
                        </p>
                        <p class="mt-0.5 text-xs text-slate-300">
                            "Pick a suggested layout preset or fine-tune sidebar placement, density, and visible chart sections."
                        </p>
                    </div>
                    <div class="flex items-center gap-2">
                        <button
                            type="button"
                            on:click=move |_| {
                                let default = PhysicianLayoutPrefs::default();
                                #[cfg(any(feature = "hydrate", feature = "csr"))]
                                clear_layout_prefs(STORAGE_KEY_PHYSICIAN_LAYOUT);
                                prefs.set(default);
                                let loc = locale.get_untracked();
                                saved_notice.set(Some(format!(
                                    "{}: {}",
                                    t(loc, Msg::LayoutReset),
                                    PhysicianLayoutPreset::FocusedTriage.label_for(loc)
                                )));
                            }
                            class="min-h-[34px] rounded-full border border-white/15 px-3 py-1 text-xs text-slate-300 transition hover:bg-white/10 hover:text-white"
                        >
                            {move || t(locale.get(), Msg::LayoutReset)}
                        </button>
                        <button
                            type="button"
                            on:click=move |_| open.set(false)
                            class="min-h-[34px] rounded-full border border-white/15 bg-white/[0.04] px-3 py-1 text-xs font-medium text-white transition hover:bg-white/10"
                        >
                            {move || t(locale.get(), Msg::LayoutDone)}
                        </button>
                    </div>
                </div>

                // Suggested presets
                <div class="grid gap-3 md:grid-cols-3">
                    {PhysicianLayoutPreset::ALL
                        .iter()
                        .copied()
                        .map(|preset| {
                            let is_selected = move || prefs.get().preset == Some(preset);
                            view! {
                                <button
                                    type="button"
                                    aria-pressed=move || is_selected().to_string()
                                    on:click=move |_| {
                                        let loc = locale.get_untracked();
                                        persist(preset.build(), preset.label_for(loc));
                                    }
                                    class=move || {
                                        if is_selected() {
                                            "rounded-xl border border-cyan-400/60 bg-cyan-500/15 p-3.5 text-start transition"
                                        } else {
                                            "rounded-xl border border-white/10 bg-slate-950/60 p-3.5 text-start transition hover:border-white/25"
                                        }
                                    }
                                >
                                    <div class="flex items-center justify-between gap-2">
                                        <span class="text-sm font-semibold text-white">
                                            {move || preset.label_for(locale.get())}
                                        </span>
                                        {move || is_selected().then(|| view! {
                                            <span class="rounded-full bg-cyan-400/20 px-2 py-0.5 text-[10px] font-semibold text-cyan-200">
                                                {move || t(locale.get(), Msg::LayoutActiveBadge)}
                                            </span>
                                        })}
                                    </div>
                                    <p class="mt-1 text-xs leading-relaxed text-slate-300">{preset.description()}</p>
                                </button>
                            }
                        })
                        .collect_view()}
                </div>

                // Fine-tuning row
                <div class="grid gap-4 border-t border-white/10 pt-4 md:grid-cols-3 text-xs">
                    <div class="space-y-2">
                        <p class="font-semibold uppercase tracking-wider text-slate-400">
                            {move || t(locale.get(), Msg::PhysicianPatientPanel)}
                        </p>
                        <div class="flex flex-wrap gap-1.5">
                            {SidebarPlacement::ALL
                                .iter()
                                .copied()
                                .map(|placement| {
                                    let active = move || prefs.get().sidebar == placement;
                                    view! {
                                        <button
                                            type="button"
                                            aria-pressed=move || active().to_string()
                                            on:click=move |_| {
                                                let mut next = prefs.get();
                                                next.preset = None;
                                                next.sidebar = placement;
                                                let loc = locale.get_untracked();
                                                persist(next, placement.label_for(loc));
                                            }
                                            class=move || {
                                                if active() {
                                                    "min-h-[34px] rounded-full border border-cyan-400/60 bg-cyan-500/20 px-3 py-1 font-semibold text-cyan-100"
                                                } else {
                                                    "min-h-[34px] rounded-full border border-white/10 bg-slate-950/60 px-3 py-1 text-slate-300 hover:text-white"
                                                }
                                            }
                                        >
                                            {move || placement.label_for(locale.get())}
                                        </button>
                                    }
                                })
                                .collect_view()}
                        </div>
                    </div>

                    <div class="space-y-2">
                        <p class="font-semibold uppercase tracking-wider text-slate-400">"Density & Order"</p>
                        <div class="flex flex-wrap gap-1.5">
                            {LayoutDensity::ALL
                                .iter()
                                .copied()
                                .map(|d| {
                                    let active = move || prefs.get().density == d;
                                    view! {
                                        <button
                                            type="button"
                                            aria-pressed=move || active().to_string()
                                            on:click=move |_| {
                                                let mut next = prefs.get();
                                                next.preset = None;
                                                next.density = d;
                                                let loc = locale.get_untracked();
                                                persist(next, d.label_for(loc));
                                            }
                                            class=move || {
                                                if active() {
                                                    "min-h-[34px] rounded-full border border-cyan-400/60 bg-cyan-500/20 px-3 py-1 font-semibold text-cyan-100"
                                                } else {
                                                    "min-h-[34px] rounded-full border border-white/10 bg-slate-950/60 px-3 py-1 text-slate-300 hover:text-white"
                                                }
                                            }
                                        >
                                            {move || d.label_for(locale.get())}
                                        </button>
                                    }
                                })
                                .collect_view()}
                            {[PhysicianFocusOrder::QaFirst, PhysicianFocusOrder::ClinicalFirst]
                                .into_iter()
                                .map(|ord| {
                                    let active = move || prefs.get().order == ord;
                                    view! {
                                        <button
                                            type="button"
                                            aria-pressed=move || active().to_string()
                                            on:click=move |_| {
                                                let mut next = prefs.get();
                                                next.preset = None;
                                                next.order = ord;
                                                let loc = locale.get_untracked();
                                                persist(next, ord.label_for(loc));
                                            }
                                            class=move || {
                                                if active() {
                                                    "min-h-[34px] rounded-full border border-emerald-400/60 bg-emerald-500/20 px-3 py-1 font-semibold text-emerald-100"
                                                } else {
                                                    "min-h-[34px] rounded-full border border-white/10 bg-slate-950/60 px-3 py-1 text-slate-300 hover:text-white"
                                                }
                                            }
                                        >
                                            {move || ord.label_for(locale.get())}
                                        </button>
                                    }
                                })
                                .collect_view()}
                        </div>
                    </div>

                    <div class="space-y-2">
                        <p class="font-semibold uppercase tracking-wider text-slate-400">"Visible Sections"</p>
                        <div class="flex flex-wrap gap-2">
                            <label class="inline-flex cursor-pointer items-center gap-1.5 rounded-lg border border-white/10 bg-slate-950/60 px-2.5 py-1.5 text-slate-200">
                                <input
                                    type="checkbox"
                                    prop:checked=move || prefs.get().show_qa
                                    on:change=move |ev| {
                                        let mut next = prefs.get();
                                        next.preset = None;
                                        next.show_qa = event_target_checked(&ev);
                                        let loc = locale.get_untracked();
                                        persist(next, t(loc, Msg::LayoutCustomLabel));
                                    }
                                />
                                {move || t(locale.get(), Msg::PhysicianJumpQa)}
                            </label>
                            <label class="inline-flex cursor-pointer items-center gap-1.5 rounded-lg border border-white/10 bg-slate-950/60 px-2.5 py-1.5 text-slate-200">
                                <input
                                    type="checkbox"
                                    prop:checked=move || prefs.get().show_clinical_grid
                                    on:change=move |ev| {
                                        let mut next = prefs.get();
                                        next.preset = None;
                                        next.show_clinical_grid = event_target_checked(&ev);
                                        let loc = locale.get_untracked();
                                        persist(next, t(loc, Msg::LayoutCustomLabel));
                                    }
                                />
                                {move || t(locale.get(), Msg::PhysicianJumpClinical)}
                            </label>
                            <label class="inline-flex cursor-pointer items-center gap-1.5 rounded-lg border border-white/10 bg-slate-950/60 px-2.5 py-1.5 text-slate-200">
                                <input
                                    type="checkbox"
                                    prop:checked=move || prefs.get().show_reply_draft
                                    on:change=move |ev| {
                                        let mut next = prefs.get();
                                        next.preset = None;
                                        next.show_reply_draft = event_target_checked(&ev);
                                        let loc = locale.get_untracked();
                                        persist(next, t(loc, Msg::LayoutCustomLabel));
                                    }
                                />
                                {move || t(locale.get(), Msg::PhysicianJumpReply)}
                            </label>
                            <label class="inline-flex cursor-pointer items-center gap-1.5 rounded-lg border border-white/10 bg-slate-950/60 px-2.5 py-1.5 text-slate-200">
                                <input
                                    type="checkbox"
                                    prop:checked=move || prefs.get().show_source_records
                                    on:change=move |ev| {
                                        let mut next = prefs.get();
                                        next.preset = None;
                                        next.show_source_records = event_target_checked(&ev);
                                        let loc = locale.get_untracked();
                                        persist(next, t(loc, Msg::LayoutCustomLabel));
                                    }
                                />
                                {move || t(locale.get(), Msg::PhysicianJumpRecords)}
                            </label>
                        </div>
                    </div>
                </div>

                {move || saved_notice.get().map(|msg| view! {
                    <p role="status" aria-live="polite" class="text-[11px] text-emerald-300">{msg}</p>
                })}
            </section>
        </Show>
    }
}

#[component]
pub fn PatientLayoutCustomizer(
    prefs: RwSignal<PatientLayoutPrefs>,
    open: RwSignal<bool>,
    saved_notice: RwSignal<Option<String>>,
    on_apply_start_tab: Callback<PatientStartTab>,
) -> impl IntoView {
    let locale = use_locale();
    let persist = move |next: PatientLayoutPrefs, label: &str, switch_tab: bool| {
        #[cfg(any(feature = "hydrate", feature = "csr"))]
        save_layout_prefs(STORAGE_KEY_PATIENT_LAYOUT, &next);
        let tab = next.start_tab;
        prefs.set(next);
        if switch_tab {
            on_apply_start_tab.run(tab);
        }
        let loc = locale.get_untracked();
        saved_notice.set(Some(format!(
            "{}: {label}",
            t(loc, Msg::LayoutButtonPrefix)
        )));
    };

    view! {
        <Show when=move || open.get()>
            <section
                aria-label="Customize patient workspace layout"
                class="mb-6 rounded-2xl border border-emerald-400/30 bg-slate-900/90 p-5 space-y-4 shadow-xl"
            >
                <div class="flex flex-wrap items-start justify-between gap-3">
                    <div>
                        <p class="text-xs font-semibold uppercase tracking-widest text-emerald-300">
                            {move || {
                                let loc = locale.get();
                                format!(
                                    "{} · {}",
                                    t(loc, Msg::LayoutButtonPrefix),
                                    t(loc, Msg::LayoutSavedOnDevice)
                                )
                            }}
                        </p>
                        <p class="mt-0.5 text-xs text-slate-300">
                            "Choose a suggested layout or customize section order, density, and your default tab."
                        </p>
                    </div>
                    <div class="flex items-center gap-2">
                        <button
                            type="button"
                            on:click=move |_| {
                                let default = PatientLayoutPrefs::default();
                                #[cfg(any(feature = "hydrate", feature = "csr"))]
                                clear_layout_prefs(STORAGE_KEY_PATIENT_LAYOUT);
                                prefs.set(default);
                                on_apply_start_tab.run(PatientStartTab::You);
                                let loc = locale.get_untracked();
                                saved_notice.set(Some(format!(
                                    "{}: {}",
                                    t(loc, Msg::LayoutReset),
                                    PatientLayoutPreset::CalmCompanion.label_for(loc)
                                )));
                            }
                            class="min-h-[34px] rounded-full border border-white/15 px-3 py-1 text-xs text-slate-300 transition hover:bg-white/10 hover:text-white"
                        >
                            {move || t(locale.get(), Msg::LayoutReset)}
                        </button>
                        <button
                            type="button"
                            on:click=move |_| open.set(false)
                            class="min-h-[34px] rounded-full border border-white/15 bg-white/[0.04] px-3 py-1 text-xs font-medium text-white transition hover:bg-white/10"
                        >
                            {move || t(locale.get(), Msg::LayoutDone)}
                        </button>
                    </div>
                </div>

                <div class="grid gap-3 md:grid-cols-3">
                    {PatientLayoutPreset::ALL
                        .iter()
                        .copied()
                        .map(|preset| {
                            let is_selected = move || prefs.get().preset == Some(preset);
                            view! {
                                <button
                                    type="button"
                                    aria-pressed=move || is_selected().to_string()
                                    on:click=move |_| {
                                        let loc = locale.get_untracked();
                                        persist(preset.build(), preset.label_for(loc), true);
                                    }
                                    class=move || {
                                        if is_selected() {
                                            "rounded-xl border border-emerald-400/60 bg-emerald-500/15 p-3.5 text-start transition"
                                        } else {
                                            "rounded-xl border border-white/10 bg-slate-950/60 p-3.5 text-start transition hover:border-white/25"
                                        }
                                    }
                                >
                                    <div class="flex items-center justify-between gap-2">
                                        <span class="text-sm font-semibold text-white">
                                            {move || preset.label_for(locale.get())}
                                        </span>
                                        {move || is_selected().then(|| view! {
                                            <span class="rounded-full bg-emerald-400/20 px-2 py-0.5 text-[10px] font-semibold text-emerald-200">
                                                {move || t(locale.get(), Msg::LayoutActiveBadge)}
                                            </span>
                                        })}
                                    </div>
                                    <p class="mt-1 text-xs leading-relaxed text-slate-300">{preset.description()}</p>
                                </button>
                            }
                        })
                        .collect_view()}
                </div>

                <div class="grid gap-4 border-t border-white/10 pt-4 md:grid-cols-3 text-xs">
                    <div class="space-y-2">
                        <p class="font-semibold uppercase tracking-wider text-slate-400">"Density"</p>
                        <div class="flex flex-wrap gap-1.5">
                            {LayoutDensity::ALL
                                .iter()
                                .copied()
                                .map(|d| {
                                    let active = move || prefs.get().density == d;
                                    view! {
                                        <button
                                            type="button"
                                            aria-pressed=move || active().to_string()
                                            on:click=move |_| {
                                                let mut next = prefs.get();
                                                next.preset = None;
                                                next.density = d;
                                                let loc = locale.get_untracked();
                                                persist(next, d.label_for(loc), false);
                                            }
                                            class=move || {
                                                if active() {
                                                    "min-h-[34px] rounded-full border border-emerald-400/60 bg-emerald-500/20 px-3 py-1 font-semibold text-emerald-100"
                                                } else {
                                                    "min-h-[34px] rounded-full border border-white/10 bg-slate-950/60 px-3 py-1 text-slate-300 hover:text-white"
                                                }
                                            }
                                        >
                                            {move || d.label_for(locale.get())}
                                        </button>
                                    }
                                })
                                .collect_view()}
                        </div>
                    </div>

                    <div class="space-y-2">
                        <p class="font-semibold uppercase tracking-wider text-slate-400">"Section Order"</p>
                        <div class="flex flex-wrap gap-1.5">
                            {[PatientSectionOrder::AskFirst, PatientSectionOrder::RecordsFirst]
                                .into_iter()
                                .map(|ord| {
                                    let active = move || prefs.get().order == ord;
                                    view! {
                                        <button
                                            type="button"
                                            aria-pressed=move || active().to_string()
                                            on:click=move |_| {
                                                let mut next = prefs.get();
                                                next.preset = None;
                                                next.order = ord;
                                                let loc = locale.get_untracked();
                                                persist(next, ord.label_for(loc), false);
                                            }
                                            class=move || {
                                                if active() {
                                                    "min-h-[34px] rounded-full border border-cyan-400/60 bg-cyan-500/20 px-3 py-1 font-semibold text-cyan-100"
                                                } else {
                                                    "min-h-[34px] rounded-full border border-white/10 bg-slate-950/60 px-3 py-1 text-slate-300 hover:text-white"
                                                }
                                            }
                                        >
                                            {move || ord.label_for(locale.get())}
                                        </button>
                                    }
                                })
                                .collect_view()}
                        </div>
                    </div>

                    <div class="space-y-2">
                        <p class="font-semibold uppercase tracking-wider text-slate-400">"Visible Sections"</p>
                        <div class="flex flex-wrap gap-2">
                            <label class="inline-flex cursor-pointer items-center gap-1.5 rounded-lg border border-white/10 bg-slate-950/60 px-2.5 py-1.5 text-slate-200">
                                <input
                                    type="checkbox"
                                    prop:checked=move || prefs.get().show_ask_section
                                    on:change=move |ev| {
                                        let mut next = prefs.get();
                                        next.preset = None;
                                        next.show_ask_section = event_target_checked(&ev);
                                        let loc = locale.get_untracked();
                                        persist(next, t(loc, Msg::LayoutCustomLabel), false);
                                    }
                                />
                                {move || t(locale.get(), Msg::PatientJumpAsk)}
                            </label>
                            <label class="inline-flex cursor-pointer items-center gap-1.5 rounded-lg border border-white/10 bg-slate-950/60 px-2.5 py-1.5 text-slate-200">
                                <input
                                    type="checkbox"
                                    prop:checked=move || prefs.get().show_private_notes
                                    on:change=move |ev| {
                                        let mut next = prefs.get();
                                        next.preset = None;
                                        next.show_private_notes = event_target_checked(&ev);
                                        let loc = locale.get_untracked();
                                        persist(next, t(loc, Msg::LayoutCustomLabel), false);
                                    }
                                />
                                {move || t(locale.get(), Msg::PatientJumpNotes)}
                            </label>
                        </div>
                    </div>
                </div>

                {move || saved_notice.get().map(|msg| view! {
                    <p role="status" aria-live="polite" class="text-[11px] text-emerald-300">{msg}</p>
                })}
            </section>
        </Show>
    }
}

#[component]
pub fn MentalHealthLayoutCustomizer(
    prefs: RwSignal<MentalHealthLayoutPrefs>,
    active_tab: RwSignal<MentalHealthTab>,
    open: RwSignal<bool>,
    saved_notice: RwSignal<Option<String>>,
) -> impl IntoView {
    let locale = use_locale();
    let persist = move |next: MentalHealthLayoutPrefs, label: &str, apply_tab: bool| {
        #[cfg(any(feature = "hydrate", feature = "csr"))]
        save_layout_prefs(STORAGE_KEY_MENTAL_HEALTH_LAYOUT, &next);
        let tab = next.default_tab;
        prefs.set(next);
        if apply_tab {
            active_tab.set(tab);
        }
        let loc = locale.get_untracked();
        saved_notice.set(Some(format!(
            "{}: {label}",
            t(loc, Msg::LayoutButtonPrefix)
        )));
    };

    view! {
        <Show when=move || open.get()>
            <section
                aria-label="Customize mental health dashboard layout"
                class="print-hide print:hidden mb-6 rounded-2xl border border-cyan-400/30 bg-slate-900/90 p-5 space-y-4 shadow-xl"
            >
                <div class="flex flex-wrap items-start justify-between gap-3">
                    <div>
                        <p class="text-xs font-semibold uppercase tracking-widest text-cyan-300">
                            {move || {
                                let loc = locale.get();
                                format!(
                                    "{} · {}",
                                    t(loc, Msg::LayoutButtonPrefix),
                                    t(loc, Msg::LayoutSavedOnDevice)
                                )
                            }}
                        </p>
                        <p class="mt-0.5 text-xs text-slate-300">
                            "Pick a suggested layout or choose your starting view, density, and which panels appear."
                        </p>
                    </div>
                    <div class="flex items-center gap-2">
                        <button
                            type="button"
                            on:click=move |_| {
                                let default = MentalHealthLayoutPrefs::default();
                                #[cfg(any(feature = "hydrate", feature = "csr"))]
                                clear_layout_prefs(STORAGE_KEY_MENTAL_HEALTH_LAYOUT);
                                prefs.set(default);
                                active_tab.set(MentalHealthTab::Overview);
                                let loc = locale.get_untracked();
                                saved_notice.set(Some(format!(
                                    "{}: {}",
                                    t(loc, Msg::LayoutReset),
                                    MentalHealthLayoutPreset::GuidedTabs.label_for(loc)
                                )));
                            }
                            class="min-h-[34px] rounded-full border border-white/15 px-3 py-1 text-xs text-slate-300 transition hover:bg-white/10 hover:text-white"
                        >
                            {move || t(locale.get(), Msg::LayoutReset)}
                        </button>
                        <button
                            type="button"
                            on:click=move |_| open.set(false)
                            class="min-h-[34px] rounded-full border border-white/15 bg-white/[0.04] px-3 py-1 text-xs font-medium text-white transition hover:bg-white/10"
                        >
                            {move || t(locale.get(), Msg::LayoutDone)}
                        </button>
                    </div>
                </div>

                <div class="grid gap-3 md:grid-cols-3">
                    {MentalHealthLayoutPreset::ALL
                        .iter()
                        .copied()
                        .map(|preset| {
                            let is_selected = move || prefs.get().preset == Some(preset);
                            view! {
                                <button
                                    type="button"
                                    aria-pressed=move || is_selected().to_string()
                                    on:click=move |_| {
                                        let loc = locale.get_untracked();
                                        persist(preset.build(), preset.label_for(loc), true);
                                    }
                                    class=move || {
                                        if is_selected() {
                                            "rounded-xl border border-cyan-400/60 bg-cyan-500/15 p-3.5 text-start transition"
                                        } else {
                                            "rounded-xl border border-white/10 bg-slate-950/60 p-3.5 text-start transition hover:border-white/25"
                                        }
                                    }
                                >
                                    <div class="flex items-center justify-between gap-2">
                                        <span class="text-sm font-semibold text-white">
                                            {move || preset.label_for(locale.get())}
                                        </span>
                                        {move || is_selected().then(|| view! {
                                            <span class="rounded-full bg-cyan-400/20 px-2 py-0.5 text-[10px] font-semibold text-cyan-200">
                                                {move || t(locale.get(), Msg::LayoutActiveBadge)}
                                            </span>
                                        })}
                                    </div>
                                    <p class="mt-1 text-xs leading-relaxed text-slate-300">{preset.description()}</p>
                                </button>
                            }
                        })
                        .collect_view()}
                </div>

                <div class="grid gap-4 border-t border-white/10 pt-4 md:grid-cols-3 text-xs">
                    <div class="space-y-2">
                        <p class="font-semibold uppercase tracking-wider text-slate-400">"Starting Tab"</p>
                        <select
                            aria-label="Starting dashboard tab"
                            prop:value=move || prefs.get().default_tab.slug()
                            on:change=move |ev| {
                                let v = event_target_value(&ev);
                                if let Some(tab) = MentalHealthTab::ALL.iter().copied().find(|t| t.slug() == v) {
                                    let mut next = prefs.get();
                                    next.preset = None;
                                    next.default_tab = tab;
                                    let loc = locale.get_untracked();
                                    persist(next, tab.label_for(loc), true);
                                }
                            }
                            class="min-h-[38px] w-full rounded-xl border border-white/15 bg-slate-950 px-3 py-1.5 text-xs text-white focus:border-cyan-400 focus:outline-none"
                        >
                            {MentalHealthTab::ALL
                                .iter()
                                .copied()
                                .map(|tab| {
                                    view! {
                                        <option value=tab.slug()>
                                            {move || tab.label_for(locale.get())}
                                        </option>
                                    }
                                })
                                .collect_view()}
                        </select>
                    </div>

                    <div class="space-y-2">
                        <p class="font-semibold uppercase tracking-wider text-slate-400">"Density"</p>
                        <div class="flex flex-wrap gap-1.5">
                            {LayoutDensity::ALL
                                .iter()
                                .copied()
                                .map(|d| {
                                    let active = move || prefs.get().density == d;
                                    view! {
                                        <button
                                            type="button"
                                            aria-pressed=move || active().to_string()
                                            on:click=move |_| {
                                                let mut next = prefs.get();
                                                next.preset = None;
                                                next.density = d;
                                                let loc = locale.get_untracked();
                                                persist(next, d.label_for(loc), false);
                                            }
                                            class=move || {
                                                if active() {
                                                    "min-h-[34px] rounded-full border border-cyan-400/60 bg-cyan-500/20 px-3 py-1 font-semibold text-cyan-100"
                                                } else {
                                                    "min-h-[34px] rounded-full border border-white/10 bg-slate-950/60 px-3 py-1 text-slate-300 hover:text-white"
                                                }
                                            }
                                        >
                                            {move || d.label_for(locale.get())}
                                        </button>
                                    }
                                })
                                .collect_view()}
                        </div>
                    </div>

                    <div class="space-y-2">
                        <p class="font-semibold uppercase tracking-wider text-slate-400">"Visible Modules"</p>
                        <div class="flex flex-wrap gap-1.5">
                            <label class="inline-flex cursor-pointer items-center gap-1.5 rounded-lg border border-white/10 bg-slate-950/60 px-2.5 py-1.5 text-slate-200">
                                <input
                                    type="checkbox"
                                    prop:checked=move || prefs.get().show_kpi_strip
                                    on:change=move |ev| {
                                        let mut next = prefs.get();
                                        next.preset = None;
                                        next.show_kpi_strip = event_target_checked(&ev);
                                        let loc = locale.get_untracked();
                                        persist(next, t(loc, Msg::LayoutCustomLabel), false);
                                    }
                                />
                                "KPI Strip"
                            </label>
                            <label class="inline-flex cursor-pointer items-center gap-1.5 rounded-lg border border-white/10 bg-slate-950/60 px-2.5 py-1.5 text-slate-200">
                                <input
                                    type="checkbox"
                                    prop:checked=move || prefs.get().show_alerts
                                    on:change=move |ev| {
                                        let mut next = prefs.get();
                                        next.preset = None;
                                        next.show_alerts = event_target_checked(&ev);
                                        let loc = locale.get_untracked();
                                        persist(next, t(loc, Msg::LayoutCustomLabel), false);
                                    }
                                />
                                "Alerts"
                            </label>
                            <label class="inline-flex cursor-pointer items-center gap-1.5 rounded-lg border border-white/10 bg-slate-950/60 px-2.5 py-1.5 text-slate-200">
                                <input
                                    type="checkbox"
                                    prop:checked=move || prefs.get().show_ai_chat
                                    on:change=move |ev| {
                                        let mut next = prefs.get();
                                        next.preset = None;
                                        next.show_ai_chat = event_target_checked(&ev);
                                        let loc = locale.get_untracked();
                                        persist(next, t(loc, Msg::LayoutCustomLabel), false);
                                    }
                                />
                                "AI Chat"
                            </label>
                            <label class="inline-flex cursor-pointer items-center gap-1.5 rounded-lg border border-white/10 bg-slate-950/60 px-2.5 py-1.5 text-slate-200">
                                <input
                                    type="checkbox"
                                    prop:checked=move || prefs.get().show_recovery_search
                                    on:change=move |ev| {
                                        let mut next = prefs.get();
                                        next.preset = None;
                                        next.show_recovery_search = event_target_checked(&ev);
                                        let loc = locale.get_untracked();
                                        persist(next, t(loc, Msg::LayoutCustomLabel), false);
                                    }
                                />
                                {move || t(locale.get(), Msg::MhTabRecovery)}
                            </label>
                            <label class="inline-flex cursor-pointer items-center gap-1.5 rounded-lg border border-white/10 bg-slate-950/60 px-2.5 py-1.5 text-slate-200">
                                <input
                                    type="checkbox"
                                    prop:checked=move || prefs.get().show_support_brief
                                    on:change=move |ev| {
                                        let mut next = prefs.get();
                                        next.preset = None;
                                        next.show_support_brief = event_target_checked(&ev);
                                        let loc = locale.get_untracked();
                                        persist(next, t(loc, Msg::LayoutCustomLabel), false);
                                    }
                                />
                                {move || t(locale.get(), Msg::MhTabCareBrief)}
                            </label>
                        </div>
                    </div>
                </div>

                {move || saved_notice.get().map(|msg| view! {
                    <p role="status" aria-live="polite" class="text-[11px] text-emerald-300">{msg}</p>
                })}
            </section>
        </Show>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn physician_presets_roundtrip_json_cleanly() {
        for preset in PhysicianLayoutPreset::ALL {
            let built = preset.build();
            assert_eq!(built.preset, Some(*preset));
            let json = serde_json::to_string(&built).expect("serialize physician prefs");
            let parsed: PhysicianLayoutPrefs =
                serde_json::from_str(&json).expect("deserialize physician prefs");
            assert_eq!(parsed, built);
        }
    }

    #[test]
    fn patient_and_mental_health_presets_roundtrip_and_parse_query_tabs() {
        for preset in PatientLayoutPreset::ALL {
            let built = preset.build();
            let json = serde_json::to_string(&built).expect("serialize patient prefs");
            let parsed: PatientLayoutPrefs =
                serde_json::from_str(&json).expect("deserialize patient prefs");
            assert_eq!(parsed, built);
        }
        for preset in MentalHealthLayoutPreset::ALL {
            let built = preset.build();
            let json = serde_json::to_string(&built).expect("serialize mh prefs");
            let parsed: MentalHealthLayoutPrefs =
                serde_json::from_str(&json).expect("deserialize mh prefs");
            assert_eq!(parsed, built);
        }
        assert_eq!(
            MentalHealthTab::from_query_search("?tab=recovery"),
            Some(MentalHealthTab::Recovery)
        );
        assert_eq!(
            MentalHealthTab::from_query_search("?foo=1&tab=log"),
            Some(MentalHealthTab::LogImport)
        );
        assert_eq!(MentalHealthTab::from_query_search(""), None);
    }

    #[test]
    fn localized_labels_match_english_defaults_and_translate_for_non_english() {
        for d in LayoutDensity::ALL {
            assert_eq!(d.label_for(Locale::En), d.label());
            assert_ne!(d.label_for(Locale::Ar), d.label());
        }
        for s in SidebarPlacement::ALL {
            assert_eq!(s.label_for(Locale::En), s.label());
            assert_ne!(s.label_for(Locale::Es), s.label());
        }
        for p in PhysicianLayoutPreset::ALL {
            assert_eq!(p.label_for(Locale::En), p.label());
        }
        for p in PatientLayoutPreset::ALL {
            assert_eq!(p.label_for(Locale::En), p.label());
        }
        for p in MentalHealthLayoutPreset::ALL {
            assert_eq!(p.label_for(Locale::En), p.label());
        }
        let custom = PhysicianLayoutPrefs {
            preset: None,
            ..PhysicianLayoutPrefs::default()
        };
        assert_eq!(custom.active_label(), "Custom");
        assert_eq!(custom.active_label_for(Locale::En), "Custom");
        assert_eq!(
            custom.active_label_for(Locale::Ar),
            t(Locale::Ar, Msg::LayoutCustomLabel)
        );
    }
}
