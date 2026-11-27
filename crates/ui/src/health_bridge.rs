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
            Self::AndroidHealthConnect => "READ_BLOOD_GLUCOSE only · stays in device RAM",
            Self::IosHealthKit => {
                "HKQuantityTypeIdentifierBloodGlucose read-only · stays in device RAM"
            }
            Self::Other => "Read-only blood glucose · stays in device RAM",
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
    let mut prompt = String::with_capacity(2048);
    prompt.push_str(
        "You are a concise, calm, local on-device metabolic & wellness reflection assistant inside Dabney.moe Personal.\n\
         Rules:\n\
         - Use ONLY the exact numbers and facts provided below. Never invent readings, labs, or timestamps.\n\
         - Be direct, practical, and easy to read (3-6 short sentences or concise bullets).\n\
         - Never diagnose medical conditions or prescribe dosing changes; frame observations as questions to discuss with a clinician.\n\
         - If sensor saturation (<55 or >200 mg/dL clipping) or low wear-time is noted, mention it honestly.\n\n",
    );

    prompt.push_str(&format!("Monitor: {}\n", monitor.display_name()));

    if let Some(s) = summary {
        prompt.push_str(&format!(
            "CGM Metrics ({} readings, {:.0}% active wear):\n\
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

    if let Some(d) = diagnostics {
        prompt.push_str(&format!(
            "Import Validation: {} readings across {:.1} days (median cadence {:.1} min, longest gap {:.1} h, sensor-cap sentinels: {}).\n",
            d.parsed_readings,
            d.span_hours / 24.0,
            d.median_interval_minutes.unwrap_or(0.0),
            d.longest_gap_minutes.unwrap_or(0.0) / 60.0,
            d.bounded_sentinels,
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
    }
    if let Some(d) = diagnostics {
        if d.bounded_sentinels > 0 {
            facts.push(format!(
                "{} sensor-cap reading(s) detected (biosensor floor/ceiling)",
                d.bounded_sentinels
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

    if q.contains("doctor") || q.contains("visit") || q.contains("question") {
        let cap_note = diagnostics
            .filter(|d| d.bounded_sentinels > 0)
            .map(|d| {
                format!(
                    " Note: {} reading(s) hit the biosensor floor/ceiling cap.",
                    d.bounded_sentinels
                )
            })
            .unwrap_or_default();
        return format!(
            "Suggested visit questions grounded in your {} window ({} readings):\n\
             1. My mean glucose is {:.0} mg/dL (GMI {:.1}%) with {:.0}% TIR (70–180) and {:.0}% tight range (70–140)—does this align with my personal target?\n\
             2. My 24-hour AGP peaks around {} with {} spike reading(s) >140 mg/dL—what meal timing, fiber/protein pairing, or post-meal walk experiments would you suggest?\n\
             3. My glycemic variability is {:.1}% CV (target <36%) and GRI is {:.0}/100 ({}).{} (Not medical advice.)",
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
            cap_note
        );
    }

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
        return format!(
            "Across {} {} readings, there are {} readings >140 mg/dL (range {:.0}–{:.0} mg/dL, MAGE {:.0} mg/dL). \
             Your highest local 24-hour AGP clock hours are: {}. \
             Tight time-in-range (70–140 mg/dL) is {:.0}% and ADA 70–180 TIR is {:.0}%.",
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

    if q.contains("mood")
        || q.contains("anxiety")
        || q.contains("sleep")
        || q.contains("phq")
        || q.contains("gad")
        || q.contains("mental")
    {
        if checkins.is_empty() {
            return format!(
                "Your {} trace shows {:.0} mg/dL mean (CV {:.1}%, {} spikes >140), but no mood or anxiety check-ins are logged yet. Add check-ins in the form below to correlate glycemic swings with mood, anxiety, and sleep locally.",
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
        return format!(
            "Across {} local check-ins paired with your {} trace (mean {:.0} mg/dL, CV {:.1}%, {} spikes >140): \
             average mood is {:.1}/10, anxiety {:.1}/10, and sleep {:.1}h. \
             Lowest logged mood ({}/10, anxiety {}/10) was at [{}]: \"{}\". \
             Consider tracking post-meal focus/anxiety 60–90 minutes after your peak AGP hour ({}).",
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

    let sat_note = diagnostics
        .filter(|d| d.bounded_sentinels > 0)
        .map(|d| {
            format!(
                " Biosensor caveat: {} reading(s) hit the floor/ceiling cap, so true excursions past those limits may be clipped.",
                d.bounded_sentinels
            )
        })
        .unwrap_or_default();

    format!(
        "{} local summary ({} readings, {:.0}% active wear): mean glucose {:.0} mg/dL (median {:.0}, range {:.0}–{:.0}), \
         GMI {:.1}%, ADA TIR (70–180) {:.0}%, tight range (70–140) {:.0}%, CV {:.1}% (target <36%), and GRI {:.0}/100 ({}). \
         Peak 24-hour AGP local hour is {} with {} reading(s) >140 mg/dL.{}",
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
        sat_note,
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
}
