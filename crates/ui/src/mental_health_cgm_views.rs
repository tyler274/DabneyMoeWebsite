//! Longitudinal CGM visualization components for Dabney.moe `/mental-health`.
//!
//! Renders honest sensor-saturation caveats, trailing 14-day vs. prior 14-day comparisons,
//! four-block circadian summaries, ATTD sustained glycemic episodes + 15-minute velocity,
//! and compact per-day breakdowns with continuous wear-session / gap summaries.

use leptos::prelude::*;

use crate::mental_health_cgm::{ClinicalGlucoseSummary, GlucoseImportDiagnostics};
use crate::mental_health_cgm_analytics::{
    CircadianPeriodSummary, DailyGlucoseSummary, EpisodeCounts, EpisodeKind, GlycemicEpisode,
    PeriodComparison, RateOfChangeSummary, SensorGap, SensorSession,
};

fn format_duration_minutes(minutes: f64) -> String {
    if minutes <= 0.0 {
        "0m".to_string()
    } else if minutes < 60.0 {
        format!("{:.0}m", minutes.round())
    } else if minutes < 48.0 * 60.0 {
        format!("{:.1}h", minutes / 60.0)
    } else {
        format!("{:.1}d", minutes / 1440.0)
    }
}

fn format_hours_compact(hours: f64) -> String {
    if hours <= 0.0 {
        "0h".to_string()
    } else if hours < 48.0 {
        format!("{hours:.1}h")
    } else {
        format!("{:.1}d", hours / 24.0)
    }
}

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

/// Window-specific sensor floor/ceiling saturation & `"Out of range"` caveat banner.
///
/// Renders ONLY when the active window contains floor-capped (`≤54` = `<55`) or
/// ceiling-capped (`≥201` = `>200`) readings, or when import diagnostics recorded
/// `"Out of range"` status rows.
#[component]
pub fn SensorSaturationBanner(
    summary: ClinicalGlucoseSummary,
    diagnostics: Option<GlucoseImportDiagnostics>,
) -> impl IntoView {
    let out_of_range_rows = diagnostics
        .as_ref()
        .map(|d| d.out_of_range_rows)
        .unwrap_or(0);
    if !summary.has_sensor_saturation() && out_of_range_rows == 0 {
        return None;
    }

    let monitor_name = summary.monitor.display_name();
    let has_ceiling = summary.ceiling_capped_readings > 0;
    let has_floor = summary.floor_capped_readings > 0;

    Some(view! {
        <section
            data-sensor-saturation-banner="true"
            class="mb-8 rounded-2xl border border-amber-400/35 bg-amber-500/10 p-5 text-xs text-slate-200 space-y-3"
        >
            <div class="flex flex-wrap items-center justify-between gap-2">
                <div class="flex items-center gap-2">
                    <span class="inline-block h-2.5 w-2.5 rounded-full bg-orange-400"></span>
                    <h2 class="text-sm font-semibold text-amber-100">
                        "Sensor Saturation & Bound Clipping Notice"
                    </h2>
                </div>
                <span class="rounded-full border border-amber-300/30 bg-slate-950/60 px-2.5 py-0.5 text-[11px] font-medium text-amber-200">
                    {monitor_name}
                </span>
            </div>

            <div class="flex flex-wrap gap-2">
                {has_ceiling.then(|| view! {
                    <span class="inline-flex items-center gap-1.5 rounded-full border border-orange-400/40 bg-orange-500/15 px-3 py-1 font-mono text-[11px] font-semibold text-orange-200">
                        {format!(
                            "Ceiling-capped (≥201 / >200): {} ({:.1}%)",
                            summary.ceiling_capped_readings, summary.ceiling_capped_pct
                        )}
                    </span>
                })}
                {has_floor.then(|| view! {
                    <span class="inline-flex items-center gap-1.5 rounded-full border border-rose-400/40 bg-rose-500/15 px-3 py-1 font-mono text-[11px] font-semibold text-rose-200">
                        {format!(
                            "Floor-capped (≤54 / <55): {} ({:.1}%)",
                            summary.floor_capped_readings, summary.floor_capped_pct
                        )}
                    </span>
                })}
                {(out_of_range_rows > 0).then(|| view! {
                    <span class="inline-flex items-center gap-1.5 rounded-full border border-amber-400/40 bg-amber-500/15 px-3 py-1 font-mono text-[11px] font-semibold text-amber-200">
                        {format!("Out-of-range status rows (full export): {out_of_range_rows}")}
                    </span>
                })}
            </div>

            <div class="space-y-1.5 text-[11px] leading-relaxed text-slate-200">
                {has_ceiling.then(|| view! {
                    <p>
                        <strong class="text-orange-200">"Ceiling clipping: "</strong>
                        "Because readings above the sensor ceiling are reported at the cap (≥201 = >200 mg/dL), "
                        <strong>"Mean, GMI (est. A1C), and TAR >250 are lower bounds"</strong>
                        " in this window, and "
                        <strong>"SD / CV% may be artificially suppressed"</strong>
                        " during flat-line ceiling plateaus."
                    </p>
                })}
                {has_floor.then(|| view! {
                    <p>
                        <strong class="text-rose-200">"Floor clipping: "</strong>
                        "Readings at the sensor floor (≤54 = <55 mg/dL) cannot distinguish 54 from <54 mg/dL; "
                        <strong>"TBR <54 may be understated"</strong>
                        " — review the sustained Level 2 / floor hypoglycemia episode count below."
                    </p>
                })}
                {(out_of_range_rows > 0).then(|| view! {
                    <p>
                        <strong class="text-amber-200">"Out-of-range rows: "</strong>
                        {format!(
                            "{out_of_range_rows} non-numeric \"Out of range\" status row(s) were preserved in diagnostics separately from malformed CSV rows; these commonly occur alongside sustained sensor ceiling or floor saturation."
                        )}
                    </p>
                })}
            </div>
        </section>
    })
}

/// Trailing 14-day vs. immediately preceding 14-day clinical trend comparison card.
#[component]
pub fn PeriodComparisonCard(comparison: PeriodComparison) -> impl IntoView {
    let prior_truncated = comparison.prior_days < comparison.recent_days - 1.0;

    let delta_color = |delta: f64, higher_is_better: bool| -> &'static str {
        if delta.abs() < 0.05 {
            "text-slate-400"
        } else if (delta > 0.0) == higher_is_better {
            "text-emerald-300"
        } else {
            "text-rose-300"
        }
    };

    let mean_cls = delta_color(comparison.delta_mean_mg_dl, false);
    let gmi_cls = delta_color(comparison.delta_gmi_pct, false);
    let tir_cls = delta_color(comparison.delta_tir_pct, true);
    let titr_cls = delta_color(comparison.delta_titr_pct, true);
    let tbr_cls = delta_color(comparison.delta_tbr_pct, false);
    let ceil_cls = delta_color(comparison.delta_ceiling_capped_pct, false);

    view! {
        <section
            data-period-comparison-card="true"
            class="mb-8 rounded-2xl border border-white/10 bg-white/[0.02] p-6 space-y-4"
        >
            <div class="flex flex-wrap items-center justify-between gap-3">
                <div>
                    <h2 class="text-lg font-semibold text-white">
                        "14-Day Trend vs. Prior 14 Days"
                    </h2>
                    <p class="text-sm text-slate-400">
                        {format!(
                            "Trailing {:.1}d ({} readings) compared against prior {:.1}d ({} readings).",
                            comparison.recent_days,
                            comparison.recent_readings,
                            comparison.prior_days,
                            comparison.prior_readings
                        )}
                    </p>
                </div>
                <span class="rounded-full border border-cyan-400/25 bg-cyan-500/10 px-3 py-1 text-xs font-medium text-cyan-200">
                    "Period-over-Period"
                </span>
            </div>

            {prior_truncated.then(|| view! {
                <div class="rounded-xl border border-amber-400/30 bg-amber-500/10 px-3.5 py-2 text-xs text-amber-200">
                    {format!(
                        "Coverage note: prior comparison window spans {:.1}d vs. {:.1}d recent due to sensor gaps — interpret deltas with partial coverage in mind.",
                        comparison.prior_days, comparison.recent_days
                    )}
                </div>
            })}

            <div class="grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-6">
                <div class="rounded-xl border border-white/10 bg-slate-950/60 p-3">
                    <p class="text-[11px] font-medium text-slate-400">"Mean Glucose"</p>
                    <p class="mt-1 font-mono text-lg font-bold text-cyan-300">
                        {format!("{:.0} mg/dL", comparison.recent_mean_mg_dl)}
                    </p>
                    <p class=format!("mt-0.5 font-mono text-xs font-semibold {mean_cls}")>
                        {format!("{:+.1} mg/dL", comparison.delta_mean_mg_dl)}
                    </p>
                    <p class="text-[10px] text-slate-500">
                        {format!("Prior {:.0} mg/dL", comparison.prior_mean_mg_dl)}
                    </p>
                </div>

                <div class="rounded-xl border border-white/10 bg-slate-950/60 p-3">
                    <p class="text-[11px] font-medium text-slate-400">"GMI (est. A1C)"</p>
                    <p class="mt-1 font-mono text-lg font-bold text-cyan-200">
                        {format!("{:.2}%", comparison.recent_gmi_pct)}
                    </p>
                    <p class=format!("mt-0.5 font-mono text-xs font-semibold {gmi_cls}")>
                        {format!("{:+.2}%", comparison.delta_gmi_pct)}
                    </p>
                    <p class="text-[10px] text-slate-500">
                        {format!("Prior {:.2}%", comparison.prior_gmi_pct)}
                    </p>
                </div>

                <div class="rounded-xl border border-white/10 bg-slate-950/60 p-3">
                    <p class="text-[11px] font-medium text-slate-400">"TIR 70–180"</p>
                    <p class="mt-1 font-mono text-lg font-bold text-emerald-300">
                        {format!("{:.1}%", comparison.recent_tir_pct)}
                    </p>
                    <p class=format!("mt-0.5 font-mono text-xs font-semibold {tir_cls}")>
                        {format!("{:+.1} pp", comparison.delta_tir_pct)}
                    </p>
                    <p class="text-[10px] text-slate-500">
                        {format!("Prior {:.1}%", comparison.prior_tir_pct)}
                    </p>
                </div>

                <div class="rounded-xl border border-white/10 bg-slate-950/60 p-3">
                    <p class="text-[11px] font-medium text-slate-400">"Tight TIR 70–140"</p>
                    <p class="mt-1 font-mono text-lg font-bold text-teal-300">
                        {format!("{:.1}%", comparison.recent_titr_pct)}
                    </p>
                    <p class=format!("mt-0.5 font-mono text-xs font-semibold {titr_cls}")>
                        {format!("{:+.1} pp", comparison.delta_titr_pct)}
                    </p>
                    <p class="text-[10px] text-slate-500">
                        {format!("Prior {:.1}%", comparison.prior_titr_pct)}
                    </p>
                </div>

                <div class="rounded-xl border border-white/10 bg-slate-950/60 p-3">
                    <p class="text-[11px] font-medium text-slate-400">"TBR <70"</p>
                    <p class="mt-1 font-mono text-lg font-bold text-rose-300">
                        {format!("{:.1}%", comparison.recent_tbr_pct)}
                    </p>
                    <p class=format!("mt-0.5 font-mono text-xs font-semibold {tbr_cls}")>
                        {format!("{:+.1} pp", comparison.delta_tbr_pct)}
                    </p>
                    <p class="text-[10px] text-slate-500">
                        {format!("Prior {:.1}%", comparison.prior_tbr_pct)}
                    </p>
                </div>

                <div class="rounded-xl border border-white/10 bg-slate-950/60 p-3">
                    <p class="text-[11px] font-medium text-slate-400">"Ceiling-Cap %"</p>
                    <p class="mt-1 font-mono text-lg font-bold text-orange-300">
                        {format!("{:.1}%", comparison.recent_ceiling_capped_pct)}
                    </p>
                    <p class=format!("mt-0.5 font-mono text-xs font-semibold {ceil_cls}")>
                        {format!("{:+.1} pp", comparison.delta_ceiling_capped_pct)}
                    </p>
                    <p class="text-[10px] text-slate-500">
                        {format!("Prior {:.1}%", comparison.prior_ceiling_capped_pct)}
                    </p>
                </div>
            </div>
        </section>
    }
}

/// Four-quadrant local clock-time circadian breakdown (Overnight, Morning, Afternoon, Evening).
#[component]
pub fn CircadianBreakdownSection(periods: Vec<CircadianPeriodSummary>) -> impl IntoView {
    let any_data = periods.iter().any(|p| p.reading_count > 0);
    if !any_data {
        return None;
    }

    Some(view! {
        <section
            data-circadian-breakdown="true"
            class="mb-8 rounded-2xl border border-white/10 bg-white/[0.02] p-6 space-y-4"
        >
            <div>
                <h2 class="text-lg font-semibold text-white">
                    "Circadian Breakdown (Local Clock Time)"
                </h2>
                <p class="text-sm text-slate-400">
                    "Mean, median, 95th percentile, TIR 70–180%, TBR <70%, and ceiling saturation across four 6-hour periods."
                </p>
            </div>

            <div class="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
                {periods.into_iter().map(|p| {
                    let has_readings = p.reading_count > 0;
                    let tbr_cls = if p.tbr_lt_70_pct > 0.0 {
                        "text-rose-300"
                    } else {
                        "text-slate-300"
                    };
                    let ceil_cls = if p.ceiling_capped_pct > 0.0 {
                        "text-orange-300"
                    } else {
                        "text-slate-400"
                    };
                    let tir_w = p.tir_70_180_pct.clamp(0.0, 100.0);
                    let tbr_w = p.tbr_lt_70_pct.clamp(0.0, 100.0 - tir_w);
                    let tar_w = (100.0 - tir_w - tbr_w).max(0.0);

                    view! {
                        <div class="rounded-xl border border-white/10 bg-slate-950/60 p-4 space-y-3">
                            <div class="flex items-center justify-between gap-2">
                                <div>
                                    <p class="text-sm font-semibold text-white">{p.period.label()}</p>
                                    <p class="font-mono text-[11px] text-slate-400">{p.period.hours_label()}</p>
                                </div>
                                <span class="rounded-full border border-white/10 bg-slate-900 px-2 py-0.5 font-mono text-[10px] text-slate-300">
                                    {format!("{} rdgs", p.reading_count)}
                                </span>
                            </div>

                            {if has_readings {
                                view! {
                                    <div class="space-y-2.5">
                                        <div>
                                            <p class="font-mono text-2xl font-bold text-cyan-300">
                                                {format!("{:.0} mg/dL", p.mean_mg_dl)}
                                            </p>
                                            <p class="font-mono text-[11px] text-slate-400">
                                                {format!("Med {:.0} · p95 {:.0}", p.median_mg_dl, p.p95_mg_dl)}
                                            </p>
                                        </div>

                                        <div class="flex h-2 w-full overflow-hidden rounded-full bg-slate-900">
                                            <div style=format!("width: {tbr_w:.1}%") class="bg-rose-400"></div>
                                            <div style=format!("width: {tir_w:.1}%") class="bg-emerald-400"></div>
                                            <div style=format!("width: {tar_w:.1}%") class="bg-amber-400"></div>
                                        </div>

                                        <div class="grid grid-cols-3 gap-1.5 text-[11px]">
                                            <div class="rounded-lg border border-white/5 bg-slate-900/70 px-2 py-1">
                                                <p class="text-[10px] text-slate-400">"TIR"</p>
                                                <p class="font-mono font-semibold text-emerald-300">{format!("{:.1}%", p.tir_70_180_pct)}</p>
                                            </div>
                                            <div class="rounded-lg border border-white/5 bg-slate-900/70 px-2 py-1">
                                                <p class="text-[10px] text-slate-400">"TBR <70"</p>
                                                <p class=format!("font-mono font-semibold {tbr_cls}")>{format!("{:.1}%", p.tbr_lt_70_pct)}</p>
                                            </div>
                                            <div class="rounded-lg border border-white/5 bg-slate-900/70 px-2 py-1">
                                                <p class="text-[10px] text-slate-400">"Ceil cap"</p>
                                                <p class=format!("font-mono font-semibold {ceil_cls}")>{format!("{:.1}%", p.ceiling_capped_pct)}</p>
                                            </div>
                                        </div>
                                    </div>
                                }
                                .into_any()
                            } else {
                                view! {
                                    <p class="text-xs text-slate-500">"No readings in this clock block."</p>
                                }
                                .into_any()
                            }}
                        </div>
                    }
                }).collect_view()}
            </div>
        </section>
    })
}

/// ATTD sustained glycemic episodes (`>= 15 min`) and 15-minute glucose velocity (RoC) panel.
#[component]
pub fn GlycemicEpisodesAndRocSection(
    episodes: Vec<GlycemicEpisode>,
    counts: EpisodeCounts,
    roc: RateOfChangeSummary,
) -> impl IntoView {
    // Bound DOM table to top 8 most recent clinically notable hypo/hyper episodes
    // (falling back to Lingo >140 excursions when no hypo or >180 episodes exist).
    let primary_notable: Vec<GlycemicEpisode> = episodes
        .iter()
        .rev()
        .filter(|e| {
            matches!(
                e.kind,
                EpisodeKind::Level2HypoLt54 | EpisodeKind::Level1HypoLt70 | EpisodeKind::HyperGt180
            )
        })
        .take(8)
        .cloned()
        .collect();
    let notable_episodes = if !primary_notable.is_empty() {
        primary_notable
    } else {
        episodes.iter().rev().take(8).cloned().collect()
    };

    view! {
        <section
            data-glycemic-episodes-roc="true"
            class="mb-8 rounded-2xl border border-white/10 bg-white/[0.02] p-6 space-y-5"
        >
            <div>
                <h2 class="text-lg font-semibold text-white">
                    "Sustained Glycemic Episodes (≥15 min) & 15-Min Rate of Change"
                </h2>
                <p class="text-sm text-slate-400">
                    "Distinct ATTD sustained events (broken across >30 min sensor gaps) paired with unbroken 15-minute glucose velocity."
                </p>
            </div>

            <div class="grid grid-cols-2 gap-3 lg:grid-cols-4">
                <div class="rounded-xl border border-rose-400/20 bg-slate-950/60 p-3.5">
                    <p class="text-xs font-medium text-slate-400">"Level 2 Hypo (<54 / <55 floor)"</p>
                    <p class="mt-1 font-mono text-2xl font-bold text-red-400">{counts.level2_hypo_count}</p>
                    <p class="mt-0.5 text-[11px] text-slate-400">
                        {format!("Total duration: {}", format_duration_minutes(counts.level2_hypo_minutes))}
                    </p>
                </div>

                <div class="rounded-xl border border-rose-400/15 bg-slate-950/60 p-3.5">
                    <p class="text-xs font-medium text-slate-400">"Level 1 Hypo (<70 mg/dL)"</p>
                    <p class="mt-1 font-mono text-2xl font-bold text-rose-300">{counts.level1_hypo_count}</p>
                    <p class="mt-0.5 text-[11px] text-slate-400">
                        {format!("Total duration: {}", format_duration_minutes(counts.level1_hypo_minutes))}
                    </p>
                </div>

                <div class="rounded-xl border border-orange-400/20 bg-slate-950/60 p-3.5">
                    <p class="text-xs font-medium text-slate-400">"Hyperglycemia (>180 mg/dL)"</p>
                    <p class="mt-1 font-mono text-2xl font-bold text-orange-300">{counts.hyper_gt_180_count}</p>
                    <p class="mt-0.5 text-[11px] text-slate-400">
                        {format!("Total duration: {}", format_duration_minutes(counts.hyper_gt_180_minutes))}
                    </p>
                </div>

                <div class="rounded-xl border border-amber-400/20 bg-slate-950/60 p-3.5">
                    <p class="text-xs font-medium text-slate-400">"Lingo Excursions (>140)"</p>
                    <p class="mt-1 font-mono text-2xl font-bold text-amber-300">{counts.lingo_excursion_gt_140_count}</p>
                    <p class="mt-0.5 text-[11px] text-slate-400">
                        {format!("Total duration: {}", format_duration_minutes(counts.lingo_excursion_gt_140_minutes))}
                    </p>
                </div>
            </div>

            // 15-min Rate of Change velocity strip
            <div class="grid gap-3 rounded-xl border border-white/10 bg-slate-950/60 p-4 text-xs sm:grid-cols-3">
                <div>
                    <p class="text-[11px] font-medium text-slate-400">"Rapid Rises (≥ +2.0 mg/dL/min)"</p>
                    <p class="mt-1 font-mono text-base font-semibold text-amber-300">
                        {format!("{} bursts · max {:+.2} mg/dL/min", roc.rapid_rise_events, roc.max_rise_mg_dl_per_min)}
                    </p>
                </div>
                <div>
                    <p class="text-[11px] font-medium text-slate-400">"Rapid Falls (≤ −2.0 mg/dL/min)"</p>
                    <p class="mt-1 font-mono text-base font-semibold text-rose-300">
                        {format!("{} bursts · max {:+.2} mg/dL/min", roc.rapid_fall_events, roc.max_fall_mg_dl_per_min)}
                    </p>
                </div>
                <div>
                    <p class="text-[11px] font-medium text-slate-400">"Mean |15-min Velocity|"</p>
                    <p class="mt-1 font-mono text-base font-semibold text-cyan-200">
                        {format!("{:.2} mg/dL/min ({} pairs)", roc.mean_abs_roc, roc.sample_pairs)}
                    </p>
                </div>
            </div>

            // Bounded top-8 recent notable episodes table
            {if notable_episodes.is_empty() {
                view! {
                    <p class="rounded-xl border border-white/5 bg-slate-950/50 px-4 py-3 text-xs text-slate-400">
                        "No sustained ≥15-minute hypoglycemia or hyperglycemia episodes detected in this window."
                    </p>
                }
                .into_any()
            } else {
                view! {
                    <div class="space-y-2">
                        <div class="flex items-center justify-between text-xs text-slate-400">
                            <span class="font-medium text-slate-200">"Most Recent Notable Episodes (up to 8)"</span>
                            <span>{format!("{} total episodes in window", episodes.len())}</span>
                        </div>
                        <div class="overflow-x-auto">
                            <table class="w-full min-w-[620px] border-collapse text-left text-xs">
                                <thead>
                                    <tr class="border-b border-white/10 text-[11px] uppercase tracking-wider text-slate-400">
                                        <th class="py-2 pr-3">"Episode"</th>
                                        <th class="py-2 pr-3">"Start → End"</th>
                                        <th class="py-2 pr-3">"Duration"</th>
                                        <th class="py-2 pr-3">"Readings"</th>
                                        <th class="py-2 pr-3">"Peak / Nadir"</th>
                                        <th class="py-2">"Sensor Cap"</th>
                                    </tr>
                                </thead>
                                <tbody class="divide-y divide-white/5 text-slate-200">
                                    {notable_episodes.into_iter().map(|ep| {
                                        let (badge_cls, extreme_prefix) = match ep.kind {
                                            EpisodeKind::Level2HypoLt54 => (
                                                "border-red-400/35 bg-red-500/15 text-red-200",
                                                "Nadir",
                                            ),
                                            EpisodeKind::Level1HypoLt70 => (
                                                "border-rose-400/35 bg-rose-500/15 text-rose-200",
                                                "Nadir",
                                            ),
                                            EpisodeKind::HyperGt180 => (
                                                "border-orange-400/35 bg-orange-500/15 text-orange-200",
                                                "Peak",
                                            ),
                                            EpisodeKind::LingoExcursionGt140 => (
                                                "border-amber-400/35 bg-amber-500/15 text-amber-200",
                                                "Peak",
                                            ),
                                        };
                                        let cap_badge = if ep.sensor_capped {
                                            match ep.kind {
                                                EpisodeKind::Level2HypoLt54 | EpisodeKind::Level1HypoLt70 => {
                                                    Some(("border-rose-400/40 bg-rose-500/15 text-rose-200", "Floor cap"))
                                                }
                                                EpisodeKind::HyperGt180 | EpisodeKind::LingoExcursionGt140 => {
                                                    Some(("border-orange-400/40 bg-orange-500/15 text-orange-200", "Ceiling cap"))
                                                }
                                            }
                                        } else {
                                            None
                                        };
                                        view! {
                                            <tr>
                                                <td class="py-2 pr-3 whitespace-nowrap">
                                                    <span class=format!("inline-flex rounded-full border px-2 py-0.5 text-[10px] font-semibold {badge_cls}")>
                                                        {ep.kind.label()}
                                                    </span>
                                                </td>
                                                <td class="py-2 pr-3 whitespace-nowrap font-mono text-[11px] text-slate-300">
                                                    {format!(
                                                        "{} → {}",
                                                        format_short_timestamp(&ep.start_timestamp),
                                                        format_short_timestamp(&ep.end_timestamp)
                                                    )}
                                                </td>
                                                <td class="py-2 pr-3 whitespace-nowrap font-mono text-slate-200">
                                                    {format_duration_minutes(ep.duration_minutes)}
                                                </td>
                                                <td class="py-2 pr-3 whitespace-nowrap font-mono text-slate-300">
                                                    {ep.reading_count}
                                                </td>
                                                <td class="py-2 pr-3 whitespace-nowrap font-mono text-cyan-200">
                                                    {format!("{extreme_prefix} {:.0} mg/dL", ep.extreme_mg_dl)}
                                                </td>
                                                <td class="py-2 whitespace-nowrap">
                                                    {match cap_badge {
                                                        Some((cls, label)) => view! {
                                                            <span class=format!("inline-flex rounded-full border px-2 py-0.5 text-[10px] font-semibold {cls}")>
                                                                {label}
                                                            </span>
                                                        }.into_any(),
                                                        None => view! {
                                                            <span class="text-[11px] text-slate-500">"Measured"</span>
                                                        }.into_any(),
                                                    }}
                                                </td>
                                            </tr>
                                        }
                                    }).collect_view()}
                                </tbody>
                            </table>
                        </div>
                    </div>
                }
                .into_any()
            }}
        </section>
    }
}

/// Compact per-day glycemic breakdown (defaulting to most recent 14 days with a
/// `"Show all N days"` toggle) plus continuous Sensor Sessions & Gaps summary strip.
#[component]
pub fn DailyGlucoseAndSessionsSection(
    days: Vec<DailyGlucoseSummary>,
    sessions: Vec<SensorSession>,
    gaps: Vec<SensorGap>,
) -> impl IntoView {
    if days.is_empty() && sessions.is_empty() {
        return None;
    }

    let show_all = RwSignal::new(false);
    let total_days = days.len();
    let total_sessions = sessions.len();
    let total_gaps = gaps.len();
    let total_gap_hours: f64 = gaps.iter().map(|g| g.duration_hours).sum();
    let longest_gap_hours = gaps
        .iter()
        .map(|g| g.duration_hours)
        .fold(0.0_f64, f64::max);

    // Reverse chronologically so most recent days appear first.
    let days_rev: Vec<DailyGlucoseSummary> = days.into_iter().rev().collect();
    let stored_days = StoredValue::new(days_rev);

    // Show up to 6 most recent wear sessions in the compact strip.
    let recent_sessions: Vec<SensorSession> = sessions.iter().rev().take(6).cloned().collect();
    let hidden_sessions = total_sessions.saturating_sub(recent_sessions.len());

    Some(view! {
        <section
            data-daily-sessions="true"
            class="mb-8 rounded-2xl border border-white/10 bg-white/[0.02] p-6 space-y-5"
        >
            <div class="flex flex-wrap items-center justify-between gap-3">
                <div>
                    <h2 class="text-lg font-semibold text-white">
                        "Daily Breakdown & Sensor Wear Sessions"
                    </h2>
                    <p class="text-sm text-slate-400">
                        {format!(
                            "{total_days} local calendar day(s) · {total_sessions} continuous wear session(s) · {total_gaps} gap(s) ≥2h."
                        )}
                    </p>
                </div>
                {(total_days > 14).then(|| view! {
                    <button
                        type="button"
                        on:click=move |_| show_all.update(|v| *v = !*v)
                        class="rounded-full border border-cyan-400/30 bg-cyan-500/10 px-3.5 py-1.5 text-xs font-semibold text-cyan-200 transition hover:bg-cyan-500/20"
                    >
                        {move || if show_all.get() {
                            "Show recent 14 days".to_string()
                        } else {
                            format!("Show all {total_days} days")
                        }}
                    </button>
                })}
            </div>

            // Compact Sensor Sessions & Gaps summary strip
            <div class="rounded-xl border border-white/10 bg-slate-950/60 p-4 space-y-3 text-xs">
                <div class="flex flex-wrap items-center justify-between gap-2">
                    <span class="font-semibold text-slate-200">"Sensor Sessions & Gaps (≥2h break)"</span>
                    <div class="flex flex-wrap gap-3 font-mono text-[11px] text-slate-300">
                        <span>{format!("Sessions: {total_sessions}")}</span>
                        <span>{format!("Gaps ≥2h: {total_gaps}")}</span>
                        <span>
                            {if longest_gap_hours > 0.0 {
                                format!(
                                    "Longest gap: {} (total {})",
                                    format_hours_compact(longest_gap_hours),
                                    format_hours_compact(total_gap_hours)
                                )
                            } else {
                                "No gaps ≥2h".to_string()
                            }}
                        </span>
                    </div>
                </div>

                {(!recent_sessions.is_empty()).then(|| view! {
                    <div class="flex flex-wrap gap-2">
                        {recent_sessions.into_iter().map(|s| {
                            let gap_note = s
                                .preceding_gap_hours
                                .map(|g| format!(" · after {} gap", format_hours_compact(g)))
                                .unwrap_or_default();
                            view! {
                                <div class="rounded-lg border border-white/10 bg-slate-900/80 px-2.5 py-1.5 font-mono text-[11px] text-slate-300">
                                    <span class="font-semibold text-cyan-300">{format!("S{}", s.session_index)}</span>
                                    <span>
                                        {format!(
                                            " · {} · {:.0}% wear · mean {:.0} · TIR {:.0}%{gap_note}",
                                            format_hours_compact(s.duration_hours),
                                            s.active_wear_pct,
                                            s.mean_mg_dl,
                                            s.tir_70_180_pct
                                        )}
                                    </span>
                                </div>
                            }
                        }).collect_view()}
                        {(hidden_sessions > 0).then(|| view! {
                            <span class="self-center font-mono text-[11px] text-slate-500">
                                {format!("+{hidden_sessions} earlier session(s)")}
                            </span>
                        })}
                    </div>
                })}
            </div>

            // Per-day breakdown table
            <div class="overflow-x-auto">
                <table class="w-full min-w-[660px] border-collapse text-left text-xs">
                    <thead>
                        <tr class="border-b border-white/10 text-[11px] uppercase tracking-wider text-slate-400">
                            <th class="py-2 pr-3">"Date"</th>
                            <th class="py-2 pr-3">"Rdgs / Wear"</th>
                            <th class="py-2 pr-3">"Mean"</th>
                            <th class="py-2 pr-3">"Min–Max"</th>
                            <th class="py-2 pr-3">"TIR / 5-Band Mix"</th>
                            <th class="py-2">"Ceil Cap"</th>
                        </tr>
                    </thead>
                    <tbody class="divide-y divide-white/5 text-slate-200">
                        {move || {
                            let limit = if show_all.get() { total_days } else { 14 };
                            stored_days.with_value(|list| {
                                list.iter()
                                    .take(limit)
                                    .map(|day| {
                                        let tbr_w = day.tbr_lt_70_pct.clamp(0.0, 100.0);
                                        let tight_w = day.titr_70_140_pct.clamp(0.0, 100.0 - tbr_w);
                                        let upper_tir_w = (day.tir_70_180_pct - day.titr_70_140_pct)
                                            .clamp(0.0, 100.0 - tbr_w - tight_w);
                                        let tar_w = (100.0 - tbr_w - tight_w - upper_tir_w).max(0.0);
                                        let ceil_cls = if day.ceiling_capped_pct > 0.0 {
                                            "font-semibold text-orange-300"
                                        } else {
                                            "text-slate-500"
                                        };
                                        let floor_note = if day.floor_capped_pct > 0.0 {
                                            format!(" · floor {:.1}%", day.floor_capped_pct)
                                        } else {
                                            String::new()
                                        };
                                        let date = day.date.clone();
                                        let rdgs = day.reading_count;
                                        let wear = day.active_wear_pct;
                                        let mean = day.mean_mg_dl;
                                        let min_g = day.min_mg_dl;
                                        let max_g = day.max_mg_dl;
                                        let tir = day.tir_70_180_pct;
                                        let ceil = day.ceiling_capped_pct;
                                        view! {
                                            <tr>
                                                <td class="py-2 pr-3 whitespace-nowrap font-mono text-slate-200">{date}</td>
                                                <td class="py-2 pr-3 whitespace-nowrap font-mono text-slate-300">
                                                    {format!("{rdgs} · {wear:.0}%")}
                                                </td>
                                                <td class="py-2 pr-3 whitespace-nowrap font-mono font-semibold text-cyan-200">
                                                    {format!("{mean:.0} mg/dL")}
                                                </td>
                                                <td class="py-2 pr-3 whitespace-nowrap font-mono text-slate-300">
                                                    {format!("{min_g:.0}–{max_g:.0}")}
                                                </td>
                                                <td class="py-2 pr-3">
                                                    <div class="flex items-center gap-2">
                                                        <div class="flex h-2.5 w-28 shrink-0 overflow-hidden rounded-full border border-white/10 bg-slate-900">
                                                            <div style=format!("width: {tbr_w:.1}%") class="bg-rose-400"></div>
                                                            <div style=format!("width: {tight_w:.1}%") class="bg-emerald-400"></div>
                                                            <div style=format!("width: {upper_tir_w:.1}%") class="bg-teal-500/80"></div>
                                                            <div style=format!("width: {tar_w:.1}%") class="bg-amber-400"></div>
                                                        </div>
                                                        <span class="font-mono text-[11px] text-emerald-300">
                                                            {format!("{tir:.0}%")}
                                                        </span>
                                                    </div>
                                                </td>
                                                <td class=format!("py-2 whitespace-nowrap font-mono {ceil_cls}")>
                                                    {format!("{ceil:.1}%{floor_note}")}
                                                </td>
                                            </tr>
                                        }
                                    })
                                    .collect_view()
                            })
                        }}
                    </tbody>
                </table>
            </div>
        </section>
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mental_health_cgm::{
        parse_commercial_glucose_input, summarize_commercial_glucose, GlucoseMonitor, GlucoseUnit,
        MeasurementMethod,
    };
    use crate::mental_health_cgm_analytics::analyze_glucose_trace;
    use crate::mental_health_lingo::{GlucoseReadingBound, LingoReading, SAMPLE_LINGO_CSV};
    use chrono::NaiveDate;
    use leptos::prelude::RenderHtml;

    fn synthetic_multi_day_trace(num_days: i64) -> Vec<LingoReading> {
        let base = NaiveDate::from_ymd_opt(2026, 7, 1).unwrap();
        let mut out = Vec::new();
        for day in 0..num_days {
            let date = base + chrono::TimeDelta::days(day);
            for slot in 0..48_u32 {
                // Create a 4-hour sensor gap on day 5 to test session/gap rendering
                if day == 5 && (20..28).contains(&slot) {
                    continue;
                }
                let hour = slot / 2;
                let minute = (slot % 2) * 30;
                let hour_offset = day as f64 * 24.0 + hour as f64 + minute as f64 / 60.0;
                let (g, bound) = if day == num_days - 1 && hour == 2 {
                    (54.0, GlucoseReadingBound::BelowSensorFloor)
                } else if day == num_days - 1 && (13..=14).contains(&hour) {
                    (201.0, GlucoseReadingBound::AboveSensorCeiling)
                } else if day < 14 && (12..18).contains(&hour) {
                    (172.0, GlucoseReadingBound::Measured)
                } else if hour < 6 {
                    (88.0, GlucoseReadingBound::Measured)
                } else {
                    (112.0, GlucoseReadingBound::Measured)
                };
                out.push(LingoReading {
                    timestamp: format!("{date}T{hour:02}:{minute:02}-07:00"),
                    hour_offset,
                    glucose_mg_dl: g,
                    lingo_count: None,
                    tag: None,
                    bound,
                });
            }
        }
        out
    }

    #[test]
    fn saturation_banner_hides_when_unsaturated_and_renders_when_capped() {
        let sample = parse_commercial_glucose_input(SAMPLE_LINGO_CSV, None).unwrap();
        let clean_summary = summarize_commercial_glucose(
            &sample.readings,
            sample.monitor,
            sample.source_unit,
            sample.method,
        )
        .unwrap();
        assert!(!clean_summary.has_sensor_saturation());
        let hidden_html = view! {
            <SensorSaturationBanner summary=clean_summary diagnostics=Some(sample.diagnostics) />
        }
        .to_html();
        assert!(!hidden_html.contains("Sensor Saturation &amp; Bound Clipping Notice"));

        let capped_readings = synthetic_multi_day_trace(3);
        let capped_summary = summarize_commercial_glucose(
            &capped_readings,
            GlucoseMonitor::AbbottLingo,
            GlucoseUnit::MgDl,
            MeasurementMethod::InterstitialCgm,
        )
        .unwrap();
        assert!(capped_summary.has_sensor_saturation());
        let shown_html = view! {
            <SensorSaturationBanner summary=capped_summary diagnostics=None />
        }
        .to_html();
        assert!(shown_html.contains("Sensor Saturation &amp; Bound Clipping Notice"));
        assert!(shown_html.contains("Ceiling-capped"));
        assert!(shown_html.contains("Floor-capped"));
        assert!(shown_html.contains("lower bounds"));
    }

    #[test]
    fn longitudinal_cgm_views_render_cleanly_on_ssr() {
        let trace = synthetic_multi_day_trace(28);
        let analytics = analyze_glucose_trace(
            &trace,
            GlucoseMonitor::AbbottLingo,
            MeasurementMethod::InterstitialCgm,
            None,
        );

        let cmp = analytics
            .period_comparison
            .clone()
            .expect("28d synthetic trace yields period comparison");
        let cmp_html = view! { <PeriodComparisonCard comparison=cmp /> }.to_html();
        assert!(cmp_html.contains("14-Day Trend vs. Prior 14 Days"));
        assert!(cmp_html.contains("Ceiling-Cap %"));

        // Verify truncated prior-window warning renders when prior_days < recent_days - 1.0
        let mut trunc_cmp = analytics.period_comparison.unwrap();
        trunc_cmp.recent_days = 14.0;
        trunc_cmp.prior_days = 9.5;
        let trunc_html = view! { <PeriodComparisonCard comparison=trunc_cmp /> }.to_html();
        assert!(trunc_html.contains("Coverage note:"));

        let circ_html = view! {
            <CircadianBreakdownSection periods=analytics.circadian />
        }
        .to_html();
        assert!(circ_html.contains("Overnight"));
        assert!(circ_html.contains("Morning"));
        assert!(circ_html.contains("Afternoon"));
        assert!(circ_html.contains("Evening"));

        let ep_html = view! {
            <GlycemicEpisodesAndRocSection
                episodes=analytics.episodes
                counts=analytics.episode_counts
                roc=analytics.rate_of_change
            />
        }
        .to_html();
        assert!(ep_html.contains("Sustained Glycemic Episodes"));
        assert!(ep_html.contains("Rapid Rises"));
        assert!(ep_html.contains("Rapid Falls"));

        let daily_html = view! {
            <DailyGlucoseAndSessionsSection
                days=analytics.daily
                sessions=analytics.sessions
                gaps=analytics.gaps
            />
        }
        .to_html();
        assert!(daily_html.contains("Daily Breakdown &amp; Sensor Wear Sessions"));
        assert!(daily_html.contains("Show all 28 days"));
        assert!(daily_html.contains("Gaps ≥2h: 1"));
    }
}
