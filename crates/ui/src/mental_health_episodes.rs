//! Pure-Rust, local-first Mental Health Episode Activation & Self-Activity Cadence Monitor.
//!
//! Designed strictly for personal self-monitoring on-device: parses only timestamps and
//! coarse event kinds (`message`, `call`, `publish`) from a user's own activity log or
//! Discord Data Package CSV export. Message text and usernames are discarded immediately at
//! parse time and are never stored, displayed, or exported.

use leptos::prelude::*;

use crate::mental_health_lingo::csv_data_uri;

/// Coarse kind of self-initiated communication or publishing event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActivityEventKind {
    Message,
    CallAttempt,
    PublishOrPost,
}

impl ActivityEventKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Message => "message",
            Self::CallAttempt => "call",
            Self::PublishOrPost => "publish",
        }
    }
}

/// A single privacy-stripped self-activity timestamp (no message body or usernames).
#[derive(Clone, Debug, PartialEq)]
pub struct SelfActivityEvent {
    /// Hour of day (`0..24`) in local time.
    pub hour: u8,
    /// Minute of day (`0..1440`) in local time.
    pub minute_of_day: u32,
    /// Absolute pseudo-minutes for chronological burst & quiet-gap analysis.
    pub absolute_minutes: i64,
    /// Coarse event category.
    pub kind: ActivityEventKind,
}

/// Standard Altman Self-Rating Mania Scale (ASRM, `0..=20`) band.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AsrmBand {
    Minimal,
    Elevated,
}

impl AsrmBand {
    pub fn label(self) -> &'static str {
        match self {
            Self::Minimal => "Baseline (0–5)",
            Self::Elevated => "Elevated activation (≥6)",
        }
    }
}

/// Classify an Altman Self-Rating Mania Scale score (`0..=20`, cutoff `>= 6`).
pub fn classify_asrm(score: u8) -> AsrmBand {
    if score >= 6 {
        AsrmBand::Elevated
    } else {
        AsrmBand::Minimal
    }
}

/// Composite self-monitoring activation state combining sleep, ASRM, and late-night cadence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActivationLevel {
    Grounded,
    Watch,
    Elevated,
}

impl ActivationLevel {
    pub fn label(self) -> &'static str {
        match self {
            Self::Grounded => "Grounded",
            Self::Watch => "Early Watch",
            Self::Elevated => "Elevated Activation — Pause Plan Active",
        }
    }

    pub fn badge_class(self) -> &'static str {
        match self {
            Self::Grounded => "bg-emerald-500/15 text-emerald-300 border-emerald-400/30",
            Self::Watch => "bg-amber-500/15 text-amber-300 border-amber-400/30",
            Self::Elevated => "bg-rose-500/15 text-rose-300 border-rose-400/30",
        }
    }
}

/// Aggregate cadence & activation metrics computed purely from stripped timestamps.
#[derive(Clone, Debug, PartialEq)]
pub struct EpisodeCadenceSummary {
    pub total_events: usize,
    pub late_night_events: usize,
    pub late_night_calls: usize,
    pub rapid_burst_clusters: usize,
    pub longest_quiet_gap_hours: f64,
    pub hourly_counts: [usize; 24],
    pub activation: ActivationLevel,
}

/// A user-authored, local-only rule in the pre-committed cool-down plan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoolDownRule {
    pub id: usize,
    pub text: String,
    pub checked: bool,
}

/// Default pre-committed cool-down checklist written while calm.
pub fn default_cooldown_rules() -> Vec<CoolDownRule> {
    vec![
        CoolDownRule {
            id: 1,
            text: "Wait until after 7+ hours of sleep before publishing about other people or sending high-stakes pitches.".to_string(),
            checked: false,
        },
        CoolDownRule {
            id: 2,
            text: "If someone asks to pause, stop calling, or be removed from a site, honor it immediately without negotiating.".to_string(),
            checked: false,
        },
        CoolDownRule {
            id: 3,
            text: "Switch Discord and phone notifications to Do Not Disturb between 00:00 and 07:00 when sleep < 6h or ASRM ≥ 6.".to_string(),
            checked: false,
        },
        CoolDownRule {
            id: 4,
            text: "Share this aggregate sleep/ASRM/cadence summary with a trusted support person or care clinician when signals stack up.".to_string(),
            checked: false,
        },
    ]
}

/// 100% synthetic self-activity sample demonstrating overnight activation & burst detection.
pub const SAMPLE_SELF_ACTIVITY_CSV: &str = "\
timestamp,event_type
2026-11-21 09:15,message
2026-11-21 10:40,message
2026-11-21 14:20,message
2026-11-21 19:05,message
2026-11-22 01:42,message
2026-11-22 01:45,message
2026-11-22 01:47,message
2026-11-22 01:50,publish
2026-11-22 02:18,call
2026-11-22 02:21,call
2026-11-22 02:24,message
2026-11-22 02:26,publish";

/// Parse a user's local self-activity CSV or Discord Data Package `messages.csv` rows.
///
/// Accepts `timestamp,event_type` OR Discord's `ID,Timestamp,Contents,Attachments` format.
/// Discards all message text, IDs, and usernames immediately—retaining ONLY timestamp and
/// coarse [`ActivityEventKind`].
pub fn parse_self_activity_log(input: &str) -> Result<Vec<SelfActivityEvent>, String> {
    let mut events = Vec::new();

    for (idx, raw_line) in input.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let lower = line.to_ascii_lowercase();
        if events.is_empty()
            && (lower.starts_with("timestamp") || lower.starts_with("id,timestamp"))
        {
            continue;
        }

        let Some((ts_str, remainder)) = extract_timestamp_and_rest(line) else {
            return Err(format!(
                "Line {} does not contain a `YYYY-MM-DD HH:MM` or ISO-8601 timestamp.",
                idx + 1
            ));
        };

        let Some((abs_min, hour, minute_of_day)) = parse_timestamp_components(&ts_str) else {
            return Err(format!(
                "Line {} has an invalid timestamp `{ts_str}`.",
                idx + 1
            ));
        };

        let rest_lower = remainder.to_ascii_lowercase();
        let kind = if rest_lower.contains("call") {
            ActivityEventKind::CallAttempt
        } else if rest_lower.contains("publish")
            || rest_lower.contains("deploy")
            || rest_lower.contains("post")
        {
            ActivityEventKind::PublishOrPost
        } else {
            ActivityEventKind::Message
        };

        events.push(SelfActivityEvent {
            hour,
            minute_of_day,
            absolute_minutes: abs_min,
            kind,
        });
    }

    if events.is_empty() {
        return Err("No valid timestamped self-activity rows found.".to_string());
    }

    events.sort_by_key(|e| e.absolute_minutes);
    Ok(events)
}

/// Locate a `YYYY-MM-DD[T ]HH:MM` timestamp anywhere in a CSV row (handles both
/// `timestamp,event_type` and Discord Data Package `ID,Timestamp,Contents,...`).
fn extract_timestamp_and_rest(line: &str) -> Option<(String, &str)> {
    let bytes = line.as_bytes();
    if bytes.len() < 16 {
        return None;
    }
    for i in 0..=bytes.len() - 16 {
        let slice = &bytes[i..i + 16];
        let is_ts = slice[0..4].iter().all(u8::is_ascii_digit)
            && slice[4] == b'-'
            && slice[5..7].iter().all(u8::is_ascii_digit)
            && slice[7] == b'-'
            && slice[8..10].iter().all(u8::is_ascii_digit)
            && (slice[10] == b' ' || slice[10] == b'T')
            && slice[11..13].iter().all(u8::is_ascii_digit)
            && slice[13] == b':'
            && slice[14..16].iter().all(u8::is_ascii_digit);
        if is_ts {
            let ts = String::from_utf8_lossy(slice).into_owned();
            let rest = &line[i + 16..];
            return Some((ts, rest));
        }
    }
    None
}

fn parse_timestamp_components(ts: &str) -> Option<(i64, u8, u32)> {
    let year: i64 = ts.get(0..4)?.parse().ok()?;
    let month: i64 = ts.get(5..7)?.parse().ok()?;
    let day: i64 = ts.get(8..10)?.parse().ok()?;
    let hour: u32 = ts.get(11..13)?.parse().ok()?;
    let minute: u32 = ts.get(14..16)?.parse().ok()?;

    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour >= 24 || minute >= 60 {
        return None;
    }
    let abs_days = year * 372 + month * 31 + day;
    let minute_of_day = hour * 60 + minute;
    let abs_min = abs_days * 1440 + i64::from(minute_of_day);
    Some((abs_min, hour as u8, minute_of_day))
}

/// Single authoritative rule classifying current activation level from sleep, ASRM, and cadence.
pub fn assess_activation_level(
    asrm: Option<u8>,
    sleep_hours: f32,
    late_night_events: usize,
    late_night_calls: usize,
    rapid_burst_clusters: usize,
) -> ActivationLevel {
    let asrm_elevated = asrm.is_some_and(|s| s >= 6);
    let low_sleep = sleep_hours < 5.5;
    let high_late_cadence = late_night_events >= 4 || late_night_calls >= 2;

    if asrm_elevated
        || (low_sleep && high_late_cadence)
        || (late_night_calls >= 2 && rapid_burst_clusters >= 1)
    {
        ActivationLevel::Elevated
    } else if sleep_hours < 6.5 || late_night_events >= 2 || rapid_burst_clusters >= 1 {
        ActivationLevel::Watch
    } else {
        ActivationLevel::Grounded
    }
}

/// Summarize stripped self-activity events into hourly counts, bursts, quiet window, and activation state.
pub fn summarize_episode_cadence(
    events: &[SelfActivityEvent],
    asrm: Option<u8>,
    sleep_hours: f32,
) -> EpisodeCadenceSummary {
    let mut hourly_counts = [0usize; 24];
    let mut late_night_events = 0usize;
    let mut late_night_calls = 0usize;

    for ev in events {
        let h = usize::from(ev.hour.min(23));
        hourly_counts[h] += 1;
        if ev.hour < 5 {
            late_night_events += 1;
            if ev.kind == ActivityEventKind::CallAttempt {
                late_night_calls += 1;
            }
        }
    }

    // Count distinct rapid-burst clusters (>= 4 events inside a 15-minute window).
    let mut rapid_burst_clusters = 0usize;
    let mut idx = 0usize;
    while idx < events.len() {
        let start = events[idx].absolute_minutes;
        let mut end = idx;
        while end + 1 < events.len() && events[end + 1].absolute_minutes - start <= 15 {
            end += 1;
        }
        if end - idx + 1 >= 4 {
            rapid_burst_clusters += 1;
            idx = end + 1;
        } else {
            idx += 1;
        }
    }

    // Longest quiet gap between consecutive events (hours).
    let longest_gap_min = events
        .windows(2)
        .map(|w| (w[1].absolute_minutes - w[0].absolute_minutes).max(0))
        .max()
        .unwrap_or(0);
    let longest_quiet_gap_hours = longest_gap_min as f64 / 60.0;

    let activation = assess_activation_level(
        asrm,
        sleep_hours,
        late_night_events,
        late_night_calls,
        rapid_burst_clusters,
    );

    EpisodeCadenceSummary {
        total_events: events.len(),
        late_night_events,
        late_night_calls,
        rapid_burst_clusters,
        longest_quiet_gap_hours,
        hourly_counts,
        activation,
    }
}

/// Export privacy-stripped aggregate episode cadence metrics as CSV (zero message text or handles).
pub fn export_episode_summary_csv(
    summary: &EpisodeCadenceSummary,
    asrm: Option<u8>,
    sleep_hours: f32,
) -> String {
    let asrm_str = asrm.map(|v| v.to_string()).unwrap_or_default();
    format!(
        "activation_level,asrm_score,sleep_hours,total_events,late_night_events_00_05,late_night_calls_00_05,rapid_burst_clusters,longest_quiet_gap_hours\n\
         \"{}\",{},{:.1},{},{},{},{},{:.2}\n",
        summary.activation.label(),
        asrm_str,
        sleep_hours,
        summary.total_events,
        summary.late_night_events,
        summary.late_night_calls,
        summary.rapid_burst_clusters,
        summary.longest_quiet_gap_hours,
    )
}

/// Interactive Episode Activation, Local-Only Activity Cadence, & Pre-Committed Cool-Down section.
#[component]
pub fn EpisodeActivationSection(
    asrm_score: RwSignal<Option<u8>>,
    sleep_hours: Signal<f32>,
) -> impl IntoView {
    let activity_input = RwSignal::new(SAMPLE_SELF_ACTIVITY_CSV.to_string());
    let events =
        RwSignal::new(parse_self_activity_log(SAMPLE_SELF_ACTIVITY_CSV).unwrap_or_default());
    let parse_error = RwSignal::new(Option::<String>::None);
    let rules = RwSignal::new(default_cooldown_rules());
    let new_rule_input = RwSignal::new(String::new());
    let asrm_text = RwSignal::new(
        asrm_score
            .get_untracked()
            .map(|v| v.to_string())
            .unwrap_or_else(|| "7".to_string()),
    );

    let apply_activity = move |_| match parse_self_activity_log(&activity_input.get()) {
        Ok(parsed) => {
            events.set(parsed);
            parse_error.set(None);
        }
        Err(err) => parse_error.set(Some(err)),
    };

    let add_rule = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let trimmed = new_rule_input.get().trim().to_string();
        if trimmed.is_empty() {
            return;
        }
        rules.update(|list| {
            let next_id = list.iter().map(|r| r.id).max().unwrap_or(0) + 1;
            list.push(CoolDownRule {
                id: next_id,
                text: trimmed,
                checked: false,
            });
        });
        new_rule_input.set(String::new());
    };

    view! {
        <section class="mt-8 rounded-2xl border border-white/10 bg-white/[0.02] p-6">
            <div class="mb-5 flex flex-wrap items-start justify-between gap-4">
                <div class="max-w-2xl">
                    <p class="mb-1 text-xs font-semibold uppercase tracking-widest text-rose-300">
                        "Local-First Episode Self-Monitoring"
                    </p>
                    <h2 class="text-lg font-semibold text-white">
                        "Episode Activation (ASRM), Self-Activity Cadence & Cool-Down Plan"
                    </h2>
                    <p class="mt-1 text-sm text-slate-400">
                        "Track sleep drops, Altman Self-Rating Mania Scale (ASRM, 0–20), and late-night messaging/call cadence from your own exported timestamps. Message text and usernames are discarded immediately in your browser."
                    </p>
                </div>
                {move || {
                    let summary = summarize_episode_cadence(&events.get(), asrm_score.get(), sleep_hours.get());
                    let badge_cls = summary.activation.badge_class();
                    let label = summary.activation.label();
                    let csv_href = csv_data_uri(&export_episode_summary_csv(
                        &summary,
                        asrm_score.get(),
                        sleep_hours.get(),
                    ));
                    view! {
                        <div class="flex flex-wrap items-center gap-2.5">
                            <span class=format!("inline-flex rounded-full border px-3 py-1 text-xs font-semibold {badge_cls}")>
                                {label}
                            </span>
                            <a
                                href=csv_href
                                download="coda-episode-cadence-summary.csv"
                                class="inline-flex items-center rounded-full border border-white/15 px-3.5 py-1.5 text-xs font-medium text-slate-200 transition hover:bg-white/5"
                            >
                                "Export aggregate CSV"
                            </a>
                        </div>
                    }
                }}
            </div>

            // Cadence KPI cards + ASRM quick control
            {move || {
                let summary = summarize_episode_cadence(&events.get(), asrm_score.get(), sleep_hours.get());
                let asrm_val = asrm_score.get();
                view! {
                    <div class="mb-6 grid grid-cols-2 gap-3 sm:grid-cols-4">
                        <div class="rounded-xl border border-white/10 bg-slate-950/60 p-3.5">
                            <p class="text-xs text-slate-400">"ASRM Score (0–20)"</p>
                            <div class="mt-1 flex items-baseline gap-2">
                                <input
                                    type="number"
                                    min="0"
                                    max="20"
                                    placeholder="—"
                                    prop:value=move || asrm_text.get()
                                    on:input=move |ev| {
                                        let raw = event_target_value(&ev);
                                        asrm_text.set(raw.clone());
                                        let parsed = raw.trim().parse::<u8>().ok().filter(|v| *v <= 20);
                                        asrm_score.set(parsed);
                                    }
                                    class="w-16 rounded-md border border-white/15 bg-slate-900 px-2 py-0.5 text-lg font-bold text-rose-300"
                                />
                                <span class="text-xs text-slate-400">
                                    {asrm_val.map(|v| classify_asrm(v).label()).unwrap_or("Unscored")}
                                </span>
                            </div>
                        </div>
                        <div class="rounded-xl border border-white/10 bg-slate-950/60 p-3.5">
                            <p class="text-xs text-slate-400">"Late-Night Events (00–05h)"</p>
                            <p class="mt-1 text-2xl font-bold text-amber-300">{summary.late_night_events}</p>
                            <p class="text-xs text-slate-400">{format!("{} late-night call attempts", summary.late_night_calls)}</p>
                        </div>
                        <div class="rounded-xl border border-white/10 bg-slate-950/60 p-3.5">
                            <p class="text-xs text-slate-400">"Rapid-Burst Clusters"</p>
                            <p class="mt-1 text-2xl font-bold text-cyan-300">{summary.rapid_burst_clusters}</p>
                            <p class="text-xs text-slate-400">"≥4 events inside 15 min"</p>
                        </div>
                        <div class="rounded-xl border border-white/10 bg-slate-950/60 p-3.5">
                            <p class="text-xs text-slate-400">"Longest Quiet Window"</p>
                            <p class="mt-1 text-2xl font-bold text-emerald-300">{format!("{:.1}h", summary.longest_quiet_gap_hours)}</p>
                            <p class="text-xs text-slate-400">{format!("{} total stripped timestamps", summary.total_events)}</p>
                        </div>
                    </div>

                    // 24-hour circadian activity cadence SVG chart
                    <div class="mb-6 rounded-xl border border-white/10 bg-slate-950/60 p-4">
                        <div class="mb-2 flex items-center justify-between text-xs text-slate-400">
                            <span class="font-medium text-slate-200">"24-Hour Self-Activity Cadence (00:00–05:00 sleep window shaded)"</span>
                            <span>"Timestamps only · zero text retained"</span>
                        </div>
                        <HourlyCadenceChart hourly_counts=summary.hourly_counts />
                    </div>
                }
            }}

            // Two columns: Local-Only Discord/Activity Timestamp Import + Pre-Committed Cool-Down Plan
            <div class="grid gap-6 lg:grid-cols-2">
                <div class="rounded-xl border border-white/10 bg-slate-950/50 p-4">
                    <h3 class="text-sm font-semibold text-white">
                        "Import Your Own Activity Timestamps (Local Only)"
                    </h3>
                    <p class="mt-1 mb-3 text-xs text-slate-400">
                        "Paste `YYYY-MM-DD HH:MM,message|call|publish` rows or rows from your own Discord Data Package `messages.csv`. Any message text or usernames are dropped at parse time."
                    </p>
                    <textarea
                        rows="6"
                        prop:value=move || activity_input.get()
                        on:input=move |ev| activity_input.set(event_target_value(&ev))
                        class="w-full rounded-lg border border-white/10 bg-slate-950 p-2.5 font-mono text-xs text-slate-200"
                    />
                    <div class="mt-3 flex flex-wrap items-center justify-between gap-2">
                        <button
                            type="button"
                            on:click=apply_activity
                            class="rounded-full bg-rose-500 px-4 py-1.5 text-xs font-semibold text-slate-950 transition hover:bg-rose-400"
                        >
                            "Analyze cadence locally"
                        </button>
                        {move || parse_error.get().map(|err| view! {
                            <span class="text-xs text-rose-400">{err}</span>
                        })}
                    </div>
                </div>

                <div class="rounded-xl border border-white/10 bg-slate-950/50 p-4">
                    <h3 class="text-sm font-semibold text-white">
                        "Pre-Committed Personal Cool-Down Plan"
                    </h3>
                    <p class="mt-1 mb-3 text-xs text-slate-400">
                        "Written while calm and stored only in this browser session. Check off each pause step before messaging, calling, or publishing during high activation."
                    </p>
                    <ul class="space-y-2 text-xs text-slate-200">
                        {move || {
                            rules.get().into_iter().map(|rule| {
                                let id = rule.id;
                                view! {
                                    <li class="flex items-start gap-2.5 rounded-lg border border-white/5 bg-white/[0.02] p-2">
                                        <input
                                            type="checkbox"
                                            prop:checked=rule.checked
                                            on:change=move |_| {
                                                rules.update(|list| {
                                                    if let Some(item) = list.iter_mut().find(|r| r.id == id) {
                                                        item.checked = !item.checked;
                                                    }
                                                });
                                            }
                                            class="mt-0.5 accent-emerald-400"
                                        />
                                        <span class=if rule.checked { "line-through text-slate-500" } else { "text-slate-200" }>
                                            {rule.text}
                                        </span>
                                    </li>
                                }
                            }).collect_view()
                        }}
                    </ul>
                    <form on:submit=add_rule class="mt-3 flex gap-2">
                        <input
                            type="text"
                            placeholder="Add your own calm-state pause rule..."
                            prop:value=move || new_rule_input.get()
                            on:input=move |ev| new_rule_input.set(event_target_value(&ev))
                            class="flex-1 rounded-lg border border-white/10 bg-slate-900 px-3 py-1.5 text-xs text-white placeholder:text-slate-500"
                        />
                        <button
                            type="submit"
                            class="rounded-full border border-white/15 px-3 py-1.5 text-xs font-medium text-slate-200 transition hover:bg-white/5"
                        >
                            "Add rule"
                        </button>
                    </form>
                </div>
            </div>
        </section>
    }
}

#[component]
fn HourlyCadenceChart(hourly_counts: [usize; 24]) -> impl IntoView {
    let width = 720.0_f64;
    let height = 120.0_f64;
    let pad_l = 24.0_f64;
    let pad_r = 12.0_f64;
    let pad_t = 12.0_f64;
    let pad_b = 22.0_f64;
    let plot_w = width - pad_l - pad_r;
    let plot_h = height - pad_t - pad_b;
    let bar_slot = plot_w / 24.0;
    let max_count = hourly_counts.iter().copied().max().unwrap_or(1).max(1) as f64;

    view! {
        <svg
            viewBox="0 0 720 120"
            class="w-full"
            role="img"
            aria-label="24-hour self-activity cadence chart highlighting 00:00 to 05:00"
        >
            // 00:00 - 05:00 late-night risk band
            <rect
                x=pad_l
                y=pad_t
                width=bar_slot * 5.0
                height=plot_h
                fill="rgba(251, 113, 133, 0.12)"
            />
            {hourly_counts.into_iter().enumerate().map(|(hour, count)| {
                let x = pad_l + hour as f64 * bar_slot + 3.0;
                let bar_w = (bar_slot - 6.0).max(4.0);
                let bar_h = if count == 0 {
                    2.0
                } else {
                    ((count as f64 / max_count) * plot_h).max(4.0)
                };
                let y = pad_t + plot_h - bar_h;
                let fill = if count == 0 {
                    "rgba(148, 163, 184, 0.18)"
                } else if hour < 5 {
                    "#fb7185"
                } else {
                    "#22d3ee"
                };
                view! {
                    <rect x=x y=y width=bar_w height=bar_h rx="2" fill=fill />
                }
            }).collect_view()}
            <text x=pad_l y=height - 6.0 fill="#fb7185" font-size="9">"00h"</text>
            <text x=pad_l + bar_slot * 5.0 y=height - 6.0 fill="#fb7185" font-size="9">"05h"</text>
            <text x=pad_l + bar_slot * 12.0 y=height - 6.0 fill="#94a3b8" font-size="9">"12h"</text>
            <text x=pad_l + bar_slot * 18.0 y=height - 6.0 fill="#94a3b8" font-size="9">"18h"</text>
            <text x=width - pad_r y=height - 6.0 text-anchor="end" fill="#94a3b8" font-size="9">"23h"</text>
        </svg>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use leptos::prelude::RenderHtml;

    #[test]
    fn parses_sample_cadence_and_detects_late_night_bursts() {
        let events = parse_self_activity_log(SAMPLE_SELF_ACTIVITY_CSV).expect("sample parses");
        assert_eq!(events.len(), 12);

        let summary = summarize_episode_cadence(&events, Some(7), 4.5);
        assert_eq!(summary.late_night_events, 8);
        assert_eq!(summary.late_night_calls, 2);
        assert_eq!(summary.rapid_burst_clusters, 2);
        assert_eq!(summary.activation, ActivationLevel::Elevated);
        assert!(summary.longest_quiet_gap_hours > 6.0);
    }

    #[test]
    fn discards_discord_message_text_and_usernames_at_parse_time() {
        let raw = "ID,Timestamp,Contents,Attachments\n\
                   12345,2026-11-22 02:31:00,\"PrivateUserHandle: secret text started a call\",\"\"\n";
        let events = parse_self_activity_log(raw).expect("discord CSV parses");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, ActivityEventKind::CallAttempt);
        assert_eq!(events[0].hour, 2);

        let summary = summarize_episode_cadence(&events, Some(4), 7.5);
        let csv = export_episode_summary_csv(&summary, Some(4), 7.5);
        assert!(!csv.contains("PrivateUserHandle"));
        assert!(!csv.contains("secret text"));
        assert!(csv.contains("activation_level,asrm_score"));
    }

    #[test]
    fn renders_episode_activation_section_on_ssr() {
        let asrm = RwSignal::new(Some(7u8));
        let sleep = RwSignal::new(5.0f32);
        let html = view! {
            <EpisodeActivationSection asrm_score=asrm sleep_hours=sleep.into() />
        }
        .to_html();
        assert!(html.contains("Episode Activation (ASRM), Self-Activity Cadence"));
        assert!(html.contains("Pre-Committed Personal Cool-Down Plan"));
        assert!(html.contains("24-hour self-activity cadence chart"));
    }
}
