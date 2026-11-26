//! User-configurable target-range glucose alert controls, live status banner,
//! notification channel controls (browser Web Notifications & `moe.dabney.personal`
//! Android/iOS native notifications), custom target-range breakdown, and recent
//! breach log for Dabney.moe `/mental-health`.

use leptos::prelude::*;

use crate::health_bridge::AppAlertStatusUi;
use crate::mental_health_cgm_alerts::{
    format_glucose_reading_honestly, save_alert_config_to_storage, AlertSeverity,
    BrowserNotificationPermission, GlucoseAlertConfig, GlucoseTargetPreset, RangeBreachEpisode,
    WindowRangeAlertSummary, MAX_ALERT_THRESHOLD_MG_DL, MIN_ALERT_THRESHOLD_MG_DL,
};

const MAX_RECENT_BREACH_ROWS: usize = 6;
const COOLDOWN_OPTIONS: &[u32] = &[15, 30, 60, 120];
const SUSTAINED_OPTIONS: &[(u32, &str)] = &[
    (0, "Immediate (0 min)"),
    (10, "10 min"),
    (15, "15 min"),
    (30, "30 min"),
];
const AUTO_POLL_OPTIONS: &[(u32, &str)] =
    &[(0, "Off"), (5, "5 min"), (15, "15 min"), (30, "30 min")];

fn format_short_timestamp(ts: &str) -> String {
    let trimmed = ts.trim();
    if trimmed.is_ascii()
        && trimmed.len() >= 16
        && trimmed
            .as_bytes()
            .get(10)
            .is_some_and(|&b| b == b'T' || b == b' ')
    {
        format!("{} {}", &trimmed[0..10], &trimmed[11..16])
    } else {
        trimmed.to_string()
    }
}

fn format_breach_duration(minutes: f64, reading_count: usize) -> String {
    if minutes < 1.0 {
        if reading_count <= 1 {
            "single reading".to_string()
        } else {
            format!("{reading_count} rdgs")
        }
    } else if minutes < 60.0 {
        format!("{:.0}m", minutes.round())
    } else if minutes < 48.0 * 60.0 {
        format!("{:.1}h", minutes / 60.0)
    } else {
        format!("{:.1}d", minutes / 1440.0)
    }
}

fn format_breach_extreme(breach: &RangeBreachEpisode) -> String {
    if breach.sensor_capped {
        if breach.severity.is_low() {
            format!("≤{:.0} mg/dL", breach.extreme_mg_dl)
        } else {
            format!("≥{:.0} mg/dL", breach.extreme_mg_dl)
        }
    } else {
        format!("{:.0} mg/dL", breach.extreme_mg_dl)
    }
}

fn live_status_pill(
    cfg: &GlucoseAlertConfig,
    summary: &WindowRangeAlertSummary,
) -> (&'static str, &'static str) {
    if !cfg.enabled {
        return (
            "Alerts Paused",
            "border-slate-500/40 bg-slate-800/80 text-slate-300",
        );
    }
    if let Some(alert) = summary.latest.active_alert.as_ref() {
        return match alert.severity {
            AlertSeverity::UrgentLow => (
                "Urgent Low",
                "border-rose-400/50 bg-rose-500/20 text-rose-100",
            ),
            AlertSeverity::UrgentHigh => (
                "Urgent High",
                "border-rose-400/50 bg-rose-500/20 text-rose-100",
            ),
            AlertSeverity::Low => (
                "Below Target",
                "border-amber-400/40 bg-amber-500/15 text-amber-200",
            ),
            AlertSeverity::High => (
                "Above Target",
                "border-amber-400/40 bg-amber-500/15 text-amber-200",
            ),
            AlertSeverity::RapidFall => (
                "Rapid Fall",
                "border-rose-400/40 bg-rose-500/15 text-rose-200",
            ),
            AlertSeverity::RapidRise => (
                "Rapid Rise",
                "border-amber-400/40 bg-amber-500/15 text-amber-200",
            ),
        };
    }
    let Some(latest_val) = summary.latest.latest_reading_mg_dl else {
        return (
            "No Readings",
            "border-slate-500/40 bg-slate-800/80 text-slate-300",
        );
    };
    if !summary.latest.in_target_range {
        if latest_val < summary.target_low_mg_dl {
            return (
                "Below Target",
                "border-amber-400/40 bg-amber-500/15 text-amber-200",
            );
        }
        return (
            "Above Target",
            "border-amber-400/40 bg-amber-500/15 text-amber-200",
        );
    }
    (
        "In Target Range",
        "border-emerald-400/40 bg-emerald-500/15 text-emerald-200",
    )
}

fn native_platform_badge(status: &AppAlertStatusUi) -> &'static str {
    if status.platform.contains("ios") {
        "Apple Notifications"
    } else {
        "Android Notifications"
    }
}

fn browser_permission_badge(perm: BrowserNotificationPermission) -> (&'static str, &'static str) {
    match perm {
        BrowserNotificationPermission::Granted => (
            "Granted",
            "border-emerald-400/35 bg-emerald-500/15 text-emerald-200",
        ),
        BrowserNotificationPermission::Default => (
            "Not enabled",
            "border-amber-400/35 bg-amber-500/15 text-amber-200",
        ),
        BrowserNotificationPermission::Denied => (
            "Blocked in browser settings",
            "border-rose-400/35 bg-rose-500/15 text-rose-200",
        ),
        BrowserNotificationPermission::Unsupported => (
            "Unsupported",
            "border-slate-500/40 bg-slate-800/80 text-slate-300",
        ),
    }
}

/// Interactive automated glucose target-range alerts card for `/mental-health`.
///
/// Surfaces live target-range status, in-app alert callouts, clinical presets,
/// threshold & anti-fatigue controls persisted to `localStorage`, browser & native
/// `moe.dabney.personal` notification channel controls, foreground Health Connect
/// auto-poll intervals, a 3-segment target-range compliance bar, and a bounded
/// recent breach log.
#[component]
pub fn GlucoseRangeAlertsCard(
    config: RwSignal<GlucoseAlertConfig>,
    summary: Signal<WindowRangeAlertSummary>,
    browser_permission: Signal<BrowserNotificationPermission>,
    app_alert_status: Signal<Option<AppAlertStatusUi>>,
    health_connect_ready: Signal<bool>,
    personal_data_loaded: Signal<bool>,
    alert_feedback: Signal<Option<String>>,
    on_request_browser_permission: Callback<()>,
    on_request_app_permission: Callback<()>,
    on_send_test_alert: Callback<()>,
) -> impl IntoView {
    let initial_cfg = config.get_untracked().sanitized();
    let urgent_low_draft = RwSignal::new(format!("{:.0}", initial_cfg.urgent_low_mg_dl));
    let low_draft = RwSignal::new(format!("{:.0}", initial_cfg.low_mg_dl));
    let high_draft = RwSignal::new(format!("{:.0}", initial_cfg.high_mg_dl));
    let urgent_high_draft = RwSignal::new(format!("{:.0}", initial_cfg.urgent_high_mg_dl));

    // Keep text input drafts aligned when presets, Reset defaults, or localStorage hydration
    // update `config`.
    Effect::new(move |_| {
        let cfg = config.get().sanitized();
        urgent_low_draft.set(format!("{:.0}", cfg.urgent_low_mg_dl));
        low_draft.set(format!("{:.0}", cfg.low_mg_dl));
        high_draft.set(format!("{:.0}", cfg.high_mg_dl));
        urgent_high_draft.set(format!("{:.0}", cfg.urgent_high_mg_dl));
    });

    let persist_update = move |mutate: Box<dyn FnOnce(&mut GlucoseAlertConfig)>| {
        let mut next = config.get_untracked();
        mutate(&mut next);
        let sanitized = next.sanitized();
        config.set(sanitized.clone());
        save_alert_config_to_storage(&sanitized);
    };

    let commit_threshold_drafts = move || {
        let ulow = urgent_low_draft.get_untracked();
        let low = low_draft.get_untracked();
        let high = high_draft.get_untracked();
        let uhigh = urgent_high_draft.get_untracked();

        let mut next = config.get_untracked();
        if let Ok(v) = ulow.trim().parse::<f64>() {
            if v.is_finite() {
                next.urgent_low_mg_dl = v;
            }
        }
        if let Ok(v) = low.trim().parse::<f64>() {
            if v.is_finite() {
                next.low_mg_dl = v;
            }
        }
        if let Ok(v) = high.trim().parse::<f64>() {
            if v.is_finite() {
                next.high_mg_dl = v;
            }
        }
        if let Ok(v) = uhigh.trim().parse::<f64>() {
            if v.is_finite() {
                next.urgent_high_mg_dl = v;
            }
        }
        next.preset = GlucoseTargetPreset::Custom;
        let sanitized = next.sanitized();
        config.set(sanitized.clone());
        save_alert_config_to_storage(&sanitized);
        urgent_low_draft.set(format!("{:.0}", sanitized.urgent_low_mg_dl));
        low_draft.set(format!("{:.0}", sanitized.low_mg_dl));
        high_draft.set(format!("{:.0}", sanitized.high_mg_dl));
        urgent_high_draft.set(format!("{:.0}", sanitized.urgent_high_mg_dl));
    };

    let apply_tight_preset = move |_| {
        persist_update(Box::new(|cfg| {
            cfg.apply_preset(GlucoseTargetPreset::TightMetabolic);
        }));
    };

    let apply_ada_preset = move |_| {
        persist_update(Box::new(|cfg| {
            cfg.apply_preset(GlucoseTargetPreset::AdaStandard);
        }));
    };

    let reset_defaults = move |_| {
        let defaults = GlucoseAlertConfig::default();
        config.set(defaults.clone());
        save_alert_config_to_storage(&defaults);
    };

    let on_thresholds_submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        commit_threshold_drafts();
    };

    view! {
        <section
            data-glucose-range-alerts="true"
            class="mb-8 rounded-2xl border border-white/10 bg-white/[0.02] p-6 space-y-5"
        >
            // 1. Live Target-Range Status Header
            <div class="flex flex-wrap items-start justify-between gap-3">
                <div>
                    <div class="flex flex-wrap items-center gap-2">
                        <h2 class="text-lg font-semibold text-white">
                            "Automated Glucose Range Alerts"
                        </h2>
                        {move || {
                            let cfg = config.get().sanitized();
                            let sum = summary.get();
                            let (pill_label, pill_cls) = live_status_pill(&cfg, &sum);
                            view! {
                                <span
                                    data-alert-status-pill="true"
                                    class=format!(
                                        "inline-flex rounded-full border px-2.5 py-0.5 text-[11px] font-semibold {pill_cls}"
                                    )
                                >
                                    {pill_label}
                                </span>
                            }
                        }}
                    </div>
                    <p class="mt-1 text-xs text-slate-400">
                        "Configurable target-range thresholds, anti-fatigue cooldown & escalation, and privacy-safe lock-screen notifications—evaluated 100% locally on device."
                    </p>
                </div>

                <div class="flex flex-wrap items-center gap-2 text-[11px]">
                    {move || {
                        let sum = summary.get();
                        view! {
                            <span
                                data-active-range-badge="true"
                                class="rounded-full border border-cyan-400/30 bg-cyan-500/10 px-3 py-1 font-mono font-medium text-cyan-200"
                            >
                                {format!(
                                    "Target {:.0}–{:.0} · Urgent <{:.0} / >{:.0} mg/dL",
                                    sum.target_low_mg_dl,
                                    sum.target_high_mg_dl,
                                    sum.urgent_low_mg_dl,
                                    sum.urgent_high_mg_dl,
                                )}
                            </span>
                        }
                    }}
                    {move || {
                        let sum = summary.get();
                        sum.latest.latest_reading_mg_dl.map(|mg| {
                            let reading_str =
                                format_glucose_reading_honestly(mg, sum.latest.latest_bound);
                            view! {
                                <span class="rounded-full border border-white/10 bg-slate-900/90 px-3 py-1 font-mono text-slate-200">
                                    {format!("Latest: {reading_str}")}
                                </span>
                            }
                        })
                    }}
                </div>
            </div>

            // Active in-app alert callout (or pending sustained-filter suppression notice)
            {move || {
                let sum = summary.get();
                let is_personal = personal_data_loaded.get();
                if let Some(alert) = sum.latest.active_alert {
                    let callout_cls = if alert.severity.is_urgent() {
                        "border-rose-400/45 bg-rose-500/12 text-rose-100"
                    } else if alert.severity.is_low() {
                        "border-amber-400/40 bg-amber-500/12 text-amber-100"
                    } else {
                        "border-amber-400/35 bg-amber-500/10 text-amber-100"
                    };
                    let ts_display = format_short_timestamp(&alert.timestamp);
                    let suppressed = sum.latest.suppressed_reason.clone();
                    Some(
                        view! {
                            <div
                                data-active-alert-callout="true"
                                role="status"
                                class=format!("rounded-xl border p-4 space-y-2 text-xs {callout_cls}")
                            >
                                <div class="flex flex-wrap items-center justify-between gap-2">
                                    <div class="flex flex-wrap items-center gap-2">
                                        <span class=format!(
                                            "rounded-full border px-2 py-0.5 text-[10px] font-semibold uppercase tracking-wide {}",
                                            alert.severity.badge_class()
                                        )>
                                            {alert.severity.label()}
                                        </span>
                                        <strong class="text-sm font-semibold text-white">
                                            {alert.in_app_title}
                                        </strong>
                                    </div>
                                    <span class="font-mono text-[11px] text-slate-300">
                                        {ts_display}
                                    </span>
                                </div>
                                <p class="leading-relaxed text-slate-200">{alert.in_app_detail}</p>
                                {suppressed.map(|reason| view! {
                                    <p class="text-[11px] text-slate-300">
                                        <span class="font-semibold text-amber-200">"Cooldown / rate-limit: "</span>
                                        {reason}
                                    </p>
                                })}
                                {(!is_personal).then(|| view! {
                                    <p class="text-[11px] text-slate-400">
                                        "Sample preview mode — automatic OS/browser notifications stay muted until your personal trace is imported or synced (or when you click Send Test Alert)."
                                    </p>
                                })}
                            </div>
                        }
                        .into_any(),
                    )
                } else if let Some(reason) = sum.latest.suppressed_reason {
                    if !sum.latest.in_target_range {
                        Some(
                            view! {
                                <div class="rounded-xl border border-amber-400/25 bg-amber-500/5 px-3.5 py-2.5 text-xs text-amber-200">
                                    {reason}
                                </div>
                            }
                            .into_any(),
                        )
                    } else {
                        None
                    }
                } else {
                    None
                }
            }}

            // 2. Preset Switcher & Master Toggle
            <div class="flex flex-wrap items-center justify-between gap-3 border-y border-white/10 py-3">
                <label class="inline-flex cursor-pointer items-center gap-2.5 text-xs font-semibold text-white">
                    <input
                        type="checkbox"
                        prop:checked=move || config.get().enabled
                        on:change=move |ev| {
                            let checked = event_target_checked(&ev);
                            persist_update(Box::new(move |cfg| {
                                cfg.enabled = checked;
                            }));
                        }
                        class="h-4 w-4 rounded border-emerald-400/50 bg-slate-900 accent-emerald-400"
                    />
                    <span>"Automated range alerts"</span>
                </label>

                <div class="flex flex-wrap items-center gap-1.5 text-xs">
                    <button
                        type="button"
                        on:click=apply_tight_preset
                        class=move || {
                            if config.get().sanitized().preset == GlucoseTargetPreset::TightMetabolic {
                                "rounded-full bg-emerald-400 px-3 py-1 font-semibold text-slate-950 transition"
                            } else {
                                "rounded-full border border-white/15 bg-slate-900/80 px-3 py-1 font-medium text-slate-200 transition hover:bg-white/10"
                            }
                        }
                    >
                        "Tight / Lingo (70–140)"
                    </button>
                    <button
                        type="button"
                        on:click=apply_ada_preset
                        class=move || {
                            if config.get().sanitized().preset == GlucoseTargetPreset::AdaStandard {
                                "rounded-full bg-cyan-400 px-3 py-1 font-semibold text-slate-950 transition"
                            } else {
                                "rounded-full border border-white/15 bg-slate-900/80 px-3 py-1 font-medium text-slate-200 transition hover:bg-white/10"
                            }
                        }
                    >
                        "ADA Standard (70–180)"
                    </button>
                    {move || {
                        (config.get().sanitized().preset == GlucoseTargetPreset::Custom).then(|| view! {
                            <span class="rounded-full border border-violet-400/40 bg-violet-500/15 px-2.5 py-1 text-[11px] font-semibold text-violet-200">
                                "Custom"
                            </span>
                        })
                    }}
                    <button
                        type="button"
                        on:click=reset_defaults
                        class="rounded-full border border-white/15 px-3 py-1 font-medium text-slate-300 transition hover:bg-white/10 hover:text-white"
                    >
                        "Reset defaults"
                    </button>
                </div>
            </div>

            // 3. User-Configurable Thresholds & Anti-Fatigue Controls
            <form on:submit=on_thresholds_submit class="space-y-4">
                <div class="grid grid-cols-2 gap-3 sm:grid-cols-4">
                    <label class="block text-xs text-slate-300">
                        "Urgent Low (mg/dL)"
                        <input
                            type="number"
                            min=MIN_ALERT_THRESHOLD_MG_DL
                            max=MAX_ALERT_THRESHOLD_MG_DL
                            step="1"
                            value=move || urgent_low_draft.get()
                            prop:value=move || urgent_low_draft.get()
                            on:input=move |ev| urgent_low_draft.set(event_target_value(&ev))
                            on:change=move |_| commit_threshold_drafts()
                            on:blur=move |_| commit_threshold_drafts()
                            class="mt-1 w-full rounded-lg border border-rose-400/25 bg-slate-900 px-2.5 py-1.5 font-mono text-sm text-white"
                        />
                    </label>
                    <label class="block text-xs text-slate-300">
                        "Target Low (mg/dL)"
                        <input
                            type="number"
                            min=MIN_ALERT_THRESHOLD_MG_DL
                            max=MAX_ALERT_THRESHOLD_MG_DL
                            step="1"
                            value=move || low_draft.get()
                            prop:value=move || low_draft.get()
                            on:input=move |ev| low_draft.set(event_target_value(&ev))
                            on:change=move |_| commit_threshold_drafts()
                            on:blur=move |_| commit_threshold_drafts()
                            class="mt-1 w-full rounded-lg border border-emerald-400/25 bg-slate-900 px-2.5 py-1.5 font-mono text-sm text-white"
                        />
                    </label>
                    <label class="block text-xs text-slate-300">
                        "Target High (mg/dL)"
                        <input
                            type="number"
                            min=MIN_ALERT_THRESHOLD_MG_DL
                            max=MAX_ALERT_THRESHOLD_MG_DL
                            step="1"
                            value=move || high_draft.get()
                            prop:value=move || high_draft.get()
                            on:input=move |ev| high_draft.set(event_target_value(&ev))
                            on:change=move |_| commit_threshold_drafts()
                            on:blur=move |_| commit_threshold_drafts()
                            class="mt-1 w-full rounded-lg border border-emerald-400/25 bg-slate-900 px-2.5 py-1.5 font-mono text-sm text-white"
                        />
                    </label>
                    <label class="block text-xs text-slate-300">
                        "Urgent High (mg/dL)"
                        <input
                            type="number"
                            min=MIN_ALERT_THRESHOLD_MG_DL
                            max=MAX_ALERT_THRESHOLD_MG_DL
                            step="1"
                            value=move || urgent_high_draft.get()
                            prop:value=move || urgent_high_draft.get()
                            on:input=move |ev| urgent_high_draft.set(event_target_value(&ev))
                            on:change=move |_| commit_threshold_drafts()
                            on:blur=move |_| commit_threshold_drafts()
                            class="mt-1 w-full rounded-lg border border-amber-400/25 bg-slate-900 px-2.5 py-1.5 font-mono text-sm text-white"
                        />
                    </label>
                </div>

                <div class="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
                    <label class="block text-xs text-slate-300">
                        "Cooldown (min)"
                        <select
                            prop:value=move || config.get().sanitized().cooldown_minutes.to_string()
                            on:change=move |ev| {
                                if let Ok(mins) = event_target_value(&ev).parse::<u32>() {
                                    persist_update(Box::new(move |cfg| {
                                        cfg.cooldown_minutes = mins;
                                    }));
                                }
                            }
                            class="mt-1 w-full rounded-lg border border-white/10 bg-slate-900 px-2.5 py-1.5 text-xs text-white"
                        >
                            {COOLDOWN_OPTIONS.iter().copied().map(|mins| {
                                let label = format!("{mins} min");
                                view! {
                                    <option
                                        value=mins.to_string()
                                        selected=move || config.get().sanitized().cooldown_minutes == mins
                                    >
                                        {label}
                                    </option>
                                }
                            }).collect_view()}
                        </select>
                    </label>

                    <label class="block text-xs text-slate-300">
                        "Sustained (min)"
                        <select
                            prop:value=move || config.get().sanitized().sustained_minutes.to_string()
                            on:change=move |ev| {
                                if let Ok(mins) = event_target_value(&ev).parse::<u32>() {
                                    persist_update(Box::new(move |cfg| {
                                        cfg.sustained_minutes = mins;
                                    }));
                                }
                            }
                            class="mt-1 w-full rounded-lg border border-white/10 bg-slate-900 px-2.5 py-1.5 text-xs text-white"
                        >
                            {SUSTAINED_OPTIONS.iter().copied().map(|(mins, label)| {
                                view! {
                                    <option
                                        value=mins.to_string()
                                        selected=move || config.get().sanitized().sustained_minutes == mins
                                    >
                                        {label}
                                    </option>
                                }
                            }).collect_view()}
                        </select>
                    </label>

                    <label class="flex cursor-pointer items-start gap-2 rounded-xl border border-white/10 bg-slate-950/60 p-2.5 text-xs text-slate-200">
                        <input
                            type="checkbox"
                            prop:checked=move || config.get().notify_on_rapid_change
                            on:change=move |ev| {
                                let checked = event_target_checked(&ev);
                                persist_update(Box::new(move |cfg| {
                                    cfg.notify_on_rapid_change = checked;
                                }));
                            }
                            class="mt-0.5 h-3.5 w-3.5 shrink-0 rounded border-white/20 bg-slate-900 accent-amber-400"
                        />
                        <span>
                            <strong class="block font-medium text-white">
                                "Rapid change (±2 mg/dL/min)"
                            </strong>
                            <span class="text-[11px] text-slate-400">
                                "Alert when approaching low/high bounds rapidly"
                            </span>
                        </span>
                    </label>

                    <label class="flex cursor-pointer items-start gap-2 rounded-xl border border-white/10 bg-slate-950/60 p-2.5 text-xs text-slate-200">
                        <input
                            type="checkbox"
                            prop:checked=move || config.get().privacy_safe_lockscreen
                            on:change=move |ev| {
                                let checked = event_target_checked(&ev);
                                persist_update(Box::new(move |cfg| {
                                    cfg.privacy_safe_lockscreen = checked;
                                }));
                            }
                            class="mt-0.5 h-3.5 w-3.5 shrink-0 rounded border-white/20 bg-slate-900 accent-emerald-400"
                        />
                        <span>
                            <strong class="block font-medium text-white">
                                "Privacy-safe lock-screen text"
                            </strong>
                            <span class="text-[11px] text-slate-400">
                                "Hide exact mg/dL on OS/browser notifications"
                            </span>
                        </span>
                    </label>
                </div>
            </form>

            // 4. Browser & Native App Notification Channel Controls
            <div
                data-alert-channels="true"
                class="rounded-xl border border-white/10 bg-slate-950/70 p-4 space-y-3 text-xs"
            >
                {move || {
                    if let Some(status) = app_alert_status.get().filter(|s| s.enabled) {
                        let platform_badge = native_platform_badge(&status);
                        let perm_granted = status.permission_granted;
                        let (perm_label, perm_cls) = if perm_granted {
                            (
                                "Granted",
                                "border-emerald-400/35 bg-emerald-500/15 text-emerald-200",
                            )
                        } else {
                            (
                                "Permission required",
                                "border-amber-400/35 bg-amber-500/15 text-amber-200",
                            )
                        };
                        let hc_ready = health_connect_ready.get();

                        view! {
                            <div class="flex flex-wrap items-center justify-between gap-3">
                                <div class="flex flex-wrap items-center gap-2">
                                    <span class="rounded-full border border-cyan-400/35 bg-cyan-500/15 px-2.5 py-0.5 text-[11px] font-semibold text-cyan-200">
                                        {platform_badge}
                                    </span>
                                    <span class=format!(
                                        "rounded-full border px-2.5 py-0.5 text-[11px] font-semibold {perm_cls}"
                                    )>
                                        {perm_label}
                                    </span>
                                    {(!status.channel_id.is_empty()).then(|| view! {
                                        <span class="font-mono text-[10px] text-slate-400">
                                            {format!("channel {}", status.channel_id)}
                                        </span>
                                    })}
                                </div>

                                <div class="flex flex-wrap items-center gap-2">
                                    {(!perm_granted).then(|| view! {
                                        <button
                                            type="button"
                                            on:click=move |_| on_request_app_permission.run(())
                                            class="rounded-full bg-emerald-400 px-3.5 py-1.5 text-xs font-semibold text-slate-950 transition hover:bg-emerald-300"
                                        >
                                            "Enable App Notifications"
                                        </button>
                                    })}
                                    <button
                                        type="button"
                                        on:click=move |_| on_send_test_alert.run(())
                                        class="rounded-full border border-cyan-400/40 bg-cyan-500/15 px-3.5 py-1.5 text-xs font-semibold text-cyan-100 transition hover:bg-cyan-500/25"
                                    >
                                        "Send Test Alert"
                                    </button>
                                </div>
                            </div>

                            <div class="flex flex-wrap items-center justify-between gap-3 border-t border-white/5 pt-2.5">
                                <label class="inline-flex cursor-pointer items-center gap-2 text-xs text-slate-200">
                                    <input
                                        type="checkbox"
                                        prop:checked=move || config.get().app_notifications_enabled
                                        on:change=move |ev| {
                                            let checked = event_target_checked(&ev);
                                            persist_update(Box::new(move |cfg| {
                                                cfg.app_notifications_enabled = checked;
                                            }));
                                        }
                                        class="h-3.5 w-3.5 rounded border-white/20 bg-slate-900 accent-emerald-400"
                                    />
                                    <span>"Deliver native app notifications on personal readings"</span>
                                </label>

                                {hc_ready.then(|| view! {
                                    <label class="inline-flex items-center gap-2 text-xs text-slate-300">
                                        <span>"Auto-check Health Connect"</span>
                                        <select
                                            prop:value=move || config.get().sanitized().auto_poll_minutes.to_string()
                                            on:change=move |ev| {
                                                if let Ok(mins) = event_target_value(&ev).parse::<u32>() {
                                                    persist_update(Box::new(move |cfg| {
                                                        cfg.auto_poll_minutes = mins;
                                                    }));
                                                }
                                            }
                                            class="rounded-lg border border-emerald-400/30 bg-slate-900 px-2.5 py-1 text-xs text-white"
                                        >
                                            {AUTO_POLL_OPTIONS.iter().copied().map(|(mins, label)| {
                                                view! {
                                                    <option
                                                        value=mins.to_string()
                                                        selected=move || config.get().sanitized().auto_poll_minutes == mins
                                                    >
                                                        {label}
                                                    </option>
                                                }
                                            }).collect_view()}
                                        </select>
                                    </label>
                                })}
                            </div>
                        }
                        .into_any()
                    } else {
                        let perm = browser_permission.get();
                        let (perm_label, perm_cls) = browser_permission_badge(perm);
                        view! {
                            <div class="flex flex-wrap items-center justify-between gap-3">
                                <div class="flex flex-wrap items-center gap-2">
                                    <span class="font-semibold text-slate-200">
                                        "Browser Notifications"
                                    </span>
                                    <span class=format!(
                                        "rounded-full border px-2.5 py-0.5 text-[11px] font-semibold {perm_cls}"
                                    )>
                                        {perm_label}
                                    </span>
                                </div>

                                <div class="flex flex-wrap items-center gap-2">
                                    {(perm == BrowserNotificationPermission::Default).then(|| view! {
                                        <button
                                            type="button"
                                            on:click=move |_| on_request_browser_permission.run(())
                                            class="rounded-full bg-emerald-400 px-3.5 py-1.5 text-xs font-semibold text-slate-950 transition hover:bg-emerald-300"
                                        >
                                            "Enable Browser Notifications"
                                        </button>
                                    })}
                                    <button
                                        type="button"
                                        on:click=move |_| on_send_test_alert.run(())
                                        class="rounded-full border border-cyan-400/40 bg-cyan-500/15 px-3.5 py-1.5 text-xs font-semibold text-cyan-100 transition hover:bg-cyan-500/25"
                                    >
                                        "Send Test Alert"
                                    </button>
                                </div>
                            </div>

                            {(perm == BrowserNotificationPermission::Granted).then(|| view! {
                                <div class="border-t border-white/5 pt-2.5">
                                    <label class="inline-flex cursor-pointer items-center gap-2 text-xs text-slate-200">
                                        <input
                                            type="checkbox"
                                            prop:checked=move || config.get().browser_notifications_enabled
                                            on:change=move |ev| {
                                                let checked = event_target_checked(&ev);
                                                persist_update(Box::new(move |cfg| {
                                                    cfg.browser_notifications_enabled = checked;
                                                }));
                                            }
                                            class="h-3.5 w-3.5 rounded border-white/20 bg-slate-900 accent-emerald-400"
                                        />
                                        <span>"Deliver browser notifications when personal readings breach target range"</span>
                                    </label>
                                </div>
                            })}
                        }
                        .into_any()
                    }
                }}

                {move || alert_feedback.get().map(|msg| view! {
                    <p data-alert-feedback="true" class="text-[11px] text-cyan-200">
                        {msg}
                    </p>
                })}
            </div>

            // 5. Custom Target-Range Breakdown & Recent Breach Log
            {move || {
                let sum = summary.get();
                let below = sum.time_below_target_pct.clamp(0.0, 100.0);
                let in_target = sum.time_in_target_pct.clamp(0.0, 100.0);
                let above = sum.time_above_target_pct.clamp(0.0, 100.0);
                let total_breaches = sum.breaches.len();
                let recent_breaches: Vec<RangeBreachEpisode> = sum
                    .breaches
                    .iter()
                    .rev()
                    .take(MAX_RECENT_BREACH_ROWS)
                    .cloned()
                    .collect();

                view! {
                    <div class="space-y-3">
                        <div class="flex flex-wrap items-center justify-between gap-2 text-xs">
                            <span class="font-semibold text-slate-200">
                                {format!(
                                    "Custom Target-Range Breakdown ({:.0}–{:.0} mg/dL · {} readings)",
                                    sum.target_low_mg_dl,
                                    sum.target_high_mg_dl,
                                    sum.reading_count,
                                )}
                            </span>
                            <div class="flex flex-wrap items-center gap-3 font-mono text-[11px]">
                                <span class="text-rose-300">
                                    {format!("Below target: {below:.1}%")}
                                </span>
                                <span class="text-emerald-300">
                                    {format!("In custom target: {in_target:.1}%")}
                                </span>
                                <span class="text-amber-300">
                                    {format!("Above target: {above:.1}%")}
                                </span>
                            </div>
                        </div>

                        <div
                            data-custom-target-bar="true"
                            class="flex h-3 w-full overflow-hidden rounded-full border border-white/10 bg-slate-900"
                        >
                            {(below > 0.0).then(|| view! {
                                <div
                                    style=format!("width: {below:.2}%")
                                    class="h-full bg-rose-500"
                                    title=format!("Below target: {below:.1}%")
                                />
                            })}
                            {(in_target > 0.0).then(|| view! {
                                <div
                                    style=format!("width: {in_target:.2}%")
                                    class="h-full bg-emerald-400"
                                    title=format!("In custom target: {in_target:.1}%")
                                />
                            })}
                            {(above > 0.0).then(|| view! {
                                <div
                                    style=format!("width: {above:.2}%")
                                    class="h-full bg-amber-400"
                                    title=format!("Above target: {above:.1}%")
                                />
                            })}
                        </div>

                        {(sum.urgent_low_readings > 0 || sum.urgent_high_readings > 0).then(|| view! {
                            <div class="flex flex-wrap gap-2 text-[11px] font-mono">
                                {(sum.urgent_low_readings > 0).then(|| view! {
                                    <span class="rounded-full border border-rose-400/40 bg-rose-500/15 px-2.5 py-0.5 text-rose-200">
                                        {format!(
                                            "Urgent low (<{:.0}): {} reading(s)",
                                            sum.urgent_low_mg_dl, sum.urgent_low_readings
                                        )}
                                    </span>
                                })}
                                {(sum.urgent_high_readings > 0).then(|| view! {
                                    <span class="rounded-full border border-orange-400/40 bg-orange-500/15 px-2.5 py-0.5 text-orange-200">
                                        {format!(
                                            "Urgent high (>{:.0}): {} reading(s)",
                                            sum.urgent_high_mg_dl, sum.urgent_high_readings
                                        )}
                                    </span>
                                })}
                            </div>
                        })}

                        <div class="rounded-xl border border-white/10 bg-slate-950/60 p-3.5 space-y-2">
                            <div class="flex flex-wrap items-center justify-between gap-2 text-xs">
                                <span class="font-semibold text-white">"Recent Range Breaches"</span>
                                <span class="text-[11px] text-slate-400">
                                    {if total_breaches > MAX_RECENT_BREACH_ROWS {
                                        format!(
                                            "Showing newest {MAX_RECENT_BREACH_ROWS} of {total_breaches} breach episode(s)"
                                        )
                                    } else {
                                        format!("{total_breaches} breach episode(s) in active window")
                                    }}
                                </span>
                            </div>

                            {if recent_breaches.is_empty() {
                                view! {
                                    <p class="text-xs text-slate-400">
                                        "No target-range breaches in the active window."
                                    </p>
                                }
                                .into_any()
                            } else {
                                view! {
                                    <ul data-recent-breach-list="true" class="divide-y divide-white/5 text-xs">
                                        {recent_breaches.into_iter().map(|breach| {
                                            let start_ts = format_short_timestamp(&breach.start_timestamp);
                                            let end_ts = format_short_timestamp(&breach.end_timestamp);
                                            let span_str = if start_ts == end_ts {
                                                start_ts
                                            } else {
                                                format!("{start_ts} → {end_ts}")
                                            };
                                            let duration_str = format_breach_duration(
                                                breach.duration_minutes,
                                                breach.reading_count,
                                            );
                                            let extreme_str = format_breach_extreme(&breach);
                                            let sev = breach.severity;
                                            let capped = breach.sensor_capped;

                                            view! {
                                                <li
                                                    data-breach-row="true"
                                                    class="flex flex-wrap items-center justify-between gap-2 py-2"
                                                >
                                                    <div class="flex flex-wrap items-center gap-2">
                                                        <span class=format!(
                                                            "rounded-full border px-2 py-0.5 text-[10px] font-semibold {}",
                                                            sev.badge_class()
                                                        )>
                                                            {sev.label()}
                                                        </span>
                                                        <span class="font-mono text-[11px] text-slate-300">
                                                            {span_str}
                                                        </span>
                                                    </div>
                                                    <div class="flex flex-wrap items-center gap-2 font-mono text-[11px]">
                                                        <span class="text-slate-400">{duration_str}</span>
                                                        <span class="font-semibold text-white">{extreme_str}</span>
                                                        {capped.then(|| view! {
                                                            <span class="rounded-full border border-orange-400/40 bg-orange-500/15 px-2 py-0.5 text-[10px] font-semibold text-orange-200">
                                                                "Sensor cap"
                                                            </span>
                                                        })}
                                                    </div>
                                                </li>
                                            }
                                        }).collect_view()}
                                    </ul>
                                }
                                .into_any()
                            }}
                        </div>
                    </div>
                }
            }}
        </section>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mental_health_cgm_alerts::{evaluate_window_range_alerts, AlertDispatchState};
    use crate::mental_health_lingo::{GlucoseReadingBound, LingoReading};
    use leptos::prelude::RenderHtml;

    fn synthetic_reading(
        timestamp: &str,
        hour_offset: f64,
        glucose_mg_dl: f64,
        bound: GlucoseReadingBound,
    ) -> LingoReading {
        LingoReading {
            timestamp: timestamp.to_string(),
            hour_offset,
            glucose_mg_dl,
            lingo_count: None,
            tag: None,
            bound,
        }
    }

    fn synthetic_breaching_trace() -> Vec<LingoReading> {
        let mut readings = Vec::new();
        // 8 distinct breach episodes separated by in-range readings so we also verify the
        // 6-row DOM bound, ending on a ceiling-capped urgent high alert.
        for ep in 0..7 {
            let base_h = ep as f64 * 2.0;
            let h = (ep * 2) % 24;
            readings.push(synthetic_reading(
                &format!("2026-08-01T{h:02}:00-07:00"),
                base_h,
                102.0,
                GlucoseReadingBound::Measured,
            ));
            readings.push(synthetic_reading(
                &format!("2026-08-01T{h:02}:15-07:00"),
                base_h + 0.25,
                158.0,
                GlucoseReadingBound::Measured,
            ));
            readings.push(synthetic_reading(
                &format!("2026-08-01T{h:02}:30-07:00"),
                base_h + 0.5,
                105.0,
                GlucoseReadingBound::Measured,
            ));
        }
        // Final floor-cap dip then ceiling-cap urgent high as the latest reading.
        readings.push(synthetic_reading(
            "2026-08-01T15:00-07:00",
            15.0,
            54.0,
            GlucoseReadingBound::BelowSensorFloor,
        ));
        readings.push(synthetic_reading(
            "2026-08-01T15:15-07:00",
            15.25,
            100.0,
            GlucoseReadingBound::Measured,
        ));
        readings.push(synthetic_reading(
            "2026-08-01T15:30-07:00",
            15.5,
            201.0,
            GlucoseReadingBound::AboveSensorCeiling,
        ));
        readings
    }

    #[test]
    fn renders_browser_mode_alerts_card_with_active_alert_presets_and_bounded_breaches() {
        let cfg = RwSignal::new(GlucoseAlertConfig::default());
        let trace = synthetic_breaching_trace();
        let summary_val = evaluate_window_range_alerts(
            &trace,
            &cfg.get_untracked(),
            &AlertDispatchState::default(),
        );
        assert!(summary_val.breaches.len() > MAX_RECENT_BREACH_ROWS);

        let summary = RwSignal::new(summary_val);
        let browser_perm = RwSignal::new(BrowserNotificationPermission::Default);
        let app_status = RwSignal::new(Option::<AppAlertStatusUi>::None);
        let hc_ready = RwSignal::new(false);
        let personal_loaded = RwSignal::new(false);
        let feedback = RwSignal::new(Some("Browser notification preview ready.".to_string()));

        let html = view! {
            <GlucoseRangeAlertsCard
                config=cfg
                summary=summary.into()
                browser_permission=browser_perm.into()
                app_alert_status=app_status.into()
                health_connect_ready=hc_ready.into()
                personal_data_loaded=personal_loaded.into()
                alert_feedback=feedback.into()
                on_request_browser_permission=Callback::new(|()| {})
                on_request_app_permission=Callback::new(|()| {})
                on_send_test_alert=Callback::new(|()| {})
            />
        }
        .to_html();

        assert!(html.contains("Automated Glucose Range Alerts"));
        assert!(html.contains("Urgent High"));
        assert!(html.contains("Target 70–140 · Urgent &lt;55 / &gt;180 mg/dL"));
        assert!(html.contains("Urgent High Glucose — &gt;200 (sensor ceiling)"));
        assert!(html.contains("Tight / Lingo (70–140)"));
        assert!(html.contains("ADA Standard (70–180)"));
        assert!(html.contains("Reset defaults"));
        assert!(html.contains("Hide exact mg/dL on OS/browser notifications"));
        assert!(html.contains("Browser Notifications"));
        assert!(html.contains("Enable Browser Notifications"));
        assert!(html.contains("Send Test Alert"));
        assert!(html.contains("Browser notification preview ready."));
        assert!(html.contains("Sensor cap"));
        assert!(html.contains("Showing newest 6 of"));
        assert_eq!(
            html.matches("data-breach-row=\"true\"").count(),
            MAX_RECENT_BREACH_ROWS
        );
        assert!(!html.contains("Auto-check Health Connect"));
    }

    #[test]
    fn renders_native_personal_app_mode_for_android_and_ios_with_auto_poll_control() {
        let cfg = RwSignal::new(GlucoseAlertConfig::from_preset(
            GlucoseTargetPreset::AdaStandard,
        ));
        let trace = vec![
            synthetic_reading(
                "2026-08-01T08:00-07:00",
                0.0,
                96.0,
                GlucoseReadingBound::Measured,
            ),
            synthetic_reading(
                "2026-08-01T08:15-07:00",
                0.25,
                118.0,
                GlucoseReadingBound::Measured,
            ),
        ];
        let summary = RwSignal::new(evaluate_window_range_alerts(
            &trace,
            &cfg.get_untracked(),
            &AlertDispatchState::default(),
        ));
        let browser_perm = RwSignal::new(BrowserNotificationPermission::Unsupported);
        let android_status = RwSignal::new(Some(AppAlertStatusUi {
            enabled: true,
            platform: "android_notification_manager".to_string(),
            permission_granted: false,
            permission_status: "prompt".to_string(),
            channel_id: "dabney_glucose_alerts".to_string(),
        }));
        let hc_ready = RwSignal::new(true);
        let personal_loaded = RwSignal::new(true);
        let feedback = RwSignal::new(Option::<String>::None);

        let android_html = view! {
            <GlucoseRangeAlertsCard
                config=cfg
                summary=summary.into()
                browser_permission=browser_perm.into()
                app_alert_status=android_status.into()
                health_connect_ready=hc_ready.into()
                personal_data_loaded=personal_loaded.into()
                alert_feedback=feedback.into()
                on_request_browser_permission=Callback::new(|()| {})
                on_request_app_permission=Callback::new(|()| {})
                on_send_test_alert=Callback::new(|()| {})
            />
        }
        .to_html();

        assert!(android_html.contains("In Target Range"));
        assert!(android_html.contains("Target 70–180 · Urgent &lt;54 / &gt;250 mg/dL"));
        assert!(android_html.contains("Android Notifications"));
        assert!(android_html.contains("Permission required"));
        assert!(android_html.contains("Enable App Notifications"));
        assert!(android_html.contains("Auto-check Health Connect"));
        assert!(android_html.contains("Send Test Alert"));
        assert!(android_html.contains("No target-range breaches in the active window."));
        assert!(!android_html.contains("Enable Browser Notifications"));

        let ios_status = RwSignal::new(Some(AppAlertStatusUi {
            enabled: true,
            platform: "ios_user_notifications".to_string(),
            permission_granted: true,
            permission_status: "granted".to_string(),
            channel_id: "dabney_glucose_alerts".to_string(),
        }));
        let ios_html = view! {
            <GlucoseRangeAlertsCard
                config=cfg
                summary=summary.into()
                browser_permission=browser_perm.into()
                app_alert_status=ios_status.into()
                health_connect_ready=hc_ready.into()
                personal_data_loaded=personal_loaded.into()
                alert_feedback=feedback.into()
                on_request_browser_permission=Callback::new(|()| {})
                on_request_app_permission=Callback::new(|()| {})
                on_send_test_alert=Callback::new(|()| {})
            />
        }
        .to_html();

        assert!(ios_html.contains("Apple Notifications"));
        assert!(ios_html.contains("Granted"));
        assert!(!ios_html.contains("Enable App Notifications"));
        assert!(ios_html.contains("Auto-check Health Connect"));
    }
}
