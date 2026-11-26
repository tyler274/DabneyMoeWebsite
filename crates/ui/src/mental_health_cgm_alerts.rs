//! User-configurable target-range glucose alert engine, cooldown/escalation anti-fatigue
//! state machine, browser Web Notifications API bridge, and `localStorage` persistence.
//!
//! 100% pure Rust and `wasm32`-safe with zero network calls or external crate additions.
//! Defaults to privacy-safe lock-screen notification copy so OS/browser notifications never
//! leak raw glucose numbers onto a locked screen unless the user explicitly opts in.

use serde::{Deserialize, Serialize};

use crate::mental_health_cgm::parse_timestamp;
use crate::mental_health_cgm_analytics::RAPID_ROC_THRESHOLD_MG_DL_PER_MIN;
use crate::mental_health_lingo::{GlucoseReadingBound, LingoReading};

/// `localStorage` key used to persist user alert preferences on-device (no PHI stored).
pub const ALERT_CONFIG_STORAGE_KEY: &str = "dabney.cgm.v1.alert_config";

/// Hard lower safety floor for configured glucose thresholds (`mg/dL`).
pub const MIN_ALERT_THRESHOLD_MG_DL: f64 = 45.0;
/// Hard upper safety ceiling for configured glucose thresholds (`mg/dL`).
pub const MAX_ALERT_THRESHOLD_MG_DL: f64 = 350.0;
/// Minimum required separation between `low_mg_dl` and `high_mg_dl`.
pub const MIN_TARGET_RANGE_SPAN_MG_DL: f64 = 10.0;
/// Minimum required monotonic separation between standard and urgent thresholds.
const MIN_URGENT_SEPARATION_MG_DL: f64 = 1.0;

/// Maximum sensor gap allowed inside a contiguous sustained out-of-range run (`minutes`).
const MAX_SUSTAINED_GAP_MINUTES: f64 = 30.0;

/// Minimum/target/maximum elapsed minutes for latest 15-minute rate-of-change calculation.
const ROC_MIN_DT_MINUTES: f64 = 10.0;
const ROC_TARGET_DT_MINUTES: f64 = 15.0;
const ROC_MAX_DT_MINUTES: f64 = 25.0;

/// Proximity buffers for predictive rapid-change alerts (`mg/dL`).
const RAPID_FALL_PROXIMITY_MG_DL: f64 = 25.0;
const RAPID_RISE_PROXIMITY_MG_DL: f64 = 20.0;

/// Clinical target-range preset for automated glucose alerts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum GlucoseTargetPreset {
    /// Tight metabolic target (`70–140 mg/dL`, urgent `<55` / `>180 mg/dL`).
    #[default]
    TightMetabolic,
    /// ADA clinical standard (`70–180 mg/dL`, urgent `<54` / `>250 mg/dL`).
    AdaStandard,
    /// User-customized monotonic target thresholds.
    Custom,
}

impl GlucoseTargetPreset {
    pub fn label(self) -> &'static str {
        match self {
            Self::TightMetabolic => "Tight Metabolic (70–140 mg/dL)",
            Self::AdaStandard => "ADA Standard (70–180 mg/dL)",
            Self::Custom => "Custom Target Range",
        }
    }

    pub fn short_label(self) -> &'static str {
        match self {
            Self::TightMetabolic => "Tight (70–140)",
            Self::AdaStandard => "ADA (70–180)",
            Self::Custom => "Custom",
        }
    }

    pub fn slug(self) -> &'static str {
        match self {
            Self::TightMetabolic => "tight_metabolic",
            Self::AdaStandard => "ada_standard",
            Self::Custom => "custom",
        }
    }

    fn baseline_thresholds(self) -> (f64, f64, f64, f64) {
        match self {
            Self::AdaStandard => (54.0, 70.0, 180.0, 250.0),
            Self::TightMetabolic | Self::Custom => (55.0, 70.0, 140.0, 180.0),
        }
    }
}

/// User-configurable glucose range alert preferences with reliable clinical defaults.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GlucoseAlertConfig {
    pub enabled: bool,
    pub preset: GlucoseTargetPreset,
    pub low_mg_dl: f64,
    pub high_mg_dl: f64,
    pub urgent_low_mg_dl: f64,
    pub urgent_high_mg_dl: f64,
    pub cooldown_minutes: u32,
    pub sustained_minutes: u32,
    pub notify_on_rapid_change: bool,
    pub privacy_safe_lockscreen: bool,
    pub browser_notifications_enabled: bool,
    pub app_notifications_enabled: bool,
    pub auto_poll_minutes: u32,
}

impl Default for GlucoseAlertConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            preset: GlucoseTargetPreset::TightMetabolic,
            low_mg_dl: 70.0,
            high_mg_dl: 140.0,
            urgent_low_mg_dl: 55.0,
            urgent_high_mg_dl: 180.0,
            cooldown_minutes: 30,
            sustained_minutes: 0,
            notify_on_rapid_change: true,
            privacy_safe_lockscreen: true,
            browser_notifications_enabled: false,
            app_notifications_enabled: true,
            auto_poll_minutes: 15,
        }
    }
}

impl GlucoseAlertConfig {
    /// Construct a fresh alert configuration initialized to `preset`.
    pub fn from_preset(preset: GlucoseTargetPreset) -> Self {
        let mut cfg = Self::default();
        cfg.apply_preset(preset);
        cfg
    }

    /// Apply a clinical preset to `self`, updating target & urgent thresholds while keeping
    /// cooldown, sustained-duration, notification channel, and lock-screen privacy preferences.
    pub fn apply_preset(&mut self, preset: GlucoseTargetPreset) {
        self.preset = preset;
        match preset {
            GlucoseTargetPreset::TightMetabolic => {
                self.urgent_low_mg_dl = 55.0;
                self.low_mg_dl = 70.0;
                self.high_mg_dl = 140.0;
                self.urgent_high_mg_dl = 180.0;
            }
            GlucoseTargetPreset::AdaStandard => {
                self.urgent_low_mg_dl = 54.0;
                self.low_mg_dl = 70.0;
                self.high_mg_dl = 180.0;
                self.urgent_high_mg_dl = 250.0;
            }
            GlucoseTargetPreset::Custom => {
                let sanitized = self.sanitized();
                self.urgent_low_mg_dl = sanitized.urgent_low_mg_dl;
                self.low_mg_dl = sanitized.low_mg_dl;
                self.high_mg_dl = sanitized.high_mg_dl;
                self.urgent_high_mg_dl = sanitized.urgent_high_mg_dl;
            }
        }
    }

    /// Return a strictly sanitized copy enforcing finite monotonic clinical safety bounds:
    /// `45.0 <= urgent_low < low < high < urgent_high <= 350.0` with at least `10.0 mg/dL`
    /// separation between `low` and `high`, `cooldown_minutes` in `5..=240`,
    /// `sustained_minutes` in `0..=60`, and `auto_poll_minutes` in `{0, 5, 15, 30}`.
    pub fn sanitized(&self) -> Self {
        let (def_ulow, def_low, def_high, def_uhigh) = self.preset.baseline_thresholds();

        let mut urgent_low = finite_or(self.urgent_low_mg_dl, def_ulow);
        let mut low = finite_or(self.low_mg_dl, def_low);
        let mut high = finite_or(self.high_mg_dl, def_high);
        let mut urgent_high = finite_or(self.urgent_high_mg_dl, def_uhigh);

        // Fix inverted standard target bounds before clamping.
        if low > high {
            std::mem::swap(&mut low, &mut high);
        }

        let min_low = MIN_ALERT_THRESHOLD_MG_DL + MIN_URGENT_SEPARATION_MG_DL;
        let max_low =
            MAX_ALERT_THRESHOLD_MG_DL - MIN_URGENT_SEPARATION_MG_DL - MIN_TARGET_RANGE_SPAN_MG_DL;
        let min_high = min_low + MIN_TARGET_RANGE_SPAN_MG_DL;
        let max_high = MAX_ALERT_THRESHOLD_MG_DL - MIN_URGENT_SEPARATION_MG_DL;

        low = low.clamp(min_low, max_low);
        high = high.clamp(min_high, max_high);

        if high - low < MIN_TARGET_RANGE_SPAN_MG_DL {
            high = (low + MIN_TARGET_RANGE_SPAN_MG_DL).min(max_high);
            low = low.min(high - MIN_TARGET_RANGE_SPAN_MG_DL);
        }

        urgent_low = urgent_low.clamp(MIN_ALERT_THRESHOLD_MG_DL, low - MIN_URGENT_SEPARATION_MG_DL);
        urgent_high = urgent_high.clamp(
            high + MIN_URGENT_SEPARATION_MG_DL,
            MAX_ALERT_THRESHOLD_MG_DL,
        );

        let auto_poll_minutes = match self.auto_poll_minutes {
            0 => 0,
            1..=10 => 5,
            11..=22 => 15,
            _ => 30,
        };

        let preset = match self.preset {
            GlucoseTargetPreset::TightMetabolic
                if matches_thresholds(
                    urgent_low,
                    low,
                    high,
                    urgent_high,
                    (55.0, 70.0, 140.0, 180.0),
                ) =>
            {
                GlucoseTargetPreset::TightMetabolic
            }
            GlucoseTargetPreset::AdaStandard
                if matches_thresholds(
                    urgent_low,
                    low,
                    high,
                    urgent_high,
                    (54.0, 70.0, 180.0, 250.0),
                ) =>
            {
                GlucoseTargetPreset::AdaStandard
            }
            _ => GlucoseTargetPreset::Custom,
        };

        Self {
            enabled: self.enabled,
            preset,
            low_mg_dl: low,
            high_mg_dl: high,
            urgent_low_mg_dl: urgent_low,
            urgent_high_mg_dl: urgent_high,
            cooldown_minutes: self.cooldown_minutes.clamp(5, 240),
            sustained_minutes: self.sustained_minutes.clamp(0, 60),
            notify_on_rapid_change: self.notify_on_rapid_change,
            privacy_safe_lockscreen: self.privacy_safe_lockscreen,
            browser_notifications_enabled: self.browser_notifications_enabled,
            app_notifications_enabled: self.app_notifications_enabled,
            auto_poll_minutes,
        }
    }
}

fn finite_or(val: f64, fallback: f64) -> f64 {
    if val.is_finite() {
        val
    } else {
        fallback
    }
}

fn matches_thresholds(
    urgent_low: f64,
    low: f64,
    high: f64,
    urgent_high: f64,
    expected: (f64, f64, f64, f64),
) -> bool {
    (urgent_low - expected.0).abs() < 1e-6
        && (low - expected.1).abs() < 1e-6
        && (high - expected.2).abs() < 1e-6
        && (urgent_high - expected.3).abs() < 1e-6
}

/// Ordered alert severity levels (`RapidRise < RapidFall < High < Low < UrgentHigh < UrgentLow`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum AlertSeverity {
    RapidRise,
    RapidFall,
    High,
    Low,
    UrgentHigh,
    UrgentLow,
}

impl AlertSeverity {
    pub fn label(self) -> &'static str {
        match self {
            Self::UrgentLow => "Urgent Low Glucose",
            Self::Low => "Low Glucose",
            Self::UrgentHigh => "Urgent High Glucose",
            Self::High => "High Glucose",
            Self::RapidFall => "Rapid Glucose Fall",
            Self::RapidRise => "Rapid Glucose Rise",
        }
    }

    pub fn slug(self) -> &'static str {
        match self {
            Self::UrgentLow => "urgent_low",
            Self::Low => "low",
            Self::UrgentHigh => "urgent_high",
            Self::High => "high",
            Self::RapidFall => "rapid_fall",
            Self::RapidRise => "rapid_rise",
        }
    }

    pub fn is_urgent(self) -> bool {
        matches!(self, Self::UrgentLow | Self::UrgentHigh)
    }

    pub fn is_low(self) -> bool {
        matches!(self, Self::UrgentLow | Self::Low | Self::RapidFall)
    }

    pub fn badge_class(self) -> &'static str {
        match self {
            Self::UrgentLow => "border-rose-400/50 bg-rose-500/20 text-rose-100",
            Self::UrgentHigh => "border-orange-400/50 bg-orange-500/20 text-orange-100",
            Self::Low => "border-rose-400/35 bg-rose-500/15 text-rose-200",
            Self::High => "border-amber-400/35 bg-amber-500/15 text-amber-200",
            Self::RapidFall => "border-sky-400/35 bg-sky-500/15 text-sky-200",
            Self::RapidRise => "border-amber-400/30 bg-amber-500/10 text-amber-200",
        }
    }

    fn direction_slug(self) -> &'static str {
        if self.is_low() {
            "low"
        } else {
            "high"
        }
    }
}

/// Concrete alert event ready for in-app banner rendering and OS/browser notification dispatch.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GlucoseAlertEvent {
    /// Deterministic deduplication key: `"{timestamp}:{severity.slug()}"`.
    pub id: String,
    pub severity: AlertSeverity,
    pub timestamp: String,
    pub glucose_mg_dl: f64,
    pub bound: GlucoseReadingBound,
    pub target_low_mg_dl: f64,
    pub target_high_mg_dl: f64,
    pub roc_mg_dl_per_min: Option<f64>,
    pub in_app_title: String,
    pub in_app_detail: String,
    pub os_title: String,
    pub os_body: String,
    /// Stable notification deduplication tag (e.g. `dabney-cgm-alert-low` / `dabney-cgm-alert-high`).
    pub tag: String,
}

/// Stateful anti-fatigue memory carried across alert evaluations.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AlertDispatchState {
    pub last_dispatched_timestamp: Option<String>,
    pub last_dispatched_utc_minutes: Option<f64>,
    pub last_severity: Option<AlertSeverity>,
}

/// Outcome of evaluating the latest glucose reading against [`GlucoseAlertConfig`] and
/// [`AlertDispatchState`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LatestAlertCheck {
    pub latest_reading_mg_dl: Option<f64>,
    pub latest_timestamp: Option<String>,
    pub latest_bound: GlucoseReadingBound,
    pub in_target_range: bool,
    pub active_alert: Option<GlucoseAlertEvent>,
    pub should_dispatch_os: bool,
    pub suppressed_reason: Option<String>,
    pub next_dispatch_state: AlertDispatchState,
}

/// A contiguous out-of-range breach episode within a summarized trace window.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RangeBreachEpisode {
    pub severity: AlertSeverity,
    pub start_timestamp: String,
    pub end_timestamp: String,
    pub duration_minutes: f64,
    pub reading_count: usize,
    pub extreme_mg_dl: f64,
    pub sensor_capped: bool,
}

/// Custom target-range summary, breach episode timeline, and latest alert check over a window.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowRangeAlertSummary {
    pub target_low_mg_dl: f64,
    pub target_high_mg_dl: f64,
    pub urgent_low_mg_dl: f64,
    pub urgent_high_mg_dl: f64,
    pub reading_count: usize,
    pub time_in_target_pct: f64,
    pub time_below_target_pct: f64,
    pub time_above_target_pct: f64,
    pub urgent_low_readings: usize,
    pub urgent_high_readings: usize,
    pub breaches: Vec<RangeBreachEpisode>,
    pub latest: LatestAlertCheck,
}

/// Format a glucose reading honestly, never hiding sensor floor (`<55`) or ceiling (`>200`) caps.
pub fn format_glucose_reading_honestly(mg_dl: f64, bound: GlucoseReadingBound) -> String {
    match bound {
        GlucoseReadingBound::BelowSensorFloor => {
            if (53.5..=55.5).contains(&mg_dl) {
                "<55 (sensor floor)".to_string()
            } else if (39.5..=40.5).contains(&mg_dl) {
                "<40 (sensor floor)".to_string()
            } else {
                format!("<{:.0} (sensor floor)", mg_dl.ceil())
            }
        }
        GlucoseReadingBound::AboveSensorCeiling => {
            if (199.5..=205.0).contains(&mg_dl) {
                ">200 (sensor ceiling)".to_string()
            } else if (399.5..=405.0).contains(&mg_dl) {
                ">400 (sensor ceiling)".to_string()
            } else if (499.5..=505.0).contains(&mg_dl) {
                ">500 (sensor ceiling)".to_string()
            } else {
                format!(">{:.0} (sensor ceiling)", mg_dl.floor())
            }
        }
        GlucoseReadingBound::Measured => format!("{mg_dl:.0} mg/dL"),
    }
}

fn reading_utc_minutes(reading: &LingoReading) -> Option<f64> {
    parse_timestamp(&reading.timestamp, None).map(|parsed| parsed.utc_minutes)
}

fn reading_clock_minutes(reading: &LingoReading) -> Option<f64> {
    reading_utc_minutes(reading).or_else(|| {
        reading
            .hour_offset
            .is_finite()
            .then_some(reading.hour_offset * 60.0)
    })
}

fn elapsed_minutes_between(earlier: &LingoReading, later: &LingoReading) -> f64 {
    if let (Some(a), Some(b)) = (reading_utc_minutes(earlier), reading_utc_minutes(later)) {
        let dt = b - a;
        if dt.abs() > 1e-6 || (later.hour_offset - earlier.hour_offset).abs() <= 1e-6 {
            return dt;
        }
    }
    (later.hour_offset - earlier.hour_offset) * 60.0
}

/// Classify a single reading against sanitized target & urgent thresholds.
fn classify_threshold_severity(
    reading: &LingoReading,
    cfg: &GlucoseAlertConfig,
) -> Option<AlertSeverity> {
    let g = reading.glucose_mg_dl;
    if !g.is_finite() {
        return None;
    }
    match reading.bound {
        GlucoseReadingBound::BelowSensorFloor => {
            if g <= cfg.urgent_low_mg_dl + 1e-6 {
                Some(AlertSeverity::UrgentLow)
            } else if g <= cfg.low_mg_dl + 1e-6 {
                Some(AlertSeverity::Low)
            } else {
                None
            }
        }
        GlucoseReadingBound::AboveSensorCeiling => {
            if g >= cfg.urgent_high_mg_dl - 1e-6 {
                Some(AlertSeverity::UrgentHigh)
            } else if g >= cfg.high_mg_dl - 1e-6 {
                Some(AlertSeverity::High)
            } else {
                None
            }
        }
        GlucoseReadingBound::Measured => {
            if g < cfg.urgent_low_mg_dl {
                Some(AlertSeverity::UrgentLow)
            } else if g < cfg.low_mg_dl {
                Some(AlertSeverity::Low)
            } else if g > cfg.urgent_high_mg_dl {
                Some(AlertSeverity::UrgentHigh)
            } else if g > cfg.high_mg_dl {
                Some(AlertSeverity::High)
            } else {
                None
            }
        }
    }
}

/// Compute the 15-minute (10–25 min unbroken window) glucose velocity at the latest reading.
fn compute_latest_roc_mg_dl_per_min(readings: &[LingoReading]) -> Option<f64> {
    if readings.len() < 2 {
        return None;
    }
    let latest_idx = readings.len() - 1;
    let latest = &readings[latest_idx];
    if !latest.glucose_mg_dl.is_finite() {
        return None;
    }

    let mut best: Option<(usize, f64)> = None;
    for i in (0..latest_idx).rev() {
        let step_min = elapsed_minutes_between(&readings[i], &readings[i + 1]);
        if !(-1e-6..=ROC_MAX_DT_MINUTES + 1e-6).contains(&step_min) {
            break;
        }
        let dt_min = elapsed_minutes_between(&readings[i], latest);
        if dt_min > ROC_MAX_DT_MINUTES + 1e-6 {
            break;
        }
        if dt_min >= ROC_MIN_DT_MINUTES - 1e-6 {
            match best {
                None => best = Some((i, dt_min)),
                Some((_, prev_dt))
                    if (dt_min - ROC_TARGET_DT_MINUTES).abs()
                        < (prev_dt - ROC_TARGET_DT_MINUTES).abs() =>
                {
                    best = Some((i, dt_min));
                }
                _ => {}
            }
        }
    }

    let (start_idx, dt_min) = best?;
    let start_g = readings[start_idx].glucose_mg_dl;
    if !start_g.is_finite() || dt_min <= 0.0 {
        return None;
    }
    Some((latest.glucose_mg_dl - start_g) / dt_min)
}

/// Compute contiguous sustained out-of-range duration (in minutes) ending at the latest reading
/// for the same polarity (`is_low`) with no sensor gap exceeding 30 minutes.
fn latest_sustained_span_minutes(
    readings: &[LingoReading],
    cfg: &GlucoseAlertConfig,
    is_low_side: bool,
) -> f64 {
    if readings.is_empty() {
        return 0.0;
    }
    let latest_idx = readings.len() - 1;
    let mut start_idx = latest_idx;

    while start_idx > 0 {
        let prev_idx = start_idx - 1;
        let gap_min = elapsed_minutes_between(&readings[prev_idx], &readings[start_idx]);
        if !(-1e-6..=MAX_SUSTAINED_GAP_MINUTES + 1e-6).contains(&gap_min) {
            break;
        }
        match classify_threshold_severity(&readings[prev_idx], cfg) {
            Some(prev_sev) if prev_sev.is_low() == is_low_side => {
                start_idx = prev_idx;
            }
            _ => break,
        }
    }

    elapsed_minutes_between(&readings[start_idx], &readings[latest_idx]).max(0.0)
}

fn build_alert_event(
    reading: &LingoReading,
    severity: AlertSeverity,
    cfg: &GlucoseAlertConfig,
    roc_mg_dl_per_min: Option<f64>,
) -> GlucoseAlertEvent {
    let value_str = format_glucose_reading_honestly(reading.glucose_mg_dl, reading.bound);
    let range_str = format!("{:.0}–{:.0} mg/dL", cfg.low_mg_dl, cfg.high_mg_dl);
    let roc_suffix = roc_mg_dl_per_min
        .map(|r| format!(" (15-min trend {r:+.2} mg/dL/min)"))
        .unwrap_or_default();

    let in_app_title = format!("{} — {value_str}", severity.label());
    let explicit_detail = match severity {
        AlertSeverity::UrgentLow => format!(
            "Latest reading {value_str} is at or below your urgent low threshold ({:.0} mg/dL; target {range_str}){roc_suffix}.",
            cfg.urgent_low_mg_dl
        ),
        AlertSeverity::Low => format!(
            "Latest reading {value_str} is below your {range_str} target range{roc_suffix}."
        ),
        AlertSeverity::UrgentHigh => format!(
            "Latest reading {value_str} is at or above your urgent high threshold ({:.0} mg/dL; target {range_str}){roc_suffix}.",
            cfg.urgent_high_mg_dl
        ),
        AlertSeverity::High => format!(
            "Latest reading {value_str} is above your {range_str} target range{roc_suffix}."
        ),
        AlertSeverity::RapidFall => format!(
            "Latest reading {value_str} is falling rapidly toward your {:.0} mg/dL low threshold{roc_suffix}.",
            cfg.low_mg_dl
        ),
        AlertSeverity::RapidRise => format!(
            "Latest reading {value_str} is rising rapidly toward your {:.0} mg/dL high threshold{roc_suffix}.",
            cfg.high_mg_dl
        ),
    };

    let (os_title, os_body) = if cfg.privacy_safe_lockscreen {
        let title = if severity.is_urgent() {
            "Dabney.moe Urgent Glucose Alert".to_string()
        } else {
            "Dabney.moe Glucose Range Alert".to_string()
        };
        let body = match severity {
            AlertSeverity::UrgentLow | AlertSeverity::UrgentHigh => {
                "Urgent glucose alert triggered for your configured range. Open Dabney.moe to inspect."
                    .to_string()
            }
            AlertSeverity::RapidFall | AlertSeverity::RapidRise => {
                "Rapid glucose change detected near your configured target range. Open Dabney.moe to inspect."
                    .to_string()
            }
            AlertSeverity::Low | AlertSeverity::High => {
                "Glucose moved outside your configured target range. Open Dabney.moe to inspect."
                    .to_string()
            }
        };
        (title, body)
    } else {
        (
            format!("{}: {value_str}", severity.label()),
            explicit_detail.clone(),
        )
    };

    GlucoseAlertEvent {
        id: format!("{}:{}", reading.timestamp, severity.slug()),
        severity,
        timestamp: reading.timestamp.clone(),
        glucose_mg_dl: reading.glucose_mg_dl,
        bound: reading.bound,
        target_low_mg_dl: cfg.low_mg_dl,
        target_high_mg_dl: cfg.high_mg_dl,
        roc_mg_dl_per_min,
        in_app_title,
        in_app_detail: explicit_detail,
        os_title,
        os_body,
        tag: format!("dabney-cgm-alert-{}", severity.direction_slug()),
    }
}

/// Evaluate the newest reading in `readings` against `config.sanitized()` and anti-fatigue
/// cooldown/escalation state `prev_state`.
pub fn evaluate_latest_reading_alert(
    readings: &[LingoReading],
    config: &GlucoseAlertConfig,
    prev_state: &AlertDispatchState,
) -> LatestAlertCheck {
    let cfg = config.sanitized();
    let Some(latest) = readings.last() else {
        return LatestAlertCheck {
            latest_reading_mg_dl: None,
            latest_timestamp: None,
            latest_bound: GlucoseReadingBound::Measured,
            in_target_range: true,
            active_alert: None,
            should_dispatch_os: false,
            suppressed_reason: Some("No glucose readings available.".to_string()),
            next_dispatch_state: prev_state.clone(),
        };
    };

    let threshold_sev = classify_threshold_severity(latest, &cfg);
    let in_target_range = threshold_sev.is_none();

    if !cfg.enabled {
        return LatestAlertCheck {
            latest_reading_mg_dl: Some(latest.glucose_mg_dl),
            latest_timestamp: Some(latest.timestamp.clone()),
            latest_bound: latest.bound,
            in_target_range,
            active_alert: None,
            should_dispatch_os: false,
            suppressed_reason: Some("Glucose range alerts are disabled.".to_string()),
            next_dispatch_state: prev_state.clone(),
        };
    }

    let roc = compute_latest_roc_mg_dl_per_min(readings);

    // Determine candidate alert severity (threshold breach with optional sustained filter, or
    // predictive rapid-change alert when in-range and approaching a target bound).
    let candidate_severity = if let Some(sev) = threshold_sev {
        if cfg.sustained_minutes > 0 {
            let span_min = latest_sustained_span_minutes(readings, &cfg, sev.is_low());
            if span_min < f64::from(cfg.sustained_minutes) - 1e-6 {
                return LatestAlertCheck {
                    latest_reading_mg_dl: Some(latest.glucose_mg_dl),
                    latest_timestamp: Some(latest.timestamp.clone()),
                    latest_bound: latest.bound,
                    in_target_range: false,
                    active_alert: None,
                    should_dispatch_os: false,
                    suppressed_reason: Some(format!(
                        "Out-of-range reading not yet sustained for {} min (current span {:.0} min).",
                        cfg.sustained_minutes, span_min
                    )),
                    next_dispatch_state: prev_state.clone(),
                };
            }
        }
        Some(sev)
    } else if cfg.notify_on_rapid_change {
        match roc {
            Some(r)
                if r <= -RAPID_ROC_THRESHOLD_MG_DL_PER_MIN
                    && latest.glucose_mg_dl <= cfg.low_mg_dl + RAPID_FALL_PROXIMITY_MG_DL =>
            {
                Some(AlertSeverity::RapidFall)
            }
            Some(r)
                if r >= RAPID_ROC_THRESHOLD_MG_DL_PER_MIN
                    && latest.glucose_mg_dl >= cfg.high_mg_dl - RAPID_RISE_PROXIMITY_MG_DL =>
            {
                Some(AlertSeverity::RapidRise)
            }
            _ => None,
        }
    } else {
        None
    };

    let Some(severity) = candidate_severity else {
        return LatestAlertCheck {
            latest_reading_mg_dl: Some(latest.glucose_mg_dl),
            latest_timestamp: Some(latest.timestamp.clone()),
            latest_bound: latest.bound,
            in_target_range,
            active_alert: None,
            should_dispatch_os: false,
            suppressed_reason: None,
            next_dispatch_state: prev_state.clone(),
        };
    };

    let event = build_alert_event(latest, severity, &cfg, roc);
    let curr_clock_min = reading_clock_minutes(latest);

    let (should_dispatch_os, suppressed_reason) =
        evaluate_dispatch_gate(latest, severity, curr_clock_min, &cfg, prev_state);

    let next_dispatch_state = if should_dispatch_os {
        AlertDispatchState {
            last_dispatched_timestamp: Some(latest.timestamp.clone()),
            last_dispatched_utc_minutes: curr_clock_min,
            last_severity: Some(severity),
        }
    } else {
        prev_state.clone()
    };

    LatestAlertCheck {
        latest_reading_mg_dl: Some(latest.glucose_mg_dl),
        latest_timestamp: Some(latest.timestamp.clone()),
        latest_bound: latest.bound,
        in_target_range,
        active_alert: Some(event),
        should_dispatch_os,
        suppressed_reason,
        next_dispatch_state,
    }
}

fn evaluate_dispatch_gate(
    latest: &LingoReading,
    severity: AlertSeverity,
    curr_clock_min: Option<f64>,
    cfg: &GlucoseAlertConfig,
    prev_state: &AlertDispatchState,
) -> (bool, Option<String>) {
    let same_timestamp = prev_state
        .last_dispatched_timestamp
        .as_deref()
        .is_some_and(|prev_ts| prev_ts == latest.timestamp);

    if same_timestamp {
        if let Some(prev_sev) = prev_state.last_severity {
            if severity <= prev_sev {
                return (
                    false,
                    Some("Alert already dispatched for this reading timestamp.".to_string()),
                );
            }
            // Escalated severity for the same timestamp (e.g. thresholds tightened): allow dispatch.
            return (true, None);
        }
        return (
            false,
            Some("Alert already dispatched for this reading timestamp.".to_string()),
        );
    }

    let Some(prev_sev) = prev_state.last_severity else {
        return (true, None);
    };

    // Direction flip (Low <-> High) overrides cooldown immediately.
    if severity.is_low() != prev_sev.is_low() {
        return (true, None);
    }

    // Same-direction escalation (e.g. High -> UrgentHigh, Low -> UrgentLow, RapidFall -> Low)
    // overrides cooldown immediately.
    if severity > prev_sev {
        return (true, None);
    }

    // Same direction and same or lower severity: enforce anti-fatigue cooldown window.
    if let (Some(prev_min), Some(curr_min)) =
        (prev_state.last_dispatched_utc_minutes, curr_clock_min)
    {
        let elapsed = curr_min - prev_min;
        if elapsed < -1e-6 {
            return (
                false,
                Some("Latest reading predates last dispatched alert.".to_string()),
            );
        }
        if elapsed < f64::from(cfg.cooldown_minutes) - 1e-6 {
            return (
                false,
                Some(format!(
                    "Suppressed by {}-min cooldown ({:.0} min elapsed since last {} alert).",
                    cfg.cooldown_minutes,
                    elapsed.max(0.0),
                    prev_sev.label()
                )),
            );
        }
    }

    (true, None)
}

/// Summarize custom target-range compliance, contiguous target-range breach episodes, and the
/// latest reading alert status over a trace window.
pub fn evaluate_window_range_alerts(
    readings: &[LingoReading],
    config: &GlucoseAlertConfig,
    prev_state: &AlertDispatchState,
) -> WindowRangeAlertSummary {
    let cfg = config.sanitized();
    let latest = evaluate_latest_reading_alert(readings, &cfg, prev_state);

    if readings.is_empty() {
        return WindowRangeAlertSummary {
            target_low_mg_dl: cfg.low_mg_dl,
            target_high_mg_dl: cfg.high_mg_dl,
            urgent_low_mg_dl: cfg.urgent_low_mg_dl,
            urgent_high_mg_dl: cfg.urgent_high_mg_dl,
            reading_count: 0,
            time_in_target_pct: 0.0,
            time_below_target_pct: 0.0,
            time_above_target_pct: 0.0,
            urgent_low_readings: 0,
            urgent_high_readings: 0,
            breaches: Vec::new(),
            latest,
        };
    }

    let mut in_target = 0usize;
    let mut below_target = 0usize;
    let mut above_target = 0usize;
    let mut urgent_low_readings = 0usize;
    let mut urgent_high_readings = 0usize;

    struct ActiveBreach {
        is_low: bool,
        severity: AlertSeverity,
        start_idx: usize,
        end_idx: usize,
        count: usize,
        extreme_mg_dl: f64,
        sensor_capped: bool,
    }

    let mut breaches: Vec<RangeBreachEpisode> = Vec::new();
    let mut active: Option<ActiveBreach> = None;

    let flush_breach = |breach: ActiveBreach, out: &mut Vec<RangeBreachEpisode>| {
        let duration_minutes =
            elapsed_minutes_between(&readings[breach.start_idx], &readings[breach.end_idx])
                .max(0.0);
        if cfg.sustained_minutes == 0 || duration_minutes >= f64::from(cfg.sustained_minutes) - 1e-6
        {
            out.push(RangeBreachEpisode {
                severity: breach.severity,
                start_timestamp: readings[breach.start_idx].timestamp.clone(),
                end_timestamp: readings[breach.end_idx].timestamp.clone(),
                duration_minutes,
                reading_count: breach.count,
                extreme_mg_dl: breach.extreme_mg_dl,
                sensor_capped: breach.sensor_capped,
            });
        }
    };

    for (idx, reading) in readings.iter().enumerate() {
        let sev = classify_threshold_severity(reading, &cfg);
        match sev {
            Some(AlertSeverity::UrgentLow) => {
                below_target += 1;
                urgent_low_readings += 1;
            }
            Some(AlertSeverity::Low) => {
                below_target += 1;
            }
            Some(AlertSeverity::UrgentHigh) => {
                above_target += 1;
                urgent_high_readings += 1;
            }
            Some(AlertSeverity::High) => {
                above_target += 1;
            }
            Some(AlertSeverity::RapidFall | AlertSeverity::RapidRise) | None => {
                in_target += 1;
            }
        }

        if idx > 0 {
            let gap_min = elapsed_minutes_between(&readings[idx - 1], reading);
            if !(-1e-6..=MAX_SUSTAINED_GAP_MINUTES + 1e-6).contains(&gap_min) {
                if let Some(breach) = active.take() {
                    flush_breach(breach, &mut breaches);
                }
            }
        }

        match sev {
            Some(s) => {
                let is_low = s.is_low();
                let relevant_cap = if is_low {
                    reading.bound == GlucoseReadingBound::BelowSensorFloor
                } else {
                    reading.bound == GlucoseReadingBound::AboveSensorCeiling
                };
                match active.as_mut() {
                    Some(cur) if cur.is_low == is_low => {
                        cur.end_idx = idx;
                        cur.count += 1;
                        if s > cur.severity {
                            cur.severity = s;
                        }
                        if is_low {
                            cur.extreme_mg_dl = cur.extreme_mg_dl.min(reading.glucose_mg_dl);
                        } else {
                            cur.extreme_mg_dl = cur.extreme_mg_dl.max(reading.glucose_mg_dl);
                        }
                        cur.sensor_capped |= relevant_cap;
                    }
                    _ => {
                        if let Some(prev_breach) = active.take() {
                            flush_breach(prev_breach, &mut breaches);
                        }
                        active = Some(ActiveBreach {
                            is_low,
                            severity: s,
                            start_idx: idx,
                            end_idx: idx,
                            count: 1,
                            extreme_mg_dl: reading.glucose_mg_dl,
                            sensor_capped: relevant_cap,
                        });
                    }
                }
            }
            None => {
                if let Some(prev_breach) = active.take() {
                    flush_breach(prev_breach, &mut breaches);
                }
            }
        }
    }

    if let Some(last_breach) = active.take() {
        flush_breach(last_breach, &mut breaches);
    }

    let n = readings.len() as f64;
    WindowRangeAlertSummary {
        target_low_mg_dl: cfg.low_mg_dl,
        target_high_mg_dl: cfg.high_mg_dl,
        urgent_low_mg_dl: cfg.urgent_low_mg_dl,
        urgent_high_mg_dl: cfg.urgent_high_mg_dl,
        reading_count: readings.len(),
        time_in_target_pct: (in_target as f64 / n) * 100.0,
        time_below_target_pct: (below_target as f64 / n) * 100.0,
        time_above_target_pct: (above_target as f64 / n) * 100.0,
        urgent_low_readings,
        urgent_high_readings,
        breaches,
        latest,
    }
}

/// Build a deterministic synthetic test alert event for verifying browser or native notification
/// delivery without referencing any real patient data.
pub fn build_test_alert_event(config: &GlucoseAlertConfig) -> GlucoseAlertEvent {
    let cfg = config.sanitized();
    let sample_mg_dl = (cfg.high_mg_dl + 15.0)
        .min(cfg.urgent_high_mg_dl - 1.0)
        .max(cfg.high_mg_dl + 1.0);
    let range_str = format!("{:.0}–{:.0} mg/dL", cfg.low_mg_dl, cfg.high_mg_dl);

    let in_app_title = format!("Test Glucose Alert — {sample_mg_dl:.0} mg/dL");
    let in_app_detail = format!(
        "Synthetic test notification for your {range_str} target range (sample {sample_mg_dl:.0} mg/dL; no real personal data)."
    );

    let (os_title, os_body) = if cfg.privacy_safe_lockscreen {
        (
            "Dabney.moe Glucose Range Alert (Test)".to_string(),
            "Test alert delivered. Lock-screen privacy mode hides glucose values. Open Dabney.moe to inspect."
                .to_string(),
        )
    } else {
        (
            format!("Test Glucose Alert: {sample_mg_dl:.0} mg/dL"),
            format!(
                "Synthetic test reading {sample_mg_dl:.0} mg/dL is above your {range_str} target range."
            ),
        )
    };

    GlucoseAlertEvent {
        id: "test-alert:high".to_string(),
        severity: AlertSeverity::High,
        timestamp: "test-alert".to_string(),
        glucose_mg_dl: sample_mg_dl,
        bound: GlucoseReadingBound::Measured,
        target_low_mg_dl: cfg.low_mg_dl,
        target_high_mg_dl: cfg.high_mg_dl,
        roc_mg_dl_per_min: None,
        in_app_title,
        in_app_detail,
        os_title,
        os_body,
        tag: "dabney-cgm-alert-test".to_string(),
    }
}

/// Browser Web Notifications API permission status (available on all targets).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BrowserNotificationPermission {
    Granted,
    Default,
    Denied,
    Unsupported,
}

impl BrowserNotificationPermission {
    pub fn label(self) -> &'static str {
        match self {
            Self::Granted => "Allowed",
            Self::Default => "Not requested",
            Self::Denied => "Blocked",
            Self::Unsupported => "Unsupported",
        }
    }

    pub fn from_js_str(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "granted" => Self::Granted,
            "denied" => Self::Denied,
            "default" => Self::Default,
            _ => Self::Unsupported,
        }
    }
}

/// Parse a JSON-encoded [`GlucoseAlertConfig`] and apply [`GlucoseAlertConfig::sanitized`].
pub fn parse_alert_config_json(raw: &str) -> Option<GlucoseAlertConfig> {
    serde_json::from_str::<GlucoseAlertConfig>(raw)
        .ok()
        .map(|cfg| cfg.sanitized())
}

/// Serialize a sanitized [`GlucoseAlertConfig`] to JSON for local storage persistence.
pub fn serialize_alert_config_json(cfg: &GlucoseAlertConfig) -> Result<String, String> {
    serde_json::to_string(&cfg.sanitized())
        .map_err(|err| format!("Failed to serialize alert config: {err}"))
}

/// Load [`GlucoseAlertConfig`] from browser `localStorage` (`dabney.cgm.v1.alert_config`),
/// falling back to sanitized clinical defaults on SSR or missing/corrupted storage entries.
pub fn load_alert_config_from_storage() -> GlucoseAlertConfig {
    #[cfg(any(feature = "hydrate", feature = "csr"))]
    {
        web_sys::window()
            .and_then(|win| win.local_storage().ok().flatten())
            .and_then(|storage| storage.get_item(ALERT_CONFIG_STORAGE_KEY).ok().flatten())
            .and_then(|raw| parse_alert_config_json(&raw))
            .unwrap_or_default()
    }
    #[cfg(not(any(feature = "hydrate", feature = "csr")))]
    {
        GlucoseAlertConfig::default()
    }
}

/// Persist a sanitized [`GlucoseAlertConfig`] to browser `localStorage` (`dabney.cgm.v1.alert_config`).
/// No-op on SSR targets.
pub fn save_alert_config_to_storage(cfg: &GlucoseAlertConfig) {
    #[cfg(any(feature = "hydrate", feature = "csr"))]
    {
        if let Ok(json) = serialize_alert_config_json(cfg) {
            if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten())
            {
                let _ = storage.set_item(ALERT_CONFIG_STORAGE_KEY, &json);
            }
        }
    }
    #[cfg(not(any(feature = "hydrate", feature = "csr")))]
    {
        let _ = cfg;
    }
}

/// Query `window.Notification.permission` via `js_sys::Reflect`, returning
/// [`BrowserNotificationPermission::Unsupported`] on SSR or when `Notification` is absent.
pub fn query_browser_notification_permission() -> BrowserNotificationPermission {
    #[cfg(any(feature = "hydrate", feature = "csr"))]
    {
        use js_sys::Reflect;
        use wasm_bindgen::JsValue;

        let Some(win) = web_sys::window() else {
            return BrowserNotificationPermission::Unsupported;
        };
        let Ok(notif) = Reflect::get(win.as_ref(), &JsValue::from_str("Notification")) else {
            return BrowserNotificationPermission::Unsupported;
        };
        if notif.is_undefined() || notif.is_null() {
            return BrowserNotificationPermission::Unsupported;
        }
        let Ok(perm) = Reflect::get(&notif, &JsValue::from_str("permission")) else {
            return BrowserNotificationPermission::Unsupported;
        };
        perm.as_string()
            .map(|s| BrowserNotificationPermission::from_js_str(&s))
            .unwrap_or(BrowserNotificationPermission::Unsupported)
    }
    #[cfg(not(any(feature = "hydrate", feature = "csr")))]
    {
        BrowserNotificationPermission::Unsupported
    }
}

/// Request browser Web Notifications permission (`window.Notification.requestPermission()`).
#[cfg(any(feature = "hydrate", feature = "csr"))]
pub fn request_browser_notification_permission<
    F: FnOnce(BrowserNotificationPermission) + 'static,
>(
    on_done: F,
) {
    use js_sys::{Function, Reflect};
    use std::cell::RefCell;
    use std::rc::Rc;
    use wasm_bindgen::{closure::Closure, JsCast, JsValue};

    let Some(win) = web_sys::window() else {
        on_done(BrowserNotificationPermission::Unsupported);
        return;
    };
    let Ok(notif) = Reflect::get(win.as_ref(), &JsValue::from_str("Notification")) else {
        on_done(BrowserNotificationPermission::Unsupported);
        return;
    };
    if notif.is_undefined() || notif.is_null() {
        on_done(BrowserNotificationPermission::Unsupported);
        return;
    }
    let Ok(req_val) = Reflect::get(&notif, &JsValue::from_str("requestPermission")) else {
        on_done(BrowserNotificationPermission::Unsupported);
        return;
    };
    let Some(req_fn) = req_val.dyn_ref::<Function>() else {
        on_done(BrowserNotificationPermission::Unsupported);
        return;
    };

    let Ok(result) = req_fn.call0(&notif) else {
        on_done(query_browser_notification_permission());
        return;
    };

    if let Some(direct) = result.as_string() {
        on_done(BrowserNotificationPermission::from_js_str(&direct));
        return;
    }

    let then_val = Reflect::get(&result, &JsValue::from_str("then")).unwrap_or(JsValue::UNDEFINED);
    let Some(then_fn) = then_val.dyn_ref::<Function>() else {
        on_done(query_browser_notification_permission());
        return;
    };

    let cb_ok = Rc::new(RefCell::new(Some(on_done)));
    let cb_err = Rc::clone(&cb_ok);

    let on_ok = Closure::once_into_js(move |val: JsValue| {
        if let Some(done) = cb_ok.borrow_mut().take() {
            let perm = val
                .as_string()
                .map(|s| BrowserNotificationPermission::from_js_str(&s))
                .unwrap_or_else(query_browser_notification_permission);
            done(perm);
        }
    });

    let on_err = Closure::once_into_js(move |_err: JsValue| {
        if let Some(done) = cb_err.borrow_mut().take() {
            done(query_browser_notification_permission());
        }
    });

    let _ = then_fn.call2(&result, &on_ok, &on_err);
}

/// Dispatch a browser Web Notification via `new Notification(event.os_title, { body, tag, icon })`.
#[cfg(any(feature = "hydrate", feature = "csr"))]
pub fn dispatch_browser_notification(event: &GlucoseAlertEvent) -> Result<(), String> {
    use js_sys::{Array, Function, Object, Reflect, JSON};
    use wasm_bindgen::{JsCast, JsValue};

    match query_browser_notification_permission() {
        BrowserNotificationPermission::Granted => {}
        BrowserNotificationPermission::Default => {
            return Err("Browser notification permission has not been granted yet.".to_string());
        }
        BrowserNotificationPermission::Denied => {
            return Err("Browser notifications are blocked in site settings.".to_string());
        }
        BrowserNotificationPermission::Unsupported => {
            return Err("Browser Notifications API is not supported in this browser.".to_string());
        }
    }

    let win = web_sys::window().ok_or_else(|| "Browser window unavailable.".to_string())?;
    let notif_val = Reflect::get(win.as_ref(), &JsValue::from_str("Notification"))
        .map_err(|_| "Browser Notifications API unavailable.".to_string())?;
    let notif_ctor = notif_val
        .dyn_into::<Function>()
        .map_err(|_| "window.Notification is not a constructor.".to_string())?;

    let options = Object::new();
    let _ = Reflect::set(
        &options,
        &JsValue::from_str("body"),
        &JsValue::from_str(&event.os_body),
    );
    let _ = Reflect::set(
        &options,
        &JsValue::from_str("tag"),
        &JsValue::from_str(&event.tag),
    );
    let _ = Reflect::set(
        &options,
        &JsValue::from_str("icon"),
        &JsValue::from_str("/favicon.png"),
    );

    let args = Array::new();
    args.push(&JsValue::from_str(&event.os_title));
    args.push(&options);

    Reflect::construct(&notif_ctor, &args)
        .map(|_| ())
        .map_err(|err| {
            let detail = err
                .as_string()
                .or_else(|| {
                    Reflect::get(&err, &JsValue::from_str("message"))
                        .ok()
                        .and_then(|m| m.as_string())
                })
                .or_else(|| JSON::stringify(&err).ok().and_then(|s| s.as_string()))
                .unwrap_or_else(|| "unknown error".to_string());
            format!("Failed to display browser notification: {detail}")
        })
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn default_config_uses_reliable_tight_metabolic_bounds_and_privacy_safe_lockscreen() {
        let cfg = GlucoseAlertConfig::default();
        assert!(cfg.enabled);
        assert_eq!(cfg.preset, GlucoseTargetPreset::TightMetabolic);
        assert_eq!(cfg.urgent_low_mg_dl, 55.0);
        assert_eq!(cfg.low_mg_dl, 70.0);
        assert_eq!(cfg.high_mg_dl, 140.0);
        assert_eq!(cfg.urgent_high_mg_dl, 180.0);
        assert_eq!(cfg.cooldown_minutes, 30);
        assert_eq!(cfg.sustained_minutes, 0);
        assert!(cfg.notify_on_rapid_change);
        assert!(cfg.privacy_safe_lockscreen);
        assert!(!cfg.browser_notifications_enabled);
        assert!(cfg.app_notifications_enabled);
        assert_eq!(cfg.auto_poll_minutes, 15);
        assert_eq!(cfg.sanitized(), cfg);
    }

    #[test]
    fn sanitized_repairs_nan_infinity_inverted_and_out_of_bounds_inputs() {
        let corrupted = GlucoseAlertConfig {
            enabled: true,
            preset: GlucoseTargetPreset::TightMetabolic,
            urgent_low_mg_dl: f64::NAN,
            low_mg_dl: 220.0,
            high_mg_dl: 65.0, // inverted relative to low
            urgent_high_mg_dl: f64::INFINITY,
            cooldown_minutes: 0,
            sustained_minutes: 999,
            notify_on_rapid_change: true,
            privacy_safe_lockscreen: true,
            browser_notifications_enabled: false,
            app_notifications_enabled: true,
            auto_poll_minutes: 7,
        };
        let clean = corrupted.sanitized();
        assert!(clean.urgent_low_mg_dl.is_finite());
        assert!(clean.low_mg_dl.is_finite());
        assert!(clean.high_mg_dl.is_finite());
        assert!(clean.urgent_high_mg_dl.is_finite());
        assert!(clean.urgent_low_mg_dl >= 45.0);
        assert!(clean.urgent_low_mg_dl < clean.low_mg_dl);
        assert!(clean.high_mg_dl - clean.low_mg_dl >= 10.0);
        assert!(clean.high_mg_dl < clean.urgent_high_mg_dl);
        assert!(clean.urgent_high_mg_dl <= 350.0);
        assert_eq!(clean.cooldown_minutes, 5);
        assert_eq!(clean.sustained_minutes, 60);
        assert_eq!(clean.auto_poll_minutes, 5);
        // Sanitization is idempotent.
        assert_eq!(clean.sanitized(), clean);

        // Extremely tight/colliding thresholds still maintain minimum 10 mg/dL target span.
        let squeezed = GlucoseAlertConfig {
            urgent_low_mg_dl: 400.0,
            low_mg_dl: 348.0,
            high_mg_dl: 349.0,
            urgent_high_mg_dl: 10.0,
            cooldown_minutes: 500,
            auto_poll_minutes: 99,
            ..Default::default()
        }
        .sanitized();
        assert!(45.0 <= squeezed.urgent_low_mg_dl);
        assert!(squeezed.urgent_low_mg_dl < squeezed.low_mg_dl);
        assert!(squeezed.high_mg_dl - squeezed.low_mg_dl >= 10.0);
        assert!(squeezed.high_mg_dl < squeezed.urgent_high_mg_dl);
        assert!(squeezed.urgent_high_mg_dl <= 350.0);
        assert_eq!(squeezed.cooldown_minutes, 240);
        assert_eq!(squeezed.auto_poll_minutes, 30);
    }

    #[test]
    fn preset_switching_updates_thresholds_while_preserving_user_preferences() {
        let mut cfg = GlucoseAlertConfig {
            cooldown_minutes: 45,
            sustained_minutes: 15,
            privacy_safe_lockscreen: false,
            ..Default::default()
        };
        cfg.apply_preset(GlucoseTargetPreset::AdaStandard);
        assert_eq!(cfg.preset, GlucoseTargetPreset::AdaStandard);
        assert_eq!(cfg.urgent_low_mg_dl, 54.0);
        assert_eq!(cfg.low_mg_dl, 70.0);
        assert_eq!(cfg.high_mg_dl, 180.0);
        assert_eq!(cfg.urgent_high_mg_dl, 250.0);
        assert_eq!(cfg.cooldown_minutes, 45);
        assert_eq!(cfg.sustained_minutes, 15);
        assert!(!cfg.privacy_safe_lockscreen);

        let ada = GlucoseAlertConfig::from_preset(GlucoseTargetPreset::AdaStandard);
        assert_eq!(ada.high_mg_dl, 180.0);
        assert_eq!(ada.urgent_high_mg_dl, 250.0);
        assert_eq!(ada.sanitized().preset, GlucoseTargetPreset::AdaStandard);

        cfg.apply_preset(GlucoseTargetPreset::Custom);
        assert_eq!(cfg.preset, GlucoseTargetPreset::Custom);
        assert_eq!(cfg.high_mg_dl, 180.0);
    }

    #[test]
    fn sensor_floor_and_ceiling_bounds_trigger_and_format_honestly() {
        let cfg = GlucoseAlertConfig {
            privacy_safe_lockscreen: false,
            ..Default::default()
        };

        // Lingo floor sentinel `<55` stored as `54.0` with BelowSensorFloor.
        let floor_trace = vec![synthetic_reading(
            "2026-11-26T02:15:00-07:00",
            0.0,
            54.0,
            GlucoseReadingBound::BelowSensorFloor,
        )];
        let floor_check =
            evaluate_latest_reading_alert(&floor_trace, &cfg, &AlertDispatchState::default());
        assert!(!floor_check.in_target_range);
        assert!(floor_check.should_dispatch_os);
        let floor_ev = floor_check.active_alert.expect("urgent low alert expected");
        assert_eq!(floor_ev.severity, AlertSeverity::UrgentLow);
        assert!(floor_ev.severity.is_urgent());
        assert!(floor_ev.severity.is_low());
        assert!(floor_ev.in_app_title.contains("<55 (sensor floor)"));
        assert!(floor_ev.in_app_detail.contains("<55 (sensor floor)"));
        assert!(floor_ev.os_body.contains("<55 (sensor floor)"));
        assert_eq!(floor_ev.tag, "dabney-cgm-alert-low");

        // Also verifies ADA urgent_low == 54.0 still triggers UrgentLow on BelowSensorFloor 54.0.
        let ada_cfg = GlucoseAlertConfig::from_preset(GlucoseTargetPreset::AdaStandard);
        let ada_floor =
            evaluate_latest_reading_alert(&floor_trace, &ada_cfg, &AlertDispatchState::default());
        assert_eq!(
            ada_floor.active_alert.as_ref().map(|e| e.severity),
            Some(AlertSeverity::UrgentLow)
        );

        // Lingo ceiling sentinel `>200` stored as `201.0` with AboveSensorCeiling.
        let ceil_trace = vec![synthetic_reading(
            "2026-11-26T13:30:00-07:00",
            0.0,
            201.0,
            GlucoseReadingBound::AboveSensorCeiling,
        )];
        let ceil_check =
            evaluate_latest_reading_alert(&ceil_trace, &cfg, &AlertDispatchState::default());
        assert!(!ceil_check.in_target_range);
        let ceil_ev = ceil_check.active_alert.expect("urgent high alert expected");
        assert_eq!(ceil_ev.severity, AlertSeverity::UrgentHigh);
        assert!(ceil_ev.severity.is_urgent());
        assert!(!ceil_ev.severity.is_low());
        assert!(ceil_ev.in_app_title.contains(">200 (sensor ceiling)"));
        assert!(ceil_ev.in_app_detail.contains(">200 (sensor ceiling)"));
        assert!(ceil_ev.os_body.contains(">200 (sensor ceiling)"));
        assert_eq!(ceil_ev.tag, "dabney-cgm-alert-high");
    }

    #[test]
    fn cooldown_suppresses_repeat_alerts_but_escalation_and_direction_flip_override_immediately() {
        let cfg = GlucoseAlertConfig {
            cooldown_minutes: 30,
            ..Default::default()
        };

        // 1. Initial High alert at 12:00 dispatches.
        let r1 = vec![synthetic_reading(
            "2026-11-26T12:00:00Z",
            0.0,
            152.0,
            GlucoseReadingBound::Measured,
        )];
        let c1 = evaluate_latest_reading_alert(&r1, &cfg, &AlertDispatchState::default());
        assert!(c1.should_dispatch_os);
        assert_eq!(
            c1.active_alert.as_ref().unwrap().severity,
            AlertSeverity::High
        );

        // 2. Re-evaluating the exact same timestamp never re-fires OS dispatch.
        let c1_dup = evaluate_latest_reading_alert(&r1, &cfg, &c1.next_dispatch_state);
        assert!(!c1_dup.should_dispatch_os);
        assert!(c1_dup.suppressed_reason.is_some());

        // 3. Another High reading 10 minutes later is suppressed by 30-min cooldown.
        let r2 = vec![
            r1[0].clone(),
            synthetic_reading(
                "2026-11-26T12:10:00Z",
                10.0 / 60.0,
                160.0,
                GlucoseReadingBound::Measured,
            ),
        ];
        let c2 = evaluate_latest_reading_alert(&r2, &cfg, &c1.next_dispatch_state);
        assert!(c2.active_alert.is_some());
        assert!(!c2.should_dispatch_os);
        assert!(c2
            .suppressed_reason
            .as_deref()
            .unwrap_or("")
            .contains("cooldown"));

        // 4. Escalation High -> UrgentHigh at 12:15 overrides cooldown immediately!
        let r3 = vec![
            r2[0].clone(),
            r2[1].clone(),
            synthetic_reading(
                "2026-11-26T12:15:00Z",
                15.0 / 60.0,
                192.0,
                GlucoseReadingBound::Measured,
            ),
        ];
        let c3 = evaluate_latest_reading_alert(&r3, &cfg, &c2.next_dispatch_state);
        assert!(c3.should_dispatch_os);
        assert_eq!(
            c3.active_alert.as_ref().unwrap().severity,
            AlertSeverity::UrgentHigh
        );

        // 5. Direction flip UrgentHigh -> Low at 12:25 overrides cooldown immediately!
        let r4 = vec![
            r3[2].clone(),
            synthetic_reading(
                "2026-11-26T12:25:00Z",
                25.0 / 60.0,
                64.0,
                GlucoseReadingBound::Measured,
            ),
        ];
        let c4 = evaluate_latest_reading_alert(&r4, &cfg, &c3.next_dispatch_state);
        assert!(c4.should_dispatch_os);
        assert_eq!(
            c4.active_alert.as_ref().unwrap().severity,
            AlertSeverity::Low
        );

        // 6. Escalation Low -> UrgentLow at 12:30 overrides cooldown immediately!
        let r5 = vec![
            r4[1].clone(),
            synthetic_reading(
                "2026-11-26T12:30:00Z",
                30.0 / 60.0,
                52.0,
                GlucoseReadingBound::Measured,
            ),
        ];
        let c5 = evaluate_latest_reading_alert(&r5, &cfg, &c4.next_dispatch_state);
        assert!(c5.should_dispatch_os);
        assert_eq!(
            c5.active_alert.as_ref().unwrap().severity,
            AlertSeverity::UrgentLow
        );

        // 7. After 35 minutes (>= 30 min cooldown), another UrgentLow dispatches again.
        let r6 = vec![
            r5[1].clone(),
            synthetic_reading(
                "2026-11-26T13:05:00Z",
                65.0 / 60.0,
                51.0,
                GlucoseReadingBound::Measured,
            ),
        ];
        let c6 = evaluate_latest_reading_alert(&r6, &cfg, &c5.next_dispatch_state);
        assert!(c6.should_dispatch_os);
    }

    #[test]
    fn sustained_minutes_filters_transient_blip_and_sensor_gaps_but_fires_when_contiguous() {
        let cfg = GlucoseAlertConfig {
            sustained_minutes: 15,
            ..Default::default()
        };

        // Single transient blip above 140 after an in-range reading -> suppressed.
        let blip = vec![
            synthetic_reading(
                "2026-11-26T08:00:00Z",
                0.0,
                110.0,
                GlucoseReadingBound::Measured,
            ),
            synthetic_reading(
                "2026-11-26T08:05:00Z",
                5.0 / 60.0,
                155.0,
                GlucoseReadingBound::Measured,
            ),
        ];
        let blip_check = evaluate_latest_reading_alert(&blip, &cfg, &AlertDispatchState::default());
        assert!(!blip_check.in_target_range);
        assert!(blip_check.active_alert.is_none());
        assert!(!blip_check.should_dispatch_os);
        assert!(blip_check
            .suppressed_reason
            .as_deref()
            .unwrap_or("")
            .contains("not yet sustained"));

        // Two high readings separated by a 45-min sensor gap (>30 min) -> NOT contiguous!
        let gapped = vec![
            synthetic_reading(
                "2026-11-26T08:00:00Z",
                0.0,
                155.0,
                GlucoseReadingBound::Measured,
            ),
            synthetic_reading(
                "2026-11-26T08:45:00Z",
                45.0 / 60.0,
                160.0,
                GlucoseReadingBound::Measured,
            ),
        ];
        let gapped_check =
            evaluate_latest_reading_alert(&gapped, &cfg, &AlertDispatchState::default());
        assert!(gapped_check.active_alert.is_none());
        assert!(!gapped_check.should_dispatch_os);

        // Contiguous high readings spanning 15 minutes -> fires!
        let sustained = vec![
            synthetic_reading(
                "2026-11-26T08:00:00Z",
                0.0,
                148.0,
                GlucoseReadingBound::Measured,
            ),
            synthetic_reading(
                "2026-11-26T08:05:00Z",
                5.0 / 60.0,
                154.0,
                GlucoseReadingBound::Measured,
            ),
            synthetic_reading(
                "2026-11-26T08:10:00Z",
                10.0 / 60.0,
                159.0,
                GlucoseReadingBound::Measured,
            ),
            synthetic_reading(
                "2026-11-26T08:15:00Z",
                15.0 / 60.0,
                162.0,
                GlucoseReadingBound::Measured,
            ),
        ];
        let sustained_check =
            evaluate_latest_reading_alert(&sustained, &cfg, &AlertDispatchState::default());
        assert!(sustained_check.should_dispatch_os);
        assert_eq!(
            sustained_check.active_alert.as_ref().unwrap().severity,
            AlertSeverity::High
        );
    }

    #[test]
    fn rapid_fall_and_rapid_rise_trigger_near_target_bounds_and_respect_toggle() {
        let cfg = GlucoseAlertConfig::default();

        // Falling from 125 -> 88 in 15 min (-2.47 mg/dL/min), latest 88 <= 70 + 25 (95).
        let falling = vec![
            synthetic_reading(
                "2026-11-26T09:00:00Z",
                0.0,
                125.0,
                GlucoseReadingBound::Measured,
            ),
            synthetic_reading(
                "2026-11-26T09:05:00Z",
                5.0 / 60.0,
                113.0,
                GlucoseReadingBound::Measured,
            ),
            synthetic_reading(
                "2026-11-26T09:10:00Z",
                10.0 / 60.0,
                100.0,
                GlucoseReadingBound::Measured,
            ),
            synthetic_reading(
                "2026-11-26T09:15:00Z",
                15.0 / 60.0,
                88.0,
                GlucoseReadingBound::Measured,
            ),
        ];
        let fall_check =
            evaluate_latest_reading_alert(&falling, &cfg, &AlertDispatchState::default());
        assert!(fall_check.in_target_range);
        assert!(fall_check.should_dispatch_os);
        let fall_ev = fall_check.active_alert.expect("rapid fall expected");
        assert_eq!(fall_ev.severity, AlertSeverity::RapidFall);
        assert!(fall_ev.roc_mg_dl_per_min.unwrap() <= -2.0);

        // Rising from 92 -> 128 in 15 min (+2.40 mg/dL/min), latest 128 >= 140 - 20 (120).
        let rising = vec![
            synthetic_reading(
                "2026-11-26T10:00:00Z",
                0.0,
                92.0,
                GlucoseReadingBound::Measured,
            ),
            synthetic_reading(
                "2026-11-26T10:05:00Z",
                5.0 / 60.0,
                104.0,
                GlucoseReadingBound::Measured,
            ),
            synthetic_reading(
                "2026-11-26T10:10:00Z",
                10.0 / 60.0,
                116.0,
                GlucoseReadingBound::Measured,
            ),
            synthetic_reading(
                "2026-11-26T10:15:00Z",
                15.0 / 60.0,
                128.0,
                GlucoseReadingBound::Measured,
            ),
        ];
        let rise_check =
            evaluate_latest_reading_alert(&rising, &cfg, &AlertDispatchState::default());
        assert!(rise_check.in_target_range);
        assert!(rise_check.should_dispatch_os);
        let rise_ev = rise_check.active_alert.expect("rapid rise expected");
        assert_eq!(rise_ev.severity, AlertSeverity::RapidRise);
        assert!(rise_ev.roc_mg_dl_per_min.unwrap() >= 2.0);

        // Disabled rapid-change toggle suppresses in-range rapid alerts.
        let no_roc_cfg = GlucoseAlertConfig {
            notify_on_rapid_change: false,
            ..Default::default()
        };
        let suppressed =
            evaluate_latest_reading_alert(&falling, &no_roc_cfg, &AlertDispatchState::default());
        assert!(suppressed.active_alert.is_none());
        assert!(!suppressed.should_dispatch_os);
    }

    #[test]
    fn privacy_safe_lockscreen_hides_values_on_os_while_preserving_in_app_detail() {
        let trace = vec![synthetic_reading(
            "2026-11-26T14:00:00Z",
            0.0,
            168.0,
            GlucoseReadingBound::Measured,
        )];

        // Privacy-safe mode (default true): OS title/body contain NO glucose numbers or thresholds.
        let safe_cfg = GlucoseAlertConfig {
            privacy_safe_lockscreen: true,
            ..Default::default()
        };
        let safe_ev =
            evaluate_latest_reading_alert(&trace, &safe_cfg, &AlertDispatchState::default())
                .active_alert
                .unwrap();
        assert!(!safe_ev.os_title.contains("168"));
        assert!(!safe_ev.os_body.contains("168"));
        assert!(!safe_ev.os_body.contains("140"));
        assert!(!safe_ev.os_body.chars().any(|c| c.is_ascii_digit()));
        assert!(safe_ev.in_app_title.contains("168 mg/dL"));
        assert!(safe_ev.in_app_detail.contains("70–140 mg/dL"));

        // Explicit mode (privacy_safe_lockscreen = false): OS title/body include exact reading & range.
        let explicit_cfg = GlucoseAlertConfig {
            privacy_safe_lockscreen: false,
            ..Default::default()
        };
        let explicit_ev =
            evaluate_latest_reading_alert(&trace, &explicit_cfg, &AlertDispatchState::default())
                .active_alert
                .unwrap();
        assert!(explicit_ev.os_title.contains("168 mg/dL"));
        assert!(explicit_ev.os_body.contains("168 mg/dL"));
        assert!(explicit_ev.os_body.contains("70–140 mg/dL"));

        // Test alert builder also honors privacy_safe_lockscreen.
        let safe_test = build_test_alert_event(&safe_cfg);
        assert!(!safe_test.os_body.chars().any(|c| c.is_ascii_digit()));
        let explicit_test = build_test_alert_event(&explicit_cfg);
        assert!(explicit_test.os_body.contains("155 mg/dL"));
    }

    #[test]
    fn window_range_summary_computes_custom_tir_and_distinct_breach_episodes() {
        let trace = vec![
            synthetic_reading(
                "2026-11-26T08:00:00Z",
                0.0,
                95.0,
                GlucoseReadingBound::Measured,
            ),
            synthetic_reading(
                "2026-11-26T08:05:00Z",
                5.0 / 60.0,
                148.0,
                GlucoseReadingBound::Measured,
            ),
            synthetic_reading(
                "2026-11-26T08:10:00Z",
                10.0 / 60.0,
                201.0,
                GlucoseReadingBound::AboveSensorCeiling,
            ),
            synthetic_reading(
                "2026-11-26T08:20:00Z",
                20.0 / 60.0,
                160.0,
                GlucoseReadingBound::Measured,
            ),
            synthetic_reading(
                "2026-11-26T08:30:00Z",
                30.0 / 60.0,
                102.0,
                GlucoseReadingBound::Measured,
            ),
            synthetic_reading(
                "2026-11-26T09:00:00Z",
                60.0 / 60.0,
                64.0,
                GlucoseReadingBound::Measured,
            ),
            synthetic_reading(
                "2026-11-26T09:15:00Z",
                75.0 / 60.0,
                54.0,
                GlucoseReadingBound::BelowSensorFloor,
            ),
            synthetic_reading(
                "2026-11-26T09:20:00Z",
                80.0 / 60.0,
                90.0,
                GlucoseReadingBound::Measured,
            ),
        ];

        let cfg = GlucoseAlertConfig::default();
        let summary = evaluate_window_range_alerts(&trace, &cfg, &AlertDispatchState::default());

        assert_eq!(summary.reading_count, 8);
        assert_eq!(summary.urgent_high_readings, 1);
        assert_eq!(summary.urgent_low_readings, 1);
        // 3 in-target (95, 102, 90) = 37.5%; 2 below (64, 54) = 25.0%; 3 above (148, 201, 160) = 37.5%.
        assert!((summary.time_in_target_pct - 37.5).abs() < 1e-6);
        assert!((summary.time_below_target_pct - 25.0).abs() < 1e-6);
        assert!((summary.time_above_target_pct - 37.5).abs() < 1e-6);
        assert!(summary.latest.in_target_range);
        assert!(summary.latest.active_alert.is_none());

        assert_eq!(summary.breaches.len(), 2);
        let high_breach = &summary.breaches[0];
        assert_eq!(high_breach.severity, AlertSeverity::UrgentHigh);
        assert_eq!(high_breach.reading_count, 3);
        assert!((high_breach.duration_minutes - 15.0).abs() < 1e-6);
        assert_eq!(high_breach.extreme_mg_dl, 201.0);
        assert!(high_breach.sensor_capped);

        let low_breach = &summary.breaches[1];
        assert_eq!(low_breach.severity, AlertSeverity::UrgentLow);
        assert_eq!(low_breach.reading_count, 2);
        assert!((low_breach.duration_minutes - 15.0).abs() < 1e-6);
        assert_eq!(low_breach.extreme_mg_dl, 54.0);
        assert!(low_breach.sensor_capped);
    }

    #[test]
    fn storage_json_roundtrip_and_ssr_fallbacks_work_safely() {
        let cfg = GlucoseAlertConfig {
            preset: GlucoseTargetPreset::Custom,
            low_mg_dl: 75.0,
            high_mg_dl: 150.0,
            urgent_low_mg_dl: 58.0,
            urgent_high_mg_dl: 210.0,
            cooldown_minutes: 45,
            sustained_minutes: 10,
            ..Default::default()
        };
        let encoded = serialize_alert_config_json(&cfg).unwrap();
        let decoded = parse_alert_config_json(&encoded).unwrap();
        assert_eq!(decoded, cfg.sanitized());

        // Corrupted JSON returns None; missing fields fill from serde defaults.
        assert!(parse_alert_config_json("{bad json").is_none());
        let partial = parse_alert_config_json(r#"{"high_mg_dl":165.0}"#).unwrap();
        assert_eq!(partial.low_mg_dl, 70.0);
        assert_eq!(partial.high_mg_dl, 165.0);
        assert_eq!(partial.preset, GlucoseTargetPreset::Custom);

        // SSR storage and permission queries never panic.
        assert_eq!(
            load_alert_config_from_storage(),
            GlucoseAlertConfig::default()
        );
        save_alert_config_to_storage(&cfg);
        assert_eq!(
            query_browser_notification_permission(),
            BrowserNotificationPermission::Unsupported
        );
        assert_eq!(
            BrowserNotificationPermission::from_js_str("granted"),
            BrowserNotificationPermission::Granted
        );
        assert_eq!(
            BrowserNotificationPermission::from_js_str("denied"),
            BrowserNotificationPermission::Denied
        );
        assert_eq!(
            BrowserNotificationPermission::from_js_str("default"),
            BrowserNotificationPermission::Default
        );
    }
}
