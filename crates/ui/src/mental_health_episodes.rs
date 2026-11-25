//! Pure-Rust, local-first Opt-In Multi-Platform Self-Activity Synthesizer & Episode Activation Monitor.
//!
//! Designed strictly for personal self-monitoring on-device: accepts official personal exports
//! from Discord (`messages.csv` / `.jsonl`), Telegram (`result.json`), WhatsApp (`.txt`),
//! and Facebook Messenger (`message_*.json`). Multi-party exports fail closed without the
//! user's own sender handle; all other participants, message text, IDs, attachments, and
//! usernames are discarded immediately in WASM—only stripped timestamps persist locally.

use std::collections::BTreeMap;

use leptos::prelude::*;
use serde::{Deserialize, Serialize};

use crate::mental_health_exports::{
    load_synthetic_4platform_synthesis, merge_synthesized_events, parse_platform_export,
    purge_platform_events, synthesize_opted_in_events, ActivityPlatform, PlatformBreakdown,
    PlatformConsentState,
};
use crate::mental_health_lingo::csv_data_uri;

#[cfg(any(feature = "hydrate", feature = "csr"))]
const STORAGE_KEY_ASRM: &str = "dabney.mental_health.v1.asrm";
#[cfg(any(feature = "hydrate", feature = "csr"))]
const STORAGE_KEY_RULES: &str = "dabney.mental_health.v1.cooldown_rules";
#[cfg(any(feature = "hydrate", feature = "csr"))]
const STORAGE_KEY_CADENCE: &str = "dabney.mental_health.v1.stripped_cadence";
#[cfg(any(feature = "hydrate", feature = "csr"))]
const STORAGE_KEY_CONSENT: &str = "dabney.mental_health.v1.platform_consent";

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
    /// Calendar date key (`YYYY-MM-DD`) for multi-day actogram grouping.
    pub date_key: String,
    /// Stripped `YYYY-MM-DD HH:MM` timestamp for privacy-safe local persistence.
    pub formatted_ts: String,
    /// Hour of day (`0..24`).
    pub hour: u8,
    /// Minute of day (`0..1440`).
    pub minute_of_day: u32,
    /// Absolute pseudo-minutes for chronological burst, baseline, & quiet-gap analysis.
    pub absolute_minutes: i64,
    /// Coarse event category.
    pub kind: ActivityEventKind,
    /// Opt-in source platform (for cross-platform synthesis and per-platform revocation).
    pub platform: ActivityPlatform,
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

/// One calendar day's hourly activity counts for the Day × Hour actogram heatmap.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DayActogramRow {
    pub date: String,
    pub hourly: [usize; 24],
    pub late_night_count: usize,
    pub total: usize,
}

/// Comparison of recent 48-hour late-night cadence against the user's own earlier baseline.
#[derive(Clone, Debug, PartialEq)]
pub struct BaselineComparison {
    pub recent_48h_events: usize,
    pub recent_48h_late_night_pct: f64,
    pub baseline_events: usize,
    pub baseline_late_night_pct: Option<f64>,
    pub late_night_ratio: Option<f64>,
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
    pub actogram_days: Vec<DayActogramRow>,
    pub baseline: BaselineComparison,
    pub by_platform: Vec<PlatformBreakdown>,
    pub activation: ActivationLevel,
}

/// A user-authored, local-only rule in the pre-committed cool-down plan.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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

/// 100% synthetic 5-day self-activity sample (3 grounded baseline days + 2 late-night activation days).
pub const SAMPLE_SELF_ACTIVITY_CSV: &str = "\
timestamp,event_type
2026-11-18 09:10,message
2026-11-18 11:30,message
2026-11-18 15:05,message
2026-11-18 19:20,message
2026-11-18 21:40,message
2026-11-19 08:45,message
2026-11-19 12:15,message
2026-11-19 16:30,message
2026-11-19 20:10,message
2026-11-19 22:05,message
2026-11-20 09:30,message
2026-11-20 13:20,message
2026-11-20 17:45,message
2026-11-20 21:15,message
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

/// Serialize privacy-stripped events back into minimal `timestamp,event_type,platform` CSV.
pub fn events_to_stripped_csv(events: &[SelfActivityEvent]) -> String {
    let mut out = String::from("timestamp,event_type,platform\n");
    for ev in events {
        out.push_str(&ev.formatted_ts);
        out.push(',');
        out.push_str(ev.kind.label());
        out.push(',');
        out.push_str(ev.platform.id());
        out.push('\n');
    }
    out
}

/// Parse a user's local self-activity CSV or Discord Data Package `messages.csv` rows.
pub fn parse_self_activity_log(input: &str) -> Result<Vec<SelfActivityEvent>, String> {
    parse_self_activity_log_with_filter(input, None)
}

/// Parse CSV (`timestamp,event_type` / Discord `ID,Timestamp,Contents`) or Discord JSONL
/// exports locally, discarding all message bodies, IDs, and usernames immediately.
///
/// When `author_filter` is `Some(non_empty)` and input lines are JSONL objects, only
/// records whose author matches `author_filter` (case-insensitive substring on
/// `global_name`, `username`, `display_name`, or `nick`) are retained.
pub fn parse_self_activity_log_with_filter(
    input: &str,
    author_filter: Option<&str>,
) -> Result<Vec<SelfActivityEvent>, String> {
    let filter_lower = author_filter
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_ascii_lowercase);
    let mut events = Vec::new();

    for (idx, raw_line) in input.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if line.starts_with('{') {
            if let Some(ev) = parse_jsonl_line(line, filter_lower.as_deref())
                .map_err(|err| format!("Line {}: {err}", idx + 1))?
            {
                events.push(ev);
            }
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

        let Some((date_key, formatted_ts, abs_min, hour, minute_of_day)) =
            parse_timestamp_components(&ts_str)
        else {
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
        let platform = remainder
            .split(',')
            .nth(2)
            .map(ActivityPlatform::from_id)
            .unwrap_or_else(|| {
                if lower.contains("discord") {
                    ActivityPlatform::Discord
                } else {
                    ActivityPlatform::ManualOrGeneric
                }
            });

        events.push(SelfActivityEvent {
            date_key,
            formatted_ts,
            hour,
            minute_of_day,
            absolute_minutes: abs_min,
            kind,
            platform,
        });
    }

    if events.is_empty() {
        return Err(
            "No matching timestamped self-activity rows found (check author filter if using JSONL)."
                .to_string(),
        );
    }

    events.sort_by_key(|e| e.absolute_minutes);
    Ok(events)
}

fn parse_jsonl_line(
    line: &str,
    filter_lower: Option<&str>,
) -> Result<Option<SelfActivityEvent>, String> {
    let val: serde_json::Value =
        serde_json::from_str(line).map_err(|err| format!("invalid JSONL: {err}"))?;
    let msg = val.get("message").unwrap_or(&val);

    if let Some(needle) = filter_lower {
        let author = msg.get("author");
        let matched = author
            .map(|a| {
                ["global_name", "username", "display_name", "nick"]
                    .iter()
                    .filter_map(|k| a.get(*k).and_then(serde_json::Value::as_str))
                    .any(|s| s.to_ascii_lowercase().contains(needle))
            })
            .unwrap_or(false);
        if !matched {
            return Ok(None);
        }
    }

    let Some(ts_raw) = msg.get("timestamp").and_then(serde_json::Value::as_str) else {
        return Ok(None);
    };
    let Some((ts_str, _)) = extract_timestamp_and_rest(ts_raw) else {
        return Ok(None);
    };
    let Some((date_key, formatted_ts, abs_min, hour, minute_of_day)) =
        parse_timestamp_components(&ts_str)
    else {
        return Ok(None);
    };

    let msg_type = msg
        .get("type")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let kind = if msg_type == 3 {
        ActivityEventKind::CallAttempt
    } else {
        ActivityEventKind::Message
    };

    Ok(Some(SelfActivityEvent {
        date_key,
        formatted_ts,
        hour,
        minute_of_day,
        absolute_minutes: abs_min,
        kind,
        platform: ActivityPlatform::Discord,
    }))
}

/// Locate a `YYYY-MM-DD[T ]HH:MM` timestamp anywhere in a CSV or ISO-8601 string.
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

pub(crate) fn parse_timestamp_components(ts: &str) -> Option<(String, String, i64, u8, u32)> {
    let year: i64 = ts.get(0..4)?.parse().ok()?;
    let month: i64 = ts.get(5..7)?.parse().ok()?;
    let day: i64 = ts.get(8..10)?.parse().ok()?;
    let hour: u32 = ts.get(11..13)?.parse().ok()?;
    let minute: u32 = ts.get(14..16)?.parse().ok()?;

    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour >= 24 || minute >= 60 {
        return None;
    }
    let date_key = format!("{year:04}-{month:02}-{day:02}");
    let formatted_ts = format!("{date_key} {hour:02}:{minute:02}");
    let abs_days = year * 372 + month * 31 + day;
    let minute_of_day = hour * 60 + minute;
    let abs_min = abs_days * 1440 + i64::from(minute_of_day);
    Some((date_key, formatted_ts, abs_min, hour as u8, minute_of_day))
}

/// Single authoritative rule classifying current activation level from sleep, ASRM, cadence,
/// and personal baseline deviation.
pub fn assess_activation_level(
    asrm: Option<u8>,
    sleep_hours: f32,
    late_night_events: usize,
    late_night_calls: usize,
    rapid_burst_clusters: usize,
    baseline_ratio: Option<f64>,
) -> ActivationLevel {
    let asrm_elevated = asrm.is_some_and(|s| s >= 6);
    let low_sleep = sleep_hours < 5.5;
    let baseline_spike = baseline_ratio.is_some_and(|r| r >= 2.0);
    let high_late_cadence = late_night_events >= 4 || late_night_calls >= 2 || baseline_spike;

    if asrm_elevated
        || (low_sleep && high_late_cadence)
        || (late_night_calls >= 2 && rapid_burst_clusters >= 1)
    {
        ActivationLevel::Elevated
    } else if sleep_hours < 6.5
        || late_night_events >= 2
        || rapid_burst_clusters >= 1
        || baseline_ratio.is_some_and(|r| r >= 1.5)
    {
        ActivationLevel::Watch
    } else {
        ActivationLevel::Grounded
    }
}

/// Summarize stripped self-activity events into hourly counts, 14-day actogram rows,
/// personal baseline comparison, bursts, quiet window, and activation state.
pub fn summarize_episode_cadence(
    events: &[SelfActivityEvent],
    asrm: Option<u8>,
    sleep_hours: f32,
) -> EpisodeCadenceSummary {
    let mut hourly_counts = [0usize; 24];
    let mut by_day: BTreeMap<String, DayActogramRow> = BTreeMap::new();

    for ev in events {
        let h = usize::from(ev.hour.min(23));
        hourly_counts[h] += 1;
        let row = by_day
            .entry(ev.date_key.clone())
            .or_insert_with(|| DayActogramRow {
                date: ev.date_key.clone(),
                hourly: [0; 24],
                late_night_count: 0,
                total: 0,
            });
        row.hourly[h] += 1;
        row.total += 1;
        if ev.hour < 5 {
            row.late_night_count += 1;
        }
    }

    let mut actogram_days: Vec<DayActogramRow> = by_day.into_values().collect();
    if actogram_days.len() > 14 {
        let drop_n = actogram_days.len() - 14;
        actogram_days.drain(0..drop_n);
    }

    // Split into recent 48h window vs earlier personal baseline (when > 48h exists).
    let latest_min = events.last().map(|e| e.absolute_minutes).unwrap_or(0);
    let recent_cutoff = latest_min - 48 * 60;
    let recent_slice: Vec<&SelfActivityEvent> = events
        .iter()
        .filter(|e| e.absolute_minutes >= recent_cutoff)
        .collect();
    let baseline_slice: Vec<&SelfActivityEvent> = events
        .iter()
        .filter(|e| e.absolute_minutes < recent_cutoff)
        .collect();

    let late_night_events = recent_slice.iter().filter(|e| e.hour < 5).count();
    let late_night_calls = recent_slice
        .iter()
        .filter(|e| e.hour < 5 && e.kind == ActivityEventKind::CallAttempt)
        .count();

    let recent_48h_late_night_pct = if recent_slice.is_empty() {
        0.0
    } else {
        (late_night_events as f64 / recent_slice.len() as f64) * 100.0
    };
    let baseline_late_night_pct = if baseline_slice.len() >= 4 {
        let base_late = baseline_slice.iter().filter(|e| e.hour < 5).count();
        Some((base_late as f64 / baseline_slice.len() as f64) * 100.0)
    } else {
        None
    };
    let late_night_ratio = baseline_late_night_pct.and_then(|base| {
        if base > 0.5 {
            Some(recent_48h_late_night_pct / base)
        } else if recent_48h_late_night_pct > 0.0 {
            Some(recent_48h_late_night_pct / 5.0)
        } else {
            None
        }
    });

    // Count distinct rapid-burst clusters (>= 4 events inside a 15-minute window) in recent 48h.
    let mut rapid_burst_clusters = 0usize;
    let mut idx = 0usize;
    while idx < recent_slice.len() {
        let start = recent_slice[idx].absolute_minutes;
        let mut end = idx;
        while end + 1 < recent_slice.len() && recent_slice[end + 1].absolute_minutes - start <= 15 {
            end += 1;
        }
        if end - idx + 1 >= 4 {
            rapid_burst_clusters += 1;
            idx = end + 1;
        } else {
            idx += 1;
        }
    }

    // Longest quiet gap between consecutive events in recent 48h (hours).
    let longest_gap_min = recent_slice
        .windows(2)
        .map(|w| (w[1].absolute_minutes - w[0].absolute_minutes).max(0))
        .max()
        .unwrap_or(0);
    let longest_quiet_gap_hours = longest_gap_min as f64 / 60.0;

    // Per-platform breakdown across the synthesized active events.
    let mut platform_totals: BTreeMap<ActivityPlatform, (usize, usize, usize)> = BTreeMap::new();
    for ev in events {
        let entry = platform_totals.entry(ev.platform).or_insert((0, 0, 0));
        entry.0 += 1;
        if ev.absolute_minutes >= recent_cutoff {
            entry.1 += 1;
            if ev.hour < 5 {
                entry.2 += 1;
            }
        }
    }
    let total_f = events.len().max(1) as f64;
    let by_platform: Vec<PlatformBreakdown> = platform_totals
        .into_iter()
        .map(
            |(platform, (total_events, recent_48h_events, late_night_events))| PlatformBreakdown {
                platform,
                total_events,
                recent_48h_events,
                late_night_events,
                share_pct: (total_events as f64 / total_f) * 100.0,
            },
        )
        .collect();

    let activation = assess_activation_level(
        asrm,
        sleep_hours,
        late_night_events,
        late_night_calls,
        rapid_burst_clusters,
        late_night_ratio,
    );

    EpisodeCadenceSummary {
        total_events: events.len(),
        late_night_events,
        late_night_calls,
        rapid_burst_clusters,
        longest_quiet_gap_hours,
        hourly_counts,
        actogram_days,
        baseline: BaselineComparison {
            recent_48h_events: recent_slice.len(),
            recent_48h_late_night_pct,
            baseline_events: baseline_slice.len(),
            baseline_late_night_pct,
            late_night_ratio,
        },
        by_platform,
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
    let ratio_str = summary
        .baseline
        .late_night_ratio
        .map(|r| format!("{r:.2}"))
        .unwrap_or_default();
    format!(
        "activation_level,asrm_score,sleep_hours,total_events,recent_48h_events,late_night_events_00_05,late_night_calls_00_05,recent_late_night_pct,baseline_late_night_ratio,rapid_burst_clusters,longest_quiet_gap_hours\n\
         \"{}\",{},{:.1},{},{},{},{},{:.1},{},{},{:.2}\n",
        summary.activation.label(),
        asrm_str,
        sleep_hours,
        summary.total_events,
        summary.baseline.recent_48h_events,
        summary.late_night_events,
        summary.late_night_calls,
        summary.baseline.recent_48h_late_night_pct,
        ratio_str,
        summary.rapid_burst_clusters,
        summary.longest_quiet_gap_hours,
    )
}

#[cfg(any(feature = "hydrate", feature = "csr"))]
fn read_local_storage(key: &str) -> Option<String> {
    let storage = web_sys::window()?.local_storage().ok()??;
    storage.get_item(key).ok().flatten()
}

#[cfg(any(feature = "hydrate", feature = "csr"))]
fn write_local_storage(key: &str, value: &str) {
    if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        let _ = storage.set_item(key, value);
    }
}

#[cfg(any(feature = "hydrate", feature = "csr"))]
fn clear_episode_local_storage() {
    if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        let _ = storage.remove_item(STORAGE_KEY_ASRM);
        let _ = storage.remove_item(STORAGE_KEY_RULES);
        let _ = storage.remove_item(STORAGE_KEY_CADENCE);
        let _ = storage.remove_item(STORAGE_KEY_CONSENT);
    }
}

/// Top-of-page calm Cool-Down banner surfaced automatically when Watch or Elevated signals stack up.
#[component]
pub fn TopCoolDownBanner(
    events: Signal<Vec<SelfActivityEvent>>,
    asrm_score: Signal<Option<u8>>,
    sleep_hours: Signal<f32>,
) -> impl IntoView {
    move || {
        let summary = summarize_episode_cadence(&events.get(), asrm_score.get(), sleep_hours.get());
        if summary.activation == ActivationLevel::Grounded {
            return None;
        }
        let is_elevated = summary.activation == ActivationLevel::Elevated;
        let container_cls = if is_elevated {
            "mb-6 flex flex-wrap items-center justify-between gap-3 rounded-2xl border border-rose-400/30 bg-rose-500/10 px-5 py-3.5"
        } else {
            "mb-6 flex flex-wrap items-center justify-between gap-3 rounded-2xl border border-amber-400/30 bg-amber-500/10 px-5 py-3.5"
        };
        let ratio_note = summary
            .baseline
            .late_night_ratio
            .map(|r| format!(" · {r:.1}× personal late-night baseline"))
            .unwrap_or_default();
        let detail = format!(
            "Self-monitoring signal (not a medical diagnosis): {} late-night events (00–05h), {} burst clusters{ratio_note}. Pause and review your pre-committed plan before messaging or publishing.",
            summary.late_night_events,
            summary.rapid_burst_clusters,
        );
        Some(view! {
            <div class=container_cls role="status">
                <div class="max-w-3xl">
                    <p class=if is_elevated { "text-xs font-semibold uppercase tracking-wider text-rose-300" } else { "text-xs font-semibold uppercase tracking-wider text-amber-300" }>
                        {summary.activation.label()}
                    </p>
                    <p class="mt-0.5 text-xs text-slate-200">{detail}</p>
                </div>
                <a
                    href="#cooldown-plan"
                    class="inline-flex items-center rounded-full border border-white/20 bg-slate-950/70 px-3.5 py-1.5 text-xs font-semibold text-white transition hover:bg-slate-900"
                >
                    "Open Cool-Down Plan ↓"
                </a>
            </div>
        })
    }
}

/// Interactive Opt-In Multi-Platform Self-Activity Synthesizer, 14-Day Actogram & Cool-Down section.
#[component]
pub fn EpisodeActivationSection(
    events: RwSignal<Vec<SelfActivityEvent>>,
    consent: RwSignal<PlatformConsentState>,
    asrm_score: RwSignal<Option<u8>>,
    sleep_hours: Signal<f32>,
) -> impl IntoView {
    let activity_input = RwSignal::new(events_to_stripped_csv(&events.get_untracked()));
    let author_filter = RwSignal::new(String::new());
    let platform_choice = RwSignal::new(Option::<ActivityPlatform>::None);
    let merge_mode = RwSignal::new(true);
    let parse_error = RwSignal::new(Option::<String>::None);
    let import_status = RwSignal::new(Option::<String>::None);
    let rules = RwSignal::new(default_cooldown_rules());
    let asrm_text = RwSignal::new(
        asrm_score
            .get_untracked()
            .map(|v| v.to_string())
            .unwrap_or_else(|| "7".to_string()),
    );
    let new_rule_input = RwSignal::new(String::new());
    #[cfg(any(feature = "hydrate", feature = "csr"))]
    let hydrated = RwSignal::new(false);

    // Restore saved local-only state on mount in the browser.
    #[cfg(any(feature = "hydrate", feature = "csr"))]
    Effect::new(move |_| {
        if hydrated.get_untracked() {
            return;
        }
        if let Some(saved_asrm) = read_local_storage(STORAGE_KEY_ASRM) {
            if let Ok(v) = saved_asrm.parse::<u8>() {
                if v <= 20 {
                    asrm_score.set(Some(v));
                    asrm_text.set(v.to_string());
                }
            }
        }
        if let Some(saved_rules) = read_local_storage(STORAGE_KEY_RULES) {
            if let Ok(parsed_rules) = serde_json::from_str::<Vec<CoolDownRule>>(&saved_rules) {
                if !parsed_rules.is_empty() {
                    rules.set(parsed_rules);
                }
            }
        }
        if let Some(saved_consent) = read_local_storage(STORAGE_KEY_CONSENT) {
            if let Ok(parsed_consent) = serde_json::from_str::<PlatformConsentState>(&saved_consent)
            {
                consent.set(parsed_consent);
            }
        }
        if let Some(saved_csv) = read_local_storage(STORAGE_KEY_CADENCE) {
            if let Ok(parsed) = parse_self_activity_log(&saved_csv) {
                activity_input.set(saved_csv);
                events.set(parsed);
            }
        }
        hydrated.set(true);
    });

    // Persist ASRM, Cool-Down rules, and Opt-In Platform Consent locally whenever updated.
    #[cfg(any(feature = "hydrate", feature = "csr"))]
    Effect::new(move |_| {
        let asrm_val = asrm_score.get();
        let current_rules = rules.get();
        let current_consent = consent.get();
        if !hydrated.get() {
            return;
        }
        if let Some(v) = asrm_val {
            write_local_storage(STORAGE_KEY_ASRM, &v.to_string());
        }
        if let Ok(json) = serde_json::to_string(&current_rules) {
            write_local_storage(STORAGE_KEY_RULES, &json);
        }
        if let Ok(json) = serde_json::to_string(&current_consent) {
            write_local_storage(STORAGE_KEY_CONSENT, &json);
        }
    });

    let apply_raw_text = move |raw: &str, require_consent_checkbox: bool| {
        let current_consent = consent.get();
        if require_consent_checkbox && !current_consent.self_only_confirmed {
            parse_error.set(Some(
                "Check the opt-in self-only consent box above before importing a personal export file."
                    .to_string(),
            ));
            import_status.set(None);
            return;
        }

        let filter = author_filter.get();
        let filter_opt = if filter.trim().is_empty() {
            None
        } else {
            Some(filter.as_str())
        };
        match parse_platform_export(raw, platform_choice.get(), filter_opt) {
            Ok(parsed) => {
                if !current_consent.is_opted_in(parsed.platform) {
                    parse_error.set(Some(format!(
                        "{} is currently opted out. Toggle {} opt-in ON first to synthesize its timestamps.",
                        parsed.platform.display_name(),
                        parsed.platform.display_name(),
                    )));
                    import_status.set(None);
                    return;
                }
                let imported_count = parsed.events.len();
                let dropped_others = parsed.dropped_other_participants;
                let updated = if merge_mode.get() {
                    merge_synthesized_events(&events.get(), &parsed.events)
                } else {
                    parsed.events
                };
                let stripped = events_to_stripped_csv(&updated);
                events.set(updated);
                activity_input.set(stripped.clone());
                #[cfg(any(feature = "hydrate", feature = "csr"))]
                write_local_storage(STORAGE_KEY_CADENCE, &stripped);
                parse_error.set(None);
                let drop_note = if dropped_others > 0 {
                    format!(" ({dropped_others} other-participant rows discarded)")
                } else {
                    String::new()
                };
                import_status.set(Some(format!(
                    "Synthesized {imported_count} stripped {} timestamps locally{drop_note}. Zero message text stored.",
                    parsed.platform.display_name(),
                )));
            }
            Err(err) => {
                parse_error.set(Some(err));
                import_status.set(None);
            }
        }
    };

    let apply_activity = move |_| {
        let raw = activity_input.get();
        apply_raw_text(&raw, false);
    };

    let load_4platform_demo = move |_| {
        let demo = load_synthetic_4platform_synthesis();
        let stripped = events_to_stripped_csv(&demo);
        consent.update(|c| {
            c.discord = true;
            c.telegram = true;
            c.whatsapp = true;
            c.messenger = true;
        });
        events.set(demo);
        activity_input.set(stripped);
        parse_error.set(None);
        import_status.set(Some(
            "Loaded 100% synthetic 4-platform demo (Discord + Telegram + WhatsApp + Messenger; fictional Alex Demo)."
                .to_string(),
        ));
    };

    let on_file_change = move |ev: leptos::ev::Event| {
        #[cfg(any(feature = "hydrate", feature = "csr"))]
        {
            use wasm_bindgen::closure::Closure;
            use wasm_bindgen::JsCast;

            let Some(target) = ev.target() else { return };
            let Ok(input_el) = target.dyn_into::<web_sys::HtmlInputElement>() else {
                return;
            };
            let Some(files) = input_el.files() else {
                return;
            };
            let Some(file) = files.get(0) else { return };
            let Ok(reader) = web_sys::FileReader::new() else {
                parse_error.set(Some("Could not initialize browser FileReader.".into()));
                return;
            };
            let onload = Closure::<dyn FnMut(web_sys::ProgressEvent)>::new(
                move |pe: web_sys::ProgressEvent| {
                    let Some(t) = pe.target() else { return };
                    let Ok(r) = t.dyn_into::<web_sys::FileReader>() else {
                        return;
                    };
                    if let Ok(val) = r.result() {
                        if let Some(text) = val.as_string() {
                            apply_raw_text(&text, true);
                        }
                    }
                },
            );
            reader.set_onload(Some(onload.as_ref().unchecked_ref()));
            onload.forget();
            if reader.read_as_text(&file).is_err() {
                parse_error.set(Some("Could not read selected file as UTF-8 text.".into()));
            }
        }
        #[cfg(not(any(feature = "hydrate", feature = "csr")))]
        let _ = ev;
    };

    let wipe_local_data = move |_| {
        #[cfg(any(feature = "hydrate", feature = "csr"))]
        clear_episode_local_storage();
        let demo = load_synthetic_4platform_synthesis();
        let stripped = events_to_stripped_csv(&demo);
        activity_input.set(stripped);
        events.set(demo);
        consent.set(PlatformConsentState::default());
        rules.set(default_cooldown_rules());
        asrm_score.set(Some(7));
        asrm_text.set("7".to_string());
        parse_error.set(None);
        import_status.set(Some(
            "Local browser storage cleared and reset to synthetic 4-platform demo.".into(),
        ));
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
                        "Opt-In Local-First Multi-Platform Synthesizer"
                    </p>
                    <h2 class="text-lg font-semibold text-white">
                        "Episode Activation (ASRM), Cross-Platform Self-Activity Actogram & Cool-Down Plan"
                    </h2>
                    <p class="mt-1 text-sm text-slate-400">
                        "Synthesize your own opted-in exports from Discord, Telegram, WhatsApp, and Messenger directly in browser WASM. Multi-party exports require your own handle—every other participant and all message text are discarded immediately."
                    </p>
                </div>
                {move || {
                    let active_events = synthesize_opted_in_events(&events.get(), &consent.get());
                    let summary = summarize_episode_cadence(&active_events, asrm_score.get(), sleep_hours.get());
                    let badge_cls = summary.activation.badge_class();
                    let label = summary.activation.label();
                    let csv_href = csv_data_uri(&export_episode_summary_csv(
                        &summary,
                        asrm_score.get(),
                        sleep_hours.get(),
                    ));
                    view! {
                        <div class="flex flex-wrap items-center gap-2">
                            <span class=format!("inline-flex rounded-full border px-3 py-1 text-xs font-semibold {badge_cls}")>
                                {label}
                            </span>
                            <button
                                type="button"
                                on:click=load_4platform_demo
                                class="rounded-full border border-cyan-400/30 px-3 py-1.5 text-xs font-medium text-cyan-300 transition hover:bg-cyan-500/10"
                            >
                                "Load 4-Platform Demo"
                            </button>
                            <a
                                href=csv_href
                                download="dabney-episode-cadence-summary.csv"
                                class="inline-flex items-center rounded-full border border-white/15 px-3 py-1.5 text-xs font-medium text-slate-200 transition hover:bg-white/5"
                            >
                                "Export aggregate CSV"
                            </a>
                            <button
                                type="button"
                                on:click=wipe_local_data
                                class="rounded-full border border-rose-400/25 px-3 py-1.5 text-xs font-medium text-rose-300 transition hover:bg-rose-500/10"
                            >
                                "Wipe local data"
                            </button>
                        </div>
                    }
                }}
            </div>

            // Explicit Per-Platform Opt-In Consent & Cross-Platform Synthesis Strip
            {move || {
                let raw_events = events.get();
                let current_consent = consent.get();
                let active_events = synthesize_opted_in_events(&raw_events, &current_consent);
                let summary = summarize_episode_cadence(&active_events, asrm_score.get(), sleep_hours.get());
                view! {
                    <div class="mb-6 rounded-xl border border-white/10 bg-slate-950/60 p-4">
                        <div class="mb-3 flex flex-wrap items-center justify-between gap-2">
                            <div>
                                <p class="text-xs font-semibold uppercase tracking-wider text-cyan-300">
                                    "Per-Platform Opt-In Consent & Synthesis Ledger"
                                </p>
                                <p class="text-xs text-slate-400">
                                    "Toggle any platform off to exclude it immediately, or purge its stripped timestamps from local storage."
                                </p>
                            </div>
                            <span class="rounded-full border border-white/10 bg-white/5 px-2.5 py-1 text-xs text-slate-300">
                                {format!(
                                    "{}/4 platforms opted in · {} synthesized timestamps",
                                    current_consent.opted_in_count(),
                                    summary.total_events
                                )}
                            </span>
                        </div>
                        <div class="grid gap-2.5 sm:grid-cols-2 lg:grid-cols-4">
                            {ActivityPlatform::ALL_OPT_IN.into_iter().map(|platform| {
                                let opted_in = current_consent.is_opted_in(platform);
                                let stored_count = raw_events.iter().filter(|e| e.platform == platform).count();
                                let breakdown = summary.by_platform.iter().find(|b| b.platform == platform);
                                let late_n = breakdown.map(|b| b.late_night_events).unwrap_or(0);
                                let share = breakdown.map(|b| b.share_pct).unwrap_or(0.0);
                                let badge_cls = platform.badge_class();
                                view! {
                                    <div class=format!(
                                        "rounded-xl border p-3 transition {}",
                                        if opted_in {
                                            "border-white/15 bg-white/[0.03]"
                                        } else {
                                            "border-white/5 bg-slate-950/40 opacity-65"
                                        }
                                    )>
                                        <div class="flex items-center justify-between gap-2">
                                            <label class="inline-flex cursor-pointer items-center gap-2 text-xs font-semibold text-white">
                                                <input
                                                    type="checkbox"
                                                    prop:checked=opted_in
                                                    on:change=move |_| {
                                                        consent.update(|c| c.toggle(platform));
                                                    }
                                                    class="accent-cyan-400"
                                                />
                                                <span>{platform.display_name()}</span>
                                            </label>
                                            <span class=format!("rounded-full border px-2 py-0.5 text-[10px] font-semibold {badge_cls}")>
                                                {if opted_in { "Opted in" } else { "Excluded" }}
                                            </span>
                                        </div>
                                        <p class="mt-2 text-xs text-slate-300">
                                            {if opted_in {
                                                format!("{stored_count} events ({share:.0}%) · {late_n} late-night")
                                            } else {
                                                format!("{stored_count} cached · excluded from synthesis")
                                            }}
                                        </p>
                                        <div class="mt-2 flex items-center justify-between gap-2">
                                            <span class="truncate text-[10px] text-slate-500">{platform.export_hint()}</span>
                                            {(stored_count > 0).then(|| view! {
                                                <button
                                                    type="button"
                                                    on:click=move |_| {
                                                        let remaining = purge_platform_events(&events.get(), platform);
                                                        let stripped = events_to_stripped_csv(&remaining);
                                                        events.set(remaining);
                                                        activity_input.set(stripped.clone());
                                                        #[cfg(any(feature = "hydrate", feature = "csr"))]
                                                        write_local_storage(STORAGE_KEY_CADENCE, &stripped);
                                                        import_status.set(Some(format!(
                                                            "Purged all {} timestamps from local storage.",
                                                            platform.display_name()
                                                        )));
                                                    }
                                                    class="shrink-0 text-[10px] font-medium text-rose-300 transition hover:text-rose-200"
                                                >
                                                    "Purge"
                                                </button>
                                            })}
                                        </div>
                                    </div>
                                }
                            }).collect_view()}
                        </div>
                        {(!summary.by_platform.is_empty()).then(|| view! {
                            <div class="mt-3">
                                <div class="flex h-2 w-full overflow-hidden rounded-full bg-slate-900">
                                    {summary.by_platform.iter().map(|b| view! {
                                        <div
                                            class=b.platform.bar_class()
                                            style=format!("width: {:.2}%", b.share_pct)
                                            title=format!("{}: {} events ({:.0}%)", b.platform.display_name(), b.total_events, b.share_pct)
                                        />
                                    }).collect_view()}
                                </div>
                            </div>
                        })}
                    </div>
                }
            }}

            // Cadence KPI cards + ASRM + Personal Baseline Ratio
            {move || {
                let active_events = synthesize_opted_in_events(&events.get(), &consent.get());
                let summary = summarize_episode_cadence(&active_events, asrm_score.get(), sleep_hours.get());
                let asrm_val = asrm_score.get();
                let baseline_detail = match (summary.baseline.baseline_late_night_pct, summary.baseline.late_night_ratio) {
                    (Some(base), Some(ratio)) => format!("{:.0}% (48h) vs {:.0}% base ({ratio:.1}×)", summary.baseline.recent_48h_late_night_pct, base),
                    _ => format!("{:.0}% of recent 48h events", summary.baseline.recent_48h_late_night_pct),
                };
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
                            <p class="text-xs text-slate-400">"Late-Night (00–05h, 48h)"</p>
                            <p class="mt-1 text-2xl font-bold text-amber-300">{summary.late_night_events}</p>
                            <p class="text-xs text-slate-400">{baseline_detail}</p>
                        </div>
                        <div class="rounded-xl border border-white/10 bg-slate-950/60 p-3.5">
                            <p class="text-xs text-slate-400">"Rapid-Burst Clusters"</p>
                            <p class="mt-1 text-2xl font-bold text-cyan-300">{summary.rapid_burst_clusters}</p>
                            <p class="text-xs text-slate-400">{format!("{} late-night calls · ≥4 / 15m", summary.late_night_calls)}</p>
                        </div>
                        <div class="rounded-xl border border-white/10 bg-slate-950/60 p-3.5">
                            <p class="text-xs text-slate-400">"Longest Quiet Gap (48h)"</p>
                            <p class="mt-1 text-2xl font-bold text-emerald-300">{format!("{:.1}h", summary.longest_quiet_gap_hours)}</p>
                            <p class="text-xs text-slate-400">{format!("{} synthesized timestamps", summary.total_events)}</p>
                        </div>
                    </div>

                    // Multi-day Day x Hour Actogram Heatmap (up to 14 active days)
                    <div class="mb-6 rounded-xl border border-white/10 bg-slate-950/60 p-4">
                        <div class="mb-2 flex flex-wrap items-center justify-between gap-2 text-xs text-slate-400">
                            <span class="font-medium text-slate-200">
                                "Multi-Day Circadian Actogram (Day × Hour · 00–05h shaded rose)"
                            </span>
                            <span>"Unified across opted-in platforms · personal baseline vs. recent 48h"</span>
                        </div>
                        <DayHourActogram days=summary.actogram_days />
                    </div>
                }
            }}

            // Two columns: Multi-Platform Local Export Importer + Pre-Committed Cool-Down Plan
            <div class="grid gap-6 lg:grid-cols-2">
                <div class="rounded-xl border border-white/10 bg-slate-950/50 p-4">
                    <h3 class="text-sm font-semibold text-white">
                        "Import Personal Exports (Discord, Telegram, WhatsApp, Messenger)"
                    </h3>
                    <p class="mt-1 mb-3 text-xs text-slate-400">
                        "Drop official personal export files directly into your browser (`messages.csv`/`.jsonl`, Telegram `result.json`, WhatsApp `.txt`, Messenger `message_*.json`). Multi-party exports fail closed unless your own sender name is set."
                    </p>
                    <label class="mb-3 flex items-start gap-2 rounded-lg border border-cyan-400/20 bg-cyan-500/5 p-2.5 text-xs text-slate-200">
                        <input
                            type="checkbox"
                            prop:checked=move || consent.get().self_only_confirmed
                            on:change=move |_| {
                                consent.update(|c| c.self_only_confirmed = !c.self_only_confirmed);
                            }
                            class="mt-0.5 accent-cyan-400"
                        />
                        <span>
                            "Opt-in consent: I confirm any file I upload is my own personal export, and I want only my own timestamps synthesized locally (discarding all other participants and message text immediately)."
                        </span>
                    </label>
                    <div class="mb-3 grid gap-2 sm:grid-cols-3">
                        <label class="block text-[11px] text-slate-300">
                            "Platform format"
                            <select
                                on:change=move |ev| {
                                    let val = event_target_value(&ev);
                                    let chosen = match val.as_str() {
                                        "discord" => Some(ActivityPlatform::Discord),
                                        "telegram" => Some(ActivityPlatform::Telegram),
                                        "whatsapp" => Some(ActivityPlatform::WhatsApp),
                                        "messenger" => Some(ActivityPlatform::Messenger),
                                        _ => None,
                                    };
                                    platform_choice.set(chosen);
                                }
                                class="mt-1 w-full rounded-lg border border-white/10 bg-slate-900 px-2.5 py-1.5 text-xs text-white"
                            >
                                <option value="auto">"Auto-detect"</option>
                                <option value="discord">"Discord (CSV / JSONL)"</option>
                                <option value="telegram">"Telegram (result.json)"</option>
                                <option value="whatsapp">"WhatsApp (Export .txt)"</option>
                                <option value="messenger">"Messenger (message_*.json)"</option>
                            </select>
                        </label>
                        <label class="block text-[11px] text-slate-300">
                            "Your own sender handle"
                            <input
                                type="text"
                                placeholder="Required for multi-party files"
                                prop:value=move || author_filter.get()
                                on:input=move |ev| author_filter.set(event_target_value(&ev))
                                class="mt-1 w-full rounded-lg border border-white/10 bg-slate-900 px-2.5 py-1.5 text-xs text-white placeholder:text-slate-500"
                            />
                        </label>
                        <label class="block text-[11px] text-slate-300">
                            "Local export file"
                            <input
                                type="file"
                                accept=".jsonl,.json,.csv,.txt"
                                on:change=on_file_change
                                class="mt-1 block w-full text-xs text-slate-300 file:mr-2 file:rounded-full file:border-0 file:bg-cyan-500 file:px-3 file:py-1 file:text-xs file:font-semibold file:text-slate-950 hover:file:bg-cyan-400"
                            />
                        </label>
                    </div>
                    <label class="mb-2 inline-flex items-center gap-2 text-xs text-slate-300">
                        <input
                            type="checkbox"
                            prop:checked=move || merge_mode.get()
                            on:change=move |_| merge_mode.update(|m| *m = !*m)
                            class="accent-cyan-400"
                        />
                        "Merge with existing platforms (uncheck to replace)"
                    </label>
                    <textarea
                        rows="5"
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
                            "Synthesize locally"
                        </button>
                        {move || parse_error.get().map(|err| view! {
                            <span class="text-xs text-rose-400">{err}</span>
                        })}
                        {move || import_status.get().map(|msg| view! {
                            <span class="text-xs text-emerald-300">{msg}</span>
                        })}
                    </div>
                </div>

                <div id="cooldown-plan" class="rounded-xl border border-white/10 bg-slate-950/50 p-4">
                    <h3 class="text-sm font-semibold text-white">
                        "Pre-Committed Personal Cool-Down Plan"
                    </h3>
                    <p class="mt-1 mb-3 text-xs text-slate-400">
                        "Written while calm and saved only in your browser's local storage. Check off each pause step before messaging, calling, or publishing during high activation."
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
fn DayHourActogram(days: Vec<DayActogramRow>) -> impl IntoView {
    let row_h = 18.0_f64;
    let pad_l = 74.0_f64;
    let pad_r = 12.0_f64;
    let pad_t = 18.0_f64;
    let pad_b = 10.0_f64;
    let width = 720.0_f64;
    let row_count = days.len().max(1);
    let height = pad_t + row_count as f64 * row_h + pad_b;
    let plot_w = width - pad_l - pad_r;
    let cell_w = plot_w / 24.0;
    let max_cell = days
        .iter()
        .flat_map(|d| d.hourly.iter().copied())
        .max()
        .unwrap_or(1)
        .max(1) as f64;

    view! {
        <svg
            viewBox=format!("0 0 {width} {height}")
            class="w-full"
            role="img"
            aria-label="Multi-day circadian actogram heatmap highlighting 00:00 to 05:00"
        >
            // 00:00 - 05:00 shaded sleep-risk columns
            <rect
                x=pad_l
                y=pad_t
                width=cell_w * 5.0
                height=row_count as f64 * row_h
                fill="rgba(251, 113, 133, 0.08)"
            />
            // Column hour headers
            <text x=pad_l y="11" fill="#fb7185" font-size="9">"00h"</text>
            <text x=pad_l + cell_w * 5.0 y="11" fill="#fb7185" font-size="9">"05h"</text>
            <text x=pad_l + cell_w * 12.0 y="11" fill="#94a3b8" font-size="9">"12h"</text>
            <text x=pad_l + cell_w * 18.0 y="11" fill="#94a3b8" font-size="9">"18h"</text>
            <text x=width - pad_r y="11" text-anchor="end" fill="#94a3b8" font-size="9">"23h"</text>

            {days.into_iter().enumerate().map(|(row_idx, day)| {
                let y = pad_t + row_idx as f64 * row_h;
                view! {
                    <g>
                        <text x="4" y=y + 12.0 fill="#94a3b8" font-size="9" font-family="monospace">
                            {day.date}
                        </text>
                        {day.hourly.into_iter().enumerate().map(|(hour, count)| {
                            let x = pad_l + hour as f64 * cell_w + 1.5;
                            let w = (cell_w - 3.0).max(3.0);
                            let h = row_h - 3.0;
                            let fill = if count == 0 {
                                "rgba(148, 163, 184, 0.07)".to_string()
                            } else {
                                let alpha = 0.25 + 0.75 * (count as f64 / max_cell).min(1.0);
                                if hour < 5 {
                                    format!("rgba(251, 113, 133, {alpha:.2})")
                                } else {
                                    format!("rgba(34, 211, 238, {alpha:.2})")
                                }
                            };
                            view! {
                                <rect x=x y=y + 1.5 width=w height=h rx="2" fill=fill />
                            }
                        }).collect_view()}
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
    fn parses_5day_sample_and_computes_actogram_and_baseline_ratio() {
        let events = parse_self_activity_log(SAMPLE_SELF_ACTIVITY_CSV).expect("sample parses");
        assert_eq!(events.len(), 26);

        let summary = summarize_episode_cadence(&events, Some(7), 4.5);
        assert_eq!(summary.actogram_days.len(), 5);
        assert_eq!(summary.late_night_events, 8);
        assert_eq!(summary.late_night_calls, 2);
        assert_eq!(summary.rapid_burst_clusters, 2);
        assert!(summary.baseline.baseline_events >= 10);
        assert!(summary.baseline.late_night_ratio.is_some_and(|r| r >= 2.0));
        assert_eq!(summary.activation, ActivationLevel::Elevated);

        let stripped = events_to_stripped_csv(&events);
        assert!(stripped.starts_with("timestamp,event_type,platform\n"));
        assert_eq!(parse_self_activity_log(&stripped).unwrap().len(), 26);
    }

    #[test]
    fn parses_multi_user_jsonl_with_author_filter_and_drops_all_text() {
        let jsonl = r#"{"message":{"author":{"global_name":"Luluco Trigger","username":"luluco"},"timestamp":"2026-11-22T02:31:00.000Z","type":3,"content":"secret call text"}}
{"message":{"author":{"global_name":"OtherMember","username":"other"},"timestamp":"2026-11-22T02:32:00.000Z","type":0,"content":"other person private text"}}
{"message":{"author":{"global_name":"Luluco Trigger","username":"luluco"},"timestamp":"2026-11-22T02:33:00.000Z","type":0,"content":"my private text"}}"#;

        let events =
            parse_self_activity_log_with_filter(jsonl, Some("luluco")).expect("jsonl parses");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].kind, ActivityEventKind::CallAttempt);
        assert_eq!(events[1].kind, ActivityEventKind::Message);

        let stripped = events_to_stripped_csv(&events);
        assert!(!stripped.contains("OtherMember"));
        assert!(!stripped.contains("secret call text"));
        assert!(!stripped.contains("my private text"));

        let summary = summarize_episode_cadence(&events, Some(4), 7.5);
        let csv = export_episode_summary_csv(&summary, Some(4), 7.5);
        assert!(!csv.contains("Luluco"));
        assert!(!csv.contains("OtherMember"));
    }

    #[test]
    fn renders_top_banner_and_actogram_section_on_ssr() {
        let events = RwSignal::new(load_synthetic_4platform_synthesis());
        let consent = RwSignal::new(PlatformConsentState::default());
        let asrm = RwSignal::new(Some(7u8));
        let sleep = RwSignal::new(5.0f32);

        let banner_html = view! {
            <TopCoolDownBanner events=events.into() asrm_score=asrm.into() sleep_hours=sleep.into() />
        }
        .to_html();
        assert!(banner_html.contains("Open Cool-Down Plan"));
        assert!(banner_html.contains("not a medical diagnosis"));

        let html = view! {
            <EpisodeActivationSection
                events=events
                consent=consent
                asrm_score=asrm
                sleep_hours=sleep.into()
            />
        }
        .to_html();
        assert!(html.contains("Multi-Day Circadian Actogram"));
        assert!(html.contains("Per-Platform Opt-In Consent"));
        assert!(html.contains("Discord"));
        assert!(html.contains("Telegram"));
        assert!(html.contains("WhatsApp"));
        assert!(html.contains("Messenger"));
        assert!(html.contains("Wipe local data"));
        assert!(html.contains("cooldown-plan"));
    }
}
