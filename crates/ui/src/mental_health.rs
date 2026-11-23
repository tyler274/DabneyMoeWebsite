//! Interactive Coda Health Mental Health & Abbott Lingo CGM tracking workspace (`/mental-health`).

use leptos::prelude::*;
use leptos_meta::{Link, Meta, Title};

use crate::mental_health_3d::LingoMood3DChart;
use crate::mental_health_episodes::{
    parse_self_activity_log, EpisodeActivationSection, TopCoolDownBanner, SAMPLE_SELF_ACTIVITY_CSV,
};
use crate::mental_health_lingo::{
    build_assessment_series, classify_gad7, classify_phq9, correlate_glucose_and_mood,
    csv_data_uri, export_correlations_csv, parse_lingo_csv, sample_assessment_history,
    sample_checkins, summarize_lingo, AssessmentPoint, LingoReading, MentalHealthCheckIn,
    SAMPLE_LINGO_CSV,
};
use crate::sections::{Footer, NavBar};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChartViewMode {
    WebGl3D,
    Svg2D,
}

const CYAN: &str = "#22d3ee";
const EMERALD: &str = "#34d399";
const AMBER: &str = "#fbbf24";
const ROSE: &str = "#fb7185";
const VIOLET: &str = "#a78bfa";
const SLATE_LIGHT: &str = "#94a3b8";

#[component]
pub fn MentalHealthPage() -> impl IntoView {
    let csv_input = RwSignal::new(SAMPLE_LINGO_CSV.to_string());
    let readings = RwSignal::new(parse_lingo_csv(SAMPLE_LINGO_CSV).unwrap_or_default());
    let csv_error = RwSignal::new(Option::<String>::None);

    let checkins = RwSignal::new(sample_checkins());
    let chart_mode = RwSignal::new(ChartViewMode::WebGl3D);
    let asrm_score = RwSignal::new(Some(7u8));
    let activity_events =
        RwSignal::new(parse_self_activity_log(SAMPLE_SELF_ACTIVITY_CSV).unwrap_or_default());

    // Form state for adding a new mental health check-in.
    let mood_input = RwSignal::new(6u8);
    let anxiety_input = RwSignal::new(5u8);
    let energy_input = RwSignal::new(6u8);
    let sleep_input = RwSignal::new("7.0".to_string());
    let phq9_input = RwSignal::new(String::new());
    let gad7_input = RwSignal::new(String::new());
    let note_input = RwSignal::new(String::new());
    let hour_input = RwSignal::new("18.0".to_string());
    let form_status = RwSignal::new(Option::<String>::None);

    let apply_csv = move |_| match parse_lingo_csv(&csv_input.get()) {
        Ok(parsed) => {
            readings.set(parsed);
            csv_error.set(None);
        }
        Err(err) => {
            csv_error.set(Some(err));
        }
    };

    let reset_sample = move |_| {
        csv_input.set(SAMPLE_LINGO_CSV.to_string());
        readings.set(parse_lingo_csv(SAMPLE_LINGO_CSV).unwrap_or_default());
        checkins.set(sample_checkins());
        csv_error.set(None);
        form_status.set(None);
    };

    let submit_checkin = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let sleep_hours = sleep_input
            .get()
            .trim()
            .parse::<f32>()
            .ok()
            .filter(|v| v.is_finite() && (0.0..=24.0).contains(v))
            .unwrap_or(7.0);
        let hour_offset = hour_input
            .get()
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite() && (0.0..=48.0).contains(v))
            .unwrap_or(12.0);
        let phq9 = phq9_input
            .get()
            .trim()
            .parse::<u8>()
            .ok()
            .filter(|v| *v <= 27);
        let gad7 = gad7_input
            .get()
            .trim()
            .parse::<u8>()
            .ok()
            .filter(|v| *v <= 21);
        let note_text = note_input.get().trim().to_string();
        let note = if note_text.is_empty() {
            "Logged check-in".to_string()
        } else {
            note_text
        };

        let hour_whole = hour_offset.floor() as u32;
        let minute = ((hour_offset - hour_offset.floor()) * 60.0).round() as u32;
        let recorded_at = format!("+{hour_whole:02}h {minute:02}m");

        let entry = MentalHealthCheckIn {
            recorded_at,
            hour_offset,
            mood: mood_input.get().clamp(1, 10),
            anxiety: anxiety_input.get().clamp(1, 10),
            energy: energy_input.get().clamp(1, 10),
            sleep_hours,
            phq9,
            gad7,
            note,
        };
        checkins.update(|list| {
            list.push(entry);
            list.sort_by(|a, b| a.hour_offset.total_cmp(&b.hour_offset));
        });
        note_input.set(String::new());
        form_status.set(Some(
            "Check-in added and correlated against the Lingo CGM timeline.".to_string(),
        ));
    };

    view! {
        <Title text="Coda Health — Mental Health & Abbott Lingo CGM Tracking | dabney.moe" />
        <Meta
            name="description"
            content="Local-first mental health tracking paired with Abbott Lingo continuous glucose monitoring (CGM), PHQ-9/GAD-7 scoring, and Coda Health clinical workspace integration."
        />
        <Link rel="canonical" href="https://dabney.moe/mental-health" />

        <NavBar />

        <div class="mx-auto max-w-5xl px-6 py-12">
            // Hero
            <div class="mb-10 flex flex-col gap-6 border-b border-white/10 pb-8 md:flex-row md:items-end md:justify-between">
                <div class="max-w-2xl">
                    <p class="mb-2 text-xs font-semibold uppercase tracking-widest text-cyan-400">
                        "Coda Health · Metabolic Psychiatry & CGM"
                    </p>
                    <h1 class="text-3xl font-bold tracking-tight text-white sm:text-4xl">
                        "Mental Health & Abbott Lingo CGM"
                    </h1>
                    <p class="mt-3 text-base leading-relaxed text-slate-300">
                        "Track daily mood, anxiety, energy, sleep, PHQ-9, and GAD-7 alongside continuous interstitial glucose from Abbott Lingo. Everything parses in pure Rust in your browser—nothing leaves your device unless you open the Coda clinical workspace."
                    </p>
                </div>
                <div class="flex flex-wrap gap-3">
                    <a
                        href="/coda/physician/"
                        class="inline-flex items-center gap-2 rounded-full bg-cyan-500 px-5 py-2.5 text-sm font-semibold text-slate-950 transition hover:bg-cyan-400"
                    >
                        "Open Coda Live Harness →"
                    </a>
                    <button
                        type="button"
                        on:click=reset_sample
                        class="rounded-full border border-white/15 px-4 py-2.5 text-sm font-medium text-slate-200 transition hover:bg-white/5"
                    >
                        "Reset sample"
                    </button>
                </div>
            </div>

            // Top-of-page calm Cool-Down banner when activation signals stack up
            <TopCoolDownBanner
                events=activity_events.into()
                asrm_score=asrm_score.into()
                sleep_hours=Signal::derive(move || {
                    sleep_input
                        .get()
                        .trim()
                        .parse::<f32>()
                        .ok()
                        .filter(|v| v.is_finite() && (0.0..=24.0).contains(v))
                        .unwrap_or(7.0)
                })
            />

            // KPI summary strip
            {move || {
                let current_readings = readings.get();
                let current_checkins = checkins.get();
                let summary = summarize_lingo(&current_readings);
                let latest_phq9 = current_checkins.iter().rev().find_map(|c| c.phq9);
                let latest_gad7 = current_checkins.iter().rev().find_map(|c| c.gad7);

                view! {
                    <div class="mb-8 grid grid-cols-2 gap-4 sm:grid-cols-3 lg:grid-cols-5">
                        <MetricCard
                            label="Mean glucose"
                            value=summary.as_ref().map(|s| format!("{:.0} mg/dL", s.mean_mg_dl)).unwrap_or_else(|| "—".to_string())
                            detail=summary.as_ref().map(|s| format!("Range {:.0}–{:.0}", s.min_mg_dl, s.max_mg_dl)).unwrap_or_default()
                            accent="text-cyan-300"
                        />
                        <MetricCard
                            label="Lingo TIR (70–140)"
                            value=summary.as_ref().map(|s| format!("{:.0}%", s.tir_metabolic_pct)).unwrap_or_else(|| "—".to_string())
                            detail=summary.as_ref().map(|s| format!("Clinical 70–180: {:.0}%", s.tir_clinical_pct)).unwrap_or_default()
                            accent="text-emerald-300"
                        />
                        <MetricCard
                            label="Variability (CV%)"
                            value=summary.as_ref().map(|s| format!("{:.1}%", s.cv_percent)).unwrap_or_else(|| "—".to_string())
                            detail=summary.as_ref().map(|s| format!("SD {:.1} mg/dL", s.std_dev_mg_dl)).unwrap_or_default()
                            accent="text-amber-300"
                        />
                        <MetricCard
                            label="Lingo Count"
                            value=summary.as_ref().map(|s| format!("{:.0}", s.total_lingo_count)).unwrap_or_else(|| "—".to_string())
                            detail=summary.as_ref().map(|s| format!("{} spike readings (>140)", s.spike_readings)).unwrap_or_default()
                            accent="text-rose-300"
                        />
                        <MetricCard
                            label="PHQ-9 / GAD-7"
                            value=format!(
                                "{} / {}",
                                latest_phq9.map(|v| v.to_string()).unwrap_or_else(|| "—".to_string()),
                                latest_gad7.map(|v| v.to_string()).unwrap_or_else(|| "—".to_string()),
                            )
                            detail=format!(
                                "{} · {}",
                                latest_phq9.map(|v| classify_phq9(v).label()).unwrap_or("Unscored"),
                                latest_gad7.map(|v| classify_gad7(v).label()).unwrap_or("Unscored"),
                            )
                            accent="text-violet-300"
                        />
                    </div>
                }
            }}

            // Interactive 3D WebGL2 / 2D SVG dual-axis chart
            <section class="mb-8 rounded-2xl border border-white/10 bg-white/[0.02] p-6">
                <div class="mb-4 flex flex-wrap items-center justify-between gap-3">
                    <div>
                        <h2 class="text-lg font-semibold text-white">
                            "Abbott Lingo Glucose & Mental State Timeline"
                        </h2>
                        <p class="text-sm text-slate-400">
                            "Explore Lingo's 70–140 mg/dL metabolic zone with mood (emerald) and anxiety (rose) in pure-Rust WebGL2 3D or classic 2D SVG."
                        </p>
                    </div>
                    <div class="flex flex-wrap items-center gap-4 text-xs text-slate-300">
                        <div class="inline-flex rounded-full border border-white/15 bg-slate-900/80 p-0.5">
                            <button
                                type="button"
                                on:click=move |_| chart_mode.set(ChartViewMode::WebGl3D)
                                class=move || {
                                    if chart_mode.get() == ChartViewMode::WebGl3D {
                                        "rounded-full bg-cyan-500 px-3 py-1 font-semibold text-slate-950 transition"
                                    } else {
                                        "rounded-full px-3 py-1 font-medium text-slate-300 transition hover:text-white"
                                    }
                                }
                            >
                                "3D WebGL2"
                            </button>
                            <button
                                type="button"
                                on:click=move |_| chart_mode.set(ChartViewMode::Svg2D)
                                class=move || {
                                    if chart_mode.get() == ChartViewMode::Svg2D {
                                        "rounded-full bg-cyan-500 px-3 py-1 font-semibold text-slate-950 transition"
                                    } else {
                                        "rounded-full px-3 py-1 font-medium text-slate-300 transition hover:text-white"
                                    }
                                }
                            >
                                "2D SVG"
                            </button>
                        </div>
                        <span class="flex items-center gap-1.5">
                            <span class="inline-block h-2.5 w-2.5 rounded-full bg-cyan-400"></span>
                            "Lingo glucose (mg/dL)"
                        </span>
                        <span class="flex items-center gap-1.5">
                            <span class="inline-block h-2.5 w-2.5 rounded-full bg-emerald-400"></span>
                            "Mood (1–10)"
                        </span>
                        <span class="flex items-center gap-1.5">
                            <span class="inline-block h-2.5 w-2.5 rounded-full bg-rose-400"></span>
                            "Anxiety (1–10)"
                        </span>
                    </div>
                </div>
                {move || match chart_mode.get() {
                    ChartViewMode::WebGl3D => view! {
                        <LingoMood3DChart readings=readings.into() checkins=checkins.into() />
                    }.into_any(),
                    ChartViewMode::Svg2D => view! {
                        <LingoMoodTimelineChart readings=readings.get() checkins=checkins.get() />
                    }.into_any(),
                }}
            </section>

            // Longitudinal PHQ-9 & GAD-7 Trend Chart
            <section class="mb-8 rounded-2xl border border-white/10 bg-white/[0.02] p-6">
                <div class="mb-4 flex flex-wrap items-center justify-between gap-3">
                    <div>
                        <h2 class="text-lg font-semibold text-white">
                            "PHQ-9 & GAD-7 Longitudinal Trajectory"
                        </h2>
                        <p class="text-sm text-slate-400">
                            "Multi-week depression (PHQ-9, 0–27) and anxiety (GAD-7, 0–21) scores with Mild (5) and Moderate (10) clinical thresholds."
                        </p>
                    </div>
                    <div class="flex flex-wrap gap-4 text-xs text-slate-300">
                        <span class="flex items-center gap-1.5">
                            <span class="inline-block h-2.5 w-2.5 rounded-full bg-violet-400"></span>
                            "PHQ-9 (0–27)"
                        </span>
                        <span class="flex items-center gap-1.5">
                            <span class="inline-block h-2.5 w-2.5 rounded-full bg-rose-400"></span>
                            "GAD-7 (0–21)"
                        </span>
                    </div>
                </div>
                {move || {
                    let series = build_assessment_series(&sample_assessment_history(), &checkins.get());
                    view! { <Phq9Gad7TrendChart series=series /> }
                }}
            </section>

            // Two-column workspace: Check-in Logger + Abbott Lingo CSV Import
            <div class="mb-8 grid gap-6 lg:grid-cols-2">
                <section class="rounded-2xl border border-white/10 bg-white/[0.02] p-6">
                    <h2 class="mb-1 text-lg font-semibold text-white">"Log Mental Health Check-In"</h2>
                    <p class="mb-4 text-sm text-slate-400">
                        "Correlates immediately with your nearest Lingo glucose reading and prior 2-hour excursion peak."
                    </p>
                    <form on:submit=submit_checkin class="space-y-4">
                        <div class="grid grid-cols-3 gap-3">
                            <label class="block text-xs text-slate-300">
                                {move || format!("Mood: {}/10", mood_input.get())}
                                <input
                                    type="range"
                                    min="1"
                                    max="10"
                                    prop:value=move || mood_input.get().to_string()
                                    on:input=move |ev| {
                                        if let Ok(v) = event_target_value(&ev).parse::<u8>() {
                                            mood_input.set(v);
                                        }
                                    }
                                    class="mt-1 w-full accent-emerald-400"
                                />
                            </label>
                            <label class="block text-xs text-slate-300">
                                {move || format!("Anxiety: {}/10", anxiety_input.get())}
                                <input
                                    type="range"
                                    min="1"
                                    max="10"
                                    prop:value=move || anxiety_input.get().to_string()
                                    on:input=move |ev| {
                                        if let Ok(v) = event_target_value(&ev).parse::<u8>() {
                                            anxiety_input.set(v);
                                        }
                                    }
                                    class="mt-1 w-full accent-rose-400"
                                />
                            </label>
                            <label class="block text-xs text-slate-300">
                                {move || format!("Energy: {}/10", energy_input.get())}
                                <input
                                    type="range"
                                    min="1"
                                    max="10"
                                    prop:value=move || energy_input.get().to_string()
                                    on:input=move |ev| {
                                        if let Ok(v) = event_target_value(&ev).parse::<u8>() {
                                            energy_input.set(v);
                                        }
                                    }
                                    class="mt-1 w-full accent-cyan-400"
                                />
                            </label>
                        </div>
                        <div class="grid grid-cols-2 gap-3 sm:grid-cols-4">
                            <label class="block text-xs text-slate-300">
                                "Hour offset"
                                <input
                                    type="number"
                                    step="0.5"
                                    min="0"
                                    max="48"
                                    prop:value=move || hour_input.get()
                                    on:input=move |ev| hour_input.set(event_target_value(&ev))
                                    class="mt-1 w-full rounded-lg border border-white/10 bg-slate-900 px-2.5 py-1.5 text-sm text-white"
                                />
                            </label>
                            <label class="block text-xs text-slate-300">
                                "Sleep (hrs)"
                                <input
                                    type="number"
                                    step="0.5"
                                    min="0"
                                    max="24"
                                    prop:value=move || sleep_input.get()
                                    on:input=move |ev| sleep_input.set(event_target_value(&ev))
                                    class="mt-1 w-full rounded-lg border border-white/10 bg-slate-900 px-2.5 py-1.5 text-sm text-white"
                                />
                            </label>
                            <label class="block text-xs text-slate-300">
                                "PHQ-9 (0–27)"
                                <input
                                    type="number"
                                    min="0"
                                    max="27"
                                    placeholder="opt"
                                    prop:value=move || phq9_input.get()
                                    on:input=move |ev| phq9_input.set(event_target_value(&ev))
                                    class="mt-1 w-full rounded-lg border border-white/10 bg-slate-900 px-2.5 py-1.5 text-sm text-white"
                                />
                            </label>
                            <label class="block text-xs text-slate-300">
                                "GAD-7 (0–21)"
                                <input
                                    type="number"
                                    min="0"
                                    max="21"
                                    placeholder="opt"
                                    prop:value=move || gad7_input.get()
                                    on:input=move |ev| gad7_input.set(event_target_value(&ev))
                                    class="mt-1 w-full rounded-lg border border-white/10 bg-slate-900 px-2.5 py-1.5 text-sm text-white"
                                />
                            </label>
                        </div>
                        <label class="block text-xs text-slate-300">
                            "Context note (meal, stressor, walk, medication)"
                            <input
                                type="text"
                                placeholder="e.g. Afternoon focus dip 60m after sweet snack"
                                prop:value=move || note_input.get()
                                on:input=move |ev| note_input.set(event_target_value(&ev))
                                class="mt-1 w-full rounded-lg border border-white/10 bg-slate-900 px-3 py-2 text-sm text-white placeholder:text-slate-500"
                            />
                        </label>
                        <div class="flex items-center justify-between gap-3">
                            <button
                                type="submit"
                                class="rounded-full bg-emerald-500 px-4 py-2 text-sm font-semibold text-slate-950 transition hover:bg-emerald-400"
                            >
                                "Add check-in"
                            </button>
                            {move || form_status.get().map(|msg| view! {
                                <span class="text-xs text-emerald-300">{msg}</span>
                            })}
                        </div>
                    </form>
                </section>

                <section class="rounded-2xl border border-white/10 bg-white/[0.02] p-6">
                    <h2 class="mb-1 text-lg font-semibold text-white">"Abbott Lingo CGM CSV Import"</h2>
                    <p class="mb-4 text-sm text-slate-400">
                        "Paste `timestamp,glucose_mg_dl[,lingo_count[,tag]]` rows from your Lingo export to recompute TIR, CV%, and spike correlations locally."
                    </p>
                    <textarea
                        rows="8"
                        prop:value=move || csv_input.get()
                        on:input=move |ev| csv_input.set(event_target_value(&ev))
                        class="w-full rounded-xl border border-white/10 bg-slate-950 p-3 font-mono text-xs text-slate-200"
                    />
                    <div class="mt-3 flex flex-wrap items-center justify-between gap-3">
                        <button
                            type="button"
                            on:click=apply_csv
                            class="rounded-full bg-cyan-500 px-4 py-2 text-sm font-semibold text-slate-950 transition hover:bg-cyan-400"
                        >
                            "Parse Lingo CSV"
                        </button>
                        {move || csv_error.get().map(|err| view! {
                            <span class="text-xs text-rose-400">{err}</span>
                        })}
                    </div>
                </section>
            </div>

            // Correlated check-ins table
            <section class="rounded-2xl border border-white/10 bg-white/[0.02] p-6">
                <div class="mb-4 flex flex-wrap items-center justify-between gap-3">
                    <div>
                        <h2 class="mb-1 text-lg font-semibold text-white">"Glucose–Mood Correlation Log"</h2>
                        <p class="text-sm text-slate-400">
                            "Each mental health check-in paired with nearest Abbott Lingo glucose and peak reading in the preceding 2 hours."
                        </p>
                    </div>
                    {move || {
                        let corr = correlate_glucose_and_mood(&readings.get(), &checkins.get());
                        let href = csv_data_uri(&export_correlations_csv(&corr));
                        view! {
                            <a
                                href=href
                                download="coda-mental-health-lingo.csv"
                                class="inline-flex items-center gap-1.5 rounded-full border border-white/15 px-4 py-2 text-xs font-medium text-slate-200 transition hover:bg-white/5"
                            >
                                "Download CSV"
                            </a>
                        }
                    }}
                </div>
                <div class="overflow-x-auto">
                    <table class="w-full min-w-[600px] border-collapse text-left text-sm">
                        <thead>
                            <tr class="border-b border-white/10 text-xs uppercase tracking-wider text-slate-400">
                                <th class="py-2 pr-4">"Time"</th>
                                <th class="py-2 pr-4">"Glucose / 2h Peak"</th>
                                <th class="py-2 pr-4">"Mood · Anx · Eng"</th>
                                <th class="py-2 pr-4">"Pattern"</th>
                                <th class="py-2">"Context"</th>
                            </tr>
                        </thead>
                        <tbody class="divide-y divide-white/5 text-slate-200">
                            {move || {
                                let corr = correlate_glucose_and_mood(&readings.get(), &checkins.get());
                                corr.into_iter().map(|row| {
                                    let badge = if row.spike_adjacent {
                                        view! {
                                            <span class="inline-flex rounded-full bg-amber-500/15 px-2.5 py-0.5 text-xs font-medium text-amber-300">
                                                "Post-spike (>140)"
                                            </span>
                                        }.into_any()
                                    } else {
                                        view! {
                                            <span class="inline-flex rounded-full bg-emerald-500/15 px-2.5 py-0.5 text-xs font-medium text-emerald-300">
                                                "In Lingo zone"
                                            </span>
                                        }.into_any()
                                    };
                                    view! {
                                        <tr>
                                            <td class="py-3 pr-4 whitespace-nowrap font-mono text-xs text-slate-300">{row.recorded_at}</td>
                                            <td class="py-3 pr-4 whitespace-nowrap font-mono text-xs text-cyan-300">
                                                {format!("{:.0} / {:.0} mg/dL", row.nearest_glucose_mg_dl, row.peak_prior_2h_mg_dl)}
                                            </td>
                                            <td class="py-3 pr-4 whitespace-nowrap text-xs">
                                                {format!("{}/10 · {}/10 · {}/10", row.mood, row.anxiety, row.energy)}
                                            </td>
                                            <td class="py-3 pr-4 whitespace-nowrap">{badge}</td>
                                            <td class="py-3 text-xs text-slate-300">{row.note}</td>
                                        </tr>
                                    }
                                }).collect_view()
                            }}
                        </tbody>
                    </table>
                </div>
            </section>

            // Local-first Episode Activation (ASRM), Self-Activity Actogram & Cool-Down Plan
            <EpisodeActivationSection
                events=activity_events
                asrm_score=asrm_score
                sleep_hours=Signal::derive(move || {
                    sleep_input
                        .get()
                        .trim()
                        .parse::<f32>()
                        .ok()
                        .filter(|v| v.is_finite() && (0.0..=24.0).contains(v))
                        .unwrap_or(7.0)
                })
            />
        </div>

        <Footer />
    }
}

#[component]
fn MetricCard(
    label: &'static str,
    value: String,
    detail: String,
    accent: &'static str,
) -> impl IntoView {
    view! {
        <div class="rounded-2xl border border-white/10 bg-white/[0.02] p-4">
            <p class="text-xs font-medium text-slate-400">{label}</p>
            <p class=format!("mt-1 text-2xl font-bold tracking-tight {accent}")>{value}</p>
            <p class="mt-1 text-xs text-slate-400">{detail}</p>
        </div>
    }
}

#[component]
fn LingoMoodTimelineChart(
    readings: Vec<LingoReading>,
    checkins: Vec<MentalHealthCheckIn>,
) -> impl IntoView {
    let width = 760.0_f64;
    let height = 240.0_f64;
    let pad_l = 48.0_f64;
    let pad_r = 24.0_f64;
    let pad_t = 18.0_f64;
    let pad_b = 30.0_f64;
    let plot_w = width - pad_l - pad_r;
    let plot_h = height - pad_t - pad_b;

    let max_hour = readings
        .iter()
        .map(|r| r.hour_offset)
        .chain(checkins.iter().map(|c| c.hour_offset))
        .fold(24.0_f64, f64::max)
        .max(1.0);
    let min_g = 55.0_f64;
    let max_g = readings
        .iter()
        .map(|r| r.glucose_mg_dl)
        .fold(180.0_f64, f64::max)
        .max(180.0);

    let x_for = |h: f64| pad_l + (h / max_hour) * plot_w;
    let y_for_g = |g: f64| {
        let clamped = g.clamp(min_g, max_g);
        pad_t + plot_h - ((clamped - min_g) / (max_g - min_g)) * plot_h
    };
    let y_for_score = |score: u8| {
        let norm = (f64::from(score.clamp(1, 10)) - 1.0) / 9.0;
        pad_t + plot_h - norm * plot_h
    };

    let band_top = y_for_g(140.0);
    let band_bottom = y_for_g(70.0);
    let band_h = (band_bottom - band_top).max(1.0);

    let points: String = readings
        .iter()
        .map(|r| {
            format!(
                "{:.1},{:.1}",
                x_for(r.hour_offset),
                y_for_g(r.glucose_mg_dl)
            )
        })
        .collect::<Vec<_>>()
        .join(" ");

    view! {
        <svg
            viewBox="0 0 760 240"
            class="w-full"
            role="img"
            aria-label="Abbott Lingo continuous glucose monitoring and mental health check-in timeline"
        >
            // Target 70-140 mg/dL shaded metabolic zone
            <rect
                x=pad_l
                y=band_top
                width=plot_w
                height=band_h
                fill="rgba(52, 211, 153, 0.10)"
            />
            <line x1=pad_l y1=band_top x2=width - pad_r y2=band_top stroke="rgba(52, 211, 153, 0.35)" stroke-dasharray="4 4" />
            <line x1=pad_l y1=band_bottom x2=width - pad_r y2=band_bottom stroke="rgba(52, 211, 153, 0.35)" stroke-dasharray="4 4" />

            // Y-axis labels
            <text x="8" y=band_top + 4.0 fill=SLATE_LIGHT font-size="10">"140"</text>
            <text x="8" y=band_bottom + 4.0 fill=SLATE_LIGHT font-size="10">"70"</text>
            <text x="8" y=pad_t + 6.0 fill=SLATE_LIGHT font-size="10">{format!("{:.0}", max_g)}</text>

            // Glucose curve
            <polyline
                fill="none"
                stroke=CYAN
                stroke-width="2.5"
                stroke-linejoin="round"
                stroke-linecap="round"
                points=points
            />

            // Spike markers (> 140 mg/dL)
            {readings.into_iter().filter(|r| r.glucose_mg_dl > 140.0).map(|r| {
                let cx = x_for(r.hour_offset);
                let cy = y_for_g(r.glucose_mg_dl);
                view! {
                    <circle cx=cx cy=cy r="4.5" fill=AMBER stroke="#0f172a" stroke-width="1.5" />
                }
            }).collect_view()}

            // Mood & Anxiety markers
            {checkins.into_iter().map(|c| {
                let cx = x_for(c.hour_offset);
                let my = y_for_score(c.mood);
                let ay = y_for_score(c.anxiety);
                view! {
                    <g>
                        <line x1=cx y1=pad_t x2=cx y2=height - pad_b stroke="rgba(148,163,184,0.18)" stroke-dasharray="2 3" />
                        <circle cx=cx cy=my r="4.5" fill=EMERALD stroke="#0f172a" stroke-width="1.5" />
                        <circle cx=cx cy=ay r="4.5" fill=ROSE stroke="#0f172a" stroke-width="1.5" />
                    </g>
                }
            }).collect_view()}

            // X-axis labels
            <text x=pad_l y=height - 8.0 fill=SLATE_LIGHT font-size="10">"0h"</text>
            <text x=pad_l + plot_w / 2.0 y=height - 8.0 text-anchor="middle" fill=SLATE_LIGHT font-size="10">
                {format!("{:.0}h", max_hour / 2.0)}
            </text>
            <text x=width - pad_r y=height - 8.0 text-anchor="end" fill=SLATE_LIGHT font-size="10">
                {format!("{:.0}h", max_hour)}
            </text>
        </svg>
    }
}

#[component]
fn Phq9Gad7TrendChart(series: Vec<AssessmentPoint>) -> impl IntoView {
    let width = 760.0_f64;
    let height = 190.0_f64;
    let pad_l = 44.0_f64;
    let pad_r = 28.0_f64;
    let pad_t = 16.0_f64;
    let pad_b = 28.0_f64;
    let plot_w = width - pad_l - pad_r;
    let plot_h = height - pad_t - pad_b;
    let max_score = 27.0_f64;

    let count = series.len().max(2);
    let x_for = |idx: usize| pad_l + (idx as f64 / (count - 1) as f64) * plot_w;
    let y_for = |score: u8| pad_t + plot_h - (f64::from(score).min(max_score) / max_score) * plot_h;

    let phq9_points: String = series
        .iter()
        .enumerate()
        .filter_map(|(i, p)| p.phq9.map(|v| format!("{:.1},{:.1}", x_for(i), y_for(v))))
        .collect::<Vec<_>>()
        .join(" ");
    let gad7_points: String = series
        .iter()
        .enumerate()
        .filter_map(|(i, p)| p.gad7.map(|v| format!("{:.1},{:.1}", x_for(i), y_for(v))))
        .collect::<Vec<_>>()
        .join(" ");

    let y_mild = y_for(5);
    let y_mod = y_for(10);

    view! {
        <svg
            viewBox="0 0 760 190"
            class="w-full"
            role="img"
            aria-label="PHQ-9 and GAD-7 longitudinal score trajectory"
        >
            <line x1=pad_l y1=y_mod x2=width - pad_r y2=y_mod stroke="rgba(251, 191, 36, 0.35)" stroke-dasharray="4 4" />
            <line x1=pad_l y1=y_mild x2=width - pad_r y2=y_mild stroke="rgba(52, 211, 153, 0.30)" stroke-dasharray="4 4" />

            <text x="6" y=y_mod + 4.0 fill=AMBER font-size="10">"10"</text>
            <text x="6" y=y_mild + 4.0 fill=EMERALD font-size="10">"5"</text>
            <text x="6" y=pad_t + 6.0 fill=SLATE_LIGHT font-size="10">"27"</text>

            <polyline fill="none" stroke=VIOLET stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" points=phq9_points />
            <polyline fill="none" stroke=ROSE stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" points=gad7_points />

            {series.into_iter().enumerate().map(|(i, p)| {
                let cx = x_for(i);
                let phq_circle = p.phq9.map(|v| view! {
                    <circle cx=cx cy=y_for(v) r="4" fill=VIOLET stroke="#0f172a" stroke-width="1.5" />
                });
                let gad_circle = p.gad7.map(|v| view! {
                    <circle cx=cx cy=y_for(v) r="4" fill=ROSE stroke="#0f172a" stroke-width="1.5" />
                });
                view! {
                    <g>
                        {phq_circle}
                        {gad_circle}
                        <text x=cx y=height - 8.0 text-anchor="middle" fill=SLATE_LIGHT font-size="10">{p.date}</text>
                    </g>
                }
            }).collect_view()}
        </svg>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use leptos::prelude::RenderHtml;

    #[test]
    fn mental_health_chart_and_metrics_render_cleanly() {
        let readings = parse_lingo_csv(SAMPLE_LINGO_CSV).unwrap();
        let checkins = sample_checkins();
        let html = view! {
            <LingoMoodTimelineChart readings=readings checkins=checkins />
        }
        .to_html();
        assert!(html.contains("Abbott Lingo continuous glucose monitoring"));
        assert!(html.contains("<polyline"));
        assert!(html.contains("140"));
        assert!(html.contains("70"));

        let trend_html = view! {
            <Phq9Gad7TrendChart series=sample_assessment_history() />
        }
        .to_html();
        assert!(trend_html.contains("PHQ-9 and GAD-7 longitudinal score trajectory"));
        assert!(trend_html.contains("2026-10-31"));
    }
}
