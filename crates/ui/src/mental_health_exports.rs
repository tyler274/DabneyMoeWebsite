//! Local-first, opt-in multi-platform personal export parsers and cross-platform
//! self-activity synthesizer for Discord, Telegram, WhatsApp, and Facebook Messenger.
//!
//! Privacy & consent invariants (enforced at parse time in browser WASM):
//! - Never connects to live chat APIs, bots, or user accounts; accepts only user-supplied
//!   official personal export files dropped locally into the browser.
//! - Multi-party exports (Discord `.jsonl`, Telegram `result.json`, WhatsApp `.txt`,
//!   Messenger `message_*.json`) fail closed unless the user supplies their own sender
//!   handle/name (`self_handle`).
//! - Every other participant's rows and 100% of message text, attachments, IDs, and
//!   usernames are discarded immediately during parsing; only `(timestamp, kind, platform)`
//!   survives locally.

use serde::{Deserialize, Serialize};

use crate::mental_health_episodes::{
    parse_timestamp_components, ActivityEventKind, SelfActivityEvent,
};

/// Messaging/publishing source for a privacy-stripped self-activity timestamp.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ActivityPlatform {
    Discord,
    Telegram,
    WhatsApp,
    Messenger,
    #[default]
    ManualOrGeneric,
}

impl ActivityPlatform {
    /// The four explicit opt-in messaging platforms supported by the synthesizer.
    pub const ALL_OPT_IN: [ActivityPlatform; 4] = [
        Self::Discord,
        Self::Telegram,
        Self::WhatsApp,
        Self::Messenger,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::Discord => "discord",
            Self::Telegram => "telegram",
            Self::WhatsApp => "whatsapp",
            Self::Messenger => "messenger",
            Self::ManualOrGeneric => "generic",
        }
    }

    pub fn from_id(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "discord" => Self::Discord,
            "telegram" => Self::Telegram,
            "whatsapp" | "wa" => Self::WhatsApp,
            "messenger" | "facebook" | "meta" => Self::Messenger,
            _ => Self::ManualOrGeneric,
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Discord => "Discord",
            Self::Telegram => "Telegram",
            Self::WhatsApp => "WhatsApp",
            Self::Messenger => "Messenger",
            Self::ManualOrGeneric => "Manual / CSV",
        }
    }

    pub fn badge_class(self) -> &'static str {
        match self {
            Self::Discord => "border-indigo-400/30 bg-indigo-500/15 text-indigo-200",
            Self::Telegram => "border-sky-400/30 bg-sky-500/15 text-sky-200",
            Self::WhatsApp => "border-emerald-400/30 bg-emerald-500/15 text-emerald-200",
            Self::Messenger => "border-fuchsia-400/30 bg-fuchsia-500/15 text-fuchsia-200",
            Self::ManualOrGeneric => "border-slate-400/30 bg-slate-500/15 text-slate-200",
        }
    }

    pub fn bar_class(self) -> &'static str {
        match self {
            Self::Discord => "bg-indigo-400",
            Self::Telegram => "bg-sky-400",
            Self::WhatsApp => "bg-emerald-400",
            Self::Messenger => "bg-fuchsia-400",
            Self::ManualOrGeneric => "bg-slate-400",
        }
    }

    pub fn export_hint(self) -> &'static str {
        match self {
            Self::Discord => "Discord Data Package `messages.csv` or `.jsonl`",
            Self::Telegram => "Telegram Desktop JSON export (`result.json`)",
            Self::WhatsApp => "WhatsApp Export Chat (`.txt` — iOS or Android)",
            Self::Messenger => "Meta Download Your Information (`message_*.json`)",
            Self::ManualOrGeneric => "Stripped `timestamp,event_type[,platform]` CSV",
        }
    }
}

/// Explicit per-platform opt-in consent state persisted only in browser `localStorage`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlatformConsentState {
    pub discord: bool,
    pub telegram: bool,
    pub whatsapp: bool,
    pub messenger: bool,
    /// Explicit user confirmation that imported files are their own personal exports
    /// and should retain only their own stripped timestamps locally.
    pub self_only_confirmed: bool,
}

impl Default for PlatformConsentState {
    fn default() -> Self {
        Self {
            discord: true,
            telegram: true,
            whatsapp: true,
            messenger: true,
            self_only_confirmed: false,
        }
    }
}

impl PlatformConsentState {
    pub fn is_opted_in(&self, platform: ActivityPlatform) -> bool {
        match platform {
            ActivityPlatform::Discord => self.discord,
            ActivityPlatform::Telegram => self.telegram,
            ActivityPlatform::WhatsApp => self.whatsapp,
            ActivityPlatform::Messenger => self.messenger,
            ActivityPlatform::ManualOrGeneric => true,
        }
    }

    pub fn toggle(&mut self, platform: ActivityPlatform) {
        match platform {
            ActivityPlatform::Discord => self.discord = !self.discord,
            ActivityPlatform::Telegram => self.telegram = !self.telegram,
            ActivityPlatform::WhatsApp => self.whatsapp = !self.whatsapp,
            ActivityPlatform::Messenger => self.messenger = !self.messenger,
            ActivityPlatform::ManualOrGeneric => {}
        }
    }

    pub fn set_opt_in(&mut self, platform: ActivityPlatform, enabled: bool) {
        match platform {
            ActivityPlatform::Discord => self.discord = enabled,
            ActivityPlatform::Telegram => self.telegram = enabled,
            ActivityPlatform::WhatsApp => self.whatsapp = enabled,
            ActivityPlatform::Messenger => self.messenger = enabled,
            ActivityPlatform::ManualOrGeneric => {}
        }
    }

    pub fn opted_in_count(&self) -> usize {
        ActivityPlatform::ALL_OPT_IN
            .iter()
            .filter(|p| self.is_opted_in(**p))
            .count()
    }
}

/// Per-platform aggregate summary inside the synthesized cross-platform timeline.
#[derive(Clone, Debug, PartialEq)]
pub struct PlatformBreakdown {
    pub platform: ActivityPlatform,
    pub total_events: usize,
    pub recent_48h_events: usize,
    pub late_night_events: usize,
    pub share_pct: f64,
}

/// Result of parsing a local personal export file with privacy stripping applied.
#[derive(Clone, Debug, PartialEq)]
pub struct ParsedPlatformExport {
    pub platform: ActivityPlatform,
    pub events: Vec<SelfActivityEvent>,
    pub dropped_other_participants: usize,
}

/// Synthesize stripped self-activity events strictly from currently opted-in platforms,
/// sorted chronologically and deduplicated on `(absolute_minutes, kind, platform)`.
pub fn synthesize_opted_in_events(
    all_events: &[SelfActivityEvent],
    consent: &PlatformConsentState,
) -> Vec<SelfActivityEvent> {
    let mut active: Vec<SelfActivityEvent> = all_events
        .iter()
        .filter(|ev| consent.is_opted_in(ev.platform))
        .cloned()
        .collect();
    dedup_and_sort_events(&mut active);
    active
}

/// Merge newly parsed events for a platform into an existing collection without duplicating
/// identical `(absolute_minutes, kind, platform)` entries.
pub fn merge_synthesized_events(
    existing: &[SelfActivityEvent],
    incoming: &[SelfActivityEvent],
) -> Vec<SelfActivityEvent> {
    let mut combined = Vec::with_capacity(existing.len() + incoming.len());
    combined.extend_from_slice(existing);
    combined.extend_from_slice(incoming);
    dedup_and_sort_events(&mut combined);
    combined
}

/// Remove all events belonging to a revoked platform.
pub fn purge_platform_events(
    existing: &[SelfActivityEvent],
    platform: ActivityPlatform,
) -> Vec<SelfActivityEvent> {
    existing
        .iter()
        .filter(|ev| ev.platform != platform)
        .cloned()
        .collect()
}

fn dedup_and_sort_events(events: &mut Vec<SelfActivityEvent>) {
    events.sort_by(|a, b| {
        a.absolute_minutes
            .cmp(&b.absolute_minutes)
            .then_with(|| a.platform.cmp(&b.platform))
            .then_with(|| a.kind.label().cmp(b.kind.label()))
    });
    events.dedup_by(|a, b| {
        a.absolute_minutes == b.absolute_minutes && a.platform == b.platform && a.kind == b.kind
    });
}

/// Auto-detect which official personal export format a local file or pasted snippet uses.
pub fn detect_export_platform(input: &str) -> ActivityPlatform {
    let trimmed = input.trim_start_matches('\u{feff}').trim();
    if trimmed.is_empty() {
        return ActivityPlatform::ManualOrGeneric;
    }

    // Whole-document JSON (Telegram `result.json` or Meta Messenger `message_*.json`).
    if trimmed.starts_with('{') && serde_json::from_str::<serde_json::Value>(trimmed).is_ok() {
        if trimmed.contains("\"timestamp_ms\"") || trimmed.contains("\"sender_name\"") {
            return ActivityPlatform::Messenger;
        }
        if trimmed.contains("\"date_unixtime\"")
            || trimmed.contains("\"from_id\"")
            || trimmed.contains("\"personal_chat\"")
            || (trimmed.contains("\"messages\"") && trimmed.contains("\"from\""))
        {
            return ActivityPlatform::Telegram;
        }
        if trimmed.contains("\"author\"") {
            return ActivityPlatform::Discord;
        }
    }

    let first_non_empty = trimmed
        .lines()
        .map(|l| strip_whatsapp_bidi(l).trim())
        .find(|l| !l.is_empty() && !l.starts_with('#'))
        .unwrap_or("");
    let first_lower = first_non_empty.to_ascii_lowercase();

    if first_non_empty.starts_with('{') {
        return ActivityPlatform::Discord;
    }
    if first_lower.starts_with("id,timestamp") {
        return ActivityPlatform::Discord;
    }
    if parse_whatsapp_line_header(first_non_empty).is_some() {
        return ActivityPlatform::WhatsApp;
    }
    ActivityPlatform::ManualOrGeneric
}

/// Parse a local export from Discord, Telegram, WhatsApp, Messenger, or stripped CSV,
/// enforcing fail-closed self-handle filtering on multi-party exports and discarding
/// 100% of message bodies and other participants immediately.
pub fn parse_platform_export(
    input: &str,
    platform_override: Option<ActivityPlatform>,
    self_handle: Option<&str>,
) -> Result<ParsedPlatformExport, String> {
    let detected = detect_export_platform(input);
    let platform = platform_override.unwrap_or(detected);
    let needle = self_handle
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_ascii_lowercase);

    match platform {
        ActivityPlatform::Telegram => parse_telegram_json(input, needle.as_deref()),
        ActivityPlatform::Messenger => parse_messenger_json(input, needle.as_deref()),
        ActivityPlatform::WhatsApp => parse_whatsapp_txt(input, needle.as_deref()),
        ActivityPlatform::Discord => parse_discord_export(input, needle.as_deref()),
        ActivityPlatform::ManualOrGeneric => {
            if detected != ActivityPlatform::ManualOrGeneric {
                return parse_platform_export(input, Some(detected), self_handle);
            }
            let events = crate::mental_health_episodes::parse_self_activity_log_with_filter(
                input,
                self_handle,
            )?;
            Ok(ParsedPlatformExport {
                platform: ActivityPlatform::ManualOrGeneric,
                events,
                dropped_other_participants: 0,
            })
        }
    }
}

fn require_self_handle(platform: ActivityPlatform, needle: Option<&str>) -> Result<&str, String> {
    needle.ok_or_else(|| {
        format!(
            "{} exports include multiple participants. Enter your own sender name/handle first so only your own timestamps are kept and all other people are discarded.",
            platform.display_name()
        )
    })
}

fn parse_discord_export(input: &str, needle: Option<&str>) -> Result<ParsedPlatformExport, String> {
    let trimmed = input.trim_start_matches('\u{feff}').trim();
    let is_jsonl = trimmed
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with('#'))
        .is_some_and(|l| l.starts_with('{'));

    if is_jsonl {
        let required = require_self_handle(ActivityPlatform::Discord, needle)?;
        let mut events = Vec::new();
        let mut dropped = 0usize;
        for (idx, raw_line) in trimmed.lines().enumerate() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let val: serde_json::Value = serde_json::from_str(line)
                .map_err(|err| format!("Discord JSONL line {}: {err}", idx + 1))?;
            let msg = val.get("message").unwrap_or(&val);
            let matched = msg
                .get("author")
                .map(|a| {
                    ["global_name", "username", "display_name", "nick"]
                        .iter()
                        .filter_map(|k| a.get(*k).and_then(serde_json::Value::as_str))
                        .any(|s| s.to_ascii_lowercase().contains(required))
                })
                .unwrap_or(false);
            if !matched {
                dropped += 1;
                continue;
            }
            let Some(ts_raw) = msg.get("timestamp").and_then(serde_json::Value::as_str) else {
                continue;
            };
            let Some(ev) = build_event_from_iso(
                ts_raw,
                if msg.get("type").and_then(serde_json::Value::as_u64) == Some(3) {
                    ActivityEventKind::CallAttempt
                } else {
                    ActivityEventKind::Message
                },
                ActivityPlatform::Discord,
            ) else {
                continue;
            };
            events.push(ev);
        }
        if events.is_empty() {
            return Err(format!(
                "No Discord JSONL messages matched your handle `{required}` ({dropped} other-participant rows discarded)."
            ));
        }
        dedup_and_sort_events(&mut events);
        return Ok(ParsedPlatformExport {
            platform: ActivityPlatform::Discord,
            events,
            dropped_other_participants: dropped,
        });
    }

    // Discord Official Data Package `messages.csv` contains only the user's own sent messages.
    let mut events =
        crate::mental_health_episodes::parse_self_activity_log_with_filter(input, None)?;
    for ev in &mut events {
        ev.platform = ActivityPlatform::Discord;
    }
    dedup_and_sort_events(&mut events);
    Ok(ParsedPlatformExport {
        platform: ActivityPlatform::Discord,
        events,
        dropped_other_participants: 0,
    })
}

fn parse_telegram_json(input: &str, needle: Option<&str>) -> Result<ParsedPlatformExport, String> {
    let required = require_self_handle(ActivityPlatform::Telegram, needle)?;
    let cleaned = input.trim_start_matches('\u{feff}').trim();
    let root: serde_json::Value = serde_json::from_str(cleaned)
        .map_err(|err| format!("Invalid Telegram Desktop JSON export: {err}"))?;

    let messages = root
        .get("messages")
        .and_then(serde_json::Value::as_array)
        .or_else(|| root.as_array())
        .ok_or_else(|| {
            "Telegram JSON must contain a `messages` array (from Telegram Desktop `result.json`)."
                .to_string()
        })?;

    let mut events = Vec::new();
    let mut dropped = 0usize;

    for msg in messages {
        let msg_type = msg
            .get("type")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("message");
        let action = msg.get("action").and_then(serde_json::Value::as_str);

        let kind = if msg_type == "message" {
            ActivityEventKind::Message
        } else if msg_type == "service" && action == Some("phone_call") {
            ActivityEventKind::CallAttempt
        } else {
            continue;
        };

        let matched = ["from", "from_id", "actor", "actor_id"]
            .iter()
            .filter_map(|k| msg.get(*k).and_then(serde_json::Value::as_str))
            .any(|s| s.to_ascii_lowercase().contains(required));
        if !matched {
            dropped += 1;
            continue;
        }

        let maybe_event = msg
            .get("date")
            .and_then(serde_json::Value::as_str)
            .and_then(|ts| build_event_from_iso(ts, kind, ActivityPlatform::Telegram))
            .or_else(|| {
                let unix_secs = msg.get("date_unixtime").and_then(|v| {
                    v.as_i64()
                        .or_else(|| v.as_str().and_then(|s| s.parse::<i64>().ok()))
                })?;
                build_event_from_unix_secs(unix_secs, kind, ActivityPlatform::Telegram)
            });

        if let Some(ev) = maybe_event {
            events.push(ev);
        }
    }

    if events.is_empty() {
        return Err(format!(
            "No Telegram messages matched your handle `{required}` ({dropped} other-participant rows discarded)."
        ));
    }

    dedup_and_sort_events(&mut events);
    Ok(ParsedPlatformExport {
        platform: ActivityPlatform::Telegram,
        events,
        dropped_other_participants: dropped,
    })
}

fn parse_messenger_json(input: &str, needle: Option<&str>) -> Result<ParsedPlatformExport, String> {
    let required = require_self_handle(ActivityPlatform::Messenger, needle)?;
    let cleaned = input.trim_start_matches('\u{feff}').trim();
    let root: serde_json::Value = serde_json::from_str(cleaned)
        .map_err(|err| format!("Invalid Facebook Messenger JSON export: {err}"))?;

    let messages = root
        .get("messages")
        .and_then(serde_json::Value::as_array)
        .or_else(|| root.as_array())
        .ok_or_else(|| {
            "Messenger JSON must contain a `messages` array (from Meta `message_*.json`)."
                .to_string()
        })?;

    let mut events = Vec::new();
    let mut dropped = 0usize;

    for msg in messages {
        let sender = msg
            .get("sender_name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        if !sender.to_ascii_lowercase().contains(required) {
            dropped += 1;
            continue;
        }

        let Some(ts_ms) = msg.get("timestamp_ms").and_then(|v| {
            v.as_i64()
                .or_else(|| v.as_str().and_then(|s| s.parse::<i64>().ok()))
        }) else {
            continue;
        };

        let is_call = msg.get("call_duration").is_some()
            || msg
                .get("type")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|t| t.eq_ignore_ascii_case("call"));
        let kind = if is_call {
            ActivityEventKind::CallAttempt
        } else {
            ActivityEventKind::Message
        };

        if let Some(ev) =
            build_event_from_unix_secs(ts_ms.div_euclid(1000), kind, ActivityPlatform::Messenger)
        {
            events.push(ev);
        }
    }

    if events.is_empty() {
        return Err(format!(
            "No Messenger messages matched your handle `{required}` ({dropped} other-participant rows discarded)."
        ));
    }

    dedup_and_sort_events(&mut events);
    Ok(ParsedPlatformExport {
        platform: ActivityPlatform::Messenger,
        events,
        dropped_other_participants: dropped,
    })
}

fn parse_whatsapp_txt(input: &str, needle: Option<&str>) -> Result<ParsedPlatformExport, String> {
    let required = require_self_handle(ActivityPlatform::WhatsApp, needle)?;
    let mut events = Vec::new();
    let mut dropped = 0usize;

    for raw_line in input.lines() {
        let cleaned = strip_whatsapp_bidi(raw_line).trim();
        let Some((ts_str, sender, body)) = parse_whatsapp_line_header(cleaned) else {
            continue;
        };
        if !sender.to_ascii_lowercase().contains(required) {
            dropped += 1;
            continue;
        }
        let body_lower = body.to_ascii_lowercase();
        let kind = if body_lower.contains("voice call")
            || body_lower.contains("video call")
            || body_lower.contains("missed call")
        {
            ActivityEventKind::CallAttempt
        } else {
            ActivityEventKind::Message
        };
        if let Some((date_key, formatted_ts, abs_min, hour, minute_of_day)) =
            parse_timestamp_components(&ts_str)
        {
            events.push(SelfActivityEvent {
                date_key,
                formatted_ts,
                hour,
                minute_of_day,
                absolute_minutes: abs_min,
                kind,
                platform: ActivityPlatform::WhatsApp,
            });
        }
    }

    if events.is_empty() {
        return Err(format!(
            "No WhatsApp messages matched your handle `{required}` ({dropped} other-participant rows discarded). Export using WhatsApp 'Export Chat' (.txt)."
        ));
    }

    dedup_and_sort_events(&mut events);
    Ok(ParsedPlatformExport {
        platform: ActivityPlatform::WhatsApp,
        events,
        dropped_other_participants: dropped,
    })
}

fn strip_whatsapp_bidi(line: &str) -> &str {
    line.trim_start_matches(['\u{feff}', '\u{200e}', '\u{200f}', '\u{202a}', '\u{202c}'])
}

/// Parse either iOS `[date, time] Sender: body` or Android `date, time - Sender: body`.
/// Returns `(normalized_yyyy_mm_dd_hh_mm, sender, body_for_call_check_only)`.
fn parse_whatsapp_line_header(line: &str) -> Option<(String, &str, &str)> {
    let (raw_dt, after_dt) = if let Some(rest) = line.strip_prefix('[') {
        let close = rest.find(']')?;
        (&rest[..close], rest[close + 1..].trim_start())
    } else if let Some(idx) = line.find(" - ") {
        (&line[..idx], &line[idx + 3..])
    } else if let Some(idx) = line.find(" – ") {
        (&line[..idx], &line[idx + " – ".len()..])
    } else {
        return None;
    };

    let colon = after_dt.find(": ")?;
    let sender = after_dt[..colon].trim();
    let body = &after_dt[colon + 2..];
    if sender.is_empty() {
        return None;
    }

    let normalized_ts = parse_whatsapp_datetime(raw_dt)?;
    Some((normalized_ts, sender, body))
}

fn parse_whatsapp_datetime(raw: &str) -> Option<String> {
    let cleaned = raw.replace(['\u{202f}', '\u{00a0}'], " ");
    let (date_part, time_part) = cleaned
        .split_once(',')
        .or_else(|| cleaned.split_once(' '))?;
    let date_part = date_part.trim();
    let time_part = time_part.trim();

    let (year, month, day) = if date_part.contains('-') {
        let mut parts = date_part.split('-');
        let a: i64 = parts.next()?.parse().ok()?;
        let b: u32 = parts.next()?.parse().ok()?;
        let c: u32 = parts.next()?.parse().ok()?;
        if a >= 1900 {
            (a, b, c)
        } else {
            let y = if c < 100 {
                2000 + i64::from(c)
            } else {
                i64::from(c)
            };
            (y, a as u32, b)
        }
    } else if date_part.contains('/') {
        let mut parts = date_part.split('/');
        let a: u32 = parts.next()?.parse().ok()?;
        let b: u32 = parts.next()?.parse().ok()?;
        let c: i64 = parts.next()?.parse().ok()?;
        let year = if c < 100 { 2000 + c } else { c };
        // Support both US M/D/YY and DD/MM/YY when day > 12.
        let (month, day) = if a > 12 && b <= 12 { (b, a) } else { (a, b) };
        (year, month, day)
    } else {
        return None;
    };

    let upper_time = time_part.to_ascii_uppercase();
    let is_pm = upper_time.ends_with("PM");
    let is_am = upper_time.ends_with("AM");
    let digits_only = upper_time
        .trim_end_matches("AM")
        .trim_end_matches("PM")
        .trim();
    let mut tparts = digits_only.split(':');
    let mut hour: u32 = tparts.next()?.parse().ok()?;
    let minute: u32 = tparts.next()?.parse().ok()?;
    if is_pm && hour < 12 {
        hour += 12;
    } else if is_am && hour == 12 {
        hour = 0;
    }

    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour >= 24 || minute >= 60 {
        return None;
    }
    Some(format!(
        "{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}"
    ))
}

fn build_event_from_iso(
    ts_raw: &str,
    kind: ActivityEventKind,
    platform: ActivityPlatform,
) -> Option<SelfActivityEvent> {
    let normalized = ts_raw.trim();
    if normalized.len() < 16 {
        return None;
    }
    let slice = &normalized[..16];
    let (date_key, formatted_ts, abs_min, hour, minute_of_day) = parse_timestamp_components(slice)?;
    Some(SelfActivityEvent {
        date_key,
        formatted_ts,
        hour,
        minute_of_day,
        absolute_minutes: abs_min,
        kind,
        platform,
    })
}

fn build_event_from_unix_secs(
    unix_secs: i64,
    kind: ActivityEventKind,
    platform: ActivityPlatform,
) -> Option<SelfActivityEvent> {
    let (year, month, day, hour, minute) = unix_secs_to_utc_ymd_hm(unix_secs)?;
    let ts = format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}");
    let (date_key, formatted_ts, abs_min, h, minute_of_day) = parse_timestamp_components(&ts)?;
    Some(SelfActivityEvent {
        date_key,
        formatted_ts,
        hour: h,
        minute_of_day,
        absolute_minutes: abs_min,
        kind,
        platform,
    })
}

/// Exact civil calendar conversion from Unix epoch seconds (UTC) via Howard Hinnant's algorithm.
fn unix_secs_to_utc_ymd_hm(unix_secs: i64) -> Option<(i64, u32, u32, u32, u32)> {
    if !(0..=4_102_444_800).contains(&unix_secs) {
        return None;
    }
    let days = unix_secs.div_euclid(86_400);
    let secs_of_day = unix_secs.rem_euclid(86_400) as u32;
    let hour = secs_of_day / 3600;
    let minute = (secs_of_day % 3600) / 60;

    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = y + i64::from(m <= 2);
    Some((year, m as u32, d as u32, hour, minute))
}

/// 100% synthetic Discord Data Package sample (`messages.csv` format, fictional demo).
pub const SAMPLE_DISCORD_EXPORT_CSV: &str = "\
ID,Timestamp,Contents,Attachments
9001,2026-11-18 09:15:00,Synthetic daytime check-in,
9002,2026-11-18 14:20:00,Synthetic afternoon note,
9003,2026-11-19 10:05:00,Synthetic daytime update,
9004,2026-11-20 11:30:00,Synthetic daytime sync,
9005,2026-11-21 15:10:00,Synthetic afternoon message,
9006,2026-11-22 01:40:00,Synthetic late-night burst 1,
9007,2026-11-22 01:43:00,Synthetic late-night burst 2,
9008,2026-11-22 01:46:00,Synthetic late-night burst 3,
9009,2026-11-22 01:49:00,Synthetic late-night burst 4,
9010,2026-11-22 02:18:00,Synthetic late-night call,";

/// 100% synthetic Telegram Desktop `result.json` sample (fictional `Alex Demo` vs `Other Person`).
pub const SAMPLE_TELEGRAM_EXPORT_JSON: &str = r#"{
  "name": "Synthetic Demo Chat",
  "type": "personal_chat",
  "messages": [
    {"id": 1, "type": "message", "date": "2026-11-18T12:10:00", "from": "Alex Demo", "text": "private telegram text discarded"},
    {"id": 2, "type": "message", "date": "2026-11-18T12:12:00", "from": "Other Person", "text": "other person text discarded"},
    {"id": 3, "type": "message", "date": "2026-11-19T18:30:00", "from": "Alex Demo", "text": "synthetic daytime"},
    {"id": 4, "type": "message", "date": "2026-11-20T16:45:00", "from": "Alex Demo", "text": "synthetic daytime"},
    {"id": 5, "type": "message", "date": "2026-11-22T02:05:00", "from": "Alex Demo", "text": "synthetic late-night"},
    {"id": 6, "type": "service", "action": "phone_call", "date": "2026-11-22T02:20:00", "actor": "Alex Demo"}
  ]
}"#;

/// 100% synthetic WhatsApp Export Chat `.txt` sample (fictional `Alex Demo` vs `Other Person`).
pub const SAMPLE_WHATSAPP_EXPORT_TXT: &str = "\
[11/18/26, 8:45:10 AM] Alex Demo: Synthetic morning check-in
[11/18/26, 8:47:00 AM] Other Person: Other person text must never survive
[11/19/26, 1:15:00 PM] Alex Demo: Synthetic lunch note
[11/20/26, 9:20:00 PM] Alex Demo: Synthetic evening check-in
[11/22/26, 2:10:00 AM] Alex Demo: Synthetic late-night message
[11/22/26, 2:24:00 AM] Alex Demo: Voice call";

/// 100% synthetic Meta Messenger `message_1.json` sample (fictional `Alex Demo` vs `Other Person`).
/// Timestamps correspond to UTC 2026-11-18 19:00, 2026-11-19 20:15, 2026-11-21 18:30, 2026-11-22 02:32.
pub const SAMPLE_MESSENGER_EXPORT_JSON: &str = r#"{
  "participants": [{"name": "Alex Demo"}, {"name": "Other Person"}],
  "messages": [
    {"sender_name": "Alex Demo", "timestamp_ms": 1795028400000, "content": "private messenger text discarded"},
    {"sender_name": "Other Person", "timestamp_ms": 1795028700000, "content": "other person messenger text discarded"},
    {"sender_name": "Alex Demo", "timestamp_ms": 1795119300000, "content": "synthetic evening"},
    {"sender_name": "Alex Demo", "timestamp_ms": 1795285800000, "content": "synthetic evening"},
    {"sender_name": "Alex Demo", "timestamp_ms": 1795314720000, "content": "synthetic late-night", "call_duration": 42}
  ]
}"#;

/// Build the 100% synthetic 4-platform demo timeline by running all four real parsers
/// end-to-end with fictional self handle `"Alex Demo"` (stripping `"Other Person"` and all text).
pub fn load_synthetic_4platform_synthesis() -> Vec<SelfActivityEvent> {
    let mut combined = Vec::new();
    for (raw, platform, handle) in [
        (
            SAMPLE_DISCORD_EXPORT_CSV,
            ActivityPlatform::Discord,
            Some("Alex Demo"),
        ),
        (
            SAMPLE_TELEGRAM_EXPORT_JSON,
            ActivityPlatform::Telegram,
            Some("Alex Demo"),
        ),
        (
            SAMPLE_WHATSAPP_EXPORT_TXT,
            ActivityPlatform::WhatsApp,
            Some("Alex Demo"),
        ),
        (
            SAMPLE_MESSENGER_EXPORT_JSON,
            ActivityPlatform::Messenger,
            Some("Alex Demo"),
        ),
    ] {
        if let Ok(parsed) = parse_platform_export(raw, Some(platform), handle) {
            combined = merge_synthesized_events(&combined, &parsed.events);
        }
    }
    combined
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mental_health_episodes::{events_to_stripped_csv, summarize_episode_cadence};

    #[test]
    fn parses_all_four_platforms_and_strips_other_participants_and_text() {
        let discord = parse_platform_export(SAMPLE_DISCORD_EXPORT_CSV, None, None)
            .expect("discord csv parses");
        assert_eq!(discord.platform, ActivityPlatform::Discord);
        assert_eq!(discord.events.len(), 10);
        assert!(discord
            .events
            .iter()
            .all(|e| e.platform == ActivityPlatform::Discord));

        let telegram = parse_platform_export(SAMPLE_TELEGRAM_EXPORT_JSON, None, Some("Alex Demo"))
            .expect("telegram json parses");
        assert_eq!(telegram.platform, ActivityPlatform::Telegram);
        assert_eq!(telegram.events.len(), 5);
        assert_eq!(telegram.dropped_other_participants, 1);
        assert!(telegram
            .events
            .iter()
            .any(|e| e.kind == ActivityEventKind::CallAttempt));

        let whatsapp = parse_platform_export(SAMPLE_WHATSAPP_EXPORT_TXT, None, Some("Alex Demo"))
            .expect("whatsapp txt parses");
        assert_eq!(whatsapp.platform, ActivityPlatform::WhatsApp);
        assert_eq!(whatsapp.events.len(), 5);
        assert_eq!(whatsapp.dropped_other_participants, 1);
        assert!(whatsapp
            .events
            .iter()
            .any(|e| e.kind == ActivityEventKind::CallAttempt));

        let messenger =
            parse_platform_export(SAMPLE_MESSENGER_EXPORT_JSON, None, Some("Alex Demo"))
                .expect("messenger json parses");
        assert_eq!(messenger.platform, ActivityPlatform::Messenger);
        assert_eq!(messenger.events.len(), 4);
        assert_eq!(messenger.dropped_other_participants, 1);
        assert!(messenger
            .events
            .iter()
            .any(|e| e.kind == ActivityEventKind::CallAttempt));
        assert_eq!(messenger.events[0].date_key, "2026-11-18");

        let synthesized = load_synthetic_4platform_synthesis();
        assert_eq!(synthesized.len(), 24);
        let stripped = events_to_stripped_csv(&synthesized);
        assert!(!stripped.contains("Alex Demo"));
        assert!(!stripped.contains("Other Person"));
        assert!(!stripped.contains("private telegram text"));
        assert!(!stripped.contains("private messenger text"));
        assert!(!stripped.contains("Other person text"));
    }

    #[test]
    fn multi_party_exports_fail_closed_without_self_handle() {
        assert!(parse_platform_export(SAMPLE_TELEGRAM_EXPORT_JSON, None, None).is_err());
        assert!(parse_platform_export(SAMPLE_WHATSAPP_EXPORT_TXT, None, Some("   ")).is_err());
        assert!(parse_platform_export(SAMPLE_MESSENGER_EXPORT_JSON, None, None).is_err());

        let discord_jsonl = r#"{"message":{"author":{"username":"alex"},"timestamp":"2026-11-22T02:00:00Z","type":0,"content":"secret"}}"#;
        assert!(parse_platform_export(discord_jsonl, None, None).is_err());
    }

    #[test]
    fn android_whatsapp_format_and_bidi_marks_parse_cleanly() {
        let android_txt = "\u{feff}\u{200e}11/22/26, 2:15 AM - Alex Demo: late check\n\
                           11/22/26, 2:16 AM - Other Person: ignored line\n\
                           continuation line without header\n\
                           22/11/2026, 14:30 - Alex Demo: afternoon check\n";
        let parsed =
            parse_platform_export(android_txt, Some(ActivityPlatform::WhatsApp), Some("alex"))
                .expect("android whatsapp parses");
        assert_eq!(parsed.events.len(), 2);
        assert_eq!(parsed.dropped_other_participants, 1);
        assert_eq!(parsed.events[0].formatted_ts, "2026-11-22 02:15");
        assert_eq!(parsed.events[1].formatted_ts, "2026-11-22 14:30");
    }

    #[test]
    fn consent_toggles_and_purge_immediately_update_synthesis() {
        let all = load_synthetic_4platform_synthesis();
        let mut consent = PlatformConsentState::default();
        let full_summary =
            summarize_episode_cadence(&synthesize_opted_in_events(&all, &consent), Some(6), 5.5);
        assert_eq!(full_summary.by_platform.len(), 4);

        // Revoke Telegram & Messenger via opt-in consent toggles.
        consent.set_opt_in(ActivityPlatform::Telegram, false);
        consent.set_opt_in(ActivityPlatform::Messenger, false);
        let filtered = synthesize_opted_in_events(&all, &consent);
        assert!(filtered.iter().all(|e| matches!(
            e.platform,
            ActivityPlatform::Discord | ActivityPlatform::WhatsApp
        )));
        let summary = summarize_episode_cadence(&filtered, Some(6), 5.5);
        assert_eq!(summary.by_platform.len(), 2);

        // Purge Discord completely.
        let purged = purge_platform_events(&all, ActivityPlatform::Discord);
        assert!(purged
            .iter()
            .all(|e| e.platform != ActivityPlatform::Discord));

        // Deduplicating repeated merge does not double-count.
        let doubled = merge_synthesized_events(&all, &all);
        assert_eq!(doubled.len(), all.len());
    }
}
