//! Pure-Rust commercial continuous glucose monitoring (CGM) and capillary blood glucose meter
//! (BGM) multi-device CSV ingestion, auto-detection, and ADA/ATTD International Consensus
//! Ambulatory Glucose Profile (AGP) clinical analytics.

use crate::driver::DbError;
use crate::lingo::{clinical_tir_status, tight_tir_status};
use crate::store::{DEMO_DATE, DemoSourceRecord, DemoStore, next_seq};
use physician_wire::{CitedFieldWire, CitedTextPartWire};
use serde::{Deserialize, Serialize};
use serde_json::json;

/// Conversion factor from `mmol/L` to `mg/dL` (`180.156 g/mol` glucose molar mass -> `18.0182`).
pub const MMOL_L_TO_MG_DL: f64 = 18.0182;

const MIN_COMMERCIAL_GLUCOSE_MG_DL: f64 = 20.0;
const MAX_COMMERCIAL_GLUCOSE_MG_DL: f64 = 600.0;

const ADA_VERY_LOW_MG_DL: f64 = 54.0;
const ADA_LOW_MG_DL: f64 = 70.0;
const ADA_TIGHT_HIGH_MG_DL: f64 = 140.0;
const ADA_CLINICAL_HIGH_MG_DL: f64 = 180.0;
const ADA_VERY_HIGH_MG_DL: f64 = 250.0;

/// Supported commercial continuous glucose monitors (CGM) and blood glucose meters (BGM).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GlucoseMonitor {
    /// Abbott Lingo over-the-counter upper-arm biosensor.
    AbbottLingo,
    /// Abbott FreeStyle Libre 2 / Libre 3 / LibreView export.
    AbbottFreeStyleLibre,
    /// Dexcom G6 / G7 / ONE+ Clarity export.
    DexcomClarity,
    /// Dexcom Stelo over-the-counter glucose biosensor.
    DexcomStelo,
    /// Medtronic CareLink (MiniMed 780G / Guardian 4 / Simplera) export.
    MedtronicCareLink,
    /// Senseonics Eversense E3 / 365 implantable CGM export.
    SenseonicsEversense,
    /// Tandem Source / t:connect (t:slim X2 / Tandem Mobi) CGM export.
    TandemSource,
    /// Omnipod 5 / Glooko / Tidepool aggregated diabetes data export.
    GlookoTidepool,
    /// Capillary fingerstick blood glucose meter (Accu-Chek, OneTouch, Contour, True Metrix).
    FingerstickBgm,
    /// Generic timestamped CSV glucose stream.
    GenericCsv,
}

impl Serialize for GlucoseMonitor {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.slug())
    }
}

impl<'de> Deserialize<'de> for GlucoseMonitor {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Self::from_slug(&s).ok_or_else(|| {
            serde::de::Error::custom(format!("unsupported GlucoseMonitor slug `{s}`"))
        })
    }
}

impl GlucoseMonitor {
    /// Human-readable clinical source label for provenance and `SourcePane`.
    pub fn display_name(self) -> &'static str {
        match self {
            Self::AbbottLingo => "Abbott Lingo CGM",
            Self::AbbottFreeStyleLibre => "Abbott FreeStyle Libre CGM",
            Self::DexcomClarity => "Dexcom Clarity CGM",
            Self::DexcomStelo => "Dexcom Stelo CGM",
            Self::MedtronicCareLink => "Medtronic CareLink CGM",
            Self::SenseonicsEversense => "Senseonics Eversense CGM",
            Self::TandemSource => "Tandem Source CGM",
            Self::GlookoTidepool => "Glooko / Tidepool CGM",
            Self::FingerstickBgm => "Capillary Blood Glucose Meter (BGM)",
            Self::GenericCsv => "Generic Glucose CSV",
        }
    }

    /// Default physiological measurement modality for this device family.
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

    /// Canonical snake_case identifier for API requests and responses.
    pub fn slug(self) -> &'static str {
        match self {
            Self::AbbottLingo => "abbott_lingo",
            Self::AbbottFreeStyleLibre => "abbott_freestyle_libre",
            Self::DexcomClarity => "dexcom_clarity",
            Self::DexcomStelo => "dexcom_stelo",
            Self::MedtronicCareLink => "medtronic_carelink",
            Self::SenseonicsEversense => "senseonics_eversense",
            Self::TandemSource => "tandem_source",
            Self::GlookoTidepool => "glooko_tidepool",
            Self::FingerstickBgm => "fingerstick_bgm",
            Self::GenericCsv => "generic_csv",
        }
    }

    /// Parses a canonical slug or common device alias into [`GlucoseMonitor`].
    pub fn from_slug(raw: &str) -> Option<Self> {
        let normalized = raw.trim().to_ascii_lowercase().replace(['-', ' '], "_");
        match normalized.as_str() {
            "abbott_lingo" | "lingo" => Some(Self::AbbottLingo),
            "abbott_freestyle_libre"
            | "abbott_free_style_libre"
            | "freestyle_libre"
            | "libreview"
            | "libre" => Some(Self::AbbottFreeStyleLibre),
            "dexcom_clarity" | "dexcom" | "dexcom_g7" | "dexcom_g6" | "dexcom_one" => {
                Some(Self::DexcomClarity)
            }
            "dexcom_stelo" | "stelo" => Some(Self::DexcomStelo),
            "medtronic_carelink" | "carelink" | "medtronic" | "guardian" | "simplera" => {
                Some(Self::MedtronicCareLink)
            }
            "senseonics_eversense" | "eversense" | "senseonics" => Some(Self::SenseonicsEversense),
            "tandem_source" | "tandem" | "tconnect" | "t:connect" | "tslim" => {
                Some(Self::TandemSource)
            }
            "glooko_tidepool" | "glooko" | "tidepool" | "omnipod" | "omnipod_5" => {
                Some(Self::GlookoTidepool)
            }
            "fingerstick_bgm" | "bgm" | "fingerstick" | "smbg" | "glucometer" => {
                Some(Self::FingerstickBgm)
            }
            "generic_csv" | "generic" => Some(Self::GenericCsv),
            _ => None,
        }
    }

    fn nominal_cadence_secs(self) -> f64 {
        match self {
            Self::AbbottFreeStyleLibre => 900.0,
            Self::AbbottLingo
            | Self::DexcomClarity
            | Self::DexcomStelo
            | Self::MedtronicCareLink
            | Self::SenseonicsEversense
            | Self::TandemSource
            | Self::GlookoTidepool
            | Self::FingerstickBgm
            | Self::GenericCsv => 300.0,
        }
    }
}

/// Source unit of a glucose export before normalization to `mg/dL`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GlucoseUnit {
    /// Milligrams per deciliter (`mg/dL`, US clinical convention).
    MgDl,
    /// Millimoles per liter (`mmol/L`, SI / international clinical convention).
    MmolL,
}

impl GlucoseUnit {
    /// Converts `value` in this unit into `mg/dL`.
    pub fn to_mg_dl(self, value: f64) -> f64 {
        match self {
            Self::MgDl => value,
            Self::MmolL => value * MMOL_L_TO_MG_DL,
        }
    }

    /// Display unit string (`"mg/dL"` or `"mmol/L"`).
    pub fn label(self) -> &'static str {
        match self {
            Self::MgDl => "mg/dL",
            Self::MmolL => "mmol/L",
        }
    }
}

/// Physiological sampling compartment of a glucose measurement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeasurementMethod {
    /// Subcutaneous interstitial fluid glucose biosensor (CGM).
    InterstitialCgm,
    /// Capillary whole-blood / plasma-calibrated test-strip meter (BGM / SMBG).
    CapillaryBgm,
}

/// Indicates whether a reading was within numeric sensor range or clipped at a device boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GlucoseReadingBound {
    /// Numeric measurement reported within the sensor's dynamic range.
    Measured,
    /// Clipped at the device's lower reporting threshold (for example `"Low"` or `"Below 70"`).
    BelowSensorFloor,
    /// Clipped at the device's upper reporting threshold (for example `"High"` or `"Above 250"`).
    AboveSensorCeiling,
}

/// Klonoff et al. (2023) Glycemia Risk Index (GRI) clinical risk zone (`0..=100`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GriZone {
    /// Lowest dysglycemia risk (`0.0..=20.0`).
    ZoneA,
    /// Mild dysglycemia risk (`> 20.0..=40.0`).
    ZoneB,
    /// Moderate dysglycemia risk (`> 40.0..=60.0`).
    ZoneC,
    /// High dysglycemia risk (`> 60.0..=80.0`).
    ZoneD,
    /// Very high dysglycemia risk (`> 80.0..=100.0`).
    ZoneE,
}

impl GriZone {
    /// Maps a clamped GRI score (`0.0..=100.0`) to its clinical risk zone.
    pub fn from_gri(gri: f64) -> Self {
        if gri <= 20.0 {
            Self::ZoneA
        } else if gri <= 40.0 {
            Self::ZoneB
        } else if gri <= 60.0 {
            Self::ZoneC
        } else if gri <= 80.0 {
            Self::ZoneD
        } else {
            Self::ZoneE
        }
    }

    /// Physician-facing zone label.
    pub fn label(self) -> &'static str {
        match self {
            Self::ZoneA => "Zone A",
            Self::ZoneB => "Zone B",
            Self::ZoneC => "Zone C",
            Self::ZoneD => "Zone D",
            Self::ZoneE => "Zone E",
        }
    }

    fn field_status(self) -> &'static str {
        match self {
            Self::ZoneA => "Normal",
            Self::ZoneB => "Mild",
            Self::ZoneC => "Moderate",
            Self::ZoneD | Self::ZoneE => "High",
        }
    }
}

/// One normalized glucose measurement from any commercial CGM or BGM export.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GlucoseReading {
    /// Device timestamp string as recorded in the export.
    pub timestamp: String,
    /// Normalized glucose concentration in `mg/dL`.
    pub glucose_mg_dl: f64,
    /// Original numeric (or sentinel-mapped) value in [`Self::source_unit`].
    pub raw_value: f64,
    /// Unit in which the export reported this reading.
    pub source_unit: GlucoseUnit,
    /// Measurement compartment (`InterstitialCgm` or `CapillaryBgm`).
    pub method: MeasurementMethod,
    /// Dynamic-range bound status (`Measured`, `BelowSensorFloor`, or `AboveSensorCeiling`).
    pub bound: GlucoseReadingBound,
    /// Optional glucose rate of change in `mg/dL/min` (Dexcom / Eversense / Tandem).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trend_mg_dl_per_min: Option<f64>,
    /// Optional Abbott Lingo Count exposure points for this interval.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lingo_count: Option<f64>,
    /// Optional meal, symptom, insulin, or event context note.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl GlucoseReading {
    /// Validates timestamp presence and physiological commercial bounds (`20.0..=600.0 mg/dL`).
    pub fn validate(&self, line: usize) -> Result<(), CgmParseError> {
        if self.timestamp.trim().is_empty() {
            return Err(CgmParseError::InvalidRow {
                line,
                detail: "timestamp cannot be empty".into(),
            });
        }
        if !self.glucose_mg_dl.is_finite()
            || !(MIN_COMMERCIAL_GLUCOSE_MG_DL..=MAX_COMMERCIAL_GLUCOSE_MG_DL)
                .contains(&self.glucose_mg_dl)
        {
            return Err(CgmParseError::OutOfRange {
                line,
                field: "glucose_mg_dl",
                detail: format!(
                    "expected finite value in {MIN_COMMERCIAL_GLUCOSE_MG_DL}..={MAX_COMMERCIAL_GLUCOSE_MG_DL} mg/dL, got {:.1}",
                    self.glucose_mg_dl
                ),
            });
        }
        if let Some(lc) = self.lingo_count
            && (!lc.is_finite() || lc < 0.0)
        {
            return Err(CgmParseError::OutOfRange {
                line,
                field: "lingo_count",
                detail: format!("expected non-negative finite number, got {lc}"),
            });
        }
        Ok(())
    }
}

/// Result of parsing a commercial CGM or BGM CSV export.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ParsedGlucoseImport {
    /// Detected (or caller-specified) commercial monitor family.
    pub monitor: GlucoseMonitor,
    /// Primary reporting unit detected in the export (`mg/dL` or `mmol/L`).
    pub source_unit: GlucoseUnit,
    /// Primary measurement modality across parsed readings.
    pub method: MeasurementMethod,
    /// Validated glucose readings in file order.
    pub readings: Vec<GlucoseReading>,
}

/// Errors returned while auto-detecting, parsing, or validating commercial CGM/BGM exports.
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum CgmParseError {
    /// No valid glucose readings were present in the input.
    #[error("CGM/BGM CSV input contains no valid glucose readings")]
    EmptyInput,
    /// Header or structure did not match any supported commercial glucose export.
    #[error("unrecognized glucose CSV format: {detail}")]
    UnrecognizedFormat {
        /// Explanation of what failed format detection.
        detail: String,
    },
    /// Malformed CSV syntax or missing required columns on a row.
    #[error("invalid glucose CSV row at line {line}: {detail}")]
    InvalidRow {
        /// 1-based input line number.
        line: usize,
        /// Diagnostic detail.
        detail: String,
    },
    /// Numeric field failed floating-point parsing.
    #[error("invalid `{field}` value `{raw}` at line {line}")]
    InvalidFloat {
        /// 1-based input line number.
        line: usize,
        /// Column field name.
        field: &'static str,
        /// Raw text that failed to parse.
        raw: String,
        /// Underlying float parse error.
        #[source]
        source: std::num::ParseFloatError,
    },
    /// Measurement fell outside supported physiological bounds (`20.0..=600.0 mg/dL`).
    #[error("glucose `{field}` out of range at line {line}: {detail}")]
    OutOfRange {
        /// 1-based input line number.
        line: usize,
        /// Field name.
        field: &'static str,
        /// Range diagnostic detail.
        detail: String,
    },
}

/// One local-hour (`0..=23`) percentile bin of an Ambulatory Glucose Profile (AGP).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AgpHourBin {
    /// Local hour of day (`0..=23`).
    pub hour: u8,
    /// Number of readings falling in this hour across the window.
    pub count: usize,
    /// 5th percentile glucose (`mg/dL`).
    pub p5: f64,
    /// 25th percentile glucose (`mg/dL`, lower interquartile bound).
    pub p25: f64,
    /// 50th percentile (median) glucose (`mg/dL`).
    pub p50: f64,
    /// 75th percentile glucose (`mg/dL`, upper interquartile bound).
    pub p75: f64,
    /// 95th percentile glucose (`mg/dL`).
    pub p95: f64,
}

/// Full ADA/ATTD International Consensus clinical summary and 24-hour AGP over a glucose window.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CgmClinicalSummary {
    /// Commercial monitor family.
    pub monitor: GlucoseMonitor,
    /// Source unit reported in the original export.
    pub source_unit: GlucoseUnit,
    /// Primary measurement modality (`InterstitialCgm` or `CapillaryBgm`).
    pub method: MeasurementMethod,
    /// Number of valid glucose readings analyzed.
    pub count: usize,
    /// First reading timestamp in the window.
    pub start_timestamp: String,
    /// Last reading timestamp in the window.
    pub end_timestamp: String,
    /// Arithmetic mean glucose (`mg/dL`).
    pub mean_mg_dl: f64,
    /// Median glucose (`mg/dL`).
    pub median_mg_dl: f64,
    /// Minimum (nadir) glucose (`mg/dL`).
    pub min_mg_dl: f64,
    /// Maximum (peak) glucose (`mg/dL`).
    pub max_mg_dl: f64,
    /// Population standard deviation (`mg/dL`).
    pub std_dev_mg_dl: f64,
    /// Coefficient of variation (`100 * SD / mean`), ADA stability target `<= 36.0%`.
    pub cv_percent: f64,
    /// Level 2 hypoglycemia `< 54 mg/dL` (`%`), ADA target `< 1.0%`.
    pub tbr_very_low_lt_54_percent: f64,
    /// Level 1 hypoglycemia `54–69 mg/dL` (`%`), combined `< 70` target `< 4.0%`.
    pub tbr_low_54_69_percent: f64,
    /// Standard clinical Time in Range `70–180 mg/dL` (`%`), ADA target `>= 70.0%`.
    pub tir_70_180_percent: f64,
    /// Time in Tight Range `70–140 mg/dL` (`%`), metabolic target `>= 85.0%`.
    pub titr_70_140_percent: f64,
    /// Level 1 hyperglycemia `181–250 mg/dL` (`%`), combined `> 180` target `< 25.0%`.
    pub tar_high_181_250_percent: f64,
    /// Level 2 hyperglycemia `> 250 mg/dL` (`%`), ADA target `< 5.0%`.
    pub tar_very_high_gt_250_percent: f64,
    /// Glucose Management Indicator (`%`), `3.31 + 0.02392 * mean_mg_dl`.
    pub gmi_percent: f64,
    /// Glycemia Risk Index (`0.0..=100.0`), `(3.0*VLow + 2.4*Low + 1.6*VHigh + 0.8*High)`.
    pub gri: f64,
    /// Clinical risk zone (`ZoneA`..`ZoneE`) corresponding to [`Self::gri`].
    pub gri_zone: GriZone,
    /// Mean Amplitude of Glycemic Excursions (`mg/dL`) exceeding `1 SD`.
    pub mage_mg_dl: f64,
    /// Estimated percentage of active sensor wear across `[start_timestamp, end_timestamp]`.
    pub estimated_active_wear_percent: f64,
    /// Count of readings `> 140 mg/dL`.
    pub spike_count_over_140: usize,
    /// Count of readings `> 180 mg/dL`.
    pub high_count_over_180: usize,
    /// Count of readings `< 70 mg/dL`.
    pub low_count_under_70: usize,
    /// Count of readings `< 54 mg/dL`.
    pub very_low_count_under_54: usize,
    /// Optional cumulative Abbott Lingo Count points when Lingo readings are present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_lingo_count: Option<f64>,
    /// 24-hour Ambulatory Glucose Profile percentile bins in ascending hour order.
    pub agp_hourly: Vec<AgpHourBin>,
}

impl CgmClinicalSummary {
    /// Computes full ADA/ATTD clinical summary and 24h AGP from a [`ParsedGlucoseImport`].
    pub fn from_import(import: &ParsedGlucoseImport) -> Result<Self, CgmParseError> {
        Self::from_readings(
            import.monitor,
            import.source_unit,
            import.method,
            &import.readings,
        )
    }

    /// Computes full ADA/ATTD clinical summary and 24h AGP from explicit metadata and readings.
    pub fn from_readings(
        monitor: GlucoseMonitor,
        source_unit: GlucoseUnit,
        method: MeasurementMethod,
        readings: &[GlucoseReading],
    ) -> Result<Self, CgmParseError> {
        if readings.is_empty() {
            return Err(CgmParseError::EmptyInput);
        }
        for (idx, r) in readings.iter().enumerate() {
            r.validate(idx + 1)?;
        }

        let n = readings.len();
        let n_f = n as f64;
        let mut sum = 0.0_f64;
        let mut min_v = f64::INFINITY;
        let mut max_v = f64::NEG_INFINITY;
        let mut sorted_vals = Vec::with_capacity(n);

        let mut vlow_cnt = 0usize;
        let mut low_54_69_cnt = 0usize;
        let mut tir_70_180_cnt = 0usize;
        let mut titr_70_140_cnt = 0usize;
        let mut high_181_250_cnt = 0usize;
        let mut vhigh_gt_250_cnt = 0usize;
        let mut spike_gt_140_cnt = 0usize;

        let mut lingo_sum = 0.0_f64;
        let mut saw_lingo = monitor == GlucoseMonitor::AbbottLingo;

        let mut hour_buckets: [Vec<f64>; 24] = std::array::from_fn(|_| Vec::new());
        let mut epoch_secs: Vec<i64> = Vec::with_capacity(n);

        for r in readings {
            let g = r.glucose_mg_dl;
            sum += g;
            sorted_vals.push(g);
            if g < min_v {
                min_v = g;
            }
            if g > max_v {
                max_v = g;
            }

            match classify_ada_band(g, r.bound) {
                AdaBand::VeryLowLt54 => vlow_cnt += 1,
                AdaBand::Low54To69 => low_54_69_cnt += 1,
                AdaBand::InRange70To180 { tight_70_to_140 } => {
                    tir_70_180_cnt += 1;
                    if tight_70_to_140 {
                        titr_70_140_cnt += 1;
                    }
                }
                AdaBand::High181To250 => high_181_250_cnt += 1,
                AdaBand::VeryHighGt250 => vhigh_gt_250_cnt += 1,
            }
            if g > ADA_TIGHT_HIGH_MG_DL
                || (r.bound == GlucoseReadingBound::AboveSensorCeiling && g >= ADA_TIGHT_HIGH_MG_DL)
            {
                spike_gt_140_cnt += 1;
            }

            if let Some(lc) = r.lingo_count {
                saw_lingo = true;
                lingo_sum += lc;
            } else if monitor == GlucoseMonitor::AbbottLingo && g > ADA_TIGHT_HIGH_MG_DL {
                lingo_sum += ((g - ADA_TIGHT_HIGH_MG_DL) * 0.25).max(0.5);
            }

            if let Some(ts) = parse_timestamp_parts(&r.timestamp) {
                epoch_secs.push(ts.epoch_secs);
                if let Some(bucket) = hour_buckets.get_mut(usize::from(ts.hour)) {
                    bucket.push(g);
                }
            }
        }

        sorted_vals.sort_by(f64::total_cmp);
        let median = percentile_sorted(&sorted_vals, 0.50);
        let mean = sum / n_f;
        let variance = readings
            .iter()
            .map(|r| {
                let d = r.glucose_mg_dl - mean;
                d * d
            })
            .sum::<f64>()
            / n_f;
        let std_dev = variance.sqrt();
        let cv = if mean > 0.0 {
            (std_dev / mean) * 100.0
        } else {
            0.0
        };

        let tbr_vlow_raw = (vlow_cnt as f64 / n_f) * 100.0;
        let tbr_low_raw = (low_54_69_cnt as f64 / n_f) * 100.0;
        let tir_raw = (tir_70_180_cnt as f64 / n_f) * 100.0;
        let titr_raw = (titr_70_140_cnt as f64 / n_f) * 100.0;
        let tar_high_raw = (high_181_250_cnt as f64 / n_f) * 100.0;
        let tar_vhigh_raw = (vhigh_gt_250_cnt as f64 / n_f) * 100.0;

        let gmi = round1(3.31 + 0.02392 * mean);
        let gri = round1(
            (3.0 * tbr_vlow_raw + 2.4 * tbr_low_raw + 1.6 * tar_vhigh_raw + 0.8 * tar_high_raw)
                .clamp(0.0, 100.0),
        );
        let gri_zone = GriZone::from_gri(gri);
        let mage = round1(compute_mage_mg_dl(readings, std_dev));
        let wear = round1(estimate_active_wear_percent(
            monitor,
            method,
            n,
            &mut epoch_secs,
        ));

        let mut agp_hourly = Vec::new();
        for (hr, mut vals) in hour_buckets.into_iter().enumerate() {
            if vals.is_empty() {
                continue;
            }
            vals.sort_by(f64::total_cmp);
            agp_hourly.push(AgpHourBin {
                hour: u8::try_from(hr).unwrap_or(0),
                count: vals.len(),
                p5: round1(percentile_sorted(&vals, 0.05)),
                p25: round1(percentile_sorted(&vals, 0.25)),
                p50: round1(percentile_sorted(&vals, 0.50)),
                p75: round1(percentile_sorted(&vals, 0.75)),
                p95: round1(percentile_sorted(&vals, 0.95)),
            });
        }

        Ok(Self {
            monitor,
            source_unit,
            method,
            count: n,
            start_timestamp: readings[0].timestamp.trim().to_owned(),
            end_timestamp: readings[n - 1].timestamp.trim().to_owned(),
            mean_mg_dl: round1(mean),
            median_mg_dl: round1(median),
            min_mg_dl: round1(min_v),
            max_mg_dl: round1(max_v),
            std_dev_mg_dl: round1(std_dev),
            cv_percent: round1(cv),
            tbr_very_low_lt_54_percent: round1(tbr_vlow_raw),
            tbr_low_54_69_percent: round1(tbr_low_raw),
            tir_70_180_percent: round1(tir_raw),
            titr_70_140_percent: round1(titr_raw),
            tar_high_181_250_percent: round1(tar_high_raw),
            tar_very_high_gt_250_percent: round1(tar_vhigh_raw),
            gmi_percent: gmi,
            gri,
            gri_zone,
            mage_mg_dl: mage,
            estimated_active_wear_percent: wear,
            spike_count_over_140: spike_gt_140_cnt,
            high_count_over_180: high_181_250_cnt + vhigh_gt_250_cnt,
            low_count_under_70: vlow_cnt + low_54_69_cnt,
            very_low_count_under_54: vlow_cnt,
            total_lingo_count: saw_lingo.then(|| round1(lingo_sum)),
            agp_hourly,
        })
    }

    /// Renders structured [`CitedFieldWire`] measurements for `CitedValuesTable` and `SourcePane`.
    pub fn to_cited_fields(&self) -> Vec<CitedFieldWire> {
        let mean_status = if self.mean_mg_dl < ADA_LOW_MG_DL {
            "Low"
        } else if self.mean_mg_dl > 154.0 {
            "Elevated"
        } else {
            "Normal"
        };
        let gmi_status = if self.gmi_percent >= 8.0 {
            "High"
        } else if self.gmi_percent >= 7.0 {
            "Elevated"
        } else {
            "Normal"
        };
        let cv_status = if self.cv_percent > 36.0 {
            "High"
        } else {
            "Normal"
        };
        let tbr_lt_70 = round1(self.tbr_very_low_lt_54_percent + self.tbr_low_54_69_percent);
        let tar_gt_180 = round1(self.tar_high_181_250_percent + self.tar_very_high_gt_250_percent);
        let peak_status =
            if self.tar_very_high_gt_250_percent > 0.0 || self.max_mg_dl > ADA_VERY_HIGH_MG_DL {
                "High"
            } else if self.high_count_over_180 > 0 || self.max_mg_dl > ADA_CLINICAL_HIGH_MG_DL {
                "Elevated"
            } else {
                "Normal"
            };
        let nadir_status = if self.low_count_under_70 > 0 || self.min_mg_dl < ADA_LOW_MG_DL {
            "Low"
        } else {
            "Normal"
        };
        let mage_status = if self.mage_mg_dl >= 65.0 {
            "Elevated"
        } else {
            "Normal"
        };
        let (wear_range, wear_status) = match self.method {
            MeasurementMethod::InterstitialCgm => (
                Some(">= 70.0%".into()),
                Some(
                    if self.estimated_active_wear_percent < 70.0 {
                        "Low"
                    } else {
                        "Normal"
                    }
                    .into(),
                ),
            ),
            MeasurementMethod::CapillaryBgm => (None, None),
        };

        let mut fields = vec![
            CitedFieldWire {
                key: "cgm_monitor".into(),
                name: "Glucose Monitor".into(),
                value: self.monitor.display_name().into(),
                unit: None,
                range: None,
                status: None,
            },
            CitedFieldWire {
                key: "cgm_mean_glucose".into(),
                name: "Mean Glucose".into(),
                value: format!("{:.1}", self.mean_mg_dl),
                unit: Some("mg/dL".into()),
                range: Some("70–154 mg/dL".into()),
                status: Some(mean_status.into()),
            },
            CitedFieldWire {
                key: "cgm_gmi_percent".into(),
                name: "Glucose Management Indicator (GMI)".into(),
                value: format!("{:.1}", self.gmi_percent),
                unit: Some("%".into()),
                range: Some("< 7.0%".into()),
                status: Some(gmi_status.into()),
            },
            CitedFieldWire {
                key: "cgm_gri".into(),
                name: format!("Glycemia Risk Index ({})", self.gri_zone.label()),
                value: format!("{:.1}", self.gri),
                unit: Some("/100".into()),
                range: Some("0–20 Zone A".into()),
                status: Some(self.gri_zone.field_status().into()),
            },
            CitedFieldWire {
                key: "cgm_cv_percent".into(),
                name: "Glycemic Variability (CV)".into(),
                value: format!("{:.1}", self.cv_percent),
                unit: Some("%".into()),
                range: Some("<= 36.0%".into()),
                status: Some(cv_status.into()),
            },
            CitedFieldWire {
                key: "cgm_titr_70_140".into(),
                name: "Time in Tight Range (70–140)".into(),
                value: format!("{:.1}", self.titr_70_140_percent),
                unit: Some("%".into()),
                range: Some(">= 85.0%".into()),
                status: Some(tight_tir_status(self.titr_70_140_percent).into()),
            },
            CitedFieldWire {
                key: "cgm_tir_70_180".into(),
                name: "Time in Range (70–180)".into(),
                value: format!("{:.1}", self.tir_70_180_percent),
                unit: Some("%".into()),
                range: Some(">= 70.0%".into()),
                status: Some(clinical_tir_status(self.tir_70_180_percent).into()),
            },
            CitedFieldWire {
                key: "cgm_tbr_lt_70".into(),
                name: "Time Below Range (< 70)".into(),
                value: format!("{tbr_lt_70:.1}"),
                unit: Some("%".into()),
                range: Some("< 4.0%".into()),
                status: Some(
                    if tbr_lt_70 >= 4.0 {
                        "Elevated"
                    } else {
                        "Normal"
                    }
                    .into(),
                ),
            },
            CitedFieldWire {
                key: "cgm_tbr_lt_54".into(),
                name: "Time Very Low (< 54)".into(),
                value: format!("{:.1}", self.tbr_very_low_lt_54_percent),
                unit: Some("%".into()),
                range: Some("< 1.0%".into()),
                status: Some(
                    if self.tbr_very_low_lt_54_percent >= 1.0 {
                        "High"
                    } else {
                        "Normal"
                    }
                    .into(),
                ),
            },
            CitedFieldWire {
                key: "cgm_tar_gt_180".into(),
                name: "Time Above Range (> 180)".into(),
                value: format!("{tar_gt_180:.1}"),
                unit: Some("%".into()),
                range: Some("< 25.0%".into()),
                status: Some(
                    if tar_gt_180 >= 25.0 {
                        "Elevated"
                    } else {
                        "Normal"
                    }
                    .into(),
                ),
            },
            CitedFieldWire {
                key: "cgm_tar_gt_250".into(),
                name: "Time Very High (> 250)".into(),
                value: format!("{:.1}", self.tar_very_high_gt_250_percent),
                unit: Some("%".into()),
                range: Some("< 5.0%".into()),
                status: Some(
                    if self.tar_very_high_gt_250_percent >= 5.0 {
                        "High"
                    } else {
                        "Normal"
                    }
                    .into(),
                ),
            },
            CitedFieldWire {
                key: "cgm_peak_glucose".into(),
                name: "Peak Glucose".into(),
                value: format!("{:.1}", self.max_mg_dl),
                unit: Some("mg/dL".into()),
                range: Some("<= 180.0 mg/dL".into()),
                status: Some(peak_status.into()),
            },
            CitedFieldWire {
                key: "cgm_nadir_glucose".into(),
                name: "Nadir Glucose".into(),
                value: format!("{:.1}", self.min_mg_dl),
                unit: Some("mg/dL".into()),
                range: Some(">= 70.0 mg/dL".into()),
                status: Some(nadir_status.into()),
            },
            CitedFieldWire {
                key: "cgm_mage_mg_dl".into(),
                name: "MAGE (Excursion Amplitude)".into(),
                value: format!("{:.1}", self.mage_mg_dl),
                unit: Some("mg/dL".into()),
                range: Some("< 65.0 mg/dL".into()),
                status: Some(mage_status.into()),
            },
            CitedFieldWire {
                key: "cgm_active_wear_percent".into(),
                name: "Active Sensor Wear".into(),
                value: format!("{:.1}", self.estimated_active_wear_percent),
                unit: Some("%".into()),
                range: wear_range,
                status: wear_status,
            },
        ];

        if let Some(lc) = self.total_lingo_count {
            fields.push(CitedFieldWire {
                key: "lingo_total_count".into(),
                name: "Total Lingo Count".into(),
                value: format!("{lc:.1}"),
                unit: Some("pts".into()),
                range: Some("<= 45 pts/day".into()),
                status: None,
            });
        }
        fields
    }
}

/// Auto-detects and parses CSV exports from all major commercial CGM systems and fingerstick BGM
/// meters in either `mg/dL` or `mmol/L`.
pub fn parse_commercial_glucose_csv(
    csv_text: &str,
    monitor_hint: Option<GlucoseMonitor>,
) -> Result<ParsedGlucoseImport, CgmParseError> {
    let clean_text = csv_text.strip_prefix('\u{feff}').unwrap_or(csv_text);
    let mut non_empty_lines: Vec<(usize, &str)> = Vec::new();
    let mut preamble_lower = String::new();

    for (idx, raw) in clean_text.lines().enumerate() {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with('#') {
            preamble_lower.push_str(&trimmed.to_ascii_lowercase());
            preamble_lower.push('\n');
            continue;
        }
        if is_carelink_divider(trimmed) {
            preamble_lower.push_str("medtronic carelink divider ");
            preamble_lower.push_str(&trimmed.to_ascii_lowercase());
            preamble_lower.push('\n');
            continue;
        }
        non_empty_lines.push((idx + 1, trimmed));
    }

    if non_empty_lines.is_empty() {
        return Err(CgmParseError::EmptyInput);
    }

    let delim = detect_delimiter(&non_empty_lines);
    let mut header_pos: Option<(usize, Vec<String>)> = None;

    for (pos, &(line_no, line_text)) in non_empty_lines.iter().enumerate() {
        let cols =
            split_csv_row_delim(line_text, delim).map_err(|detail| CgmParseError::InvalidRow {
                line: line_no,
                detail: detail.into(),
            })?;
        if is_commercial_header_row(&cols) {
            header_pos = Some((pos, cols));
            break;
        }
        preamble_lower.push_str(&line_text.to_ascii_lowercase());
        preamble_lower.push('\n');
    }

    if let Some((h_idx, header_cols)) = header_pos {
        let data_rows = &non_empty_lines[h_idx + 1..];
        return parse_with_header(
            &preamble_lower,
            &header_cols,
            data_rows,
            delim,
            monitor_hint,
        );
    }

    // Headerless fallback (`timestamp,glucose[,lingo_count[,note]]`).
    parse_headerless(&non_empty_lines, delim, monitor_hint)
}

#[derive(Default)]
struct ColumnMapping {
    ts_col: Option<usize>,
    date_col: Option<usize>,
    time_col: Option<usize>,
    record_type_col: Option<usize>,
    event_type_col: Option<usize>,
    tidepool_type_col: Option<usize>,
    historic_gl_col: Option<(usize, GlucoseUnit)>,
    scan_gl_col: Option<(usize, GlucoseUnit)>,
    strip_gl_col: Option<(usize, GlucoseUnit)>,
    sensor_gl_col: Option<(usize, GlucoseUnit)>,
    bg_gl_col: Option<(usize, GlucoseUnit)>,
    primary_gl_col: Option<(usize, Option<GlucoseUnit>)>,
    unit_col: Option<usize>,
    rate_col: Option<(usize, GlucoseUnit)>,
    lingo_col: Option<usize>,
    note_col: Option<usize>,
    control_col: Option<usize>,
    device_col: Option<usize>,
}

fn parse_with_header(
    preamble_lower: &str,
    header_cols: &[String],
    data_rows: &[(usize, &str)],
    delim: char,
    monitor_hint: Option<GlucoseMonitor>,
) -> Result<ParsedGlucoseImport, CgmParseError> {
    let map = build_column_mapping(header_cols)?;
    let header_joined = header_cols
        .iter()
        .map(|s| s.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join(" | ");

    let mut device_sample_lower = String::new();
    let mut saw_stelo_sentinel = false;
    for &(_, raw_row) in data_rows.iter().take(12) {
        let lower = raw_row.to_ascii_lowercase();
        if lower.contains("below 70") || lower.contains("above 250") || lower.contains("stelo") {
            saw_stelo_sentinel = true;
        }
        if let Ok(cols) = split_csv_row_delim(raw_row, delim)
            && let Some(dev_idx) = map.device_col
            && let Some(dev) = cols.get(dev_idx)
            && !dev.trim().is_empty()
        {
            device_sample_lower.push_str(&dev.to_ascii_lowercase());
            device_sample_lower.push(' ');
        }
    }

    let detected_monitor = monitor_hint.unwrap_or_else(|| {
        detect_monitor_from_signatures(
            preamble_lower,
            &header_joined,
            &device_sample_lower,
            &map,
            saw_stelo_sentinel,
        )
    });

    let mut readings = Vec::new();
    let mut primary_unit: Option<GlucoseUnit> = None;

    for &(line_no, raw_row) in data_rows {
        if is_carelink_divider(raw_row) {
            continue;
        }
        let cols =
            split_csv_row_delim(raw_row, delim).map_err(|detail| CgmParseError::InvalidRow {
                line: line_no,
                detail: detail.into(),
            })?;
        if is_commercial_header_row(&cols) {
            continue;
        }

        if let Some(ctrl_idx) = map.control_col
            && let Some(ctrl_val) = cols.get(ctrl_idx).map(|s| s.trim().to_ascii_lowercase())
            && matches!(ctrl_val.as_str(), "true" | "yes" | "1" | "control" | "ctl")
        {
            continue;
        }

        if let Some(ev_idx) = map.event_type_col
            && let Some(ev) = cols.get(ev_idx).map(|s| s.trim())
            && !ev.is_empty()
        {
            let ev_low = ev.to_ascii_lowercase();
            // Dexcom Clarity / Tandem non-glucose metadata or insulin/carb rows.
            if !ev_low.contains("egv")
                && !ev_low.contains("cgm")
                && !ev_low.contains("glucose")
                && !ev_low.contains("bg")
                && !ev_low.contains("smbg")
            {
                continue;
            }
        }

        let mut row_method = detected_monitor.default_method();
        if let Some(tp_idx) = map.tidepool_type_col
            && let Some(tp) = cols.get(tp_idx).map(|s| s.trim().to_ascii_lowercase())
            && !tp.is_empty()
        {
            match tp.as_str() {
                "cbg" | "cgm" => row_method = MeasurementMethod::InterstitialCgm,
                "smbg" | "bgm" | "bg" => row_method = MeasurementMethod::CapillaryBgm,
                _ => continue,
            }
        }

        let timestamp = extract_row_timestamp(&cols, &map);
        if timestamp.is_empty() {
            // Dexcom Clarity patient/device header rows have empty Timestamp.
            continue;
        }

        let row_unit_override = map
            .unit_col
            .and_then(|i| cols.get(i))
            .and_then(|u| parse_unit_token(u));

        let selected_cell: Option<(&str, GlucoseUnit, MeasurementMethod)> =
            if let Some(rt_idx) = map.record_type_col {
                let rt = cols.get(rt_idx).map(|s| s.trim()).unwrap_or("");
                match rt {
                    "0" => map.historic_gl_col.or(map.scan_gl_col).and_then(|(i, u)| {
                        cols.get(i)
                            .map(|s| (s.as_str(), u, MeasurementMethod::InterstitialCgm))
                    }),
                    "1" => map.scan_gl_col.or(map.historic_gl_col).and_then(|(i, u)| {
                        cols.get(i)
                            .map(|s| (s.as_str(), u, MeasurementMethod::InterstitialCgm))
                    }),
                    "2" => map.strip_gl_col.or(map.bg_gl_col).and_then(|(i, u)| {
                        cols.get(i)
                            .map(|s| (s.as_str(), u, MeasurementMethod::CapillaryBgm))
                    }),
                    _ => continue,
                }
            } else if map.sensor_gl_col.is_some() || map.bg_gl_col.is_some() {
                let sensor_cell = map
                    .sensor_gl_col
                    .and_then(|(i, u)| cols.get(i).map(|s| (s.trim(), u)))
                    .filter(|(s, _)| !s.is_empty());
                if let Some((s, u)) = sensor_cell {
                    Some((
                        s,
                        row_unit_override.unwrap_or(u),
                        MeasurementMethod::InterstitialCgm,
                    ))
                } else if let Some((s, u)) = map
                    .bg_gl_col
                    .and_then(|(i, u)| cols.get(i).map(|s| (s.trim(), u)))
                    .filter(|(s, _)| !s.is_empty())
                {
                    Some((
                        s,
                        row_unit_override.unwrap_or(u),
                        MeasurementMethod::CapillaryBgm,
                    ))
                } else {
                    continue;
                }
            } else if let Some((i, col_unit)) = map.primary_gl_col {
                let unit = row_unit_override.or(col_unit).unwrap_or(GlucoseUnit::MgDl);
                cols.get(i).map(|s| (s.as_str(), unit, row_method))
            } else {
                None
            };

        let Some((raw_gl_str, cell_unit, method)) = selected_cell else {
            continue;
        };
        let trimmed_gl = raw_gl_str.trim();
        if trimmed_gl.is_empty() {
            if map.event_type_col.is_some()
                || map.record_type_col.is_some()
                || map.tidepool_type_col.is_some()
                || detected_monitor == GlucoseMonitor::TandemSource
            {
                continue;
            }
            return Err(CgmParseError::InvalidRow {
                line: line_no,
                detail: "missing glucose value".into(),
            });
        }

        let (raw_value, glucose_mg_dl, bound) =
            parse_glucose_value_or_sentinel(trimmed_gl, cell_unit, detected_monitor, line_no)?;
        if primary_unit.is_none() {
            primary_unit = Some(cell_unit);
        }

        let trend_mg_dl_per_min = match map
            .rate_col
            .and_then(|(i, u)| cols.get(i).map(|s| (s.trim(), u)))
        {
            Some((s, rate_unit)) if !s.is_empty() => s
                .parse::<f64>()
                .ok()
                .filter(|v| v.is_finite())
                .map(|v| round1(rate_unit.to_mg_dl(v))),
            _ => None,
        };

        let lingo_count = match map.lingo_col.and_then(|i| cols.get(i)).map(|s| s.trim()) {
            Some(s) if !s.is_empty() => {
                Some(
                    s.parse::<f64>()
                        .map_err(|source| CgmParseError::InvalidFloat {
                            line: line_no,
                            field: "lingo_count",
                            raw: s.to_owned(),
                            source,
                        })?,
                )
            }
            _ => None,
        };

        let note = map
            .note_col
            .and_then(|i| cols.get(i))
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(str::to_owned);

        let reading = GlucoseReading {
            timestamp,
            glucose_mg_dl: round1(glucose_mg_dl),
            raw_value,
            source_unit: cell_unit,
            method,
            bound,
            trend_mg_dl_per_min,
            lingo_count,
            note,
        };
        reading.validate(line_no)?;
        readings.push(reading);
    }

    if readings.is_empty() {
        return Err(CgmParseError::EmptyInput);
    }

    let source_unit = primary_unit.unwrap_or(GlucoseUnit::MgDl);
    let bgm_count = readings
        .iter()
        .filter(|r| r.method == MeasurementMethod::CapillaryBgm)
        .count();
    let method = if bgm_count * 2 > readings.len() {
        MeasurementMethod::CapillaryBgm
    } else {
        detected_monitor.default_method()
    };

    Ok(ParsedGlucoseImport {
        monitor: detected_monitor,
        source_unit,
        method,
        readings,
    })
}

fn parse_headerless(
    rows: &[(usize, &str)],
    delim: char,
    monitor_hint: Option<GlucoseMonitor>,
) -> Result<ParsedGlucoseImport, CgmParseError> {
    let monitor = monitor_hint.unwrap_or(GlucoseMonitor::GenericCsv);
    let method = monitor.default_method();
    let mut readings = Vec::with_capacity(rows.len());

    for &(line_no, raw_line) in rows {
        let cols =
            split_csv_row_delim(raw_line, delim).map_err(|detail| CgmParseError::InvalidRow {
                line: line_no,
                detail: detail.into(),
            })?;
        if cols.len() < 2 {
            return Err(CgmParseError::UnrecognizedFormat {
                detail: format!("line {line_no} has fewer than 2 CSV columns"),
            });
        }
        let timestamp = cols[0].trim().to_owned();
        let (raw_value, glucose_mg_dl, bound) =
            parse_glucose_value_or_sentinel(cols[1].trim(), GlucoseUnit::MgDl, monitor, line_no)?;
        let lingo_count = match cols.get(2).map(|s| s.trim()) {
            Some(s) if !s.is_empty() && monitor == GlucoseMonitor::AbbottLingo => Some(
                s.parse::<f64>()
                    .map_err(|source| CgmParseError::InvalidFloat {
                        line: line_no,
                        field: "lingo_count",
                        raw: s.to_owned(),
                        source,
                    })?,
            ),
            _ => None,
        };
        let note_idx = if monitor == GlucoseMonitor::AbbottLingo {
            3
        } else {
            2
        };
        let note = cols
            .get(note_idx)
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
        let reading = GlucoseReading {
            timestamp,
            glucose_mg_dl: round1(glucose_mg_dl),
            raw_value,
            source_unit: GlucoseUnit::MgDl,
            method,
            bound,
            trend_mg_dl_per_min: None,
            lingo_count,
            note,
        };
        reading.validate(line_no)?;
        readings.push(reading);
    }

    if readings.is_empty() {
        return Err(CgmParseError::EmptyInput);
    }
    Ok(ParsedGlucoseImport {
        monitor,
        source_unit: GlucoseUnit::MgDl,
        method,
        readings,
    })
}

fn parse_glucose_value_or_sentinel(
    raw: &str,
    unit: GlucoseUnit,
    monitor: GlucoseMonitor,
    line_no: usize,
) -> Result<(f64, f64, GlucoseReadingBound), CgmParseError> {
    let lower = raw.trim().to_ascii_lowercase();
    if matches!(lower.as_str(), "low" | "lo" | "below 40" | "<40" | "< 40") {
        let floor_mg = if monitor == GlucoseMonitor::FingerstickBgm {
            20.0
        } else {
            40.0
        };
        let raw_in_unit = match unit {
            GlucoseUnit::MgDl => floor_mg,
            GlucoseUnit::MmolL => round1(floor_mg / MMOL_L_TO_MG_DL),
        };
        return Ok((raw_in_unit, floor_mg, GlucoseReadingBound::BelowSensorFloor));
    }
    if matches!(lower.as_str(), "below 70" | "<70" | "< 70") {
        let raw_in_unit = match unit {
            GlucoseUnit::MgDl => 70.0,
            GlucoseUnit::MmolL => round1(70.0 / MMOL_L_TO_MG_DL),
        };
        return Ok((raw_in_unit, 70.0, GlucoseReadingBound::BelowSensorFloor));
    }
    if matches!(lower.as_str(), "above 250" | ">250" | "> 250") {
        let raw_in_unit = match unit {
            GlucoseUnit::MgDl => 250.0,
            GlucoseUnit::MmolL => round1(250.0 / MMOL_L_TO_MG_DL),
        };
        return Ok((raw_in_unit, 250.0, GlucoseReadingBound::AboveSensorCeiling));
    }
    if matches!(
        lower.as_str(),
        "high" | "hi" | "above 400" | ">400" | "> 400" | "above 500" | ">500"
    ) {
        let ceil_mg = match monitor {
            GlucoseMonitor::AbbottFreeStyleLibre | GlucoseMonitor::AbbottLingo => 500.0,
            GlucoseMonitor::FingerstickBgm => 600.0,
            GlucoseMonitor::DexcomStelo => 250.0,
            _ => 400.0,
        };
        let raw_in_unit = match unit {
            GlucoseUnit::MgDl => ceil_mg,
            GlucoseUnit::MmolL => round1(ceil_mg / MMOL_L_TO_MG_DL),
        };
        return Ok((
            raw_in_unit,
            ceil_mg,
            GlucoseReadingBound::AboveSensorCeiling,
        ));
    }

    let parsed = raw
        .parse::<f64>()
        .map_err(|source| CgmParseError::InvalidFloat {
            line: line_no,
            field: "glucose",
            raw: raw.to_owned(),
            source,
        })?;
    if !parsed.is_finite() {
        return Err(CgmParseError::OutOfRange {
            line: line_no,
            field: "glucose",
            detail: format!("non-finite glucose value `{raw}`"),
        });
    }
    Ok((parsed, unit.to_mg_dl(parsed), GlucoseReadingBound::Measured))
}

fn build_column_mapping(header_cols: &[String]) -> Result<ColumnMapping, CgmParseError> {
    let mut map = ColumnMapping::default();

    for (i, raw) in header_cols.iter().enumerate() {
        let h = raw.trim().to_ascii_lowercase();
        let unit = unit_from_header(&h);

        if h == "date" && map.date_col.is_none() {
            map.date_col = Some(i);
            continue;
        }
        if h == "time" && map.date_col.is_some() && map.time_col.is_none() {
            map.time_col = Some(i);
            continue;
        }
        if map.ts_col.is_none()
            && !h.contains("transmitter time")
            && !h.contains("duration")
            && (h == "time"
                || h.contains("timestamp")
                || h.contains("datetime")
                || h.contains("date_time")
                || h.contains("recorded")
                || h == "reading time")
        {
            map.ts_col = Some(i);
            continue;
        }

        if h == "record type" {
            map.record_type_col = Some(i);
            continue;
        }
        if h == "event type" || h == "eventtype" {
            map.event_type_col = Some(i);
            continue;
        }
        if h == "type" && map.tidepool_type_col.is_none() {
            map.tidepool_type_col = Some(i);
            continue;
        }
        if (h == "unit" || h == "units") && map.unit_col.is_none() {
            map.unit_col = Some(i);
            continue;
        }
        if h.contains("rate of change") || h == "trend_mg_dl_per_min" {
            map.rate_col = Some((i, unit.unwrap_or(GlucoseUnit::MgDl)));
            continue;
        }
        if h.contains("control") && h.contains("solution") {
            map.control_col = Some(i);
            continue;
        }
        if (h == "device" || h == "device info" || h == "deviceid" || h == "devicetype")
            && map.device_col.is_none()
        {
            map.device_col = Some(i);
            continue;
        }
        if h.contains("lingo") && map.lingo_col.is_none() {
            map.lingo_col = Some(i);
            continue;
        }
        if (h == "note"
            || h == "notes"
            || h.contains("context")
            || h == "comment"
            || h == "comments"
            || h == "meal marker"
            || h == "event marker")
            && map.note_col.is_none()
        {
            map.note_col = Some(i);
            continue;
        }

        // Specific multi-column glucose layouts first (LibreView, CareLink, Glooko).
        if h.starts_with("historic glucose") {
            map.historic_gl_col = Some((i, unit.unwrap_or(GlucoseUnit::MgDl)));
        } else if h.starts_with("scan glucose") {
            map.scan_gl_col = Some((i, unit.unwrap_or(GlucoseUnit::MgDl)));
        } else if h.starts_with("strip glucose") {
            map.strip_gl_col = Some((i, unit.unwrap_or(GlucoseUnit::MgDl)));
        } else if h.starts_with("sensor glucose") || h.starts_with("cgm glucose") {
            map.sensor_gl_col = Some((i, unit.unwrap_or(GlucoseUnit::MgDl)));
        } else if h.starts_with("bg reading")
            || h.starts_with("blood glucose")
            || h.starts_with("bg value")
            || h == "bg"
        {
            if map.primary_gl_col.is_none() {
                map.primary_gl_col = Some((i, unit));
            }
            map.bg_gl_col = Some((i, unit.unwrap_or(GlucoseUnit::MgDl)));
        } else if map.primary_gl_col.is_none()
            && !h.contains("non-numeric")
            && !h.contains("insulin")
            && !h.contains("ketone")
            && !h.contains("carb")
            && (h.contains("glucose")
                || h.starts_with("readings (cgm)")
                || h == "cgmglucosevalue"
                || h == "mg_dl"
                || h == "mmol_l"
                || h == "value"
                || h.starts_with("result ("))
        {
            map.primary_gl_col = Some((i, unit));
        }
    }

    let has_ts = map.ts_col.is_some() || map.date_col.is_some();
    let has_gl = map.historic_gl_col.is_some()
        || map.scan_gl_col.is_some()
        || map.strip_gl_col.is_some()
        || map.sensor_gl_col.is_some()
        || map.bg_gl_col.is_some()
        || map.primary_gl_col.is_some();

    if !has_ts || !has_gl {
        return Err(CgmParseError::UnrecognizedFormat {
            detail: "CSV header must contain timestamp/date and glucose columns".into(),
        });
    }
    Ok(map)
}

fn detect_monitor_from_signatures(
    preamble_lower: &str,
    header_joined: &str,
    device_sample_lower: &str,
    map: &ColumnMapping,
    saw_stelo_sentinel: bool,
) -> GlucoseMonitor {
    let combined = format!("{preamble_lower} {header_joined} {device_sample_lower}");

    if map.lingo_col.is_some() || combined.contains("lingo") {
        return GlucoseMonitor::AbbottLingo;
    }
    if (map.record_type_col.is_some()
        && (map.historic_gl_col.is_some() || map.scan_gl_col.is_some()))
        || combined.contains("freestyle")
        || combined.contains("libreview")
        || combined.contains("libre ")
    {
        return GlucoseMonitor::AbbottFreeStyleLibre;
    }
    if saw_stelo_sentinel || combined.contains("stelo") {
        return GlucoseMonitor::DexcomStelo;
    }
    if combined.contains("eversense") || combined.contains("senseonics") {
        return GlucoseMonitor::SenseonicsEversense;
    }
    if combined.contains("tandem")
        || combined.contains("t:slim")
        || combined.contains("t:connect")
        || header_joined.contains("readings (cgm)")
        || header_joined.contains("cgmglucosevalue")
        || header_joined.contains("eventdatetime")
    {
        return GlucoseMonitor::TandemSource;
    }
    if combined.contains("carelink")
        || combined.contains("medtronic")
        || combined.contains("minimed")
        || combined.contains("guardian")
        || combined.contains("simplera")
        || header_joined.contains("isig value")
        || (map.sensor_gl_col.is_some() && map.bg_gl_col.is_some() && map.date_col.is_some())
    {
        return GlucoseMonitor::MedtronicCareLink;
    }
    if combined.contains("glooko")
        || combined.contains("tidepool")
        || combined.contains("omnipod")
        || map.tidepool_type_col.is_some()
        || header_joined.contains("cgm glucose value")
    {
        return GlucoseMonitor::GlookoTidepool;
    }
    if combined.contains("dexcom")
        || combined.contains("clarity")
        || (map.event_type_col.is_some()
            && (header_joined.contains("source device id")
                || header_joined.contains("transmitter")
                || map.rate_col.is_some()))
    {
        return GlucoseMonitor::DexcomClarity;
    }
    if combined.contains("accu-chek")
        || combined.contains("onetouch")
        || combined.contains("contour")
        || combined.contains("true metrix")
        || combined.contains("fingerstick")
        || combined.contains("meal marker")
        || combined.contains("control solution")
        || (map.bg_gl_col.is_some() && map.sensor_gl_col.is_none())
    {
        return GlucoseMonitor::FingerstickBgm;
    }
    GlucoseMonitor::GenericCsv
}

fn extract_row_timestamp(cols: &[String], map: &ColumnMapping) -> String {
    if let Some(ts_idx) = map.ts_col
        && let Some(ts) = cols.get(ts_idx).map(|s| s.trim())
        && !ts.is_empty()
    {
        return ts.to_owned();
    }
    let date_part = map
        .date_col
        .and_then(|i| cols.get(i))
        .map(|s| s.trim())
        .unwrap_or("");
    let time_part = map
        .time_col
        .and_then(|i| cols.get(i))
        .map(|s| s.trim())
        .unwrap_or("");
    match (date_part.is_empty(), time_part.is_empty()) {
        (true, true) => String::new(),
        (false, true) => date_part.to_owned(),
        (true, false) => time_part.to_owned(),
        (false, false) => format!("{date_part} {time_part}"),
    }
}

fn unit_from_header(header_lower: &str) -> Option<GlucoseUnit> {
    if header_lower.contains("mmol") {
        Some(GlucoseUnit::MmolL)
    } else if header_lower.contains("mg/dl") || header_lower.contains("mg_dl") {
        Some(GlucoseUnit::MgDl)
    } else {
        None
    }
}

fn parse_unit_token(raw: &str) -> Option<GlucoseUnit> {
    let lower = raw.trim().to_ascii_lowercase();
    if lower.contains("mmol") {
        Some(GlucoseUnit::MmolL)
    } else if lower.contains("mg") {
        Some(GlucoseUnit::MgDl)
    } else {
        None
    }
}

fn is_carelink_divider(line: &str) -> bool {
    line.starts_with("-------") || line.starts_with("=======")
}

fn detect_delimiter(lines: &[(usize, &str)]) -> char {
    for &(_, line) in lines.iter().take(12) {
        let commas = line.matches(',').count();
        let semis = line.matches(';').count();
        if semis >= 3 && commas == 0 {
            return ';';
        }
    }
    ','
}

fn is_commercial_header_row(cols: &[String]) -> bool {
    if cols.len() < 2 {
        return false;
    }
    let has_ts = cols.iter().any(|c| {
        let h = c.trim().to_ascii_lowercase();
        !h.chars().any(|ch| ch.is_ascii_digit())
            && !h.contains("generated on")
            && !h.contains("date range")
            && (h == "date"
                || h == "time"
                || h.contains("timestamp")
                || h.contains("datetime")
                || h.contains("date_time")
                || h.contains("recorded")
                || h == "reading time")
    });
    let has_gl = cols.iter().any(|c| {
        let h = c.trim().to_ascii_lowercase();
        !h.contains("glucose data")
            && (h.contains("glucose")
                || h.starts_with("bg ")
                || h == "bg"
                || h.starts_with("readings (cgm)")
                || h == "cgmglucosevalue"
                || h == "mg_dl"
                || h == "mmol_l"
                || h == "value"
                || h.starts_with("result ("))
    });
    has_ts && has_gl
}

/// Quote-aware RFC-4180 CSV splitter shared across `cgm.rs` and `lingo.rs`.
pub(crate) fn split_csv_row_delim(line: &str, delim: char) -> Result<Vec<String>, &'static str> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut chars = line.chars().peekable();
    let mut in_quotes = false;
    let mut field_was_quoted = false;
    let mut after_closing_quote = false;

    while let Some(ch) = chars.next() {
        if in_quotes {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    let _ = chars.next();
                    current.push('"');
                } else {
                    in_quotes = false;
                    after_closing_quote = true;
                }
            } else {
                current.push(ch);
            }
        } else if ch == delim {
            fields.push(current.trim().to_owned());
            current.clear();
            field_was_quoted = false;
            after_closing_quote = false;
        } else if after_closing_quote {
            if !ch.is_whitespace() {
                return Err("unexpected characters after closing quote in CSV field");
            }
        } else if ch == '"' && current.trim().is_empty() && !field_was_quoted {
            current.clear();
            in_quotes = true;
            field_was_quoted = true;
        } else {
            current.push(ch);
        }
    }

    if in_quotes {
        return Err("unterminated quoted CSV field");
    }
    fields.push(current.trim().to_owned());
    Ok(fields)
}

pub(crate) fn round1(val: f64) -> f64 {
    (val * 10.0).round() / 10.0
}

fn percentile_sorted(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    if sorted.len() == 1 {
        return sorted[0];
    }
    let idx = p.clamp(0.0, 1.0) * ((sorted.len() - 1) as f64);
    let lo = idx.floor() as usize;
    let hi = idx.ceil() as usize;
    let frac = idx - (lo as f64);
    sorted[lo] + frac * (sorted[hi] - sorted[lo])
}

/// Computes Mean Amplitude of Glycemic Excursions (MAGE) over turning points whose
/// peak-to-trough amplitude exceeds `1 SD`, pruning sub-SD intermediate ripples first.
fn compute_mage_mg_dl(readings: &[GlucoseReading], std_dev: f64) -> f64 {
    if readings.len() < 2 || std_dev <= 0.0 {
        return 0.0;
    }

    let mut dedup: Vec<f64> = Vec::with_capacity(readings.len());
    for r in readings {
        if dedup
            .last()
            .is_none_or(|prev| (prev - r.glucose_mg_dl).abs() > f64::EPSILON)
        {
            dedup.push(r.glucose_mg_dl);
        }
    }
    if dedup.len() < 2 {
        return 0.0;
    }

    let mut extrema: Vec<f64> = Vec::with_capacity(dedup.len());
    extrema.push(dedup[0]);
    for i in 1..dedup.len() - 1 {
        let prev = dedup[i - 1];
        let curr = dedup[i];
        let next = dedup[i + 1];
        let is_peak = curr > prev && curr > next;
        let is_trough = curr < prev && curr < next;
        if is_peak || is_trough {
            extrema.push(curr);
        }
    }
    extrema.push(dedup[dedup.len() - 1]);

    // Prune interior ripple pairs whose amplitude is <= 1 SD while interior pairs remain.
    loop {
        if extrema.len() < 4 {
            break;
        }
        let mut smallest_idx = None;
        let mut smallest_amp = f64::INFINITY;
        for i in 1..extrema.len() - 2 {
            let amp = (extrema[i + 1] - extrema[i]).abs();
            if amp <= std_dev && amp < smallest_amp {
                smallest_amp = amp;
                smallest_idx = Some(i);
            }
        }
        if let Some(i) = smallest_idx {
            extrema.drain(i..=i + 1);
        } else {
            break;
        }
    }

    let significant: Vec<f64> = extrema
        .windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .filter(|&amp| amp > std_dev)
        .collect();
    if significant.is_empty() {
        0.0
    } else {
        significant.iter().sum::<f64>() / (significant.len() as f64)
    }
}

fn estimate_active_wear_percent(
    monitor: GlucoseMonitor,
    method: MeasurementMethod,
    count: usize,
    epoch_secs: &mut [i64],
) -> f64 {
    if count == 0 {
        return 0.0;
    }
    if epoch_secs.len() < 2 {
        return 100.0;
    }
    epoch_secs.sort_unstable();
    let span_secs = epoch_secs[epoch_secs.len() - 1] - epoch_secs[0];
    if span_secs <= 0 {
        return 100.0;
    }

    if method == MeasurementMethod::CapillaryBgm {
        let mut days: Vec<i64> = epoch_secs.iter().map(|s| s.div_euclid(86_400)).collect();
        days.dedup();
        let total_days = ((span_secs as f64) / 86_400.0).floor() as usize + 1;
        return ((days.len() as f64 / total_days.max(1) as f64) * 100.0).clamp(0.0, 100.0);
    }

    let mut deltas: Vec<i64> = epoch_secs
        .windows(2)
        .map(|w| w[1] - w[0])
        .filter(|&d| d > 0)
        .collect();
    deltas.sort_unstable();
    let cadence_secs = if let Some(&med) = deltas.get(deltas.len() / 2) {
        if (45..=90).contains(&med) {
            60.0
        } else if (240..=420).contains(&med) {
            300.0
        } else if (720..=1200).contains(&med) {
            900.0
        } else {
            monitor.nominal_cadence_secs()
        }
    } else {
        monitor.nominal_cadence_secs()
    };

    let expected_samples = (span_secs as f64 / cadence_secs) + 1.0;
    ((count as f64 / expected_samples.max(1.0)) * 100.0).clamp(0.0, 100.0)
}

enum AdaBand {
    VeryLowLt54,
    Low54To69,
    InRange70To180 { tight_70_to_140: bool },
    High181To250,
    VeryHighGt250,
}

fn classify_ada_band(g: f64, bound: GlucoseReadingBound) -> AdaBand {
    match bound {
        GlucoseReadingBound::BelowSensorFloor if g < ADA_VERY_LOW_MG_DL => AdaBand::VeryLowLt54,
        GlucoseReadingBound::BelowSensorFloor if g <= ADA_LOW_MG_DL => AdaBand::Low54To69,
        GlucoseReadingBound::AboveSensorCeiling if g >= ADA_VERY_HIGH_MG_DL => {
            AdaBand::VeryHighGt250
        }
        GlucoseReadingBound::AboveSensorCeiling if g >= ADA_CLINICAL_HIGH_MG_DL => {
            AdaBand::High181To250
        }
        GlucoseReadingBound::Measured
        | GlucoseReadingBound::BelowSensorFloor
        | GlucoseReadingBound::AboveSensorCeiling => {
            if g < ADA_VERY_LOW_MG_DL {
                AdaBand::VeryLowLt54
            } else if g < ADA_LOW_MG_DL {
                AdaBand::Low54To69
            } else if g <= ADA_CLINICAL_HIGH_MG_DL {
                AdaBand::InRange70To180 {
                    tight_70_to_140: g <= ADA_TIGHT_HIGH_MG_DL,
                }
            } else if g <= ADA_VERY_HIGH_MG_DL {
                AdaBand::High181To250
            } else {
                AdaBand::VeryHighGt250
            }
        }
    }
}

pub(crate) struct ParsedTimestamp {
    pub(crate) epoch_secs: i64,
    pub(crate) hour: u8,
    pub(crate) iso_date: String,
}

/// Pure-Rust timestamp parser extracting `(epoch_seconds, local_hour_0_to_23, YYYY-MM-DD)` across
/// ISO-8601, `YYYY-MM-DD HH:MM`, US/EU slash dates, named months, and 12h AM/PM strings.
pub(crate) fn parse_timestamp_parts(raw: &str) -> Option<ParsedTimestamp> {
    let s = raw.trim();
    if s.is_empty() {
        return None;
    }
    let lower = s.to_ascii_lowercase();
    let is_pm = lower.ends_with("pm") || lower.contains(" pm");
    let is_am = lower.ends_with("am") || lower.contains(" am");

    // Extract time component `HH:MM[:SS]` anywhere after date separator (`T` or whitespace).
    let (date_part, time_part) = if let Some(t_pos) = s.find('T') {
        (&s[..t_pos], &s[t_pos + 1..])
    } else if let Some(sp_pos) = s.find(' ') {
        // Handle `"Aug 26, 2026 12:45"` where date itself contains spaces.
        if let Some(colon_pos) = s.find(':') {
            let before_colon = &s[..colon_pos];
            let time_start = before_colon.rfind(' ').map_or(sp_pos, |idx| idx + 1);
            (s[..time_start].trim(), s[time_start..].trim())
        } else {
            (&s[..sp_pos], &s[sp_pos + 1..])
        }
    } else {
        (s, "00:00:00")
    };

    let (year, month, day) = parse_date_triple(date_part)?;
    let (mut hour, minute, second) = parse_hms(time_part)?;
    if is_pm && hour < 12 {
        hour += 12;
    } else if is_am && hour == 12 {
        hour = 0;
    }
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }

    let days = days_from_civil(year, month, day);
    let secs = days * 86_400 + i64::from(hour) * 3_600 + i64::from(minute) * 60 + i64::from(second);
    Some(ParsedTimestamp {
        epoch_secs: secs,
        hour,
        iso_date: format!("{year:04}-{month:02}-{day:02}"),
    })
}

fn parse_date_triple(raw: &str) -> Option<(i32, u32, u32)> {
    let clean = raw.trim().trim_matches(',');
    if clean.chars().any(|c| c.is_ascii_alphabetic()) {
        let tokens: Vec<&str> = clean
            .split(|c: char| c.is_whitespace() || c == ',')
            .filter(|t| !t.is_empty())
            .collect();
        if tokens.len() >= 3 {
            if let Some(m) = month_from_name(tokens[0]) {
                let d = tokens[1].parse::<u32>().ok()?;
                let y = tokens[2].parse::<i32>().ok()?;
                return valid_ymd(y, m, d);
            }
            if let Some(m) = month_from_name(tokens[1]) {
                let d = tokens[0].parse::<u32>().ok()?;
                let y = tokens[2].parse::<i32>().ok()?;
                return valid_ymd(y, m, d);
            }
        }
        return None;
    }

    let parts: Vec<&str> = clean.split(['-', '/', '.']).collect();
    if parts.len() != 3 {
        return None;
    }
    if parts[0].len() == 4 {
        let y = parts[0].parse::<i32>().ok()?;
        let m = parts[1].parse::<u32>().ok()?;
        let d = parts[2].parse::<u32>().ok()?;
        return valid_ymd(y, m, d);
    }
    if parts[2].len() == 4 {
        let y = parts[2].parse::<i32>().ok()?;
        let first = parts[0].parse::<u32>().ok()?;
        let second = parts[1].parse::<u32>().ok()?;
        if first > 12 {
            return valid_ymd(y, second, first);
        }
        return valid_ymd(y, first, second);
    }
    None
}

fn valid_ymd(year: i32, month: u32, day: u32) -> Option<(i32, u32, u32)> {
    if !(1970..=2100).contains(&year) || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    Some((year, month, day))
}

fn month_from_name(token: &str) -> Option<u32> {
    match token.trim().to_ascii_lowercase().get(..3)? {
        "jan" => Some(1),
        "feb" => Some(2),
        "mar" => Some(3),
        "apr" => Some(4),
        "may" => Some(5),
        "jun" => Some(6),
        "jul" => Some(7),
        "aug" => Some(8),
        "sep" => Some(9),
        "oct" => Some(10),
        "nov" => Some(11),
        "dec" => Some(12),
        _ => None,
    }
}

fn parse_hms(raw: &str) -> Option<(u8, u8, u8)> {
    let prefix: String = raw
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == ':')
        .collect();
    let mut iter = prefix.split(':');
    let h = iter.next()?.parse::<u8>().ok()?;
    let m = iter.next().unwrap_or("0").parse::<u8>().ok()?;
    let sec = iter.next().unwrap_or("0").parse::<u8>().ok()?;
    Some((h, m, sec))
}

/// Howard Hinnant civil-to-days algorithm (days relative to `1970-01-01`).
fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let y = i64::from(year) - if month <= 2 { 1 } else { 0 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let m = u64::from(month);
    let d = u64::from(day);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + (doe as i64) - 719_468
}

/// Computes [`CgmClinicalSummary`] (5-band TIR + TITR, GMI, GRI + Zone, MAGE, active wear %,
/// and 24-hour AGP) from a [`ParsedGlucoseImport`] and constructs a structured
/// [`DemoSourceRecord`] with [`CitedFieldWire`] measurements.
pub(crate) fn build_commercial_cgm_record(
    workspace_id: &str,
    record_id: String,
    summary_title: &str,
    import: &ParsedGlucoseImport,
) -> Result<(DemoSourceRecord, CgmClinicalSummary), CgmParseError> {
    let summary = CgmClinicalSummary::from_import(import)?;

    let monitor_label = import.monitor.display_name();
    let title = match summary_title.trim() {
        "" => format!("{monitor_label} — AGP & Glycemic Summary"),
        clean => clean.to_owned(),
    };
    let recorded_at = parse_timestamp_parts(&summary.end_timestamp)
        .map(|ts| ts.iso_date)
        .unwrap_or_else(|| DEMO_DATE.to_owned());

    let fields = summary.to_cited_fields();
    let tbr_total = round1(summary.tbr_very_low_lt_54_percent + summary.tbr_low_54_69_percent);
    let tar_total = round1(summary.tar_high_181_250_percent + summary.tar_very_high_gt_250_percent);

    let lingo_clause = summary
        .total_lingo_count
        .map(|lc| format!(" Total Lingo Count: {lc:.1} pts."))
        .unwrap_or_default();
    let summary_prose = format!(
        "{monitor_label} summary ({} to {}, n = {} readings, source unit {}, active wear {:.1}%): mean glucose {:.1} mg/dL, median {:.1} mg/dL, range {:.1}–{:.1} mg/dL, SD {:.1} mg/dL, CV {:.1}%, GMI {:.1}%, GRI {:.1}/100 ({}), MAGE {:.1} mg/dL. Time in Ranges: TITR (70–140 mg/dL) {:.1}%, TIR (70–180 mg/dL) {:.1}%, TBR <70 mg/dL {:.1}% (<54 mg/dL: {:.1}%), TAR >180 mg/dL {:.1}% (>250 mg/dL: {:.1}%).{lingo_clause}",
        summary.start_timestamp,
        summary.end_timestamp,
        summary.count,
        summary.source_unit.label(),
        summary.estimated_active_wear_percent,
        summary.mean_mg_dl,
        summary.median_mg_dl,
        summary.min_mg_dl,
        summary.max_mg_dl,
        summary.std_dev_mg_dl,
        summary.cv_percent,
        summary.gmi_percent,
        summary.gri,
        summary.gri_zone.label(),
        summary.mage_mg_dl,
        summary.titr_70_140_percent,
        summary.tir_70_180_percent,
        tbr_total,
        summary.tbr_very_low_lt_54_percent,
        tar_total,
        summary.tar_very_high_gt_250_percent,
    );

    let agp_lines: Vec<String> = summary
        .agp_hourly
        .iter()
        .map(|b| {
            format!(
                "{:02}:00 (n={}): p5 {:.1}, p25 {:.1}, median {:.1}, p75 {:.1}, p95 {:.1} mg/dL",
                b.hour, b.count, b.p5, b.p25, b.p50, b.p75, b.p95
            )
        })
        .collect();
    let agp_prose = if agp_lines.is_empty() {
        "24-Hour AGP: timestamps lacked time-of-day hours.".to_owned()
    } else {
        format!(
            "24-Hour Ambulatory Glucose Profile (AGP):\n- {}",
            agp_lines.join("\n- ")
        )
    };

    let mut body = format!("{summary_prose}\n\n{agp_prose}");
    let clipped_low = import
        .readings
        .iter()
        .filter(|r| r.bound == GlucoseReadingBound::BelowSensorFloor)
        .count();
    let clipped_high = import
        .readings
        .iter()
        .filter(|r| r.bound == GlucoseReadingBound::AboveSensorCeiling)
        .count();
    let boundary_prose = (clipped_low > 0 || clipped_high > 0).then(|| {
        format!(
            "Sensor boundary events: {clipped_low} below sensor floor, {clipped_high} above sensor ceiling."
        )
    });
    if let Some(ref bprose) = boundary_prose {
        body.push_str("\n\n");
        body.push_str(bprose);
    }

    let mut readable_parts = vec![CitedTextPartWire {
        label: Some(format!("{monitor_label} Clinical Summary")),
        text: summary_prose,
    }];
    if !agp_lines.is_empty() {
        readable_parts.push(CitedTextPartWire {
            label: Some("24-Hour Ambulatory Glucose Profile (AGP)".into()),
            text: agp_prose,
        });
    }
    if let Some(bprose) = boundary_prose {
        readable_parts.push(CitedTextPartWire {
            label: Some("Sensor Boundary Events".into()),
            text: bprose,
        });
    }

    let raw_content = json!({
        "source": import.monitor.slug(),
        "source_unit": import.source_unit,
        "method": import.method,
        "summary": &summary,
        "sample_readings": import.readings.iter().take(24).collect::<Vec<_>>(),
    })
    .to_string();

    let rec = DemoSourceRecord {
        id: record_id,
        workspace_id: workspace_id.to_owned(),
        group: "results".into(),
        title,
        recorded_at,
        body,
        source_label: monitor_label.into(),
        readable_parts,
        fields,
        record_status: "final".into(),
        raw_content: Some(raw_content),
    };
    Ok((rec, summary))
}

impl DemoStore {
    /// Computes [`CgmClinicalSummary`] (5-band TIR + TITR, GMI, GRI + Zone, MAGE, active wear %,
    /// and 24-hour AGP) from a [`ParsedGlucoseImport`] and files a structured `SourceRecords`
    /// entry with `CitedFieldWire` measurements.
    pub async fn ingest_commercial_cgm(
        &self,
        workspace_id: &str,
        summary_title: &str,
        import: &ParsedGlucoseImport,
    ) -> Result<(DemoSourceRecord, CgmClinicalSummary), DbError> {
        self.ensure_ready().await?;
        let _ = self.load_patient_name(workspace_id)?;
        let record_id = format!("rec-{workspace_id}-cgm-{}", next_seq());
        let (rec, summary) =
            build_commercial_cgm_record(workspace_id, record_id, summary_title, import)
                .map_err(|err| DbError::InvalidCommand(err.to_string()))?;
        self.insert_structured_source_record(&rec)?;
        Ok((rec, summary))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_ten_commercial_glucose_monitors_in_mg_dl_and_mmol_l() {
        // 1) Abbott Lingo
        let lingo_csv = "\
timestamp,glucose_mg_dl,lingo_count,note
2026-08-26T08:00:00Z,90,0,Fasting
2026-08-26T12:45:00Z,162,12,\"Sweetened latte, rice bowl\"
2026-08-26T14:30:00Z,74,0,Afternoon dip
";
        let lingo = parse_commercial_glucose_csv(lingo_csv, None).expect("Abbott Lingo");
        assert_eq!(lingo.monitor, GlucoseMonitor::AbbottLingo);
        assert_eq!(lingo.source_unit, GlucoseUnit::MgDl);
        assert_eq!(lingo.readings.len(), 3);
        let lingo_sum = CgmClinicalSummary::from_import(&lingo).expect("lingo summary");
        assert_eq!(lingo_sum.total_lingo_count, Some(12.0));

        // 2) Abbott FreeStyle Libre / LibreView (multi-line preamble + mmol/L + Record Type 0/1/2/5)
        let libre_mmol_csv = "\
Glucose Data,Generated on,2026-08-26 15:00 UTC,Generated by,Maya Lin
Patient Name,Maya Lin,Patient ID,PT-882
Device,Serial Number,Device Timestamp,Record Type,Historic Glucose mmol/L,Scan Glucose mmol/L,Strip Glucose mmol/L,Notes
FreeStyle Libre 3,SN-L3,2026-08-26 08:00,0,5.0,,,Fasting baseline
FreeStyle Libre 3,SN-L3,2026-08-26 08:15,0,10.5,,,Post-breakfast rise
FreeStyle Libre 3,SN-L3,2026-08-26 08:30,1,,11.0,,Scanned peak
FreeStyle Libre 3,SN-L3,2026-08-26 08:45,2,,,4.0,Fingerstick confirmation
FreeStyle Libre 3,SN-L3,2026-08-26 09:00,5,,,,Logged exercise note only
";
        let libre = parse_commercial_glucose_csv(libre_mmol_csv, None).expect("LibreView mmol/L");
        assert_eq!(libre.monitor, GlucoseMonitor::AbbottFreeStyleLibre);
        assert_eq!(libre.source_unit, GlucoseUnit::MmolL);
        assert_eq!(libre.readings.len(), 4);
        assert!((libre.readings[0].glucose_mg_dl - 90.1).abs() <= 0.1);
        assert_eq!(libre.readings[3].method, MeasurementMethod::CapillaryBgm);

        // 3) Dexcom Clarity (BOM + non-EGV header rows + "Low"/"High" sentinels + Rate of Change)
        let dexcom_csv = "\u{feff}\
Index,Timestamp (YYYY-MM-DDThh:mm:ss),Event Type,Event Subtype,Patient Info,Device Info,Source Device ID,Glucose Value (mg/dL),Glucose Rate of Change (mg/dL/min)
1,,FirstName,,Maya,,,,
2,,Device,,Dexcom G7 Mobile App,Dexcom G7,DX-77,,
3,2026-08-26T06:00:00,EGV,,,Dexcom G7,DX-77,Low,-1.8
4,2026-08-26T06:05:00,EGV,,,Dexcom G7,DX-77,64,-0.5
5,2026-08-26T12:30:00,EGV,,,Dexcom G7,DX-77,210,2.4
6,2026-08-26T13:00:00,EGV,,,Dexcom G7,DX-77,High,3.1
7,2026-08-26T13:05:00,Insulin,,,Dexcom G7,DX-77,,
";
        let dexcom = parse_commercial_glucose_csv(dexcom_csv, None).expect("Dexcom Clarity");
        assert_eq!(dexcom.monitor, GlucoseMonitor::DexcomClarity);
        assert_eq!(dexcom.readings.len(), 4);
        assert_eq!(
            dexcom.readings[0].bound,
            GlucoseReadingBound::BelowSensorFloor
        );
        assert_eq!(dexcom.readings[0].glucose_mg_dl, 40.0);
        assert_eq!(
            dexcom.readings[3].bound,
            GlucoseReadingBound::AboveSensorCeiling
        );
        assert_eq!(dexcom.readings[3].glucose_mg_dl, 400.0);
        assert_eq!(dexcom.readings[2].trend_mg_dl_per_min, Some(2.4));

        // 4) Dexcom Stelo ("Below 70" / "Above 250" OTC biosensor sentinels)
        let stelo_csv = "\
Timestamp,Device,Glucose Value (mg/dL),Notes
2026-08-26T07:00:00,Dexcom Stelo,Below 70,Early morning fast
2026-08-26T12:30:00,Dexcom Stelo,134,Balanced lunch
2026-08-26T19:45:00,Dexcom Stelo,Above 250,Dessert spike
";
        let stelo = parse_commercial_glucose_csv(stelo_csv, None).expect("Dexcom Stelo");
        assert_eq!(stelo.monitor, GlucoseMonitor::DexcomStelo);
        assert_eq!(stelo.readings[0].glucose_mg_dl, 70.0);
        assert_eq!(
            stelo.readings[0].bound,
            GlucoseReadingBound::BelowSensorFloor
        );
        assert_eq!(stelo.readings[2].glucose_mg_dl, 250.0);
        assert_eq!(
            stelo.readings[2].bound,
            GlucoseReadingBound::AboveSensorCeiling
        );
        let stelo_sum = CgmClinicalSummary::from_import(&stelo).expect("stelo summary");
        assert_eq!(stelo_sum.tbr_low_54_69_percent, 33.3);
        assert_eq!(stelo_sum.tir_70_180_percent, 33.3);
        assert_eq!(stelo_sum.titr_70_140_percent, 33.3);
        assert_eq!(stelo_sum.tar_very_high_gt_250_percent, 33.3);
        assert_eq!(stelo_sum.tar_high_181_250_percent, 0.0);
        assert_eq!(stelo_sum.low_count_under_70, 1);
        assert_eq!(stelo_sum.high_count_over_180, 1);
        assert_eq!(stelo_sum.spike_count_over_140, 1);
        assert_eq!(stelo_sum.gri, 100.0);
        assert_eq!(stelo_sum.gri_zone, GriZone::ZoneE);
        let stelo_fields = stelo_sum.to_cited_fields();
        assert!(
            stelo_fields
                .iter()
                .any(|f| f.key == "cgm_nadir_glucose" && f.status.as_deref() == Some("Low")),
            "Stelo Below 70 must mark Nadir Glucose as Low: {stelo_fields:?}"
        );
        assert!(
            stelo_fields
                .iter()
                .any(|f| f.key == "cgm_peak_glucose" && f.status.as_deref() == Some("High")),
            "Stelo Above 250 must mark Peak Glucose as High: {stelo_fields:?}"
        );

        // 5) Medtronic CareLink (metadata preamble + ------- divider + Date/Time + Sensor/BG)
        let carelink_csv = "\
Last Name,First Name,Patient ID
Lin,Maya,CL-99
-------,MiniMed 780G / Guardian 4,-------
Index,Date,Time,BG Reading (mg/dL),Sensor Glucose (mg/dL),ISIG Value,Event Marker
1,2026/08/26,08:00:00,,98,24.1,
2,2026/08/26,08:05:00,,104,25.0,
3,2026/08/26,08:10:00,106,,,Calibration BG
4,2026/08/26,08:15:00,,,,Basal row without glucose
";
        let carelink = parse_commercial_glucose_csv(carelink_csv, None).expect("CareLink");
        assert_eq!(carelink.monitor, GlucoseMonitor::MedtronicCareLink);
        assert_eq!(carelink.readings.len(), 3);
        assert_eq!(carelink.readings[0].timestamp, "2026/08/26 08:00:00");
        assert_eq!(carelink.readings[2].method, MeasurementMethod::CapillaryBgm);

        // 6) Senseonics Eversense
        let eversense_csv = "\
# Senseonics Eversense 365 Export
Timestamp,Sensor Glucose (mg/dL),Rate of Change (mg/dL/min),Notes
2026-08-26T09:00:00Z,102,0.2,Morning check
2026-08-26T09:05:00Z,110,1.1,Post-coffee
";
        let eversense = parse_commercial_glucose_csv(eversense_csv, None).expect("Eversense");
        assert_eq!(eversense.monitor, GlucoseMonitor::SenseonicsEversense);
        assert_eq!(eversense.readings.len(), 2);

        // 7) Tandem Source (t:slim X2 EventDateTime + Readings (CGM))
        let tandem_csv = "\
DeviceType,EventDateTime,EventType,Readings (CGM)
t:slim X2,2026-08-26T10:00:00,EGV,112
t:slim X2,2026-08-26T10:05:00,Bolus,
t:slim X2,2026-08-26T10:10:00,EGV,126
";
        let tandem = parse_commercial_glucose_csv(tandem_csv, None).expect("Tandem Source");
        assert_eq!(tandem.monitor, GlucoseMonitor::TandemSource);
        assert_eq!(tandem.readings.len(), 2);

        // 8) Omnipod 5 / Glooko / Tidepool (type = cbg/smbg/bolus + mmol/L units column)
        let tidepool_csv = "\
time,type,value,units,deviceId,notes
2026-08-26T11:00:00Z,cbg,5.5,mmol/L,Omnipod-5,Steady
2026-08-26T11:05:00Z,bolus,2.5,U,Omnipod-5,Meal bolus
2026-08-26T11:10:00Z,smbg,6.0,mmol/L,Omnipod-5,Meter check
";
        let tidepool = parse_commercial_glucose_csv(tidepool_csv, None).expect("Tidepool");
        assert_eq!(tidepool.monitor, GlucoseMonitor::GlookoTidepool);
        assert_eq!(tidepool.source_unit, GlucoseUnit::MmolL);
        assert_eq!(tidepool.readings.len(), 2);
        assert!((tidepool.readings[1].glucose_mg_dl - 108.1).abs() <= 0.1);

        // 9) Fingerstick BGM (Accu-Chek / Contour with Control Solution filtering)
        let bgm_csv = "\
Date,Time,Blood Glucose (mg/dL),Meal Marker,Control Solution
08/26/2026,07:30 AM,94,Pre-meal,No
08/26/2026,07:31 AM,120,,Yes
08/26/2026,01:30 PM,148,Post-meal,No
";
        let bgm = parse_commercial_glucose_csv(bgm_csv, None).expect("Fingerstick BGM");
        assert_eq!(bgm.monitor, GlucoseMonitor::FingerstickBgm);
        assert_eq!(bgm.method, MeasurementMethod::CapillaryBgm);
        assert_eq!(bgm.readings.len(), 2);
        assert_eq!(
            parse_timestamp_parts(&carelink.readings[2].timestamp)
                .map(|t| t.iso_date)
                .as_deref(),
            Some("2026-08-26")
        );
        assert_eq!(
            parse_timestamp_parts(&bgm.readings[1].timestamp)
                .map(|t| t.iso_date)
                .as_deref(),
            Some("2026-08-26")
        );
        assert_eq!(
            parse_timestamp_parts("Aug 26, 2026 12:45")
                .map(|t| t.iso_date)
                .as_deref(),
            Some("2026-08-26")
        );

        // 10) Generic ISO-8601 CSV
        let generic_csv = "\
timestamp,glucose_mg_dl,note
2026-08-26T08:00:00Z,96,Morning
2026-08-26T08:05:00Z,101,Steady
";
        let generic = parse_commercial_glucose_csv(generic_csv, None).expect("Generic CSV");
        assert_eq!(generic.monitor, GlucoseMonitor::GenericCsv);
        assert_eq!(generic.readings.len(), 2);

        for monitor in [
            GlucoseMonitor::AbbottLingo,
            GlucoseMonitor::AbbottFreeStyleLibre,
            GlucoseMonitor::DexcomClarity,
            GlucoseMonitor::DexcomStelo,
            GlucoseMonitor::MedtronicCareLink,
            GlucoseMonitor::SenseonicsEversense,
            GlucoseMonitor::TandemSource,
            GlucoseMonitor::GlookoTidepool,
            GlucoseMonitor::FingerstickBgm,
            GlucoseMonitor::GenericCsv,
        ] {
            let encoded = serde_json::to_string(&monitor).unwrap();
            assert_eq!(encoded, format!("\"{}\"", monitor.slug()));
            assert_eq!(GlucoseMonitor::from_slug(monitor.slug()), Some(monitor));
        }
    }

    #[test]
    fn computes_ada_5_band_tir_gmi_gri_mage_wear_and_24h_agp_accurately() {
        let csv = "\
timestamp,glucose_mg_dl
2026-08-26T02:00:00Z,50
2026-08-26T02:05:00Z,62
2026-08-26T08:00:00Z,95
2026-08-26T08:05:00Z,110
2026-08-26T08:10:00Z,105
2026-08-26T12:00:00Z,165
2026-08-26T12:05:00Z,210
2026-08-26T12:10:00Z,275
2026-08-26T18:00:00Z,115
2026-08-26T18:05:00Z,98
";
        let import =
            parse_commercial_glucose_csv(csv, Some(GlucoseMonitor::DexcomClarity)).unwrap();
        let summary = CgmClinicalSummary::from_import(&import).unwrap();

        assert_eq!(summary.count, 10);
        assert_eq!(summary.tbr_very_low_lt_54_percent, 10.0);
        assert_eq!(summary.tbr_low_54_69_percent, 10.0);
        assert_eq!(summary.tir_70_180_percent, 60.0);
        assert_eq!(summary.titr_70_140_percent, 50.0);
        assert_eq!(summary.tar_high_181_250_percent, 10.0);
        assert_eq!(summary.tar_very_high_gt_250_percent, 10.0);

        let band_sum = summary.tbr_very_low_lt_54_percent
            + summary.tbr_low_54_69_percent
            + summary.tir_70_180_percent
            + summary.tar_high_181_250_percent
            + summary.tar_very_high_gt_250_percent;
        assert!((band_sum - 100.0).abs() < 1e-6);

        // Mean = 128.5 -> GMI = 3.31 + 0.02392 * 128.5 = 6.3837 -> 6.4%
        assert_eq!(summary.mean_mg_dl, 128.5);
        assert_eq!(summary.gmi_percent, 6.4);

        // GRI = 3.0*10 + 2.4*10 + 1.6*10 + 0.8*10 = 30 + 24 + 16 + 8 = 78.0 -> Zone D
        assert_eq!(summary.gri, 78.0);
        assert_eq!(summary.gri_zone, GriZone::ZoneD);

        // Major excursion 50 -> 275 -> 98 cleanly prunes the 110 -> 105 ripple and exceeds 1 SD
        assert!(summary.mage_mg_dl > summary.std_dev_mg_dl);

        // 4 distinct hours populated (02, 08, 12, 18), ordered and non-decreasing p5..=p95
        let hours: Vec<u8> = summary.agp_hourly.iter().map(|b| b.hour).collect();
        assert_eq!(hours, vec![2, 8, 12, 18]);
        for bin in &summary.agp_hourly {
            assert!(
                bin.p5 <= bin.p25 && bin.p25 <= bin.p50 && bin.p50 <= bin.p75 && bin.p75 <= bin.p95
            );
        }

        let fields = summary.to_cited_fields();
        assert!(
            fields
                .iter()
                .any(|f| f.key == "cgm_tir_70_180" && f.status.as_deref() == Some("Low"))
        );
        assert!(
            fields
                .iter()
                .any(|f| f.key == "cgm_gri" && f.status.as_deref() == Some("High"))
        );
    }

    #[test]
    fn build_commercial_cgm_record_preserves_sensor_boundary_readable_parts_parity() {
        let dexcom_csv = "\
Index,Timestamp (YYYY-MM-DDThh:mm:ss),Event Type,Source Device ID,Glucose Value (mg/dL),Glucose Rate of Change (mg/dL/min)
1,2026-08-27T04:00:00,EGV,DX-G7,Low,-1.5
2,2026-08-27T08:00:00,EGV,DX-G7,104,0.2
3,2026-08-27T14:00:00,EGV,DX-G7,High,2.4
";
        let import = parse_commercial_glucose_csv(dexcom_csv, None).expect("parse dexcom");
        let (rec, _summary) = build_commercial_cgm_record(
            "unit-wellness-asrm-1",
            "rec-test-clipped".into(),
            "Clipped Dexcom Trace",
            &import,
        )
        .expect("build record");
        assert!(rec.body.contains("Sensor boundary events:"));
        for line in rec.body.lines().map(str::trim).filter(|l| !l.is_empty()) {
            assert!(
                rec.readable_parts.iter().any(|p| p.text.contains(line)),
                "every body line including sensor boundary events must appear verbatim in readable_parts: {line:?}"
            );
        }
    }

    #[test]
    fn rejects_empty_unrecognized_or_out_of_bounds_commercial_csv() {
        assert_eq!(
            parse_commercial_glucose_csv("  \n# empty\n", None),
            Err(CgmParseError::EmptyInput)
        );
        assert!(matches!(
            parse_commercial_glucose_csv("foo,bar\nabc,def\n", None),
            Err(CgmParseError::InvalidFloat { .. } | CgmParseError::UnrecognizedFormat { .. })
        ));
        assert!(matches!(
            parse_commercial_glucose_csv(
                "timestamp,glucose_mmol_l\n2026-08-26T08:00:00Z,45.0\n",
                None
            ),
            Err(CgmParseError::OutOfRange {
                field: "glucose_mg_dl",
                ..
            })
        ));
    }
}
