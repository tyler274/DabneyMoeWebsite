//! Pure-Rust Abbott Lingo continuous glucose monitoring (CGM) and mental health check-in models,
//! CSV ingestion, summary statistics, and structured SQLite chart persistence.

use crate::driver::DbError;
use crate::schema::SourceRecords;
use crate::store::{DEMO_DATE, DemoSourceRecord, DemoStore, next_seq};
use physician_wire::{CitedFieldWire, CitedTextPartWire};
use serde::{Deserialize, Serialize};
use serde_json::json;

const MIN_SENSOR_GLUCOSE_MG_DL: f64 = 20.0;
const MAX_SENSOR_GLUCOSE_MG_DL: f64 = 500.0;
const TIGHT_LOW_MG_DL: f64 = 70.0;
const TIGHT_HIGH_MG_DL: f64 = 140.0;
const CLINICAL_HIGH_MG_DL: f64 = 180.0;
pub(crate) const TIGHT_TIR_TARGET_PERCENT: f64 = 85.0;
pub(crate) const CLINICAL_TIR_TARGET_PERCENT: f64 = 70.0;

pub(crate) fn tight_tir_status(percent: f64) -> &'static str {
    if percent < TIGHT_TIR_TARGET_PERCENT {
        "Low"
    } else {
        "Normal"
    }
}

pub(crate) fn clinical_tir_status(percent: f64) -> &'static str {
    if percent < CLINICAL_TIR_TARGET_PERCENT {
        "Low"
    } else {
        "Normal"
    }
}

/// One timestamped glucose biosensor reading from an Abbott Lingo CGM stream or export.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct LingoReading {
    /// ISO 8601 or `YYYY-MM-DD HH:MM` timestamp of the sensor reading.
    pub timestamp: String,
    /// Interstitial glucose concentration in `mg/dL`.
    pub glucose_mg_dl: f64,
    /// Optional Lingo Count points accrued for this interval.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lingo_count: Option<f64>,
    /// Optional meal, exercise, or symptom context note logged alongside the reading.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl LingoReading {
    /// Validates timestamp presence and physiological biosensor bounds (`20.0..=500.0 mg/dL`).
    pub fn validate(&self, line: usize) -> Result<(), LingoParseError> {
        if self.timestamp.trim().is_empty() {
            return Err(LingoParseError::InvalidRow {
                line,
                detail: "timestamp cannot be empty".into(),
            });
        }
        if !self.glucose_mg_dl.is_finite()
            || !(MIN_SENSOR_GLUCOSE_MG_DL..=MAX_SENSOR_GLUCOSE_MG_DL).contains(&self.glucose_mg_dl)
        {
            return Err(LingoParseError::OutOfRange {
                line,
                field: "glucose_mg_dl",
                detail: format!(
                    "expected finite value in {MIN_SENSOR_GLUCOSE_MG_DL}..={MAX_SENSOR_GLUCOSE_MG_DL}, got {}",
                    self.glucose_mg_dl
                ),
            });
        }
        if let Some(lc) = self.lingo_count
            && (!lc.is_finite() || lc < 0.0)
        {
            return Err(LingoParseError::OutOfRange {
                line,
                field: "lingo_count",
                detail: format!("expected non-negative finite number, got {lc}"),
            });
        }
        Ok(())
    }
}

/// Errors returned while parsing or validating Abbott Lingo CGM readings.
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum LingoParseError {
    /// No valid glucose readings were supplied.
    #[error("Lingo CGM input contains no valid glucose readings")]
    EmptyInput,
    /// A CSV row or reading lacked required columns.
    #[error("invalid Lingo CGM row at line {line}: {detail}")]
    InvalidRow {
        /// 1-based input line or reading index.
        line: usize,
        /// Explanation of what failed to parse.
        detail: String,
    },
    /// A numeric column could not be parsed as a floating-point number.
    #[error("invalid Lingo CGM `{field}` value `{raw}` at line {line}")]
    InvalidFloat {
        /// 1-based input line number.
        line: usize,
        /// Column field name (`glucose_mg_dl` or `lingo_count`).
        field: &'static str,
        /// Raw text that failed to parse.
        raw: String,
        /// Underlying float parse error.
        #[source]
        source: std::num::ParseFloatError,
    },
    /// A numeric measurement fell outside supported biosensor bounds.
    #[error("Lingo CGM `{field}` out of range at line {line}: {detail}")]
    OutOfRange {
        /// 1-based input line or reading index.
        line: usize,
        /// Field name that failed range validation.
        field: &'static str,
        /// Range diagnostic detail.
        detail: String,
    },
}

/// Glycemic summary metrics computed over a window of [`LingoReading`]s.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct LingoCgmSummary {
    /// Number of valid readings included in the window.
    pub count: usize,
    /// Arithmetic mean glucose in `mg/dL`.
    pub mean_mg_dl: f64,
    /// Minimum glucose in `mg/dL`.
    pub min_mg_dl: f64,
    /// Maximum (peak) glucose in `mg/dL`.
    pub max_mg_dl: f64,
    /// Population standard deviation in `mg/dL`.
    pub std_dev_mg_dl: f64,
    /// Coefficient of variation (`100 * SD / mean`), measuring glycemic variability.
    pub cv_percent: f64,
    /// Percentage of readings within metabolic tight range (`70–140 mg/dL`).
    pub time_in_range_70_140_percent: f64,
    /// Percentage of readings within standard clinical range (`70–180 mg/dL`).
    pub time_in_range_70_180_percent: f64,
    /// Count of hyperglycemic / post-prandial excursion readings `> 140 mg/dL`.
    pub spike_count_over_140: usize,
    /// Count of low readings `< 70 mg/dL`.
    pub low_count_under_70: usize,
    /// Total Lingo Count exposure points across the window.
    pub total_lingo_count: f64,
    /// First reading timestamp in the window.
    pub start_timestamp: String,
    /// Last reading timestamp in the window.
    pub end_timestamp: String,
}

impl LingoCgmSummary {
    /// Computes summary metrics from a non-empty slice of validated [`LingoReading`]s.
    pub fn from_readings(readings: &[LingoReading]) -> Result<Self, LingoParseError> {
        if readings.is_empty() {
            return Err(LingoParseError::EmptyInput);
        }
        for (idx, reading) in readings.iter().enumerate() {
            reading.validate(idx + 1)?;
        }

        let n = readings.len();
        let n_f = n as f64;
        let mut sum = 0.0_f64;
        let mut min_v = f64::INFINITY;
        let mut max_v = f64::NEG_INFINITY;
        let mut in_70_140 = 0usize;
        let mut in_70_180 = 0usize;
        let mut spikes = 0usize;
        let mut lows = 0usize;
        let mut total_lc = 0.0_f64;

        for r in readings {
            let g = r.glucose_mg_dl;
            sum += g;
            if g < min_v {
                min_v = g;
            }
            if g > max_v {
                max_v = g;
            }
            if (TIGHT_LOW_MG_DL..=TIGHT_HIGH_MG_DL).contains(&g) {
                in_70_140 += 1;
            }
            if (TIGHT_LOW_MG_DL..=CLINICAL_HIGH_MG_DL).contains(&g) {
                in_70_180 += 1;
            }
            if g > TIGHT_HIGH_MG_DL {
                spikes += 1;
            }
            if g < TIGHT_LOW_MG_DL {
                lows += 1;
            }
            total_lc += r.lingo_count.unwrap_or_else(|| {
                if g > TIGHT_HIGH_MG_DL {
                    ((g - TIGHT_HIGH_MG_DL) * 0.25).max(0.5)
                } else {
                    0.0
                }
            });
        }

        let mean = sum / n_f;
        let variance = readings
            .iter()
            .map(|r| {
                let diff = r.glucose_mg_dl - mean;
                diff * diff
            })
            .sum::<f64>()
            / n_f;
        let std_dev = variance.sqrt();
        let cv = if mean > 0.0 {
            (std_dev / mean) * 100.0
        } else {
            0.0
        };

        Ok(Self {
            count: n,
            mean_mg_dl: round1(mean),
            min_mg_dl: round1(min_v),
            max_mg_dl: round1(max_v),
            std_dev_mg_dl: round1(std_dev),
            cv_percent: round1(cv),
            time_in_range_70_140_percent: round1((in_70_140 as f64 / n_f) * 100.0),
            time_in_range_70_180_percent: round1((in_70_180 as f64 / n_f) * 100.0),
            spike_count_over_140: spikes,
            low_count_under_70: lows,
            total_lingo_count: round1(total_lc),
            start_timestamp: readings[0].timestamp.trim().to_owned(),
            end_timestamp: readings[n - 1].timestamp.trim().to_owned(),
        })
    }

    /// Renders structured [`CitedFieldWire`] rows so `CitedValuesTable` and `SourcePane`
    /// surface key CGM metrics with units, targets, and abnormality status flags.
    pub fn to_cited_fields(&self) -> Vec<CitedFieldWire> {
        let mean_status = if self.mean_mg_dl > 115.0 {
            "Elevated"
        } else if self.mean_mg_dl < TIGHT_LOW_MG_DL {
            "Low"
        } else {
            "Normal"
        };
        let peak_status = if self.max_mg_dl > CLINICAL_HIGH_MG_DL {
            "High"
        } else if self.max_mg_dl > TIGHT_HIGH_MG_DL {
            "Elevated"
        } else {
            "Normal"
        };
        let cv_status = if self.cv_percent >= 36.0 {
            "High"
        } else if self.cv_percent >= 20.0 {
            "Elevated"
        } else {
            "Normal"
        };
        let tir_tight_status = tight_tir_status(self.time_in_range_70_140_percent);
        let tir_clinical_status = clinical_tir_status(self.time_in_range_70_180_percent);
        vec![
            CitedFieldWire {
                key: "lingo_mean_glucose".into(),
                name: "Mean Glucose (Lingo CGM)".into(),
                value: format!("{:.1}", self.mean_mg_dl),
                unit: Some("mg/dL".into()),
                range: Some("70–115 mg/dL".into()),
                status: Some(mean_status.into()),
            },
            CitedFieldWire {
                key: "lingo_peak_glucose".into(),
                name: "Peak Glucose".into(),
                value: format!("{:.1}", self.max_mg_dl),
                unit: Some("mg/dL".into()),
                range: Some("<= 140 mg/dL".into()),
                status: Some(peak_status.into()),
            },
            CitedFieldWire {
                key: "lingo_nadir_glucose".into(),
                name: "Nadir Glucose".into(),
                value: format!("{:.1}", self.min_mg_dl),
                unit: Some("mg/dL".into()),
                range: Some(">= 70 mg/dL".into()),
                status: Some(if self.min_mg_dl < TIGHT_LOW_MG_DL {
                    "Low".into()
                } else {
                    "Normal".into()
                }),
            },
            CitedFieldWire {
                key: "lingo_cv_percent".into(),
                name: "Glycemic Variability (CV)".into(),
                value: format!("{:.1}", self.cv_percent),
                unit: Some("%".into()),
                range: Some("< 20.0%".into()),
                status: Some(cv_status.into()),
            },
            CitedFieldWire {
                key: "lingo_tir_70_140".into(),
                name: "Time in Tight Range (70–140)".into(),
                value: format!("{:.1}", self.time_in_range_70_140_percent),
                unit: Some("%".into()),
                range: Some(">= 85.0%".into()),
                status: Some(tir_tight_status.into()),
            },
            CitedFieldWire {
                key: "lingo_tir_70_180".into(),
                name: "Time in Range (70–180)".into(),
                value: format!("{:.1}", self.time_in_range_70_180_percent),
                unit: Some("%".into()),
                range: Some(">= 70.0%".into()),
                status: Some(tir_clinical_status.into()),
            },
            CitedFieldWire {
                key: "lingo_spikes_over_140".into(),
                name: "Excursions > 140 mg/dL".into(),
                value: self.spike_count_over_140.to_string(),
                unit: Some("events".into()),
                range: None,
                status: Some(if self.spike_count_over_140 > 0 {
                    "Elevated".into()
                } else {
                    "Normal".into()
                }),
            },
            CitedFieldWire {
                key: "lingo_total_count".into(),
                name: "Total Lingo Count".into(),
                value: format!("{:.1}", self.total_lingo_count),
                unit: Some("pts".into()),
                range: Some("<= 45 pts/day".into()),
                status: None,
            },
        ]
    }
}

/// Parses Abbott Lingo CGM CSV text (`timestamp,glucose_mg_dl[,lingo_count[,note]]`),
/// skipping blank and `#`-comment lines and supporting optional column headers.
pub fn parse_lingo_csv(csv_text: &str) -> Result<Vec<LingoReading>, LingoParseError> {
    let mut readings = Vec::new();
    let mut col_map: Option<(usize, usize, Option<usize>, Option<usize>)> = None;

    for (idx, raw_line) in csv_text.lines().enumerate() {
        let line_no = idx + 1;
        let trimmed = raw_line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let cols = split_csv_row(trimmed, line_no)?;
        if cols.len() < 2 {
            return Err(LingoParseError::InvalidRow {
                line: line_no,
                detail: "expected at least timestamp and glucose_mg_dl columns".into(),
            });
        }

        if col_map.is_none() && readings.is_empty() && looks_like_header(&cols) {
            col_map = Some(resolve_header_columns(&cols, line_no)?);
            continue;
        }

        let (ts_idx, gl_idx, lc_idx, note_idx) = col_map.unwrap_or((0, 1, Some(2), Some(3)));
        let timestamp = cols.get(ts_idx).map(|s| s.trim()).unwrap_or("").to_owned();
        let glucose_str = cols.get(gl_idx).map(|s| s.trim()).unwrap_or("");
        let glucose_mg_dl =
            glucose_str
                .parse::<f64>()
                .map_err(|source| LingoParseError::InvalidFloat {
                    line: line_no,
                    field: "glucose_mg_dl",
                    raw: glucose_str.to_owned(),
                    source,
                })?;

        let lingo_count = match lc_idx.and_then(|i| cols.get(i)).map(|s| s.trim()) {
            Some(s) if !s.is_empty() => {
                Some(
                    s.parse::<f64>()
                        .map_err(|source| LingoParseError::InvalidFloat {
                            line: line_no,
                            field: "lingo_count",
                            raw: s.to_owned(),
                            source,
                        })?,
                )
            }
            _ => None,
        };
        let note = note_idx
            .and_then(|i| cols.get(i))
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(str::to_owned);

        let reading = LingoReading {
            timestamp,
            glucose_mg_dl,
            lingo_count,
            note,
        };
        reading.validate(line_no)?;
        readings.push(reading);
    }

    if readings.is_empty() {
        return Err(LingoParseError::EmptyInput);
    }
    Ok(readings)
}

fn split_csv_row(line: &str, line_no: usize) -> Result<Vec<String>, LingoParseError> {
    crate::cgm::split_csv_row_delim(line, ',').map_err(|detail| LingoParseError::InvalidRow {
        line: line_no,
        detail: detail.into(),
    })
}

fn is_timestamp_header(raw: &str) -> bool {
    let h = raw.trim().to_ascii_lowercase();
    !h.chars().any(|c| c.is_ascii_digit()) && (h.contains("time") || h.contains("date"))
}

fn is_glucose_header(raw: &str) -> bool {
    let h = raw.trim().to_ascii_lowercase();
    h.contains("glucose") || h == "mg_dl" || h == "mg/dl" || h == "value"
}

fn looks_like_header(cols: &[String]) -> bool {
    let has_ts = cols.iter().any(|c| is_timestamp_header(c));
    let has_gl = cols.iter().any(|c| is_glucose_header(c));
    if has_ts && has_gl {
        return cols.iter().enumerate().any(|(i, c)| {
            is_timestamp_header(c)
                && cols
                    .iter()
                    .enumerate()
                    .any(|(j, g)| j != i && is_glucose_header(g))
        });
    }
    if has_ts || has_gl {
        return cols
            .iter()
            .take(2)
            .all(|c| c.trim().parse::<f64>().is_err());
    }
    false
}

fn resolve_header_columns(
    cols: &[String],
    line: usize,
) -> Result<(usize, usize, Option<usize>, Option<usize>), LingoParseError> {
    let mut ts_idx = None;
    let mut gl_idx = None;
    let mut lc_idx = None;
    let mut note_idx = None;
    for (i, raw) in cols.iter().enumerate() {
        let h = raw.trim().to_ascii_lowercase();
        if ts_idx.is_none() && is_timestamp_header(raw) {
            ts_idx = Some(i);
        } else if gl_idx.is_none() && is_glucose_header(raw) {
            gl_idx = Some(i);
        } else if lc_idx.is_none() && h.contains("lingo") {
            lc_idx = Some(i);
        } else if note_idx.is_none() && (h.contains("note") || h.contains("context")) {
            note_idx = Some(i);
        }
    }
    let (Some(ts), Some(gl)) = (ts_idx, gl_idx) else {
        return Err(LingoParseError::InvalidRow {
            line,
            detail: "CSV header must include timestamp and glucose columns".into(),
        });
    };
    Ok((ts, gl, lc_idx, note_idx))
}

/// Standard clinical PHQ-9 depression severity band (`0..=27`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phq9Severity {
    /// Score `0..=4`.
    Minimal,
    /// Score `5..=9`.
    Mild,
    /// Score `10..=14`.
    Moderate,
    /// Score `15..=19`.
    ModeratelySevere,
    /// Score `20..=27`.
    Severe,
}

impl Phq9Severity {
    /// Maps a validated PHQ-9 score (`0..=27`) to its standard clinical severity band.
    pub fn from_score(score: u8) -> Result<Self, MentalHealthValidationError> {
        match score {
            0..=4 => Ok(Self::Minimal),
            5..=9 => Ok(Self::Mild),
            10..=14 => Ok(Self::Moderate),
            15..=19 => Ok(Self::ModeratelySevere),
            20..=27 => Ok(Self::Severe),
            other => Err(MentalHealthValidationError::Phq9OutOfRange(other)),
        }
    }

    /// Physician-facing severity label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Minimal => "Minimal",
            Self::Mild => "Mild",
            Self::Moderate => "Moderate",
            Self::ModeratelySevere => "Moderately severe",
            Self::Severe => "Severe",
        }
    }

    fn field_status(self) -> &'static str {
        match self {
            Self::Minimal => "Normal",
            Self::Mild => "Mild",
            Self::Moderate => "Moderate",
            Self::ModeratelySevere | Self::Severe => "Elevated",
        }
    }
}

/// Standard clinical GAD-7 anxiety severity band (`0..=21`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Gad7Severity {
    /// Score `0..=4`.
    Minimal,
    /// Score `5..=9`.
    Mild,
    /// Score `10..=14`.
    Moderate,
    /// Score `15..=21`.
    Severe,
}

impl Gad7Severity {
    /// Maps a validated GAD-7 score (`0..=21`) to its standard clinical severity band.
    pub fn from_score(score: u8) -> Result<Self, MentalHealthValidationError> {
        match score {
            0..=4 => Ok(Self::Minimal),
            5..=9 => Ok(Self::Mild),
            10..=14 => Ok(Self::Moderate),
            15..=21 => Ok(Self::Severe),
            other => Err(MentalHealthValidationError::Gad7OutOfRange(other)),
        }
    }

    /// Physician-facing severity label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Minimal => "Minimal",
            Self::Mild => "Mild",
            Self::Moderate => "Moderate",
            Self::Severe => "Severe",
        }
    }

    fn field_status(self) -> &'static str {
        match self {
            Self::Minimal => "Normal",
            Self::Mild => "Mild",
            Self::Moderate => "Moderate",
            Self::Severe => "Elevated",
        }
    }
}

/// Validation errors for [`MentalHealthCheckIn`].
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum MentalHealthValidationError {
    /// `recorded_at` was blank.
    #[error("mental health check-in `recorded_at` cannot be empty")]
    MissingRecordedAt,
    /// `mood_1_to_10` was outside `1..=10`.
    #[error("`mood_1_to_10` must be in 1..=10, got {0}")]
    MoodOutOfRange(u8),
    /// `anxiety_1_to_10` was outside `1..=10`.
    #[error("`anxiety_1_to_10` must be in 1..=10, got {0}")]
    AnxietyOutOfRange(u8),
    /// `energy_1_to_10` was outside `1..=10`.
    #[error("`energy_1_to_10` must be in 1..=10, got {0}")]
    EnergyOutOfRange(u8),
    /// `sleep_hours` was non-finite or outside `0.0..=24.0`.
    #[error("`sleep_hours` must be finite in 0.0..=24.0, got {0}")]
    SleepHoursOutOfRange(String),
    /// `phq9_score` was outside `0..=27`.
    #[error("`phq9_score` must be in 0..=27, got {0}")]
    Phq9OutOfRange(u8),
    /// `gad7_score` was outside `0..=21`.
    #[error("`gad7_score` must be in 0..=21, got {0}")]
    Gad7OutOfRange(u8),
}

/// Structured patient mental health check-in with optional PHQ-9 / GAD-7 screening scores.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct MentalHealthCheckIn {
    /// ISO date or timestamp when the check-in was recorded.
    pub recorded_at: String,
    /// Self-reported mood rating (`1..=10`, higher is brighter mood).
    pub mood_1_to_10: u8,
    /// Self-reported anxiety intensity (`1..=10`, higher is greater anxiety).
    pub anxiety_1_to_10: u8,
    /// Self-reported daytime energy rating (`1..=10`, higher is stronger energy).
    pub energy_1_to_10: u8,
    /// Prior-night sleep duration in hours (`0.0..=24.0`).
    pub sleep_hours: f32,
    /// Optional PHQ-9 depression total score (`0..=27`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phq9_score: Option<u8>,
    /// Optional GAD-7 generalized anxiety total score (`0..=21`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gad7_score: Option<u8>,
    /// Free-text symptom or context note from the check-in.
    #[serde(default)]
    pub note: String,
}

impl MentalHealthCheckIn {
    /// Validates all rating scales, sleep hours, and optional PHQ-9 / GAD-7 totals.
    pub fn validate(&self) -> Result<(), MentalHealthValidationError> {
        if self.recorded_at.trim().is_empty() {
            return Err(MentalHealthValidationError::MissingRecordedAt);
        }
        if !(1..=10).contains(&self.mood_1_to_10) {
            return Err(MentalHealthValidationError::MoodOutOfRange(
                self.mood_1_to_10,
            ));
        }
        if !(1..=10).contains(&self.anxiety_1_to_10) {
            return Err(MentalHealthValidationError::AnxietyOutOfRange(
                self.anxiety_1_to_10,
            ));
        }
        if !(1..=10).contains(&self.energy_1_to_10) {
            return Err(MentalHealthValidationError::EnergyOutOfRange(
                self.energy_1_to_10,
            ));
        }
        if !self.sleep_hours.is_finite() || !(0.0..=24.0).contains(&self.sleep_hours) {
            return Err(MentalHealthValidationError::SleepHoursOutOfRange(format!(
                "{}",
                self.sleep_hours
            )));
        }
        if let Some(phq9) = self.phq9_score {
            let _ = Phq9Severity::from_score(phq9)?;
        }
        if let Some(gad7) = self.gad7_score {
            let _ = Gad7Severity::from_score(gad7)?;
        }
        Ok(())
    }

    /// Builds structured [`CitedFieldWire`] measurements for `CitedValuesTable` and `SourcePane`.
    pub fn to_cited_fields(&self) -> Result<Vec<CitedFieldWire>, MentalHealthValidationError> {
        self.validate()?;
        let mut fields = Vec::new();
        if let Some(phq9) = self.phq9_score {
            let sev = Phq9Severity::from_score(phq9)?;
            fields.push(CitedFieldWire {
                key: "phq9_score".into(),
                name: format!("PHQ-9 ({})", sev.label()),
                value: phq9.to_string(),
                unit: Some("/27".into()),
                range: Some("0–4 Minimal".into()),
                status: Some(sev.field_status().into()),
            });
        }
        if let Some(gad7) = self.gad7_score {
            let sev = Gad7Severity::from_score(gad7)?;
            fields.push(CitedFieldWire {
                key: "gad7_score".into(),
                name: format!("GAD-7 ({})", sev.label()),
                value: gad7.to_string(),
                unit: Some("/21".into()),
                range: Some("0–4 Minimal".into()),
                status: Some(sev.field_status().into()),
            });
        }
        fields.push(CitedFieldWire {
            key: "mood_rating".into(),
            name: "Mood Rating".into(),
            value: self.mood_1_to_10.to_string(),
            unit: Some("/10".into()),
            range: Some("6–10".into()),
            status: Some(if self.mood_1_to_10 <= 3 {
                "Low".into()
            } else {
                "Normal".into()
            }),
        });
        fields.push(CitedFieldWire {
            key: "anxiety_rating".into(),
            name: "Anxiety Rating".into(),
            value: self.anxiety_1_to_10.to_string(),
            unit: Some("/10".into()),
            range: Some("1–4".into()),
            status: Some(if self.anxiety_1_to_10 >= 7 {
                "Elevated".into()
            } else {
                "Normal".into()
            }),
        });
        fields.push(CitedFieldWire {
            key: "energy_rating".into(),
            name: "Energy Rating".into(),
            value: self.energy_1_to_10.to_string(),
            unit: Some("/10".into()),
            range: Some("6–10".into()),
            status: Some(if self.energy_1_to_10 <= 3 {
                "Low".into()
            } else {
                "Normal".into()
            }),
        });
        fields.push(CitedFieldWire {
            key: "sleep_hours".into(),
            name: "Sleep Duration".into(),
            value: format!("{:.1}", self.sleep_hours),
            unit: Some("h".into()),
            range: Some("7.0–9.0 h".into()),
            status: Some(if self.sleep_hours < 6.0 {
                "Low".into()
            } else {
                "Normal".into()
            }),
        });
        Ok(fields)
    }
}

impl DemoStore {
    /// Validates `readings`, computes [`LingoCgmSummary`], and files a structured
    /// Abbott Lingo CGM `SourceRecords` entry with `CitedFieldWire` measurements.
    pub async fn ingest_lingo_cgm(
        &self,
        workspace_id: &str,
        summary_title: &str,
        readings: &[LingoReading],
    ) -> Result<(DemoSourceRecord, LingoCgmSummary), DbError> {
        self.ensure_ready().await?;
        let _ = self.load_patient_name(workspace_id)?;
        let summary = LingoCgmSummary::from_readings(readings)
            .map_err(|err| DbError::InvalidCommand(err.to_string()))?;

        let title = match summary_title.trim() {
            "" => "Abbott Lingo CGM Summary".to_owned(),
            clean => clean.to_owned(),
        };
        let recorded_at = summary
            .end_timestamp
            .get(..10)
            .filter(|d| d.len() == 10)
            .unwrap_or(DEMO_DATE)
            .to_owned();
        let fields = summary.to_cited_fields();

        let mut body = format!(
            "Abbott Lingo CGM biosensor summary ({} to {}, n = {} readings): mean glucose {:.1} mg/dL, range {:.1}–{:.1} mg/dL, SD {:.1} mg/dL, glycemic variability CV {:.1}%, Time in Tight Range (70–140 mg/dL) {:.1}%, Time in Range (70–180 mg/dL) {:.1}%, excursions >140 mg/dL: {}, readings <70 mg/dL: {}, Total Lingo Count {:.1} pts.",
            summary.start_timestamp,
            summary.end_timestamp,
            summary.count,
            summary.mean_mg_dl,
            summary.min_mg_dl,
            summary.max_mg_dl,
            summary.std_dev_mg_dl,
            summary.cv_percent,
            summary.time_in_range_70_140_percent,
            summary.time_in_range_70_180_percent,
            summary.spike_count_over_140,
            summary.low_count_under_70,
            summary.total_lingo_count,
        );
        let noted_events: Vec<String> = readings
            .iter()
            .filter_map(|r| {
                let n = r.note.as_deref()?.trim();
                (!n.is_empty())
                    .then(|| format!("{} ({:.0} mg/dL): {n}", r.timestamp, r.glucose_mg_dl))
            })
            .take(6)
            .collect();
        if !noted_events.is_empty() {
            body.push_str("\n\nLogged context events:\n- ");
            body.push_str(&noted_events.join("\n- "));
        }

        let readable_parts = vec![CitedTextPartWire {
            label: Some("Abbott Lingo CGM Summary".into()),
            text: body.clone(),
        }];
        let raw_content = json!({
            "source": "abbott_lingo_cgm",
            "summary": &summary,
            "sample_readings": readings.iter().take(24).collect::<Vec<_>>(),
        })
        .to_string();
        let rec = DemoSourceRecord {
            id: format!("rec-{workspace_id}-lingo-{}", next_seq()),
            workspace_id: workspace_id.to_owned(),
            group: "results".into(),
            title,
            recorded_at,
            body,
            source_label: "Abbott Lingo CGM".into(),
            readable_parts,
            fields,
            record_status: "final".into(),
            raw_content: Some(raw_content),
        };
        self.insert_structured_source_record(&rec)?;
        Ok((rec, summary))
    }

    /// Validates `checkin` and files a structured mental health check-in `SourceRecords` entry
    /// with PHQ-9, GAD-7, mood, anxiety, energy, and sleep `CitedFieldWire` measurements.
    pub async fn log_mental_health_checkin(
        &self,
        workspace_id: &str,
        checkin: &MentalHealthCheckIn,
    ) -> Result<DemoSourceRecord, DbError> {
        self.ensure_ready().await?;
        let _ = self.load_patient_name(workspace_id)?;
        let fields = checkin
            .to_cited_fields()
            .map_err(|err| DbError::InvalidCommand(err.to_string()))?;

        let recorded_date = checkin
            .recorded_at
            .trim()
            .get(..10)
            .filter(|d| d.len() == 10)
            .unwrap_or_else(|| checkin.recorded_at.trim())
            .to_owned();

        let mut scales = Vec::new();
        if let Some(phq9) = checkin.phq9_score
            && let Ok(sev) = Phq9Severity::from_score(phq9)
        {
            scales.push(format!("PHQ-9 {phq9}/27 ({})", sev.label()));
        }
        if let Some(gad7) = checkin.gad7_score
            && let Ok(sev) = Gad7Severity::from_score(gad7)
        {
            scales.push(format!("GAD-7 {gad7}/21 ({})", sev.label()));
        }
        let title = if scales.is_empty() {
            "Mental Health Daily Check-In".to_owned()
        } else {
            format!("Mental Health Check-In — {}", scales.join(", "))
        };
        let scales_clause = if scales.is_empty() {
            String::new()
        } else {
            format!(" Standardized scales: {}.", scales.join("; "))
        };
        let note_clean = checkin.note.trim();
        let note_clause = if note_clean.is_empty() {
            String::new()
        } else {
            format!(" Patient note: {note_clean}")
        };
        let body = format!(
            "Mental health check-in ({}): mood {}/10, anxiety {}/10, energy {}/10, prior-night sleep {:.1} h.{}{}",
            checkin.recorded_at.trim(),
            checkin.mood_1_to_10,
            checkin.anxiety_1_to_10,
            checkin.energy_1_to_10,
            checkin.sleep_hours,
            scales_clause,
            note_clause,
        );
        let readable_parts = vec![CitedTextPartWire {
            label: Some(title.clone()),
            text: body.clone(),
        }];
        let raw_content = json!({
            "source": "duet_mental_health_checkin",
            "checkin": checkin,
        })
        .to_string();
        let rec = DemoSourceRecord {
            id: format!("rec-{workspace_id}-mh-{}", next_seq()),
            workspace_id: workspace_id.to_owned(),
            group: "results".into(),
            title,
            recorded_at: recorded_date,
            body,
            source_label: "Duet Mental Health Check-In".into(),
            readable_parts,
            fields,
            record_status: "final".into(),
            raw_content: Some(raw_content),
        };
        self.insert_structured_source_record(&rec)?;
        Ok(rec)
    }

    pub(crate) fn insert_structured_source_record(
        &self,
        rec: &DemoSourceRecord,
    ) -> Result<(), DbError> {
        let parts_json = serde_json::to_string(&rec.readable_parts)?;
        let fields_json = serde_json::to_string(&rec.fields)?;
        self.driver.insert_row(
            SourceRecords::Table,
            SourceRecords::COLS,
            [
                rec.id.as_str().into(),
                rec.workspace_id.as_str().into(),
                rec.group.as_str().into(),
                rec.title.as_str().into(),
                rec.recorded_at.as_str().into(),
                rec.body.as_str().into(),
                rec.source_label.as_str().into(),
                parts_json.as_str().into(),
                fields_json.as_str().into(),
                rec.record_status.as_str().into(),
                rec.raw_content.as_deref().unwrap_or("").into(),
            ],
        )
    }
}

use crate::cgm::round1;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lingo_csv_and_computes_summary_metrics() {
        let csv = "\
# Abbott Lingo export
timestamp,glucose_mg_dl,lingo_count,note
2026-08-26T08:00:00Z,92,0,Fasting wakeup
2026-08-26T12:45:00Z,168,14,Sweetened iced latte + white-rice bowl
2026-08-26T14:45:00Z,72,0,Afternoon shakiness and anxiety 8/10
2026-08-26T19:30:00Z,118,0,Salmon + greens dinner
";
        let readings = parse_lingo_csv(csv).expect("valid Lingo CSV");
        assert_eq!(readings.len(), 4);
        assert_eq!(readings[1].glucose_mg_dl, 168.0);
        assert_eq!(readings[1].lingo_count, Some(14.0));
        assert_eq!(
            readings[2].note.as_deref(),
            Some("Afternoon shakiness and anxiety 8/10")
        );

        let summary = LingoCgmSummary::from_readings(&readings).expect("summary");
        assert_eq!(summary.count, 4);
        assert_eq!(summary.mean_mg_dl, 112.5);
        assert_eq!(summary.min_mg_dl, 72.0);
        assert_eq!(summary.max_mg_dl, 168.0);
        assert_eq!(summary.spike_count_over_140, 1);
        assert_eq!(summary.low_count_under_70, 0);
        assert_eq!(summary.time_in_range_70_140_percent, 75.0);
        assert_eq!(summary.time_in_range_70_180_percent, 100.0);
        assert_eq!(summary.total_lingo_count, 14.0);
        assert!(summary.cv_percent > 30.0);

        let fields = summary.to_cited_fields();
        assert!(
            fields
                .iter()
                .any(|f| f.key == "lingo_peak_glucose" && f.status.as_deref() == Some("Elevated"))
        );
        assert!(
            fields
                .iter()
                .any(|f| f.key == "lingo_tir_70_140" && f.status.as_deref() == Some("Low"))
        );
        assert!(
            fields
                .iter()
                .any(|f| f.key == "lingo_tir_70_180" && f.status.as_deref() == Some("Normal"))
        );
    }

    #[test]
    fn parses_quoted_commas_leading_device_columns_and_flags_low_clinical_tir() {
        let csv = "\
device,serial_no,recorded_time,glucose_mg_dl,lingo_count,context_note
Abbott Lingo,SN-441,\"Aug 26, 2026 08:00\",62,0,\"Fasting nadir, lightheaded\"
Abbott Lingo,SN-441,\"Aug 26, 2026 12:45\",198,22,\"Oat latte, white-rice bowl, and \"\"extra\"\" syrup\"
Abbott Lingo,SN-441,\"Aug 26, 2026 14:15\",186,18,\"Sustained post-meal excursion, anxious\"
Abbott Lingo,SN-441,\"Aug 26, 2026 19:00\",104,0,\"Salmon, greens, and avocado\"
";
        let readings = parse_lingo_csv(csv).expect("quoted CSV with leading columns");
        assert_eq!(readings.len(), 4);
        assert_eq!(readings[0].timestamp, "Aug 26, 2026 08:00");
        assert_eq!(readings[0].glucose_mg_dl, 62.0);
        assert_eq!(
            readings[1].note.as_deref(),
            Some("Oat latte, white-rice bowl, and \"extra\" syrup")
        );

        let summary = LingoCgmSummary::from_readings(&readings).expect("summary");
        assert_eq!(summary.time_in_range_70_140_percent, 25.0);
        assert_eq!(summary.time_in_range_70_180_percent, 25.0);
        let fields = summary.to_cited_fields();
        assert!(
            fields
                .iter()
                .any(|f| f.key == "lingo_tir_70_140" && f.status.as_deref() == Some("Low")),
            "tight TIR < 85% must carry Low status: {fields:?}"
        );
        assert!(
            fields
                .iter()
                .any(|f| f.key == "lingo_tir_70_180" && f.status.as_deref() == Some("Low")),
            "clinical TIR < 70% must carry Low status: {fields:?}"
        );

        assert!(matches!(
            parse_lingo_csv("timestamp,glucose_mg_dl,note\n2026-08-26T08:00:00Z,95,\"unclosed\n"),
            Err(LingoParseError::InvalidRow { line: 2, .. })
        ));
    }

    #[test]
    fn rejects_empty_or_out_of_bounds_lingo_csv() {
        assert_eq!(
            parse_lingo_csv("   \n# comment only\n"),
            Err(LingoParseError::EmptyInput)
        );
        assert!(matches!(
            parse_lingo_csv("timestamp,glucose_mg_dl\n2026-08-26T08:00:00Z,610\n"),
            Err(LingoParseError::OutOfRange {
                field: "glucose_mg_dl",
                ..
            })
        ));
    }

    #[test]
    fn validates_mental_health_checkin_ranges_and_severity_bands() {
        let valid = MentalHealthCheckIn {
            recorded_at: "2026-08-26T15:00:00Z".into(),
            mood_1_to_10: 6,
            anxiety_1_to_10: 8,
            energy_1_to_10: 5,
            sleep_hours: 5.5,
            phq9_score: Some(8),
            gad7_score: Some(12),
            note: "Afternoon jitteriness after post-lunch glucose drop.".into(),
        };
        let fields = valid.to_cited_fields().expect("valid check-in");
        assert!(
            fields
                .iter()
                .any(|f| f.key == "phq9_score" && f.name == "PHQ-9 (Mild)")
        );
        assert!(fields.iter().any(|f| {
            f.key == "gad7_score"
                && f.name == "GAD-7 (Moderate)"
                && f.status.as_deref() == Some("Moderate")
        }));
        assert!(
            fields
                .iter()
                .any(|f| { f.key == "anxiety_rating" && f.status.as_deref() == Some("Elevated") })
        );

        let bad_mood = MentalHealthCheckIn {
            mood_1_to_10: 0,
            ..valid.clone()
        };
        assert_eq!(
            bad_mood.validate(),
            Err(MentalHealthValidationError::MoodOutOfRange(0))
        );
        let bad_phq = MentalHealthCheckIn {
            phq9_score: Some(28),
            ..valid.clone()
        };
        assert_eq!(
            bad_phq.validate(),
            Err(MentalHealthValidationError::Phq9OutOfRange(28))
        );
        let bad_gad = MentalHealthCheckIn {
            gad7_score: Some(22),
            ..valid
        };
        assert_eq!(
            bad_gad.validate(),
            Err(MentalHealthValidationError::Gad7OutOfRange(22))
        );
    }
}
