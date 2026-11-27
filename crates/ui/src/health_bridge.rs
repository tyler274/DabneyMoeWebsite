//! Shared client-side Tauri v2 IPC bridge for opt-in personal mobile capabilities
//! inside `moe.dabney.personal` (Cargo feature `health-connect`):
//!
//! 1. **Native Health Sync** (Android Google Health Connect & iOS Apple HealthKit)
//! 2. **Encrypted Local Personal Vault** (Android Keystore AES-256-GCM at-rest storage)
//! 3. **On-Device LLM Inference** (Android AICore / Gemini Nano via ML Kit GenAI Prompt,
//!    with automatic deterministic grounded Rust fallback on unsupported devices)
//!
//! On web browsers and inside the public zero-PHI Google Play demo (`moe.dabney.demo`),
//! these commands are absent or return `enabled: false`, keeping the public demo strictly
//! zero-PHI with zero health permissions, zero persistent storage, and zero ML Kit bloat.

use clinical_workspace::ChatAnswer;
use serde::{Deserialize, Serialize};

use crate::mental_health_cgm::{ClinicalGlucoseSummary, GlucoseImportDiagnostics, GlucoseMonitor};
use crate::mental_health_cgm_analytics::{
    AdvancedGlucoseAnalytics, CircadianPeriod, CircadianPeriodSummary, EpisodeKind,
    PeriodComparison,
};
use crate::mental_health_lingo::MentalHealthCheckIn;

/// Native health bridge platform reported by `health_connect_status`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HealthBridgePlatform {
    AndroidHealthConnect,
    IosHealthKit,
    Other,
}

impl HealthBridgePlatform {
    pub fn from_wire(platform: &str) -> Self {
        match platform {
            "android_health_connect" => Self::AndroidHealthConnect,
            "ios_healthkit" => Self::IosHealthKit,
            _ => Self::Other,
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::AndroidHealthConnect => "Android Health Connect",
            Self::IosHealthKit => "Apple HealthKit",
            Self::Other => "On-Device Health Bridge",
        }
    }

    pub fn permission_summary(self) -> &'static str {
        match self {
            Self::AndroidHealthConnect => {
                "Read-only blood glucose · stored encrypted on this device only"
            }
            Self::IosHealthKit => "Read-only blood glucose · stored encrypted on this device only",
            Self::Other => "Read-only blood glucose · stays on this device only",
        }
    }

    pub fn unavailable_hint(self) -> &'static str {
        match self {
            Self::AndroidHealthConnect => "Android Health Connect is not available on this device (Android 9+ with Health Connect required). You can still import Abbott Lingo CSV files below.",
            Self::IosHealthKit => "Apple HealthKit is not available on this device. You can still import an Apple Health export.xml or Abbott Lingo CSV file below.",
            Self::Other => "On-device health sync is not available on this device. You can still import CGM exports below.",
        }
    }

    pub fn empty_sync_hint(self, days: u32) -> String {
        match self {
            Self::AndroidHealthConnect => format!(
                "No Blood Glucose records found in Android Health Connect over the last {days} days. Confirm Abbott Lingo Health Connect sync is enabled."
            ),
            Self::IosHealthKit => format!(
                "No Blood Glucose samples found in Apple Health over the last {days} days. Confirm Abbott Lingo → Apple Health sync is enabled and Blood Glucose read access is granted in iOS Health Settings."
            ),
            Self::Other => format!("No Blood Glucose records found over the last {days} days."),
        }
    }
}

/// Runtime status returned by `health_connect_status` in `moe.dabney.personal`.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct HealthBridgeStatusUi {
    pub enabled: bool,
    pub platform: String,
    pub sdk_status: String,
    pub has_glucose_permission: bool,
    pub package_id: String,
}

impl HealthBridgeStatusUi {
    pub fn platform_kind(&self) -> HealthBridgePlatform {
        HealthBridgePlatform::from_wire(&self.platform)
    }
}

/// Outcome returned by `health_connect_request_permission`.
#[cfg(any(feature = "hydrate", feature = "csr", test))]
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct HealthBridgePermissionUi {
    pub granted: bool,
    #[serde(default)]
    pub granted_permissions: Vec<String>,
}

/// Canonical `dabney.health_bridge.v1` payload returned by `health_connect_read_glucose`.
#[cfg(any(feature = "hydrate", feature = "csr", test))]
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct HealthBridgeReadUi {
    pub bridge_json: String,
    pub record_count: usize,
    #[serde(default)]
    pub sources: Vec<String>,
}

/// Encrypted on-device personal vault status returned by `personal_vault_status`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersonalVaultStatusUi {
    pub enabled: bool,
    pub encrypted: bool,
    pub cipher: String,
    pub storage_backend: String,
    pub has_cgm_snapshot: bool,
    pub cgm_reading_count: usize,
    pub cgm_saved_at: Option<String>,
    pub checkin_count: usize,
    pub note_count: usize,
    pub chat_turn_count: usize,
    pub total_bytes: u64,
}

/// Decrypted snapshot restored from `personal_vault_load`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersonalVaultSnapshotUi {
    pub found: bool,
    pub cgm_bridge_json: Option<String>,
    pub cgm_source_label: Option<String>,
    pub cgm_saved_at: Option<String>,
    pub cgm_reading_count: usize,
    pub checkins_json: Option<String>,
    pub notes_json: Option<String>,
    pub chat_history_json: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub passport_json: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roi_requests_json: Option<String>,
}

/// Partial update merged into `personal_vault_save`.
#[cfg(any(feature = "hydrate", feature = "csr", test))]
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersonalVaultSaveUi {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cgm_bridge_json: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cgm_source_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cgm_reading_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checkins_json: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes_json: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat_history_json: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub passport_json: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roi_requests_json: Option<String>,
}

/// Availability classification of the native on-device LLM backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocalLlmAvailability {
    Available,
    Downloadable,
    Downloading,
    Unavailable,
}

impl LocalLlmAvailability {
    pub fn from_wire(raw: &str) -> Self {
        match raw {
            "available" => Self::Available,
            "downloadable" => Self::Downloadable,
            "downloading" => Self::Downloading,
            _ => Self::Unavailable,
        }
    }
}

/// On-device LLM hardware capability & AICore status returned by `local_llm_status`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalLlmStatusUi {
    pub enabled: bool,
    pub backend: String,
    pub model_name: String,
    pub availability: String,
    pub device_summary: String,
    pub total_ram_mb: u64,
    pub aicore_installed: bool,
    pub detail: String,
}

impl LocalLlmStatusUi {
    pub fn availability_kind(&self) -> LocalLlmAvailability {
        LocalLlmAvailability::from_wire(&self.availability)
    }
}

/// Result of `local_llm_prepare`.
#[cfg(any(feature = "hydrate", feature = "csr", test))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalLlmPrepareUi {
    pub ready: bool,
    pub status: String,
    pub detail: String,
}

/// Output of `local_llm_generate`.
#[cfg(any(feature = "hydrate", feature = "csr", test))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalLlmGenerateUi {
    pub text: String,
    pub backend: String,
    pub model_name: String,
    pub latency_ms: u64,
}

/// One executed local tool step attached to a [`LocalChatTurn`] so the user can
/// inspect exactly which local computations grounded a CGM & mood answer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalToolTraceStep {
    pub tool_name: String,
    pub label: String,
    pub status: String,
    pub summary: String,
}

/// A single local chat exchange persisted inside the encrypted personal vault.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalChatTurn {
    pub id: String,
    pub question: String,
    pub answer: String,
    pub engine_label: String,
    pub latency_ms: Option<u64>,
    pub grounding_facts: Vec<String>,
    /// Ordered trace of local Rust tools executed to ground this turn.
    /// Defaults to empty when restoring older encrypted vault snapshots.
    #[serde(default)]
    pub tool_steps: Vec<LocalToolTraceStep>,
}

/// Produce a deterministic, auditable tool-execution trace describing which
/// local CGM & mood computations grounded a `/mental-health` chat turn.
#[must_use]
pub fn build_cgm_agent_steps(
    question: &str,
    monitor: GlucoseMonitor,
    summary: Option<&ClinicalGlucoseSummary>,
    diagnostics: Option<&GlucoseImportDiagnostics>,
    checkins: &[MentalHealthCheckIn],
    used_aicore: bool,
) -> Vec<LocalToolTraceStep> {
    let q = question.to_ascii_lowercase();
    let mut steps = Vec::new();

    if let Some(s) = summary {
        steps.push(LocalToolTraceStep {
            tool_name: "cgm_window_metrics".to_string(),
            label: "Compute CGM window metrics".to_string(),
            status: "ok".to_string(),
            summary: format!(
                "{} · {} readings · Mean {:.0} mg/dL · GMI {:.1}% · TIR {:.0}% · CV {:.1}%",
                monitor.display_name(),
                s.reading_count,
                s.mean_mg_dl,
                s.gmi_percent,
                s.tir_70_180_pct,
                s.cv_percent,
            ),
        });

        let mut peak_hours: Vec<_> = s.agp_hourly.iter().filter(|h| h.count > 0).collect();
        peak_hours.sort_by(|a, b| b.p50.total_cmp(&a.p50));
        if let Some(top) = peak_hours.first() {
            steps.push(LocalToolTraceStep {
                tool_name: "agp_24h_peaks".to_string(),
                label: "Scan 24h AGP hourly peaks".to_string(),
                status: "ok".to_string(),
                summary: format!(
                    "Peak {:02}:00 local (median {:.0}, p95 {:.0} mg/dL) · {} spike(s) >140 mg/dL",
                    top.hour, top.p50, top.p95, s.spike_readings_gt_140
                ),
            });
        }
    } else {
        steps.push(LocalToolTraceStep {
            tool_name: "cgm_window_metrics".to_string(),
            label: "Compute CGM window metrics".to_string(),
            status: "empty".to_string(),
            summary: "No glucose readings loaded yet.".to_string(),
        });
    }

    if let Some(d) = diagnostics {
        steps.push(LocalToolTraceStep {
            tool_name: "import_validation".to_string(),
            label: "Check sensor cadence & caps".to_string(),
            status: "ok".to_string(),
            summary: format!(
                "{} parsed across {:.1}d · median {:.1}m · {} cap sentinel(s)",
                d.parsed_readings,
                d.span_hours / 24.0,
                d.median_interval_minutes.unwrap_or(0.0),
                d.bounded_sentinels,
            ),
        });
    }

    let mentions_mood = q.contains("mood")
        || q.contains("anxiety")
        || q.contains("sleep")
        || q.contains("phq")
        || q.contains("gad")
        || q.contains("mental")
        || q.contains("doctor")
        || q.contains("visit");
    if mentions_mood || !checkins.is_empty() {
        let status = if checkins.is_empty() { "empty" } else { "ok" };
        let summary_text = if checkins.is_empty() {
            "No local mood or anxiety check-ins logged yet.".to_string()
        } else {
            let avg_mood =
                checkins.iter().map(|c| f64::from(c.mood)).sum::<f64>() / checkins.len() as f64;
            let avg_anx =
                checkins.iter().map(|c| f64::from(c.anxiety)).sum::<f64>() / checkins.len() as f64;
            format!(
                "Correlated {} check-in(s) · avg mood {:.1}/10 · avg anxiety {:.1}/10",
                checkins.len(),
                avg_mood,
                avg_anx
            )
        };
        steps.push(LocalToolTraceStep {
            tool_name: "mood_checkin_lookup".to_string(),
            label: "Correlate local mood check-ins".to_string(),
            status: status.to_string(),
            summary: summary_text,
        });
    }

    steps.push(LocalToolTraceStep {
        tool_name: if used_aicore {
            "aicore_gemini_nano".to_string()
        } else {
            "rust_grounded_synth".to_string()
        },
        label: if used_aicore {
            "Synthesize on-device with Gemini Nano".to_string()
        } else {
            "Synthesize grounded answer in local Rust".to_string()
        },
        status: "ok".to_string(),
        summary: "Zero cloud egress; numbers restricted to computed local metrics.".to_string(),
    });

    steps
}

fn format_duration_minutes(minutes: f64) -> String {
    if minutes >= 120.0 {
        format!("{:.1}h ({:.0}m)", minutes / 60.0, minutes)
    } else {
        format!("{:.0}m", minutes)
    }
}

/// Format windowed biosensor saturation details strictly from `summary` (never mixing
/// full-import `diagnostics.bounded_sentinels` with windowed `summary.reading_count`).
fn windowed_saturation_detail(summary: &ClinicalGlucoseSummary) -> Option<String> {
    if !summary.has_sensor_saturation() {
        return None;
    }
    let mut parts = Vec::new();
    if summary.ceiling_capped_readings > 0 {
        parts.push(format!(
            "{} ceiling-capped >=201 mg/dL (>200 sensor cap, {:.1}%)",
            summary.ceiling_capped_readings, summary.ceiling_capped_pct
        ));
    }
    if summary.floor_capped_readings > 0 {
        parts.push(format!(
            "{} floor-capped <=54 mg/dL (<55 sensor cap, {:.1}%)",
            summary.floor_capped_readings, summary.floor_capped_pct
        ));
    }
    let impact = match (
        summary.ceiling_capped_readings > 0,
        summary.floor_capped_readings > 0,
    ) {
        (true, true) => {
            "Mean, GMI, TAR >250, and CV% (flat-line suppression at >=201) are lower bounds / clipped, and true low nadirs below floor are clipped."
        }
        (true, false) => {
            "Mean, GMI, TAR >250, and CV% (flat-line suppression at >=201) are lower bounds / clipped in this window."
        }
        (false, true) => "True low nadirs below the sensor floor are clipped in this window.",
        (false, false) => "",
    };
    Some(format!("{}; {}", parts.join(" and "), impact))
}

fn daytime_circadian_aggregate(circadian: &[CircadianPeriodSummary]) -> Option<(usize, f64, f64)> {
    let mut total_n = 0usize;
    let mut weighted_mean = 0.0_f64;
    let mut weighted_tir = 0.0_f64;
    for p in circadian {
        if p.period != CircadianPeriod::Overnight && p.reading_count > 0 {
            let n = p.reading_count as f64;
            total_n += p.reading_count;
            weighted_mean += p.mean_mg_dl * n;
            weighted_tir += p.tir_70_180_pct * n;
        }
    }
    if total_n == 0 {
        None
    } else {
        let denom = total_n as f64;
        Some((total_n, weighted_mean / denom, weighted_tir / denom))
    }
}

fn format_period_comparison_summary(cmp: &PeriodComparison) -> String {
    let gap_caveat = if cmp.prior_days < cmp.recent_days - 1.0 {
        format!(
            " (note: prior window covers only {:.1}d active span vs recent {:.1}d due to sensor gaps)",
            cmp.prior_days, cmp.recent_days
        )
    } else {
        String::new()
    };
    format!(
        "Recent {:.1}d ({} rdgs) vs prior {:.1}d ({} rdgs): Mean {:.1} vs {:.1} mg/dL ({:+.1}), \
         GMI {:.2}% vs {:.2}% ({:+.2}%), TIR 70–180 {:.1}% vs {:.1}% ({:+.1}pp), \
         Tight 70–140 {:.1}% vs {:.1}% ({:+.1}pp), TBR <70 {:.1}% vs {:.1}% ({:+.1}pp), \
         Ceiling-cap {:.1}% vs {:.1}% ({:+.1}pp){gap_caveat}",
        cmp.recent_days,
        cmp.recent_readings,
        cmp.prior_days,
        cmp.prior_readings,
        cmp.recent_mean_mg_dl,
        cmp.prior_mean_mg_dl,
        cmp.delta_mean_mg_dl,
        cmp.recent_gmi_pct,
        cmp.prior_gmi_pct,
        cmp.delta_gmi_pct,
        cmp.recent_tir_pct,
        cmp.prior_tir_pct,
        cmp.delta_tir_pct,
        cmp.recent_titr_pct,
        cmp.prior_titr_pct,
        cmp.delta_titr_pct,
        cmp.recent_tbr_pct,
        cmp.prior_tbr_pct,
        cmp.delta_tbr_pct,
        cmp.recent_ceiling_capped_pct,
        cmp.prior_ceiling_capped_pct,
        cmp.delta_ceiling_capped_pct,
    )
}

/// Build a compact, strictly factual on-device LLM prompt grounded in the user's
/// locally computed CGM metrics, 24h AGP peaks, validation warnings, and check-ins.
pub fn build_personal_cgm_chat_prompt(
    question: &str,
    monitor: GlucoseMonitor,
    summary: Option<&ClinicalGlucoseSummary>,
    diagnostics: Option<&GlucoseImportDiagnostics>,
    checkins: &[MentalHealthCheckIn],
    recent_turns: &[LocalChatTurn],
) -> String {
    build_personal_cgm_chat_prompt_with_analytics(
        question,
        monitor,
        summary,
        diagnostics,
        None,
        checkins,
        recent_turns,
    )
}

/// Build a compact, strictly factual on-device LLM prompt grounded in the user's
/// locally computed CGM metrics, windowed saturation caveats, longitudinal analytics
/// (sustained episodes, circadian periods, 14-day comparison, 15-min RoC, sessions/gaps),
/// import diagnostics, and mood check-ins.
pub fn build_personal_cgm_chat_prompt_with_analytics(
    question: &str,
    monitor: GlucoseMonitor,
    summary: Option<&ClinicalGlucoseSummary>,
    diagnostics: Option<&GlucoseImportDiagnostics>,
    analytics: Option<&AdvancedGlucoseAnalytics>,
    checkins: &[MentalHealthCheckIn],
    recent_turns: &[LocalChatTurn],
) -> String {
    let mut prompt = String::with_capacity(2800);
    prompt.push_str(
        "You are a concise, calm, local on-device metabolic & wellness reflection assistant inside Curie Mobile.\n\
         Rules:\n\
         - Use ONLY the exact numbers and facts provided below. Never invent readings, labs, or timestamps.\n\
         - Be direct, practical, and easy to read (3-6 short sentences or concise bullets).\n\
         - Never diagnose medical conditions or prescribe dosing changes; frame observations as questions to discuss with a clinician.\n\
         - If sensor saturation (<=54 / <55 floor or >=201 / >200 ceiling clipping), low wear-time, or truncated prior spans are noted, mention them honestly.\n\n",
    );

    prompt.push_str(&format!("Monitor: {}\n", monitor.display_name()));

    if let Some(s) = summary {
        prompt.push_str(&format!(
            "CGM Metrics ({} readings in selected window, {:.0}% active wear):\n\
             - Mean: {:.1} mg/dL | Median: {:.0} mg/dL | Range: {:.0}-{:.0} mg/dL\n\
             - GMI (est. A1C): {:.1}% | CV%: {:.1}% (target <36%) | SD: {:.1} mg/dL | MAGE: {:.0} mg/dL\n\
             - ADA 5-Band TIR: Very Low (<54) {:.1}%, Low (54-69) {:.1}%, In Range (70-180) {:.1}%, Tight (70-140) {:.1}%, High (181-250) {:.1}%, Very High (>250) {:.1}%\n\
             - GRI Risk Index: {:.0}/100 ({}) | Spikes >140 mg/dL: {}\n",
            s.reading_count,
            s.estimated_active_wear_pct,
            s.mean_mg_dl,
            s.median_mg_dl,
            s.min_mg_dl,
            s.max_mg_dl,
            s.gmi_percent,
            s.cv_percent,
            s.std_dev_mg_dl,
            s.mage_mg_dl,
            s.tbr_very_low_lt_54_pct,
            s.tbr_low_54_69_pct,
            s.tir_70_180_pct,
            s.titr_70_140_pct,
            s.tar_high_181_250_pct,
            s.tar_very_high_gt_250_pct,
            s.gri_score,
            s.gri_zone.label(),
            s.spike_readings_gt_140,
        ));

        if let Some(sat) = windowed_saturation_detail(s) {
            prompt.push_str(&format!("- Window Sensor Saturation Caveat: {sat}\n"));
        }

        let mut peak_hours: Vec<_> = s.agp_hourly.iter().filter(|h| h.count > 0).collect();
        peak_hours.sort_by(|a, b| b.p50.total_cmp(&a.p50));
        if !peak_hours.is_empty() {
            let top3: Vec<String> = peak_hours
                .iter()
                .take(3)
                .map(|h| {
                    format!(
                        "{:02}:00 (median {:.0}, p95 {:.0} mg/dL)",
                        h.hour, h.p50, h.p95
                    )
                })
                .collect();
            prompt.push_str(&format!(
                "- Highest 24h AGP local hours: {}\n",
                top3.join("; ")
            ));
        }
    } else {
        prompt.push_str("CGM Metrics: none loaded yet.\n");
    }

    if let Some(a) = analytics {
        let ep = &a.episode_counts;
        prompt.push_str(&format!(
            "Longitudinal Window Analytics:\n\
             - Sustained Episodes (>=15m): Level 2/Floor Hypo (<54/<55) {} ({}), Level 1 Hypo (<70) {} ({}), Hyper (>180) {} ({}), Lingo Excursions (>140) {} ({})\n",
            ep.level2_hypo_count,
            format_duration_minutes(ep.level2_hypo_minutes),
            ep.level1_hypo_count,
            format_duration_minutes(ep.level1_hypo_minutes),
            ep.hyper_gt_180_count,
            format_duration_minutes(ep.hyper_gt_180_minutes),
            ep.lingo_excursion_gt_140_count,
            format_duration_minutes(ep.lingo_excursion_gt_140_minutes),
        ));

        let circ_parts: Vec<String> = a
            .circadian
            .iter()
            .filter(|c| c.reading_count > 0)
            .map(|c| {
                format!(
                    "{} ({}): mean {:.0} mg/dL, TIR {:.0}%, TBR {:.1}%, ceil {:.0}%",
                    c.period.label(),
                    c.period.hours_label(),
                    c.mean_mg_dl,
                    c.tir_70_180_pct,
                    c.tbr_lt_70_pct,
                    c.ceiling_capped_pct
                )
            })
            .collect();
        if !circ_parts.is_empty() {
            prompt.push_str(&format!(
                "- Circadian Periods: {}\n",
                circ_parts.join(" | ")
            ));
        }

        if let Some(cmp) = &a.period_comparison {
            prompt.push_str(&format!(
                "- 14-Day vs Prior 14-Day Comparison: {}\n",
                format_period_comparison_summary(cmp)
            ));
        }

        let roc = &a.rate_of_change;
        if roc.sample_pairs > 0 {
            prompt.push_str(&format!(
                "- 15-Min Velocity (RoC): {} rapid rise burst(s) (>=+2.0), {} rapid fall burst(s) (<=-2.0 mg/dL/min), max rise {:+.2}, max fall {:+.2}, mean |RoC| {:.2} mg/dL/min\n",
                roc.rapid_rise_events,
                roc.rapid_fall_events,
                roc.max_rise_mg_dl_per_min,
                roc.max_fall_mg_dl_per_min,
                roc.mean_abs_roc,
            ));
        }

        if !a.sessions.is_empty() {
            let longest_gap_h = a
                .gaps
                .iter()
                .map(|g| g.duration_hours)
                .fold(0.0_f64, f64::max);
            prompt.push_str(&format!(
                "- Wear Sessions & Gaps: {} continuous session(s), {} gap(s) >=2h (longest {:.1}h), {} calendar day(s)\n",
                a.sessions.len(),
                a.gaps.len(),
                longest_gap_h,
                a.daily.len(),
            ));
        }
    }

    if let Some(d) = diagnostics {
        let oor_note = if d.out_of_range_rows > 0 {
            format!(
                ", \"Out of range\" status rows excluded: {}",
                d.out_of_range_rows
            )
        } else {
            String::new()
        };
        prompt.push_str(&format!(
            "Overall Import Span: {} total readings across {:.1} days (median cadence {:.1} min, longest import gap {:.1} h{oor_note}).\n",
            d.parsed_readings,
            d.span_hours / 24.0,
            d.median_interval_minutes.unwrap_or(0.0),
            d.longest_gap_minutes.unwrap_or(0.0) / 60.0,
        ));
    }

    if !checkins.is_empty() {
        prompt.push_str("Recent Mood & Mental Health Check-Ins:\n");
        for c in checkins.iter().rev().take(5).rev() {
            let phq = c.phq9.map(|v| format!(", PHQ-9 {v}")).unwrap_or_default();
            let gad = c.gad7.map(|v| format!(", GAD-7 {v}")).unwrap_or_default();
            prompt.push_str(&format!(
                "- [{}] Mood {}/10, Anxiety {}/10, Energy {}/10, Sleep {:.1}h{}{} — {}\n",
                c.recorded_at, c.mood, c.anxiety, c.energy, c.sleep_hours, phq, gad, c.note
            ));
        }
    }

    if !recent_turns.is_empty() {
        prompt.push_str("\nRecent conversation context:\n");
        for t in recent_turns.iter().rev().take(2).rev() {
            let short_ans: String = t.answer.chars().take(220).collect();
            prompt.push_str(&format!("User: {}\nAssistant: {}\n", t.question, short_ans));
        }
    }

    prompt.push_str(&format!("\nUser Question: {}\nAnswer:", question.trim()));
    prompt
}

/// Collect concise numerical grounding facts shown beneath every personal CGM chat answer.
pub fn collect_cgm_grounding_facts(
    monitor: GlucoseMonitor,
    summary: Option<&ClinicalGlucoseSummary>,
    diagnostics: Option<&GlucoseImportDiagnostics>,
    checkins: &[MentalHealthCheckIn],
) -> Vec<String> {
    collect_cgm_grounding_facts_with_analytics(monitor, summary, diagnostics, None, checkins)
}

/// Collect concise numerical grounding facts shown beneath every personal CGM chat answer,
/// using windowed saturation counts from `summary` (never full-import `diagnostics.bounded_sentinels`)
/// and optional [`AdvancedGlucoseAnalytics`].
pub fn collect_cgm_grounding_facts_with_analytics(
    monitor: GlucoseMonitor,
    summary: Option<&ClinicalGlucoseSummary>,
    diagnostics: Option<&GlucoseImportDiagnostics>,
    analytics: Option<&AdvancedGlucoseAnalytics>,
    checkins: &[MentalHealthCheckIn],
) -> Vec<String> {
    let mut facts = Vec::new();
    if let Some(s) = summary {
        facts.push(format!(
            "{}: {} readings · Mean {:.0} mg/dL · GMI {:.1}%",
            monitor.display_name(),
            s.reading_count,
            s.mean_mg_dl,
            s.gmi_percent
        ));
        facts.push(format!(
            "TIR 70–180: {:.0}% · Tight 70–140: {:.0}% · CV {:.1}% · GRI {:.0}/100",
            s.tir_70_180_pct, s.titr_70_140_pct, s.cv_percent, s.gri_score
        ));
        if s.has_sensor_saturation() {
            let mut cap_bits = Vec::new();
            if s.ceiling_capped_readings > 0 {
                cap_bits.push(format!(
                    "{} ceiling ≥201 ({:.1}%)",
                    s.ceiling_capped_readings, s.ceiling_capped_pct
                ));
            }
            if s.floor_capped_readings > 0 {
                cap_bits.push(format!(
                    "{} floor ≤54 ({:.1}%)",
                    s.floor_capped_readings, s.floor_capped_pct
                ));
            }
            facts.push(format!(
                "Window saturation: {} (Mean/GMI/CV clipped)",
                cap_bits.join(" · ")
            ));
        }
    }
    if let Some(d) = diagnostics {
        if d.out_of_range_rows > 0 {
            facts.push(format!(
                "Import: {} \"Out of range\" row(s) across {:.1}d",
                d.out_of_range_rows,
                d.span_hours / 24.0
            ));
        }
    }
    if let Some(a) = analytics {
        let ep = &a.episode_counts;
        facts.push(format!(
            "Sustained ≥15m episodes: {} hypo <70 ({} floor/<54) · {} hyper >180",
            ep.level1_hypo_count, ep.level2_hypo_count, ep.hyper_gt_180_count
        ));
        let overnight = a
            .circadian
            .iter()
            .find(|c| c.period == CircadianPeriod::Overnight && c.reading_count > 0);
        if let (Some(ov), Some((_, day_mean, day_tir))) =
            (overnight, daytime_circadian_aggregate(&a.circadian))
        {
            facts.push(format!(
                "Overnight {:.0} mg/dL ({:.0}% TIR) vs Daytime {:.0} mg/dL ({:.0}% TIR)",
                ov.mean_mg_dl, ov.tir_70_180_pct, day_mean, day_tir
            ));
        }
        if let Some(cmp) = &a.period_comparison {
            let trunc = if cmp.prior_days < cmp.recent_days - 1.0 {
                format!(" (prior {:.1}d)", cmp.prior_days)
            } else {
                String::new()
            };
            facts.push(format!(
                "14d trend{trunc}: Mean {:+.1} mg/dL · TIR {:+.1}pp · Ceil-cap {:+.1}pp",
                cmp.delta_mean_mg_dl, cmp.delta_tir_pct, cmp.delta_ceiling_capped_pct
            ));
        }
        let roc = &a.rate_of_change;
        if roc.sample_pairs > 0 && (roc.rapid_rise_events > 0 || roc.rapid_fall_events > 0) {
            facts.push(format!(
                "15m RoC: {} rapid rises (≥+2.0) · {} rapid falls (≤-2.0 mg/dL/min)",
                roc.rapid_rise_events, roc.rapid_fall_events
            ));
        }
    }
    if !checkins.is_empty() {
        facts.push(format!(
            "{} logged mood/mental-state check-in(s)",
            checkins.len()
        ));
    }
    facts
}

/// Deterministic local Rust fallback answering personal CGM & mood questions when
/// Android AICore / Gemini Nano is unavailable (e.g. emulator, non-AICore phones, browsers).
pub fn synthesize_grounded_cgm_answer(
    question: &str,
    monitor: GlucoseMonitor,
    summary: Option<&ClinicalGlucoseSummary>,
    diagnostics: Option<&GlucoseImportDiagnostics>,
    checkins: &[MentalHealthCheckIn],
) -> String {
    synthesize_grounded_cgm_answer_with_analytics(
        question,
        monitor,
        summary,
        diagnostics,
        None,
        checkins,
    )
}

/// Deterministic local Rust fallback answering personal CGM, longitudinal analytics,
/// and mood questions when Android AICore / Gemini Nano is unavailable.
pub fn synthesize_grounded_cgm_answer_with_analytics(
    question: &str,
    monitor: GlucoseMonitor,
    summary: Option<&ClinicalGlucoseSummary>,
    diagnostics: Option<&GlucoseImportDiagnostics>,
    analytics: Option<&AdvancedGlucoseAnalytics>,
    checkins: &[MentalHealthCheckIn],
) -> String {
    let q = question.to_ascii_lowercase();
    let Some(s) = summary else {
        return "No glucose readings are loaded yet. Sync from Android Health Connect or select an Abbott Lingo CSV / Health Connect JSON export first so I can ground the answer in your local data.".to_string();
    };

    let mut peak_hours: Vec<_> = s.agp_hourly.iter().filter(|h| h.count > 0).collect();
    peak_hours.sort_by(|a, b| b.p50.total_cmp(&a.p50));
    let top_agp = peak_hours
        .first()
        .map(|h| {
            format!(
                "{:02}:00 local (median {:.0} mg/dL, p95 {:.0} mg/dL)",
                h.hour, h.p50, h.p95
            )
        })
        .unwrap_or_else(|| "insufficient hourly data".to_string());

    let sat_sentence = windowed_saturation_detail(s)
        .map(|detail| format!(" Window saturation caveat: {detail}"))
        .unwrap_or_default();

    // 1. Clinician / doctor visit questions
    if q.contains("doctor") || q.contains("visit") || q.contains("question") {
        let trend_item = analytics
            .and_then(|a| a.period_comparison.as_ref())
            .map(|cmp| {
                format!(
                    "\n4. Over the last {:.1} days vs the prior {:.1} days, mean shifted {:+.1} mg/dL and TIR shifted {:+.1} percentage points—what might explain this change?",
                    cmp.recent_days, cmp.prior_days, cmp.delta_mean_mg_dl, cmp.delta_tir_pct
                )
            })
            .unwrap_or_default();
        let episode_note = analytics
            .map(|a| {
                let ep = &a.episode_counts;
                format!(
                    " Sustained ≥15m episodes in this window: {} hypo <70 ({} floor/<54) and {} hyper >180.",
                    ep.level1_hypo_count, ep.level2_hypo_count, ep.hyper_gt_180_count
                )
            })
            .unwrap_or_default();
        return format!(
            "Suggested visit questions grounded in your {} window ({} readings):\n\
             1. My mean glucose is {:.0} mg/dL (GMI {:.1}%) with {:.0}% TIR (70–180) and {:.0}% tight range (70–140)—does this align with my personal target?\n\
             2. My 24-hour AGP peaks around {} with {} spike reading(s) >140 mg/dL—what meal timing, fiber/protein pairing, or post-meal walk experiments would you suggest?\n\
             3. My glycemic variability is {:.1}% CV (target <36%) and GRI is {:.0}/100 ({}).{episode_note}{sat_sentence}{trend_item} (Not medical advice.)",
            monitor.display_name(),
            s.reading_count,
            s.mean_mg_dl,
            s.gmi_percent,
            s.tir_70_180_pct,
            s.titr_70_140_pct,
            top_agp,
            s.spike_readings_gt_140,
            s.cv_percent,
            s.gri_score,
            s.gri_zone.label(),
        );
    }

    // 2. Period-over-period trend / progress / 14-day comparison
    if q.contains("trend")
        || q.contains("improv")
        || q.contains("worse")
        || q.contains("compar")
        || q.contains("14")
        || q.contains("last two weeks")
        || q.contains("change")
    {
        if let Some(cmp) = analytics.and_then(|a| a.period_comparison.as_ref()) {
            let roc_note = analytics
                .filter(|a| a.rate_of_change.sample_pairs > 0)
                .map(|a| {
                    format!(
                        " 15-min velocity shows {} rapid rise burst(s) (≥+2.0 mg/dL/min) and {} rapid fall burst(s) (≤-2.0 mg/dL/min).",
                        a.rate_of_change.rapid_rise_events, a.rate_of_change.rapid_fall_events
                    )
                })
                .unwrap_or_default();
            return format!(
                "14-day vs prior 14-day {} comparison: {}.{roc_note}{sat_sentence} (Not medical advice.)",
                monitor.display_name(),
                format_period_comparison_summary(cmp),
            );
        }
        return format!(
            "A 14-day vs prior 14-day comparison requires at least ~20 days of CGM history in the loaded trace. \
             In your current {} window ({} readings, {:.0}% wear), mean glucose is {:.1} mg/dL (GMI {:.1}%), \
             TIR 70–180 is {:.1}%, tight 70–140 is {:.1}%, and CV is {:.1}%.{sat_sentence}",
            monitor.display_name(),
            s.reading_count,
            s.estimated_active_wear_pct,
            s.mean_mg_dl,
            s.gmi_percent,
            s.tir_70_180_pct,
            s.titr_70_140_pct,
            s.cv_percent,
        );
    }

    // 3. Sensor saturation, out-of-range rows, gaps, and wear reliability
    if q.contains("cap")
        || q.contains("saturat")
        || q.contains("201")
        || q.contains("out of range")
        || q.contains("gap")
        || q.contains("wear")
    {
        let sat_body = windowed_saturation_detail(s).unwrap_or_else(|| {
            "No floor (<=54) or ceiling (>=201) sensor-cap readings occurred in this window."
                .to_string()
        });
        let session_body = analytics
            .filter(|a| !a.sessions.is_empty())
            .map(|a| {
                let longest_gap = a
                    .gaps
                    .iter()
                    .map(|g| g.duration_hours)
                    .fold(0.0_f64, f64::max);
                if a.gaps.is_empty() {
                    format!(
                        " Detected {} continuous wear session across {} calendar day(s) with no gaps >=2h.",
                        a.sessions.len(),
                        a.daily.len()
                    )
                } else {
                    format!(
                        " Detected {} continuous wear session(s) and {} gap(s) >=2h (longest {:.1}h) across {} calendar day(s).",
                        a.sessions.len(),
                        a.gaps.len(),
                        longest_gap,
                        a.daily.len()
                    )
                }
            })
            .unwrap_or_default();
        let import_body = diagnostics
            .map(|d| {
                if d.out_of_range_rows > 0 {
                    format!(
                        " Across the full {:.1}-day import ({} valid readings), {} non-numeric \"Out of range\" sensor status row(s) were tracked separately and excluded from numeric averages.",
                        d.span_hours / 24.0,
                        d.parsed_readings,
                        d.out_of_range_rows
                    )
                } else {
                    format!(
                        " Full import spans {:.1} days ({} valid readings, 0 \"Out of range\" status rows).",
                        d.span_hours / 24.0,
                        d.parsed_readings
                    )
                }
            })
            .unwrap_or_default();
        return format!(
            "{} sensor reliability & saturation ({} readings in selected window, {:.1}% active wear): {sat_body}{session_body}{import_body}",
            monitor.display_name(),
            s.reading_count,
            s.estimated_active_wear_pct,
        );
    }

    // 4. Overnight / dawn / circadian patterns (also surfaces hypo episodes if asked together)
    if q.contains("overnight")
        || q.contains("night")
        || q.contains("dawn")
        || q.contains("morning")
        || q.contains("afternoon")
        || q.contains("evening")
        || q.contains("circadian")
    {
        let asks_low = q.contains("low")
            || q.contains("hypo")
            || q.contains("episode")
            || q.contains("<70")
            || q.contains("<54")
            || q.contains("<55");

        let circadian_body = analytics
            .map(|a| {
                let populated: Vec<String> = a
                    .circadian
                    .iter()
                    .filter(|c| c.reading_count > 0)
                    .map(|c| {
                        format!(
                            "{} ({}): mean {:.0} mg/dL, TIR {:.0}%, TBR <70 {:.1}%, ceil {:.0}%",
                            c.period.label(),
                            c.period.hours_label(),
                            c.mean_mg_dl,
                            c.tir_70_180_pct,
                            c.tbr_lt_70_pct,
                            c.ceiling_capped_pct
                        )
                    })
                    .collect();
                if populated.is_empty() {
                    String::new()
                } else {
                    let contrast = a
                        .circadian
                        .iter()
                        .find(|c| c.period == CircadianPeriod::Overnight && c.reading_count > 0)
                        .zip(daytime_circadian_aggregate(&a.circadian))
                        .map(|(ov, (_, day_mean, day_tir))| {
                            format!(
                                "Overnight averages {:.1} mg/dL ({:.0}% TIR) vs Daytime {:.1} mg/dL ({:.0}% TIR). ",
                                ov.mean_mg_dl, ov.tir_70_180_pct, day_mean, day_tir
                            )
                        })
                        .unwrap_or_default();
                    format!("{contrast}Circadian breakdown: {}.", populated.join("; "))
                }
            })
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| {
                let overnight_bins: Vec<_> = s
                    .agp_hourly
                    .iter()
                    .filter(|h| h.hour < 6 && h.count > 0)
                    .collect();
                let daytime_bins: Vec<_> = s
                    .agp_hourly
                    .iter()
                    .filter(|h| h.hour >= 6 && h.count > 0)
                    .collect();
                let ov_med = if overnight_bins.is_empty() {
                    None
                } else {
                    Some(
                        overnight_bins.iter().map(|h| h.p50).sum::<f64>()
                            / overnight_bins.len() as f64,
                    )
                };
                let day_med = if daytime_bins.is_empty() {
                    None
                } else {
                    Some(
                        daytime_bins.iter().map(|h| h.p50).sum::<f64>() / daytime_bins.len() as f64,
                    )
                };
                match (ov_med, day_med) {
                    (Some(ov), Some(day)) => format!(
                        "Overnight (00:00–06:00) AGP median averages {:.0} mg/dL vs Daytime (06:00–24:00) {:.0} mg/dL; highest hourly AGP peak is {}.",
                        ov, day, top_agp
                    ),
                    _ => format!("Highest hourly AGP local peak is {top_agp}."),
                }
            });

        let hypo_addon = if asks_low {
            if let Some(a) = analytics {
                let ep = &a.episode_counts;
                format!(
                    " Sustained low episodes (≥15m): {} Level 1 hypo (<70 mg/dL, total {}) including {} Level 2/floor hypo (<54 / ≤54 floor, total {}); overall TBR <70 is {:.1}% (floor-capped ≤54: {}, {:.1}%).",
                    ep.level1_hypo_count,
                    format_duration_minutes(ep.level1_hypo_minutes),
                    ep.level2_hypo_count,
                    format_duration_minutes(ep.level2_hypo_minutes),
                    s.tbr_very_low_lt_54_pct + s.tbr_low_54_69_pct,
                    s.floor_capped_readings,
                    s.floor_capped_pct,
                )
            } else {
                format!(
                    " Time below 70 mg/dL is {:.1}% ({} reading(s), min {:.0} mg/dL; floor-capped ≤54: {}, {:.1}%).",
                    s.tbr_very_low_lt_54_pct + s.tbr_low_54_69_pct,
                    s.low_readings_lt_70,
                    s.min_mg_dl,
                    s.floor_capped_readings,
                    s.floor_capped_pct,
                )
            }
        } else {
            String::new()
        };

        return format!(
            "{} circadian pattern ({} readings): {circadian_body}{hypo_addon}{sat_sentence}",
            monitor.display_name(),
            s.reading_count,
        );
    }

    // 5. Hypoglycemia / low glucose episodes
    if q.contains("low")
        || q.contains("hypo")
        || q.contains("<70")
        || q.contains("<54")
        || q.contains("<55")
    {
        let combined_tbr = s.tbr_very_low_lt_54_pct + s.tbr_low_54_69_pct;
        let episodes_body = if let Some(a) = analytics {
            let ep = &a.episode_counts;
            let longest_hypo = a
                .episodes
                .iter()
                .filter(|e| {
                    matches!(
                        e.kind,
                        EpisodeKind::Level1HypoLt70 | EpisodeKind::Level2HypoLt54
                    )
                })
                .max_by(|a, b| a.duration_minutes.total_cmp(&b.duration_minutes));
            let longest_note = longest_hypo
                .map(|e| {
                    format!(
                        " Longest sustained low lasted {} (nadir {:.0} mg/dL{}).",
                        format_duration_minutes(e.duration_minutes),
                        e.extreme_mg_dl,
                        if e.sensor_capped {
                            ", floor-capped"
                        } else {
                            ""
                        }
                    )
                })
                .unwrap_or_default();
            let fall_note = if a.rate_of_change.sample_pairs > 0 {
                format!(
                    " Rapid falls (≤-2.0 mg/dL/min): {} burst(s) (steepest {:+.2} mg/dL/min).",
                    a.rate_of_change.rapid_fall_events, a.rate_of_change.max_fall_mg_dl_per_min
                )
            } else {
                String::new()
            };
            format!(
                " Distinct sustained ≥15m episodes: {} Level 1 hypo (<70 mg/dL, total {}) and {} Level 2/floor hypo (<54 / ≤54 floor, total {}).{longest_note}{fall_note}",
                ep.level1_hypo_count,
                format_duration_minutes(ep.level1_hypo_minutes),
                ep.level2_hypo_count,
                format_duration_minutes(ep.level2_hypo_minutes),
            )
        } else {
            String::new()
        };
        let floor_note = if s.floor_capped_readings > 0 {
            format!(
                " {} reading(s) ({:.1}%) hit the biosensor floor cap (<=54 / <55 mg/dL), so true nadirs may be lower.",
                s.floor_capped_readings, s.floor_capped_pct
            )
        } else {
            String::new()
        };
        return format!(
            "{} hypoglycemia summary ({} readings): TBR <70 mg/dL is {:.1}% (<54: {:.1}%, 54–69: {:.1}%; ADA target <4% total and <1% <54), \
             with {} low reading(s) and minimum {:.0} mg/dL.{episodes_body}{floor_note} (Discuss any persistent lows with your clinician.)",
            monitor.display_name(),
            s.reading_count,
            combined_tbr,
            s.tbr_very_low_lt_54_pct,
            s.tbr_low_54_69_pct,
            s.low_readings_lt_70,
            s.min_mg_dl,
        );
    }

    // 6. AGP / time of day / spikes
    if q.contains("agp")
        || q.contains("time of day")
        || q.contains("hour")
        || q.contains("spike")
        || q.contains("cluster")
    {
        let top3: Vec<String> = peak_hours
            .iter()
            .take(3)
            .map(|h| {
                format!(
                    "{:02}:00 (median {:.0}, p95 {:.0} mg/dL)",
                    h.hour, h.p50, h.p95
                )
            })
            .collect();
        let analytics_addon = analytics
            .map(|a| {
                let ep = &a.episode_counts;
                let roc = &a.rate_of_change;
                let roc_part = if roc.sample_pairs > 0 {
                    format!(
                        "; 15-min velocity shows {} rapid rise burst(s) (≥+2.0) and {} rapid fall burst(s) (≤-2.0 mg/dL/min)",
                        roc.rapid_rise_events, roc.rapid_fall_events
                    )
                } else {
                    String::new()
                };
                format!(
                    " Distinct sustained ≥15m episodes: {} hyper >180 ({}) and {} excursion(s) >140 ({}){roc_part}.",
                    ep.hyper_gt_180_count,
                    format_duration_minutes(ep.hyper_gt_180_minutes),
                    ep.lingo_excursion_gt_140_count,
                    format_duration_minutes(ep.lingo_excursion_gt_140_minutes),
                )
            })
            .unwrap_or_default();
        return format!(
            "Across {} {} readings, there are {} readings >140 mg/dL (range {:.0}–{:.0} mg/dL, MAGE {:.0} mg/dL). \
             Your highest local 24-hour AGP clock hours are: {}. \
             Tight time-in-range (70–140 mg/dL) is {:.0}% and ADA 70–180 TIR is {:.0}%.{analytics_addon}{sat_sentence}",
            s.reading_count,
            monitor.display_name(),
            s.spike_readings_gt_140,
            s.min_mg_dl,
            s.max_mg_dl,
            s.mage_mg_dl,
            if top3.is_empty() {
                "not enough hourly bins yet".to_string()
            } else {
                top3.join(", ")
            },
            s.titr_70_140_pct,
            s.tir_70_180_pct,
        );
    }

    // 7. Mood / anxiety / sleep / PHQ-9 / GAD-7 correlation
    if q.contains("mood")
        || q.contains("anxiety")
        || q.contains("sleep")
        || q.contains("phq")
        || q.contains("gad")
        || q.contains("mental")
    {
        if checkins.is_empty() {
            return format!(
                "Your {} trace shows {:.0} mg/dL mean (CV {:.1}%, {} spikes >140), but no mood or anxiety check-ins are logged yet. Add check-ins in the form below to correlate glycemic swings with mood, anxiety, and sleep locally.{sat_sentence}",
                monitor.display_name(),
                s.mean_mg_dl,
                s.cv_percent,
                s.spike_readings_gt_140,
            );
        }
        let avg_mood =
            checkins.iter().map(|c| f64::from(c.mood)).sum::<f64>() / checkins.len() as f64;
        let avg_anx =
            checkins.iter().map(|c| f64::from(c.anxiety)).sum::<f64>() / checkins.len() as f64;
        let avg_sleep = checkins
            .iter()
            .map(|c| f64::from(c.sleep_hours))
            .sum::<f64>()
            / checkins.len() as f64;
        let lowest = checkins.iter().min_by_key(|c| c.mood).unwrap();
        let roc_note = analytics
            .filter(|a| a.rate_of_change.sample_pairs > 0)
            .map(|a| {
                format!(
                    " Rapid glycemic shifts in this window: {} rise burst(s) and {} fall burst(s) (≥2.0 mg/dL/min).",
                    a.rate_of_change.rapid_rise_events, a.rate_of_change.rapid_fall_events
                )
            })
            .unwrap_or_default();
        return format!(
            "Across {} local check-ins paired with your {} trace (mean {:.0} mg/dL, CV {:.1}%, {} spikes >140): \
             average mood is {:.1}/10, anxiety {:.1}/10, and sleep {:.1}h. \
             Lowest logged mood ({}/10, anxiety {}/10) was at [{}]: \"{}\". \
             Consider tracking post-meal focus/anxiety 60–90 minutes after your peak AGP hour ({}).{roc_note}{sat_sentence}",
            checkins.len(),
            monitor.display_name(),
            s.mean_mg_dl,
            s.cv_percent,
            s.spike_readings_gt_140,
            avg_mood,
            avg_anx,
            avg_sleep,
            lowest.mood,
            lowest.anxiety,
            lowest.recorded_at,
            lowest.note,
            top_agp,
        );
    }

    // 8. Default comprehensive summary
    let analytics_summary = analytics
        .map(|a| {
            let ep = &a.episode_counts;
            let mut parts = vec![format!(
                "Sustained ≥15m episodes: {} hypo <70 ({} floor/<54), {} hyper >180",
                ep.level1_hypo_count, ep.level2_hypo_count, ep.hyper_gt_180_count
            )];
            if let (Some(ov), Some((_, day_mean, day_tir))) = (
                a.circadian
                    .iter()
                    .find(|c| c.period == CircadianPeriod::Overnight && c.reading_count > 0),
                daytime_circadian_aggregate(&a.circadian),
            ) {
                parts.push(format!(
                    "Overnight mean {:.0} mg/dL ({:.0}% TIR) vs Daytime {:.0} mg/dL ({:.0}% TIR)",
                    ov.mean_mg_dl, ov.tir_70_180_pct, day_mean, day_tir
                ));
            }
            if let Some(cmp) = &a.period_comparison {
                parts.push(format!(
                    "14d vs prior {:.1}d: Mean {:+.1} mg/dL, TIR {:+.1}pp",
                    cmp.prior_days, cmp.delta_mean_mg_dl, cmp.delta_tir_pct
                ));
            }
            if a.rate_of_change.sample_pairs > 0 {
                parts.push(format!(
                    "15-min velocity: {} rapid rise(s), {} rapid fall(s)",
                    a.rate_of_change.rapid_rise_events, a.rate_of_change.rapid_fall_events
                ));
            }
            format!(" {}.", parts.join(". "))
        })
        .unwrap_or_default();

    format!(
        "{} local summary ({} readings, {:.0}% active wear): mean glucose {:.0} mg/dL (median {:.0}, range {:.0}–{:.0}), \
         GMI {:.1}%, ADA TIR (70–180) {:.0}%, tight range (70–140) {:.0}%, CV {:.1}% (target <36%), and GRI {:.0}/100 ({}). \
         Peak 24-hour AGP local hour is {} with {} reading(s) >140 mg/dL.{analytics_summary}{sat_sentence}",
        monitor.display_name(),
        s.reading_count,
        s.estimated_active_wear_pct,
        s.mean_mg_dl,
        s.median_mg_dl,
        s.min_mg_dl,
        s.max_mg_dl,
        s.gmi_percent,
        s.tir_70_180_pct,
        s.titr_70_140_pct,
        s.cv_percent,
        s.gri_score,
        s.gri_zone.label(),
        top_agp,
        s.spike_readings_gt_140,
    )
}

/// Build a strictly single-patient grounded prompt for on-device Gemini Nano synthesis
/// over the verbatim citations already retrieved by `clinical_workspace`.
pub fn build_chart_chat_prompt(
    audience: &str,
    patient_name: &str,
    question: &str,
    grounded: &ChatAnswer,
) -> String {
    let mut prompt = String::with_capacity(2048);
    prompt.push_str(&format!(
        "You are a careful, concise on-device clinical demo assistant speaking to a {audience}.\n\
         Rules:\n\
         - Answer ONLY from the single patient's verbatim chart excerpts below for {patient_name}.\n\
         - Keep it to 3-5 clear sentences, cite record titles/dates accurately, never invent measurements, and note if the excerpts do not cover part of the question.\n\n"
    ));

    if grounded.citations.is_empty() {
        prompt.push_str("Retrieved chart excerpts: none matched.\n");
    } else {
        prompt.push_str("Verbatim chart excerpts:\n");
        for cit in &grounded.citations {
            prompt.push_str(&format!(
                "- [{} | {} | {}] {}\n",
                cit.title,
                cit.recorded_at,
                cit.kind.label(),
                cit.excerpt
            ));
        }
    }

    prompt.push_str(&format!(
        "\nBaseline factual summary: {}\n\nQuestion: {}\nGrounded answer:",
        grounded.answer,
        question.trim()
    ));
    prompt
}

#[cfg(any(feature = "hydrate", feature = "csr"))]
fn js_error_to_string(err: wasm_bindgen::JsValue) -> String {
    use js_sys::{Reflect, JSON};
    use wasm_bindgen::JsValue;

    if let Some(s) = err.as_string() {
        return s;
    }
    if let Ok(msg) = Reflect::get(&err, &JsValue::from_str("message")) {
        if let Some(s) = msg.as_string() {
            return s;
        }
    }
    JSON::stringify(&err)
        .ok()
        .and_then(|s| s.as_string())
        .unwrap_or_else(|| "Tauri command failed".to_string())
}

/// Invoke an app-registered Tauri v2 command via `window.__TAURI__.core.invoke`
/// without extra JS-bridge crate dependencies. Calls `on_done(Err("not_tauri"))`
/// immediately when running in a plain browser.
#[cfg(any(feature = "hydrate", feature = "csr"))]
pub fn invoke_tauri_command<T, F>(cmd: &str, args: Option<js_sys::Object>, on_done: F)
where
    T: serde::de::DeserializeOwned + 'static,
    F: FnOnce(Result<T, String>) + 'static,
{
    use js_sys::{Function, Object, Reflect, JSON};
    use std::cell::RefCell;
    use std::rc::Rc;
    use wasm_bindgen::{closure::Closure, JsCast, JsValue};

    let Some(win) = web_sys::window() else {
        on_done(Err("not_tauri".to_string()));
        return;
    };
    let tauri =
        Reflect::get(win.as_ref(), &JsValue::from_str("__TAURI__")).unwrap_or(JsValue::UNDEFINED);
    if tauri.is_undefined() || tauri.is_null() {
        on_done(Err("not_tauri".to_string()));
        return;
    }
    let core = Reflect::get(&tauri, &JsValue::from_str("core")).unwrap_or(JsValue::UNDEFINED);
    if core.is_undefined() || core.is_null() {
        on_done(Err("not_tauri".to_string()));
        return;
    }
    let invoke = Reflect::get(&core, &JsValue::from_str("invoke")).unwrap_or(JsValue::UNDEFINED);
    let Some(invoke_fn) = invoke.dyn_ref::<Function>() else {
        on_done(Err("not_tauri".to_string()));
        return;
    };

    let args_obj = args.unwrap_or_else(Object::new);
    let Ok(promise_val) = invoke_fn.call2(&core, &JsValue::from_str(cmd), args_obj.as_ref()) else {
        on_done(Err(format!("Failed to invoke {cmd}")));
        return;
    };
    let then_val =
        Reflect::get(&promise_val, &JsValue::from_str("then")).unwrap_or(JsValue::UNDEFINED);
    let Some(then_fn) = then_val.dyn_ref::<Function>() else {
        on_done(Err(format!("Command {cmd} did not return a Promise")));
        return;
    };

    let cb_ok = Rc::new(RefCell::new(Some(on_done)));
    let cb_err = Rc::clone(&cb_ok);

    let on_ok = Closure::once_into_js(move |val: JsValue| {
        if let Some(done) = cb_ok.borrow_mut().take() {
            let parsed = JSON::stringify(&val)
                .ok()
                .and_then(|s| s.as_string())
                .ok_or_else(|| "Could not serialize Tauri response to JSON".to_string())
                .and_then(|raw| {
                    serde_json::from_str::<T>(&raw)
                        .map_err(|err| format!("Unexpected Tauri response shape: {err}"))
                });
            done(parsed);
        }
    });

    let on_err = Closure::once_into_js(move |err: JsValue| {
        if let Some(done) = cb_err.borrow_mut().take() {
            done(Err(js_error_to_string(err)));
        }
    });

    let _ = then_fn.call2(&promise_val, &on_ok, &on_err);
}

/// Invoke a Tauri v2 command passing a serde-serializable JSON object as arguments.
#[cfg(any(feature = "hydrate", feature = "csr"))]
pub fn invoke_tauri_json<A, T, F>(cmd: &str, args: &A, on_done: F)
where
    A: Serialize,
    T: serde::de::DeserializeOwned + 'static,
    F: FnOnce(Result<T, String>) + 'static,
{
    use js_sys::{Object, JSON};
    use wasm_bindgen::JsCast;

    let Ok(raw) = serde_json::to_string(args) else {
        on_done(Err(format!("Failed to serialize arguments for {cmd}")));
        return;
    };
    let Ok(js_val) = JSON::parse(&raw) else {
        on_done(Err(format!("Failed to parse JS arguments for {cmd}")));
        return;
    };
    let Ok(obj) = js_val.dyn_into::<Object>() else {
        on_done(Err(format!(
            "Command {cmd} arguments must be a JSON object"
        )));
        return;
    };
    invoke_tauri_command(cmd, Some(obj), on_done);
}

/// Probe `health_connect_status` silently; invokes `on_enabled` only when `enabled == true`.
#[cfg(any(feature = "hydrate", feature = "csr"))]
pub fn probe_personal_health_bridge<F>(on_enabled: F)
where
    F: FnOnce(HealthBridgeStatusUi) + 'static,
{
    invoke_tauri_command::<HealthBridgeStatusUi, _>("health_connect_status", None, move |res| {
        if let Ok(status) = res {
            if status.enabled {
                on_enabled(status);
            }
        }
    });
}

/// Probe `personal_vault_status` silently; invokes `on_enabled` only when `enabled == true`.
#[cfg(any(feature = "hydrate", feature = "csr"))]
pub fn probe_personal_vault<F>(on_enabled: F)
where
    F: FnOnce(PersonalVaultStatusUi) + 'static,
{
    invoke_tauri_command::<PersonalVaultStatusUi, _>("personal_vault_status", None, move |res| {
        if let Ok(status) = res {
            if status.enabled {
                on_enabled(status);
            }
        }
    });
}

/// Decrypt and load the personal vault snapshot on-device.
#[cfg(any(feature = "hydrate", feature = "csr"))]
pub fn load_personal_vault<F>(on_done: F)
where
    F: FnOnce(Result<PersonalVaultSnapshotUi, String>) + 'static,
{
    invoke_tauri_command("personal_vault_load", None, on_done);
}

/// Merge and encrypt an updated personal vault payload on-device.
#[cfg(any(feature = "hydrate", feature = "csr"))]
pub fn save_personal_vault<F>(payload: &PersonalVaultSaveUi, on_done: F)
where
    F: FnOnce(Result<PersonalVaultStatusUi, String>) + 'static,
{
    #[derive(Serialize)]
    struct Wrapper<'a> {
        payload: &'a PersonalVaultSaveUi,
    }
    invoke_tauri_json("personal_vault_save", &Wrapper { payload }, on_done);
}

/// Cryptographically wipe the encrypted personal vault and Android Keystore key alias.
#[cfg(any(feature = "hydrate", feature = "csr"))]
pub fn wipe_personal_vault<F>(on_done: F)
where
    F: FnOnce(Result<PersonalVaultStatusUi, String>) + 'static,
{
    invoke_tauri_command("personal_vault_wipe", None, on_done);
}

/// Probe `local_llm_status` silently; invokes `on_enabled` only when `enabled == true`.
#[cfg(any(feature = "hydrate", feature = "csr"))]
pub fn probe_local_llm<F>(on_enabled: F)
where
    F: FnOnce(LocalLlmStatusUi) + 'static,
{
    invoke_tauri_command::<LocalLlmStatusUi, _>("local_llm_status", None, move |res| {
        if let Ok(status) = res {
            if status.enabled {
                on_enabled(status);
            }
        }
    });
}

/// Trigger Gemini Nano download via Android AICore when `downloadable`.
#[cfg(any(feature = "hydrate", feature = "csr"))]
pub fn prepare_local_llm<F>(on_done: F)
where
    F: FnOnce(Result<LocalLlmPrepareUi, String>) + 'static,
{
    invoke_tauri_command("local_llm_prepare", None, on_done);
}

/// Run on-device Gemini Nano text generation via Android AICore.
#[cfg(any(feature = "hydrate", feature = "csr"))]
pub fn generate_local_llm<F>(prompt: &str, max_tokens: Option<u32>, on_done: F)
where
    F: FnOnce(Result<LocalLlmGenerateUi, String>) + 'static,
{
    #[derive(Serialize)]
    struct Args<'a> {
        prompt: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        max_tokens: Option<u32>,
    }
    invoke_tauri_json("local_llm_generate", &Args { prompt, max_tokens }, on_done);
}

/// Native mobile glucose alert bridge status returned by `app_alerts_status`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppAlertStatusUi {
    pub enabled: bool,
    pub platform: String,
    pub permission_granted: bool,
    pub permission_status: String,
    pub channel_id: String,
}

/// Outcome returned by `app_alerts_request_permission`.
#[cfg(any(feature = "hydrate", feature = "csr", test))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppAlertPermissionUi {
    pub granted: bool,
    pub status: String,
}

/// Notification payload sent to `app_alerts_send_notification`.
#[cfg(any(feature = "hydrate", feature = "csr", test))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppAlertNotificationUi {
    pub title: String,
    pub body: String,
    pub severity: String,
    pub tag: String,
}

/// Outcome returned by `app_alerts_send_notification`.
#[cfg(any(feature = "hydrate", feature = "csr", test))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppAlertDispatchUi {
    pub delivered: bool,
    pub notification_id: i32,
    pub detail: String,
}

/// Probe `app_alerts_status` silently; invokes `on_enabled` only when `enabled == true`.
#[cfg(any(feature = "hydrate", feature = "csr"))]
pub fn probe_app_alerts<F: FnOnce(AppAlertStatusUi) + 'static>(on_enabled: F) {
    invoke_tauri_command::<AppAlertStatusUi, _>("app_alerts_status", None, move |res| {
        if let Ok(status) = res {
            if status.enabled {
                on_enabled(status);
            }
        }
    });
}

/// Request native mobile notification permission via `app_alerts_request_permission`.
#[cfg(any(feature = "hydrate", feature = "csr"))]
pub fn request_app_alert_permission<F: FnOnce(Result<AppAlertPermissionUi, String>) + 'static>(
    on_done: F,
) {
    invoke_tauri_command("app_alerts_request_permission", None, on_done);
}

/// Dispatch an opt-in local glucose alert notification via `app_alerts_send_notification`.
#[cfg(any(feature = "hydrate", feature = "csr"))]
pub fn send_app_alert_notification<F: FnOnce(Result<AppAlertDispatchUi, String>) + 'static>(
    payload: &AppAlertNotificationUi,
    on_done: F,
) {
    #[derive(Serialize)]
    struct Wrapper<'a> {
        payload: &'a AppAlertNotificationUi,
    }
    invoke_tauri_json(
        "app_alerts_send_notification",
        &Wrapper { payload },
        on_done,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mental_health_cgm::{parse_commercial_glucose_input, summarize_commercial_glucose};
    use crate::mental_health_lingo::{sample_checkins, SAMPLE_LINGO_CSV};
    use clinical_workspace::{answer_patient_question, synthetic_panel};

    #[test]
    fn recognizes_android_and_ios_health_bridge_platforms() {
        let android: HealthBridgeStatusUi = serde_json::from_str(
            r#"{
                "enabled": true,
                "platform": "android_health_connect",
                "sdk_status": "available",
                "has_glucose_permission": true,
                "package_id": "moe.dabney.personal"
            }"#,
        )
        .unwrap();
        assert_eq!(
            android.platform_kind(),
            HealthBridgePlatform::AndroidHealthConnect
        );
        assert_eq!(
            android.platform_kind().display_name(),
            "Android Health Connect"
        );

        let ios: HealthBridgeStatusUi = serde_json::from_str(
            r#"{
                "enabled": true,
                "platform": "ios_healthkit",
                "sdk_status": "available",
                "has_glucose_permission": false,
                "package_id": "moe.dabney.personal"
            }"#,
        )
        .unwrap();
        assert_eq!(ios.platform_kind(), HealthBridgePlatform::IosHealthKit);
        assert_eq!(ios.platform_kind().display_name(), "Apple HealthKit");
    }

    #[test]
    fn grounded_cgm_prompt_and_fallback_include_computed_metrics() {
        let import = parse_commercial_glucose_input(SAMPLE_LINGO_CSV, None).unwrap();
        let summary = summarize_commercial_glucose(
            &import.readings,
            import.monitor,
            import.source_unit,
            import.method,
        )
        .unwrap();
        let checkins = sample_checkins();

        let prompt = build_personal_cgm_chat_prompt(
            "What time of day do spikes peak?",
            import.monitor,
            Some(&summary),
            Some(&import.diagnostics),
            &checkins,
            &[],
        );
        assert!(prompt.contains("Abbott Lingo"));
        assert!(prompt.contains("ADA 5-Band TIR"));
        assert!(prompt.contains("Highest 24h AGP local hours"));
        assert!(prompt.len() < 4000);

        let fallback = synthesize_grounded_cgm_answer(
            "Draft questions for my doctor",
            import.monitor,
            Some(&summary),
            Some(&import.diagnostics),
            &checkins,
        );
        assert!(fallback.contains("Suggested visit questions"));
        assert!(fallback.contains("Not medical advice"));

        let mood_ans = synthesize_grounded_cgm_answer(
            "How do mood and anxiety line up?",
            import.monitor,
            Some(&summary),
            Some(&import.diagnostics),
            &checkins,
        );
        assert!(mood_ans.contains("average mood"));

        let steps = build_cgm_agent_steps(
            "How do mood and anxiety line up?",
            import.monitor,
            Some(&summary),
            Some(&import.diagnostics),
            &checkins,
            false,
        );
        assert!(steps.iter().any(|s| s.tool_name == "cgm_window_metrics"));
        assert!(steps.iter().any(|s| s.tool_name == "agp_24h_peaks"));
        assert!(steps.iter().any(|s| s.tool_name == "mood_checkin_lookup"));
        assert!(steps.iter().any(|s| s.tool_name == "rust_grounded_synth"));

        // Older encrypted vault turns serialized without `tool_steps` still deserialize cleanly.
        let legacy_turn: LocalChatTurn = serde_json::from_str(
            r#"{
                "id": "cgm-chat-1",
                "question": "Summarize my glucose",
                "answer": "Mean 104 mg/dL",
                "engine_label": "Grounded Rust · On-Device",
                "latency_ms": null,
                "grounding_facts": ["TIR 96%"]
            }"#,
        )
        .unwrap();
        assert!(legacy_turn.tool_steps.is_empty());
    }

    #[test]
    fn chart_chat_prompt_stays_scoped_to_single_patient_citations() {
        let panel = synthetic_panel();
        let nadia = &panel[0];
        let grounded = answer_patient_question(nadia, "What does my CGM say about mornings?");
        let prompt = build_chart_chat_prompt(
            "patient",
            &nadia.display_name,
            "What does my CGM say about mornings?",
            &grounded,
        );
        assert!(prompt.contains("Nadia Kulkarni"));
        assert!(prompt.contains("Verbatim chart excerpts:"));
        assert!(!prompt.contains("Carlos Mendez"));
    }

    #[test]
    fn vault_and_local_llm_wire_payloads_deserialize_cleanly() {
        let vault: PersonalVaultStatusUi = serde_json::from_str(
            r#"{
                "enabled": true,
                "encrypted": true,
                "cipher": "AES-256-GCM (Android Keystore)",
                "storage_backend": "android_keystore_files",
                "has_cgm_snapshot": true,
                "cgm_reading_count": 288,
                "cgm_saved_at": "2026-11-25T19:00:00Z",
                "checkin_count": 2,
                "note_count": 1,
                "chat_turn_count": 3,
                "total_bytes": 12000
            }"#,
        )
        .unwrap();
        assert!(vault.enabled && vault.encrypted);
        assert_eq!(vault.cgm_reading_count, 288);

        let llm: LocalLlmStatusUi = serde_json::from_str(
            r#"{
                "enabled": true,
                "backend": "android_aicore_gemini_nano",
                "model_name": "Gemini Nano (Android AICore)",
                "availability": "available",
                "device_summary": "Android Flagship · Snapdragon · 12.0 GB RAM · API 36",
                "total_ram_mb": 12288,
                "aicore_installed": true,
                "detail": "Ready"
            }"#,
        )
        .unwrap();
        assert_eq!(llm.availability_kind(), LocalLlmAvailability::Available);
        assert!(llm.aicore_installed);
    }

    #[test]
    fn windowed_saturation_never_leaks_full_import_bounded_sentinels_into_clean_window() {
        use crate::mental_health_cgm::{
            slice_readings_window, summarize_commercial_glucose_with_window, GlucoseWindow,
        };
        use crate::mental_health_cgm_analytics::analyze_glucose_trace;

        // Synthetic 3-day Lingo CSV: Day 1 has ceiling-capped 201 and "Out of range" rows,
        // while the trailing 24h (Day 3) is completely in-range with zero sensor saturation.
        let mut csv = String::from("Timestamp,Glucose (mg/dL)\n");
        for m in (0..720).step_by(5) {
            let h = m / 60;
            let min = m % 60;
            let val = if m < 120 {
                "201"
            } else if m < 130 {
                "Out of range"
            } else {
                "112"
            };
            csv.push_str(&format!("2025-05-01T{h:02}:{min:02}-07:00,{val}\n"));
        }
        for m in (0..1440).step_by(15) {
            let h = m / 60;
            let min = m % 60;
            csv.push_str(&format!("2025-05-03T{h:02}:{min:02}-07:00,104\n"));
        }

        let import =
            parse_commercial_glucose_input(&csv, Some(GlucoseMonitor::AbbottLingo)).unwrap();
        assert!(import.diagnostics.bounded_sentinels > 0);
        assert!(import.diagnostics.out_of_range_rows > 0);

        let window_24h = slice_readings_window(&import.readings, GlucoseWindow::Hours24);
        let summary_24h = summarize_commercial_glucose_with_window(
            &window_24h,
            import.monitor,
            import.source_unit,
            import.method,
            Some(24.0),
        )
        .unwrap();
        assert!(!summary_24h.has_sensor_saturation());

        let analytics_24h = analyze_glucose_trace(
            &window_24h,
            import.monitor,
            import.method,
            Some(&import.readings),
        );

        let facts = collect_cgm_grounding_facts_with_analytics(
            import.monitor,
            Some(&summary_24h),
            Some(&import.diagnostics),
            Some(&analytics_24h),
            &[],
        );
        assert!(
            !facts.iter().any(|f| f.contains("Window saturation")),
            "Clean 24h window must not inherit full-import bounded_sentinels: {facts:?}"
        );
        assert!(facts.iter().any(|f| f.contains("Out of range")));

        let cap_answer = synthesize_grounded_cgm_answer_with_analytics(
            "Are sensor caps or gaps affecting my readings?",
            import.monitor,
            Some(&summary_24h),
            Some(&import.diagnostics),
            Some(&analytics_24h),
            &[],
        );
        assert!(cap_answer.contains(
            "No floor (<=54) or ceiling (>=201) sensor-cap readings occurred in this window"
        ));
        assert!(cap_answer.contains("Out of range"));
    }

    #[test]
    fn analytics_grounded_chat_covers_overnight_hypos_trends_and_saturation() {
        use crate::mental_health_cgm::{
            nominal_window_hours_for_trace, slice_readings_window,
            summarize_commercial_glucose_with_window, GlucoseWindow,
        };
        use crate::mental_health_cgm_analytics::analyze_glucose_trace;

        // Build a synthetic 28-day trace with a gap in the prior 14d (truncated prior span),
        // floor-capped 54 mg/dL sustained overnight hypo, ceiling-capped 201 mg/dL hyper plateau,
        // and rapid 15-min rise/fall excursions.
        let mut csv = String::from("Timestamp,Glucose (mg/dL)\n");
        for day in 1..=28 {
            // Leave a 5-day gap on days 4..=8 so prior 14d span is truncated.
            if (4..=8).contains(&day) {
                continue;
            }
            for step in 0..48 {
                let total_min = step * 30;
                let h = total_min / 60;
                let min = total_min % 60;
                let val = if day <= 14 {
                    // Prior 14d: mostly in-range 110
                    110
                } else if h < 2 {
                    // Overnight low (floor-capped on day 27)
                    if day == 27 {
                        54
                    } else {
                        98
                    }
                } else if (12..=15).contains(&h) {
                    // Afternoon ceiling-capped plateau
                    201
                } else {
                    135
                };
                csv.push_str(&format!("2025-04-{day:02}T{h:02}:{min:02}-07:00,{val}\n"));
            }
        }
        // Add dense 5-minute rapid rise + rapid fall + sustained floor hypo on day 28
        for (min, val) in [
            (0, 54),
            (5, 54),
            (10, 54),
            (15, 54),
            (20, 54),
            (25, 62),
            (30, 80),
            (35, 110),
            (40, 145),
            (45, 185),
            (50, 201),
            (55, 201),
        ] {
            csv.push_str(&format!("2025-04-28T01:{min:02}-07:00,{val}\n"));
        }

        let import =
            parse_commercial_glucose_input(&csv, Some(GlucoseMonitor::AbbottLingo)).unwrap();
        let win14 = slice_readings_window(&import.readings, GlucoseWindow::Days14);
        let nom_h = nominal_window_hours_for_trace(&import.readings, GlucoseWindow::Days14);
        let summary = summarize_commercial_glucose_with_window(
            &win14,
            import.monitor,
            import.source_unit,
            import.method,
            nom_h,
        )
        .unwrap();
        let analytics = analyze_glucose_trace(
            &win14,
            import.monitor,
            import.method,
            Some(&import.readings),
        );

        let prompt = build_personal_cgm_chat_prompt_with_analytics(
            "How do my last 14 days compare?",
            import.monitor,
            Some(&summary),
            Some(&import.diagnostics),
            Some(&analytics),
            &[],
            &[],
        );
        assert!(prompt.contains("Window Sensor Saturation Caveat:"));
        assert!(prompt.contains("Sustained Episodes (>=15m):"));
        assert!(prompt.contains("Circadian Periods:"));
        assert!(prompt.contains("14-Day vs Prior 14-Day Comparison:"));
        assert!(prompt.contains("15-Min Velocity (RoC):"));

        let facts = collect_cgm_grounding_facts_with_analytics(
            import.monitor,
            Some(&summary),
            Some(&import.diagnostics),
            Some(&analytics),
            &[],
        );
        assert!(facts.iter().any(|f| f.contains("Window saturation:")));
        assert!(facts.iter().any(|f| f.contains("Sustained ≥15m episodes:")));
        assert!(facts.iter().any(|f| f.contains("Overnight")));
        assert!(facts.iter().any(|f| f.contains("14d trend")));

        let trend_ans = synthesize_grounded_cgm_answer_with_analytics(
            "How do my last 14 days compare to the prior 14 days?",
            import.monitor,
            Some(&summary),
            Some(&import.diagnostics),
            Some(&analytics),
            &[],
        );
        assert!(trend_ans.contains("14-day vs prior 14-day"));
        assert!(trend_ans.contains("Ceiling-cap"));

        let overnight_ans = synthesize_grounded_cgm_answer_with_analytics(
            "How are my overnight glucose and low episodes?",
            import.monitor,
            Some(&summary),
            Some(&import.diagnostics),
            Some(&analytics),
            &[],
        );
        assert!(overnight_ans.contains("Overnight"));
        assert!(overnight_ans.contains("Sustained low episodes"));

        let hypo_ans = synthesize_grounded_cgm_answer_with_analytics(
            "Tell me about my low <70 and <54 hypo episodes",
            import.monitor,
            Some(&summary),
            Some(&import.diagnostics),
            Some(&analytics),
            &[],
        );
        assert!(hypo_ans.contains("hypoglycemia summary"));
        assert!(hypo_ans.contains("Level 2/floor hypo"));

        let sat_ans = synthesize_grounded_cgm_answer_with_analytics(
            "How much is the 201 saturation cap affecting my wear and GMI?",
            import.monitor,
            Some(&summary),
            Some(&import.diagnostics),
            Some(&analytics),
            &[],
        );
        assert!(sat_ans.contains("ceiling-capped >=201"));
        assert!(sat_ans.contains("lower bounds / clipped"));
    }

    #[test]
    fn app_alerts_wire_payloads_deserialize_cleanly() {
        let android_status: AppAlertStatusUi = serde_json::from_str(
            r#"{
                "enabled": true,
                "platform": "android_notification_manager",
                "permission_granted": true,
                "permission_status": "granted",
                "channel_id": "dabney_glucose_alerts"
            }"#,
        )
        .unwrap();
        assert!(android_status.enabled);
        assert!(android_status.permission_granted);
        assert_eq!(android_status.channel_id, "dabney_glucose_alerts");

        let desktop_status: AppAlertStatusUi = serde_json::from_str(
            r#"{
                "enabled": false,
                "platform": "desktop",
                "permission_granted": false,
                "permission_status": "webview_fallback",
                "channel_id": ""
            }"#,
        )
        .unwrap();
        assert!(!desktop_status.enabled);
        assert_eq!(desktop_status.permission_status, "webview_fallback");

        let perm: AppAlertPermissionUi = serde_json::from_str(
            r#"{
                "granted": true,
                "status": "granted"
            }"#,
        )
        .unwrap();
        assert!(perm.granted);
        assert_eq!(perm.status, "granted");

        let req = AppAlertNotificationUi {
            title: "Glucose range alert".to_string(),
            body: "Reading crossed your configured high threshold.".to_string(),
            severity: "urgent".to_string(),
            tag: "glucose-high-urgent".to_string(),
        };
        let encoded_req = serde_json::to_string(&req).unwrap();
        let decoded_req: AppAlertNotificationUi = serde_json::from_str(&encoded_req).unwrap();
        assert_eq!(decoded_req, req);

        let dispatch: AppAlertDispatchUi = serde_json::from_str(
            r#"{
                "delivered": true,
                "notification_id": 123456,
                "detail": "Posted to dabney_glucose_urgent"
            }"#,
        )
        .unwrap();
        assert!(dispatch.delivered);
        assert_eq!(dispatch.notification_id, 123456);
    }
}
