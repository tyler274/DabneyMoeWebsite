//! Pure-Rust commercial continuous glucose monitor (CGM) & blood glucose meter (BGM)
//! parser, auto-detector, ADA/ATTD clinical analytics engine (5-band TIR, GMI, GRI, MAGE,
//! 24-hour Ambulatory Glucose Profile), and SVG visualizations.
//!
//! Runs 100% locally in the browser (WASM) with zero external uploads.

use leptos::prelude::*;

use crate::mental_health_lingo::{split_csv_row, LingoReading, SAMPLE_LINGO_CSV};

const MMOL_TO_MG_DL: f64 = 18.0182;
const MIN_MG_DL: f64 = 20.0;
const MAX_MG_DL: f64 = 500.0;

/// Supported commercial CGM biosensors, pump ecosystems, aggregators, and fingerstick BGMs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

/// Indicates whether a sensor reading was measured directly or hit a sensor ceiling/floor sentinel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlucoseReadingBound {
    Measured,
    BelowSensorFloor,
    AboveSensorCeiling,
}

/// Glycemia Risk Index clinical zone (Klonoff et al. 2022, `0..=100`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
#[derive(Clone, Debug, PartialEq)]
pub struct AgpHourBin {
    pub hour: u8,
    pub count: usize,
    pub p5: f64,
    pub p25: f64,
    pub p50: f64,
    pub p75: f64,
    pub p95: f64,
}

/// Parsed commercial glucose export ready for visualization and clinical analytics.
#[derive(Clone, Debug, PartialEq)]
pub struct CommercialGlucoseImport {
    pub monitor: GlucoseMonitor,
    pub source_unit: GlucoseUnit,
    pub method: MeasurementMethod,
    pub readings: Vec<LingoReading>,
    pub bounded_sentinels: usize,
}

/// Comprehensive ADA/ATTD clinical glucose analytics over a parsed commercial monitor trace.
#[derive(Clone, Debug, PartialEq)]
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
    pub total_lingo_count: f64,
    pub estimated_active_wear_pct: f64,
    pub agp_hourly: Vec<AgpHourBin>,
}

struct HeaderSpec {
    monitor: GlucoseMonitor,
    unit: GlucoseUnit,
    method: MeasurementMethod,
    ts_col: usize,
    time_col: Option<usize>,
    glucose_cols: Vec<usize>,
    event_type_col: Option<usize>,
    record_type_col: Option<usize>,
    lingo_count_col: Option<usize>,
    note_col: Option<usize>,
}

struct ParsedGlucoseRow {
    timestamp: String,
    glucose_mg_dl: f64,
    lingo_count: Option<f64>,
    tag: Option<String>,
    bound: GlucoseReadingBound,
}

struct IndexedGlucoseRow {
    minutes: Option<f64>,
    order: usize,
    row: ParsedGlucoseRow,
}

/// Auto-detect and parse CSV exports from all 10 major commercial glucose monitors
/// in either `mg/dL` or `mmol/L`.
pub fn parse_commercial_glucose_csv(
    input: &str,
    monitor_hint: Option<GlucoseMonitor>,
) -> Result<CommercialGlucoseImport, String> {
    let cleaned_input = input.strip_prefix('\u{feff}').unwrap_or(input);
    let mut preamble_monitor = None;
    let mut header_spec: Option<HeaderSpec> = None;
    let mut raw_rows: Vec<ParsedGlucoseRow> = Vec::new();
    let mut saw_bgm_only = true;
    let mut saw_stelo_signal = false;

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

        let cols = match split_csv_row(line) {
            Ok(c) => c,
            Err(err) => {
                if header_spec.is_none() {
                    continue;
                }
                return Err(format!("Line {}: {err}", line_idx + 1));
            }
        };

        if header_spec.is_none() {
            if let Some(spec) = try_parse_header(&cols, monitor_hint.or(preamble_monitor)) {
                header_spec = Some(spec);
                continue;
            }
            // Allow headerless `timestamp,glucose_mg_dl[,lingo_count[,tag]]` rows
            if cols.len() >= 2 && parse_timestamp_minutes(&cols[0]).is_some() {
                if let Some((g_val, bound)) = parse_glucose_cell(&cols[1], GlucoseUnit::MgDl) {
                    let monitor = monitor_hint.unwrap_or(GlucoseMonitor::AbbottLingo);
                    let lc = cols
                        .get(2)
                        .filter(|s| !s.is_empty())
                        .and_then(|s| s.parse::<f64>().ok())
                        .filter(|v| v.is_finite() && *v >= 0.0);
                    let tag = cols.get(3).filter(|s| !s.is_empty()).cloned();
                    raw_rows.push(ParsedGlucoseRow {
                        timestamp: cols[0].clone(),
                        glucose_mg_dl: g_val,
                        lingo_count: lc,
                        tag,
                        bound,
                    });
                    saw_bgm_only = monitor == GlucoseMonitor::FingerstickBgm;
                    header_spec = Some(HeaderSpec {
                        monitor,
                        unit: GlucoseUnit::MgDl,
                        method: monitor.default_method(),
                        ts_col: 0,
                        time_col: None,
                        glucose_cols: vec![1],
                        event_type_col: None,
                        record_type_col: None,
                        lingo_count_col: Some(2),
                        note_col: Some(3),
                    });
                    continue;
                }
            }
            continue;
        }

        let Some(spec) = header_spec.as_ref() else {
            continue;
        };

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
                continue;
            }
        }

        // LibreView: accept Record Type 0 (Historic CGM), 1 (Scan CGM), or 2 (Strip BGM).
        if let Some(rt_idx) = spec.record_type_col {
            let rt = cols.get(rt_idx).map(|s| s.trim()).unwrap_or("");
            if !rt.is_empty() && rt != "0" && rt != "1" && rt != "2" {
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

        let mut parsed_glucose: Option<(f64, GlucoseReadingBound)> = None;
        for &g_idx in &spec.glucose_cols {
            let cell = cols.get(g_idx).map(|s| s.trim()).unwrap_or("");
            if cell.is_empty() {
                continue;
            }
            if let Some(val) = parse_glucose_cell(cell, spec.unit) {
                parsed_glucose = Some(val);
                break;
            }
        }
        let Some((glucose_mg_dl, bound)) = parsed_glucose else {
            continue;
        };

        if !glucose_mg_dl.is_finite() || !(MIN_MG_DL..=MAX_MG_DL).contains(&glucose_mg_dl) {
            return Err(format!(
                "Line {} glucose `{glucose_mg_dl:.1} mg/dL` is outside physiological range ({MIN_MG_DL}–{MAX_MG_DL} mg/dL).",
                line_idx + 1
            ));
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

        raw_rows.push(ParsedGlucoseRow {
            timestamp,
            glucose_mg_dl,
            lingo_count,
            tag,
            bound,
        });
    }

    if raw_rows.is_empty() {
        return Err(
            "No valid glucose readings found. Supported exports: Abbott Lingo, FreeStyle LibreView, Dexcom Clarity/Stelo, Medtronic CareLink, Eversense, Tandem, Omnipod/Glooko/Tidepool, or BGM CSV."
                .to_string(),
        );
    }

    let spec = header_spec.unwrap_or(HeaderSpec {
        monitor: monitor_hint.unwrap_or(GlucoseMonitor::GenericCsv),
        unit: GlucoseUnit::MgDl,
        method: MeasurementMethod::InterstitialCgm,
        ts_col: 0,
        time_col: None,
        glucose_cols: vec![1],
        event_type_col: None,
        record_type_col: None,
        lingo_count_col: None,
        note_col: None,
    });

    // Compute relative hour offsets chronologically.
    let mut indexed: Vec<IndexedGlucoseRow> = raw_rows
        .into_iter()
        .enumerate()
        .map(|(order, row)| IndexedGlucoseRow {
            minutes: parse_timestamp_minutes(&row.timestamp),
            order,
            row,
        })
        .collect();
    indexed.sort_by(|a, b| match (a.minutes, b.minutes) {
        (Some(ma), Some(mb)) => ma.total_cmp(&mb),
        _ => a.order.cmp(&b.order),
    });

    let first_min = indexed.iter().find_map(|item| item.minutes);
    let mut bounded_sentinels = 0usize;
    let readings: Vec<LingoReading> = indexed
        .into_iter()
        .enumerate()
        .map(|(idx, item)| {
            if item.row.bound != GlucoseReadingBound::Measured {
                bounded_sentinels += 1;
            }
            let hour_offset = match (item.minutes, first_min) {
                (Some(m), Some(start)) => ((m - start) / 60.0).max(0.0),
                _ => idx as f64 * 0.25,
            };
            LingoReading {
                timestamp: item.row.timestamp,
                hour_offset,
                glucose_mg_dl: item.row.glucose_mg_dl,
                lingo_count: item.row.lingo_count,
                tag: item.row.tag,
            }
        })
        .collect();

    let method = if saw_bgm_only && spec.monitor == GlucoseMonitor::FingerstickBgm {
        MeasurementMethod::CapillaryBgm
    } else {
        spec.method
    };

    let detected = if spec.monitor == GlucoseMonitor::DexcomClarity && saw_stelo_signal {
        GlucoseMonitor::DexcomStelo
    } else {
        spec.monitor
    };

    Ok(CommercialGlucoseImport {
        monitor: monitor_hint.unwrap_or(detected),
        source_unit: spec.unit,
        method,
        readings,
        bounded_sentinels,
    })
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
    let lower: Vec<String> = cols.iter().map(|c| c.trim().to_ascii_lowercase()).collect();
    let joined = lower.join("|");

    let mut ts_col = None;
    let mut time_col = None;
    let mut glucose_cols = Vec::new();
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
                || h == "eventdatetime"
                || h == "datetime"
                || h == "date/time"
                || h == "date")
        {
            ts_col = Some(i);
        } else if time_col.is_none() && h == "time" {
            time_col = Some(i);
        }

        let is_rate = h.contains("rate of change");
        let is_glucose_col = !is_rate
            && (h.contains("historic glucose")
                || h.contains("scan glucose")
                || h.contains("sensor glucose")
                || h.contains("cgm glucose")
                || h.contains("readings (cgm)")
                || h.contains("glucose value")
                || h.contains("glucose_mg_dl")
                || h.contains("glucose_mmol")
                || h.contains("blood glucose")
                || h.contains("bg reading")
                || h.contains("strip glucose")
                || h == "glucose"
                || h == "glucose (mg/dl)"
                || h == "glucose (mmol/l)"
                || h == "bg (mg/dl)"
                || h == "bg (mmol/l)"
                || h == "mg/dl"
                || h == "mmol/l");
        if is_glucose_col {
            glucose_cols.push(i);
        }

        if event_type_col.is_none() && h == "event type" {
            event_type_col = Some(i);
        }
        if record_type_col.is_none() && h == "record type" {
            record_type_col = Some(i);
        }
        if lingo_count_col.is_none() && h.contains("lingo") {
            lingo_count_col = Some(i);
        }
        if note_col.is_none() && (h == "tag" || h == "note" || h == "notes" || h == "context") {
            note_col = Some(i);
        }
    }

    let ts_col = ts_col?;
    if glucose_cols.is_empty() {
        return None;
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
    } else if joined.contains("lingo_count") || joined.contains("lingo count") {
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

    Some(HeaderSpec {
        monitor: detected_monitor,
        unit,
        method: detected_monitor.default_method(),
        ts_col,
        time_col,
        glucose_cols,
        event_type_col,
        record_type_col,
        lingo_count_col,
        note_col,
    })
}

fn parse_glucose_cell(cell: &str, unit: GlucoseUnit) -> Option<(f64, GlucoseReadingBound)> {
    let trimmed = cell.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower == "low" || lower == "lo" || lower.starts_with("below 40") {
        return Some((40.0, GlucoseReadingBound::BelowSensorFloor));
    }
    if lower.starts_with("below 70") || lower == "<70" || lower == "< 70" {
        return Some((65.0, GlucoseReadingBound::BelowSensorFloor));
    }
    if lower == "high" || lower == "hi" || lower.starts_with("above 400") {
        return Some((400.0, GlucoseReadingBound::AboveSensorCeiling));
    }
    if lower.starts_with("above 250") || lower == ">250" || lower == "> 250" {
        return Some((255.0, GlucoseReadingBound::AboveSensorCeiling));
    }

    let numeric_str = trimmed
        .trim_start_matches('<')
        .trim_start_matches('>')
        .trim();
    let raw = numeric_str.parse::<f64>().ok()?;
    Some((unit.to_mg_dl(raw), GlucoseReadingBound::Measured))
}

/// Extract monotonic minutes from ISO-8601 (`YYYY-MM-DDTHH:MM`), `YYYY-MM-DD HH:MM`,
/// `MM/DD/YYYY HH:MM [AM|PM]`, or `DD-MM-YYYY HH:MM`.
fn parse_timestamp_minutes(ts: &str) -> Option<f64> {
    let cleaned = ts.trim().replace('T', " ");
    let mut parts = cleaned.split_whitespace();
    let date_part = parts.next()?;
    let time_part = parts.next()?;
    let am_pm = parts.next().map(str::to_ascii_uppercase);

    let (year, month, day) = if date_part.contains('-') {
        let d: Vec<&str> = date_part.split('-').collect();
        if d.len() != 3 {
            return None;
        }
        if d[0].len() == 4 {
            (
                d[0].parse::<i64>().ok()?,
                d[1].parse::<i64>().ok()?,
                d[2].parse::<i64>().ok()?,
            )
        } else {
            // DD-MM-YYYY or MM-DD-YYYY (LibreView US uses MM-DD-YYYY)
            let a = d[0].parse::<i64>().ok()?;
            let b = d[1].parse::<i64>().ok()?;
            let y = d[2].parse::<i64>().ok()?;
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
                d[0].parse::<i64>().ok()?,
                d[1].parse::<i64>().ok()?,
                d[2].parse::<i64>().ok()?,
            )
        } else {
            // CareLink / US BGM: MM/DD/YYYY
            (
                d[2].parse::<i64>().ok()?,
                d[0].parse::<i64>().ok()?,
                d[1].parse::<i64>().ok()?,
            )
        }
    } else {
        return None;
    };

    let time_clean = time_part
        .trim_end_matches('Z')
        .split('+')
        .next()
        .unwrap_or(time_part);
    let mut t = time_clean.split(':');
    let mut hour: i64 = t.next()?.parse().ok()?;
    let minute: i64 = t.next()?.parse().ok()?;
    if let Some(meridiem) = am_pm {
        if meridiem == "PM" && hour < 12 {
            hour += 12;
        } else if meridiem == "AM" && hour == 12 {
            hour = 0;
        }
    }
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || !(0..24).contains(&hour)
        || !(0..60).contains(&minute)
    {
        return None;
    }

    let day_index = year * 372 + month * 31 + day;
    Some((day_index * 1440 + hour * 60 + minute) as f64)
}

fn extract_clock_hour(ts: &str, fallback_offset: f64) -> u8 {
    let cleaned = ts.trim().replace('T', " ");
    let mut parts = cleaned.split_whitespace();
    let _date = parts.next();
    if let Some(time_part) = parts.next() {
        let am_pm = parts.next().map(str::to_ascii_uppercase);
        if let Some(mut h) = time_part
            .split(':')
            .next()
            .and_then(|s| s.parse::<u8>().ok())
        {
            if let Some(meridiem) = am_pm {
                if meridiem == "PM" && h < 12 {
                    h += 12;
                } else if meridiem == "AM" && h == 12 {
                    h = 0;
                }
            }
            if h < 24 {
                return h;
            }
        }
    }
    ((fallback_offset.floor() as i64).rem_euclid(24)) as u8
}

fn percentile_sorted(sorted: &[f64], p: f64) -> f64 {
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

fn compute_mage(readings: &[LingoReading], std_dev: f64) -> f64 {
    if readings.len() < 3 || std_dev <= 0.5 {
        return 0.0;
    }
    let mut extrema = vec![readings[0].glucose_mg_dl];
    for w in readings.windows(3) {
        let (a, b, c) = (w[0].glucose_mg_dl, w[1].glucose_mg_dl, w[2].glucose_mg_dl);
        if (b >= a && b > c) || (b <= a && b < c) {
            extrema.push(b);
        }
    }
    extrema.push(readings[readings.len() - 1].glucose_mg_dl);

    let swings: Vec<f64> = extrema
        .windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .filter(|delta| *delta > std_dev)
        .collect();
    if swings.is_empty() {
        0.0
    } else {
        swings.iter().sum::<f64>() / swings.len() as f64
    }
}

/// Compute comprehensive ADA/ATTD clinical summary + 24-hour AGP percentile bins.
pub fn summarize_commercial_glucose(
    readings: &[LingoReading],
    monitor: GlucoseMonitor,
    source_unit: GlucoseUnit,
    method: MeasurementMethod,
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
    }

    let tbr_very_low_lt_54_pct = (very_low as f64 / n) * 100.0;
    let tbr_low_54_69_pct = (low_54_69 as f64 / n) * 100.0;
    let titr_70_140_pct = (tight_70_140 as f64 / n) * 100.0;
    let tir_70_180_pct = (tir_70_180 as f64 / n) * 100.0;
    let tar_high_181_250_pct = (high_181_250 as f64 / n) * 100.0;
    let tar_very_high_gt_250_pct = (very_high_250 as f64 / n) * 100.0;

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
    let span_h = readings
        .last()
        .map(|r| r.hour_offset)
        .unwrap_or(0.0)
        .max(0.0);
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

/// Return a built-in synthetic sample CSV for quick one-click testing of major commercial ecosystems.
pub fn sample_csv_for_monitor(monitor: GlucoseMonitor) -> &'static str {
    match monitor {
        GlucoseMonitor::DexcomClarity | GlucoseMonitor::DexcomStelo => SAMPLE_DEXCOM_CLARITY_CSV,
        GlucoseMonitor::AbbottFreeStyleLibre => SAMPLE_LIBREVIEW_MMOL_CSV,
        GlucoseMonitor::MedtronicCareLink => SAMPLE_MEDTRONIC_CARELINK_CSV,
        GlucoseMonitor::FingerstickBgm => SAMPLE_BGM_FINGERSTICK_CSV,
        GlucoseMonitor::AbbottLingo
        | GlucoseMonitor::SenseonicsEversense
        | GlucoseMonitor::TandemSource
        | GlucoseMonitor::GlookoTidepool
        | GlucoseMonitor::GenericCsv => SAMPLE_LINGO_CSV,
    }
}

/// ADA/ATTD 5-Band Time-in-Range stacked bar + target compliance strip.
#[component]
pub fn AdaTimeInRangeBar(summary: ClinicalGlucoseSummary) -> impl IntoView {
    let vlow = summary.tbr_very_low_lt_54_pct;
    let low = summary.tbr_low_54_69_pct;
    let tight = summary.titr_70_140_pct;
    let upper_in_range = (summary.tir_70_180_pct - summary.titr_70_140_pct).max(0.0);
    let high = summary.tar_high_181_250_pct;
    let vhigh = summary.tar_very_high_gt_250_pct;

    view! {
        <div class="space-y-3">
            <div class="flex h-6 w-full overflow-hidden rounded-full border border-white/10 bg-slate-900">
                <div style=format!("width: {vlow:.2}%") class="bg-red-500" title=format!("<54 mg/dL: {vlow:.1}%")></div>
                <div style=format!("width: {low:.2}%") class="bg-rose-400" title=format!("54–69 mg/dL: {low:.1}%")></div>
                <div style=format!("width: {tight:.2}%") class="bg-emerald-400" title=format!("70–140 tight: {tight:.1}%")></div>
                <div style=format!("width: {upper_in_range:.2}%") class="bg-teal-500/80" title=format!("141–180 in-range: {upper_in_range:.1}%")></div>
                <div style=format!("width: {high:.2}%") class="bg-amber-400" title=format!("181–250 high: {high:.1}%")></div>
                <div style=format!("width: {vhigh:.2}%") class="bg-orange-500" title=format!(">250 very high: {vhigh:.1}%")></div>
            </div>
            <div class="grid grid-cols-2 gap-2 text-xs sm:grid-cols-3 lg:grid-cols-6">
                <div class="rounded-lg border border-white/5 bg-slate-950/60 px-2.5 py-1.5">
                    <span class="font-semibold text-red-400">{format!("{vlow:.1}%")}</span>
                    <p class="text-[11px] text-slate-400">"<54 (target <1%)"</p>
                </div>
                <div class="rounded-lg border border-white/5 bg-slate-950/60 px-2.5 py-1.5">
                    <span class="font-semibold text-rose-300">{format!("{low:.1}%")}</span>
                    <p class="text-[11px] text-slate-400">"54–69 (<70 <4%)"</p>
                </div>
                <div class="rounded-lg border border-white/5 bg-slate-950/60 px-2.5 py-1.5">
                    <span class="font-semibold text-emerald-300">{format!("{tight:.1}%")}</span>
                    <p class="text-[11px] text-slate-400">"70–140 Tight"</p>
                </div>
                <div class="rounded-lg border border-white/5 bg-slate-950/60 px-2.5 py-1.5">
                    <span class="font-semibold text-teal-300">{format!("{:.1}%", summary.tir_70_180_pct)}</span>
                    <p class="text-[11px] text-slate-400">"70–180 (≥70%)"</p>
                </div>
                <div class="rounded-lg border border-white/5 bg-slate-950/60 px-2.5 py-1.5">
                    <span class="font-semibold text-amber-300">{format!("{high:.1}%")}</span>
                    <p class="text-[11px] text-slate-400">"181–250 (<25%)"</p>
                </div>
                <div class="rounded-lg border border-white/5 bg-slate-950/60 px-2.5 py-1.5">
                    <span class="font-semibold text-orange-400">{format!("{vhigh:.1}%")}</span>
                    <p class="text-[11px] text-slate-400">">250 (target <5%)"</p>
                </div>
            </div>
        </div>
    }
}

/// 24-Hour Ambulatory Glucose Profile (AGP) percentile ribbon SVG (`p5–p95`, `p25–p75`, `p50`).
#[component]
pub fn AgpPercentileChart(bins: Vec<AgpHourBin>) -> impl IntoView {
    let width = 760.0_f64;
    let height = 210.0_f64;
    let pad_l = 44.0_f64;
    let pad_r = 20.0_f64;
    let pad_t = 16.0_f64;
    let pad_b = 26.0_f64;
    let plot_w = width - pad_l - pad_r;
    let plot_h = height - pad_t - pad_b;

    let populated: Vec<&AgpHourBin> = bins.iter().filter(|b| b.count > 0).collect();
    let max_g = populated
        .iter()
        .map(|b| b.p95)
        .fold(220.0_f64, f64::max)
        .max(220.0);
    let min_g = 45.0_f64;

    let x_for = |h: u8| pad_l + (f64::from(h) / 23.0) * plot_w;
    let y_for = |g: f64| {
        let clamped = g.clamp(min_g, max_g);
        pad_t + plot_h - ((clamped - min_g) / (max_g - min_g)) * plot_h
    };

    let build_ribbon =
        |upper: &dyn Fn(&AgpHourBin) -> f64, lower: &dyn Fn(&AgpHourBin) -> f64| -> String {
            if populated.len() < 2 {
                return String::new();
            }
            let mut pts: Vec<String> = populated
                .iter()
                .map(|b| format!("{:.1},{:.1}", x_for(b.hour), y_for(upper(b))))
                .collect();
            for b in populated.iter().rev() {
                pts.push(format!("{:.1},{:.1}", x_for(b.hour), y_for(lower(b))));
            }
            pts.join(" ")
        };

    let outer_poly = build_ribbon(&|b| b.p95, &|b| b.p5);
    let iqr_poly = build_ribbon(&|b| b.p75, &|b| b.p25);
    let median_line: String = populated
        .iter()
        .map(|b| format!("{:.1},{:.1}", x_for(b.hour), y_for(b.p50)))
        .collect::<Vec<_>>()
        .join(" ");

    let y70 = y_for(70.0);
    let y140 = y_for(140.0);
    let y180 = y_for(180.0);

    view! {
        <svg
            viewBox="0 0 760 210"
            class="w-full"
            role="img"
            aria-label="24-hour Ambulatory Glucose Profile showing 5th-95th and 25th-75th percentile ribbons and median"
        >
            // 70-180 clinical target band + 70-140 tight sub-band
            <rect x=pad_l y=y180 width=plot_w height=(y70 - y180).max(1.0) fill="rgba(45, 212, 191, 0.06)" />
            <rect x=pad_l y=y140 width=plot_w height=(y70 - y140).max(1.0) fill="rgba(52, 211, 153, 0.09)" />
            <line x1=pad_l y1=y180 x2=width - pad_r y2=y180 stroke="rgba(251, 191, 36, 0.35)" stroke-dasharray="4 4" />
            <line x1=pad_l y1=y140 x2=width - pad_r y2=y140 stroke="rgba(52, 211, 153, 0.35)" stroke-dasharray="3 3" />
            <line x1=pad_l y1=y70 x2=width - pad_r y2=y70 stroke="rgba(251, 113, 133, 0.40)" stroke-dasharray="4 4" />

            // Outer 5th-95th ribbon
            {(!outer_poly.is_empty()).then(|| view! {
                <polygon points=outer_poly fill="rgba(34, 211, 238, 0.14)" />
            })}
            // Inner 25th-75th IQR ribbon
            {(!iqr_poly.is_empty()).then(|| view! {
                <polygon points=iqr_poly fill="rgba(34, 211, 238, 0.30)" />
            })}
            // 50th percentile median curve
            {(!median_line.is_empty()).then(|| view! {
                <polyline
                    fill="none"
                    stroke="#22d3ee"
                    stroke-width="2.5"
                    stroke-linecap="round"
                    stroke-linejoin="round"
                    points=median_line
                />
            })}

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
}
