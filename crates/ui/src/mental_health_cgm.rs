//! Pure-Rust commercial continuous glucose monitor (CGM) & blood glucose meter (BGM)
//! parser, auto-detector, ADA/ATTD clinical analytics engine (5-band TIR, GMI, GRI, MAGE,
//! 24-hour Ambulatory Glucose Profile), validation diagnostics, and SVG visualizations.
//!
//! Supports commercial CSV/TSV exports (including Abbott Lingo 5-minute ISO-8601 offset CSVs),
//! Apple Health `export.xml` (`HKQuantityTypeIdentifierBloodGlucose`), and the clean-room
//! `dabney.health_bridge.v1` / Android Health Connect `BloodGlucoseRecord` JSON schema.
//!
//! Runs 100% locally in the browser (WASM) or offline CLI with zero external uploads.

use chrono::NaiveDate;
use leptos::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use crate::mental_health_lingo::GlucoseReadingBound;
use crate::mental_health_lingo::{
    sample_checkins, sample_lingo_6mo_checkins, split_delimited_row, LingoReading,
    MentalHealthCheckIn, SAMPLE_LINGO_6MO_CSV, SAMPLE_LINGO_CSV,
};

const MMOL_TO_MG_DL: f64 = 18.0182;
const MIN_MG_DL: f64 = 20.0;
const MAX_MG_DL: f64 = 500.0;
const LINGO_FLOOR_MG_DL: f64 = 55.0;
const LINGO_CEILING_MG_DL: f64 = 200.0;
const MAX_SAMPLE_WARNINGS: usize = 6;

/// Canonical versioned JSON schema shared between offline validation, Android Health Connect,
/// and Apple HealthKit native bridges.
pub const HEALTH_BRIDGE_SCHEMA_V1: &str = "dabney.health_bridge.v1";

/// Supported commercial CGM biosensors, pump ecosystems, aggregators, and fingerstick BGMs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GlucoseMonitor {
    AbbottLingo,
    AbbottFreeStyleLibre,
    DexcomClarity,
    DexcomStelo,
    MedtronicCareLink,
    SenseonicsEversense,
    TandemSource,
    GlookoTidepool,
    FingerstickBgm,
    GenericCsv,
}

impl GlucoseMonitor {
    pub const ALL: [Self; 10] = [
        Self::AbbottLingo,
        Self::AbbottFreeStyleLibre,
        Self::DexcomClarity,
        Self::DexcomStelo,
        Self::MedtronicCareLink,
        Self::SenseonicsEversense,
        Self::TandemSource,
        Self::GlookoTidepool,
        Self::FingerstickBgm,
        Self::GenericCsv,
    ];

    pub fn display_name(self) -> &'static str {
        match self {
            Self::AbbottLingo => "Abbott Lingo",
            Self::AbbottFreeStyleLibre => "Abbott FreeStyle Libre (LibreView)",
            Self::DexcomClarity => "Dexcom G6 / G7 / ONE+ (Clarity)",
            Self::DexcomStelo => "Dexcom Stelo",
            Self::MedtronicCareLink => "Medtronic Guardian / Simplera (CareLink)",
            Self::SenseonicsEversense => "Senseonics Eversense E3 / 365",
            Self::TandemSource => "Tandem t:slim X2 / Mobi (Source)",
            Self::GlookoTidepool => "Omnipod 5 / Glooko / Tidepool",
            Self::FingerstickBgm => "Fingerstick BGM (Accu-Chek / OneTouch / Contour)",
            Self::GenericCsv => "Apple Health / Nightscout / Generic CSV",
        }
    }

    pub fn slug(self) -> &'static str {
        match self {
            Self::AbbottLingo => "abbott-lingo",
            Self::AbbottFreeStyleLibre => "freestyle-libre",
            Self::DexcomClarity => "dexcom-clarity",
            Self::DexcomStelo => "dexcom-stelo",
            Self::MedtronicCareLink => "medtronic-carelink",
            Self::SenseonicsEversense => "senseonics-eversense",
            Self::TandemSource => "tandem-source",
            Self::GlookoTidepool => "glooko-tidepool",
            Self::FingerstickBgm => "fingerstick-bgm",
            Self::GenericCsv => "generic-csv",
        }
    }

    pub fn from_slug(slug: &str) -> Option<Self> {
        match slug {
            "abbott-lingo" => Some(Self::AbbottLingo),
            "freestyle-libre" => Some(Self::AbbottFreeStyleLibre),
            "dexcom-clarity" => Some(Self::DexcomClarity),
            "dexcom-stelo" => Some(Self::DexcomStelo),
            "medtronic-carelink" => Some(Self::MedtronicCareLink),
            "senseonics-eversense" => Some(Self::SenseonicsEversense),
            "tandem-source" => Some(Self::TandemSource),
            "glooko-tidepool" => Some(Self::GlookoTidepool),
            "fingerstick-bgm" => Some(Self::FingerstickBgm),
            "generic-csv" => Some(Self::GenericCsv),
            _ => None,
        }
    }

    pub fn default_method(self) -> MeasurementMethod {
        match self {
            Self::FingerstickBgm => MeasurementMethod::CapillaryBgm,
            Self::AbbottLingo
            | Self::AbbottFreeStyleLibre
            | Self::DexcomClarity
            | Self::DexcomStelo
            | Self::MedtronicCareLink
            | Self::SenseonicsEversense
            | Self::TandemSource
            | Self::GlookoTidepool
            | Self::GenericCsv => MeasurementMethod::InterstitialCgm,
        }
    }
}

/// Native glucose unit detected in the imported export.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GlucoseUnit {
    MgDl,
    MmolL,
}

impl GlucoseUnit {
    pub fn label(self) -> &'static str {
        match self {
            Self::MgDl => "mg/dL",
            Self::MmolL => "mmol/L",
        }
    }

    pub fn to_mg_dl(self, value: f64) -> f64 {
        match self {
            Self::MgDl => value,
            Self::MmolL => value * MMOL_TO_MG_DL,
        }
    }
}

/// Physiological sampling method.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MeasurementMethod {
    InterstitialCgm,
    CapillaryBgm,
}

impl MeasurementMethod {
    pub fn label(self) -> &'static str {
        match self {
            Self::InterstitialCgm => "Interstitial CGM",
            Self::CapillaryBgm => "Capillary BGM",
        }
    }
}

/// Glycemia Risk Index clinical zone (Klonoff et al. 2022, `0..=100`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GriZone {
    ZoneA,
    ZoneB,
    ZoneC,
    ZoneD,
    ZoneE,
}

impl GriZone {
    pub fn from_score(score: f64) -> Self {
        if score <= 20.0 {
            Self::ZoneA
        } else if score <= 40.0 {
            Self::ZoneB
        } else if score <= 60.0 {
            Self::ZoneC
        } else if score <= 80.0 {
            Self::ZoneD
        } else {
            Self::ZoneE
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::ZoneA => "Zone A (Optimal)",
            Self::ZoneB => "Zone B (Low Risk)",
            Self::ZoneC => "Zone C (Moderate)",
            Self::ZoneD => "Zone D (Elevated)",
            Self::ZoneE => "Zone E (High Risk)",
        }
    }
}

/// One clock hour (`0..24`) in a 24-hour Ambulatory Glucose Profile (AGP).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AgpHourBin {
    pub hour: u8,
    pub count: usize,
    pub p5: f64,
    pub p25: f64,
    pub p50: f64,
    pub p75: f64,
    pub p95: f64,
}

/// Detected serialization format of an imported glucose payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImportPayloadFormat {
    Csv,
    AppleHealthXml,
    HealthBridgeJson,
}

impl ImportPayloadFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::Csv => "Commercial CSV / TSV",
            Self::AppleHealthXml => "Apple Health export.xml",
            Self::HealthBridgeJson => "Health Connect / HealthKit JSON",
        }
    }
}

/// Outcome level for an individual import validation check.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ValidationLevel {
    Pass,
    Warn,
    Info,
}

/// Structured diagnostic check surfaced after parsing a CGM/BGM export.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationCheck {
    pub name: String,
    pub level: ValidationLevel,
    pub detail: String,
}

/// Detailed validation diagnostics for verifying personal CGM exports (including Abbott Lingo).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GlucoseImportDiagnostics {
    pub format: ImportPayloadFormat,
    pub total_rows_seen: usize,
    pub parsed_readings: usize,
    pub skipped_non_glucose_rows: usize,
    pub skipped_invalid_rows: usize,
    /// Non-numeric sensor status rows such as Abbott Lingo `"Out of range"` (tracked separately
    /// from malformed CSV/XML rows).
    #[serde(default)]
    pub out_of_range_rows: usize,
    pub duplicate_timestamps_dropped: usize,
    pub bounded_sentinels: usize,
    pub timestamps_parsed: usize,
    pub timestamps_unparsed: usize,
    pub first_timestamp: Option<String>,
    pub last_timestamp: Option<String>,
    pub span_hours: f64,
    pub median_interval_minutes: Option<f64>,
    pub longest_gap_minutes: Option<f64>,
    pub detected_sources: Vec<String>,
    pub sample_warnings: Vec<String>,
    pub checks: Vec<ValidationCheck>,
}

/// Analysis / visualization time window over an imported glucose trace.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GlucoseWindow {
    Hours24,
    Days3,
    Days7,
    Days14,
    Days30,
    Days90,
    All,
}

impl GlucoseWindow {
    pub const ALL: [Self; 7] = [
        Self::Hours24,
        Self::Days3,
        Self::Days7,
        Self::Days14,
        Self::Days30,
        Self::Days90,
        Self::All,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Hours24 => "24h",
            Self::Days3 => "3d",
            Self::Days7 => "7d",
            Self::Days14 => "14d",
            Self::Days30 => "30d",
            Self::Days90 => "90d",
            Self::All => "All",
        }
    }

    /// The window a freshly loaded trace opens in: the ATTD 14-day view for a trace longer than
    /// 15 days, so the charts and AGP stay readable, and the whole trace otherwise.
    pub fn default_for_trace(readings: &[LingoReading]) -> Self {
        let span_h = readings
            .first()
            .zip(readings.last())
            .map_or(0.0, |(first, last)| last.hour_offset - first.hour_offset);
        if span_h > 360.0 {
            Self::Days14
        } else {
            Self::All
        }
    }

    pub fn max_hours(self) -> Option<f64> {
        match self {
            Self::Hours24 => Some(24.0),
            Self::Days3 => Some(72.0),
            Self::Days7 => Some(168.0),
            Self::Days14 => Some(336.0),
            Self::Days30 => Some(720.0),
            Self::Days90 => Some(2160.0),
            Self::All => None,
        }
    }
}

/// Clean-room versioned JSON interchange envelope shared by offline validation,
/// future Android Health Connect (`BloodGlucoseRecord`), and Apple HealthKit bridges.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HealthBridgeEnvelope {
    pub schema: String,
    pub platform: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exported_at: Option<String>,
    pub records: Vec<HealthBridgeGlucoseRecord>,
}

/// Single blood glucose record inside [`HealthBridgeEnvelope`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HealthBridgeGlucoseRecord {
    pub time: String,
    #[serde(
        rename = "zoneOffset",
        alias = "zone_offset",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub zone_offset: Option<String>,
    #[serde(
        rename = "levelMgDl",
        alias = "level_mg_dl",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub level_mg_dl: Option<f64>,
    #[serde(
        rename = "levelMmolL",
        alias = "level_mmol_l",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub level_mmol_l: Option<f64>,
    #[serde(
        rename = "relationToMeal",
        alias = "relation_to_meal",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub relation_to_meal: Option<String>,
    #[serde(
        rename = "specimenSource",
        alias = "specimen_source",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub specimen_source: Option<String>,
    #[serde(
        rename = "dataOrigin",
        alias = "data_origin",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub data_origin: Option<String>,
    #[serde(
        rename = "lingoCount",
        alias = "lingo_count",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub lingo_count: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bound: Option<String>,
}

/// Parsed commercial glucose export ready for visualization and clinical analytics.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CommercialGlucoseImport {
    pub monitor: GlucoseMonitor,
    pub source_unit: GlucoseUnit,
    pub method: MeasurementMethod,
    pub readings: Vec<LingoReading>,
    pub bounded_sentinels: usize,
    pub diagnostics: GlucoseImportDiagnostics,
}

/// Comprehensive ADA/ATTD clinical glucose analytics over a parsed commercial monitor trace.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ClinicalGlucoseSummary {
    pub monitor: GlucoseMonitor,
    pub source_unit: GlucoseUnit,
    pub method: MeasurementMethod,
    pub reading_count: usize,
    pub mean_mg_dl: f64,
    pub median_mg_dl: f64,
    pub min_mg_dl: f64,
    pub max_mg_dl: f64,
    pub std_dev_mg_dl: f64,
    pub cv_percent: f64,
    /// Glucose Management Indicator (`3.31 + 0.02392 * mean_mg_dl`).
    pub gmi_percent: f64,
    /// Glycemia Risk Index (`0..=100`).
    pub gri_score: f64,
    pub gri_zone: GriZone,
    /// Mean Amplitude of Glycemic Excursions (`> 1 SD`, `mg/dL`).
    pub mage_mg_dl: f64,
    /// Level 2 Hypoglycemia (`< 54 mg/dL`, ADA target `< 1%`).
    pub tbr_very_low_lt_54_pct: f64,
    /// Level 1 Hypoglycemia (`54–69 mg/dL`, ADA combined `< 70` target `< 4%`).
    pub tbr_low_54_69_pct: f64,
    /// Time in Tight Range / Lingo metabolic zone (`70–140 mg/dL`).
    pub titr_70_140_pct: f64,
    /// Standard Clinical Time in Range (`70–180 mg/dL`, ADA target `>= 70%`).
    pub tir_70_180_pct: f64,
    /// Level 1 Hyperglycemia (`181–250 mg/dL`, ADA combined `> 180` target `< 25%`).
    pub tar_high_181_250_pct: f64,
    /// Level 2 Hyperglycemia (`> 250 mg/dL`, ADA target `< 5%`).
    pub tar_very_high_gt_250_pct: f64,
    pub spike_readings_gt_140: usize,
    pub high_readings_gt_180: usize,
    pub low_readings_lt_70: usize,
    pub very_low_readings_lt_54: usize,
    /// Number of readings in this window clipped at the sensor floor (e.g., Lingo `<55 mg/dL`).
    pub floor_capped_readings: usize,
    /// Number of readings in this window clipped at the sensor ceiling (e.g., Lingo `>200 mg/dL`).
    pub ceiling_capped_readings: usize,
    /// Percentage of readings in this window clipped at the sensor floor.
    pub floor_capped_pct: f64,
    /// Percentage of readings in this window clipped at the sensor ceiling.
    pub ceiling_capped_pct: f64,
    pub total_lingo_count: f64,
    pub estimated_active_wear_pct: f64,
    pub agp_hourly: Vec<AgpHourBin>,
}

impl ClinicalGlucoseSummary {
    /// Whether any readings in this summarized window hit a sensor floor or ceiling saturation cap.
    pub fn has_sensor_saturation(&self) -> bool {
        self.floor_capped_readings > 0 || self.ceiling_capped_readings > 0
    }
}

/// Resolve the effective sensor bound for a reading, including legacy Lingo traces loaded
/// without an explicit `bound` field (`<= 54.5` floor / `>= 200.5` ceiling).
pub(crate) fn effective_reading_bound(
    reading: &LingoReading,
    monitor: GlucoseMonitor,
) -> GlucoseReadingBound {
    if reading.bound != GlucoseReadingBound::Measured {
        reading.bound
    } else if monitor == GlucoseMonitor::AbbottLingo
        && reading.glucose_mg_dl <= LINGO_FLOOR_MG_DL - 0.5
    {
        GlucoseReadingBound::BelowSensorFloor
    } else if monitor == GlucoseMonitor::AbbottLingo
        && reading.glucose_mg_dl >= LINGO_CEILING_MG_DL + 0.5
    {
        GlucoseReadingBound::AboveSensorCeiling
    } else {
        GlucoseReadingBound::Measured
    }
}

/// Infer a monitor hint from an uploaded file's name when the user left format on Auto-detect.
pub fn monitor_hint_from_filename(filename: &str) -> Option<GlucoseMonitor> {
    let lower = filename.to_ascii_lowercase();
    if lower.contains("lingo") {
        Some(GlucoseMonitor::AbbottLingo)
    } else if lower.contains("libreview") || lower.contains("freestyle") || lower.contains("libre")
    {
        Some(GlucoseMonitor::AbbottFreeStyleLibre)
    } else if lower.contains("stelo") {
        Some(GlucoseMonitor::DexcomStelo)
    } else if lower.contains("clarity") || lower.contains("dexcom") {
        Some(GlucoseMonitor::DexcomClarity)
    } else if lower.contains("carelink") || lower.contains("medtronic") {
        Some(GlucoseMonitor::MedtronicCareLink)
    } else if lower.contains("eversense") {
        Some(GlucoseMonitor::SenseonicsEversense)
    } else if lower.contains("tandem") {
        Some(GlucoseMonitor::TandemSource)
    } else if lower.contains("glooko") || lower.contains("tidepool") || lower.contains("omnipod") {
        Some(GlucoseMonitor::GlookoTidepool)
    } else {
        None
    }
}

/// Slice a chronologically ordered glucose trace to the trailing [`GlucoseWindow`],
/// rebasing `hour_offset` to `0.0` at the start of the selected window.
pub fn slice_readings_window(
    readings: &[LingoReading],
    window: GlucoseWindow,
) -> Vec<LingoReading> {
    let Some(start_h) = window_start_hour(readings, window) else {
        return Vec::new();
    };
    readings
        .iter()
        .filter(|r| r.hour_offset >= start_h - 1e-6)
        .cloned()
        .map(|mut r| {
            r.hour_offset = (r.hour_offset - start_h).max(0.0);
            r
        })
        .collect()
}

/// Slice check-ins to the same trailing [`GlucoseWindow`] as [`slice_readings_window`] over
/// `readings`, rebased onto the same `0.0` start, so both share one timeline. Without readings
/// there is no window to place them in, and every check-in is kept as it is.
pub fn slice_checkins_window(
    checkins: &[MentalHealthCheckIn],
    readings: &[LingoReading],
    window: GlucoseWindow,
) -> Vec<MentalHealthCheckIn> {
    let Some(start_h) = window_start_hour(readings, window) else {
        return checkins.to_vec();
    };
    checkins
        .iter()
        .filter(|c| c.hour_offset >= start_h - 1e-6)
        .cloned()
        .map(|mut c| {
            c.hour_offset = (c.hour_offset - start_h).max(0.0);
            c
        })
        .collect()
}

/// Where `window` starts on the trace's own hour axis: the window's length before the last
/// reading, or the first reading when the trace is shorter than the window or the window is
/// [`GlucoseWindow::All`]. A trailing window that opens inside a sensor gap still starts at its
/// cutoff, so the gap shows as empty time rather than shortening the window. None for an empty
/// trace.
fn window_start_hour(readings: &[LingoReading], window: GlucoseWindow) -> Option<f64> {
    let first_h = readings.first()?.hour_offset;
    let end_h = readings.last()?.hour_offset;
    Some(
        window
            .max_hours()
            .map_or(first_h, |max_h| (end_h - max_h).max(first_h)),
    )
}

/// The hours a glucose timeline spans from `0.0`: through its last reading or check-in, whichever
/// is later, and never less than one hour so a lone reading still has room.
pub fn timeline_hour_span(readings: &[LingoReading], checkins: &[MentalHealthCheckIn]) -> f64 {
    readings
        .iter()
        .map(|r| r.hour_offset)
        .chain(checkins.iter().map(|c| c.hour_offset))
        .filter(|h| h.is_finite())
        .fold(1.0_f64, f64::max)
}

/// The glucose values (mg/dL) every chart's value axis spans: from 40, or 10 below the lowest
/// value, to 200, or 10 above the highest, each rounded out to a multiple of 10. `values` carries
/// the readings and every reference the chart draws (target bounds, the 180 line), so nothing a
/// chart shows falls outside its axis.
pub fn glucose_axis_domain(values: impl IntoIterator<Item = f64>) -> (f64, f64) {
    let (lowest, highest) = values
        .into_iter()
        .filter(|v| v.is_finite())
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| {
            (lo.min(v), hi.max(v))
        });
    let floor = (((lowest - 10.0) / 10.0).floor() * 10.0).clamp(0.0, 40.0);
    let ceiling = (((highest + 10.0) / 10.0).ceil() * 10.0).max(200.0);
    (floor, ceiling)
}

/// Round glucose gridline values inside `domain`: every 50 mg/dL, or every 100 across a span wider
/// than 300 mg/dL.
pub fn glucose_axis_ticks(domain: (f64, f64)) -> Vec<f64> {
    let step = if domain.1 - domain.0 > 300.0 {
        100.0
    } else {
        50.0
    };
    let mut tick = (domain.0 / step).ceil() * step;
    let mut ticks = Vec::new();
    while tick <= domain.1 {
        ticks.push(tick);
        tick += step;
    }
    ticks
}

/// Downsample a large multi-week CGM trace for smooth SVG / WebGL2 rendering while preserving
/// every bucket's visual minimum and maximum (so spikes and dips are never smoothed away).
pub fn downsample_readings_for_chart(
    readings: &[LingoReading],
    max_points: usize,
) -> Vec<LingoReading> {
    if readings.len() <= max_points || max_points < 4 {
        return readings.to_vec();
    }
    let interior = &readings[1..readings.len() - 1];
    let buckets = ((max_points - 2) / 2).max(1);
    let mut out = Vec::with_capacity(max_points);
    out.push(readings[0].clone());

    for bucket_idx in 0..buckets {
        let start = bucket_idx * interior.len() / buckets;
        let end = ((bucket_idx + 1) * interior.len() / buckets).max(start + 1);
        let slice = &interior[start..end.min(interior.len())];
        if slice.is_empty() {
            continue;
        }
        let mut min_i = 0usize;
        let mut max_i = 0usize;
        for (i, r) in slice.iter().enumerate() {
            if r.glucose_mg_dl < slice[min_i].glucose_mg_dl {
                min_i = i;
            }
            if r.glucose_mg_dl > slice[max_i].glucose_mg_dl {
                max_i = i;
            }
        }
        if min_i == max_i {
            out.push(slice[0].clone());
            if slice.len() > 1 {
                out.push(slice[slice.len() - 1].clone());
            }
        } else if min_i < max_i {
            out.push(slice[min_i].clone());
            out.push(slice[max_i].clone());
        } else {
            out.push(slice[max_i].clone());
            out.push(slice[min_i].clone());
        }
    }

    out.push(readings[readings.len() - 1].clone());
    out
}

/// Select one representative peak per contiguous excursion above `140 mg/dL` so multi-week
/// charts highlight distinct spikes cleanly without flooding the DOM or GPU with duplicate
/// markers on flat ceiling plateaus (e.g., Abbott Lingo `201 mg/dL`). Excursions are broken
/// across sensor gaps exceeding `1.5` hours.
pub fn select_notable_spike_peaks(
    readings: &[LingoReading],
    max_markers: usize,
) -> Vec<LingoReading> {
    select_notable_spike_peaks_above(readings, 140.0, max_markers)
}

/// Select one representative peak per contiguous excursion above `threshold_mg_dl` (or
/// ceiling-capped), breaking excursions across sensor gaps exceeding `1.5` hours and spreading
/// up to `max_markers` across equal-width temporal buckets.
pub fn select_notable_spike_peaks_above(
    readings: &[LingoReading],
    threshold_mg_dl: f64,
    max_markers: usize,
) -> Vec<LingoReading> {
    if readings.is_empty() || max_markers == 0 {
        return Vec::new();
    }
    let threshold = if threshold_mg_dl.is_finite() {
        threshold_mg_dl
    } else {
        140.0
    };

    const EXCURSION_GAP_BREAK_HOURS: f64 = 1.5;

    // Group contiguous readings > threshold (or ceiling-capped), breaking across gaps > 1.5h.
    // For each excursion, pick the midpoint of its peak plateau so flat 201 mg/dL runs
    // emit exactly one clean representative marker.
    struct ExcursionPeak {
        rep_idx: usize,
        span_readings: usize,
    }

    let mut peaks: Vec<ExcursionPeak> = Vec::new();
    let mut active_start: Option<usize> = None;
    let mut prev_spike_idx: usize = 0;

    let flush_excursion = |start: usize, end: usize, out: &mut Vec<ExcursionPeak>| {
        let mut max_g = f64::NEG_INFINITY;
        for r in &readings[start..=end] {
            if r.glucose_mg_dl > max_g {
                max_g = r.glucose_mg_dl;
            }
        }
        let plateau_indices: Vec<usize> = (start..=end)
            .filter(|&i| (readings[i].glucose_mg_dl - max_g).abs() <= 1e-6)
            .collect();
        let rep_idx = plateau_indices
            .get(plateau_indices.len() / 2)
            .copied()
            .unwrap_or(start);
        out.push(ExcursionPeak {
            rep_idx,
            span_readings: end - start + 1,
        });
    };

    for (idx, r) in readings.iter().enumerate() {
        if r.bound.is_ceiling_capped() || r.glucose_mg_dl > threshold {
            match active_start {
                None => {
                    active_start = Some(idx);
                    prev_spike_idx = idx;
                }
                Some(start) => {
                    let gap_hours = (r.hour_offset - readings[prev_spike_idx].hour_offset).abs();
                    if gap_hours > EXCURSION_GAP_BREAK_HOURS {
                        flush_excursion(start, prev_spike_idx, &mut peaks);
                        active_start = Some(idx);
                    }
                    prev_spike_idx = idx;
                }
            }
        } else if let Some(start) = active_start.take() {
            flush_excursion(start, prev_spike_idx, &mut peaks);
        }
    }
    if let Some(start) = active_start {
        flush_excursion(start, prev_spike_idx, &mut peaks);
    }

    if peaks.len() > max_markers {
        // Distribute markers across equal-width temporal buckets, choosing the most prominent
        // excursion (highest peak, then longest span) in each bucket so tied ceiling plateaus
        // (e.g. Abbott Lingo `201 mg/dL`) are spread across the full timeline rather than
        // clustered at the start. Fill any spare slots (e.g. from empty gap buckets) by
        // remaining global prominence.
        let min_h = peaks
            .iter()
            .map(|p| readings[p.rep_idx].hour_offset)
            .fold(f64::INFINITY, f64::min);
        let max_h = peaks
            .iter()
            .map(|p| readings[p.rep_idx].hour_offset)
            .fold(f64::NEG_INFINITY, f64::max);
        let span_h = (max_h - min_h).max(0.0);

        let cmp_prominence = |a: &ExcursionPeak, b: &ExcursionPeak| {
            readings[b.rep_idx]
                .glucose_mg_dl
                .total_cmp(&readings[a.rep_idx].glucose_mg_dl)
                .then_with(|| b.span_readings.cmp(&a.span_readings))
                .then_with(|| a.rep_idx.cmp(&b.rep_idx))
        };

        if span_h > 1e-6 {
            let mut chosen = vec![false; peaks.len()];
            let mut bucket_best: Vec<Option<usize>> = vec![None; max_markers];
            for (i, p) in peaks.iter().enumerate() {
                let norm = ((readings[p.rep_idx].hour_offset - min_h) / span_h).clamp(0.0, 1.0);
                let bucket = ((norm * max_markers as f64).floor() as usize).min(max_markers - 1);
                match bucket_best[bucket] {
                    None => bucket_best[bucket] = Some(i),
                    Some(prev_i) => {
                        if cmp_prominence(&peaks[i], &peaks[prev_i]).is_lt() {
                            bucket_best[bucket] = Some(i);
                        }
                    }
                }
            }

            let mut selected_indices: Vec<usize> = Vec::with_capacity(max_markers);
            for idx in bucket_best.into_iter().flatten() {
                chosen[idx] = true;
                selected_indices.push(idx);
            }

            if selected_indices.len() < max_markers {
                let mut remaining: Vec<usize> = (0..peaks.len()).filter(|&i| !chosen[i]).collect();
                remaining.sort_by(|&a, &b| cmp_prominence(&peaks[a], &peaks[b]));
                let need = max_markers - selected_indices.len();
                selected_indices.extend(remaining.into_iter().take(need));
            }

            selected_indices.sort_by_key(|&i| peaks[i].rep_idx);
            return selected_indices
                .into_iter()
                .map(|i| readings[peaks[i].rep_idx].clone())
                .collect();
        }

        peaks.sort_by(cmp_prominence);
        peaks.truncate(max_markers);
        peaks.sort_by_key(|p| p.rep_idx);
    }

    peaks
        .into_iter()
        .map(|p| readings[p.rep_idx].clone())
        .collect()
}

fn build_health_bridge_envelope(
    import: &CommercialGlucoseImport,
    platform: &str,
) -> HealthBridgeEnvelope {
    let origin = import
        .diagnostics
        .detected_sources
        .first()
        .cloned()
        .unwrap_or_else(|| match import.monitor {
            GlucoseMonitor::AbbottLingo => "com.abbott.lingo".to_string(),
            other => other.slug().to_string(),
        });
    HealthBridgeEnvelope {
        schema: HEALTH_BRIDGE_SCHEMA_V1.to_string(),
        platform: platform.to_string(),
        exported_at: import.diagnostics.last_timestamp.clone(),
        records: import
            .readings
            .iter()
            .map(|r| {
                let bound = effective_reading_bound(r, import.monitor);
                HealthBridgeGlucoseRecord {
                    time: r.timestamp.clone(),
                    zone_offset: extract_zone_offset_str(&r.timestamp),
                    level_mg_dl: Some((r.glucose_mg_dl * 10.0).round() / 10.0),
                    level_mmol_l: None,
                    relation_to_meal: r.tag.clone(),
                    specimen_source: Some(match import.method {
                        MeasurementMethod::InterstitialCgm => "interstitial_fluid".to_string(),
                        MeasurementMethod::CapillaryBgm => "capillary_blood".to_string(),
                    }),
                    data_origin: Some(origin.clone()),
                    lingo_count: r.lingo_count,
                    bound: bound.bridge_str().map(str::to_owned),
                }
            })
            .collect(),
    }
}

/// Export any parsed [`CommercialGlucoseImport`] to the canonical pretty-printed
/// `dabney.health_bridge.v1` JSON schema for offline validation and parity testing with
/// future Health Connect / HealthKit bridges.
pub fn export_health_bridge_json(import: &CommercialGlucoseImport, platform: &str) -> String {
    let envelope = build_health_bridge_envelope(import, platform);
    serde_json::to_string_pretty(&envelope).unwrap_or_else(|_| "{}".to_string())
}

/// Export any parsed [`CommercialGlucoseImport`] to compact (minified) `dabney.health_bridge.v1`
/// JSON for encrypted mobile personal vault storage (staying well under payload limits even for
/// multi-month 5-minute CGM archives).
pub fn export_health_bridge_json_compact(
    import: &CommercialGlucoseImport,
    platform: &str,
) -> String {
    let envelope = build_health_bridge_envelope(import, platform);
    serde_json::to_string(&envelope).unwrap_or_else(|_| "{}".to_string())
}

struct HeaderSpec {
    monitor: GlucoseMonitor,
    unit: GlucoseUnit,
    unit_explicit: bool,
    method: MeasurementMethod,
    ts_col: usize,
    time_col: Option<usize>,
    glucose_cols: Vec<usize>,
    unit_col: Option<usize>,
    source_col: Option<usize>,
    event_type_col: Option<usize>,
    record_type_col: Option<usize>,
    lingo_count_col: Option<usize>,
    note_col: Option<usize>,
}

struct ParsedGlucoseRow {
    timestamp: String,
    parsed_ts: Option<ParsedTimestamp>,
    glucose_mg_dl: f64,
    lingo_count: Option<f64>,
    tag: Option<String>,
    bound: GlucoseReadingBound,
}

struct IndexedGlucoseRow {
    order: usize,
    row: ParsedGlucoseRow,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ParsedTimestamp {
    /// Exact monotonic UTC epoch minutes (computed with Gregorian calendar dates).
    pub(crate) utc_minutes: f64,
    /// Local calendar date (`YYYY-MM-DD`) in the reading's own timezone offset.
    pub(crate) local_date: NaiveDate,
    /// Local wall-clock hour (`0..24`) for 24-hour AGP & circadian binning.
    pub(crate) local_hour: u8,
}

enum CellParseOutcome {
    Empty,
    Parsed {
        mg_dl: f64,
        bound: GlucoseReadingBound,
        detected_unit: Option<GlucoseUnit>,
    },
    OutOfRangeSentinel(String),
    Invalid(String),
}

struct FinalizeImportArgs {
    format: ImportPayloadFormat,
    monitor: GlucoseMonitor,
    source_unit: GlucoseUnit,
    method: MeasurementMethod,
    raw_rows: Vec<ParsedGlucoseRow>,
    total_rows_seen: usize,
    skipped_non_glucose_rows: usize,
    skipped_invalid_rows: usize,
    out_of_range_rows: usize,
    detected_sources: Vec<String>,
    sample_warnings: Vec<String>,
}

/// Auto-detect and parse any supported commercial glucose export:
/// - Commercial CSV / TSV (Abbott Lingo, LibreView, Dexcom Clarity/Stelo, CareLink, Eversense, Tandem, Glooko/Tidepool, BGM)
/// - Apple Health `export.xml` (`HKQuantityTypeIdentifierBloodGlucose`)
/// - Clean-room `dabney.health_bridge.v1` / Android Health Connect `BloodGlucoseRecord` JSON
pub fn parse_commercial_glucose_input(
    input: &str,
    monitor_hint: Option<GlucoseMonitor>,
) -> Result<CommercialGlucoseImport, String> {
    let cleaned = input.strip_prefix('\u{feff}').unwrap_or(input);
    let trimmed = cleaned.trim_start();
    if trimmed.is_empty() {
        return Err("Glucose export is empty.".to_string());
    }
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        return parse_health_bridge_json(cleaned, monitor_hint);
    }
    if trimmed.starts_with("<?xml")
        || trimmed.starts_with("<HealthData")
        || trimmed.starts_with("<!DOCTYPE HealthData")
        || cleaned.contains("HKQuantityTypeIdentifierBloodGlucose")
    {
        return parse_apple_health_xml(cleaned, monitor_hint);
    }
    parse_commercial_glucose_csv(cleaned, monitor_hint)
}

/// Auto-detect and parse CSV/TSV exports from all 10 major commercial glucose monitors
/// in either `mg/dL` or `mmol/L`.
pub fn parse_commercial_glucose_csv(
    input: &str,
    monitor_hint: Option<GlucoseMonitor>,
) -> Result<CommercialGlucoseImport, String> {
    let cleaned_input = input.strip_prefix('\u{feff}').unwrap_or(input);
    let mut preamble_monitor = None;
    let mut header_spec: Option<HeaderSpec> = None;
    let mut delimiter: Option<char> = None;
    let mut raw_rows: Vec<ParsedGlucoseRow> = Vec::new();
    let mut saw_bgm_only = true;
    let mut saw_stelo_signal = false;
    let mut saw_mmol_cell = false;
    let mut total_rows_seen = 0usize;
    let mut skipped_non_glucose_rows = 0usize;
    let mut skipped_invalid_rows = 0usize;
    let mut out_of_range_rows = 0usize;
    let mut detected_sources: Vec<String> = Vec::new();
    let mut sample_warnings: Vec<String> = Vec::new();

    for (line_idx, raw_line) in cleaned_input.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        let lower = line.to_ascii_lowercase();
        if lower.contains("stelo") || lower.contains("below 70") || lower.contains("above 250") {
            saw_stelo_signal = true;
        }
        if preamble_monitor.is_none() {
            preamble_monitor = detect_preamble_monitor(&lower);
        }
        if line.starts_with('#') || line.starts_with("-------") {
            continue;
        }

        let active_delim = delimiter.unwrap_or_else(|| detect_line_delimiter(line));
        let cols = match split_delimited_row(line, active_delim) {
            Ok(c) => c,
            Err(err) => {
                if header_spec.is_none() {
                    continue;
                }
                total_rows_seen += 1;
                skipped_invalid_rows += 1;
                push_warning(
                    &mut sample_warnings,
                    format!("Line {}: {err}", line_idx + 1),
                );
                continue;
            }
        };

        if header_spec.is_none() {
            if let Some(spec) = try_parse_header(&cols, monitor_hint.or(preamble_monitor)) {
                delimiter = Some(active_delim);
                header_spec = Some(spec);
                continue;
            }
            // Allow headerless `timestamp,glucose_mg_dl[,lingo_count[,tag]]` rows
            if cols.len() >= 2 {
                if let Some(parsed_ts) = parse_timestamp(&cols[0], None) {
                    if let CellParseOutcome::Parsed {
                        mg_dl,
                        bound,
                        detected_unit,
                    } = parse_glucose_cell(&cols[1], GlucoseUnit::MgDl)
                    {
                        if (MIN_MG_DL..=MAX_MG_DL).contains(&mg_dl) {
                            delimiter = Some(active_delim);
                            let monitor = monitor_hint
                                .or(preamble_monitor)
                                .unwrap_or(GlucoseMonitor::AbbottLingo);
                            let unit = detected_unit.unwrap_or(GlucoseUnit::MgDl);
                            if unit == GlucoseUnit::MmolL {
                                saw_mmol_cell = true;
                            }
                            let lc = cols
                                .get(2)
                                .filter(|s| !s.is_empty())
                                .and_then(|s| s.parse::<f64>().ok())
                                .filter(|v| v.is_finite() && *v >= 0.0);
                            let tag = cols.get(3).filter(|s| !s.is_empty()).cloned();
                            total_rows_seen += 1;
                            raw_rows.push(ParsedGlucoseRow {
                                timestamp: cols[0].clone(),
                                parsed_ts: Some(parsed_ts),
                                glucose_mg_dl: mg_dl,
                                lingo_count: lc,
                                tag,
                                bound,
                            });
                            saw_bgm_only = monitor == GlucoseMonitor::FingerstickBgm;
                            header_spec = Some(HeaderSpec {
                                monitor,
                                unit,
                                unit_explicit: detected_unit.is_some(),
                                method: monitor.default_method(),
                                ts_col: 0,
                                time_col: None,
                                glucose_cols: vec![1],
                                unit_col: None,
                                source_col: None,
                                event_type_col: None,
                                record_type_col: None,
                                lingo_count_col: Some(2),
                                note_col: Some(3),
                            });
                            continue;
                        }
                    }
                }
            }
            continue;
        }

        let Some(spec) = header_spec.as_ref() else {
            continue;
        };
        total_rows_seen += 1;

        // Dexcom Clarity / Stelo: skip non-EGV metadata/calibration/event rows cleanly.
        if let Some(et_idx) = spec.event_type_col {
            let event_type = cols
                .get(et_idx)
                .map(|s| s.trim().to_ascii_lowercase())
                .unwrap_or_default();
            if !event_type.is_empty()
                && event_type != "egv"
                && event_type != "cgm"
                && event_type != "bgm"
                && event_type != "bg"
            {
                skipped_non_glucose_rows += 1;
                continue;
            }
        }

        // LibreView: accept Record Type 0 (Historic CGM), 1 (Scan CGM), or 2 (Strip BGM).
        if let Some(rt_idx) = spec.record_type_col {
            let rt = cols.get(rt_idx).map(|s| s.trim()).unwrap_or("");
            if !rt.is_empty() && rt != "0" && rt != "1" && rt != "2" {
                skipped_non_glucose_rows += 1;
                continue;
            }
            if rt == "0" || rt == "1" {
                saw_bgm_only = false;
            }
        } else if spec.method == MeasurementMethod::InterstitialCgm {
            saw_bgm_only = false;
        }

        let ts_raw = cols.get(spec.ts_col).map(|s| s.trim()).unwrap_or("");
        if ts_raw.is_empty() {
            skipped_non_glucose_rows += 1;
            continue;
        }
        let timestamp = if let Some(t_idx) = spec.time_col {
            let t_raw = cols.get(t_idx).map(|s| s.trim()).unwrap_or("");
            if t_raw.is_empty() {
                ts_raw.to_string()
            } else {
                format!("{ts_raw} {t_raw}")
            }
        } else {
            ts_raw.to_string()
        };

        let row_unit = spec
            .unit_col
            .and_then(|u_idx| cols.get(u_idx))
            .map(|u| {
                if u.to_ascii_lowercase().contains("mmol") {
                    GlucoseUnit::MmolL
                } else {
                    GlucoseUnit::MgDl
                }
            })
            .unwrap_or_else(|| {
                if !spec.unit_explicit && saw_mmol_cell {
                    GlucoseUnit::MmolL
                } else {
                    spec.unit
                }
            });
        if row_unit == GlucoseUnit::MmolL {
            saw_mmol_cell = true;
        }

        if let Some(src) = spec
            .source_col
            .and_then(|s_idx| cols.get(s_idx))
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        {
            record_source(&mut detected_sources, src);
        }

        let mut parsed_glucose: Option<(f64, GlucoseReadingBound)> = None;
        let mut cell_issue: Option<(bool, String)> = None;
        for &g_idx in &spec.glucose_cols {
            let cell = cols.get(g_idx).map(|s| s.trim()).unwrap_or("");
            match parse_glucose_cell(cell, row_unit) {
                CellParseOutcome::Empty => continue,
                CellParseOutcome::Parsed {
                    mut mg_dl,
                    bound,
                    detected_unit,
                } => {
                    if detected_unit == Some(GlucoseUnit::MmolL) {
                        saw_mmol_cell = true;
                    } else if !spec.unit_explicit
                        && row_unit == GlucoseUnit::MgDl
                        && detected_unit.is_none()
                        && (1.1..=19.9).contains(&mg_dl)
                    {
                        // Unitless header (`Timestamp,Glucose`) with UK/EU mmol/L values (e.g., 5.4).
                        mg_dl *= MMOL_TO_MG_DL;
                        saw_mmol_cell = true;
                    }
                    parsed_glucose = Some((mg_dl, bound));
                    break;
                }
                CellParseOutcome::OutOfRangeSentinel(raw) => {
                    cell_issue = Some((
                        true,
                        format!("Line {}: sensor status cell `{raw}`.", line_idx + 1),
                    ));
                }
                CellParseOutcome::Invalid(raw) => {
                    cell_issue = Some((
                        false,
                        format!(
                            "Line {}: could not parse glucose cell `{raw}`.",
                            line_idx + 1
                        ),
                    ));
                }
            }
        }
        let Some((glucose_mg_dl, bound)) = parsed_glucose else {
            if let Some((is_oor, msg)) = cell_issue {
                if is_oor {
                    out_of_range_rows += 1;
                } else {
                    skipped_invalid_rows += 1;
                }
                push_warning(&mut sample_warnings, msg);
            } else {
                skipped_non_glucose_rows += 1;
            }
            continue;
        };

        if !glucose_mg_dl.is_finite() || !(MIN_MG_DL..=MAX_MG_DL).contains(&glucose_mg_dl) {
            skipped_invalid_rows += 1;
            push_warning(
                &mut sample_warnings,
                format!(
                    "Line {}: skipped out-of-range glucose `{glucose_mg_dl:.1} mg/dL` (expected {MIN_MG_DL}–{MAX_MG_DL} mg/dL).",
                    line_idx + 1
                ),
            );
            continue;
        }

        let lingo_count = spec
            .lingo_count_col
            .and_then(|i| cols.get(i))
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .and_then(|s| s.parse::<f64>().ok())
            .filter(|v| v.is_finite() && *v >= 0.0);

        let tag = spec
            .note_col
            .and_then(|i| cols.get(i))
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(str::to_owned);

        let parsed_ts = parse_timestamp(&timestamp, None);
        raw_rows.push(ParsedGlucoseRow {
            timestamp,
            parsed_ts,
            glucose_mg_dl,
            lingo_count,
            tag,
            bound,
        });
    }

    if raw_rows.is_empty() {
        if let Some(first_warn) = sample_warnings.first() {
            return Err(format!("No valid glucose readings found. {first_warn}"));
        }
        return Err(
            "No valid glucose readings found. Supported exports: Abbott Lingo CSV, Apple Health export.xml, Health Connect JSON, FreeStyle LibreView, Dexcom Clarity/Stelo, Medtronic CareLink, Eversense, Tandem, Omnipod/Glooko/Tidepool, or BGM CSV."
                .to_string(),
        );
    }

    let spec = header_spec.unwrap_or(HeaderSpec {
        monitor: monitor_hint.unwrap_or(GlucoseMonitor::GenericCsv),
        unit: GlucoseUnit::MgDl,
        unit_explicit: false,
        method: MeasurementMethod::InterstitialCgm,
        ts_col: 0,
        time_col: None,
        glucose_cols: vec![1],
        unit_col: None,
        source_col: None,
        event_type_col: None,
        record_type_col: None,
        lingo_count_col: None,
        note_col: None,
    });

    let method = if saw_bgm_only && spec.monitor == GlucoseMonitor::FingerstickBgm {
        MeasurementMethod::CapillaryBgm
    } else {
        spec.method
    };

    let source_monitor = detected_sources
        .iter()
        .find_map(|s| detect_preamble_monitor(&s.to_ascii_lowercase()));

    let detected = if spec.monitor == GlucoseMonitor::DexcomClarity && saw_stelo_signal {
        GlucoseMonitor::DexcomStelo
    } else if spec.monitor == GlucoseMonitor::GenericCsv {
        source_monitor.unwrap_or(GlucoseMonitor::GenericCsv)
    } else {
        spec.monitor
    };

    let final_unit = if saw_mmol_cell {
        GlucoseUnit::MmolL
    } else {
        spec.unit
    };

    Ok(finalize_glucose_import(FinalizeImportArgs {
        format: ImportPayloadFormat::Csv,
        monitor: monitor_hint.unwrap_or(detected),
        source_unit: final_unit,
        method,
        raw_rows,
        total_rows_seen,
        skipped_non_glucose_rows,
        skipped_invalid_rows,
        out_of_range_rows,
        detected_sources,
        sample_warnings,
    }))
}

/// Streaming zero-dependency parser for Apple Health `export.xml` blood glucose records
/// (`HKQuantityTypeIdentifierBloodGlucose`, including Abbott Lingo iOS syncs).
pub fn parse_apple_health_xml(
    input: &str,
    monitor_hint: Option<GlucoseMonitor>,
) -> Result<CommercialGlucoseImport, String> {
    let cleaned = input.strip_prefix('\u{feff}').unwrap_or(input);
    let mut raw_rows: Vec<ParsedGlucoseRow> = Vec::new();
    let mut total_rows_seen = 0usize;
    let mut skipped_non_glucose_rows = 0usize;
    let mut skipped_invalid_rows = 0usize;
    let mut out_of_range_rows = 0usize;
    let mut detected_sources: Vec<String> = Vec::new();
    let mut sample_warnings: Vec<String> = Vec::new();
    let mut saw_mmol = false;

    // Scan every `<Record ...>` tag, whether pretty-printed line-by-line or minified.
    let mut rest = cleaned;
    while let Some(pos) = rest.find("<Record") {
        let after = &rest[pos + 7..];
        let Some(end) = after.find('>') else {
            break;
        };
        let attrs = &after[..end];
        rest = &after[end + 1..];
        total_rows_seen += 1;

        if !attrs.contains("HKQuantityTypeIdentifierBloodGlucose") {
            skipped_non_glucose_rows += 1;
            continue;
        }

        let Some(start_date) = extract_xml_attr(attrs, "startDate") else {
            skipped_invalid_rows += 1;
            push_warning(
                &mut sample_warnings,
                "Skipped Apple Health BloodGlucose record missing startDate.".to_string(),
            );
            continue;
        };
        let Some(val_str) = extract_xml_attr(attrs, "value") else {
            skipped_invalid_rows += 1;
            push_warning(
                &mut sample_warnings,
                format!("Skipped Apple Health record at `{start_date}` missing value."),
            );
            continue;
        };
        let unit_attr = extract_xml_attr(attrs, "unit").unwrap_or("mg/dL");
        let unit = if unit_attr.to_ascii_lowercase().contains("mmol") {
            saw_mmol = true;
            GlucoseUnit::MmolL
        } else {
            GlucoseUnit::MgDl
        };
        let source_name = extract_xml_attr(attrs, "sourceName");
        if let Some(src) = source_name {
            record_source(&mut detected_sources, src);
        }

        match parse_glucose_cell(val_str, unit) {
            CellParseOutcome::Parsed { mg_dl, bound, .. }
                if mg_dl.is_finite() && (MIN_MG_DL..=MAX_MG_DL).contains(&mg_dl) =>
            {
                let parsed_ts = parse_timestamp(start_date, None);
                raw_rows.push(ParsedGlucoseRow {
                    timestamp: start_date.to_string(),
                    parsed_ts,
                    glucose_mg_dl: mg_dl,
                    lingo_count: None,
                    tag: source_name.map(str::to_owned),
                    bound,
                });
            }
            CellParseOutcome::OutOfRangeSentinel(raw) => {
                out_of_range_rows += 1;
                push_warning(
                    &mut sample_warnings,
                    format!("Apple Health record at `{start_date}` had sensor status `{raw}`."),
                );
            }
            _ => {
                skipped_invalid_rows += 1;
                push_warning(
                    &mut sample_warnings,
                    format!("Skipped Apple Health record at `{start_date}` with invalid value `{val_str}`."),
                );
            }
        }
    }

    if raw_rows.is_empty() {
        return Err(
            "No `HKQuantityTypeIdentifierBloodGlucose` records found in Apple Health XML."
                .to_string(),
        );
    }

    let inferred = detected_sources
        .iter()
        .find_map(|s| detect_preamble_monitor(&s.to_ascii_lowercase()))
        .unwrap_or(GlucoseMonitor::GenericCsv);
    let monitor = monitor_hint.unwrap_or(inferred);

    Ok(finalize_glucose_import(FinalizeImportArgs {
        format: ImportPayloadFormat::AppleHealthXml,
        monitor,
        source_unit: if saw_mmol {
            GlucoseUnit::MmolL
        } else {
            GlucoseUnit::MgDl
        },
        method: monitor.default_method(),
        raw_rows,
        total_rows_seen,
        skipped_non_glucose_rows,
        skipped_invalid_rows,
        out_of_range_rows,
        detected_sources,
        sample_warnings,
    }))
}

/// Parse `dabney.health_bridge.v1` envelopes or raw Android Health Connect / Nightscout JSON
/// arrays containing `BloodGlucoseRecord` entries.
pub fn parse_health_bridge_json(
    input: &str,
    monitor_hint: Option<GlucoseMonitor>,
) -> Result<CommercialGlucoseImport, String> {
    let root: Value = serde_json::from_str(input.strip_prefix('\u{feff}').unwrap_or(input))
        .map_err(|e| format!("Invalid Health Connect / bridge JSON: {e}"))?;

    let records_slice: &[Value] = if let Some(arr) = root.as_array() {
        arr.as_slice()
    } else if let Some(arr) = root.get("records").and_then(Value::as_array) {
        arr.as_slice()
    } else if let Some(arr) = root
        .get("bloodGlucose")
        .or_else(|| root.get("blood_glucose"))
        .or_else(|| root.get("data"))
        .and_then(Value::as_array)
    {
        arr.as_slice()
    } else {
        return Err(
            "JSON payload must be an array of glucose records or an object with a `records` array."
                .to_string(),
        );
    };

    let mut raw_rows: Vec<ParsedGlucoseRow> = Vec::new();
    let mut total_rows_seen = 0usize;
    let mut skipped_invalid_rows = 0usize;
    let mut detected_sources: Vec<String> = Vec::new();
    let mut sample_warnings: Vec<String> = Vec::new();
    let mut saw_mmol = false;
    let mut saw_capillary_only = true;

    for (idx, item) in records_slice.iter().enumerate() {
        total_rows_seen += 1;
        let Some(obj) = item.as_object() else {
            skipped_invalid_rows += 1;
            push_warning(
                &mut sample_warnings,
                format!("Record #{}: expected JSON object.", idx + 1),
            );
            continue;
        };

        let Some(raw_time) = obj
            .get("time")
            .or_else(|| obj.get("startTime"))
            .or_else(|| obj.get("timestamp"))
            .or_else(|| obj.get("start_time"))
            .or_else(|| obj.get("dateString"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
        else {
            skipped_invalid_rows += 1;
            push_warning(
                &mut sample_warnings,
                format!("Record #{}: missing `time` / `timestamp`.", idx + 1),
            );
            continue;
        };

        let zone_offset = obj
            .get("zoneOffset")
            .or_else(|| obj.get("zone_offset"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty());

        let (mg_dl_opt, unit_used) = extract_json_glucose(obj);
        if unit_used == GlucoseUnit::MmolL {
            saw_mmol = true;
        }
        let Some(glucose_mg_dl) = mg_dl_opt else {
            skipped_invalid_rows += 1;
            push_warning(
                &mut sample_warnings,
                format!(
                    "Record #{} (`{raw_time}`): missing numeric glucose level.",
                    idx + 1
                ),
            );
            continue;
        };
        if !glucose_mg_dl.is_finite() || !(MIN_MG_DL..=MAX_MG_DL).contains(&glucose_mg_dl) {
            skipped_invalid_rows += 1;
            push_warning(
                &mut sample_warnings,
                format!(
                    "Record #{} (`{raw_time}`): out-of-range glucose `{glucose_mg_dl:.1} mg/dL`.",
                    idx + 1
                ),
            );
            continue;
        }

        if let Some(origin) = obj
            .get("dataOrigin")
            .or_else(|| obj.get("data_origin"))
            .or_else(|| obj.get("sourceName"))
            .or_else(|| obj.get("source"))
            .and_then(Value::as_str)
            .or_else(|| {
                obj.get("metadata")
                    .and_then(|m| m.get("dataOrigin"))
                    .and_then(|d| d.get("packageName").or(Some(d)))
                    .and_then(Value::as_str)
            })
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            record_source(&mut detected_sources, origin);
        }

        let specimen = obj
            .get("specimenSource")
            .or_else(|| obj.get("specimen_source"))
            .and_then(Value::as_str)
            .map(|s| s.to_ascii_lowercase());
        if specimen.as_deref() != Some("capillary_blood") {
            saw_capillary_only = false;
        }

        let tag = obj
            .get("relationToMeal")
            .or_else(|| obj.get("relation_to_meal"))
            .or_else(|| obj.get("note"))
            .or_else(|| obj.get("tag"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("unknown"))
            .map(str::to_owned);

        let lingo_count = obj
            .get("lingoCount")
            .or_else(|| obj.get("lingo_count"))
            .and_then(Value::as_f64)
            .filter(|v| v.is_finite() && *v >= 0.0);

        let bound = match obj
            .get("bound")
            .and_then(Value::as_str)
            .map(|s| s.to_ascii_lowercase())
            .as_deref()
        {
            Some("below_floor" | "low" | "below_sensor_floor") => {
                GlucoseReadingBound::BelowSensorFloor
            }
            Some("above_ceiling" | "high" | "above_sensor_ceiling") => {
                GlucoseReadingBound::AboveSensorCeiling
            }
            _ => GlucoseReadingBound::Measured,
        };

        let display_timestamp = normalize_timestamp_with_zone_offset(raw_time, zone_offset);
        let parsed_ts = parse_timestamp(&display_timestamp, zone_offset);

        raw_rows.push(ParsedGlucoseRow {
            timestamp: display_timestamp,
            parsed_ts,
            glucose_mg_dl,
            lingo_count,
            tag,
            bound,
        });
    }

    if raw_rows.is_empty() {
        return Err("No valid blood glucose records found in JSON payload.".to_string());
    }

    let inferred = detected_sources
        .iter()
        .find_map(|s| detect_preamble_monitor(&s.to_ascii_lowercase()))
        .unwrap_or(GlucoseMonitor::GenericCsv);
    let monitor = monitor_hint.unwrap_or(inferred);
    let method = if saw_capillary_only {
        MeasurementMethod::CapillaryBgm
    } else {
        monitor.default_method()
    };

    Ok(finalize_glucose_import(FinalizeImportArgs {
        format: ImportPayloadFormat::HealthBridgeJson,
        monitor,
        source_unit: if saw_mmol {
            GlucoseUnit::MmolL
        } else {
            GlucoseUnit::MgDl
        },
        method,
        raw_rows,
        total_rows_seen,
        skipped_non_glucose_rows: 0,
        skipped_invalid_rows,
        out_of_range_rows: 0,
        detected_sources,
        sample_warnings,
    }))
}

fn finalize_glucose_import(args: FinalizeImportArgs) -> CommercialGlucoseImport {
    let mut indexed: Vec<IndexedGlucoseRow> = args
        .raw_rows
        .into_iter()
        .enumerate()
        .map(|(order, row)| IndexedGlucoseRow { order, row })
        .collect();
    indexed.sort_by(|a, b| match (a.row.parsed_ts, b.row.parsed_ts) {
        (Some(ta), Some(tb)) => ta
            .utc_minutes
            .total_cmp(&tb.utc_minutes)
            .then_with(|| a.order.cmp(&b.order)),
        _ => a.order.cmp(&b.order),
    });

    // Drop exact (utc_minutes, rounded glucose) duplicate sync rows cleanly.
    let mut deduped: Vec<IndexedGlucoseRow> = Vec::with_capacity(indexed.len());
    let mut duplicate_timestamps_dropped = 0usize;
    for item in indexed {
        if let (Some(prev), Some(curr_ts)) = (deduped.last(), item.row.parsed_ts) {
            if let Some(prev_ts) = prev.row.parsed_ts {
                if (curr_ts.utc_minutes - prev_ts.utc_minutes).abs() < 1e-6
                    && (item.row.glucose_mg_dl - prev.row.glucose_mg_dl).abs() < 0.05
                {
                    duplicate_timestamps_dropped += 1;
                    continue;
                }
            }
        }
        deduped.push(item);
    }

    let first_min = deduped
        .iter()
        .find_map(|item| item.row.parsed_ts.map(|t| t.utc_minutes));
    let mut bounded_sentinels = 0usize;
    let mut lingo_floor_capped = 0usize;
    let mut lingo_ceiling_capped = 0usize;
    let mut timestamps_parsed = 0usize;
    let mut timestamps_unparsed = 0usize;
    let is_lingo = args.monitor == GlucoseMonitor::AbbottLingo;
    let readings: Vec<LingoReading> = deduped
        .into_iter()
        .enumerate()
        .map(|(idx, item)| {
            let effective_bound = if item.row.bound != GlucoseReadingBound::Measured {
                item.row.bound
            } else if is_lingo && item.row.glucose_mg_dl <= LINGO_FLOOR_MG_DL - 0.5 {
                GlucoseReadingBound::BelowSensorFloor
            } else if is_lingo && item.row.glucose_mg_dl >= LINGO_CEILING_MG_DL + 0.5 {
                GlucoseReadingBound::AboveSensorCeiling
            } else {
                GlucoseReadingBound::Measured
            };
            match effective_bound {
                GlucoseReadingBound::BelowSensorFloor => {
                    bounded_sentinels += 1;
                    lingo_floor_capped += 1;
                }
                GlucoseReadingBound::AboveSensorCeiling => {
                    bounded_sentinels += 1;
                    lingo_ceiling_capped += 1;
                }
                GlucoseReadingBound::Measured => {}
            }
            let hour_offset = match (item.row.parsed_ts, first_min) {
                (Some(t), Some(start)) => {
                    timestamps_parsed += 1;
                    ((t.utc_minutes - start) / 60.0).max(0.0)
                }
                _ => {
                    timestamps_unparsed += 1;
                    idx as f64 * (5.0 / 60.0)
                }
            };
            LingoReading {
                timestamp: item.row.timestamp,
                hour_offset,
                glucose_mg_dl: item.row.glucose_mg_dl,
                lingo_count: item.row.lingo_count,
                tag: item.row.tag,
                bound: effective_bound,
            }
        })
        .collect();

    let mut intervals_min: Vec<f64> = readings
        .windows(2)
        .map(|w| ((w[1].hour_offset - w[0].hour_offset).abs()) * 60.0)
        .filter(|m| *m > 0.05)
        .collect();
    intervals_min.sort_by(f64::total_cmp);
    let median_interval_minutes = if intervals_min.is_empty() {
        None
    } else {
        Some(percentile_sorted(&intervals_min, 0.50))
    };
    let longest_gap_minutes = intervals_min.last().copied();
    let span_hours = readings
        .last()
        .map(|r| r.hour_offset)
        .unwrap_or(0.0)
        .max(0.0);

    let first_timestamp = readings.first().map(|r| r.timestamp.clone());
    let last_timestamp = readings.last().map(|r| r.timestamp.clone());

    let checks = build_validation_checks(
        args.monitor,
        args.method,
        &readings,
        timestamps_parsed,
        timestamps_unparsed,
        duplicate_timestamps_dropped,
        bounded_sentinels,
        lingo_floor_capped,
        lingo_ceiling_capped,
        args.skipped_invalid_rows,
        args.out_of_range_rows,
        span_hours,
        median_interval_minutes,
        longest_gap_minutes,
    );

    let diagnostics = GlucoseImportDiagnostics {
        format: args.format,
        total_rows_seen: args.total_rows_seen,
        parsed_readings: readings.len(),
        skipped_non_glucose_rows: args.skipped_non_glucose_rows,
        skipped_invalid_rows: args.skipped_invalid_rows,
        out_of_range_rows: args.out_of_range_rows,
        duplicate_timestamps_dropped,
        bounded_sentinels,
        timestamps_parsed,
        timestamps_unparsed,
        first_timestamp,
        last_timestamp,
        span_hours,
        median_interval_minutes,
        longest_gap_minutes,
        detected_sources: args.detected_sources,
        sample_warnings: args.sample_warnings,
        checks,
    };

    CommercialGlucoseImport {
        monitor: args.monitor,
        source_unit: args.source_unit,
        method: args.method,
        readings,
        bounded_sentinels,
        diagnostics,
    }
}

#[allow(clippy::too_many_arguments)]
fn build_validation_checks(
    monitor: GlucoseMonitor,
    method: MeasurementMethod,
    readings: &[LingoReading],
    timestamps_parsed: usize,
    timestamps_unparsed: usize,
    duplicate_timestamps_dropped: usize,
    bounded_sentinels: usize,
    lingo_floor_capped: usize,
    lingo_ceiling_capped: usize,
    skipped_invalid_rows: usize,
    out_of_range_rows: usize,
    span_hours: f64,
    median_interval_minutes: Option<f64>,
    longest_gap_minutes: Option<f64>,
) -> Vec<ValidationCheck> {
    let mut checks = Vec::new();

    // 1. Timestamp & timezone chronology check
    if timestamps_unparsed == 0 && duplicate_timestamps_dropped == 0 {
        checks.push(ValidationCheck {
            name: "Timestamps & Chronology".to_string(),
            level: ValidationLevel::Pass,
            detail: format!(
                "{timestamps_parsed} timestamps parsed with exact calendar & timezone ordering."
            ),
        });
    } else if timestamps_unparsed == 0 {
        checks.push(ValidationCheck {
            name: "Timestamps & Chronology".to_string(),
            level: ValidationLevel::Info,
            detail: format!(
                "{timestamps_parsed} timestamps parsed; {duplicate_timestamps_dropped} exact duplicate sync row(s) deduplicated."
            ),
        });
    } else {
        checks.push(ValidationCheck {
            name: "Timestamps & Chronology".to_string(),
            level: ValidationLevel::Warn,
            detail: format!(
                "{timestamps_unparsed} row(s) used fallback spacing because timestamp format was unrecognized."
            ),
        });
    }

    // 2. Sampling cadence check (Abbott Lingo exports at 5-minute intervals)
    if let Some(med_min) = median_interval_minutes {
        let (level, note) = if method == MeasurementMethod::CapillaryBgm {
            (
                ValidationLevel::Info,
                format!("Median {med_min:.1} min between capillary fingerstick readings."),
            )
        } else if monitor == GlucoseMonitor::AbbottLingo && (4.0..=6.0).contains(&med_min) {
            (
                ValidationLevel::Pass,
                format!("Median {med_min:.1} min — matches Abbott Lingo 5-minute CGM cadence."),
            )
        } else if (4.0..=16.0).contains(&med_min) {
            (
                ValidationLevel::Pass,
                format!("Median {med_min:.1} min continuous CGM cadence."),
            )
        } else {
            (
                ValidationLevel::Info,
                format!("Median {med_min:.1} min sampling interval (irregular or coarse export)."),
            )
        };
        let gap_suffix = longest_gap_minutes
            .filter(|g| *g >= 60.0)
            .map(|g| format!(" Longest sensor gap: {:.1}h.", g / 60.0))
            .unwrap_or_default();
        checks.push(ValidationCheck {
            name: "Sampling Cadence".to_string(),
            level,
            detail: format!("{note}{gap_suffix}"),
        });
    }

    // 3. Sensor range & Lingo 55–200 mg/dL bounds check
    let min_g = readings
        .iter()
        .map(|r| r.glucose_mg_dl)
        .fold(f64::INFINITY, f64::min);
    let max_g = readings
        .iter()
        .map(|r| r.glucose_mg_dl)
        .fold(f64::NEG_INFINITY, f64::max);
    if monitor == GlucoseMonitor::AbbottLingo {
        let within_lingo = min_g >= LINGO_FLOOR_MG_DL && max_g <= LINGO_CEILING_MG_DL;
        let capped_pct = if readings.is_empty() {
            0.0
        } else {
            (bounded_sentinels as f64 / readings.len() as f64) * 100.0
        };
        let (level, detail) = if bounded_sentinels > 0 {
            let level = if capped_pct >= 5.0 {
                ValidationLevel::Warn
            } else {
                ValidationLevel::Info
            };
            (
                level,
                format!(
                    "Observed {min_g:.0}–{max_g:.0} mg/dL with {lingo_floor_capped} floor-capped (≤54 = <55) and {lingo_ceiling_capped} ceiling-capped (≥201 = >200) reading(s) ({capped_pct:.1}%). Abbott Lingo clips readings outside 55–200 mg/dL, so true mean, GMI, TAR >250, and peak excursions may exceed displayed values."
                ),
            )
        } else if within_lingo {
            (
                ValidationLevel::Pass,
                format!(
                    "All readings ({min_g:.0}–{max_g:.0} mg/dL) fall inside Abbott Lingo's 55–200 mg/dL sensor envelope."
                ),
            )
        } else {
            (
                ValidationLevel::Info,
                format!(
                    "Observed {min_g:.0}–{max_g:.0} mg/dL (extends beyond Lingo's typical 55–200 mg/dL range)."
                ),
            )
        };
        checks.push(ValidationCheck {
            name: "Biosensor Range & Saturation".to_string(),
            level,
            detail,
        });
    } else {
        checks.push(ValidationCheck {
            name: "Biosensor Range".to_string(),
            level: ValidationLevel::Pass,
            detail: format!(
                "Observed {min_g:.0}–{max_g:.0} mg/dL ({bounded_sentinels} bounded floor/ceiling sentinel(s))."
            ),
        });
    }

    // 4. ATTD consensus wear-time & duration check
    let days = span_hours / 24.0;
    let wear_pct = if method == MeasurementMethod::CapillaryBgm
        || median_interval_minutes.is_none()
        || span_hours <= 0.0
    {
        100.0
    } else {
        let cadence_h = (median_interval_minutes.unwrap_or(5.0) / 60.0).max(1.0 / 60.0);
        let expected = (span_hours / cadence_h) + 1.0;
        ((readings.len() as f64 / expected) * 100.0).clamp(0.0, 100.0)
    };
    if method != MeasurementMethod::CapillaryBgm {
        let (level, detail) = if days >= 13.5 && wear_pct >= 70.0 {
            (
                ValidationLevel::Pass,
                format!(
                    "{days:.1} days captured · {wear_pct:.0}% active wear (meets ATTD >=14d & >=70% AGP/GMI standard)."
                ),
            )
        } else if wear_pct < 70.0 {
            (
                ValidationLevel::Warn,
                format!(
                    "{days:.1} days captured · {wear_pct:.0}% active wear (below 70% ATTD completeness target; check Bluetooth/sensor gaps)."
                ),
            )
        } else {
            (
                ValidationLevel::Info,
                format!(
                    "{days:.1} days captured · {wear_pct:.0}% active wear (GMI & 24h AGP stabilize best at >=14 days)."
                ),
            )
        };
        checks.push(ValidationCheck {
            name: "ATTD Wear Completeness".to_string(),
            level,
            detail,
        });
    }

    // 5. Sensor out-of-range status rows (tracked separately from malformed rows)
    if out_of_range_rows > 0 {
        let level = if out_of_range_rows >= 24 {
            ValidationLevel::Warn
        } else {
            ValidationLevel::Info
        };
        checks.push(ValidationCheck {
            name: "Sensor Out-of-Range Rows".to_string(),
            level,
            detail: format!(
                "Observed {out_of_range_rows} non-numeric sensor status row(s) (e.g. \"Out of range\"); excluded from numeric mean/TIR while preserving valid readings."
            ),
        });
    }

    // 6. Skipped / malformed rows check
    if skipped_invalid_rows > 0 {
        checks.push(ValidationCheck {
            name: "Row Integrity".to_string(),
            level: ValidationLevel::Warn,
            detail: format!(
                "Skipped {skipped_invalid_rows} malformed row(s); valid readings were preserved."
            ),
        });
    }

    checks
}

fn push_warning(warnings: &mut Vec<String>, msg: String) {
    if warnings.len() < MAX_SAMPLE_WARNINGS {
        warnings.push(msg);
    }
}

fn record_source(sources: &mut Vec<String>, src: &str) {
    if !sources
        .iter()
        .any(|existing| existing.eq_ignore_ascii_case(src))
        && sources.len() < 8
    {
        sources.push(src.to_string());
    }
}

fn detect_line_delimiter(line: &str) -> char {
    let mut commas = 0usize;
    let mut semis = 0usize;
    let mut tabs = 0usize;
    let mut in_quotes = false;
    for ch in line.chars() {
        match ch {
            '"' => in_quotes = !in_quotes,
            ',' if !in_quotes => commas += 1,
            ';' if !in_quotes => semis += 1,
            '\t' if !in_quotes => tabs += 1,
            _ => {}
        }
    }
    if tabs > commas && tabs >= semis {
        '\t'
    } else if semis > commas && semis > 0 {
        ';'
    } else {
        ','
    }
}

fn extract_xml_attr<'a>(attrs: &'a str, name: &str) -> Option<&'a str> {
    let pattern_dq = format!("{name}=\"");
    if let Some(idx) = attrs.find(&pattern_dq) {
        let start = idx + pattern_dq.len();
        let rest = &attrs[start..];
        let end = rest.find('"')?;
        return Some(&rest[..end]);
    }
    let pattern_sq = format!("{name}='");
    if let Some(idx) = attrs.find(&pattern_sq) {
        let start = idx + pattern_sq.len();
        let rest = &attrs[start..];
        let end = rest.find('\'')?;
        return Some(&rest[..end]);
    }
    None
}

fn extract_json_glucose(obj: &serde_json::Map<String, Value>) -> (Option<f64>, GlucoseUnit) {
    if let Some(v) = obj
        .get("levelMgDl")
        .or_else(|| obj.get("level_mg_dl"))
        .or_else(|| obj.get("glucose_mg_dl"))
        .or_else(|| obj.get("sgv"))
        .and_then(Value::as_f64)
    {
        return (Some(v), GlucoseUnit::MgDl);
    }
    if let Some(v) = obj
        .get("levelMmolL")
        .or_else(|| obj.get("level_mmol_l"))
        .or_else(|| obj.get("glucose_mmol_l"))
        .and_then(Value::as_f64)
    {
        return (Some(v * MMOL_TO_MG_DL), GlucoseUnit::MmolL);
    }
    if let Some(level) = obj.get("level") {
        if let Some(v) = level
            .get("inMilligramsPerDeciliter")
            .or_else(|| level.get("mgDl"))
            .or_else(|| level.get("mg_dl"))
            .and_then(Value::as_f64)
        {
            return (Some(v), GlucoseUnit::MgDl);
        }
        if let Some(v) = level
            .get("inMillimolesPerLiter")
            .or_else(|| level.get("mmolL"))
            .or_else(|| level.get("mmol_l"))
            .and_then(Value::as_f64)
        {
            return (Some(v * MMOL_TO_MG_DL), GlucoseUnit::MmolL);
        }
        if let Some(v) = level.as_f64() {
            return (Some(v), GlucoseUnit::MgDl);
        }
    }
    if let Some(val) = obj
        .get("value")
        .or_else(|| obj.get("glucose"))
        .and_then(|v| {
            v.as_f64()
                .or_else(|| v.as_str().and_then(|s| s.trim().parse::<f64>().ok()))
        })
    {
        let unit_str = obj
            .get("unit")
            .or_else(|| obj.get("units"))
            .and_then(Value::as_str)
            .unwrap_or("mg/dL");
        let unit = if unit_str.to_ascii_lowercase().contains("mmol") {
            GlucoseUnit::MmolL
        } else {
            GlucoseUnit::MgDl
        };
        return (Some(unit.to_mg_dl(val)), unit);
    }
    (None, GlucoseUnit::MgDl)
}

fn detect_preamble_monitor(lower: &str) -> Option<GlucoseMonitor> {
    if lower.contains("freestyle libre")
        || lower.contains("libreview")
        || lower.contains("glucose data,generated on")
    {
        Some(GlucoseMonitor::AbbottFreeStyleLibre)
    } else if lower.contains("stelo") {
        Some(GlucoseMonitor::DexcomStelo)
    } else if lower.contains("dexcom") || lower.contains("clarity") {
        Some(GlucoseMonitor::DexcomClarity)
    } else if lower.contains("carelink")
        || lower.contains("minimed")
        || lower.contains("guardian")
        || lower.contains("simplera")
    {
        Some(GlucoseMonitor::MedtronicCareLink)
    } else if lower.contains("eversense") || lower.contains("senseonics") {
        Some(GlucoseMonitor::SenseonicsEversense)
    } else if lower.contains("tandem") || lower.contains("t:slim") || lower.contains("mobi") {
        Some(GlucoseMonitor::TandemSource)
    } else if lower.contains("omnipod") || lower.contains("glooko") || lower.contains("tidepool") {
        Some(GlucoseMonitor::GlookoTidepool)
    } else if lower.contains("lingo") {
        Some(GlucoseMonitor::AbbottLingo)
    } else if lower.contains("accu-chek") || lower.contains("onetouch") || lower.contains("contour")
    {
        Some(GlucoseMonitor::FingerstickBgm)
    } else {
        None
    }
}

fn try_parse_header(cols: &[String], hint: Option<GlucoseMonitor>) -> Option<HeaderSpec> {
    let lower: Vec<String> = cols
        .iter()
        .map(|c| c.trim().trim_matches('"').to_ascii_lowercase())
        .collect();
    let joined = lower.join("|");

    let mut ts_col = None;
    let mut time_col = None;
    let mut glucose_cols = Vec::new();
    let mut value_fallback_col = None;
    let mut unit_col = None;
    let mut source_col = None;
    let mut event_type_col = None;
    let mut record_type_col = None;
    let mut lingo_count_col = None;
    let mut note_col = None;
    let mut saw_mmol = false;

    for (i, h) in lower.iter().enumerate() {
        if h.contains("mmol") {
            saw_mmol = true;
        }
        if ts_col.is_none()
            && (h.contains("timestamp")
                || h.contains("time of glucose reading")
                || h == "eventdatetime"
                || h == "datetime"
                || h == "date/time"
                || h == "date time"
                || h == "start time"
                || h == "start_time"
                || h == "startdate"
                || h == "date")
        {
            ts_col = Some(i);
        } else if time_col.is_none() && h == "time" {
            time_col = Some(i);
        }

        let is_non_reading_glucose = h.contains("rate of change")
            || h.contains("time of glucose")
            || h.contains("trend")
            || h.contains("unit")
            || h.contains("status")
            || h.contains("source")
            || h.contains("note")
            || h.contains("event");
        let is_glucose_col = !is_non_reading_glucose
            && (h.contains("historic glucose")
                || h.contains("scan glucose")
                || h.contains("sensor glucose")
                || h.contains("cgm glucose")
                || h.contains("readings (cgm)")
                || h.contains("glucose value")
                || h.contains("glucose_value")
                || h.contains("glucose_mg_dl")
                || h.contains("glucose_mmol")
                || h.contains("blood glucose")
                || h.contains("bg reading")
                || h.contains("strip glucose")
                || h.starts_with("glucose")
                || h.starts_with("measurement")
                || h == "bg (mg/dl)"
                || h == "bg (mmol/l)"
                || h == "mg/dl"
                || h == "mmol/l");
        if is_glucose_col {
            glucose_cols.push(i);
        } else if value_fallback_col.is_none() && (h == "value" || h == "reading") {
            value_fallback_col = Some(i);
        }

        if unit_col.is_none() && (h == "unit" || h == "units") {
            unit_col = Some(i);
        }
        if source_col.is_none()
            && (h == "source" || h == "sourcename" || h == "source name" || h == "device")
        {
            source_col = Some(i);
        }
        if event_type_col.is_none() && h == "event type" {
            event_type_col = Some(i);
        }
        if record_type_col.is_none() && h == "record type" {
            record_type_col = Some(i);
        }
        if lingo_count_col.is_none()
            && (h.contains("lingo_count") || h.contains("lingo count") || h == "lingo")
        {
            lingo_count_col = Some(i);
        }
        if note_col.is_none() && (h == "tag" || h == "note" || h == "notes" || h == "context") {
            note_col = Some(i);
        }
    }

    // Allow a single `Time` header when there is no separate `Date` / `Timestamp` column.
    let (ts_col, time_col) = match (ts_col, time_col) {
        (Some(ts), t) => (ts, t),
        (None, Some(t)) => (t, None),
        (None, None) => return None,
    };

    if glucose_cols.is_empty() {
        if let Some(v_idx) = value_fallback_col {
            glucose_cols.push(v_idx);
        } else {
            return None;
        }
    }

    let detected_monitor = if let Some(m) = hint {
        m
    } else if joined.contains("historic glucose")
        || (joined.contains("device timestamp") && joined.contains("record type"))
    {
        GlucoseMonitor::AbbottFreeStyleLibre
    } else if joined.contains("event type") && joined.contains("glucose value") {
        if joined.contains("stelo") {
            GlucoseMonitor::DexcomStelo
        } else {
            GlucoseMonitor::DexcomClarity
        }
    } else if joined.contains("sensor glucose") || joined.contains("bg reading") {
        GlucoseMonitor::MedtronicCareLink
    } else if joined.contains("eventdatetime") || joined.contains("readings (cgm)") {
        GlucoseMonitor::TandemSource
    } else if joined.contains("cgm glucose value")
        || joined.contains("glooko")
        || joined.contains("tidepool")
    {
        GlucoseMonitor::GlookoTidepool
    } else if joined.contains("lingo")
        || (joined.contains("time of glucose reading") && joined.contains("measurement"))
    {
        GlucoseMonitor::AbbottLingo
    } else if joined.contains("blood glucose")
        || joined.contains("bg (mg/dl)")
        || joined.contains("bg (mmol/l)")
    {
        GlucoseMonitor::FingerstickBgm
    } else if joined.contains("eversense") {
        GlucoseMonitor::SenseonicsEversense
    } else {
        GlucoseMonitor::GenericCsv
    };

    let unit = if saw_mmol {
        GlucoseUnit::MmolL
    } else {
        GlucoseUnit::MgDl
    };
    let unit_explicit = saw_mmol
        || joined.contains("mg/dl")
        || joined.contains("mg_dl")
        || joined.contains("mgdl")
        || unit_col.is_some();

    Some(HeaderSpec {
        monitor: detected_monitor,
        unit,
        unit_explicit,
        method: detected_monitor.default_method(),
        ts_col,
        time_col,
        glucose_cols,
        unit_col,
        source_col,
        event_type_col,
        record_type_col,
        lingo_count_col,
        note_col,
    })
}

fn parse_glucose_cell(cell: &str, default_unit: GlucoseUnit) -> CellParseOutcome {
    let trimmed = cell.trim().trim_matches('"').trim();
    if trimmed.is_empty() {
        return CellParseOutcome::Empty;
    }
    let lower = trimmed.to_ascii_lowercase();

    if matches!(
        lower.as_str(),
        "out of range" | "out-of-range" | "sensor error" | "error" | "n/a" | "na" | "--" | "null"
    ) {
        return CellParseOutcome::OutOfRangeSentinel(trimmed.to_string());
    }

    // Named sensor floor / ceiling sentinels across Dexcom, Stelo, Libre, and Abbott Lingo.
    if lower == "low" || lower == "lo" || lower.starts_with("below 40") {
        return CellParseOutcome::Parsed {
            mg_dl: 40.0,
            bound: GlucoseReadingBound::BelowSensorFloor,
            detected_unit: None,
        };
    }
    if lower.starts_with("below 55")
        || lower == "<55"
        || lower == "< 55"
        || lower.starts_with("<55 ")
        || lower.starts_with("low (<55")
        || lower.starts_with("below 3.1")
        || lower == "<3.1"
        || lower == "< 3.1"
    {
        return CellParseOutcome::Parsed {
            mg_dl: 54.0,
            bound: GlucoseReadingBound::BelowSensorFloor,
            detected_unit: None,
        };
    }
    if lower.starts_with("below 70") || lower == "<70" || lower == "< 70" {
        return CellParseOutcome::Parsed {
            mg_dl: 65.0,
            bound: GlucoseReadingBound::BelowSensorFloor,
            detected_unit: None,
        };
    }
    if lower == "high" || lower == "hi" || lower.starts_with("above 400") {
        return CellParseOutcome::Parsed {
            mg_dl: 400.0,
            bound: GlucoseReadingBound::AboveSensorCeiling,
            detected_unit: None,
        };
    }
    if lower.starts_with("above 250") || lower == ">250" || lower == "> 250" {
        return CellParseOutcome::Parsed {
            mg_dl: 255.0,
            bound: GlucoseReadingBound::AboveSensorCeiling,
            detected_unit: None,
        };
    }
    if lower.starts_with("above 200")
        || lower == ">200"
        || lower == "> 200"
        || lower.starts_with(">200 ")
        || lower.starts_with("high (>200")
        || lower.starts_with("above 11.1")
        || lower == ">11.1"
        || lower == "> 11.1"
    {
        return CellParseOutcome::Parsed {
            mg_dl: 201.0,
            bound: GlucoseReadingBound::AboveSensorCeiling,
            detected_unit: None,
        };
    }

    let (bound, stripped_prefix) = if let Some(rest) = trimmed.strip_prefix('<') {
        (GlucoseReadingBound::BelowSensorFloor, rest.trim())
    } else if let Some(rest) = trimmed.strip_prefix('>') {
        (GlucoseReadingBound::AboveSensorCeiling, rest.trim())
    } else if let Some(rest) = lower.strip_prefix("below ") {
        (GlucoseReadingBound::BelowSensorFloor, rest.trim())
    } else if let Some(rest) = lower.strip_prefix("above ") {
        (GlucoseReadingBound::AboveSensorCeiling, rest.trim())
    } else {
        (GlucoseReadingBound::Measured, trimmed)
    };

    // Strip optional inline units (`115 mg/dL`, `6.4 mmol/L`) and handle European decimal comma.
    let lower_body = stripped_prefix.to_ascii_lowercase();
    let (detected_unit, num_body) = if let Some(pos) = lower_body.find("mmol") {
        (Some(GlucoseUnit::MmolL), stripped_prefix[..pos].trim())
    } else if let Some(pos) = lower_body.find("mg/dl") {
        (Some(GlucoseUnit::MgDl), stripped_prefix[..pos].trim())
    } else if let Some(pos) = lower_body.find("mgdl") {
        (Some(GlucoseUnit::MgDl), stripped_prefix[..pos].trim())
    } else {
        (None, stripped_prefix)
    };

    let normalized_num = if num_body.contains(',') && !num_body.contains('.') {
        num_body.replace(',', ".")
    } else {
        num_body.to_string()
    };

    let effective_unit = detected_unit.unwrap_or(default_unit);
    match normalized_num.parse::<f64>() {
        Ok(raw) if raw.is_finite() => CellParseOutcome::Parsed {
            mg_dl: effective_unit.to_mg_dl(raw),
            bound,
            detected_unit,
        },
        _ => CellParseOutcome::Invalid(trimmed.to_string()),
    }
}

/// Extract an inline timezone offset token (`Z`, `+HH:MM`, `-HH:MM`, `+HHMM`, `-HHMM`) if present.
fn extract_zone_offset_str(ts: &str) -> Option<String> {
    let trimmed = ts.trim();
    if trimmed.ends_with('Z') || trimmed.ends_with('z') {
        return Some("Z".to_string());
    }
    let time_region = trimmed
        .split_once('T')
        .or_else(|| trimmed.split_once(' '))
        .map(|(_, r)| r)?;
    if let Some(pos) = time_region.rfind(['+', '-']) {
        let candidate = time_region[pos..].trim();
        if parse_offset_minutes(candidate).is_some() {
            return Some(candidate.to_string());
        }
    }
    None
}

/// Parse timezone offset strings (`Z`, `+05:30`, `-07:00`, `-0700`, `+0000`) into signed minutes.
fn parse_offset_minutes(offset: &str) -> Option<i64> {
    let s = offset.trim();
    if s.eq_ignore_ascii_case("z") || s.eq_ignore_ascii_case("utc") {
        return Some(0);
    }
    let (sign, rest) = if let Some(r) = s.strip_prefix('+') {
        (1_i64, r)
    } else if let Some(r) = s.strip_prefix('-') {
        (-1_i64, r)
    } else {
        return None;
    };
    let (h, m) = if let Some((hs, ms)) = rest.split_once(':') {
        (hs.parse::<i64>().ok()?, ms.parse::<i64>().ok()?)
    } else if rest.len() == 4 {
        (
            rest[0..2].parse::<i64>().ok()?,
            rest[2..4].parse::<i64>().ok()?,
        )
    } else if rest.len() == 2 {
        (rest.parse::<i64>().ok()?, 0)
    } else {
        return None;
    };
    if !(0..=14).contains(&h) || !(0..60).contains(&m) {
        return None;
    }
    Some(sign * (h * 60 + m))
}

/// When a Health Connect record arrives as UTC (`...Z`) plus a separate `zoneOffset` (`-07:00`),
/// convert its displayed ISO-8601 timestamp into the user's local offset so AGP clock hours and
/// timeline tables reflect real local meal/sleep times while preserving the exact UTC instant.
fn normalize_timestamp_with_zone_offset(ts: &str, zone_offset: Option<&str>) -> String {
    let trimmed = ts.trim();
    let Some(offset_str) = zone_offset else {
        return trimmed.to_string();
    };
    let Some(offset_mins) = parse_offset_minutes(offset_str) else {
        return trimmed.to_string();
    };
    if !trimmed.ends_with('Z') && !trimmed.ends_with('z') {
        return trimmed.to_string();
    }
    let Some(parsed) = parse_timestamp(trimmed, None) else {
        return trimmed.to_string();
    };
    let local_total_mins = parsed.utc_minutes.round() as i64 + offset_mins;
    let days_from_epoch = local_total_mins.div_euclid(1440);
    let mins_in_day = local_total_mins.rem_euclid(1440);
    let hour = mins_in_day / 60;
    let minute = mins_in_day % 60;
    let Some(epoch) = NaiveDate::from_ymd_opt(1970, 1, 1) else {
        return trimmed.to_string();
    };
    let Some(date) = epoch.checked_add_signed(chrono::TimeDelta::days(days_from_epoch)) else {
        return trimmed.to_string();
    };
    let sign = if offset_mins >= 0 { '+' } else { '-' };
    let abs_off = offset_mins.abs();
    let off_h = abs_off / 60;
    let off_m = abs_off % 60;
    format!("{date}T{hour:02}:{minute:02}:00{sign}{off_h:02}:{off_m:02}")
}

/// Parse commercial CGM timestamps with exact Gregorian calendar days and full timezone offsets:
/// - Abbott Lingo ISO-8601 (`2026-02-07T11:38-05:00`, `2026-11-24T08:00:00-07:00`, `...Z`)
/// - Apple Health `export.xml` (`2026-11-25 10:00:00 -0700`)
/// - Local ISO (`2026-11-21 06:00`, `2026-11-21 06:00:00`)
/// - US / EU slash & dash dates (`MM/DD/YYYY HH:MM[:SS] [AM|PM]`, `DD-MM-YYYY HH:MM`)
pub(crate) fn parse_timestamp(
    ts: &str,
    extra_zone_offset: Option<&str>,
) -> Option<ParsedTimestamp> {
    let cleaned = ts.trim().replace('T', " ");
    let mut parts = cleaned.split_whitespace();
    let date_part = parts.next()?;
    let time_part = parts.next()?;
    let third_part = parts.next();

    let (year, month, day) = if date_part.contains('-') {
        let d: Vec<&str> = date_part.split('-').collect();
        if d.len() != 3 {
            return None;
        }
        if d[0].len() == 4 {
            (
                d[0].parse::<i32>().ok()?,
                d[1].parse::<u32>().ok()?,
                d[2].parse::<u32>().ok()?,
            )
        } else {
            // DD-MM-YYYY or MM-DD-YYYY (LibreView US uses MM-DD-YYYY)
            let a = d[0].parse::<u32>().ok()?;
            let b = d[1].parse::<u32>().ok()?;
            let y = d[2].parse::<i32>().ok()?;
            if a > 12 {
                (y, b, a)
            } else {
                (y, a, b)
            }
        }
    } else if date_part.contains('/') {
        let d: Vec<&str> = date_part.split('/').collect();
        if d.len() != 3 {
            return None;
        }
        if d[0].len() == 4 {
            (
                d[0].parse::<i32>().ok()?,
                d[1].parse::<u32>().ok()?,
                d[2].parse::<u32>().ok()?,
            )
        } else {
            // CareLink / US BGM: MM/DD/YYYY
            (
                d[2].parse::<i32>().ok()?,
                d[0].parse::<u32>().ok()?,
                d[1].parse::<u32>().ok()?,
            )
        }
    } else {
        return None;
    };

    let date = NaiveDate::from_ymd_opt(year, month, day)?;
    let epoch = NaiveDate::from_ymd_opt(1970, 1, 1)?;
    let days_since_epoch = (date - epoch).num_days();

    // Split optional inline offset (`Z`, `+HH:MM`, `-HH:MM`, `+HHMM`, `-HHMM`) from `time_part`.
    let (time_core, inline_offset_mins, had_z_suffix) =
        if let Some(stripped) = time_part.strip_suffix(['Z', 'z']) {
            (stripped, Some(0_i64), true)
        } else if let Some(pos) = time_part.rfind(['+', '-']) {
            let candidate = &time_part[pos..];
            if let Some(off) = parse_offset_minutes(candidate) {
                (&time_part[..pos], Some(off), false)
            } else {
                (time_part, None, false)
            }
        } else {
            (time_part, None, false)
        };

    // Third token can be AM/PM or an Apple Health space-separated offset (`-0700`, `+05:30`).
    let mut am_pm: Option<String> = None;
    let mut space_offset_mins: Option<i64> = None;
    if let Some(tok) = third_part {
        let up = tok.to_ascii_uppercase();
        if up == "AM" || up == "PM" {
            am_pm = Some(up);
        } else if let Some(off) = parse_offset_minutes(tok) {
            space_offset_mins = Some(off);
        }
    }

    let mut t = time_core.split(':');
    let mut hour: i64 = t.next()?.parse().ok()?;
    let minute: i64 = t.next()?.parse().ok()?;
    let second: f64 = t
        .next()
        .and_then(|s| s.parse::<f64>().ok())
        .filter(|s| s.is_finite() && (0.0..60.0).contains(s))
        .unwrap_or(0.0);

    if let Some(meridiem) = am_pm {
        if meridiem == "PM" && hour < 12 {
            hour += 12;
        } else if meridiem == "AM" && hour == 12 {
            hour = 0;
        }
    }
    if !(0..24).contains(&hour) || !(0..60).contains(&minute) {
        return None;
    }

    let wall_minutes = (days_since_epoch * 1440 + hour * 60 + minute) as f64 + (second / 60.0);
    let applied_offset = inline_offset_mins.or(space_offset_mins);
    let utc_minutes = match applied_offset {
        Some(off) => wall_minutes - off as f64,
        None => wall_minutes,
    };

    let (local_date, local_hour) = if had_z_suffix {
        if let Some(extra_off) = extra_zone_offset.and_then(parse_offset_minutes) {
            let local_mins = utc_minutes.round() as i64 + extra_off;
            let days_from_epoch = local_mins.div_euclid(1440);
            let adjusted_date = epoch
                .checked_add_signed(chrono::TimeDelta::days(days_from_epoch))
                .unwrap_or(date);
            let adjusted_hour = (local_mins.div_euclid(60).rem_euclid(24)) as u8;
            (adjusted_date, adjusted_hour)
        } else {
            (date, hour as u8)
        }
    } else {
        (date, hour as u8)
    };

    Some(ParsedTimestamp {
        utc_minutes,
        local_date,
        local_hour,
    })
}

pub(crate) fn extract_clock_hour(ts: &str, fallback_offset: f64) -> u8 {
    if let Some(parsed) = parse_timestamp(ts, None) {
        return parsed.local_hour;
    }
    ((fallback_offset.floor() as i64).rem_euclid(24)) as u8
}

pub(crate) fn percentile_sorted(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    if sorted.len() == 1 {
        return sorted[0];
    }
    let rank = (p.clamp(0.0, 1.0)) * (sorted.len() - 1) as f64;
    let lo = rank.floor() as usize;
    let hi = rank.ceil() as usize;
    let frac = rank - lo as f64;
    sorted[lo] * (1.0 - frac) + sorted[hi] * frac
}

/// Compute gap-aware Mean Amplitude of Glycemic Excursions (MAGE, `> 1 SD`) using classic
/// significant turning-point hysteresis (`> 1 SD` reversal) within each continuous segment
/// so 1 mg/dL sensor jitter does not reset swings and multi-hour sensor gaps are never bridged.
fn compute_mage(readings: &[LingoReading], std_dev: f64) -> f64 {
    if readings.len() < 3 || std_dev <= 0.5 {
        return 0.0;
    }

    // Break segments at > 2.0h for continuous CGM traces; for coarse multi-hour sample/BGM
    // traces whose own median interval exceeds 1h, scale the break threshold accordingly.
    let mut pos_intervals: Vec<f64> = readings
        .windows(2)
        .map(|w| (w[1].hour_offset - w[0].hour_offset).abs())
        .filter(|d| *d > 0.001)
        .collect();
    pos_intervals.sort_by(f64::total_cmp);
    let median_h = if pos_intervals.is_empty() {
        5.0 / 60.0
    } else {
        percentile_sorted(&pos_intervals, 0.50)
    };
    let max_gap_h = if median_h <= 1.0 {
        2.0
    } else {
        (median_h * 3.0).max(8.0)
    };

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum SwingDir {
        Up,
        Down,
    }

    let mut swings: Vec<f64> = Vec::new();
    let mut seg_start = 0usize;
    while seg_start < readings.len() {
        let mut seg_end = seg_start;
        while seg_end + 1 < readings.len() {
            let gap_h = (readings[seg_end + 1].hour_offset - readings[seg_end].hour_offset).abs();
            if gap_h > max_gap_h + 1e-6 {
                break;
            }
            seg_end += 1;
        }
        let seg = &readings[seg_start..=seg_end];
        if seg.len() >= 2 {
            let mut turning_points: Vec<f64> = Vec::new();
            let mut cur_min = seg[0].glucose_mg_dl;
            let mut cur_max = seg[0].glucose_mg_dl;
            let mut dir: Option<SwingDir> = None;

            for r in &seg[1..] {
                let g = r.glucose_mg_dl;
                match dir {
                    None => {
                        if g < cur_min {
                            cur_min = g;
                        }
                        if g > cur_max {
                            cur_max = g;
                        }
                        if g - cur_min > std_dev {
                            turning_points.push(cur_min);
                            dir = Some(SwingDir::Up);
                            cur_max = g;
                        } else if cur_max - g > std_dev {
                            turning_points.push(cur_max);
                            dir = Some(SwingDir::Down);
                            cur_min = g;
                        }
                    }
                    Some(SwingDir::Up) => {
                        if g > cur_max {
                            cur_max = g;
                        } else if cur_max - g > std_dev {
                            turning_points.push(cur_max);
                            dir = Some(SwingDir::Down);
                            cur_min = g;
                        }
                    }
                    Some(SwingDir::Down) => {
                        if g < cur_min {
                            cur_min = g;
                        } else if g - cur_min > std_dev {
                            turning_points.push(cur_min);
                            dir = Some(SwingDir::Up);
                            cur_max = g;
                        }
                    }
                }
            }
            match dir {
                Some(SwingDir::Up) => turning_points.push(cur_max),
                Some(SwingDir::Down) => turning_points.push(cur_min),
                None => {}
            }
            for w in turning_points.windows(2) {
                let delta = (w[1] - w[0]).abs();
                if delta > std_dev {
                    swings.push(delta);
                }
            }
        }
        seg_start = seg_end + 1;
    }

    if swings.is_empty() {
        0.0
    } else {
        swings.iter().sum::<f64>() / swings.len() as f64
    }
}

/// Compute the effective nominal window span in hours for `window` against a full trace,
/// capped by the full trace's elapsed span so shorter imports are not penalized.
pub fn nominal_window_hours_for_trace(
    full_readings: &[LingoReading],
    window: GlucoseWindow,
) -> Option<f64> {
    let max_h = window.max_hours()?;
    let full_span = full_readings
        .last()
        .zip(full_readings.first())
        .map(|(last, first)| (last.hour_offset - first.hour_offset).max(0.0))
        .unwrap_or(0.0);
    if full_span <= 0.0 {
        None
    } else {
        Some(max_h.min(full_span))
    }
}

/// Compute comprehensive ADA/ATTD clinical summary + 24-hour AGP percentile bins.
pub fn summarize_commercial_glucose(
    readings: &[LingoReading],
    monitor: GlucoseMonitor,
    source_unit: GlucoseUnit,
    method: MeasurementMethod,
) -> Option<ClinicalGlucoseSummary> {
    summarize_commercial_glucose_with_window(readings, monitor, source_unit, method, None)
}

/// Compute comprehensive ADA/ATTD clinical summary + 24-hour AGP percentile bins, using an
/// optional `nominal_window_hours` floor (e.g., from [`nominal_window_hours_for_trace`]) so
/// `estimated_active_wear_pct` is not inflated when a trailing window cutoff lands inside a
/// multi-day sensor gap.
pub fn summarize_commercial_glucose_with_window(
    readings: &[LingoReading],
    monitor: GlucoseMonitor,
    source_unit: GlucoseUnit,
    method: MeasurementMethod,
    nominal_window_hours: Option<f64>,
) -> Option<ClinicalGlucoseSummary> {
    if readings.is_empty() {
        return None;
    }

    let n = readings.len() as f64;
    let mut sorted_all: Vec<f64> = readings.iter().map(|r| r.glucose_mg_dl).collect();
    sorted_all.sort_by(f64::total_cmp);

    let mean = sorted_all.iter().sum::<f64>() / n;
    let median = percentile_sorted(&sorted_all, 0.50);
    let min_v = sorted_all[0];
    let max_v = sorted_all[sorted_all.len() - 1];
    let variance = sorted_all.iter().map(|g| (g - mean).powi(2)).sum::<f64>() / n;
    let std_dev = variance.sqrt();
    let cv_percent = if mean > 0.0 {
        (std_dev / mean) * 100.0
    } else {
        0.0
    };
    let gmi_percent = 3.31 + 0.02392 * mean;

    let mut very_low = 0usize;
    let mut low_54_69 = 0usize;
    let mut tight_70_140 = 0usize;
    let mut tir_70_180 = 0usize;
    let mut high_181_250 = 0usize;
    let mut very_high_250 = 0usize;
    let mut spikes_140 = 0usize;
    let mut floor_capped_readings = 0usize;
    let mut ceiling_capped_readings = 0usize;

    for r in readings {
        let g = r.glucose_mg_dl;
        if g < 54.0 {
            very_low += 1;
        } else if g < 70.0 {
            low_54_69 += 1;
        } else if g <= 180.0 {
            tir_70_180 += 1;
            if g <= 140.0 {
                tight_70_140 += 1;
            }
        } else if g <= 250.0 {
            high_181_250 += 1;
        } else {
            very_high_250 += 1;
        }
        if g > 140.0 {
            spikes_140 += 1;
        }
        match effective_reading_bound(r, monitor) {
            GlucoseReadingBound::BelowSensorFloor => floor_capped_readings += 1,
            GlucoseReadingBound::AboveSensorCeiling => ceiling_capped_readings += 1,
            GlucoseReadingBound::Measured => {}
        }
    }

    let tbr_very_low_lt_54_pct = (very_low as f64 / n) * 100.0;
    let tbr_low_54_69_pct = (low_54_69 as f64 / n) * 100.0;
    let titr_70_140_pct = (tight_70_140 as f64 / n) * 100.0;
    let tir_70_180_pct = (tir_70_180 as f64 / n) * 100.0;
    let tar_high_181_250_pct = (high_181_250 as f64 / n) * 100.0;
    let tar_very_high_gt_250_pct = (very_high_250 as f64 / n) * 100.0;
    let floor_capped_pct = (floor_capped_readings as f64 / n) * 100.0;
    let ceiling_capped_pct = (ceiling_capped_readings as f64 / n) * 100.0;

    let gri_score = (3.0 * tbr_very_low_lt_54_pct
        + 2.4 * tbr_low_54_69_pct
        + 1.6 * tar_very_high_gt_250_pct
        + 0.8 * tar_high_181_250_pct)
        .clamp(0.0, 100.0);
    let gri_zone = GriZone::from_score(gri_score);
    let mage_mg_dl = compute_mage(readings, std_dev);

    let explicit_lc: f64 = readings.iter().filter_map(|r| r.lingo_count).sum();
    let total_lingo_count = if explicit_lc > 0.0 {
        explicit_lc
    } else {
        readings
            .iter()
            .filter(|r| r.glucose_mg_dl > 140.0)
            .map(|r| ((r.glucose_mg_dl - 140.0) / 10.0).max(1.0))
            .sum::<f64>()
            .round()
    };

    // Estimate active sensor wear % from median sampling interval vs total elapsed span.
    let mut intervals: Vec<f64> = readings
        .windows(2)
        .map(|w| (w[1].hour_offset - w[0].hour_offset).abs())
        .filter(|d| *d > 0.001)
        .collect();
    intervals.sort_by(f64::total_cmp);
    let actual_span_h = readings
        .last()
        .zip(readings.first())
        .map(|(last, first)| (last.hour_offset - first.hour_offset).max(0.0))
        .unwrap_or(0.0);
    let span_h = match nominal_window_hours {
        Some(nom) if nom.is_finite() && nom > 0.0 => actual_span_h.max(nom),
        _ => actual_span_h,
    };
    let estimated_active_wear_pct =
        if method == MeasurementMethod::CapillaryBgm || intervals.is_empty() || span_h <= 0.0 {
            100.0
        } else {
            let cadence = percentile_sorted(&intervals, 0.50).max(1.0 / 60.0);
            let expected = (span_h / cadence) + 1.0;
            ((n / expected) * 100.0).clamp(0.0, 100.0)
        };

    // Build 24-hour AGP percentile bins.
    let mut hour_buckets: [Vec<f64>; 24] = std::array::from_fn(|_| Vec::new());
    for r in readings {
        let h = usize::from(extract_clock_hour(&r.timestamp, r.hour_offset).min(23));
        hour_buckets[h].push(r.glucose_mg_dl);
    }
    let agp_hourly = hour_buckets
        .into_iter()
        .enumerate()
        .map(|(h, mut vals)| {
            vals.sort_by(f64::total_cmp);
            AgpHourBin {
                hour: h as u8,
                count: vals.len(),
                p5: percentile_sorted(&vals, 0.05),
                p25: percentile_sorted(&vals, 0.25),
                p50: percentile_sorted(&vals, 0.50),
                p75: percentile_sorted(&vals, 0.75),
                p95: percentile_sorted(&vals, 0.95),
            }
        })
        .collect();

    Some(ClinicalGlucoseSummary {
        monitor,
        source_unit,
        method,
        reading_count: readings.len(),
        mean_mg_dl: mean,
        median_mg_dl: median,
        min_mg_dl: min_v,
        max_mg_dl: max_v,
        std_dev_mg_dl: std_dev,
        cv_percent,
        gmi_percent,
        gri_score,
        gri_zone,
        mage_mg_dl,
        tbr_very_low_lt_54_pct,
        tbr_low_54_69_pct,
        titr_70_140_pct,
        tir_70_180_pct,
        tar_high_181_250_pct,
        tar_very_high_gt_250_pct,
        spike_readings_gt_140: spikes_140,
        high_readings_gt_180: high_181_250 + very_high_250,
        low_readings_lt_70: very_low + low_54_69,
        very_low_readings_lt_54: very_low,
        floor_capped_readings,
        ceiling_capped_readings,
        floor_capped_pct,
        ceiling_capped_pct,
        total_lingo_count,
        estimated_active_wear_pct,
        agp_hourly,
    })
}

/// Synthetic 3-day Dexcom G7 Clarity export demonstrating EGV filtering, `"Low"`/`"High"` bounds, and AGP ribbons.
pub const SAMPLE_DEXCOM_CLARITY_CSV: &str = "\
Index,Timestamp (YYYY-MM-DDThh:mm:ss),Event Type,Event Subtype,Patient Info,Device Info,Source Device ID,Glucose Value (mg/dL),Insulin Value (u),Carb Value (grams),Duration (hh:mm:ss),Glucose Rate of Change (mg/dL/min),Transmitter Time (Long Integer),Transmitter ID
1,,FirstName,,Synthetic,,Dexcom G7,,,,,,,
2,2026-11-19T06:00:00,EGV,,,,,94,,,,0.1,,
3,2026-11-19T08:00:00,EGV,,,,,138,,,,1.4,,
4,2026-11-19T12:30:00,EGV,,,,,172,,,,1.8,,
5,2026-11-19T15:00:00,EGV,,,,,86,,,,-1.6,,
6,2026-11-19T19:00:00,EGV,,,,,148,,,,1.1,,
7,2026-11-19T23:00:00,EGV,,,,,102,,,,-0.3,,
8,2026-11-20T06:00:00,EGV,,,,,88,,,,0.0,,
9,2026-11-20T08:00:00,EGV,,,,,156,,,,1.9,,
10,2026-11-20T12:30:00,EGV,,,,,214,,,,2.4,,
11,2026-11-20T15:00:00,EGV,,,,,64,,,,-2.1,,
12,2026-11-20T19:00:00,EGV,,,,,162,,,,1.2,,
13,2026-11-20T23:00:00,EGV,,,,,112,,,,-0.4,,
14,2026-11-21T06:00:00,EGV,,,,,91,,,,0.1,,
15,2026-11-21T08:00:00,EGV,,,,,129,,,,0.9,,
16,2026-11-21T12:30:00,EGV,,,,,188,,,,2.0,,
17,2026-11-21T15:00:00,EGV,,,,,74,,,,-1.8,,
18,2026-11-21T19:00:00,EGV,,,,,134,,,,0.8,,
19,2026-11-21T23:00:00,EGV,,,,,96,,,,-0.2,,";

/// Synthetic Abbott FreeStyle Libre 3 (LibreView) export in `mmol/L` with real preamble and Record Types `0`/`1`.
pub const SAMPLE_LIBREVIEW_MMOL_CSV: &str = "\
Glucose Data,Generated on,2026-11-21 18:00 UTC,Generated by,Synthetic LibreView
Device,Serial Number,Device Timestamp,Record Type,Historic Glucose mmol/L,Scan Glucose mmol/L,Notes
FreeStyle Libre 3,SN-0001,2026-11-20 06:00,0,5.1,,Fasting
FreeStyle Libre 3,SN-0001,2026-11-20 09:00,0,7.8,,Breakfast peak
FreeStyle Libre 3,SN-0001,2026-11-20 13:00,1,,9.4,Post-lunch scan
FreeStyle Libre 3,SN-0001,2026-11-20 16:00,0,4.2,,Afternoon dip
FreeStyle Libre 3,SN-0001,2026-11-20 20:00,0,6.7,,Evening steady
FreeStyle Libre 3,SN-0001,2026-11-21 06:00,0,4.9,,Fasting
FreeStyle Libre 3,SN-0001,2026-11-21 09:00,0,6.6,,Protein breakfast
FreeStyle Libre 3,SN-0001,2026-11-21 13:00,0,8.2,,Lunch
FreeStyle Libre 3,SN-0001,2026-11-21 16:00,0,5.4,,Walk
FreeStyle Libre 3,SN-0001,2026-11-21 20:00,0,6.1,,Dinner";

/// Synthetic Medtronic MiniMed 780G / Guardian 4 CareLink export with section divider.
pub const SAMPLE_MEDTRONIC_CARELINK_CSV: &str = "\
-------,MiniMed 780G / Guardian 4 CareLink Export,-------
Index,Date,Time,Sensor Glucose (mg/dL),BG Reading (mg/dL)
1,11/20/2026,06:00:00,98,
2,11/20/2026,09:00:00,146,
3,11/20/2026,13:00:00,192,190
4,11/20/2026,16:00:00,118,
5,11/20/2026,20:00:00,134,
6,11/21/2026,06:00:00,92,94
7,11/21/2026,09:00:00,138,
8,11/21/2026,13:00:00,168,
9,11/21/2026,16:00:00,104,
10,11/21/2026,20:00:00,122,";

/// Synthetic fingerstick capillary BGM log (Accu-Chek / OneTouch / Contour).
pub const SAMPLE_BGM_FINGERSTICK_CSV: &str = "\
Date,Time,Blood Glucose (mg/dL),Notes
2026-11-20,07:00,92,Fasting fingerstick
2026-11-20,10:00,136,2h post-breakfast
2026-11-20,14:00,158,2h post-lunch
2026-11-20,21:00,112,Bedtime
2026-11-21,07:00,88,Fasting fingerstick
2026-11-21,10:00,124,2h post-breakfast
2026-11-21,14:00,142,2h post-lunch
2026-11-21,21:00,106,Bedtime";

/// Synthetic `dabney.health_bridge.v1` JSON sample matching the future Android Health Connect /
/// Apple HealthKit `BloodGlucoseRecord` bridge payload for Abbott Lingo.
pub const SAMPLE_HEALTH_BRIDGE_JSON: &str = r#"{
  "schema": "dabney.health_bridge.v1",
  "platform": "android_health_connect",
  "exported_at": "2026-11-24T22:00:00-07:00",
  "records": [
    { "time": "2026-11-24T13:00:00Z", "zoneOffset": "-07:00", "levelMgDl": 86, "relationToMeal": "fasting", "specimenSource": "interstitial_fluid", "dataOrigin": "com.abbott.lingo" },
    { "time": "2026-11-24T13:05:00Z", "zoneOffset": "-07:00", "levelMgDl": 88, "specimenSource": "interstitial_fluid", "dataOrigin": "com.abbott.lingo" },
    { "time": "2026-11-24T15:00:00Z", "zoneOffset": "-07:00", "levelMgDl": 124, "relationToMeal": "after_meal", "specimenSource": "interstitial_fluid", "dataOrigin": "com.abbott.lingo" },
    { "time": "2026-11-24T15:05:00Z", "zoneOffset": "-07:00", "levelMgDl": 132, "specimenSource": "interstitial_fluid", "dataOrigin": "com.abbott.lingo" },
    { "time": "2026-11-24T19:30:00Z", "zoneOffset": "-07:00", "levelMgDl": 158, "relationToMeal": "after_meal", "specimenSource": "interstitial_fluid", "dataOrigin": "com.abbott.lingo" },
    { "time": "2026-11-24T20:30:00Z", "zoneOffset": "-07:00", "levelMgDl": 94, "specimenSource": "interstitial_fluid", "dataOrigin": "com.abbott.lingo" },
    { "time": "2026-11-25T01:00:00Z", "zoneOffset": "-07:00", "levelMgDl": 112, "relationToMeal": "after_meal", "specimenSource": "interstitial_fluid", "dataOrigin": "com.abbott.lingo" },
    { "time": "2026-11-25T05:00:00Z", "zoneOffset": "-07:00", "levelMgDl": 90, "specimenSource": "interstitial_fluid", "dataOrigin": "com.abbott.lingo" }
  ]
}"#;

/// Synthetic Apple Health `export.xml` snippet showing Abbott Lingo iOS HealthKit records.
pub const SAMPLE_APPLE_HEALTH_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<HealthData locale="en_US">
  <Record type="HKQuantityTypeIdentifierHeartRate" sourceName="Apple Watch" unit="count/min" value="62" startDate="2026-11-24 06:00:00 -0700" endDate="2026-11-24 06:00:00 -0700" />
  <Record type="HKQuantityTypeIdentifierBloodGlucose" sourceName="Lingo" unit="mg/dL" value="85" startDate="2026-11-24 06:00:00 -0700" endDate="2026-11-24 06:00:00 -0700" />
  <Record type="HKQuantityTypeIdentifierBloodGlucose" sourceName="Lingo" unit="mg/dL" value="91" startDate="2026-11-24 06:05:00 -0700" endDate="2026-11-24 06:05:00 -0700" />
  <Record type="HKQuantityTypeIdentifierBloodGlucose" sourceName="Lingo" unit="mg/dL" value="146" startDate="2026-11-24 08:30:00 -0700" endDate="2026-11-24 08:30:00 -0700" />
  <Record type="HKQuantityTypeIdentifierBloodGlucose" sourceName="Lingo" unit="mg/dL" value="104" startDate="2026-11-24 11:00:00 -0700" endDate="2026-11-24 11:00:00 -0700" />
  <Record type="HKQuantityTypeIdentifierBloodGlucose" sourceName="Lingo" unit="mg/dL" value="96" startDate="2026-11-24 18:00:00 -0700" endDate="2026-11-24 18:00:00 -0700" />
</HealthData>"#;

/// Return a built-in synthetic sample CSV for quick one-click testing of major commercial ecosystems.
pub fn sample_csv_for_monitor(monitor: GlucoseMonitor) -> &'static str {
    match monitor {
        GlucoseMonitor::DexcomClarity | GlucoseMonitor::DexcomStelo => SAMPLE_DEXCOM_CLARITY_CSV,
        GlucoseMonitor::AbbottFreeStyleLibre => SAMPLE_LIBREVIEW_MMOL_CSV,
        GlucoseMonitor::MedtronicCareLink => SAMPLE_MEDTRONIC_CARELINK_CSV,
        GlucoseMonitor::FingerstickBgm => SAMPLE_BGM_FINGERSTICK_CSV,
        GlucoseMonitor::AbbottLingo => SAMPLE_LINGO_6MO_CSV,
        GlucoseMonitor::SenseonicsEversense
        | GlucoseMonitor::TandemSource
        | GlucoseMonitor::GlookoTidepool
        | GlucoseMonitor::GenericCsv => SAMPLE_LINGO_CSV,
    }
}

/// The synthetic check-ins that belong on the timeline of [`sample_csv_for_monitor`]'s sample.
pub fn sample_checkins_for_monitor(monitor: GlucoseMonitor) -> Vec<MentalHealthCheckIn> {
    match monitor {
        GlucoseMonitor::AbbottLingo => sample_lingo_6mo_checkins(),
        GlucoseMonitor::DexcomClarity
        | GlucoseMonitor::DexcomStelo
        | GlucoseMonitor::AbbottFreeStyleLibre
        | GlucoseMonitor::MedtronicCareLink
        | GlucoseMonitor::FingerstickBgm
        | GlucoseMonitor::SenseonicsEversense
        | GlucoseMonitor::TandemSource
        | GlucoseMonitor::GlookoTidepool
        | GlucoseMonitor::GenericCsv => sample_checkins(),
    }
}

/// ADA/ATTD 5-Band Time-in-Range stacked bar + target compliance strip.
#[component]
pub fn AdaTimeInRangeBar(summary: ClinicalGlucoseSummary) -> impl IntoView {
    use crate::info_tip::InfoTip;

    let vlow = summary.tbr_very_low_lt_54_pct;
    let low = summary.tbr_low_54_69_pct;
    let tight = summary.titr_70_140_pct;
    let upper_in_range = (summary.tir_70_180_pct - summary.titr_70_140_pct).max(0.0);
    let high = summary.tar_high_181_250_pct;
    let vhigh = summary.tar_very_high_gt_250_pct;
    let bar_aria_label = format!(
        "5-band glucose distribution: Very Low under 54 is {vlow:.1}%, Low 54 to 69 is {low:.1}%, Tight 70 to 140 is {tight:.1}%, Upper in-range 141 to 180 is {upper_in_range:.1}%, High 181 to 250 is {high:.1}%, Very High over 250 is {vhigh:.1}%."
    );

    view! {
        <div class="space-y-3">
            <div
                role="img"
                aria-label=bar_aria_label
                class="flex h-6 w-full overflow-hidden rounded-full border border-white/10 bg-slate-900"
            >
                <div style=format!("width: {vlow:.2}%") class="bg-red-500 transition-[width] duration-300" title=format!("<54 mg/dL: {vlow:.1}%")></div>
                <div style=format!("width: {low:.2}%") class="bg-rose-400 transition-[width] duration-300" title=format!("54–69 mg/dL: {low:.1}%")></div>
                <div style=format!("width: {tight:.2}%") class="bg-emerald-400 transition-[width] duration-300" title=format!("70–140 tight: {tight:.1}%")></div>
                <div style=format!("width: {upper_in_range:.2}%") class="bg-teal-500/80 transition-[width] duration-300" title=format!("141–180 in-range: {upper_in_range:.1}%")></div>
                <div style=format!("width: {high:.2}%") class="bg-amber-400 transition-[width] duration-300" title=format!("181–250 high: {high:.1}%")></div>
                <div style=format!("width: {vhigh:.2}%") class="bg-orange-500 transition-[width] duration-300" title=format!(">250 very high: {vhigh:.1}%")></div>
            </div>
            <div class="grid grid-cols-2 gap-2 text-xs sm:grid-cols-3 lg:grid-cols-6">
                <div class="rounded-lg border border-white/5 bg-slate-950/60 px-2.5 py-1.5">
                    <span class="font-semibold text-red-400">{format!("{vlow:.1}%")}</span>
                    <p class="mt-0.5 flex items-center text-[11px] text-slate-400">
                        <span>"<54 (target <1%)"</span>
                        <InfoTip
                            term="TBR <54"
                            text="Time Below Range Level 2 (<54 mg/dL): clinically significant low glucose. International consensus recommends staying under 1% (<15 min/day)."
                        />
                    </p>
                </div>
                <div class="rounded-lg border border-white/5 bg-slate-950/60 px-2.5 py-1.5">
                    <span class="font-semibold text-rose-300">{format!("{low:.1}%")}</span>
                    <p class="mt-0.5 flex items-center text-[11px] text-slate-400">
                        <span>"54–69 (<70 <4%)"</span>
                        <InfoTip
                            term="TBR 54–69"
                            text="Time Below Range Level 1 (54–69 mg/dL): mild low glucose. Combined time below 70 mg/dL should stay under 4% (<1 hr/day)."
                        />
                    </p>
                </div>
                <div class="rounded-lg border border-white/5 bg-slate-950/60 px-2.5 py-1.5">
                    <span class="font-semibold text-emerald-300">{format!("{tight:.1}%")}</span>
                    <p class="mt-0.5 flex items-center text-[11px] text-slate-400">
                        <span>"70–140 Tight"</span>
                        <InfoTip
                            term="TITR"
                            text="Time in Tight Range (70–140 mg/dL): narrower physiologic target band often used for metabolic stability and wellness tracking."
                        />
                    </p>
                </div>
                <div class="rounded-lg border border-white/5 bg-slate-950/60 px-2.5 py-1.5">
                    <span class="font-semibold text-teal-300">{format!("{:.1}%", summary.tir_70_180_pct)}</span>
                    <p class="mt-0.5 flex items-center text-[11px] text-slate-400">
                        <span>"70–180 (≥70%)"</span>
                        <InfoTip
                            term="TIR"
                            text="Time in Range (70–180 mg/dL): standard ADA/ATTD clinical target band. Most adults aim for at least 70% (>16.8 hrs/day)."
                        />
                    </p>
                </div>
                <div class="rounded-lg border border-white/5 bg-slate-950/60 px-2.5 py-1.5">
                    <span class="font-semibold text-amber-300">{format!("{high:.1}%")}</span>
                    <p class="mt-0.5 flex items-center text-[11px] text-slate-400">
                        <span>"181–250 (<25%)"</span>
                        <InfoTip
                            term="TAR 181–250"
                            text="Time Above Range Level 1 (181–250 mg/dL): elevated glucose. Combined time above 180 mg/dL is typically targeted under 25%."
                        />
                    </p>
                </div>
                <div class="rounded-lg border border-white/5 bg-slate-950/60 px-2.5 py-1.5">
                    <span class="font-semibold text-orange-400">{format!("{vhigh:.1}%")}</span>
                    <p class="mt-0.5 flex items-center text-[11px] text-slate-400">
                        <span>">250 (target <5%)"</span>
                        <InfoTip
                            term="TAR >250"
                            text="Time Above Range Level 2 (>250 mg/dL): marked hyperglycemia. International consensus recommends keeping this under 5%."
                        />
                    </p>
                </div>
            </div>
        </div>
    }
}

/// 24-Hour Ambulatory Glucose Profile (AGP) percentile ribbon SVG (`p5–p95`, `p25–p75`, `p50`).
#[component]
pub fn AgpPercentileChart(
    bins: Vec<AgpHourBin>,
    /// Optional sensor ceiling (mg/dL) to draw as a dashed orange reference hairline ONLY when the
    /// summarized window actually contains ceiling-capped readings.
    #[prop(optional)]
    sensor_ceiling_mg_dl: Option<f64>,
) -> impl IntoView {
    let width = 760.0_f64;
    let height = 210.0_f64;
    let pad_l = 44.0_f64;
    let pad_r = 20.0_f64;
    let pad_t = 16.0_f64;
    let pad_b = 26.0_f64;
    let plot_w = width - pad_l - pad_r;
    let plot_h = height - pad_t - pad_b;

    let mut populated: Vec<AgpHourBin> = bins.into_iter().filter(|b| b.count > 0).collect();
    populated.sort_by_key(|b| b.hour);
    let sensor_ceiling = sensor_ceiling_mg_dl.filter(|v| v.is_finite() && *v > 0.0);
    let (min_g, max_g) = glucose_axis_domain(
        populated
            .iter()
            .flat_map(|b| [b.p5, b.p95])
            .chain([70.0, 180.0])
            .chain(sensor_ceiling),
    );

    let x_for = |h: u8| pad_l + (f64::from(h.min(23)) / 23.0) * plot_w;
    let y_for = |g: f64| {
        let clamped = g.clamp(min_g, max_g);
        pad_t + plot_h - ((clamped - min_g) / (max_g - min_g)) * plot_h
    };
    let grid_ticks: Vec<(f64, f64)> = glucose_axis_ticks((min_g, max_g))
        .into_iter()
        .map(|g| (g, y_for(g)))
        .filter(|&(_, y)| {
            [70.0, 140.0, 180.0]
                .iter()
                .all(|&r| (y - y_for(r)).abs() > 12.0)
        })
        .collect();

    // Split populated hours across >2h missing blocks (matching `DEFAULT_CHART_GAP_HOURS`) so
    // sparse/BGM traces never draw phantom ribbons across multi-hour unmeasured overnight/midday
    // gaps, while 1–2h CGM samples connect smoothly and isolated 1-hour bins render visible
    // percentile whiskers + median dots instead of a blank plot.
    let mut contiguous_runs: Vec<Vec<AgpHourBin>> = Vec::new();
    for bin in &populated {
        if let Some(last_run) = contiguous_runs.last_mut() {
            if last_run
                .last()
                .is_some_and(|prev| bin.hour.saturating_sub(prev.hour) <= 2)
            {
                last_run.push(bin.clone());
                continue;
            }
        }
        contiguous_runs.push(vec![bin.clone()]);
    }

    let build_ribbon_for = |run: &[AgpHourBin],
                            upper: &dyn Fn(&AgpHourBin) -> f64,
                            lower: &dyn Fn(&AgpHourBin) -> f64|
     -> String {
        if run.len() < 2 {
            return String::new();
        }
        let mut pts: Vec<String> = run
            .iter()
            .map(|b| format!("{:.1},{:.1}", x_for(b.hour), y_for(upper(b))))
            .collect();
        for b in run.iter().rev() {
            pts.push(format!("{:.1},{:.1}", x_for(b.hour), y_for(lower(b))));
        }
        pts.join(" ")
    };

    let outer_polys: Vec<String> = contiguous_runs
        .iter()
        .filter(|r| r.len() >= 2)
        .map(|r| build_ribbon_for(r, &|b| b.p95, &|b| b.p5))
        .collect();
    let iqr_polys: Vec<String> = contiguous_runs
        .iter()
        .filter(|r| r.len() >= 2)
        .map(|r| build_ribbon_for(r, &|b| b.p75, &|b| b.p25))
        .collect();
    let median_lines: Vec<String> = contiguous_runs
        .iter()
        .filter(|r| r.len() >= 2)
        .map(|r| {
            r.iter()
                .map(|b| format!("{:.1},{:.1}", x_for(b.hour), y_for(b.p50)))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect();
    let isolated_bins: Vec<AgpHourBin> = contiguous_runs
        .iter()
        .filter(|r| r.len() == 1)
        .map(|r| r[0].clone())
        .collect();

    let y70 = y_for(70.0);
    let y140 = y_for(140.0);
    let y180 = y_for(180.0);
    let y_ceiling = sensor_ceiling.map(&y_for);
    let half_hour_step = (plot_w / 23.0) * 0.5;
    let plot_right = width - pad_r;
    let selected_hour = RwSignal::new(Option::<u8>::None);
    let stored_bins = StoredValue::new(populated.clone());

    view! {
        <div class="space-y-2.5">
            <svg
                viewBox="0 0 760 210"
                class="w-full"
                role="img"
                aria-label="24-hour Ambulatory Glucose Profile showing 5th-95th and 25th-75th percentile ribbons and median"
            >
                // Round value gridlines, labelled where they clear the 70/140/180 labels
                {grid_ticks.into_iter().map(|(g, y)| view! {
                    <line x1=pad_l y1=y x2=plot_right y2=y stroke="rgba(148, 163, 184, 0.12)" />
                    <text x="6" y=y + 4.0 fill="#94a3b8" font-size="10">{format!("{g:.0}")}</text>
                }).collect_view()}
                // 70-180 clinical target band + 70-140 tight sub-band
                <rect x=pad_l y=y180 width=plot_w height=(y70 - y180).max(1.0) fill="rgba(45, 212, 191, 0.06)" />
                <rect x=pad_l y=y140 width=plot_w height=(y70 - y140).max(1.0) fill="rgba(52, 211, 153, 0.09)" />
                {y_ceiling.map(|yc| view! {
                    <line
                        x1=pad_l
                        y1=yc
                        x2=plot_right
                        y2=yc
                        stroke="rgba(249, 115, 22, 0.50)"
                        stroke-dasharray="2 3"
                    />
                })}
                <line x1=pad_l y1=y180 x2=plot_right y2=y180 stroke="rgba(251, 191, 36, 0.35)" stroke-dasharray="4 4" />
                <line x1=pad_l y1=y140 x2=plot_right y2=y140 stroke="rgba(52, 211, 153, 0.35)" stroke-dasharray="3 3" />
                <line x1=pad_l y1=y70 x2=plot_right y2=y70 stroke="rgba(251, 113, 133, 0.40)" stroke-dasharray="4 4" />

                // Outer 5th-95th ribbons (broken across empty clock hours)
                {outer_polys.into_iter().map(|pts| view! {
                    <polygon points=pts fill="rgba(34, 211, 238, 0.14)" />
                }).collect_view()}
                // Inner 25th-75th IQR ribbons
                {iqr_polys.into_iter().map(|pts| view! {
                    <polygon points=pts fill="rgba(34, 211, 238, 0.30)" />
                }).collect_view()}
                // 50th percentile median curves
                {median_lines.into_iter().map(|pts| view! {
                    <polyline
                        fill="none"
                        stroke="#22d3ee"
                        stroke-width="2.5"
                        stroke-linecap="round"
                        stroke-linejoin="round"
                        points=pts
                    />
                }).collect_view()}
                // Isolated populated clock hours (1-hour exports or sparse BGM): whisker + median dot
                {isolated_bins.into_iter().map(|b| {
                    let cx = x_for(b.hour);
                    let y_p5 = y_for(b.p5);
                    let y_p25 = y_for(b.p25);
                    let y_p50 = y_for(b.p50);
                    let y_p75 = y_for(b.p75);
                    let y_p95 = y_for(b.p95);
                    view! {
                        <g>
                            <line x1=cx y1=y_p95 x2=cx y2=y_p5 stroke="rgba(34, 211, 238, 0.45)" stroke-width="2" />
                            <line x1=cx y1=y_p75 x2=cx y2=y_p25 stroke="rgba(34, 211, 238, 0.85)" stroke-width="5" stroke-linecap="round" />
                            <circle cx=cx cy=y_p50 r="3.5" fill="#22d3ee" stroke="#0f172a" stroke-width="1.2" />
                        </g>
                    }
                }).collect_view()}

                // Interactive hover / focus / tap hour inspection strips (clamped strictly to plot bounds)
                {populated.into_iter().map(|bin| {
                    let h = bin.hour;
                    let cx = x_for(h);
                    let cy = y_for(bin.p50);
                    let left = if h == 0 {
                        pad_l
                    } else {
                        (cx - half_hour_step).max(pad_l)
                    };
                    let right = if h == 23 {
                        plot_right
                    } else {
                        (cx + half_hour_step).min(plot_right)
                    };
                    let strip_x = left;
                    let strip_w = (right - left).max(1.0);
                    let tip = format!(
                        "{h:02}:00 — Median {:.0} mg/dL · IQR (p25–p75) {:.0}–{:.0} · Outer (p5–p95) {:.0}–{:.0} (n={})",
                        bin.p50, bin.p25, bin.p75, bin.p5, bin.p95, bin.count
                    );
                    let aria_tip = tip.clone();
                    view! {
                        <g
                            tabindex="0"
                            role="button"
                            aria-label=aria_tip
                            class="cursor-pointer focus:outline-none"
                            on:mouseenter=move |_| selected_hour.set(Some(h))
                            on:focus=move |_| selected_hour.set(Some(h))
                            on:click=move |_| selected_hour.set(Some(h))
                        >
                            <title>{tip}</title>
                            <rect
                                x=strip_x
                                y=pad_t
                                width=strip_w
                                height=plot_h
                                fill="transparent"
                            />
                            {move || (selected_hour.get() == Some(h)).then(|| view! {
                                <g>
                                    <line
                                        x1=cx
                                        y1=pad_t
                                        x2=cx
                                        y2=pad_t + plot_h
                                        stroke="rgba(34, 211, 238, 0.55)"
                                        stroke-width="1.2"
                                        stroke-dasharray="3 3"
                                    />
                                    <circle
                                        cx=cx
                                        cy=cy
                                        r="5"
                                        fill="#22d3ee"
                                        stroke="#0f172a"
                                        stroke-width="2"
                                    />
                                </g>
                            })}
                        </g>
                    }
                }).collect_view()}

                // Y-axis labels
                <text x="6" y=y180 + 4.0 fill="#fbbf24" font-size="10">"180"</text>
                <text x="6" y=y140 + 4.0 fill="#34d399" font-size="10">"140"</text>
                <text x="6" y=y70 + 4.0 fill="#fb7185" font-size="10">"70"</text>

                // X-axis clock hour labels
                <text x=pad_l y=height - 6.0 fill="#94a3b8" font-size="10">"00:00"</text>
                <text x=pad_l + plot_w * (6.0 / 23.0) y=height - 6.0 text-anchor="middle" fill="#94a3b8" font-size="10">"06:00"</text>
                <text x=pad_l + plot_w * (12.0 / 23.0) y=height - 6.0 text-anchor="middle" fill="#94a3b8" font-size="10">"12:00"</text>
                <text x=pad_l + plot_w * (18.0 / 23.0) y=height - 6.0 text-anchor="middle" fill="#94a3b8" font-size="10">"18:00"</text>
                <text x=width - pad_r y=height - 6.0 text-anchor="end" fill="#94a3b8" font-size="10">"23:00"</text>
            </svg>
            <div class="flex flex-wrap items-center justify-between gap-2 border-t border-white/5 pt-2 text-[11px]">
                <p role="status" aria-live="polite" class="font-mono text-cyan-200">
                    {move || {
                        selected_hour
                            .get()
                            .and_then(|h| {
                                stored_bins.with_value(|list| {
                                    list.iter().find(|b| b.hour == h).map(|b| {
                                        format!(
                                            "{:02}:00 · Median {:.0} mg/dL · IQR {:.0}–{:.0} · p5–p95 {:.0}–{:.0} · {} readings",
                                            b.hour, b.p50, b.p25, b.p75, b.p5, b.p95, b.count
                                        )
                                    })
                                })
                            })
                            .unwrap_or_else(|| {
                                "Hover, focus, or tap any hour to inspect median & percentile glucose values."
                                    .to_string()
                            })
                    }}
                </p>
                <div class="flex flex-wrap items-center gap-3 text-[10px] text-slate-400">
                    <span class="inline-flex items-center gap-1">
                        <span class="inline-block h-2 w-3 rounded-sm bg-cyan-400"></span>
                        "Median (p50)"
                    </span>
                    <span class="inline-flex items-center gap-1">
                        <span class="inline-block h-2 w-3 rounded-sm bg-cyan-400/35"></span>
                        "IQR (p25–p75)"
                    </span>
                    <span class="inline-flex items-center gap-1">
                        <span class="inline-block h-2 w-3 rounded-sm bg-cyan-400/15"></span>
                        "Outer (p5–p95)"
                    </span>
                </div>
            </div>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use leptos::prelude::RenderHtml;

    #[test]
    fn parses_all_major_commercial_monitors_and_converts_mmol() {
        // 1. Abbott Lingo
        let lingo = parse_commercial_glucose_csv(SAMPLE_LINGO_CSV, None).unwrap();
        assert_eq!(lingo.monitor, GlucoseMonitor::AbbottLingo);
        assert_eq!(lingo.source_unit, GlucoseUnit::MgDl);
        assert_eq!(lingo.readings.len(), 15);

        // 2. Dexcom Clarity (filters metadata row, parses EGV)
        let dexcom = parse_commercial_glucose_csv(SAMPLE_DEXCOM_CLARITY_CSV, None).unwrap();
        assert_eq!(dexcom.monitor, GlucoseMonitor::DexcomClarity);
        assert_eq!(dexcom.readings.len(), 18);

        // 3. Dexcom Clarity / Stelo sentinel bounds ("Low", "High", "Below 70", "Above 250")
        let stelo_csv =
            "Index,Timestamp (YYYY-MM-DDThh:mm:ss),Event Type,Device Info,Glucose Value (mg/dL)\n\
                         1,2026-11-21T07:00:00,EGV,Stelo,Below 70\n\
                         2,2026-11-21T13:00:00,EGV,Stelo,Above 250\n\
                         3,2026-11-21T18:00:00,EGV,Stelo,Low\n\
                         4,2026-11-21T20:00:00,EGV,Stelo,High\n";
        let stelo = parse_commercial_glucose_csv(stelo_csv, None).unwrap();
        assert_eq!(stelo.monitor, GlucoseMonitor::DexcomStelo);
        assert_eq!(stelo.bounded_sentinels, 4);
        assert!((stelo.readings[0].glucose_mg_dl - 65.0).abs() < 1e-6);
        assert!((stelo.readings[1].glucose_mg_dl - 255.0).abs() < 1e-6);
        assert!((stelo.readings[2].glucose_mg_dl - 40.0).abs() < 1e-6);
        assert!((stelo.readings[3].glucose_mg_dl - 400.0).abs() < 1e-6);

        // 4. FreeStyle LibreView (mmol/L conversion + preamble + Record Type 0/1)
        let libre = parse_commercial_glucose_csv(SAMPLE_LIBREVIEW_MMOL_CSV, None).unwrap();
        assert_eq!(libre.monitor, GlucoseMonitor::AbbottFreeStyleLibre);
        assert_eq!(libre.source_unit, GlucoseUnit::MmolL);
        assert_eq!(libre.readings.len(), 10);
        assert!((libre.readings[0].glucose_mg_dl - 5.1 * MMOL_TO_MG_DL).abs() < 0.05);

        // 5. Medtronic CareLink (divider + Date + Time + Sensor Glucose)
        let carelink = parse_commercial_glucose_csv(SAMPLE_MEDTRONIC_CARELINK_CSV, None).unwrap();
        assert_eq!(carelink.monitor, GlucoseMonitor::MedtronicCareLink);
        assert_eq!(carelink.readings.len(), 10);

        // 6. Tandem Source
        let tandem_csv =
            "EventDateTime,Readings (CGM)\n2026-11-21T08:00:00,112\n2026-11-21T08:05:00,116\n";
        let tandem = parse_commercial_glucose_csv(tandem_csv, None).unwrap();
        assert_eq!(tandem.monitor, GlucoseMonitor::TandemSource);
        assert_eq!(tandem.readings.len(), 2);

        // 7. Omnipod / Glooko / Tidepool
        let glooko_csv =
            "Timestamp,CGM Glucose Value (mg/dL)\n2026-11-21 09:00,121\n2026-11-21 09:05,126\n";
        let glooko = parse_commercial_glucose_csv(glooko_csv, None).unwrap();
        assert_eq!(glooko.monitor, GlucoseMonitor::GlookoTidepool);
        assert_eq!(glooko.readings.len(), 2);

        // 8. Senseonics Eversense (via preamble or hint)
        let eversense_csv = "# Senseonics Eversense 365 Export\nTimestamp,Glucose (mg/dL)\n2026-11-21 07:00,96\n2026-11-21 07:05,99\n";
        let eversense = parse_commercial_glucose_csv(eversense_csv, None).unwrap();
        assert_eq!(eversense.monitor, GlucoseMonitor::SenseonicsEversense);
        assert_eq!(eversense.readings.len(), 2);

        // 9. Fingerstick BGM
        let bgm = parse_commercial_glucose_csv(SAMPLE_BGM_FINGERSTICK_CSV, None).unwrap();
        assert_eq!(bgm.monitor, GlucoseMonitor::FingerstickBgm);
        assert_eq!(bgm.method, MeasurementMethod::CapillaryBgm);
        assert_eq!(bgm.readings.len(), 8);
    }

    #[test]
    fn computes_ada_5band_gmi_gri_mage_and_agp_and_renders_svg() {
        let dexcom = parse_commercial_glucose_csv(SAMPLE_DEXCOM_CLARITY_CSV, None).unwrap();
        let summary = summarize_commercial_glucose(
            &dexcom.readings,
            dexcom.monitor,
            dexcom.source_unit,
            dexcom.method,
        )
        .expect("summary present");

        let band_sum = summary.tbr_very_low_lt_54_pct
            + summary.tbr_low_54_69_pct
            + summary.tir_70_180_pct
            + summary.tar_high_181_250_pct
            + summary.tar_very_high_gt_250_pct;
        assert!((band_sum - 100.0).abs() < 1e-6);
        assert!(summary.gmi_percent > 5.0 && summary.gmi_percent < 9.0);
        assert!(summary.gri_score >= 0.0 && summary.gri_score <= 100.0);
        assert!(summary.mage_mg_dl > 0.0);
        assert_eq!(summary.agp_hourly.len(), 24);

        let bar_html = view! { <AdaTimeInRangeBar summary=summary.clone() /> }.to_html();
        assert!(bar_html.contains("70–180"));
        assert!(bar_html.contains("70–140 Tight"));

        let agp_html = view! { <AgpPercentileChart bins=summary.agp_hourly /> }.to_html();
        assert!(agp_html.contains("24-hour Ambulatory Glucose Profile"));
        assert!(agp_html.contains("<polygon"));
        assert!(agp_html.contains("<polyline"));
    }

    #[test]
    fn parses_real_abbott_lingo_iso_offset_csv_across_february_month_boundary_and_bounds() {
        // Real Abbott Lingo app exports use 5-minute ISO-8601 timestamps with negative US offsets
        // (`YYYY-MM-DDTHH:MM-05:00`), 55–200 mg/dL bounds, and occasional `"Out of range"` cells.
        let lingo_real_shape = "\u{feff}Timestamp,Glucose (mg/dL)\n\
            2026-02-28T23:50-05:00,92\n\
            2026-02-28T23:55-05:00,94\n\
            2026-03-01T00:00-05:00,96\n\
            2026-03-01T00:05-05:00,<55\n\
            2026-03-01T00:10-05:00,>200\n\
            2026-03-01T00:15-05:00,Out of range\n\
            2026-03-01T00:20-05:00,118 mg/dL\n";

        let import =
            parse_commercial_glucose_input(lingo_real_shape, Some(GlucoseMonitor::AbbottLingo))
                .expect("Lingo ISO-offset CSV parses");

        assert_eq!(import.monitor, GlucoseMonitor::AbbottLingo);
        assert_eq!(import.diagnostics.format, ImportPayloadFormat::Csv);
        assert_eq!(import.readings.len(), 6);
        assert_eq!(import.bounded_sentinels, 2);
        assert_eq!(import.diagnostics.out_of_range_rows, 1);
        assert_eq!(import.diagnostics.skipped_invalid_rows, 0);
        assert_eq!(import.diagnostics.timestamps_unparsed, 0);

        // Exact Gregorian calendar diff from Feb 28 23:50 to Mar 1 00:00 is 10 minutes (0.1667h),
        // NOT a phantom 3-day jump!
        assert!((import.readings[1].hour_offset - (5.0 / 60.0)).abs() < 1e-6);
        assert!((import.readings[2].hour_offset - (10.0 / 60.0)).abs() < 1e-6);
        assert!((import.readings[5].hour_offset - (30.0 / 60.0)).abs() < 1e-6);
        assert!((import.readings[3].glucose_mg_dl - 54.0).abs() < 1e-6);
        assert!((import.readings[4].glucose_mg_dl - 201.0).abs() < 1e-6);
        assert!((import.readings[5].glucose_mg_dl - 118.0).abs() < 1e-6);

        let median_min = import
            .diagnostics
            .median_interval_minutes
            .expect("median cadence computed");
        assert!((median_min - 5.0).abs() < 1e-6);
        assert!(import
            .diagnostics
            .checks
            .iter()
            .any(|c| c.name == "Sampling Cadence" && c.detail.contains("Abbott Lingo 5-minute")));
    }

    #[test]
    fn parses_apple_health_xml_with_lingo_source_and_mmol_conversion() {
        let import = parse_commercial_glucose_input(SAMPLE_APPLE_HEALTH_XML, None)
            .expect("Apple Health XML parses");
        assert_eq!(
            import.diagnostics.format,
            ImportPayloadFormat::AppleHealthXml
        );
        assert_eq!(import.monitor, GlucoseMonitor::AbbottLingo);
        assert_eq!(import.readings.len(), 5);
        assert_eq!(import.diagnostics.skipped_non_glucose_rows, 1);
        assert_eq!(import.diagnostics.timestamps_unparsed, 0);
        // 06:00 to 18:00 is exactly 12.0 hours
        assert!((import.readings.last().unwrap().hour_offset - 12.0).abs() < 1e-6);

        // Verify Apple Health's quirky `mmol<180.1558800000541>/L` unit string converts accurately.
        let mmol_xml = r#"<?xml version="1.0"?><HealthData>
            <Record type="HKQuantityTypeIdentifierBloodGlucose" sourceName="Lingo" unit="mmol&lt;180.15588&gt;/L" value="6.0" startDate="2026-11-24 08:00:00 +0000" />
        </HealthData>"#;
        let mmol_import = parse_commercial_glucose_input(mmol_xml, None).unwrap();
        assert_eq!(mmol_import.source_unit, GlucoseUnit::MmolL);
        assert!((mmol_import.readings[0].glucose_mg_dl - 6.0 * MMOL_TO_MG_DL).abs() < 0.05);
    }

    #[test]
    fn parses_health_connect_bridge_json_preserves_local_agp_hour_and_roundtrips() {
        let import = parse_commercial_glucose_input(SAMPLE_HEALTH_BRIDGE_JSON, None)
            .expect("Health bridge JSON parses");
        assert_eq!(
            import.diagnostics.format,
            ImportPayloadFormat::HealthBridgeJson
        );
        assert_eq!(import.monitor, GlucoseMonitor::AbbottLingo);
        assert_eq!(import.readings.len(), 8);
        assert_eq!(import.diagnostics.timestamps_unparsed, 0);

        // 13:00Z / 13:05Z (-07:00) -> 06:00 local; 15:00Z / 15:05Z (-07:00) -> 08:00 local;
        // 20:30Z (-07:00) -> 13:30 local. Neither 15:00 nor 19:00 UTC should leak into UTC hours!
        assert!(import.readings[0].timestamp.contains("T06:00:00-07:00"));
        let summary = summarize_commercial_glucose(
            &import.readings,
            import.monitor,
            import.source_unit,
            import.method,
        )
        .unwrap();
        assert_eq!(summary.agp_hourly[6].count, 2);
        assert_eq!(summary.agp_hourly[8].count, 2);
        assert_eq!(summary.agp_hourly[13].count, 1);
        assert_eq!(summary.agp_hourly[15].count, 0);
        assert_eq!(summary.agp_hourly[19].count, 0);

        // Verify raw Android Health Connect nested `level.inMilligramsPerDeciliter` + deduplication.
        let raw_hc_json = r#"[
            {
                "time": "2026-11-24T15:00:00Z",
                "zoneOffset": "-07:00",
                "level": { "inMilligramsPerDeciliter": 110.0 },
                "metadata": { "dataOrigin": { "packageName": "com.abbott.lingo" } }
            },
            {
                "time": "2026-11-24T15:00:00Z",
                "zoneOffset": "-07:00",
                "level": { "inMilligramsPerDeciliter": 110.0 },
                "metadata": { "dataOrigin": { "packageName": "com.abbott.lingo" } }
            },
            {
                "time": "2026-11-24T15:05:00Z",
                "zoneOffset": "-07:00",
                "level": { "inMillimolesPerLiter": 6.5 },
                "metadata": { "dataOrigin": { "packageName": "com.abbott.lingo" } }
            }
        ]"#;
        let raw_hc = parse_commercial_glucose_input(raw_hc_json, None).unwrap();
        assert_eq!(raw_hc.monitor, GlucoseMonitor::AbbottLingo);
        assert_eq!(raw_hc.readings.len(), 2);
        assert_eq!(raw_hc.diagnostics.duplicate_timestamps_dropped, 1);

        // Round-trip export -> import parity
        let exported = export_health_bridge_json(&import, "offline_validation");
        let reparsed = parse_commercial_glucose_input(&exported, None).unwrap();
        assert_eq!(reparsed.readings.len(), import.readings.len());
        assert_eq!(reparsed.monitor, GlucoseMonitor::AbbottLingo);

        // Verified Android Abbott Lingo package ID (`com.abbott.lingo.wellness`) + 55-200 sensor caps
        let wellness_hc = r#"{
            "schema": "dabney.health_bridge.v1",
            "platform": "android_health_connect",
            "exported_at": "2026-11-25T18:00:00Z",
            "records": [
                { "time": "2026-11-25T15:00:00Z", "zoneOffset": "-07:00", "levelMgDl": 54.0, "dataOrigin": "com.abbott.lingo.wellness" },
                { "time": "2026-11-25T15:05:00Z", "zoneOffset": "-07:00", "levelMgDl": 112.0, "dataOrigin": "com.abbott.lingo.wellness" },
                { "time": "2026-11-25T15:10:00Z", "zoneOffset": "-07:00", "levelMgDl": 201.0, "dataOrigin": "com.abbott.lingo.wellness" }
            ]
        }"#;
        let wellness_parsed = parse_commercial_glucose_input(wellness_hc, None).unwrap();
        assert_eq!(wellness_parsed.monitor, GlucoseMonitor::AbbottLingo);
        assert_eq!(wellness_parsed.diagnostics.bounded_sentinels, 2);
        assert!(wellness_parsed
            .diagnostics
            .checks
            .iter()
            .any(|c| c.name == "Biosensor Range & Saturation" && c.level == ValidationLevel::Warn));
    }

    #[test]
    fn windowing_downsampling_and_filename_hints_scale_to_14_day_lingo_traces() {
        assert_eq!(
            monitor_hint_from_filename("Lingo_glucose_export_2026.csv"),
            Some(GlucoseMonitor::AbbottLingo)
        );
        assert_eq!(
            monitor_hint_from_filename("Clarity_Export.csv"),
            Some(GlucoseMonitor::DexcomClarity)
        );

        // Synthesize a full 14-day 5-minute Lingo trace (4,032 readings) with one known peak & trough.
        let readings: Vec<LingoReading> = (0..4032)
            .map(|i| {
                let g = if i == 2500 {
                    196.0
                } else if i == 1200 {
                    58.0
                } else {
                    95.0 + ((i % 24) as f64)
                };
                LingoReading {
                    timestamp: format!(
                        "2026-11-{:02}T{:02}:{:02}:00-07:00",
                        1 + (i / 288),
                        (i % 288) / 12,
                        (i % 12) * 5
                    ),
                    hour_offset: i as f64 * (5.0 / 60.0),
                    glucose_mg_dl: g,
                    lingo_count: None,
                    tag: None,
                    bound: GlucoseReadingBound::Measured,
                }
            })
            .collect();

        let last_24h = slice_readings_window(&readings, GlucoseWindow::Hours24);
        assert!((last_24h.last().unwrap().hour_offset - 24.0).abs() < 0.2);
        assert!(last_24h.len() <= 290);

        let downsampled = downsample_readings_for_chart(&readings, 320);
        assert!(downsampled.len() <= 320);
        // Min/max bucket preservation guarantees global spike and trough are retained.
        assert!(downsampled
            .iter()
            .any(|r| (r.glucose_mg_dl - 196.0).abs() < 1e-6));
        assert!(downsampled
            .iter()
            .any(|r| (r.glucose_mg_dl - 58.0).abs() < 1e-6));

        let peaks = select_notable_spike_peaks(&readings, 24);
        assert_eq!(peaks.len(), 1);
        assert!((peaks[0].glucose_mg_dl - 196.0).abs() < 1e-6);
    }

    #[test]
    fn infers_unitless_mmol_and_semicolon_decimal_comma_lingo_csv() {
        let uk_csv = "Timestamp,Glucose\n\
            2026-11-24T08:00+00:00,5.4\n\
            2026-11-24T08:05+00:00,6.1\n\
            2026-11-24T08:10+00:00,7.2\n";
        let uk = parse_commercial_glucose_input(uk_csv, Some(GlucoseMonitor::AbbottLingo))
            .expect("unitless mmol/L Lingo CSV parses");
        assert_eq!(uk.source_unit, GlucoseUnit::MmolL);
        assert!((uk.readings[0].glucose_mg_dl - 5.4 * MMOL_TO_MG_DL).abs() < 0.05);

        let eu_semi = "Timestamp;Glucose\n\
            2026-11-24T08:00+01:00;5,5\n\
            2026-11-24T08:05+01:00;6,2\n";
        let eu = parse_commercial_glucose_input(eu_semi, Some(GlucoseMonitor::AbbottLingo))
            .expect("semicolon decimal-comma CSV parses");
        assert_eq!(eu.source_unit, GlucoseUnit::MmolL);
        assert_eq!(eu.readings.len(), 2);
        assert!((eu.readings[1].glucose_mg_dl - 6.2 * MMOL_TO_MG_DL).abs() < 0.05);
    }

    #[test]
    fn parses_official_abbott_lingo_header_reverse_order_numeric_caps_and_dst() {
        // Synthetic trace mirroring Abbott Lingo's official mobile app CSV export schema:
        // reverse-chronological order, DST fall-back (`-06:00` -> `-07:00`), numeric floor `54`
        // (`<55`) and ceiling `201` (`>200`) sentinels, and `Out of range` rows.
        let official_mg = "\
Time of Glucose Reading [T=(local time) +/- (time zone offset)], Measurement(mg/dL)
2025-11-02T01:10-07:00,201
2025-11-02T01:05-07:00,Out of range
2025-11-02T01:00-07:00,142
2025-11-02T01:55-06:00,118
2025-11-02T01:50-06:00,54
";
        let imp = parse_commercial_glucose_input(official_mg, None)
            .expect("official Lingo mg/dL header auto-detects without hint");
        assert_eq!(imp.monitor, GlucoseMonitor::AbbottLingo);
        assert_eq!(imp.source_unit, GlucoseUnit::MgDl);
        assert_eq!(imp.readings.len(), 4);
        assert_eq!(imp.bounded_sentinels, 2);
        assert_eq!(imp.diagnostics.out_of_range_rows, 1);
        assert_eq!(imp.diagnostics.skipped_invalid_rows, 0);
        assert_eq!(imp.diagnostics.timestamps_unparsed, 0);
        // Chronologically sorted across DST fall-back:
        // 01:50-06:00 (07:50Z) -> 01:55-06:00 (07:55Z) -> 01:00-07:00 (08:00Z) -> 01:10-07:00 (08:10Z)
        assert_eq!(imp.readings[0].timestamp, "2025-11-02T01:50-06:00");
        assert_eq!(imp.readings[3].timestamp, "2025-11-02T01:10-07:00");
        assert!((imp.readings[1].hour_offset - (5.0 / 60.0)).abs() < 1e-6);
        assert!((imp.readings[2].hour_offset - (10.0 / 60.0)).abs() < 1e-6);
        assert!((imp.readings[3].hour_offset - (20.0 / 60.0)).abs() < 1e-6);
        assert!(imp.diagnostics.checks.iter().any(|c| {
            c.name == "Biosensor Range & Saturation"
                && c.level == ValidationLevel::Warn
                && c.detail.contains("ceiling-capped")
        }));

        // Official mmol/L header variant auto-detects Abbott Lingo + mmol/L cleanly.
        let official_mmol = "\
Time of Glucose Reading [T=(local time) +/- (time zone offset)], Measurement(mmol/L)
2025-11-02T08:05+00:00,6.2
2025-11-02T08:00+00:00,5.5
";
        let imp_mmol = parse_commercial_glucose_input(official_mmol, None).unwrap();
        assert_eq!(imp_mmol.monitor, GlucoseMonitor::AbbottLingo);
        assert_eq!(imp_mmol.source_unit, GlucoseUnit::MmolL);
        assert!((imp_mmol.readings[0].glucose_mg_dl - 5.5 * MMOL_TO_MG_DL).abs() < 0.05);
    }

    #[test]
    fn preserves_reading_bounds_across_bridge_json_and_compact_export() {
        let csv = "\
Time of Glucose Reading [T=(local time) +/- (time zone offset)], Measurement(mg/dL)
2026-08-01T08:00-07:00,54
2026-08-01T08:05-07:00,110
2026-08-01T08:10-07:00,201
";
        let imp = parse_commercial_glucose_input(csv, None).unwrap();
        assert_eq!(imp.readings[0].bound, GlucoseReadingBound::BelowSensorFloor);
        assert_eq!(imp.readings[1].bound, GlucoseReadingBound::Measured);
        assert_eq!(
            imp.readings[2].bound,
            GlucoseReadingBound::AboveSensorCeiling
        );

        let pretty = export_health_bridge_json(&imp, "offline_validation");
        let compact = export_health_bridge_json_compact(&imp, "mobile_vault");
        assert!(compact.len() < pretty.len());
        assert!(pretty.contains("\"below_floor\""));
        assert!(pretty.contains("\"above_ceiling\""));
        assert!(compact.contains("\"below_floor\""));
        assert!(compact.contains("\"above_ceiling\""));

        // Re-import compact JSON as GenericCsv (no Lingo numeric fallback) to prove
        // `bound` itself round-trips cleanly.
        let reparsed =
            parse_commercial_glucose_input(&compact, Some(GlucoseMonitor::GenericCsv)).unwrap();
        assert_eq!(reparsed.readings.len(), 3);
        assert_eq!(
            reparsed.readings[0].bound,
            GlucoseReadingBound::BelowSensorFloor
        );
        assert_eq!(reparsed.readings[1].bound, GlucoseReadingBound::Measured);
        assert_eq!(
            reparsed.readings[2].bound,
            GlucoseReadingBound::AboveSensorCeiling
        );
    }

    #[test]
    fn every_window_slices_the_six_month_sample_and_its_checkins_onto_one_timeline() {
        let readings = parse_commercial_glucose_input(SAMPLE_LINGO_6MO_CSV, None)
            .unwrap()
            .readings;
        let checkins = sample_lingo_6mo_checkins();
        let trace_h = readings.last().unwrap().hour_offset - readings[0].hour_offset;
        assert!(trace_h > 180.0 * 24.0, "sample spans about six months");
        assert_eq!(
            GlucoseWindow::default_for_trace(&readings),
            GlucoseWindow::Days14
        );

        let mut previous_len = 0;
        for window in GlucoseWindow::ALL {
            let sliced = slice_readings_window(&readings, window);
            let sliced_checkins = slice_checkins_window(&checkins, &readings, window);
            let label = window.label();
            assert!(
                sliced.len() > previous_len,
                "{label}: wider window adds readings"
            );
            previous_len = sliced.len();

            let end_h = sliced.last().unwrap().hour_offset;
            let window_h = window.max_hours().unwrap_or(trace_h);
            assert!(sliced[0].hour_offset >= 0.0, "{label}: rebased to zero");
            assert!(end_h <= window_h + 1e-6, "{label}: ends within window");
            assert!(
                (end_h - window_h).abs() < 1e-6,
                "{label}: last reading sits at the window's end"
            );
            assert!(
                sliced
                    .windows(2)
                    .all(|p| p[0].hour_offset <= p[1].hour_offset),
                "{label}: stays in time order"
            );
            assert!(
                sliced_checkins
                    .iter()
                    .all(|c| (0.0..=end_h + 24.0).contains(&c.hour_offset)),
                "{label}: check-ins share the readings' timeline"
            );
            if window.max_hours().is_some_and(|h| h >= 24.0 * 7.0) {
                assert!(!sliced_checkins.is_empty(), "{label}: has check-ins");
            }
        }
        assert_eq!(previous_len, readings.len());
        assert_eq!(
            slice_checkins_window(&checkins, &readings, GlucoseWindow::All).len(),
            checkins.len()
        );
    }

    #[test]
    fn supports_30d_and_90d_windows_and_windowed_saturation_metrics() {
        assert_eq!(
            GlucoseWindow::ALL,
            [
                GlucoseWindow::Hours24,
                GlucoseWindow::Days3,
                GlucoseWindow::Days7,
                GlucoseWindow::Days14,
                GlucoseWindow::Days30,
                GlucoseWindow::Days90,
                GlucoseWindow::All,
            ]
        );
        assert_eq!(GlucoseWindow::Days30.label(), "30d");
        assert_eq!(GlucoseWindow::Days30.max_hours(), Some(720.0));
        assert_eq!(GlucoseWindow::Days90.label(), "90d");
        assert_eq!(GlucoseWindow::Days90.max_hours(), Some(2160.0));

        // Build a synthetic 100-day hourly trace:
        // days 0..70 have floor caps (54), days 70..100 have ceiling caps (201) on some hours.
        let total_hours = 100 * 24;
        let readings: Vec<LingoReading> = (0..total_hours)
            .map(|h| {
                let day = h / 24;
                let (g, bound) = if day < 70 && h % 24 == 3 {
                    // Legacy reading without explicit bound to verify Lingo fallback too
                    (54.0, GlucoseReadingBound::Measured)
                } else if day >= 70 && h % 24 == 14 {
                    (201.0, GlucoseReadingBound::AboveSensorCeiling)
                } else {
                    (108.0, GlucoseReadingBound::Measured)
                };
                LingoReading {
                    timestamp: format!("2026-01-01T{:02}:00:00-07:00", h % 24),
                    hour_offset: h as f64,
                    glucose_mg_dl: g,
                    lingo_count: None,
                    tag: None,
                    bound,
                }
            })
            .collect();

        let w30 = slice_readings_window(&readings, GlucoseWindow::Days30);
        let w90 = slice_readings_window(&readings, GlucoseWindow::Days90);
        assert!(w30.len() <= 721 && w30.len() >= 720);
        assert!(w90.len() <= 2161 && w90.len() >= 2160);

        let sum30 = summarize_commercial_glucose(
            &w30,
            GlucoseMonitor::AbbottLingo,
            GlucoseUnit::MgDl,
            MeasurementMethod::InterstitialCgm,
        )
        .unwrap();
        assert!(sum30.has_sensor_saturation());
        assert_eq!(sum30.floor_capped_readings, 0);
        assert_eq!(sum30.ceiling_capped_readings, 30);
        assert!((sum30.ceiling_capped_pct - (30.0 / w30.len() as f64) * 100.0).abs() < 1e-6);

        let sum_all = summarize_commercial_glucose(
            &readings,
            GlucoseMonitor::AbbottLingo,
            GlucoseUnit::MgDl,
            MeasurementMethod::InterstitialCgm,
        )
        .unwrap();
        assert_eq!(sum_all.floor_capped_readings, 70);
        assert_eq!(sum_all.ceiling_capped_readings, 30);
        assert!((sum_all.floor_capped_pct - (70.0 / readings.len() as f64) * 100.0).abs() < 1e-6);
    }

    #[test]
    fn select_notable_spike_peaks_deduplicates_plateaus_and_breaks_across_gaps() {
        let mk = |hour_offset: f64, g: f64| LingoReading {
            timestamp: format!("2026-08-01T00:00:00-07:00@{hour_offset:.2}"),
            hour_offset,
            glucose_mg_dl: g,
            lingo_count: None,
            tag: None,
            bound: if g >= 200.5 {
                GlucoseReadingBound::AboveSensorCeiling
            } else {
                GlucoseReadingBound::Measured
            },
        };

        // Excursion 1: ramp up to a 5-point flat 201 ceiling plateau, then back below 140.
        // Excursion 2: spike to 175, then a 2.5h sensor gap while still > 140.
        // Excursion 3: post-gap excursion to 188.
        let readings = vec![
            mk(0.0, 110.0),
            mk(0.083, 155.0),
            mk(0.167, 201.0),
            mk(0.250, 201.0),
            mk(0.333, 201.0), // midpoint of the 5-point plateau
            mk(0.417, 201.0),
            mk(0.500, 201.0),
            mk(0.583, 165.0),
            mk(0.667, 130.0),
            mk(1.000, 160.0),
            mk(1.083, 175.0), // peak of excursion 2
            mk(1.167, 150.0),
            // 2.5h sensor gap (> 1.5h)
            mk(3.667, 162.0),
            mk(3.750, 188.0), // peak of excursion 3
            mk(3.833, 120.0),
        ];

        let peaks = select_notable_spike_peaks(&readings, 24);
        assert_eq!(peaks.len(), 3);
        assert!((peaks[0].hour_offset - 0.333).abs() < 1e-6);
        assert!((peaks[0].glucose_mg_dl - 201.0).abs() < 1e-6);
        assert!((peaks[1].hour_offset - 1.083).abs() < 1e-6);
        assert!((peaks[1].glucose_mg_dl - 175.0).abs() < 1e-6);
        assert!((peaks[2].hour_offset - 3.750).abs() < 1e-6);
        assert!((peaks[2].glucose_mg_dl - 188.0).abs() < 1e-6);

        // Verify temporal spread when > max_markers excursions all tie at 201 mg/dL:
        // 60 daily excursions across 60 days capped to 12 markers must span the full timeline,
        // NOT cluster only in the first 12 days.
        let mut tied_trace = Vec::new();
        for day in 0..60 {
            let base_h = day as f64 * 24.0;
            tied_trace.push(mk(base_h, 100.0));
            tied_trace.push(mk(base_h + 1.0, 201.0));
            tied_trace.push(mk(base_h + 1.1, 201.0));
            tied_trace.push(mk(base_h + 2.0, 105.0));
        }
        let spread = select_notable_spike_peaks(&tied_trace, 12);
        assert_eq!(spread.len(), 12);
        assert!(spread.first().unwrap().hour_offset < 120.0);
        assert!(spread.last().unwrap().hour_offset > 1200.0);
    }

    #[test]
    fn gap_aware_mage_ignores_jitter_and_does_not_bridge_sensor_gaps() {
        let mk = |hour_offset: f64, g: f64| LingoReading {
            timestamp: format!("2026-08-01T00:00:00-07:00@{hour_offset:.2}"),
            hour_offset,
            glucose_mg_dl: g,
            lingo_count: None,
            tag: None,
            bound: GlucoseReadingBound::Measured,
        };
        // Two flat segments separated by a 12-hour sensor gap (80 before gap, 190 after gap).
        // Bridging the gap would report MAGE = 110; gap-aware MAGE must be 0.0 because
        // neither continuous segment has a > 1 SD swing!
        let mut gapped = Vec::new();
        for i in 0..20 {
            gapped.push(mk(i as f64 * (5.0 / 60.0), 80.0));
        }
        for i in 0..20 {
            gapped.push(mk(14.0 + i as f64 * (5.0 / 60.0), 190.0));
        }
        let sum = summarize_commercial_glucose(
            &gapped,
            GlucoseMonitor::AbbottLingo,
            GlucoseUnit::MgDl,
            MeasurementMethod::InterstitialCgm,
        )
        .unwrap();
        assert!((sum.mage_mg_dl - 0.0).abs() < 1e-6);
    }

    #[test]
    fn nominal_window_prevents_overstating_active_wear_across_gap() {
        // 40-day 5-min synthetic trace with a 10-day sensor gap from day 15..25.
        // Trailing 30d window (days 10..40) has readings on days 10..15 + 25..40 = 20d active
        // out of 30d nominal -> true wear ~66.7%. If instead days 10..20 were missing (cutoff
        // lands inside the gap at day 10, first reading inside 30d is at day 20), actual_span
        // is only 20d -> without nominal window wear would be falsely 100%, whereas with
        // nominal_window_hours_for_trace it accurately reports ~66.7%!
        let mut readings = Vec::new();
        for day in 0..40_usize {
            if (8..20).contains(&day) {
                continue; // 12-day gap spanning the 30d cutoff (day 10)
            }
            for step in 0..288_usize {
                let hour_offset = day as f64 * 24.0 + step as f64 * (5.0 / 60.0);
                readings.push(LingoReading {
                    timestamp: format!(
                        "2026-08-01T{:02}:{:02}:00-07:00",
                        (step / 12) % 24,
                        (step % 12) * 5
                    ),
                    hour_offset,
                    glucose_mg_dl: 105.0,
                    lingo_count: None,
                    tag: None,
                    bound: GlucoseReadingBound::Measured,
                });
            }
        }
        let w30 = slice_readings_window(&readings, GlucoseWindow::Days30);
        let nom_h = nominal_window_hours_for_trace(&readings, GlucoseWindow::Days30);
        assert_eq!(nom_h, Some(720.0));

        let naive = summarize_commercial_glucose(
            &w30,
            GlucoseMonitor::AbbottLingo,
            GlucoseUnit::MgDl,
            MeasurementMethod::InterstitialCgm,
        )
        .unwrap();
        let accurate = summarize_commercial_glucose_with_window(
            &w30,
            GlucoseMonitor::AbbottLingo,
            GlucoseUnit::MgDl,
            MeasurementMethod::InterstitialCgm,
            nom_h,
        )
        .unwrap();
        assert!(naive.estimated_active_wear_pct > 99.0);
        assert!(
            accurate.estimated_active_wear_pct > 64.0 && accurate.estimated_active_wear_pct < 69.0
        );
    }

    fn hourly_trace(hours: usize) -> Vec<LingoReading> {
        (0..=hours)
            .map(|h| LingoReading {
                timestamp: format!("2026-11-{:02}T{:02}:00:00-07:00", 1 + h / 24, h % 24),
                hour_offset: h as f64,
                glucose_mg_dl: 100.0,
                lingo_count: None,
                tag: None,
                bound: GlucoseReadingBound::Measured,
            })
            .collect()
    }

    fn checkin_at(hour_offset: f64) -> MentalHealthCheckIn {
        MentalHealthCheckIn {
            recorded_at: String::new(),
            hour_offset,
            mood: 5,
            anxiety: 5,
            energy: 5,
            sleep_hours: 7.0,
            phq9: None,
            gad7: None,
            note: String::new(),
        }
    }

    #[test]
    fn checkins_share_the_readings_window_and_its_start() {
        let readings = hourly_trace(14 * 24);
        let checkins = [1.0, 300.0, 330.0].map(checkin_at);

        let sliced = slice_readings_window(&readings, GlucoseWindow::Hours24);
        let kept = slice_checkins_window(&checkins, &readings, GlucoseWindow::Hours24);
        assert_eq!(sliced.first().unwrap().hour_offset, 0.0);
        assert_eq!(sliced.last().unwrap().hour_offset, 24.0);
        // 336h trace: the 24h window starts at 312h, so only the 330h check-in falls inside.
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].hour_offset, 18.0);
        assert_eq!(timeline_hour_span(&sliced, &kept), 24.0);

        let all = slice_checkins_window(&checkins, &readings, GlucoseWindow::All);
        assert_eq!(all, checkins.to_vec());
        assert_eq!(
            slice_checkins_window(&checkins, &[], GlucoseWindow::Hours24).len(),
            3
        );
    }

    #[test]
    fn a_short_trace_fills_the_timeline_rather_than_a_fixed_day() {
        let readings = hourly_trace(6);
        assert_eq!(timeline_hour_span(&readings, &[]), 6.0);
        assert_eq!(timeline_hour_span(&readings[..1], &[]), 1.0);
    }

    #[test]
    fn the_glucose_axis_keeps_lows_off_the_floor_and_references_inside() {
        // Typical day: the standard 40-200 envelope.
        assert_eq!(
            glucose_axis_domain([95.0, 160.0, 70.0, 180.0]),
            (40.0, 200.0)
        );
        // A Dexcom "LOW" 40 and a 25 mg/dL reading sit above the floor, not pinned to it.
        assert_eq!(glucose_axis_domain([40.0, 120.0]), (30.0, 200.0));
        assert_eq!(glucose_axis_domain([25.0, 120.0]), (10.0, 200.0));
        assert_eq!(glucose_axis_domain([20.0, 120.0]), (10.0, 200.0));
        assert_eq!(glucose_axis_domain([5.0]), (0.0, 200.0));
        // Highs round out with headroom; the reference set keeps a 250 target visible.
        assert_eq!(glucose_axis_domain([100.0, 312.0]), (40.0, 330.0));
        assert_eq!(glucose_axis_domain([100.0, 250.0]), (40.0, 260.0));
        assert_eq!(glucose_axis_domain([f64::NAN]), (40.0, 200.0));
    }

    #[test]
    fn glucose_ticks_take_round_steps_inside_the_axis() {
        assert_eq!(
            glucose_axis_ticks((40.0, 200.0)),
            vec![50.0, 100.0, 150.0, 200.0]
        );
        assert_eq!(
            glucose_axis_ticks((10.0, 500.0)),
            vec![100.0, 200.0, 300.0, 400.0, 500.0]
        );
    }

    #[test]
    fn agp_axis_spreads_past_a_fixed_ceiling_and_labels_round_steps() {
        let bin = |hour: u8, p95: f64| AgpHourBin {
            hour,
            count: 12,
            p5: 60.0,
            p25: 90.0,
            p50: 110.0,
            p75: 140.0,
            p95,
        };
        let html =
            view! { <AgpPercentileChart bins=vec![bin(0, 180.0), bin(12, 310.0)] /> }.to_html();
        assert!(html.contains(">300</text>"), "{html}");
        assert!(html.contains(">100</text>"), "{html}");
        // Non-contiguous hours (0 and 12) render isolated whiskers/dots instead of bridging across
        // empty overnight/morning hours.
        assert!(!html.contains("<polygon"));
        assert_eq!(html.matches("<circle").count(), 2);
    }

    #[test]
    fn select_notable_spike_peaks_above_respects_tight_and_high_custom_thresholds() {
        let mut readings = Vec::new();
        // 40 distinct excursions peaking at 165 mg/dL (above 140, below 180)
        for i in 0..40 {
            let base = i as f64 * 4.0;
            readings.push(LingoReading {
                timestamp: format!("2026-07-01T00:00-07:00#{i}a"),
                hour_offset: base,
                glucose_mg_dl: 100.0,
                lingo_count: None,
                tag: None,
                bound: GlucoseReadingBound::Measured,
            });
            readings.push(LingoReading {
                timestamp: format!("2026-07-01T00:30-07:00#{i}b"),
                hour_offset: base + 0.5,
                glucose_mg_dl: 165.0,
                lingo_count: None,
                tag: None,
                bound: GlucoseReadingBound::Measured,
            });
        }
        // Plus one moderate excursion at 130 mg/dL and one true >180 excursion at 195 mg/dL.
        readings.push(LingoReading {
            timestamp: "2026-07-10T10:00-07:00".to_string(),
            hour_offset: 170.0,
            glucose_mg_dl: 130.0,
            lingo_count: None,
            tag: None,
            bound: GlucoseReadingBound::Measured,
        });
        readings.push(LingoReading {
            timestamp: "2026-07-10T12:00-07:00".to_string(),
            hour_offset: 172.0,
            glucose_mg_dl: 195.0,
            lingo_count: None,
            tag: None,
            bound: GlucoseReadingBound::Measured,
        });

        let ada_peaks = select_notable_spike_peaks_above(&readings, 180.0, 36);
        assert_eq!(ada_peaks.len(), 1);
        assert!((ada_peaks[0].glucose_mg_dl - 195.0).abs() < 1e-6);

        let tight_trace = vec![
            LingoReading {
                timestamp: "2026-07-01T08:00-07:00".to_string(),
                hour_offset: 0.0,
                glucose_mg_dl: 98.0,
                lingo_count: None,
                tag: None,
                bound: GlucoseReadingBound::Measured,
            },
            LingoReading {
                timestamp: "2026-07-01T08:30-07:00".to_string(),
                hour_offset: 0.5,
                glucose_mg_dl: 128.0,
                lingo_count: None,
                tag: None,
                bound: GlucoseReadingBound::Measured,
            },
        ];
        let tight_peaks = select_notable_spike_peaks_above(&tight_trace, 120.0, 36);
        assert_eq!(tight_peaks.len(), 1);
        assert!((tight_peaks[0].glucose_mg_dl - 128.0).abs() < 1e-6);
    }

    #[test]
    fn agp_renders_single_hour_whisker_clamps_edge_strips_and_marks_ceiling_cap() {
        let one_bin = AgpHourBin {
            hour: 10,
            count: 8,
            p5: 90.0,
            p25: 110.0,
            p50: 145.0,
            p75: 185.0,
            p95: 201.0,
        };
        let single_html =
            view! { <AgpPercentileChart bins=vec![one_bin.clone()] sensor_ceiling_mg_dl=200.0 /> }
                .to_html();
        assert!(
            single_html.contains("<circle"),
            "single populated hour must render a median dot instead of blank SVG"
        );
        assert!(
            single_html.contains("rgba(249, 115, 22, 0.50)"),
            "ceiling-saturated AGP should render a sensor-cap reference line when explicit ceiling is supplied"
        );
        let uncapped_html = view! { <AgpPercentileChart bins=vec![one_bin] /> }.to_html();
        assert!(
            !uncapped_html.contains("rgba(249, 115, 22, 0.50)"),
            "uncapped traces above 200 mg/dL must not draw a false Lingo ceiling-cap line"
        );

        let edge_bins = vec![
            AgpHourBin {
                hour: 0,
                count: 5,
                p5: 80.0,
                p25: 90.0,
                p50: 100.0,
                p75: 110.0,
                p95: 120.0,
            },
            AgpHourBin {
                hour: 23,
                count: 5,
                p5: 80.0,
                p25: 90.0,
                p50: 100.0,
                p75: 110.0,
                p95: 120.0,
            },
        ];
        let edge_html = view! { <AgpPercentileChart bins=edge_bins /> }.to_html();
        // Hour 23 strip must end strictly at plot_right = 740 (never x=725.5 width=29 overflowing to 754.5).
        assert!(
            !edge_html.contains("x=\"725.5\""),
            "hour 23 hit strip must not overflow past right plot edge"
        );
    }
}
