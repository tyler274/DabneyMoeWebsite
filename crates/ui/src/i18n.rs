//! Zero-dependency, compile-time exhaustive 9-language internationalization (i18n)
//! and RTL support for Dabney.moe (Leptos 0.8: `ssr` + `hydrate` + `csr`).
//!
//! Supported locales:
//! - English (`en`, LTR, default)
//! - Spanish (`es`, LTR)
//! - French (`fr`, LTR)
//! - Portuguese (`pt`, LTR)
//! - German (`de`, LTR)
//! - Simplified Chinese (`zh-Hans`, LTR)
//! - Japanese (`ja`, LTR)
//! - Korean (`ko`, LTR)
//! - Arabic (`ar`, RTL)
//!
//! Locale resolution on the client follows strict priority order:
//! 1. URL query parameter (`?lang=...`)
//! 2. Persisted user preference in `localStorage` ([`STORAGE_KEY_LOCALE`])
//! 3. Browser `navigator.languages` / `navigator.language`
//! 4. Fallback to [`Locale::En`]

use leptos::prelude::*;

/// `localStorage` key storing the user's chosen BCP-47 locale tag.
pub const STORAGE_KEY_LOCALE: &str = "dabney.locale.v1";

/// Writing direction for a [`Locale`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TextDirection {
    /// Left-to-right script.
    #[default]
    Ltr,
    /// Right-to-left script (e.g., Arabic).
    Rtl,
}

impl TextDirection {
    /// HTML `dir` attribute value (`"ltr"` or `"rtl"`).
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ltr => "ltr",
            Self::Rtl => "rtl",
        }
    }

    /// Returns `true` for right-to-left locales.
    pub const fn is_rtl(self) -> bool {
        matches!(self, Self::Rtl)
    }
}

/// Supported UI locales across Dabney.moe and its clinical demo suite.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Locale {
    #[default]
    En,
    Es,
    Fr,
    Pt,
    De,
    ZhHans,
    Ja,
    Ko,
    Ar,
}

impl Locale {
    /// All 9 supported locales in display order.
    pub const ALL: &'static [Locale] = &[
        Locale::En,
        Locale::Es,
        Locale::Fr,
        Locale::Pt,
        Locale::De,
        Locale::ZhHans,
        Locale::Ja,
        Locale::Ko,
        Locale::Ar,
    ];

    /// Canonical BCP-47 language tag.
    pub const fn bcp47(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::Es => "es",
            Self::Fr => "fr",
            Self::Pt => "pt",
            Self::De => "de",
            Self::ZhHans => "zh-Hans",
            Self::Ja => "ja",
            Self::Ko => "ko",
            Self::Ar => "ar",
        }
    }

    /// Native autonym displayed in the language switcher.
    pub const fn native_name(self) -> &'static str {
        match self {
            Self::En => "English",
            Self::Es => "Español",
            Self::Fr => "Français",
            Self::Pt => "Português",
            Self::De => "Deutsch",
            Self::ZhHans => "简体中文",
            Self::Ja => "日本語",
            Self::Ko => "한국어",
            Self::Ar => "العربية",
        }
    }

    /// Compact language code or script name for tight chrome bars.
    pub const fn short_code(self) -> &'static str {
        match self {
            Self::En => "EN",
            Self::Es => "ES",
            Self::Fr => "FR",
            Self::Pt => "PT",
            Self::De => "DE",
            Self::ZhHans => "中文",
            Self::Ja => "日本語",
            Self::Ko => "한국어",
            Self::Ar => "العربية",
        }
    }

    /// Text direction (`rtl` for Arabic, `ltr` for all other supported locales).
    pub const fn dir(self) -> TextDirection {
        match self {
            Self::Ar => TextDirection::Rtl,
            Self::En
            | Self::Es
            | Self::Fr
            | Self::Pt
            | Self::De
            | Self::ZhHans
            | Self::Ja
            | Self::Ko => TextDirection::Ltr,
        }
    }

    /// Case-insensitive BCP-47 primary-subtag matching (`"es-MX"`, `"zh-CN"`,
    /// `"zh-SG"`, `"zh-Hans"`, `"zh"`, `"pt-BR"`, `"fr-CA"`, `"de-AT"`,
    /// `"ja-JP"`, `"ko-KR"`, `"ar-EG"`, `"en-GB"`, etc.).
    pub fn parse_tag(raw: &str) -> Option<Locale> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }
        let normalized = trimmed.replace('_', "-").to_ascii_lowercase();
        let primary = normalized.split('-').next()?;
        match primary {
            "en" => Some(Self::En),
            "es" => Some(Self::Es),
            "fr" => Some(Self::Fr),
            "pt" => Some(Self::Pt),
            "de" => Some(Self::De),
            "zh" => Some(Self::ZhHans),
            "ja" => Some(Self::Ja),
            "ko" => Some(Self::Ko),
            "ar" => Some(Self::Ar),
            _ => None,
        }
    }

    /// Parses `?lang=...` or `&lang=...` from a URL query string.
    pub fn from_query_search(search: &str) -> Option<Locale> {
        let without_hash = search.split('#').next().unwrap_or(search);
        let query = without_hash.trim_start_matches('?');
        for pair in query.split('&') {
            let Some((key, value)) = pair.split_once('=') else {
                continue;
            };
            if key.eq_ignore_ascii_case("lang") {
                return Self::parse_tag(value);
            }
        }
        None
    }

    /// Selects the first supported locale from a browser language preference list,
    /// falling back to [`Locale::En`].
    pub fn from_browser_languages(langs: &[String]) -> Locale {
        langs
            .iter()
            .find_map(|tag| Self::parse_tag(tag))
            .unwrap_or(Self::En)
    }
}

/// Provides the root reactive [`Locale`] context.
///
/// On SSR, initializes cleanly to [`Locale::En`] to keep server-rendered HTML
/// deterministic and hydration-safe. On browser targets (`hydrate` / `csr`),
/// resolves the initial locale in priority order (`?lang=` > `localStorage` >
/// `navigator.languages` / `navigator.language` > `Locale::En`), syncs
/// `<html lang dir>`, and persists user-initiated locale changes to
/// `localStorage`.
pub fn provide_i18n_context() -> RwSignal<Locale> {
    let locale = RwSignal::new(Locale::En);
    provide_context(locale);

    #[cfg(any(feature = "hydrate", feature = "csr"))]
    {
        let initialized = StoredValue::new(false);
        let last_synced = StoredValue::new(Locale::En);

        // Mount effect: resolve initial locale once after hydration without
        // overwriting localStorage unless the user changes language explicitly.
        Effect::new(move |_| {
            if initialized.get_value() {
                return;
            }
            initialized.set_value(true);
            let resolved = resolve_initial_browser_locale();
            last_synced.set_value(resolved);
            apply_document_locale(resolved);
            if locale.get_untracked() != resolved {
                locale.set(resolved);
            }
        });

        // Reactive effect: keep `<html lang dir>` in sync and persist user updates.
        Effect::new(move |_| {
            let current = locale.get();
            apply_document_locale(current);
            if initialized.get_value() && last_synced.get_value() != current {
                last_synced.set_value(current);
                write_stored_locale(current);
            }
        });
    }

    locale
}

/// Reads the active [`Locale`] signal from Leptos context, falling back to an
/// isolated [`Locale::En`] signal so standalone SSR unit tests never panic.
pub fn use_locale() -> RwSignal<Locale> {
    use_context::<RwSignal<Locale>>().unwrap_or_else(|| RwSignal::new(Locale::En))
}

/// Updates the active [`Locale`] from an explicit user choice, immediately
/// persisting to `localStorage` and syncing `<html lang dir>` on client builds.
pub fn set_user_locale(signal: RwSignal<Locale>, next: Locale) {
    signal.set(next);
    #[cfg(any(feature = "hydrate", feature = "csr"))]
    {
        write_stored_locale(next);
        apply_document_locale(next);
    }
}

/// Returns a reactive, `Copy + Send + Sync` closure translating `key` for the
/// current [`Locale`] context.
pub fn tr(key: Msg) -> impl Fn() -> &'static str + Copy + Send + Sync {
    let locale = use_locale();
    move || t(locale.get(), key)
}

/// Convenience helper that reads the current [`Locale`] context and immediately
/// returns the translated string for `key`.
pub fn t_now(key: Msg) -> &'static str {
    t(use_locale().get(), key)
}

#[cfg(any(feature = "hydrate", feature = "csr"))]
fn resolve_initial_browser_locale() -> Locale {
    if let Some(loc) = read_query_locale() {
        return loc;
    }
    if let Some(loc) = read_stored_locale() {
        return loc;
    }
    let browser_langs = read_browser_languages();
    Locale::from_browser_languages(&browser_langs)
}

#[cfg(any(feature = "hydrate", feature = "csr"))]
fn read_query_locale() -> Option<Locale> {
    let search = web_sys::window()?.location().search().ok()?;
    Locale::from_query_search(&search)
}

#[cfg(any(feature = "hydrate", feature = "csr"))]
fn read_stored_locale() -> Option<Locale> {
    let storage = web_sys::window()?.local_storage().ok()??;
    let raw = storage.get_item(STORAGE_KEY_LOCALE).ok()??;
    Locale::parse_tag(&raw)
}

#[cfg(any(feature = "hydrate", feature = "csr"))]
fn write_stored_locale(locale: Locale) {
    if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        let _ = storage.set_item(STORAGE_KEY_LOCALE, locale.bcp47());
    }
}

#[cfg(any(feature = "hydrate", feature = "csr"))]
fn read_browser_languages() -> Vec<String> {
    let Some(window) = web_sys::window() else {
        return Vec::new();
    };
    let navigator = window.navigator();
    let mut langs: Vec<String> = navigator
        .languages()
        .iter()
        .filter_map(|val| val.as_string())
        .collect();
    if langs.is_empty() {
        langs.extend(navigator.language());
    }
    langs
}

#[cfg(any(feature = "hydrate", feature = "csr"))]
fn apply_document_locale(locale: Locale) {
    if let Some(doc_el) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.document_element())
    {
        let _ = doc_el.set_attribute("lang", locale.bcp47());
        let _ = doc_el.set_attribute("dir", locale.dir().as_str());
    }
}

/// Accessible native `<label>` + `<select>` language switcher showing all 9
/// locales in their native autonyms with per-option `lang` and `dir` attributes.
#[component]
pub fn LanguageSwitcher(#[prop(optional)] compact: bool) -> impl IntoView {
    let locale = use_locale();

    let wrapper_class = if compact {
        "inline-flex items-center gap-1.5 rounded-full border border-white/15 bg-slate-900/90 px-2.5 py-1 text-xs font-medium text-slate-200 transition hover:border-white/30 focus-within:border-cyan-400"
    } else {
        "inline-flex items-center gap-2.5 text-sm text-slate-200"
    };

    let select_class = if compact {
        "cursor-pointer bg-transparent pe-1 text-xs font-medium text-slate-100 focus:outline-none"
    } else {
        "min-h-[38px] cursor-pointer rounded-xl border border-white/15 bg-slate-950 px-3 py-1.5 text-sm font-medium text-white focus:border-cyan-400 focus:outline-none"
    };

    view! {
        <label class=wrapper_class>
            {if compact {
                view! {
                    <span class="sr-only">{move || t(locale.get(), Msg::LanguageLabel)}</span>
                    <svg
                        class="h-3.5 w-3.5 shrink-0 text-cyan-300"
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        stroke-width="2"
                        stroke-linecap="round"
                        stroke-linejoin="round"
                        aria-hidden="true"
                    >
                        <circle cx="12" cy="12" r="10" />
                        <path d="M2 12h20" />
                        <path d="M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z" />
                    </svg>
                }
                .into_any()
            } else {
                view! {
                    <span class="text-xs font-semibold uppercase tracking-wider text-slate-400">
                        {move || t(locale.get(), Msg::LanguageLabel)}
                    </span>
                }
                .into_any()
            }}
            <select
                class=select_class
                aria-label=move || t(locale.get(), Msg::LanguageLabel)
                prop:value=move || locale.get().bcp47()
                on:change=move |ev| {
                    let val = event_target_value(&ev);
                    if let Some(next) = Locale::parse_tag(&val) {
                        set_user_locale(locale, next);
                    }
                }
            >
                {Locale::ALL
                    .iter()
                    .copied()
                    .map(|loc| {
                        view! {
                            <option
                                class="bg-slate-900 text-slate-100"
                                value=loc.bcp47()
                                lang=loc.bcp47()
                                dir=loc.dir().as_str()
                                selected=move || locale.get() == loc
                            >
                                {loc.native_name()}
                            </option>
                        }
                    })
                    .collect_view()}
            </select>
        </label>
    }
}

/// Compile-time exhaustive UI message catalog keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Msg {
    // Nav & Chrome
    NavAbout,
    NavExpertise,
    NavServices,
    NavProjects,
    NavExperience,
    NavSkills,
    NavCommissions,
    NavTools,
    NavHireMe,
    NavOpenMenu,
    NavCloseMenu,
    LanguageLabel,
    SkipToContent,
    FooterPrivacy,
    FooterAllRights,
    FooterBuiltWith,
    FooterCommunityPolicy,
    FooterCommunityActivity,
    FooterCreators,
    FooterCodeOwnership,
    ToolMentalHealth,
    ToolRecoveryFinder,
    ToolClinicalSuite,
    ToolPatientDemo,
    ToolPhysicianDemo,
    ToolPortfolio,
    ToolAndroidApp,
    ToolDuxAncestry,
    ToolOwnership,

    // Clinical Suite Switcher & Overview
    SuiteAriaLabel,
    SuitePatient,
    SuitePhysician,
    SuiteMentalHealth,
    SuiteRecoveryFinder,
    SuiteOverview,
    ZeroPhiBannerTitle,
    ZeroPhiBannerBadge,
    OnDeviceOnlyBadge,
    OfflineReadyBadge,
    SyntheticDemoNotice,

    // Physician Workspace (/clinical/physician)
    PhysicianEyebrow,
    PhysicianTitle,
    PhysicianTodayQueue,
    PhysicianFilterAll,
    PhysicianFilterHigh,
    PhysicianFilterModerate,
    PhysicianFilterRoutine,
    PhysicianFilterDrafts,
    PhysicianPatientPanel,
    PhysicianSearchPlaceholder,
    PhysicianNoMatch,
    PhysicianShowPanel,
    PhysicianJumpTo,
    PhysicianJumpSummary,
    PhysicianJumpQa,
    PhysicianJumpClinical,
    PhysicianJumpReply,
    PhysicianJumpRecords,
    PhysicianQaEyebrow,
    PhysicianQaHeading,
    PhysicianAskPlaceholder,
    PhysicianAskButton,
    PhysicianSynthesizing,
    PhysicianProblems,
    PhysicianMedications,
    PhysicianFollowUps,
    PhysicianSourceRecords,
    PhysicianBackToQa,
    PhysicianClearFocus,

    // Patient Companion (/clinical/patient)
    PatientEyebrow,
    PatientTitle,
    PatientChooseProfile,
    PatientResetSample,
    PatientTabYou,
    PatientTabDoctor,
    PatientJumpSummary,
    PatientJumpAsk,
    PatientJumpRecords,
    PatientJumpNotes,
    PatientFindRecoveryLink,
    PatientMedicationsHeading,
    PatientFocusAreasHeading,
    PatientAskHeading,
    PatientAskPlaceholder,
    PatientExplainButton,
    PatientRecordsHeading,
    PatientPrivateNotesHeading,
    PatientAlwaysExcluded,
    PatientReviewForDoctor,
    PatientIncludeForDoctor,
    PatientIncludedForDoctor,

    // Mental Health (/mental-health)
    MhEyebrow,
    MhTitle,
    MhOpenMobile,
    MhOpenClinic,
    MhResetSample,
    MhTabOverview,
    MhTabLogImport,
    MhTabAlertsAi,
    MhTabCareBrief,
    MhTabRecovery,
    MhTabAll,
    MhUploadCgmCta,
    MhLogCheckInHeading,
    MhAddCheckIn,
    MhCgmImportHeading,
    MhParseValidateExport,
    MhCorrelationLogHeading,
    MhDownloadCsv,

    // Recovery Search (/tools/recovery & /mental-health?tab=recovery)
    RecoveryEyebrow,
    RecoveryTitle,
    RecoveryCrisisBannerTitle,
    RecoveryCrisis988Detail,
    RecoveryCrisisLanguageNote,
    RecoveryStateLabel,
    RecoverySearchPlaceholder,
    RecoveryUseLocation,
    RecoveryLocating,
    RecoveryClearFilters,
    RecoveryMoreFilters,
    RecoveryCareFocus,
    RecoverySortName,
    RecoverySortDistance,
    RecoverySaveCareTeam,
    RecoverySavedCareTeam,
    RecoveryCallNow,
    RecoveryDirections,
    RecoveryWebsite,
    RecoveryPrevPage,
    RecoveryNextPage,

    // Dashboard Layout Customizer
    LayoutButtonPrefix,
    LayoutSavedOnDevice,
    LayoutReset,
    LayoutDone,
    LayoutActiveBadge,
    LayoutCustomLabel,
    LayoutDensityCompact,
    LayoutDensityBalanced,
    LayoutDensitySpacious,
    LayoutPanelLeft,
    LayoutPanelRight,
    LayoutPanelFullWidth,
    LayoutQaFirst,
    LayoutClinicalFirst,
    LayoutAskFirst,
    LayoutRecordsFirst,
    LayoutPresetFocusedTriage,
    LayoutPresetChartFirstSplit,
    LayoutPresetCalmReading,
    LayoutPresetCalmCompanion,
    LayoutPresetRecordsFirstCompact,
    LayoutPresetVisitPrepFocus,
    LayoutPresetGuidedTabs,
    LayoutPresetDailyCheckIn,
    LayoutPresetFullAnalyticsWall,

    // Chart Citations & Provenance
    CitationJumpToSource,
    CitationClickToJump,
    CitationFocusedBadge,
    CitationCitedRecordsHeading,
    CitationSourceLanguageNote,
}

impl Msg {
    /// Complete slice of all [`Msg`] variants for exhaustive validation.
    pub const ALL: &'static [Msg] = &[
        // Nav & Chrome
        Msg::NavAbout,
        Msg::NavExpertise,
        Msg::NavServices,
        Msg::NavProjects,
        Msg::NavExperience,
        Msg::NavSkills,
        Msg::NavCommissions,
        Msg::NavTools,
        Msg::NavHireMe,
        Msg::NavOpenMenu,
        Msg::NavCloseMenu,
        Msg::LanguageLabel,
        Msg::SkipToContent,
        Msg::FooterPrivacy,
        Msg::FooterAllRights,
        Msg::FooterBuiltWith,
        Msg::FooterCommunityPolicy,
        Msg::FooterCommunityActivity,
        Msg::FooterCreators,
        Msg::FooterCodeOwnership,
        Msg::ToolMentalHealth,
        Msg::ToolRecoveryFinder,
        Msg::ToolClinicalSuite,
        Msg::ToolPatientDemo,
        Msg::ToolPhysicianDemo,
        Msg::ToolPortfolio,
        Msg::ToolAndroidApp,
        Msg::ToolDuxAncestry,
        Msg::ToolOwnership,
        // Clinical Suite Switcher & Overview
        Msg::SuiteAriaLabel,
        Msg::SuitePatient,
        Msg::SuitePhysician,
        Msg::SuiteMentalHealth,
        Msg::SuiteRecoveryFinder,
        Msg::SuiteOverview,
        Msg::ZeroPhiBannerTitle,
        Msg::ZeroPhiBannerBadge,
        Msg::OnDeviceOnlyBadge,
        Msg::OfflineReadyBadge,
        Msg::SyntheticDemoNotice,
        // Physician Workspace
        Msg::PhysicianEyebrow,
        Msg::PhysicianTitle,
        Msg::PhysicianTodayQueue,
        Msg::PhysicianFilterAll,
        Msg::PhysicianFilterHigh,
        Msg::PhysicianFilterModerate,
        Msg::PhysicianFilterRoutine,
        Msg::PhysicianFilterDrafts,
        Msg::PhysicianPatientPanel,
        Msg::PhysicianSearchPlaceholder,
        Msg::PhysicianNoMatch,
        Msg::PhysicianShowPanel,
        Msg::PhysicianJumpTo,
        Msg::PhysicianJumpSummary,
        Msg::PhysicianJumpQa,
        Msg::PhysicianJumpClinical,
        Msg::PhysicianJumpReply,
        Msg::PhysicianJumpRecords,
        Msg::PhysicianQaEyebrow,
        Msg::PhysicianQaHeading,
        Msg::PhysicianAskPlaceholder,
        Msg::PhysicianAskButton,
        Msg::PhysicianSynthesizing,
        Msg::PhysicianProblems,
        Msg::PhysicianMedications,
        Msg::PhysicianFollowUps,
        Msg::PhysicianSourceRecords,
        Msg::PhysicianBackToQa,
        Msg::PhysicianClearFocus,
        // Patient Companion
        Msg::PatientEyebrow,
        Msg::PatientTitle,
        Msg::PatientChooseProfile,
        Msg::PatientResetSample,
        Msg::PatientTabYou,
        Msg::PatientTabDoctor,
        Msg::PatientJumpSummary,
        Msg::PatientJumpAsk,
        Msg::PatientJumpRecords,
        Msg::PatientJumpNotes,
        Msg::PatientFindRecoveryLink,
        Msg::PatientMedicationsHeading,
        Msg::PatientFocusAreasHeading,
        Msg::PatientAskHeading,
        Msg::PatientAskPlaceholder,
        Msg::PatientExplainButton,
        Msg::PatientRecordsHeading,
        Msg::PatientPrivateNotesHeading,
        Msg::PatientAlwaysExcluded,
        Msg::PatientReviewForDoctor,
        Msg::PatientIncludeForDoctor,
        Msg::PatientIncludedForDoctor,
        // Mental Health
        Msg::MhEyebrow,
        Msg::MhTitle,
        Msg::MhOpenMobile,
        Msg::MhOpenClinic,
        Msg::MhResetSample,
        Msg::MhTabOverview,
        Msg::MhTabLogImport,
        Msg::MhTabAlertsAi,
        Msg::MhTabCareBrief,
        Msg::MhTabRecovery,
        Msg::MhTabAll,
        Msg::MhUploadCgmCta,
        Msg::MhLogCheckInHeading,
        Msg::MhAddCheckIn,
        Msg::MhCgmImportHeading,
        Msg::MhParseValidateExport,
        Msg::MhCorrelationLogHeading,
        Msg::MhDownloadCsv,
        // Recovery Search
        Msg::RecoveryEyebrow,
        Msg::RecoveryTitle,
        Msg::RecoveryCrisisBannerTitle,
        Msg::RecoveryCrisis988Detail,
        Msg::RecoveryCrisisLanguageNote,
        Msg::RecoveryStateLabel,
        Msg::RecoverySearchPlaceholder,
        Msg::RecoveryUseLocation,
        Msg::RecoveryLocating,
        Msg::RecoveryClearFilters,
        Msg::RecoveryMoreFilters,
        Msg::RecoveryCareFocus,
        Msg::RecoverySortName,
        Msg::RecoverySortDistance,
        Msg::RecoverySaveCareTeam,
        Msg::RecoverySavedCareTeam,
        Msg::RecoveryCallNow,
        Msg::RecoveryDirections,
        Msg::RecoveryWebsite,
        Msg::RecoveryPrevPage,
        Msg::RecoveryNextPage,
        // Dashboard Layout Customizer
        Msg::LayoutButtonPrefix,
        Msg::LayoutSavedOnDevice,
        Msg::LayoutReset,
        Msg::LayoutDone,
        Msg::LayoutActiveBadge,
        Msg::LayoutCustomLabel,
        Msg::LayoutDensityCompact,
        Msg::LayoutDensityBalanced,
        Msg::LayoutDensitySpacious,
        Msg::LayoutPanelLeft,
        Msg::LayoutPanelRight,
        Msg::LayoutPanelFullWidth,
        Msg::LayoutQaFirst,
        Msg::LayoutClinicalFirst,
        Msg::LayoutAskFirst,
        Msg::LayoutRecordsFirst,
        Msg::LayoutPresetFocusedTriage,
        Msg::LayoutPresetChartFirstSplit,
        Msg::LayoutPresetCalmReading,
        Msg::LayoutPresetCalmCompanion,
        Msg::LayoutPresetRecordsFirstCompact,
        Msg::LayoutPresetVisitPrepFocus,
        Msg::LayoutPresetGuidedTabs,
        Msg::LayoutPresetDailyCheckIn,
        Msg::LayoutPresetFullAnalyticsWall,
        // Chart Citations & Provenance
        Msg::CitationJumpToSource,
        Msg::CitationClickToJump,
        Msg::CitationFocusedBadge,
        Msg::CitationCitedRecordsHeading,
        Msg::CitationSourceLanguageNote,
    ];
}

/// Compile-time exhaustive translation lookup for any `(Locale, Msg)` pair.
pub const fn t(locale: Locale, key: Msg) -> &'static str {
    match locale {
        Locale::En => translate_en(key),
        Locale::Es => translate_es(key),
        Locale::Fr => translate_fr(key),
        Locale::Pt => translate_pt(key),
        Locale::De => translate_de(key),
        Locale::ZhHans => translate_zh_hans(key),
        Locale::Ja => translate_ja(key),
        Locale::Ko => translate_ko(key),
        Locale::Ar => translate_ar(key),
    }
}

const fn translate_en(key: Msg) -> &'static str {
    match key {
        // Nav & Chrome
        Msg::NavAbout => "About",
        Msg::NavExpertise => "Expertise",
        Msg::NavServices => "Services",
        Msg::NavProjects => "Projects",
        Msg::NavExperience => "Experience",
        Msg::NavSkills => "Skills",
        Msg::NavCommissions => "Commissions",
        Msg::NavTools => "Tools",
        Msg::NavHireMe => "Hire me",
        Msg::NavOpenMenu => "Open menu",
        Msg::NavCloseMenu => "Close menu",
        Msg::LanguageLabel => "Language",
        Msg::SkipToContent => "Skip to content",
        Msg::FooterPrivacy => "Privacy",
        Msg::FooterAllRights => "All rights reserved",
        Msg::FooterBuiltWith => "Built with Rust, Leptos & Tauri",
        Msg::FooterCommunityPolicy => "Community policy",
        Msg::FooterCommunityActivity => "Community activity",
        Msg::FooterCreators => "Creators",
        Msg::FooterCodeOwnership => "Code ownership",
        Msg::ToolMentalHealth => "Mental Health & CGM",
        Msg::ToolRecoveryFinder => "Recovery Care Finder",
        Msg::ToolClinicalSuite => "Clinical Workspace",
        Msg::ToolPatientDemo => "Patient Experience Demo",
        Msg::ToolPhysicianDemo => "Physician Workspace Demo",
        Msg::ToolPortfolio => "Investment Account Calculator",
        Msg::ToolAndroidApp => "Android App (F-Droid / APK)",
        Msg::ToolDuxAncestry => "Dux Ancestry & Genetics",
        Msg::ToolOwnership => "Code Ownership Report",

        // Clinical Suite Switcher & Overview
        Msg::SuiteAriaLabel => "Clinical demo suite",
        Msg::SuitePatient => "Curie Mobile (Patient)",
        Msg::SuitePhysician => "Curie Clinic (Physician)",
        Msg::SuiteMentalHealth => "Mental Health & CGM",
        Msg::SuiteRecoveryFinder => "Recovery Care Finder",
        Msg::SuiteOverview => "Suite Overview",
        Msg::ZeroPhiBannerTitle => "Synthetic Zero-PHI Guarantee",
        Msg::ZeroPhiBannerBadge => "Zero-PHI Demo",
        Msg::OnDeviceOnlyBadge => "On-Device Only",
        Msg::OfflineReadyBadge => "Offline-Ready",
        Msg::SyntheticDemoNotice => {
            "100% Synthetic Demo · Zero PHI · Do Not Enter Real Health Information"
        }

        // Physician Workspace
        Msg::PhysicianEyebrow => "Curie Clinic",
        Msg::PhysicianTitle => "Physician Workspace Demo",
        Msg::PhysicianTodayQueue => "Today's Clinic Queue",
        Msg::PhysicianFilterAll => "All",
        Msg::PhysicianFilterHigh => "High",
        Msg::PhysicianFilterModerate => "Moderate",
        Msg::PhysicianFilterRoutine => "Routine",
        Msg::PhysicianFilterDrafts => "Drafts",
        Msg::PhysicianPatientPanel => "Patient Panel",
        Msg::PhysicianSearchPlaceholder => "Search patient, MRN, focus…",
        Msg::PhysicianNoMatch => "No patients match this filter.",
        Msg::PhysicianShowPanel => "Show Patient Panel",
        Msg::PhysicianJumpTo => "Jump to:",
        Msg::PhysicianJumpSummary => "Summary",
        Msg::PhysicianJumpQa => "Chart Q&A",
        Msg::PhysicianJumpClinical => "Problems & Meds",
        Msg::PhysicianJumpReply => "Reply Draft",
        Msg::PhysicianJumpRecords => "Source Records",
        Msg::PhysicianQaEyebrow => "Grounded Chart Q&A",
        Msg::PhysicianQaHeading => "Ask this chart",
        Msg::PhysicianAskPlaceholder => "Ask a clinical question grounded in this chart…",
        Msg::PhysicianAskButton => "Ask Chart",
        Msg::PhysicianSynthesizing => "Synthesizing…",
        Msg::PhysicianProblems => "Active Problems",
        Msg::PhysicianMedications => "Medications",
        Msg::PhysicianFollowUps => "Follow-Ups",
        Msg::PhysicianSourceRecords => "Longitudinal Source Records",
        Msg::PhysicianBackToQa => "Back to Q&A ↑",
        Msg::PhysicianClearFocus => "Clear",

        // Patient Companion
        Msg::PatientEyebrow => "Curie Mobile",
        Msg::PatientTitle => "Patient Experience Demo",
        Msg::PatientChooseProfile => "Choose a Fictional Sample Profile",
        Msg::PatientResetSample => "Reset sample state",
        Msg::PatientTabYou => "Your Health (You)",
        Msg::PatientTabDoctor => "For My Doctor",
        Msg::PatientJumpSummary => "Summary",
        Msg::PatientJumpAsk => "Ask Curie",
        Msg::PatientJumpRecords => "Health Records",
        Msg::PatientJumpNotes => "Private Notes",
        Msg::PatientFindRecoveryLink => "Find Recovery Care",
        Msg::PatientMedicationsHeading => "Medications",
        Msg::PatientFocusAreasHeading => "Focus Areas",
        Msg::PatientAskHeading => "Ask Curie about your records",
        Msg::PatientAskPlaceholder => "Ask a question in plain language…",
        Msg::PatientExplainButton => "Explain from My Records",
        Msg::PatientRecordsHeading => "Health Records",
        Msg::PatientPrivateNotesHeading => "Private Notes (Never Shared)",
        Msg::PatientAlwaysExcluded => "Always excluded from sharing",
        Msg::PatientReviewForDoctor => "Review for My Doctor",
        Msg::PatientIncludeForDoctor => "Include for my doctor",
        Msg::PatientIncludedForDoctor => "Included for doctor",

        // Mental Health
        Msg::MhEyebrow => "Curie Mobile · Metabolic Health & All Commercial CGMs",
        Msg::MhTitle => "Mental Health & Comprehensive Glucose Tracking",
        Msg::MhOpenMobile => "Open Curie Mobile →",
        Msg::MhOpenClinic => "Open Curie Clinic →",
        Msg::MhResetSample => "Reset sample",
        Msg::MhTabOverview => "Overview & Charts",
        Msg::MhTabLogImport => "Log & Import",
        Msg::MhTabAlertsAi => "Alerts & AI",
        Msg::MhTabCareBrief => "Care & Brief",
        Msg::MhTabRecovery => "Find Recovery Care",
        Msg::MhTabAll => "All Sections",
        Msg::MhUploadCgmCta => "Upload your CGM file ↓",
        Msg::MhLogCheckInHeading => "Log Mental Health Check-In",
        Msg::MhAddCheckIn => "Add Check-In",
        Msg::MhCgmImportHeading => "Commercial CGM & Glucose File Import",
        Msg::MhParseValidateExport => "Parse & Validate Export",
        Msg::MhCorrelationLogHeading => "Glucose–Mood Correlation Log",
        Msg::MhDownloadCsv => "Download CSV",

        // Recovery Search
        Msg::RecoveryEyebrow => "100% On-Device Search · SAMHSA + Public Directories",
        Msg::RecoveryTitle => "Find Mental Health & Recovery Care Near You",
        Msg::RecoveryCrisisBannerTitle => "Need immediate help? Call or text 988",
        Msg::RecoveryCrisis988Detail => {
            "Free, confidential, 24/7 crisis support across the United States."
        }
        Msg::RecoveryCrisisLanguageNote => {
            "988 offers direct Spanish support (press 2 or text AYUDA) and interpretation in 240+ languages."
        }
        Msg::RecoveryStateLabel => "State or Territory",
        Msg::RecoverySearchPlaceholder => "Search city, program, specialty, or zip…",
        Msg::RecoveryUseLocation => "Use my location",
        Msg::RecoveryLocating => "Locating…",
        Msg::RecoveryClearFilters => "Clear filters",
        Msg::RecoveryMoreFilters => "More filters",
        Msg::RecoveryCareFocus => "Care Focus",
        Msg::RecoverySortName => "Sort: Name",
        Msg::RecoverySortDistance => "Sort: Distance",
        Msg::RecoverySaveCareTeam => "+ Care Team",
        Msg::RecoverySavedCareTeam => "Saved to Care Team",
        Msg::RecoveryCallNow => "Call",
        Msg::RecoveryDirections => "Directions ↗",
        Msg::RecoveryWebsite => "Website ↗",
        Msg::RecoveryPrevPage => "← Previous",
        Msg::RecoveryNextPage => "Next →",

        // Dashboard Layout Customizer
        Msg::LayoutButtonPrefix => "Layout",
        Msg::LayoutSavedOnDevice => "Saved on This Device",
        Msg::LayoutReset => "Reset",
        Msg::LayoutDone => "Done",
        Msg::LayoutActiveBadge => "Active",
        Msg::LayoutCustomLabel => "Custom",
        Msg::LayoutDensityCompact => "Compact",
        Msg::LayoutDensityBalanced => "Balanced",
        Msg::LayoutDensitySpacious => "Spacious",
        Msg::LayoutPanelLeft => "Panel left",
        Msg::LayoutPanelRight => "Panel right",
        Msg::LayoutPanelFullWidth => "Full-width chart",
        Msg::LayoutQaFirst => "Chart Q&A first",
        Msg::LayoutClinicalFirst => "Problems & Meds first",
        Msg::LayoutAskFirst => "Ask Curie first",
        Msg::LayoutRecordsFirst => "Health records first",
        Msg::LayoutPresetFocusedTriage => "Focused Triage",
        Msg::LayoutPresetChartFirstSplit => "Chart-First Split",
        Msg::LayoutPresetCalmReading => "Calm Reading",
        Msg::LayoutPresetCalmCompanion => "Calm Companion",
        Msg::LayoutPresetRecordsFirstCompact => "Records-First Compact",
        Msg::LayoutPresetVisitPrepFocus => "Visit Prep Focus",
        Msg::LayoutPresetGuidedTabs => "Guided Tabs",
        Msg::LayoutPresetDailyCheckIn => "Daily Check-In & Care",
        Msg::LayoutPresetFullAnalyticsWall => "Full Analytics Wall",

        // Chart Citations & Provenance
        Msg::CitationJumpToSource => "Jump to source record ↓",
        Msg::CitationClickToJump => "Click to jump to source record ↓",
        Msg::CitationFocusedBadge => "Focused from citation",
        Msg::CitationCitedRecordsHeading => "Cited Source Records",
        Msg::CitationSourceLanguageNote => "Original record language: English (verbatim)",
    }
}

const fn translate_es(key: Msg) -> &'static str {
    match key {
        // Nav & Chrome
        Msg::NavAbout => "Acerca de",
        Msg::NavExpertise => "Especialidad",
        Msg::NavServices => "Servicios",
        Msg::NavProjects => "Proyectos",
        Msg::NavExperience => "Experiencia",
        Msg::NavSkills => "Habilidades",
        Msg::NavCommissions => "Encargos",
        Msg::NavTools => "Herramientas",
        Msg::NavHireMe => "Contrátame",
        Msg::NavOpenMenu => "Abrir menú",
        Msg::NavCloseMenu => "Cerrar menú",
        Msg::LanguageLabel => "Idioma",
        Msg::SkipToContent => "Saltar al contenido",
        Msg::FooterPrivacy => "Privacidad",
        Msg::FooterAllRights => "Todos los derechos reservados",
        Msg::FooterBuiltWith => "Creado con Rust, Leptos y Tauri",
        Msg::FooterCommunityPolicy => "Política comunitaria",
        Msg::FooterCommunityActivity => "Actividad comunitaria",
        Msg::FooterCreators => "Creadores",
        Msg::FooterCodeOwnership => "Propiedad del código",
        Msg::ToolMentalHealth => "Salud Mental y CGM",
        Msg::ToolRecoveryFinder => "Buscador de Recuperación",
        Msg::ToolClinicalSuite => "Espacio Clínico",
        Msg::ToolPatientDemo => "Demo de Experiencia del Paciente",
        Msg::ToolPhysicianDemo => "Demo de Espacio Médico",
        Msg::ToolPortfolio => "Calculadora de Cuenta de Inversión",
        Msg::ToolAndroidApp => "App Android (F-Droid / APK)",
        Msg::ToolDuxAncestry => "Dux Ascendencia y Genética",
        Msg::ToolOwnership => "Informe de Propiedad del Código",

        // Clinical Suite Switcher & Overview
        Msg::SuiteAriaLabel => "Suite clínica de demostración",
        Msg::SuitePatient => "Curie Mobile (Paciente)",
        Msg::SuitePhysician => "Curie Clinic (Médico)",
        Msg::SuiteMentalHealth => "Salud Mental y CGM",
        Msg::SuiteRecoveryFinder => "Buscador de Recuperación",
        Msg::SuiteOverview => "Resumen de la Suite",
        Msg::ZeroPhiBannerTitle => "Garantía Sintética Sin PHI",
        Msg::ZeroPhiBannerBadge => "Demo Sin PHI",
        Msg::OnDeviceOnlyBadge => "Solo en el dispositivo",
        Msg::OfflineReadyBadge => "Listo sin conexión",
        Msg::SyntheticDemoNotice => {
            "Demostración 100 % sintética · Sin PHI · No ingrese información médica real"
        }

        // Physician Workspace
        Msg::PhysicianEyebrow => "Curie Clinic",
        Msg::PhysicianTitle => "Demo de Espacio Médico",
        Msg::PhysicianTodayQueue => "Cola Clínica de Hoy",
        Msg::PhysicianFilterAll => "Todos",
        Msg::PhysicianFilterHigh => "Alta",
        Msg::PhysicianFilterModerate => "Moderada",
        Msg::PhysicianFilterRoutine => "Rutina",
        Msg::PhysicianFilterDrafts => "Borradores",
        Msg::PhysicianPatientPanel => "Panel de Pacientes",
        Msg::PhysicianSearchPlaceholder => "Buscar paciente, MRN, motivo…",
        Msg::PhysicianNoMatch => "Ningún paciente coincide con este filtro.",
        Msg::PhysicianShowPanel => "Mostrar Panel de Pacientes",
        Msg::PhysicianJumpTo => "Ir a:",
        Msg::PhysicianJumpSummary => "Resumen",
        Msg::PhysicianJumpQa => "Consultas del Historial",
        Msg::PhysicianJumpClinical => "Problemas y Medicamentos",
        Msg::PhysicianJumpReply => "Borrador de Respuesta",
        Msg::PhysicianJumpRecords => "Registros Fuente",
        Msg::PhysicianQaEyebrow => "Consultas Fundamentadas en el Historial",
        Msg::PhysicianQaHeading => "Consultar este historial",
        Msg::PhysicianAskPlaceholder => {
            "Formule una pregunta clínica fundamentada en este historial…"
        }
        Msg::PhysicianAskButton => "Consultar Historial",
        Msg::PhysicianSynthesizing => "Sintetizando…",
        Msg::PhysicianProblems => "Problemas Activos",
        Msg::PhysicianMedications => "Medicamentos",
        Msg::PhysicianFollowUps => "Seguimientos",
        Msg::PhysicianSourceRecords => "Registros Fuente Longitudinales",
        Msg::PhysicianBackToQa => "Volver a Consultas ↑",
        Msg::PhysicianClearFocus => "Limpiar",

        // Patient Companion
        Msg::PatientEyebrow => "Curie Mobile",
        Msg::PatientTitle => "Demo de Experiencia del Paciente",
        Msg::PatientChooseProfile => "Elija un Perfil Ficticio de Muestra",
        Msg::PatientResetSample => "Restablecer muestra",
        Msg::PatientTabYou => "Tu Salud (Tú)",
        Msg::PatientTabDoctor => "Para Mi Médico",
        Msg::PatientJumpSummary => "Resumen",
        Msg::PatientJumpAsk => "Preguntar a Curie",
        Msg::PatientJumpRecords => "Registros de Salud",
        Msg::PatientJumpNotes => "Notas Privadas",
        Msg::PatientFindRecoveryLink => "Buscar Atención de Recuperación",
        Msg::PatientMedicationsHeading => "Medicamentos",
        Msg::PatientFocusAreasHeading => "Áreas de Atención",
        Msg::PatientAskHeading => "Pregunta a Curie sobre tus registros",
        Msg::PatientAskPlaceholder => "Haz una pregunta en lenguaje sencillo…",
        Msg::PatientExplainButton => "Explicar según Mis Registros",
        Msg::PatientRecordsHeading => "Registros de Salud",
        Msg::PatientPrivateNotesHeading => "Notas Privadas (Nunca Compartidas)",
        Msg::PatientAlwaysExcluded => "Siempre excluidas al compartir",
        Msg::PatientReviewForDoctor => "Revisar para Mi Médico",
        Msg::PatientIncludeForDoctor => "Incluir para mi médico",
        Msg::PatientIncludedForDoctor => "Incluido para el médico",

        // Mental Health
        Msg::MhEyebrow => "Curie Mobile · Salud Metabólica y Todos los CGM Comerciales",
        Msg::MhTitle => "Salud Mental y Seguimiento Integral de Glucosa",
        Msg::MhOpenMobile => "Abrir Curie Mobile →",
        Msg::MhOpenClinic => "Abrir Curie Clinic →",
        Msg::MhResetSample => "Restablecer muestra",
        Msg::MhTabOverview => "Resumen y Gráficos",
        Msg::MhTabLogImport => "Registro e Importación",
        Msg::MhTabAlertsAi => "Alertas e IA",
        Msg::MhTabCareBrief => "Cuidado y Resumen",
        Msg::MhTabRecovery => "Buscar Recuperación",
        Msg::MhTabAll => "Todas las Secciones",
        Msg::MhUploadCgmCta => "Subir tu archivo CGM ↓",
        Msg::MhLogCheckInHeading => "Registrar Estado de Salud Mental",
        Msg::MhAddCheckIn => "Guardar Registro",
        Msg::MhCgmImportHeading => "Importación de Archivos CGM y Glucosa",
        Msg::MhParseValidateExport => "Analizar y Validar Exportación",
        Msg::MhCorrelationLogHeading => "Registro de Correlación Glucosa–Ánimo",
        Msg::MhDownloadCsv => "Descargar CSV",

        // Recovery Search
        Msg::RecoveryEyebrow => "Búsqueda 100 % en el Dispositivo · SAMHSA y Directorios Públicos",
        Msg::RecoveryTitle => "Encuentra Atención de Salud Mental y Recuperación Cerca de Ti",
        Msg::RecoveryCrisisBannerTitle => "¿Necesitas ayuda inmediata? Llama o escribe al 988",
        Msg::RecoveryCrisis988Detail => {
            "Apoyo gratuito, confidencial y disponible 24/7 en todo Estados Unidos."
        }
        Msg::RecoveryCrisisLanguageNote => {
            "El 988 ofrece atención directa en español (marque 2 o envíe AYUDA por SMS) e interpretación en más de 240 idiomas."
        }
        Msg::RecoveryStateLabel => "Estado o Territorio",
        Msg::RecoverySearchPlaceholder => "Buscar ciudad, programa, especialidad o código postal…",
        Msg::RecoveryUseLocation => "Usar mi ubicación",
        Msg::RecoveryLocating => "Localizando…",
        Msg::RecoveryClearFilters => "Borrar filtros",
        Msg::RecoveryMoreFilters => "Más filtros",
        Msg::RecoveryCareFocus => "Tipo de Atención",
        Msg::RecoverySortName => "Ordenar: Nombre",
        Msg::RecoverySortDistance => "Ordenar: Distancia",
        Msg::RecoverySaveCareTeam => "+ Equipo de Cuidado",
        Msg::RecoverySavedCareTeam => "Guardado en Equipo de Cuidado",
        Msg::RecoveryCallNow => "Llamar",
        Msg::RecoveryDirections => "Cómo llegar ↗",
        Msg::RecoveryWebsite => "Sitio web ↗",
        Msg::RecoveryPrevPage => "← Anterior",
        Msg::RecoveryNextPage => "Siguiente →",

        // Dashboard Layout Customizer
        Msg::LayoutButtonPrefix => "Diseño",
        Msg::LayoutSavedOnDevice => "Guardado en Este Dispositivo",
        Msg::LayoutReset => "Restablecer",
        Msg::LayoutDone => "Listo",
        Msg::LayoutActiveBadge => "Activo",
        Msg::LayoutCustomLabel => "Personalizado",
        Msg::LayoutDensityCompact => "Compacto",
        Msg::LayoutDensityBalanced => "Equilibrado",
        Msg::LayoutDensitySpacious => "Amplio",
        Msg::LayoutPanelLeft => "Panel a la izquierda",
        Msg::LayoutPanelRight => "Panel a la derecha",
        Msg::LayoutPanelFullWidth => "Historial ancho completo",
        Msg::LayoutQaFirst => "Consultas primero",
        Msg::LayoutClinicalFirst => "Problemas y medicamentos primero",
        Msg::LayoutAskFirst => "Preguntar a Curie primero",
        Msg::LayoutRecordsFirst => "Registros de salud primero",
        Msg::LayoutPresetFocusedTriage => "Triaje Enfocado",
        Msg::LayoutPresetChartFirstSplit => "Historial Dividido Primero",
        Msg::LayoutPresetCalmReading => "Lectura Tranquila",
        Msg::LayoutPresetCalmCompanion => "Acompañante Sereno",
        Msg::LayoutPresetRecordsFirstCompact => "Registros Primero Compacto",
        Msg::LayoutPresetVisitPrepFocus => "Enfoque de Preparación de Consulta",
        Msg::LayoutPresetGuidedTabs => "Pestañas Guiadas",
        Msg::LayoutPresetDailyCheckIn => "Registro Diario y Cuidado",
        Msg::LayoutPresetFullAnalyticsWall => "Panel Analítico Completo",

        // Chart Citations & Provenance
        Msg::CitationJumpToSource => "Ir al registro fuente ↓",
        Msg::CitationClickToJump => "Haz clic para ir al registro fuente ↓",
        Msg::CitationFocusedBadge => "Enfocado desde cita",
        Msg::CitationCitedRecordsHeading => "Registros Fuente Citados",
        Msg::CitationSourceLanguageNote => "Idioma original del registro: inglés (texto literal)",
    }
}

const fn translate_fr(key: Msg) -> &'static str {
    match key {
        // Nav & Chrome
        Msg::NavAbout => "À propos",
        Msg::NavExpertise => "Expertise",
        Msg::NavServices => "Services",
        Msg::NavProjects => "Projets",
        Msg::NavExperience => "Expérience",
        Msg::NavSkills => "Compétences",
        Msg::NavCommissions => "Commandes",
        Msg::NavTools => "Outils",
        Msg::NavHireMe => "Me recruter",
        Msg::NavOpenMenu => "Ouvrir le menu",
        Msg::NavCloseMenu => "Fermer le menu",
        Msg::LanguageLabel => "Langue",
        Msg::SkipToContent => "Aller au contenu",
        Msg::FooterPrivacy => "Confidentialité",
        Msg::FooterAllRights => "Tous droits réservés",
        Msg::FooterBuiltWith => "Conçu avec Rust, Leptos et Tauri",
        Msg::FooterCommunityPolicy => "Charte communautaire",
        Msg::FooterCommunityActivity => "Activité communautaire",
        Msg::FooterCreators => "Créateurs",
        Msg::FooterCodeOwnership => "Propriété du code",
        Msg::ToolMentalHealth => "Santé Mentale et CGM",
        Msg::ToolRecoveryFinder => "Recherche de Soins de Rétablissement",
        Msg::ToolClinicalSuite => "Espace Clinique",
        Msg::ToolPatientDemo => "Démo Expérience Patient",
        Msg::ToolPhysicianDemo => "Démo Espace Médecin",
        Msg::ToolPortfolio => "Calculateur de Compte d'Investissement",
        Msg::ToolAndroidApp => "Application Android (F-Droid / APK)",
        Msg::ToolDuxAncestry => "Dux Ascendance et Génétique",
        Msg::ToolOwnership => "Rapport de Propriété du Code",

        // Clinical Suite Switcher & Overview
        Msg::SuiteAriaLabel => "Suite clinique de démonstration",
        Msg::SuitePatient => "Curie Mobile (Patient)",
        Msg::SuitePhysician => "Curie Clinic (Médecin)",
        Msg::SuiteMentalHealth => "Santé Mentale et CGM",
        Msg::SuiteRecoveryFinder => "Recherche de Rétablissement",
        Msg::SuiteOverview => "Aperçu de la Suite",
        Msg::ZeroPhiBannerTitle => "Garantie Synthétique Sans PHI",
        Msg::ZeroPhiBannerBadge => "Démo Sans PHI",
        Msg::OnDeviceOnlyBadge => "Sur l'appareil uniquement",
        Msg::OfflineReadyBadge => "Prêt hors ligne",
        Msg::SyntheticDemoNotice => {
            "Démo 100 % synthétique · Aucune PHI · Ne saisissez aucune donnée médicale réelle"
        }

        // Physician Workspace
        Msg::PhysicianEyebrow => "Curie Clinic",
        Msg::PhysicianTitle => "Démo Espace Médecin",
        Msg::PhysicianTodayQueue => "File Clinique du Jour",
        Msg::PhysicianFilterAll => "Tous",
        Msg::PhysicianFilterHigh => "Élevée",
        Msg::PhysicianFilterModerate => "Modérée",
        Msg::PhysicianFilterRoutine => "Routine",
        Msg::PhysicianFilterDrafts => "Brouillons",
        Msg::PhysicianPatientPanel => "Liste des Patients",
        Msg::PhysicianSearchPlaceholder => "Rechercher patient, MRN, motif…",
        Msg::PhysicianNoMatch => "Aucun patient ne correspond à ce filtre.",
        Msg::PhysicianShowPanel => "Afficher la Liste des Patients",
        Msg::PhysicianJumpTo => "Aller à :",
        Msg::PhysicianJumpSummary => "Synthèse",
        Msg::PhysicianJumpQa => "Questions sur le Dossier",
        Msg::PhysicianJumpClinical => "Problèmes et Traitements",
        Msg::PhysicianJumpReply => "Brouillon de Réponse",
        Msg::PhysicianJumpRecords => "Dossiers Sources",
        Msg::PhysicianQaEyebrow => "Questions-Réponses Fondées sur le Dossier",
        Msg::PhysicianQaHeading => "Interroger ce dossier",
        Msg::PhysicianAskPlaceholder => "Posez une question clinique fondée sur ce dossier…",
        Msg::PhysicianAskButton => "Interroger le Dossier",
        Msg::PhysicianSynthesizing => "Synthèse en cours…",
        Msg::PhysicianProblems => "Problèmes Actifs",
        Msg::PhysicianMedications => "Médicaments",
        Msg::PhysicianFollowUps => "Suivis",
        Msg::PhysicianSourceRecords => "Dossiers Sources Longitudinaux",
        Msg::PhysicianBackToQa => "Retour aux Questions ↑",
        Msg::PhysicianClearFocus => "Effacer",

        // Patient Companion
        Msg::PatientEyebrow => "Curie Mobile",
        Msg::PatientTitle => "Démo Expérience Patient",
        Msg::PatientChooseProfile => "Choisissez un Profil Fictif d'Exemple",
        Msg::PatientResetSample => "Réinitialiser l'exemple",
        Msg::PatientTabYou => "Votre Santé (Vous)",
        Msg::PatientTabDoctor => "Pour Mon Médecin",
        Msg::PatientJumpSummary => "Synthèse",
        Msg::PatientJumpAsk => "Demander à Curie",
        Msg::PatientJumpRecords => "Dossiers de Santé",
        Msg::PatientJumpNotes => "Notes Privées",
        Msg::PatientFindRecoveryLink => "Trouver des Soins de Rétablissement",
        Msg::PatientMedicationsHeading => "Médicaments",
        Msg::PatientFocusAreasHeading => "Points d'Attention",
        Msg::PatientAskHeading => "Interrogez Curie sur vos dossiers",
        Msg::PatientAskPlaceholder => "Posez une question en langage simple…",
        Msg::PatientExplainButton => "Expliquer d'après Mes Dossiers",
        Msg::PatientRecordsHeading => "Dossiers de Santé",
        Msg::PatientPrivateNotesHeading => "Notes Privées (Jamais Partagées)",
        Msg::PatientAlwaysExcluded => "Toujours exclues du partage",
        Msg::PatientReviewForDoctor => "Préparer pour Mon Médecin",
        Msg::PatientIncludeForDoctor => "Inclure pour mon médecin",
        Msg::PatientIncludedForDoctor => "Inclus pour le médecin",

        // Mental Health
        Msg::MhEyebrow => "Curie Mobile · Santé Métabolique et Tous les CGM Commerciaux",
        Msg::MhTitle => "Santé Mentale et Suivi Complet de la Glycémie",
        Msg::MhOpenMobile => "Ouvrir Curie Mobile →",
        Msg::MhOpenClinic => "Ouvrir Curie Clinic →",
        Msg::MhResetSample => "Réinitialiser l'exemple",
        Msg::MhTabOverview => "Aperçu et Graphiques",
        Msg::MhTabLogImport => "Journal et Import",
        Msg::MhTabAlertsAi => "Alertes et IA",
        Msg::MhTabCareBrief => "Soins et Synthèse",
        Msg::MhTabRecovery => "Soins de Rétablissement",
        Msg::MhTabAll => "Toutes les Sections",
        Msg::MhUploadCgmCta => "Importer votre fichier CGM ↓",
        Msg::MhLogCheckInHeading => "Enregistrer un Bilan de Santé Mentale",
        Msg::MhAddCheckIn => "Ajouter le Bilan",
        Msg::MhCgmImportHeading => "Import de Fichiers CGM et Glycémie",
        Msg::MhParseValidateExport => "Analyser et Valider l'Export",
        Msg::MhCorrelationLogHeading => "Journal de Corrélation Glycémie–Humeur",
        Msg::MhDownloadCsv => "Télécharger le CSV",

        // Recovery Search
        Msg::RecoveryEyebrow => "Recherche 100 % sur l'Appareil · SAMHSA et Annuaires Publics",
        Msg::RecoveryTitle => "Trouver des Soins de Santé Mentale et de Rétablissement",
        Msg::RecoveryCrisisBannerTitle => "Besoin d'aide immédiate ? Appelez ou écrivez au 988",
        Msg::RecoveryCrisis988Detail => {
            "Soutien de crise gratuit, confidentiel et disponible 24 h/24 aux États-Unis."
        }
        Msg::RecoveryCrisisLanguageNote => {
            "Le 988 propose une ligne directe en espagnol (tapez 2 ou envoyez AYUDA) et un service d'interprétation dans plus de 240 langues."
        }
        Msg::RecoveryStateLabel => "État ou Territoire",
        Msg::RecoverySearchPlaceholder => "Rechercher ville, programme, spécialité ou code postal…",
        Msg::RecoveryUseLocation => "Utiliser ma position",
        Msg::RecoveryLocating => "Localisation…",
        Msg::RecoveryClearFilters => "Effacer les filtres",
        Msg::RecoveryMoreFilters => "Plus de filtres",
        Msg::RecoveryCareFocus => "Domaine de Soins",
        Msg::RecoverySortName => "Trier : Nom",
        Msg::RecoverySortDistance => "Trier : Distance",
        Msg::RecoverySaveCareTeam => "+ Équipe de Soins",
        Msg::RecoverySavedCareTeam => "Enregistré dans l'Équipe de Soins",
        Msg::RecoveryCallNow => "Appeler",
        Msg::RecoveryDirections => "Itinéraire ↗",
        Msg::RecoveryWebsite => "Site web ↗",
        Msg::RecoveryPrevPage => "← Précédent",
        Msg::RecoveryNextPage => "Suivant →",

        // Dashboard Layout Customizer
        Msg::LayoutButtonPrefix => "Disposition",
        Msg::LayoutSavedOnDevice => "Enregistré sur Cet Appareil",
        Msg::LayoutReset => "Réinitialiser",
        Msg::LayoutDone => "Terminé",
        Msg::LayoutActiveBadge => "Actif",
        Msg::LayoutCustomLabel => "Personnalisé",
        Msg::LayoutDensityCompact => "Compacte",
        Msg::LayoutDensityBalanced => "Équilibrée",
        Msg::LayoutDensitySpacious => "Aérée",
        Msg::LayoutPanelLeft => "Panneau à gauche",
        Msg::LayoutPanelRight => "Panneau à droite",
        Msg::LayoutPanelFullWidth => "Dossier pleine largeur",
        Msg::LayoutQaFirst => "Questions en premier",
        Msg::LayoutClinicalFirst => "Problèmes et médicaments d'abord",
        Msg::LayoutAskFirst => "Demander à Curie d'abord",
        Msg::LayoutRecordsFirst => "Dossiers de santé d'abord",
        Msg::LayoutPresetFocusedTriage => "Tri Ciblé",
        Msg::LayoutPresetChartFirstSplit => "Vue Scindée Dossier d'Abord",
        Msg::LayoutPresetCalmReading => "Lecture Sereine",
        Msg::LayoutPresetCalmCompanion => "Compagnon Serein",
        Msg::LayoutPresetRecordsFirstCompact => "Dossiers d'Abord Compact",
        Msg::LayoutPresetVisitPrepFocus => "Préparation de Consultation",
        Msg::LayoutPresetGuidedTabs => "Onglets Guidés",
        Msg::LayoutPresetDailyCheckIn => "Bilan Quotidien et Soins",
        Msg::LayoutPresetFullAnalyticsWall => "Mur Analytique Complet",

        // Chart Citations & Provenance
        Msg::CitationJumpToSource => "Aller au dossier source ↓",
        Msg::CitationClickToJump => "Cliquer pour aller au dossier source ↓",
        Msg::CitationFocusedBadge => "Ciblé depuis la citation",
        Msg::CitationCitedRecordsHeading => "Dossiers Sources Cités",
        Msg::CitationSourceLanguageNote => "Langue d'origine du dossier : anglais (texte intégral)",
    }
}

const fn translate_pt(key: Msg) -> &'static str {
    match key {
        // Nav & Chrome
        Msg::NavAbout => "Sobre",
        Msg::NavExpertise => "Especialidades",
        Msg::NavServices => "Serviços",
        Msg::NavProjects => "Projetos",
        Msg::NavExperience => "Experiência",
        Msg::NavSkills => "Habilidades",
        Msg::NavCommissions => "Encomendas",
        Msg::NavTools => "Ferramentas",
        Msg::NavHireMe => "Contrate-me",
        Msg::NavOpenMenu => "Abrir menu",
        Msg::NavCloseMenu => "Fechar menu",
        Msg::LanguageLabel => "Idioma",
        Msg::SkipToContent => "Pular para o conteúdo",
        Msg::FooterPrivacy => "Privacidade",
        Msg::FooterAllRights => "Todos os direitos reservados",
        Msg::FooterBuiltWith => "Feito com Rust, Leptos e Tauri",
        Msg::FooterCommunityPolicy => "Política da comunidade",
        Msg::FooterCommunityActivity => "Atividade da comunidade",
        Msg::FooterCreators => "Criadores",
        Msg::FooterCodeOwnership => "Propriedade do código",
        Msg::ToolMentalHealth => "Saúde Mental e CGM",
        Msg::ToolRecoveryFinder => "Busca de Cuidados de Recuperação",
        Msg::ToolClinicalSuite => "Espaço Clínico",
        Msg::ToolPatientDemo => "Demo da Experiência do Paciente",
        Msg::ToolPhysicianDemo => "Demo do Espaço Médico",
        Msg::ToolPortfolio => "Calculadora de Conta de Investimento",
        Msg::ToolAndroidApp => "App Android (F-Droid / APK)",
        Msg::ToolDuxAncestry => "Dux Ancestralidade e Genética",
        Msg::ToolOwnership => "Relatório de Propriedade do Código",

        // Clinical Suite Switcher & Overview
        Msg::SuiteAriaLabel => "Suíte clínica de demonstração",
        Msg::SuitePatient => "Curie Mobile (Paciente)",
        Msg::SuitePhysician => "Curie Clinic (Médico)",
        Msg::SuiteMentalHealth => "Saúde Mental e CGM",
        Msg::SuiteRecoveryFinder => "Busca de Recuperação",
        Msg::SuiteOverview => "Visão Geral da Suíte",
        Msg::ZeroPhiBannerTitle => "Garantia Sintética Sem PHI",
        Msg::ZeroPhiBannerBadge => "Demo Sem PHI",
        Msg::OnDeviceOnlyBadge => "Somente no dispositivo",
        Msg::OfflineReadyBadge => "Pronto offline",
        Msg::SyntheticDemoNotice => {
            "Demo 100% sintética · Sem PHI · Não insira informações médicas reais"
        }

        // Physician Workspace
        Msg::PhysicianEyebrow => "Curie Clinic",
        Msg::PhysicianTitle => "Demo do Espaço Médico",
        Msg::PhysicianTodayQueue => "Fila Clínica de Hoje",
        Msg::PhysicianFilterAll => "Todos",
        Msg::PhysicianFilterHigh => "Alta",
        Msg::PhysicianFilterModerate => "Moderada",
        Msg::PhysicianFilterRoutine => "Rotina",
        Msg::PhysicianFilterDrafts => "Rascunhos",
        Msg::PhysicianPatientPanel => "Painel de Pacientes",
        Msg::PhysicianSearchPlaceholder => "Buscar paciente, MRN, foco…",
        Msg::PhysicianNoMatch => "Nenhum paciente corresponde a este filtro.",
        Msg::PhysicianShowPanel => "Mostrar Painel de Pacientes",
        Msg::PhysicianJumpTo => "Ir para:",
        Msg::PhysicianJumpSummary => "Resumo",
        Msg::PhysicianJumpQa => "Perguntas do Prontuário",
        Msg::PhysicianJumpClinical => "Problemas e Medicamentos",
        Msg::PhysicianJumpReply => "Rascunho de Resposta",
        Msg::PhysicianJumpRecords => "Registros de Origem",
        Msg::PhysicianQaEyebrow => "Perguntas Fundamentadas no Prontuário",
        Msg::PhysicianQaHeading => "Perguntar sobre este prontuário",
        Msg::PhysicianAskPlaceholder => "Faça uma pergunta clínica baseada neste prontuário…",
        Msg::PhysicianAskButton => "Consultar Prontuário",
        Msg::PhysicianSynthesizing => "Sintetizando…",
        Msg::PhysicianProblems => "Problemas Ativos",
        Msg::PhysicianMedications => "Medicamentos",
        Msg::PhysicianFollowUps => "Acompanhamentos",
        Msg::PhysicianSourceRecords => "Registros de Origem Longitudinais",
        Msg::PhysicianBackToQa => "Voltar às Perguntas ↑",
        Msg::PhysicianClearFocus => "Limpar",

        // Patient Companion
        Msg::PatientEyebrow => "Curie Mobile",
        Msg::PatientTitle => "Demo da Experiência do Paciente",
        Msg::PatientChooseProfile => "Escolha um Perfil Fictício de Exemplo",
        Msg::PatientResetSample => "Restaurar exemplo",
        Msg::PatientTabYou => "Sua Saúde (Você)",
        Msg::PatientTabDoctor => "Para Meu Médico",
        Msg::PatientJumpSummary => "Resumo",
        Msg::PatientJumpAsk => "Perguntar ao Curie",
        Msg::PatientJumpRecords => "Registros de Saúde",
        Msg::PatientJumpNotes => "Notas Privadas",
        Msg::PatientFindRecoveryLink => "Buscar Cuidados de Recuperação",
        Msg::PatientMedicationsHeading => "Medicamentos",
        Msg::PatientFocusAreasHeading => "Áreas de Atenção",
        Msg::PatientAskHeading => "Pergunte ao Curie sobre seus registros",
        Msg::PatientAskPlaceholder => "Faça uma pergunta em linguagem simples…",
        Msg::PatientExplainButton => "Explicar pelos Meus Registros",
        Msg::PatientRecordsHeading => "Registros de Saúde",
        Msg::PatientPrivateNotesHeading => "Notas Privadas (Nunca Compartilhadas)",
        Msg::PatientAlwaysExcluded => "Sempre excluídas do compartilhamento",
        Msg::PatientReviewForDoctor => "Revisar para Meu Médico",
        Msg::PatientIncludeForDoctor => "Incluir para meu médico",
        Msg::PatientIncludedForDoctor => "Incluído para o médico",

        // Mental Health
        Msg::MhEyebrow => "Curie Mobile · Saúde Metabólica e Todos os CGMs Comerciais",
        Msg::MhTitle => "Saúde Mental e Acompanhamento Completo de Glicose",
        Msg::MhOpenMobile => "Abrir Curie Mobile →",
        Msg::MhOpenClinic => "Abrir Curie Clinic →",
        Msg::MhResetSample => "Restaurar exemplo",
        Msg::MhTabOverview => "Visão Geral e Gráficos",
        Msg::MhTabLogImport => "Registro e Importação",
        Msg::MhTabAlertsAi => "Alertas e IA",
        Msg::MhTabCareBrief => "Cuidados e Resumo",
        Msg::MhTabRecovery => "Buscar Recuperação",
        Msg::MhTabAll => "Todas as Seções",
        Msg::MhUploadCgmCta => "Enviar seu arquivo CGM ↓",
        Msg::MhLogCheckInHeading => "Registrar Check-In de Saúde Mental",
        Msg::MhAddCheckIn => "Adicionar Check-In",
        Msg::MhCgmImportHeading => "Importação de Arquivos CGM e Glicose",
        Msg::MhParseValidateExport => "Analisar e Validar Exportação",
        Msg::MhCorrelationLogHeading => "Registro de Correlação Glicose–Humor",
        Msg::MhDownloadCsv => "Baixar CSV",

        // Recovery Search
        Msg::RecoveryEyebrow => "Busca 100% no Dispositivo · SAMHSA e Diretórios Públicos",
        Msg::RecoveryTitle => "Encontre Cuidados de Saúde Mental e Recuperação Perto de Você",
        Msg::RecoveryCrisisBannerTitle => "Precisa de ajuda imediata? Ligue ou mande SMS para 988",
        Msg::RecoveryCrisis988Detail => {
            "Apoio gratuito, confidencial e disponível 24/7 em todos os Estados Unidos."
        }
        Msg::RecoveryCrisisLanguageNote => {
            "O 988 oferece atendimento direto em espanhol (digite 2 ou envie AYUDA) e interpretação em mais de 240 idiomas."
        }
        Msg::RecoveryStateLabel => "Estado ou Território",
        Msg::RecoverySearchPlaceholder => "Buscar cidade, programa, especialidade ou CEP…",
        Msg::RecoveryUseLocation => "Usar minha localização",
        Msg::RecoveryLocating => "Localizando…",
        Msg::RecoveryClearFilters => "Limpar filtros",
        Msg::RecoveryMoreFilters => "Mais filtros",
        Msg::RecoveryCareFocus => "Foco de Cuidado",
        Msg::RecoverySortName => "Ordenar: Nome",
        Msg::RecoverySortDistance => "Ordenar: Distância",
        Msg::RecoverySaveCareTeam => "+ Equipe de Cuidados",
        Msg::RecoverySavedCareTeam => "Salvo na Equipe de Cuidados",
        Msg::RecoveryCallNow => "Ligar",
        Msg::RecoveryDirections => "Rotas ↗",
        Msg::RecoveryWebsite => "Site ↗",
        Msg::RecoveryPrevPage => "← Anterior",
        Msg::RecoveryNextPage => "Próxima →",

        // Dashboard Layout Customizer
        Msg::LayoutButtonPrefix => "Layout",
        Msg::LayoutSavedOnDevice => "Salvo Neste Dispositivo",
        Msg::LayoutReset => "Redefinir",
        Msg::LayoutDone => "Concluir",
        Msg::LayoutActiveBadge => "Ativo",
        Msg::LayoutCustomLabel => "Personalizado",
        Msg::LayoutDensityCompact => "Compacto",
        Msg::LayoutDensityBalanced => "Equilibrado",
        Msg::LayoutDensitySpacious => "Espaçoso",
        Msg::LayoutPanelLeft => "Painel à esquerda",
        Msg::LayoutPanelRight => "Painel à direita",
        Msg::LayoutPanelFullWidth => "Prontuário em largura total",
        Msg::LayoutQaFirst => "Perguntas primeiro",
        Msg::LayoutClinicalFirst => "Problemas e medicamentos primeiro",
        Msg::LayoutAskFirst => "Perguntar ao Curie primeiro",
        Msg::LayoutRecordsFirst => "Registros de saúde primeiro",
        Msg::LayoutPresetFocusedTriage => "Triagem Focada",
        Msg::LayoutPresetChartFirstSplit => "Prontuário Dividido Primeiro",
        Msg::LayoutPresetCalmReading => "Leitura Tranquila",
        Msg::LayoutPresetCalmCompanion => "Companheiro Calmo",
        Msg::LayoutPresetRecordsFirstCompact => "Registros Primeiro Compacto",
        Msg::LayoutPresetVisitPrepFocus => "Foco na Preparação da Consulta",
        Msg::LayoutPresetGuidedTabs => "Abas Guiadas",
        Msg::LayoutPresetDailyCheckIn => "Check-In Diário e Cuidado",
        Msg::LayoutPresetFullAnalyticsWall => "Painel Analítico Completo",

        // Chart Citations & Provenance
        Msg::CitationJumpToSource => "Ir para o registro de origem ↓",
        Msg::CitationClickToJump => "Clique para ir ao registro de origem ↓",
        Msg::CitationFocusedBadge => "Destacado pela citação",
        Msg::CitationCitedRecordsHeading => "Registros de Origem Citados",
        Msg::CitationSourceLanguageNote => "Idioma original do registro: inglês (texto literal)",
    }
}

const fn translate_de(key: Msg) -> &'static str {
    match key {
        // Nav & Chrome
        Msg::NavAbout => "Über mich",
        Msg::NavExpertise => "Fachgebiete",
        Msg::NavServices => "Leistungen",
        Msg::NavProjects => "Projekte",
        Msg::NavExperience => "Erfahrung",
        Msg::NavSkills => "Kompetenzen",
        Msg::NavCommissions => "Aufträge",
        Msg::NavTools => "Werkzeuge",
        Msg::NavHireMe => "Kontakt",
        Msg::NavOpenMenu => "Menü öffnen",
        Msg::NavCloseMenu => "Menü schließen",
        Msg::LanguageLabel => "Sprache",
        Msg::SkipToContent => "Zum Inhalt springen",
        Msg::FooterPrivacy => "Datenschutz",
        Msg::FooterAllRights => "Alle Rechte vorbehalten",
        Msg::FooterBuiltWith => "Entwickelt mit Rust, Leptos & Tauri",
        Msg::FooterCommunityPolicy => "Community-Richtlinien",
        Msg::FooterCommunityActivity => "Community-Aktivität",
        Msg::FooterCreators => "Mitwirkende",
        Msg::FooterCodeOwnership => "Code-Eigentümerschaft",
        Msg::ToolMentalHealth => "Psychische Gesundheit & CGM",
        Msg::ToolRecoveryFinder => "Therapie- & Hilfesuche",
        Msg::ToolClinicalSuite => "Klinischer Arbeitsbereich",
        Msg::ToolPatientDemo => "Patientenerlebnis-Demo",
        Msg::ToolPhysicianDemo => "Arzt-Arbeitsbereich-Demo",
        Msg::ToolPortfolio => "Anlagekonto-Rechner",
        Msg::ToolAndroidApp => "Android-App (F-Droid / APK)",
        Msg::ToolDuxAncestry => "Dux Abstammung & Genetik",
        Msg::ToolOwnership => "Code-Eigentümerschaftsbericht",

        // Clinical Suite Switcher & Overview
        Msg::SuiteAriaLabel => "Klinische Demo-Suite",
        Msg::SuitePatient => "Curie Mobile (Patient)",
        Msg::SuitePhysician => "Curie Clinic (Arzt)",
        Msg::SuiteMentalHealth => "Psychische Gesundheit & CGM",
        Msg::SuiteRecoveryFinder => "Therapie- & Hilfesuche",
        Msg::SuiteOverview => "Suite-Übersicht",
        Msg::ZeroPhiBannerTitle => "Synthetische Zero-PHI-Garantie",
        Msg::ZeroPhiBannerBadge => "Zero-PHI-Demo",
        Msg::OnDeviceOnlyBadge => "Nur auf dem Gerät",
        Msg::OfflineReadyBadge => "Offline-bereit",
        Msg::SyntheticDemoNotice => {
            "100 % synthetische Demo · Keine PHI · Bitte keine echten Gesundheitsdaten eingeben"
        }

        // Physician Workspace
        Msg::PhysicianEyebrow => "Curie Clinic",
        Msg::PhysicianTitle => "Arzt-Arbeitsbereich-Demo",
        Msg::PhysicianTodayQueue => "Heutige Klinik-Warteliste",
        Msg::PhysicianFilterAll => "Alle",
        Msg::PhysicianFilterHigh => "Hoch",
        Msg::PhysicianFilterModerate => "Mittel",
        Msg::PhysicianFilterRoutine => "Routine",
        Msg::PhysicianFilterDrafts => "Entwürfe",
        Msg::PhysicianPatientPanel => "Patientenliste",
        Msg::PhysicianSearchPlaceholder => "Patient, MRN, Schwerpunkt suchen…",
        Msg::PhysicianNoMatch => "Keine Patienten entsprechen diesem Filter.",
        Msg::PhysicianShowPanel => "Patientenliste einblenden",
        Msg::PhysicianJumpTo => "Springen zu:",
        Msg::PhysicianJumpSummary => "Übersicht",
        Msg::PhysicianJumpQa => "Akten-Q&A",
        Msg::PhysicianJumpClinical => "Diagnosen & Medikation",
        Msg::PhysicianJumpReply => "Antwortentwurf",
        Msg::PhysicianJumpRecords => "Quellakten",
        Msg::PhysicianQaEyebrow => "Belegbasierte Akten-Q&A",
        Msg::PhysicianQaHeading => "Diese Akte befragen",
        Msg::PhysicianAskPlaceholder => "Klinische Frage auf Basis dieser Akte stellen…",
        Msg::PhysicianAskButton => "Akte fragen",
        Msg::PhysicianSynthesizing => "Wird ausgewertet…",
        Msg::PhysicianProblems => "Aktive Diagnosen",
        Msg::PhysicianMedications => "Medikation",
        Msg::PhysicianFollowUps => "Nachsorge",
        Msg::PhysicianSourceRecords => "Longitudinale Quellakten",
        Msg::PhysicianBackToQa => "Zurück zu Q&A ↑",
        Msg::PhysicianClearFocus => "Zurücksetzen",

        // Patient Companion
        Msg::PatientEyebrow => "Curie Mobile",
        Msg::PatientTitle => "Patientenerlebnis-Demo",
        Msg::PatientChooseProfile => "Fiktives Beispielprofil wählen",
        Msg::PatientResetSample => "Beispieldaten zurücksetzen",
        Msg::PatientTabYou => "Deine Gesundheit (Du)",
        Msg::PatientTabDoctor => "Für Meine Ärztin / Meinen Arzt",
        Msg::PatientJumpSummary => "Übersicht",
        Msg::PatientJumpAsk => "Curie fragen",
        Msg::PatientJumpRecords => "Gesundheitsakten",
        Msg::PatientJumpNotes => "Private Notizen",
        Msg::PatientFindRecoveryLink => "Therapie & Unterstützung finden",
        Msg::PatientMedicationsHeading => "Medikation",
        Msg::PatientFocusAreasHeading => "Schwerpunkte",
        Msg::PatientAskHeading => "Curie zu deinen Unterlagen fragen",
        Msg::PatientAskPlaceholder => "Stelle eine Frage in einfacher Sprache…",
        Msg::PatientExplainButton => "Aus meinen Akten erklären",
        Msg::PatientRecordsHeading => "Gesundheitsakten",
        Msg::PatientPrivateNotesHeading => "Private Notizen (Nie geteilt)",
        Msg::PatientAlwaysExcluded => "Immer vom Teilen ausgeschlossen",
        Msg::PatientReviewForDoctor => "Für den Arzttermin prüfen",
        Msg::PatientIncludeForDoctor => "Für den Arzt einschließen",
        Msg::PatientIncludedForDoctor => "Für den Arzt eingeschlossen",

        // Mental Health
        Msg::MhEyebrow => "Curie Mobile · Stoffwechselgesundheit & alle kommerziellen CGMs",
        Msg::MhTitle => "Psychische Gesundheit & umfassende Glukoseverfolgung",
        Msg::MhOpenMobile => "Curie Mobile öffnen →",
        Msg::MhOpenClinic => "Curie Clinic öffnen →",
        Msg::MhResetSample => "Beispiel zurücksetzen",
        Msg::MhTabOverview => "Übersicht & Diagramme",
        Msg::MhTabLogImport => "Eintrag & Import",
        Msg::MhTabAlertsAi => "Warnungen & KI",
        Msg::MhTabCareBrief => "Versorgung & Bericht",
        Msg::MhTabRecovery => "Hilfe & Therapie finden",
        Msg::MhTabAll => "Alle Bereiche",
        Msg::MhUploadCgmCta => "CGM-Datei hochladen ↓",
        Msg::MhLogCheckInHeading => "Check-In zur psychischen Gesundheit erfassen",
        Msg::MhAddCheckIn => "Check-In hinzufügen",
        Msg::MhCgmImportHeading => "Import kommerzieller CGM- & Glukosedateien",
        Msg::MhParseValidateExport => "Export prüfen & einlesen",
        Msg::MhCorrelationLogHeading => "Glukose–Stimmung-Korrelationsprotokoll",
        Msg::MhDownloadCsv => "CSV herunterladen",

        // Recovery Search
        Msg::RecoveryEyebrow => "100 % lokale Suche · SAMHSA & öffentliche Verzeichnisse",
        Msg::RecoveryTitle => "Psychische Gesundheits- & Suchthilfe in deiner Nähe finden",
        Msg::RecoveryCrisisBannerTitle => "Sofortige Hilfe nötig? 988 anrufen oder SMS senden",
        Msg::RecoveryCrisis988Detail => {
            "Kostenlose, vertrauliche Krisenunterstützung rund um die Uhr in den USA."
        }
        Msg::RecoveryCrisisLanguageNote => {
            "988 bietet direkte Hilfe auf Spanisch (Taste 2 oder SMS AYUDA) sowie Dolmetschen in über 240 Sprachen."
        }
        Msg::RecoveryStateLabel => "Bundesstaat oder Territorium",
        Msg::RecoverySearchPlaceholder => "Stadt, Programm, Fachgebiet oder PLZ suchen…",
        Msg::RecoveryUseLocation => "Meinen Standort verwenden",
        Msg::RecoveryLocating => "Standort wird ermittelt…",
        Msg::RecoveryClearFilters => "Filter zurücksetzen",
        Msg::RecoveryMoreFilters => "Weitere Filter",
        Msg::RecoveryCareFocus => "Behandlungsschwerpunkt",
        Msg::RecoverySortName => "Sortierung: Name",
        Msg::RecoverySortDistance => "Sortierung: Entfernung",
        Msg::RecoverySaveCareTeam => "+ Betreuungsteam",
        Msg::RecoverySavedCareTeam => "Im Betreuungsteam gespeichert",
        Msg::RecoveryCallNow => "Anrufen",
        Msg::RecoveryDirections => "Route ↗",
        Msg::RecoveryWebsite => "Website ↗",
        Msg::RecoveryPrevPage => "← Zurück",
        Msg::RecoveryNextPage => "Weiter →",

        // Dashboard Layout Customizer
        Msg::LayoutButtonPrefix => "Layout",
        Msg::LayoutSavedOnDevice => "Auf diesem Gerät gespeichert",
        Msg::LayoutReset => "Zurücksetzen",
        Msg::LayoutDone => "Fertig",
        Msg::LayoutActiveBadge => "Aktiv",
        Msg::LayoutCustomLabel => "Benutzerdefiniert",
        Msg::LayoutDensityCompact => "Kompakt",
        Msg::LayoutDensityBalanced => "Ausgewogen",
        Msg::LayoutDensitySpacious => "Großzügig",
        Msg::LayoutPanelLeft => "Liste links",
        Msg::LayoutPanelRight => "Liste rechts",
        Msg::LayoutPanelFullWidth => "Akte in voller Breite",
        Msg::LayoutQaFirst => "Akten-Q&A zuerst",
        Msg::LayoutClinicalFirst => "Diagnosen & Medikation zuerst",
        Msg::LayoutAskFirst => "Curie fragen zuerst",
        Msg::LayoutRecordsFirst => "Gesundheitsakten zuerst",
        Msg::LayoutPresetFocusedTriage => "Fokussierte Triage",
        Msg::LayoutPresetChartFirstSplit => "Geteilte Aktenansicht",
        Msg::LayoutPresetCalmReading => "Ruhiges Lesen",
        Msg::LayoutPresetCalmCompanion => "Ruhiger Begleiter",
        Msg::LayoutPresetRecordsFirstCompact => "Akten zuerst kompakt",
        Msg::LayoutPresetVisitPrepFocus => "Fokus Arzttermin",
        Msg::LayoutPresetGuidedTabs => "Geführte Tabs",
        Msg::LayoutPresetDailyCheckIn => "Täglicher Check-In & Hilfe",
        Msg::LayoutPresetFullAnalyticsWall => "Vollständige Analyseansicht",

        // Chart Citations & Provenance
        Msg::CitationJumpToSource => "Zur Quellakte springen ↓",
        Msg::CitationClickToJump => "Klicken, um zur Quellakte zu springen ↓",
        Msg::CitationFocusedBadge => "Über Zitat fokussiert",
        Msg::CitationCitedRecordsHeading => "Zitierte Quellakten",
        Msg::CitationSourceLanguageNote => "Originalsprache der Akte: Englisch (wortgetreu)",
    }
}

const fn translate_zh_hans(key: Msg) -> &'static str {
    match key {
        // Nav & Chrome
        Msg::NavAbout => "关于",
        Msg::NavExpertise => "专长",
        Msg::NavServices => "服务",
        Msg::NavProjects => "项目",
        Msg::NavExperience => "经历",
        Msg::NavSkills => "技能",
        Msg::NavCommissions => "委托",
        Msg::NavTools => "工具",
        Msg::NavHireMe => "联系合作",
        Msg::NavOpenMenu => "打开菜单",
        Msg::NavCloseMenu => "关闭菜单",
        Msg::LanguageLabel => "语言",
        Msg::SkipToContent => "跳至正文",
        Msg::FooterPrivacy => "隐私政策",
        Msg::FooterAllRights => "保留所有权利",
        Msg::FooterBuiltWith => "基于 Rust、Leptos 与 Tauri 构建",
        Msg::FooterCommunityPolicy => "社区准则",
        Msg::FooterCommunityActivity => "社区动态",
        Msg::FooterCreators => "创作者",
        Msg::FooterCodeOwnership => "代码归属",
        Msg::ToolMentalHealth => "心理健康与 CGM",
        Msg::ToolRecoveryFinder => "康复与心理照护检索",
        Msg::ToolClinicalSuite => "临床工作台",
        Msg::ToolPatientDemo => "患者体验演示",
        Msg::ToolPhysicianDemo => "医生工作台演示",
        Msg::ToolPortfolio => "投资账户计算器",
        Msg::ToolAndroidApp => "Android 应用（F-Droid / APK）",
        Msg::ToolDuxAncestry => "Dux 祖源与遗传分析",
        Msg::ToolOwnership => "代码归属报告",

        // Clinical Suite Switcher & Overview
        Msg::SuiteAriaLabel => "临床演示套件",
        Msg::SuitePatient => "Curie Mobile（患者端）",
        Msg::SuitePhysician => "Curie Clinic（医生端）",
        Msg::SuiteMentalHealth => "心理健康与 CGM",
        Msg::SuiteRecoveryFinder => "康复照护检索",
        Msg::SuiteOverview => "套件总览",
        Msg::ZeroPhiBannerTitle => "合成零受保护健康信息（Zero-PHI）保障",
        Msg::ZeroPhiBannerBadge => "Zero-PHI 演示",
        Msg::OnDeviceOnlyBadge => "仅限本机",
        Msg::OfflineReadyBadge => "支持离线",
        Msg::SyntheticDemoNotice => "100% 合成演示 · 零 PHI · 请勿输入真实健康信息",

        // Physician Workspace
        Msg::PhysicianEyebrow => "Curie Clinic",
        Msg::PhysicianTitle => "医生工作台演示",
        Msg::PhysicianTodayQueue => "今日门诊队列",
        Msg::PhysicianFilterAll => "全部",
        Msg::PhysicianFilterHigh => "高优先级",
        Msg::PhysicianFilterModerate => "中优先级",
        Msg::PhysicianFilterRoutine => "常规",
        Msg::PhysicianFilterDrafts => "有草稿",
        Msg::PhysicianPatientPanel => "患者列表",
        Msg::PhysicianSearchPlaceholder => "搜索患者、病历号、重点问题…",
        Msg::PhysicianNoMatch => "没有符合该筛选条件的患者。",
        Msg::PhysicianShowPanel => "显示患者列表",
        Msg::PhysicianJumpTo => "跳转至：",
        Msg::PhysicianJumpSummary => "摘要",
        Msg::PhysicianJumpQa => "病历问答",
        Msg::PhysicianJumpClinical => "问题与用药",
        Msg::PhysicianJumpReply => "回复草稿",
        Msg::PhysicianJumpRecords => "原始记录",
        Msg::PhysicianQaEyebrow => "有据可循的病历问答",
        Msg::PhysicianQaHeading => "询问此病历",
        Msg::PhysicianAskPlaceholder => "输入基于此病历的临床问题…",
        Msg::PhysicianAskButton => "查询病历",
        Msg::PhysicianSynthesizing => "正在整理…",
        Msg::PhysicianProblems => "现存问题",
        Msg::PhysicianMedications => "当前用药",
        Msg::PhysicianFollowUps => "随访计划",
        Msg::PhysicianSourceRecords => "纵向原始病历记录",
        Msg::PhysicianBackToQa => "返回问答 ↑",
        Msg::PhysicianClearFocus => "清除",

        // Patient Companion
        Msg::PatientEyebrow => "Curie Mobile",
        Msg::PatientTitle => "患者体验演示",
        Msg::PatientChooseProfile => "选择虚构示例档案",
        Msg::PatientResetSample => "重置示例状态",
        Msg::PatientTabYou => "你的健康（本人）",
        Msg::PatientTabDoctor => "给我的医生",
        Msg::PatientJumpSummary => "摘要",
        Msg::PatientJumpAsk => "询问 Curie",
        Msg::PatientJumpRecords => "健康记录",
        Msg::PatientJumpNotes => "私密笔记",
        Msg::PatientFindRecoveryLink => "查找康复与心理照护",
        Msg::PatientMedicationsHeading => "用药清单",
        Msg::PatientFocusAreasHeading => "关注重点",
        Msg::PatientAskHeading => "向 Curie 咨询你的健康记录",
        Msg::PatientAskPlaceholder => "用通俗语言提出问题…",
        Msg::PatientExplainButton => "根据我的记录解释",
        Msg::PatientRecordsHeading => "健康记录",
        Msg::PatientPrivateNotesHeading => "私密笔记（永不共享）",
        Msg::PatientAlwaysExcluded => "共享时始终排除",
        Msg::PatientReviewForDoctor => "整理给医生查看",
        Msg::PatientIncludeForDoctor => "纳入给医生查看",
        Msg::PatientIncludedForDoctor => "已纳入给医生",

        // Mental Health
        Msg::MhEyebrow => "Curie Mobile · 代谢健康与主流商用 CGM",
        Msg::MhTitle => "心理健康与全面血糖追踪",
        Msg::MhOpenMobile => "打开 Curie Mobile →",
        Msg::MhOpenClinic => "打开 Curie Clinic →",
        Msg::MhResetSample => "重置示例",
        Msg::MhTabOverview => "总览与图表",
        Msg::MhTabLogImport => "记录与导入",
        Msg::MhTabAlertsAi => "提醒与 AI",
        Msg::MhTabCareBrief => "照护与摘要",
        Msg::MhTabRecovery => "查找康复照护",
        Msg::MhTabAll => "全部板块",
        Msg::MhUploadCgmCta => "上传你的 CGM 文件 ↓",
        Msg::MhLogCheckInHeading => "记录心理健康打卡",
        Msg::MhAddCheckIn => "添加打卡",
        Msg::MhCgmImportHeading => "商用 CGM 与血糖文件导入",
        Msg::MhParseValidateExport => "解析并校验导出文件",
        Msg::MhCorrelationLogHeading => "血糖–情绪关联日志",
        Msg::MhDownloadCsv => "下载 CSV",

        // Recovery Search
        Msg::RecoveryEyebrow => "100% 本机检索 · SAMHSA 与公开医疗目录",
        Msg::RecoveryTitle => "查找身边的心理健康与康复照护资源",
        Msg::RecoveryCrisisBannerTitle => "需要紧急帮助？请致电或发短信至 988",
        Msg::RecoveryCrisis988Detail => "全美免费、保密、全天候 24/7 危机支持热线。",
        Msg::RecoveryCrisisLanguageNote => {
            "988 提供西班牙语专线支持（按 2 或发短信 AYUDA）以及 240 多种语言的口译服务。"
        }
        Msg::RecoveryStateLabel => "州或领地",
        Msg::RecoverySearchPlaceholder => "搜索城市、项目、专科或邮编…",
        Msg::RecoveryUseLocation => "使用我的位置",
        Msg::RecoveryLocating => "正在定位…",
        Msg::RecoveryClearFilters => "清除筛选",
        Msg::RecoveryMoreFilters => "更多筛选",
        Msg::RecoveryCareFocus => "照护方向",
        Msg::RecoverySortName => "排序：名称",
        Msg::RecoverySortDistance => "排序：距离",
        Msg::RecoverySaveCareTeam => "+ 照护团队",
        Msg::RecoverySavedCareTeam => "已保存至照护团队",
        Msg::RecoveryCallNow => "拨打",
        Msg::RecoveryDirections => "路线导航 ↗",
        Msg::RecoveryWebsite => "官网 ↗",
        Msg::RecoveryPrevPage => "← 上一页",
        Msg::RecoveryNextPage => "下一页 →",

        // Dashboard Layout Customizer
        Msg::LayoutButtonPrefix => "布局",
        Msg::LayoutSavedOnDevice => "已保存在本机",
        Msg::LayoutReset => "重置",
        Msg::LayoutDone => "完成",
        Msg::LayoutActiveBadge => "当前",
        Msg::LayoutCustomLabel => "自定义",
        Msg::LayoutDensityCompact => "紧凑",
        Msg::LayoutDensityBalanced => "均衡",
        Msg::LayoutDensitySpacious => "宽松",
        Msg::LayoutPanelLeft => "列表在左",
        Msg::LayoutPanelRight => "列表在右",
        Msg::LayoutPanelFullWidth => "全宽病历",
        Msg::LayoutQaFirst => "病历问答优先",
        Msg::LayoutClinicalFirst => "问题与用药优先",
        Msg::LayoutAskFirst => "询问 Curie 优先",
        Msg::LayoutRecordsFirst => "健康记录优先",
        Msg::LayoutPresetFocusedTriage => "专注分诊",
        Msg::LayoutPresetChartFirstSplit => "病历优先分栏",
        Msg::LayoutPresetCalmReading => "舒缓阅读",
        Msg::LayoutPresetCalmCompanion => "平和陪伴",
        Msg::LayoutPresetRecordsFirstCompact => "记录优先紧凑版",
        Msg::LayoutPresetVisitPrepFocus => "就诊准备聚焦",
        Msg::LayoutPresetGuidedTabs => "引导标签页",
        Msg::LayoutPresetDailyCheckIn => "每日打卡与照护",
        Msg::LayoutPresetFullAnalyticsWall => "全景分析视图",

        // Chart Citations & Provenance
        Msg::CitationJumpToSource => "跳转至原始记录 ↓",
        Msg::CitationClickToJump => "点击跳转至原始记录 ↓",
        Msg::CitationFocusedBadge => "已由引用定位",
        Msg::CitationCitedRecordsHeading => "引用的原始记录",
        Msg::CitationSourceLanguageNote => "原始病历语言：英语（原文摘录）",
    }
}

const fn translate_ja(key: Msg) -> &'static str {
    match key {
        // Nav & Chrome
        Msg::NavAbout => "プロフィール",
        Msg::NavExpertise => "専門分野",
        Msg::NavServices => "サービス",
        Msg::NavProjects => "プロジェクト",
        Msg::NavExperience => "経歴",
        Msg::NavSkills => "スキル",
        Msg::NavCommissions => "ご依頼",
        Msg::NavTools => "ツール",
        Msg::NavHireMe => "お仕事のご相談",
        Msg::NavOpenMenu => "メニューを開く",
        Msg::NavCloseMenu => "メニューを閉じる",
        Msg::LanguageLabel => "言語",
        Msg::SkipToContent => "本文へ移動",
        Msg::FooterPrivacy => "プライバシー",
        Msg::FooterAllRights => "無断転載を禁じます",
        Msg::FooterBuiltWith => "Rust・Leptos・Tauri で構築",
        Msg::FooterCommunityPolicy => "コミュニティ方針",
        Msg::FooterCommunityActivity => "コミュニティ活動",
        Msg::FooterCreators => "クリエイター",
        Msg::FooterCodeOwnership => "コード所有権",
        Msg::ToolMentalHealth => "メンタルヘルス & CGM",
        Msg::ToolRecoveryFinder => "回復・支援ケア検索",
        Msg::ToolClinicalSuite => "クリニカルワークスペース",
        Msg::ToolPatientDemo => "患者向け体験デモ",
        Msg::ToolPhysicianDemo => "医師向けワークスペースデモ",
        Msg::ToolPortfolio => "投資口座計算ツール",
        Msg::ToolAndroidApp => "Android アプリ（F-Droid / APK）",
        Msg::ToolDuxAncestry => "Dux 祖先・遺伝解析",
        Msg::ToolOwnership => "コード所有権レポート",

        // Clinical Suite Switcher & Overview
        Msg::SuiteAriaLabel => "クリニカルデモスイート",
        Msg::SuitePatient => "Curie Mobile（患者向け）",
        Msg::SuitePhysician => "Curie Clinic（医師向け）",
        Msg::SuiteMentalHealth => "メンタルヘルス & CGM",
        Msg::SuiteRecoveryFinder => "回復・支援ケア検索",
        Msg::SuiteOverview => "スイート概要",
        Msg::ZeroPhiBannerTitle => "合成データ・Zero-PHI 保証",
        Msg::ZeroPhiBannerBadge => "Zero-PHI デモ",
        Msg::OnDeviceOnlyBadge => "端末内処理のみ",
        Msg::OfflineReadyBadge => "オフライン対応",
        Msg::SyntheticDemoNotice => {
            "100% 合成データデモ・PHI ゼロ・実際の健康情報は入力しないでください"
        }

        // Physician Workspace
        Msg::PhysicianEyebrow => "Curie Clinic",
        Msg::PhysicianTitle => "医師向けワークスペースデモ",
        Msg::PhysicianTodayQueue => "本日の診療キュー",
        Msg::PhysicianFilterAll => "すべて",
        Msg::PhysicianFilterHigh => "高優先度",
        Msg::PhysicianFilterModerate => "中優先度",
        Msg::PhysicianFilterRoutine => "通常",
        Msg::PhysicianFilterDrafts => "下書きあり",
        Msg::PhysicianPatientPanel => "患者パネル",
        Msg::PhysicianSearchPlaceholder => "患者名・MRN・主訴で検索…",
        Msg::PhysicianNoMatch => "この条件に一致する患者はいません。",
        Msg::PhysicianShowPanel => "患者パネルを表示",
        Msg::PhysicianJumpTo => "移動：",
        Msg::PhysicianJumpSummary => "サマリー",
        Msg::PhysicianJumpQa => "カルテ Q&A",
        Msg::PhysicianJumpClinical => "課題と処方薬",
        Msg::PhysicianJumpReply => "返信下書き",
        Msg::PhysicianJumpRecords => "元の記録",
        Msg::PhysicianQaEyebrow => "根拠付きカルテ Q&A",
        Msg::PhysicianQaHeading => "このカルテに質問する",
        Msg::PhysicianAskPlaceholder => "このカルテに基づく臨床的な質問を入力…",
        Msg::PhysicianAskButton => "カルテに質問",
        Msg::PhysicianSynthesizing => "要約を作成中…",
        Msg::PhysicianProblems => "現在の課題",
        Msg::PhysicianMedications => "処方薬",
        Msg::PhysicianFollowUps => "フォローアップ",
        Msg::PhysicianSourceRecords => "経時的な元の診療記録",
        Msg::PhysicianBackToQa => "Q&A に戻る ↑",
        Msg::PhysicianClearFocus => "解除",

        // Patient Companion
        Msg::PatientEyebrow => "Curie Mobile",
        Msg::PatientTitle => "患者向け体験デモ",
        Msg::PatientChooseProfile => "架空のサンプルプロフィールを選択",
        Msg::PatientResetSample => "サンプルをリセット",
        Msg::PatientTabYou => "あなたの健康（本人）",
        Msg::PatientTabDoctor => "主治医へ共有",
        Msg::PatientJumpSummary => "サマリー",
        Msg::PatientJumpAsk => "Curie に質問",
        Msg::PatientJumpRecords => "健康記録",
        Msg::PatientJumpNotes => "プライベートメモ",
        Msg::PatientFindRecoveryLink => "回復・支援ケアを探す",
        Msg::PatientMedicationsHeading => "お薬",
        Msg::PatientFocusAreasHeading => "注目ポイント",
        Msg::PatientAskHeading => "あなたの記録について Curie に聞く",
        Msg::PatientAskPlaceholder => "わかりやすい言葉で質問してください…",
        Msg::PatientExplainButton => "自分の記録から説明してもらう",
        Msg::PatientRecordsHeading => "健康記録",
        Msg::PatientPrivateNotesHeading => "プライベートメモ（共有されません）",
        Msg::PatientAlwaysExcluded => "共有対象から常に除外",
        Msg::PatientReviewForDoctor => "主治医向けの内容を確認",
        Msg::PatientIncludeForDoctor => "主治医への共有に含める",
        Msg::PatientIncludedForDoctor => "主治医向けに選択済み",

        // Mental Health
        Msg::MhEyebrow => "Curie Mobile・代謝ヘルス & 主要市販 CGM 対応",
        Msg::MhTitle => "メンタルヘルス & 包括的な血糖トラッキング",
        Msg::MhOpenMobile => "Curie Mobile を開く →",
        Msg::MhOpenClinic => "Curie Clinic を開く →",
        Msg::MhResetSample => "サンプルをリセット",
        Msg::MhTabOverview => "概要とグラフ",
        Msg::MhTabLogImport => "記録とインポート",
        Msg::MhTabAlertsAi => "アラートと AI",
        Msg::MhTabCareBrief => "ケアと要約",
        Msg::MhTabRecovery => "回復ケア検索",
        Msg::MhTabAll => "すべて表示",
        Msg::MhUploadCgmCta => "CGM ファイルをアップロード ↓",
        Msg::MhLogCheckInHeading => "メンタルヘルス・チェックインを記録",
        Msg::MhAddCheckIn => "チェックインを追加",
        Msg::MhCgmImportHeading => "市販 CGM・血糖ファイル取り込み",
        Msg::MhParseValidateExport => "エクスポートを解析・検証",
        Msg::MhCorrelationLogHeading => "血糖・気分 相関ログ",
        Msg::MhDownloadCsv => "CSV をダウンロード",

        // Recovery Search
        Msg::RecoveryEyebrow => "100% 端末内検索・SAMHSA および公的ディレクトリ",
        Msg::RecoveryTitle => "近くのメンタルヘルス・回復支援ケアを探す",
        Msg::RecoveryCrisisBannerTitle => "今すぐ助けが必要ですか？988 へ電話または SMS",
        Msg::RecoveryCrisis988Detail => "米国内で無料・秘密厳守・24時間年中無休の危機支援。",
        Msg::RecoveryCrisisLanguageNote => {
            "988 はスペイン語直通（2番を押すか AYUDA とSMS送信）および240以上の言語通訳に対応しています。"
        }
        Msg::RecoveryStateLabel => "州または地域",
        Msg::RecoverySearchPlaceholder => "都市・プログラム・専門分野・郵便番号で検索…",
        Msg::RecoveryUseLocation => "現在地を使用",
        Msg::RecoveryLocating => "位置を取得中…",
        Msg::RecoveryClearFilters => "フィルターを解除",
        Msg::RecoveryMoreFilters => "詳細フィルター",
        Msg::RecoveryCareFocus => "ケア対象",
        Msg::RecoverySortName => "並び替え：名称",
        Msg::RecoverySortDistance => "並び替え：距離",
        Msg::RecoverySaveCareTeam => "+ ケアチーム",
        Msg::RecoverySavedCareTeam => "ケアチームに保存済み",
        Msg::RecoveryCallNow => "電話する",
        Msg::RecoveryDirections => "経路 ↗",
        Msg::RecoveryWebsite => "公式サイト ↗",
        Msg::RecoveryPrevPage => "← 前へ",
        Msg::RecoveryNextPage => "次へ →",

        // Dashboard Layout Customizer
        Msg::LayoutButtonPrefix => "レイアウト",
        Msg::LayoutSavedOnDevice => "この端末に保存済み",
        Msg::LayoutReset => "リセット",
        Msg::LayoutDone => "完了",
        Msg::LayoutActiveBadge => "使用中",
        Msg::LayoutCustomLabel => "カスタム",
        Msg::LayoutDensityCompact => "コンパクト",
        Msg::LayoutDensityBalanced => "標準",
        Msg::LayoutDensitySpacious => "ゆったり",
        Msg::LayoutPanelLeft => "パネルを左に",
        Msg::LayoutPanelRight => "パネルを右に",
        Msg::LayoutPanelFullWidth => "カルテ全幅表示",
        Msg::LayoutQaFirst => "カルテ Q&A を先頭に",
        Msg::LayoutClinicalFirst => "課題と処方薬を先頭に",
        Msg::LayoutAskFirst => "Curie への質問を先頭に",
        Msg::LayoutRecordsFirst => "健康記録を先頭に",
        Msg::LayoutPresetFocusedTriage => "集中トリアージ",
        Msg::LayoutPresetChartFirstSplit => "カルテ優先分割",
        Msg::LayoutPresetCalmReading => "落ち着いた閲覧",
        Msg::LayoutPresetCalmCompanion => "穏やかなコンパニオン",
        Msg::LayoutPresetRecordsFirstCompact => "記録優先コンパクト",
        Msg::LayoutPresetVisitPrepFocus => "受診準備フォーカス",
        Msg::LayoutPresetGuidedTabs => "ガイド付きタブ",
        Msg::LayoutPresetDailyCheckIn => "日々のチェックイン & ケア",
        Msg::LayoutPresetFullAnalyticsWall => "フル分析ビュー",

        // Chart Citations & Provenance
        Msg::CitationJumpToSource => "元の記録へ移動 ↓",
        Msg::CitationClickToJump => "クリックして元の記録へ移動 ↓",
        Msg::CitationFocusedBadge => "引用からフォーカス中",
        Msg::CitationCitedRecordsHeading => "引用された元の記録",
        Msg::CitationSourceLanguageNote => "元の診療記録の言語：英語（原文そのまま）",
    }
}

const fn translate_ko(key: Msg) -> &'static str {
    match key {
        // Nav & Chrome
        Msg::NavAbout => "소개",
        Msg::NavExpertise => "전문 분야",
        Msg::NavServices => "서비스",
        Msg::NavProjects => "프로젝트",
        Msg::NavExperience => "경력",
        Msg::NavSkills => "기술",
        Msg::NavCommissions => "의뢰",
        Msg::NavTools => "도구",
        Msg::NavHireMe => "협업 문의",
        Msg::NavOpenMenu => "메뉴 열기",
        Msg::NavCloseMenu => "메뉴 닫기",
        Msg::LanguageLabel => "언어",
        Msg::SkipToContent => "본문으로 건너뛰기",
        Msg::FooterPrivacy => "개인정보 처리방침",
        Msg::FooterAllRights => "모든 권리 보유",
        Msg::FooterBuiltWith => "Rust, Leptos 및 Tauri로 제작",
        Msg::FooterCommunityPolicy => "커뮤니티 정책",
        Msg::FooterCommunityActivity => "커뮤니티 활동",
        Msg::FooterCreators => "크리에이터",
        Msg::FooterCodeOwnership => "코드 소유권",
        Msg::ToolMentalHealth => "정신건강 및 CGM",
        Msg::ToolRecoveryFinder => "회복·돌봄 기관 찾기",
        Msg::ToolClinicalSuite => "임상 워크스페이스",
        Msg::ToolPatientDemo => "환자 경험 데모",
        Msg::ToolPhysicianDemo => "의료진 워크스페이스 데모",
        Msg::ToolPortfolio => "투자 계좌 계산기",
        Msg::ToolAndroidApp => "Android 앱 (F-Droid / APK)",
        Msg::ToolDuxAncestry => "Dux 조상·유전 분석",
        Msg::ToolOwnership => "코드 소유권 보고서",

        // Clinical Suite Switcher & Overview
        Msg::SuiteAriaLabel => "임상 데모 스위트",
        Msg::SuitePatient => "Curie Mobile (환자)",
        Msg::SuitePhysician => "Curie Clinic (의료진)",
        Msg::SuiteMentalHealth => "정신건강 및 CGM",
        Msg::SuiteRecoveryFinder => "회복 돌봄 찾기",
        Msg::SuiteOverview => "스위트 개요",
        Msg::ZeroPhiBannerTitle => "합성 Zero-PHI 보장",
        Msg::ZeroPhiBannerBadge => "Zero-PHI 데모",
        Msg::OnDeviceOnlyBadge => "기기 내 전용",
        Msg::OfflineReadyBadge => "오프라인 지원",
        Msg::SyntheticDemoNotice => "100% 합성 데모 · PHI 없음 · 실제 건강 정보를 입력하지 마세요",

        // Physician Workspace
        Msg::PhysicianEyebrow => "Curie Clinic",
        Msg::PhysicianTitle => "의료진 워크스페이스 데모",
        Msg::PhysicianTodayQueue => "오늘의 진료 대기열",
        Msg::PhysicianFilterAll => "전체",
        Msg::PhysicianFilterHigh => "높음",
        Msg::PhysicianFilterModerate => "보통",
        Msg::PhysicianFilterRoutine => "정기",
        Msg::PhysicianFilterDrafts => "초안 있음",
        Msg::PhysicianPatientPanel => "환자 목록",
        Msg::PhysicianSearchPlaceholder => "환자명, MRN, 주요 항목 검색…",
        Msg::PhysicianNoMatch => "이 필터와 일치하는 환자가 없습니다.",
        Msg::PhysicianShowPanel => "환자 목록 표시",
        Msg::PhysicianJumpTo => "이동:",
        Msg::PhysicianJumpSummary => "요약",
        Msg::PhysicianJumpQa => "차트 Q&A",
        Msg::PhysicianJumpClinical => "문제 및 복약",
        Msg::PhysicianJumpReply => "답변 초안",
        Msg::PhysicianJumpRecords => "원본 기록",
        Msg::PhysicianQaEyebrow => "근거 기반 차트 Q&A",
        Msg::PhysicianQaHeading => "이 차트에 질문하기",
        Msg::PhysicianAskPlaceholder => "이 차트에 근거한 임상 질문을 입력하세요…",
        Msg::PhysicianAskButton => "차트 조회",
        Msg::PhysicianSynthesizing => "정리 중…",
        Msg::PhysicianProblems => "활성 문제",
        Msg::PhysicianMedications => "복용 약물",
        Msg::PhysicianFollowUps => "추적 관찰",
        Msg::PhysicianSourceRecords => "종단 원본 진료 기록",
        Msg::PhysicianBackToQa => "Q&A로 돌아가기 ↑",
        Msg::PhysicianClearFocus => "해제",

        // Patient Companion
        Msg::PatientEyebrow => "Curie Mobile",
        Msg::PatientTitle => "환자 경험 데모",
        Msg::PatientChooseProfile => "가상 샘플 프로필 선택",
        Msg::PatientResetSample => "샘플 초기화",
        Msg::PatientTabYou => "내 건강 (본인)",
        Msg::PatientTabDoctor => "주치의 공유용",
        Msg::PatientJumpSummary => "요약",
        Msg::PatientJumpAsk => "Curie에게 묻기",
        Msg::PatientJumpRecords => "건강 기록",
        Msg::PatientJumpNotes => "비공개 메모",
        Msg::PatientFindRecoveryLink => "회복·돌봄 기관 찾기",
        Msg::PatientMedicationsHeading => "복용 약물",
        Msg::PatientFocusAreasHeading => "집중 관리 영역",
        Msg::PatientAskHeading => "내 기록에 대해 Curie에게 물어보기",
        Msg::PatientAskPlaceholder => "쉬운 말로 질문해 보세요…",
        Msg::PatientExplainButton => "내 기록으로 설명받기",
        Msg::PatientRecordsHeading => "건강 기록",
        Msg::PatientPrivateNotesHeading => "비공개 메모 (절대 공유되지 않음)",
        Msg::PatientAlwaysExcluded => "공유 시 항상 제외",
        Msg::PatientReviewForDoctor => "주치의 공유 내용 검토",
        Msg::PatientIncludeForDoctor => "주치의 공유에 포함",
        Msg::PatientIncludedForDoctor => "주치의 공유에 포함됨",

        // Mental Health
        Msg::MhEyebrow => "Curie Mobile · 대사 건강 및 모든 상용 CGM",
        Msg::MhTitle => "정신건강 및 통합 혈당 추적",
        Msg::MhOpenMobile => "Curie Mobile 열기 →",
        Msg::MhOpenClinic => "Curie Clinic 열기 →",
        Msg::MhResetSample => "샘플 초기화",
        Msg::MhTabOverview => "개요 및 차트",
        Msg::MhTabLogImport => "기록 및 가져오기",
        Msg::MhTabAlertsAi => "알림 및 AI",
        Msg::MhTabCareBrief => "돌봄 및 요약",
        Msg::MhTabRecovery => "회복 돌봄 찾기",
        Msg::MhTabAll => "전체 섹션",
        Msg::MhUploadCgmCta => "CGM 파일 업로드 ↓",
        Msg::MhLogCheckInHeading => "정신건강 체크인 기록",
        Msg::MhAddCheckIn => "체크인 추가",
        Msg::MhCgmImportHeading => "상용 CGM 및 혈당 파일 가져오기",
        Msg::MhParseValidateExport => "내보내기 파일 분석 및 검증",
        Msg::MhCorrelationLogHeading => "혈당–기분 상관관계 기록",
        Msg::MhDownloadCsv => "CSV 다운로드",

        // Recovery Search
        Msg::RecoveryEyebrow => "100% 기기 내 검색 · SAMHSA 및 공공 디렉터리",
        Msg::RecoveryTitle => "가까운 정신건강 및 회복 지원 기관 찾기",
        Msg::RecoveryCrisisBannerTitle => "즉시 도움이 필요하신가요? 988로 전화 또는 문자하세요",
        Msg::RecoveryCrisis988Detail => "미국 전역에서 무료·비밀 보장·연중무휴 24시간 위기 지원.",
        Msg::RecoveryCrisisLanguageNote => {
            "988은 스페인어 직통 지원(2번 또는 AYUDA 문자)과 240개 이상의 언어 통역을 제공합니다."
        }
        Msg::RecoveryStateLabel => "주 또는 준주",
        Msg::RecoverySearchPlaceholder => "도시, 프로그램, 전문 분야 또는 우편번호 검색…",
        Msg::RecoveryUseLocation => "내 위치 사용",
        Msg::RecoveryLocating => "위치 확인 중…",
        Msg::RecoveryClearFilters => "필터 지우기",
        Msg::RecoveryMoreFilters => "필터 더보기",
        Msg::RecoveryCareFocus => "돌봄 분야",
        Msg::RecoverySortName => "정렬: 이름순",
        Msg::RecoverySortDistance => "정렬: 거리순",
        Msg::RecoverySaveCareTeam => "+ 돌봄 팀",
        Msg::RecoverySavedCareTeam => "돌봄 팀에 저장됨",
        Msg::RecoveryCallNow => "전화하기",
        Msg::RecoveryDirections => "길찾기 ↗",
        Msg::RecoveryWebsite => "웹사이트 ↗",
        Msg::RecoveryPrevPage => "← 이전",
        Msg::RecoveryNextPage => "다음 →",

        // Dashboard Layout Customizer
        Msg::LayoutButtonPrefix => "레이아웃",
        Msg::LayoutSavedOnDevice => "이 기기에 저장됨",
        Msg::LayoutReset => "초기화",
        Msg::LayoutDone => "완료",
        Msg::LayoutActiveBadge => "사용 중",
        Msg::LayoutCustomLabel => "사용자 지정",
        Msg::LayoutDensityCompact => "좁게",
        Msg::LayoutDensityBalanced => "균형",
        Msg::LayoutDensitySpacious => "넓게",
        Msg::LayoutPanelLeft => "왼쪽 패널",
        Msg::LayoutPanelRight => "오른쪽 패널",
        Msg::LayoutPanelFullWidth => "전체 너비 차트",
        Msg::LayoutQaFirst => "차트 Q&A 먼저",
        Msg::LayoutClinicalFirst => "문제 및 복약 먼저",
        Msg::LayoutAskFirst => "Curie에게 묻기 먼저",
        Msg::LayoutRecordsFirst => "건강 기록 먼저",
        Msg::LayoutPresetFocusedTriage => "집중 분류",
        Msg::LayoutPresetChartFirstSplit => "차트 우선 분할",
        Msg::LayoutPresetCalmReading => "차분한 읽기",
        Msg::LayoutPresetCalmCompanion => "차분한 동반자",
        Msg::LayoutPresetRecordsFirstCompact => "기록 우선 컴팩트",
        Msg::LayoutPresetVisitPrepFocus => "진료 준비 집중",
        Msg::LayoutPresetGuidedTabs => "안내 탭",
        Msg::LayoutPresetDailyCheckIn => "일일 체크인 및 돌봄",
        Msg::LayoutPresetFullAnalyticsWall => "전체 분석 보기",

        // Chart Citations & Provenance
        Msg::CitationJumpToSource => "원본 기록으로 이동 ↓",
        Msg::CitationClickToJump => "클릭하여 원본 기록으로 이동 ↓",
        Msg::CitationFocusedBadge => "인용에서 강조됨",
        Msg::CitationCitedRecordsHeading => "인용된 원본 기록",
        Msg::CitationSourceLanguageNote => "원본 진료 기록 언어: 영어(원문 그대로)",
    }
}

const fn translate_ar(key: Msg) -> &'static str {
    match key {
        // Nav & Chrome
        Msg::NavAbout => "نبذة",
        Msg::NavExpertise => "الخبرات",
        Msg::NavServices => "الخدمات",
        Msg::NavProjects => "المشاريع",
        Msg::NavExperience => "المسيرة المهنية",
        Msg::NavSkills => "المهارات",
        Msg::NavCommissions => "الطلبات الخاصة",
        Msg::NavTools => "الأدوات",
        Msg::NavHireMe => "تواصل للعمل",
        Msg::NavOpenMenu => "فتح القائمة",
        Msg::NavCloseMenu => "إغلاق القائمة",
        Msg::LanguageLabel => "اللغة",
        Msg::SkipToContent => "انتقل إلى المحتوى",
        Msg::FooterPrivacy => "الخصوصية",
        Msg::FooterAllRights => "جميع الحقوق محفوظة",
        Msg::FooterBuiltWith => "مبني باستخدام Rust وLeptos وTauri",
        Msg::FooterCommunityPolicy => "سياسة المجتمع",
        Msg::FooterCommunityActivity => "نشاط المجتمع",
        Msg::FooterCreators => "المبدعون",
        Msg::FooterCodeOwnership => "ملكية الشيفرة",
        Msg::ToolMentalHealth => "الصحة النفسية وCGM",
        Msg::ToolRecoveryFinder => "دليل رعاية التعافي",
        Msg::ToolClinicalSuite => "مساحة العمل السريرية",
        Msg::ToolPatientDemo => "عرض تجربة المريض",
        Msg::ToolPhysicianDemo => "عرض مساحة الطبيب",
        Msg::ToolPortfolio => "حاسبة حساب الاستثمار",
        Msg::ToolAndroidApp => "تطبيق أندرويد (F-Droid / APK)",
        Msg::ToolDuxAncestry => "Dux تحليل الأصول والوراثة",
        Msg::ToolOwnership => "تقرير ملكية الشيفرة",

        // Clinical Suite Switcher & Overview
        Msg::SuiteAriaLabel => "حزمة العرض السريري",
        Msg::SuitePatient => "Curie Mobile (المريض)",
        Msg::SuitePhysician => "Curie Clinic (الطبيب)",
        Msg::SuiteMentalHealth => "الصحة النفسية وCGM",
        Msg::SuiteRecoveryFinder => "دليل رعاية التعافي",
        Msg::SuiteOverview => "نظرة عامة على الحزمة",
        Msg::ZeroPhiBannerTitle => "ضمان بيانات اصطناعية خالية من PHI",
        Msg::ZeroPhiBannerBadge => "عرض تجريبي بلا PHI",
        Msg::OnDeviceOnlyBadge => "على الجهاز فقط",
        Msg::OfflineReadyBadge => "جاهز دون اتصال",
        Msg::SyntheticDemoNotice => {
            "عرض اصطناعي 100% · خالٍ من PHI · يُرجى عدم إدخال معلومات صحية حقيقية"
        }

        // Physician Workspace
        Msg::PhysicianEyebrow => "Curie Clinic",
        Msg::PhysicianTitle => "عرض مساحة الطبيب",
        Msg::PhysicianTodayQueue => "قائمة عيادة اليوم",
        Msg::PhysicianFilterAll => "الكل",
        Msg::PhysicianFilterHigh => "مرتفعة",
        Msg::PhysicianFilterModerate => "متوسطة",
        Msg::PhysicianFilterRoutine => "روتينية",
        Msg::PhysicianFilterDrafts => "المسودات",
        Msg::PhysicianPatientPanel => "قائمة المرضى",
        Msg::PhysicianSearchPlaceholder => "ابحث عن مريض أو رقم ملف أو محور…",
        Msg::PhysicianNoMatch => "لا يوجد مرضى مطابقون لهذا الفلتر.",
        Msg::PhysicianShowPanel => "إظهار قائمة المرضى",
        Msg::PhysicianJumpTo => "انتقال إلى:",
        Msg::PhysicianJumpSummary => "الملخص",
        Msg::PhysicianJumpQa => "أسئلة الملف",
        Msg::PhysicianJumpClinical => "المشكلات والأدوية",
        Msg::PhysicianJumpReply => "مسودة الرد",
        Msg::PhysicianJumpRecords => "السجلات المصدرية",
        Msg::PhysicianQaEyebrow => "أسئلة وأجوبة موثقة من الملف",
        Msg::PhysicianQaHeading => "اسأل حول هذا الملف",
        Msg::PhysicianAskPlaceholder => "اطرح سؤالاً سريرياً مستنداً إلى هذا الملف…",
        Msg::PhysicianAskButton => "استعلام الملف",
        Msg::PhysicianSynthesizing => "جارٍ التلخيص…",
        Msg::PhysicianProblems => "المشكلات النشطة",
        Msg::PhysicianMedications => "الأدوية",
        Msg::PhysicianFollowUps => "المتابعات",
        Msg::PhysicianSourceRecords => "السجلات المصدرية الطولية",
        Msg::PhysicianBackToQa => "العودة إلى الأسئلة ↑",
        Msg::PhysicianClearFocus => "مسح",

        // Patient Companion
        Msg::PatientEyebrow => "Curie Mobile",
        Msg::PatientTitle => "عرض تجربة المريض",
        Msg::PatientChooseProfile => "اختر ملفاً تجريبياً افتراضياً",
        Msg::PatientResetSample => "إعادة ضبط العينة",
        Msg::PatientTabYou => "صحتك (أنت)",
        Msg::PatientTabDoctor => "لطبيبي",
        Msg::PatientJumpSummary => "الملخص",
        Msg::PatientJumpAsk => "اسأل Curie",
        Msg::PatientJumpRecords => "السجلات الصحية",
        Msg::PatientJumpNotes => "ملاحظات خاصة",
        Msg::PatientFindRecoveryLink => "ابحث عن رعاية التعافي",
        Msg::PatientMedicationsHeading => "الأدوية",
        Msg::PatientFocusAreasHeading => "محاور التركيز",
        Msg::PatientAskHeading => "اسأل Curie عن سجلاتك",
        Msg::PatientAskPlaceholder => "اطرح سؤالاً بلغة بسيطة…",
        Msg::PatientExplainButton => "اشرح من سجلاتي",
        Msg::PatientRecordsHeading => "السجلات الصحية",
        Msg::PatientPrivateNotesHeading => "ملاحظات خاصة (لا تُشارك أبداً)",
        Msg::PatientAlwaysExcluded => "مستثناة دائماً من المشاركة",
        Msg::PatientReviewForDoctor => "مراجعة لمشاركتها مع طبيبي",
        Msg::PatientIncludeForDoctor => "تضمين لطبيبي",
        Msg::PatientIncludedForDoctor => "مُضمّن للطبيب",

        // Mental Health
        Msg::MhEyebrow => "Curie Mobile · الصحة الأيضية وجميع أجهزة CGM التجارية",
        Msg::MhTitle => "الصحة النفسية والمتابعة الشاملة للغلوكوز",
        Msg::MhOpenMobile => "فتح Curie Mobile ←",
        Msg::MhOpenClinic => "فتح Curie Clinic ←",
        Msg::MhResetSample => "إعادة ضبط العينة",
        Msg::MhTabOverview => "النظرة العامة والرسوم",
        Msg::MhTabLogImport => "التسجيل والاستيراد",
        Msg::MhTabAlertsAi => "التنبيهات والذكاء الاصطناعي",
        Msg::MhTabCareBrief => "الرعاية والملخص",
        Msg::MhTabRecovery => "رعاية التعافي",
        Msg::MhTabAll => "جميع الأقسام",
        Msg::MhUploadCgmCta => "رفع ملف CGM الخاص بك ↓",
        Msg::MhLogCheckInHeading => "تسجيل متابعة الصحة النفسية",
        Msg::MhAddCheckIn => "إضافة متابعة",
        Msg::MhCgmImportHeading => "استيراد ملفات CGM والغلوكوز التجارية",
        Msg::MhParseValidateExport => "تحليل الملف والتحقق منه",
        Msg::MhCorrelationLogHeading => "سجل الارتباط بين الغلوكوز والمزاج",
        Msg::MhDownloadCsv => "تنزيل CSV",

        // Recovery Search
        Msg::RecoveryEyebrow => "بحث محلي 100% على الجهاز · SAMHSA والأدلة العامة",
        Msg::RecoveryTitle => "ابحث عن رعاية الصحة النفسية والتعافي بالقرب منك",
        Msg::RecoveryCrisisBannerTitle => "تحتاج مساعدة فورية؟ اتصل أو أرسل رسالة إلى 988",
        Msg::RecoveryCrisis988Detail => {
            "دعم مجاني وسري للأزمات على مدار الساعة طوال أيام الأسبوع في الولايات المتحدة."
        }
        Msg::RecoveryCrisisLanguageNote => {
            "يوفر الرقم 988 دعماً مباشراً بالإسبانية (اضغط 2 أو أرسل AYUDA) وترجمة فورية بأكثر من 240 لغة."
        }
        Msg::RecoveryStateLabel => "الولاية أو الإقليم",
        Msg::RecoverySearchPlaceholder => "ابحث بالمدينة أو البرنامج أو التخصص أو الرمز البريدي…",
        Msg::RecoveryUseLocation => "استخدام موقعي",
        Msg::RecoveryLocating => "جارٍ تحديد الموقع…",
        Msg::RecoveryClearFilters => "مسح الفلاتر",
        Msg::RecoveryMoreFilters => "مزيد من الفلاتر",
        Msg::RecoveryCareFocus => "نوع الرعاية",
        Msg::RecoverySortName => "الترتيب: الاسم",
        Msg::RecoverySortDistance => "الترتيب: المسافة",
        Msg::RecoverySaveCareTeam => "+ فريق الرعاية",
        Msg::RecoverySavedCareTeam => "محفوظ في فريق الرعاية",
        Msg::RecoveryCallNow => "اتصال",
        Msg::RecoveryDirections => "الاتجاهات ↗",
        Msg::RecoveryWebsite => "الموقع الإلكتروني ↗",
        Msg::RecoveryPrevPage => "→ السابق",
        Msg::RecoveryNextPage => "التالي ←",

        // Dashboard Layout Customizer
        Msg::LayoutButtonPrefix => "التخطيط",
        Msg::LayoutSavedOnDevice => "محفوظ على هذا الجهاز",
        Msg::LayoutReset => "إعادة ضبط",
        Msg::LayoutDone => "تم",
        Msg::LayoutActiveBadge => "نشط",
        Msg::LayoutCustomLabel => "مخصص",
        Msg::LayoutDensityCompact => "مكثف",
        Msg::LayoutDensityBalanced => "متوازن",
        Msg::LayoutDensitySpacious => "مريح",
        Msg::LayoutPanelLeft => "القائمة يساراً",
        Msg::LayoutPanelRight => "القائمة يميناً",
        Msg::LayoutPanelFullWidth => "الملف بعرض كامل",
        Msg::LayoutQaFirst => "أسئلة الملف أولاً",
        Msg::LayoutClinicalFirst => "المشكلات والأدوية أولاً",
        Msg::LayoutAskFirst => "اسأل Curie أولاً",
        Msg::LayoutRecordsFirst => "السجلات الصحية أولاً",
        Msg::LayoutPresetFocusedTriage => "فرز مركّز",
        Msg::LayoutPresetChartFirstSplit => "عرض مقسّم يبدأ بالملف",
        Msg::LayoutPresetCalmReading => "قراءة هادئة",
        Msg::LayoutPresetCalmCompanion => "مرافق هادئ",
        Msg::LayoutPresetRecordsFirstCompact => "السجلات أولاً (مكثف)",
        Msg::LayoutPresetVisitPrepFocus => "تركيز التحضير للزيارة",
        Msg::LayoutPresetGuidedTabs => "تبويبات موجّهة",
        Msg::LayoutPresetDailyCheckIn => "المتابعة اليومية والرعاية",
        Msg::LayoutPresetFullAnalyticsWall => "لوحة التحليلات الكاملة",

        // Chart Citations & Provenance
        Msg::CitationJumpToSource => "انتقل إلى السجل المصدر ↓",
        Msg::CitationClickToJump => "انقر للانتقال إلى السجل المصدر ↓",
        Msg::CitationFocusedBadge => "مُركّز من الاقتباس",
        Msg::CitationCitedRecordsHeading => "السجلات المصدرية المُستشهد بها",
        Msg::CitationSourceLanguageNote => "لغة السجل الأصلي: الإنجليزية (نص حرفي)",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn parse_bcp47_tags_and_rejects_unknown() {
        assert_eq!(Locale::parse_tag("en"), Some(Locale::En));
        assert_eq!(Locale::parse_tag("en-GB"), Some(Locale::En));
        assert_eq!(Locale::parse_tag("es-MX"), Some(Locale::Es));
        assert_eq!(Locale::parse_tag("ES_es"), Some(Locale::Es));
        assert_eq!(Locale::parse_tag("fr-CA"), Some(Locale::Fr));
        assert_eq!(Locale::parse_tag("pt-BR"), Some(Locale::Pt));
        assert_eq!(Locale::parse_tag("de-AT"), Some(Locale::De));
        assert_eq!(Locale::parse_tag("de-DE"), Some(Locale::De));
        assert_eq!(Locale::parse_tag("zh-CN"), Some(Locale::ZhHans));
        assert_eq!(Locale::parse_tag("zh-SG"), Some(Locale::ZhHans));
        assert_eq!(Locale::parse_tag("zh-Hans"), Some(Locale::ZhHans));
        assert_eq!(Locale::parse_tag("zh"), Some(Locale::ZhHans));
        assert_eq!(Locale::parse_tag("ja-JP"), Some(Locale::Ja));
        assert_eq!(Locale::parse_tag("ko-KR"), Some(Locale::Ko));
        assert_eq!(Locale::parse_tag("ar-SA"), Some(Locale::Ar));
        assert_eq!(Locale::parse_tag("ar-EG"), Some(Locale::Ar));

        assert_eq!(Locale::parse_tag(""), None);
        assert_eq!(Locale::parse_tag("   "), None);
        assert_eq!(Locale::parse_tag("zz-ZZ"), None);
        assert_eq!(Locale::parse_tag("arc"), None);
        assert_eq!(Locale::parse_tag("english"), None);
    }

    #[test]
    fn parse_query_search_lang_param() {
        assert_eq!(Locale::from_query_search("?lang=ar"), Some(Locale::Ar));
        assert_eq!(
            Locale::from_query_search("?tab=recovery&lang=es"),
            Some(Locale::Es)
        );
        assert_eq!(
            Locale::from_query_search("?foo=1&LANG=zh-Hans&bar=2#section"),
            Some(Locale::ZhHans)
        );
        assert_eq!(Locale::from_query_search("?tab=recovery"), None);
        assert_eq!(Locale::from_query_search("?lang=unknown"), None);
    }

    #[test]
    fn browser_languages_picks_first_supported_or_en() {
        let langs = vec![
            "sv-SE".to_string(),
            "pt-BR".to_string(),
            "fr-FR".to_string(),
        ];
        assert_eq!(Locale::from_browser_languages(&langs), Locale::Pt);

        let unsupported = vec!["nl-NL".to_string(), "pl-PL".to_string()];
        assert_eq!(Locale::from_browser_languages(&unsupported), Locale::En);
        assert_eq!(Locale::from_browser_languages(&[]), Locale::En);
    }

    #[test]
    fn direction_is_rtl_only_for_arabic() {
        assert_eq!(Locale::Ar.dir(), TextDirection::Rtl);
        assert_eq!(Locale::Ar.dir().as_str(), "rtl");
        assert!(Locale::Ar.dir().is_rtl());

        for &loc in Locale::ALL {
            if loc != Locale::Ar {
                assert_eq!(loc.dir(), TextDirection::Ltr, "{loc:?} must be LTR");
                assert_eq!(loc.dir().as_str(), "ltr");
                assert!(!loc.dir().is_rtl());
            }
        }
    }

    #[test]
    fn exhaustive_translations_are_non_empty_for_all_locales_and_msgs() {
        let unique_msgs: HashSet<Msg> = Msg::ALL.iter().copied().collect();
        assert_eq!(
            unique_msgs.len(),
            Msg::ALL.len(),
            "Msg::ALL must not contain duplicate variants"
        );
        assert_eq!(Locale::ALL.len(), 9);

        for &loc in Locale::ALL {
            assert!(!loc.bcp47().trim().is_empty());
            assert!(!loc.native_name().trim().is_empty());
            assert!(!loc.short_code().trim().is_empty());
            assert_eq!(Locale::parse_tag(loc.bcp47()), Some(loc));

            for &msg in Msg::ALL {
                let text = t(loc, msg);
                assert!(
                    !text.trim().is_empty(),
                    "empty translation for ({loc:?}, {msg:?})"
                );
            }
        }
    }

    #[cfg(feature = "ssr")]
    #[test]
    fn language_switcher_ssr_renders_all_nine_native_autonyms_and_lang_attrs() {
        let owner = Owner::new();
        let html = owner.with(|| view! { <LanguageSwitcher /> }.to_html());
        drop(owner);

        assert!(html.contains("<select"), "missing <select> element");
        assert!(html.contains("<label"), "missing <label> element");
        assert!(
            html.contains("aria-label=\"Language\""),
            "missing localized aria-label"
        );

        for &loc in Locale::ALL {
            let lang_attr = format!("lang=\"{}\"", loc.bcp47());
            assert!(
                html.contains(&lang_attr),
                "missing {lang_attr} attribute for {loc:?}"
            );
            assert!(
                html.contains(loc.native_name()),
                "missing native autonym {} for {loc:?}",
                loc.native_name()
            );
        }
    }
}
